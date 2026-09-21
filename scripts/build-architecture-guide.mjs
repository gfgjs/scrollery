#!/usr/bin/env node
/**
 * 静态架构导览生成器。
 *
 * 读取 docs/architecture/ 下 4 份人工维护的架构文档（README/modules/pipelines/coverage），
 * 产出单个自包含的 docs/architecture/index.html：Markdown 渲染、GitHub 风格标题锚点、
 * 表格行 ID 锚点（D01…/P01…）、源码证据链接（GitHub blob 深链）、Mermaid 构建期渲染为
 * 双主题 SVG、图注（标题/范围说明/文字阅读路径）、目录树、前端搜索与图形缩放交互。
 *
 * 设计约束（为什么这么做）：
 * - 产物零运行时外部请求：内容/CSS/JS/SVG 全部内联，file:// 双击可开，静态托管可用；
 * - 所有派生内容（锚点、链接、图注、阅读路径）从文档原文自动派生，不手写文案；
 * - 构建期断言全部通过才 exit 0，防止"生成成功但内容坏掉"的静默产物。
 *
 * 依赖：Node 内置模块 + marked / puppeteer-core / mermaid（均为 devDependencies）。
 * 用法：npm run build:architecture-guide
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { marked } from 'marked';
import puppeteer from 'puppeteer-core';

/* ---------------------------------------------------------------- 常量与配置 */

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DOCS_DIR = 'docs/architecture';
const OUTPUT_PATH = path.join(ROOT, DOCS_DIR, 'index.html');
const MERMAID_DIST = path.join(ROOT, 'node_modules', 'mermaid', 'dist', 'mermaid.min.js');

// 文档基线 commit：所有 GitHub 深链固定指向它（文档结论以此为准，不随 HEAD 漂移）
const BASE_COMMIT = '162732f3';
const REPO_URL = 'https://github.com/gfgjs/scrollery-private';

const DOC_DEFS = [
  { file: 'README.md', id: 'doc-readme', figBase: 'fig-readme' },
  { file: 'modules.md', id: 'doc-modules', figBase: 'fig-modules' },
  { file: 'pipelines.md', id: 'doc-pipelines', figBase: 'fig-pipelines' },
  { file: 'coverage.md', id: 'doc-coverage', figBase: 'fig-coverage' },
];

// 入口卡片短名（规格给定，作为卡片角标；标题/副标题仍取 README 阅读导航表原文）
const CARD_KICKERS = ['总览', '模块与源码入口', '流水线', '覆盖与待确认'];

// 源码证据链接的路径白名单前缀：只有这些前缀下的路径视为"仓库相对路径"。
// 根级已知文件（Cargo.toml 等）不硬编码清单，用 worktree 实存判断。
const PATH_PREFIX_WHITELIST = ['src-tauri/', 'src/', 'crates/', 'scripts/', 'tools/', 'docs/'];
// 文档简写约定：后端模块相对 src-tauri/src/（如 `scanner/mod.rs`）、前端模块相对 src/
// （如 `components/layout/`）、Tauri 配置相对 src-tauri/（如 `capabilities/`）。
// 简写按此顺序解析，首个磁盘命中生效；全部未命中保持纯文本并汇总。
const SHORT_ROOTS = ['src-tauri/src/', 'src/', 'src-tauri/'];
// 裸文件名（如 `state.rs`）仅在 git 跟踪集合中唯一命中时解析；候选排除这些
// vendored/构建/依赖目录，避免误链到第三方同名文件。
const BARENAME_EXCLUDE = ['node_modules/', 'vendor/', 'third-party/', 'dist/', 'target/', 'venv/'];
const isExcludedDir = (p) => BARENAME_EXCLUDE.some((x) => p.startsWith(x));
// 行内代码形如 `路径` / `路径::符号` / `路径::数字`；其余（纯符号等）不处理
const PATH_TEXT_RE = /^([A-Za-z0-9_][\w./-]*?)(::([^\s:]+))?$/;

// 无头浏览器探测顺序：环境变量显式指定 → Windows Chrome/Edge → macOS/Linux 常见路径
const BROWSER_CANDIDATES = [
  process.env.SCROLLERY_GUIDE_BROWSER,
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
  '/usr/bin/microsoft-edge',
].filter(Boolean);

const MERMAID_THEMES = ['default', 'dark'];

/* ---------------------------------------------------------------- 构建状态 */

/** 断言失败清单：非空则 exit 1 */
const failures = [];
/** 降级/警告清单：不影响 exit code，但必须在摘要中如实呈现 */
const warnings = [];
/** 未链接的行内代码（保持纯文本），按原因分组汇总 */
const unlinked = new Map(); // text -> { reason, docs: Set<string> }
/** 已生成的源码链接数 */
let srcLinkCount = 0;
/** md 相对链接解析失败清单 */
const linkFailures = [];

function assert(cond, msg) {
  if (!cond) failures.push(msg);
}
function warn(msg) {
  warnings.push(msg);
}

/* ---------------------------------------------------------------- 小工具 */

function escapeHtml(s) {
  return String(s)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

// 行内代码/单元格提取时用：marked 只会产出这 5 种具名实体
function decodeEntities(s) {
  return String(s)
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&#39;', "'")
    .replaceAll('&amp;', '&');
}

function stripTags(s) {
  return String(s).replace(/<[^>]+>/g, '');
}

function sha256File(p) {
  return crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
}

function git(args) {
  return execFileSync('git', args, { cwd: ROOT, maxBuffer: 64 * 1024 * 1024, encoding: 'utf8' });
}

function formatBytes(n) {
  return n >= 1024 ? (n / 1024).toFixed(1) + ' KB' : n + ' B';
}

/* ---------------------------------------------------------------- GitHub 风格 slug */

class Slugger {
  constructor() {
    // 记录每个 slug 的出现次数，重复时加 -1/-2 后缀（同一构建内保证唯一）
    this.counts = new Map();
  }

  slug(text) {
    // 小写 → 只保留字母/数字/空白/连字符（\p{L}\p{N} 保留 CJK）→ 空格转 -
    let s = String(text).toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu, '').replace(/\s+/g, '-');
    if (!s) s = 'section';
    const n = this.counts.get(s) ?? 0;
    this.counts.set(s, n + 1);
    return n === 0 ? s : `${s}-${n}`;
  }
}

/* ---------------------------------------------------------------- 元数据采集 */

function collectMeta() {
  const headSha = git(['rev-parse', 'HEAD']).trim();
  if (!headSha.startsWith(BASE_COMMIT)) {
    warn(`HEAD（${headSha.slice(0, 8)}）不是文档基线 commit ${BASE_COMMIT}；深链仍固定指向基线，请确认是否需要重新核实文档。`);
  }
  // 文档提交状态徽标：构建时以 git status 为准（docs/ 当前整体未跟踪 → 显示"未提交"）
  const porcelain = git(['status', '--porcelain', DOCS_DIR]);
  // 已跟踪文件集合：源码证据链接必须指向已跟踪路径，未跟踪路径只报告不加链接
  const tracked = new Set(
    git(['ls-files', '-z'])
      .split('\0')
      .filter(Boolean)
  );
  return { headSha, docsCommitted: porcelain.trim().length === 0, tracked };
}

function isTracked(trackedSet, relPath) {
  if (trackedSet.has(relPath)) return true;
  // 目录：任一已跟踪文件位于该目录下即视为已跟踪
  const prefix = relPath.endsWith('/') ? relPath : relPath + '/';
  for (const t of trackedSet) {
    if (t.startsWith(prefix)) return true;
  }
  return false;
}

/* ---------------------------------------------------------------- Markdown 解析 */

/** 从 md 原文提取 mermaid 围栏（保留原始代码用于解析与折叠展示），返回 { code, fenceLine } 列表 */
function extractMermaidBlocks(raw) {
  const lines = raw.split(/\r?\n/);
  const blocks = [];
  for (let i = 0; i < lines.length; i++) {
    if (!/^```mermaid\s*$/.test(lines[i].trim())) continue;
    const codeLines = [];
    let j = i + 1;
    for (; j < lines.length && !/^```\s*$/.test(lines[j].trim()); j++) codeLines.push(lines[j]);
    if (j >= lines.length) throw new Error(`${'mermaid'} 围栏未闭合（第 ${i + 1} 行起）`);
    blocks.push({ code: codeLines.join('\n').trim(), fenceLine: i });
    i = j;
  }
  return blocks;
}

