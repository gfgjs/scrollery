import { describe, expect, it } from 'vitest'
import { enhanceSelectionBlock } from './enhanceCapability'
import type { EnhanceReadiness, EnhanceStatus } from '../types/enhance'

function status(readiness: EnhanceReadiness): EnhanceStatus {
  return {
    availability: 'authorized',
    workerReady: true,
    storeUrl: null,
    provider: null,
    models: [{ id: 'scunet', task: 'denoise', scale: 1, installed: readiness === 'ready',
      manifestReady: readiness !== 'manifestUnready', canDownload: readiness !== 'manifestUnready', readiness }],
  }
}

describe('增强预览与提交门控', () => {
  it.each([
    ['ready', null],
    ['manifestUnready', 'enhance_manifest_unready'],
    ['unlicensed', 'enhance_unlicensed'],
    ['modelMissing', 'enhance_model_missing'],
  ] as const)('保留后端 %s 判定', (readiness, code) => {
    expect(enhanceSelectionBlock(status(readiness), ['scunet'])).toBe(code)
  })

  it('没有状态时不开放执行', () => {
    expect(enhanceSelectionBlock(null, ['scunet'])).toBe('enhance_status_unavailable')
  })

})
