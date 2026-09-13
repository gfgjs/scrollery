// viewerKind 单测(顶栏重构 P0-2)。纯函数,node 可测。
import { describe, it, expect } from 'vitest'
import { resolveViewerKind } from './viewerKind'

describe('resolveViewerKind', () => {
  it('image/video/audio 主类直接映射,忽略 fileFormat', () => {
    expect(resolveViewerKind('image', 'jpg')).toBe('image')
    expect(resolveViewerKind('image', 'heic')).toBe('image')
    expect(resolveViewerKind('image', '')).toBe('image')
    expect(resolveViewerKind('video', 'mp4')).toBe('video')
    expect(resolveViewerKind('audio', 'flac')).toBe('audio')
  })

  it('document 按扩展名细分 pdf/epub/markdown/text', () => {
    expect(resolveViewerKind('document', 'pdf')).toBe('pdf')
    expect(resolveViewerKind('document', 'epub')).toBe('epub')
    expect(resolveViewerKind('document', 'md')).toBe('markdown')
    expect(resolveViewerKind('document', 'markdown')).toBe('markdown')
    expect(resolveViewerKind('document', 'txt')).toBe('text')
  })

  it('大小写不敏感 + 去首尾空白', () => {
    expect(resolveViewerKind('document', 'PDF')).toBe('pdf')
    expect(resolveViewerKind('document', 'EPub')).toBe('epub')
    expect(resolveViewerKind('document', '  MD  ')).toBe('markdown')
    expect(resolveViewerKind('document', 'TXT')).toBe('text')
  })

  it('未知文档扩展名兜底 text(不可渲染格式由分发层前置拦截,不达此处)', () => {
    expect(resolveViewerKind('document', 'rtf')).toBe('text')
    expect(resolveViewerKind('document', 'docx')).toBe('text')
    expect(resolveViewerKind('document', '')).toBe('text')
  })
})
