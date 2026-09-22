import type { EnhanceErrorCode, EnhanceReadiness, EnhanceStatus } from '../types/enhance'

const BLOCK_CODE: Record<Exclude<EnhanceReadiness, 'ready'>, EnhanceErrorCode> = {
  workerMissing: 'enhance_worker_missing',
  manifestUnready: 'enhance_manifest_unready',
  unlicensed: 'enhance_unlicensed',
  modelMissing: 'enhance_model_missing',
}

/** 预览和增强消费同一后端判定；状态查询失败时禁止使用旧的可用快照。 */
export function enhanceSelectionBlock(
  status: EnhanceStatus | null,
  modelIds: string[],
): EnhanceErrorCode | null {
  if (!status) return 'enhance_status_unavailable'
  if (!status.workerReady) return 'enhance_worker_missing'
  for (const id of modelIds) {
    const model = status.models.find((entry) => entry.id === id)
    if (!model) return 'enhance_invalid_params'
    if (model.readiness !== 'ready') return BLOCK_CODE[model.readiness]
  }
  return null
}
