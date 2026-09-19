export type UiHarnessScene = 'gallery' | 'settings' | 'viewer'

// Vitest node 环境无 window;本模块被 appWindow/appEvents 等适配层传递 import,顶层取值必须环境守卫。
const params =
  typeof window === 'undefined' ? null : new URLSearchParams(window.location.search)

const requestedScene = params?.get('ui-harness') ?? null

/** browser UI harness 只在 Vite dev 中启用，production/Tauri 永远走真实边界。 */
export const uiHarnessScene: UiHarnessScene | null =
  import.meta.env.DEV &&
  (requestedScene === 'gallery' || requestedScene === 'settings' || requestedScene === 'viewer')
    ? requestedScene
    : null

export const isUiHarness = uiHarnessScene !== null

/**
 * `&items=<N>` — Gallery 极密网格性能场景。默认视觉 fixture 仍为 18 张；只有 DEV harness
 * 显式传参才扩容，且封顶 10000，避免误输入让浏览器一次构造无界数据。
 */
const requestedItems = Number.parseInt(params?.get('items') ?? '', 10)
export const uiHarnessItems: number | null =
  isUiHarness && Number.isFinite(requestedItems)
    ? Math.min(10_000, Math.max(18, requestedItems))
    : null

/**
 * `&rowHeight=<n>` — 覆盖 fixture 行高(perf 场景默认 60,视觉场景默认 220)。Canvas 换图波
 * 基准需要在「默认密度 ~200px」与「极密 60px」两档间脚本化切换,故与 items 同权做成参数;
 * 钳到产品滑杆同量级的安全区间,防误输入产生病态布局。
 */
const requestedRowHeight = Number.parseInt(params?.get('rowHeight') ?? '', 10)
export const uiHarnessRowHeight: number | null =
  isUiHarness && Number.isFinite(requestedRowHeight)
    ? Math.min(960, Math.max(40, requestedRowHeight))
    : null

/**
 * `&thumbStatus=0|3` — 覆盖 fixture 条目的初始缩略图状态(默认 3 走原图)。「未生成冷库
 * 首览」基准场景置 0,配合 ipcFixtures 对 BATCH_REQUEST_THUMBNAILS 的模拟生成,走完整的
 * 「请求→生成→回填→sig 变→重载」链路。仅接受 0/3(harness 只面向这两态,其余忽略)。
 */
const requestedThumbStatus = Number.parseInt(params?.get('thumbStatus') ?? '', 10)
export const uiHarnessThumbStatus: 0 | 3 | null =
  isUiHarness && (requestedThumbStatus === 0 || requestedThumbStatus === 3)
    ? requestedThumbStatus
    : null

/**
 * `&avail=offline|missing` + `&availRatio=<0..1>` — 把前 ratio 比例的条目标为离线/缺失
 * (默认全 online)。offline/missing 高占比资源集的绘制成本基准场景用;ratio 钳到 [0,1]。
 */
export const uiHarnessAvail: 'offline' | 'missing' | null =
  isUiHarness && (params?.get('avail') === 'offline' || params?.get('avail') === 'missing')
    ? (params.get('avail') as 'offline' | 'missing')
    : null
const requestedAvailRatio = Number.parseFloat(params?.get('availRatio') ?? '')
export const uiHarnessAvailRatio: number =
  isUiHarness && Number.isFinite(requestedAvailRatio)
    ? Math.min(1, Math.max(0, requestedAvailRatio))
    : 0

/**
 * `&appearance=light|dark` — 主题截图矩阵用:指定 harness 呈现哪一档配色。
 *
 * 只承载 URL 里的**原始字符串**,不校验取值:本模块是「读 window 的零依赖入口」,引入主题层
 * 会让它的顶层副作用传染给消费方(9 个 spec 崩溃的既有教训,见 findings 会话续)。合法性判定在
 * 消费方(ipcFixtures),非法值落回 fixture 默认。
 */
export const uiHarnessAppearance: 'light' | 'dark' | null =
  isUiHarness && (params?.get('appearance') === 'light' || params?.get('appearance') === 'dark')
    ? (params.get('appearance') as 'light' | 'dark')
    : null

/**
 * `&seed=custom` — 截图矩阵的「明显自定义配色」档:消费方把两套配色换成一组刻意偏离默认的
 * 种子(仍走真实 generateTheme 生成),用于人眼核对任意用户配色下的观感。仅认字面量 custom。
 */
export const uiHarnessSeed: 'custom' | null =
  isUiHarness && params?.get('seed') === 'custom' ? 'custom' : null

/**
 * `&render=dom|canvas` — 画廊渲染引擎档:DOM 与 Canvas 两条绘制路径必须都出图(色板同源是
 * 生成层的契约,但两条路径的绘制实现不同,只有分别截图才能看出差异)。
 */
export const uiHarnessRenderMode: 'dom' | 'canvas' | null =
  isUiHarness && (params?.get('render') === 'dom' || params?.get('render') === 'canvas')
    ? (params.get('render') as 'dom' | 'canvas')
    : null
