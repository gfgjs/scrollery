// src/composables/useExoticStore.ts
// 插件商店数据层（Part5 T11，消费 Part6 registry/install/processing 命令）。
//
// 前后端职责：本 composable 只做「取列表 / 触发安装生命周期 / 读进度」的薄封装；
//    验签 / 防回滚 / 安装完整性校验全在后端（命令只接受已校验 pluginId，绝不接受 URL/路径/hash）。
//    前端不解析包、不碰下载坐标、不持验签逻辑。

import { ref } from 'vue'

import { IPC } from '../constants/ipc'
import type {
  ExoticInstallState,
  ExoticProcessingStatus,
  ExoticRegistryEntry,
  InstalledExoticPlugin,
  RegistrySummary,
} from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

/**
 * 商店展示行（registry × installed 合并）。UI 据此渲染「可装 / 已装 / 可升级 / 损坏」。
 * `installState=null` 即未安装（仅在 registry 中）；`registryEntry=null` 即已装但当前 registry 无对应
 * （远程下架 / 平台不匹配）——仍需展示以支持卸载/修复。
 */
export interface StorePluginRow {
  pluginId: string
  /** 展示名（来自 Registry/Catalog；旧数据回退 pluginId）。 */
  name: string | null
  /** registry 可安装版本（未在 registry 时为 null）。 */
  availableVersion: string | null
  /** 已安装版本（未安装时为 null）。 */
  installedVersion: string | null
  installState: ExoticInstallState | null
  formats: string[]
  sku: string | null
  storeUrl: string | null
  registryExpired: boolean
  /** 已装且 registry 有更高 packageSequence → 可升级。 */
  upgradable: boolean
}

/**
 * 合并 registry 与 installed 为展示行（纯函数，商店行模型的单一事实源）。
 * 按 pluginId 全外连接：registry-only=可装，installed-only=已装（registry 无），both=已装+可查升级。
 */
export function mergeStorePlugins(
  registry: ExoticRegistryEntry[],
  installed: InstalledExoticPlugin[],
): StorePluginRow[] {
  const byId = new Map<string, StorePluginRow>()

  for (const r of registry) {
    byId.set(r.pluginId, {
      pluginId: r.pluginId,
      name: r.name ?? r.pluginId,
      availableVersion: r.version,
      installedVersion: null,
      installState: null,
      formats: r.formats,
      sku: r.sku,
      storeUrl: r.storeUrl,
      registryExpired: r.registryExpired,
      upgradable: false,
    })
  }

  for (const inst of installed) {
    const row = byId.get(inst.pluginId)
    const regEntry = registry.find((r) => r.pluginId === inst.pluginId)
    // registry 有更高 package_sequence（且未过期）→ 可升级。
    const upgradable =
      !!regEntry && !regEntry.registryExpired && regEntry.packageSequence > inst.packageSequence
    if (row) {
      row.installedVersion = inst.version
      row.installState = inst.installState
      row.upgradable = upgradable
    } else {
      // 已装但当前 registry 无此条目（下架/平台不匹配）：仍展示以支持卸载/修复。
      byId.set(inst.pluginId, {
        pluginId: inst.pluginId,
        name: inst.pluginId,
        availableVersion: null,
        installedVersion: inst.version,
        installState: inst.installState,
        formats: [],
        sku: null,
        storeUrl: null,
        registryExpired: false,
        upgradable: false,
      })
    }
  }

  return Array.from(byId.values())
}

export function useExoticStore() {
  const registry = ref<ExoticRegistryEntry[]>([])
  const installed = ref<InstalledExoticPlugin[]>([])
  const status = ref<ExoticProcessingStatus | null>(null)
  const loading = ref(false)
  const error = ref<IpcError | null>(null)

  /** 从本地缓存列出可安装条目（无缓存→空）。 */
  async function loadRegistry(): Promise<void> {
    registry.value = await invokeIpc<ExoticRegistryEntry[]>(IPC.LIST_EXOTIC_REGISTRY)
  }

  /** 列已安装插件。 */
  async function loadInstalled(): Promise<void> {
    installed.value = await invokeIpc<InstalledExoticPlugin[]>(IPC.LIST_INSTALLED_EXOTIC_PLUGINS)
  }

  /** 读处理进度摘要。 */
  async function loadStatus(): Promise<void> {
    status.value = await invokeIpc<ExoticProcessingStatus>(IPC.GET_EXOTIC_PROCESSING_STATUS)
  }

  /** 一次性刷新商店三态（列表 + 已装 + 进度）。失败置 `error` 不抛（列表视图容错）。 */
  async function loadAll(): Promise<void> {
    loading.value = true
    error.value = null
    try {
      await Promise.all([loadRegistry(), loadInstalled(), loadStatus()])
    } catch (e) {
      error.value = e as IpcError
    } finally {
      loading.value = false
    }
  }

  /** 拉取远程签名 Registry（验签+防回滚，后端完成），成功后本地列表已更新——重载列表。 */
  async function refreshRegistry(): Promise<RegistrySummary> {
    const summary = await invokeIpc<RegistrySummary>(IPC.FETCH_EXOTIC_REGISTRY)
    await loadRegistry()
    return summary
  }

  // ── 安装生命周期（错误上抛供视图 toast；成功后重载相关列表）─────────────────
  // 参数只传 pluginId（后端校验字符集，绝不接受路径/URL）。

  async function install(pluginId: string): Promise<void> {
    await invokeIpc(IPC.INSTALL_EXOTIC_PLUGIN, { pluginId })
    await loadInstalled()
  }

  async function uninstall(pluginId: string): Promise<void> {
    await invokeIpc(IPC.UNINSTALL_EXOTIC_PLUGIN, { pluginId })
    await loadInstalled()
  }

  async function repair(pluginId: string): Promise<void> {
    await invokeIpc(IPC.REPAIR_EXOTIC_PLUGIN, { pluginId })
    await loadInstalled()
  }

  async function rollback(pluginId: string): Promise<void> {
    await invokeIpc(IPC.ROLLBACK_EXOTIC_PLUGIN, { pluginId })
    await loadInstalled()
  }

  // ── 处理控制（恢复/暂停/停止本次运行/重试失败）；均后随刷新进度 ────────────────

  async function startProcessing(): Promise<void> {
    await invokeIpc(IPC.START_EXOTIC_PROCESSING)
    await loadStatus()
  }

  async function pauseProcessing(): Promise<void> {
    await invokeIpc(IPC.PAUSE_EXOTIC_PROCESSING)
    await loadStatus()
  }

  async function stopProcessing(): Promise<void> {
    await invokeIpc(IPC.STOP_EXOTIC_PROCESSING)
    await loadStatus()
  }

  /** 重试某插件全部失败任务（error → pending 并唤醒调度）。 */
  async function retryFailures(pluginId: string): Promise<void> {
    await invokeIpc(IPC.RETRY_EXOTIC_PLUGIN_FAILURES, { pluginId })
    await loadStatus()
  }

  return {
    registry,
    installed,
    status,
    loading,
    error,
    loadRegistry,
    loadInstalled,
    loadStatus,
    loadAll,
    refreshRegistry,
    install,
    uninstall,
    repair,
    rollback,
    startProcessing,
    pauseProcessing,
    stopProcessing,
    retryFailures,
  }
}
