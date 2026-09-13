// TimelineScrubber 的纯映射逻辑（与组件分离以便单测，Part5 §3.3）。
// 无 DOM / 无响应式依赖 —— 时间均布的 index↔逻辑 y 映射、密度归一化、年份边界判定，是 scrubber
// 最易藏 off-by-one 的部分（frac=1 越界、末月 +∞ 上界、空桶）。组件里依赖 getBoundingClientRect /
// 指针事件的部分留在 .vue（DOM 相关，不在此测）。
//
// 入参用最小结构化类型（只取所需字段）而非整个 MonthBucket，便于测试构造轻量 fixture。

import { logger } from '../../utils/logger'

/** 最热月项数（密度归一化分母）；至少 1 防除零（与组件 reduce(...,1) 一致）。 */
export function maxBucketCount(buckets: readonly { count: number }[]): number {
  return buckets.reduce((m, b) => Math.max(m, b.count), 1)
}

/**
 * 密度热力条宽度（占轨道宽百分比）：保底 `floorPct` 让「有但少」的月也可见，其余按比例铺到 100%。
 * @param count 该月项数
 * @param maxCount 最热月项数（归一化分母）
 * @param floorPct 保底百分比（默认 12）
 */
export function densityBarWidth(count: number, maxCount: number, floorPct = 12): number {
  const safeMax = maxCount > 0 ? maxCount : 1
  return floorPct + (count / safeMax) * (100 - floorPct)
}

/**
 * 当前逻辑 y 落在哪个月：buckets 按显示序排列，月 i 覆盖 `[b[i].y, b[i+1].y)`（末月上界 +∞）。
 * 空桶返回 -1；y 在首月之前等未命中区间时兜底返回 0（最新月）。
 */
export function findActiveMonthIndex(
  buckets: readonly { y: number }[],
  currentY: number,
): number {
  if (buckets.length === 0) return -1
  for (let i = 0; i < buckets.length; i++) {
    const nextY = i + 1 < buckets.length ? buckets[i + 1].y : Infinity
    if (currentY >= buckets[i].y && currentY < nextY) return i
  }
  return 0
}

/** 是否某年首月：i=0 恒真；否则与上一桶年份不同处为真（最新→最旧排列下即每年最上一格）。 */
export function isYearBoundary(buckets: readonly { year: number }[], i: number): boolean {
  if (i === 0) return true
  return buckets[i].year !== buckets[i - 1].year
}

/**
 * 比例布局（按逻辑 y 定位）下,决定「年份标签」实际显示在哪些月索引 —— 防止内容稀疏的相邻年份
 * 在轨道上像素挤叠成一团。做法:遍历年份边界月,只有当其像素位置距上一个已显示标签 >= minGapPx
 * 时才显示（每簇取最靠上的一个）。空桶 / 无高度 → 空集。
 * @param buckets 月桶（需 y / year），按显示序（最新→最旧）
 * @param totalHeight 布局总逻辑高度（y→比例基准）
 * @param trackH 轨道像素高（比例→像素）
 * @param minGapPx 相邻标签最小像素间距（默认 12）
 */
export function visibleYearLabelSet(
  buckets: readonly { y: number; year: number }[],
  totalHeight: number,
  trackH: number,
  minGapPx = 12,
): Set<number> {
  const set = new Set<number>()
  if (buckets.length === 0 || trackH <= 0) return set
  const h = totalHeight > 0 ? totalHeight : 1
  let lastPx = -Infinity
  for (let i = 0; i < buckets.length; i++) {
    const isBoundary = i === 0 || buckets[i].year !== buckets[i - 1].year
    if (!isBoundary) continue
    const px = (buckets[i].y / h) * trackH
    if (px - lastPx >= minGapPx) {
      set.add(i)
      lastPx = px
    }
  }
  return set
}

/**
 * 轨道纵向比例 → 月索引：`floor(frac*n)` 并 clamp 到 `[0, n-1]`（frac=1 时不越界到 n）。
 * @param frac 已 clamp 到 [0,1] 的纵向比例
 * @param monthCount 月数；<=0 返回 0（无月可指）
 */
export function fractionToMonthIndex(frac: number, monthCount: number): number {
  if (monthCount <= 0) return 0
  return Math.min(monthCount - 1, Math.floor(frac * monthCount))
}

