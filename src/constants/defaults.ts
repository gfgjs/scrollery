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
  GALLERY_BLEND_ENABLED: false,
  GALLERY_BLEND_EDGE_WIDTH: 2,
  GALLERY_BLEND_BLUR_RADIUS: 3,
  GALLERY_BLEND_STRENGTH: 100,
  // 搜索防抖默认值与 uiStore 的初始值同源。
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
