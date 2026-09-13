// src/types/enhance.ts
// 影像增强类型定义（降噪/超分子系统 P0 批 5，消费 enhance_commands.rs 七命令）。
// 每个类型与 Rust serde 形态严格对齐——见类型头注标注的后端出处与 rename 规则。
//
// ⚠ 两处 serde 口径差异（load-bearing，勿抹平）：
//   1. 模型列表 task（EnhanceModelDto.task = EnhanceTaskKind，camelCase）→ 'dejpegArtifact'；
//      而执行链 step task（协议 EnhanceStep.task = EnhanceTask，snake_case）→ 'dejpeg_artifact'。
//   2. EnhanceParams 本体 rename_all=camelCase → outputFormat；但内嵌 EnhanceStep 无 rename_all，
//      字段原样序列化 → 必须发 `model_id`（下划线）而非 `modelId`。

import type { Availability } from './exotic'
import type { ModelDownloadProgress } from './ai'

// 复用既有下载进度类型：download_enhance_model 的 Channel<DownloadProgress>（model_download.rs，
// serde camelCase）与 CLIP/OCR 下载进度同形，直接复用避免重复定义。
export type { ModelDownloadProgress }

/** 增强任务种类——模型列表口径（enhance_status → EnhanceModelDto.task，serde camelCase）。 */
export type EnhanceModelTask = 'denoise' | 'dejpegArtifact' | 'upscale'

/**
 * 增强任务种类——协议 step 口径（EnhanceStep.task = exotic_protocol::EnhanceTask，serde snake_case）。
 * 去伪影这里是 'dejpeg_artifact'（下划线），与 EnhanceModelTask 的 'dejpegArtifact' 不同。
 */
export type EnhanceStepTask = 'denoise' | 'dejpeg_artifact' | 'upscale'

/** 输出格式选择（EnhanceParams.output_format = OutputFormatChoice，serde snake_case）。 */
export type EnhanceOutputFormat = 'follow_source' | 'jpeg' | 'png'

/** 单个增强模型档（enhance_status 返回，serde camelCase EnhanceModelDto）。 */
export interface EnhanceModelInfo {
  id: string
  task: EnhanceModelTask
  /** 输出/输入尺度比：超分模型为 4，降噪/去伪影为 1。 */
  scale: number
  installed: boolean
  /** 该档下载清单是否就绪（URL 待回填时为 false）。 */
  manifestReady: boolean
}

/** enhance_status 返回（EnhanceStatusDto，serde camelCase）。 */
export interface EnhanceStatus {
  availability: Availability
  storeUrl: string | null
  /** 当前推理执行提供器回声（worker 会话未起时为 null）。 */
  provider: string | null
  models: EnhanceModelInfo[]
}

/**
 * 执行链单步（发往 enhance_start / enhance_preview 的 EnhanceParams.steps[]）。
 * ⚠ 字段名 model_id 为 snake_case：协议 EnhanceStep 无 rename_all，字段原样反序列化。
 */
export interface EnhanceStepInput {
  task: EnhanceStepTask
  model_id: string
  /** 原生量纲强度（DRUNet σ / FBCNN QF）；缺省 = 用模型默认参数。 */
  strength?: number
}

/**
 * 一次增强提交参数（EnhanceParams）。本体 serde camelCase → outputFormat；
 * steps 内 model_id 仍 snake_case（见文件头注差异 2）。
 */
export interface EnhanceParams {
  steps: EnhanceStepInput[]
  outputFormat: EnhanceOutputFormat
}

/** job 生命周期状态（JobStatus，serde camelCase）。 */
export type EnhanceJobStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled'

/** 队列内单 job 前端投影（JobDto，serde camelCase）。 */
export interface EnhanceJob {
  id: number
  status: EnhanceJobStatus
  /** 本 job 的源 item 总数。 */
  total: number
  /** 已完成 item 数。 */
  done: number
  /** 累计已完成 tile 数（per-tile Progress 心跳）。 */
  tileDone: number
  /**
   * 入队时估算的总 tile 数（批 5.5：与 worker `chain.rs` 同源几何估算，见 service.rs
   * `estimate_tiles_total`）。0 = 估算不可用（item 尺寸未知）——UI 回退不定长进度态。
   */
  tilesTotal: number
  errorCode: string | null
}

/**
 * 前后对比预览结果（enhance_preview 实现后返回 { beforePath, afterPath }）。
 * P0 后端返回 enhance_not_implemented 占位码，本类型供 4.5 批实现后接线。
 */
export interface EnhancePreviewResult {
  beforePath: string
  afterPath: string
}

/**
 * EnhanceDialog 的源信息（由入口方——查看器/画廊——构造传入）。
 * itemIds 支持多选（P0 顺序入队）；ext/width/height 用于「自动」建议 + 预计输出尺寸，
 * 拿不到（无元数据）时对应建议/尺寸估算跳过，不阻塞。
 */
export interface EnhanceSource {
  itemIds: number[]
  fileName: string
  /** 源扩展名（小写，不含点），如 'jpg'。 */
  ext?: string
  width?: number
  height?: number
}

/** 稳定错误码联合（enhance_commands.rs / service.rs 的 AppError::Enhance code）。 */
export type EnhanceErrorCode =
  | 'enhance_input_too_large'
  | 'enhance_unlicensed'
  | 'enhance_input_unsupported'
  | 'enhance_model_missing'
  | 'enhance_busy'
  | 'enhance_download_failed'
  | 'enhance_io'
  | 'enhance_invalid_params'
  | 'enhance_not_implemented'
