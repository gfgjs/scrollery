// src/themes/theme-contract.spec.ts
// 主题 token 契约测试(设计 §4.6):themes/ 下所有主题的 CSS 变量键集必须完全一致,
// 新主题不齐键直接红。本测试即未来「主题商店」manifest 校验器的雏形——外置主题
// 上架前跑同一套键集校验。
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join, basename, extname, relative } from 'node:path'
import { BUILTIN_THEMES, DEFAULT_LIGHT_THEME, DEFAULT_DARK_THEME, getTheme } from './registry'
import { CANONICAL_BADGE_COLORS } from '../components/media/mediaGridCanvas.palette'

// 直接 fs 读 CSS 源码(vitest 为 node 环境)。不用 import.meta.glob('?raw'):
// vitest 默认对 .css 做空桩(test.css=false),?raw 查询同样被截空——实测返回 ''。
const stylesDir = join(dirname(fileURLToPath(import.meta.url)), '../assets/styles')
const themesDir = join(stylesDir, 'themes')
const styleIndexCss = readFileSync(join(stylesDir, 'index.css'), 'utf-8')
const variablesCss = readFileSync(join(stylesDir, 'variables.css'), 'utf-8')
const materialCss = readFileSync(join(stylesDir, 'material.css'), 'utf-8')
const cssById = new Map<string, string>(
  readdirSync(themesDir)
    .filter((f) => f.endsWith('.css'))
    .map((f) => [basename(f, '.css'), readFileSync(join(themesDir, f), 'utf-8')]),
)

/** 提取声明位置的自定义属性名(`--x:` 形式;var(--x) 用法因后随 `)` 不会被误捕)。 */
function customPropsOf(css: string): Set<string> {
  return new Set(css.match(/--[\w-]+(?=\s*:)/g) ?? [])
}

/** 提取自定义属性声明及其值；供 adapter 的精确别名契约使用。 */
function customPropEntriesOf(css: string): [string, string][] {
  return [...stripComments(css).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(([, key, value]) => [
    key,
    value.replace(/\s+/g, ' ').replace(/\(\s+/g, '(').replace(/\s+\)/g, ')').trim(),
  ])
}

const srcDir = join(dirname(fileURLToPath(import.meta.url)), '..')
const CODE_EXT = new Set(['.vue', '.css', '.ts'])

function walkSrc(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry)
    if (statSync(p).isDirectory()) {
      if (entry !== 'vendor') walkSrc(p, out)
    } else if (CODE_EXT.has(extname(p))) out.push(p)
  }
  return out
}

/**
 * 剥注释后再扫 token 引用。
 *
 * 必须剥:本仓修幽灵 token 时按约定在注释里留旧名作历史记录(如「原 var(--color-danger)
 * 为幽灵 token(S5 修)」)。不剥则已修站点被永久误报,门禁反而惩罚良好注释。
 * 只剥不解析:宁可漏判(少报一个幽灵)也不误判(把已修站点报红)——`//` 规避 `://` 防吃 URL。
 */
function stripComments(src: string): string {
  return src
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/(^|[^:])\/\/[^\n]*/g, '$1 ')
}

