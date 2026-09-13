// src/composables/useWindowDrag.ts
// 自绘标题栏「整行按住拖动移窗」——单一委托处理器,按命中目标三路分流。
//
// 为什么不用 data-tauri-drag-region:该属性有两条硬限制 ——
//   ① 只作用于 mousedown 命中的「那个元素本身」(不冒泡到子元素),故按钮/输入框等交互控件
//      天然不在拖拽面上;窄窗口下控件占满整行时,只剩裸露的 logo/空白能拖。
//   ② Tauri 在 mousedown 瞬间即 startDragging(),没有位移阈值 → 一旦落在交互控件上就会立刻
//      进入 OS 移窗循环、吞掉后续 click,无法区分「点击」与「按住拖动」。
// 全屏态一律惰性(2026-07-16):三态互斥——全屏下窗口没有可移动的位置,也没有「更大」可最大化。
//   闸门读 useWindowMode.canDragWindow(窗口三态唯一所有者),而非由调用方传参:传参可被遗忘,
//   直读则这条不变量结构上不可绕过。
// 本 composable 挂在标题栏根上、靠 pointerdown 冒泡覆盖整行,按命中目标三路分流:
//   路径 A 排除(isDragExcluded):表单控件 / data-no-window-drag → 不拦,走各自手势。
//   路径 B 纯空隙面(isDragSurface,e.target 自身即被标记的空隙容器)→ 无点击歧义,pointerdown
//         「即时」startDragging → 打滑只剩 OS 移窗循环那一截(与 VSCode 齐平)。双击留给 onDblClick 最大化。
//   路径 C 其余(按钮 / 可点 div / 文本)→ 阈值路径:按下先不拖,「移动越阈 距离(2px)OR 按住到点降阈」
//         才 startDragging;未触发即松手 → 浏览器照常派发 click,按钮/控件原动作生效。
//
// 为什么 isDragSurface 用 matches 而非 closest:closest 会把「被标记空隙容器的所有后代」都算成拖拽面
//   → 容器内的可点 div(如混合搜索下拉项)会被误判为空隙、被即时移窗吞掉点击。matches 只在 e.target
//   「自身」就是被标记容器(即命中的是容器本体的空隙/padding,而非其某个后代)时才成立,后代按钮/可点
//   div/文本一律落进安全的阈值路径 C。由此得 fail-safe 默认:未显式标记者一律走阈值(按下不动=点击,
//   永不吞),将来新增任何可点元素自动安全,无需记得逐个豁免。

import { onBeforeUnmount, onMounted, type Ref } from 'vue'
import { getAppWindow } from '../utils/appWindow'
import { logger } from '../utils/logger'
import { canDragWindow, toggleMaximize as setWindowMaximizeToggled } from './useWindowMode'

/**
 * 阈值路径(路径 C:按钮/可点 div/文本)的**距离**分量(px):按下后指针位移**超过**此值即判定为
 * 「拖动」并 startDragging();未超过即松手 → 判定为点击。取 2 而非 1:1px 易被点击时的手抖误判成拖动,
 * 2px 能过滤 (1,1) 级抖动((1,1) 的平方和=2,不越 2²=4)。纯空隙面(路径 B)不经此值——它无点击歧义、
 * pointerdown 即拖。特意与 usePointerDrag 的 DRAG_THRESHOLD(=5,选区框选 / 拖入文件夹共享)**解耦**:
 * 单独调此值不牵连那些手势。
 */
export const WINDOW_DRAG_THRESHOLD = 2

/**
 * 阈值路径的**按住时间**分量(ms):按下后原地按住达此时长,即把有效距离阈值降到 0——之后**任意一次
 * 移动**(≥1px)即起拖,近乎零打滑(决策②「距离 OR 按住时间」的时间支)。采 gentle 语义:仅「降阈值」、
 * **不**预先 startDragging,故「按住到点后原地松手」仍照常派发 click(命令按钮的撤销/重做等不被吞)。
 * 取 300ms 偏保守以护住「慢速点击」(其多在 300ms 内松手→走不到降阈);想让按钮拖动的零打滑更易触发
 * 可调小,但过小会让带手抖的慢速点击被误判成拖动。**与距离阈值同为手感旋钮**。
 */
export const WINDOW_DRAG_HOLD_MS = 300