/**
 * 派生图注元数据（全部引用原文，不写新文案）：
 * - 标题 = 所属最近标题文本 + 序号
 * - 范围/关系说明：pipelines.md 取该小节 `**目的**：…` 整行；README 取图前段落第一、二句；其余取图前紧邻段落
 */
function deriveFigureMeta(raw, fenceLine, docFile) {
  const lines = raw.split(/\r?\n/);
  let headingText = '';
  for (let i = fenceLine - 1; i >= 0; i--) {
    const m = /^#{1,4}\s+(.+?)\s*#*\s*$/.exec(lines[i]);
    if (m) {
      headingText = m[1];
      break;
    }
  }

  let descRaw = '';
  if (docFile === 'pipelines.md') {
    // 该小节的 **目的** 行（从最近 h2 标题到围栏之间）
    const start = (() => {
      for (let i = fenceLine - 1; i >= 0; i--) if (/^##\s/.test(lines[i])) return i;
      return 0;
    })();
    for (let i = start; i < fenceLine; i++) {
      if (lines[i].trim().startsWith('**目的**')) {
        descRaw = lines[i].trim();
        break;
      }
    }
    if (!descRaw) throw new Error('pipelines.md 图前未找到 **目的** 行');
  } else {
    // 图前紧邻段落（跳过空行，遇结构行即止）
    const para = [];
    for (let i = fenceLine - 1; i >= 0; i--) {
      const t = lines[i].trim();
      if (!t) {
        if (para.length) break;
        continue;
      }
      if (/^(```|#{1,6}\s|\||>|\s*[-*]\s)/.test(t)) break;
      para.unshift(t);
    }
    descRaw = para.join('');
    if (docFile === 'README.md') {
      // 总览图：取图前段落第一、二句
      const sentences = descRaw.split(/(?<=。)/).filter(Boolean);
      descRaw = sentences.slice(0, 2).join('');
    }
  }
  if (!descRaw) throw new Error(`${docFile} 图前未找到可用段落`);

  return { headingText, descRaw };
}

/** 解析 README 阅读导航表，产出入口卡片（标题=文档列，副标题=内容列，均为原文） */
function parseNavCards(readmeRaw) {
  const lines = readmeRaw.split(/\r?\n/);
  const start = lines.findIndex((l) => /^##\s+阅读导航/.test(l));
  if (start < 0) throw new Error('README.md 未找到"阅读导航"节');
  const cards = [];
  for (let i = start + 1; i < lines.length; i++) {
    const l = lines[i].trim();
    if (/^#{1,6}\s/.test(l)) break;
    if (!l.startsWith('|')) continue;
    const cells = l.split('|').map((c) => c.trim());
    if (cells.length < 4) continue;
    const [c1, c2] = [cells[1], cells[2]];
    if (c1 === '文档' || /^:?-{3,}:?$/.test(c1)) continue; // 表头/分隔行
    const link = /\[([^\]]+)\]\(([^)]+)\)/.exec(c1);
    if (!link) continue;
    const doc = DOC_DEFS.find((d) => d.file === link[2]);
    if (!doc) {
      linkFailures.push(`阅读导航表链接指向未知文档: ${link[2]}`);
      continue;
    }
    cards.push({ docId: doc.id, title: c1.replaceAll(/\[([^\]]+)\]\([^)]+\)/g, '$1'), subtitle: c2 });
  }
  return cards;
}

/* ---------------------------------------------------------------- Markdown 渲染 */

let mermaidSlotCounter = 0;

function setupMarked() {
  // mermaid 围栏替换为占位符，待构建期渲染出 SVG 后回填 figure；
  // 非 mermaid 围栏按转义输出——本文档集不应出现，出现会被"多余 <pre>"断言拦下。
  marked.use({
    renderer: {
      code(token) {
        if ((token.lang || '').trim() === 'mermaid') {
          return `<div class="mermaid-slot" data-slot="${mermaidSlotCounter++}"></div>`;
        }
        return `<pre><code>${escapeHtml(token.text ?? '')}</code></pre>`;
      },
    },
  });
}

/** 渲染单份文档并做后处理（锚点/行 ID/相对链接/源码证据链接），返回处理后的 HTML 与统计 */
function renderAndPostProcess(doc, raw, slugger, assignedIds, trackedSet) {
  // slugger: { heading: Slugger, rowCounts: Map }，计数器跨文档共享
  let html = marked.parse(raw);

  // 渲染安全断言：marked 默认透传原始 HTML，若文档被写入脚本/iframe 必须构建失败
  assert(!/<script/i.test(html), `${doc.file} 渲染结果含 <script`);
  assert(!/<iframe/i.test(html), `${doc.file} 渲染结果含 <iframe`);
  assert(!html.includes('<pre>'), `${doc.file} 出现意外的代码围栏（<pre>）——当前文档集只应有 mermaid 图`);

  const stats = { headings: 0, rows: 0, mdLinks: 0 };

  // 1) 标题锚点：h1-h4 加 id 与悬停锚链接（点击写 hash 由浏览器原生完成）
  html = html.replace(/<h([1-4])>([\s\S]*?)<\/h\1>/g, (_m, lvl, inner) => {
    const text = decodeEntities(stripTags(inner)).trim();
    const id = slugger.heading.slug(text);
    assignedIds.add(id);
    stats.headings++;
    return `<h${lvl} id="${id}">${inner}<a class="h-anchor" href="#${id}" aria-label="跳转到此节">#</a></h${lvl}>`;
  });

  // 2) 表格行 ID：首列（或任一单元格）精确匹配 D\d{2}/P\d{2} 的行加小写 id，重复加后缀。
  //    计数器跨文档共享：README 功能地图 D 行、pipelines 索引 P 行先渲染，因此无后缀 id 落在索引/地图行上。
  html = html.replace(/<tr>([\s\S]*?)<\/tr>/g, (_m, inner) => {
    const cells = [...inner.matchAll(/<t([dh])[^>]*>([\s\S]*?)<\/t\1>/g)].map((c) =>
      decodeEntities(stripTags(c[2])).trim()
    );
    const hit = cells.find((t) => /^[DP]\d{2}$/.test(t));
    if (!hit) return _m;
    const base = hit.toLowerCase();
    const n = slugger.rowCounts.get(base) ?? 0;
    slugger.rowCounts.set(base, n + 1);
    const id = n === 0 ? base : `${base}-${n}`;
    assignedIds.add(id);
    stats.rows++;
    return `<tr id="${id}">${inner}</tr>`;
  });

  // 3) 表格包一层横向滚动容器（窄屏可横滚，不撑破布局）
  html = html.replaceAll('<table>', '<div class="table-wrap"><table>').replaceAll('</table>', '</table></div>');

  // 4) 文档相对链接 → 页内锚；指向 4 份文档之外的链接失败并报告，不静默丢弃。
  //    例外：产物自引 index.html 保持原样（README 导航说明里链接图形化导览自身）。
  const docLinkMap = Object.fromEntries(DOC_DEFS.map((d) => [d.file, `#${d.id}`]));
  html = html.replace(/href="([^"]+)"/g, (m, href) => {
    if (href.startsWith('#')) return m;
    if (href === 'index.html') return m;
    const target = docLinkMap[href];
    if (!target) {
      linkFailures.push(`${doc.def.file}: 相对链接未解析到 4 份架构文档 → ${href}`);
      return m;
    }
    stats.mdLinks++;
    return `href="${target}"`;
  });

  // 5) 源码证据链接：包裹真实存在且已跟踪的仓库路径；::数字 追加 #L 行锚。
  //    不改动可见文本（<a> 只包在 <code> 外面），校验失败的保持纯文本并汇总。
  html = html.replace(/<code>([^<]*)<\/code>/g, (m, rawInner) => {
    const text = decodeEntities(rawInner);
    const parsed = PATH_TEXT_RE.exec(text);
    if (!parsed) return m;
    const p = parsed[1];
    const sym = parsed[3] || '';
    if (p.startsWith('.')) return m; // .tmp/.models/ 等非路径片段
    const isRootFile = !p.includes('/');
    if (isRootFile && !p.includes('.')) return m; // 纯符号（AppState、cache_key…）
    // 解析为仓库相对路径（clean），逐级退化：
    // 1) 白名单前缀 → 仓库相对，但须磁盘实存（crate 内部 `src/...` 会被 `src/` 前缀误吞）；
    // 2) 简写约定根（src-tauri/src/、src/、src-tauri/）；
    // 3) git 跟踪集合内唯一后缀匹配（crate 内部路径、crate 目录简写）；
    // 4) 裸文件名按 basename 唯一命中；全部未命中保持纯文本并汇总
    let clean = null;
    const cand = p.replace(/\/+$/, '');
    if (!isRootFile && PATH_PREFIX_WHITELIST.some((x) => p.startsWith(x))) {
      try {
        if (fs.statSync(path.resolve(ROOT, cand))) clean = cand;
      } catch {
        // 仓库相对不存在，落入下方简写解析
      }
    }
    if (!clean && !isRootFile) {
      for (const root of SHORT_ROOTS) {
        try {
          if (fs.statSync(path.resolve(ROOT, root, cand))) {
            clean = root + cand;
            break;
          }
        } catch {
          // 该简写根未命中，继续下一个
        }
      }
    }
    if (!clean && !isRootFile) {
      const hits = [...trackedSet].filter(
        (t) => !isExcludedDir(t) && (t === cand || t.endsWith('/' + cand) || t.includes('/' + cand + '/'))
      );
      if (hits.length === 1) {
        const t = hits[0];
        const idx = t.indexOf('/' + cand + '/');
        clean = idx >= 0 ? t.slice(0, idx + 1 + cand.length) : t;
      }
    }
    if (!clean) {
      if (isRootFile) {
        const hits = [...trackedSet].filter((t) => !isExcludedDir(t) && (t === p || t.endsWith('/' + p)));
        if (hits.length === 1) {
          clean = hits[0];
        } else {
          recordUnlinked(text, hits.length ? '裸文件名多重命中' : '路径不存在', doc.def.file);
          return m;
        }
      } else {
        recordUnlinked(text, '简写路径未命中', doc.def.file);
        return m;
      }
    }
    let st = null;
    try {
      st = fs.statSync(path.resolve(ROOT, clean));
    } catch {
      recordUnlinked(text, '路径不存在', doc.file);
      return m;
    }
    if (!isTracked(trackedSet, clean)) {
      recordUnlinked(text, '未被 git 跟踪', doc.file);
      return m;
    }
    const isDir = st.isDirectory();
    const base = `${REPO_URL}/${isDir ? 'tree' : 'blob'}/${BASE_COMMIT}/${clean}`;
    const href = !isDir && sym && /^\d+$/.test(sym) ? `${base}#L${sym}` : base;
    srcLinkCount++;
    return `<a class="src-link" href="${href}" target="_blank" rel="noopener noreferrer">${m}</a>`;
  });

  return { html, stats };
}

