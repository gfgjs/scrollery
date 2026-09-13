// 主题对比度验证(设计 docs/designs/2026-07-06-前端UI优化与多主题系统.md §7):
// 对 themes/ 下每套主题计算关键「文本 × 底色」组合的 WCAG 2.x 对比度。
//
// 硬门槛(任一不达标 exit 1,输出留作 commit 证据):
//   text-primary        × bg-primary/secondary/surface/elevated  ≥ 4.5 (正文 AA)
//   text-secondary      × bg-primary/surface                     ≥ 3.0 (次级,设计 §5.1)
//   text-secondary      × bg-secondary                            ≥ 4.5 (2026-07-10 新增:
//                          侧栏群组标签/粘性标题等 bg-secondary 底上的可交互文本,须正文 AA)
//   text-tertiary       × bg-primary/surface                     ≥ 3.0 (2026-07-10 升门禁:
//                          元数据/时间戳等真实文本 28 文件消费,原值全线 2.4-2.8)
//   text-placeholder    × bg-surface                             ≥ 3.0 (2026-07-10 升门禁)
//   success/warning/error/info × bg-primary/surface              ≥ 4.5 (2026-07-10 新增:
//                          状态色全库 30+ 处直接作文本色,须按正文 AA 把关)
//   sidebar-active-text × bg-primary/secondary                   ≥ 4.5 (导航文本)
//   accent              × bg-primary                             ≥ 3.0 (UI 字形/图形)
// 其余组合(accent-hover 等)仅报告不拦截。
//
// 用法: node scripts/check-theme-contrast.mjs
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname, basename } from 'node:path'
import { fileURLToPath } from 'node:url'

const stylesDir = join(
  dirname(fileURLToPath(import.meta.url)),
  '../src/assets/styles',
)
const themesDir = join(stylesDir, 'themes')

/** 解析一份主题 CSS 的自定义属性表(--x: value)。 */
function parseProps(css) {
  const props = {}
  for (const m of css.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    props[m[1]] = m[2].trim()
  }
  return props
}

