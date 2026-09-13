// viewer-video 命令册单测(视频播放器重构 GD 批)。锁 when 谓词(仅 video 可见)+ 键位映射
// (含 seek/切条目双绑分域:裸 ←/→=seek,shift+←/→=切条目)+ run 分发(经 ctx.activeViewer.api)。
// 纯数据 + 谓词,node 友好无需 DOM,仿 viewer-image.spec.ts 结构。
import { describe, it, expect, vi } from 'vitest'
import { viewerVideoCommands } from './viewer-video'
import { isCommandVisible } from '../registry'
import type { CommandContext } from '../types'
import type { ViewerApi } from '../../stores/viewerStore'
import type { MediaType } from '../../types/media'

function makeCtx(
  kind: 'image' | 'video' | 'pdf' | null,
  api: ViewerApi = {},
): CommandContext {
  return {
    view: kind ? 'viewer' : 'grid',
    activeViewer: kind
      ? {
          kind,
          mediaType: (kind === 'pdf' ? 'document' : kind) as MediaType,
          fileFormat: kind === 'video' ? 'mp4' : 'jpg',
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

describe('viewer-video 命令册(GD)', () => {
  it('when 谓词:仅 video 可见,image/pdf/网格(null)不可见', () => {
    const vid = makeCtx('video')
    const img = makeCtx('image')
    const pdf = makeCtx('pdf')
    const grid = makeCtx(null)
    for (const cmd of viewerVideoCommands) {
      expect(isCommandVisible(cmd, vid)).toBe(true)
      expect(isCommandVisible(cmd, img)).toBe(false)
      expect(isCommandVisible(cmd, pdf)).toBe(false)
      expect(isCommandVisible(cmd, grid)).toBe(false)
    }
  })

  it('键位映射:裸 ←/→ = seek,shift+←/→ = 切条目,无双绑', () => {
    const byId = Object.fromEntries(viewerVideoCommands.map((c) => [c.id, c]))
    expect(byId['video.seekBack'].keybinding).toBe('ArrowLeft')
    expect(byId['video.seekFwd'].keybinding).toBe('ArrowRight')
    expect(byId['video.prevItem'].keybinding).toBe('shift+ArrowLeft')
    expect(byId['video.nextItem'].keybinding).toBe('shift+ArrowRight')
    // seek 允许自动重复(按住连续 seek),切条目/prevItem 不声明 ignoreKeyRepeat(非开关类)。
    expect(byId['video.seekBack'].ignoreKeyRepeat).toBeFalsy()
    expect(byId['video.seekFwd'].ignoreKeyRepeat).toBeFalsy()
  })

  it('开关类命令(playPause/mute/fullscreen/loop/pip)声明 ignoreKeyRepeat', () => {
    const byId = Object.fromEntries(viewerVideoCommands.map((c) => [c.id, c]))
    for (const id of ['video.playPause', 'video.mute', 'video.fullscreen', 'video.loop', 'video.pip']) {
      expect(byId[id].ignoreKeyRepeat).toBe(true)
    }
  })

  it('playPause 键位含空格与 k 别名', () => {
    const cmd = viewerVideoCommands.find((c) => c.id === 'video.playPause')!
    expect(cmd.keybinding).toBe(' ')
    expect(cmd.keybindingAliases).toEqual(['k'])
  })

  it('音量/倍速键位', () => {
    const byId = Object.fromEntries(viewerVideoCommands.map((c) => [c.id, c]))
    expect(byId['video.volumeUp'].keybinding).toBe('ArrowUp')
    expect(byId['video.volumeDown'].keybinding).toBe('ArrowDown')
    expect(byId['video.rateUp'].keybinding).toBe('>')
    expect(byId['video.rateDown'].keybinding).toBe('<')
  })

  it('captureFrame 无默认键位(仅命令面板/顶栏入口)', () => {
    const cmd = viewerVideoCommands.find((c) => c.id === 'video.captureFrame')!
    expect(cmd.keybinding).toBeUndefined()
  })

  it('run 经 ctx.activeViewer.api 调用对应动作,含参数值', () => {
    const api: Record<string, ReturnType<typeof vi.fn>> = {
      playPause: vi.fn(),
      seekBy: vi.fn(),
      volumeBy: vi.fn(),
      toggleMute: vi.fn(),
      toggleFullscreenPair: vi.fn(),
      toggleLoop: vi.fn(),
      togglePip: vi.fn(),
      rateStep: vi.fn(),
      prev: vi.fn(),
      next: vi.fn(),
      captureFrame: vi.fn(),
    }
    const ctx = makeCtx('video', api as ViewerApi)
    const byId = Object.fromEntries(viewerVideoCommands.map((c) => [c.id, c]))

    byId['video.playPause'].run(ctx)
    expect(api.playPause).toHaveBeenCalledOnce()

    byId['video.seekBack'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(-5)
    byId['video.seekFwd'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(5)

    byId['video.volumeUp'].run(ctx)
    expect(api.volumeBy).toHaveBeenCalledWith(0.05)
    byId['video.volumeDown'].run(ctx)
    expect(api.volumeBy).toHaveBeenCalledWith(-0.05)

    byId['video.mute'].run(ctx)
    expect(api.toggleMute).toHaveBeenCalledOnce()

    byId['video.fullscreen'].run(ctx)
    expect(api.toggleFullscreenPair).toHaveBeenCalledOnce()

    byId['video.loop'].run(ctx)
    expect(api.toggleLoop).toHaveBeenCalledOnce()

    byId['video.pip'].run(ctx)
    expect(api.togglePip).toHaveBeenCalledOnce()

    byId['video.rateUp'].run(ctx)
    expect(api.rateStep).toHaveBeenCalledWith(1)
    byId['video.rateDown'].run(ctx)
    expect(api.rateStep).toHaveBeenCalledWith(-1)

    byId['video.prevItem'].run(ctx)
    expect(api.prev).toHaveBeenCalledOnce()
    byId['video.nextItem'].run(ctx)
    expect(api.next).toHaveBeenCalledOnce()

    byId['video.captureFrame'].run(ctx)
    expect(api.captureFrame).toHaveBeenCalledOnce()
  })

  it('run 对缺失 api 方法安全(可选链不抛)', () => {
    const ctx = makeCtx('video', {}) // 空 api:所有方法未实现
    for (const cmd of viewerVideoCommands) {
      expect(() => cmd.run(ctx)).not.toThrow()
    }
  })
})
