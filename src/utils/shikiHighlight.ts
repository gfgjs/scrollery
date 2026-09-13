// src/utils/shikiHighlight.ts
// md 代码块语法高亮（阅读器方案 R4，§7-R4「md 插件位（shiki 懒载）」）。
//
// 关键约束（用户 2026-07-07 定方向）：**必须用 JS 正则引擎**（createJavaScriptRegexEngine），
// 不用 shiki 默认的 WASM（oniguruma）引擎——WASM 需 CSP `wasm-unsafe-eval`，而生产 CSP 已硬化
// 删除 `unsafe-eval`（422a806）。JS 引擎把 TextMate 语法编译为原生 RegExp，无 eval/wasm → CSP 安全。
//
// 全部经**动态 import 懒加载**：shiki core / 引擎 / 每个语法 / 主题都是独立 chunk，仅在打开含代码块的
// md 时按需拉取，不进主包（守「核心 <10MB」定位）。未收录的语言回退纯文本（不盲目膨胀语言集）。

import type { HighlighterCore } from 'shiki/core'

// isDarkColor 已迁 utils/color（颜色数学单一归宿，R3）；此处 re-export 保持既有 import 路径与单测不变。
export { isDarkColor } from './color'

/** fence 语言 → { canonical 名, 语法模块加载器 }。静态 import 路径便于 Vite 按语言切独立懒加载 chunk。 */
interface LangEntry {
  name: string
  load: () => Promise<{ default: unknown }>
}

// 精选常用集 + 常见别名。key = fence 里写的语言（小写），name = shiki canonical 名（codeToHtml 用）。
const LANGS: Record<string, LangEntry> = {
  js: { name: 'javascript', load: () => import('shiki/langs/javascript.mjs') },
  javascript: { name: 'javascript', load: () => import('shiki/langs/javascript.mjs') },
  mjs: { name: 'javascript', load: () => import('shiki/langs/javascript.mjs') },
  jsx: { name: 'jsx', load: () => import('shiki/langs/jsx.mjs') },
  ts: { name: 'typescript', load: () => import('shiki/langs/typescript.mjs') },
  typescript: { name: 'typescript', load: () => import('shiki/langs/typescript.mjs') },
  tsx: { name: 'tsx', load: () => import('shiki/langs/tsx.mjs') },
  vue: { name: 'vue', load: () => import('shiki/langs/vue.mjs') },
  json: { name: 'json', load: () => import('shiki/langs/json.mjs') },
  html: { name: 'html', load: () => import('shiki/langs/html.mjs') },
  xml: { name: 'xml', load: () => import('shiki/langs/xml.mjs') },
  css: { name: 'css', load: () => import('shiki/langs/css.mjs') },
  bash: { name: 'bash', load: () => import('shiki/langs/bash.mjs') },
  sh: { name: 'bash', load: () => import('shiki/langs/bash.mjs') },
  shell: { name: 'bash', load: () => import('shiki/langs/bash.mjs') },
  python: { name: 'python', load: () => import('shiki/langs/python.mjs') },
  py: { name: 'python', load: () => import('shiki/langs/python.mjs') },
  rust: { name: 'rust', load: () => import('shiki/langs/rust.mjs') },
  rs: { name: 'rust', load: () => import('shiki/langs/rust.mjs') },
  go: { name: 'go', load: () => import('shiki/langs/go.mjs') },
  java: { name: 'java', load: () => import('shiki/langs/java.mjs') },
  c: { name: 'c', load: () => import('shiki/langs/c.mjs') },
  cpp: { name: 'cpp', load: () => import('shiki/langs/cpp.mjs') },
  sql: { name: 'sql', load: () => import('shiki/langs/sql.mjs') },
  yaml: { name: 'yaml', load: () => import('shiki/langs/yaml.mjs') },
  yml: { name: 'yaml', load: () => import('shiki/langs/yaml.mjs') },
  toml: { name: 'toml', load: () => import('shiki/langs/toml.mjs') },
  markdown: { name: 'markdown', load: () => import('shiki/langs/markdown.mjs') },
  md: { name: 'markdown', load: () => import('shiki/langs/markdown.mjs') },
}

