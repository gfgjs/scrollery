// 静态主题色板校验与更新器。
//
// 下方值是生成器锚点。CSS 保留完整语义 token 集，便于检视并支持无构建步骤运行。
// --check 用于 CI，--write 用于修复生成声明。
//
// 底色 wash token(bg 五层/canvas 三件/doc-paper 两件,2026-09-06 主题色浓度)与文字 ramp
// (text 四件,2026-09-06 文字浓度)在 CSS 中不是裸 hex,而是由锚点生成的 color-mix 浓度
// 表达式(tintExpr/textExpr):锚点=满浓度(100%)原色,运行时分别由 --theme-tint-scale /
// --theme-text-scale 向中性色稀释。亮主题向白混,暗主题向同明度无彩灰混(oklch 相对色语法,
// 单源自锚点)。两机制的运行时常量与消费约定见 src/themes/strength.ts,本脚本是表达式的
// 唯一权威,改锚点后跑 --write 同步六主题。
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const themesDir = join(dirname(fileURLToPath(import.meta.url)), '../src/assets/styles/themes')

/** 浓度 mix 表达式(与 src/themes/strength.ts 的消费约定配对)。 */
function strengthExpr(varName, hex, kind) {
  const neutral = kind === 'light' ? '#ffffff' : `oklch(from ${hex} l 0 h)`
  return `color-mix(in oklch, ${hex} calc(var(--${varName}, 1) * 100%), ${neutral})`
}

function tintExpr(hex, kind) {
  return strengthExpr('theme-tint-scale', hex, kind)
}

function textExpr(hex, kind) {
  return strengthExpr('theme-text-scale', hex, kind)
}

const STATUS = {
  light: {
    success: '#18702f',
    warning: '#a14705',
    error: '#b21f26',
    info: '#0959a4',
    on: '#ffffff',
  },
  dark: {
    success: '#34c759',
    warning: '#ff9f0a',
    error: '#ff665d',
    info: '#64d2ff',
  },
}

const THEME_ANCHORS = {
  'fresh-light': {
    kind: 'light',
    layers: {
      primary: '#edf5f1',
      secondary: '#f4f9f6',
      surface: '#fcfffd',
      elevated: '#ffffff',
      inset: '#e2ede7',
    },
    canvas: { canvas: '#edf5f1', placeholder: '#d5e4dc', gap: '#edf5f1' },
    paper: { paper: '#f7fbf9', line: '#cde0d6' },
    text: { primary: '#132a22', secondary: '#3d594e', tertiary: '#5f7b70', placeholder: '#5f7b70' },
    accent: { base: '#0b7c52', hover: '#086140', text: '#0a6e49', on: '#ffffff' },
  },
  'fresh-dark': {
    kind: 'dark',
    layers: {
      primary: '#0d1512',
      secondary: '#121c18',
      surface: '#18241f',
      elevated: '#1e2d27',
      inset: '#090e0c',
    },
    canvas: { canvas: '#101815', placeholder: '#23342d', gap: '#101815' },
    paper: { paper: '#16221d', line: '#2a4036' },
    text: { primary: '#edf7f2', secondary: '#a4c2b4', tertiary: '#78998b', placeholder: '#8ba699' },
    accent: { base: '#34d399', hover: '#6ee7b7', text: '#34d399', on: '#091e15' },
  },
  'minimal-light': {
    kind: 'light',
    layers: {
      primary: '#f1f3f5',
      secondary: '#f8f9fa',
      surface: '#ffffff',
      elevated: '#ffffff',
      inset: '#e9ecef',
    },
    canvas: { canvas: '#f1f3f5', placeholder: '#dee2e6', gap: '#f1f3f5' },
    paper: { paper: '#fcfcfd', line: '#dee2e6' },
    text: { primary: '#18181b', secondary: '#52525b', tertiary: '#71717a', placeholder: '#71717a' },
    accent: { base: '#27272a', hover: '#18181b', text: '#27272a', on: '#ffffff' },
  },
  'minimal-dark': {
    kind: 'dark',
    layers: {
      primary: '#121214',
      secondary: '#18181b',
      surface: '#202023',
      elevated: '#27272a',
      inset: '#0c0c0e',
    },
    canvas: { canvas: '#141416', placeholder: '#27272a', gap: '#141416' },
    paper: { paper: '#1a1a1d', line: '#323236' },
    text: { primary: '#f4f4f5', secondary: '#a1a1aa', tertiary: '#71717a', placeholder: '#8e8e93' },
    accent: { base: '#e4e4e7', hover: '#ffffff', text: '#e4e4e7', on: '#18181b' },
  },
  'tech-light': {
    kind: 'light',
    layers: {
      primary: '#ebf1fa',
      secondary: '#f4f7fc',
      surface: '#fbfdff',
      elevated: '#ffffff',
      inset: '#dfe7f5',
    },
    canvas: { canvas: '#ebf1fa', placeholder: '#d2def0', gap: '#ebf1fa' },
    paper: { paper: '#f7faff', line: '#ccdaed' },
    text: { primary: '#111f38', secondary: '#3d5073', tertiary: '#5f7296', placeholder: '#5f7296' },
    accent: { base: '#2563eb', hover: '#1d4ed8', text: '#1d4ed8', on: '#ffffff' },
  },
  'tech-dark': {
    kind: 'dark',
    layers: {
      primary: '#0e131b',
      secondary: '#141b26',
      surface: '#1a2330',
      elevated: '#222d3d',
      inset: '#090d13',
    },
    canvas: { canvas: '#111721', placeholder: '#263345', gap: '#111721' },
    paper: { paper: '#18212e', line: '#2e3e54' },
    text: { primary: '#eff4fc', secondary: '#a2b4cf', tertiary: '#768aa8', placeholder: '#889cb8' },
    accent: { base: '#60a5fa', hover: '#93c5fd', text: '#60a5fa', on: '#0b192e' },
  },
}

