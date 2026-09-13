// syntheticBook 单测（阅读器方案 R2-4）。vitest environment=node，故浏览器 API（Blob URL /
// DOMParser）走注入的假实现 —— 由此可在 node 下验证「易漏的资源生命周期」：load 缓存、unload 回收、
// destroy 清理，以及 foliate book 契约的纯数据面（sections / toc / splitTOCHref 与 id 可比等）。
import { describe, it, expect } from 'vitest'
import {
  escapeXml,
  buildChapterXHTML,
  buildTextSyntheticBook,
  buildMarkdownHTML,
  buildMarkdownSyntheticBook,
  groupMarkdownBlocks,
  MD_SECTION_BUDGET_CHARS,
  FORCE_SCROLLED_SECTION_CHARS,
  hasOversizedSection,
} from './syntheticBook'
import type { TextBookIndex, TextChapterContent } from '../types/reader'

// ── 测试替身 ────────────────────────────────────────────────────────────────
const INDEX: TextBookIndex = {
  encoding: 'UTF-8',
  confidence: 'detected',
  chapters: [
    { title: '第一章 起', charLen: 1200 },
    { title: '第二章 承', charLen: 3400 },
    { title: '', charLen: 0 }, // 空标题 + 空章：验兜底 label 与 size=0
  ],
}

const CHAPTERS: TextChapterContent[] = [
  { title: '第一章 起', paragraphs: ['风起于青萍之末。', '浪成于微澜之间。'] },
  { title: '第二章 承', paragraphs: ['a < b && c > d 需转义。'] },
  { title: '', paragraphs: [] },
]

/** 建一个带假浏览器 API seam 的 book + 探针（记录 load / revoke 调用）。 */
function makeHarness() {
  const calls = { loadChapter: [] as number[], created: [] as string[], revoked: [] as string[] }
  let seq = 0
  const book = buildTextSyntheticBook({
    index: INDEX,
    loadChapter: async (i) => {
      calls.loadChapter.push(i)
      return CHAPTERS[i]
    },
    createObjectURL: (xhtml) => {
      const url = `blob:fake:${seq++}`
      calls.created.push(xhtml)
      return url
    },
    revokeObjectURL: (u) => calls.revoked.push(u),
    // parseDocument 不在这些测试里触发（不测 createDocument 的解析结果，node 无 DOMParser）。
    parseDocument: () => ({}) as unknown as Document,
  })
  return { book, calls }
}

describe('escapeXml', () => {
  it('转义 & < > " 与单引号', () => {
    expect(escapeXml(`a<b>&"'`)).toBe('a&lt;b&gt;&amp;&quot;&apos;')
  })
  it('无特殊字符原样返回', () => {
    expect(escapeXml('普通文字 abc 123')).toBe('普通文字 abc 123')
  })
  it('剥离 C0 控制字符,保留合法空白与标点(防误删回归)', () => {
    // 用 fromCharCode 构造控制字节,避免源码里的转义被工具管道改写。
    const NUL = String.fromCharCode(0)
    const FF = String.fromCharCode(12) // form feed 0x0C
    const SOH = String.fromCharCode(1)
    const TAB = String.fromCharCode(9)
    const LF = String.fromCharCode(10)
    const CR = String.fromCharCode(13)
    // XML 1.0 非法控制字节被剔除(否则严格 XHTML 整章 parsererror,章不可读)
    expect(escapeXml(`a${NUL}b${FF}c${SOH}`)).toBe('abc')
    // 合法空白(制表/换行/回车)必须保留
    expect(escapeXml(`x${TAB}${LF}${CR}y`)).toBe(`x${TAB}${LF}${CR}y`)
    // 关键回归防线:空格与常见标点绝不能被误删(坏正则曾误删 0x20-0x2D 段)
    expect(escapeXml('a b - c , d !')).toBe('a b - c , d !')
  })
})

