// AccordionSection 展开揭示的滚动数学(与组件分离以便单测,承接 folderTree.helpers 惯例)。
// 纯函数、无 DOM / 无响应式。
//
// 背景:侧栏手风琴的区块标题是双向 sticky(粘顶+粘底堆叠),而主体留在文档流原位。内容极长时,
// 被钉住的标题与其主体在文档流中可能相隔整个滚动范围——点击展开只见箭头变化,主体在画面外
// 展开(bug:「菜单原地没动」)。展开后须把主体滚进可视区,且要避开上/下两侧的粘性标题堆叠。

/**
 * 求「把刚展开的主体滚进可视区」应设的滚动位置——scrollIntoView block:'nearest' 语义,
 * 但把上下的粘性标题堆叠算成不可用区(inset)。已完全可见则不动(返回 cur)。
 *
 * 几何:主体完全可见 ⇔ scrollTop ∈ [alignBottom, alignTop],其中
 *   alignTop    = bodyFlowTop − topInset            (主体顶恰贴在上方标题堆叠之下)
 *   alignBottom = bodyFlowTop + bodyH + bottomInset − viewportH  (主体底恰贴在下方堆叠之上)
 * 区间为空(主体+堆叠高于视口)时对齐顶部,让用户从主体开头看起。
 *
 * @param cur 当前 scrollTop
 * @param bodyFlowTop 主体在滚动内容坐标系中的文档流顶(主体非 sticky,rect 可直接换算;标题 rect
 *                    反映的是钉住后的视觉位置,不可用)
 * @param bodyH 主体展开后的完整高度
 * @param viewportH 滚动区可视高
 * @param topInset 主体上方粘顶标题的堆叠总高(含其自身标题,= (index+1) × headerH)
 * @param bottomInset 主体下方粘底标题的堆叠总高(= (total−1−index) × headerH)
 */
export function clampRevealScrollTop(
  cur: number,
  bodyFlowTop: number,
  bodyH: number,
  viewportH: number,
  topInset: number,
  bottomInset: number,
): number {
  const alignTop = bodyFlowTop - topInset
  const alignBottom = bodyFlowTop + bodyH + bottomInset - viewportH
  // 区间为空(主体高于视口)时两界同取 alignTop → 恒对齐顶部。
  const lo = Math.min(alignBottom, alignTop)
  const hi = alignTop
  return Math.max(lo, Math.min(cur, hi))
}

/**
 * 「点击意图化」判据:展开态区块被点击时,若其主体已被粘性标题堆叠挤出可用视口带(几乎不可见),
 * 点击应「揭示」(滚回来)而非「折叠」——否则用户看到的"折叠外观"其实是滚动造成的,状态仍是展开,
 * 一次点击先真折叠再一次才展开,凭空多一次点击 + 揭示动画顿挫(见 AccordionSection 用法)。
 *
 * 判据取「主体与可用带的交集高度 ≤ eps」,即主体基本完全在带外。刻意用交集高度而非可见占比:
 *  - 图库/工具被推出顶部、管理被推出底部 → 交集 0 → 揭示 ✓(正是 bug 场景);
 *  - 文件夹这类超高主体在滚进树时始终与带相交(交集 > 0)→ 折叠照常生效 ✓,不会误判成"揭示回树顶";
 *  - 屏内哪怕只露一条 → 交集 > eps → 折叠照常,不打扰。
 *
 * 可用带 = [scrollTop + topInset, scrollTop + viewportH − bottomInset](扣除上下不透明 sticky 堆叠)。
 * inset 语义与 clampRevealScrollTop 完全一致,便于两者共用同一套几何量。
 *
 * @param eps 视为「不可见」的交集高度上限(px),默认 1(容忍亚像素/边界)
 * @returns true 表示应揭示(不折叠);false 表示照常 toggle
 */
export function bodyRevealNeeded(
  scrollTop: number,
  bodyFlowTop: number,
  bodyH: number,
  viewportH: number,
  topInset: number,
  bottomInset: number,
  eps = 1,
): boolean {
  if (bodyH <= 0) return false // 无主体(空区块)→ 无所谓揭示,交给 toggle
  const bandTop = scrollTop + topInset
  const bandBottom = scrollTop + viewportH - bottomInset
  const visTop = Math.max(bodyFlowTop, bandTop)
  const visBottom = Math.min(bodyFlowTop + bodyH, bandBottom)
  return visBottom - visTop <= eps
}
