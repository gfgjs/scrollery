// src/composables/useViewIds.spec.ts
// 2026-07-06 审查 P0-2 补测:选区全集(flat_ids)是「零测试的命门路径」。
// 锁两件事:① in-flight token——迟到的旧版本应答(成功或拒绝)不得覆盖/清空已落地的新版本;
//          ② 基础契约——refresh 成功建索引、失败清空、rangeBetween 端点缺失返回空。
// 模块级单例:各用例通过「成功 refresh 一个新版本」重置为已知态,不依赖模块重载。

import { describe, it, expect, vi } from 'vitest'

type InvokeHandler = (cmd: string, args: unknown) => unknown
const { state } = vi.hoisted(() => ({ state: { handler: (() => undefined) as InvokeHandler } }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: unknown) => state.handler(cmd, args),
}))

import { useViewIds } from './useViewIds'

/** 可手动 resolve/reject 的延迟应答,用于制造「在途/迟到」。 */
function deferred<T>() {
  let resolve!: (v: T) => void
  let reject!: (e: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('useViewIds', () => {
  const view = useViewIds()

  it('refresh 成功:全集/索引/isFresh 就位,rangeBetween 与方向无关', async () => {
    state.handler = () => Promise.resolve([10, 20, 30, 40])
    await view.refresh(1)
    expect(view.allIds()).toEqual([10, 20, 30, 40])
    expect(view.totalCount()).toBe(4)
    expect(view.isFresh(1)).toBe(true)
    expect(view.indexOf(30)).toBe(2)
    expect(view.indexOf(999)).toBe(-1)
    expect(view.rangeBetween(20, 40)).toEqual([20, 30, 40])
    expect(view.rangeBetween(40, 20)).toEqual([20, 30, 40])
    // 端点不在视图 → 空数组,由调用侧降级
    expect(view.rangeBetween(999, 20)).toEqual([])
  })

  it('refresh 失败(ViewStale 等):清空待重取,isFresh 翻 false', async () => {
    state.handler = () => Promise.resolve([1, 2])
    await view.refresh(2)
    expect(view.isFresh(2)).toBe(true)

    state.handler = () => Promise.reject(new Error('ViewStale'))
    await view.refresh(3)
    expect(view.allIds()).toEqual([])
    expect(view.isFresh(3)).toBe(false)
    expect(view.indexOf(1)).toBe(-1)
  })

  it('token:迟到的旧版本成功应答不得覆盖新版本全集', async () => {
    const slow = deferred<number[]>()
    // 第一发(旧版本)挂起在途
    state.handler = () => slow.promise
    const p1 = view.refresh(10)
    // 第二发(新版本)先落地
    state.handler = () => Promise.resolve([7, 8, 9])
    await view.refresh(11)
    expect(view.allIds()).toEqual([7, 8, 9])
    // 旧应答此刻才到 → 必须被丢弃
    slow.resolve([1, 2, 3])
    await p1
    expect(view.allIds()).toEqual([7, 8, 9])
    expect(view.isFresh(11)).toBe(true)
    expect(view.isFresh(10)).toBe(false)
  })

  it('token:迟到的旧版本拒绝不得清空刚落地的新版本(P0 竞态本体)', async () => {
    const slow = deferred<number[]>()
    state.handler = () => slow.promise
    const p1 = view.refresh(20)
    state.handler = () => Promise.resolve([5, 6])
    await view.refresh(21)
    expect(view.allIds()).toEqual([5, 6])
    // 旧版本此刻被后端以 ViewStale 拒绝 → 不得走「清空待重取」抹掉新数据
    slow.reject(new Error('ViewStale'))
    await p1
    expect(view.allIds()).toEqual([5, 6])
    expect(view.isFresh(21)).toBe(true)
    expect(view.rangeBetween(5, 6)).toEqual([5, 6])
  })

  it('ensureFresh:版本未变不发 IPC,变化才重取', async () => {
    let hits = 0
    state.handler = () => {
      hits++
      return Promise.resolve([1])
    }
    await view.refresh(30)
    expect(hits).toBe(1)
    await view.ensureFresh(30) // 同版本 → 不重取
    expect(hits).toBe(1)
    await view.ensureFresh(31) // 新版本 → 重取
    expect(hits).toBe(2)
    expect(view.isFresh(31)).toBe(true)
  })
})
