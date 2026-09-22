// src/composables/useExoticGate.ts
// Exotic 逐项 gate（Part5 T12 增量3）：把某媒体项映射为 PluginGate 可消费的授权态，查询失败单独展示。
//
// 前后端职责：本 composable 只向后端取「格式解析 / 逐项状态」并适配为展示 DTO，
//    **不持任何验签逻辑**——可用态与激活验签全在后端。
//
// 触点用法：详情覆盖层等打开某项时调 `resolveForItem(itemId, fileFormat)`；普通格式（非 exotic
// catalog）直接返回 false 且**不发** item-state IPC——避免为绝大多数 jpg/png 空跑一次往返。

import { ref } from 'vue'

import { IPC } from '../constants/ipc'
import type { ExoticItemState, FormatResolution, PluginEntitlement } from '../types/exotic'
import { invokeIpc } from '../utils/ipc'

// ── Exotic 格式集缓存（模块级单例，跨组件共享）──────────────────────────────
// Catalog 提供的格式在一次运行内基本静态；缓存一次即可，避免每次开图都拉全量解析。
// 失败**不写缓存**（本次展示查询错误），下次重试；inflight 去重并发首拉。
let exoticFormatsCache: ReadonlySet<string> | null = null
let inflight: Promise<ReadonlySet<string>> | null = null

async function loadExoticFormats(): Promise<ReadonlySet<string>> {
  if (exoticFormatsCache) return exoticFormatsCache
  if (!inflight) {
    inflight = (async () => {
      try {
        const list = await invokeIpc<FormatResolution[]>(IPC.LIST_EXOTIC_FORMAT_RESOLUTIONS)
        exoticFormatsCache = new Set(list.map((r) => r.format.toLowerCase()))
        return exoticFormatsCache
      } finally {
        inflight = null
      }
    })()
  }
  return inflight
}

/** 清空格式集缓存（测试 / 未来「刷新 Catalog」用）。 */
export function resetExoticFormatCache(): void {
  exoticFormatsCache = null
  inflight = null
}

/**
 * 把逐项 `FormatResolution` 适配为 `PluginGate` 消费的 `PluginEntitlement`（纯函数）。
 * item-state **不带** sku / sourceTag：sku 置 null（gate 的产品编号行 v-if 自动隐藏），
 * sourceTag 置空串（gate 不渲染该字段）。如需产品编号，触点可另调 get_plugin_entitlement 补齐。
 */
export function resolutionToEntitlement(res: FormatResolution): PluginEntitlement {
  return {
    pluginId: res.pluginId ?? '',
    availability: res.availability,
    sourceTag: '',
    sku: null,
    storeUrl: res.storeUrl,
  }
}

export function useExoticGate() {
  /** 当前项的 gate 授权态（null = 非 exotic / 未解析 → 触点放行渲染真实内容）。 */
  const entitlement = ref<PluginEntitlement | null>(null)
  const loading = ref(false)
  const failed = ref(false)
  let resolveGeneration = 0

  /**
   * 为某项解析 gate 授权态。
   * @returns 该项是否为需要 gate 判定的 exotic 格式（true 时 `entitlement` 已就绪）。
   *          普通格式返回 false 且不发 item-state IPC。
   */
  async function resolveForItem(itemId: number, fileFormat: string): Promise<boolean> {
    const generation = ++resolveGeneration
    entitlement.value = null
    loading.value = true
    failed.value = false
    try {
    const formats = await loadExoticFormats()
    if (generation !== resolveGeneration) return false
    if (!formats.has(fileFormat.toLowerCase())) return false

      const st = await invokeIpc<ExoticItemState>(IPC.GET_EXOTIC_ITEM_STATE, { itemId })
      if (generation !== resolveGeneration) return false
      // resolution=null 表示后端也认为非 catalog 格式（与格式集缓存竞态时的兜底）→ 放行。
      entitlement.value = st.resolution ? resolutionToEntitlement(st.resolution) : null
      return entitlement.value !== null
    } catch {
      if (generation !== resolveGeneration) return false
      entitlement.value = null
      failed.value = true
      return false
    } finally {
      if (generation === resolveGeneration) loading.value = false
    }
  }

  /** 关闭触点时清态。 */
  function reset(): void {
    failed.value = false
    resolveGeneration++
    entitlement.value = null
    loading.value = false
  }

  return { entitlement, loading, failed, resolveForItem, reset }
}