/**
 * 「纯空隙拖拽面」标记选择器(路径 B)。**只**打在确定无点击语义的空隙/布局容器上(标题栏各 flex 容器
 * 的间隙 / 品牌区 / 面包屑),使命中其本体(而非后代)时即时移窗。用属性而非 class:与样式解耦、可 grep、
 * 抗 class 改名。判定见 isDragSurface(用 matches 只认 e.target 自身,不认后代——见文件顶注)。
 */
export const WINDOW_DRAG_SURFACE_SELECTOR = '[data-window-drag-surface]'

/**
 * 空隙面(路径 B)双击最大化的时间窗(ms):两次按下间隔 ≤ 此值(且位移在 SLOP 内)判为双击。
 * 为什么手动判双击而非用 DOM dblclick / e.detail:路径 B 首击即 startDragging → OS 移窗循环打断浏览器
 * click 计数 → dblclick 事件不派发、pointerdown 的 detail 也到不了 2。取 500(≈Windows 默认双击时长)。
 */
export const WINDOW_DBLCLICK_MS = 500

/** 空隙面双击判定的位移容差(px):两次按下相距超过此值即视为两次独立单击,不合成双击。 */
export const WINDOW_DBLCLICK_SLOP = 4

/**
 * 排除出「移窗拖拽」的元素:自带指针手势者按下应走各自手势,不抢去移窗。
 * - `input`/`textarea`/`select`/`contenteditable`:行高滑块(range)、搜索输入框、分组·排序下拉
 *   的拖滑块 / 选文本 / 展下拉手势优先(用户裁决:排除所有原生表单控件)。
 * - `[data-no-window-drag]`:逃生舱——窗口三键等「有意排除」的区域挂此属性。
 * contenteditable="false" 显式关闭者不算(否则会误伤把 false 当可编辑)。
 */
export const DRAG_EXCLUDE_SELECTOR =
  'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [data-no-window-drag]'

/**
 * 排除出「双击最大化」的元素:比拖拽排除更宽——按钮虽是拖拽面(按住可拖),但双击按钮不应最大化
 * (复刻 data-tauri-drag-region 时代:双击落在按钮上不触发 detail===2 的内建最大化)。故这里额外
 * 排除一切可点交互元素(button / a[href] / data-no-window-drag),只让双击落在裸露标题面时才最大化。
 */
export const MAXIMIZE_EXCLUDE_SELECTOR =
  'button, a[href], input, textarea, select, [contenteditable]:not([contenteditable="false"]), [data-no-window-drag]'

/**
 * 命中目标是否落在某个「排除选择器」的元素(自身或祖先)内。
 * 鸭子类型判 `closest` 而非 `instanceof Element`:node 测试环境无 DOM 全局 `Element`,
 * `instanceof Element` 会 ReferenceError;判 `closest` 既真机可用、又对 null / 非元素安全。
 */
function matchesExclude(target: EventTarget | null, selector: string): boolean {
  const el = target as (Element & { closest?: (s: string) => Element | null }) | null
  return el != null && typeof el.closest === 'function' && el.closest(selector) != null
}

/** 命中目标是否应排除出移窗拖拽(表单控件 / data-no-window-drag)。 */
export function isDragExcluded(target: EventTarget | null): boolean {
  return matchesExclude(target, DRAG_EXCLUDE_SELECTOR)
}

/** 命中目标是否应排除出双击最大化(拖拽排除 + 一切可点交互元素)。 */
export function isMaximizeExcluded(target: EventTarget | null): boolean {
  return matchesExclude(target, MAXIMIZE_EXCLUDE_SELECTOR)
}

/**
 * 命中目标**自身**是否为被标记的「纯空隙拖拽面」(路径 B)。
 * 关键:用 `matches`(仅测 e.target 自身)而非 `closest`(会连带后代)——后者会把标记容器里的可点 div
 * 误判成空隙、即时移窗吞点击。鸭子类型判 `matches` 而非 `instanceof Element`:node 测试环境无 DOM 全局
 * `Element`,且对 null / 非元素安全(见 matchesExclude 同款理由)。
 */
export function isDragSurface(target: EventTarget | null): boolean {
  const el = target as (Element & { matches?: (s: string) => boolean }) | null
  return el != null && typeof el.matches === 'function' && el.matches(WINDOW_DRAG_SURFACE_SELECTOR)
}

