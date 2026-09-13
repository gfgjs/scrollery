// 画廊首屏骨架屏(S5)可见性防抖 + 铺满视口(自 MediaGrid.vue 结构拆分抽出,逻辑逐字保留)。
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { DEFAULTS } from '../constants/defaults'

export interface GallerySkeletonDeps {
  /** 容器内容区宽度(去 scrollbar)。 */
  containerWidth: () => number
  /** 画廊视口高(内容盒)。 */
  viewportHeight: () => number
}

export function useGallerySkeleton(deps: GallerySkeletonDeps) {
  const media = useMediaStore()
  const ui = useUiStore()

  // 防抖:相册切换到空相册时 compute_layout 极快返回,骨架若无条件即显会「闪一下 12 张占位
  // 又转空态」的观感。仅当计算持续超过阈值(真有内容要落位、慢首屏)才显骨架;快速空结果
  // 直接进空态,不闪。isComputingLayout 转 false 时立即撤骨架。
  const SKELETON_DELAY_MS = 220
  const showSkeleton = ref(false)
  let skeletonTimer: ReturnType<typeof setTimeout> | null = null
  watch(
    () => media.isComputingLayout,
    (computing) => {
      if (skeletonTimer) {
        clearTimeout(skeletonTimer)
        skeletonTimer = null
      }
      if (computing) {
        skeletonTimer = setTimeout(() => {
          // 阈值到点仍在算且无既有布局才显(与旧 totalRows===0 条件同轴,已有旧布局走
          // stale-while-revalidate 不显骨架)。
          if (media.isComputingLayout && media.totalRows === 0) showSkeleton.value = true
          skeletonTimer = null
        }, SKELETON_DELAY_MS)
      } else {
        showSkeleton.value = false
      }
    },
  )

  // 骨架格数:按视口宽高铺满画廊(旧固定 12 只填半屏)。CSS 侧 .media-grid__skeleton 高 100%
  // + overflow:hidden 裁掉溢出行,保证恰好铺满、不产生滚动。宽高未测得时回退 12。
  const skeletonCount = computed(() => {
    const w = deps.containerWidth()
    const h = deps.viewportHeight()
    if (w <= 0 || h <= 0) return 12
    const gap = DEFAULTS.GRID_GAP
    // 近似列宽:CSS 为 flex 1 1 200px / max-width 340,取 220 折中估实际列数。
    const cellW = 220
    const cellH = ui.gridRowHeight * 0.75
    const cols = Math.max(1, Math.floor((w + gap) / (cellW + gap)))
    const rows = Math.ceil((h + gap) / (cellH + gap)) + 1
    return Math.min(cols * rows, 80)
  })

  onBeforeUnmount(() => {
    // 骨架屏防抖定时器:在途卸载须掐断,回调对死组件无害但清掉零成本。
    if (skeletonTimer) clearTimeout(skeletonTimer)
  })

  return { showSkeleton, skeletonCount }
}
