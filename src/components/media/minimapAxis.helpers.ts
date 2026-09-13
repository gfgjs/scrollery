// src/components/media/minimapAxis.helpers.ts
// 无缝 minimap 轴的纯几何(镜像 mediaScrollbar.helpers 的「纯函数 + 单测锁定」纪律)。
//
// 为什么滑窗:大库逻辑总高可达数千万 px,全高等比缩进一条轨道等于每项亚像素,
// 微缩预览无从谈起。取 VSCode proportional 模式——缩放系数固定(轴宽/内容宽),
// minimap 内容总高超出轨道时,内容窗随滚动进度按比例平移。
//
// 数学不变量(spec round-trip 锁定):视口框顶 rawTop = (currentY − 窗顶y)×scale
// 对 currentY 是**线性**函数,斜率 k = scale − max(0, mini内容高 − 轨道高)/可滚动量
// (滑动形态化简为 (轨道高 − 视口×scale)/可滚动量;非滑动形态为 scale 本身)。
// 拖拽逆映射据此闭式求逆,无隐式方程。唯一偏差源是框高被 MIN_SLIDER_PX 钳制时:
// 框位置优先与画出的内容对齐(rawTop 再钳到轨道内),与逆映射存在 < 钳制量的
// 手感偏差;逆映射端点经钳制仍可达 [0, 可滚动量] 全程。

export interface MinimapWindow {
  /** 内容窗顶(逻辑 y,内容坐标)。 */
  topY: number
  /** 内容窗底(逻辑 y)。 */
  bottomY: number
}

export interface MinimapSlider {
  /** 视口框顶(轨道 px)。 */
  top: number
  /** 视口框高(px,已含最小高钳制)。 */
  height: number
}

/** 视口框最小高(px):深库下 viewportH×scale 缩到亚像素,钳到可视/可抓下限。 */
export const MIN_SLIDER_PX = 16

/** 键盘方向键的小步进(px),取桌面浏览器滚动容器的常见默认量级。 */
export const KEYBOARD_LINE_PX = 40

/** 微缩略图瞬时失败的有限退避；数组长度即额外重试次数。 */
export const MINIMAP_THUMB_RETRY_DELAYS_MS = [400, 1200] as const

/** 第 N 次失败后的重试延迟；超过预算返回 null，调用方转为本代次终态失败。 */
export function minimapThumbRetryDelay(failureCount: number): number | null {
  if (!Number.isInteger(failureCount) || failureCount < 1) return null
  return MINIMAP_THUMB_RETRY_DELAYS_MS[failureCount - 1] ?? null
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v))
}

/** 缩放系数 = 轴宽 / 内容宽。入参无效(含 NaN/0)返回 0,调用方以 0 判不可渲染。 */
export function minimapScale(miniWidth: number, containerWidth: number): number {
  if (!(miniWidth > 0) || !(containerWidth > 0)) return 0
  return miniWidth / containerWidth
}

/**
 * 当前滚动位 → minimap 内容窗(该画哪段内容)。
 * 内容缩后不足一轨道 → 窗覆盖全量(不滑动);轨道/缩放/总高无效 → null。
 */
export function minimapWindow(
  currentY: number,
  totalHeight: number,
  viewportHeight: number,
  trackHeight: number,
  scale: number,
): MinimapWindow | null {
  if (!(scale > 0) || !(trackHeight > 0) || !(totalHeight > 0)) return null
  const miniContentH = totalHeight * scale
  if (miniContentH <= trackHeight) return { topY: 0, bottomY: totalHeight }
  const maxScroll = Math.max(1, totalHeight - viewportHeight)
  const frac = clamp01(currentY / maxScroll)
  const topY = (frac * (miniContentH - trackHeight)) / scale
  return { topY, bottomY: topY + trackHeight / scale }
}

/**
 * 视口框几何。内容不足一屏(无可滚动)或窗无效 → null(隐藏框)。
 * 框顶与内容窗对齐(rawTop,见头注不变量);钳高形态下再钳到轨道内。
 */
export function minimapSlider(
  currentY: number,
  totalHeight: number,
  viewportHeight: number,
  trackHeight: number,
  scale: number,
  minSlider: number = MIN_SLIDER_PX,
): MinimapSlider | null {
  if (!(viewportHeight > 0) || !(totalHeight > viewportHeight)) return null
  const win = minimapWindow(currentY, totalHeight, viewportHeight, trackHeight, scale)
  if (!win) return null
  const height = Math.min(trackHeight, Math.max(minSlider, viewportHeight * scale))
  const rawTop = (currentY - win.topY) * scale
  const top = Math.min(Math.max(0, rawTop), Math.max(0, trackHeight - height))
  return { top, height }
}

