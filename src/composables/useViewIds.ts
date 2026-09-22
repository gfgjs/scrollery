// 当前视图按顺序排列的全集 ID；复制、传输和建索引为 O(N)，仅在顺序换代时支付。
// 选区范围/反选必须消费此全集，不能用已经渲染的几行代替。
import { shallowRef, ref, computed } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'

const EMPTY_IDS: readonly number[] = []
const cachedIds = shallowRef<readonly number[]>(EMPTY_IDS)
let idIndex = new Map<number, number>()
const loadedVersion = ref<number | null>(null)
const expectedVersion = ref<number | null>(null)
let requestToken = 0
let inFlight: { version: number; promise: Promise<void> } | null = null

/** 当前全集已属于正在展示的成员与顺序。 */
function isReady(): boolean {
  return expectedVersion.value !== null && loadedVersion.value === expectedVersion.value
}
const viewIds = computed(() => isReady() ? cachedIds.value : EMPTY_IDS)

function clearLoaded() {
  loadedVersion.value = null
  cachedIds.value = EMPTY_IDS
  idIndex = new Map()
}

/** 接受后端顺序身份；null 撤销操作资格，同一版本恢复时保留数组与索引。 */
function setExpectedVersion(version: number | null) {
  const next = version !== null && version > 0 ? version : null
  if (next === expectedVersion.value) return
  expectedVersion.value = next
  requestToken++
  inFlight = null
  if (next !== null && loadedVersion.value !== next) clearLoaded()
}

/** 同版在途共用一次取数，成功和失败都须仍属于当前顺序。 */
function refresh(orderVersion: number): Promise<void> {
  if (orderVersion <= 0 || expectedVersion.value !== orderVersion) return Promise.resolve()
  if (inFlight?.version === orderVersion) return inFlight.promise
  clearLoaded()
  const token = ++requestToken
  const promise = (async () => {
    try {
      const ids = await invokeIpc<number[]>(IPC.GET_VIEW_IDS, { orderVersion })
      // 先复核资格，再做 O(N) 的索引构建；迟到结果不再占用主线程建立无用 Map。
      if (token !== requestToken || expectedVersion.value !== orderVersion) return
      const index = new Map<number, number>()
      for (let i = 0; i < ids.length; i++) index.set(ids[i], i)
      idIndex = index
      cachedIds.value = ids
      loadedVersion.value = orderVersion
    } catch (error) {
      if (token !== requestToken || expectedVersion.value !== orderVersion) return
      clearLoaded()
      logger.warn('[useViewIds] get_view_ids 失败，等待下一次有效请求', { error })
    } finally {
      if (token === requestToken) inFlight = null
    }
  })()
  inFlight = { version: orderVersion, promise }
  return promise
}

/** 几何重排沿用已加载全集；初挂载和布局通知共用同版在途。 */
function ensureFresh(orderVersion: number): Promise<void> {
  return isFresh(orderVersion) ? Promise.resolve() : refresh(orderVersion)
}

/** ID 的当前顺序下标，全集未就绪或 ID 不在集合内时为 -1。 */
function indexOf(id: number): number {
  return isReady() ? idIndex.get(id) ?? -1 : -1
}

/** 当前可操作的全集是否属于指定顺序。 */
function isFresh(orderVersion: number): boolean {
  return isReady() && loadedVersion.value === orderVersion
}

/** 顺序已提交,即使全集尚未传到前端,后端仍可按该版本做单步导航。 */
function isExpected(orderVersion: number): boolean {
  return expectedVersion.value === orderVersion
}

/** 当前完整顺序上的闭区间；全集未就绪或任一端点不在其中时为空。 */
function rangeBetween(anchorId: number, toId: number): number[] {
  const a = indexOf(anchorId)
  const b = indexOf(toId)
  if (a < 0 || b < 0) return []
  return cachedIds.value.slice(Math.min(a, b), Math.max(a, b) + 1)
}

/** 当前可操作的全集；待新集合/顺序提交期间不暴露旧 ID。 */
function allIds(): readonly number[] { return viewIds.value }

/** 当前可操作全集的大小。 */
function totalCount(): number { return viewIds.value.length }

/** 单例全集缓存，布局提交后按 orderVersion 加载。 */
export function useViewIds() {
  return { viewIds, setExpectedVersion, isReady, refresh, ensureFresh, indexOf, isFresh, isExpected,
    rangeBetween, allIds, totalCount }
}
