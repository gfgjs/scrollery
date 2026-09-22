// src/stores/historyStore.ts
// 撤销/重做历史（仅会话内，内存存储）。
//
// 两类记录：
// ① 文件夹/媒体的 move & copy —— 结构化记录（见各 *Record），重做要重新执行真实 IO。
// ② 软删除类（收藏夹删除 / 人物误检 / 人物隐藏 / 删照片）—— CallbackRecord，闭包登记（见 pushUndoable）。
//
// 文件系统级操作不跨重启持久化 —— 两次会话间磁盘状态可能已变，过期的撤销有风险。
//
// ⚠ 软删除类同样不跨重启（栈在内存里），但其**后端数据是永久保留的**（restore 无时效校验、全库无
// purge）。故「重启后还能不能撤销」是一个独立的产品问题：需要的是**回收站类的持久 UI**，而不是持久化
// 这个栈。四类软删现均有持久入口：删照片 → `/trash`；收藏夹 → 收藏夹页「已删除」区（2026-07-16 补，
// `list_deleted_collections`）；人物误检/隐藏 → 人物页的「已忽略」桶。本栈只负责**即时**撤销。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invokeIpc, ipcErrorMessage, moveRecoveryOf } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import type { CopyDirResult, MoveDirResult } from '../types/ipc'
import i18n from '../i18n'
import { logger } from '../utils/logger'
import { useToastStore } from './toastStore'
import { useScanStore } from './scanStore'
import { useDirectoryMoveRecoveryStore } from './directoryMoveRecoveryStore'

interface MoveRecord {
  type: 'move'
  dirId: number
  name: string
  fromParentId: number
  toParentId: number
}

interface CopyRecord {
  type: 'copy'
  sourceDirId: number
  targetDirId: number
  name: string
  createdRootId: number
  createdRelPath: string
  createdAbsPath: string
}

/** 一个或多个媒体项移动到文件夹（拖到文件夹）。每项带原目录，撤销时精确还原（问题5）。 */
interface MoveMediaRecord {
  type: 'moveMedia'
  items: { id: number; fromDirId: number }[]
  targetDirId: number
  label: string
}

/** 一个或多个媒体项复制到文件夹（Shift / 右键拖拽）。记录新建行 id，撤销时精确删除这些副本（问题2）。 */
interface CopyMediaRecord {
  type: 'copyMedia'
  srcIds: number[]
  targetDirId: number
  createdIds: number[]
  label: string
}

/** relocate_media_items 的后端结果 */
interface MediaRelocationResult {
  id: number
  fromDirId: number
  targetDirId: number
}

/** copy_media_items_db 的后端结果 */
interface MediaCopyResult {
  srcId: number
  newId: number
}

/**
 * 通用可撤销记录：调用方自己执行动作，只把「怎么撤 / 怎么重做」登记进来。
 *
 * 与上面四种**文件系统级**记录的分工：那些必须把参数结构化存下（重做要重新执行真实 IO、catch 里还要
 * 判定记录是否已失效）；而软删除类操作的 undo/redo 就是一对幂等的 store 调用，结构化没有收益，只会逼
 * historyStore 认识每一个领域（collections / persons / media），判别链每加一个领域长一截。
 *
 * **闭包 ≠ 退回 toast 那套**（真机 round10 #7 的病根）：toast 的 `actions` 住在 toast 对象里，
 * `removeToast` 一 splice 回调就不可达——**撤销的寿命成了提示条时长的副产品**，没人决定过它只有 5 秒。
 * 本记录的闭包与 store 同寿（整个会话）。不变量：**撤销的寿命由撤销栈决定，不由提示条决定。**
 * 幂等回调失败时保留原栈位置，供重试；成功后才移到另一栈。
 */
interface CallbackRecord {
  type: 'callback'
  /** 记录标识：供 undoIfTop 判定「toast 上那个撤销钮指的还是不是栈顶这条」。 */
  id: number
  undo: () => Promise<void>
  redo: () => Promise<void>
  /** 撤销成功后的 toast 文案（已翻译）。 */
  undoMessage: string
  /** 重做成功后的 toast 文案（已翻译）。 */
  redoMessage: string
}