function rgbOfHex(hex) {
  const value = Number.parseInt(hex.slice(1), 16)
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255]
}

function rgbaOfHex(hex, alpha) {
  const [r, g, b] = rgbOfHex(hex)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function generatedEntries(theme) {
  const kind = theme.kind
  const entries = {
    '--color-bg-primary': tintExpr(theme.layers.primary, kind),
    '--color-bg-secondary': tintExpr(theme.layers.secondary, kind),
    '--color-bg-surface': tintExpr(theme.layers.surface, kind),
    '--color-bg-elevated': tintExpr(theme.layers.elevated, kind),
    '--color-bg-inset': tintExpr(theme.layers.inset, kind),
    '--color-bg-canvas': tintExpr(theme.canvas.canvas, kind),
    '--color-bg-canvas-placeholder': tintExpr(theme.canvas.placeholder, kind),
    '--color-bg-canvas-gap': tintExpr(theme.canvas.gap, kind),
    '--color-doc-paper': tintExpr(theme.paper.paper, kind),
    '--color-doc-paper-line': tintExpr(theme.paper.line, kind),
    '--color-text-primary': textExpr(theme.text.primary, kind),
    '--color-text-secondary': textExpr(theme.text.secondary, kind),
    '--color-text-tertiary': textExpr(theme.text.tertiary, kind),
    '--color-text-placeholder': textExpr(theme.text.placeholder, kind),
    '--color-accent': theme.accent.base,
    '--color-accent-hover': theme.accent.hover,
    '--color-accent-subtle': rgbaOfHex(theme.accent.base, theme.kind === 'light' ? 0.1 : 0.14),
    '--color-accent-text': theme.accent.text,
    '--color-text-on-accent': theme.accent.on,
    '--color-bg-active': 'var(--color-accent-subtle)',
    '--color-sidebar-active-bg': 'var(--color-accent-subtle)',
    '--color-focus-ring': rgbaOfHex(theme.accent.base, theme.kind === 'light' ? 0.42 : 0.46),
  }
  const statuses = STATUS[theme.kind]
  for (const [name, color] of Object.entries(statuses)) {
    if (name === 'on') continue
    entries[`--color-${name}`] = color
    entries[`--color-${name}-subtle`] = rgbaOfHex(color, 0.08)
    entries[`--color-text-on-${name}`] = theme.kind === 'light' ? statuses.on : theme.layers.primary
  }
  return entries
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

const write = process.argv.includes('--write')
const mismatches = []
let written = 0
const expectedFiles = new Set(Object.keys(THEME_ANCHORS).map((id) => `${id}.css`))
const actualFiles = readdirSync(themesDir).filter((file) => file.endsWith('.css'))

for (const file of actualFiles) {
  if (!expectedFiles.has(file)) mismatches.push(`旧主题文件仍存在: ${file}`)
}
for (const file of expectedFiles) {
  if (!actualFiles.includes(file)) mismatches.push(`缺少主题文件: ${file}`)
}

for (const [id, theme] of Object.entries(THEME_ANCHORS)) {
  const file = join(themesDir, `${id}.css`)
  if (!actualFiles.includes(`${id}.css`)) continue

  let css = readFileSync(file, 'utf8')
  for (const [key, expected] of Object.entries(generatedEntries(theme))) {
    const declarationPattern = new RegExp(`${escapeRegExp(key)}\\s*:\\s*([^;]+);`)
    const actualValue = css.match(declarationPattern)?.[1]?.trim()
    if (!actualValue) {
      mismatches.push(`${id}:缺少 ${key}(期望 ${expected})`)
      continue
    }
    if (actualValue !== expected) {
      mismatches.push(`${id}:${key}=${actualValue}(期望 ${expected})`)
      if (write) {
        const replacePattern = new RegExp(`(${escapeRegExp(key)}\\s*:\\s*)[^;]+(;)`)
        css = css.replace(replacePattern, `$1${expected}$2`)
        written++
      }
    }
  }
  if (write) writeFileSync(file, css)
}

if (mismatches.length > 0 && !write) {
  console.error(`✗ ${mismatches.length} 个主题色板问题:`)
  for (const mismatch of mismatches) console.error(`  - ${mismatch}`)
  process.exit(1)
}

if (mismatches.length > 0 && write) {
  console.log(`✓ 已按新色板写回 ${written} 个静态声明`)
} else {
  console.log('✓ 六主题静态色板与锚点一致')
}
