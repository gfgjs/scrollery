// src/composables/adjustFormula.ts
// E3 调色双端同源公式(设计 §4.3;D-104):与 Rust `editing::adjust` 严格同式,共享
// `src/fixtures/adjustGolden.json` 黄金向量对拍。**有意**作用在 sRGB gamma 编码域而非
// 线性光,与 CSS/主流简易编辑器语义对齐——不是疏漏,防未来误「修」。
// 应用序钉死:亮度 → 对比度 → 饱和度;每步 clamp [0,1];alpha 通道不动。
// 预览位图由后端 CMS 转好 sRGB(D-109),本模块只做公式,无浏览器色彩管理变量。

/** 三参数共用闭区间端点(Rust `editing::adjust` 同值)。 */
export const ADJUST_MIN = -100
export const ADJUST_MAX = 100

export interface AdjustParams {
  brightness: number
  contrast: number
  saturation: number
}

export function isNeutralAdjust(params: AdjustParams): boolean {
  return params.brightness === 0 && params.contrast === 0 && params.saturation === 0
}

/** 预计算系数:饱和度矩阵每参数组只算一次,不进像素循环。 */
export interface AdjustCoefficients {
  /** 亮度乘法系数 `2^(b/100)`(±100 恰为 ±1EV)。 */
  brightness: number
  /** 对比度线性系数 `(100+c)/100`(不采用 image crate 的平方映射)。 */
  contrast: number
  /** W3C Filter Effects saturate 矩阵,luma 权重 [0.213, 0.715, 0.072],行优先 3×3。 */
  saturate: [number, number, number, number, number, number, number, number, number]
}

export function adjustCoefficients(params: AdjustParams): AdjustCoefficients {
  const kS = (100 + params.saturation) / 100
  return {
    brightness: Math.pow(2, params.brightness / 100),
    contrast: (100 + params.contrast) / 100,
    saturate: [
      0.213 + 0.787 * kS,
      0.715 - 0.715 * kS,
      0.072 - 0.072 * kS,
      0.213 - 0.213 * kS,
      0.715 + 0.285 * kS,
      0.072 - 0.072 * kS,
      0.213 - 0.213 * kS,
      0.715 - 0.715 * kS,
      0.072 + 0.928 * kS,
    ],
  }
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v))
}

function brightnessContrast(v: number, k: AdjustCoefficients): number {
  const bright = clamp01(v * k.brightness)
  return clamp01((bright - 0.5) * k.contrast + 0.5)
}

/** 单像素同源公式;输入输出均为 sRGB 编码域 [0,1]。 */
export function adjustRgb(
  r: number,
  g: number,
  b: number,
  k: AdjustCoefficients,
): [number, number, number] {
  const pr = brightnessContrast(r, k)
  const pg = brightnessContrast(g, k)
  const pb = brightnessContrast(b, k)
  const m = k.saturate
  return [
    clamp01(m[0] * pr + m[1] * pg + m[2] * pb),
    clamp01(m[3] * pr + m[4] * pg + m[5] * pb),
    clamp01(m[6] * pr + m[7] * pg + m[8] * pb),
  ]
}

/**
 * canvas ImageData 整体应用(就地):RGBA 8-bit,alpha 不动。
 * 量化契约与 Rust 同款:`round(clamp(v)·255)`(正值域内 Math.round 与 f32 round 一致;
 * 黄金向量 expected8 依此断言)。
 */
export function applyAdjustToPixels(data: Uint8ClampedArray, k: AdjustCoefficients): void {
  for (let i = 0; i < data.length; i += 4) {
    const [r, g, b] = adjustRgb(data[i] / 255, data[i + 1] / 255, data[i + 2] / 255, k)
    data[i] = Math.round(r * 255)
    data[i + 1] = Math.round(g * 255)
    data[i + 2] = Math.round(b * 255)
  }
}
