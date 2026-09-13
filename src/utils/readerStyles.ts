// src/utils/readerStyles.ts
// 阅读器排版基线（阅读器完善方案 R2-5a）。生成注入 foliate section 文档的默认排版 CSS。
//
// 注入策略（见 paginator.js setStyles）：放入「before」槽（$beforeStyle，prepend 到 doc.head）=
// 低优先默认。对 txt（合成 book，文档无自带 CSS）全量生效；对 epub（书自带排版）经 cascade 书样式
// 胜出 → **绝不回归 epub 既有观感**。txt 专属的段首缩进 / 段间距 / 章标题样式用 `.sate-chapter`
// 选择器隔离（epub 文档无此 class，故不受影响）。「用户偏好强制覆盖书样式」属 R2-6（走 $style 高优先槽）。
//
// section doc 在 iframe，不继承 app 的 CSS 变量 → 颜色须由调用方读取当前主题解析值后内联传入。
// 纯函数、无 DOM 依赖，node 环境可单测。

/** 阅读页颜色（自当前主题的 CSS 变量解析而来）。 */
export interface ReaderColors {
  /** 正文颜色（--color-text-primary）。 */
  text: string
  /** 页面背景（--color-bg-primary）。 */
  background: string
}

/** 排版参数。R2-5a 用合理默认；R2-6 设置面板将其做成可调旋钮并写每书 prefs。 */
export type ReaderFontFamily = 'serif' | 'sans'

/** 正文对齐（R3）：justify 两端对齐（贴近纸书，默认）/ left 左对齐（避免 CJK 两端对齐拉大字距）。 */
export type ReaderTextAlign = 'justify' | 'left'

export interface ReaderTypography {
  /** 正文字号（px）。 */
  fontSizePx?: number
  /** 行高倍数。 */
  lineHeight?: number
  /** 段首缩进（em）；CJK 阅读惯例每段缩进 2 字。仅作用于 txt（.sate-chapter）。 */
  indentEm?: number
  /** 字体族（默认 serif；R2-6b）。 */
  fontFamily?: ReaderFontFamily
  /** 段距（R5-c）：段落之间的块末间距（em，走 margin-block-end，横竖排两轴通吃）。缺省 0.4。 */
  paragraphSpacingEm?: number
  /** 字重（R3）：CSS font-weight 数值（400 常规 / 500 中 / 700 粗）。缺省 400。 */
  fontWeight?: number
  /** 字距（R3）：letter-spacing，em 单位（0..0.15 舒适区）。缺省 0。 */
  letterSpacingEm?: number
  /** 正文对齐（R3）。缺省 justify。 */
  textAlign?: ReaderTextAlign
  /** 章题缩放（R3「章题独立」）：章标题字号相对基准 1.4em 的倍数（0.8..2.0）。缺省 1。仅 txt（.sate-chapter-title）。 */
  titleScale?: number
}

/** 阅读正文字体栈：CJK 优先，回落西文与系统字体。衬线贴近纸书，无衬线屏幕更清晰。 */
const FONT_STACKS: Record<ReaderFontFamily, string> = {
  serif:
    '"Noto Serif CJK SC", "Source Han Serif SC", "Songti SC", STSong, SimSun, Georgia, "Times New Roman", serif',
  sans: '"Noto Sans CJK SC", "Source Han Sans SC", "PingFang SC", "Microsoft YaHei", "Hiragino Sans GB", system-ui, sans-serif',
}

