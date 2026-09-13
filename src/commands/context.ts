// 从当前 app 状态构造 CommandContext 快照(DRY:右键菜单、上下文工具栏共用此构造入口,
// 避免每处各自拼 ctx)。命令消费方在事件触发点(右键 / 点按钮)调用,拿到即时态快照。

import { useViewerStore } from '../stores/viewerStore'
import { useSelection } from '../composables/useSelection'
import type { CommandContext, ContextTarget } from './types'

export interface BuildCtxOptions {
  /** 上下文动作的目标资产(右键点中的项 / 单项操作对象);缺省 null。 */
  contextTarget?: ContextTarget | null
}

/**
 * 构造当前 CommandContext 快照。view 由 viewerStore.activeViewer 判定(非空=viewer,否则 grid);
 * selection 从 useSelection 模块级单例读(随处读到同一份当前选区)。
 */
export function buildCommandContext(opts: BuildCtxOptions = {}): CommandContext {
  const viewer = useViewerStore()
  const selection = useSelection()
  const count = selection.selectedCount.value
  return {
    view: viewer.activeViewer ? 'viewer' : 'grid',
    activeViewer: viewer.activeViewer,
    selection: {
      count,
      isSingle: count === 1,
    },
    contextTarget: opts.contextTarget ?? null,
  }
}
