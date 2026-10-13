export interface SourceIndentResult {
  source: string
  start: number
  end: number
  edits: { position: number; remove: number; insert: string }[]
}

/**
 * 源码使用两空格缩进；选区按整行处理，反缩进只移除共有空白前缀。
 * start/end 为正文内有序的 UTF-16 选区偏移；返回新正文、选区及按原文位置递增的空白编辑段。
 */
export function indentSource(source: string, start: number, end: number, outdent: boolean): SourceIndentResult {
  const indent = '  '
  if (!outdent && start === end) {
    return { source: source.slice(0, start) + indent + source.slice(end), start: start + 2, end: end + 2, edits: [{ position: start, remove: 0, insert: indent }] }
  }
  const lineStarts = [start === 0 ? 0 : source.lastIndexOf('\n', start - 1) + 1]
  let newline = source.indexOf('\n', lineStarts[0])
  // 选区恰好结束在下一行开头时，不把未选中的那一行算进来。
  while (newline !== -1 && newline + 1 < end) {
    lineStarts.push(newline + 1)
    newline = source.indexOf('\n', newline + 1)
  }
  let prefix = indent
  if (outdent) {
    const lines = lineStarts.map(position => {
      const endOfLine = source.indexOf('\n', position)
      return source.slice(position, endOfLine === -1 ? source.length : endOfLine)
    })
    const nonempty = lines.filter(line => line.trim().length > 0)
    const prefixes = (nonempty.length ? nonempty : lines).map(line => line.match(/^(?:\t| {1,2})/)?.[0] ?? '')
    prefix = prefixes[0]!
    for (const next of prefixes.slice(1)) {
      while (prefix && !next.startsWith(prefix)) prefix = prefix.slice(0, -1)
    }
    if (!prefix) return { source, start, end, edits: [] }
  }
  const edits = lineStarts.map(position => ({
    position,
    remove: outdent && source.startsWith(prefix, position) ? prefix.length : 0,
    insert: outdent ? '' : indent,
  }))
  const chunks: string[] = []
  let previous = 0
  for (const edit of edits) {
    chunks.push(source.slice(previous, edit.position), edit.insert)
    previous = edit.position + edit.remove
  }
  chunks.push(source.slice(previous))
  const mapPosition = (position: number): number => {
    let delta = 0
    for (const edit of edits) {
      if (position < edit.position) break
      if (position <= edit.position + edit.remove) return edit.position + delta + edit.insert.length
      delta += edit.insert.length - edit.remove
    }
    return position + delta
  }
  return { source: chunks.join(''), start: mapPosition(start), end: mapPosition(end), edits }
}