const LIGHT_THEME = 'github-light'
const DARK_THEME = 'github-dark'

/**
 * 单块代码高亮尺寸上限(2026-07-17 内存爆炸修复)。TextMate 语法逐行正则扫描,多 MB 围栏
 * (如整份日志包进 ```)会把 CPU/内存烧在无人细读的高亮上;超限回退纯文本 `<pre><code>`,
 * 阅读不中断。100K ≈ 数千行代码,正常 md 远达不到。
 */
const MAX_HIGHLIGHT_CHARS = 100_000

// 懒建的高亮器单例 + 已加载语言集（按 canonical 名去重，避免重复 loadLanguage）。
let coreP: Promise<HighlighterCore> | null = null
const loadedLangs = new Set<string>()

async function getCore(): Promise<HighlighterCore> {
  if (!coreP) {
    coreP = (async () => {
      const [{ createHighlighterCore }, { createJavaScriptRegexEngine }] = await Promise.all([
        import('shiki/core'),
        import('shiki/engine/javascript'),
      ])
      return createHighlighterCore({
        // 双主题预载：codeToHtml 按明暗即时选择，无需重建高亮器。
        themes: [import('shiki/themes/github-light.mjs'), import('shiki/themes/github-dark.mjs')],
        langs: [], // 语言按需 loadLanguage（见 highlightCode）。
        engine: createJavaScriptRegexEngine(), // CSP 安全：JS 正则引擎，非 WASM。
      })
    })()
  }
  return coreP
}

/**
 * 高亮一段代码 → shiki 的 `<pre class="shiki">…` HTML。未收录语言 / 加载 / 高亮失败 → 返回 null
 * （调用方保留原始 `<pre><code>`，阅读不中断）。`isDark` 决定 github-dark / github-light。
 */
export async function highlightCode(
  code: string,
  lang: string,
  isDark: boolean,
): Promise<string | null> {
  const entry = LANGS[lang.trim().toLowerCase()]
  if (!entry) return null
  if (code.length > MAX_HIGHLIGHT_CHARS) return null // 超限回退纯文本(见常量注释)
  try {
    const core = await getCore()
    if (!loadedLangs.has(entry.name)) {
      const mod = await entry.load()
      await core.loadLanguage(mod.default as Parameters<HighlighterCore['loadLanguage']>[0])
      loadedLangs.add(entry.name)
    }
    return core.codeToHtml(code, {
      lang: entry.name,
      theme: isDark ? DARK_THEME : LIGHT_THEME,
    })
  } catch {
    return null // 高亮失败不阻断阅读。
  }
}

/**
 * 对 renderMarkdown 产出的 HTML 就地高亮所有 `<pre><code class="language-xxx">` 代码块。
 * 用 DOMParser 解析（textContent 天然反转义得原始代码）→ 逐块交 shiki → 用其输出替换整个 `<pre>`。
 * 任一块未收录 / 失败则保留原样。无代码块时快速短路（连 shiki chunk 都不拉）。浏览器专用（DOMParser）。
 */
export async function highlightMarkdownHtml(html: string, isDark: boolean): Promise<string> {
  if (!html.includes('<pre><code')) return html
  const doc = new DOMParser().parseFromString(html, 'text/html')
  const codeEls = Array.from(doc.querySelectorAll('pre > code[class*="language-"]'))
  if (!codeEls.length) return html

  let changed = false
  for (const codeEl of codeEls) {
    const lang = /language-(\S+)/.exec(codeEl.getAttribute('class') ?? '')?.[1] ?? ''
    const highlighted = await highlightCode(codeEl.textContent ?? '', lang, isDark)
    if (!highlighted) continue
    const tpl = doc.createElement('template')
    tpl.innerHTML = highlighted
    const shikiPre = tpl.content.firstElementChild
    const oldPre = codeEl.parentElement
    if (shikiPre && oldPre?.parentNode) {
      oldPre.parentNode.replaceChild(shikiPre, oldPre)
      changed = true
    }
  }
  return changed ? doc.body.innerHTML : html
}
