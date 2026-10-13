/** 把原文按 LF 计数的行首映射到 textarea 规范化换行后的 UTF-16 偏移。 */
export function sourceLineOffset(source: string, line: number): number | null {
  if (!Number.isSafeInteger(line) || line < 0) return null
  let offset = 0
  for (let current = 0; current < line; current++) {
    const newline = source.indexOf('\n', offset)
    if (newline < 0) return null
    offset = newline + 1
  }
  // 单个 CR 在 textarea 中仍占一个码元，只有 CRLF 会从两个码元缩成一个。
  return source.slice(0, offset).replace(/\r\n/g, '\n').length
}

/** 计算解码原文快照摘要；不可用时跳过阅读位置映射，仍允许阅读编辑。 */
export async function hashTextSource(source: string): Promise<string | null> {
  try {
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(source))
    return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('')
  } catch {
    return null
  }
}

/** 核对阅读章与编辑缓存是否来自同一份文本；不会为过期映射定位光标。 */
export async function matchesTextSourceHash(source: string, expected: string): Promise<boolean> {
  return /^[a-f0-9]{64}$/.test(expected) && await hashTextSource(source) === expected
}
