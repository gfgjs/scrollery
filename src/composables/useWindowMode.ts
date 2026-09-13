// src/composables/useWindowMode.ts
// 窗口三态(normal / maximized / fullscreen)的**唯一所有者**。模块级单例(非 per-component 状态):
// 窗口只有一个,其模式也只该有一份真相。
//
// 为什么要有这个模块(2026-07-16 真机 round11 #8 根因):此前窗口态**无主**,三个碎片各持一角、
// 互不知情 ——
//   ① uiStore.isFullscreen:只写 ref,无 OS 回同步(转换失败/OS 侧改变全屏都不会纠正它);
//   ② WindowChrome.isMaximized:局部 ref,DOM resize 驱动,不知全屏;
//   ③ useWindowDrag:两者都不知,无条件 startDragging() / toggleMaximize()。
// 没有主 → 没人写下「三态互斥」这条不变量 → 真机四个症状全是它的直接推论:
//   · 全屏态仍能按住标题栏把窗口拖着走(①③ 失联);
//   · 最大化 + F11 → 窗口铺满但底部一条黑边(未先卸最大化态就转全屏,见 setFullscreen 注);
//   · 拖拽把窗口拽出全屏后再也贴不了顶最大化(isFullscreen 停在臆测的 true,与 OS 脱钩);
//   · 全屏仍留标题栏(全屏从未接过沉浸态)。
// 故本模块的职责不是「多一层封装」,而是**让三态互斥成为结构上唯一可表达的形状**:
// mode 是单一 computed,isFullscreen / isMaximized 都从它派生,不存在「既全屏又最大化」的可表示状态。

import { computed, type ComputedRef } from 'vue'
import { ref } from 'vue'
import { getAppWindow } from '../utils/appWindow'
import { logger } from '../utils/logger'

/** 窗口三态。互斥——全屏优先级最高(全屏期间 OS 侧的「最大化」标志无语义,见 mode)。 */
export type WindowMode = 'normal' | 'maximized' | 'fullscreen'

/** 全屏态。唯一写点=setFullscreen / syncFromOs(OS 回读),不接受任何其它来源的臆测值。 */
const fullscreen = ref(false)

/** OS 侧观测到的最大化标志。**不直接对外**——全屏期间它无语义,对外只暴露从 mode 派生的 isMaximized。 */
const osMaximized = ref(false)

/**
 * 入全屏前窗口是否处于最大化 —— 退全屏时据此还原,使「最大化 → F11 → F11」回到最大化而非标准窗口。
 * 不是 ref:纯粹的转换内部记忆,无任何 UI 读它。
 */
let restoreMaximized = false

/**
 * 转换互斥闸。一次转换未落地前**丢弃**(而非排队)后续请求。
 * 为什么丢弃而非排队:F11 被按住时键盘自动重复会灌进几十次 toggle,排队则窗口会全屏/退出来回抽搐几秒;
 * 丢弃则最多损失一次「快速连按两下 F11 应回到原态」——而转换本身 <100ms,人手连按极难落进窗内。
 */
let busy = false

/**
 * 窗口当前模式。三态互斥的**结构性**保证:全屏优先——即便 OS 在全屏期间仍报 IsZoomed=true
 * (Windows 下确有此情形),也绝不会算成 'maximized'。
 */
export const windowMode: ComputedRef<WindowMode> = computed(() =>
  fullscreen.value ? 'fullscreen' : osMaximized.value ? 'maximized' : 'normal',
)

/** 是否全屏。UI 读这个,不读内部 ref。 */
export const isFullscreen: ComputedRef<boolean> = computed(() => windowMode.value === 'fullscreen')

/** 是否最大化(驱动窗口三键的「方块 ↔ 还原」图标)。全屏态恒 false——三态互斥。 */
export const isMaximized: ComputedRef<boolean> = computed(() => windowMode.value === 'maximized')

