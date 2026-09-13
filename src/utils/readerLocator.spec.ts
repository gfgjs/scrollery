// readerLocator 单测(阅读器方案 R2-7)。覆盖 loc1 编解码、旧值兼容、三级恢复各分支。
// 恢复用真实构造的 canonical 文本(含 \n 分段),偏移由 indexOf 现算,不硬编码脆弱常量。
import { describe, it, expect } from 'vitest'
import {
  LOC1_PREFIX,
  CONTEXT_CHARS,
  RECOVER_WINDOW,
  encodeLocator,
  parseLocator,
  parseLegacyPosition,
  contextTriple,
  snapToLineStart,
  buildTextLocator,
  buildEpubLocator,
  recoverTextOffset,
} from './readerLocator'
import type { ReaderLocator } from '../types/reader'

// 真实小说风格章节文本(段落以 \n 分隔),供恢复测试。
const CH = [
  '第一段:从前有座山,山里有座庙,庙里的故事就此展开。',
  '第二段:庙里有个老和尚,正在给一个小和尚讲一个古老的故事。',
  '第三段:讲的是什么故事呢?说的是从前有座山,山里有座庙……',
  '第四段:如此循环往复,绵绵不绝,听者与讲者都乐在其中。',
].join('\n')

describe('encodeLocator / parseLocator 往返', () => {
  it('txt 定位器编码后可无损解析', () => {
    const off = CH.indexOf('老和尚')
    const loc = buildTextLocator({
      isMarkdown: false,
      chapterIndex: 1,
      charOffset: off,
      chapterText: CH,
      bookFraction: 0.25,
    })
    const s = encodeLocator(loc)
    expect(s.startsWith(LOC1_PREFIX)).toBe(true)
    expect(parseLocator(s)).toEqual(loc)
  })

  it('epub 定位器编码后可无损解析', () => {
    const loc = buildEpubLocator('epubcfi(/6/4[chap01]!/4/2/1:0)', 0.42)
    expect(parseLocator(encodeLocator(loc))).toEqual(loc)
  })
})

describe('parseLocator 拒斥非法/旧值', () => {
  it('非 loc1 前缀 → null', () => {
    expect(parseLocator('scroll:0.5')).toBeNull()
    expect(parseLocator('cfi:epubcfi(/6)')).toBeNull()
    expect(parseLocator('')).toBeNull()
  })
  it('损坏 JSON → null(不抛)', () => {
    expect(parseLocator('loc1:{不是合法 json')).toBeNull()
  })
  it('缺 k 或 k 非法 → null', () => {
    expect(parseLocator('loc1:{"o":10}')).toBeNull()
    expect(parseLocator('loc1:{"k":"pdf"}')).toBeNull()
  })
})

describe('parseLegacyPosition 旧值兼容', () => {
  it('scroll:0.xx', () => {
    expect(parseLegacyPosition('scroll:0.3746')).toEqual({ kind: 'scroll', ratio: 0.3746 })
  })
  it('page:N', () => {
    expect(parseLegacyPosition('page:12')).toEqual({ kind: 'page', page: 12 })
  })
  it('cfi:<epubcfi>', () => {
    expect(parseLegacyPosition('cfi:epubcfi(/6/4!/4)')).toEqual({
      kind: 'cfi',
      cfi: 'epubcfi(/6/4!/4)',
    })
  })
  it('loc1 新值与垃圾串 → null', () => {
    expect(parseLegacyPosition('loc1:{"k":"txt"}')).toBeNull()
    expect(parseLegacyPosition('garbage')).toBeNull()
    expect(parseLegacyPosition('')).toBeNull()
  })
})

describe('contextTriple 边界', () => {
  it('中间偏移:b/h/a 各截取', () => {
    // 锚点取在第四段,off > CONTEXT_CHARS,使前文 b 为完整 40 字窗口(正区间,可精确对拍)。
    const off = CH.indexOf('循环往复')
    expect(off).toBeGreaterThan(CONTEXT_CHARS)
    const { b, h, a } = contextTriple(CH, off)
    expect(h).toBe(CH.slice(off, off + CONTEXT_CHARS))
    expect(b).toBe(CH.slice(off - CONTEXT_CHARS, off))
    expect(a.length).toBeLessThanOrEqual(CONTEXT_CHARS)
  })
  it('偏移 0:前文为空', () => {
    expect(contextTriple(CH, 0).b).toBe('')
  })
  it('偏移在末尾:锚点与后文为空', () => {
    const t = contextTriple(CH, CH.length)
    expect(t.h).toBe('')
    expect(t.a).toBe('')
  })
})

