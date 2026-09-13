// 顶栏筛选 chips 的**唯一事实源**（S 线 §5 / D-008）。
//
// ## 为什么要有这个文件
//
// chip 列表此前散在**三处**，各自手写、互不知道对方存在：
//
// | 处 | 原形态 | 漏配后果 |
// |---|---|---|
// | `GalleryFilterChips.vue` | `inDom(0)…folded(7)` 字面量索引，映射只活在模板顶部一行注释里 | 插一个 chip 就要把后面每个数字加一，错了 = 折错 chip |
// | `AppToolbar.vue` `chipCount` | `hasActiveFilters ? 8 : 7` | 与模板脱节 = 视图控件 baseIndex 偏移，折叠位次全错 |
// | `AppToolbar.vue` `overflowRemeasureKey` | 手工枚举筛选字段 | **最危险**：漏一项 → 该 chip 文案变宽不触发重测 → 顶栏闪展再收，且**无编译/SSR/测试信号**，只在真机抖动 |
//
// 三处的共同点是「同一个列表被抄了三遍」。抄漏不报错 —— 这正是 experience.md §14 的元教训
// （按症状类别建不变量，别按单触发器打补丁）与顶栏 round7 已踩过的坑（docked 分支漏配对齐，
// 同样无信号）。故把列表收敛成一个数组，三处全部**推导**出来。
//
// ## 为什么不做成「全数据驱动模板」
//
// 各 chip 的**内容**结构差异很大（普通按钮 / 内联 StarRating / ColorLabelPicker / 开弹层的日期
// chip），硬塞进一个 `v-for` 要么得写一堆 `v-if` 分支，要么得给每种搞一个动态组件 —— 那是为
// DRY 而 DRY，可读性净亏。真正会出错的从来不是 chip 长什么样，而是**它排第几、在不在、宽度受
// 谁影响**。故本文件只收敛这三件事，chip 本体仍留模板里写死。

/**
 * chip 标识。**顺序即优先级**：靠前越晚被折叠（`useToolbarOverflow` 按可见前 N 项切分）。
 *
 * 「重复项」chip（2026-09-02 方案 §4.1）恒在首位固定可见,是筛选 chip 集合首段的镜头入口;
 * 「已暂停 N 个筛选」摘要 chip 仅在镜头激活且有普通筛选时存在(紧随其后)。
 *
 * 常显顺序依 §5/D-008：图片 | 视频 | 文档 | 音频 | LIVE | 收藏 | …。
 * 文档/音频补进来的依据是实测：文档 4,818 项（主体 txt 4,615）有实际价值；音频 32 项近乎空但无害。
 *
 * LIVE/收藏**不是**媒体类型，是与媒体类型相交的布尔 facet —— 仅 UI 同列，领域模型分离（§5）。
 */
export type FilterChipId =
  | 'duplicates'
  | 'lensPaused'
  | 'image'
  | 'video'
  | 'document'
  | 'audio'
  | 'live'
  | 'favorite'
  | 'rating'
  | 'color'
  | 'date'
  | 'format'
  | 'clear'

/**
 * 描述符读到的筛选状态**结构子集**。
 *
 * 有意不收 `filterStore` 实例：本模块是纯函数，测试不该为了问「date chip 排第几」去建一个 pinia。
 * 普通筛选维度补齐了 livePhotoOnly/favoritedOnly/mediaTypeCount（「已暂停 N 个筛选」要逐维度计数,
 * 仅有 hasActiveFilters 布尔不够）。构造统一走 `chipStateOf`,勿在组件里手抄字段表。
 */
export interface FilterChipState {
  hasActiveFilters: boolean
  mediaTypeCount: number
  /** 已选细分格式（S 线 P3）。触发器文案随其**个数**变宽：「格式」→「格式 3」。 */
  fileFormats: readonly string[]
  livePhotoOnly: boolean
  favoritedOnly: boolean
  minRating: number
  colorLabel: number
  dateFrom: number | null
  dateTo: number | null
  /**
   * 重复镜头激活（2026-09-02 方案 §4.1）。激活时普通筛选 chip **整体暂停**:普通筛选值原样保留但
   * 不参与镜头查询,UI 收束为 duplicates chip + 摘要 chip——普通 chip 与「清除」全部隐没,防在
   * 镜头内误改筛选(镜头内不可修改,§4.1)。
   */
  lensActive: boolean
}

