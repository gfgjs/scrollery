// 图片编辑高级功能授权态：后端复用 EntitlementProvider，前端只消费展示 DTO。
// 与通用 exotic gate 不同，本功能是付费真门；授权查询失败必须 fail-closed，不能因 null 放行。

import { computed, ref } from 'vue'

import { IPC } from '../constants/ipc'
import type { PluginEntitlement } from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

export const EDITING_PLUGIN_ID = 'feature-editing'

function failClosedEntitlement(): PluginEntitlement {
  return {
    pluginId: EDITING_PLUGIN_ID,
    availability: 'installedUnlicensed',
    sourceTag: 'unavailable',
    sku: 'editing-tools-2026',
    storeUrl: null,
  }
}

export function useEditingEntitlement() {
  const entitlement = ref<PluginEntitlement | null>(null)
  const loading = ref(false)
  const activating = ref(false)
  const error = ref<IpcError | null>(null)

  async function fetchEntitlement(): Promise<PluginEntitlement> {
    loading.value = true
    error.value = null
    try {
      const result = await invokeIpc<PluginEntitlement>(IPC.GET_EDITING_ENTITLEMENT)
      entitlement.value = result
      return result
    } catch (cause) {
      error.value = cause as IpcError
      // `PluginGate` 对 null 会放行；付费编辑不能沿用该 fail-open 语义。
      const fallback = failClosedEntitlement()
      entitlement.value = fallback
      return fallback
    } finally {
      loading.value = false
    }
  }

  async function activate(_pluginId: string, token: string): Promise<void> {
    activating.value = true
    try {
      await invokeIpc(IPC.ACTIVATE_EDITING_FEATURE, { token })
    } finally {
      activating.value = false
    }
  }

  const isAuthorized = computed(() => entitlement.value?.availability === 'authorized')

  return { entitlement, loading, activating, error, fetchEntitlement, activate, isAuthorized }
}