/**
 * 位移是否已越过阈值、可判定为「拖动」。用欧氏距离平方比较(免开方),与方向无关。
 * @param dx 相对按下点的横向位移(px)
 * @param dy 相对按下点的纵向位移(px)
 * @param threshold 阈值(px),默认 WINDOW_DRAG_THRESHOLD
 */
export function shouldStartDrag(dx: number, dy: number, threshold = WINDOW_DRAG_THRESHOLD): boolean {
  return dx * dx + dy * dy > threshold * threshold
}

/**
 * 两次「空隙面按下」是否构成一次双击(路径 B 手动双击判定,抽为纯函数便于单测)。
 * @param elapsedMs 距上次空隙面按下的时长(ms);无上次按下时传 Infinity → 恒 false
 * @param dx 相对上次按下点的横向位移(px)
 * @param dy 相对上次按下点的纵向位移(px)
 * @param msWindow 双击时间窗(ms),默认 WINDOW_DBLCLICK_MS
 * @param slop 位移容差(px),默认 WINDOW_DBLCLICK_SLOP
 */
export function isDoubleClick(
  elapsedMs: number,
  dx: number,
  dy: number,
  msWindow = WINDOW_DBLCLICK_MS,
  slop = WINDOW_DBLCLICK_SLOP,
): boolean {
  return elapsedMs <= msWindow && Math.abs(dx) <= slop && Math.abs(dy) <= slop
}

/**
 * 让 `rootRef` 容器「整行按住拖动移窗」:委托监听 pointerdown,越阈值转 OS 移窗;
 * 双击裸露标题面触发 toggleMaximize(复刻 data-tauri-drag-region 的双击最大化/还原)。
 * @param rootRef 承载委托监听的容器 ref(自绘标题栏根)
 */
