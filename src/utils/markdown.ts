import { Lexer, type MarkedToken, type Token, type Tokens } from 'marked'
import { decodeHTMLStrict } from 'entities/decode'
import { getDocumentResourceBaseUrl } from './documentResourceUrl'
import { parseMarkdownTable } from './markdownTable'
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

function safeResourceUrl(destination: string): boolean {
  if (!destination.trim()) return false
  try {
    return ['http:', 'https:', 'asset:'].includes(new URL(destination, 'https://scrollery.invalid/').protocol)
  } catch { return false }
}

function renderInlineTokens(tokens: Token[], resourceBase?: string, breakLines = false): string {
  return tokens.map(rawToken => {
    // 仅使用内建 Lexer，未注册自定义 token；未知块仍按字面文本显示。
    const token = rawToken as MarkedToken
    switch (token.type) {
      case 'strong': case 'em': case 'del':
        return `<${token.type}>${renderInlineTokens(token.tokens, resourceBase, breakLines)}</${token.type}>`
      case 'codespan': return `<code>${escapeHtml(token.text)}</code>`
      case 'br': return '<br>'
      case 'link': {
        const label = renderInlineTokens(token.tokens, resourceBase, breakLines)
        const href = token.href.trim()
        if (!href.trim() || (/^[a-z][a-z\d+.-]*:/i.test(href) && !/^https?:\/\//i.test(href)) || /^[\/\\?]/.test(href)) {
          return `<a aria-disabled="true">${label}</a>`
        }
        // 相对文档链接保留原始引用，由宿主授权解析到库内身份，不能当合成书章节号。
        if (!href.startsWith('#') && !/^https?:\/\//i.test(href)) {
          return `<a href="${escapeHtml(href)}" data-document-link="">${label}</a>`
        }
        return `<a href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer">${label}</a>`
      }
      case 'image': {
        // Vditor 粘贴位图使用内嵌 data URL；只允许位图 base64，链接和其他 data 内容仍不开放。
        const embeddedBitmap = /^data:image\/(?:png|jpeg|gif|webp|bmp|avif);base64,[a-z0-9+/]+={0,2}$/i.test(token.href)
        if (!embeddedBitmap && !safeResourceUrl(token.href)) return escapeHtml(token.text)
        const src = resourceBase ? new URL(token.href, resourceBase).href : token.href
        const title = token.title ? ` title="${escapeHtml(token.title)}"` : ''
        return `<img src="${escapeHtml(src)}" alt="${escapeHtml(token.text)}"${title}>`
      }
      case 'text': case 'escape': {
        // 普通 Markdown 正文的分号实体只解码一次，再统一转义；代码和反斜线转义仍按字面显示。
        const text = escapeHtml(token.type === 'text' ? decodeHTMLStrict(token.text) : token.text)
        return breakLines ? text.replace(/\n/g, '<br>') : text
      }
      default: return escapeHtml(token.raw)
    }
  }).join('')
}

const MAX_PARA_LINES = 200
const MAX_PARA_CHARS = 6000

/**
 * Markdown 先整体块词法分析，令引用定义、容器和围栏共享同一文档上下文。
 * HTML 仅由白名单生成；段落在行边界按 200 行/6K 源字符切块，供既有 24K section 分组。
 * 单行和不可拆容器仍整体输出，超大 section 沿用阅读器的 256K 强制滚动护栏。
 */
export function renderMarkdownBlocks(src: string, options?: {
  documentUrl?: string
  /** 可选输出：各 HTML 块在 LF 规范化源码中的 UTF-16 起点，与返回块数组一一对应。 */
  sourceOffsets?: number[]
}): string[] {
  const source = src.replace(/\r\n?/g, '\n')
  const sourceOffsets = options?.sourceOffsets
  if (sourceOffsets) sourceOffsets.length = 0
  const tokenOffsets = new Map<Token, number>()
  let recordedTokens = 0, pendingOffset = 0
  const recordTokens = () => {
    for (; recordedTokens < lexer.tokens.length; recordedTokens++) {
      tokenOffsets.set(lexer.tokens[recordedTokens], pendingOffset)
    }
  }
  const lexer = new Lexer({
    gfm: true,
    ...(sourceOffsets ? { extensions: { renderers: {}, childTokens: {}, block: [(remaining, currentTokens) => {
      if (currentTokens === lexer.tokens) {
        // 上轮 token 已产出；无 token 的重复定义也会消耗原文，不能只累计 token.raw。
        recordTokens()
        pendingOffset = source.length - remaining.length
      }
      return undefined
    }] } } : {}),
  })
  const tokens = lexer.blockTokens(source, lexer.tokens)
  if (sourceOffsets) recordTokens()
  // 避免先对巨段整段分析行内语法，再在安全分块后重复解析；定义表已由块词法器收齐。
  lexer.inlineQueue = []
  const base = options?.documentUrl ? getDocumentResourceBaseUrl(options.documentUrl) : undefined
  const inline = (text: string, breakLines = false) => renderInlineTokens(lexer.inlineTokens(text), base, breakLines)
  const paragraph = (token: Tokens.Paragraph | Tokens.Text, wrap = true, prefix = '', breakLines = true, sourceStart?: number): string[] => {
    const lines = token.text.split('\n')
    if (lines.length <= MAX_PARA_LINES && token.text.length <= MAX_PARA_CHARS) {
      const content = prefix + renderInlineTokens(token.tokens?.length ? token.tokens : lexer.inlineTokens(token.text), base, breakLines)
      if (sourceStart !== undefined) sourceOffsets?.push(sourceStart)
      return [wrap ? `<p>${content}</p>` : content]
    }
    const chunks: string[] = []
    let buf: string[] = [], chars = 0, firstLine = 0
    const rawLineOffsets = [0]
    if (sourceStart !== undefined) {
      for (let i = 0; i < token.raw.length; i++) if (token.raw[i] === '\n') rawLineOffsets.push(i + 1)
    }
    const flush = () => {
      if (!buf.length) return
      const content = prefix + inline(buf.join('\n'), breakLines)
      prefix = ''
      chunks.push(wrap ? `<p>${content}</p>` : content)
      // 行内语法和缩进可能改变显示长度；分块起点只沿 token 原始行界推进。
      if (sourceStart !== undefined) sourceOffsets?.push(sourceStart + rawLineOffsets[firstLine])
      buf = []; chars = 0
    }
    for (const [lineIndex, line] of lines.entries()) {
      if (buf.length && (buf.length >= MAX_PARA_LINES || chars + line.length > MAX_PARA_CHARS)) flush()
      if (!buf.length) firstLine = lineIndex
      buf.push(line); chars += line.length
    }
    flush()
    return chunks
  }
  const renderList = (list: Tokens.List): string => {
    const tag = list.ordered ? 'ol' : 'ul'
    return `<${tag}${list.ordered && list.start !== 1 ? ` start="${list.start}"` : ''}>` + list.items.map(item => {
      let prefix = item.task ? `<input type="checkbox" disabled${item.checked ? ' checked' : ''}> ` : ''
      const content = item.tokens.flatMap(raw => {
        const token = raw as MarkedToken
        if (token.type === 'checkbox' || token.type === 'space') return []
        if (token.type === 'text' || token.type === 'paragraph') {
          const chunks = paragraph(token, list.loose || token.type === 'paragraph', prefix, false)
          prefix = ''
          return chunks
        }
        const html = prefix + block(token).join('\n')
        prefix = ''
        return [html]
      }).join('')
      return `<li${item.task ? ' class="sate-md-task-item"' : ''}>${content}${prefix}</li>`
    }).join('') + `</${tag}>`
  }
  const block = (token: MarkedToken): string[] => {
    switch (token.type) {
      case 'space': case 'def': return []
      case 'heading': return [`<h${token.depth}>${renderInlineTokens(lexer.inlineTokens(token.text), base)}</h${token.depth}>`]
      case 'paragraph': case 'text': return paragraph(token)
      case 'hr': return ['<hr>']
      case 'code': return [codeBlockHtml((token.lang ?? '').split(/\s+/)[0], token.text)]
      case 'blockquote': return [`<blockquote>${token.tokens.flatMap(child => block(child as MarkedToken)).join('\n')}</blockquote>`]
      case 'list': return [renderList(token)]
      case 'table': {
        // 已识别表格仍保留代码内容中的竖线；直接消费 GFM cells 会把未转义的代码竖线拆列。
        const result = parseMarkdownTable(token.raw.split('\n'), 0, inline)
        return result ? [result.html] : [`<p>${escapeHtml(token.raw).replace(/\n/g, '<br>')}</p>`]
      }
      default: return [`<p>${escapeHtml(token.raw).replace(/\n/g, '<br>')}</p>`]
    }
  }
  return tokens.flatMap(rawToken => {
    const token = rawToken as MarkedToken
    const offset = tokenOffsets.get(token)
    if (sourceOffsets && (token.type === 'paragraph' || token.type === 'text')) {
      return paragraph(token, true, '', true, offset)
    }
    const rendered = block(token)
    if (sourceOffsets) for (const _html of rendered) sourceOffsets.push(offset ?? -1)
    return rendered
  })
}

/** 渲染安全 Markdown HTML；按块分片的阅读器使用 renderMarkdownBlocks。 */
export function renderMarkdown(src: string): string {
  return renderMarkdownBlocks(src).join('\n')
}

/** 纯文本保留空白，并转义所有 HTML 和属性特殊字符。 */
export function renderPlainText(src: string): string {
  return `<pre class="doc-plain">${escapeHtml(src.replace(/\r\n?/g, '\n'))}</pre>`
}