function recordUnlinked(text, reason, docFile) {
  const rec = unlinked.get(text) ?? { reason, docs: new Set() };
  rec.reason = reason;
  rec.docs.add(docFile);
  unlinked.set(text, rec);
}

/* ---------------------------------------------------------------- Mermaid 源码解析（文字阅读路径） */

/**
 * 从 mermaid 源码解析节点与边，按边顺序线性化为可读文本：
 * `节点标签 → (边标签) 节点标签 → …`，链在源变化处断开、以"；"连接。
 * 纯自动派生，不手工编写；节点缺标签定义视为解析缺口（构建失败）。
 */
function linearizeMermaid(code) {
  const labels = new Map();
  const edges = [];

  // 节点定义：ID["标签"] / ID[("标签")] / ID{"标签"} 等，标签统一为双引号形式
  const NODE_DEF = /([A-Za-z_]\w*)\s*[\[{]\s*\(*\s*"((?:[^"\\]|\\.)*)"\s*\)*\s*[\]}]/g;
  // 边语句：支持 & 扇出与 -->、-- 标签 -->、-. 标签 .->、-.->、==> 形式
  const ID_LIST = '[A-Za-z_]\\w*(?:\\s*&\\s*[A-Za-z_]\\w*)*';
  const EDGE = new RegExp(
    `^\\s*(${ID_LIST})\\s*(-->|-\\.->|==>` +
      `|--\\s*(?:"((?:[^"\\\\]|\\\\.)*)"|([^-]*?))\\s*-->` +
      `|-\\.\\s*(?:"((?:[^"\\\\]|\\\\.)*)"|([^.]*?))\\.->` +
      `|==\\s*(?:"((?:[^"\\\\]|\\\\.)*)"|([^=]*?))\\s*==>` +
      `)\\s*(${ID_LIST})\\s*$`
  );

  for (const rawLine of code.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('%%') || /^(flowchart|graph|subgraph|end)\b/.test(line)) continue;
    const stripped = line.replace(NODE_DEF, (_m, id, label) => {
      labels.set(id, label);
      return id;
    });
    const em = EDGE.exec(stripped);
    if (!em) continue;
    const srcs = em[1].split('&').map((s) => s.trim());
    // 组 2 是完整箭头分支，内部标签组为 3–8；组 9 才是目标 ID 列表
    const tgts = em[9].split('&').map((s) => s.trim());
    const label = (em[3] ?? em[4] ?? em[5] ?? em[6] ?? em[7] ?? em[8] ?? '').trim();
    for (const s of srcs) {
      for (const t of tgts) edges.push({ s, t, label });
    }
  }

  const lab = (id) => {
    const l = labels.get(id);
    if (l === undefined) throw new Error(`mermaid 解析缺口：节点 ${id} 无标签定义`);
    return l.replace(/<br\s*\/?>/gi, ' ').replace(/\s{2,}/g, ' ').trim();
  };

  const clauses = [];
  let cur = null; // { last, parts }
  for (const e of edges) {
    const step = e.label ? ` → (${e.label}) ${lab(e.t)}` : ` → ${lab(e.t)}`;
    if (cur && cur.last === e.s) {
      cur.parts.push(step);
      cur.last = e.t;
    } else {
      if (cur) clauses.push(cur.parts.join(''));
      cur = { last: e.t, parts: [lab(e.s) + step] };
    }
  }
  if (cur) clauses.push(cur.parts.join(''));
  return clauses.join('；');
}

/* ---------------------------------------------------------------- Mermaid 构建期渲染（puppeteer） */

function findBrowser() {
  for (const p of BROWSER_CANDIDATES) {
    try {
      if (p && fs.existsSync(p)) return p;
    } catch {
      /* 路径非法则继续探测 */
    }
  }
  throw new Error(
    '未找到可用的本地浏览器（已探测：\n  ' +
      BROWSER_CANDIDATES.join('\n  ') +
      '\n）可用环境变量 SCROLLERY_GUIDE_BROWSER 指定浏览器可执行文件路径。'
  );
}

