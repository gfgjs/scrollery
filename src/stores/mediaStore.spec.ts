import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('./uiStore', () => ({ useUiStore: () => ({ thumbInfoElements: [] }) }))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn(), warn: vi.fn() } }))

import { useMediaStore } from './mediaStore'
import type { LayoutSummary } from '../types/layout'

function layoutSummary(version: number): LayoutSummary {
  return {
    totalRows: 1,
    totalHeight: 200,
    layoutVersion: version,
    orderVersion: version,
    totalItems: 1,
    separators: [],
    monthBuckets: [],
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('mediaStore 详情请求时序', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    invokeIpc.mockReset()
  })

  it('排队请求被替代时明确结束，最新请求失败不提交中间布局且释放槽位', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const store = useMediaStore()
    const first = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(first.promise).mockRejectedValueOnce(new Error('query failed'))
    const initial = store.computeLayout({ containerWidth: 800 })
    const replaced = store.computeLayout({ containerWidth: 900 })
    const latest = store.computeLayout({ containerWidth: 1000 })
    expect(await replaced).toBe('superseded')
    first.resolve(layoutSummary(10))
    expect(await initial).toBe('superseded')
    expect(await latest).toBe('failed')
    expect(store.layoutSummary).toBeNull()
    expect(store.isComputingLayout).toBe(false)
    expect(invokeIpc).toHaveBeenCalledTimes(2)

    invokeIpc.mockResolvedValueOnce(layoutSummary(12))
    expect(await store.computeLayout({ containerWidth: 1100 })).toBe('committed')
    expect(store.layoutSummary?.layoutVersion).toBe(12)
  })

  it('当前布局取行失效只触发一次重算，恢复后新版本仍可自行恢复', async () => {
    const store = useMediaStore()
    const stale = { code: 'LayoutNotReady', message: '布局已失效' }
    store.layoutSummary = layoutSummary(10)
    invokeIpc.mockRejectedValue(stale)
    await expect(store.fetchRowsByY(0, 200)).rejects.toBe(stale)
    await expect(store.fetchBucketRows(0, 200)).rejects.toBe(stale)
    expect(store.layoutDirty).toBe(true)
    expect(store.layoutDirtyRevision).toBe(1)
    // 重算失败时不让后续滚动产生无穷重试；成功换代后允许恢复新版本。
    store.consumeLayoutDirty()
    await expect(store.fetchRowsByY(200, 400)).rejects.toBe(stale)
    expect(store.layoutDirtyRevision).toBe(1)
    store.layoutSummary = layoutSummary(11)
    await expect(store.fetchBucketRows(200, 400)).rejects.toBe(stale)
    expect(store.layoutDirtyRevision).toBe(2)
  })

  it('旧布局迟到的失败和普通读取错误不撤销当前布局', async () => {
    const store = useMediaStore()
    store.layoutSummary = layoutSummary(10)
    let reject!: (error: unknown) => void
    invokeIpc.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail }))
    const reading = store.fetchBucketRows(0, 200)
    const failed = expect(reading).rejects.toMatchObject({ code: 'LayoutNotReady' })
    store.layoutSummary = layoutSummary(11)
    reject({ code: 'LayoutNotReady', message: '旧布局已失效' })
    await failed
    invokeIpc.mockRejectedValueOnce(new Error('read failed'))
    await expect(store.fetchRowsByY(0, 200)).rejects.toThrow('read failed')
    expect(store.layoutDirty).toBe(false)
    expect(store.layoutDirtyRevision).toBe(0)
  })
})
