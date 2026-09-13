// 应用程序默认值

export const THUMB_SIZE_TIERS = [64, 128, 256, 512, 1024] as const
export type ThumbSizeTier = (typeof THUMB_SIZE_TIERS)[number]

export const DEFAULTS = {
  THUMB_SIZE: 512,
  THUMB_SKIP_MAX_KB: 200,
  THUMB_QUALITY: 80,
  THUMB_FORMAT: 'webp',
  THUMB_STRATEGY: 'cpu',
  GPU_ENGINE: 'wic',
  SIDEBAR_WIDTH: 260,
  GRID_ROW_HEIGHT: 200,
  GRID_GAP: 4,
  // 配置重构批次C(2026-07-22)修正:此前 150 与 AppToolbar.vue 混合/语义搜索防抖的实际硬编码值
  // (500ms)长期不一致——本常量在被 uiStore.searchDebounceMs 接线消费前从未被任何调用点真正
  // 读取过,是摆设值;改为与既有真实行为对齐的 500。uiStore.ts 的 searchDebounceMs/
  // resizeDebounceMs 两个 ref 以本文件同名常量为初始默认值(单源,见其声明处)。
  SEARCH_DEBOUNCE_MS: 500,
  RESIZE_DEBOUNCE_MS: 300,
  // 视口上下各保留的离屏缓冲行数；据此换算自适应像素缓冲，避免极小行高时多渲染数百个单元。
  SCROLL_BUFFER_ROWS: 8,
  THUMB_BATCH_SIZE: 24,
  ENRICHMENT_BATCH: 500,
  SCAN_PROGRESS_INTERVAL: 500,
} as const

// px — 固定的 DateSeparator 行高
export const SEPARATOR_HEIGHT = 36
