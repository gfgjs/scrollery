// viewer-audio 命令册单测(顶栏重构 P5 余项)。锁 when 谓词(仅 audio 可见)+ run 分发
// (经 ctx.activeViewer.api 调 togglePlay/seekBy,含 seekBy 方向参数)+ 缺 api 安全。
// 纯数据 + 谓词,node 友好无需 DOM。
import { describe, it, expect, vi } from 'vitest'
import { viewerAudioCommands } from './viewer-audio'
import { isCommandVisible } from '../registry'
import type { CommandContext } from '../types'
import type { ViewerApi } from '../../stores/viewerStore'
import type { MediaType } from '../../types/media'

function makeCtx(kind: 'audio' | 'image' | null, api: ViewerApi = {}): CommandContext {
  return {
    view: kind ? 'viewer' : 'grid',
    activeViewer: kind
      ? {
          kind,
          mediaType: kind as MediaType,
          fileFormat: 'mp3',
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

describe('viewer-audio 命令册(P5 余项)', () => {
  it('when 谓词:audio 可见,image(非音频)/网格(null)不可见', () => {
    const audio = makeCtx('audio')
    const img = makeCtx('image')
    const grid = makeCtx(null)
    for (const cmd of viewerAudioCommands) {
      expect(isCommandVisible(cmd, audio)).toBe(true)
      expect(isCommandVisible(cmd, img)).toBe(false)
      expect(isCommandVisible(cmd, grid)).toBe(false)
    }
  })

  it('run 经 ctx.activeViewer.api 调用对应动作(seekBy 带方向 ±10)', () => {
    const api = {
      togglePlay: vi.fn(),
      seekBy: vi.fn(),
    }
    const ctx = makeCtx('audio', api as ViewerApi)
    const byId = Object.fromEntries(viewerAudioCommands.map((c) => [c.id, c]))

    byId['viewer.audio.togglePlay'].run(ctx)
    expect(api.togglePlay).toHaveBeenCalledOnce()

    byId['viewer.audio.seekBackward'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(-10)

    byId['viewer.audio.seekForward'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(10)
  })

  it('run 对缺失 api 方法安全(可选链不抛)', () => {
    const ctx = makeCtx('audio', {}) // 空 api
    for (const cmd of viewerAudioCommands) {
      expect(() => cmd.run(ctx)).not.toThrow()
    }
  })
})
