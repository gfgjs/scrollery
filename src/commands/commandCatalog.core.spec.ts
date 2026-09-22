// src/commands/commandCatalog.core.spec.ts
// 命令册（数据 + 谓词）与右键菜单解析。2026-09-16 由 viewer-image / viewer-video / viewer-audio /
// viewer-reader 四份命令册单测(顶栏重构 P5-2 / 视频播放器重构 GD)与 contextMenu.spec.ts(P2-3)
// 集中而来:同域纯数据、无 mock,五份文件删去;每条场景仍是独立 it。
//
// 为什么是这五份而不是「命令分发」三件套里的 contextMenu:keybinding 与 registry 同仓但 contextMenu
// 需要应用级单例注册表里的 global/watch 命令(registerBuiltins)才能解析,与 keybinding 的单例装入
// (viewer-image 一册)会互相干扰(同一 id 重复注册、grid 的 F11/mod+z 与用例自建命令抢键),
// 故按「是否依赖单例注册表」分开集中,不硬拼。
//
// 键位分域(视频):裸 ←/→ = seek,shift+←/→ = 切条目。
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { viewerImageCommands } from './builtins/viewer-image'
import { viewerVideoCommands } from './builtins/viewer-video'
import { viewerAudioCommands } from './builtins/viewer-audio'
import { viewerReaderCommands } from './builtins/viewer-reader'
import { registerBuiltins } from './index'
import { resolveMediaContextCommands } from './contextMenu'
import { isCommandVisible } from './registry'
import type { Command, CommandContext, ContextTarget } from './types'
import type { ViewerApi } from '../stores/viewerStore'
import type { ViewerKind } from '../utils/viewerKind'
import type { MediaType } from '../types/media'

const DOC_KINDS = ['pdf', 'epub', 'text', 'markdown']