/** buildReaderCSS 选项。 */
export interface ReaderCSSOptions {
  /**
   * 智能排版（覆盖出版方样式）。true → body 排版属性加 `!important`，并追加 `body *` 字体/行距
   * 通配覆盖 + 全局 `p` 缩进/对齐，**压过 epub 书自带的高特异性 CSS**（注入 foliate 的 after 槽）。
   * false（默认）→ 无 !important、段落样式仅 `.sate-chapter`（txt）隔离，供注入 before 槽让书样式胜出。
   */
  override?: boolean
  /**
   * 羊皮纸纹理（R3）：在纯色背景上叠一层极淡的暖色斑驳（纯 CSS 多重径向渐变，无外部资源/无 data-URI，
   * CSP 安全）。仅在阅读主题标 texture 时开；渐变透明度极低（不干扰阅读、不压对比度门禁——门禁只测纯色对）。
   */
  texture?: boolean
  /**
   * 竖排（R5）：段落规则改用 CSS 逻辑属性（margin-block / border-inline 等），使段距落在**块轴**（竖排下
   * 即段与段之间的横向间隔）而非物理 bottom（竖排下会落到列的下端 = 不分段）；并**关闭首行缩进**改用清晰
   * 段距分段——首行缩进(text-indent)在竖排会把新段起始列顶端下压 ~2 字，造成列错位（真机实证）。横排（默认
   * false）行为逐像素不变。writing-mode 本身由 BookReader onLoad 注入文档根，不在此。
   */
  vertical?: boolean
}

/**
 * 构造阅读器排版 CSS 字符串。
 * - **自带排版**（override=false，注入 before 槽/低优先）：全局项填空缺，epub 书样式经 cascade 胜出；
 *   段落/标题排版仅作用于 txt（`.sate-chapter` 隔离）。
 * - **智能排版**（override=true，注入 after 槽/高优先）：`!important` + `body *` 通配强制统一字体/行距/
 *   颜色，段落缩进/对齐作用于所有 `p` → 覆盖排版差/无排版的 epub。
 */
