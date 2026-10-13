// 音频命令通过 activeViewer.api 调用 AudioPlayer，显示在标题栏导航区。
// AudioPlayer 按注册表分发键盘操作，与按钮共用同一组动作和键位。

import { markRaw } from 'vue'
import { Play, SkipBack, SkipForward } from '@lucide/vue'
import type { Command, CommandContext } from '../types'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

/** 音频可见判据:仅当活动查看器是音频(AudioPlayer 承载)。 */
function isAudio(ctx: CommandContext): boolean {
  return ctx.activeViewer?.kind === 'audio'
}

export const viewerAudioCommands: Command[] = [
  {
    id: 'viewer.audio.seekBackward',
    title: () => t('audio.rewind10'),
    icon: markRaw(SkipBack),
    group: 'navigation',
    order: 10,
    keybinding: 'ArrowLeft',
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(-10),
  },
  {
    id: 'viewer.audio.togglePlay',
    title: () => t('audio.playPause'),
    icon: markRaw(Play),
    group: 'navigation',
    order: 20,
    keybinding: ' ',
    keybindingAliases: ['k'],
    ignoreKeyRepeat: true,
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.togglePlay?.(),
  },
  {
    id: 'viewer.audio.seekForward',
    title: () => t('audio.forward10'),
    icon: markRaw(SkipForward),
    group: 'navigation',
    order: 30,
    keybinding: 'ArrowRight',
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(10),
  },
]