/**
 * folder/none 模式：把「点轨道任意处」吸附到逻辑 y 最接近的真实分组边界，返回该 separator 索引
 * （T0，§9.4；取代原「按比例跳 frac*totalHeight」——0.3px 间距的小圆点根本点不中特定一个）。
 * 前提：separators 按逻辑 y 升序（后端行遍历自然满足）。二分定位后比较左右邻取更近者，
 * 相等时偏向更靠前（索引小）者。空数组返回 -1（调用方回退按比例）。
 * @param separators 分隔符（仅需 y 字段，须按 y 升序）
 * @param frac 已 clamp 到 [0,1] 的纵向比例
 * @param totalHeight 布局总逻辑高度（frac→目标 y 的换算基准）
 */
export function nearestSeparatorIndex(
  separators: readonly { y: number }[],
  frac: number,
  totalHeight: number,
): number {
  const n = separators.length
  if (n === 0) return -1
  const targetY = frac * totalHeight
  // 二分：找第一个 y >= targetY 的下标 lo（可能为 n，表示全部小于 targetY）。
  let lo = 0
  let hi = n
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (separators[mid].y < targetY) lo = mid + 1
    else hi = mid
  }
  if (lo === 0) return 0 // targetY 在首个分组之前或恰等
  if (lo >= n) return n - 1 // targetY 在末个分组之后
  const prev = lo - 1
  // prev.y < targetY <= lo.y：比较两侧距离，相等偏向 prev（更靠前）。
  return targetY - separators[prev].y <= separators[lo].y - targetY ? prev : lo
}

/**
 * 取「光标最近的 K 个分隔符」窗口（时间轴放大镜方案乙：显示定数项而非固定 y 窗口，避免密集区节点
 * 爆炸/不可读）。以 nearestSeparatorIndex 定位中心 center，向两侧各扩 ⌊K/2⌋ 到 K 个；近首/尾时窗口
 * 整体贴边补足 K 个（不越界）。返回 `{ start, end, center }`（end 不含，供 slice；center 为高亮项）。
 * @param separators 分隔符（需 y 字段，按 y 升序）
 * @param frac 已 clamp 到 [0,1] 的纵向比例（= loupeCenterY / totalHeight）
 * @param totalHeight 布局总逻辑高度
 * @param k 窗口项数（如 9）
 */
export function nearestSeparatorWindow(
  separators: readonly { y: number }[],
  frac: number,
  totalHeight: number,
  k: number,
): { start: number; end: number; center: number } {
  const n = separators.length
  if (n === 0 || k <= 0) return { start: 0, end: 0, center: -1 }
  const center = nearestSeparatorIndex(separators, frac, totalHeight)
  const kk = Math.min(k, n)
  const half = Math.floor(kk / 2)
  let start = center - half
  if (start < 0) start = 0
  let end = start + kk
  if (end > n) {
    end = n
    start = n - kk // 贴尾时整体上移补足 K 个
  }
  return { start, end, center }
}

/**
 * 密度带重采样(canvas 密度带升级方案 §3):把 separators 的 (y, count) 重采样到 `trackH` 个像素行,
 * 每行给出归一化 [0,1] 密度强度(渲染分辨率=像素行数,与库规模解耦),供 canvas 画成连续密度带。
 * item-proportional 坐标:separator i 覆盖 y 区间 `[y_i, y_{i+1})`(末个到 totalHeight),按占比累加到像素行,
 * `intensity[r] = Σcount / Σweight`(该行覆盖分隔符的平均项数)。归一化分母 = 全行最大值(至少 1 防除零)。
 * 空 separators / trackH<=0 → 全零数组。
 * @param separators 分隔符(需 y / count,按 y 升序)
 * @param totalHeight 布局总逻辑高度(y→行归一化基准)
 * @param trackH 轨道像素高(= 输出数组长度,即渲染行数)
 */
export function buildRowIntensity(
  separators: readonly { y: number; count: number }[],
  totalHeight: number,
  trackH: number,
): Float32Array {
  const rows = Math.max(0, Math.floor(trackH))
  const out = new Float32Array(rows)
  if (rows === 0 || separators.length === 0) return out
  const H = totalHeight > 0 ? totalHeight : 1
  const n = separators.length
  const sum = new Float32Array(rows)
  const wt = new Float32Array(rows)
  const clampRow = (r: number) => Math.min(rows - 1, Math.max(0, r))
  for (let i = 0; i < n; i++) {
    const yTop = separators[i].y
    const yBot = i + 1 < n ? separators[i + 1].y : H
    const r0 = clampRow(Math.floor((yTop / H) * rows))
    // ceil 保证至少覆盖一行;上界不越界。
    const r1 = Math.min(rows, Math.max(r0 + 1, Math.ceil((yBot / H) * rows)))
    const c = separators[i].count
    for (let r = r0; r < r1; r++) {
      sum[r] += c
      wt[r] += 1
    }
  }
  let max = 1
  for (let r = 0; r < rows; r++) {
    const v = wt[r] > 0 ? sum[r] / wt[r] : 0
    out[r] = v
    if (v > max) max = v
  }
  for (let r = 0; r < rows; r++) out[r] /= max // 归一化到 [0,1]
  return out
}

