// readerStyles 单测（阅读器方案 R2-5a）。纯字符串生成，node 可测。
import { describe, it, expect } from 'vitest'
import { buildReaderCSS } from './readerStyles'

const COLORS = { text: '#111', background: '#fefefe' }

describe('buildReaderCSS', () => {
  it('内联传入的主题色（iframe 不继承 app 变量，故须显式）', () => {
    const css = buildReaderCSS(COLORS)
    expect(css).toContain('color: #111')
    expect(css).toContain('background: #fefefe')
  })

  it('默认字号/行高就位', () => {
    const css = buildReaderCSS(COLORS)
    expect(css).toContain('font-size: 19px')
    expect(css).toContain('line-height: 1.75')
  })

  it('排版参数可覆盖', () => {
    const css = buildReaderCSS(COLORS, { fontSizePx: 22, lineHeight: 2, indentEm: 1 })
    expect(css).toContain('font-size: 22px')
    expect(css).toContain('line-height: 2')
    expect(css).toContain('text-indent: 1em')
  })

  it('txt 专属样式用 .sate-chapter 隔离（不波及 epub 文档）', () => {
    const css = buildReaderCSS(COLORS)
    expect(css).toContain('.sate-chapter p {')
    expect(css).toContain('.sate-chapter-title {')
    // 段落缩进/标题居中是 txt 专属，必须挂在 .sate-chapter 下
    expect(css).not.toMatch(/^p \{/m) // 不得有裸 p 规则（会污染 epub）
  })

  it('图片自适应栏宽（epub 内嵌图不溢出）', () => {
    expect(buildReaderCSS(COLORS)).toContain('max-width: 100%')
  })

  it('字体族默认衬线，可切无衬线', () => {
    expect(buildReaderCSS(COLORS)).toContain('Noto Serif CJK SC')
    const sans = buildReaderCSS(COLORS, { fontFamily: 'sans' })
    expect(sans).toContain('Noto Sans CJK SC')
    expect(sans).not.toContain('Noto Serif CJK SC')
  })

  it('自带排版（默认）不含 !important，段落样式仅 .sate-chapter 隔离', () => {
    const css = buildReaderCSS(COLORS)
    expect(css).not.toContain('!important')
    expect(css).not.toContain('body *')
  })

  it('智能排版（override）加 !important 且通配覆盖字体/行距 + 全局 p 缩进', () => {
    const css = buildReaderCSS(COLORS, {}, { override: true })
    expect(css).toContain('!important')
    // body * 通配强制字体/行距，压过 epub 书自带的高特异性样式
    expect(css).toContain('body * {')
    // 段落缩进/对齐作用于所有 p（不限 .sate-chapter），epub 段落才会被覆盖
    expect(css).toMatch(/\np \{[^}]*text-indent[^}]*!important/)
    expect(css).toContain('color: #111 !important')
  })

  it('排版全参数（R3）：字重/字距/对齐/章题缩放默认与可覆盖', () => {
    // 默认：常规字重、零字距、两端对齐、章题 1.40em
    const def = buildReaderCSS(COLORS)
    expect(def).toContain('font-weight: 400')
    expect(def).toContain('letter-spacing: 0em')
    expect(def).toContain('text-align: justify')
    expect(def).toContain('font-size: 1.40em') // 章题 = 1.4 * 1
    // 覆盖：粗体 / 0.05em 字距 / 左对齐 / 章题 1.5x（=2.10em）
    const css = buildReaderCSS(COLORS, {
      fontWeight: 700,
      letterSpacingEm: 0.05,
      textAlign: 'left',
      titleScale: 1.5,
    })
    expect(css).toContain('font-weight: 700')
    expect(css).toContain('letter-spacing: 0.05em')
    expect(css).toContain('.sate-chapter p { text-indent: 2em; margin-block: 0 0.4em; text-align: left; }')
    expect(css).toContain('font-size: 2.10em') // 1.4 * 1.5
  })

  it('段距可设置（R5-c）：默认 0.4em，可调；横竖排共用', () => {
    // 默认 0.4em（比原 0.15em 更明显）
    expect(buildReaderCSS(COLORS)).toContain('margin-block: 0 0.4em')
    // 可覆盖：0.9em
    const wide = buildReaderCSS(COLORS, { paragraphSpacingEm: 0.9 })
    expect(wide).toContain('.sate-chapter p { text-indent: 2em; margin-block: 0 0.9em; text-align: justify; }')
    // 竖排也用同一段距设置（首行缩进关）
    const v = buildReaderCSS(COLORS, { paragraphSpacingEm: 1.2 }, { vertical: true })
    expect(v).toContain('.sate-chapter p { text-indent: 0em; margin-block: 0 1.2em; text-align: justify; }')
  })

  it('竖排（R5）：段落用逻辑属性 margin-block + 关首行缩进；横排默认不变', () => {
    // 横排（默认）：首行缩进 2em + 默认 0.4em 段距（逻辑属性，与横排物理 bottom 同轴）
    const h = buildReaderCSS(COLORS)
    expect(h).toContain('.sate-chapter p { text-indent: 2em; margin-block: 0 0.4em; text-align: justify; }')
    // 竖排：首行缩进关（避免列顶错位），段距同用设置默认 0.4em
    const v = buildReaderCSS(COLORS, {}, { vertical: true })
    expect(v).toContain('.sate-chapter p { text-indent: 0em; margin-block: 0 0.4em; text-align: justify; }')
    // 逻辑属性:块级间距/边框用 margin-block / border-inline-start(两轴通吃)
    expect(v).toContain('margin-block:')
    expect(v).toContain('border-inline-start:')
    expect(v).not.toContain('margin: 0 0 0.15em') // 物理 bottom margin 已淘汰
  })

  it('智能排版（R3）：字重不压 h1..h6，对齐/字距随参数带 !important', () => {
    const css = buildReaderCSS(COLORS, { fontWeight: 700, textAlign: 'left' }, { override: true })
    // 字重仅作用于正文类元素，不含 h1..h6（标题本就该重于正文）
    expect(css).toMatch(/body :is\(p, li, div, span, td, th, blockquote\) \{ font-weight: 700 !important/)
    expect(css).toMatch(/\np \{[^}]*text-align: left !important/)
  })

  it('羊皮纸纹理（R3）：开时叠径向渐变 + fixed 附着，关时纯色底', () => {
    const plain = buildReaderCSS(COLORS)
    expect(plain).not.toContain('radial-gradient')
    expect(plain).not.toContain('background-attachment')
    const textured = buildReaderCSS(COLORS, {}, { texture: true })
    expect(textured).toContain('radial-gradient')
    expect(textured).toContain('background-attachment: fixed')
    // 纹理叠在原背景色之上（纯色底仍在，保证与对比度门禁一致）
    expect(textured).toContain('#fefefe radial-gradient')
  })
})
