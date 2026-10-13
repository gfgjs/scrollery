import type { PluginAvailability } from '../types/exotic'

export type GateMode = 'passthrough' | 'ready' | 'activation' | 'blocked'

/** 普通格式直接展示；已知插件区分免费激活、安装和不可用状态。 */
export function gateModeFor(state: PluginAvailability | null): GateMode {
  if (!state || state.availability === 'noOffering') return 'passthrough'
  if (state.availability === 'ready') return 'ready'
  if (state.availability === 'inactive' || state.availability === 'availableUninstalled') return 'activation'
  return 'blocked'
}
