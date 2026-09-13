// src/types/ocr.ts
// OCR 文字提取类型定义（B′ 路线，2026-07-23，T7）

import type { Availability } from './exotic'

/**
 * 单行 OCR 结果（后端 `OcrLineDto`，来自 `ocr_extract_image`/`ocr_extract_frame`）。
 * `quad` 为四角坐标（原图坐标系，阅读序：左上/右上/右下/左下），二期用于高亮，一期未渲染。
 */
export interface OcrLine {
  text: string
  quad: [[number, number], [number, number], [number, number], [number, number]]
  confidence: number
}

/**
 * OCR 识别结果（后端 `OcrResultDto`）：`width`/`height` 为解码图实际尺寸（quad 坐标系）。
 */
export interface OcrResult {
  lines: OcrLine[]
  width: number
  height: number
}

/** 某 OCR 档位的安装/展示信息（后端 `OcrTierDto`）。 */
export interface OcrTier {
  id: string
  displayName: string
  sizeMb: number
  installed: boolean
  /**
   * 该档位下载清单是否就绪（后端资产 URL 已钉定）；为 false 时设置页应禁用该档位的下载按钮。
   * 2026-07-23 裁决 J14：由 `OcrStatus` 顶层单值改为逐档字段（原单值取 mobile 档代表全体，
   * 日后某档下线时会误判其余档位可点）。
   */
  manifestReady: boolean
}

/**
 * OCR 门控 + 安装态（后端 `OcrStatusDto`，来自 `ocr_status`）。
 * **不含 busy 字段**——交互忙态由前端 `useOcr` 单源持有。
 */
export interface OcrStatus {
  availability: Availability
  storeUrl: string | null
  activeTier: string
  tiers: OcrTier[]
}
