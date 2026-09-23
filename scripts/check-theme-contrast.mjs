// 主题对比度门(方案 §4.2/§9):对**生成的**色板计算关键「前景 × 实际承载面」的 WCAG 2.x 对比度。
//
// 为什么经 vite-node 跑 TS:色板的唯一真源是 src/themes/generate.ts 的 generateTheme。此前的实现
// 解析六份主题 CSS 的 --color-* 字面量——那份文件结构正被本方案替换,而任何「在脚本里再写一遍
// 生成规则」的做法都会立刻与真实色板漂移(门禁给一个不上屏的值背书)。故本脚本直接 import 生成器,
// 零算法复制。vite-node 是既有工具链自带的运行时(vitest 依赖),不新增依赖、不做构建。
//
// 硬门槛(任一不达标 exit 1,输出留作 commit 证据):
//   textPrimary          × background/surface/elevated/inset/canvas  ≥ 4.5 (正文 AA)
//   textSecondary        × background/surface/elevated                ≥ 4.5 (辅助文字,方案 §4.2 同档)
//   textTertiary         × background/surface                         ≥ 3.0 (次要元信息)
//   textPlaceholder      × surface                                    ≥ 3.0 (占位符,非正文)
//   accentText           × background/surface/selection               ≥ 4.5 (强调色文字)
//   status 四色          × background/surface                         ≥ 4.5 (状态色直接作文本)
//   textOnAccent         × accent/accentHover                         ≥ 4.5 (强调填充上的文字)
//   textOnStatus         × status                                     ≥ 4.5 (状态填充上的文字)
//   accent               × background                                 ≥ 3.0 (UI 图形/焦点)
//   controlBorder/Track  × background/surface/elevated                ≥ 3.0 (有含义的控件边界)
//   canvasText(Secondary) × canvas                                    ≥ 4.5 (画廊/时间轴文字)
//   状态色 × (subtle 合成底)                                          ≥ 4.5 (合成后再算)
//   徽标类别色 × scrim ≥ 3.0;白字 × scrim ≥ 4.5
// 仅报告不拦截:divider(装饰性分隔线,方案 §4.2 明确不套控件门槛)、canvasPlaceholder。
//
// 覆盖范围:内置三预设的浅深两档 + 出厂默认。**任意自定义色不承诺达标**(方案 §4.2),
// 故门禁不在这里硬套;自定义色下的观感由截图矩阵交人眼。
//
// 用法:npm run check:contrast

import { contrastRatio, parseColorToRgb } from '../src/utils/color.ts'
import { minContrast } from '../src/themes/colors.ts'
import { MIN_CONTROL_CONTRAST, MIN_TEXT_CONTRAST, generateTheme } from '../src/themes/generate.ts'
import { BUILTIN_PRESETS, DEFAULT_THEME_DEFINITION } from '../src/themes/presets.ts'

const RGBA = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,]+([\d.]+))?\s*\)$/i

/** 半透明色合成到实际底色上:alpha 颜色必须先合成再算对比,否则会把半透明色当成实心(虚假绿灯)。 */
function composite(base, overlay) {
  const top = RGBA.exec(overlay.trim())
  // 实色(hex)无需合成;不可解析时退回底色(该组合随后必然算成 1:1 而报红,不留假绿)。
  if (!top) return overlay
  const rgb = parseColorToRgb(base)
  if (!rgb) return base
  const alpha = top[4] === undefined ? 1 : Number(top[4])
  const channel = (index, under) => Math.round(Number(top[index]) * alpha + under * (1 - alpha))
  return (
    '#' +
    [channel(1, rgb.r), channel(2, rgb.g), channel(3, rgb.b)]
      .map((value) => value.toString(16).padStart(2, '0'))
      .join('')
  )
}

let failures = 0

/** 一条硬门槛;report 为真时只报告不拦截。 */
function gate(label, foreground, background, threshold, report = false) {
  const ratio = contrastRatio(foreground, background)
  const pass = report || ratio >= threshold
  if (!pass) failures += 1
  const tag = report ? '·' : pass ? '✓' : '✗'
  const need = report ? '(报告)' : `(≥${threshold})`
  console.log(`  ${tag}  ${label}: ${ratio.toFixed(2)} ${need}`)
  return ratio
}

const STATUS_KEYS = ['success', 'warning', 'error', 'info']
const ON_STATUS_KEYS = {
  success: 'textOnSuccess',
  warning: 'textOnWarning',
  error: 'textOnError',
  info: 'textOnInfo',
}
const SUBTLE_KEYS = {
  success: 'successSubtle',
  warning: 'warningSubtle',
  error: 'errorSubtle',
  info: 'infoSubtle',
}

/** 一份含模式名的色板清单:三预设的浅深两档 + 出厂默认(去重)。 */
function palettes() {
  const entries = []
  const seen = new Set()
  for (const preset of BUILTIN_PRESETS) {
    for (const mode of ['light', 'dark']) {
      const palette = generateTheme(preset.definition[mode], mode, preset.definition.visualStyle)
      const key = `${preset.id}-${mode}`
      if (seen.has(key)) continue
      seen.add(key)
      entries.push({ label: key, preset, palette })
    }
  }
  entries.push({
    label: 'default-definition-light',
    preset: { id: 'default' },
    palette: generateTheme(DEFAULT_THEME_DEFINITION.light, 'light'),
  })
  entries.push({
    label: 'default-definition-dark',
    preset: { id: 'default' },
    palette: generateTheme(DEFAULT_THEME_DEFINITION.dark, 'dark'),
  })
  return entries
}

