<template>
  <!-- 宿主底色绑生效的阅读主题背景(2026-07-17):此前是 app 的 --color-bg-surface,与 iframe 内
       阅读主题(如羊皮纸)底色错配——沉浸模式下侧带/头带残留 app 色尤其刺眼。 -->
  <div
    class="book-reader"
    :class="{ 'book-reader--immersive': immersive }"
    :style="hostBg ? { background: hostBg } : undefined"
  >
    <div v-if="error" class="book-reader__error">{{ error }}</div>
    <div ref="hostEl" class="book-reader__host"></div>
    <!-- 仿真翻页 overlay（R6）：翻页时叠一层页色 + 卷边光影的面板扫过（不依赖 iframe 快照）。 -->
    <div ref="curlEl" class="book-curl"></div>
    <!-- 侧翻页钮(2026-07-17 重造型):由 2×48px 通栏列改为 overlay 浮动圆钮——不再占用阅读区宽度,
         文本字形 ‹ › 换 lucide chevron。浮在 iframe 之上 → 自身 :hover 不依赖指针桥,沉浸模式
         「平时全隐、悬停唤出」纯 CSS 可达。 -->
    <button
      class="book-nav book-nav--prev"
      @click="prev"
      :title="t('doc.prevPage')"

    >
      <ChevronLeft :size="22" />
    </button>
    <button
      class="book-nav book-nav--next"
      @click="next"
      :title="t('doc.nextPage')"

    >
      <ChevronRight :size="22" />
    </button>
  </div>
</template>

<script setup lang="ts">
// 统一渲染器（阅读器方案 R2）：包 vendored foliate-js 的 <foliate-view> 自定义元素，
// EPUB / txt 共用同一渲染管线（md 待 R2-4b）。对齐现有 ReaderApi 契约（next/prev/getScrollEl），
// 进度用 "cfi:<epubcfi>" 前缀（epub 及 txt 合成 book 的 foliate 原生 CFI 均走此路，存量兼容）。
//
// 已交付：epub（R2-2，真机验平价）；txt 经 SyntheticBook 入统一管线（R2-4，textSource prop）。
// 待做：md 感知分章（R2-4b）；fraction 页脚/scrolled 双流/TOC 面板（R2-3）；排版/主题注入（R2-5）。
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronLeft, ChevronRight } from '@lucide/vue'
import { applyReplacerToDom } from '../../utils/replacements'
import { IPC } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'
import {
  buildTextSyntheticBook,
  buildMarkdownSyntheticBook,
  hasOversizedSection,
} from '../../utils/syntheticBook'
import { renderMarkdownBlocks } from '../../utils/markdown'
import { applyZhConvertToDom } from '../../utils/zhConvert'
import { relayKeyboardEvents, relayPointerMoves } from '../../utils/keyRelay'
import { denyScriptResources } from '../../utils/epubScriptGate'
import { notifyPointerFromFrame } from '../../composables/useChromeReveal'
import { highlightMarkdownHtml, isDarkColor } from '../../utils/shikiHighlight'
import {
  buildReaderCSS,
  type ReaderColors,
  type ReaderFontFamily,
  type ReaderTextAlign,
} from '../../utils/readerStyles'
import { getReaderTheme, READER_THEME_FOLLOW } from '../../themes/readerThemes'
import { useThemeStore } from '../../stores/themeStore'
import type { TextBookIndex, TextChapterContent, ZhConvertConfig } from '../../types/reader'
import type {
  FoliateView,
  FoliateBook,
  FoliateLoadDetail,
  FoliateRelocateDetail,
  FoliateTocItem,
} from '../../vendor/foliate-js/view.js'

