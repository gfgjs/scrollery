// src/themes/strength.ts
// 主题「浓度」类 CSS 变量的单源(与 utils/uiScale.ts 的玻璃分层缩放同模式,归属 themes 域)。
//
// 两个同构机制,只是缩放的 token 面与 CSS 变量不同:
// · 主题色浓度:底色 wash token(bg 五层/canvas 三件/doc-paper 两件)声明为
//     color-mix(in oklch, <满浓度锚点> calc(var(--theme-tint-scale, 1) * 100%), <中性色>)
//   100=满浓度(等于出厂锚点原值),数值越小底色越向中性色稀释;亮主题的中性色是白色
//   (更浅且更淡),暗主题是同明度无彩灰(只去色不变暗)。默认 60(用户反馈出厂偏深)。
// · 文字浓度:文字 ramp(text-primary/secondary/tertiary/placeholder)同式声明,受
//     --theme-text-scale 缩放。文字与底色极性相反,稀释=文字向底色靠拢=对比下降:
//   100=满浓度(出厂文字色),默认 75(用户反馈出厂文字对比过强;默认底色上实测 primary
//   对比 6.1–8.0、secondary 3.3–3.7 仍守 AA,tertiary/placeholder 属元数据文字允许更浅)。
//   下限 40:再低正文逼近不可读,防「文字消失」类反馈。
// 首帧由 public/theme-snapshot.js 按快照预写两个变量,权威源仍是 app_config(启动批/
// 外置 config.toml 热更新经 uiStore 走到这里)。

export const THEME_TINT_MIN = 0
export const THEME_TINT_MAX = 100
export const THEME_TINT_DEFAULT = 60

export const THEME_TEXT_MIN = 40
export const THEME_TEXT_MAX = 100
export const THEME_TEXT_DEFAULT = 100

/** 配置文件是外部输入,统一收敛浓度值,避免 CSS 收到 NaN 或越界比例(color-mix 百分比须 ≤100%)。 */
function clampStrength(pct: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(pct)) return fallback
  return Math.min(max, Math.max(min, Math.round(pct)))
}

export function clampThemeTint(pct: number): number {
  return clampStrength(pct, THEME_TINT_MIN, THEME_TINT_MAX, THEME_TINT_DEFAULT)
}

export function clampThemeText(pct: number): number {
  return clampStrength(pct, THEME_TEXT_MIN, THEME_TEXT_MAX, THEME_TEXT_DEFAULT)
}

/** 将主题色浓度写入 CSS;设置页 setter 与启动/热更新水合共用。 */
export function applyThemeTintStrength(pct: number): void {
  if (!Number.isFinite(pct)) return
  document.documentElement.style?.setProperty('--theme-tint-scale', `${clampThemeTint(pct) / 100}`)
}

/** 将文字浓度写入 CSS;设置页 setter 与启动/热更新水合共用。 */
export function applyThemeTextStrength(pct: number): void {
  if (!Number.isFinite(pct)) return
  document.documentElement.style?.setProperty('--theme-text-scale', `${clampThemeText(pct) / 100}`)
}
