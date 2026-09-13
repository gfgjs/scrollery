// src/composables/useRenderMode.ts
// 画廊 / 时间轴的 DOM↔Canvas 渲染引擎偏好——模块级响应式单例 + localStorage 持久化。
//
// 背景:此前渲染模式是 MediaGrid 的局部 ref + 直接读写 localStorage,且切换药丸被 dev-gate 隐藏
// (import.meta.env.DEV && localStorage 'scrollery.debug.renderMode'==='1'),真机用户无从切换。
// 用户裁决(2026-07-12):在设置页 surface 为「实验性」高级开关。要让设置项与 MediaGrid 共享同一份
// 可响应状态(设置改动 live 生效、无需刷新),故把状态从 MediaGrid 局部提升为本模块级单例。
//
// 仍用 localStorage 持久(非后端 app_config)——canvas 是原型引擎,偏好属前端本地实验态,不进后端配置 schema。
// 消费方:MediaGrid(读 galleryRenderMode/timelineRenderMode 驱动 canvasMode/scrubber 变体)、
// DynamicSettingControl 的 select 绑定(设置页开关)。
import { ref } from 'vue'

export type RenderMode = 'dom' | 'canvas'

const GALLERY_KEY = 'gallery_render_mode'
const TIMELINE_KEY = 'timeline_render_mode'

function readMode(key: string): RenderMode {
  // 防御式:仅 'canvas' 显式命中,其余(含缺省 / 垃圾值 / localStorage 不可用)一律回落 'dom'。
  try {
    return localStorage.getItem(key) === 'canvas' ? 'canvas' : 'dom'
  } catch {
    return 'dom'
  }
}

// 模块级单例 ref:所有 useRenderMode() 调用方共享同一份响应式状态。
const galleryRenderMode = ref<RenderMode>(readMode(GALLERY_KEY))
const timelineRenderMode = ref<RenderMode>(readMode(TIMELINE_KEY))

function setGalleryRenderMode(mode: RenderMode) {
  galleryRenderMode.value = mode
  try {
    localStorage.setItem(GALLERY_KEY, mode)
  } catch {
    // localStorage 不可用只损失持久化,本次会话内切换仍生效,静默降级。
  }
}

function setTimelineRenderMode(mode: RenderMode) {
  timelineRenderMode.value = mode
  try {
    localStorage.setItem(TIMELINE_KEY, mode)
  } catch {
    // 同上,静默降级。
  }
}

export function useRenderMode() {
  return {
    galleryRenderMode,
    timelineRenderMode,
    setGalleryRenderMode,
    setTimelineRenderMode,
  }
}