async function renderFigures(figures) {
  if (!fs.existsSync(MERMAID_DIST)) throw new Error(`未找到 mermaid 发行文件: ${MERMAID_DIST}`);
  const executablePath = findBrowser();
  const mermaidJs = fs.readFileSync(MERMAID_DIST, 'utf8');
  const browser = await puppeteer.launch({
    executablePath,
    headless: true,
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 900 });
    await page.setContent('<!DOCTYPE html><html><head><meta charset="utf-8"></head><body><div id="cc" style="width:960px"></div></body></html>');
    await page.addScriptTag({ content: mermaidJs });

    // 每图渲染两遍：default 与 dark；两份 SVG 都内联，CSS 按 prefers-color-scheme 切换
    for (const theme of MERMAID_THEMES) {
      await page.evaluate((t) => {
        window.mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: t });
      }, theme);
      for (const fig of figures) {
        const renderId = `mmd-${fig.figId}-${theme}`;
        const res = await page.evaluate(
          async (id, code) => {
            try {
              const out = await window.mermaid.render(id, code);
              return { ok: true, svg: out.svg };
            } catch (e) {
              return { ok: false, error: String((e && e.message) || e) };
            }
          },
          renderId,
          fig.code
        );
        if (!res.ok) throw new Error(`mermaid 渲染失败（${fig.figId} @ ${theme}）: ${res.error}`);
        fig.svgs[theme] = res.svg;
      }
    }

    // 文字裁切检查：把全部 SVG 挂进可见 DOM 后逐个测量。
    // <text> 元素用 getBBox 对比 viewBox（容差 2px）；foreignObject 用 getBoundingClientRect 对比 SVG 矩形。
    const allSvgs = figures.flatMap((f) => MERMAID_THEMES.map((t) => f.svgs[t]));
    await page.setContent(
      `<!DOCTYPE html><html><head><meta charset="utf-8"></head><body><div id="cc" style="width:960px">${allSvgs.join('')}</div></body></html>`
    );
    const report = await page.evaluate(() => {
      const tol = 2;
      const out = [];
      document.querySelectorAll('#cc > svg').forEach((svg) => {
        const vb = svg.viewBox && svg.viewBox.baseVal;
        const item = { id: svg.id || '(无 id)', hasViewBox: !!(vb && vb.width > 0), textCount: 0, foCount: 0, issues: [] };
        if (item.hasViewBox) {
          svg.querySelectorAll('text').forEach((t) => {
            item.textCount++;
            try {
              const b = t.getBBox();
              if (
                b.x < vb.x - tol ||
                b.y < vb.y - tol ||
                b.x + b.width > vb.x + vb.width + tol ||
                b.y + b.height > vb.y + vb.height + tol
              ) {
                item.issues.push({ kind: 'text', label: (t.textContent || '').slice(0, 40), box: [b.x, b.y, b.width, b.height].map(Math.round) });
              }
            } catch (e) {
              item.issues.push({ kind: 'text-error', label: String(e) });
            }
          });
          const sr = svg.getBoundingClientRect();
          svg.querySelectorAll('foreignObject').forEach((f) => {
            item.foCount++;
            const r = f.getBoundingClientRect();
            if (r.left < sr.left - tol || r.top < sr.top - tol || r.right > sr.right + tol || r.bottom > sr.bottom + tol) {
              item.issues.push({
                kind: 'foreignObject',
                label: (f.textContent || '').slice(0, 40),
                box: [r.left, r.top, r.right, r.bottom].map(Math.round),
                svgBox: [sr.left, sr.top, sr.right, sr.bottom].map(Math.round),
              });
            }
          });
        }
        out.push(item);
      });
      return out;
    });

    // 汇总裁切检查结果；检查不可行时如实降级为警告
    const bySvg = new Map(report.map((r) => [r.id, r]));
    for (const fig of figures) {
      for (const theme of MERMAID_THEMES) {
        const r = bySvg.get(`mmd-${fig.figId}-${theme}`);
        assert(r, `裁切检查缺少渲染结果: ${fig.figId} @ ${theme}`);
        if (!r) continue;
        assert(r.hasViewBox, `SVG 缺少 viewBox: ${fig.figId} @ ${theme}`);
        for (const issue of r.issues) {
          failures.push(
            `文字裁切越界（${fig.figId} @ ${theme}, ${issue.kind}）: "${issue.label}" box=${JSON.stringify(issue.box)}` +
              (issue.svgBox ? ` svg=${JSON.stringify(issue.svgBox)}` : '')
          );
        }
        if (r.textCount === 0 && r.foCount === 0) {
          warn(`${fig.figId} @ ${theme}: SVG 中无 <text> 与 <foreignObject>，文字裁切检查不可行，已降级为警告。`);
        } else if (r.textCount === 0) {
          warn(
            `${fig.figId} @ ${theme}: 无 <text> 元素（mermaid 11 htmlLabels 模式将文本渲染为 foreignObject），` +
              `getBBox 文本检查不可行，已降级；文本边界由 foreignObject 检查覆盖（${r.foCount} 个）。`
          );
        }
      }
    }
    return { rendered: figures.length * MERMAID_THEMES.length, browserPath: executablePath, clipReport: report };
  } finally {
    // 用完即关，不留常驻浏览器进程
    await browser.close();
  }
}

/* ---------------------------------------------------------------- figure HTML 与页面装配 */

function buildFigureHtml(fig) {
  return [
    `<figure id="${fig.figId}" class="arch-fig">`,
    '  <div class="fig-canvas">',
    '    <div class="fig-stage">',
    `      <div class="fig-svg fig-theme-default">${fig.svgs.default}</div>`,
    `      <div class="fig-svg fig-theme-dark">${fig.svgs.dark}</div>`,
    '    </div>',
    '    <div class="fig-toolbar" role="group" aria-label="图形缩放工具条">',
    '      <button type="button" class="fig-btn" data-act="in" title="放大" aria-label="放大">＋</button>',
    '      <button type="button" class="fig-btn" data-act="out" title="缩小" aria-label="缩小">−</button>',
    '      <button type="button" class="fig-btn" data-act="reset" title="复位为适配宽度" aria-label="复位">复位</button>',
    '    </div>',
    '  </div>',
    '  <figcaption class="fig-cap">',
    `    <div class="fig-title">${escapeHtml(fig.title)}</div>`,
    `    <div class="fig-desc">${fig.descHtml}</div>`,
    `    <div class="fig-path"><span class="fig-path-label">文字阅读路径：</span>${escapeHtml(fig.readingPath)}</div>`,
    '    <details class="fig-src"><summary>Mermaid 源（GitHub 兼容语法）</summary>',
    `      <pre><code>${escapeHtml(fig.code)}</code></pre>`,
    '    </details>',
    '  </figcaption>',
    '</figure>',
  ].join('\n');
}

function buildHeadComment(docs, meta, genTime) {
  const hashLines = docs.map((d) => `  ${DOCS_DIR}/${d.def.file}  sha256=${d.sha256}`).join('\n');
  // 注意 HTML 注释内不得出现 "--"，时间戳与哈希均为无连字符连续段或单连字符，安全
  return [
    '<!--',
    '本文件由 scripts/build-architecture-guide.mjs 自动生成，勿手改。重建：npm run build:architecture-guide',
    `生成时间：${genTime}`,
    `源码基线：commit ${BASE_COMMIT}（构建时 HEAD：${meta.headSha}）`,
    '输入文件 sha256：',
    hashLines,
    '-->',
  ].join('\n');
}

function buildBaselineBar(meta, staticPhrase, genTime) {
  const badge = meta.docsCommitted
    ? '<span class="badge ok">架构文档已提交</span>'
    : '<span class="badge warn">架构文档未提交</span>';
  return [
    '<div class="baseline-bar">',
    `  <span>源码基线：<a href="${REPO_URL}/commit/${BASE_COMMIT}" target="_blank" rel="noopener noreferrer"><code>${BASE_COMMIT}</code></a></span>`,
    `  ${badge}`,
    `  <span class="limit-note">限制说明（引 coverage.md）：${escapeHtml(staticPhrase)}</span>`,
    `  <span>生成时间：${escapeHtml(genTime)}</span>`,
    '</div>',
  ].join('\n');
}

function buildCardsHtml(cards) {
  return [
    '<nav class="cards" aria-label="文档入口">',
    ...cards.map(
      (c, i) =>
        `  <a class="card" href="#${c.docId}">` +
        `<span class="card-kicker">${escapeHtml(CARD_KICKERS[i] ?? '')}</span>` +
        `<span class="card-title">${escapeHtml(c.title)}</span>` +
        `<span class="card-sub">${escapeHtml(c.subtitle)}</span></a>`
    ),
    '</nav>',
  ].join('\n');
}

/* ---------------------------------------------------------------- 页面内联 CSS（无依赖，双主题） */

