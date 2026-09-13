// src/composables/useChromeReveal.ts
// chrome(顶栏 / 底栏)的「自动隐藏 + 边缘唤出」态。模块级单例:唤出态跨组件——顶栏宿主
// (titlebar-host)在 App.vue,底栏(app-statusbar)与分离模式的页面工具栏(app-toolbar)在 AppShell
// **内部**,二者不在同一作用域,局部 ref 传不过去。
//
// 前身是 App.vue 里的局部 chromeRevealed(2026-07-10「查看器沉浸期鼠标移入顶部滑入顶栏」)。
// 2026-07-16 用户新需求「F11 = 全屏沉浸式画廊,无顶栏+无底栏,仅鼠标移入才显示,且显示后不能拖动」
// 使它需要:① 第二个来源(全屏,而非只有查看器沉浸);② 第二个唤出边(底);③ 跨到 AppShell 内部。
//
// 「显示后不能拖动」不在本模块:它是窗口三态的推论(全屏 → canDragWindow=false),已由
// useWindowDrag 直读 useWindowMode 落实,唤出的顶栏天然不可拖,无需本模块再管一份。
//
// ── 唤出/收起为何必须**同为几何判据**(2026-07-16 真机回归,首版设计错误) ──────────────────
// 首版:唤出是几何的(window 级 pointermove 测 clientY),收起是拓扑的(靠宿主的 mouseleave)。
// 二者不可组合 —— mouseleave 必须先有 mouseenter 才可能派发。指针**快速掠过** 4px 唤出带再落到
// 远处:采样点触发唤出 → 顶栏滑入到空无一人的位置 → 浏览器从未对它派发 enter → **leave 永不派发**
// → 卡在展开态;用户随后移进顶栏再移出才补上那唯一一次 leave → 收起。观感恰是「移出变显示、
// 移入变隐藏」(用户原话)。
// 故收起改为同一个 pointermove 里的几何判据:指针越出该侧 chrome 所占的**布局**高度即收。
// 由此得三个额外好处:
//   ① 只剩一条监听、一个判据,不存在"两半各自为政"的相位差;
//   ② 分离模式下顶侧两条(标题栏 40 + 页面工具栏 48)天然被同一个 88px 边界覆盖——指针在两条之间
//      移动不再需要"同侧豁免"这种补丁(首版为此专门加过 movedWithinSide 判据,现已成结构性免疫);
//   ③ 唤出(y≤4)与收起(y>extent)之间留出大片死区,不会在边界抖动。

import { computed, onBeforeUnmount, ref, watch, type ComputedRef } from 'vue'
import { useViewerStore } from '../stores/viewerStore'
import { useUiStore } from '../stores/uiStore'
import { isFullscreen } from './useWindowMode'

/**
 * 指针距视口上/下缘多少像素内算「移入边缘」。取 4:够窄以免误触(画廊顶部行的悬停不该弹出顶栏),
 * 又够宽以容纳快速甩动指针时的采样间隔(pointermove 逐帧采样,1px 会被高速移动跳过)。
 * 全屏(任意触发来源)专用值——全屏区间 useWindowMode.applyFullscreen 已 setResizable(false)
 * 消掉 wry 的隐形 drag-resize 子窗口输入死区(见下方 CHROME_REVEAL_EDGE_PX_WINDOWED 注),
 * 4px 在全屏下已够用,不必加宽。
 */
export const CHROME_REVEAL_EDGE_PX = 4

/**
 * 非全屏(窗口态,resizable 恒为 true)专用的唤出带宽度(D-423,2026-07-23 复核裁决更正)。
 *
 * **分档依据是窗口态本身(死区是否在场),不是触发来源**——首版误把「窗口化设置触发」当分档
 * 条件,漏判了「非全屏 + 查看器沉浸」同样在窗口态下运行、同样吃这条死区。tauri-runtime-wry
 * 给 undecorated+resizable 窗口盖的隐形 drag-resize 子窗口,只要 resizable=true 就在屏幕顶/
 * 底缘拦一条 ≈4px 的指针输入死区(见 utils/appWindow.ts::setResizable 注),与 4px 唤出带
 * 几乎完全重合时 WebView 收不到带内采样、唤出形同虚设——这与「隐藏是被谁触发的」无关,只与
 * 「当前是否处于非全屏窗口态」有关。加宽到 8px 使其中仍有 ~4px 落在死区之外、可被正常收到;
 * 全屏(任意来源)已被 setResizable(false) 消带,维持 4px 即可。
 */
