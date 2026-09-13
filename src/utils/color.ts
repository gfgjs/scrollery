// src/utils/color.ts
// 颜色数学单一归宿（阅读器方案 R3）。此前 isDarkColor（感知亮度）散落在 shikiHighlight，
// R3 的阅读主题对比度门禁又要 WCAG 相对亮度——两处各写一份解析器易漂移，故抽此共享模块：
//   · parseColorToRgb —— 唯一的 CSS 颜色解析器（#rgb / #rrggbb / rgb() / rgba() / oklch() / color(srgb)）。
//   · isDarkColor      —— 感知亮度（0.299/0.587/0.114），给 shiki 选明/暗主题（沿用旧行为）。
//   · relativeLuminance / contrastRatio —— WCAG 2.x 线性化亮度与对比度（阅读主题门禁用）。
// 纯函数、无 DOM 依赖，node 环境可单测。

export interface Rgb {
  r: number
  g: number
  b: number
}

/**
 * 解析 CSS 颜色为 0..255 的 RGB；无法解析（空串 / 具名色 / 非法值）→ null。
 * 支持 `#rgb` / `#rrggbb` / `rgb()` / `rgba()`（含逗号或空格分隔）、`oklch()` 与
 * `color(srgb ...)`——后两者是 getComputedStyle 对主题 color-mix token 的典型序列化形态
 * （主题底色自 2026-09-06 起为 color-mix 浓度表达式,经 DOM 解析后可能以 oklch 形态回读）。
 * **不支持具名色**（如 blue）——具名色需 DOM 解析,纯函数拿不到,返回 null 由调用方按默认处理。
 */
export function parseColorToRgb(css: string): Rgb | null {
  const c = css.trim()
  if (!c) return null
  let r: number
  let g: number
  let b: number
  const rgb = /rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(c)
  if (rgb) {
    r = Number(rgb[1])
    g = Number(rgb[2])
    b = Number(rgb[3])
  } else {
    const okl = parseOklchToRgb(c)
    if (okl) return okl
    const srgb = /color\(\s*srgb\s+([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(c)
    if (srgb) {
      r = Math.round(Number(srgb[1]) * 255)
      g = Math.round(Number(srgb[2]) * 255)
      b = Math.round(Number(srgb[3]) * 255)
    } else {
      const hex = c.replace(/^#/, '')
      if (hex.length === 3) {
        r = parseInt(hex[0] + hex[0], 16)
        g = parseInt(hex[1] + hex[1], 16)
        b = parseInt(hex[2] + hex[2], 16)
      } else if (hex.length === 6) {
        r = parseInt(hex.slice(0, 2), 16)
        g = parseInt(hex.slice(2, 4), 16)
        b = parseInt(hex.slice(4, 6), 16)
      } else {
        return null
      }
    }
  }
  if (![r, g, b].every((n) => Number.isFinite(n))) return null
  return { r, g, b }
}

/**
 * oklch() → RGB:Björn Ottosson oklab 标准矩阵;解析不出（非 oklch 形态/通道非数）→ null。
 * 色相支持 `none`（缺失分量）:主题 color-mix 得到的近中性色经 getComputedStyle 会序列化成
 * `oklch(L C none)`,按 CSS Color 4 §4.4 缺失分量在转换时取零值,即 0deg。此前该形态解析
 * 失败返回 null,调用方回退硬编码底色(mediaGridCanvas.palette.ts)才显出画布分隔黑条。
 */
export function parseOklchToRgb(css: string): Rgb | null {
  const m =
    /^oklch\(\s*([\d.]+)%?\s*[\s,]+([\d.]+)\s*[\s,]+(none|[\d.]+(?:deg)?)\s*(?:\/\s*([\d.%]+)\s*)?\)$/i.exec(
      css.trim(),
    )
  if (!m) return null
  const L = Number(m[1]) > 1 ? Number(m[1]) / 100 : Number(m[1])
  const C = Number(m[2])
  // none 只补色相角 0deg,chroma 照旧参与极坐标——令 a=C、b=0,不能把 a/b 一并归零
  // (那样会把带彩度的颜色错算成中性灰)。数值色相剥掉 deg 后缀后整体转数,畸形值如
  // `12.3.4` 得 NaN 走末段有限性检查被拒,不用 parseFloat 前缀解析把非法输入误读成合法角度。
  const hDeg =
    m[3].toLowerCase() === 'none'
      ? 0
      : Number(m[3].replace(/deg$/i, '')) * (Math.PI / 180)
  // oklch → oklab（h 退回直角坐标）→ LMS（**立方**,逆向;正向才是 cbrt）→ 线性 sRGB（Ottosson 矩阵）。
  const a = C * Math.cos(hDeg)
  const b = C * Math.sin(hDeg)
  const l_ = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m_ = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s_ = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  const lin = [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
  ]
  const chan = (x: number) => {
    const v = x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055
    return Math.round(Math.min(1, Math.max(0, v)) * 255)
  }
  const [r, g, bb] = lin.map(chan)
  return Number.isFinite(r) && Number.isFinite(g) && Number.isFinite(bb) ? { r, g, b: bb } : null
}

/**
 * 粗判 CSS 颜色是否偏暗（**感知亮度** < 0.5）。用于据阅读页背景选 shiki 明/暗主题；
 * 无法解析 → false（默认按亮色处理）。注意：这是感知加权近似，非 WCAG 相对亮度——
 * 选主题够用，涉及可读性门禁请用 contrastRatio。
 */
export function isDarkColor(css: string): boolean {
  const rgb = parseColorToRgb(css)
  if (!rgb) return false
  // sRGB 感知亮度近似（人眼对绿最敏感）。
  const lum = (0.299 * rgb.r + 0.587 * rgb.g + 0.114 * rgb.b) / 255
  return lum < 0.5
}

/**
 * WCAG 2.x 相对亮度（0..1）：每通道先归一化并做 sRGB→线性 gamma 展开，再按人眼权重加权。
 * 与 isDarkColor 的感知近似不同，这是对比度计算的**规范**亮度。
 */
export function relativeLuminance(rgb: Rgb): number {
  const chan = (v: number) => {
    const s = v / 255
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4)
  }
  return 0.2126 * chan(rgb.r) + 0.7152 * chan(rgb.g) + 0.0722 * chan(rgb.b)
}

/**
 * 两色 WCAG 对比度（1..21）：(L_亮+0.05)/(L_暗+0.05)。任一色无法解析 → 0（视作不达标，
 * 供对比度门禁「解析失败即红」）。AA 正文阈值 4.5、大字 3.0。
 */
export function contrastRatio(a: string, b: string): number {
  const ra = parseColorToRgb(a)
  const rb = parseColorToRgb(b)
  if (!ra || !rb) return 0
  const la = relativeLuminance(ra)
  const lb = relativeLuminance(rb)
  const light = Math.max(la, lb)
  const dark = Math.min(la, lb)
  return (light + 0.05) / (dark + 0.05)
}
