// src/utils/syntheticBook.ts
// 合成 BookModel（阅读器完善方案 R2-4）：把 txt 接入统一的 foliate-js 渲染管线，
// 使 txt / epub 共用同一套翻页 / 排版 / 竖排 / 仿真翻页（R6）实现，退役旧 TextReader（R2-8）。
//
// foliate 的 book 契约（实读 vendored view.js / paginator.js / fb2.js 得出，见 view.d.ts）：
//   - book.sections[]：每项 { id, size, linear?, load()→url, unload?, createDocument? }
//       · load() → 章 XHTML 的 blob URL（paginator.js:1013 塞进 iframe 渲染；**可返回 Promise**
//         → 天然按章懒加载，即所需的「滑窗」，无需自建）。
//       · createDocument() → Document（仅 view.js 的搜索 / TOC 锚点用，渲染**不**走它）。
//       · size → 进度权重（SectionProgress 在 view.open() 里、任何 load 之前即需 → 用 R1 的
//         charLen 当代理，这正是 get_text_book_index 预先返回字符数的理由）。
//   - book.toc / metadata.language / dir / rendition.layout（'reflowable' → 走 paginator 而非 fxl）。
//   - splitTOCHref + getTOCFragment：view.open() 用 `if (book.splitTOCHref && book.getTOCFragment)`
//     门控进度初始化 —— 缺其一则无 relocate fraction / tocItem。
//   - resolveHref：TOC 跳章；isExternal：链接处理。
//
// 章内容按需经注入的 loadChapter（生产 = IPC get_text_chapter；版本 / 编码 / 简繁在后端 seam
// 已解析，前端只认 itemId + 章号）拉取。浏览器 API（Blob URL / DOMParser）走**依赖注入 seam**，
// 使分节 / 缓存 / 回收 / TOC 等纯逻辑在 node 环境（vitest environment=node）可单测。

import type { FoliateBook, FoliateSection } from '../vendor/foliate-js/view.js'
import type { TextBookIndex, TextChapterContent } from '../types/reader'

/** txt section 文档的 MIME：application/xhtml+xml（须良构 XML → 文本一律转义）。 */
const XHTML_MIME = 'application/xhtml+xml'

/** md section 文档的 MIME：text/html（renderMarkdown 输出含 <br>/<hr> 非自闭合，非良构 XHTML → 用宽容的 HTML 解析）。 */
const HTML_MIME = 'text/html'

/** XML/XHTML 文本转义（正文段落是外部输入，防注入 + 保证良构）。 */
export function escapeXml(s: string): string {
  // 先剥离 XML 1.0 非法的 C0 控制字符:保留制表/换行/回车(码点 9/10/13)与 >=0x20,其余(如 NUL)剔除。
  // 脏 / 误解码的 txt 若夹控制字节,会让严格的 application/xhtml+xml 整章解析成 <parsererror> 而不可读
  // (md 走宽容的 text/html 免疫,故只在此 txt 管线防御)。用码点判定而非正则字面量,规避工具管道对转义的改写。
  let cleaned = ''
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    if (c >= 0x20 || c === 9 || c === 10 || c === 13) cleaned += s[i]
  }
  return cleaned
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;')
}

/**
 * 把一章（标题 + 段落）构造为良构 XHTML 字符串。
 * txt：标题 → `<h1>`，每段 → `<p>`（文本转义）。缩进 / 段间距由渲染层 CSS（R2-5）施加，不在此加空白。
 * 段落文本是 canonical（替换规则 / 简繁转换是 DOM 层后置变换，不改 canonical）。
 * `data-sate-chapter` 标记章 section 本体。
 */
export function buildChapterXHTML(chapter: TextChapterContent, opts?: { lang?: string }): string {
  const lang = opts?.lang ?? 'zh'
  const heading = chapter.title
    ? `<h1 class="sate-chapter-title">${escapeXml(chapter.title)}</h1>`
    : ''
  const body = chapter.paragraphs.map((p) => `<p>${escapeXml(p)}</p>`).join('\n')
  // xml:lang + lang 双写：foliate languageInfo 读 metadata.language 判 CJK；文档级 lang 供断行 / 字体回落。
  return `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="${lang}" lang="${lang}">
<head><meta charset="utf-8"/><title>${escapeXml(chapter.title)}</title></head>
<body><section class="sate-chapter" data-sate-chapter="">
${heading}
${body}
</section></body>
</html>`
}

