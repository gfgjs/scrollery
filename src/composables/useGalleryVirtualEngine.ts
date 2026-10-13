// src/composables/useGalleryVirtualEngine.ts
// 画廊虚拟滚动交汇层(自 MediaGrid.vue 结构拆分抽出):bucket 分段引擎 + Canvas 渲染分支所需
// 的派生读数面。原「方案 A ↔ bucket 双引擎互斥」已随 P22 收敛为 bucket 单引擎——方案 A 的
// SAFE_MAX 平移态与 bucket 的 B3 段级映射态同构,且 bucket 额外覆盖滚轮/键盘/触摸输入分类与
// 段级取数管线,故「换引擎」不再是运行时可切状态(见 useBucketVirtualScroll 头注)。
//
// 布局源(含 watch/onBeforeUnmount)必须在组件 setup 上下文中构造。
import { computed, ref } from 'vue'
import { useUiStore } from '../stores/uiStore'
import { useMediaStore } from '../stores/mediaStore'
import { useJustifiedLayout } from './useJustifiedLayout'
import { useBucketVirtualScroll, type LayoutRestoreTarget } from './useBucketVirtualScroll'
import { useSelection } from './useSelection'
import { useRenderMode } from './useRenderMode'
import type { LayoutRow } from '../types/layout'

export interface GalleryVirtualEngineDeps {
  /** 滚动容器(模板 ref,只能声明在根组件)。 */
  gridRef: () => HTMLElement | null
  /**
   * 离屏时暂停布局取数；KeepAlive 的 watcher 仍运行，须避免旧布局污染复激活首帧。
   */
  onScreen: () => boolean
  resolveLayoutTarget: (version: number, isCurrent: () => boolean) => Promise<LayoutRestoreTarget | null>
}

export function useGalleryVirtualEngine(deps: GalleryVirtualEngineDeps) {
  const ui = useUiStore()
  const media = useMediaStore()
  const selection = useSelection()
  const { galleryRenderMode } = useRenderMode()

  // 容器内容区宽度（去 scrollbar）——布局重算的输入。
  const containerWidth = ref(0)

  // justified 与 grid 使用同一 LayoutRow 契约，共用后端布局源。
  const { compute, onResize, cancelPendingResize, requestCompute, flushIfDeferred } =
    useJustifiedLayout(() => containerWidth.value, { enabled: deps.onScreen })

  // 画布只占视口，绘制/命中均消费逻辑 y；bucket 封顶物理占位并映射逻辑滚动位，
  // 因此总高不限制 Canvas，映射态也无需逐格 DOM 镜像。
  // Canvas 的取行窗覆盖位图预取范围；模式切换立即重取，无需等待滚动。
  const canvasActive = computed(
    () => galleryRenderMode.value === 'canvas' && media.totalRows > 0,
  )

  // 滚动和 layoutVersion 变化驱动段表与取数窗口更新。
  const bucketScroll = useBucketVirtualScroll({
    totalHeight: () => media.totalHeight,
    layoutVersion: () => media.layoutVersion,
    resolveLayoutTarget: deps.resolveLayoutTarget,
    fetchBucketRows: (startY, endY) => media.fetchBucketRows(startY, endY),
    containerRef: () => deps.gridRef(),
    // 自适应段高输入:小行高→小段,遏制跨段挂载/反序列化爆帧(§滚动卡顿分析 #3)。
    rowHeight: () => ui.gridRowHeight,
    // 愿望窗口下界(仅数据面):canvas 生效时至少覆盖位图预取的 1.25 屏几何。
    canvasMode: () => canvasActive.value,
  })
  const {
    segments: bucketSegments,
    anchorDelta: bucketAnchorDelta,
    spacerHeight: bucketSpacerHeight,
    logicalScrollTop: currentLogicalY,
  } = bucketScroll

  // 当前可视行(可视项就地 patch:缩略图/收藏/评分/色标/右键查找/行高锚点)。行 y 始终为
  // 逻辑坐标,与 currentLogicalY 同源;映射态不将物理 scrollTop 当作绘制或命中坐标。
  const activeRowsRef = computed<LayoutRow[]>(() => bucketScroll.mountedRows())
  function activeRows(): LayoutRow[] {
    return activeRowsRef.value
  }

  // 就地更新后递增信号驱动 Canvas 重绘，避免深 watch 遍历整窗；DOM 由响应式更新。
  const canvasPatchTick = ref(0)
  function bumpCanvasPatchTick() {
    canvasPatchTick.value++
  }
  // 选区版本覆盖等基数的成员变化；仅监听数量会漏掉重绘。
  const canvasSelectionVersion = computed(() => selection.selectionEpoch.value)

  return {
    containerWidth,
    bucketScroll,
    bucketSegments,
    bucketAnchorDelta,
    bucketSpacerHeight,
    currentLogicalY,
    activeRows,
    activeRowsRef,
    /** Canvas 偏好与当前数据合成开关，取行窗已按此自动更新。 */
    canvasActive,
    canvasPatchTick,
    bumpCanvasPatchTick,
    canvasSelectionVersion,
    compute,
    requestCompute,
    onResize,
    cancelPendingResize,
    flushIfDeferred,
  }
}
