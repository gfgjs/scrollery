import { computed, ref } from 'vue'

import { IPC } from '../constants/ipc'
import type { PluginEntitlement } from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

/** 官方版授权查询；失败保留独立错误态，避免误显示购买或授权成功。 */
export function useOfficialEntitlement() {
  const entitlement = ref<PluginEntitlement | null>(null)
  const loading = ref(false)
  const error = ref<IpcError | null>(null)
  let generation = 0

  async function fetchEntitlement(): Promise<PluginEntitlement | null> {
    const current = ++generation
    loading.value = true
    error.value = null
    entitlement.value = null
    try {
      const result = await invokeIpc<PluginEntitlement>(IPC.GET_OFFICIAL_ENTITLEMENT)
      if (current !== generation) return null
      entitlement.value = result
      return result
    } catch (cause) {
      if (current === generation) error.value = cause as IpcError
      return null
    } finally {
      if (current === generation) loading.value = false
    }
  }

  const isAuthorized = computed(() => entitlement.value?.availability === 'authorized')
  return { entitlement, loading, error, fetchEntitlement, isAuthorized }
}
