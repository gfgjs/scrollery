// src/composables/selection/sweep.ts
// 框选扫过区间的「反转」算子 —— 纯函数,脱离 DOM/指针,便于单测。
//
// 语义(真机交互「本体滑动 = 反转扫选」):对布局序区间 rangeIds 中的每个 id,相对拖拽起手基线
// baseline 做一次翻转 —— 基线含则移除、不含则加入(相对基线 XOR)。因每次 pointermove 都以
// 「anchor..current 全区间」重算,回弹/收缩天然正确(移出区间的项恢复基线状态),等价「对划过的
// 内容做一次选择状态反转」;来回滑过同一格不会重复翻转(与旧固定 select/deselect 框选的区别)。
//
// ⚠️ 判据用 baseline.has(id) 而非 next.has(id):即便 rangeIds 含重复项(理论上 flat_ids 区间无重复,
//    但纯函数须对入参鲁棒),也保证每个 id「相对基线只翻一次」,不被重复项双翻。

/**
 * 把区间 rangeIds 相对 baseline 做一次反转,产出新选区集合(不改入参)。
 * @param baseline 拖拽起手时的选区快照。
 * @param rangeIds 本次扫过的布局序区间 id(anchor..current 全量)。
 * @returns 新的显式选区 Set —— baseline 中区间内的项被翻转。
 */
export function applyRangeInvert(
  baseline: ReadonlySet<number>,
  rangeIds: Iterable<number>,
): Set<number> {
  const next = new Set(baseline)
  for (const id of rangeIds) {
    if (baseline.has(id)) next.delete(id)
    else next.add(id)
  }
  return next
}