const props = defineProps<{
  /** 文件 URL（convertFileSrc 资产地址）；epub 经 foliate makeBook 自动探测格式。 */
  url?: string
  /** txt 文本源（R2-4）：提供时构造 SyntheticBook 走统一管线，优先于 url。版本/编码在后端 seam 解析，前端只认 itemId。 */
  textSource?: { itemId: number; isMarkdown: boolean; reflow?: boolean }
  /** 上次阅读位置（"cfi:<cfi>"）。 */
  initial: string | null
  /** 替换规则函数（§5.2）；每章文档载入后对其文本节点就地应用。 */
  replacer?: (t: string) => string
  /** 阅读流：'paginated' 书卷翻页（默认）/ 'scrolled' 连续滚动。foliate paginator 原生支持，txt/epub 统一。 */
  flow?: 'paginated' | 'scrolled'
  /** 排版模式：'book' 自带排版（默认，书样式胜出）/ 'reader' 智能排版（阅读器排版覆盖书样式）。主要影响 epub。 */
  styleMode?: 'book' | 'reader'
  /**
   * 翻页动画（R6）：'none' 瞬切 / 'slide' foliate 内建滑动（默认）/ 'curl' 仿真翻页（滑动 + CSS 卷页 overlay）。
   * 真·内容卷页需快照 iframe（Chromium 不可靠），故 curl 为不依赖快照的卷边光影近似，视觉待真机调优。
   */
  pageTurn?: 'none' | 'slide' | 'curl'
  /**
   * 排版参数（R2-6a/b + R3）：字号 px / 行距 / 字体族 / 栏宽 px + 字重 / 字距(em) / 对齐 / 章题缩放。
   * 缺省用 buildReaderCSS 内置默认（19 / 1.75 / serif / 720 / 400 / 0 / justify / 1）。
   */
  typography?: {
    fontSizePx?: number
    lineHeight?: number
    fontFamily?: ReaderFontFamily
    maxInlineSizePx?: number
    fontWeight?: number
    letterSpacingEm?: number
    textAlign?: ReaderTextAlign
    titleScale?: number
    paragraphSpacingEm?: number
  }
  /** 简繁转换档位(R4,§5.10):缺省=关(不转换)。纯显示层,txt/md/epub 通用;切换由父层 remount 生效(转换不可逆)。 */
  zhConvert?: ZhConvertConfig
  /** 自动翻页（R4）：开启后按 autoScrollSec 间隔自动 next()（分页流=翻页，滚动流=下移一屏）。 */
  autoScroll?: boolean
  /** 自动翻页间隔（秒）；缺省 5。 */
  autoScrollSec?: number
  /**
   * 阅读主题（R3）日/夜两槽的全局选择（reader theme id 或 FOLLOW=跟随应用）。据 app 当前明暗择槽；
   * FOLLOW → 读 app 的 --color-* 变量（R2 起的既有行为，零回归默认）。
   */
  readerThemeLight?: string
  readerThemeDark?: string
  /** 每书阅读主题覆盖（R3）：非空且 kind 与当前 app 明暗相符时压过全局槽；'' / FOLLOW = 用全局。 */
  bookReaderTheme?: string
  /**
   * 竖排模式（R5，品牌签名）：'off' 横排（默认）/ 'vertical-rl' 竖排右起（CJK 传统）/ 'vertical-lr' 左起。
   * 经 onLoad 把 writing-mode 落到文档根——**须在 foliate getDirection 读取前**（paginator 在 load 事件后
   * 紧接探测 writingMode 并对换全部轴逻辑：分页/滑动/滚动/snap/overlayer/dir）。writing-mode 可继承 → body
   * 生效。切换须由父层 remount（getDirection 仅在 section load 运行，改 prop 不重探测已加载 section）。
   */
  vertical?: 'off' | 'vertical-rl' | 'vertical-lr'
  /**
   * 沉浸模式（2026-07-17）：隐藏侧翻页钮（悬停唤出）并去 foliate 头/脚带（margin=0px），
   * 使 iframe 内运行头（章名）与页脚带随工具栏一同消失。父层（DocumentViewer）传入。
   */
  immersive?: boolean
}>()

const emit = defineEmits<{
  (e: 'ready'): void
  (e: 'progress', pos: string): void
  /** 位置变化（R2-3）：全书进度 0..1 + 当前章标题，供页脚百分比 / 章名显示。 */
  (e: 'locate', v: { fraction: number; tocLabel: string; tocHref: string }): void
  /** 目录（R2-3）：ready 后上报 book.toc，供 TOC 面板渲染。 */
  (e: 'toc', toc: FoliateTocItem[]): void
  /** 护栏触发（2026-07-17）：存在超限单片，本组件已强制 scrolled 流（无视 flow prop），父层提示 + 禁用切换。 */
  (e: 'flow-forced'): void
}>()

const { t } = useI18n()
const theme = useThemeStore()

const hostEl = ref<HTMLElement | null>(null)
const curlEl = ref<HTMLElement | null>(null)
const error = ref('')
// 宿主底色=生效阅读主题背景(2026-07-17):随 applyReaderStyles 同步更新;空串期间用 CSS 兜底色。
const hostBg = ref('')

