// 选色器的 HSV 数学(方案 §2 二维饱和度／明度面板 + 色相条)。
// 解析统一复用 utils/color.ts 的唯一实现,这里只补 HSV↔HEX 转换与钳制;纯函数,node 可单测。

import { parseColorToRgb } from '../../utils/color'

export interface Hsv {
  /** 色相 0–360。 */
  h: number
  /** 饱和度 0–1。 */
  s: number
  /** 明度 0–1。 */
  v: number
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.min(1, Math.max(0, value))
}

/** HEX → HSV;不可解析返回黑色(h=0,s=0,v=0)。 */
export function hexToHsv(hex: string): Hsv {
  const rgb = parseColorToRgb(hex)
  if (!rgb) return { h: 0, s: 0, v: 0 }
  const r = rgb.r / 255
  const g = rgb.g / 255
  const b = rgb.b / 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  let h = 0
  if (delta > 0) {
    if (max === r) h = 60 * (((g - b) / delta) % 6)
    else if (max === g) h = 60 * ((b - r) / delta + 2)
    else h = 60 * ((r - g) / delta + 4)
  }
  if (h < 0) h += 360
  return { h, s: max === 0 ? 0 : delta / max, v: max }
}

/** HSV → 规范 #rrggbb;h 取模 360,s／v 钳到 0–1。 */
export function hsvToHex(h: number, s: number, v: number): string {
  const hue = ((Number.isFinite(h) ? h : 0) % 360 + 360) % 360
  const sat = clamp01(s)
  const val = clamp01(v)
  const c = val * sat
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1))
  const m = val - c
  const sector = Math.floor(hue / 60) % 6
  const table: readonly (readonly [number, number, number])[] = [
    [c, x, 0],
    [x, c, 0],
    [0, c, x],
    [0, x, c],
    [x, 0, c],
    [c, 0, x],
  ]
  const [r, g, b] = table[sector]
  const toHex = (channel: number): string =>
    Math.round((channel + m) * 255)
      .toString(16)
      .padStart(2, '0')
  return '#' + toHex(r) + toHex(g) + toHex(b)
}

/** 面板背景色(纯色相 + 满饱和满明度),供二维面板与色相条的 CSS 渐变起点使用。 */
export function hueColor(h: number): string {
  return hsvToHex(h, 1, 1)
}
