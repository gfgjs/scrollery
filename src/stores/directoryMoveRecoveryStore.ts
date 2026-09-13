// src/stores/directoryMoveRecoveryStore.ts
// 目录移动的半完成恢复清单（P0-1）。
//
// 后端把「物理已落盘、索引或源目录清理未完成」的移动留成阶段日志，并暴露只读清单
// （list_pending_directory_moves）与按日志 id 的重试（retry_directory_move）。本 store 是这两条
// 命令在前端的唯一落点：
//   - **读清单只读**：不触发任何物理动作；收尾只能由用户点重试（跨卷可能整树拷贝，不能自动重做）；
//   - 半完成的移动**不进撤销栈**（反向移动会留下两份内容），改登记成这里可重试的收尾任务；
//   - 重试结果分三支如实提示：已收尾 / 仍待收尾（目标离线、目标冲突等，报具体原因而非成功）/
//     无需收尾（目标根已删、双侧皆无）。刷新或重启后由启动点再读一次清单。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invokeIpc, ipcErrorMessage } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import type { DirectoryMoveRecovery } from '../types/ipc'
import { logger } from '../utils/logger'
import i18n from '../i18n'
import { useToastStore } from './toastStore'

/**
 * dir_move.rs 里 detail 的稳定短标签白名单。
 * 后端新增标签时前端回落到 unknown 文案——不把生码直接甩给用户。
 */
const RECOVERY_DETAILS = [
  'completed',
  'absent',
  'target_root_missing',
  'target_missing',
  'target_unverified',
  'target_conflict',
  'target_unresolved',
  'source_leftover',
  'source_changed',
  'staging_busy',
  'payload_pending',
  'published',
] as const

/** detail → i18n 文案键（未知标签回落到 unknown）。 */
export function dirMoveRecoveryDetailKey(detail: string): string {
  return (RECOVERY_DETAILS as readonly string[]).includes(detail)
    ? `sidebar.dirRecovery.detail.${detail}`
    : 'sidebar.dirRecovery.detail.unknown'
}

/**
 * 重试响应的形状守卫：只有后端报告该有的字段齐了才认，缺字段就当契约外（**不**猜结果——
 * 猜错会把一条仍需收尾的移动报成「已收尾」，条目和重试入口一起消失）。
 */
function isRecoveryReport(v: unknown): v is DirectoryMoveRecovery {
  if (!v || typeof v !== 'object') return false
  const r = v as Partial<DirectoryMoveRecovery>
  return (
    typeof r.recoveryId === 'number' &&
    typeof r.detail === 'string' &&
    typeof r.sourceName === 'string'
  )
}

