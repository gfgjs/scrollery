// 视频查看器专属命令(视频播放器重构 GD 批)。VideoPlayer 经 ContentViewer viewerApi 透传
// PlayerApi(playPause/seekBy/volumeBy/toggleMute/setRate/rateStep/toggleLoop/togglePip/
// toggleFullscreenPair/captureFrame),本册命令经 ctx.activeViewer.api 调用具体动作;when 谓词
// 按 activeViewer.kind==='video' 收窄(仿 viewer-image.ts 的 when 谓词模式)。
//
// 快捷键裁决(task_plan「快捷键:视频上下文夺走 ←/→ 作 seek」):video 上下文 ←/→ 语义从
// viewer-image 的「切条目」改为「seek」,切条目改 Shift+←/→(navigation 组沿用 viewer.prev/next
// 的顶栏位;viewer-image.ts 的 viewer.prev/next when 已收窄到 isImage,本册在此接管 video)。

import { markRaw } from 'vue'
import { ChevronLeft, ChevronRight } from '@lucide/vue'
import type { Command, CommandContext } from '../types'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

/** 视频可见判据:仅当活动查看器是视频(ContentViewer 承载,VideoPlayer 实现)。 */
function isVideo(ctx: CommandContext): boolean {
  return ctx.activeViewer?.kind === 'video'
}

export const viewerVideoCommands: Command[] = [
  {
    id: 'video.playPause',
    title: () => t('player.play'),
    group: 'playback',
    keybinding: ' ',
    keybindingAliases: ['k'],
    ignoreKeyRepeat: true,
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.playPause?.(),
  },
  {
    id: 'video.seekBack',
    title: () => t('player.seek'),
    group: 'playback',
    keybinding: 'ArrowLeft',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(-5),
  },
  {
    id: 'video.seekFwd',
    title: () => t('player.seek'),
    group: 'playback',
    keybinding: 'ArrowRight',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(5),
  },
  {
    id: 'video.volumeUp',
    title: () => t('player.volume'),
    group: 'playback',
    keybinding: 'ArrowUp',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.volumeBy?.(0.05),
  },
  {
    id: 'video.volumeDown',
    title: () => t('player.volume'),
    group: 'playback',
    keybinding: 'ArrowDown',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.volumeBy?.(-0.05),
  },
  {
    id: 'video.mute',
    title: () => t('player.mute'),
    group: 'playback',
    keybinding: 'm',
    ignoreKeyRepeat: true,
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.toggleMute?.(),
  },
  {
    id: 'video.fullscreen',
    title: () => t('player.fullscreen'),
    group: 'playback',
    keybinding: 'f',
    ignoreKeyRepeat: true,
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.toggleFullscreenPair?.(),
  },
  {
    id: 'video.loop',
    title: () => t('player.loop'),
    group: 'playback',
    keybinding: 'l',
    ignoreKeyRepeat: true,
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.toggleLoop?.(),
  },
  {
    id: 'video.pip',
    title: () => t('player.pip'),
    group: 'playback',
    keybinding: 'p',
    ignoreKeyRepeat: true,
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.togglePip?.(),
  },
  {
    id: 'video.rateUp',
    title: () => t('player.playbackRate'),
    group: 'playback',
    keybinding: '>',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.rateStep?.(1),
  },
  {
    id: 'video.rateDown',
    title: () => t('player.playbackRate'),
    group: 'playback',
    keybinding: '<',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.rateStep?.(-1),
  },
  {
    id: 'video.prevItem',
    title: () => t('detail.prev'),
    icon: markRaw(ChevronLeft),
    group: 'navigation',
    order: 10,
    keybinding: 'shift+ArrowLeft', // gitleaks:allow — 键盘快捷键，不是凭据。
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.prev?.(),
  },
  {
    id: 'video.nextItem',
    title: () => t('detail.next'),
    icon: markRaw(ChevronRight),
    group: 'navigation',
    order: 20,
    keybinding: 'shift+ArrowRight',
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.next?.(),
  },
  {
    id: 'video.captureFrame',
    title: () => t('player.captureFrame'),
    group: 'playback',
    // 无默认快捷键(仅命令面板/顶栏入口),避免与常用截图工具热键冲突。
    when: isVideo,
    run: (ctx) => ctx.activeViewer?.api?.captureFrame?.(),
  },
]
