// src/themes/apply.ts
// 色板 → 文档根(方案 §6:apply.ts = 色板 → CSS 变量;单次主题发布)。
//
// 变量名全部来自 generate.ts 的 PALETTE_CSS_VARS(唯一映射),本模块不另立第二张表、不含颜色
// 算法;写入的都是 hex/rgb/rgba 实色,不向任何消费者传 var()/color-mix() 表达式。
//
// 本模块同时产出**默认启动 CSS**(index.html 注入用),故刻意不 import Vue/Tauri/平台探测:
// 浏览器与 vite 配置插件(Node 侧)共用同一实现,平台差异由调用方以参数传入。

import { THEME_PALETTE_VARS, generateTheme, paletteToCssVars } from './generate'
import type { ThemeDefinition, ThemeMaterial, ThemePalette } from './types'

/** 材质开启时写入窗口根的不透明度变量(百分比文本,glass.css 消费;bootstrap 同名写入)。 */
export const WINDOW_OPACITY_VAR = '--window-opacity'

/** 明暗属性:组件按 [data-color-scheme='dark'] 分支,不再对具体主题 id 写选择器。 */
export const COLOR_SCHEME_ATTR = 'data-color-scheme'
/** 原生玻璃背板属性:仅 Windows 且材质非 none 时存在(glass.css 唯一入口)。 */
export const GLASS_ATTR = 'data-glass'

/** 上一次写入的变量与根元素:同一份值不重复 setProperty,避免无谓的样式失效。 */
let appliedRoot: HTMLElement | null = null
const appliedVars = new Map<string, string>()

function writeVars(root: HTMLElement, vars: Record<string, string>): void {
  if (appliedRoot !== root) {
    appliedVars.clear()
    appliedRoot = root
  }
  for (const name of THEME_PALETTE_VARS) {
    const value = vars[name]
    if (value === undefined || appliedVars.get(name) === value) continue
    root.style.setProperty(name, value)
    appliedVars.set(name, value)
  }
}

/** 发布一份色板:整份变量 + 明暗属性。同值重复调用不会产生任何 DOM 写入。 */
export function applyPalette(palette: ThemePalette, root?: HTMLElement): void {
  const target = root ?? document.documentElement
  writeVars(target, paletteToCssVars(palette))
  target.setAttribute(COLOR_SCHEME_ATTR, palette.mode)
}

/**
 * 发布材质:非 Windows 或材质 none 时**必须**移除属性并清掉不透明度变量——否则 CSS 会按
 * 「有玻璃底」把半透明填充画在没有原生背板的窗口上,观感是整页发灰的透明块。
 *
 * @param windows 是否具备原生背板的平台(由调用方探测;本模块不 import 平台层)。
 */
export function applyMaterial(
  material: ThemeMaterial,
  opacity: number,
  options: { root?: HTMLElement; windows?: boolean } = {},
): void {
  const root = options.root ?? document.documentElement
  if (options.windows === true && material !== 'none') {
    root.setAttribute(GLASS_ATTR, material)
    // 不透明度由设置边界(config.ts parseOpacity)收敛过,这里只做写入。
    root.style.setProperty(WINDOW_OPACITY_VAR, `${opacity}%`)
    return
  }
  root.removeAttribute(GLASS_ATTR)
  root.style.removeProperty(WINDOW_OPACITY_VAR)
}

function cssDeclarations(palette: ThemePalette): string {
  const vars = paletteToCssVars(palette)
  return THEME_PALETTE_VARS.map((name) => `${name}:${vars[name]}`).join(';')
}

/**
 * 默认启动 CSS:浅色落 :root,深色落 html[data-color-scheme='dark'](属性选择器特异性更高,
 * 深色块稳定覆盖浅色块)。构建与开发同源注入 index.html;缓存缺失时首帧即由它着色。
 *
 * 只产出色值,不包含材质相关声明(材质由运行时属性驱动)。
 */
export function themeDefaultCss(definition: ThemeDefinition): string {
  return [
    '/* 由 src/themes/apply.ts themeDefaultCss 生成:默认主题色板,勿手改(改种子/presets) */',
    `:root{${cssDeclarations(generateTheme(definition.light, 'light'))}}`,
    `html[${COLOR_SCHEME_ATTR}='dark']{${cssDeclarations(generateTheme(definition.dark, 'dark'))}}`,
  ].join('\n')
}