const PAGE_CSS = String.raw`
:root {
  --bg: #ffffff;
  --fg: #1f2328;
  --muted: #59636e;
  --border: #d1d9e0;
  --card: #f6f8fa;
  --code-bg: #f6f8fa;
  --accent: #0969da;
  --accent-soft: rgba(9, 105, 218, 0.08);
  --warn: #9a6700;
  --warn-border: #d4a72c66;
  --fig-bg: #ffffff;
  --row-target: #fff8c5;
  --shadow: 0 8px 24px rgba(140, 149, 159, 0.2);
  --topbar-h: 58px;
  --mono: ui-monospace, "Cascadia Mono", "SF Mono", Consolas, "Courier New", monospace;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d1117;
    --fg: #e6edf3;
    --muted: #9198a1;
    --border: #3d444d;
    --card: #151b23;
    --code-bg: #151b23;
    --accent: #4c8dff;
    --accent-soft: rgba(76, 141, 255, 0.14);
    --warn: #d29922;
    --warn-border: #d2992255;
    --fig-bg: #0d1117;
    --row-target: #272115;
    --shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font: 15px/1.75 system-ui, -apple-system, "Segoe UI", "Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", sans-serif;
  text-rendering: optimizeLegibility;
}
a { color: var(--accent); }
code {
  font-family: var(--mono);
  font-size: 0.88em;
  background: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 0.08em 0.35em;
  word-break: break-all;
}
pre code { display: block; border: none; padding: 0; }

/* 顶栏（标题 + 搜索），窄屏时搜索框移入抽屉 */
.topbar {
  position: sticky;
  top: 0;
  z-index: 40;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
}
.topbar-row {
  display: flex;
  align-items: center;
  gap: 12px;
  max-width: 1400px;
  margin: 0 auto;
  padding: 10px 16px;
  height: var(--topbar-h);
}
.site-title { font-size: 17px; font-weight: 700; white-space: nowrap; }
.menu-btn {
  display: none;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  border-radius: 8px;
  width: 36px;
  height: 34px;
  font-size: 16px;
  cursor: pointer;
}
.search-wrap { position: relative; flex: 1; max-width: 560px; margin-left: auto; }
.search-input {
  width: 100%;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  font: inherit;
  font-size: 13.5px;
}
.search-input:focus { outline: 2px solid var(--accent-soft); border-color: var(--accent); }
.search-pop {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  right: 0;
  max-height: 60vh;
  overflow-y: auto;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--shadow);
  display: none;
  z-index: 60;
}
.search-pop.open { display: block; }
.sr-meta { padding: 8px 12px 2px; font-size: 11.5px; color: var(--muted); }
.sr-group { padding: 8px 12px 2px; font-size: 11px; color: var(--muted); font-weight: 700; }
.sr-item { display: block; padding: 6px 12px; text-decoration: none; color: inherit; }
.sr-item:hover { background: var(--card); }
.sr-title { display: block; font-size: 13.5px; font-weight: 600; }
.sr-excerpt { display: block; font-size: 12px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* 基线信息条与入口卡片（不随滚动吸顶，保持顶栏轻量） */
.baseline-bar {
  max-width: 1400px;
  margin: 0 auto;
  padding: 8px 16px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px 18px;
  align-items: center;
  font-size: 12.5px;
  color: var(--muted);
}
.badge {
  display: inline-block;
  padding: 0 10px;
  border-radius: 999px;
  border: 1px solid var(--border);
  font-weight: 600;
}
.badge.warn { color: var(--warn); border-color: var(--warn-border); }
.badge.ok { color: var(--muted); }
.limit-note { flex: 1 1 320px; }
.cards {
  max-width: 1400px;
  margin: 0 auto;
  padding: 4px 16px 14px;
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}
.card {
  display: block;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 10px 14px;
  background: var(--card);
  color: inherit;
  text-decoration: none;
}
.card:hover { border-color: var(--accent); }
.card-kicker { display: block; font-size: 11px; font-weight: 700; color: var(--accent); letter-spacing: 0.04em; }
.card-title { display: block; font-size: 14px; font-weight: 650; margin-top: 2px; }
.card-sub { display: block; font-size: 12.5px; color: var(--muted); margin-top: 2px; }

/* 主布局：左侧目录 + 正文 */
.layout {
  display: grid;
  grid-template-columns: 268px minmax(0, 1fr);
  max-width: 1400px;
  margin: 0 auto;
  align-items: start;
}
#sidebar {
  position: sticky;
  top: var(--topbar-h);
  height: calc(100vh - var(--topbar-h));
  overflow-y: auto;
  padding: 14px 10px 40px;
  border-right: 1px solid var(--border);
  font-size: 13px;
}
.toc-doc { display: block; font-weight: 700; padding: 6px 8px; text-decoration: none; color: var(--fg); }
.toc-item {
  display: block;
  padding: 3px 8px;
  border-left: 2px solid transparent;
  text-decoration: none;
  color: var(--muted);
  line-height: 1.5;
}
.toc-item.toc-h2 { padding-left: 16px; }
.toc-item.toc-h3 { padding-left: 30px; font-size: 12.5px; }
.toc-item:hover { color: var(--fg); }
.toc-item.active { color: var(--accent); border-left-color: var(--accent); background: var(--accent-soft); }
main { padding: 10px 30px 90px; min-width: 0; }
.doc { margin-bottom: 64px; }
[id] { scroll-margin-top: calc(var(--topbar-h) + 14px); }

/* 文档排版 */
.doc h1 { font-size: 26px; line-height: 1.35; border-bottom: 1px solid var(--border); padding-bottom: 8px; }
.doc h2 { font-size: 21px; margin-top: 38px; border-bottom: 1px solid var(--border); padding-bottom: 6px; }
.doc h3 { font-size: 17px; margin-top: 30px; }
.doc h4 { font-size: 15.5px; margin-top: 24px; }
.doc h1, .doc h2, .doc h3, .doc h4 { line-height: 1.45; }
.h-anchor { opacity: 0; margin-left: 8px; font-weight: 400; text-decoration: none; }
.doc h1:hover .h-anchor, .doc h2:hover .h-anchor, .doc h3:hover .h-anchor, .doc h4:hover .h-anchor { opacity: 1; }
.doc blockquote {
  margin: 14px 0;
  padding: 8px 16px;
  border-left: 3px solid var(--accent);
  background: var(--card);
  border-radius: 0 8px 8px 0;
  color: var(--muted);
}
.doc blockquote p { margin: 4px 0; }
.table-wrap { overflow-x: auto; margin: 14px 0; border: 1px solid var(--border); border-radius: 10px; }
table { border-collapse: collapse; width: 100%; font-size: 13.5px; line-height: 1.6; }
th, td { border: 1px solid var(--border); padding: 7px 11px; text-align: left; vertical-align: top; }
th { background: var(--card); white-space: nowrap; }
tr:target { background: var(--row-target); }
tr:target td:first-child { box-shadow: inset 3px 0 0 var(--accent); }
.doc li { margin: 3px 0; }
a.src-link { text-decoration: none; }
a.src-link:hover code { border-color: var(--accent); color: var(--accent); }
.doc hr { border: none; border-top: 1px solid var(--border); margin: 28px 0; }

/* 图：画布 + 右上工具条 + 图注 */
figure.arch-fig { margin: 22px 0; border: 1px solid var(--border); border-radius: 12px; overflow: hidden; }
.fig-canvas { position: relative; overflow: hidden; background: var(--fig-bg); padding: 16px 16px 8px; touch-action: pan-y; }
.fig-canvas.zoomed { cursor: grab; user-select: none; }
.fig-canvas.zoomed:active { cursor: grabbing; }
.fig-stage { transform-origin: 0 0; }
.fig-svg { width: 100%; }
.fig-svg svg { width: 100%; height: auto; display: block; }
.fig-theme-dark { display: none; }
@media (prefers-color-scheme: dark) {
  .fig-theme-default { display: none; }
  .fig-theme-dark { display: block; }
}
.fig-toolbar { position: absolute; top: 8px; right: 8px; display: flex; gap: 6px; z-index: 5; }
.fig-btn {
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  border-radius: 7px;
  min-width: 30px;
  height: 28px;
  padding: 0 9px;
  font-size: 13px;
  cursor: pointer;
}
.fig-btn:hover { border-color: var(--accent); color: var(--accent); }
.fig-cap { border-top: 1px solid var(--border); background: var(--card); padding: 10px 16px 12px; font-size: 13px; }
.fig-title { font-weight: 700; font-size: 13.5px; }
.fig-desc { margin-top: 4px; }
.fig-path { margin-top: 6px; color: var(--muted); line-height: 1.7; }
.fig-path-label { font-weight: 600; color: var(--fg); }
.fig-src { margin-top: 8px; }
.fig-src summary { cursor: pointer; color: var(--accent); font-size: 12.5px; }
.fig-src pre { background: var(--code-bg); border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; overflow-x: auto; font-size: 12px; line-height: 1.6; }

/* 窄屏：侧栏变抽屉，搜索框随抽屉可达 */
#backdrop { display: none; }
.drawer-search { display: none; }
@media (max-width: 900px) {
  .topbar .search-wrap { display: none; }
  .drawer-search { display: block; margin-bottom: 12px; }
  .menu-btn { display: inline-flex; align-items: center; justify-content: center; }
  .cards { grid-template-columns: 1fr; }
  .layout { grid-template-columns: minmax(0, 1fr); }
  #sidebar {
    position: fixed;
    left: 0;
    top: 0;
    bottom: 0;
    height: 100vh;
    width: min(320px, 85vw);
    background: var(--bg);
    z-index: 80;
    transform: translateX(-103%);
    transition: transform 0.2s ease;
    box-shadow: var(--shadow);
  }
  body.drawer-open #sidebar { transform: none; }
  #backdrop {
    display: none;
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 70;
  }
  body.drawer-open #backdrop { display: block; }
  main { padding: 8px 14px 70px; }
}
`;

