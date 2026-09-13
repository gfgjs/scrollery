// src/stores/viewerStore.ts
// activeViewer 单一事实源 —— 补现状缺失的「当前打开资产」全局态(设计文档 §3.3 / L2)。
// 立项时(2026-07-08)图/视走 body 覆盖层(MediaDetailOverlay)、文档/音频走独立路由,两条路径
// 无贯穿的「当前查看器上下文」全局状态,媒体类型判定三层拼凑。本 store 收敛为单源,供上下文
// 工具栏(L4)与命令 when 谓词(L3)统一消费。Phase 4 已把覆盖层一并路由化(MediaDetailOverlay
// 退役 → ContentViewer),现全部查看器统一经 /view/:id 等路由 populate 到此。

import { defineStore } from 'pinia'
import { shallowRef, computed } from 'vue'
import type { MediaDetail, MediaType } from '../types/media'
import type { ViewerKind } from '../utils/viewerKind'

/**
 * 底栏文件信息(内容页动态文件信息线,2026-07-17):当前资产的逐项小标量快照。
 * rating/isFavorited/colorLabel 可交互(经 mediaStore 动作落库后由 itemPatchSignal 桥回写);
 * 其余为只读展示标量。null 载荷 = 数据未就绪/不适用(如音频页标量异步补齐前)。
 */
export interface ViewerFileInfo {
  rating: number
  isFavorited: boolean
  colorLabel: number
  fileSize: number
  /** 像素尺寸;0/缺失(音频、文档)归一为 null,展示层直接以 null 判「无此段」。 */
  width: number | null
  height: number | null
  durationMs: number | null
}

/** MediaDetail → ViewerFileInfo 单点投影(三个内容页共用,零值归一 null 只在此处做)。 */
export function toViewerFileInfo(d: MediaDetail): ViewerFileInfo {
  return {
    rating: d.rating,
    isFavorited: d.isFavorited,
    colorLabel: d.colorLabel,
    fileSize: d.fileSize,
    width: d.width > 0 ? d.width : null,
    height: d.height > 0 ? d.height : null,
    durationMs: d.durationMs != null && d.durationMs > 0 ? d.durationMs : null,
  }
}

/**
 * 查看器命令 API 契约(设计文档 §4.3)。各查看器 `defineExpose` 同形对象(按能力可选),
 * 命令层经 activeViewer.api 调用具体查看器动作。字段全部可选:某查看器只实现它支持的动作
 * (如音频无 zoom、图像无 toc)。
 */
export interface ViewerApi {
  // ── 通用导航 ──────────────────────────────────────────────
  next?: () => void
  prev?: () => void
  close?: () => void
  toggleImmersive?: () => void
  // ── 图像 / 视频 ───────────────────────────────────────────
  zoomIn?: () => void
  zoomOut?: () => void
  cycleZoomMode?: () => void
  rotate?: () => void
  toggleInfo?: () => void
  /** 打开图片简单编辑覆层(方案 C §7,仅图像)。 */
  edit?: () => void
  // ── 阅读器(BookReader 已具 next/prev/goToHref/searchBook 等)────
  toggleToc?: () => void
  toggleSearch?: () => void
  toggleBookmarks?: () => void
  toggleSettings?: () => void
  // ── 音视频 ────────────────────────────────────────────────
  togglePlay?: () => void
  seekBy?: (seconds: number) => void
  // ── 视频专属(播放器线;GD 批命令注册表经 activeViewer.api 调用)────────────
  /** 播放/暂停切换。 */
  playPause?: () => void
  /** 相对调节音量(键盘 ↑/↓ ±5%,delta ∈ [-1,1])。 */
  volumeBy?: (delta: number) => void
  /** 静音切换。 */
  toggleMute?: () => void
  /** 设置倍速(clamp [0.25,3])。 */
  setRate?: (rate: number) => void
  /** 倍速按档步进(< / >;dir=+1 提速、-1 降速)。 */
  rateStep?: (dir: 1 | -1) => void
  /** 循环切换。 */
  toggleLoop?: () => void
  /** 画中画切换。 */
  togglePip?: () => void
  /** F 全屏对(窗口全屏 + 沉浸)切换。 */
  toggleFullscreenPair?: () => void
  /** 截帧(GE 批实现)。 */
  captureFrame?: () => void
  /** 是否处于本播放器发起的全屏对(供 GD 的 Esc 链判定是否先退全屏)。 */
  isFullscreenPair?: () => boolean
}

/**
 * 当前内容查看器的上下文快照;null = 在主视图(网格)。
 * L4 上下文工具栏与 L3 命令 when 谓词均以此为准。
 */
