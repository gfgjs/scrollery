// 画廊双引擎交汇层(自 MediaGrid.vue 结构拆分抽出):方案 A 虚拟滚动 ↔ T16 方案B bucket 分段,
// 加 Canvas 渲染分支所需的派生读数面。**只换文件,不换语义**。
//
// 🔴 红线:
//  - `bucketActive` 全场唯一实例——两引擎的 enabled() 必须引用同一个 computed,否则可能出现
//    一帧内两套引擎同时 enabled 的竞态;
//  - 引擎切换 watcher 内 `await nextTick()` 的相对位置一个字符都不能挪(见该 watcher 头注);
//  - canvasCapable 的 iOS + SAFE_MAX 判据与画廊轴 time 线性映射(9cd4bfb / R-5)共享同一
//    「总高是否进映射态」语义,不得在结构拆分时顺手统一。
import { computed, nextTick, ref, watch } from 'vue'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { useGalleryLayoutSource } from './galleryLayoutSource'
import { useVirtualScroll, resolveSafeMax } from './useVirtualScroll'
import { useBucketVirtualScroll } from './useBucketVirtualScroll'
import { useSelection } from './useSelection'
import { useRenderMode } from './useRenderMode'
import type { LayoutRow } from '../types/layout'

export interface GalleryVirtualEngineDeps {
  /** 滚动容器(模板 ref,只能声明在根组件)。 */
  gridRef: () => HTMLElement | null
  /** 方案 A 的渲染层(模板 ref)。 */
  layerRef: () => HTMLElement | null
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

  // 容器内容区宽度（去 scrollbar）——布局重算的输入；在策略源之前声明，惰性 getter 透传。
  const containerWidth = ref(0)

  // 布局策略源（A1，见 docs/refactor_2026/T20_T18-layout_布局策略接缝_合并设计.md）：
  // 当前仅 justified 一种策略。useVirtualScroll 只依赖 source 的 totalHeight/totalRows/
  // fetchRowsByY 抽象契约，与具体布局算法解耦——A2 将新增 grid 策略实现同一接口后由此切换。
  const layoutSource = useGalleryLayoutSource(() => containerWidth.value, {
    enabled: deps.onScreen,
  })

  // ── Canvas 渲染模式判据(§9 T4 最简原型,与 DOM 一键切换对比)─────────────────────
  // canvas 混合网格能力门控(§9.4 + §9「转正」):
  //  - iOS/WKWebView 强制 DOM(§2.3 单页 GPU 内存 ~1.4-1.5GB 上限 + 降帧的反向劣化风险;
  //    iOS 尚未出包,此为前瞻实现,真机不可证——标 ⏸iOS)。
  //  - 仅线性坐标区(总高 ≤ SAFE_MAX)可用:镜像命中格按逻辑坐标定位于高占位 wrap,>SAFE_MAX 的
  //    映射/平移态压缩物理坐标会令镜像与 sticky canvas 错位,故超限自动回退 DOM(多百万级超大库,罕见)。
  // 判据块声明在两引擎之前:引擎构造时会同步读 canvasMode 求首帧取行边距(§4.3 S3),晚声明会
  // 撞上暂时性死区。
  const isIOSLike =
    /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  const canvasCapable = computed(() => !isIOSLike && media.totalHeight <= resolveSafeMax())

  // Canvas 生效开关(§4.3 S3):两引擎据此把取行窗抬到位图预取几何之上。运行时切换由各自
  // 的 canvasMode watch 立即按新窗重取,不等下一次滚动。
  const canvasActive = computed(
    () => galleryRenderMode.value === 'canvas' && media.totalRows > 0 && canvasCapable.value,
  )

  // ── T16 方案B:bucket 分段引擎适用域(声明须先于两引擎构造——useVirtualScroll 内部
  // watch(opts.enabled) 构造时即求值)──────────────────────────────────────────────
  // B1.5 等高算术分段与分组语义无关 → 三种分组(date/folder/none)统一覆盖;B3 段级坐标
  // 映射落地后**无总高上限**(>16M 进映射态:spacer 封顶、局部 1:1 + 低频重锚,见
  // useBucketVirtualScroll 头注)——bucketActive 仅由开关与非空视图决定。两引擎常驻
  // 实例化、各以 enabled() 休眠/接管,运行时即切即生效(滚动位保持见下方 watch)。
  const bucketActive = computed(() => ui.bucketSegmentedScroll && media.totalRows > 0)

  const {
    visibleRows,
    updateVisible,
    onScroll,
    spacerHeight,
    renderAnchor,
    logicalScrollTop,
    logicalToPhysical,
  } = useVirtualScroll({
    totalHeight: layoutSource.totalHeight,
    totalRows: layoutSource.totalRows,
    fetchRowsByY: layoutSource.fetchRowsByY,
    containerRef: () => deps.gridRef(),
    layerRef: () => deps.layerRef(),
    rowHeight: () => ui.gridRowHeight,
    // 方案 A 与 bucket 引擎互斥:bucket 接管时本引擎休眠(不取数、卸 wheel 补偿)。
    enabled: () => !bucketActive.value,
    // 取行窗下界(仅数据面):canvas 生效时抬到覆盖位图预取几何;DOM 路径按既有缓冲语义。
    canvasMode: () => canvasActive.value,
  })