/**
 * 拖拽:框顶(轨道 px)→ 逻辑 y。rawTop(currentY) 的闭式求逆(斜率 k 见头注),
 * 滑动/非滑动两形态统一覆盖;越界钳制到 [0, 可滚动量];无行程(k≤0)恒 0。
 * 显示函数在 trackH−框高 处被钳截断(minimapSlider 的 top 钳制),拖到截断点及
 * 以外一律吸附末端——未钳形态下该点逆映射本就 ≥ 可滚动量(经钳等价),钳高深库
 * 形态下则由吸附补齐否则差 k 斜率误差不可达的末段(spec 锁定)。
 */
export function sliderTopToLogicalY(
  sliderTop: number,
  totalHeight: number,
  viewportHeight: number,
  trackHeight: number,
  scale: number,
  minSlider: number = MIN_SLIDER_PX,
): number {
  const maxScroll = totalHeight - viewportHeight
  if (!(maxScroll > 0) || !(scale > 0) || !(trackHeight > 0)) return 0
  const miniContentH = totalHeight * scale
  const k = scale - Math.max(0, miniContentH - trackHeight) / maxScroll
  if (!(k > 0)) return 0
  const height = Math.min(trackHeight, Math.max(minSlider, viewportHeight * scale))
  if (trackHeight - height > 0 && sliderTop >= trackHeight - height) return maxScroll
  return Math.min(maxScroll, Math.max(0, sliderTop / k))
}

/** 轨道点击 → 目标逻辑 y:点击处内容居中于视口,钳到可滚动范围。 */
export function clickToLogicalY(
  clickPx: number,
  window: MinimapWindow,
  scale: number,
  viewportHeight: number,
  totalHeight: number,
): number {
  if (!(scale > 0)) return 0
  const contentY = window.topY + clickPx / scale
  const maxScroll = Math.max(0, totalHeight - viewportHeight)
  return Math.min(maxScroll, Math.max(0, contentY - viewportHeight / 2))
}

/**
 * WheelEvent 的 deltaMode:0=像素、1=行、2=页。minimap 会阻止默认滚动并手动转发,
 * 因而必须先归一到像素,否则 Firefox/部分鼠标的一格滚轮只会移动 3px 左右。
 */
export function normalizeWheelDelta(
  deltaY: number,
  deltaMode: number,
  viewportHeight: number,
): number {
  if (deltaMode === 1) return deltaY * 16
  if (deltaMode === 2) return deltaY * (viewportHeight > 0 ? viewportHeight : 800)
  return deltaY
}

/** 当前/待提交位置叠加一次滚轮输入并钳到可滚动范围。调用方可连续回喂结果,避免同帧吞量。 */
export function wheelToLogicalY(
  baseY: number,
  deltaY: number,
  deltaMode: number,
  totalHeight: number,
  viewportHeight: number,
): number {
  const delta = normalizeWheelDelta(deltaY, deltaMode, viewportHeight)
  const maxScroll = Math.max(0, totalHeight - viewportHeight)
  return Math.min(maxScroll, Math.max(0, baseY + delta))
}

/** 缩略图资源签名:缓存目录与模式代次都属于身份,任一变化必须硬失效。 */
export function minimapThumbSig(
  epoch: number,
  cacheDir: string,
  thumbStatus: number,
  thumbPath: string | null | undefined,
): string {
  return `${epoch}|${cacheDir}|${thumbStatus}|${thumbPath ?? ''}`
}

/** scrollbar 键盘导航:方向键小步、Page 键整页、Home/End 到两端;非导航键返回 null。 */
export function keyboardToLogicalY(
  key: string,
  currentY: number,
  totalHeight: number,
  viewportHeight: number,
): number | null {
  const maxScroll = Math.max(0, totalHeight - viewportHeight)
  let next: number
  switch (key) {
    case 'ArrowUp':
    case 'ArrowLeft':
      next = currentY - KEYBOARD_LINE_PX
      break
    case 'ArrowDown':
    case 'ArrowRight':
      next = currentY + KEYBOARD_LINE_PX
      break
    case 'PageUp':
      next = currentY - Math.max(0, viewportHeight)
      break
    case 'PageDown':
      next = currentY + Math.max(0, viewportHeight)
      break
    case 'Home':
      next = 0
      break
    case 'End':
      next = maxScroll
      break
    default:
      return null
  }
  return Math.min(maxScroll, Math.max(0, next))
}
