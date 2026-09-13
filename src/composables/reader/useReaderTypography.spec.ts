// src/composables/reader/useReaderTypography.spec.ts
// characterization:verticalChanged 判定 → requestRemount(true) 调用(方案 §5 风险面 3,红线§3风险2:
// 仅有的两条 capture-first remount 路径之一，本文件只可调用 deps.requestRemount(true)，不得自写
// capture+bump)。断言 requestRemount 收到的实参恰为 true(captureFirst)。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { effectScope, ref } from 'vue'

const { invokeIpc } = vi.hoisted(() => ({
  invokeIpc: vi.fn((..._args: unknown[]) => Promise.resolve(null)),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc }))
vi.mock('../../constants/ipc', () => ({
  IPC: { GET_APP_CONFIG: 'get_app_config', SET_APP_CONFIG: 'set_app_config' },
}))
// 主题归一化换恒等最小 fake:不关心具体别名表,只需两端可比较。
vi.mock('../../themes/readerThemes', () => ({
  normalizeReaderThemeId: (v: string) => v,
}))

import { useReaderTypography, type TypographyChangePayload } from './useReaderTypography'

function basePayload(overrides: Partial<TypographyChangePayload> = {}): TypographyChangePayload {
  return {
    fontSizePx: 19,
    lineHeight: 1.75,
    fontFamily: 'serif',
    maxInlineSizePx: 720,
    pageTurn: 'slide',
    autoScrollSec: 0,
    readerThemeLight: 'light',
    readerThemeDark: 'dark',
    fontWeight: 400,
    letterSpacingEm: 0,
    textAlign: 'justify',
    titleScale: 1,
    paragraphSpacingEm: 0.4,
    vertical: 'off',
    ...overrides,
  }
}

function withTypography() {
  const requestRemount = vi.fn()
  const deps = {
    readerAutoScrollSec: ref(0),
    readerThemeLight: ref('light'),
    readerThemeDark: ref('dark'),
    requestRemount,
  }
  const scope = effectScope()
  const api = scope.run(() => useReaderTypography(deps))!
  return { api, requestRemount, scope }
}

describe('useReaderTypography:verticalChanged → requestRemount(true)(characterization)', () => {
  beforeEach(() => {
    invokeIpc.mockClear()
  })

  it('vertical 从 off 变为 vertical-rl:调用 requestRemount,实参 captureFirst=true', () => {
    const { api, requestRemount, scope } = withTypography()
    api.onTypographyChange(basePayload({ vertical: 'vertical-rl' }))
    expect(requestRemount).toHaveBeenCalledTimes(1)
    expect(requestRemount).toHaveBeenCalledWith(true)
    scope.stop()
  })

  it('vertical 不变(仍为 off):不调用 requestRemount', () => {
    const { api, requestRemount, scope } = withTypography()
    api.onTypographyChange(basePayload({ vertical: 'off', fontSizePx: 22 }))
    expect(requestRemount).not.toHaveBeenCalled()
    scope.stop()
  })

  it('其余非 vertical 字段变化(如 fontSizePx)不触发 requestRemount,只落 ref', () => {
    const { api, requestRemount, scope } = withTypography()
    api.onTypographyChange(basePayload({ fontSizePx: 24, lineHeight: 2 }))
    expect(requestRemount).not.toHaveBeenCalled()
    expect(api.readerFontSize.value).toBe(24)
    expect(api.readerLineHeight.value).toBe(2)
    scope.stop()
  })

  it('readerTypography computed 字段名与 BookReader :typography prop 契约一致(红线§3风险1)', () => {
    const { api, scope } = withTypography()
    expect(Object.keys(api.readerTypography.value).sort()).toEqual(
      [
        'fontSizePx',
        'lineHeight',
        'fontFamily',
        'maxInlineSizePx',
        'fontWeight',
        'letterSpacingEm',
        'textAlign',
        'titleScale',
        'paragraphSpacingEm',
      ].sort(),
    )
    scope.stop()
  })

  it('只写真正变化的键:全字段与初始值相同时,不产生任何 SET_APP_CONFIG 写(防冗余 IPC)', () => {
    const { api, scope } = withTypography()
    invokeIpc.mockClear()
    api.onTypographyChange(basePayload()) // 与初始 refs 完全一致
    const setCalls = invokeIpc.mock.calls.filter((c) => c[0] === 'set_app_config')
    expect(setCalls.length).toBe(0)
    scope.stop()
  })
})