/** 引用位置的 `--color-*`:CSS 的 `var(--x)` + canvas 侧读回的字符串字面量 `'--x'`。 */
function colorTokenRefsOf(src: string): string[] {
  return [
    ...stripComments(src).matchAll(/var\(\s*(--color-[a-z0-9-]+)|['"](--color-[a-z0-9-]+)['"]/g),
  ]
    .map((m) => m[1] || m[2])
    .filter(Boolean)
}

const ACCENT_ROLE_KEYS = [
  '--color-accent',
  '--color-accent-hover',
  '--color-accent-subtle',
  '--color-accent-text',
  '--color-text-on-accent',
] as const

const STATUS_ROLE_KEYS = ['success', 'warning', 'error', 'info'].flatMap((name) => [
  `--color-${name}`,
  `--color-${name}-subtle`,
  `--color-text-on-${name}`,
])

const CANONICAL_MEDIA_KEYS = [
  ['--color-badge-scrim', 'scrim'],
  ['--color-badge-mark-live', 'markLive'],
  ['--color-badge-mark-audio', 'markAudio'],
  ['--color-badge-mark-document', 'markDocument'],
  ['--color-rating-amber', 'ratingAmber'],
] as const

const NEUTRAL_LAYER_KEYS = [
  '--color-bg-primary',
  '--color-bg-secondary',
  '--color-bg-surface',
  '--color-bg-elevated',
  '--color-bg-inset',
] as const

/**
 * 浓度 mix 表达式的满浓度锚点色(wash token 形如
 * `color-mix(in oklch, #hex calc(var(--theme-tint-scale, 1) * 100%), <中性>)`):
 * scale=1 时 mix 结果即锚点本身,故按锚点评估既是运行时的上界,也让本文件无需复刻
 * CSS 的 oklch 插值。纯色声明(非 wash token)原样返回。
 */
function anchorColorOf(declaration: string): string {
  return declaration.match(/^color-mix\(in oklch, (#[0-9a-fA-F]{6}) /)?.[1] ?? declaration
}

function luminanceOf(value: string): number | null {
  const match = anchorColorOf(value.trim()).match(/^#([0-9a-fA-F]{6})$/)
  if (!match) return null
  const n = Number.parseInt(match[1], 16)
  const channels = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((channel) => {
    const srgb = channel / 255
    return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]
}

function themeProps(css: string): Record<string, string> {
  return Object.fromEntries(
    [...css.matchAll(/(--[A-Za-z0-9-]+)\s*:\s*([^;]+);/g)].map(([, key, value]) => [
      key,
      value.trim(),
    ]),
  )
}

describe('--color-* token 引用闭环', () => {
  // 定义面 = 语义色层(六套主题)+共享色板(variables.css);共享色板用于跨主题的
  // 媒体徽标/评分等内容标记,不能被误报成幽灵 token。
  const definedTokens = new Set<string>(
    [...cssById.values(), variablesCss]
      .flatMap((css) => [...stripComments(css).matchAll(/(--color-[a-z0-9-]+)\s*:/g)])
      .map((m) => m[1]),
  )

  it('全库消费的 --color-* 均在主题层有定义(无幽灵 token)', () => {
    // 幽灵 token = 被 var() 消费却无人定义。带 fallback 时静默走死值(不随主题),
    // 无 fallback 时该声明整条失效(渲染透明/继承色)——两种都是肉眼难察的真 bug,
    // 且历史上反复复发(S5/S6/S7 各修过一批)。故用门禁钉死,而非靠人肉复查。
    const ghosts = new Map<string, string[]>()
    for (const file of walkSrc(srcDir)) {
      for (const token of colorTokenRefsOf(readFileSync(file, 'utf-8'))) {
        if (definedTokens.has(token)) continue
        const rel = relative(srcDir, file).replace(/\\/g, '/')
        const hit = ghosts.get(token) ?? []
        if (!hit.includes(rel)) hit.push(rel)
        ghosts.set(token, hit)
      }
    }
    const report = [...ghosts].map(([t, files]) => `${t} <- ${files.join(', ')}`)
    expect(
      report,
      '存在幽灵 token(消费但六套主题均未定义):须在主题层补定义,或改用既有同义 token',
    ).toEqual([])
  })

  it('主题层定义的 --color-* 均有人消费(无死 token)', () => {
    // 死 token = 六主题都定义、却无人 var()。它不渲染任何像素,却持续骗人:
    // check:contrast 曾把硬门花在 --color-text-placeholder 上,而占位符实际由
    // tertiary 渲染——门禁在为一个不上屏的值背书。且死票只能靠人肉复查,
    // 本仓一次攒出 5 个(S7 清理:2 个接线、3 个证实无靶后删)。故双向钉死:
    // 定义面与消费面必须严格互等,新增 token 须同批接线。
    const consumed = new Set(
      walkSrc(srcDir).flatMap((f) => colorTokenRefsOf(readFileSync(f, 'utf-8'))),
    )
    const dead = [...definedTokens].filter((t) => !consumed.has(t)).sort()
    expect(
      dead,
      '存在死 token(主题层有定义但全库无人 var()):须接线到真实消费者,或从六套主题一并删除',
    ).toEqual([])
  })

  it('定义面非空且覆盖语义色大类(扫描器自身防呆)', () => {
    // 上一条是「集合包含」断言:若定义面因正则/路径失效而误收成空集,幽灵表反而爆红——
    // 但若引用面误收成空集,断言会假绿。故正向钉住两面都非空。
    expect(definedTokens.size).toBeGreaterThanOrEqual(30)
    expect(definedTokens.has('--color-accent')).toBe(true)
    const refs = walkSrc(srcDir).flatMap((f) => colorTokenRefsOf(readFileSync(f, 'utf-8')))
    expect(new Set(refs).size).toBeGreaterThanOrEqual(30)
  })

  it('共享媒体色板 token 只定义一次且由 Canvas 调色板读取', () => {
    const paletteCss = readFileSync(
      join(srcDir, 'components/media/mediaGridCanvas.palette.ts'),
      'utf-8',
    )
    for (const [token, constantKey] of CANONICAL_MEDIA_KEYS) {
      const declaration = variablesCss.match(new RegExp(`${token}\\s*:\\s*([^;]+);`))
      expect(declaration, `variables.css 缺共享 token ${token}`).not.toBeNull()
      expect(declaration?.[1].trim(), `${token} 与 canonical fallback 漂移`).toBe(
        CANONICAL_BADGE_COLORS[constantKey],
      )
      for (const [id, css] of cssById) {
        expect(css, `${id}.css 不得重复覆写共享 token ${token}`).not.toMatch(
          new RegExp(`${token}\\s*:`),
        )
      }
      expect(paletteCss, `palette.ts 未读取 canonical token ${token}`).toContain(
        `g('${token}', CANONICAL_BADGE_COLORS.${constantKey})`,
      )
    }
  })
})

const MATERIAL_CONTRACT = {
  '--material-canvas': 'var(--color-bg-primary)',
  '--material-chrome': 'var(--color-bg-secondary)',
  '--material-surface': 'var(--color-bg-surface, var(--card-bg))',
  '--material-surface-strong': 'var(--card-bg, var(--color-bg-elevated))',
  '--material-surface-muted': 'var(--color-bg-inset)',
  '--material-float': 'var(--card-bg, var(--popover-bg, var(--color-bg-elevated)))',
  '--material-border': 'var(--color-border)',
  '--material-border-strong': 'var(--color-border-strong)',
  '--material-shadow-shell': 'var(--shadow-sm)',
  '--material-shadow-float': 'var(--popover-shadow, var(--shadow-lg))',
  '--material-glaze': 'none',
  '--material-blur-float': '0px',
  '--material-noise': 'var(--texture-chrome, none)',
  '--material-recipe-canvas-background-color': 'var(--material-canvas, var(--color-bg-primary))',
  '--material-recipe-chrome-background-color': 'var(--material-chrome, var(--color-bg-secondary))',
  '--material-recipe-chrome-background-image':
    'var(--material-glaze, none), var(--material-noise, none)',
  '--material-recipe-chrome-border-color': 'var(--material-border, var(--color-border))',
  '--material-recipe-surface-background-color': 'var(--material-surface, var(--color-bg-surface))',
  '--material-recipe-surface-border-color':
    'var(--color-border-subtle, var(--material-border, var(--color-border)))',
  '--material-recipe-surface-box-shadow': 'none',
  '--material-recipe-elevated-background-color':
    'var(--material-surface-strong, var(--card-bg, var(--color-bg-elevated)))',
  '--material-recipe-elevated-border-color':
    'var(--material-border-strong, var(--color-border-strong))',
  '--material-recipe-elevated-box-shadow': 'var(--material-shadow-shell, var(--shadow-sm))',
  '--material-recipe-float-background-color':
    'var(--material-float, var(--popover-bg, var(--color-bg-elevated)))',
  '--material-recipe-float-border-color':
    'var(--material-border-strong, var(--popover-border, var(--color-border-strong)))',
  '--material-recipe-float-box-shadow':
    'var(--material-shadow-float, var(--popover-shadow, var(--shadow-lg)))',
  '--material-recipe-float-backdrop-filter': 'blur(var(--material-blur-float, 0px))',
} as const

describe('material adaptor 契约', () => {
  const materialEntries = customPropEntriesOf(materialCss).filter(([key]) =>
    key.startsWith('--material-'),
  )
  const materialKeys = new Set(materialEntries.map(([key]) => key))
  const variablesKeys = customPropsOf(variablesCss)

  it('固定导入在全部主题之后、任一全局消费者之前', () => {
    const materialImport = "@import './material.css';"
    const materialIndex = styleIndexCss.indexOf(materialImport)
    expect(materialIndex, 'index.css 缺 material.css 导入').toBeGreaterThanOrEqual(0)
    expect(styleIndexCss.lastIndexOf(materialImport), 'material.css 只能导入一次').toBe(
      materialIndex,
    )

    for (const id of cssById.keys()) {
      const themeIndex = styleIndexCss.indexOf(`@import './themes/${id}.css';`)
      expect(themeIndex, `index.css 缺 ${id}.css 导入`).toBeGreaterThanOrEqual(0)
      expect(themeIndex, `${id}.css 必须先于 material.css`).toBeLessThan(materialIndex)
    }

    for (const consumerImport of ["@import './reset.css';", "@import './animations.css';"]) {
      const consumerIndex = styleIndexCss.indexOf(consumerImport)
      expect(consumerIndex, `index.css 缺 ${consumerImport}`).toBeGreaterThan(materialIndex)
    }
    expect(
      styleIndexCss.indexOf('.flex {'),
      'material.css 必须先于 index.css 内全局消费者',
    ).toBeGreaterThan(materialIndex)
  })

  it('角色与可消费 recipe 定义完整且 alias/fallback 不漂移', () => {
    expect(stripComments(materialCss).trim()).toMatch(/^:where\(:root\)\s*\{/)
    expect(materialEntries.length, 'material token 不得重复声明').toBe(materialKeys.size)
    expect([...materialEntries].sort(([a], [b]) => a.localeCompare(b))).toEqual(
      Object.entries(MATERIAL_CONTRACT).sort(([a], [b]) => a.localeCompare(b)),
    )
    expect(materialCss).not.toContain('!important')
  })

  it('主题源键在六主题齐备，共享 source token 均有定义', () => {
    const refs = new Set(
      materialEntries.flatMap(([, value]) =>
        [...value.matchAll(/var\(\s*(--[\w-]+)/g)].map((match) => match[1]),
      ),
    )
    const sourceRefs = [...refs].filter((key) => !materialKeys.has(key))
    const themeSourceRefs = sourceRefs.filter((key) => /^(--color-|--shadow-|--texture-)/.test(key))
    const sharedSourceRefs = sourceRefs.filter((key) => !themeSourceRefs.includes(key))

    for (const [id, css] of cssById) {
      const keys = customPropsOf(css)
      const missing = themeSourceRefs.filter((key) => !keys.has(key))
      expect(missing, `${id}.css 缺 material adaptor 所需主题 source token`).toEqual([])
    }
    const missingShared = sharedSourceRefs.filter((key) => !variablesKeys.has(key))
    expect(missingShared, 'variables.css 缺 material adaptor 所需共享 source token').toEqual([])
  })

  it('旧 token 层不反向引用 material token，且关闭值保持有效', () => {
    const upstreamCss = stripComments([variablesCss, ...cssById.values()].join('\n'))
    expect(upstreamCss.match(/var\(\s*--material-[\w-]+/g) ?? []).toEqual([])
    const values = new Map(materialEntries)
    expect(values.get('--material-glaze')).toBe('none')
    expect(values.get('--material-blur-float')).toBe('0px')
    expect(values.get('--material-recipe-float-background-color')).toContain('--popover-bg')
    expect(values.get('--material-recipe-float-backdrop-filter')).toBe(
      'blur(var(--material-blur-float, 0px))',
    )
  })
})

describe('主题 token 契约', () => {
  it('CSS 文件与注册表条目一一对应', () => {
    const registryIds = BUILTIN_THEMES.map((t) => t.id).sort()
    expect([...cssById.keys()].sort()).toEqual(registryIds)
    // id 唯一
    expect(new Set(registryIds).size).toBe(registryIds.length)
  })

  it('每份主题文件的选择器与文件名一致', () => {
    for (const [id, css] of cssById) {
      expect(css, `${id}.css 缺 [data-theme='${id}'] 选择器`).toContain(`[data-theme='${id}']`)
    }
  })

  it('全部主题键集完全一致且非空', () => {
    const entries = [...cssById.entries()]
    const [refId, refCss] = entries[0]
    const refKeys = customPropsOf(refCss)
    // 语义色层至少覆盖 bg/text/accent/border/shadow/状态色等大类,空壳主题直接红
    expect(refKeys.size).toBeGreaterThanOrEqual(30)
    for (const [id, css] of entries.slice(1)) {
      const keys = customPropsOf(css)
      const missing = [...refKeys].filter((k) => !keys.has(k))
      const extra = [...keys].filter((k) => !refKeys.has(k))
      expect(missing, `${id}.css 相对 ${refId}.css 缺键`).toEqual([])
      expect(extra, `${id}.css 相对 ${refId}.css 多键`).toEqual([])
    }
  })

  it('六主题均声明完整的 accent 五角色', () => {
    for (const [id, css] of cssById) {
      const keys = customPropsOf(css)
      const missing = ACCENT_ROLE_KEYS.filter((key) => !keys.has(key))
      expect(missing, `${id}.css 缺 accent 角色`).toEqual([])
    }
  })

  it('六主题均声明完整的状态 paired roles', () => {
    for (const [id, css] of cssById) {
      const keys = customPropsOf(css)
      const missing = STATUS_ROLE_KEYS.filter((key) => !keys.has(key))
      expect(missing, `${id}.css 缺状态 paired role`).toEqual([])
    }
  })

  it('中性表面明度不发生角色倒置', () => {
    // 锚点层序 = 任意浓度下的层序:各 wash 层共用同一 --theme-tint-scale,亮主题向同一
    // 白点、暗主题向各自同明度无彩灰(oklch L 不变)按同一线性比例插值,序严格保持。
    for (const [id, css] of cssById) {
      const props = themeProps(css)
      const missing = NEUTRAL_LAYER_KEYS.filter((key) => !props[key])
      expect(missing, `${id}.css 缺中性表面层`).toEqual([])
      const values = NEUTRAL_LAYER_KEYS.map((key) => luminanceOf(props[key] ?? ''))
      expect(values.every((value) => value !== null), `${id}.css 中性层须为可解析 hex`).toBe(true)
      if (values.some((value) => value === null)) continue
      const [primary, secondary, surface, elevated, inset] = values as number[]
      const kind = getTheme(id)?.kind
      if (kind === 'light') {
        expect(elevated, `${id}.css elevated 不得低于 surface`).toBeGreaterThanOrEqual(surface)
        expect(surface, `${id}.css surface 应高于 secondary`).toBeGreaterThan(secondary)
        expect(secondary, `${id}.css secondary 应高于 primary`).toBeGreaterThan(primary)
        expect(primary, `${id}.css primary 应高于 inset`).toBeGreaterThan(inset)
      } else {
        expect(elevated, `${id}.css elevated 不得低于 surface`).toBeGreaterThanOrEqual(surface)
        expect(surface, `${id}.css surface 不得低于 secondary`).toBeGreaterThanOrEqual(secondary)
        expect(secondary, `${id}.css secondary 不得低于 primary`).toBeGreaterThanOrEqual(primary)
        expect(primary, `${id}.css primary 不得低于 inset`).toBeGreaterThanOrEqual(inset)
      }
    }
  })

  it('所有主题的画廊清屏色与画廊底色一致', () => {
    for (const [id, css] of cssById) {
      const props = Object.fromEntries(
        [...css.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(([, key, value]) => [
          key,
          value.trim(),
        ]),
      )
      expect(props['--color-bg-canvas-gap'], `${id}.css 缺少画廊清屏色`).toBe(
        props['--color-bg-canvas'],
      )
    }
  })

  it('每份主题声明 color-scheme 且与注册表 kind 一致', () => {
    for (const [id, css] of cssById) {
      const kind = getTheme(id)?.kind
      expect(kind, `注册表缺 ${id}`).toBeDefined()
      const m = css.match(/color-scheme:\s*(light|dark)/)
      expect(m, `${id}.css 缺 color-scheme 声明`).not.toBeNull()
      expect(m?.[1], `${id}.css 的 color-scheme 与注册表 kind 不符`).toBe(kind)
    }
  })

  it('注册表 preview 为合法 hex 色', () => {
    const hex = /^#[0-9a-fA-F]{6}$/
    for (const t of BUILTIN_THEMES) {
      for (const [slot, v] of Object.entries(t.preview)) {
        expect(v, `${t.id}.preview.${slot} 非法`).toMatch(hex)
      }
    }
  })

  it('风格卡的四色预览与实际主题同步', () => {
    const previewTokens = {
      bg: '--color-bg-primary',
      surface: '--color-bg-surface',
      text: '--color-text-primary',
      accent: '--color-accent',
    } as const
    for (const theme of BUILTIN_THEMES) {
      const props = themeProps(cssById.get(theme.id) ?? '')
      for (const key of Object.keys(previewTokens) as (keyof typeof previewTokens)[]) {
        // bg/surface 是浓度 mix 表达式:预览展示满浓度锚点色(主题身份色),浓度是运行时叠加。
        expect(theme.preview[key], `${theme.id}.preview.${key} 与 CSS 漂移`).toBe(
          anchorColorOf(props[previewTokens[key]]),
        )
      }
    }
  })

  it('亮暗槽位默认主题存在且 kind 正确', () => {
    expect(getTheme(DEFAULT_LIGHT_THEME)?.kind).toBe('light')
    expect(getTheme(DEFAULT_DARK_THEME)?.kind).toBe('dark')
  })
})
