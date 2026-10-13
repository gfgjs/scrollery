// 画廊与时间轴的 DOM/Canvas 偏好共用模块级响应式状态，来源为中央设置。
// 设置变更即时生效，由 MediaGrid 与设置控件共同消费。
import { computed } from 'vue'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingEnum } from './settingsValues'

export type RenderMode = 'dom' | 'canvas'

const GALLERY_KEY = 'gallery_render_mode'
const TIMELINE_KEY = 'timeline_render_mode'
const RENDER_MODES = ['dom', 'canvas'] as const

// 默认使用 Canvas；缺键或非法值回落默认，显式保存的 DOM 偏好仍生效。
const galleryRenderMode = computed(() =>
  readSettingEnum<RenderMode>(GALLERY_KEY, RENDER_MODES, 'canvas'),
)
const timelineRenderMode = computed(() =>
  readSettingEnum<RenderMode>(TIMELINE_KEY, RENDER_MODES, 'canvas'),
)

function setGalleryRenderMode(mode: RenderMode) {
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [GALLERY_KEY]: mode }).catch(() => {})
}

function setTimelineRenderMode(mode: RenderMode) {
  // 同上。
  writeSettings({ [TIMELINE_KEY]: mode }).catch(() => {})
}

export function useRenderMode() {
  return {
    galleryRenderMode,
    timelineRenderMode,
    setGalleryRenderMode,
    setTimelineRenderMode,
  }
}