/**
 * 「移窗拖拽 / 双击最大化」当前是否允许。全屏态恒 false:全屏下窗口没有可移动的位置,
 * 把它拖着走只会得到一个「尺寸还是全屏、位置却错位」的窗口(真机 round11 #8-2 实证)。
 * useWindowDrag 直接读它,故这条不变量不可被调用方遗忘。
 */
export const canDragWindow: ComputedRef<boolean> = computed(() => !isFullscreen.value)

function warn(op: string, err: unknown) {
  logger.warn(`[useWindowMode] ${op} failed`, { error: err })
}

/** 从 OS 回读三态真相。任何转换收尾 / DOM resize 后都走它,绝不让 ref 停在臆测值。 */
async function syncFromOs(): Promise<void> {
  const appWindow = getAppWindow()
  try {
    fullscreen.value = await appWindow.isFullscreen()
  } catch {
    // 非 Tauri(纯浏览器 dev)降级读 DOM 全屏。
    fullscreen.value = typeof document !== 'undefined' && !!document.fullscreenElement
  }
  try {
    osMaximized.value = await appWindow.isMaximized()
  } catch {
    // 无权限 / 非 Tauri:降级 false。只影响三键图标,不影响关闭/最小化功能。
    osMaximized.value = false
  }
}

/**
 * 真机转换。**先卸最大化再进全屏**是本函数存在的核心理由 ——
 * 2026-07-16 用户真机 A/B:标准窗口按 F11 全屏完全正常;最大化窗口按 F11 则窗口铺满、任务栏正确隐藏
 * (图标消失),但底部残留一条未绘制的纯黑边 = webview 客户区仍按旧的「工作区」高度(屏高减任务栏)排布。
 * 该 A/B 的唯一自变量就是「进全屏前是否已最大化」,故修法是把这个自变量消掉:转换前先退出最大化态,
 * 让全屏永远从标准窗口态起步(=已证正常的那条路径)。退出时再按 restoreMaximized 还原。
 * (Windows/tao 内部为何如此 —— 疑与 undecorated 窗口最大化态下的 WM_NCCALCSIZE 边框内缩有关,
 *  **未经证实**;但修法只依赖上述 A/B,不依赖该猜测成立。)
 */
async function applyFullscreen(on: boolean): Promise<void> {
  const appWindow = getAppWindow()
  if (on) {
    restoreMaximized = await appWindow.isMaximized()
    if (restoreMaximized) await appWindow.unmaximize()
    await appWindow.setFullscreen(true)
    // 根除全屏顶缘输入死区(2026-07-16 真机「顶栏唤出不稳定」根因):tauri-runtime-wry 给
    // undecorated+resizable 窗口盖了一个 HWND_TOP 的隐形子窗口(TAURI_DRAG_RESIZE_WINDOW,
    // undecorated_resizing.rs),shadow=true 时其命中区=顶部一条 SM_CYFRAME(≈4 物理像素)全宽横带,
    // WM_NCHITTEST 恒 HTTOP,且只在 maximized 才撤、全屏不撤 —— 光标进带即离开 WebView
    // (pointerout relatedTarget=null),F9 探针实测 clientY 地板 3~5、y=0 永不可达,
    // 与 4px 唤出带正好骑线 = 「有时出有时不出」+「横移(带内滑行)必不出」的全部来源。
    // SetResizable(false) 的运行时处理器恰好 detach 该子窗口(tauri-runtime-wry/src/lib.rs),
    // 全屏本无 resize 语义,故这是零 Win32、纯公开 API 的根治。
    // 次序不变量:resizable=false 严格嵌在**成功的**全屏区间内(进全屏成功后才禁,退全屏前先恢复),
    // setResizable 自身失败只降级(死区回归),绝不拖垮全屏转换。
    try {
      await appWindow.setResizable(false)
    } catch (err) {
      warn('setResizable(false)', err)
    }
  } else {
    try {
      await appWindow.setResizable(true)
    } catch (err) {
      warn('setResizable(true)', err)
    }
    await appWindow.setFullscreen(false)
    if (restoreMaximized) await appWindow.maximize()
    restoreMaximized = false
  }
}

