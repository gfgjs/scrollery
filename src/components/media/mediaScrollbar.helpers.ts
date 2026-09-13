// src/components/media/mediaScrollbar.helpers.ts
// T16 B3.2:自研逻辑滚动条的纯几何(单测锁定)。
//
// 为什么自研:原生滚动条只跟随容器的物理 scrollTop/scrollHeight——映射态下物理 spacer
// 被钳到 16M、逻辑总高可达数千万,原生拇指结构上无法表达库内比例;且停稳偿债要挪
// scrollTop 对账,原生拇指随之回跳(真机「急速滚动拇指往回跳」)。本条直接渲染
// 逻辑百分比(logicalScrollTop / 逻辑总高),与画廊逐帧同步、永不回跳;物理账务
// (锚差/偿债)对它完全不可见。
//
// viewH 取轨道高:覆盖层与滚动视口同盒,差异仅容器内边距(纯装饰量级,不影响手感)。

export interface ThumbGeometry {
  /// 拇指顶边相对轨道顶(px)。
  top: number
  /// 拇指高(px,已含最小高钳制)。
  height: number
}

/** 最小拇指高(px):百万级库的纯比例拇指会缩到亚像素,钳到可抓取的下限。 */
export const MIN_THUMB_PX = 32

/**
 * 逻辑位 → 拇指几何。内容不足一屏(无需滚动)或轨道尺寸无效时返回 null(隐藏拇指)。
 * 拇指高被钳制后,位置仍按「可行程比例」计算——顶/底恰好贴轨道两端。
 * fixedThumb(固定指针模式):提供且 >0 时高度恒为该值(钳到轨道高为上限),
 * 跳过比例/最小高计算——指针尺寸恒定,但位置仍按可行程比例计算,行为与比例拇指一致。
 */
export function thumbGeometry(
  logicalY: number,
  totalHeight: number,
  trackHeight: number,
  minThumb: number = MIN_THUMB_PX,
  fixedThumb?: number,
): ThumbGeometry | null {
  if (!(trackHeight > 0) || !(totalHeight > trackHeight)) return null
  const height =
    fixedThumb !== undefined && fixedThumb > 0
      ? Math.min(trackHeight, fixedThumb)
      : Math.min(trackHeight, Math.max(minThumb, (trackHeight / totalHeight) * trackHeight))
  const maxY = totalHeight - trackHeight
  const frac = Math.min(1, Math.max(0, logicalY / maxY))
  return { top: frac * (trackHeight - height), height }
}

/**
 * 拇指顶边(拖拽中的目标位置)→ 逻辑滚动位。与 thumbGeometry 互逆(单测锁 round-trip);
 * 越界钳制到 [0, 逻辑最大滚动位];拇指占满轨道(无可行程)时恒 0。
 */
export function thumbTopToLogicalY(
  thumbTop: number,
  totalHeight: number,
  trackHeight: number,
  thumbHeight: number,
): number {
  const range = trackHeight - thumbHeight
  if (!(range > 0) || !(totalHeight > trackHeight)) return 0
  const frac = Math.min(1, Math.max(0, thumbTop / range))
  return frac * (totalHeight - trackHeight)
}
