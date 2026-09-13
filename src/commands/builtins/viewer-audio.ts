// 音频查看器专属命令(顶栏重构 P5 余项)。AudioPlayer 经 viewerStore.populate 暴露 ViewerApi
// (togglePlay / seekBy),本册命令经 ctx.activeViewer.api 调用;when 谓词按 activeViewer.kind
// 过滤(audio)。登记为 navigation 组 → 显示在标题栏上下文工具栏主按钮区。
//
// 过渡态(承 P5-5 用户决策=保留局部控件):与 AudioPlayer 内的播放控件并存(双入口),顶栏为补充。
// 未挂 keybinding:AudioPlayer 尚未接 dispatchKeybinding(空格等键位同源留后续),避免 tooltip 显
// 无意义的裸键位。

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
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(-10),
  },
  {
    id: 'viewer.audio.togglePlay',
    title: () => t('audio.playPause'),
    icon: markRaw(Play),
    group: 'navigation',
    order: 20,
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.togglePlay?.(),
  },
  {
    id: 'viewer.audio.seekForward',
    title: () => t('audio.forward10'),
    icon: markRaw(SkipForward),
    group: 'navigation',
    order: 30,
    when: isAudio,
    run: (ctx) => ctx.activeViewer?.api?.seekBy?.(10),
  },
]
