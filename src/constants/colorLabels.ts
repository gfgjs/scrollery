// 颜色标签调色板（Part5 T16）。
// 后端 schema 仅存 0-7 档位（0=未标，见 schema.rs:582），**颜色映射在前端**——改色只需动此表，
// 改名在 i18n 词典（name 存 i18n 键），均不触后端。采用 Lightroom / Adobe Bridge / macOS Finder
// 一脉相承的标准 7 色作默认（单一事实源，全 UI 经 colorLabelHex / COLOR_LABELS 取色）。

import i18n from '../i18n'

export interface ColorLabel {
  /** 档位 1-7（0 保留为「未标」，不在此表）。 */
  value: number
  /** 展示名的 i18n 键（渲染点经 t() 取词，用于 tooltip）。 */
  name: string
  /** 显示色（swatch / 网格色条）。 */
  hex: string
}

export const COLOR_LABELS: readonly ColorLabel[] = [
  { value: 1, name: 'colorLabels.red', hex: '#e53935' },
  { value: 2, name: 'colorLabels.orange', hex: '#fb8c00' },
  { value: 3, name: 'colorLabels.yellow', hex: '#fdd835' },
  { value: 4, name: 'colorLabels.green', hex: '#43a047' },
  { value: 5, name: 'colorLabels.blue', hex: '#1e88e5' },
  { value: 6, name: 'colorLabels.purple', hex: '#8e24aa' },
  { value: 7, name: 'colorLabels.gray', hex: '#757575' },
]

/** 档位 → 显示色；0 或越界（含未来 schema 扩档）→ null，调用方据此不渲染色块。 */
export function colorLabelHex(value: number): string | null {
  return COLOR_LABELS.find((c) => c.value === value)?.hex ?? null
}

/** 档位 → 翻译后的展示名（函数内惰性 t()，随 locale 切换）；用于 tooltip。0/越界 → 空串。 */
export function colorLabelName(value: number): string {
  const key = COLOR_LABELS.find((c) => c.value === value)?.name
  return key ? i18n.global.t(key) : ''
}
