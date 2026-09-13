// src/types/ai.ts
// AI 模块类型定义

/** 后端返回的执行提供者 */
export type AiProvider = 'directml' | 'cuda' | 'coreml' | 'openvino' | 'cpu'

/** 来自 get_ai_status IPC 的 AI 状态摘要 */
export interface AiStatusSummary {
  provider: string
  gpuName: string
  vramGb: number | null
  batchSize: number
  /** 当前图像变体的固定 batch k（>1）；动态/单批为 null。驱动设置页 batch 最小限制 */
  activeFixedBatch: number | null
  clipLoaded: boolean
  totalItems: number
  analyzedItems: number
  pendingItems: number
  /** ai_status=Error 项数（审查 A11：失败面可见化；pending 已不含 error） */
  errorItems: number
  isAnalyzing: boolean
  /** 分析处于「期望运行」态——运行中，或暂停/中断但仍有剩余工作（问题7） */
  analysisActive: boolean
  /** 本次轮询时的让步阻塞源快照，仅 isAnalyzing 时非空（可观测性三修 #2） */
  waitingOn: string[]
}

/** 带相似度分数的语义搜索结果 */
export interface SemanticSearchResult {
  id: number
  fileName: string
  mediaType: string
  width: number
  height: number
  thumbPath: string | null
  thumbhash: number[] | null
  thumbStatus: number
  /** [0, 1] 范围内的余弦相似度 */
  similarity: number
}

/** 提供者探测结果 */
export interface AiProviderInfo {
  provider: string
  gpuName: string
  clipLoaded: boolean
}

/** 搜索模式切换 */
export type SearchMode = 'mixed' | 'semantic' | 'normal'

// ── 模型库（架构 → batch 变体）──────

/** 变体图像塔 batch 轴类型 */
export type BatchKind = 'single' | 'dynamic' | 'fixed'

/** 一个可下载的图像编码器 batch 变体 */
export interface ModelVariant {
  /** 图像 onnx 文件名，亦为下载/切换标识 */
  imageFile: string
  batchKind: BatchKind
  /** 固定 batch k（仅 fixed 时非空） */
  fixedBatch: number | null
  /** 该变体总字节数（image+extra+text+vocab）；未知/离线时为 0 */
  sizeBytes: number
  installed: boolean
  active: boolean
}

/** 一个架构分组（= 仓库一个文件夹） */
export interface ModelArch {
  /** 稳定架构 id = 向量空间主键 */
  id: string
  displayName: string
  description: string
  imageSize: number
  embedDim: number
  sizeMb: number
  fp16: boolean
  active: boolean
  variants: ModelVariant[]
}

/** list_model_registry 的返回 */
export interface ModelRegistry {
  archs: ModelArch[]
  activeArchId: string
  activeImageFile: string
  /** 在线发现失败（离线回退）时为 false */
  online: boolean
}

/** download_model 流式下载进度 */
export interface ModelDownloadProgress {
  modelId: string
  currentFile: string
  fileIndex: number
  fileCount: number
  received: number
  total: number
  done: boolean
  error: string | null
}
