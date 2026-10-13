// 视频命令通过 ContentViewer 暴露的播放器 API 执行。视频中的 ←/→ 用于 seek，
// Shift+←/→ 用于切换条目；when 谓词按 video 上下文收窄，避免与图片键位冲突。

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
