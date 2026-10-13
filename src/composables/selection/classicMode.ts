// src/composables/selection/classicMode.ts
// 经典选区模式：单击替换、Ctrl 翻转、Shift 区间、Ctrl+A 全选及反选。
// apply 只依据旧态、意图与上下文返回新态，不修改输入。

import type { SelectionMode, SelectionState } from './types'

/**
 * 在 explicit 集上翻转单 id（不可变：返回新 Set）。
 * all 态的「翻转」语义相反 —— 操作的是 excluded 集,故各自处理,不共用此助手。
 */
function toggleInSet(set: ReadonlySet<number>, id: number): Set<number> {
  const next = new Set(set)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  return next
}

export const classicMode: SelectionMode = {
  id: 'classic',
  label: '经典（桌面照片管理器）',

  apply(state: SelectionState, intent, ctx): SelectionState {
    switch (intent.type) {
      // 单击：清空既有,仅选中单项 → 永远落到 explicit 单元素
      case 'replace':
        return { kind: 'explicit', ids: new Set([intent.id]) }

      // Ctrl/Cmd+单击：翻转单项。两态语义相反 ——
      //   explicit：在 ids 中加/减；all：在 excluded 中加/减（排除=从全选里挖掉）
      case 'toggle':
        if (state.kind === 'explicit') {
          return { kind: 'explicit', ids: toggleInSet(state.ids, intent.id) }
        }
        return { kind: 'all', excluded: toggleInSet(state.excluded, intent.id) }

      // Shift+单击 / 框选：布局序区间并入。区间 id 由 ctx.rangeBetween 在 flat_ids 上算（跨视口稳定）。
      //   explicit：union 当前选区 + 区间；all：区间表示「选中这些」→ 从 excluded 中移除它们
      case 'range': {
        const rangeIds = ctx.rangeBetween(intent.anchorId, intent.toId)
        if (state.kind === 'explicit') {
          const next = new Set(state.ids)
          for (const id of rangeIds) next.add(id)
          return { kind: 'explicit', ids: next }
        }
        const next = new Set(state.excluded)
        for (const id of rangeIds) next.delete(id)
        return { kind: 'all', excluded: next }
      }

      // Ctrl/Cmd+A：全选语义,不物化百万 id
      case 'selectAll':
        return { kind: 'all', excluded: new Set() }

      // Esc / 点空白：清空 → explicit 空态
      case 'clear':
        return { kind: 'explicit', ids: new Set() }

      // 反选:
      //   explicit{ids} → explicit{ 全集中不在 ids 的 }（必须物化,反选本就是全集运算）
      //   all{excluded} → explicit{ excluded }（全选去掉 excluded 的补集,恰好就是 excluded 本身,廉价）
      case 'invert': {
        if (state.kind === 'all') {
          return { kind: 'explicit', ids: new Set(state.excluded) }
        }
        const next = new Set<number>()
        for (const id of ctx.viewIds) {
          if (!state.ids.has(id)) next.add(id)
        }
        return { kind: 'explicit', ids: next }
      }
    }
  },
}