export const useDirectoryMoveRecoveryStore = defineStore('directoryMoveRecovery', () => {
  /** 未完成的移动（后端清单为权威来源；本 store 不自行拼装报告）。 */
  const items = ref<DirectoryMoveRecovery[]>([])
  const loading = ref(false)
  /** 正在重试的日志 id：同一 id 在跑时不重复发起。 */
  const retryingIds = ref<number[]>([])

  // 清单读取的单飞：并发调用收敛成一次在途读取；期间有新的调用则读完再补读一次，
  // 避免调用方拿到「自己登记之前」的旧快照（迟到刷新）。
  let inFlight: Promise<void> | null = null
  let reloadQueued = false

  /**
   * 清单**就地改动**序号（retry 的收尾结果：drop / upsert）。
   *
   * 只护 load/load 不够：一次早于重试发起的读，可能晚于重试完成才回传——那份快照里还带着
   * 刚收尾掉的条目，直接落盘就把已完成任务复活了。所以每次就地改动都推进序号，让在途读
   * 自己发现「我这份已过期」并丢掉重读（见 load）。
   */
  let listRevision = 0

  const pendingCount = computed(() => items.value.length)

  function isRetrying(recoveryId: number): boolean {
    return retryingIds.value.includes(recoveryId)
  }

  function upsert(report: DirectoryMoveRecovery): void {
    const known = items.value.some((i) => i.recoveryId === report.recoveryId)
    items.value = known
      ? items.value.map((i) => (i.recoveryId === report.recoveryId ? report : i))
      : [...items.value, report]
    listRevision += 1
  }

  function drop(recoveryId: number): void {
    items.value = items.value.filter((i) => i.recoveryId !== recoveryId)
    listRevision += 1
  }

  /** 目录树 + 画廊刷新（复用既有事件通道，不新开通道）。 */
  function refreshLibrary(): void {
    window.dispatchEvent(new CustomEvent('folder-stats-changed'))
  }

  /**
   * 读后端未完成清单（只读，不触发任何物理动作）。
   * 读取失败保留现有清单：恢复入口比「显示为空」重要，用户仍能点已有条目的重试。
   */
  async function load(): Promise<void> {
    if (inFlight) {
      reloadQueued = true
      return inFlight
    }
    const run = (async () => {
      loading.value = true
      try {
        do {
          reloadQueued = false
          const revision = listRevision
          const list = await invokeIpc<DirectoryMoveRecovery[]>(IPC.LIST_PENDING_DIRECTORY_MOVES)
          if (revision !== listRevision) {
            // 读取期间清单被就地改动（重试已收尾/退回一条）：这份快照已过期，丢掉重读。
            reloadQueued = true
            continue
          }
          // 非数组响应（如 UI harness 未覆盖该命令时的兜底 null）按空清单处理，别让契约外数据抛崩整块 UI。
          items.value = Array.isArray(list) ? list : []
        } while (reloadQueued)
      } catch (e) {
        logger.error('directory move: 读取未完成清单失败', { error: e })
      } finally {
        loading.value = false
        inFlight = null
      }
    })()
    inFlight = run
    return run
  }

  /**
   * 按阶段日志 id 重试一条收尾（幂等，允许重做物理搬运与整树校验）。
   * 同一 id 在跑时不再发起第二次；返回 true = 这条已不在未完成清单里。
   */
  async function retry(recoveryId: number): Promise<boolean> {
    if (isRetrying(recoveryId)) return false
    const toast = useToastStore()
    const known = items.value.find((i) => i.recoveryId === recoveryId)
    retryingIds.value = [...retryingIds.value, recoveryId]
    try {
      const report: unknown = await invokeIpc<DirectoryMoveRecovery | null>(
        IPC.RETRY_DIRECTORY_MOVE,
        { recoveryId },
      )
      // 后端已无该日志行 = 这条已经收尾干净（重试幂等，重复点击也走这里）。
      if (report === null) {
        drop(recoveryId)
        refreshLibrary()
        toast.addToast(
          'success',
          i18n.global.t('sidebar.dirRecovery.completed', { name: known?.sourceName ?? '' }),
        )
        return true
      }
      if (!isRecoveryReport(report)) {
        // 契约外响应：不猜结果，条目留在清单里让用户再试。
        logger.error('directory move: 重试返回了契约外响应', { recoveryId, report })
        return false
      }
      if (report.detail === 'completed') {
        drop(recoveryId)
        refreshLibrary()
        toast.addToast(
          'success',
          i18n.global.t('sidebar.dirRecovery.completed', { name: report.sourceName }),
        )
        return true
      }
      if (report.needsRetry) {
        // 目标离线 / 目标冲突 / 源残留仍在：条目与原因都得留下，不能报「已收尾」。
        upsert(report)
        toast.addToast(
          'warning',
          i18n.global.t('sidebar.dirRecovery.stillPending', {
            name: report.sourceName,
            detail: i18n.global.t(dirMoveRecoveryDetailKey(report.detail)),
          }),
          8000,
        )
        return false
      }
      // needsRetry=false 且非 completed：这条移动已无收尾必要（目标根已删 / 双侧皆无）。
      drop(recoveryId)
      toast.addToast(
        'info',
        i18n.global.t('sidebar.dirRecovery.notRequired', {
          name: report.sourceName,
          detail: i18n.global.t(dirMoveRecoveryDetailKey(report.detail)),
        }),
        6000,
      )
      return true
    } catch (e) {
      // 重试自身失败：条目留在清单里，用户可再试。
      toast.addToast(
        'error',
        i18n.global.t('sidebar.dirRecovery.failed', { error: ipcErrorMessage(e) }),
        6000,
      )
      return false
    } finally {
      retryingIds.value = retryingIds.value.filter((id) => id !== recoveryId)
    }
  }

  /**
   * 登记一次「半完成」移动（物理已落盘、索引或清理未完成）：
   * 弹一条可立即重试的提示，并读一次后端清单让管理区显示（清单是权威详情，不在前端拼装报告）。
   * 不触发任何物理动作——收尾只能由用户点重试。
   */
  function notePending(seed: { recoveryId: number; targetAbsPath: string; name: string }): void {
    useToastStore().addToast(
      'warning',
      i18n.global.t('sidebar.dirRecovery.pending', {
        name: seed.name,
        path: seed.targetAbsPath,
      }),
      10000,
      [
        {
          label: i18n.global.t('sidebar.dirRecovery.retry'),
          onClick: () => void retry(seed.recoveryId),
        },
      ],
    )
    void load()
  }

  return {
    items,
    loading,
    pendingCount,
    isRetrying,
    load,
    retry,
    notePending,
  }
})