/* ---------------------------------------------------------------- 页面内联脚本（原生 JS；避免反引号与 ${} 以便安全内联） */

const CLIENT_JS = String.raw`
(function () {
  'use strict';
  function $(sel, el) { return (el || document).querySelector(sel); }
  function $$(sel, el) { return Array.prototype.slice.call((el || document).querySelectorAll(sel)); }
  function esc(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }
  function headingText(h) {
    var clone = h.cloneNode(true);
    $$('.h-anchor', clone).forEach(function (a) { a.remove(); });
    return (clone.textContent || '').replace(/\s+/g, ' ').trim();
  }
  function secTitle(el) {
    var sec = el && el.closest ? el.closest('.doc') : null;
    return sec ? (sec.getAttribute('data-title') || '') : '';
  }
  function closeDrawer() { document.body.classList.remove('drawer-open'); }

  /* ---------- 目录树：4 个文档分组，h2/h3 缩进 ---------- */
  var tocNav = $('#toc');
  var tocLinks = [];
  $$('.doc').forEach(function (sec) {
    var group = document.createElement('div');
    group.className = 'toc-group';
    var h1 = $('h1', sec);
    if (h1 && h1.id) {
      var g = document.createElement('a');
      g.className = 'toc-doc';
      g.href = '#' + h1.id;
      g.textContent = headingText(h1);
      group.appendChild(g);
    }
    $$('h2, h3', sec).forEach(function (h) {
      if (!h.id) return;
      var a = document.createElement('a');
      a.className = 'toc-item toc-' + h.tagName.toLowerCase();
      a.href = '#' + h.id;
      a.textContent = headingText(h);
      group.appendChild(a);
      tocLinks.push(a);
    });
    tocNav.appendChild(group);
  });

  /* ---------- IntersectionObserver 滚动高亮 ---------- */
  var activeLink = null;
  function activate(id) {
    var next = null;
    for (var i = 0; i < tocLinks.length; i++) {
      if (tocLinks[i].getAttribute('href') === '#' + id) { next = tocLinks[i]; break; }
    }
    if (!next || next === activeLink) return;
    if (activeLink) activeLink.classList.remove('active');
    activeLink = next;
    activeLink.classList.add('active');
    if (activeLink.scrollIntoViewIfNeeded) activeLink.scrollIntoViewIfNeeded(false);
  }
  if ('IntersectionObserver' in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { if (en.isIntersecting) activate(en.target.id); });
    }, { rootMargin: '0px 0px -72% 0px', threshold: 0 });
    $$('.doc h2[id], .doc h3[id]').forEach(function (h) { io.observe(h); });
  }

  /* ---------- 搜索：运行时从 DOM 建索引（标题 / 图注 / 带 ID 表格行 / 正文按节聚合） ---------- */
  var GROUP_ORDER = ['标题', '图', '正文', '表格行'];
  var searchIndex = null;
  function buildIndex() {
    if (searchIndex) return searchIndex;
    searchIndex = [];
    $$('.doc h1[id], .doc h2[id], .doc h3[id], .doc h4[id]').forEach(function (h) {
      var t = headingText(h);
      searchIndex.push({ type: '标题', id: h.id, title: t, doc: secTitle(h), text: t });
    });
    $$('figure.arch-fig').forEach(function (f) {
      var parts = $$('.fig-title, .fig-desc, .fig-path', f).map(function (e) { return e.textContent || ''; });
      searchIndex.push({
        type: '图',
        id: f.id,
        title: parts[0] || f.id,
        doc: secTitle(f),
        text: parts.join(' ').replace(/\s+/g, ' ')
      });
    });
    $$('tr[id]').forEach(function (tr) {
      var txt = (tr.textContent || '').replace(/\s+/g, ' ').trim();
      searchIndex.push({ type: '表格行', id: tr.id, title: txt.length > 46 ? txt.slice(0, 46) + '…' : txt, doc: secTitle(tr), text: txt });
    });
    $$('.doc').forEach(function (sec) {
      var t = sec.getAttribute('data-title') || sec.id;
      searchIndex.push({ type: '正文', id: sec.id, title: t, doc: t, text: (sec.textContent || '').replace(/\s+/g, ' ') });
    });
    return searchIndex;
  }
  function excerpt(text, q) {
    var i = text.toLowerCase().indexOf(q);
    if (i < 0) return text.slice(0, 80);
    var start = Math.max(0, i - 24);
    var end = Math.min(text.length, i + q.length + 48);
    return (start > 0 ? '…' : '') + text.slice(start, end) + (end < text.length ? '…' : '');
  }
  function runSearch(q) {
    if (q.length < 2) return [];
    var ql = q.toLowerCase();
    var byType = {};
    var total = 0;
    var idx = buildIndex();
    for (var i = 0; i < idx.length && total < 200; i++) {
      var it = idx[i];
      if (it.title.toLowerCase().indexOf(ql) < 0 && it.text.toLowerCase().indexOf(ql) < 0) continue;
      if (!byType[it.type]) byType[it.type] = [];
      byType[it.type].push(it);
      total++;
    }
    var out = [];
    GROUP_ORDER.forEach(function (t) {
      (byType[t] || []).slice(0, 12).forEach(function (it) { out.push(it); });
    });
    return out;
  }

  var searchInputs = $$('.search-input');
  function renderPop(pop, results, q) {
    if (q.length < 2 || !results.length) {
      pop.classList.remove('open');
      pop.innerHTML = '';
      return;
    }
    var html = '<div class="sr-meta">命中 ' + results.length + ' 条（Enter 跳转第一条，Esc 关闭）</div>';
    var lastType = null;
    results.forEach(function (it) {
      if (it.type !== lastType) {
        html += '<div class="sr-group">' + it.type + '</div>';
        lastType = it.type;
      }
      html += '<a class="sr-item" href="#" data-id="' + esc(it.id) + '">' +
        '<span class="sr-title">' + esc(it.title) + '</span>' +
        '<span class="sr-excerpt">' + esc(it.doc + ' · ' + excerpt(it.text, q.toLowerCase())) + '</span></a>';
    });
    pop.innerHTML = html;
    pop.classList.add('open');
    $$('.sr-item', pop).forEach(function (a) {
      a.addEventListener('click', function (ev) {
        ev.preventDefault();
        location.hash = a.getAttribute('data-id');
        $$('.search-pop').forEach(function (p) { p.classList.remove('open'); });
        closeDrawer();
      });
    });
  }
  searchInputs.forEach(function (input) {
    var pop = input.parentElement.querySelector('.search-pop');
    function refresh() {
      var q = input.value.trim();
      searchInputs.forEach(function (o) { if (o !== input) o.value = input.value; });
      var results = runSearch(q);
      $$('.search-pop').forEach(function (p) { renderPop(p, results, q); });
    }
    input.addEventListener('input', refresh);
    input.addEventListener('focus', function () {
      if (input.value.trim().length >= 2) refresh();
    });
    input.addEventListener('keydown', function (e) {
      if (e.key === 'Escape') {
        $$('.search-pop').forEach(function (p) { p.classList.remove('open'); });
        input.blur();
      } else if (e.key === 'Enter') {
        var first = $('.sr-item', pop);
        if (first) {
          location.hash = first.getAttribute('data-id');
          $$('.search-pop').forEach(function (p) { p.classList.remove('open'); });
          closeDrawer();
        }
      }
    });
  });
  document.addEventListener('click', function (e) {
    if (!e.target.closest('.search-wrap')) {
      $$('.search-pop').forEach(function (p) { p.classList.remove('open'); });
    }
  });

  /* ---------- 图形交互：右上工具条缩放，放大后拖拽平移，双击复位 ---------- */
  $$('figure.arch-fig').forEach(function (fig) {
    var canvas = $('.fig-canvas', fig);
    var stage = $('.fig-stage', fig);
    var scale = 1, tx = 0, ty = 0;
    var dragging = false, sx = 0, sy = 0, ox = 0, oy = 0;
    function clampPan() {
      var cw = canvas.clientWidth, ch = canvas.clientHeight;
      if (scale <= 1) { tx = 0; ty = 0; return; }
      var minX = cw * (1 - scale), minY = ch * (1 - scale);
      if (tx > 0) tx = 0;
      if (tx < minX) tx = minX;
      if (ty > 0) ty = 0;
      if (ty < minY) ty = minY;
    }
    function apply() {
      stage.style.transform = 'translate(' + tx + 'px,' + ty + 'px) scale(' + scale + ')';
      canvas.classList.toggle('zoomed', scale > 1);
    }
    function setScale(s) {
      scale = Math.min(4, Math.max(1, s));
      if (scale === 1) { tx = 0; ty = 0; }
      clampPan();
      apply();
    }
    $('[data-act="in"]', fig).addEventListener('click', function () { setScale(scale * 1.25); });
    $('[data-act="out"]', fig).addEventListener('click', function () { setScale(scale / 1.25); });
    $('[data-act="reset"]', fig).addEventListener('click', function () { setScale(1); });
    canvas.addEventListener('dblclick', function () { setScale(1); });
    canvas.addEventListener('pointerdown', function (e) {
      if (scale <= 1) return;
      dragging = true;
      sx = e.clientX; sy = e.clientY; ox = tx; oy = ty;
      canvas.setPointerCapture(e.pointerId);
      e.preventDefault();
    });
    canvas.addEventListener('pointermove', function (e) {
      if (!dragging) return;
      tx = ox + (e.clientX - sx);
      ty = oy + (e.clientY - sy);
      clampPan();
      apply();
    });
    canvas.addEventListener('pointerup', function () { dragging = false; });
    canvas.addEventListener('pointercancel', function () { dragging = false; });
    window.addEventListener('resize', function () { clampPan(); apply(); });
  });

  /* ---------- 窄屏抽屉 ---------- */
  var menuBtn = $('#menu-btn');
  if (menuBtn) {
    menuBtn.addEventListener('click', function () { document.body.classList.toggle('drawer-open'); });
  }
  var backdrop = $('#backdrop');
  if (backdrop) backdrop.addEventListener('click', closeDrawer);
  $$('a[href^="#"]').forEach(function (a) {
    a.addEventListener('click', function () { closeDrawer(); });
  });
})();
`;