  // bucket 分段引擎(T16 方案B B1.5):愿望窗口/取数自驱于 onScroll 算术同步与
  // layoutVersion watch,宿主的各处 compute()+updateVisible() 调用无需逐一适配——
  // compute 换 layoutVersion 即触发段表重建。
  const bucketScroll = useBucketVirtualScroll({
    enabled: () => bucketActive.value,
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
  } = bucketScroll

  // 双引擎统一读数面:逻辑滚动位(scrubber 高亮/分隔符联动)与「当前可视行」(可视项
  // 就地 patch:缩略图/收藏/评分/色标/右键查找/行高锚点)。bucket 模式零映射,
  // 物理 scrollTop 即逻辑 y。
  const currentLogicalY = computed(() =>
    bucketActive.value ? bucketScroll.logicalScrollTop.value : logicalScrollTop.value,
  )

  function activeRows(): LayoutRow[] {
    return bucketActive.value ? bucketScroll.mountedRows() : visibleRows.value
  }

  const canvasRows = computed<LayoutRow[]>(() =>
    bucketActive.value ? bucketScroll.mountedRows() : visibleRows.value,
  )
  // 就地 patch 信号(#15):乐观更新(收藏/评分/色标/缩略图回写)改行 item 字段后递增,
  // 通知 canvas 重绘——替代其原 rows 深 watch(bucket 段边界帧全窗深遍历,周期卡顿主因)。
  // DOM 模式不消费(深响应式自触发);恒 bump 无副作用(canvas 未挂载即无 watcher)。
  const canvasPatchTick = ref(0)
  function bumpCanvasPatchTick() {
    canvasPatchTick.value++
  }
  const canvasSpacerHeight = computed(() =>
    bucketActive.value ? bucketSpacerHeight.value : spacerHeight.value,
  )
  // 用 selectionEpoch 而非 selectedCount(2026-07-10 审查 B10):prop 契约=「任何选区变化时递增」,
  // count 别名在等基数成员变化(扣除框选滑动/反选恰半)下不变,曾致 canvas 漏重绘。
  const canvasSelectionVersion = computed(() => selection.selectionEpoch.value)

  // 引擎切换(一键开关/空视图翻转)时保持逻辑滚动位:两引擎的物理 scrollTop 语义不同
  // (bucket 经 scrollToLogicalY 统一入口——B3 映射态的重锚自处理;方案 A 平移模式 =
  // 压缩坐标)。切换瞬间读「离开方」的逻辑位,等 DOM(容器内容高度)换代后按「进入方」
  // 语义回设。方案 A 的重取由其 enabled watch 自触发(scheduleUpdate(true) 的 rAF 晚于
  // 本 nextTick,读到的已是回设后的 scrollTop)。
  watch(bucketActive, async (nowBucket) => {
    const el = deps.gridRef()
    if (!el) return
    const logicalY = nowBucket ? logicalScrollTop.value : bucketScroll.logicalScrollTop.value
    await nextTick()
    if (nowBucket) {
      await bucketScroll.scrollToLogicalY(Math.max(0, logicalY))
    } else {
      el.scrollTop = logicalToPhysical(logicalY)
    }
  })

  // `y` 逻辑坐标。smooth 由调用方定:时间轴/滚动条**指针拖拽 = false 即时落点**(跟手,VSCode
  // minimap 手感),单击导航 / 键盘步进 = true 平滑。默认 true(点击语义)。
  function scrollToY(y: number, smooth = true) {
    if (bucketActive.value) {
      void bucketScroll.scrollToLogicalY(y, { smooth })
      return
    }
    const el = deps.gridRef()
    if (el) {
      el.scrollTo({ top: logicalToPhysical(y), behavior: smooth ? 'smooth' : 'auto' })
    }
  }

  // compute / onResize 由布局策略源提供（A1 接缝）；当前为 justified 策略。
  const { recompute: compute, requestCompute, onResize, cancelPendingResize } = layoutSource

  return {
    containerWidth,
    layoutSource,
    bucketActive,
    bucketScroll,
    visibleRows,
    updateVisible,
    onScroll,
    spacerHeight,
    renderAnchor,
    logicalToPhysical,
    bucketSegments,
    bucketAnchorDelta,
    bucketSpacerHeight,
    currentLogicalY,
    activeRows,
    canvasCapable,
    /// Canvas 生效开关(canvasCapable 与渲染偏好的合成判据):供宿主显式接入或断言;
    /// 两引擎的取行窗已按它自驱,宿主无需再透传。
    canvasActive,
    canvasRows,
    canvasPatchTick,
    bumpCanvasPatchTick,
    canvasSpacerHeight,
    canvasSelectionVersion,
    compute,
    requestCompute,
    onResize,
    cancelPendingResize,
    scrollToY,
  }
}