let view: FoliateView | null = null
// 本组件构造的 SyntheticBook（txt）：卸载时须 destroy() 回收其 blob URL（foliate 不代管 book 资源）。
let builtBook: FoliateBook | null = null
let destroyed = false

// 超限单片强制 scrolled 护栏(D-002):阈值与谓词在 utils/syntheticBook.ts 单源
// (FORCE_SCROLLED_SECTION_CHARS/hasOversizedSection),md 与 txt 两分支共用(审查 F-04)。
// 护栏态:置位后 effectiveFlow 恒 scrolled(无视 flow prop),并已 emit('flow-forced') 告知父层。
let forcedScrolled = false

/** 生效阅读流:护栏优先于用户偏好。 */
function effectiveFlow(): 'paginated' | 'scrolled' {
  return forcedScrolled ? 'scrolled' : (props.flow ?? 'paginated')
}
let wheelLock = 0
// 最近一次 relocate 的位置（供书签「添加当前位置」读取当前 CFI/进度/章名）。
let lastCfi = ''
let lastFraction = 0
let lastLabel = ''
// 当前章 href（TOC 面板据此高亮当前章；与 book.toc 的 href 同源，foliate 的 tocItem 直接引用 toc 节点）。
let lastHref = ''
// 自动翻页（R4）计时器句柄。
let autoScrollTimer: ReturnType<typeof setInterval> | null = null

// ── ReaderApi 契约 ────────────────────────────────────────────────────────────
function next() {
  playCurl('next')
  view?.next()
}
function prev() {
  playCurl('prev')
  view?.prev()
}

// 仿真翻页：翻页时给 overlay 加方向 class 触发 CSS 卷页动画（仅 pageTurn==='curl'）。
// remove + 强制回流 + add 以便同方向连续翻页可重复触发；animationend 清理见 onMounted。
function playCurl(dir: 'next' | 'prev') {
  if (props.pageTurn !== 'curl') return
  const el = curlEl.value
  if (!el) return
  el.classList.remove('book-curl--next', 'book-curl--prev')
  void el.offsetWidth
  el.classList.add(dir === 'next' ? 'book-curl--next' : 'book-curl--prev')
}
function onCurlEnd() {
  curlEl.value?.classList.remove('book-curl--next', 'book-curl--prev')
}
// 分页流无原生滚动容器 → 返回 null（usePager 仅接键盘；滚轮由本组件处理，与旧 EpubReader 同）。
function getScrollEl(): HTMLElement | null {
  return null
}
defineExpose({
  next,
  prev,
  getScrollEl,
  goToHref,
  searchBook,
  clearSearch,
  getCurrentLocation,
  goToLocator,
})

// 滚轮翻页（节流）：分页流无原生滚动，foliate paginator 只自管 touchstart 滑动、不管 wheel，
// 故由本组件统一处理翻页（不经 usePager）。滚动流则让 foliate 容器（overflow:auto）原生滚动接管。
function onWheel(e: WheelEvent) {
  if (effectiveFlow() === 'scrolled') return
  e.preventDefault()
  const now = Date.now()
  if (now < wheelLock) return
  wheelLock = now + 350
  if (e.deltaY > 0) next()
  else if (e.deltaY < 0) prev()
}

// 施加阅读流到 renderer：paginator 的 observedAttributes 含 'flow'，运行时改属性即触发 render() 重排
// （位置由 foliate 保持），故切换翻页/滚动无需 remount。
function applyFlow() {
  view?.renderer?.setAttribute('flow', effectiveFlow())
}
watch(
  () => props.flow,
  () => applyFlow(),
)

// 沉浸残留 chrome 治理(2026-07-17):去 foliate 头/脚带。'margin' 是 paginator observedAttribute
// (attributeChangedCallback 把值写进 --_margin;#header/#footer 高度=var(--_margin)) → 置 0px 即隐去
// iframe 侧的运行头(章名)与页脚带,零 vendor 补丁。#header 无 part 属性(仅 #background 有),外部
// ::part 不可达——margin 属性是唯一零补丁通道。scrolled 流该值只作内容上下 padding,置 0=沉浸全出血,
// 符合预期。退出时显式还原 '48px'(paginator 默认),不走 removeAttribute(回调收 null,写坏 CSS 变量)。
function applyImmersiveChrome() {
  view?.renderer?.setAttribute('margin', props.immersive ? '0px' : '48px')
}
watch(
  () => props.immersive,
  () => applyImmersiveChrome(),
)

