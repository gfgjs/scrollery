// src/utils/format.ts
// 前端格式化工具

import i18n from '../i18n'

/** 格式化字节数为人类可读的字符串（例如 "4.2 MB"）。 */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

// formatDate(仅日期)死导出已删(2026-07-10 审查 B15):全库零消费者;需要时从 formatDateTime 派生。

/**
 * 将 Unix 时间戳格式化为日期 + 时间,跟随当前界面语言。
 *
 * locale 取自 i18n 而非写死 'zh-CN'(2026-07-10 审查 B15):EXIF 信息面板等消费点在
 * en-US 界面下曾渲染出「2026年7月10日」中文日期,违反「en-US 全界面无中文」验收口径。
 */
export function formatDateTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString(i18n.global.locale.value, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/** 将毫秒持续时间格式化为 "mm:ss" 或 "h:mm:ss"。 */
export function formatDuration(ms: number): string {
  const totalSec = Math.floor(ms / 1000)
  const hours = Math.floor(totalSec / 3600)
  const mins = Math.floor((totalSec % 3600) / 60)
  const secs = totalSec % 60

  const pad = (n: number) => String(n).padStart(2, '0')

  if (hours > 0) return `${hours}:${pad(mins)}:${pad(secs)}`
  return `${mins}:${pad(secs)}`
}

/** 将光圈值格式化为 "f/1.8" 样式。 */
export function formatAperture(aperture: number): string {
  return `f/${aperture.toFixed(1)}`
}

/** 将焦距格式化为 "35mm" 样式。 */
export function formatFocalLength(mm: number): string {
  return `${mm.toFixed(0)}mm`
}

/** 将 GPS 坐标格式化为 "40.7128° N, 74.0060° W"。 */
export function formatGps(lat: number, lng: number): string {
  const latDir = lat >= 0 ? 'N' : 'S'
  const lngDir = lng >= 0 ? 'E' : 'W'
  return `${Math.abs(lat).toFixed(4)}° ${latDir}, ${Math.abs(lng).toFixed(4)}° ${lngDir}`
}

/**
 * 将秒数格式化为播放器时间读数 "m:ss" 或 "h:mm:ss"。NaN/非有限值 → "--:--"。
 * opts.negative 为 true 时前缀 "-"(用于"剩余时间"展示,输入应传绝对值)。
 */
export function formatPlayerTime(seconds: number, opts?: { negative?: boolean }): string {
  if (!Number.isFinite(seconds)) return '--:--'

  const totalSec = Math.floor(Math.abs(seconds))
  const hours = Math.floor(totalSec / 3600)
  const mins = Math.floor((totalSec % 3600) / 60)
  const secs = totalSec % 60

  const pad = (n: number) => String(n).padStart(2, '0')
  const prefix = opts?.negative ? '-' : ''

  if (hours > 0) return `${prefix}${hours}:${pad(mins)}:${pad(secs)}`
  return `${prefix}${mins}:${pad(secs)}`
}

/** 获取媒体类型的徽章标签。 */
export function mediaBadgeLabel(mediaType: string, isLivePhoto: boolean): string | null {
  if (mediaType === 'image' && isLivePhoto) return 'LIVE'
  if (mediaType === 'video') return '▶'
  if (mediaType === 'audio') return '♪'
  if (mediaType === 'document') return 'DOC'
  return null
}