/* ---------------------------------------------------------------- 页面装配 */

function assemblePage({ docs, cards, meta, staticPhrase, genTime }) {
  const sectionsHtml = docs
    .map((d) => `<section id="${d.def.id}" class="doc" data-title="${escapeHtml(d.title)}">\n${d.html}\n</section>`)
    .join('\n');
  return [
    '<!DOCTYPE html>',
    '<html lang="zh-CN">',
    '<head>',
    '<meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    '<title>Scrollery 架构导览</title>',
    `<style>${PAGE_CSS}</style>`,
    '</head>',
    '<body>',
    '<header class="topbar">',
    '  <div class="topbar-row">',
    '    <button id="menu-btn" class="menu-btn" type="button" aria-label="打开目录">☰</button>',
    '    <div class="site-title">Scrollery 架构导览</div>',
    '    <div class="search-wrap">',
    '      <input id="search-main" class="search-input" type="search" placeholder="搜索标题 / 图 / 表格行 / 正文（≥2 字符）" autocomplete="off">',
    '      <div class="search-pop"></div>',
    '    </div>',
    '  </div>',
    '</header>',
    buildBaselineBar(meta, staticPhrase, genTime),
    buildCardsHtml(cards),
    '<div id="backdrop"></div>',
    '<div class="layout">',
    '  <aside id="sidebar" aria-label="目录">',
    '    <div class="search-wrap drawer-search">',
    '      <input id="search-drawer" class="search-input" type="search" placeholder="搜索（≥2 字符）" autocomplete="off">',
    '      <div class="search-pop"></div>',
    '    </div>',
    '    <nav id="toc" class="toc"></nav>',
    '  </aside>',
    '  <main id="main">',
    sectionsHtml,
    '  </main>',
    '</div>',
    `<script>${CLIENT_JS}</script>`,
    '</body>',
    '</html>',
  ].join('\n');
}

/* ---------------------------------------------------------------- 断言（构建期） */

