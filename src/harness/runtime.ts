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
 * `&bucket=1|0` — 覆盖 fixture 的 bucketSegmentedScroll(默认 'false' 保方案 A 视觉基线)。
 * 性能基准要在生产默认引擎(bucket)上跑才有代表性;缺省时维持既有视觉矩阵行为不变。
 */
export const uiHarnessBucket: boolean | null =
  isUiHarness && params?.has('bucket') ? params.get('bucket') !== '0' : null

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
 * `&theme=<id>` — 6 主题视觉矩阵截图用(S7)。仅承载 URL 里的**原始字符串**,不校验:
 * 合法性判定属注册表(themes/registry),此处引入会让本模块从「读 window 的零依赖入口」
 * 退化为依赖主题层——而本模块正因顶层副作用传染性被 9 个 spec 崩溃教训过(findings 会话续)。
 * 消费方(ipcFixtures)用 getTheme() 解析,非法 id 落回 fixture 默认。
 */
export const uiHarnessTheme: string | null = isUiHarness ? (params?.get('theme') ?? null) : null

/**
 * `&tint=<pct>` — 主题色浓度矩阵截图用(2026-09-06)。同 uiHarnessTheme:只承载原始字符串,
 * 合法性(20–100 数字)由消费方 ipcFixtures 判定,非法落回 fixture 默认(null→生产默认 60)。
 */
export const uiHarnessTint: string | null = isUiHarness ? (params?.get('tint') ?? null) : null

/**
 * `&text=<pct>` — 文字浓度矩阵截图用(2026-09-06)。同上:原始字符串进,合法性(40–100
 * 数字)由消费方 ipcFixtures 判定,非法落回 fixture 默认(null→生产默认 75)。
 */
export const uiHarnessText: string | null = isUiHarness ? (params?.get('text') ?? null) : null
