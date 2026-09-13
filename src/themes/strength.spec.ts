// 浓度工具单测(2026-09-06):clamp 收敛外部输入 + CSS 变量写点。
// 与 utils/uiScale.spec.ts 的玻璃缩放测试同型——两侧都是「配置文件是外部输入」的收敛面。
// 底色(主题色)与文字两套浓度同构,合并在本文件覆盖。
import { describe, it, expect, vi, afterEach } from 'vitest'
import {
  applyThemeTextStrength,
  applyThemeTintStrength,
  clampThemeText,
  clampThemeTint,
  THEME_TEXT_DEFAULT,
  THEME_TINT_DEFAULT,
} from './strength'

describe('主题色浓度(底色)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('默认 60,clamp 收敛到 0–100,非有限数回默认', () => {
    expect(THEME_TINT_DEFAULT).toBe(60)
    expect(clampThemeTint(60)).toBe(60)
    expect(clampThemeTint(-5)).toBe(0)
    expect(clampThemeTint(0)).toBe(0)
    expect(clampThemeTint(5)).toBe(5)
    expect(clampThemeTint(150)).toBe(100)
    expect(clampThemeTint(NaN)).toBe(60)
    expect(clampThemeTint(60.4)).toBe(60)
  })

  it('applyThemeTintStrength 以比例写 --theme-tint-scale,非有限数不写', () => {
    const setProperty = vi.fn()
    vi.stubGlobal('document', { documentElement: { style: { setProperty } } })
    applyThemeTintStrength(80)
    expect(setProperty).toHaveBeenCalledWith('--theme-tint-scale', '0.8')
    applyThemeTintStrength(Number.NaN)
    expect(setProperty).toHaveBeenCalledTimes(1)
  })
})

describe('文字浓度', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('默认 100,clamp 收敛到 40–100,非有限数回默认', () => {
    expect(THEME_TEXT_DEFAULT).toBe(100)
    expect(clampThemeText(100)).toBe(100)
    expect(clampThemeText(10)).toBe(40)
    expect(clampThemeText(150)).toBe(100)
    expect(clampThemeText(NaN)).toBe(100)
    expect(clampThemeText(75.4)).toBe(75)
  })

  it('applyThemeTextStrength 以比例写 --theme-text-scale,非有限数不写', () => {
    const setProperty = vi.fn()
    vi.stubGlobal('document', { documentElement: { style: { setProperty } } })
    applyThemeTextStrength(60)
    expect(setProperty).toHaveBeenCalledWith('--theme-text-scale', '0.6')
    applyThemeTextStrength(Number.NaN)
    expect(setProperty).toHaveBeenCalledTimes(1)
  })
})
