import { computed, ref } from 'vue'
import { IPC } from '../constants/ipc'
import type { FeatureActivation, PluginAvailability } from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

/** 单项激活查询；迟到结果不能覆盖更新后的状态。 */
export function useFeatureActivation(featureId: string) {
  const activation = ref<FeatureActivation | null>(null)
  const loading = ref(false)
  const error = ref<IpcError | null>(null)
  let generation = 0
  async function fetchActivation(): Promise<FeatureActivation | null> {
    const current = ++generation
    loading.value = true
    error.value = null
    activation.value = null
    try {
      const result = await invokeIpc<FeatureActivation>(IPC.GET_FEATURE_ACTIVATION, { featureId })
      if (current !== generation) return null
      activation.value = result
      return result
    } catch (cause) {
      if (current === generation) error.value = cause as IpcError
      return null
    } finally {
      if (current === generation) loading.value = false
    }
  }
  const isActivated = computed(() => activation.value?.activated === true)
  const availability = computed<PluginAvailability | null>(() => activation.value ? {
    pluginId: featureId, availability: activation.value.activated ? 'ready' : 'inactive',
    activationRequired: true, activated: activation.value.activated,
  } : null)
  return { activation, availability, loading, error, fetchActivation, isActivated }
}
