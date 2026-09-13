// src/utils/galleryPrefetchWindow.ts
// Canvas 位图预取窗口 ↔ 布局行数据窗口的对齐契约(方案 2026-09-12 §4.3 S3)。
//
// Canvas 只枚举「已就绪的挂载行」备图(useCanvasThumbPipeline 的 canvasPrefetchItems),
// 而两套虚拟引擎的取行边距是另一套尺度:bucket 为 min(6000, 1000 + EMA×300),方案 A
// compact 仅 ~300px/侧。高 CSS 视口(2160)下位图要 1.25 屏 = 2700px,两者都给不到。
//
// 本模块给出该几何覆盖下界(px),消费方与既有速度边距取 max,不替换既有语义。
// 屏数因子在此单点定义,避免「1.25 屏」散落多份字面量。

/// Canvas 位图预取前向屏数:开闸/关闸同值(useCanvasThumbPipeline 的两条计划同宽)。
export const CANVAS_PREFETCH_AHEAD_FACTOR = 1.25

/// Canvas 位图预取后向屏数(关闸态收缩为 0)。
export const CANVAS_PREFETCH_BEHIND_FACTOR = 0.5

/** 位图预取所需的行数据覆盖距离(px,两侧同宽给足以兜方向反转)。 */
export interface CanvasPrefetchCoverage {
  aheadPx: number
  behindPx: number
}

/**
 * 位图预取窗口所需的覆盖距离(纯函数,单测锁定)。
 *
 * 余量恰一行行高:覆盖「跨界行归其行首 y 所在段」的边缘误差,不额外撑窗。
 * `viewportH` 是 CSS 视口高,不是面板分辨率。视口 ≤ 0(未测量/KeepAlive 失活)返回 0。
 */
export function canvasPrefetchCoveragePx(
  viewportH: number,
  rowHeight: number,
): CanvasPrefetchCoverage {
  if (!(viewportH > 0)) return { aheadPx: 0, behindPx: 0 }
  const slack = rowHeight > 0 ? rowHeight : 0
  const span = (factor: number) => Math.ceil(viewportH * factor) + slack
  return {
    aheadPx: span(CANVAS_PREFETCH_AHEAD_FACTOR),
    behindPx: span(CANVAS_PREFETCH_BEHIND_FACTOR),
  }
}
