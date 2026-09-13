import { describe, it, expect } from 'vitest'
import {
  chipCountOf,
  chipIndexOf,
  chipWidthKey,
  pausedFilterCount,
  visibleChipIds,
  type FilterChipState,
} from './filterChips.descriptors'

function state(over: Partial<FilterChipState> = {}): FilterChipState {
  return {
    hasActiveFilters: false,
    mediaTypeCount: 0,
    minRating: 0,
    colorLabel: 0,
    dateFrom: null,
    dateTo: null,
    fileFormats: [],
    livePhotoOnly: false,
    favoritedOnly: false,
    lensActive: false,
    ...over,
  }
}

describe('chip 位次与总数', () => {
  it('常显顺序:重复项 | 图片 视频 文档 音频 LIVE 收藏 格式 …', () => {
    // 钉死前七项的**顺序**——顺序即折叠优先级,不是随手排的。重复项 chip(2026-09-02 方案 §4.1)
    // 恒在筛选 chip 集合首段;普通 chip 的相对顺序维持 §5/D-008(格式紧随收藏是 R-17 挪位回归钉)。
    expect(visibleChipIds(state()).slice(0, 7)).toEqual([
      'duplicates',
      'image',
      'video',
      'document',
      'audio',
      'live',
      'favorite',
    ])
  })

  // 总数 11/12(原 10/11 + 固定可见的「重复项」chip);镜头激活时收束为 1/2(见下方镜头 describe)。
  it('镜头关闭:无活动筛选 11 项;有活动筛选 12 项(多出清除 chip)', () => {
    expect(chipCountOf(state())).toBe(11)
    expect(chipCountOf(state({ hasActiveFilters: true }))).toBe(12)
  })

  /** 「格式…」是一级触发器，紧随「收藏」、在评分/颜色/日期之前（§5 字面顺序,R-17）。 */
  it('格式 chip 紧随收藏、先于评分与清除', () => {
    const ids = visibleChipIds(state({ hasActiveFilters: true }))
    expect(ids.indexOf('format')).toBe(ids.indexOf('favorite') + 1)
    expect(ids.indexOf('format')).toBeLessThan(ids.indexOf('rating'))
    expect(ids.indexOf('format')).toBeLessThan(ids.indexOf('clear'))
  })

  it('清除 chip 只在有活动筛选时存在,且恒在末位', () => {
    expect(visibleChipIds(state())).not.toContain('clear')
    const on = visibleChipIds(state({ hasActiveFilters: true }))
    expect(on[on.length - 1]).toBe('clear')
  })

  it('不存在的 chip 位次为 -1(而非 0——那会与首项撞车)', () => {
    expect(chipIndexOf('clear', state())).toBe(-1)
    expect(chipIndexOf('lensPaused', state())).toBe(-1)
  })

  /**
   * 🔴 条件存在的 chip 排在末尾 ⇒ 它的出没**不得**挪动其他 chip 的位次。
   *
   * 可证伪性:位次是 useToolbarOverflow 的切分依据。若哪天把某个条件项挪到中间,用户点一下
   * 「收藏」就会看见别的 chip 跟着折叠 —— 本用例即刻翻车。
   */
  it('清除 chip 的出没不挪动其他 chip 的位次', () => {
    const off = state()
    const on = state({ hasActiveFilters: true })
    for (const id of visibleChipIds(off)) {
      expect(chipIndexOf(id, on), `${id} 位次被清除 chip 挤动了`).toBe(chipIndexOf(id, off))
    }
  })
})

// ── 重复镜头(2026-09-02 方案 §4.1)───────────────────────────────────────────────
// 镜头激活是顶栏内容的整体换代:普通筛选 chip 全体暂停,收束为「重复项 + 已暂停 N 个筛选」。
// 位次收束发生在镜头进入/退出的一瞬(顶栏整体换语义),不构成「改个评分别的 chip 跟着折」的
// 渐进抖动,故镜头组允许放首段(见 descriptors 内 present 注释)。

describe('重复镜头激活时 chip 收束(§4.1)', () => {
  it('镜头激活、无暂停筛选:只剩重复项 chip', () => {
    const on = state({ lensActive: true })
    expect(visibleChipIds(on)).toEqual(['duplicates'])
    expect(chipCountOf(on)).toBe(1)
  })

  it('镜头激活且有暂停筛选:重复项 + 摘要 chip,摘要紧随其后、位次 1', () => {
    const on = state({ lensActive: true, hasActiveFilters: true, minRating: 3, favoritedOnly: true })
    const ids = visibleChipIds(on)
    expect(ids).toEqual(['duplicates', 'lensPaused'])
    expect(chipIndexOf('lensPaused', on)).toBe(1)
  })

  it('镜头激活时普通 chip 与清除 chip 全体隐没(镜头内不可改筛选,§4.1)', () => {
    const on = state({ lensActive: true, hasActiveFilters: true, mediaTypeCount: 2 })
    const ids = visibleChipIds(on)
    for (const id of ['image', 'video', 'document', 'audio', 'live', 'favorite', 'format', 'rating', 'color', 'date', 'clear'] as const) {
      expect(ids, `${id} 应在镜头激活时暂停`).not.toContain(id)
    }
  })

  it('pausedFilterCount 与 hasActiveFilters 同尺:逐维度计数、单端日期不算', () => {
    // 全部维度激活 = 7。
    expect(
      pausedFilterCount(
        state({
          hasActiveFilters: true,
          mediaTypeCount: 2,
          fileFormats: ['png', 'jpg'],
          livePhotoOnly: true,
          favoritedOnly: true,
          minRating: 3,
          colorLabel: 2,
          dateFrom: 1_700_000_000,
          dateTo: 1_700_086_400,
        }),
      ),
    ).toBe(7)
    // 单端日期不构成激活维度(与 hasActiveFilters 的判定一致)。
    expect(pausedFilterCount(state({ dateFrom: 1_700_000_000 }))).toBe(0)
  })
})