describe('buildChapterXHTML', () => {
  it('标题 → h1，每段 → p，且正文转义', () => {
    const xhtml = buildChapterXHTML(CHAPTERS[1])
    expect(xhtml).toContain('<h1 class="sate-chapter-title">第二章 承</h1>')
    // 正文里的 < > & 必须被转义（否则 XHTML 不良构，DOMParser 报 parsererror）
    expect(xhtml).toContain('<p>a &lt; b &amp;&amp; c &gt; d 需转义。</p>')
    expect(xhtml).not.toContain('a < b')
  })
  it('空标题不产出 h1', () => {
    const xhtml = buildChapterXHTML(CHAPTERS[2])
    expect(xhtml).not.toContain('<h1')
  })
  it('产出 XML 声明 + xhtml 命名空间 + 章标记钩子', () => {
    const xhtml = buildChapterXHTML(CHAPTERS[0])
    expect(xhtml.startsWith('<?xml')).toBe(true)
    expect(xhtml).toContain('xmlns="http://www.w3.org/1999/xhtml"')
    expect(xhtml).toContain('data-sate-chapter=""')
  })
  it('lang 可覆盖', () => {
    expect(buildChapterXHTML(CHAPTERS[0], { lang: 'en' })).toContain('lang="en"')
  })
})

describe('buildTextSyntheticBook：foliate 契约纯数据面', () => {
  it('sections 与章一一对应，size = charLen，id 为章号', () => {
    const { book } = makeHarness()
    expect(book.sections.length).toBe(3)
    expect(book.sections.map((s) => s.size)).toEqual([1200, 3400, 0])
    expect(book.sections.map((s) => s.id)).toEqual([0, 1, 2])
    for (const s of book.sections) {
      expect(typeof s.load).toBe('function')
      expect(typeof s.unload).toBe('function')
      expect(typeof s.createDocument).toBe('function')
    }
  })

  it('toc：标题 → label（空标题兜底序号），href 为章号字符串', () => {
    const { book } = makeHarness()
    expect(book.toc).toEqual([
      { label: '第一章 起', href: '0' },
      { label: '第二章 承', href: '1' },
      { label: '3', href: '2' }, // 空标题 → "N"（i+1）
    ])
  })

  it('splitTOCHref 返回值 [0] 与 section.id 同为数字可比（TOC 分组的前提）', () => {
    const { book } = makeHarness()
    const [id] = book.splitTOCHref!('2') as number[]
    expect(id).toBe(2)
    expect(book.sections[2].id).toBe(id) // 同值同类型 → TOCProgress 能对上
  })

  it('resolveHref 把章号 href 解析为 index', () => {
    const { book } = makeHarness()
    expect(book.resolveHref!('1')).toEqual({ index: 1 })
  })

  it('进度初始化门控：splitTOCHref 与 getTOCFragment 均就位', () => {
    const { book } = makeHarness()
    // view.open 用 `if (book.splitTOCHref && book.getTOCFragment)` 决定是否建 SectionProgress/TOCProgress
    expect(Boolean(book.splitTOCHref && book.getTOCFragment)).toBe(true)
  })

  it('reflowable + ltr + 语言（确保走 paginator 而非 fxl）', () => {
    const { book } = makeHarness()
    expect(book.rendition?.layout).toBe('reflowable')
    expect(book.dir).toBe('ltr')
    expect(book.metadata?.language).toBe('zh')
  })

  it('isExternal 区分外链与内部章号', () => {
    const { book } = makeHarness()
    expect(book.isExternal!('https://example.com')).toBe(true)
    expect(book.isExternal!('mailto:a@b.c')).toBe(true)
    expect(book.isExternal!('2')).toBe(false)
  })
})

describe('buildTextSyntheticBook：资源生命周期（load 缓存 / unload 回收 / destroy 清理）', () => {
  it('load 首次拉章建 URL，二次命中缓存（不重复拉章）', async () => {
    const { book, calls } = makeHarness()
    const url1 = await book.sections[0].load!()
    const url2 = await book.sections[0].load!()
    expect(url1).toBe(url2)
    expect(calls.loadChapter).toEqual([0]) // 只拉了一次
    expect(calls.created.length).toBe(1)
  })

  it('unload 回收 URL 并清缓存 → 再 load 重新拉章建新 URL', async () => {
    const { book, calls } = makeHarness()
    const url1 = await book.sections[0].load!()
    book.sections[0].unload!()
    expect(calls.revoked).toEqual([url1])
    const url2 = await book.sections[0].load!()
    expect(url2).not.toBe(url1) // 新 URL
    expect(calls.loadChapter).toEqual([0, 0]) // 重新拉了一次
  })

  it('destroy 回收所有已建 URL', async () => {
    const { book, calls } = makeHarness()
    const u0 = await book.sections[0].load!()
    const u1 = await book.sections[1].load!()
    book.destroy!()
    expect(calls.revoked.sort()).toEqual([u0, u1].sort())
  })

  it('load 注入的 XHTML 内容与章文本对应（转义生效）', async () => {
    const { book, calls } = makeHarness()
    await book.sections[1].load!()
    // createObjectURL 收到的字符串应含转义后的正文
    expect(calls.created[0]).toContain('a &lt; b &amp;&amp; c &gt; d')
  })
})

