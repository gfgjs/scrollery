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
  // 森林外壳取嫩叶奶油色：参考图侧栏是带黄绿调的奶油底，而非灰绿。
  if (style === 'forest') return mix(background, '#b5c98f', mode === 'light' ? 0.26 : 0.05)
  return background
}

/** 结构线（边框/分隔线）与悬停底的混色目标与强度倍率；有对比门槛的控件边界不走此配方。 */
export interface LineTone {
  color: string
  gain: number
}

/**
 * 森林的分隔线与悬停底取麦秆金褐色并加重：参考图以暖金色虚线划分区块，
 * 前景色混出的灰线和灰悬停底在奶油底上显脏、没有辨识度。
 */
export function lineToneFor(foreground: string, mode: ThemeMode, style: ThemeVisualStyle): LineTone {
  if (style === 'forest') return { color: '#c9a560', gain: mode === 'light' ? 2.5 : 1.6 }
  return { color: foreground, gain: 1 }
}
