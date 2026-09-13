// src/utils/srtToVtt.ts
// SRT → WebVTT 纯函数转换工具

/**
 * 判断内容是否已经是 WebVTT 格式(首个非空行以 WEBVTT 开头)。
 * 调用方据此决定是否跳过转换、直接使用原文。
 */
export function looksLikeVtt(content: string): boolean {
  const lines = content.split(/\r\n|\r|\n/)
  for (const line of lines) {
    const trimmed = line.trim()
    if (trimmed === '') continue
    return trimmed.startsWith('WEBVTT')
  }
  return false
}

// SRT 时间戳:00:01:02,345
const SRT_TIME_RE = /^(\d{1,2}):(\d{2}):(\d{2})[.,](\d{1,3})$/
// SRT cue 时间行:"00:00:01,000 --> 00:00:04,000" 允许行内附加定位信息
const SRT_ARROW_LINE_RE = /^(\S+)\s*-->\s*(\S+)(.*)$/
// cue 编号行:纯数字
const CUE_NUMBER_RE = /^\d+$/

// srt 常见格式标签白名单(保留),其余尖括号一律转义防注入
// 正则同时匹配纯标签和带属性的标签,后续在转义前剥除属性
// (VTT cue 中白名单标签不需要也不该携带 HTML 属性)
const ALLOWED_TAG_RE = /<(\/?)(b|i|u)(\s[^>]*)?>/gi

/**
 * 将单条 srt 时间戳(逗号或点分隔毫秒)标准化为 WebVTT 要求的点分隔、
 * 且小时至少两位、毫秒补齐三位。非法输入返回 null。
 */
function normalizeTimestamp(raw: string): string | null {
  const m = SRT_TIME_RE.exec(raw.trim())
  if (!m) return null
  const [, h, mm, ss, ms] = m
  const hours = h.padStart(2, '0')
  const millis = ms.padEnd(3, '0').slice(0, 3)
  return `${hours}:${mm}:${ss}.${millis}`
}

/**
 * 转义裸露的 `<` / `>`,但保留 srt 常见的 `<b>` `<i>` `<u>` 标签(含闭合)。
 * 防止字幕文本中夹带任意 HTML/脚本注入。
 */
function escapeTextKeepAllowedTags(text: string): string {
  const placeholders: string[] = []
  const withPlaceholders = text.replace(ALLOWED_TAG_RE, (_, slash: string, tagName: string) => {
    // 归一化白名单标签:剥除可能存在的属性,转换为小写纯标签
    const normalized = `<${slash}${tagName.toLowerCase()}>`
    placeholders.push(normalized)
    return ` ${placeholders.length - 1} `
  })
  const escaped = withPlaceholders.replace(/</g, '&lt;').replace(/>/g, '&gt;')
  return escaped.replace(/ (\d+) /g, (_, idx: string) => placeholders[Number(idx)])
}

/**
 * 将 SRT 字幕文本转换为 WebVTT 文本。
 * 容错策略:格式坏的 cue 直接跳过,不抛异常;CRLF/LF 均可处理;
 * cue 编号行被剥离(WebVTT 不需要);保留 `<b>` `<i>` `<u>` 标签,
 * 其余尖括号转义防止注入。
 */
export function srtToVtt(content: string): string {
  const normalized = content.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  const blocks = normalized.split(/\n\s*\n/)
  const cues: string[] = []

  for (const block of blocks) {
    const lines = block.split('\n').filter((l) => l.trim() !== '')
    if (lines.length === 0) continue

    let idx = 0
    // 跳过 cue 编号行(如果存在)
    if (CUE_NUMBER_RE.test(lines[idx].trim())) {
      idx += 1
    }
    if (idx >= lines.length) continue

    const arrowMatch = SRT_ARROW_LINE_RE.exec(lines[idx].trim())
    if (!arrowMatch) continue // 坏 cue:没有时间行,跳过
    const [, startRaw, endRaw, rest] = arrowMatch
    const start = normalizeTimestamp(startRaw)
    const end = normalizeTimestamp(endRaw)
    if (!start || !end) continue // 坏 cue:时间戳格式非法,跳过
    idx += 1

    const textLines = lines.slice(idx)
    if (textLines.length === 0) continue // 坏 cue:没有文本内容,跳过

    const escapedText = textLines.map((l) => escapeTextKeepAllowedTags(l)).join('\n')
    cues.push(`${start} --> ${end}${rest.trimEnd()}\n${escapedText}`)
  }

  return `WEBVTT\n\n${cues.join('\n\n')}${cues.length > 0 ? '\n' : ''}`
}
