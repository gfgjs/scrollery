import { describe, it, expect } from 'vitest'
import { resolveVisibleSection } from './settingsNav'

// 五个分区，等距 1000px（贴合真实注册表顺序）。
const SECTIONS = [
  { id: 'general', offsetTop: 0 },
  { id: 'media', offsetTop: 1000 },
  { id: 'ai', offsetTop: 2000 },
  { id: 'storage', offsetTop: 3000 },
  { id: 'advanced', offsetTop: 4000 },
] as const

/** 未触底的几何（scrollHeight 远大于 scrollTop+clientHeight）。 */
function midScroll(scrollTop: number) {
  return { scrollTop, clientHeight: 600, scrollHeight: 99999 }
}

describe('resolveVisibleSection', () => {
  it('空列表 → null', () => {
    expect(resolveVisibleSection([], midScroll(0))).toBeNull()
  })

  it('顶部 → 首分区', () => {
    expect(resolveVisibleSection(SECTIONS, midScroll(0))).toBe('general')
  })

  it('探针未越过下一分区 → 仍是上一分区', () => {
    // marker = 900 + 32 = 932 < 1000
    expect(resolveVisibleSection(SECTIONS, midScroll(900))).toBe('general')
  })

  it('探针恰好越过 → 切下一分区', () => {
    // marker = 968 + 32 = 1000 <= 1000
    expect(resolveVisibleSection(SECTIONS, midScroll(968))).toBe('media')
  })

  it('取最后一个越过探针的分区（不是第一个）', () => {
    // marker = 3000 + 32 = 3032 → general/media/ai/storage 都越过了，取 storage
    expect(resolveVisibleSection(SECTIONS, midScroll(3000))).toBe('storage')
  })

  it('自定义探针偏移生效', () => {
    // probeOffset=0 → marker = 1000 → media
    expect(resolveVisibleSection(SECTIONS, midScroll(1000), 0)).toBe('media')
    // probeOffset=0 → marker = 999 → 仍 general
    expect(resolveVisibleSection(SECTIONS, midScroll(999), 0)).toBe('general')
  })

  // ── 触底不变量（本轮修复核心；旧判据在此全挂）──────────────────────────────
  describe('触底 → 末分区（不问 offsetTop）', () => {
    it('末分区矮于视口、offsetTop 不可达时仍判它', () => {
      // 真机场景：advanced 折叠后只剩 200px 高。
      // scrollHeight=4200, clientHeight=600 → 最大 scrollTop=3600
      // marker = 3600 + 32 = 3632 < advanced.offsetTop(4000) → 旧判据永远返回 storage。
      const geo = { scrollTop: 3600, clientHeight: 600, scrollHeight: 4200 }
      expect(resolveVisibleSection(SECTIONS, geo)).toBe('advanced')
    })

    it('容差内的舍入差也算触底', () => {
      // 3599 + 600 = 4199 >= 4200 - 2 → 触底
      const geo = { scrollTop: 3599, clientHeight: 600, scrollHeight: 4200 }
      expect(resolveVisibleSection(SECTIONS, geo)).toBe('advanced')
    })

    it('差一大截不算触底，走探针法', () => {
      // 3000 + 600 = 3600 < 4200 - 2 → 未触底；marker=3032 → storage
      const geo = { scrollTop: 3000, clientHeight: 600, scrollHeight: 4200 }
      expect(resolveVisibleSection(SECTIONS, geo)).toBe('storage')
    })

    it('内容不足一屏（完全不可滚）→ 末分区', () => {
      // 全部展开不了：scrollHeight <= clientHeight，scrollTop 恒 0
      const geo = { scrollTop: 0, clientHeight: 600, scrollHeight: 500 }
      expect(resolveVisibleSection(SECTIONS, geo)).toBe('advanced')
    })

    it('末分区高于视口时触底判据与探针法结论一致（不冲突）', () => {
      // advanced 很高：scrollHeight=6000, clientHeight=600 → 最大 scrollTop=5400
      // marker = 5400 + 32 = 5432 > 4000 → 探针法本就返回 advanced，触底判据同结论。
      const geo = { scrollTop: 5400, clientHeight: 600, scrollHeight: 6000 }
      expect(resolveVisibleSection(SECTIONS, geo)).toBe('advanced')
    })
  })
})
