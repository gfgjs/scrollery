// src/composables/useFolderTreeIndices.spec.ts
// 回归网:锁死 useFolderTreeIndices 必须收「直连 ref」而非「包一层 computed 转发」。
// 背景 bug:FoldersSection.vue 曾以 `computed(() => folderTree.nodes.value)` 包装后传入,
// 而 useFolderTree.nodes 是 shallowRef——靠原地 mutate(splice)+ triggerRef 通知变化,数组
// 引用不变。computed 对同一引用判定「值未变」而不重算,下游索引永久停留在初始快照(直至某次
// 恰好整体替换数组触发引用变化)。本用例钉死:shallowRef 数组原地 splice + triggerRef 后,
// 索引必须含新节点——只有直传 ref 本体才能穿透 triggerRef。

import { describe, it, expect } from 'vitest'
import { shallowRef, triggerRef } from 'vue'
import { useFolderTreeIndices } from './useFolderTreeIndices'
import type { DirNode } from '../types/media'

function node(id: number, parentId: number | null, depth: number): DirNode {
  const relPath = 'dir' + id
  return {
    nodeKey: '1:' + relPath,
    parentKey: parentId === null ? null : '1:dir' + parentId,
    id,
    rootId: 1,
    parentId,
    name: relPath,
    relPath,
    depth,
    mediaCount: 0,
    hasChildren: false,
  }
}

describe('useFolderTreeIndices — shallowRef 原地 mutate 穿透', () => {
  it('splice 追加节点 + triggerRef 后,nodesById/nodesByKey 含新节点', () => {
    const nodes = shallowRef<readonly DirNode[]>([node(1, null, 0)])
    const { nodesById, nodesByKey } = useFolderTreeIndices(nodes)

    expect(nodesById.value.has(1)).toBe(true)
    expect(nodesById.value.has(2)).toBe(false)

    // 原地 mutate(与 useFolderTree.loadChildren 同款手法):splice 而非整体替换数组引用。
    ;(nodes.value as DirNode[]).splice(1, 0, node(2, 1, 1))
    triggerRef(nodes)

    expect(nodesById.value.has(2)).toBe(true)
    expect(nodesById.value.get(2)?.parentId).toBe(1)
    expect(nodesByKey.value.has('1:dir2')).toBe(true)
  })

  it('isDescendant 在 splice 追加祖先链后正确判定新节点', () => {
    const nodes = shallowRef<readonly DirNode[]>([node(1, null, 0)])
    const { isDescendant } = useFolderTreeIndices(nodes)

    expect(isDescendant(1, 2)).toBe(false) // 节点 2 尚不存在

    ;(nodes.value as DirNode[]).splice(1, 0, node(2, 1, 1))
    triggerRef(nodes)

    expect(isDescendant(1, 2)).toBe(true)
  })
})
