// src/utils/markdown.ts
// 轻量 Markdown → HTML 渲染（§5.1）。零依赖，覆盖常见语法即可（标题/粗斜体/行内码/代码块/
// 列表/引用/链接/分割线/段落）。先做 HTML 转义再套用变换，避免 XSS。
// Lightweight Markdown → HTML (§5.1). Dependency-free; covers common syntax. HTML is escaped
// first, then markdown transforms applied, so raw input can't inject markup.

/**
 * HTML 转义。**必须含引号** —— 本模块的产物里有两处**属性**上下文（代码块的 class="language-…"
 * 与链接的 href="…"），只转义 `& < >` 挡不住「用引号提前闭合属性、再补一个事件处理器」。
 *
 * 2026-07-16 安全审查实证（此前只转 & < >）：
 *   输入围栏 ```a"onmouseover="alert(1)
 *   产出     <pre><code class="language-a"onmouseover="alert(1)">…
 * 即一个 .md 文件即可把事件属性注入到渲染它的 iframe 里。链接那处同理，且**协议白名单挡不住**
 * ——注入发生在合法协议之后（[x](https://a"onmouseover="…)）。
 * 文本上下文里多转引号无害：`&quot;` / `&#39;` 在文本节点里照常渲染为 " 与 '。
 * 用 &#39; 而非 &apos;：后者不在 HTML4 命名实体表内，老解析器不认。
 */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

// 代码块 HTML：带 `language-xxx` class（语法高亮插件位，R4；shiki 等高亮器就此 class 消费）。
// lang 为空（无语言围栏）则不加 class，回退纯代码块。
function codeBlockHtml(lang: string, code: string): string {
  const cls = lang ? ` class="language-${escapeHtml(lang)}"` : ''
  return `<pre><code${cls}>${escapeHtml(code)}</code></pre>`
}

