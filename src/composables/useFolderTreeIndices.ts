// src/composables/useFolderTreeIndices.ts
// 目录节点的双身份索引(D-013:实体轴 id / 结构轴 nodeKey)+ 祖先判定——从 FoldersSection.vue
// 域 15 迁出(见 docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
// 纯索引 + 一个递归查找函数,无 DOM/生命周期。
import { computed, type Ref } from 'vue'
import type { DirNode } from '../types/media'

// 参数收 `Ref<readonly DirNode[]>` 而非 `ComputedRef`:useFolderTree.nodes 是 shallowRef,靠
// 原地 mutate + triggerRef 通知变化。调用方若包一层 `computed(() => nodes.value)` 转发,
// computed 会拿同一数组引用判定「值未变」而不重算,下游索引永久停留在初始快照——直传 ref
// 本体才能穿透 triggerRef(FoldersSection-vue 拆分复核实测复现)。
export function useFolderTreeIndices(nodes: Ref<readonly DirNode[]>) {
  // 落点合法性判定在拖拽期间每帧执行(pointermove + 边缘自动滚动 rAF),线性 find 是
  // O(节点数×深度)/帧,大树上顶不住;按 id 建索引,单次查询降为 O(深度)。
  // **实体轴**索引:只服务拖拽/移动/复制——那些能力按 §4.1 本就不对 FS-only 目录开放,故留在 id 上。
  const nodesById = computed(() => {
    const m = new Map<number, DirNode>()
    // FS-only 目录没有实体身份,**不入**本索引——它们本就不参与拖放/移动/复制(§4.1)。
    // 这不是「跳过异常数据」,而是「实体索引只索引实体」:查不到 = 该节点没有实体身份,
    // 正是调用方需要知道的事实。
    for (const n of nodes.value) if (n.id !== null) m.set(n.id, n)
    return m
  })

  // **结构轴**索引(路径身份,D-013):服务与 DB 实体无关的树操作(分页「加载更多」的归属目录反查等)。
  // 与 nodesById 并存不是重复——两者索引的是**不同的身份**,消费者按自己需要哪种身份来选。
  const nodesByKey = computed(() => {
    const m = new Map<string, DirNode>()
    for (const n of nodes.value) m.set(n.nodeKey, n)
    return m
  })

  /** Is `nodeId` inside the subtree rooted at `ancestorId`? | `nodeId` 是否在 `ancestorId` 子树内？ */
  function isDescendant(ancestorId: number, nodeId: number): boolean {
    let cur = nodesById.value.get(nodeId)
    while (cur && cur.parentId != null) {
      if (cur.parentId === ancestorId) return true
      cur = nodesById.value.get(cur.parentId)
    }
    return false
  }

  return { nodesById, nodesByKey, isDescendant }
}
