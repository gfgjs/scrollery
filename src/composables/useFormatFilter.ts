// src/composables/useFormatFilter.ts
// 细分格式筛选的取数与跨维度不变量（S 线 §7）。纯逻辑在 `formatFilter.helpers.ts`，本文件只
// 负责「发请求 / 接 store / 维持不变量」。

import { computed, ref, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'
import { useFilterStore } from '../stores/filterStore'
import type { FormatDescriptor } from '../types/format'
import type { Availability, FormatResolution } from '../types/exotic'
import type { MediaType } from '../types/media'
import {
  buildFormatGroups,
  filterGroupsByQuery,
  pruneFormatsForTypes,
} from '../components/layout/formatFilter.helpers'

/**
 * registry 是**进程级**共享缓存：内置表编译期固定，Catalog 只在插件装卸时变。每开一次弹层重拉
 * 一遍纯属浪费，而它又不该跨会话持久（插件装卸后必须能变）。
 *
 * facet **不缓存**：实测 `DISTINCT file_format` + 基础谓词 **0.0ms**（走 `idx_media_format`
 * 覆盖索引），而它的真相源是库、扫描一次就变 —— 为 0ms 建缓存层再配失效逻辑是纯负债（D-005）。
 */
let registryCache: FormatDescriptor[] | null = null
let registryInflight: Promise<FormatDescriptor[]> | null = null

/** 测试与「插件装卸后」用：丢弃 registry 缓存。 */
export function resetFormatRegistryCache(): void {
  registryCache = null
  registryInflight = null
}

async function loadRegistry(): Promise<FormatDescriptor[]> {
  if (registryCache) return registryCache
  // 在途去重：弹层与剪枝 watch 可能同帧各发一次。
  if (!registryInflight) {
    registryInflight = invokeIpc<FormatDescriptor[]>(IPC.LIST_REGISTERED_FORMATS)
      .then((list) => {
        registryCache = list
        return list
      })
      .finally(() => {
        registryInflight = null
      })
  }
  return registryInflight
}

export function useFormatFilter() {
  const filter = useFilterStore()

  const registry = ref<FormatDescriptor[]>([])
  /** 库内实际存在的扩展名（facet）。 */
  const present = ref<ReadonlySet<string>>(new Set())
  /** exotic 格式的可用态：`ext → Availability`。缺席 = 内置格式，无需 badge。 */
  const availability = ref<ReadonlyMap<string, Availability>>(new Map())
  const loading = ref(false)
  const query = ref('')

  /**
   * 拉取 registry + facet + exotic 可用态。**每次开弹层调用**。
   *
   * 三者都失败静默：格式筛选是增强项，取数失败时弹层显示空态即可，不该弹错误打断浏览。
   * availability 单独失败更无所谓 —— 那只影响 badge，不影响能不能筛（§2：availability ⊥ registered）。
   */
  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const [reg, facet] = await Promise.all([
        loadRegistry(),
        invokeIpc<string[]>(IPC.LIST_LIBRARY_FORMATS),
      ])
      registry.value = reg
      present.value = new Set(facet)
      // exotic 可用态复用既有事实源（useExoticGate 同款 IPC），不自造第二套判定。
      try {
        const res = await invokeIpc<FormatResolution[]>(IPC.LIST_EXOTIC_FORMAT_RESOLUTIONS)
        availability.value = new Map(res.map((r) => [r.format.toLowerCase(), r.availability]))
      } catch (e) {
        logger.warn('[useFormatFilter] exotic 可用态取数失败，badge 降级不显示', { error: e })
      }
    } catch (e) {
      logger.error('[useFormatFilter] 格式取数失败', { error: e })
    } finally {
      loading.value = false
    }
  }

  /** `ext → mediaType`（剪枝用）。 */
  const extToType = computed(() => {
    const m = new Map<string, MediaType>()
    for (const d of registry.value) m.set(d.ext, d.mediaType)
    return m
  })

  const groups = computed(() =>
    filterGroupsByQuery(
      buildFormatGroups(
        registry.value,
        present.value,
        new Set(filter.fileFormats),
        filter.mediaTypes,
      ),
      query.value,
    ),
  )

  /** 触发器文案用：已选的具体扩展名个数（不是别名组数 —— 状态的单位始终是扩展名）。 */
  const selectedCount = computed(() => filter.fileFormats.length)

  /**
   * 🔴 跨维度不变量（§7.5）：取消某媒体大类 → 移除该类下已选的格式。
   *
   * 挂在这里而非 `filterStore.toggleMediaType` 里：剪枝需要 registry 的 `ext → mediaType` 知识，
   * 而 store 不该持有 registry（那是运行时并集、要发 IPC）。**故本 composable 必须在顶栏常驻
   * 挂载**（`GalleryFilterChips` 的 inline 实例即可）—— 挂在弹层里就只有开着弹层时才剪，
   * 而用户取消大类 chip 时弹层通常是关的。
   *
   * registry 未就绪时不剪：`extToType` 为空 → `pruneFormatsForTypes` 保留一切未知扩展名 ——
   * 宁可晚剪一拍，也不能因为「还没拉到 registry」就把用户的选择清空。
   */
  watch(
    () => filter.mediaTypes,
    (types) => {
      if (filter.fileFormats.length === 0) return
      const kept = pruneFormatsForTypes(filter.fileFormats, types, extToType.value)
      if (kept.length !== filter.fileFormats.length) filter.setFileFormats(kept)
    },
  )

  // 剪枝依赖 registry，而用户可能在弹层从未打开过的情况下就点了大类 chip。故常驻实例挂载时
  // 就把 registry 拉起来（一次进程级缓存，不重复）。facet 仍留给 refresh() 按需拉。
  void loadRegistry()
    .then((r) => {
      registry.value = r
    })
    .catch(() => {
      /* 静默：refresh() 会再试一次，且未就绪时剪枝安全退化为不剪 */
    })

  return { registry, present, availability, loading, query, groups, selectedCount, refresh }
}