/** buildTextSyntheticBook 入参。浏览器 API 三件套默认取全局，测试注入假实现。 */
export interface TextSyntheticBookOptions {
  /** get_text_book_index 的返回（编码 / 置信度 / 章元数据）。 */
  index: TextBookIndex
  /** 按章拉取内容（生产 = IPC get_text_chapter；测试 = stub）。可异步 → foliate 懒加载。 */
  loadChapter: (chapterIndex: number) => Promise<TextChapterContent>
  /** 语言（默认 'zh'；影响 foliate CJK 处理与文档 lang 属性）。 */
  lang?: string
  /** 章 XHTML 字符串 → 可加载 URL（默认 new Blob → URL.createObjectURL；测试注入假 URL）。 */
  createObjectURL?: (xhtml: string) => string
  /** 回收 URL（默认 URL.revokeObjectURL）。 */
  revokeObjectURL?: (url: string) => void
  /** 章 XHTML 字符串 → Document（默认 new DOMParser；仅搜索 / TOC 用）。 */
  parseDocument?: (xhtml: string) => Document
}

/**
 * 构造 txt 的合成 BookModel。章 = section；section.load() 按需拉章并转 XHTML blob URL（缓存，
 * unload / destroy 回收，防泄漏）。进度 / 定位交给 foliate 原生 CFI（合成 book 走 fake-CFI，
 * 确定性 DOM → 跨重启可恢复）。
 */
export function buildTextSyntheticBook(opts: TextSyntheticBookOptions): FoliateBook {
  const {
    index,
    loadChapter,
    lang = 'zh',
    createObjectURL = (s: string) => URL.createObjectURL(new Blob([s], { type: XHTML_MIME })),
    revokeObjectURL = (u: string) => URL.revokeObjectURL(u),
    parseDocument = (s: string) => new DOMParser().parseFromString(s, XHTML_MIME),
  } = opts

  // 每章 blob URL 缓存：load 建、unload / destroy 回收。Map<章号, url>。
  const urlCache = new Map<number, string>()

  const buildXHTML = async (i: number): Promise<string> =>
    buildChapterXHTML(await loadChapter(i), { lang })

  const sections: FoliateSection[] = index.chapters.map((ch, i) => ({
    id: i, // 数字 id（同 fb2）；与 splitTOCHref 返回值同类型可比（见下）。
    size: ch.charLen, // 进度权重代理（SectionProgress 开卷前即需，无法待 load）。
    linear: undefined, // 全部线性正文。
    load: async () => {
      const cached = urlCache.get(i)
      if (cached) return cached
      const url = createObjectURL(await buildXHTML(i))
      urlCache.set(i, url)
      return url
    },
    unload: () => {
      const url = urlCache.get(i)
      if (url) {
        revokeObjectURL(url)
        urlCache.delete(i)
      }
    },
    // 搜索 / getTOCItemOf 用；渲染不走它，故不共享 load 的 URL 缓存（按需重解析可接受）。
    createDocument: async () => parseDocument(await buildXHTML(i)),
  }))

  return {
    sections,
    // 扁平 TOC：一章一项。空标题兜底为「第 N 章」序号，避免 TOC 出现空条目。
    toc: index.chapters.map((ch, i) => ({
      label: ch.title || `${i + 1}`,
      href: String(i),
    })),
    metadata: { language: lang },
    dir: 'ltr', // 竖排（R5）经 writing-mode 施加，不动 dir（dir 是 RTL 语言用）。
    rendition: { layout: 'reflowable' }, // 明确 reflowable → view.open 选 paginator。
    // href 形如 "3" 或 "3#frag"；扁平 TOC 无 frag。返回 [章号, NaN?]，[0] 与 section.id 同为数字可比。
    splitTOCHref: (href: string) => href?.split('#')?.map(Number) ?? [],
    // 扁平 TOC 无子锚点，getFragment 实际不被 TOCProgress 以有效 id 调用；返回根元素兜底（同 comic-book）。
    getTOCFragment: (doc: Document) => doc.documentElement,
    // TOC 跳章：href 即章号；无 anchor → paginator 落章首（scrollToAnchor(anchor ?? 0)）。
    resolveHref: (href: string) => ({ index: Number(href) }),
    isExternal: (uri: string) => /^\w+:/i.test(uri),
    destroy: () => {
      for (const url of urlCache.values()) revokeObjectURL(url)
      urlCache.clear()
    },
  }
}

