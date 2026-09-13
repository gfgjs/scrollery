// src/utils/readerLocator.ts
// 复合定位器(loc1,阅读器方案 §5.7 / §6.2)的编解码与三级恢复算法。
//
// 纯函数、无副作用、format 无关:txt/md 主键 = 章内字符偏移(对字号/横竖排/分页模式免疫),
// epub 主键 = CFI(交 foliate 解析);上下文三元组 `t{b,h,a}` 是布局与文本双无关的最后重锚防线。
// 偏移锚定空间见 ReaderLocator 的文档:canonical 文本(替换/简繁之前)。
//
// 本模块只做「算」——不碰 IPC、不碰 DOM;由各渲染器在存/取进度时调用(R2-7 起接线)。

import type { ReaderLocator } from '../types/reader'

/** loc1 序列化前缀(reading_progress.position / reader_bookmarks.locator 共用)。 */
export const LOC1_PREFIX = 'loc1:'

/** 上下文三元组每段截取的字符数(前文 / 锚点 / 后文各取约此数)。 */
export const CONTEXT_CHARS = 40

/**
 * 章内重锚搜索窗口(字符):失配时在 期望偏移 ±此半径 内搜锚点。
 * 取 4096 覆盖「重排/版本小改导致的局部漂移」,又不至于全章线性扫描退化。
 */
export const RECOVER_WINDOW = 4096

const clamp = (n: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, n))

/**
 * 把偏移吸附到所在行/段首(canonical 文本以 `\n` 分段)。
 * §5.7 兜底「按 p 落最近段首」用:避免恢复到段落中间造成半句起读的割裂感。
 */
export function snapToLineStart(text: string, offset: number): number {
  const at = clamp(offset, 0, text.length)
  if (at === 0) return 0
  // 若正好落在换行符后(段首),lastIndexOf 从 at-1 起找上一个 \n,其后一位即段首。
  const prevBreak = text.lastIndexOf('\n', at - 1)
  return prevBreak < 0 ? 0 : prevBreak + 1
}

/**
 * 取 offset 处的上下文三元组:h=锚点原文(自 offset 起 CONTEXT_CHARS 字),
 * b=前文(offset 前 CONTEXT_CHARS 字),a=后文(h 之后 CONTEXT_CHARS 字)。
 */
export function contextTriple(text: string, offset: number): { b: string; h: string; a: string } {
  const at = clamp(offset, 0, text.length)
  const h = text.slice(at, at + CONTEXT_CHARS)
  const b = text.slice(Math.max(0, at - CONTEXT_CHARS), at)
  const a = text.slice(at + h.length, at + h.length + CONTEXT_CHARS)
  return { b, h, a }
}

/** 构造 txt/md 定位器:章 index + 章内偏移 + progression + 上下文三元组。 */
export function buildTextLocator(opts: {
  isMarkdown: boolean
  chapterIndex: number
  charOffset: number
  chapterText: string
  /** 章内 progression 0..1;缺省由 charOffset/章长推算。 */
  chapterFraction?: number
  /** 全书 progression 0..1。 */
  bookFraction?: number
}): ReaderLocator {
  const { isMarkdown, chapterIndex, charOffset, chapterText, chapterFraction, bookFraction } = opts
  const len = chapterText.length
  const o = clamp(charOffset, 0, len)
  const p = chapterFraction ?? (len > 0 ? o / len : 0)
  return {
    k: isMarkdown ? 'md' : 'txt',
    c: chapterIndex,
    o,
    p: Number(p.toFixed(4)),
    ...(bookFraction != null ? { tp: Number(bookFraction.toFixed(4)) } : {}),
    t: contextTriple(chapterText, o),
  }
}

/** 构造 epub 定位器:CFI 主键(交 foliate 恢复)+ 可选全书 progression。 */
export function buildEpubLocator(cfi: string, bookFraction?: number): ReaderLocator {
  return {
    k: 'epub',
    cfi,
    ...(bookFraction != null ? { tp: Number(bookFraction.toFixed(4)) } : {}),
  }
}

