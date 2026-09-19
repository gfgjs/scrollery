// src/composables/useRenderMode.ts
// 画廊 / 时间轴的 DOM↔Canvas 渲染引擎偏好——模块级响应式单例,值来自中央设置(config.toml)。
//
// 背景:此前渲染模式是 MediaGrid 的局部 ref + 直接读写 localStorage,且切换药丸被 dev-gate 隐藏
// (import.meta.env.DEV && localStorage 'scrollery.debug.renderMode'==='1'),真机用户无从切换。
// 用户裁决(2026-07-12):在设置页 surface 为「实验性」高级开关。要让设置项与 MediaGrid 共享同一份
// 可响应状态(设置改动 live 生效、无需刷新),故把状态从 MediaGrid 局部提升为本模块级单例。
//
// 存储(设置集中保存,批次B):键 gallery_render_mode / timeline_render_mode 由后端 schema 注册,
// 与其它全局用户偏好同库同理。
// 消费方:MediaGrid(读 galleryRenderMode/timelineRenderMode 驱动 canvasMode/scrubber 变体)、
// DynamicSettingControl 的 select 绑定(设置页开关)。
import { computed } from 'vue'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingEnum } from './settingsValues'

export type RenderMode = 'dom' | 'canvas'

const GALLERY_KEY = 'gallery_render_mode'
const TIMELINE_KEY = 'timeline_render_mode'
const RENDER_MODES = ['dom', 'canvas'] as const

// 模块级单例:所有 useRenderMode() 调用方共享同一份响应式状态。
// 画廊与时间轴默认均 canvas(用户裁决 2026-09-16:两处默认引擎统一为 Canvas);缺省/垃圾值/快照未到回落 canvas。
// 用户显式存 'dom' 仍原样生效——回退只作用于「读不到值」,不覆盖已存的用户选择;
// 平台/超大库边界不受影响(仍由 canvasCapable 在消费方回退 DOM)。
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
