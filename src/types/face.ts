// src/types/face.ts
// 人脸识别模块类型定义（F5）

/** 来自 get_face_status IPC 的人脸状态摘要 */
export interface FaceStatusSummary {
  provider: string
  gpuName: string
  /** 人脸双 session（检测器 + 嵌入器）均已加载 */
  faceLoaded: boolean
  totalItems: number
  /** 完成检测的图像数（完成或错误），非"有脸的图像数" */
  processedItems: number
  pendingItems: number
  /** 已聚类人物数（人物墙名册规模） */
  personCount: number
  /** 当前模型下检测到的人脸总数 */
  faceCount: number
  /** face_status=Error 项数（审查 A11/F9：进度含失败到 100%，error 单列不再被掩盖） */
  errorItems: number
  isAnalyzing: boolean
  /** 分析处于「期望运行」态——运行中，或暂停/中断但仍有剩余工作 */
  analysisActive: boolean
  /** 本次轮询时的让步阻塞源快照，仅 isAnalyzing 时非空（可观测性三修 #2） */
  waitingOn: string[]
}

/** 只读模型库的一条人脸模型轨（F7） */
export interface FaceModelInfo {
  id: string
  displayName: string
  description: string
  detector: string
  embedder: string
  embedDim: number
  /** 是否允许商用（false = 仅限研究，如 InsightFace） */
  commercialOk: boolean
  license: string
  sizeMb: number
  /** 两个 onnx 文件均在磁盘上 */
  installed: boolean
  /** 当前激活轨 */
  active: boolean
  /** 有已校验清单（可一键下载）；false=仅手动导入 */
  downloadable: boolean
  /** 已与上游参考对拍;false=拒绝激活(防静默算错) */
  verified: boolean
}

/** 来自 download_face_model 的下载进度（camelCase，镜像 CLIP） */
export interface FaceModelDownloadProgress {
  modelId: string
  currentFile: string
  fileIndex: number
  fileCount: number
  received: number
  total: number
  done: boolean
  error: string | null
}
