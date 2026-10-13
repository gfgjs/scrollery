const envelopePrefix = 'scrollery-positions:'

/** 同一文档的阅读器位置互相独立，原生 CFI 不能用于 Vditor 滚动恢复。 */
export interface ReaderPositions {
  native: string | null
  vditor: string | null
}

/** 仅接受 Vditor 有限的 0..1 十进制滚动比例。 */
export function parseVditorPosition(position: string | null | undefined): number | null {
  if (!position || !/^vditor:(?:0(?:\.\d+)?|1(?:\.0+)?)$/.test(position)) return null
  const fraction = Number(position.slice('vditor:'.length))
  return Number.isFinite(fraction) && fraction >= 0 && fraction <= 1 ? fraction : null
}

/** 将可滚动范围的比例保存至百万分之一，避免浮点尾差触发重复写入。 */
export function formatVditorPosition(fraction: number): string | null {
  if (!Number.isFinite(fraction) || fraction < 0 || fraction > 1) return null
  return `vditor:${Number(fraction.toFixed(6))}`
}

/** 兼容旧原生定位字符串；损坏或未知版本的容器不得传给原生阅读器。 */
export function decodeReaderPositions(raw: string | null): ReaderPositions {
  if (!raw?.startsWith(envelopePrefix)) return { native: raw, vditor: null }
  try {
    const value: unknown = JSON.parse(raw.slice(envelopePrefix.length))
    if (typeof value === 'object' && value !== null && 'version' in value && value.version === 1
      && 'native' in value && (value.native === null || typeof value.native === 'string')
      && 'vditor' in value && (value.vditor === null || (typeof value.vditor === 'string' && parseVditorPosition(value.vditor) !== null))) {
      return { native: value.native, vditor: value.vditor }
    }
  } catch { /* 无法解析的容器不作为原生位置兜底，避免两个引擎互相污染。 */ }
  return { native: null, vditor: null }
}

/** 只有 Vditor 使用位置时才升级为容器，保留原生文档原有的存储格式。 */
export function encodeReaderPositions(positions: ReaderPositions): string {
  return positions.vditor === null
    ? positions.native ?? ''
    : envelopePrefix + JSON.stringify({ version: 1, ...positions })
}