export function buildReaderCSS(
  colors: ReaderColors,
  typo: ReaderTypography = {},
  opts: ReaderCSSOptions = {},
): string {
  const override = opts.override ?? false
  const imp = override ? ' !important' : ''
  const fontSizePx = typo.fontSizePx ?? 19
  const lineHeight = typo.lineHeight ?? 1.75
  const indentEm = typo.indentEm ?? 2
  const fontStack = FONT_STACKS[typo.fontFamily ?? 'serif']
  // R3 排版全参数：字重 / 字距 / 对齐 / 章题缩放（各有默认，缺省即 R2 观感，零回归）。
  const fontWeight = typo.fontWeight ?? 400
  const letterSpacingEm = typo.letterSpacingEm ?? 0
  const textAlign = typo.textAlign ?? 'justify'
  const titleEm = (1.4 * (typo.titleScale ?? 1)).toFixed(2)
  // R5 竖排：关闭首行缩进（竖排下会致列顶错位）；横排维持 indentEm 首行缩进。
  const vertical = opts.vertical ?? false
  const pIndentEm = vertical ? 0 : indentEm
  // R5-c 段距可设置（默认 0.4em，比原横排 0.15em 更明显）：横竖排共用，走 margin-block-end 逻辑属性两轴通吃。
  const paraGapEm = typo.paragraphSpacingEm ?? 0.4
  // 羊皮纸纹理：纯色底 + 三点极淡暖色径向渐变（fixed 附着不随文字滚动，观感似纸面斑驳）。
  // 透明度 ~0.03，肉眼几不可见却给暖白底一点「纸」的质感；纯 CSS 无外部资源，CSP 安全。
  const bg = opts.texture
    ? `${colors.background} radial-gradient(circle at 15% 25%, rgba(150,110,60,0.05), transparent 35%), radial-gradient(circle at 80% 70%, rgba(150,110,60,0.04), transparent 40%)`
    : colors.background
  const rules = [
    // color-scheme 让表单控件 / 滚动条随明暗；颜色显式内联（iframe 不继承 app 变量）。
    `html { color-scheme: light dark; }`,
    `body {`,
    `  margin: 0;`,
    `  color: ${colors.text}${imp};`,
    `  background: ${bg}${imp};`,
    // 纹理开时让渐变随视口固定（滚动流下背景不随文字漂移）。
    opts.texture ? `  background-attachment: fixed;` : '',
    `  font-family: ${fontStack}${imp};`,
    `  font-size: ${fontSizePx}px${imp};`,
    `  line-height: ${lineHeight}${imp};`,
    `  font-weight: ${fontWeight}${imp};`,
    `  letter-spacing: ${letterSpacingEm}em${imp};`,
    `}`,
    // 内嵌图片 / 矢量不溢出栏宽（epub 内嵌图、txt 无图但无害）。
    `img, svg { max-width: 100%; height: auto; }`,
    `a { color: inherit; text-decoration: underline; }`,
    // ── txt 专属（.sate-chapter 隔离，epub 文档无此 class）──
    // 段距用逻辑属性 margin-block（横排=上下、竖排=段间横向间隔，两轴通吃）；首行缩进横排开、竖排关（R5）。
    `.sate-chapter p { text-indent: ${pIndentEm}em; margin-block: 0 ${paraGapEm}em; text-align: ${textAlign}; }`,
    // 章标题：居中、留白、不缩进；字号随 titleScale 独立缩放（R3「章题独立」）；margin-block 逻辑属性适配竖排。
    `.sate-chapter-title { font-size: ${titleEm}em; font-weight: 600; text-align: center; margin-block: 1.6em 1.2em; text-indent: 0; }`,
    // ── md 专属（.sate-md 隔离，txt/epub 文档无此 class）──
    // renderMarkdown 输出无内联样式，全靠此处赋形。边框 / 底色用中性灰 rgba（iframe 取不到 app 变量，
    // 灰阶在明暗主题下都可读）；段落不缩进（markdown 惯例左对齐、空行分段）。块级间距/边框用逻辑属性适配竖排。
    `.sate-md { text-align: left; }`,
    `.sate-md p { text-indent: 0; margin-block: 0.8em; }`,
    `.sate-md h1, .sate-md h2, .sate-md h3, .sate-md h4 { line-height: 1.3; font-weight: 700; margin-block: 1.4em 0.6em; }`,
    `.sate-md h1 { font-size: 1.8em; border-block-end: 1px solid rgba(128,128,128,0.35); padding-block-end: 0.2em; }`,
    `.sate-md h2 { font-size: 1.45em; }`,
    `.sate-md h3 { font-size: 1.2em; }`,
    `.sate-md ul, .sate-md ol { margin-block: 0.6em; padding-inline-start: 1.6em; }`,
    `.sate-md li { margin-block: 0.25em; }`,
    `.sate-md code { font-family: monospace; font-size: 0.88em; background: rgba(128,128,128,0.16); padding: 0.1em 0.35em; border-radius: 3px; }`,
    `.sate-md pre { background: rgba(128,128,128,0.12); padding: 12px 14px; border-radius: 6px; overflow-x: auto; }`,
    `.sate-md pre code { background: none; padding: 0; }`,
    `.sate-md blockquote { margin-block: 0.8em; padding-inline: 1em; border-inline-start: 3px solid rgba(128,128,128,0.4); opacity: 0.85; }`,
    `.sate-md hr { border: none; border-block-start: 1px solid rgba(128,128,128,0.4); margin-block: 1.5em; }`,
  ]
  if (override) {
    // 智能排版：强制统一字体/行距到全部元素、颜色到常见文本元素，段落缩进/对齐作用于所有 p
    //（不限 .sate-chapter），从而覆盖 epub 出版方的高特异性样式。
    rules.push(
      `body * { font-family: ${fontStack} !important; line-height: ${lineHeight} !important; letter-spacing: ${letterSpacingEm}em !important; }`,
      `body :is(p, li, div, span, td, th, blockquote, h1, h2, h3, h4, h5, h6) { color: ${colors.text} !important; }`,
      // 字重不压 h1..h6（标题本就该重于正文）；仅作用于正文类元素。
      `body :is(p, li, div, span, td, th, blockquote) { font-weight: ${fontWeight} !important; }`,
      `p { text-indent: ${pIndentEm}em !important; margin-block: 0 ${paraGapEm}em !important; text-align: ${textAlign} !important; }`,
    )
  }
  return rules.join('\n')
}