/**
 * 把 renderMarkdown 产出的 HTML 片段包成完整 text/html 文档。
 * 外层 `.sate-md` class = md 专属排版 CSS 的挂钩（与 txt 的 `.sate-chapter` 并列，选择器互斥不干扰）。
 * bodyHtml 由 renderMarkdown 产出（已 HTML 转义、协议白名单，安全），此处不再转义。
 */
export function buildMarkdownHTML(bodyHtml: string, opts?: { lang?: string }): string {
  const lang = opts?.lang ?? 'zh'
  return `<!DOCTYPE html>
<html lang="${lang}">
<head><meta charset="utf-8"/></head>
<body><div class="sate-md">${bodyHtml}</div></body>
</html>`
}

/**
 * 单 section 的 HTML 字符预算(2026-07-17 内存爆炸修复,D-001)。
 * **section 大小是硬约束**:foliate paginated 对整个 section 上 CSS multicol,开卷期的全文档
 * 几何查询(getVisibleRange 逐文本节点 Range+rect、expand 的整文 contentRange rect)随
 * fragmentainer 数乘积放大 —— 实测 1.9M 字符单 section 即 2.5GB + 主线程阻塞 5min 未完成,
 * 8.8MB 达 7.9GB(测量阶梯见 docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md)。
 * 24K 对齐 txt 伪章 PSEUDO_CHARS(10K 解码字符)量级,单片布局瞬时完成;新格式接入 foliate
 * 一律须带同类分片。
 */
export const MD_SECTION_BUDGET_CHARS = 24_000

/**
 * 超限单片强制 scrolled 的阈值(2026-07-17 内存爆炸修复 D-002;2026-07-18 审查 F-04 推广到 txt)。
 * 分片后仍可能留下超限单片:md 单块不内切(巨型代码围栏独占一片);txt 分章只在**行边界**续切
 * (text_index.rs push_chapter_capped / pseudo_chapters),单行巨串(minified JSON/base64/单行日志)
 * 整行独占一章,30K 上限对它失效。paginated 对超大 section 上 multicol 是实测过的灾难
 * (1.9M 字符 → 2.5GB + 主线程阻塞 5min);同内容 scrolled 流完全健康。存在超限片即强制
 * scrolled,由 BookReader 在 open 前置位。
 */
export const FORCE_SCROLLED_SECTION_CHARS = 256_000

/** 任一 section 超过阈值 → 须强制 scrolled(size 缺省当 0:未知大小不触发护栏)。 */
export function hasOversizedSection(
  sections: FoliateSection[],
  limit: number = FORCE_SCROLLED_SECTION_CHARS,
): boolean {
  return sections.some((s) => (s.size ?? 0) > limit)
}

/** 分组后的 md 片:html = 该片块序列(\n 连接);tocLabel = 片首 heading 文本(无则 null)。 */
export interface MarkdownSection {
  html: string
  tocLabel: string | null
}

// 块级 heading 识别:renderMarkdownBlocks 产物无属性,形如 <h1>…</h1>。h1/h2 触发分片(章级语义);
// TOC label 取 h1–h3(h3 不触发分片,但作为片首时仍是最好的目录文案)。
const SPLIT_HEADING_RE = /^<h[12]>/
const LABEL_HEADING_RE = /^<h([1-3])>([\s\S]*?)<\/h\1>/

/** heading HTML → TOC 纯文本:剥行内标签 + 反转义(顺序与 escapeHtml 相反,&amp; 最后)。 */
function headingText(html: string): string {
  return html
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
    .trim()
}

