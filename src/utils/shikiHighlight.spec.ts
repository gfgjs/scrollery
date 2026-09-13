// shikiHighlight 单测（阅读器方案 R4）。vitest environment=node 无 DOM 且不宜拉 shiki chunk，
// 故只覆盖纯函数 isDarkColor（据阅读页背景选明/暗主题的判定）；高亮本体（DOM 替换 + shiki 引擎）
// 属浏览器 + vendored 引擎，不在此覆盖（已注明）。
import { describe, it, expect } from 'vitest'
import { isDarkColor } from './shikiHighlight'

describe('isDarkColor', () => {
  it('暗色 → true（#hex / rgb / rgba）', () => {
    for (const c of ['#000', '#000000', '#1a1a1a', 'rgb(20, 20, 20)', 'rgba(0,0,0,1)', '#222']) {
      expect(isDarkColor(c)).toBe(true)
    }
  })

  it('亮色 → false（含纸色 / 月白等浅背景）', () => {
    for (const c of ['#fff', '#ffffff', 'rgb(255,255,255)', '#f5f0e8', '#eae6da', 'rgb(240 240 240)']) {
      expect(isDarkColor(c)).toBe(false)
    }
  })

  it('无法解析 → false（默认按亮色）', () => {
    for (const c of ['', 'transparent', 'blue', '#12', 'not-a-color']) {
      expect(isDarkColor(c)).toBe(false)
    }
  })
})