describe('buildMarkdownHTML', () => {
  it('包成 text/html 文档并挂 .sate-md（md 排版 CSS 的钩子）', () => {
    const html = buildMarkdownHTML('<h1>标题</h1><p>正文</p>')
    expect(html.startsWith('<!DOCTYPE html>')).toBe(true)
    expect(html).toContain('<div class="sate-md"><h1>标题</h1><p>正文</p></div>')
  })
  it('lang 可覆盖', () => {
    expect(buildMarkdownHTML('x', { lang: 'en' })).toContain('lang="en"')
  })
})

describe('groupMarkdownBlocks：分片规则（2026-07-17 内存爆炸修复）', () => {
  it('h1/h2 块起新片,h3 不切;片首 heading 成 TOC label', () => {
    const parts = groupMarkdownBlocks([
      '<h1>甲</h1>',
      '<p>a</p>',
      '<h3>小节</h3>',
      '<p>b</p>',
      '<h2>乙</h2>',
      '<p>c</p>',
    ])
    expect(parts.map((p) => p.tocLabel)).toEqual(['甲', '乙'])
    expect(parts[0].html).toBe('<h1>甲</h1>\n<p>a</p>\n<h3>小节</h3>\n<p>b</p>')
    expect(parts[1].html).toBe('<h2>乙</h2>\n<p>c</p>')
  })

  it('无 heading 的日志型内容按预算切片,label 全空', () => {
    // 每块 7007 字符(含 <p></p> 开销)→ 每片装 3 块(21021 ≤ 24000,第 4 块超限起新片)。
    const block = `<p>${'x'.repeat(7000)}</p>`
    const parts = groupMarkdownBlocks(Array.from({ length: 7 }, () => block))
    expect(parts.length).toBe(3) // 3+3+1
    expect(parts.every((p) => p.tocLabel === null)).toBe(true)
    expect(parts.every((p) => p.html.length <= MD_SECTION_BUDGET_CHARS + block.length)).toBe(true)
  })

  it('单块超预算独占一片,不内切(块语义完整)', () => {
    const giant = `<pre><code>${'y'.repeat(MD_SECTION_BUDGET_CHARS * 2)}</code></pre>`
    const parts = groupMarkdownBlocks(['<p>前</p>', giant, '<p>后</p>'])
    expect(parts.length).toBe(3)
    expect(parts[1].html).toBe(giant)
  })

  it('TOC label 剥行内标签并反转义', () => {
    const parts = groupMarkdownBlocks(['<h1>A &amp; B <code>c&lt;d</code></h1>'])
    expect(parts[0].tocLabel).toBe('A & B c<d')
  })

  it('空输入产出一个空片(foliate 契约 ≥1 section)', () => {
    expect(groupMarkdownBlocks([])).toEqual([{ html: '', tocLabel: null }])
  })
})

