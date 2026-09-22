// src/stores/personStore.ts
// 人物墙状态（F6）—— 人物簇列表 + 命名/合并/隐藏 + 单图人脸框。

import { defineStore } from 'pinia'
import { ref, shallowRef } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { getThumbCacheDir } from '../utils/thumbCacheDir'
import { IPC } from '../constants/ipc'
import type { PersonSummary, FaceBox, LikelyMatchGroup } from '../types/person'

export const usePersonStore = defineStore('person', () => {
  const persons = shallowRef<PersonSummary[]>([])
  // 误检桶(is_ignored)人物簇 —— 服务端 list_persons 排除之,单独拉取供「显示已忽略」管理视图。
  const ignoredPersons = shallowRef<PersonSummary[]>([])
  const isLoading = ref(false)
  // 应用缓存目录（用于解析封面脸缩略图 URL）；只取一次。
  const cacheDir = ref('')

  async function ensureCacheDir() {
    if (cacheDir.value) return
    try {
      cacheDir.value = await getThumbCacheDir()
    } catch (e) {
      logger.error('[Person] get cache dir failed', { error: e })
    }
  }

  /** 加载人物墙的全部人物簇。 */
  async function load() {
    isLoading.value = true
    try {
      await ensureCacheDir()
      persons.value = await invokeIpc<PersonSummary[]>(IPC.LIST_FACE_PERSONS)
    } catch (e) {
      logger.error('[Person] load failed', { error: e })
    } finally {
      isLoading.value = false
    }
  }

  /** 加载误检桶人物簇（供「显示已忽略」管理视图）。 */
  async function loadIgnored() {
    try {
      await ensureCacheDir()
      ignoredPersons.value = await invokeIpc<PersonSummary[]>(IPC.LIST_IGNORED_FACE_PERSONS)
    } catch (e) {
      logger.error('[Person] loadIgnored failed', { error: e })
      ignoredPersons.value = []
    }
  }

  /** 命名（空→未命名）；成功后发布新数组，失败交调用方提示。 */
  async function rename(personId: number, name: string) {
    await invokeIpc(IPC.RENAME_FACE_PERSON, { personId, name })
    const trimmed = name.trim()
    persons.value = persons.value.map((p) =>
      p.id === personId ? { ...p, name: trimmed || null, isNamed: !!trimmed } : p,
    )
  }

  /** 显示/隐藏；成功后发布新数组，失败交调用方提示。 */
  async function setHidden(personId: number, hidden: boolean) {
    await invokeIpc(IPC.SET_FACE_PERSON_HIDDEN, { personId, hidden })
    persons.value = persons.value.map((p) =>
      p.id === personId ? { ...p, isHidden: hidden } : p,
    )
  }

  /** 标记为误检桶（审查 G1）：非人脸误检（雕像/海报），置位后不上墙、重建按锚定保护。
   *  list_face_persons 在 SQL 层排除 ignored → 本地在墙与误检桶两列表间乐观搬移(免二次拉取),
   *  「显示已忽略」管理视图即时反映。失败 rethrow 交调用方 toast。 */
  async function setIgnored(personId: number, ignored: boolean) {
    await invokeIpc(IPC.SET_FACE_PERSON_IGNORED, { personId, ignored })
    if (ignored) {
      // 移入误检桶:从墙移除,乐观加入误检桶列表首位。
      const p = persons.value.find((x) => x.id === personId)
      persons.value = persons.value.filter((x) => x.id !== personId)
      if (p && !ignoredPersons.value.some((x) => x.id === personId)) {
        ignoredPersons.value = [p, ...ignoredPersons.value]
      }
    } else {
      // 移出误检桶:从误检桶列表移除,重载墙(该人物重新上墙)。
      ignoredPersons.value = ignoredPersons.value.filter((x) => x.id !== personId)
      await load()
    }
  }

  /** 合并 `srcIds` 到 `dstId`，成功后重载（计数/质心已变）；写入失败交调用方提示。 */
  async function merge(srcIds: number[], dstId: number) {
    await invokeIpc(IPC.MERGE_FACE_PERSONS, { srcIds, dstId })
    await load()
  }

  /** 一张图中的人脸（详情叠加）。 */
  async function getFacesForItem(itemId: number): Promise<FaceBox[]> {
    try {
      return await invokeIpc<FaceBox[]>(IPC.GET_ITEM_FACES, { itemId })
    } catch (e) {
      logger.error('[Person] getFacesForItem failed', { error: e })
      return []
    }
  }

  /** 全量重新聚类：修碎片化（同一人散成多个未命名簇），不打散已确认脸/已命名人物。
   *  分析运行中会抛错（由调用方提示）。 */
  async function recluster() {
    await invokeIpc(IPC.RECLUSTER_FACES)
    await load() // 簇/计数已变 → 重载
  }

  // ── 批量审批（T10, §3.6.2）────────────────────────────────────────────────
  // likely-match 分组：未确认脸按候选 person 分组，用户对整组/选中脸一次性确认/改派/移出/拒绝/建人。
  // 审批动作的副作用直接传播 invokeIpc 的 reject（含后端中文错误消息，如跨模型改派），由调用方 toast。
  const likelyMatches = ref<LikelyMatchGroup[]>([])

  /** 加载批量审批的 likely-match 分组。可选按 `personId` 聚焦 / `limit` 限量。 */
  async function loadLikelyMatches(personId?: number, limit?: number) {
    await ensureCacheDir()
    likelyMatches.value = await invokeIpc<LikelyMatchGroup[]>(IPC.LIST_LIKELY_FACE_MATCHES, {
      personId: personId ?? null,
      limit: limit ?? null,
    })
  }

  /** 乐观更新：从内存分组移除已处理的脸，并丢弃清空的组。重排新数组以触发响应式刷新。 */
  function dropResolvedFaces(faceIds: number[]) {
    const ids = new Set(faceIds)
    likelyMatches.value = likelyMatches.value
      .map((g) => ({ ...g, candidateFaces: g.candidateFaces.filter((f) => !ids.has(f.faceId)) }))
      .filter((g) => g.candidateFaces.length > 0)
  }

  /** 确认：接受这些脸归属其候选 person（锁定 is_confirmed）。 */
  async function confirmFaces(faceIds: number[]) {
    await invokeIpc(IPC.CONFIRM_FACES, { faceIds })
    dropResolvedFaces(faceIds)
  }

  /** 改派：把这些脸改归 `personId` 并锁定（纠正聚类错误）。后端拒绝跨模型改派。 */
  async function reassignFaces(faceIds: number[], personId: number) {
    await invokeIpc(IPC.REASSIGN_FACES, { faceIds, personId })
    dropResolvedFaces(faceIds)
  }

  /** 移出：清这些脸的 person 归属与确认态（误检/归错）。 */
  async function unassignFaces(faceIds: number[]) {
    await invokeIpc(IPC.UNASSIGN_FACES, { faceIds })
    dropResolvedFaces(faceIds)
  }

  /** 拒绝：标记这些脸不属于候选 `personId`（不再作为其 likely-match）。 */
  async function rejectFaces(faceIds: number[], personId: number) {
    await invokeIpc(IPC.REJECT_FACES, { faceIds, personId })
    dropResolvedFaces(faceIds)
  }

  /** 建新人物：从这些脸新建 person（可选命名），返回新 person id。 */
  async function createPerson(faceIds: number[], name?: string): Promise<number> {
    const newId = await invokeIpc<number>(IPC.CREATE_PERSON, { faceIds, name: name ?? null })
    dropResolvedFaces(faceIds)
    return newId
  }

  return {
    persons,
    ignoredPersons,
    isLoading,
    cacheDir,
    load,
    loadIgnored,
    ensureCacheDir,
    rename,
    setHidden,
    setIgnored,
    merge,
    getFacesForItem,
    recluster,
    likelyMatches,
    loadLikelyMatches,
    confirmFaces,
    reassignFaces,
    unassignFaces,
    rejectFaces,
    createPerson,
  }
})
