/** Markdown 表格块及其后首个未消费行。 */
export interface MarkdownTableBlock {
  html: string
  nextIndex: number
}

function splitRow(source: string): { cells: string[]; hasPipe: boolean } {
  const line = source.trim()
  const cells: string[] = []
  let cell = ''
  let hasPipe = false
  let endedWithPipe = false
  // 先记录等长反引号的下一个位置：无配对的反引号不能吞掉后续列分隔符。
  const runs = [...line.matchAll(/`+/g)]
  const nextRun = new Map<number, number>()
  const lastByLength = new Map<number, number>()
  for (let index = runs.length - 1; index >= 0; index--) {
    const run = runs[index]
    const next = lastByLength.get(run[0].length)
    if (next !== undefined) nextRun.set(run.index, next)
    lastByLength.set(run[0].length, run.index)
  }
  for (let index = 0; index < line.length; index++) {
    const char = line[index]
    endedWithPipe = false
    if (char === '\\' && index + 1 < line.length) {
      const next = line[++index]
      cell += next === '|' ? '|' : '\\' + next
    } else if (char === '`' && nextRun.has(index)) {
      const end = nextRun.get(index)!
      let length = 1
      while (line[index + length] === '`') length++
      cell += line.slice(index, end + length).replace(/\\\|/g, '|')
      index = end + length - 1
    } else if (char === '|') {
      hasPipe = true
      endedWithPipe = true
      cells.push(cell.trim())
      cell = ''
    } else cell += char
  }
  cells.push(cell.trim())
  if (line.startsWith('|')) cells.shift()
  if (endedWithPipe) cells.pop()
  return { cells, hasPipe }
}

// 表格在空行或下一块结构开始处终止；正文缺列/多列遵循 GFM 的补空/忽略规则。
function startsBlock(line: string): boolean {
  return /^\s*(?:#{1,6}\s|>|[-+*]\s|\d+[.)]\s|`{3,}|~{3,})/.test(line)
    || /^\s*(?:\*\*\*|---|___)\s*$/.test(line)
}

/**
 * 从 start 表头行尝试解析常见管道表格，返回首个未消费行；未匹配时返回 null。
 * 单元格原文统一交给调用方的安全行内渲染，不在表格层插入用户 HTML。
 */
export function parseMarkdownTable(
  lines: string[],
  start: number,
  renderInline: (text: string) => string,
): MarkdownTableBlock | null {
  if (start + 1 >= lines.length) return null
  if (!lines[start].includes('|') && !lines[start + 1].includes('|')) return null
  const header = splitRow(lines[start])
  const delimiter = splitRow(lines[start + 1])
  if ((!header.hasPipe && !delimiter.hasPipe) || !header.cells.length
    || header.cells.length !== delimiter.cells.length
    || !delimiter.cells.every(cell => /^:?-+:?$/.test(cell))) return null

  const alignment = delimiter.cells.map(cell => cell.startsWith(':')
    ? (cell.endsWith(':') ? 'center' : 'left')
    : (cell.endsWith(':') ? 'right' : ''))
  const renderRow = (cells: string[], tag: 'th' | 'td') => '<tr>' + alignment.map((align, index) =>
    `<${tag}${align ? ` align="${align}"` : ''}>${renderInline(cells[index] ?? '')}</${tag}>`,
  ).join('') + '</tr>'
  const head = renderRow(header.cells, 'th')
  const rows: string[] = []
  let nextIndex = start + 2
  while (nextIndex < lines.length && lines[nextIndex].trim() && !startsBlock(lines[nextIndex])) {
    rows.push(renderRow(splitRow(lines[nextIndex]).cells, 'td'))
    nextIndex++
  }
  return {
    html: `<div class="sate-md-table"><table><thead>${head}</thead>${rows.length ? `<tbody>${rows.join('')}</tbody>` : ''}</table></div>`,
    nextIndex,
  }
}