// 翻页动画（R6）：slide/curl 均启用 foliate 内建 'animated' 滑动位移；none 移除即瞬切。curl 的卷页
// overlay 由 playCurl 叠加。paginator.js:901 以 hasAttribute('animated') 门控 300ms easeOutQuad 滑动。
function applyPageTurn() {
  const r = view?.renderer
  if (!r) return
  if ((props.pageTurn ?? 'slide') === 'none') r.removeAttribute('animated')
  else r.setAttribute('animated', '')
}
watch(
  () => props.pageTurn,
  () => applyPageTurn(),
)

// 自动翻页（R4）：开启则按间隔自动 next()（分页流翻页 / 滚动流下移一屏，均由 foliate next() 统一处理）。
// 到书末 next() 为 no-op，不需额外停表。切换开关/速率即重建定时器。
function stopAutoScroll() {
  if (autoScrollTimer !== null) {
    clearInterval(autoScrollTimer)
    autoScrollTimer = null
  }
}
function startAutoScroll() {
  stopAutoScroll()
  if (!props.autoScroll) return
  const sec = props.autoScrollSec && props.autoScrollSec > 0 ? props.autoScrollSec : 5
  autoScrollTimer = setInterval(() => view?.next(), sec * 1000)
}
watch(
  () => [props.autoScroll, props.autoScrollSec],
  () => startAutoScroll(),
)

// 读当前 app 主题的正文/背景色(iframe 不继承 app 的 CSS 变量,须内联进注入 CSS)。
// 直接取主题生成的当前色板:它与写入 DOM 的 --color-* 同源,无需再回读 computed style,
// 也不再受浓度表达式影响(浓度已收归生成函数)。
function readThemeColors(): ReaderColors {
  const palette = theme.currentPalette
  return { text: palette.textPrimary, background: palette.background }
}

// 解析生效的阅读颜色（R3）：据 app 当前明暗择日/夜槽 → 每书覆盖（kind 相符才生效）→ 落到阅读主题色；
// FOLLOW/未知一律回落 app 颜色（= readThemeColors，与 R2 行为逐像素一致）。texture 随选中主题带出。
function resolveReaderColors(): { colors: ReaderColors; texture: boolean } {
  const appColors = readThemeColors()
  // app 明暗取主题 store 的有效模式(与 Canvas/DOM 同一份;预览配色时同步)。
  const appKind = theme.isDark ? 'dark' : 'light'
  let pick =
    appKind === 'dark'
      ? (props.readerThemeDark ?? READER_THEME_FOLLOW)
      : (props.readerThemeLight ?? READER_THEME_FOLLOW)
  // 每书覆盖仅在其 kind 与当前 app 明暗相符时生效——否则会在暗界面套浅底（或反之），回落全局槽更稳。
  const bookId = props.bookReaderTheme
  if (bookId && getReaderTheme(bookId)?.kind === appKind) pick = bookId
  const rt = getReaderTheme(pick)
  if (!rt) return { colors: appColors, texture: false } // FOLLOW 或未知 → 跟随应用
  return { colors: { text: rt.text, background: rt.background }, texture: !!rt.texture }
}

