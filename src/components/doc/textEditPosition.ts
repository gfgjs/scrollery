export interface TextLineIndex {
  source: string
  starts: number[]
}

/** 按 textarea 的 LF 规范化值建立 UTF-16 行首索引，末尾换行保留最后一空行。 */
export function indexTextLines(value: string): TextLineIndex {
  const source = value.replace(/\r\n?/g, '\n')
  const starts = [0]
  for (let offset = source.indexOf('\n'); offset >= 0; offset = source.indexOf('\n', offset + 1)) starts.push(offset + 1)
  return { source, starts }
}

/** 光标事件只二分缓存的行首；列与原生 selectionStart/End 一样按 UTF-16 计数。 */
export function textPositionAt(index: TextLineIndex, offset: number): { line: number; column: number } {
  const position = Math.max(0, Math.min(offset, index.source.length))
  let low = 0
  let high = index.starts.length
  while (low + 1 < high) {
    const middle = (low + high) >>> 1
    if (index.starts[middle]! <= position) low = middle
    else high = middle
  }
  return { line: low + 1, column: position - index.starts[low]! + 1 }
}