export const CHROME_REVEAL_EDGE_PX_WINDOWED = 8

const top = ref(false)
const bottom = ref(false)

/** 顶栏当前是否已唤出。 */
export const topRevealed: ComputedRef<boolean> = computed(() => top.value)

/** 底栏当前是否已唤出。 */
export const bottomRevealed: ComputedRef<boolean> = computed(() => bottom.value)

/**
 * chrome(顶栏/底栏)是否处于「自动隐藏 + 边缘唤出」态。两个来源:
 * ① `viewer.isImmersive` —— 查看器沉浸(2026-07-10 用户裁决);
 * ② `isFullscreen` —— F11 全屏沉浸式画廊(2026-07-16 用户新需求,对标 Chrome F11)。
 *
 * **侧栏不在本判据内**(AppShell.sidebarVisible 仍只看 viewer.isImmersive):用户两次点名的都是
 * 「顶栏底栏」,且侧栏本就有自己的开关——替他关掉是越权,而他随时可自己关。
 *
 * 惰性读 store:本模块被 import 时 pinia 尚未安装,故 useViewerStore() 只能在 computed **求值时**
 * 调用(与 commands/types.ts §「环境服务不入 context」同款惯用法)。
 */
export const chromeAutoHidden: ComputedRef<boolean> = computed(
  () =>
    useViewerStore().isImmersive || isFullscreen.value || useUiStore().autoHideChromeWindowed,
)

/**
 * 当前生效的唤出带宽度。判据是**窗口态**而非触发来源(D-423):全屏(不论由 F11 还是别的入口
 * 触发)已被 setResizable(false) 消带,维持 4px;非全屏——不论是查看器沉浸还是窗口化设置
 * 触发的隐藏——resizable 恒为 true、死区恒在场,统一用加宽的 8px(见 CHROME_REVEAL_EDGE_PX_WINDOWED 注)。
 * 与 chromeAutoHidden 同款「惰性读 store」惯用法:仅在被调用时取 store,避免模块加载时
 * pinia 尚未安装。
 *
 * ⚠ 仅供本文件内部与测试使用(export 是为了让 spec 直接钉分档判据,不是给消费组件用的公共 API)。
 */
export function activeRevealEdgePx(): number {
  return isFullscreen.value ? CHROME_REVEAL_EDGE_PX : CHROME_REVEAL_EDGE_PX_WINDOWED
}

/** chrome 所在的侧。顶侧在分离模式下有**两条**(标题栏 + 页面工具栏),底侧恒一条。 */
export type ChromeSide = 'top' | 'bottom'

/**
 * 同侧 chrome 的标记选择器。用 data 属性而非 class:与样式解耦、可 grep、抗改名
 * (同 useWindowDrag 的 data-window-drag-surface 取舍)。
 * 两个用途:① 量该侧 chrome 的布局高(收起边界);② 判焦点是否落在该侧内(键盘唤出的豁免)。
 */
export const CHROME_SIDE_SELECTOR: Record<ChromeSide, string> = {
  top: '[data-chrome-top]',
  bottom: '[data-chrome-bottom]',
}

/** 收起顶栏。 */
export function collapseTop(): void {
  top.value = false
}

/** 收起底栏。 */
export function collapseBottom(): void {
  bottom.value = false
}

/**
 * 该侧 chrome 唤出后占据的**布局**高(px):同侧全部条的 offsetHeight 之和。
 * 用 offsetHeight 而非 getBoundingClientRect:前者是布局高、不含 transform,故在 0.18s 滑入动画
 * **进行中**读也恒是终值;后者会读到动画中途的位置,收起边界会随动画漂移。
 * 合并模式下 .app-toolbar 因 v-if 不在 DOM 中 → 自然不计入,无需条件分支。
 */
