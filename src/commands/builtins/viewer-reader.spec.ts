// viewer-reader 命令册单测(顶栏重构 P5 余项)。锁 when 谓词(仅 foliate epub/text/markdown 可见,
// pdf 与非阅读器不可见)+ run 分发(经 ctx.activeViewer.api 调 toggle*)+ 缺 api 安全。
// 纯数据 + 谓词,node 友好无需 DOM。
import { describe, it, expect, vi } from 'vitest'
import { viewerReaderCommands } from './viewer-reader'
import { isCommandVisible } from '../registry'
import type { CommandContext } from '../types'
import type { ViewerApi } from '../../stores/viewerStore'
import type { ViewerKind } from '../../utils/viewerKind'
import type { MediaType } from '../../types/media'

function makeCtx(kind: ViewerKind | null, api: ViewerApi = {}): CommandContext {
  return {
    view: kind ? 'viewer' : 'grid',
    activeViewer: kind
      ? {
          kind,
          mediaType: (kind === 'image' ? 'image' : 'document') as MediaType,
          fileFormat: 'epub',
          id: 1,
          path: null,
          title: 't',
          api,
          immersive: false,
          fileInfo: null,
        }
      : null,
    selection: { count: 0, isSingle: false },
    contextTarget: null,
  }
}

describe('viewer-reader 命令册(P5 余项)', () => {
  it('when 谓词:foliate(epub/text/markdown)可见,pdf/image/网格不可见', () => {
    const visible: (ViewerKind | null)[] = ['epub', 'text', 'markdown']
    const hidden: (ViewerKind | null)[] = ['pdf', 'image', null]
    for (const cmd of viewerReaderCommands) {
      for (const k of visible) expect(isCommandVisible(cmd, makeCtx(k))).toBe(true)
      for (const k of hidden) expect(isCommandVisible(cmd, makeCtx(k))).toBe(false)
    }
  })

  it('run 经 ctx.activeViewer.api 调用对应 toggle', () => {
    const api = {
      toggleToc: vi.fn(),
      toggleSearch: vi.fn(),
      toggleBookmarks: vi.fn(),
      toggleSettings: vi.fn(),
    }
    const ctx = makeCtx('epub', api as ViewerApi)
    const byId = Object.fromEntries(viewerReaderCommands.map((c) => [c.id, c]))
    const expectCall: Record<string, keyof typeof api> = {
      'viewer.reader.toc': 'toggleToc',
      'viewer.reader.search': 'toggleSearch',
      'viewer.reader.bookmarks': 'toggleBookmarks',
      'viewer.reader.settings': 'toggleSettings',
    }
    for (const [id, method] of Object.entries(expectCall)) {
      byId[id].run(ctx)
      expect(api[method]).toHaveBeenCalledOnce()
    }
  })

  it('run 对缺失 api 方法安全(可选链不抛)', () => {
    const ctx = makeCtx('epub', {}) // 空 api
    for (const cmd of viewerReaderCommands) {
      expect(() => cmd.run(ctx)).not.toThrow()
    }
  })
})
