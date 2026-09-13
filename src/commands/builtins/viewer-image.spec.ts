// viewer-image 命令册单测(顶栏重构 P5-2)。锁 when 谓词(仅 image/video 可见)+ run 分发
// (经 ctx.activeViewer.api 调对应动作)+ toggleImmersive isActive 反映沉浸态。纯数据 + 谓词,
// node 友好无需 DOM。
import { describe, it, expect, vi } from 'vitest'
import { viewerImageCommands } from './viewer-image'
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
          fileFormat: 'jpg',
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

describe('viewer-image 命令册(P5-2)', () => {
  it('when 谓词:image/video 可见,pdf(非图视)/网格(null)不可见', () => {
    const img = makeCtx('image')
    const vid = makeCtx('video')
    const pdf = makeCtx('pdf')
    const grid = makeCtx(null)
    // viewer.edit(图片简单编辑,方案 C)仅图像可见——video 不支持,单独断言,不入通用循环。
    // viewer.prev/next(GD:video 上下文 ←/→ 被 viewer-video.ts 夺走作 seek)同理仅图像,单独断言。
    for (const cmd of viewerImageCommands) {
      if (cmd.id === 'viewer.edit' || cmd.id === 'viewer.prev' || cmd.id === 'viewer.next') continue
      expect(isCommandVisible(cmd, img)).toBe(true)
      expect(isCommandVisible(cmd, vid)).toBe(true)
      expect(isCommandVisible(cmd, pdf)).toBe(false)
      expect(isCommandVisible(cmd, grid)).toBe(false)
    }
  })

  it('viewer.edit 仅图像可见(方案 C:v1 不支持视频编辑)', () => {
    const editCmd = viewerImageCommands.find((c) => c.id === 'viewer.edit')!
    expect(isCommandVisible(editCmd, makeCtx('image'))).toBe(true)
    expect(isCommandVisible(editCmd, makeCtx('video'))).toBe(false)
    expect(isCommandVisible(editCmd, makeCtx('pdf'))).toBe(false)
    expect(isCommandVisible(editCmd, makeCtx(null))).toBe(false)
  })

  it('viewer.prev/next 仅图像可见(GD:video ←/→ 已收窄到 viewer-video.ts 的 seek)', () => {
    const prevCmd = viewerImageCommands.find((c) => c.id === 'viewer.prev')!
    const nextCmd = viewerImageCommands.find((c) => c.id === 'viewer.next')!
    for (const cmd of [prevCmd, nextCmd]) {
      expect(isCommandVisible(cmd, makeCtx('image'))).toBe(true)
      expect(isCommandVisible(cmd, makeCtx('video'))).toBe(false)
      expect(isCommandVisible(cmd, makeCtx('pdf'))).toBe(false)
      expect(isCommandVisible(cmd, makeCtx(null))).toBe(false)
    }
  })

  it('run 经 ctx.activeViewer.api 调用对应动作', () => {
    const api: Record<string, ReturnType<typeof vi.fn>> = {
      close: vi.fn(),
      prev: vi.fn(),
      next: vi.fn(),
      zoomIn: vi.fn(),
      zoomOut: vi.fn(),
      cycleZoomMode: vi.fn(),
      rotate: vi.fn(),
      toggleInfo: vi.fn(),
      toggleImmersive: vi.fn(),
    }
    const ctx = makeCtx('image', api as ViewerApi)
    const byId = Object.fromEntries(viewerImageCommands.map((c) => [c.id, c]))
    const expectCall: Record<string, string> = {
      'viewer.close': 'close',
      'viewer.prev': 'prev',
      'viewer.next': 'next',
      'viewer.zoomIn': 'zoomIn',
      'viewer.zoomOut': 'zoomOut',
      'viewer.cycleZoomMode': 'cycleZoomMode',
      'viewer.rotate': 'rotate',
      'viewer.toggleInfo': 'toggleInfo',
      'viewer.toggleImmersive': 'toggleImmersive',
    }
    for (const [id, method] of Object.entries(expectCall)) {
      byId[id].run(ctx)
      expect(api[method]).toHaveBeenCalledOnce()
    }
  })

  it('命令 ownership：顶栏只保留跨项导航与沉浸，连续查看操作归入 view', () => {
    const groups = Object.fromEntries(viewerImageCommands.map((command) => [command.id, command.group]))
    expect(groups).toMatchObject({
      'viewer.close': 'navigation',
      'viewer.prev': 'navigation',
      'viewer.next': 'navigation',
      'viewer.toggleImmersive': 'navigation',
      'viewer.zoomIn': 'view',
      'viewer.zoomOut': 'view',
      'viewer.cycleZoomMode': 'view',
      'viewer.rotate': 'view',
      'viewer.toggleInfo': 'view',
    })
  })

  it('run 对缺失 api 方法安全(可选链不抛)', () => {
    const ctx = makeCtx('image', {}) // 空 api:所有方法未实现
    for (const cmd of viewerImageCommands) {
      expect(() => cmd.run(ctx)).not.toThrow()
    }
  })

  it('toggleImmersive isActive 反映 activeViewer.immersive', () => {
    const cmd = viewerImageCommands.find((c) => c.id === 'viewer.toggleImmersive')!
    const off = makeCtx('image')
    const on = makeCtx('image')
    on.activeViewer!.immersive = true
    expect(cmd.isActive?.(off)).toBe(false)
    expect(cmd.isActive?.(on)).toBe(true)
  })
})
