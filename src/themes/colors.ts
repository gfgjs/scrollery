// src/themes/colors.ts
// 主题色数学的纯函数集合(方案 §4.2):解析、通道混色、亮度、对比度与黑白选择。
//
// 解析与 WCAG 对比度复用 utils/color.ts 的既有实现——那是全库唯一的 CSS 颜色解析器与
// 对比度口径(阅读主题门禁、shiki 选深浅都在用),此处不写第二套。本文件只补主题生成所需
// 的运算:规范 HEX 归一、RGB 通道插值、alpha 合成、按目标对比度反推混色量。
// 全部为纯函数,不访问 DOM / 存储 / 框架,node 环境可单测。

import {
  contrastRatio,
  parseColorToRgb,
  relativeLuminance,
  type Rgb,
} from '../utils/color'

export { contrastRatio, parseColorToRgb, relativeLuminance }
export type { Rgb }

function isCanonical(color: string): boolean {
  return /^#[0-9a-f]{6}$/.test(color)
}

/**
 * 把外部输入收敛为规范 #rrggbb;非颜色返回 null。
 * 接受 #rgb / #rrggbb / 无 # 前缀的六位写法(HEX 输入框与手改 config.toml 都会遇到)。
 */
export function normalizeHex(raw: string): string | null {
  const value = raw.trim().replace(/^#/, '').toLowerCase()
  if (/^[0-9a-f]{3}$/.test(value)) {
    return '#' + value[0] + value[0] + value[1] + value[1] + value[2] + value[2]
  }
  if (/^[0-9a-f]{6}$/.test(value)) return '#' + value
  return null
}

/** 判断是否已是规范 #rrggbb(生成结果自检与测试用)。 */
export function isCanonicalHex(color: string): boolean {
  return isCanonical(color)
}

function clampRatio(t: number): number {
  if (!Number.isFinite(t)) return 0
  return t < 0 ? 0 : t > 1 ? 1 : t
}

function toHexChannel(value: number): string {
  const clamped = Math.round(Math.min(255, Math.max(0, value)))
  return clamped.toString(16).padStart(2, '0')
}

/** RGB → 规范 #rrggbb。 */
export function rgbToHex(rgb: Rgb): string {
  return '#' + toHexChannel(rgb.r) + toHexChannel(rgb.g) + toHexChannel(rgb.b)
}

/**
 * 简单 RGB 通道插值:mix(a, b, t) 表示从 a 向 b 混合 t(0=a,1=b)。
 *
 * 不做 gamma / OKLab 插值——首版刻意选最简单且可手算的模型,系数集中在 generate.ts 调优;
 * 任一色不可解析时返回 a(局部降级,不让一处坏输入毁掉整份色板)。
 */
export function mix(a: string, b: string, t: number): string {
  const from = parseColorToRgb(a)
  const to = parseColorToRgb(b)
  if (!from || !to) return a
  const k = clampRatio(t)
  return rgbToHex({
    r: from.r + (to.r - from.r) * k,
    g: from.g + (to.g - from.g) * k,
    b: from.b + (to.b - from.b) * k,
  })
}

/** 给颜色加 alpha(遮罩、玻璃填充、描边等确需透明的用途);色值不可解析时返回原串。 */
export function withAlpha(color: string, alpha: number): string {
  const rgb = parseColorToRgb(color)
  if (!rgb) return color
  const a = Math.round(clampRatio(alpha) * 1000) / 1000
  return 'rgba(' + rgb.r + ', ' + rgb.g + ', ' + rgb.b + ', ' + a + ')'
}

/** 相对亮度;不可解析返回 null。 */
export function luminanceOf(color: string): number | null {
  const rgb = parseColorToRgb(color)
  return rgb ? relativeLuminance(rgb) : null
}

/** 是否偏亮(相对亮度 > 0.5);不可解析按亮处理。 */
export function isLightColor(color: string): boolean {
  return (luminanceOf(color) ?? 1) > 0.5
}

/**
 * 按计算对比在纯黑与纯白中选可读性更高的一侧(强调填充上的文字)。
 * 两色中较高者的对比度恒 ≥ 4.5,取较高者即为可用解。
 */
export function pickReadableText(background: string): string {
  const black = contrastRatio('#000000', background)
  const white = contrastRatio('#ffffff', background)
  return black >= white ? '#000000' : '#ffffff'
}

/**
 * 颜色在一组底色上的**最低**对比度:门槛判定要求全部底色都达标,故取最小值。
 * 底色列表为空时返回 0(视作不达标)。
 */
export function minContrast(color: string, bases: readonly string[]): number {
  if (bases.length === 0) return 0
  return bases.reduce((lowest, base) => Math.min(lowest, contrastRatio(color, base)), Infinity)
}

/**
 * 从 from 向 toward 混色,返回**最小的**、在全部 bases 上达到 ratio 的那一档。
 *
 * 用于「有含义的控件边界」这类有明确对比门槛、又必须随用户配色推出的颜色:固定系数在任意
 * 用户配色上都无法保证达标(浅色底上 8% 混色只有约 1.2:1)。必须传入全部实际承载面——同一
 * 颜色在不同层上对比不同(亮色的 elevated 比 background 更亮,暗色的 inset 更暗),只看一层
 * 会让另一层不达标。floor 给出起始混色量,避免门槛本就满足时退化成肉眼不可见;结果为取整后
 * 的 HEX,且已复核达标,故返回值必然满足门槛。toward 由调用方决定(常为 foreground)。
 *
 * 对比度关于混色量不是单调整体:混色量小的时候边界色可能恰好落在某个承载面的明度上(对比 1),
 * 越过该点后才单调上升。故先用粗扫从 floor 向上找首个全达标档,再在该档前后二分细化。
 * 全部 bases 在 t=1 都达不到 ratio(前景与承载面本身太接近)时返回 toward 端,交由视觉验收取舍。
 */
export function mixUntilContrast(
  from: string,
  toward: string,
  bases: readonly string[],
  ratio: number,
  floor = 0,
): string {
  const start = clampRatio(floor)
  const at = (t: number) => mix(from, toward, t)
  if (minContrast(at(1), bases) < ratio) return at(1)

  const step = 1 / 200
  let hit: number | null = null
  for (let t = start; t <= 1 + 1e-9; t += step) {
    const candidate = Math.min(1, t)
    if (minContrast(at(candidate), bases) >= ratio) {
      hit = candidate
      break
    }
  }
  if (hit === null) return at(1)

  let low = Math.max(start, hit - step)
  let high = hit
  for (let i = 0; i < 24; i += 1) {
    const mid = (low + high) / 2
    if (minContrast(at(mid), bases) >= ratio) high = mid
    else low = mid
  }
  return at(high)
}

/**
 * 保证 color 在全部 bases 上达到 ratio:已达标原样返回,否则朝与承载面相反的一端调整。
 * 用于强调色文字(方案 §4.2「以 accent 为起点,必要时向黑／白调整以改善可读性」)。
 */
export function ensureContrast(
  color: string,
  bases: readonly string[],
  ratio: number,
): string {
  if (bases.every((base) => contrastRatio(color, base) >= ratio)) return color
  const toward = pickReadableText(bases[bases.length - 1])
  return mixUntilContrast(color, toward, bases, ratio)
}