// 行内：代码 `x` > 粗 **x** > 斜 *x* > 链接 [t](u)。先处理行内码占位，避免其内部再被转义破坏。
function renderInline(text: string): string {
  let out = escapeHtml(text)
  // 行内代码（先于其它，内部不再解析）
  out = out.replace(/`([^`]+)`/g, (_m, c) => `<code>${c}</code>`)
  // 粗体
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  // 斜体（避开已消费的 **）
  out = out.replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>')
  // 链接 [text](url) —— 仅允许 http(s)/相对，禁止 javascript: 等协议
  out = out.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_m, t, u) => {
    const safe = /^(https?:\/\/|\/|#)/i.test(u) ? u : '#'
    return `<a href="${safe}" target="_blank" rel="noopener noreferrer">${t}</a>`
  })
  return out
}

/**
 * 单 `<p>` 断长跑双上限(2026-07-17 内存爆炸修复)。
 * 无空行的日志型文本会把上万行聚进一个 `<p>`(`<br>` join)——单块在 foliate paginated 的
 * CSS multicol 下横跨数百列,是「整文档 rect 查询 × fragmentainer 数」乘积放大的最坏形态
 * (实证:6.6MB log.md 单 `<p>` 9k+ 行,renderer 7.9GB + 主线程阻塞分钟级,见
 * docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md 测量阶梯)。
 * 行数与字符**双上限**:只按行数切,长行日志(240 字符/行 × 200 行 ≈ 50K)仍会产出远超
 * section 预算(syntheticBook MD_SECTION_BUDGET_CHARS=24K)的单块,被迫独占超限片;
 * 6K 源字符 ≈ 数块拼一片,与预算对齐。对正常 markdown 段落双双无感。
 */
const MAX_PARA_LINES = 200
const MAX_PARA_CHARS = 6_000

/**
 * 渲染为**块级 HTML 数组**(一项 = 一个自包含块:`<p>`/`<pre>`/`<ul>`/`<h1>`…)。
 * 供 SyntheticBook 按块边界分片成多 section(阅读器 section 大小是硬约束,单 section 过大
 * 会触发 multicol 布局灾难);`renderMarkdown` = join,两者产物字节一致。
 */
export function renderMarkdownBlocks(src: string): string[] {
  const lines = src.replace(/\r\n?/g, '\n').split('\n')
  const html: string[] = []
  let inCode = false
  let codeLang = ''
  let codeBuf: string[] = []
  let listType: 'ul' | 'ol' | null = null
  let listBuf: string[] = []
  let paraBuf: string[] = []

  const flushPara = () => {
    if (paraBuf.length) {
      // 断长跑:行数/字符双上限切 `<p>`,防单块无界(见常量注释)。字符按源行长累计
      // (HTML 转义放大同量级,近似足够;单行超限自成一块,交 section 层护栏兜底)。
      let chunk: string[] = []
      let chars = 0
      const flushChunk = () => {
        if (chunk.length) {
          html.push(`<p>${chunk.map(renderInline).join('<br>')}</p>`)
          chunk = []
          chars = 0
        }
      }
      for (const line of paraBuf) {
        if (chunk.length && (chunk.length >= MAX_PARA_LINES || chars + line.length > MAX_PARA_CHARS)) {
          flushChunk()
        }
        chunk.push(line)
        chars += line.length
      }
      flushChunk()
      paraBuf = []
    }
  }
  // 列表整体收成**一个**块(项间 \n 连接,与旧「逐项 push 后统一 join」字节一致),
  // 使块数组的每一项都自包含 —— 分片时不会把 <ul> 与 </ul> 切进不同 section。
  const closeList = () => {
    if (listType) {
      html.push([`<${listType}>`, ...listBuf, `</${listType}>`].join('\n'))
      listBuf = []
      listType = null
    }
  }

  for (const raw of lines) {
    // 代码块围栏 ```（可带语言：```js / ```python）
    if (/^```/.test(raw.trim())) {
      if (inCode) {
        html.push(codeBlockHtml(codeLang, codeBuf.join('\n')))
        codeBuf = []
        inCode = false
        codeLang = ''
      } else {
        flushPara()
        closeList()
        inCode = true
        // 围栏后首个 token 为语言标识（语法高亮插件位，R4）；无则留空不高亮。
        codeLang = raw.trim().slice(3).trim().split(/\s+/)[0] ?? ''
      }
      continue
    }
    if (inCode) {
      codeBuf.push(raw)
      continue
    }

    const line = raw.trimEnd()

    // 空行：段落/列表分隔
    if (!line.trim()) {
      flushPara()
      closeList()
      continue
    }

    // 标题 # … ######
    const h = /^(#{1,6})\s+(.*)$/.exec(line)
    if (h) {
      flushPara()
      closeList()
      const level = h[1].length
      html.push(`<h${level}>${renderInline(h[2])}</h${level}>`)
      continue
    }

    // 分割线
    if (/^(\*\*\*|---|___)\s*$/.test(line)) {
      flushPara()
      closeList()
      html.push('<hr>')
      continue
    }

    // 引用 >
    if (/^>\s?/.test(line)) {
      flushPara()
      closeList()
      html.push(`<blockquote>${renderInline(line.replace(/^>\s?/, ''))}</blockquote>`)
      continue
    }

    // 无序列表 - * +
    const ul = /^[-*+]\s+(.*)$/.exec(line)
    if (ul) {
      flushPara()
      if (listType !== 'ul') {
        closeList()
        listType = 'ul'
      }
      listBuf.push(`<li>${renderInline(ul[1])}</li>`)
      continue
    }

    // 有序列表 1. 2. …
    const ol = /^\d+\.\s+(.*)$/.exec(line)
    if (ol) {
      flushPara()
      if (listType !== 'ol') {
        closeList()
        listType = 'ol'
      }
      listBuf.push(`<li>${renderInline(ol[1])}</li>`)
      continue
    }

    // 普通段落行
    closeList()
    paraBuf.push(line)
  }

  if (inCode) html.push(codeBlockHtml(codeLang, codeBuf.join('\n')))
  flushPara()
  closeList()
  return html
}

export function renderMarkdown(src: string): string {
  return renderMarkdownBlocks(src).join('\n')
}

/// 纯文本 → 安全的预格式化 HTML（保留空白）。
export function renderPlainText(src: string): string {
  return `<pre class="doc-plain">${escapeHtml(src.replace(/\r\n?/g, '\n'))}</pre>`
}
