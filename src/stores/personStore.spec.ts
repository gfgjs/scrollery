import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick, watchEffect } from 'vue'
import { IPC } from '../constants/ipc'
import type { PersonSummary } from '../types/person'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc: (...args: unknown[]) => mocks.invoke(...args) }))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../utils/thumbCacheDir', () => ({ getThumbCacheDir: () => Promise.resolve('/cache') }))
import { usePersonStore } from './personStore'

function person(id: number): PersonSummary {
  return {
    id,
    name: null,
    faceCount: 1,
    isNamed: false,
    isHidden: false,
    coverItemId: null,
    coverThumbPath: null,
    coverThumbStatus: null,
    coverBbox: null,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  mocks.invoke.mockReset()
})

describe('人物写失败的调用方契约', () => {
  it('命名失败向调用者抛错且不改本地名称，重试成功才更新', async () => {
    const store = usePersonStore()
    store.persons = [person(1)]
    let renderedName: string | null = null
    const stop = watchEffect(() => { renderedName = store.persons[0].name })
    const error = new Error('rename rejected')
    mocks.invoke.mockRejectedValueOnce(error)
    await expect(store.rename(1, ' Alice ')).rejects.toBe(error)
    expect(store.persons[0]).toMatchObject({ name: null, isNamed: false })
    await nextTick()
    expect(renderedName).toBeNull()
    mocks.invoke.mockResolvedValueOnce(undefined)
    await store.rename(1, ' Alice ')
    expect(store.persons[0]).toMatchObject({ name: 'Alice', isNamed: true })
    await nextTick()
    expect(renderedName).toBe('Alice')
    stop()
  })

  it('隐藏失败向调用者抛错且不改可见性，重试成功才更新', async () => {
    const store = usePersonStore()
    store.persons = [person(1)]
    let visibleIds: number[] = []
    const stop = watchEffect(() => {
      visibleIds = store.persons.filter((p) => !p.isHidden).map((p) => p.id)
    })
    const error = new Error('hide rejected')
    mocks.invoke.mockRejectedValueOnce(error)
    await expect(store.setHidden(1, true)).rejects.toBe(error)
    expect(store.persons[0].isHidden).toBe(false)
    await nextTick()
    expect(visibleIds).toEqual([1])
    mocks.invoke.mockResolvedValueOnce(undefined)
    await store.setHidden(1, true)
    expect(store.persons[0].isHidden).toBe(true)
    await nextTick()
    expect(visibleIds).toEqual([])
    stop()
  })

  it('合并失败不刷新或移除源人物，重试成功后再加载权威列表', async () => {
    const store = usePersonStore()
    store.persons = [person(1), person(2)]
    const error = new Error('merge rejected')
    mocks.invoke.mockRejectedValueOnce(error)
    await expect(store.merge([1], 2)).rejects.toBe(error)
    expect(mocks.invoke).toHaveBeenCalledTimes(1)
    expect(store.persons.map((p) => p.id)).toEqual([1, 2])
    const merged = { ...person(2), faceCount: 2 }
    mocks.invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce([merged])
    await store.merge([1], 2)
    expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.LIST_FACE_PERSONS)
    expect(store.persons).toEqual([merged])
  })
})