export function sideChromeExtent(side: ChromeSide): number {
  if (typeof document === 'undefined') return 0
  let h = 0
  document.querySelectorAll<HTMLElement>(CHROME_SIDE_SELECTOR[side]).forEach((el) => {
    h += el.offsetHeight
  })
  return h
}

/**
 * 指针/焦点的**去向**是否仍落在同侧的某条 chrome 内。
 * 鸭子类型判 closest(而非 instanceof Element):node 测试环境无 DOM 全局 Element,且对 null 安全
 * (同 useWindowDrag.matchesExclude 的理由)。
 */
function withinSide(side: ChromeSide, node: EventTarget | Node | null): boolean {
  const el = node as (Element & { closest?: (s: string) => Element | null }) | null
  return (
    el != null && typeof el.closest === 'function' && el.closest(CHROME_SIDE_SELECTOR[side]) != null
  )
}

/** 焦点当前是否在该侧 chrome 内 —— 是则指针路过不收(见 shouldCollapseByPointer)。 */
function focusWithinSide(side: ChromeSide): boolean {
  if (typeof document === 'undefined') return false
  return withinSide(side, document.activeElement)
}

/**
 * 几何收起判据(纯函数,便于单测):指针已越出该侧 chrome 所占的高度即应收起。
 * @param side 侧
 * @param clientY 指针纵坐标
 * @param extent 该侧 chrome 的布局高(见 sideChromeExtent)
 * @param viewportH 视口高
 */
export function shouldCollapseByPointer(
  side: ChromeSide,
  clientY: number,
  extent: number,
  viewportH: number,
): boolean {
  return side === 'top' ? clientY > extent : clientY < viewportH - extent
}

/**
 * 宿主的「焦点移出即收起」判据:焦点去向仍落在**同侧任一条** chrome 内则不收
 * (条内 Tab / 分离模式下从标题栏 Tab 进页面工具栏——标记打在宿主本身,故 closest 一并覆盖两种)。
 * @param side 本条所在侧
 * @param to 焦点去向(FocusEvent.relatedTarget)
 */
export function shouldCollapseOnFocusOut(side: ChromeSide, to: Node | null): boolean {
  if (!chromeAutoHidden.value) return false
  return !withinSide(side, to)
}

/**
 * 装配边缘唤出的全局监听。全应用调用**一次**(App.vue)——监听是 window 级,装两份即双份开销。
 * 仅在 chrome 自动隐藏期挂载;退出即卸载并复位两条的唤出态。
 * @param onKeyboardRevealTop F10 键盘唤出后的回调(宿主据此把焦点送进顶栏)
 */
// 该侧 chrome 的布局高缓存:唤出时与 resize 时重算(高度是常量,不必每次 pointermove 读布局)。
// 模块级而非闭包级:iframe 内的指针也要喂进同一套判据(见 notifyPointerY),两个入口须共用同一份量尺。
let topExtent = 0
let bottomExtent = 0

function measureExtents() {
  topExtent = sideChromeExtent('top')
  bottomExtent = sideChromeExtent('bottom')
}

/**
 * 一次指针更新的判定材料。**唤出与收起读的是不同的量,这是本模块的核心不对称**:
 *   · **唤出是「事件」** —— 本帧**有没有碰到过**边缘 → 读 nearestTop / nearestBottom(全部原始采样点的极值);
 *   · **收起是「状态」** —— 你**现在**在哪 → 读 y(被派发事件的当前位置)。
 */
interface PointerSample {
  /** 当前位置(被派发事件的 clientY,视口坐标系)。 */
  y: number
  /** 本帧全部原始采样点里**最接近顶缘**的那个 y。 */
  nearestTop: number
  /** 本帧全部原始采样点里**最接近底缘**的那个 y。 */
  nearestBottom: number
  buttons: number
}

/**
 * 指针位置的**唯一**判定入口。两个喂食口共用:
 *   ① 父文档的 window capture 监听(useChromeRevealListeners);
 *   ② foliate iframe 内的指针(notifyPointerY)—— 指针事件**不跨 browsing context**,阅读页
 *      flow="scrolled" 时 iframe 直抵 y=0,那一片父页收不到任何 pointermove(2026-07-16 查明)。
 */