/** 纯浏览器(vite dev,无 Tauri 边界)降级:走 DOM 全屏 API,无最大化概念。 */
async function applyFullscreenDom(on: boolean): Promise<void> {
  if (typeof document === 'undefined') return
  if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen()
  if (!on && document.fullscreenElement) await document.exitFullscreen()
}

/**
 * 进入 / 退出全屏。转换期间闸住重入(见 busy)。
 * @param on true=进全屏(先卸最大化),false=退全屏(还原入全屏前的最大化态)
 */
export async function setFullscreen(on: boolean): Promise<void> {
  if (busy) return
  busy = true
  try {
    try {
      await applyFullscreen(on)
    } catch (err) {
      warn(on ? 'enterFullscreen' : 'exitFullscreen', err)
      await applyFullscreenDom(on)
    }
  } catch (err) {
    warn('setFullscreen(dom fallback)', err)
  } finally {
    busy = false
  }
  // 恒从 OS 回读收尾:成功则确认,失败则自愈——旧 uiStore 的「乐观赋值 + 出错也不纠正」正是
  // 「拖出全屏后 isFullscreen 仍是 true、从此贴顶最大化失效」的来源。
  await syncFromOs()
}

/** 切换全屏(F11 / 工具栏按钮)。 */
export async function toggleFullscreen(): Promise<void> {
  await setFullscreen(!isFullscreen.value)
}

/**
 * 切换最大化(窗口三键的最大化键 / 双击裸露标题面)。
 * 全屏态下「最大化 ↔ 还原」无意义(三态互斥)——此时用户点的那个键在语义上就是「还原」,
 * 故退出全屏(并还原入全屏前的态)。这是**显式点击有标签控件**的路径,与「拖拽/双击在全屏下
 * 一律惰性」(canDragWindow)有意分流:前者意图明确,后者是误触。
 */
export async function toggleMaximize(): Promise<void> {
  if (isFullscreen.value) {
    await setFullscreen(false)
    return
  }
  try {
    await getAppWindow().toggleMaximize()
  } catch (err) {
    warn('toggleMaximize', err)
  }
  await syncFromOs()
}

// ── OS 侧变化的回同步 ────────────────────────────────────────────────────────
// 为什么用 DOM resize 而非 Tauri 的 onResized:纯 DOM 事件零 ACL 依赖(沿用 WindowChrome 原有取舍)。
// 窗口最大化/还原/全屏/边缘拖拽都改视口尺寸 → 都会触发。
// rAF 合并:边缘拖拽调窗时 resize 逐帧连发,不合并则每帧灌 2 次 IPC(旧 WindowChrome 就是逐事件直调
// isMaximized(),此处顺带收敛)。
let syncScheduled = false

function scheduleSync() {
  if (syncScheduled) return
  syncScheduled = true
  requestAnimationFrame(() => {
    syncScheduled = false
    // 自身转换进行中:OS 尚未落定,此刻回读会读到中间态。转换自己会在收尾时 syncFromOs。
    if (busy) return
    void syncFromOs()
  })
}

let inited = false

/**
 * 装配窗口态的 OS 回同步。全应用调用**一次**(AppShell onMounted)。
 * 幂等:重复调用不重复挂监听(HMR / 测试重入安全)。
 */
export async function initWindowMode(): Promise<void> {
  await syncFromOs()
  if (inited || typeof window === 'undefined') return
  inited = true
  window.addEventListener('resize', scheduleSync)
}

/** 仅供测试:复位模块级单例状态。 */
export function __resetWindowModeForTest(): void {
  fullscreen.value = false
  osMaximized.value = false
  restoreMaximized = false
  busy = false
  syncScheduled = false
}