describe('buildMarkdownSyntheticBook：md 多 section(2026-07-17 起)', () => {
  function makeMdHarness(blocks: string[] = ['<h1>H</h1>', '<p>body</p>']) {
    const calls = { created: [] as string[], revoked: [] as string[] }
    let seq = 0
    const book = buildMarkdownSyntheticBook({
      blocks,
      createObjectURL: (s) => {
        calls.created.push(s)
        return `blob:md:${seq++}`
      },
      revokeObjectURL: (u) => calls.revoked.push(u),
      parseDocument: () => ({}) as unknown as Document,
    })
    return { book, calls }
  }

  it('单 heading 起头 → 一个 section + 一条 TOC(href=片号,数字契约同 txt)', () => {
    const { book } = makeMdHarness()
    expect(book.sections.length).toBe(1)
    expect(book.toc).toEqual([{ label: 'H', href: '0' }])
    const [id] = book.splitTOCHref!('0') as number[]
    expect(book.sections[0].id).toBe(id)
    expect(book.resolveHref!('0')).toEqual({ index: 0 })
  })

  it('多 heading → 多 section,size = 片 HTML 长度(进度权重)', () => {
    const { book } = makeMdHarness(['<h1>一</h1>', '<p>a</p>', '<h2>二</h2>', '<p>bb</p>'])
    expect(book.sections.length).toBe(2)
    expect(book.toc).toEqual([
      { label: '一', href: '0' },
      { label: '二', href: '1' },
    ])
    expect(book.sections[0].size).toBe('<h1>一</h1>\n<p>a</p>'.length)
    expect(book.sections[1].size).toBe('<h2>二</h2>\n<p>bb</p>'.length)
  })

  it('无 heading → toc 为空(与旧单 section 行为一致)', () => {
    const { book } = makeMdHarness(['<p>x</p>'])
    expect(book.toc).toEqual([])
  })

  it('reflowable + 进度门控就位', () => {
    const { book } = makeMdHarness()
    expect(book.rendition?.layout).toBe('reflowable')
    expect(Boolean(book.splitTOCHref && book.getTOCFragment)).toBe(true)
  })

  it('load 建 URL 且各片独立包 .sate-md;缓存命中不重复建', async () => {
    const { book, calls } = makeMdHarness(['<h1>一</h1>', '<h2>二</h2>'])
    const u1 = await book.sections[0].load!()
    const u1b = await book.sections[0].load!()
    const u2 = await book.sections[1].load!()
    expect(u1).toBe(u1b)
    expect(u1).not.toBe(u2)
    expect(calls.created.length).toBe(2)
    expect(calls.created[0]).toContain('<div class="sate-md"><h1>一</h1></div>')
    expect(calls.created[1]).toContain('<div class="sate-md"><h2>二</h2></div>')
  })

  it('unload 回收该片 URL,再 load 建新 URL', async () => {
    const { book, calls } = makeMdHarness(['<h1>一</h1>', '<h2>二</h2>'])
    const u1 = await book.sections[0].load!()
    book.sections[0].unload!()
    expect(calls.revoked).toEqual([u1])
    const u1b = await book.sections[0].load!()
    expect(u1b).not.toBe(u1)
  })

  it('destroy 回收所有已建 URL', async () => {
    const { book, calls } = makeMdHarness(['<h1>一</h1>', '<h2>二</h2>'])
    const u1 = await book.sections[0].load!()
    const u2 = await book.sections[1].load!()
    book.destroy!()
    expect(calls.revoked.sort()).toEqual([u1, u2].sort())
  })
})

describe('hasOversizedSection：超限单片强制 scrolled 护栏(审查 F-04)', () => {
  it('常规多章 txt book 不触发护栏', () => {
    const { book } = makeHarness()
    expect(hasOversizedSection(book.sections)).toBe(false)
  })

  it('单行巨串场景:后端单章 charLen 超阈值 → 触发(txt 分章行边界局限的前端兜底)', () => {
    // 模拟 text_index 对无换行巨串的产出:单章,char_len 如实上报超限。
    const giantIndex: TextBookIndex = {
      encoding: 'UTF-8',
      confidence: 'detected',
      chapters: [{ title: '片段 1', charLen: FORCE_SCROLLED_SECTION_CHARS + 1 }],
    }
    const book = buildTextSyntheticBook({
      index: giantIndex,
      loadChapter: async () => ({ title: '片段 1', paragraphs: [] }),
      createObjectURL: () => 'blob:fake',
      revokeObjectURL: () => {},
      parseDocument: () => ({}) as unknown as Document,
    })
    expect(hasOversizedSection(book.sections)).toBe(true)
  })

  it('恰好等于阈值不触发(> 语义,与 md 分支既有判定一致)', () => {
    expect(hasOversizedSection([{ id: 0, size: FORCE_SCROLLED_SECTION_CHARS } as never])).toBe(
      false,
    )
  })

  it('size 缺省当 0:未知大小不触发', () => {
    expect(hasOversizedSection([{ id: 0 } as never])).toBe(false)
  })

  it('自定义阈值生效', () => {
    expect(hasOversizedSection([{ id: 0, size: 10 } as never], 5)).toBe(true)
  })
})