export interface ActiveViewer {
  /** 收敛后的查看器分类(resolveViewerKind 单点推导)。 */
  kind: ViewerKind
  /** DB 主类。 */
  mediaType: MediaType
  /** 文件扩展名(不含点)。 */
  fileFormat: string
  /** 库内资产 id;外部文件(OS shell 集成)为 null。 */
  id: number | null
  /** 外部文件绝对路径(为 OS shell 集成预留;库内资产可空)。 */
  path: string | null
  /** 显示标题(文件名/书名),供顶栏与 setTitle 用。 */
  title: string
  /** 查看器暴露的命令 API;populate 时未就绪可为 null,后续 patch 补。 */
  api: ViewerApi | null
  /** 沉浸模式(隐藏 chrome)。 */
  immersive: boolean
  /** 底栏文件信息标量;null = 未就绪/不适用(音频页异步补齐前)。 */
  fileInfo: ViewerFileInfo | null
}

export const useViewerStore = defineStore('viewer', () => {
  // activeViewer 内含 api(函数引用集)+ 标量,不需要深响应式:用 shallowRef 只追踪整体替换,
  // 避免 Vue 对 api 的每个方法做无谓 proxy 追踪(项目前端规范:大对象/函数引用用 shallowRef)。
  const activeViewer = shallowRef<ActiveViewer | null>(null)

  // 时序防御(§3.3):populate 发单调递增 owner token,clear(token) 仅在 token 与当前 owner
  // 匹配时才清。路由切换/异步时序中,新查看器可能先 mount 写入状态、旧查看器后 unmount 触发
  // clear;无 token 会误清新查看器刚写入的状态。token/owner 为内部簿记,非响应式、不对外暴露。
  let tokenSeq = 0
  let ownerToken: number | null = null

  /** 当前是否处于某个内容查看器(非网格主视图)。 */
  const hasActiveViewer = computed(() => activeViewer.value !== null)
  /** 当前查看器是否处于沉浸模式(网格视图恒 false)。 */
  const isImmersive = computed(() => activeViewer.value?.immersive ?? false)

  /**
   * 查看器 mount/activate 时登记当前资产上下文,返回 owner token。
   * 调用方须持有该 token,卸载时以之调 clear(token)。
   */
  function populate(viewer: ActiveViewer): number {
    const token = ++tokenSeq
    ownerToken = token
    activeViewer.value = viewer
    return token
  }

  /**
   * 查看器卸载时清空;仅当 token 与当前 owner 匹配才生效(时序防御,见上)。
   * @param token populate 返回的 owner token
   */
  function clear(token: number): void {
    if (ownerToken === token) {
      activeViewer.value = null
      ownerToken = null
    }
  }

  /**
   * 增量更新当前查看器上下文(如 api 就绪后补写、title 变更)。shallowRef 需不可变替换才触发
   * 响应式,故整体展开重建;仅 token 匹配才更新(防旧查看器改到新状态)。
   */
  function patch(token: number, partial: Partial<ActiveViewer>): void {
    if (ownerToken === token && activeViewer.value) {
      activeViewer.value = { ...activeViewer.value, ...partial }
    }
  }

  /** 切换/设置沉浸模式。shallowRef 走不可变替换以触发响应式(见文件头 Insight)。 */
  function setImmersive(on: boolean): void {
    if (activeViewer.value && activeViewer.value.immersive !== on) {
      activeViewer.value = { ...activeViewer.value, immersive: on }
    }
  }

  /**
   * 单项标量变更回写(底栏文件信息线):mediaStore.itemPatchSignal 的桥接落点。
   * 不走 token——发起方(底栏/查看器信息面板/网格 hover 快捷评分)不是查看器本体,拿不到 owner
   * token;以 **id 匹配**守卫代替(改的不是当前项就丢弃),时序上与 token 防御互不冲突:
   * 迟到的旧项 patch 因 id 不匹配天然失效。
   */
  function applyFieldPatch(
    id: number,
    field: 'isFavorited' | 'rating' | 'colorLabel',
    value: number | boolean,
  ): void {
    const av = activeViewer.value
    if (!av || av.id !== id || !av.fileInfo) return
    const fileInfo = { ...av.fileInfo }
    if (field === 'isFavorited') fileInfo.isFavorited = value as boolean
    else if (field === 'rating') fileInfo.rating = value as number
    else fileInfo.colorLabel = value as number
    activeViewer.value = { ...av, fileInfo }
  }

  return {
    activeViewer,
    hasActiveViewer,
    isImmersive,
    populate,
    clear,
    patch,
    setImmersive,
    applyFieldPatch,
  }
})