/**
 * 按块边界把 renderMarkdownBlocks 产物分组为 section(导出供单测):
 * ① h1/h2 块起新片(md 的天然章界,heading 派生 TOC 即 R2-4b);
 * ② 预算超限起新片(无 heading 的日志型 md 兜底);
 * ③ 单块超预算独占一片,**不内切**(切开 `<pre>`/`<ul>` 会破坏块语义;块本身已由
 *    renderMarkdownBlocks 的 MAX_PARA_LINES 断长跑约束,仅巨型代码围栏可能超限,交护栏兜底)。
 * 空输入产出一个空片(foliate 契约:sections 至少一项)。
 */
export function groupMarkdownBlocks(blocks: string[]): MarkdownSection[] {
  const sections: MarkdownSection[] = []
  let buf: string[] = []
  let size = 0
  const flush = () => {
    if (buf.length) {
      const label = LABEL_HEADING_RE.exec(buf[0])
      sections.push({ html: buf.join('\n'), tocLabel: label ? headingText(label[2]) : null })
      buf = []
      size = 0
    }
  }
  for (const block of blocks) {
    if (SPLIT_HEADING_RE.test(block) || size + block.length > MD_SECTION_BUDGET_CHARS) flush()
    buf.push(block)
    size += block.length
  }
  flush()
  if (!sections.length) sections.push({ html: '', tocLabel: null })
  return sections
}

/** buildMarkdownSyntheticBook 入参。浏览器 API 三件套默认取全局，测试注入假实现。 */
export interface MarkdownSyntheticBookOptions {
  /** renderMarkdownBlocks 产出的块级 HTML 数组（安全，已转义 + 协议白名单）。 */
  blocks: string[]
  lang?: string
  createObjectURL?: (html: string) => string
  revokeObjectURL?: (url: string) => void
  parseDocument?: (html: string) => Document
}

/**
 * 构造 md 的合成 BookModel（2026-07-17 起多 section）：块序列按 groupMarkdownBlocks 分片,
 * 一片 = 一个 section,foliate 按片懒加载 → paginated 只 columnize 当前片,布局成本有界。
 * heading 起头的片产 TOC 项(href = 片号,与 resolveHref/splitTOCHref 数字契约同 txt 路径)。
 */
export function buildMarkdownSyntheticBook(opts: MarkdownSyntheticBookOptions): FoliateBook {
  const {
    blocks,
    lang = 'zh',
    createObjectURL = (s: string) => URL.createObjectURL(new Blob([s], { type: HTML_MIME })),
    revokeObjectURL = (u: string) => URL.revokeObjectURL(u),
    parseDocument = (s: string) => new DOMParser().parseFromString(s, HTML_MIME),
  } = opts
  const parts = groupMarkdownBlocks(blocks)

  // 每片 blob URL 缓存:load 建、unload/destroy 回收(与 txt 章缓存同款生命周期)。
  const urlCache = new Map<number, string>()

  const sections: FoliateSection[] = parts.map((part, i) => {
    const doc = () => buildMarkdownHTML(part.html, { lang })
    return {
      id: i,
      size: part.html.length, // 进度权重(SectionProgress 开卷前即需,HTML 长度当代理)。
      linear: undefined,
      load: () => {
        const cached = urlCache.get(i)
        if (cached) return cached
        const url = createObjectURL(doc())
        urlCache.set(i, url)
        return url
      },
      unload: () => {
        const url = urlCache.get(i)
        if (url) {
          revokeObjectURL(url)
          urlCache.delete(i)
        }
      },
      createDocument: async () => parseDocument(doc()),
    }
  })

  return {
    sections,
    // heading 派生 TOC(R2-4b):仅 heading 起头的片入目录;全无 heading(日志型)→ 空目录同旧。
    toc: parts.flatMap((p, i) => (p.tocLabel ? [{ label: p.tocLabel, href: String(i) }] : [])),
    metadata: { language: lang },
    dir: 'ltr',
    rendition: { layout: 'reflowable' },
    splitTOCHref: (href: string) => href?.split('#')?.map(Number) ?? [],
    getTOCFragment: (d: Document) => d.documentElement,
    resolveHref: (href: string) => ({ index: Number(href) || 0 }),
    isExternal: (uri: string) => /^\w+:/i.test(uri),
    destroy: () => {
      for (const url of urlCache.values()) revokeObjectURL(url)
      urlCache.clear()
    },
  }
}
