interface MarkdownCodeCopyOptions {
  label: string
  onResult: (success: boolean) => void
}

/** 给已加载的 Markdown 章节追加复制装饰；不移动正文节点或向搜索/CFI 添加文字。 */
export function decorateMarkdownCodeCopy(doc: Document, options: MarkdownCodeCopyOptions): void {
  for (const code of doc.querySelectorAll<HTMLElement>('.sate-md pre > code')) {
    const pre = code.parentElement!
    const button = doc.createElement('button')
    button.type = 'button'
    button.className = 'sate-md-code-copy'
    button.setAttribute('aria-label', options.label)
    button.title = options.label
    const svg = doc.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('viewBox', '0 0 24 24')
    svg.setAttribute('aria-hidden', 'true')
    svg.setAttribute('fill', 'none')
    svg.setAttribute('stroke', 'currentColor')
    svg.setAttribute('stroke-width', '2')
    const path = doc.createElementNS(svg.namespaceURI, 'path')
    path.setAttribute('d', 'M9 9h12v12H9z M5 15H3V3h12v2')
    svg.append(path)
    button.append(svg)
    button.addEventListener('click', async () => {
      try {
        // 只读取 code，保留 tab、尾随空白、空行及换行，不带语言标签或周围正文。
        await navigator.clipboard.writeText(code.textContent ?? '')
        if (button.isConnected) options.onResult(true)
      } catch {
        if (button.isConnected) options.onResult(false)
      }
    })
    pre.classList.add('sate-md-code-container')
    pre.append(button)
  }
  // 必须先于键盘桥安装：Space/Enter 保留原生按钮激活，同时不被桥转成阅读翻页。
  const keepNativeActivation = (event: KeyboardEvent) => {
    const target = event.target as Element | null
    if ((event.key === ' ' || event.key === 'Enter') && target?.closest('.sate-md-code-copy')) {
      event.stopImmediatePropagation()
    }
  }
  doc.addEventListener('keydown', keepNativeActivation, true)
  doc.addEventListener('keyup', keepNativeActivation, true)
}
