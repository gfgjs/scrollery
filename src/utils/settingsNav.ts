/**
 * 设置页分区导航的 scroll-spy 判据（纯决策源，与 DOM 无关，可单测）。
 *
 * 背景（2026-07-16 真机 round10）：原判据「取最后一个 offsetTop ≤ 探针的分区」隐含假设**每个分区都能被
 * 滚到探针位**。对**最后一个**分区，该假设仅当它自身高度 ≥ 视口高时才成立——一旦它比视口矮（如 advanced
 * 分区把「危险操作」「开发者工具」两张卡折起来），它的 offsetTop 就落在**不可达的滚动区间**里，探针永远
 * 越不过去，判据不是「算错」而是**永远不可能返回它**。真机症状：折叠任一卡后点「高级」，高亮留在「存储与
 * 设备」，须再点一次（第二次点击时已在底部、scrollTo 不产生 scroll 事件，spy 不回填，置位才活下来）。
 *
 * 故加**触底不变量**：滚到底时可见区末尾必然是最后一个分区，直接判它，不问 offsetTop。
 */

/** 触底判定容差（px）：吸收 subpixel / 缩放导致的 scrollTop+clientHeight 与 scrollHeight 的舍入差。 */
const BOTTOM_EPSILON = 2

export interface SectionOffset<T extends string> {
  id: T
  /** 分区元素相对滚动容器的 offsetTop（px）。 */
  offsetTop: number
}

export interface ScrollGeometry {
  scrollTop: number
  clientHeight: number
  scrollHeight: number
}

/**
 * 按滚动位置解析当前应高亮的分区。
 *
 * @param sections 分区列表（**须按 offsetTop 升序**，即注册表的显示顺序）
 * @param geo 滚动容器几何
 * @param probeOffset 探针偏移（px）：容器顶往下多少距离处的分区算「当前」。缺省 32。
 * @returns 应高亮的分区 id；空列表返回 null
 */
export function resolveVisibleSection<T extends string>(
  sections: ReadonlyArray<SectionOffset<T>>,
  geo: ScrollGeometry,
  probeOffset = 32,
): T | null {
  if (sections.length === 0) return null
  // 触底 → 末分区。见文件头：末分区矮于视口时其 offsetTop 不可达，探针法对它永远失效。
  if (geo.scrollTop + geo.clientHeight >= geo.scrollHeight - BOTTOM_EPSILON) {
    return sections[sections.length - 1].id
  }
  const marker = geo.scrollTop + probeOffset
  let visible = sections[0].id
  for (const section of sections) {
    if (section.offsetTop <= marker) visible = section.id
  }
  return visible
}
