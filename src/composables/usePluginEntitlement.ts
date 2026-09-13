// src/composables/usePluginEntitlement.ts
// 插件授权态 composable（Part5 T12，消费 Part6 get_plugin_entitlement）。
//
// 前后端职责：本 composable **只**向后端取授权态并映射为 UI 展示判定
//    （是否显 gate / 购买引导），**不持任何验签逻辑**——授权真相全在后端 EntitlementProvider。

import { ref, computed } from 'vue'

import { IPC } from '../constants/ipc'
import type { PluginEntitlement, Availability } from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

/** 需要「购买 / 激活」引导的可用态（未授权但**有产品可领**）。其余为可运行或纯不可用（不引导购买）。 */
const GATED: ReadonlySet<Availability> = new Set<Availability>([
  'availableUninstalled',
  'installedUnlicensed',
  'licenseExpired',
])

/**
 * Gate 展示模式（`PluginGate.vue` 据此选择渲染分支）。
 * - `passthrough`：直接放行（无判定 / 无产品可售 → 不拦截，避免误藏功能）
 * - `authorized`：已授权 → 渲染真实功能
 * - `purchase`：未授权但有产品 → 显功能说明 + 购买/激活引导
 * - `blocked`：纯不可用（平台/版本/损坏/禁用）→ 只做信息提示，不引导购买
 */
export type GateMode = 'passthrough' | 'authorized' | 'purchase' | 'blocked'

/**
 * 把后端授权态映射为 gate 展示模式（纯函数，gate 逻辑的单一事实源）。
 * 对 `null` 与 `noOffering` 一律**放行**——gate 只在拿到明确「有产品但未授权」判定时才拦截，
 * 不确定时不藏功能（fail-open）。
 */
export function gateModeFor(e: PluginEntitlement | null): GateMode {
  if (!e) return 'passthrough'
  if (e.availability === 'authorized') return 'authorized'
  if (GATED.has(e.availability)) return 'purchase'
  if (e.availability === 'noOffering') return 'passthrough'
  return 'blocked'
}

export function usePluginEntitlement() {
  const entitlement = ref<PluginEntitlement | null>(null)
  const loading = ref(false)
  /** 最近一次错误。注意：'no_offering'（无产品）是**预期分支**，非硬故障，调用方通常静默不显 gate。 */
  const error = ref<IpcError | null>(null)

  /**
   * 拉取某插件授权态。成功返回 DTO；失败（含 'no_offering'）返回 null 并置 `error`。
   * @param pluginId 插件 id（取自 Catalog / resolve_format，**非**前端任意输入）
   */
  async function fetchEntitlement(pluginId: string): Promise<PluginEntitlement | null> {
    loading.value = true
    error.value = null
    try {
      const e = await invokeIpc<PluginEntitlement>(IPC.GET_PLUGIN_ENTITLEMENT, { pluginId })
      entitlement.value = e
      return e
    } catch (e) {
      error.value = e as IpcError
      entitlement.value = null
      return null
    } finally {
      loading.value = false
    }
  }

  /** 是否为「无此产品」——后端 catalog 无 offering（预期分支，调用方据此不显 gate）。 */
  const isNoOffering = computed(
    () => error.value?.code === 'no_offering' || entitlement.value?.availability === 'noOffering',
  )

  /** 已授权、可运行。 */
  const isAuthorized = computed(() => entitlement.value?.availability === 'authorized')

  /** 需要 gate（未授权但有产品可领 → 显功能说明 + 购买/激活引导）。 */
  const isGated = computed(() => {
    const a = entitlement.value?.availability
    return a !== undefined && GATED.has(a)
  })

  /** 购买 / 商店链接（无则 null）——「购买」按钮跳转用。 */
  const storeUrl = computed(() => entitlement.value?.storeUrl ?? null)

  return {
    entitlement,
    loading,
    error,
    fetchEntitlement,
    isNoOffering,
    isAuthorized,
    isGated,
    storeUrl,
  }
}
