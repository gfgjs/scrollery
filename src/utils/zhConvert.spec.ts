// zhConvert 单测(阅读器方案 R4)。vitest environment=node 无 DOM,故只覆盖两处纯函数风险点:
// 跳过标签判定(代码块保留原文)与长度守卫(防半程污染)。DOM 遍历本体为薄封装,不在此覆盖。
import { describe, it, expect } from 'vitest'
import { shouldSkipByTag, isConvertResultUsable } from './zhConvert'

describe('shouldSkipByTag', () => {
  it('跳过脚本/样式/代码/预格式/文本域(大小写不敏感)', () => {
    for (const tag of ['SCRIPT', 'style', 'Code', 'PRE', 'textarea', 'Pre']) {
      expect(shouldSkipByTag(tag)).toBe(true)
    }
  })

  it('普通排版元素不跳过', () => {
    for (const tag of ['P', 'span', 'DIV', 'H1', 'em', 'blockquote', 'li']) {
      expect(shouldSkipByTag(tag)).toBe(false)
    }
  })

  it('空/未定义标签名不跳过', () => {
    expect(shouldSkipByTag(null)).toBe(false)
    expect(shouldSkipByTag(undefined)).toBe(false)
    expect(shouldSkipByTag('')).toBe(false)
  })
})

describe('isConvertResultUsable', () => {
  it('等长数组可用(含空数组)', () => {
    expect(isConvertResultUsable(3, ['甲', '乙', '丙'])).toBe(true)
    expect(isConvertResultUsable(0, [])).toBe(true)
  })

  it('长度不等则不可用(防半程污染)', () => {
    expect(isConvertResultUsable(3, ['甲', '乙'])).toBe(false)
    expect(isConvertResultUsable(2, ['甲', '乙', '丙'])).toBe(false)
  })

  it('非数组一律不可用', () => {
    expect(isConvertResultUsable(1, null)).toBe(false)
    expect(isConvertResultUsable(1, undefined)).toBe(false)
    expect(isConvertResultUsable(1, '甲')).toBe(false)
    // 类数组对象也不认(Array.isArray 严格判定)。
    expect(isConvertResultUsable(1, { 0: '甲', length: 1 })).toBe(false)
  })
})
