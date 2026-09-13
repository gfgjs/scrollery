// format 单测:目前仅覆盖 formatPlayerTime(播放器线新增)
import { describe, it, expect } from 'vitest'
import { formatPlayerTime } from './format'

describe('formatPlayerTime', () => {
  it('NaN/非有限 → "--:--"', () => {
    expect(formatPlayerTime(NaN)).toBe('--:--')
    expect(formatPlayerTime(Infinity)).toBe('--:--')
    expect(formatPlayerTime(-Infinity)).toBe('--:--')
  })

  it('< 1h → m:ss', () => {
    expect(formatPlayerTime(0)).toBe('0:00')
    expect(formatPlayerTime(5)).toBe('0:05')
    expect(formatPlayerTime(65)).toBe('1:05')
    expect(formatPlayerTime(3599)).toBe('59:59')
  })

  it('>= 1h → h:mm:ss', () => {
    expect(formatPlayerTime(3600)).toBe('1:00:00')
    expect(formatPlayerTime(3661)).toBe('1:01:01')
    expect(formatPlayerTime(7325)).toBe('2:02:05')
  })

  it('opts.negative 前缀 "-"', () => {
    expect(formatPlayerTime(5, { negative: true })).toBe('-0:05')
    expect(formatPlayerTime(3661, { negative: true })).toBe('-1:01:01')
  })

  it('负数输入按绝对值处理', () => {
    expect(formatPlayerTime(-65)).toBe('1:05')
  })
})