// ── 重测键(§5 三处描述符化里**最危险**的一处)────────────────────────────────────
//
// 漏一项反应源 → 该 chip 文案变宽却不触发重测 → 折叠切分停在旧宽度上 → 顶栏闪展再收,
// 而且**无编译/SSR/测试信号**,只在真机抖动。故这里逐条钉「该变的时候变了」。
//
// 断言的是「键**变了**」而非键等于某个具体字符串:useToolbarOverflow 是 watch 它触发重测,
// 值本身无语义。钉具体值等于把实现细节焊死,改个分隔符就红,却什么 bug 也抓不到。

describe('重测键随宽度反应源变化', () => {
  const cases: Array<[string, FilterChipState, FilterChipState]> = [
    // 评分 >0 时多一个「+」后缀。
    ['评分从无到有', state(), state({ minRating: 3 })],
    // 颜色 active 态(沿用改造前 key 的既有姿态)。
    ['颜色标签从无到有', state(), state({ colorLabel: 2 })],
    // 日期两端皆备时「日期」文字换成实际区间标签 → 明显变宽。
    ['日期起点出现', state(), state({ dateFrom: 1_700_000_000 })],
    ['日期终点出现', state({ dateFrom: 1_700_000_000 }), state({ dateFrom: 1_700_000_000, dateTo: 1_700_086_400 })],
    // 清除 chip 的**存在性**:改造前那句手写 key 里的 hasActiveFilters 正是干这个的。
    ['清除 chip 出现(存在性也是反应源)', state(), state({ hasActiveFilters: true })],
    // 🔴 §5 点名「最危险」的那一项:「格式」→「格式 1」文案变宽。漏声明则顶栏闪展再收,
    // 且无编译/SSR/测试信号。P2 的变异验证专门演练过这个形态(MP4 日期漏 widthDeps)。
    ['格式从无到有(「格式」→「格式 1」变宽)', state(), state({ fileFormats: ['png'] })],
    ['格式个数变化(「格式 1」→「格式 2」变宽)', state({ fileFormats: ['png'] }), state({ fileFormats: ['png', 'jpg'] })],
    // 「已暂停 N 个筛选」随 N 变宽(2026-09-02 方案 §4.1)。
    [
      '暂停筛选数变化(摘要 chip 文案变宽)',
      state({ lensActive: true, hasActiveFilters: true, favoritedOnly: true }),
      state({ lensActive: true, hasActiveFilters: true, favoritedOnly: true, minRating: 2 }),
    ],
  ]
  for (const [name, a, b] of cases) {
    it(name, () => {
      expect(chipWidthKey(a)).not.toBe(chipWidthKey(b))
    })
  }

  /**
   * 改造前的手写键是 `[locale, minRating>0, hasActiveFilters, colorLabel>0, dateFrom, dateTo,
   * groupBy, isSemanticMode]`。本函数只接管其中的 chip 部分(前五项),locale 与视图控件那两项
   * 留在调用方 —— 它们不是某个 chip 的私事。
   *
   * 「等价」指的是**变化点等价**,不是字符串相等:键只用于触发 watch,值无语义。上面五条用例
   * 逐一覆盖了原键的五个 chip 侧反应源;这条兜底钉反面 —— 与宽度**无关**的变动不得让键抖动,
   * 否则就是白跑量尺帧(折叠态下量尺帧会全展开再带过渡收回,用户看得见)。
   */
  it('评分在 >0 区间内变动不改键(宽度只取决于「有没有 + 后缀」,不取决于几颗星)', () => {
    expect(chipWidthKey(state({ minRating: 3 }))).toBe(chipWidthKey(state({ minRating: 5 })))
  })

  it('颜色标签在 >0 区间内换色不改键(色块定宽)', () => {
    expect(chipWidthKey(state({ colorLabel: 1 }))).toBe(chipWidthKey(state({ colorLabel: 7 })))
  })

  /**
   * 反面同理:格式 chip 的宽度取决于**个数**而非选了哪几个 ——「格式 2」不因 png→gif 而变宽。
   * 反应源若直接吃数组,每换一个格式都白跑一次量尺帧(折叠态下量尺帧会全展开再带过渡收回,
   * 用户看得见)。
   */
  it('换格式但个数不变 → 不改键(「格式 2」宽度与选了哪两个无关)', () => {
    expect(chipWidthKey(state({ fileFormats: ['png', 'jpg'] }))).toBe(
      chipWidthKey(state({ fileFormats: ['gif', 'mp4'] })),
    )
  })
})