// 注入排版（R2-5a/b）：setStyles 存入 renderer #styles，每次新章载入自动重注（paginator.js）；
// 运行时改样式后 foliate 会 #view.expand() + ResizeObserver 自动重排（Chromium/WebView2）→ 无需 remount。
//  - 自带排版：默认 CSS 入 before 槽（低优先）→ txt 全量生效、epub 书样式胜出（零回归）。
//  - 智能排版：override CSS 入 after 槽（高优先 + !important）→ 覆盖排版差/无排版的 epub。
function applyReaderStyles() {
  const { colors, texture } = resolveReaderColors()
  // 宿主底色同步:iframe 外的一切残留面(侧钮带/头脚带/加载期)与主题同底。
  hostBg.value = colors.background
  const typo = props.typography ?? {}
  // 竖排（R5）：段落规则改逻辑属性 + 关首行缩进（见 buildReaderCSS）。writing-mode 本身由 onLoad 注入文档根。
  const vertical = (props.vertical ?? 'off') !== 'off'
  if ((props.styleMode ?? 'book') === 'reader') {
    view?.renderer?.setStyles([
      '',
      buildReaderCSS(colors, typo, { override: true, texture, vertical }),
    ])
  } else {
    view?.renderer?.setStyles([buildReaderCSS(colors, typo, { texture, vertical }), ''])
  }
}
// 栏宽（max-inline-size）经 renderer 属性施加（observedAttributes 含之，setAttribute 即触发 render() 重排）。
function applyReaderLayout() {
  const w = props.typography?.maxInlineSizePx
  if (w) view?.renderer?.setAttribute('max-inline-size', `${w}px`)
}
// styleMode / typography 变更 → 重注样式 + 栏宽（foliate 自动重排，保位，不 remount）。typography 走 deep：
// 父层传等值新对象不触发，仅实变时重排。
watch(
  () => props.styleMode,
  () => applyReaderStyles(),
)
watch(
  () => props.typography,
  () => {
    applyReaderStyles()
    applyReaderLayout()
  },
  { deep: true },
)
// 主题/配色/外观模式切换 → 色板换代 → 重读主题色重注(R2-6b:换主题阅读页实时重着色)。
// R3:app 明暗变化也会改「据 kind 择槽」的结果,故同一 watch 覆盖。currentPalette 是 computed,
// 参数不变时引用不变,不因无关设置或每帧发布而重排。
watch(
  () => theme.currentPalette,
  () => applyReaderStyles(),
)
// 阅读主题槽位 / 每书主题覆盖变更（R3）→ 重解析颜色重注（foliate 自动重排，不 remount）。
watch(
  () => [props.readerThemeLight, props.readerThemeDark, props.bookReaderTheme],
  () => applyReaderStyles(),
)

// 章文档载入（等价 epub.js hooks.content 的挂点）：对其 body 就地应用替换规则，再按档位施加简繁转换。
// 顺序遵 §5.13「替换 → 简繁 → 渲染」：替换是同步纯函数先施加；简繁走后端 IPC 异步，随后 fire-and-forget。
function onLoad(e: Event) {
  const detail = (e as CustomEvent<FoliateLoadDetail>).detail
  const doc = detail?.doc
  if (!doc?.body) return
  // 键盘桥（2026-07-16）：iframe 的键盘事件不跨界冒泡到父窗口 → 焦点一进书里，父页所有 window 级
  // 键位全失灵（按住 Esc 退全屏、方向键翻页、F11…）。在此把它们中继出去，见 utils/keyRelay 顶注。
  // 挂在本钩子而非别处：paginator 每节**新建**一个 View/iframe（#createView 无复用），故只有
  // per-load 才覆盖全书；FixedLayout 一个跨页 1–2 个 iframe，也各自派发一次 load，同样覆盖。
  // 不显式摘除：iframe 被 destroy/移出 DOM 时其浏览上下文连同文档与监听一并回收，无残留。
  if (hostEl.value) relayKeyboardEvents(doc, hostEl.value)
  // 同理，指针也不跨界：flow="scrolled" 时 foliate 的 iframe 直抵父页 y=0，那一片父页收不到任何
  // pointermove → 贴顶唤出顶栏结构性失效（与快慢无关，故阅读页只是「偶发」——默认 flow="paginated"
  // 有 48px header 带，指针落在带里时父页仍收得到）。只上报换算后的 y，不伪造指针事件（见 relayPointerY 注）。
  relayPointerMoves(doc, notifyPointerFromFrame)
  // 竖排（R5）：本 load 事件在 foliate getDirection 之前同步派发（paginator onLoad 先 setStyles 再 dispatch
  // 'load'，二者都在 getDirection 前），故此处把 writing-mode 落到文档根即可被随后的 getDirection 探测到
  // → 渲染器原生竖排轴对换全部生效。writing-mode 可继承，body 随之竖排。'off' 清空回横排（零回归）。
  const wm = props.vertical && props.vertical !== 'off' ? props.vertical : ''
  doc.documentElement.style.writingMode = wm
  if (props.replacer) {
    try {
      applyReplacerToDom(doc.body, props.replacer)
    } catch {
      /* 单章替换失败不阻断渲染 */
    }
  }
  // 简繁转换（R4）：仅在选了档位时调后端。纯显示层变换，不改后端 canonical。
  // 转换在渲染后异步写回，故每章至多一次短暂闪烁（section load 按章触发，章内翻页不重载）。
  const cfg = props.zhConvert
  if (cfg) {
    void applyZhConvertToDom(doc.body, (texts) =>
      invokeIpc<string[]>(IPC.CONVERT_CHINESE, { texts, config: cfg }),
    ).catch(() => {
      /* 单章简繁失败不阻断渲染 */
    })
  }
}