export function useWindowDrag(rootRef: Ref<HTMLElement | null>): void {
  const appWindow = getAppWindow()

  let startX = 0
  let startY = 0
  let armed = false // 路径 C:已按下、尚未触发的「待判定」态
  let holdElapsed = false // 按住时间已到点 → 有效距离阈值降为 0(gentle:仅降阈,不预先 startDragging)
  let holdTimer: ReturnType<typeof setTimeout> | null = null

  // 路径 B 手动双击判定:记录上次在空隙面按下的时间/坐标(为何手动见 path B 注释与 WINDOW_DBLCLICK_MS)。
  // ts 初值 -Infinity = 「无上次按下」→ 首击 elapsed=Infinity 恒非双击(免受 performance.now 起始值干扰)。
  let lastSurfaceDownTs = Number.NEGATIVE_INFINITY
  let lastSurfaceDownX = 0
  let lastSurfaceDownY = 0

  // 交给 OS 移窗循环。路径 B(pointerdown 即拖)与路径 C(越线)共用,统一告警口径。
  function startWindowDrag() {
    appWindow
      .startDragging()
      .catch((err: unknown) => logger.warn('[useWindowDrag] startDragging failed', { error: err }))
  }

  // 最大化 / 还原。路径 B 手动双击与路径 C 的 onDblClick 共用。
  // 转换本身委托 useWindowMode(窗口三态唯一所有者)——它负责三态互斥与 OS 回同步;
  // 本层只管手势判定,不再直接调 appWindow.toggleMaximize()(否则最大化态又多一个无主写点)。
  function toggleMaximize() {
    void setWindowMaximizeToggled()
  }

  function onPointerMove(e: PointerEvent) {
    if (!armed) return
    // 距离 OR 按住时间:按住到点(holdElapsed)后阈值降 0 → 任意移动即拖;否则须越 WINDOW_DRAG_THRESHOLD。
    const threshold = holdElapsed ? 0 : WINDOW_DRAG_THRESHOLD
    if (!shouldStartDrag(e.clientX - startX, e.clientY - startY, threshold)) return
    // 触发 → 判定为拖动:手势交给 OS 移窗循环,自身立即卸载监听。
    // (pointerup 常被 OS 移窗循环吞掉,不能依赖它清理,故此处主动 teardown。)
    teardown()
    startWindowDrag()
  }

  function onPointerUpOrCancel() {
    // 未触发即松手 / 取消 → 判定为点击:什么都不做,浏览器照常派发 click 到命中的按钮。
    // (gentle hold 从不预先 startDragging,故「按住到点后原地松手」也走这里 → click 不被吞。)
    teardown()
  }

  function teardown() {
    armed = false
    holdElapsed = false
    if (holdTimer !== null) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUpOrCancel)
    window.removeEventListener('pointercancel', onPointerUpOrCancel)
  }

  function onPointerDown(e: PointerEvent) {
    // 仅主指针左键。
    if (e.button !== 0 || !e.isPrimary) return
    // 全屏态:移窗与最大化俱无意义(三态互斥)。不加此闸则真机可按住标题栏把「全屏尺寸」的窗口
    // 拖着走(round11 #8-2),得到一个尺寸=全屏、位置却错位、且 OS 与 tao 内部态脱钩的窗口。
    // 闸在**最前**:三条路径(A/B/C)一并惰性——新需求「全屏沉浸态鼠标移入唤出顶栏」唤出的
    // 那条标题栏同样不可拖(用户 2026-07-16 明定「显示后不能拖动」),此处即其单一实现点。
    if (!canDragWindow.value) return
    // 路径 A:排除自带手势的控件(行高滑块 / 输入框 / 下拉 / 窗口三键 / 菜单类可点 div)。
    if (isDragExcluded(e.target)) return
    // 路径 B:命中纯空隙面**本体** → 无点击歧义,pointerdown 即交给 OS(打滑只剩 OS 那截,VSCode 级)。
    // 双击最大化手动判定:首击的 startDragging 会打断 DOM dblclick 序列(onDblClick 对空隙面失效)、
    // 且 pointerdown.detail 在 WebView2 里对双击不可靠——改按「上次按下时间+坐标」判定(复刻
    // data-tauri-drag-region 的 detail===2→toggleMaximize,但不依赖 detail / dblclick 事件)。
    if (isDragSurface(e.target)) {
      const now = performance.now()
      if (
        isDoubleClick(now - lastSurfaceDownTs, e.clientX - lastSurfaceDownX, e.clientY - lastSurfaceDownY)
      ) {
        lastSurfaceDownTs = Number.NEGATIVE_INFINITY // 消费本次双击,避免三击链成第二次 toggle
        toggleMaximize()
        return
      }
      // 单击:记录本次按下(供下一击比对),并即时交给 OS 移窗。首击的 startDragging 无位移=空操作,
      // 不影响随后第二击被判为双击(与 Tauri 首击 startDragging 同理)。
      lastSurfaceDownTs = now
      lastSurfaceDownX = e.clientX
      lastSurfaceDownY = e.clientY
      startWindowDrag()
      return
    }
    // 路径 C:按钮 / 可点 div / 文本 → 阈值 + 按住时间,越线才拖,否则松手成点击。
    startX = e.clientX
    startY = e.clientY
    armed = true
    holdElapsed = false
    holdTimer = setTimeout(() => {
      holdElapsed = true // 仅降阈;下一次 pointermove 以阈值 0 判定(见 onPointerMove)
    }, WINDOW_DRAG_HOLD_MS)
    // 监听挂 window(非 target):即便交互控件在 pointerdown 时 setPointerCapture,move/up 仍冒泡到 window。
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUpOrCancel)
    window.addEventListener('pointercancel', onPointerUpOrCancel)
  }

  function onDblClick(e: MouseEvent) {
    // 全屏态惰性(同 onPointerDown 的理由)。双击是**误触面**,故与显式点三键的最大化键有意分流:
    // 后者意图明确 → useWindowMode.toggleMaximize 会把它解释成「退出全屏」。
    if (!canDragWindow.value) return
    // 双击标题面 → 最大化 / 还原;落在按钮/输入等交互元素上不触发(见 MAXIMIZE_EXCLUDE_SELECTOR)。
    // 只覆盖**路径 C 的惰性元素**(品牌/标题文本等):它们不 startDragging → dblclick 事件正常派发。
    // 空隙面(路径 B)的 dblclick 已被首击 startDragging 打断,其最大化由 onPointerDown 内手动判定处理。
    if (isMaximizeExcluded(e.target)) return
    toggleMaximize()
  }

  onMounted(() => {
    const root = rootRef.value
    if (!root) return
    root.addEventListener('pointerdown', onPointerDown)
    root.addEventListener('dblclick', onDblClick)
  })

  onBeforeUnmount(() => {
    const root = rootRef.value
    root?.removeEventListener('pointerdown', onPointerDown)
    root?.removeEventListener('dblclick', onDblClick)
    teardown()
  })
}
