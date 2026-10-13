// 根据媒体主类与扩展名推导 activeViewer.kind，供命令谓词使用。
//
// 与 MediaType(image/video/audio/document,DB 扫描期确定)的关系:mediaType 是主轴,
// document 再按 fileFormat 细分为 pdf/epub/markdown/text;图/视/音一一对应。

import type { MediaType } from '../types/media'

/**
 * 查看器分类。比 MediaType 更细:document 展开为 pdf/epub/text/markdown(各有专属命令集与
 * 渲染管线)。
 *
 * 不含 'unsupported'；未知文档格式在 resolveViewerKind 中回退为 text。
 */
export type ViewerKind = 'image' | 'video' | 'audio' | 'pdf' | 'epub' | 'text' | 'markdown'

// md 与 markdown 扩展名均归 markdown 类。
const MARKDOWN_FORMATS = new Set(['md', 'markdown'])

/**
 * 由 (mediaType, fileFormat) 单点推导 ViewerKind。纯函数、大小写不敏感、去首尾空白。
 *
 * @param mediaType DB 扫描期确定的主类(image/video/audio/document)
 * @param fileFormat 文件扩展名(不含点,如 'jpg' / 'pdf' / 'epub')
 * @returns 收敛后的查看器分类
 */
export function resolveViewerKind(mediaType: MediaType, fileFormat: string): ViewerKind {
  switch (mediaType) {
    case 'image':
      return 'image'
    case 'video':
      return 'video'
    case 'audio':
      return 'audio'
    case 'document': {
      const fmt = fileFormat.toLowerCase().trim()
      if (fmt === 'pdf') return 'pdf'
      if (fmt === 'epub') return 'epub'
      if (MARKDOWN_FORMATS.has(fmt)) return 'markdown'
      // 其余文档格式回退为 text；调用方需自行判定该格式是否可渲染。
      return 'text'
    }
  }
}