/**
 * 「已暂停 N 个筛选」的 N:与 filterStore.hasActiveFilters 同一套维度判定,逐维度计数。
 * 纯函数:摘要 chip 的文案宽度随 N 变(重测反应源),组件弹层里列暂停项名也按同一套维度展开。
 */
export function pausedFilterCount(f: FilterChipState): number {
  let n = 0
  if (f.mediaTypeCount > 0) n++
  if (f.fileFormats.length > 0) n++
  if (f.livePhotoOnly) n++
  if (f.favoritedOnly) n++
  if (f.minRating > 0) n++
  if (f.colorLabel > 0) n++
  // 日期以两端皆备为激活态(与 hasActiveFilters 同尺,单端不算)。
  if (f.dateFrom !== null && f.dateTo !== null) n++
  return n
}

/**
 * FilterChipState 的唯一构造点：从 filterStore 实例（结构子集入参，不 import store）+ 镜头激活态
 * 组装。GalleryFilterChips 的 idx() 与 AppToolbar 的 chipCount/remeasureKey 都经此取状态——
 * FilterChipState 加字段只改这里,防多组件手抄字段表漂移（R-02/F-021 教训）。
 */
export function chipStateOf(
  f: {
    hasActiveFilters: boolean
    mediaTypes: readonly unknown[]
    fileFormats: readonly string[]
    livePhotoOnly: boolean
    favoritedOnly: boolean
    minRating: number
    colorLabel: number
    dateFrom: number | null
    dateTo: number | null
  },
  lensActive: boolean,
): FilterChipState {
  return {
    hasActiveFilters: f.hasActiveFilters,
    mediaTypeCount: f.mediaTypes.length,
    fileFormats: f.fileFormats,
    livePhotoOnly: f.livePhotoOnly,
    favoritedOnly: f.favoritedOnly,
    minRating: f.minRating,
    colorLabel: f.colorLabel,
    dateFrom: f.dateFrom,
    dateTo: f.dateTo,
    lensActive,
  }
}

interface FilterChipDescriptor {
  id: FilterChipId
  /**
   * 该 chip 是否**存在**（与折叠无关：折叠的项仍在 DOM 里，只是收拢）。缺省恒存在。
   *
   * ⚠ 条件存在项集中在**两端**:镜头组(duplicates/lensPaused)在最前,清除在末尾。中间项时有时无
   * 会让位次随状态跳动,而位次正是 `useToolbarOverflow` 的切分依据——但镜头激活本身是顶栏内容的
   * 整体换代(普通 chip 全体进出的同一瞬间),位次整体重排不构成「改个评分别的 chip 跟着折」那类
   * 渐进抖动,故镜头组允许放首段。
   */
  present?: (f: FilterChipState) => boolean
  /**
   * 影响**本 chip 自然宽**的反应源。
   *
   * 容器宽不变时 ResizeObserver 不触发，故凡是「文案/内容变宽但容器没变」的情形都必须在此声明，
   * 否则折叠切分停在旧宽度上 → 顶栏闪展再收。locale 是全体共有的，由 `chipWidthKey` 统一带上，
   * 这里只写**本 chip 特有**的。
   *
   * 判据：把这个值改一下，chip 的 `offsetWidth` 会不会变？会 → 必须列。
   */
  widthDeps?: (f: FilterChipState) => unknown[]
}

/** 普通筛选 chip 的镜头暂停谓词（§4.1）:镜头激活时全体隐没,统一引用勿散抄。 */
const pausedInLens = (f: FilterChipState) => !f.lensActive

