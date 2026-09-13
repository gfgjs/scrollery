// src/utils/viewerKind.ts
// ViewerKind 收敛:把散落在各查看器组件里的局部 kind computed(如 DocumentViewer 的
// pdf/epub/text 四分类 + 旁路 isMarkdown)统一为单一分类源。activeViewer.kind 与命令 when
// 谓词均以此为唯一来源(设计文档 §3.3 / §4.2),消除「MediaType + fileFormat + 各组件局部 kind」
// 的三层拼凑。
//
// 与 MediaType(image/video/audio/document,DB 扫描期确定)的关系:mediaType 是主轴,
// document 再按 fileFormat 细分为 pdf/epub/markdown/text;图/视/音一一对应。

import type { MediaType } from '../types/media'

/**
 * 查看器分类。比 MediaType 更细:document 展开为 pdf/epub/text/markdown(各有专属命令集与
 * 渲染管线)。
 *
 * **刻意不含 'unsupported'**:不可渲染的文档格式(rtf/docx 等)由查看器分发层(Phase 4 的
 * ContentViewer)前置拦截走「外部打开」,永不 populate 进 viewerStore,故本纯函数只负责
 * 「可渲染类」的分类,始终返回 union 内的值。
 */
export type ViewerKind = 'image' | 'video' | 'audio' | 'pdf' | 'epub' | 'text' | 'markdown'

// md 的双别名:.md 与少数工具产出的 .markdown 均归 markdown 类(现状 DocumentViewer 仅认
// 'md',此处并入 'markdown' 扩展名一并覆盖——只多容一种别名,不改现有行为)。
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
      // txt 及其它文本类归 text;不可渲染文档(rtf/docx)本不会到达此处(分发层前置拦截),
      // 兜底同样落 text 是最安全的文本近似(避免返回不在 union 内的值)。
      return 'text'
    }
  }
}