function runAssertions({ docs, figures, finalHtml, trackedSet, assignedIds }) {
  // 1) 恰好 5 个 mermaid 图且渲染成功（SVG 非空、含 viewBox）
  assert(figures.length === 5, `mermaid 图块数量应为 5，实际 ${figures.length}`);
  for (const fig of figures) {
    for (const theme of MERMAID_THEMES) {
      const svg = fig.svgs[theme] || '';
      assert(svg.length > 200, `SVG 为空或过短: ${fig.figId} @ ${theme}`);
      assert(svg.includes('viewBox'), `SVG 缺少 viewBox: ${fig.figId} @ ${theme}`);
    }
  }

  // 2) D/P ID 集合：md 原文与产物逐一对应（不丢失、不新增）；索引/地图行数 23 + 23
  const mdIds = new Set();
  for (const d of docs) for (const m of d.raw.matchAll(/[DP]\d{2}/g)) mdIds.add(m[0]);
  const htmlIdMatches = [...finalHtml.matchAll(/[DP]\d{2}/g)].map((m) => m[0]);
  const htmlIds = new Set(htmlIdMatches);
  const onlyInMd = [...mdIds].filter((x) => !htmlIds.has(x));
  const onlyInHtml = [...htmlIds].filter((x) => !mdIds.has(x));
  assert(
    onlyInMd.length === 0 && onlyInHtml.length === 0,
    `D/P ID 集合不一致：md 独有 [${onlyInMd.join(', ')}]，html 独有 [${onlyInHtml.join(', ')}]`
  );
  const readmeHtml = docs.find((d) => d.def.file === 'README.md').html;
  const pipelinesHtml = docs.find((d) => d.def.file === 'pipelines.md').html;
  const readmeDRows = [...readmeHtml.matchAll(/<tr id="(d\d{2})"/g)].map((m) => m[1]);
  const pipelinesPRows = [...pipelinesHtml.matchAll(/<tr id="(p\d{2})"/g)].map((m) => m[1]);
  assert(
    readmeDRows.length === 23 && new Set(readmeDRows).size === 23,
    `README 功能地图应为 23 行 D 锚点，实际 ${readmeDRows.length}（去重 ${new Set(readmeDRows).size}）`
  );
  assert(
    pipelinesPRows.length === 23 && new Set(pipelinesPRows).size === 23,
    `pipelines 索引表应为 23 行 P 锚点，实际 ${pipelinesPRows.length}（去重 ${new Set(pipelinesPRows).size}）`
  );

  // 3) 页内所有 href="#..." 均命中存在的 id
  const allIds = new Set(assignedIds);
  for (const m of finalHtml.matchAll(/\sid="([^"]+)"/g)) allIds.add(m[1]);
  for (const m of finalHtml.matchAll(/href="#([^"]+)"/g)) {
    assert(allIds.has(m[1]), `锚点失效: #${m[1]}`);
  }

  // 4) 相对链接、源码链接目标、绝对路径、外部资源
  for (const f of linkFailures) failures.push(f);
  for (const m of finalHtml.matchAll(
    /href="https:\/\/github\.com\/gfgjs\/scrollery-private\/(?:blob|tree)\/162732f3\/([^"#?]+)/g
  )) {
    assert(isTracked(trackedSet, m[1]), `源码链接目标未被跟踪: ${m[1]}`);
  }
  assert(!/C:\\|C:\/|\/c\/|file:\/\/\//.test(finalHtml), '产物含本机绝对路径（C:\\、C:/、/c/、file:///）');
  // 资源加载禁止外链：剔除 <a> 开标签（其 href 允许指向 GitHub）、SVG 命名空间声明
  // （xmlns 是标识符，浏览器不联网加载）、行内代码文本（如正文提到的 dev-only `file://`）
  // 之后，任何 http(s)/file:// 引用都算失败。真正的资源加载只会出现在属性里，故以上剔除不会漏检。
  const withoutAnchors = finalHtml
    .replaceAll(/<a\b[^>]*>/g, '')
    .replaceAll(/xmlns(:\w+)?="[^"]*"/g, '')
    .replaceAll(/<code>[\s\S]*?<\/code>/g, '');
  assert(!/https?:\/\//.test(withoutAnchors), '产物含外部资源引用（http/https）');
  assert(!/file:\/\//.test(withoutAnchors), '产物含外部资源引用（file://）');

  // 5) id 唯一；无 <script> / <iframe>；无残留占位符
  const idMatches = [...finalHtml.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]);
  assert(new Set(idMatches).size === idMatches.length, `存在重复 id: ${findDuplicates(idMatches).join(', ')}`);
  const scriptCount = (finalHtml.match(/<script/g) || []).length;
  assert(scriptCount === 1, `产物应恰好含 1 处内联 <script>，实际 ${scriptCount}`);
  assert(!/<iframe/i.test(finalHtml), '产物含 <iframe>');
  assert(!finalHtml.includes('mermaid-slot'), '产物残留 mermaid 占位符');
}

function findDuplicates(arr) {
  const seen = new Set();
  const dup = new Set();
  for (const x of arr) {
    if (seen.has(x)) dup.add(x);
    seen.add(x);
  }
  return [...dup];
}

/* ---------------------------------------------------------------- 主流程 */

async function main() {
  const t0 = Date.now();
  const genTime = (() => {
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  })();

  setupMarked();
  const meta = collectMeta();

  // 读取与解析
  const docs = DOC_DEFS.map((def) => {
    const abs = path.join(ROOT, DOCS_DIR, def.file);
    const raw = fs.readFileSync(abs, 'utf8');
    const blocks = extractMermaidBlocks(raw);
    const figures = blocks.map((b, i) => {
      const meta2 = deriveFigureMeta(raw, b.fenceLine, def.file);
      return {
        docFile: def.file,
        figId: `${def.figBase}-${i + 1}`,
        title: `${meta2.headingText} · 图 ${i + 1}`,
        descRaw: meta2.descRaw,
        descHtml: marked.parseInline(meta2.descRaw).trim(),
        code: b.code,
        svgs: {},
      };
    });
    return { def, raw, sha256: sha256File(abs), figures };
  });

  const allFigures = docs.flatMap((d) => d.figures);
  for (const fig of allFigures) {
    fig.readingPath = linearizeMermaid(fig.code);
  }

  // 渲染与后处理
  const slugger = { heading: new Slugger(), rowCounts: new Map() };
  const assignedIds = new Set();
  const trackedSet = meta.tracked;
  for (const d of docs) {
    const { html, stats } = renderAndPostProcess(d, d.raw, slugger, assignedIds, trackedSet);
    d.html = html;
    d.stats = stats;
  }
  assert(mermaidSlotCounter === allFigures.length, `mermaid 占位符数（${mermaidSlotCounter}）与图块数（${allFigures.length}）不一致`);

  // 构建期渲染 mermaid（puppeteer，用完即关）
  const renderInfo = await renderFigures(allFigures);

  // 回填 figure、装配页面
  for (const fig of allFigures) {
    const figHtml = buildFigureHtml(fig);
    const placeholder = `<div class="mermaid-slot" data-slot="${allFigures.indexOf(fig)}"></div>`;
    let replaced = false;
    for (const d of docs) {
      if (d.html.includes(placeholder)) {
        d.html = d.html.split(placeholder).join(figHtml);
        replaced = true;
        break;
      }
    }
    assert(replaced, `占位符未找到: slot ${allFigures.indexOf(fig)}（${fig.figId}）`);
  }

  const readmeRaw = docs.find((d) => d.def.file === 'README.md').raw;
  const cards = parseNavCards(readmeRaw);
  assert(cards.length === 4, `阅读导航表应解析出 4 张入口卡片，实际 ${cards.length}`);
  assert(
    cards.every((c, i) => c.docId === DOC_DEFS[i].id),
    '入口卡片与文档顺序不一致'
  );

  const coverageRaw = docs.find((d) => d.def.file === 'coverage.md').raw;
  const phraseMatch = /全部结论为静态阅读[^。]*。/.exec(coverageRaw);
  assert(phraseMatch, 'coverage.md 中未找到"全部结论为静态阅读"原句');
  const staticPhrase = phraseMatch ? phraseMatch[0] : '';

  const finalHtml = assemblePage({ docs, cards, meta, staticPhrase, genTime });
  runAssertions({ docs, figures: allFigures, finalHtml, trackedSet, assignedIds });

  if (failures.length) {
    console.error(`\n构建失败：${failures.length} 项断言未通过`);
    for (const f of failures) console.error('  ✗ ' + f);
    process.exitCode = 1;
    return;
  }

  fs.writeFileSync(OUTPUT_PATH, finalHtml, 'utf8');

  // ---- 构建摘要 ----
  console.log('\n== 构建摘要 ==');
  for (const d of docs) {
    console.log(
      `  输入 ${DOCS_DIR}/${d.def.file}: ${formatBytes(Buffer.byteLength(d.raw))}  标题锚点 ${d.stats.headings}  行锚点 ${d.stats.rows}  文档互链 ${d.stats.mdLinks}  sha256 ${d.sha256.slice(0, 12)}…`
    );
  }
  console.log(`  产物 ${path.relative(ROOT, OUTPUT_PATH)}: ${formatBytes(Buffer.byteLength(finalHtml))}`);
  console.log(`  图: ${allFigures.length} 个 mermaid 图，双主题共 ${renderInfo.rendered} 份 SVG（浏览器: ${renderInfo.browserPath}）`);
  const textTotal = renderInfo.clipReport.reduce((s, r) => s + r.textCount, 0);
  const foTotal = renderInfo.clipReport.reduce((s, r) => s + r.foCount, 0);
  console.log(`  裁切检查: 通过（<text> 共 ${textTotal} 个，<foreignObject> 共 ${foTotal} 个，容差 2px）`);
  console.log(`  标题锚点合计: ${docs.reduce((s, d) => s + d.stats.headings, 0)}  表格行锚点合计: ${docs.reduce((s, d) => s + d.stats.rows, 0)}  源码链接: ${srcLinkCount}`);
  console.log(`  未链接路径: ${unlinked.size} 个（保持纯文本）`);
  if (unlinked.size) {
    const byReason = new Map();
    for (const [text, rec] of unlinked) {
      if (!byReason.has(rec.reason)) byReason.set(rec.reason, []);
      byReason.get(rec.reason).push(`${text}  ←  ${[...rec.docs].join(', ')}`);
    }
    for (const [reason, list] of byReason) {
      console.log(`    [${reason}] ${list.length} 个:`);
      for (const line of list) console.log(`      ${line}`);
    }
  }
  if (warnings.length) {
    console.log(`  降级/警告: ${warnings.length} 项`);
    for (const w of warnings) console.log('    [降级] ' + w);
  } else {
    console.log('  降级/警告: 无');
  }
  const elapsed = ((Date.now() - t0) / 1000).toFixed(1);
  console.log(`  耗时 ${elapsed}s，构建通过（全部断言 OK）`);
}

main().catch((e) => {
  console.error('构建异常:', e);
  process.exitCode = 1;
});
