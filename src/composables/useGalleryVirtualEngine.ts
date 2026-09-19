// src/composables/useGalleryVirtualEngine.ts
// 画廊虚拟滚动交汇层(自 MediaGrid.vue 结构拆分抽出):bucket 分段引擎 + Canvas 渲染分支所需
// 的派生读数面。原「方案 A ↔ bucket 双引擎互斥」已随 P22 收敛为 bucket 单引擎——方案 A 的
// SAFE_MAX 平移态与 bucket 的 B3 段级映射态同构,且 bucket 额外覆盖滚轮/键盘/触摸输入分类与
// 段级取数管线,故「换引擎」不再是运行时可切状态(见 useBucketVirtualScroll 头注)。
//
// 🔴 红线:
//  - canvasCapable 的 iOS + 超大库判据与画廊轴 time 线性映射(9cd4bfb / R-5)共享同一
//    「总高是否进映射态」语义,不得顺手统一;
//  - 布局源(含 watch/onBeforeUnmount)必须在组件 setup 上下文中构造。
import { computed, ref } from 'vue'
import { useUiStore } from '../stores/uiStore'
import { useMediaStore } from '../stores/mediaStore'
import { useJustifiedLayout } from './useJustifiedLayout'
import { useBucketVirtualScroll } from './useBucketVirtualScroll'
import { useSelection } from './useSelection'
import { useRenderMode } from './useRenderMode'
import type { LayoutRow } from '../types/layout'

/**
 * Canvas 混合网格的总高能力上限(px)。超过此总高的库强制回退 DOM:镜像命中格按逻辑坐标定位于
 * 高占位 wrap,物理坐标被压缩会令镜像与 sticky canvas 错位(多百万级超大库,罕见)。
 *
 * 数值沿自原方案 A 的 SAFE_MAX 默认;判据是 Canvas 自身的能力边界,与虚拟滚动引擎无关,故随
 * 「删旧引擎的 debug override」收敛为引擎本地常量(不再有 localStorage 覆盖缝)。
 */
export const CANVAS_MAX_TOTAL_HEIGHT = 10_000_000

export interface GalleryVirtualEngineDeps {
  /** 滚动容器(模板 ref,只能声明在根组件)。 */
  gridRef: () => HTMLElement | null
  /**
   * KeepAlive 在屏标志:驱动布局源的取数闸门。**不在屏上就不取数**——失活期组件仍活着、watcher 照常跑
   * (有意为之),但 compute_layout 是全库量级 IPC,离屏时跑它既白费又会用陈旧布局污染下次激活的首帧
   * (真机 round10 #5,详见 useJustifiedLayout 的 enabled 注释)。
   */
  onScreen: () => boolean
}

export function useGalleryVirtualEngine(deps: GalleryVirtualEngineDeps) {
  const ui = useUiStore()
  const media = useMediaStore()
  const selection = useSelection()
  const { galleryRenderMode } = useRenderMode()

  // 容器内容区宽度（去 scrollbar）——布局重算的输入。
  const containerWidth = ref(0)

  // 布局源(T18/T20 接缝内联):justified 与 grid 都由后端产出兼容 LayoutRow(前端几何/行供给
  // 对两模式完全相同),故无需前端 source 分支——原先经 galleryLayoutSource 再转发一层,现直接装配。
  const { compute, onResize, cancelPendingResize, requestCompute, flushIfDeferred } =
    useJustifiedLayout(() => containerWidth.value, { enabled: deps.onScreen })

  // ── Canvas 渲染模式判据(§9 T4 最简原型,与 DOM 一键切换对比)─────────────────────
  //  - iOS/WKWebView 强制 DOM(§2.3 单页 GPU 内存 ~1.4-1.5GB 上限 + 降帧的反向劣化风险;
  //    iOS 尚未出包,此为前瞻实现,真机不可证——标 ⏸iOS)。
  //  - 仅线性坐标区(总高 ≤ CANVAS_MAX_TOTAL_HEIGHT)可用,超限自动回退 DOM。
  const isIOSLike =
    /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  const canvasCapable = computed(() => !isIOSLike && media.totalHeight <= CANVAS_MAX_TOTAL_HEIGHT)

  // Canvas 生效开关(§4.3 S3):引擎据此把取行窗抬到位图预取几何之上。运行时切换由引擎的
  // canvasMode watch 立即按新窗重取,不等下一次滚动。
  const canvasActive = computed(
    () => galleryRenderMode.value === 'canvas' && media.totalRows > 0 && canvasCapable.value,
  )

  // bucket 分段引擎(T16 方案B B1.5):愿望窗口/取数自驱于 onScroll 算术同步与 layoutVersion watch,
  // 宿主的各处 compute() 调用无需逐一适配——compute 换 layoutVersion 即触发段表重建。
  const bucketScroll = useBucketVirtualScroll({
    totalHeight: () => media.totalHeight,
    layoutVersion: () => media.layoutVersion,
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

  // 当前可视行(可视项就地 patch:缩略图/收藏/评分/色标/右键查找/行高锚点)。bucket 零映射,
  // 物理 scrollTop 即逻辑 y。
  const activeRowsRef = computed<LayoutRow[]>(() => bucketScroll.mountedRows())
  function activeRows(): LayoutRow[] {
    return activeRowsRef.value
  }

  // 就地 patch 信号(#15):乐观更新(收藏/评分/色标/缩略图回写)改行 item 字段后递增,通知 canvas
  // 重绘——替代其原 rows 深 watch(段边界帧全窗深遍历,周期卡顿主因)。DOM 模式不消费(深响应式
  // 自触发);恒 bump 无副作用(canvas 未挂载即无 watcher)。
  const canvasPatchTick = ref(0)
  function bumpCanvasPatchTick() {
    canvasPatchTick.value++
  }
  // 用 selectionEpoch 而非 selectedCount(2026-07-10 审查 B10):prop 契约=「任何选区变化时递增」,
  // count 别名在等基数成员变化(扣除框选滑动/反选恰半)下不变,曾致 canvas 漏重绘。
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
    canvasCapable,
    /// Canvas 生效开关(canvasCapable 与渲染偏好的合成判据):供宿主显式接入或断言;
    /// 引擎的取行窗已按它自驱,宿主无需再透传。
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
