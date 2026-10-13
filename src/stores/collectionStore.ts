// src/stores/collectionStore.ts
// 收藏夹状态。
// 由后端 albums/album_items 承载。系统夹（4 个播种类型夹）虚拟（类型 + is_favorited）；
// 用户夹存实体成员。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'
import type { Collection } from '../types/media'

export const useCollectionStore = defineStore('collection', () => {
  const collections = ref<Collection[]>([])
  /** 已软删除的用户夹（回收站）。与 `collections` 是同一批夹的**不重不漏划分**（后端按 deleted_at 取反）。 */
  const deleted = ref<Collection[]>([])
  const isLoading = ref(false)
  // 正列表与回收站会并发刷新，各自只接纳最新读取，避免旧快照覆盖写入后的结果。
  let loadGeneration = 0
  let deletedLoadGeneration = 0

  /** 加载全部收藏夹（系统夹在前，用户夹在后）。 */
  async function load() {
    const generation = ++loadGeneration
    isLoading.value = true
    try {
      const result = await invokeIpc<Collection[]>(IPC.LIST_COLLECTIONS)
      if (generation === loadGeneration) collections.value = result
    } catch (e) {
      logger.error('[CollectionStore] load failed', { error: e })
    } finally {
      if (generation === loadGeneration) isLoading.value = false
    }
  }

  /** 加载已软删除的用户夹（回收站读路径）。 */
  async function loadDeleted() {
    const generation = ++deletedLoadGeneration
    try {
      const result = await invokeIpc<Collection[]>(IPC.LIST_DELETED_COLLECTIONS)
      if (generation === deletedLoadGeneration) deleted.value = result
    } catch (e) {
      logger.error('[CollectionStore] loadDeleted failed', { error: e })
    }
  }

  /** 最近使用的用户收藏夹（toast chips）。 */
  async function recent(limit = 5): Promise<Collection[]> {
    try {
      return await invokeIpc<Collection[]>(IPC.RECENT_COLLECTIONS, { limit })
    } catch (e) {
      logger.error('[CollectionStore] recent failed', { error: e })
      return []
    }
  }

  /** 新建用户收藏夹并刷新列表，返回其 id。 */
  async function create(name: string, icon?: string): Promise<number | null> {
    try {
      const id = await invokeIpc<number>(IPC.CREATE_COLLECTION, { name, icon: icon ?? null })
      await load()
      return id
    } catch (e) {
      logger.error('[CollectionStore] create failed', { error: e })
      return null
    }
  }

  /** 软删除用户收藏夹并刷新两侧列表（可经 restore 撤销）。 */
  async function remove(albumId: number) {
    try {
      await invokeIpc(IPC.DELETE_COLLECTION, { albumId })
      // 两侧一起刷:删除是从 collections 移到 deleted 的**搬运**,只刷一侧会让回收站计数陈旧。
      await Promise.all([load(), loadDeleted()])
    } catch (e) {
      logger.error('[CollectionStore] remove failed', { error: e })
      // 写入失败须交给调用方，避免登记未发生的删除或错误推进撤销/重做栈。
      throw e
    }
  }

  /** 恢复软删除的收藏夹并刷新两侧列表（承接删除 undo 与回收站捞回）。 */
  async function restore(albumId: number) {
    try {
      await invokeIpc(IPC.RESTORE_COLLECTION, { albumId })
      await Promise.all([load(), loadDeleted()])
    } catch (e) {
      logger.error('[CollectionStore] restore failed', { error: e })
      throw e
    }
  }

  /** 重命名用户收藏夹（系统夹由后端守卫保护）；原地更新列表。 */
  async function rename(albumId: number, name: string) {
    try {
      await invokeIpc(IPC.RENAME_COLLECTION, { albumId, name })
      const c = collections.value.find((c) => c.id === albumId)
      if (c) c.name = name
    } catch (e) {
      logger.error('[CollectionStore] rename failed', { error: e })
    }
  }

  /** 向用户收藏夹添加项，返回插入行数。 */
  async function addItems(albumId: number, itemIds: number[]): Promise<number> {
    try {
      return await invokeIpc<number>(IPC.ADD_TO_COLLECTION, { albumId, itemIds })
    } catch (e) {
      logger.error('[CollectionStore] addItems failed', { error: e })
      // 零插入可能只是全部已在收藏夹，不能用它掩盖真实写入失败。
      throw e
    }
  }

  /** 从收藏夹移除项，返回删除行数。 */
  async function removeItems(albumId: number, itemIds: number[]): Promise<number> {
    try {
      return await invokeIpc<number>(IPC.REMOVE_FROM_COLLECTION, { albumId, itemIds })
    } catch (e) {
      logger.error('[CollectionStore] removeItems failed', { error: e })
      return 0
    }
  }

  return {
    collections,
    deleted,
    isLoading,
    load,
    loadDeleted,
    recent,
    create,
    rename,
    remove,
    restore,
    addItems,
    removeItems,
  }
})
