// src/composables/useContentViewerKeyboard.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):键盘快捷键。自带 mounted/unmounted 生命周期
// (模式同既有 useTauriListen),组件侧不再手写 onMounted/onBeforeUnmount 里的 keydown 两行。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。

import { onMounted, onBeforeUnmount, type Ref } from 'vue'
import { requestDiscardEdits, type useImageEditor } from './useImageEditor'
import { buildCommandContext, dispatchKeybinding } from '../commands'
import type { useMediaStore } from '../stores/mediaStore'
import type { useViewerStore } from '../stores/viewerStore'
import type { MediaDetail } from '../types/media'
import type VideoPlayer from '../components/media/player/VideoPlayer.vue'

export function useContentViewerKeyboard(options: {
  media: ReturnType<typeof useMediaStore>
  editor: ReturnType<typeof useImageEditor>
  viewer: ReturnType<typeof useViewerStore>
  videoRef: Ref<InstanceType<typeof VideoPlayer> | null>
  detail: () => MediaDetail | null
  close: () => void
}) {
  const { media, editor, viewer, videoRef, detail, close } = options

  // Registered via onMounted / onBeforeUnmount:本组件按路由挂载/卸载(P4-b 迁路由后不再有
  // Teleport 保活),监听器随生命周期加/摘,天然无累积。
  function onKeydown(e: KeyboardEvent) {
    // 交互目标守卫(GD):事件源落在可编辑控件上时放行浏览器原生行为,不拦截/不分发(否则空格/
    // 方向键等会在输入框内被查看器命令截走,输入体验被破坏)。
    const target = e.target as HTMLElement | null
    if (target?.matches?.('input, textarea, select, [contenteditable]')) return
    // 空格键落在已聚焦的 <button> 上时不分发:浏览器原生会触发该按钮的 click,若命令层同时把
    // 空格分发给 video.playPause 会造成双触发(如控制条按钮聚焦态下按空格)。
    if (
      e.key === ' ' &&
      document.activeElement instanceof HTMLElement &&
      document.activeElement.tagName === 'BUTTON'
    )
      return
    if (!media.detailItem) return
    // 编辑中(方案 §7):禁翻页/其余快捷键,仅接管 Esc(有改动二次确认)与裁剪框键盘微调。
    if (editor.status.value !== 'idle') {
      if (editor.status.value === 'saving') return // 保存中不响应任何按键(含 Esc)
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopImmediatePropagation()
        void (async () => {
          if (await requestDiscardEdits(editor)) editor.close()
        })()
        return
      }
      if (editor.crop.value && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) {
        e.preventDefault()
        const step = e.shiftKey ? 10 : 1
        const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0
        const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0
        editor.nudgeCrop(dx, dy)
      }
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopImmediatePropagation()
      // 视频 F 全屏对(GD):Esc 先退全屏对(窗口全屏+沉浸成对进退,详见 useVideoFullscreen),
      // 不越权代退 F11 触发的全屏(F11 进入的不带 pairFlag,isFullscreenPair 天然为 false)。
      const item = detail()
      if (item?.mediaType === 'video' && videoRef.value?.isFullscreenPair?.()) {
        videoRef.value.toggleFullscreenPair?.()
        return
      }
      // 沉浸时 Esc 先退沉浸,否则关查看器(与阅读器 Esc 语义一致)。
      if (viewer.isImmersive) viewer.setImmersive(false)
      else close()
      return
    }
    // 键位同源(P5-6):缩放/翻页/信息/旋转等交注册表按 command.keybinding 分发——与 tooltip 同一
    // 来源,杜绝「显示≠行为」漂移;命中即消费。原散点 if (e.key === '+'/'i'/'ArrowLeft'…) 收敛于此。
    if (dispatchKeybinding(e, buildCommandContext())) e.preventDefault()
  }

  onMounted(() => {
    document.addEventListener('keydown', onKeydown)
  })
  onBeforeUnmount(() => {
    document.removeEventListener('keydown', onKeydown)
  })

  return { onKeydown }
}
