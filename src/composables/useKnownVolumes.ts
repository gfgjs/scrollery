// src/composables/useKnownVolumes.ts
// 已知卷面板数据层（Part5 T13 §3.7 离线 UX）：列出应用登记的物理卷 + 重命名 / 忘记。
//
// 在线态是后端 volume_watch 每 15s 对账维护的真相，前端只读展示 + 触发用户操作，不自行判定在线。

import { ref } from 'vue'

import { IPC } from '../constants/ipc'
import { invokeIpc, type IpcError } from '../utils/ipc'

/** 卷类型（后端 `VolumeKind`，serde 小写）。 */
export type VolumeKind = 'local' | 'removable' | 'network'

/** 「已知卷」面板行（后端 `VolumeInfo`，来自 `list_volumes`）。 */
export interface VolumeInfo {
  id: number
  stableId: string
  /** 卷标（用户可改；未命名为 null → 前端回退 stableId / 挂载点展示）。 */
  label: string | null
  kind: VolumeKind
  /** 最近挂载点 / 盘符（离线后仍提示「上次在 X:」）。 */
  lastMountPath: string | null
  isOnline: boolean
  lastSeen: number | null
  /** 该卷上未删除媒体数（回收站项不计）。 */
  itemCount: number
}

export function useKnownVolumes() {
  const volumes = ref<VolumeInfo[]>([])
  const loading = ref(false)
  const error = ref<IpcError | null>(null)

  /** 列出全部已知卷。失败置 `error` 不抛（面板容错）。 */
  async function load(): Promise<void> {
    loading.value = true
    error.value = null
    try {
      volumes.value = await invokeIpc<VolumeInfo[]>(IPC.LIST_VOLUMES)
    } catch (e) {
      error.value = e as IpcError
    } finally {
      loading.value = false
    }
  }

  /** 重命名卷标（空名后端会拒；调用方应先本地校验）。成功后重载。错误上抛供 toast。 */
  async function rename(volumeId: number, label: string): Promise<void> {
    await invokeIpc(IPC.RENAME_VOLUME, { volumeId, label })
    await load()
  }

  /** 忘记卷登记（媒体行保留、仅解绑）。成功后重载。错误上抛供 toast。 */
  async function forget(volumeId: number): Promise<void> {
    await invokeIpc(IPC.FORGET_VOLUME, { volumeId })
    await load()
  }

  return { volumes, loading, error, load, rename, forget }
}
