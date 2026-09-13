// src/composables/player/useVideoResume.spec.ts
// resolveResumePosition 纯函数核心行为回归钉(边界规则:<5s / ≥duration / >98%×duration 不复位;
// duration 非有限跳过)。composable 主体(事件监听/节流写)靠 DOM 事件驱动,风险集中在这条判定
// 规则上,故 spec 只钉纯函数(计划 §施工计划「恢复判定抽纯函数 + spec(核心行为须测,风险规则)」)。
import { describe, it, expect } from 'vitest'
import { resolveResumePosition } from './useVideoResume'

describe('resolveResumePosition', () => {
  it('duration 为 NaN 时跳过(直播流/未就绪)', () => {
    expect(resolveResumePosition(60_000, Number.NaN)).toBeNull()
  })

  it('duration 为 Infinity 时跳过', () => {
    expect(resolveResumePosition(60_000, Number.POSITIVE_INFINITY)).toBeNull()
  })

  it('duration 非正时跳过', () => {
    expect(resolveResumePosition(60_000, 0)).toBeNull()
  })

  it('savedMs 非有限或非正时不复位', () => {
    expect(resolveResumePosition(Number.NaN, 100)).toBeNull()
    expect(resolveResumePosition(0, 100)).toBeNull()
    expect(resolveResumePosition(-1000, 100)).toBeNull()
  })

  it('<5s 不复位(几乎未看)', () => {
    expect(resolveResumePosition(3000, 100)).toBeNull()
    expect(resolveResumePosition(4999, 100)).toBeNull()
  })

  it('恰好 5s 复位(边界:非 <5s)', () => {
    expect(resolveResumePosition(5000, 100)).toBe(5)
  })

  it('≥duration 不复位(异常记录)', () => {
    expect(resolveResumePosition(100_000, 100)).toBeNull() // ==duration
    expect(resolveResumePosition(150_000, 100)).toBeNull() // >duration
  })

  it('>98%×duration 不复位(基本看完)', () => {
    expect(resolveResumePosition(99_000, 100)).toBeNull() // 99s > 98
  })

  it('恰好 98%×duration 复位(边界:非 >98%)', () => {
    expect(resolveResumePosition(98_000, 100)).toBe(98)
  })

  it('正常区间返回秒数', () => {
    expect(resolveResumePosition(50_000, 100)).toBe(50)
  })
})