function applyPointer(p: PointerSample) {
  // buttons !== 0 = 按键拖拽中(看图平移 / 画廊框选)。拖到边缘不算「移入」,否则平移一张大图
  // 甩到顶边就会弹出顶栏(2026-07-10 判据,此处沿用并推广到底边)。
  if (p.buttons !== 0) return
  const viewportH = window.innerHeight

  // 唤出:本帧**曾经**贴边即可(而非「被派发的那个采样点恰好贴边」)。首次唤出时量一次该侧高度。
  // 唤出带宽度按窗口态分档(见 activeRevealEdgePx 注):全屏 4px,非全屏(查看器沉浸/窗口化设置)8px。
  const edgePx = activeRevealEdgePx()
  const touchedTop = p.nearestTop <= edgePx
  const touchedBottom = !touchedTop && p.nearestBottom >= viewportH - edgePx
  if (touchedTop) {
    if (!top.value) {
      measureExtents()
      top.value = true
    }
  } else if (touchedBottom) {
    if (!bottom.value) {
      measureExtents()
      bottom.value = true
    }
  }
  const y = p.y

  // 收起:指针越出该条所占区域。三条不收:
  //  ① 本帧刚碰过该侧边缘(touchedTop/Bottom)。**这条不可省**:唤出读「本帧碰到过」、收起读「现在在哪」,
  //     二者在同一帧内会打架 —— 快甩到顶且回弹超过 extent 时,上面刚置真、下面立刻收掉,净效果是
  //     顶栏永不出现。同一帧里「碰到边缘」是意图、「已经离开」只是它的尾迹,故意图优先(2026-07-16)。
  //  ② 指针仍在该条所占区域内(几何判据,见 shouldCollapseByPointer)。
  //  ③ 焦点在条内 —— 用户是键盘唤出的(F10)或点了条内控件,指针恰好路过不该把它收掉;
  //     收起交给 focusout / 再按 F10。
  if (
    top.value &&
    !touchedTop &&
    shouldCollapseByPointer('top', y, topExtent, viewportH) &&
    !focusWithinSide('top')
  )
    collapseTop()
  if (
    bottom.value &&
    !touchedBottom &&
    shouldCollapseByPointer('bottom', y, bottomExtent, viewportH) &&
    !focusWithinSide('bottom')
  )
    collapseBottom()
}

/**
 * 从一次 pointermove 里取出判定材料。
 *
 * **为什么读 getCoalescedEvents**:Chromium 把每帧的多个原始鼠标移动**合并成一次派发**,
 * 派发事件携带的是**最后**那个位置——只看派发点会漏掉本帧内曾经贴边又弹开的中间采样。
 * 更正(2026-07-16):此前曾把"合并吞掉 y≈0"当作「快甩到顶不出顶栏」的根因,并据此解释
 * 阅读页/画廊快甩不对称。真机根因另有其人(见 sampleFrom 调用点 attach() 内 pointerrawupdate
 * 注,777824d)——那条不对称的真实来源是页面负载无关的顶缘输入死区,不是合并丢样本。
 * 保留 getCoalescedEvents 是因为它仍是正确、无害的加固(读取浏览器已产出但被合并的样本本就
 * 应该做),不是因为它曾"解决"过这个 bug。
 *
 * @param e 被派发的 pointermove(或 iframe 内的同款)
 * @param offsetY 加到各采样点上的偏移(iframe 侧传其 rect.top 以换算到父页视口系;父页侧传 0)
 */
function sampleFrom(e: PointerEvent, offsetY = 0): PointerSample {
  const y = e.clientY + offsetY
  let nearestTop = y
  let nearestBottom = y
  // getCoalescedEvents:Chromium/Firefox 有;缺失则退化为只看派发点(与旧行为一致,不更坏)。
  const raw = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : []
  for (const s of raw) {
    const sy = s.clientY + offsetY
    if (sy < nearestTop) nearestTop = sy
    if (sy > nearestBottom) nearestBottom = sy
  }
  return {
    y,
    nearestTop,
    nearestBottom,
    buttons: e.buttons,
  }
}

/**
 * 由 iframe 内的指针事件喂入(BookReader 在每节文档上装)。
 * @param e iframe 内的 pointermove
 * @param frameTop 该 iframe 在**父页视口**坐标系下的 rect.top(用于换算)
 */
