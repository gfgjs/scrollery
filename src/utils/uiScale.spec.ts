import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { applyUiFontSize } from './uiScale'

describe('applyUiFontSize', () => {
  const setProperty = vi.fn()

  beforeEach(() => {
    setProperty.mockReset()
    vi.stubGlobal('document', {
      documentElement: {
        style: { setProperty },
      },
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('以暗房方案的 13px 基准生成完整字号阶梯', () => {
    applyUiFontSize(13)

    expect(setProperty.mock.calls).toEqual([
      ['--font-size-xs', '11px'],
      ['--font-size-sm', '12px'],
      ['--font-size-base', '13px'],
      ['--font-size-md', '14px'],
      ['--font-size-lg', '16px'],
      ['--font-size-xl', '20px'],
      ['--font-size-2xl', '24px'],
    ])
  })

  it('忽略非有限字号，不污染全局变量', () => {
    applyUiFontSize(Number.NaN)

    expect(setProperty).not.toHaveBeenCalled()
  })
})