/** 序列化为 `loc1:<json>`。 */
export function encodeLocator(loc: ReaderLocator): string {
  return LOC1_PREFIX + JSON.stringify(loc)
}

/**
 * 解析 `loc1:<json>`。仅接受 loc1 前缀且结构合法(k 为 txt|md|epub)的值;否则返回 null
 * (旧值走 parseLegacyPosition)。防御:JSON 损坏 / 缺 k / k 非法一律 null,不抛。
 */
export function parseLocator(s: string): ReaderLocator | null {
  if (!s || !s.startsWith(LOC1_PREFIX)) return null
  try {
    const obj = JSON.parse(s.slice(LOC1_PREFIX.length)) as unknown
    if (!obj || typeof obj !== 'object') return null
    const k = (obj as { k?: unknown }).k
    if (k !== 'txt' && k !== 'md' && k !== 'epub') return null
    return obj as ReaderLocator
  } catch {
    return null
  }
}

/** 旧位置格式(§6.2:读到旧值按旧语义恢复一次,下次保存自然升级为 loc1,不做批量迁移)。 */
export type LegacyPosition =
  | { kind: 'scroll'; ratio: number }
  | { kind: 'page'; page: number }
  | { kind: 'cfi'; cfi: string }

/** 解析旧值 `scroll:0.xx` / `page:N` / `cfi:<epubcfi>`;非旧格式返回 null。 */
export function parseLegacyPosition(s: string): LegacyPosition | null {
  if (!s) return null
  const scroll = /^scroll:([\d.]+)$/.exec(s)
  if (scroll) {
    const ratio = parseFloat(scroll[1])
    return Number.isFinite(ratio) ? { kind: 'scroll', ratio } : null
  }
  const page = /^page:(\d+)$/.exec(s)
  if (page) return { kind: 'page', page: parseInt(page[1], 10) }
  if (s.startsWith('cfi:')) return { kind: 'cfi', cfi: s.slice(4) }
  return null
}

/**
 * txt/md 章内三级恢复(§5.7),返回 canonical 空间的章内字符偏移。
 * 1) 主键直达:o 处切片 === 锚点原文 h → 直接返回 o。
 * 2) 上下文重锚:期望偏移 ±RECOVER_WINDOW 内搜 b+h(带前文减歧义),退搜 h;窗内失配再全章搜 h。
 * 3) 兜底:按章内 progression p 落最近段首(snapToLineStart)。
 *
 * 设计意图:重排开关 / 版本小改会使 canonical 变化,主键 o 失准 → 靠文本内容(h)重新定位,
 * 这正是 t 三元组「布局与文本双无关」的价值。h 为空(旧数据/极短章)时直接走兜底。
 */
export function recoverTextOffset(chapterText: string, loc: ReaderLocator): number {
  const len = chapterText.length
  if (len === 0) return 0
  const h = loc.t?.h ?? ''
  const o = clamp(loc.o ?? 0, 0, len)

  // Level 1:主键直达 + 锚点原文校验
  if (h && chapterText.slice(o, o + h.length) === h) return o

  // Level 2:上下文重锚
  if (h) {
    const from = Math.max(0, o - RECOVER_WINDOW)
    const to = Math.min(len, o + RECOVER_WINDOW)
    const windowText = chapterText.slice(from, to)
    const b = loc.t?.b ?? ''
    // 优先 b+h(前文+锚点,歧义最小),命中则锚点落在 b 之后
    if (b) {
      const idxBH = windowText.indexOf(b + h)
      if (idxBH >= 0) return from + idxBH + b.length
    }
    const idxH = windowText.indexOf(h)
    if (idxH >= 0) return from + idxH
    // 窗内失配:全章兜底搜 h(漂移超窗口的极端情况)
    const idxAll = chapterText.indexOf(h)
    if (idxAll >= 0) return idxAll
  }

  // Level 3:progression 兜底,落最近段首
  const approx = Math.round((loc.p ?? 0) * len)
  return snapToLineStart(chapterText, clamp(approx, 0, len))
}