export function notifyPointerFromFrame(e: PointerEvent, frameTop: number): void {
  if (!chromeAutoHidden.value) return
  applyPointer(sampleFrom(e, frameTop))
}

export function useChromeRevealListeners(onKeyboardRevealTop?: () => void): void {
  const measure = measureExtents

  function onPointerMove(e: PointerEvent) {
    applyPointer(sampleFrom(e))
  }

  // F10 键盘路径:无指针时的唯一唤出手段。两条一起翻——
  // 任一收起即「全部唤出」,全部唤出才「全部收起」;键盘用户不必记两个键。
  // F10 是 App 层 chrome 行为、无 tooltip 展示面,有意不入命令 keybinding 注册表(P5-6 豁免点)。
  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'F10') return
    e.preventDefault()
    const next = !(top.value && bottom.value)
    if (next) measure()
    top.value = next
    bottom.value = next
    if (next) onKeyboardRevealTop?.()
  }

  function onResize() {
    measure()
  }

  function attach() {
    // **capture 阶段**:唤出是全局 chrome 能力,不可被任何组件的局部事件卫生掐断。
    // 真机实证(2026-07-16):TimelineScrubberCanvas 的 .tlc-viewport 挂了 @pointermove.stop,
    // 而画廊滚到顶时该矩形正压在 y=0 的唤出带上 → 冒泡期监听在那一片**完全收不到事件**,
    // 表现为「换个横坐标进入就正常」的偶发失效。capture 先于 target/bubble,天然免疫,
    // 且对未来任何新增的 .stop 同样免疫(与 useFullscreenExitGuard 用 capture 同理)。
    window.addEventListener('pointermove', onPointerMove, true)
    // **pointerrawupdate**:非 rAF 对齐,原始输入到达即派发,采样密度高于 pointermove。
    // 更正(2026-07-16):此前曾把它当作「快甩到顶不出顶栏」的正解,推断是 rAF 排队吞掉了 y≈0 那发。
    // 真机根因另有其人——tauri-runtime-wry 给 undecorated 窗口盖的隐形 drag-resize 子窗口
    // 在屏幕顶缘拦了一条 ≈4px 输入带,全屏不撤,WebView 压根收不到那条带内的采样(见
    // useWindowMode.ts applyFullscreen 的 setResizable(false) 修法,777824d)。排队理论已被
    // F9 探针的 pointerout(relatedTarget=null)物证推翻。此处保留 pointerrawupdate 是因为
    // 它本身仍是真实增益(更密的采样对手抖/高速甩动始终有利),不是因为它曾"解决"过这个 bug。
    // 可用性:Chromium 系(WebView2/Windows、Android)有;WebKit(mac/iOS)无 → 特性探测后退回
    // 纯 pointermove(即当前行为,不更坏)。两条同时挂无妨:applyPointer 幂等(碰到过就出、离开才收)。
    if ('onpointerrawupdate' in window) {
      window.addEventListener('pointerrawupdate', onPointerMove as EventListener, true)
    }
    window.addEventListener('keydown', onKeydown)
    window.addEventListener('resize', onResize)
  }

  function detach() {
    window.removeEventListener('pointermove', onPointerMove, true)
    window.removeEventListener('pointerrawupdate', onPointerMove as EventListener, true)
    window.removeEventListener('keydown', onKeydown)
    window.removeEventListener('resize', onResize)
  }

  // immediate:进入本 composable 时 chromeAutoHidden 可能已为真(如开机即全屏的将来场景);
  // 非 immediate 则那种情形下 attach 永不执行、唤出永久失效。复位+装卸都幂等,immediate 无副作用。
  watch(
    chromeAutoHidden,
    (hidden) => {
      // 退出沉浸/全屏 → 两条回归常规布局,唤出态必须复位:否则下次进入时它们会「已经是唤出的」。
      top.value = false
      bottom.value = false
      if (hidden) attach()
      else detach()
    },
    { immediate: true },
  )

  onBeforeUnmount(detach)
}

/** 仅供测试:复位模块级单例状态。 */
export function __resetChromeRevealForTest(): void {
  top.value = false
  bottom.value = false
}