type OpRecord = MoveRecord | CopyRecord | MoveMediaRecord | CopyMediaRecord | CallbackRecord

/**
 * 一次目录移动的判定结果：'complete' = 收尾干净、可安全反向执行；'pending' = 半完成、
 * 已登记成恢复任务（见 [runMove]）。调用方据它决定是否报成功、是否进历史栈。
 */
export type MoveOutcome = 'complete' | 'pending'

export const useHistoryStore = defineStore('history', () => {
  const undoStack = ref<OpRecord[]>([])
  const redoStack = ref<OpRecord[]>([])
  const busy = ref(false)
  // CallbackRecord 的自增标识（不能用对象身份比对：undoStack 是 deep ref，入栈的对象会被代理包裹）。
  let callbackSeq = 0

  const canUndo = computed(() => undoStack.value.length > 0 && !busy.value)
  const canRedo = computed(() => redoStack.value.length > 0 && !busy.value)

  // 通知界面其余部分刷新。侧边栏监听 `folder-stats-changed`（重载文件夹树、保留展开态，
  // 并在给出 `detail.selectDirId` 时选中它）；媒体网格重新计算布局。一个事件 → 一次重载。
  function refresh(selectDirId?: number | null) {
    window.dispatchEvent(new CustomEvent('folder-stats-changed', { detail: { selectDirId } }))
  }

  // ── 原子操作（初始动作 + 重做共用） ───────────────────────────────────────
  async function execMove(dirId: number, targetId: number): Promise<MoveDirResult> {
    const res = await invokeIpc<MoveDirResult>(IPC.MOVE_DIRECTORY, {
      sourceDirId: dirId,
      targetDirId: targetId,
    })
    refresh(dirId) // 自动选中移动后的文件夹
    return res
  }

  /**
   * 把复制出的物理树入库（目标根重扫）。
   *
   * 复制已落盘是既成事实：入库失败**不改写**这个事实，也不算作复制失败——如实显示
   * 「已复制、尚未完成入库」并给重扫入口（此前扫描失败会让整次复制看起来失败）。
   */
  async function ingestCopiedDir(res: CopyDirResult): Promise<void> {
    if (!res.needsRescan) return
    try {
      await useScanStore().startScan(res.createdRootId, () => {
        window.dispatchEvent(new CustomEvent('folder-stats-changed'))
      })
    } catch (e) {
      useToastStore().addToast(
        'warning',
        i18n.global.t('history.copyNeedsRescan', {
          path: res.createdAbsPath,
          count: res.copiedFiles,
          error: ipcErrorMessage(e),
        }),
        10000,
        [
          {
            label: i18n.global.t('history.copyRescanNow'),
            onClick: () => void ingestCopiedDir(res),
          },
        ],
      )
    }
  }

  async function execCopy(sourceDirId: number, targetDirId: number): Promise<CopyDirResult> {
    const res = await invokeIpc<CopyDirResult>(IPC.COPY_DIRECTORY, { sourceDirId, targetDirId })
    // 通过目标根的后台重扫把复制出的文件作为全新资产引入；失败只提示「已复制、待入库」。
    await ingestCopiedDir(res)
    refresh()
    return res
  }

  async function execDeleteCopy(rec: CopyRecord): Promise<void> {
    await invokeIpc(IPC.DELETE_DIRECTORY_TO_TRASH, {
      absPath: rec.createdAbsPath,
      rootId: rec.createdRootId,
      relPath: rec.createdRelPath,
    })
    refresh()
  }

  /**
   * 执行一次目录移动并判定「完整 / 半完成」——**初次移动、撤销、重做共用这一个判定**。
   *
   * 半完成 = IPC 报 move_db_pending（物理已落盘、索引或清理未完），或返回结果里还带着
   * sourceLeftover / recoveryId（后端保证后者两者同时非空）。处置统一为：
   *   - 登记恢复清单（按阶段日志 id 可重试收尾，管理区能看到文件真实位置）；
   *   - 调用方**不得报完全成功**，也不得把这条放进**可反向执行**的历史栈——反向移动会把
   *     「旧路径还有一份」的残留搬来搬去，最后得到两份内容。
   *
   * 返回 'complete' 才是真的收尾干净，可安全进历史；真正的失败（同名冲突等）照旧抛出。
   */
  async function runMove(dirId: number, name: string, targetId: number): Promise<MoveOutcome> {
    let res: MoveDirResult
    try {
      res = await execMove(dirId, targetId)
    } catch (e) {
      const pending = moveRecoveryOf(e)
      // 半完成不是「普通失败」：文件可能已经在新位置，凭异常里的落点登记收尾任务，判定为
      // 'pending' 返回（不抛出——它不是「什么都没发生」），调用方据此不报成功、不进历史。
      if (pending) {
        useDirectoryMoveRecoveryStore().notePending({ ...pending, name })
        return 'pending'
      }
      throw e
    }
    if (res.recoveryId !== null || res.sourceLeftover !== null) {
      if (res.recoveryId !== null) {
        useDirectoryMoveRecoveryStore().notePending({
          recoveryId: res.recoveryId,
          targetAbsPath: res.targetAbsPath,
          name,
        })
      } else {
        // 未收尾却没有日志 id：后端契约外（store 里挂不出可重试条目），至少别把它当完全成功。
        logger.error('history: 目录移动报源残留但缺 recoveryId', {
          dirId,
          targetAbsPath: res.targetAbsPath,
        })
      }
      return 'pending'
    }
    return 'complete'
  }

  // ── 对外：执行一次新的移动/复制 ──────────────────────────────────────────────
  /**
   * 移动一个文件夹。返回值即 [runMove] 的判定：'complete' = 已完全收尾（调用方可报成功）；
   * 'pending' = 半完成、已登记成恢复任务（调用方**不得**报成功）。
   */
  async function move(
    dirId: number,
    name: string,
    fromParentId: number,
    toParentId: number,
  ): Promise<MoveOutcome> {
    busy.value = true
    try {
      const outcome = await runMove(dirId, name, toParentId)
      if (outcome === 'pending') return outcome
      undoStack.value.push({ type: 'move', dirId, name, fromParentId, toParentId })
      redoStack.value = []
      return outcome
    } finally {
      busy.value = false
    }
  }

  async function copy(sourceDirId: number, name: string, targetDirId: number): Promise<void> {
    busy.value = true
    try {
      const res = await execCopy(sourceDirId, targetDirId)
      undoStack.value.push({
        type: 'copy',
        sourceDirId,
        targetDirId,
        name,
        createdRootId: res.createdRootId,
        createdRelPath: res.createdRelPath,
        createdAbsPath: res.createdAbsPath,
      })
      redoStack.value = []
    } finally {
      busy.value = false
    }
  }

  // ── 对外：把媒体项移动到文件夹（拖到文件夹，可撤销） ────────────────────────────
  /** 重定位一组项（撤销+重做共用）。 */
  async function relocateMedia(moves: { id: number; targetDirId: number }[]): Promise<void> {
    await invokeIpc(IPC.RELOCATE_MEDIA_ITEMS, { moves })
    refresh() // 重载树（实时计数）+ 重算网格
  }

  async function moveMedia(itemIds: number[], targetDirId: number, label: string): Promise<number> {
    if (itemIds.length === 0) return 0
    busy.value = true
    try {
      const results = await invokeIpc<MediaRelocationResult[]>(IPC.RELOCATE_MEDIA_ITEMS, {
        moves: itemIds.map((id) => ({ id, targetDirId })),
      })
      if (results.length === 0) return 0 // 无实际移动（已在目标目录）
      undoStack.value.push({
        type: 'moveMedia',
        items: results.map((r) => ({ id: r.id, fromDirId: r.fromDirId })),
        targetDirId,
        label,
      })
      redoStack.value = []
      refresh()
      return results.length
    } finally {
      busy.value = false
    }
  }

  async function copyMedia(itemIds: number[], targetDirId: number, label: string): Promise<number> {
    if (itemIds.length === 0) return 0
    busy.value = true
    try {
      const results = await invokeIpc<MediaCopyResult[]>(IPC.COPY_MEDIA_ITEMS_DB, {
        moves: itemIds.map((id) => ({ id, targetDirId })),
      })
      if (results.length === 0) return 0 // 无实际复制（冲突或同目录）
      undoStack.value.push({
        type: 'copyMedia',
        srcIds: itemIds,
        targetDirId,
        createdIds: results.map((r) => r.newId),
        label,
      })
      redoStack.value = []
      refresh()
      return results.length
    } finally {
      busy.value = false
    }
  }

  // ── Public: 登记一次已完成的可撤销操作(软删除类)─────────────────────────────
  /**
   * 登记撤销/重做闭包。**调用方自己已经执行完动作**，此处只入栈。
   *
   * 用于软删除类操作（收藏夹删除 / 人物误检 / 人物隐藏 / 删照片）：后端数据永久保留、restore 无时效
   * 校验、全库无 purge——**撤销能力本就无限期**。此前把它绑在 toast 的 duration 上，5 秒不是数据的
   * 保质期，而是提示条寿命的副产品（真机 round10 #7）。
   */
  function pushUndoable(rec: Omit<CallbackRecord, 'type' | 'id'>): number {
    const id = ++callbackSeq
    undoStack.value.push({ type: 'callback', id, ...rec })
    redoStack.value = []
    return id
  }

  /**
   * toast 上「撤销」按钮专用：**仅当该记录仍是栈顶时**才撤，否则静默不动。
   *
   * 为何要判栈顶：多条 undo toast 可同时在屏（5 秒内连删两项），而 undo() 恒撤栈顶。若用户先点**较早**
   * 那条的撤销钮，撤掉的会是较晚那次操作——提示写着 A、回来的却是 B。判栈顶把这种错序点击变成安全的
   * 空操作；栈本身仍按 LIFO 完整可撤（Ctrl+Z 逐个来，顺序由栈保证，且无时限）。
   *
   * 注：按出现顺序从新到旧点撤销是**正确**的，每次点时那条恰好在栈顶——被挡的只有反序点击。
   */
  async function undoIfTop(id: number): Promise<void> {
    const top = undoStack.value[undoStack.value.length - 1]
    if (!top || top.type !== 'callback' || top.id !== id) return
    await undo()
  }

  // ── Undo / Redo ────────────────────────────────────────────────────────────
  /**
   * 只弹仍然在栈顶的那条记录。
   *
   * 撤销/重做期间有 await，用户此时新起一次拖拽就会往栈里压新记录——无条件 pop() 会把**别人**
   * 挤掉。身份比对安全：同一对象的响应式代理由 Vue 按目标对象缓存，两次读出是同一个代理。
   */
  function dropTop(stack: typeof undoStack, rec: OpRecord): void {
    if (stack.value[stack.value.length - 1] === rec) stack.value.pop()
  }

  /**
   * 撤销/重做共用收尾：把记录从来源栈弹出、压进对向栈并报成功。
   *
   * @param reversedMovePending true = 这次撤销/重做是一次半完成的目录移动：**不**进对向栈
   *   （状态没走到可安全反向执行的那一步）、**不**报完全成功——收尾任务已在 [runMove] 里登记，
   *   用户在管理区看到真实落点并重试。
   */
  function settleHistory(
    from: 'undo' | 'redo',
    rec: OpRecord,
    reversedMovePending: boolean,
    msg: string,
  ): void {
    const toast = useToastStore()
    if (from === 'undo') {
      dropTop(undoStack, rec)
      if (!reversedMovePending) {
        redoStack.value.push(rec)
        toast.addToast('success', msg)
      }
    } else {
      dropTop(redoStack, rec)
      if (!reversedMovePending) {
        undoStack.value.push(rec)
        toast.addToast('success', msg)
      }
    }
  }

  async function undo(): Promise<void> {
    if (!canUndo.value) return
    const toast = useToastStore()
    const rec = undoStack.value[undoStack.value.length - 1]
    busy.value = true
    try {
      let msg: string
      // 撤销一次半完成的移动同样会留下残留/半成品：与初次移动共用判定，半完成即转成恢复任务。
      let reversedMovePending = false
      if (rec.type === 'move') {
        reversedMovePending = (await runMove(rec.dirId, rec.name, rec.fromParentId)) === 'pending'
        msg = i18n.global.t('history.undoMoveFolder', { name: rec.name })
      } else if (rec.type === 'copy') {
        await execDeleteCopy(rec)
        msg = i18n.global.t('history.undoCopyFolder', { name: rec.name })
      } else if (rec.type === 'moveMedia') {
        // 媒体移动：把每项移回原目录。
        await relocateMedia(rec.items.map((i) => ({ id: i.id, targetDirId: i.fromDirId })))
        msg = i18n.global.t('history.undoMoveItems', { count: rec.items.length })
      } else if (rec.type === 'callback') {
        await rec.undo()
        msg = rec.undoMessage
      } else {
        // 媒体复制：精确删除我们创建的副本（文件→回收站 + 行）。
        await invokeIpc(IPC.REMOVE_MEDIA_ITEMS_HARD, { ids: rec.createdIds })
        refresh()
        msg = i18n.global.t('history.undoCopyItems', { count: rec.createdIds.length })
      }
      settleHistory('undo', rec, reversedMovePending, msg)
    } catch (e) {
      // 文件操作可能已失效（目标被外部改动等），照旧丢弃；幂等回调保留供重试。
      // 注：半完成移动不走这里——它由 runMove 返回 'pending' 并已登记恢复任务。
      if (rec.type !== 'callback') dropTop(undoStack, rec)
      toast.addToast('error', i18n.global.t('common.undoFailed', { error: e }))
    } finally {
      busy.value = false
    }
  }

  async function redo(): Promise<void> {
    if (!canRedo.value) return
    const toast = useToastStore()
    const rec = redoStack.value[redoStack.value.length - 1]
    busy.value = true
    try {
      let msg: string
      let reversedMovePending = false
      if (rec.type === 'move') {
        reversedMovePending = (await runMove(rec.dirId, rec.name, rec.toParentId)) === 'pending'
        msg = i18n.global.t('history.redoMoveFolder', { name: rec.name })
      } else if (rec.type === 'copy') {
        const res = await execCopy(rec.sourceDirId, rec.targetDirId)
        rec.createdRootId = res.createdRootId
        rec.createdRelPath = res.createdRelPath
        rec.createdAbsPath = res.createdAbsPath
        msg = i18n.global.t('history.redoCopyFolder', { name: rec.name })
      } else if (rec.type === 'moveMedia') {
        // 媒体移动：把每项重新移动到目标目录。
        await relocateMedia(rec.items.map((i) => ({ id: i.id, targetDirId: rec.targetDirId })))
        msg = i18n.global.t('history.redoMoveItems', { count: rec.items.length })
      } else if (rec.type === 'callback') {
        await rec.redo()
        msg = rec.redoMessage
      } else {
        // copyMedia: re-copy from the original sources; capture the fresh new ids so a
        // subsequent undo still deletes the right rows.
        // 媒体复制：从原始源重新复制；记录新的 id，使随后的撤销仍能删除正确的行。
        const results = await invokeIpc<MediaCopyResult[]>(IPC.COPY_MEDIA_ITEMS_DB, {
          moves: rec.srcIds.map((id) => ({ id, targetDirId: rec.targetDirId })),
        })
        rec.createdIds = results.map((r) => r.newId)
        refresh()
        msg = i18n.global.t('history.redoCopyItems', { count: rec.createdIds.length })
      }
      settleHistory('redo', rec, reversedMovePending, msg)
    } catch (e) {
      if (rec.type !== 'callback') dropTop(redoStack, rec)
      toast.addToast('error', i18n.global.t('history.redoFailed', { error: e }))
    } finally {
      busy.value = false
    }
  }

  return {
    undoStack,
    redoStack,
    busy,
    canUndo,
    canRedo,
    move,
    copy,
    moveMedia,
    copyMedia,
    pushUndoable,
    undoIfTop,
    undo,
    redo,
  }
})
