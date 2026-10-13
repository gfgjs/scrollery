// 画廊时间轴、minimap 与 DOM/Canvas 偏好控制。
// axisVisible 沿用 seamless_minimap 存储键以保留用户状态；轴模式按数据可用性回退。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { useRenderMode } from './useRenderMode'
import type TimelineScrubberCanvas from '../components/media/TimelineScrubberCanvas.vue'

export function useGalleryAxisControls() {
  const ui = useUiStore()
  const media = useMediaStore()
  const { t } = useI18n()

  // DOM/Canvas 对照开关仅供显式开发调试使用，避免实验状态胶囊泄漏到产品界面。
  // 开启方式：开发构建的 DevTools 执行 localStorage.setItem('scrollery.debug.renderMode', '1') 后刷新。
  const showRenderModeDebug =
    import.meta.env.DEV && localStorage.getItem('scrollery.debug.renderMode') === '1'
  // DOM↔Canvas 渲染引擎偏好(画廊 + 时间轴)提升至 useRenderMode 共享单例,与设置页「实验性」开关
  // 同源——设置改动 live 生效、无需刷新;dev 药丸(showRenderModeDebug 门控)仍可快切。
  // 渲染偏好存于 config.toml 的 gallery_render_mode / timeline_render_mode。
  const { galleryRenderMode, timelineRenderMode, setGalleryRenderMode, setTimelineRenderMode } =
    useRenderMode()
  function toggleTimelineRenderMode() {
    setTimelineRenderMode(timelineRenderMode.value === 'dom' ? 'canvas' : 'dom')
  }
  // 渲染方式切换共用 bucket 滚动与选区状态，仅改变绘制方式。
  function toggleGalleryRenderMode() {
    setGalleryRenderMode(galleryRenderMode.value === 'dom' ? 'canvas' : 'dom')
  }
  // Canvas 时间轴专属底栏钮(视觉形态/坐标系)门控:DOM 形态与 minimap 无此概念。
  // template ref 经 InstanceType 拿到 defineExpose 的方法/响应式 ref(vue 自动 unwrap)。
  const timelineCanvasRef = ref<InstanceType<typeof TimelineScrubberCanvas> | null>(null)
  // 轴控制簇 Teleport 门控之一:仅画廊视图激活期挂载底栏轴按钮(与 hostActive 对称,自持,
  // 不复用查看器的活跃态)——onActivated/onDeactivated 维护。
  const galleryViewActive = ref(false)
  // 时间轴 scrubber 是否有可展示数据(两种渲染共用同一判据,避免模板重复长条件)。
  const canShowScrubber = computed(
    () =>
      media.totalRows > 0 &&
      ((media.layoutSummary?.monthBuckets || []).length > 0 ||
        (media.layoutSummary?.separators || []).length > 0),
  )
  // 所有非空画廊都可用 minimap；存在时间数据时默认保持原时间轴，可由模式钮切换(持久化)。
  const canShowMinimap = computed(() => media.totalRows > 0)
  const activeAxis = computed(() =>
    canShowScrubber.value && ui.axisMode === 'timeline' ? 'timeline' : 'minimap',
  )
  const timelineVisible = computed(
    () => activeAxis.value === 'timeline' && canShowScrubber.value && ui.axisVisible,
  )
  const minimapVisible = computed(
    () => activeAxis.value === 'minimap' && canShowMinimap.value && ui.axisVisible,
  )
  // Canvas 时间轴专属底栏钮(视觉形态/坐标系)门控:DOM 形态/minimap 无此概念,仅 Canvas 形态显。
  const isCanvasTimeline = computed(
    () => timelineVisible.value && timelineRenderMode.value !== 'dom',
  )
  // chevron 控制轴整体显隐(持久化,两形态通用)。
  const axisOpen = computed(() => ui.axisVisible)
  function toggleAxis() {
    ui.setAxisVisible(!ui.axisVisible)
  }
  // 目录/无分组下 monthBuckets 为空,scrubber 组件内已回退成分隔符圆点——此时轴语义是
  // 「目录轴」而非时间轴,底栏文案随数据形态换叫法(判据与组件内 hasMonths 同源)。
  const hasTimeBuckets = computed(() => (media.layoutSummary?.monthBuckets || []).length > 0)
  const timelineAxisLabel = computed(() =>
    hasTimeBuckets.value ? t('toolbar.axisTimeline') : t('toolbar.axisFolders'),
  )
  const axisToggleTitle = computed(() => {
    if (activeAxis.value !== 'timeline')
      return ui.axisVisible ? t('toolbar.hideMinimap') : t('toolbar.showMinimap')
    if (!hasTimeBuckets.value)
      return ui.axisVisible ? t('toolbar.hideFolderAxis') : t('toolbar.showFolderAxis')
    return ui.axisVisible ? t('toolbar.hideTimeline') : t('toolbar.showTimeline')
  })
  const axisModeToggleTitle = computed(() =>
    activeAxis.value === 'timeline'
      ? t('toolbar.switchToMinimap')
      : hasTimeBuckets.value
        ? t('toolbar.switchToTimeline')
        : t('toolbar.switchToFolderAxis'),
  )
  function switchAxisMode(): void {
    if (activeAxis.value === 'timeline') {
      ui.setAxisMode('minimap')
      if (!ui.axisVisible) ui.setAxisVisible(true)
    } else {
      ui.setAxisMode('timeline')
      if (!ui.axisVisible) ui.setAxisVisible(true)
    }
  }

  return {
    showRenderModeDebug,
    galleryRenderMode,
    timelineRenderMode,
    toggleTimelineRenderMode,
    toggleGalleryRenderMode,
    timelineCanvasRef,
    galleryViewActive,
    canShowScrubber,
    canShowMinimap,
    activeAxis,
    timelineVisible,
    minimapVisible,
    isCanvasTimeline,
    axisOpen,
    toggleAxis,
    hasTimeBuckets,
    timelineAxisLabel,
    axisToggleTitle,
    axisModeToggleTitle,
    switchAxisMode,
  }
}