console.log('主题对比度门(生成的色板,非 CSS 字面量)')

for (const { label, preset, palette: p } of palettes()) {
  console.log(`\n═══ ${label} ═══`)

  for (const base of ['background', 'surface', 'elevated', 'inset', 'canvas']) {
    gate(`textPrimary × ${base}`, p.textPrimary, p[base], MIN_TEXT_CONTRAST)
  }
  for (const base of ['background', 'surface', 'elevated']) {
    gate(`textSecondary × ${base}`, p.textSecondary, p[base], MIN_TEXT_CONTRAST)
  }
  if (preset.definition?.visualStyle !== undefined && preset.definition.visualStyle !== 'standard') {
    for (const base of ['shellBackground', 'shellSurface', 'shellElevated']) {
      gate(`shellTextPrimary × ${base}`, p.shellTextPrimary, p[base], MIN_TEXT_CONTRAST)
      gate(`shellTextSecondary × ${base}`, p.shellTextSecondary, p[base], MIN_TEXT_CONTRAST)
    }
    gate('shellAccentText × shellSelection', p.shellAccentText, p.shellSelection, MIN_TEXT_CONTRAST)
    for (const key of ['shellControlBorder', 'shellControlTrack']) {
      for (const base of ['shellBackground', 'shellSurface', 'shellElevated', 'shellInputBg']) {
        gate(`${key} × ${base}`, p[key], p[base], MIN_CONTROL_CONTRAST)
      }
    }
  }
  for (const base of ['background', 'surface']) {
    gate(`textTertiary × ${base}`, p.textTertiary, p[base], 3)
  }
  gate('textPlaceholder × surface', p.textPlaceholder, p.surface, 3)

  for (const base of ['background', 'surface', 'selection']) {
    gate(`accentText × ${base}`, p.accentText, p[base], MIN_TEXT_CONTRAST)
  }
  gate('textOnAccent × accent', p.textOnAccent, p.accent, MIN_TEXT_CONTRAST)
  gate('textOnAccent × accentHover', p.textOnAccent, p.accentHover, MIN_TEXT_CONTRAST)
  gate('accent × background(图形门槛)', p.accent, p.background, MIN_CONTROL_CONTRAST)

  for (const key of STATUS_KEYS) {
    for (const base of ['background', 'surface']) {
      gate(`${key} × ${base}`, p[key], p[base], MIN_TEXT_CONTRAST)
    }
    gate(`${ON_STATUS_KEYS[key]} × ${key}`, p[ON_STATUS_KEYS[key]], p[key], MIN_TEXT_CONTRAST)
    // 状态色的浅底是半透明的:必须先在它承载的实际底色上合成再算。
    gate(
      `${key} × ${SUBTLE_KEYS[key]} on background`,
      p[key],
      composite(p.background, p[SUBTLE_KEYS[key]]),
      MIN_TEXT_CONTRAST,
    )
    gate(
      `${key} × ${SUBTLE_KEYS[key]} on surface`,
      p[key],
      composite(p.surface, p[SUBTLE_KEYS[key]]),
      MIN_TEXT_CONTRAST,
    )
  }

  const controlBases = [p.background, p.surface, p.elevated]
  for (const key of ['controlBorder', 'controlTrack']) {
    const lowest = minContrast(p[key], controlBases)
    if (lowest < MIN_CONTROL_CONTRAST) failures += 1
    console.log(
      `  ${lowest >= MIN_CONTROL_CONTRAST ? '✓' : '✗'}  ${key} × background/surface/elevated(最低): ${lowest.toFixed(2)} (≥${MIN_CONTROL_CONTRAST})`,
    )
  }

  gate('canvasText × canvas', p.canvasText, p.canvas, MIN_TEXT_CONTRAST)
  gate('canvasTextSecondary × canvas', p.canvasTextSecondary, p.canvas, MIN_TEXT_CONTRAST)

  // 图片上方的徽标压在不可知的底上:按最亮极端(白底)合成 scrim 后验算。
  const scrim = composite('#ffffff', p.badgeScrim)
  for (const key of ['badgeMarkLive', 'badgeMarkAudio', 'badgeMarkDocument', 'ratingAmber']) {
    gate(`${key} × scrim`, p[key], scrim, MIN_CONTROL_CONTRAST)
  }
  gate('#ffffff × scrim', '#ffffff', scrim, MIN_TEXT_CONTRAST)

  // ── 仅报告:装饰性分隔线与占位格面不套控件门槛(方案 §4.2)──
  gate('divider × background(装饰,报告)', p.divider, p.background, MIN_CONTROL_CONTRAST, true)
  gate(
    'canvasPlaceholder × canvas(占位格面,报告)',
    p.canvasPlaceholder,
    p.canvas,
    MIN_CONTROL_CONTRAST,
    true,
  )
}

if (failures > 0) {
  console.error(`\n✗ ${failures} 个硬门槛组合不达标`)
  process.exit(1)
}
console.log('\n✓ 全部硬门槛通过')
