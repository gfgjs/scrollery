// 底栏文件信息段构建(内容页动态文件信息线,2026-07-17)。
// 纯函数:ActiveViewer 标题/格式 + ViewerFileInfo 标量 → 有序展示段数组。
// 顺序即折叠优先级(Priority+ 从尾折):文件名 > 尺寸 > 时长 > 大小 > 格式——
// 用户拍板「按重要程度先藏次要的」,格式最先折、文件名最后折;交互三件(星/心/色)不在此列、永不折。

import type { ViewerFileInfo } from '../../stores/viewerStore'
import { formatFileSize, formatDuration } from '../../utils/format'

export interface InfoSegment {
  key: 'fileName' | 'dims' | 'duration' | 'fileSize' | 'format'
  /** ⋯ 弹层行标签的 i18n 键(展示态只出值,弹层里带标签)。 */
  labelKey: string
  text: string
}

/**
 * 构建底栏展示段。null/缺失标量的段直接不产出(音频无尺寸、文档无时长等),
 * DOM 顺序即重要度降序,消费方交给 useToolbarOverflow 从尾部折叠。
 */
export function buildInfoSegments(
  title: string,
  fileFormat: string,
  info: ViewerFileInfo | null,
): InfoSegment[] {
  const segs: InfoSegment[] = []
  if (title) segs.push({ key: 'fileName', labelKey: 'detail.fileName', text: title })
  if (info?.width != null && info.height != null) {
    segs.push({ key: 'dims', labelKey: 'detail.dimensions', text: `${info.width}×${info.height}` })
  }
  if (info?.durationMs != null) {
    segs.push({
      key: 'duration',
      labelKey: 'statusbar.duration',
      text: formatDuration(info.durationMs),
    })
  }
  if (info != null) {
    segs.push({ key: 'fileSize', labelKey: 'detail.fileSize', text: formatFileSize(info.fileSize) })
  }
  if (fileFormat) {
    segs.push({ key: 'format', labelKey: 'detail.format', text: fileFormat.toUpperCase() })
  }
  return segs
}
