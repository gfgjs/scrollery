// src/utils/uiScale.ts
// UI 尺度 CSS 变量的**单一事实源**(2026-07-06 审查 P1-18)。
//
// 此前字号公式在两处各算一份:App.vue 启动应用用基准 (12,13,15,16,19,23,28)+diff(size-15),
// configStore.setUiFontSize 用 (12,13,16,17,20,24,30)+diff(size-16)——两式对任意 size 的
// xs/sm/2xl 恒差 1px,启动后一打开设置页(触发 config.loadConfig / setter)全局字号就跳 1px。
// 现统一到本模块,App.vue 与 setter 都调它,消除漂移。锚点选 base=13(与暗房方案的 UI
// 基准和 configStore 新装默认一致),相对偏移固定为 11/12/13/14/16/20/24 刻度。

/** 各档相对「基准字号」的像素偏移(base 恒等于用户设定值)。 */
const FONT_TIER_OFFSETS: Record<string, number> = {
  '--font-size-xs': -2,
  '--font-size-sm': -1,
  '--font-size-base': 0,
  '--font-size-md': 1,
  '--font-size-lg': 3,
  '--font-size-xl': 7,
  '--font-size-2xl': 11,
}

/**
 * 依据用户设定的基准字号(px)刷新全部 `--font-size-*` CSS 变量。
 * 启动应用(App.vue 从持久化配置)与实时设置(configStore.setUiFontSize)共用此函数。
 * @param basePx 基准字号(通常 12–24);非有限数静默忽略。
 */
export function applyUiFontSize(basePx: number): void {
  if (!Number.isFinite(basePx)) return
  const root = document.documentElement
  for (const [varName, offset] of Object.entries(FONT_TIER_OFFSETS)) {
    root.style.setProperty(varName, `${basePx + offset}px`)
  }
}

/**
 * 时间轴滚动条宽度 CSS 变量(`--scrollbar-width`)。同样在启动与 setter 两处应用,顺带收敛。
 * @param widthPx 宽度(px);非有限数静默忽略。
 */
export function applyTimelineScrollWidth(widthPx: number): void {
  if (!Number.isFinite(widthPx)) return
  document.documentElement.style.setProperty('--scrollbar-width', `${widthPx}px`)
}

/**
 * 时间轴轴宽 CSS 变量(`--timeline-axis-width`,驱动 .timeline-sidebar 列宽)。与滚动条 thumb 宽
 * (`--scrollbar-width`)相互独立;同样在启动与 setter 两处应用。
 * @param widthPx 宽度(px);非有限数静默忽略。
 */
export function applyTimelineAxisWidth(widthPx: number): void {
  if (!Number.isFinite(widthPx)) return
  document.documentElement.style.setProperty('--timeline-axis-width', `${widthPx}px`)
}

/**
 * 轴视窗不透明度缩放 CSS 变量(`--axis-viewport-opacity`,无量纲乘数)。驱动时间轴(DOM/Canvas)
 * 与 minimap 三处半透明拖动视窗:时间轴按 color-mix 百分比乘缩放,minimap 按元素 opacity 乘缩放。
 * 100=默认观感;上限 200 保证时间轴各态 color-mix 乘后仍 ≤100%(最大 45%×2=90%)。
 * @param pct 百分比(20–200,越界收敛);非有限数静默忽略。
 */
export function applyAxisViewportOpacity(pct: number): void {
  if (!Number.isFinite(pct)) return
  const clamped = Math.min(200, Math.max(20, pct))
  document.documentElement.style.setProperty('--axis-viewport-opacity', `${clamped / 100}`)
}

/** 毛玻璃各表面不透明度缩放的范围;100 保持 Mica/Acrylic 当前默认观感。 */
export const GLASS_OPACITY_MIN = 20
export const GLASS_OPACITY_MAX = 120
export const GLASS_OPACITY_DEFAULT = 100

export type GlassOpacitySurface = 'chrome' | 'sticky' | 'surface' | 'control' | 'content'

const GLASS_OPACITY_VARIABLES: Record<GlassOpacitySurface, string> = {
  chrome: '--glass-chrome-scale',
  sticky: '--glass-sticky-scale',
  surface: '--glass-surface-scale',
  control: '--glass-control-scale',
  content: '--glass-content-scale',
}

/** 将毛玻璃某一类表面的不透明度缩放写入 CSS;设置页与启动/热更新共用。 */
export function applyGlassOpacityScale(surface: GlassOpacitySurface, pct: number): void {
  if (!Number.isFinite(pct)) return
  const clamped = Math.min(GLASS_OPACITY_MAX, Math.max(GLASS_OPACITY_MIN, Math.round(pct)))
  document.documentElement.style?.setProperty(GLASS_OPACITY_VARIABLES[surface], `${clamped / 100}`)
}

/** 配置文件是外部输入,统一收敛毛玻璃缩放值,避免 CSS 收到 NaN 或越界比例。 */
export function clampGlassOpacity(pct: number): number {
  if (!Number.isFinite(pct)) return GLASS_OPACITY_DEFAULT
  return Math.min(GLASS_OPACITY_MAX, Math.max(GLASS_OPACITY_MIN, Math.round(pct)))
}

/** 画廊底色遮罩的直接不透明度范围;0 保持画廊完全透出原生 DWM 材质。 */
export const GLASS_GALLERY_OPACITY_MIN = 0
export const GLASS_GALLERY_OPACITY_MAX = 100
export const GLASS_GALLERY_OPACITY_DEFAULT = 0

/** 将画廊底色遮罩不透明度写入 CSS;它与四组表面缩放不是同一种语义。 */
export function applyGlassGalleryOpacity(pct: number): void {
  if (!Number.isFinite(pct)) return
  const clamped = clampGlassGalleryOpacity(pct)
  document.documentElement.style?.setProperty('--glass-gallery-opacity', `${clamped}%`)
}

/** 配置文件是外部输入,统一收敛画廊底色遮罩,避免 CSS 收到 NaN 或越界百分比。 */
export function clampGlassGalleryOpacity(pct: number): number {
  if (!Number.isFinite(pct)) return GLASS_GALLERY_OPACITY_DEFAULT
  return Math.min(
    GLASS_GALLERY_OPACITY_MAX,
    Math.max(GLASS_GALLERY_OPACITY_MIN, Math.round(pct)),
  )
}