describe('snapToLineStart', () => {
  it('落到所在段的段首', () => {
    const line2Start = CH.indexOf('第二段')
    const midLine2 = line2Start + 5
    expect(snapToLineStart(CH, midLine2)).toBe(line2Start)
  })
  it('偏移 0 → 0', () => {
    expect(snapToLineStart(CH, 0)).toBe(0)
  })
})

describe('recoverTextOffset 三级恢复', () => {
  it('Level 1:主键直达(锚点在原偏移仍匹配)', () => {
    const off = CH.indexOf('老和尚')
    const loc = buildTextLocator({ isMarkdown: false, chapterIndex: 1, charOffset: off, chapterText: CH })
    expect(recoverTextOffset(CH, loc)).toBe(off)
  })

  it('Level 2:文本前插一段后,靠上下文重锚到新位置', () => {
    const off = CH.indexOf('老和尚')
    const loc = buildTextLocator({ isMarkdown: false, chapterIndex: 1, charOffset: off, chapterText: CH })
    const shifted = '新增的第零段:楔子。\n' + CH // 前插使后续整体右移
    const rec = recoverTextOffset(shifted, loc)
    // 恢复点处的文本应正好等于锚点原文 h(证明重锚到了正确内容)
    expect(shifted.slice(rec, rec + loc.t!.h.length)).toBe(loc.t!.h)
    expect(rec).toBeGreaterThan(off) // 因前插而右移
  })

  it('Level 2:h 歧义时 b+h 消歧到正确的那一处', () => {
    // 两处完全相同的 40+ 字重复段,只有前文不同 → h 相同、b+h 唯一。
    const repeat =
      '目标内容这是一段刻意重复的文字用于制造锚点歧义需要凑够四十个汉字以上所以继续补字补字补字'
    const before1 = '第一处独有前文甲甲甲甲甲'
    const before2 = '第二处独有前文乙乙乙乙乙'
    const text = before1 + repeat + '\n' + before2 + repeat
    const occ2 = text.indexOf(repeat, text.indexOf(repeat) + 1)
    const loc = buildTextLocator({ isMarkdown: false, chapterIndex: 0, charOffset: occ2, chapterText: text })
    // 把主键 o 打歪到 0(Level 1 必失配),强制走 Level 2 的 b+h 消歧
    const broken: ReaderLocator = { ...loc, o: 0 }
    expect(recoverTextOffset(text, broken)).toBe(occ2)
  })

  it('Level 2:漂移超出搜索窗口时全章兜底搜到唯一锚点', () => {
    const filler = '填充文字'.repeat(1500) // 6000 字,> RECOVER_WINDOW(4096)
    const anchor = '这是独一无二的锚点句需要凑够足够长度以便被完整截取为 h 值继续补齐到四十字以上'
    const text = filler + anchor + '收尾'
    const off = text.indexOf(anchor)
    expect(off).toBeGreaterThan(RECOVER_WINDOW) // 前提:锚点在从 0 起的窗口之外
    const loc = buildTextLocator({ isMarkdown: false, chapterIndex: 0, charOffset: off, chapterText: text })
    const broken: ReaderLocator = { ...loc, o: 0 } // o=0 使窗口 [0,4096] 覆盖不到 off
    expect(recoverTextOffset(text, broken)).toBe(off)
  })

  it('Level 3:锚点全然不存在 → 按 progression 落最近段首', () => {
    const loc: ReaderLocator = {
      k: 'txt',
      c: 0,
      o: 9999,
      p: 0.5,
      t: { b: '', h: '这段锚点文字在章里根本不存在', a: '' },
    }
    const approx = Math.round(0.5 * CH.length)
    expect(recoverTextOffset(CH, loc)).toBe(snapToLineStart(CH, approx))
  })

  it('空章 → 0', () => {
    const loc = buildTextLocator({ isMarkdown: false, chapterIndex: 0, charOffset: 0, chapterText: '' })
    expect(recoverTextOffset('', loc)).toBe(0)
  })

  it('锚点为空(旧数据/极短章)→ 直接走 progression 兜底', () => {
    const loc: ReaderLocator = { k: 'txt', c: 0, o: 5, p: 0, t: { b: '', h: '', a: '' } }
    expect(recoverTextOffset(CH, loc)).toBe(0)
  })
})