/** hex/rgb/rgba → [r,g,b](0-255);带 alpha 时按给定底色合成。其他写法返回 null。 */
function resolveColor(value, bgRgb) {
  let m = value.match(/^#([0-9a-fA-F]{6})$/)
  if (m) {
    const n = parseInt(m[1], 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
  }
  m = value.match(/^#([0-9a-fA-F]{3})$/)
  if (m) {
    return [...m[1]].map((c) => parseInt(c + c, 16))
  }
  // 浓度表达式(2026-09-06,锚点由 check:theme-palette 钉死):按满浓度锚点评估。
  // 方向不对称,须分开说:底色稀释(底向中性靠拢)让对比单调改善,锚点=最保守界;
  // 文字稀释(文字向底色靠拢)让对比单调恶化,锚点评估的是出厂满浓度色板——稀释是
  // 用户在设置页的主动取舍,默认档(75)已实测 primary 6.1–8.0 / secondary 3.3–3.7
  // 仍守各自门槛(默认底色 tint 60 上,见 status/UI-多主题系统.md 2026-09-06 文字浓度条目)。
  m = value.match(
    /^color-mix\(in oklch, (#[0-9a-fA-F]{6}) calc\(var\(--theme-(?:tint|text)-scale, 1\) \* 100%\), /,
  )
  if (m) {
    const n = parseInt(m[1].slice(1), 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
  }
  m = value.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/)
  if (m) {
    const [r, g, b] = [Number(m[1]), Number(m[2]), Number(m[3])]
    const a = m[4] === undefined ? 1 : Number(m[4])
    if (a >= 1 || !bgRgb) return [r, g, b]
    // alpha 合成到底色(前景文本常见半透明写法)
    return [0, 1, 2].map((i) => Math.round([r, g, b][i] * a + bgRgb[i] * (1 - a)))
  }
  return null
}

/** WCAG 相对亮度。 */
function luminance([r, g, b]) {
  const lin = (c) => {
    const s = c / 255
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4)
  }
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

function contrast(fgRgb, bgRgb) {
  const l1 = luminance(fgRgb)
  const l2 = luminance(bgRgb)
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
}

// [前景 token, 底色 token, 硬门槛(null=仅报告)]
const PAIRS = [
  ['--color-text-primary', '--color-bg-primary', 4.5],
  ['--color-text-primary', '--color-bg-secondary', 4.5],
  ['--color-text-primary', '--color-bg-surface', 4.5],
  ['--color-text-primary', '--color-bg-elevated', 4.5],
  ['--color-text-secondary', '--color-bg-primary', 3.0],
  ['--color-text-secondary', '--color-bg-secondary', 4.5],
  ['--color-text-secondary', '--color-bg-surface', 3.0],
  ['--color-text-tertiary', '--color-bg-primary', 3.0],
  ['--color-text-tertiary', '--color-bg-surface', 3.0],
  ['--color-text-placeholder', '--color-bg-surface', 3.0],
  ['--color-success', '--color-bg-primary', 4.5],
  ['--color-success', '--color-bg-surface', 4.5],
  ['--color-warning', '--color-bg-primary', 4.5],
  ['--color-warning', '--color-bg-surface', 4.5],
  ['--color-error', '--color-bg-primary', 4.5],
  ['--color-error', '--color-bg-surface', 4.5],
  ['--color-info', '--color-bg-primary', 4.5],
  ['--color-info', '--color-bg-surface', 4.5],
  ['--color-sidebar-active-text', '--color-bg-primary', 4.5],
  ['--color-sidebar-active-text', '--color-bg-secondary', 4.5],
  ['--color-accent', '--color-bg-primary', 3.0],
  ['--color-accent-text', '--color-bg-primary', 4.5],
  ['--color-accent-text', '--color-bg-secondary', 4.5],
  ['--color-accent-text', '--color-bg-surface', 4.5],
  ['--color-text-on-accent', '--color-accent', 4.5],
  ['--color-text-on-accent', '--color-accent-hover', 4.5],
  ['--color-text-on-success', '--color-success', 4.5],
  ['--color-text-on-warning', '--color-warning', 4.5],
  ['--color-text-on-error', '--color-error', 4.5],
  ['--color-text-on-info', '--color-info', 4.5],
  // ── 仅报告 ──
  ['--color-accent-hover', '--color-bg-primary', null],
]

// 前景 token、半透明浅底 token、浅底所处的实际表面、硬门槛。
// 透明色必须先合成再计算,否则会把 alpha 颜色误当成实心前景而给出虚假绿灯。
const COMPOSED_PAIRS = [
  ['--color-accent-text', '--color-accent-subtle', '--color-bg-primary', 4.5],
  ['--color-accent-text', '--color-accent-subtle', '--color-bg-surface', 4.5],
  ['--color-success', '--color-success-subtle', '--color-bg-primary', 4.5],
  ['--color-success', '--color-success-subtle', '--color-bg-surface', 4.5],
  ['--color-warning', '--color-warning-subtle', '--color-bg-primary', 4.5],
  ['--color-warning', '--color-warning-subtle', '--color-bg-surface', 4.5],
  ['--color-error', '--color-error-subtle', '--color-bg-primary', 4.5],
  ['--color-error', '--color-error-subtle', '--color-bg-surface', 4.5],
  ['--color-info', '--color-info-subtle', '--color-bg-primary', 4.5],
  ['--color-info', '--color-info-subtle', '--color-bg-surface', 4.5],
]

const SHARED_PROPS = parseProps(readFileSync(join(stylesDir, 'variables.css'), 'utf8'))

let failures = 0
const files = readdirSync(themesDir).filter((f) => f.endsWith('.css')).sort()

for (const file of files) {
  const id = basename(file, '.css')
  const props = {
    ...SHARED_PROPS,
    ...parseProps(readFileSync(join(themesDir, file), 'utf-8')),
  }
  console.log(`\n═══ ${id} ═══`)
  for (const [fgKey, bgKey, threshold] of PAIRS) {
    const bgRgb = resolveColor(props[bgKey] ?? '', null)
    if (!bgRgb) {
      console.log(`  ?  ${fgKey} × ${bgKey}: 底色非纯色(${props[bgKey]}),跳过`)
      continue
    }
    const fgRgb = resolveColor(props[fgKey] ?? '', bgRgb)
    if (!fgRgb) {
      console.log(`  ?  ${fgKey} × ${bgKey}: 前景不可解析(${props[fgKey]}),跳过`)
      continue
    }
    const r = contrast(fgRgb, bgRgb)
    const tag =
      threshold === null ? '·' : r >= threshold ? '✓' : (failures++, '✗')
    const req = threshold === null ? '(报告)' : `(≥${threshold})`
    console.log(`  ${tag}  ${fgKey} × ${bgKey}: ${r.toFixed(2)} ${req}`)
  }

  for (const [fgKey, overlayKey, baseKey, threshold] of COMPOSED_PAIRS) {
    const baseRgb = resolveColor(props[baseKey] ?? '', null)
    const bgRgb = baseRgb && resolveColor(props[overlayKey] ?? '', baseRgb)
    const fgRgb = bgRgb && resolveColor(props[fgKey] ?? '', bgRgb)
    if (!baseRgb || !bgRgb || !fgRgb) {
      console.log(
        `  ?  ${fgKey} × ${overlayKey} on ${baseKey}: 色值不可解析,跳过`,
      )
      continue
    }
    const r = contrast(fgRgb, bgRgb)
    const tag = r >= threshold ? '✓' : (failures++, '✗')
    console.log(
      `  ${tag}  ${fgKey} × ${overlayKey} on ${baseKey}: ${r.toFixed(2)} (≥${threshold})`,
    )
  }

  // 徽标在照片上必须按最亮极端(白底)合成后验算;scrim 60% 黑在白底上为 #666。
  const scrimRgb = resolveColor(props['--color-badge-scrim'] ?? '', [255, 255, 255])
  if (scrimRgb) {
    const badgePairs = [
      ['--color-badge-mark-live', null, 3.0],
      ['--color-badge-mark-audio', null, 3.0],
      ['--color-badge-mark-document', null, 3.0],
      ['--color-rating-amber', null, 3.0],
    ]
    for (const [fgKey, , threshold] of badgePairs) {
      const fgRgb = resolveColor(props[fgKey] ?? '', scrimRgb)
      if (!fgRgb) {
        console.log(`  ?  ${fgKey} × --color-badge-scrim: 前景不可解析,跳过`)
        continue
      }
      const r = contrast(fgRgb, scrimRgb)
      const tag = r >= threshold ? '✓' : (failures++, '✗')
      console.log(
        `  ${tag}  ${fgKey} × --color-badge-scrim: ${r.toFixed(2)} (≥${threshold})`,
      )
    }
    const badgeTextContrast = contrast([255, 255, 255], scrimRgb)
    const badgeTextTag = badgeTextContrast >= 4.5 ? '✓' : (failures++, '✗')
    console.log(
      `  ${badgeTextTag}  #ffffff × --color-badge-scrim: ${badgeTextContrast.toFixed(2)} (≥4.5)`,
    )
  }
}

if (failures > 0) {
  console.error(`\n✗ ${failures} 个硬门槛组合不达标`)
  process.exit(1)
}
console.log('\n✓ 全部硬门槛通过')
