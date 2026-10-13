#!/usr/bin/env node
/** 从明确列出的 Markdown 构建单文件架构导览；验收通过才发布产物。 */
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { Marked } from 'marked'
import puppeteer from 'puppeteer-core'
import { findBrowser, inspectPage, inspectSourceLinks } from './architecture-guide/checks.mjs'

const ROOT = fileURLToPath(new URL('../', import.meta.url))
const DOCS = 'docs/architecture/'
// 发布边界：不遍历目录，不跟随文档链接收集额外内容。
const INPUTS = [
  'README.md', 'pipelines.md', 'library.md', 'media.md',
  'intelligence.md', 'operations.md', 'modules.md', 'coverage.md',
]
const read = (p) => fs.readFileSync(path.join(ROOT, p), 'utf8')
const git = (...args) => execFileSync('git', args, {
  cwd: ROOT, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
}).trimEnd()
const escape = (s) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;')
  .replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;')
const json = (value) => JSON.stringify(value).replaceAll('<', '\\u003c')
const hash = (value) => crypto.createHash('sha256').update(value).digest('hex')
const plainParser = new Marked()
const plain = (value) => plainParser.parseInline(value).replace(/<[^>]*>/g, '')
  .replaceAll('&amp;', '&').replaceAll('&lt;', '<').replaceAll('&gt;', '>')
  .replaceAll('&quot;', '"').replaceAll('&#39;', "'")
function check(condition, message) {
  if (!condition) throw new Error(message)
}

// 每个 Markdown 各自计算 GitHub 风格锚点，网页只增加文档命名空间。
function slugger() {
  const used = new Set()
  return (value) => {
    const base = plain(value).toLowerCase().replace(/[^\p{L}\p{M}\p{N}_\s-]/gu, '').replace(/\s/g, '-')
    let result = base
    let n = 0
    while (used.has(result)) result = `${base}-${++n}`
    used.add(result)
    return result
  }
}

