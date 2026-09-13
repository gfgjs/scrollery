/** D-111：UI 与 IPC 共用同一闭区间语义，端点合法。 */
export const STRAIGHTEN_MIN_DEGREES = -45
export const STRAIGHTEN_MAX_DEGREES = 45

export interface PixelSize {
  width: number
  height: number
}

/**
 * 求旋转后仍保持原宽高比的最大居中内接矩形。
 *
 * 对候选矩形 `s·w × s·h` 的右上角做逆旋转，两个轴向约束分别给出一个 `s` 上界；
 * 取较小者即为最大可行缩放。Rust 使用相同闭式解，最终统一向下取整，避免采到边界外像素。
 */
export function maximumAspectInscribedSize(
  width: number,
  height: number,
  angleDegrees: number,
): PixelSize {
  if (width <= 0 || height <= 0) return { width: 0, height: 0 }
  if (angleDegrees === 0) return { width: Math.floor(width), height: Math.floor(height) }

  const radians = (Math.abs(angleDegrees) * Math.PI) / 180
  const cos = Math.abs(Math.cos(radians))
  const sin = Math.abs(Math.sin(radians))
  const widthScale = width / (width * cos + height * sin)
  const heightScale = height / (width * sin + height * cos)
  const scale = Math.min(widthScale, heightScale)
  return {
    width: Math.max(1, Math.floor(width * scale)),
    height: Math.max(1, Math.floor(height * scale)),
  }
}