// 位置变化：以 "cfi:" 前缀上报进度（存量兼容）+ 全书 fraction 与当前章名（R2-3 页脚）。
function onRelocate(e: Event) {
  const d = (e as CustomEvent<FoliateRelocateDetail>).detail
  if (d?.cfi) {
    lastCfi = d.cfi
    emit('progress', `cfi:${d.cfi}`)
  }
  if (typeof d?.fraction === 'number') {
    lastFraction = d.fraction
    lastLabel = d.tocItem?.label ?? ''
    lastHref = d.tocItem?.href ?? ''
    emit('locate', { fraction: d.fraction, tocLabel: lastLabel, tocHref: lastHref })
  }
}

// TOC 跳转（R2-3）：view.goTo 解析 href（txt/md=章号、epub=章内 href）→ 定位并翻至该章。
// 搜索命中跳转（R4）亦复用此路：foliate 的 goTo 对 CFI 与 href 均可解析。
function goToHref(href: string) {
  view?.goTo(href)
}

// 书内搜索（R4）：返回 foliate 原生 search 异步生成器（全书流式，逐章 yield {label,subitems}，命中同时
// 在渲染视图内自动高亮）。宿主（DocumentViewer）迭代它把命中喂给搜索面板；未就绪返回 undefined。
// 匹配基于章源文档（createDocument 的输出），与替换规则/简繁转换后的显示文本无关——简繁开启时以源文本
// 匹配（foliate 架构固有，替换规则同理）；命中高亮仍按 CFI 结构位置落点，不受简繁近 1:1 变换影响。
function searchBook(query: string) {
  return view?.search({ query })
}
// 清除搜索高亮（关闭面板 / 换查询时）。
function clearSearch() {
  view?.clearSearch()
}

// 书签（R4）：当前阅读位置快照（locator 用 "cfi:" 前缀，与阅读进度同源）。首次 relocate 前
// lastCfi 为空 → 返回 null（宿主据此提示尚不能加书签）。
function getCurrentLocation(): { locator: string; label: string; fraction: number } | null {
  if (!lastCfi) return null
  return { locator: `cfi:${lastCfi}`, label: lastLabel, fraction: lastFraction }
}
// 跳到书签位置：剥去 "cfi:" 前缀后交 view.goTo。
function goToLocator(locator: string) {
  const target = locator.startsWith('cfi:') ? locator.slice(4) : locator
  view?.goTo(target)
}

// 解析 open 目标：txt/md 走 SyntheticBook，否则 url 交 foliate makeBook 探测（epub）。
async function resolveOpenTarget(): Promise<FoliateBook | string> {
  const ts = props.textSource
  if (ts) {
    if (ts.isMarkdown) {
      // md：取生效版本全文（GET_DOCUMENT_TEXT，含版本/编码解析）→ 零依赖 renderMarkdownBlocks →
      // 按块分片多 section(2026-07-17 内存爆炸修复:整篇单 section 在 paginated multicol 下
      // 是 GB 级放大灾难,分片使 paginated 只 columnize 当前 ~24K 片)。
      const text = await invokeIpc<string>(IPC.GET_DOCUMENT_TEXT, { itemId: ts.itemId })
      let blocks = renderMarkdownBlocks(text ?? '')
      // 代码块语法高亮（R4，shiki JS 引擎懒载）：按当前**生效阅读背景**明暗选 github 主题（R3：夜间阅读
      // 主题即使在浅色界面也取暗色码主题）；逐块施加,无围栏的块在 highlightMarkdownHtml 内短路,
      // 全篇无围栏则整段跳过(连 shiki chunk 都不拉)。主题在开卷时定，换主题不重新高亮
      //（代码块主题滞后至重开，属可接受折中）。
      if (blocks.some((b) => b.includes('<pre><code'))) {
        const dark = isDarkColor(resolveReaderColors().colors.background)
        const highlighted: string[] = []
        for (const b of blocks) highlighted.push(await highlightMarkdownHtml(b, dark))
        blocks = highlighted
      }
      builtBook = buildMarkdownSyntheticBook({ blocks })
      // 护栏(D-002):分片后仍存在超限单片(仅「单块超预算独占一片」可能,如巨型代码围栏)→
      // 强制 scrolled。须在 open 前置位:onMounted 的 applyFlow 在 resolveOpenTarget 之后运行。
      if (hasOversizedSection(builtBook.sections)) {
        forcedScrolled = true
        emit('flow-forced')
      }
      return builtBook
    }
    // txt：get_text_book_index 内部经 effective_text_ref 解析当前版本/源文件 + 编码 → 前端只传 itemId。
    const index = await invokeIpc<TextBookIndex>(IPC.GET_TEXT_BOOK_INDEX, { itemId: ts.itemId })
    builtBook = buildTextSyntheticBook({
      index,
      loadChapter: (chapterIndex) =>
        invokeIpc<TextChapterContent>(IPC.GET_TEXT_CHAPTER, {
          itemId: ts.itemId,
          chapterIndex,
          reflow: ts.reflow ?? false, // 保守默认一级分段；重排开关待 R2-6。
        }),
    })
    // 护栏(审查 F-04,与 md 分支同款):txt 分章只在行边界续切,单行巨串(minified JSON/
    // base64/单行日志)整行独占一章,30K 章上限对它失效——超限章进 paginated multicol 就是
    // 2026-07-17 修过的 GB 级内存放大。存在超限章即强制 scrolled(size = 后端 charLen)。
    if (hasOversizedSection(builtBook.sections)) {
      forcedScrolled = true
      emit('flow-forced')
    }
    return builtBook
  }
  if (props.url) return props.url
  throw new Error('no book source')
}