/** 时间比例密度带结果（time 坐标）：强度 + 逐行跳转 y + 日历刻度（LOD 网格线 / 年标重定位）。 */
export interface TimeBandResult {
  /** 每像素行归一化 [0,1] 密度强度（渲染分辨率 = 行数）。 */
  intensity: Float32Array
  /** 每像素行点击 → 跳转的 logical y（空日行按相邻真实日 y 线性插值）。 */
  rowJumpY: Float32Array
  /**
   * 每个「月初」落在的像素行（去重连续同行）；供 time 坐标 LOD 月网格线（§6）。
   * 顺序为日历迭代序（旧→新月），行号随轨道朝向递减（DESC）或递增（ASC）——消费端不得假设升序。
   */
  monthTicks: number[]
  /**
   * 每个「年初」落在的像素行 + 年份；time 坐标下 item 空间年标会错位，改由此重定位。
   * 首项恒为最旧年的锚（锚在 firstDay 行，因其 1 月 1 日通常在域外），后续为域内各年初，年代升序。
   */
  yearTicks: { row: number; year: number }[]
}

/**
 * 时间比例密度带(canvas 密度带升级方案 §3/§6,P3 **time-proportional 坐标**):把带 epochDay 的
 * separators 按**日历时间**线性铺到 `trackH` 个像素行(y ∝ 日历日而非项累计),呈现纵向疏密
 * (繁忙期尖峰、淡季留白,近 Apple/Google Photos)。轨道朝向跟随网格排序自适应:DESC(默认)顶=最新日、
 * ASC 顶=最旧日,恒与网格滚动方向一致(据最新/最旧日 separator 的 y 相对位置探测)。与
 * [`buildRowIntensity`](item 坐标)正交,仅数据摆法不同。
 *
 * - **intensity[r]**:落入该行各日项数求和后归一化 [0,1];空日行=0。
 * - **rowJumpY[r]**:真实日行取 separator.y;空日行按相邻真实日 row/y(row 升↔y 升,单调)线性插值,
 *   使点淡季空白也平滑滚到该时段。
 * - **monthTicks / yearTicks**:UTC 日历迭代出每月/年初的行(§6 LOD);year 标签在 item 坐标由
 *   monthBucket.y 定位,time 坐标会错位,故用 yearTicks 重定位。
 *
 * folder/none 分组无 epochDay → intensity 全零 + rowJumpY 线性回退(坐标切换仅在 date 分组开放,见组件门控)。
 * @param separators 分隔符(需 y / count / epochDay;date 分组 epochDay 非 null)
 * @param totalHeight 布局总逻辑高度(rowJumpY / 回退基准)
 * @param trackH 轨道像素高(= 输出数组长度)
 */
