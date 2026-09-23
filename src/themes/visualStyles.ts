import { mix } from './colors'
import type { ThemeMode, ThemeVisualStyle } from './types'

/** 已知外观的有限配方，不能从设置输入 CSS 或素材路径。 */
export const THEME_VISUAL_STYLES: readonly ThemeVisualStyle[] = ['standard', 'mint', 'forest']

/** 外壳底色从当前种子生成，用户改配色时保留造型。 */
export function shellBackgroundFor(
  background: string,
  accent: string,
  mode: ThemeMode,
  style: ThemeVisualStyle,
): string {
  if (style === 'mint') return mix(background, accent, mode === 'light' ? 0.1 : 0.08)
  if (style === 'forest') return mix(background, '#9eb77c', mode === 'light' ? 0.25 : 0.07)
  return background
}