// 从上次位置恢复：
//  - "cfi:<cfi>"：epub 及 txt 合成 book 的 foliate 原生 CFI（fake-CFI）→ init 直接解析。
//  - 其它/缺省：从首章起（保留封面/前言，与旧 EpubReader 同）。
async function restorePosition() {
  if (!view) return
  const init = props.initial
  const cfi = init?.startsWith('cfi:') ? init.slice(4) : undefined
  // 陈旧/损坏 CFI 兜底(2026-07-10 审查 B13):进度值来自 DB,是外部输入——epub 换版本、
  // txt 切 reflow 改 canonical、值被写坏时,foliate 的 resolveNavigation 只吞解析错,
  // goTo/scrollToAnchor 执行期异常会贯穿 init 直达「无法打开」总错。位置恢复失败不应
  // 有能力让整本书打不开:回退到无位置初始化(首章起)。
  try {
    await view.init({ lastLocation: cfi })
  } catch {
    if (destroyed) return
    await view.init({})
  }
}

onMounted(async () => {
  try {
    // 宿主底色先行:书未打开前(懒加载/解析期)就按主题着色,免先闪 app surface 色再跳主题色。
    hostBg.value = resolveReaderColors().colors.background
    // import 副作用即 customElements.define('foliate-view')；懒加载使 foliate 仅在打开书时入 chunk。
    await import('../../vendor/foliate-js/view.js')
    if (destroyed) return

    const target = await resolveOpenTarget()
    if (destroyed) return

    view = document.createElement('foliate-view') as unknown as FoliateView
    // 监听须在 open/init 之前挂，否则首章的 load / 首次 relocate 会漏。
    view.addEventListener('load', onLoad)
    view.addEventListener('relocate', onRelocate)
    hostEl.value?.appendChild(view as unknown as HTMLElement)

    await view.open(target)
    if (destroyed) return

    // 「脚本化 EPUB 不支持」——设计 §4.8 的**规范性**裁决。2026-07-16 安全审查查明：此前它只是一句
    // 声明，代码里没有任何东西在执行它。上游为此预留的 seam（epub.js loadItem 派发的 'load' 事件带
    // isScript/allow）**全仓零订阅**，于是 allow 恒真、EPUB 里的 <script src> 一路走到可用的 blob URL。
    // 而该 blob iframe 与父页**同源**（blob URL 继承创建者的源），父页持有 Tauri IPC 桥。
    // 挂点必须在此：open() 只建 book、renderer.open() 只存 sections（都不载章），首章载入由下方
    // restorePosition() 触发的 goTo 发起 —— 早一步则 book 还不存在，晚一步则漏掉首章。
    denyScriptResources(view.book)

    applyFlow() // 首渲前设好 flow，使初次布局即按当前阅读流（避免先翻页再跳滚动的闪烁）。
    applyPageTurn() // 翻页动画开关（animated 属性）。
    applyReaderLayout() // 栏宽（max-inline-size）→ 首渲即按当前设置。
    applyImmersiveChrome() // 沉浸态头/脚带（remount 进入时即按当前态,不闪一帧 48px 带）。
    applyReaderStyles() // 存入 renderer #styles，首章及后续章载入时自动套用排版基线。

    await restorePosition()
    if (destroyed) return

    hostEl.value?.addEventListener('wheel', onWheel, { passive: false })
    curlEl.value?.addEventListener('animationend', onCurlEnd)
    emit('ready')
    emit('toc', (view?.book?.toc ?? []) as FoliateTocItem[]) // 目录上报供 TOC 面板（R2-3）。
    startAutoScroll() // 若挂载时已处于自动翻页态（换章 remount 后续跑）。
  } catch (e) {
    error.value = t('doc.epubOpenFailed', { error: (e as Error)?.message ?? e })
    emit('ready')
  }
})