export function buildTimeBand(
  separators: readonly { y: number; count: number; epochDay: number | null }[],
  totalHeight: number,
  trackH: number,
): TimeBandResult {
  const rows = Math.max(0, Math.floor(trackH))
  const intensity = new Float32Array(rows)
  const rowJumpY = new Float32Array(rows)
  const monthTicks: number[] = []
  const yearTicks: { row: number; year: number }[] = []
  if (rows === 0) return { intensity, rowJumpY, monthTicks, yearTicks }
  const H = totalHeight > 0 ? totalHeight : 1
  // 仅 date 分组 separator 带 epochDay;过滤出有效日（narrowing 到非空）。
  const dated = separators.filter(
    (s): s is { y: number; count: number; epochDay: number } => s.epochDay != null,
  )
  if (dated.length === 0) {
    // folder/none 模式无时间坐标 → 线性 rowJumpY(intensity 全零),使 canvas 仍可点击滚动。
    for (let r = 0; r < rows; r++) rowJumpY[r] = (r / rows) * H
    return { intensity, rowJumpY, monthTicks, yearTicks }
  }

  let firstDay = Infinity
  let lastDay = -Infinity
  let yAtFirstDay = 0
  let yAtLastDay = 0
  for (const s of dated) {
    if (s.epochDay < firstDay) {
      firstDay = s.epochDay
      yAtFirstDay = s.y
    }
    if (s.epochDay > lastDay) {
      lastDay = s.epochDay
      yAtLastDay = s.y
    }
  }
  const span = lastDay - firstDay
  // 轨道朝向自适应网格排序(评审 R1):DESC(默认,最新日 y 最小)顶=最新;ASC(旧→新,最新日
  // y 最大)顶=最旧。恒保「row 升 ↔ y 升」不变量——下方锚点插值与 logicalYToTimeFrac 的
  // lower-bound 二分都依赖 rowJumpY 单调不减;若硬钉「顶=最新」,ASC 下 rowJumpY 反向单调即被击穿
  // (指示线倒置),且 band 朝向会与网格滚动方向相反。
  const asc = yAtLastDay > yAtFirstDay
  const rowOf = (d: number) => {
    if (span <= 0) return 0
    const t = asc ? (d - firstDay) / span : (lastDay - d) / span
    return Math.min(rows - 1, Math.max(0, Math.round(t * (rows - 1))))
  }

  // 密度求和 + 收集 (row, y) 锚点。
  const anchors: { row: number; y: number }[] = []
  for (const s of dated) {
    const r = rowOf(s.epochDay)
    intensity[r] += s.count
    anchors.push({ row: r, y: s.y })
  }
  let max = 1
  for (let r = 0; r < rows; r++) if (intensity[r] > max) max = intensity[r]
  for (let r = 0; r < rows; r++) intensity[r] /= max

  // rowJumpY:按 row 升序锚点线性插值。row 升 ↔ y 升(rowOf 朝向自适应后恒成立),故单调可插值。
  anchors.sort((a, b) => a.row - b.row || a.y - b.y)
  const uniq: { row: number; y: number }[] = []
  for (const a of anchors) {
    // 同 row 多日只取首个(排序后 = 最小 y = 该行最靠上一日,与轨道朝向一致),保证锚点 row 严格递增。
    if (uniq.length > 0 && uniq[uniq.length - 1].row === a.row) continue
    uniq.push(a)
  }
  let k = 0
  for (let r = 0; r < rows; r++) {
    // 推进 k 使 uniq[k].row <= r < uniq[k+1].row(或 k 到末)。
    while (k < uniq.length - 1 && uniq[k + 1].row <= r) k++
    const lo = uniq[k]
    const hi = k + 1 < uniq.length ? uniq[k + 1] : lo
    if (r <= lo.row || hi.row === lo.row) {
      rowJumpY[r] = lo.y
    } else if (r >= hi.row) {
      rowJumpY[r] = hi.y
    } else {
      const t = (r - lo.row) / (hi.row - lo.row)
      rowJumpY[r] = lo.y + t * (hi.y - lo.y)
    }
  }

  // 日历刻度(UTC 迭代 firstDay..lastDay 间的每个月初):月网格线 + 年标重定位。
  if (span > 0) {
    const MS = 86400000
    const d0 = new Date(firstDay * MS)
    const dEnd = new Date(lastDay * MS)
    let yy = d0.getUTCFullYear()
    let mm = d0.getUTCMonth() // 0-11
    let lastMonthRow = -1
    let lastYear = -Infinity
    // 最旧年恒有年标(评审 R4):其 1 月 1 日通常早于 firstDay、永远不会被下方循环产出——单年库
    // (不跨任何 1 月 1 日)因此曾一个年标都没有。锚在 firstDay 所在行;恰为 1 月 1 日时留给循环产出
    // 以避免重复。isFinite 防病态 epochDay 超出 Date 可表示范围时产出 NaN 年标。
    if (Number.isFinite(yy) && !(mm === 0 && d0.getUTCDate() === 1)) {
      yearTicks.push({ row: rowOf(firstDay), year: yy })
      lastYear = yy
    }
    // 跨度超上限(≈108 年,多为损坏 EXIF 离群日)时显式截断到最新端并告警(评审 R5「no silent
    // caps」:旧版从最旧端数满即停,会把用户照片所在的最新端刻度静默丢掉)。最旧端仍有年标锚兜底。
    const CAP = 1300
    const monthsInSpan = (dEnd.getUTCFullYear() - yy) * 12 + (dEnd.getUTCMonth() - mm) + 1
    if (monthsInSpan > CAP) {
      logger.warn(
        `[timeline] 日历跨度 ${monthsInSpan} 月超过刻度上限 ${CAP}(疑似损坏 EXIF 离群日期),` +
          `月/年刻度截断为最新 ${CAP} 个月`,
      )
      const startIdx = dEnd.getUTCFullYear() * 12 + dEnd.getUTCMonth() - (CAP - 1)
      yy = Math.floor(startIdx / 12)
      mm = startIdx - yy * 12
    }
    // guard:上限 = CAP 月,防异常 epochDay 死循环(正常情况在 tickDay > lastDay 处自然 break)。
    for (let guard = 0; guard < CAP; guard++) {
      const tickDay = Math.floor(Date.UTC(yy, mm, 1) / MS)
      if (tickDay > lastDay) break
      if (tickDay >= firstDay) {
        const row = rowOf(tickDay)
        if (row !== lastMonthRow) {
          monthTicks.push(row)
          lastMonthRow = row
        }
        if (mm === 0 && yy !== lastYear) {
          yearTicks.push({ row, year: yy })
          lastYear = yy
        }
      }
      mm++
      if (mm > 11) {
        mm = 0
        yy++
      }
    }
  }

  return { intensity, rowJumpY, monthTicks, yearTicks }
}