const docs = INPUTS.map((file) => {
  const raw = read(DOCS + file)
  const key = file.replace('.md', '').toLowerCase()
  const tokens = plainParser.lexer(raw)
  const slug = slugger()
  const headings = []
  let heading
  let scope
  for (const [i, token] of tokens.entries()) {
    if (token.type === 'heading') {
      token.guideId = `${key}--${slug(token.text)}`
      const next = tokens.slice(i + 1).find((t) => t.type !== 'space')
      const detail = /^[PM]\d{2}$/.test(token.text) ? next?.text?.match(/^\*\*(.*?)\*\*/)?.[1] : ''
      heading = { id: token.guideId, label: plain(token.text) + (detail ? ` · ${plain(detail)}` : ''), depth: token.depth }
      headings.push(heading)
    }
    if (token.type === 'paragraph' && token.text.startsWith('**图示范围与关系。**')) scope = token.text
    if (token.type === 'code' && token.lang === 'mermaid') {
      check(scope, `${file} / ${heading.label} 缺少图示范围与关系`)
      // 文字关系由 Mermaid 自己解析，拒绝网页专用回调和配置覆盖。
      check(!/%%\{|^\s*click\s/m.test(token.text), `${file} 图中不允许回调或初始化指令`)
      const figureNumber = tokens.slice(0, i).filter((t) => t.figure?.headingId === heading.id).length + 1
      token.figure = { id: `${heading.id}--figure-${figureNumber}`, headingId: heading.id, title: heading.label, scope, code: token.text }
      scope = null
    }
  }
  return { file, raw, key, id: `doc-${key}`, tokens, headings, title: headings[0].label, sha256: hash(raw) }
})
const byFile = new Map(docs.map((d) => [d.file, d]))
const baseline = docs[0].raw.match(/commit `([a-f0-9]{40})`/)?.[1]
check(baseline, 'README 必须声明完整源码 commit')
const head = git('rev-parse', 'HEAD')
const changes = new Set([
  ...git('diff', '--name-only', '-z', baseline).split('\0'),
  ...git('ls-files', '--others', '--exclude-standard', '-z').split('\0'),
].filter(Boolean))
const changed = (p) => [...changes].some((x) => x === p || x.startsWith(`${p}/`))
const evidence = new Map()

function resolveSource(value) {
  const match = /^([.\w/-]+?)(?:::(\S+))?$/.exec(value)
  if (!match) return null
  const file = match[1].replace(/\/$/, '')
  if (!file.includes('/') && !file.includes('.')) return null
  if (path.posix.isAbsolute(file) || file.split('/').includes('..')) return null
  // 只解析唯一的现有路径；模糊缩写和组合写法保留原文，避免错误归属。
  const candidates = [file, ...['src-tauri/src/', 'src/', 'src-tauri/'].map((p) => p + file)]
  const hits = candidates.filter((p) => fs.existsSync(path.join(ROOT, p)))
  const resolved = hits.includes(file) ? file : hits.length === 1 ? hits[0] : null
  return resolved ? { file: resolved, symbol: match[2] } : null
}

function sourceCode(value) {
  const original = `<code>${escape(value)}</code>`
  const parsed = resolveSource(value)
  if (!parsed || parsed.file.startsWith('docs/')) return original
  const { file } = parsed
  const dirty = changed(file)
  const directory = fs.statSync(path.join(ROOT, file)).isDirectory()
  // 普通本地文件不支持 GitHub 的行锚点，符号保留在引用文字中供查找。
  const href = path.posix.relative(DOCS, file).split('/').map(encodeURIComponent).join('/') + (directory ? '/' : '')
  evidence.set(value, { ...parsed, dirty, href })
  const title = dirty ? '打开当前工作区内容；相对文档基线有差异' : '打开当前工作区内容'
  const content = `<a class="source-link" href="${escape(href)}" target="_blank" rel="noopener noreferrer" title="${title}">${original}</a>`
  return `${content}${dirty ? '<small class="source-state" title="当前工作区内容相对文档基线有差异">工作区有差异</small>' : ''}`
}

function link(href, current) {
  if (/^https:\/\//.test(href)) return href
  if (href === 'index.html') return '#doc-readme'
  const [filename, fragment] = href.split('#')
  const target = filename ? byFile.get(filename.replace(/^\.\//, '')) : current
  check(target, `${current.file} 链接越出公开文档白名单：${href}`)
  return `#${fragment ? `${target.key}--${decodeURIComponent(fragment)}` : target.id}`
}

const features = []
const pipelines = []
const figures = docs.flatMap((d) => d.tokens.filter((t) => t.figure).map((t) => t.figure))
const search = []

function renderDoc(doc) {
  const parser = new Marked({ gfm: true })
  parser.use({ renderer: {
    html() { throw new Error(`${doc.file} 不允许原始 HTML`) },
    heading(token) {
      return `<h${token.depth} id="${token.guideId}" tabindex="-1">${this.parser.parseInline(token.tokens)}<a class="anchor" href="#${token.guideId}" aria-label="此节直达链接">#</a></h${token.depth}>`
    },
    link(token) {
      return `<a href="${escape(link(token.href, doc))}">${this.parser.parseInline(token.tokens)}</a>`
    },
    codespan(token) { return sourceCode(token.text) },
    code(token) {
      if (token.figure) return `<!-- ${token.figure.id} -->`
      return `<pre><code>${escape(token.text)}</code></pre>`
    },
    paragraph(token) {
      // 图注原文只在 figure 内显示一次。
      return token.text.startsWith('**图示范围与关系。**') ? '' : `<p>${this.parser.parseInline(token.tokens)}</p>`
    },
    table(token) {
      const cells = (row) => row.map((c) => this.parser.parseInline(c.tokens))
      const header = cells(token.header)
      const type = plain(token.rows[0]?.[0]?.text ?? '')[0]
      if ((doc.key === 'readme' && type === 'F') || (doc.key === 'pipelines' && type === 'P')) {
        const entries = doc.key === 'readme' ? features : pipelines
        const cards = token.rows.map((row) => {
          const html = cells(row)
          const id = plain(row[0].text)
          const anchor = `${doc.key}--${id.toLowerCase()}`
          entries.push({ id, title: plain(row[1].text), anchor })
          const title = doc.key === 'pipelines' ? `<a href="${escape(link(row[0].tokens[0].href, doc))}">${html[1]}</a>` : html[1]
          return `<section class="entry-card" id="${anchor}" data-entry="${id}"><div class="entry-id">${html[0]}<a class="anchor" href="#${anchor}" aria-label="${id} 直达链接">#</a></div><h3>${title}</h3>${html.slice(2).map((v, i) => `<div class="entry-detail"><span>${header[i + 2]}</span><div>${v}</div></div>`).join('')}</section>`
        })
        return `<div class="entry-grid">${cards.join('')}</div>`
      }
      return `<div class="table-wrap" tabindex="0" role="region" aria-label="可横向滚动的表格"><table><thead><tr>${header.map((h) => `<th>${h}</th>`).join('')}</tr></thead><tbody>${token.rows.map((row) => `<tr>${cells(row).map((c) => `<td>${c}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`
    },
  } })
  doc.html = parser.parser(doc.tokens)
  let section
  for (const token of doc.tokens) {
    if (token.type === 'heading') {
      section = { id: token.guideId, doc: doc.id, title: doc.headings.find((h) => h.id === token.guideId).label, text: '', group: doc.title }
      search.push(section)
    } else if (section && token.type !== 'space') {
      section.text += ` ${plain(token.text ?? token.raw).replace(/\s+/g, ' ')}`
    }
  }
}

async function renderFigures(page) {
  await page.setViewport({ width: 1440, height: 960 })
  await page.setContent('<!doctype html><meta charset="utf-8"><div id="diagrams"></div>')
  await page.addScriptTag({ content: read('node_modules/mermaid/dist/mermaid.min.js') })
  await page.evaluate(() => window.mermaid.initialize({
    startOnLoad: false, securityLevel: 'strict', theme: 'base',
    fontFamily: '"Segoe UI", "Microsoft YaHei", sans-serif',
    themeVariables: {
      primaryColor: '#ffffff', primaryTextColor: '#111111', primaryBorderColor: '#666666',
      lineColor: '#666666', secondaryColor: '#ffffff', tertiaryColor: '#f5f5f5',
      clusterBkg: '#fafafa', clusterBorder: '#cccccc', edgeLabelBackground: '#ffffff',
      textColor: '#111111', titleColor: '#111111', fontSize: '16px',
    },
    flowchart: { htmlLabels: false, curve: 'linear', padding: 20, nodeSpacing: 35, rankSpacing: 55, useMaxWidth: false },
  }))
  for (const fig of figures) {
    const result = await page.evaluate(async ({ id, code }) => {
      const diagram = await window.mermaid.mermaidAPI.getDiagramFromText(code)
      if (typeof diagram.db.getEdges !== 'function') throw new Error('当前导览只使用已核实的 flowchart 图')
      const nodes = diagram.db.getVertices()
      const groups = new Map(diagram.db.getSubGraphs().map((s) => [s.id, s.title]))
      const label = (key) => groups.get(key) || nodes.get(key)?.text || key
      const relations = diagram.db.getEdges().map((e) => ({ from: label(e.start), to: label(e.end), text: e.text, type: e.type }))
      const { svg } = await window.mermaid.render(`svg-${id}`, code)
      return { svg, relations }
    }, fig)
    fig.svg = result.svg.replace(/<svg /, `<svg role="img" aria-label="${escape(fig.title)}" `)
    fig.relations = result.relations.map((r) => `${r.from} ${r.type?.includes('double') ? '↔' : '→'} ${r.text ? `（${r.text}）` : ''}${r.to}`)
    check(fig.relations.length, `${fig.title} 无可读关系`)
    const html = `<figure id="${fig.id}" class="diagram" aria-labelledby="${fig.id}-title">
      <figcaption><div class="figure-heading"><strong id="${fig.id}-title">${escape(fig.title)}</strong><a class="anchor" href="#${fig.id}" aria-label="图形直达链接">#</a></div>${plainParser.parse(fig.scope)}</figcaption>
      <div class="diagram-tools" role="group" aria-label="图形缩放"><button data-zoom="out" aria-label="缩小图形">−</button><output>100%</output><button data-zoom="in" aria-label="放大图形">＋</button><button data-zoom="reset">复位</button><button data-zoom="actual">原尺寸</button><button data-zoom="expand">全屏查看</button></div>
      <div class="diagram-viewport" tabindex="0" aria-label="图形画布，可横向滚动"><div class="diagram-stage">${fig.svg}</div></div>
      <details class="reading-path"><summary>文字阅读路径 · ${fig.relations.length} 条关系</summary><ol>${fig.relations.map((r) => `<li>${escape(r)}</li>`).join('')}</ol></details>
      <details><summary>Mermaid 源码</summary><pre><code>${escape(fig.code)}</code></pre></details>
    </figure>`
    const doc = docs.find((d) => d.html.includes(`<!-- ${fig.id} -->`))
    doc.html = doc.html.replace(`<!-- ${fig.id} -->`, html)
  }
}

function assemble() {
  const css = read('scripts/architecture-guide/page.css')
  const js = read('scripts/architecture-guide/page.js')
  const modules = docs.find((d) => d.key === 'modules').headings.filter((h) => / · /.test(h.label))
  const flowIds = docs.filter((d) => ['library', 'media', 'intelligence', 'operations'].includes(d.key))
    .flatMap((d) => d.headings.filter((h) => /^P\d{2} · /.test(h.label)).map((h) => h.label.slice(0, 3)))
  check(new Set(flowIds).size === flowIds.length, '流程详情 ID 重复')
  check(JSON.stringify(flowIds.sort()) === JSON.stringify(pipelines.map((p) => p.id).sort()), '流水线索引与详情集合不一致')
  const sourceRows = [...evidence.values()].filter((v) => v.dirty)
  const metadata = {
    baseline, head, sourceLinkMode: 'relative-worktree',
    docs: docs.map((d) => ({ file: DOCS + d.file, sha256: d.sha256, modified: changed(DOCS + d.file) })),
    counts: { features: features.length, pipelines: pipelines.length, modules: modules.length, figures: figures.length },
  }
  const toc = docs.map((d, i) => `<details class="toc-group" ${i === 0 ? 'open' : ''}><summary><a href="#${d.id}"><span class="toc-number">${String(i + 1).padStart(2, '0')}</span>${escape(d.title)}</a></summary><div>${d.headings.filter((h) => h.depth > 1).map((h) => `<a href="#${h.id}">${escape(h.label)}</a>`).join('')}</div></details>`).join('')
  const content = docs.map((d) => `<article id="${d.id}" class="document" data-title="${escape(d.title)}">${d.html}</article>`).join('\n')
  const entrySearch = [...features, ...pipelines].map((e) => ({
    id: e.anchor, doc: e.id[0] === 'F' ? 'doc-readme' : 'doc-pipelines',
    title: `${e.id} · ${e.title}`, text: e.title, group: e.id[0] === 'F' ? docs[0].title : docs[1].title,
  }))
  const versions = `<details class="versions"><summary>版本与源码证据 · ${baseline.slice(0, 8)}${docs.some((d) => changed(DOCS + d.file)) ? ' · 文稿未提交' : ''}</summary><p>文档基线 <code>${baseline}</code>；构建时 HEAD <code>${head}</code>。源码通过相对路径打开当前工作区的文件或目录。文件中的符号请按引用文字查找。</p><div class="table-wrap"><table><thead><tr><th>公开输入</th><th>SHA-256</th><th>状态</th></tr></thead><tbody>${metadata.docs.map((d) => `<tr><td>${escape(d.file)}</td><td><code>${d.sha256}</code></td><td>${d.modified ? '未提交' : '已提交'}</td></tr>`).join('')}</tbody></table></div><p>以下路径的当前内容相对文档基线有差异；符号及能力限制仍以正文为准。</p><ul>${[...new Map(sourceRows.map((v) => [v.file, v])).values()].map((v) => `<li><code>${escape(v.file)}</code>：工作区有差异</li>`).join('')}</ul></details>`
  return `<!DOCTYPE html>
<!-- 自动生成；仅维护八份 Markdown。重建：npm run build:architecture-guide -->
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'">
<title>${escape(docs[0].title)}</title><style>${css}</style></head><body>
<a class="skip-link" href="#main">跳到正文</a>
<header class="topbar"><button id="menu-toggle" aria-controls="sidebar" aria-expanded="false" aria-label="打开目录">☰</button><a class="brand" href="#doc-readme"><span class="brand-mark" aria-hidden="true">S</span><span>Scrollery<small>架构导览</small></span></a><span class="edition">SOURCE ATLAS <b>${baseline.slice(0, 8)}</b></span><div class="search"><label class="sr-only" for="search">搜索架构文档</label><input id="search" type="search" placeholder="搜索能力、流程、源码…" autocomplete="off" aria-controls="search-results"><kbd>/</kbd><div id="search-results" hidden><p id="search-status" role="status"></p><div id="search-items"></div></div></header>
<button id="shade" aria-label="关闭目录" hidden></button><div class="layout"><aside id="sidebar"><div class="sidebar-label">阅读目录</div><nav aria-label="架构文档目录">${toc}</nav><a class="sidebar-note" href="#coverage--本轮验证记录">验证记录与使用边界 ↗</a></aside>
<main id="main" tabindex="-1"><div class="reading-bar"><span id="current-document">${escape(docs[0].title)}</span><span>${features.length} 功能 / ${pipelines.length} 流程 / ${modules.length} 模块</span></div><nav class="quick-nav" aria-label="主要阅读入口"><a href="#readme--功能地图">功能地图</a><a href="#readme--分层总览">分层总览</a><a href="#doc-pipelines">流水线总索引</a></nav>${content}${versions}<footer>Markdown → SVG / HTML · <a href="#readme--图形化阅读与重建">重建与来源</a></footer></main></div>
<dialog id="diagram-dialog" aria-label="放大图形"><button id="close-diagram" aria-label="关闭放大图形">关闭 · Esc</button><div id="expanded-diagram"></div></dialog>
<script id="guide-metadata" type="application/json">${json(metadata)}</script><script id="search-index" type="application/json">${json([...entrySearch, ...search])}</script><script>${js}</script></body></html>`
}

async function main() {
  for (const doc of docs) renderDoc(doc)
  const browser = await puppeteer.launch({ executablePath: findBrowser(), headless: true })
  try {
    const page = await browser.newPage()
    await renderFigures(page)
    const html = assemble()
    check(!/(?:[A-Z]:[\\/]|file:\/\/\/|\/Users\/|\/home\/)/.test(html), '产物包含本机绝对路径')
    check(!/href="[^"]*(?:docs\/tasks\/|\.\.\/tasks\/)/.test(html), '产物链接到非公开任务内容')
    const output = path.join(ROOT, DOCS, 'index.html')
    // 同目录临时文件便于失败时保留上一份可用导览。
    fs.writeFileSync(`${output}.tmp`, html)
    try {
      await page.setContent(html, { waitUntil: 'load' })
      const report = await inspectPage(page)
      const sources = await inspectSourceLinks(page, output)
      check(report.issues.length === 0, report.issues.join('\n'))
      check(sources.issues.length === 0, sources.issues.join('\n'))
      check(report.figures === figures.length, '图形丢失')
      check(report.features === features.length && report.pipelines === pipelines.length, '功能/流程条目丢失')
      fs.renameSync(`${output}.tmp`, output)
      console.log(JSON.stringify({ output: DOCS + 'index.html', bytes: Buffer.byteLength(html), ...report,
        sourceLinks: sources.links, sourceTargets: sources.targets,
        changedPaths: [...new Set([...evidence.values()].filter((v) => v.dirty).map((v) => v.file))],
      }, null, 2))
    } finally {
      if (fs.existsSync(`${output}.tmp`)) fs.unlinkSync(`${output}.tmp`)
    }
  } finally {
    await browser.close()
  }
}
main().catch((error) => { console.error(error.message); process.exitCode = 1 })
