import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useCollectionStore } from './collectionStore'
import { IPC } from '../constants/ipc'
import type { Collection } from '../types/media'

const invokeIpc = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>()
vi.mock('../utils/ipc', () => ({
  invokeIpc: (command: string, args?: Record<string, unknown>) => invokeIpc(command, args),
}))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

const collection: Collection = {
  id: 7,
  name: '旅行',
  kind: 'user',
  itemCount: 3,
  sortOrder: 0,
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
})

describe('收藏夹列表的异步刷新归属', () => {
  it('新建后的刷新先完成时，迟到的旧列表不能抹掉新收藏夹', async () => {
    const oldRead = deferred<Collection[]>()
    invokeIpc.mockReturnValueOnce(oldRead.promise)
    const store = useCollectionStore()
    const loading = store.load()
    invokeIpc.mockImplementation(async (command) => {
      if (command === IPC.CREATE_COLLECTION) return collection.id
      if (command === IPC.LIST_COLLECTIONS) return [collection]
      throw new Error(`Unexpected command: ${command}`)
    })

    expect(await store.create(collection.name)).toBe(collection.id)
    expect(store.collections).toEqual([collection])
    oldRead.resolve([])
    await loading

    expect(store.collections).toEqual([collection])
    expect(store.isLoading).toBe(false)
  })

  it.each(['remove', 'restore'] as const)(
    '%s 刷新两侧后，迟到的挂载读取不能把收藏夹搬回原列表',
    async (action) => {
      const oldActive = deferred<Collection[]>()
      const oldDeleted = deferred<Collection[]>()
      const beforeActive = action === 'remove' ? [collection] : []
      const beforeDeleted = action === 'restore' ? [collection] : []
      const afterActive = beforeDeleted
      const afterDeleted = beforeActive
      invokeIpc.mockImplementation((command) => {
        if (command === IPC.LIST_COLLECTIONS) return oldActive.promise
        if (command === IPC.LIST_DELETED_COLLECTIONS) return oldDeleted.promise
        throw new Error(`Unexpected command: ${command}`)
      })
      const store = useCollectionStore()
      store.collections = beforeActive
      store.deleted = beforeDeleted
      // 复现总览挂载时的并发读取，并通过真实写入 action 触发后续刷新。
      const loading = store.load()
      const loadingDeleted = store.loadDeleted()
      const writeCommand = action === 'remove' ? IPC.DELETE_COLLECTION : IPC.RESTORE_COLLECTION
      invokeIpc.mockImplementation(async (command) => {
        if (command === writeCommand) return undefined
        if (command === IPC.LIST_COLLECTIONS) return afterActive
        if (command === IPC.LIST_DELETED_COLLECTIONS) return afterDeleted
        throw new Error(`Unexpected command: ${command}`)
      })

      await store[action](collection.id)
      expect(invokeIpc).toHaveBeenCalledWith(writeCommand, { albumId: collection.id })
      expect(store.collections).toEqual(afterActive)
      expect(store.deleted).toEqual(afterDeleted)

      oldActive.resolve(beforeActive)
      oldDeleted.resolve(beforeDeleted)
      await Promise.all([loading, loadingDeleted])
      expect(store.collections).toEqual(afterActive)
      expect(store.deleted).toEqual(afterDeleted)
    },
  )

  it.each(['success', 'failure'] as const)(
    '旧读取以 %s 结束时，不能提前结束新读取的加载状态',
    async (outcome) => {
      const oldRead = deferred<Collection[]>()
      const newRead = deferred<Collection[]>()
      invokeIpc.mockReturnValueOnce(oldRead.promise).mockReturnValueOnce(newRead.promise)
      const store = useCollectionStore()
      const oldLoading = store.load()
      const newLoading = store.load()

      if (outcome === 'success') oldRead.resolve([])
      else oldRead.reject(new Error('旧读取失败'))
      await oldLoading
      expect(store.isLoading).toBe(true)

      newRead.resolve([collection])
      await newLoading
      expect(store.collections).toEqual([collection])
      expect(store.isLoading).toBe(false)
    },
  )
})