/**
 * time 坐标反查:给定 logical y,在单调 `rowJumpY` 上二分求其所在像素行的纵向比例 [0,1]。
 * 供指示线 / hover 在时间轴上正确定位——band 按时间铺时,`currentY` 仍是 item 空间,须映射回时间行,
 * 否则指示线与密度带错位。空数组 / 单行返回 0。
 * @param rowJumpY 逐行 logical y(单调非减,来自 [`buildTimeBand`])
 * @param y 目标 logical y
 */
export function logicalYToTimeFrac(rowJumpY: Float32Array | readonly number[], y: number): number {
  const n = rowJumpY.length
  if (n <= 1) return 0
  // 找第一个 rowJumpY[r] >= y 的行 lo(可能为 n)。
  let lo = 0
  let hi = n
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (rowJumpY[mid] < y) lo = mid + 1
    else hi = mid
  }
  const row = Math.min(n - 1, lo)
  return row / (n - 1)
}

/**
 * folder/none 模式 DOM 降采样（S1，§9.9）：把 N 个分组分隔符按逻辑 y 桶合并到 slotCount 个槽，
 * 每槽取落入该槽的「首个」分隔符作代表（保留真实 y + label），使渲染节点数从 O(分组) 降到 O(条像素)。
 * 分隔符数 <= slotCount 时原样返回（小库零改动零回归）。separators 须按 y 升序。
 * 注：这只降「显示节点」——全量 separators 仍在 JS，供点轨吸附（nearestSeparatorIndex）全精度定位。
 * 空槽（无分隔符落入）不产代表 → 稀疏区自然留白，视觉上反映分组疏密。
 * @param separators 分隔符（含 y，按 y 升序）
 * @param totalHeight 布局总逻辑高度（y→槽索引归一化基准）
 * @param slotCount 目标槽数（约等于轨道像素高；<=0 视为 1）
 */
export function downsampleSeparators<T extends { y: number }>(
  separators: readonly T[],
  totalHeight: number,
  slotCount: number,
): T[] {
  const n = separators.length
  const slots = Math.max(1, Math.floor(slotCount))
  if (n <= slots) return separators.slice()
  const h = totalHeight > 0 ? totalHeight : 1
  const out: T[] = []
  let lastSlot = -1
  for (const sep of separators) {
    // y 升序 → slot 单调不减，故「slot 变化」即该槽首个分隔符。
    const slot = Math.min(slots - 1, Math.max(0, Math.floor((sep.y / h) * slots)))
    if (slot !== lastSlot) {
      out.push(sep)
      lastSlot = slot
    }
  }
  return out
}

/**
 * 键盘滑块步进：据按键与当前索引算下一目标索引（已 clamp 到
 * `[0, count-1]`）。非导航键返回 null（调用方据此决定是否 preventDefault）。
 * 方向语义与轨道一致：向下（ArrowDown/PageDown）索引增（轨道下方=更旧/更靠后），向上反之；
 * Home/End 到首/尾。cur<0（尚无当前项）时以 0 为基。
 * @param cur 当前索引（可为 -1 表示未定位）
 * @param key KeyboardEvent.key
 * @param count 可导航项数（date 模式=月桶数，folder 模式=分隔符数）
 * @param page PageUp/Down 跨度（默认 10）
 */
export function stepScrubberIndex(
  cur: number,
  key: string,
  count: number,
  page = 10,
): number | null {
  if (count <= 0) return null
  const clamp = (i: number) => Math.min(count - 1, Math.max(0, i))
  const base = cur < 0 ? 0 : cur
  switch (key) {
    case 'ArrowDown':
      return clamp(base + 1)
    case 'ArrowUp':
      return clamp(base - 1)
    case 'PageDown':
      return clamp(base + page)
    case 'PageUp':
      return clamp(base - page)
    case 'Home':
      return 0
    case 'End':
      return count - 1
    default:
      return null
  }
}