function makeCtx(kind: string | null, api: ViewerApi = {}): CommandContext {
  return {
    view: kind ? 'viewer' : 'grid',
    activeViewer: kind
      ? {
          kind: kind as ViewerKind,
          mediaType: (DOC_KINDS.includes(kind) ? 'document' : kind) as MediaType,
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

function byId(commands: Command[]): Record<string, Command> {
  return Object.fromEntries(commands.map((c) => [c.id, c]))
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
    const editCmd = byId(viewerImageCommands)['viewer.edit']
    expect(isCommandVisible(editCmd, makeCtx('image'))).toBe(true)
    expect(isCommandVisible(editCmd, makeCtx('video'))).toBe(false)
    expect(isCommandVisible(editCmd, makeCtx('pdf'))).toBe(false)
    expect(isCommandVisible(editCmd, makeCtx(null))).toBe(false)
  })

  it('viewer.prev/next 仅图像可见(GD:video ←/→ 已收窄到 viewer-video.ts 的 seek)', () => {
    const commands = byId(viewerImageCommands)
    for (const cmd of [commands['viewer.prev'], commands['viewer.next']]) {
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
    const commands = byId(viewerImageCommands)
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
      commands[id].run(ctx)
      expect(api[method]).toHaveBeenCalledOnce()
    }
  })

  it('命令 ownership:顶栏只保留跨项导航与沉浸,连续查看操作归入 view', () => {
    const groups = Object.fromEntries(
      viewerImageCommands.map((command) => [command.id, command.group]),
    )
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

})

describe('viewer-video 命令册(GD)', () => {
  it('when 谓词:仅 video 可见,image/pdf/网格(null)不可见', () => {
    const vid = makeCtx('video')
    for (const cmd of viewerVideoCommands) {
      expect(isCommandVisible(cmd, vid)).toBe(true)
      expect(isCommandVisible(cmd, makeCtx('image'))).toBe(false)
      expect(isCommandVisible(cmd, makeCtx('pdf'))).toBe(false)
      expect(isCommandVisible(cmd, makeCtx(null))).toBe(false)
    }
  })

  it('键位映射:裸 ←/→ = seek,shift+←/→ = 切条目,无双绑', () => {
    const commands = byId(viewerVideoCommands)
    expect(commands['video.seekBack'].keybinding).toBe('ArrowLeft')
    expect(commands['video.seekFwd'].keybinding).toBe('ArrowRight')
    expect(commands['video.prevItem'].keybinding).toBe('shift+ArrowLeft')
    expect(commands['video.nextItem'].keybinding).toBe('shift+ArrowRight')
    // seek 允许自动重复(按住连续 seek),切条目/prevItem 不声明 ignoreKeyRepeat(非开关类)。
    expect(commands['video.seekBack'].ignoreKeyRepeat).toBeFalsy()
    expect(commands['video.seekFwd'].ignoreKeyRepeat).toBeFalsy()
  })

  it('开关类命令(playPause/mute/fullscreen/loop/pip)声明 ignoreKeyRepeat', () => {
    const commands = byId(viewerVideoCommands)
    for (const id of [
      'video.playPause',
      'video.mute',
      'video.fullscreen',
      'video.loop',
      'video.pip',
    ]) {
      expect(commands[id].ignoreKeyRepeat).toBe(true)
    }
  })

  it('playPause 键位含空格与 k 别名', () => {
    const cmd = byId(viewerVideoCommands)['video.playPause']
    expect(cmd.keybinding).toBe(' ')
    expect(cmd.keybindingAliases).toEqual(['k'])
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
    const commands = byId(viewerVideoCommands)

    commands['video.playPause'].run(ctx)
    expect(api.playPause).toHaveBeenCalledOnce()

    commands['video.seekBack'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(-5)
    commands['video.seekFwd'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(5)

    commands['video.volumeUp'].run(ctx)
    expect(api.volumeBy).toHaveBeenCalledWith(0.05)
    commands['video.volumeDown'].run(ctx)
    expect(api.volumeBy).toHaveBeenCalledWith(-0.05)

    commands['video.mute'].run(ctx)
    expect(api.toggleMute).toHaveBeenCalledOnce()
    commands['video.fullscreen'].run(ctx)
    expect(api.toggleFullscreenPair).toHaveBeenCalledOnce()
    commands['video.loop'].run(ctx)
    expect(api.toggleLoop).toHaveBeenCalledOnce()
    commands['video.pip'].run(ctx)
    expect(api.togglePip).toHaveBeenCalledOnce()

    commands['video.rateUp'].run(ctx)
    expect(api.rateStep).toHaveBeenCalledWith(1)
    commands['video.rateDown'].run(ctx)
    expect(api.rateStep).toHaveBeenCalledWith(-1)

    commands['video.prevItem'].run(ctx)
    expect(api.prev).toHaveBeenCalledOnce()
    commands['video.nextItem'].run(ctx)
    expect(api.next).toHaveBeenCalledOnce()
    commands['video.captureFrame'].run(ctx)
    expect(api.captureFrame).toHaveBeenCalledOnce()
  })

})

describe('viewer-audio 命令册(P5 余项)', () => {
  it('when 谓词:audio 可见,image(非音频)/网格(null)不可见', () => {
    const audio = makeCtx('audio')
    for (const cmd of viewerAudioCommands) {
      expect(isCommandVisible(cmd, audio)).toBe(true)
      expect(isCommandVisible(cmd, makeCtx('image'))).toBe(false)
      expect(isCommandVisible(cmd, makeCtx(null))).toBe(false)
    }
  })

  it('run 经 ctx.activeViewer.api 调用对应动作(seekBy 带方向 ±10)', () => {
    const api = { togglePlay: vi.fn(), seekBy: vi.fn() }
    const ctx = makeCtx('audio', api as ViewerApi)
    const commands = byId(viewerAudioCommands)

    commands['viewer.audio.togglePlay'].run(ctx)
    expect(api.togglePlay).toHaveBeenCalledOnce()

    commands['viewer.audio.seekBackward'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(-10)

    commands['viewer.audio.seekForward'].run(ctx)
    expect(api.seekBy).toHaveBeenCalledWith(10)
  })
})

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
    const commands = byId(viewerReaderCommands)
    const expectCall: Record<string, keyof typeof api> = {
      'viewer.reader.toc': 'toggleToc',
      'viewer.reader.search': 'toggleSearch',
      'viewer.reader.bookmarks': 'toggleBookmarks',
      'viewer.reader.settings': 'toggleSettings',
    }
    for (const [id, method] of Object.entries(expectCall)) {
      commands[id].run(ctx)
      expect(api[method]).toHaveBeenCalledOnce()
    }
  })

})

beforeAll(() => registerBuiltins())

function menuCtx(contextTarget: ContextTarget | null): CommandContext {
  return {
    view: 'grid',
    activeViewer: null,
    selection: { count: 0, isSingle: false },
    contextTarget,
  }
}

// 宿主本地 move/copy(顺序锚 30/40,落在 explorer 与 wallpaper 之间——还原现状菜单顺序)。
const localMoveCopy: Command[] = [
  { id: 'x.move', title: 'move', group: 'organize', order: 30, run: () => {} },
  { id: 'x.copy', title: 'copy', group: 'organize', order: 40, run: () => {} },
]

describe('resolveMediaContextCommands(P2-3)', () => {
  it('图片目标:copy·explorer·move·copy·wallpaper(与收敛前顺序一致)', () => {
    const ctx = menuCtx({ id: 1, mediaType: 'image' })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual([
      'global.copyImage',
      'global.showInExplorer',
      'x.move',
      'x.copy',
      'global.setWallpaper',
    ])
  })

  it('视频目标:无壁纸(壁纸-仅图片条件生效)', () => {
    const ctx = menuCtx({ id: 1, mediaType: 'video' })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['global.copyImage', 'global.showInExplorer', 'x.move', 'x.copy'])
    expect(ids).not.toContain('global.setWallpaper')
  })

  it('目标类型未知(item 查找失败降级):通用命令仍显示,壁纸不显示', () => {
    const ctx = menuCtx({ id: 1 })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['global.copyImage', 'global.showInExplorer', 'x.move', 'x.copy'])
  })

  it('无目标(contextTarget=null):共享命令全不显示,仅剩本地 move/copy', () => {
    const ctx = menuCtx(null)
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['x.move', 'x.copy'])
  })
})