/** 全部 chip，**顺序即优先级**。插入新 chip = 只改这一个数组。 */
const CHIPS: readonly FilterChipDescriptor[] = [
  // 重复项入口(2026-09-02 方案 §4.1):筛选 chip 集合首段固定可见;文本+CopyCheck 图标定宽,
  // 宽度只随 locale 变(已由 chipWidthKey 统一带上),无自有 widthDeps。
  { id: 'duplicates' },
  // 「已暂停 N 个筛选」摘要 chip:镜头激活且有普通筛选时存在,只读(弹层列暂停项,无修改入口)。
  // 文案「已暂停 N 个筛选」随 N 变宽 → N 是重测反应源。
  {
    id: 'lensPaused',
    present: (f) => f.lensActive && pausedFilterCount(f) > 0,
    widthDeps: (f) => [pausedFilterCount(f)],
  },
  // ── 以下为普通筛选 chip:镜头激活时整体暂停(§4.1),统一走 pausedInLens ──
  { id: 'image', present: pausedInLens },
  { id: 'video', present: pausedInLens },
  // 文档/音频：纯图标 + 固定文案，宽度只随 locale 变（已由 chipWidthKey 统一带上）。
  { id: 'document', present: pausedInLens },
  { id: 'audio', present: pausedInLens },
  { id: 'live', present: pausedInLens },
  { id: 'favorite', present: pausedInLens },
  // 🔴 格式 chip：位次紧随「收藏」= §5 字面顺序（R-17 挪位：原排在评分/颜色/日期之后,顺序即
  // 折叠优先级,格式 chip 被更早折叠,与设计不符且无裁决记录）。文案「格式」→「格式 3」随已选
  // 个数变宽 —— 这正是 §5 点名「漏加则顶栏闪展再收、且无编译/SSR/测试信号」的那一项。反应源取
  // **个数**而非数组本身：换了哪几个格式不改宽度，换了几个才改（同 rating 只吃 `>0` 而非星数）。
  { id: 'format', present: pausedInLens, widthDeps: (f) => [f.fileFormats.length] },
  // 评分 chip 在 minRating>0 时多出一个「+」后缀 → 变宽。
  { id: 'rating', present: pausedInLens, widthDeps: (f) => [f.minRating > 0] },
  // 颜色 chip 宽度本身不随色档变（内联色块定宽），但 active 态会改 padding/border 系的视觉；
  // 沿用改造前 remeasureKey 的既有姿态，保持逐值等价（此处不顺手「优化」掉，那是行为改动）。
  { id: 'color', present: pausedInLens, widthDeps: (f) => [f.colorLabel > 0] },
  // 日期 chip 在两端皆备时把「日期」文字换成实际区间标签 → 明显变宽。
  { id: 'date', present: pausedInLens, widthDeps: (f) => [f.dateFrom, f.dateTo] },
  // 清除 chip 仅在有活动筛选时**存在**（v-if）。排末尾故其出没不影响他项位次。
  // 镜头激活时隐没:清除=修改筛选,镜头内只读(§4.1),否则摘要 chip 说「已暂停」旁边却挂着清除入口。
  { id: 'clear', present: (f) => !f.lensActive && f.hasActiveFilters },
]

/** 当前**存在**的 chip（按优先级序）。DOM 顺序与之逐项对应。 */
export function visibleChipIds(f: FilterChipState): FilterChipId[] {
  return CHIPS.filter((c) => c.present?.(f) ?? true).map((c) => c.id)
}

/**
 * chip 总数 —— `AppToolbar` 的 `chipCount`，同时是视图控件的 `baseIndex`（视图控件排在 chips 之后）。
 */
export function chipCountOf(f: FilterChipState): number {
  return visibleChipIds(f).length
}

/**
 * 某 chip 的**位次**（`useToolbarOverflow` 的切分依据）。不存在 → `-1`。
 *
 * 模板用它替代原先的 `inDom(0)…folded(7)` 字面量：插入 chip 时再不用把后面每个数字手工加一。
 */
export function chipIndexOf(id: FilterChipId, f: FilterChipState): number {
  return visibleChipIds(f).indexOf(id)
}

/**
 * 全体 chip 的宽度反应源合成键（`AppToolbar` 的 `overflowRemeasureKey` 的 chip 部分）。
 *
 * 合成键只用于**触发重测**（`useToolbarOverflow` 是 `watch` 它，不是拿它当缓存键），故值长什么样
 * 无所谓，要紧的只有「该变的时候变了没有」。
 *
 * 两类反应源：
 * 1. **存在性本身** —— 一个 chip 的出没直接改变整行占宽与后续位次（改造前那句手写 key 里的
 *    `filter.hasActiveFilters` 正是干这个的，不是在描述清除 chip 有多宽）；
 * 2. 各 chip 自报的 `widthDeps`。
 *
 * `locale` 由调用方拼在外层：它影响每一个 chip，不是某一个的私事。视图控件那侧的反应源
 * （`groupBy` / 语义模式）同理留在调用方 —— 本模块只管 chips。
 */
export function chipWidthKey(f: FilterChipState): string {
  const present = CHIPS.filter((c) => c.present?.(f) ?? true)
  return [
    present.map((c) => c.id).join(','),
    ...present.flatMap((c) => c.widthDeps?.(f) ?? []),
  ].join('|')
}