onBeforeUnmount(() => {
  destroyed = true
  stopAutoScroll()
  hostEl.value?.removeEventListener('wheel', onWheel)
  curlEl.value?.removeEventListener('animationend', onCurlEnd)
  try {
    view?.removeEventListener('load', onLoad)
    view?.removeEventListener('relocate', onRelocate)
    view?.close() // 内部 renderer.destroy() 会 unload 各 section（回收已加载章的 URL）
    ;(view as unknown as HTMLElement | null)?.remove()
    builtBook?.destroy?.() // 再回收 SyntheticBook 剩余未 unload 的 blob URL（幂等）
  } catch {
    /* ignore teardown errors */
  }
  view = null
  builtBook = null
})
</script>

<style scoped>
.book-reader {
  position: relative;
  height: 100%;
  display: flex;
  align-items: stretch;
  background: var(--color-bg-surface, #fff);
}
.book-reader__host {
  flex: 1;
  min-width: 0;
  height: 100%;
}
.book-reader__error {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
}
/* 侧翻页钮重造型(2026-07-17):overlay 浮动圆钮,垂直居中贴边。z-index 高于 book-curl(2),
   浮在 iframe 之上 → 指针落上即父页 :hover(不需指针桥)。平时低透明度不喧宾,悬停实体化。 */
.book-nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 3;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  padding: 0;
  border: 1px solid var(--color-border-strong);
  border-radius: 50%;
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  cursor: pointer;
  opacity: 0.45;
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  transition:
    opacity var(--transition-fast),
    background var(--transition-fast),
    color var(--transition-fast);
}
.book-nav:hover,
.book-nav:focus-visible {
  opacity: 1;
  color: var(--color-text-primary);
}
.book-nav--prev {
  left: 12px;
}
.book-nav--next {
  right: 12px;
}
/* 沉浸:钮平时全隐,指针悬到其落点或键盘聚焦才现身(hit 区仍在,不折叠)。 */
.book-reader--immersive .book-nav {
  opacity: 0;
}
.book-reader--immersive .book-nav:hover,
.book-reader--immersive .book-nav:focus-visible {
  opacity: 0.9;
}

/* 仿真翻页 overlay（R6）：翻页时一道「卷边光影」扫过阅读区（暗侧=卷起阴影、亮侧=纸面高光），
   与 foliate 内建滑动同步，营造翻页立体感。不依赖 iframe 快照，纯 CSS。静止 opacity:0 不遮挡。 */
.book-curl {
  position: absolute;
  inset: 0;
  z-index: 2;
  pointer-events: none;
  overflow: hidden;
  opacity: 0;
}
.book-curl::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  width: 55%;
  background: linear-gradient(
    90deg,
    transparent 0%,
    rgba(0, 0, 0, 0.03) 38%,
    rgba(0, 0, 0, 0.14) 49%,
    rgba(255, 255, 255, 0.12) 52%,
    transparent 72%
  );
}
.book-curl--next,
.book-curl--prev {
  animation: book-curl-fade 320ms ease-out;
}
.book-curl--next::before {
  animation: book-curl-next 320ms ease-out;
}
.book-curl--prev::before {
  animation: book-curl-prev 320ms ease-out;
}
@keyframes book-curl-fade {
  0%,
  75% {
    opacity: 1;
  }
  100% {
    opacity: 0;
  }
}
/* next（LTR）：光影自右向左扫过，如当前页向书脊翻去。 */
@keyframes book-curl-next {
  0% {
    transform: translateX(105%);
  }
  100% {
    transform: translateX(-60%);
  }
}
@keyframes book-curl-prev {
  0% {
    transform: translateX(-105%);
  }
  100% {
    transform: translateX(60%);
  }
}
</style>
