// useTauriListen 单元测试(审查 P1-11)——锁住「await listen 卸载竞态」的仲裁行为:
// 无论 listen 落定先于还是后于作用域销毁,监听器最终都必被解绑一次(不泄漏、不多解)。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { effectScope } from 'vue'
import { EVENTS } from '../constants/ipc'

// 受控 listen mock:可手动决定每次调用返回的 promise 何时/以何值落定,以便精确编排竞态时序。
const listenMock = vi.fn()
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}))

import { useTauriListen } from './useTauriListen'

// 冲刷 microtask:.then/.catch 回调排在 microtask 队列,两拍足够覆盖 then→(无)catch 链。
async function flushMicrotasks() {
  await Promise.resolve()
  await Promise.resolve()
}

describe('useTauriListen', () => {
  beforeEach(() => {
    listenMock.mockReset()
  })

  it('句柄先就绪、后销毁:销毁时解绑一次', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValue(unlisten)

    const scope = effectScope()
    scope.run(() => useTauriListen(EVENTS.MEDIA_ENRICHED, () => {}))
    await flushMicrotasks() // 句柄就绪
    expect(unlisten).not.toHaveBeenCalled()

    scope.stop()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('销毁先于句柄就绪:句柄落定时立即就地解绑,不泄漏(核心竞态)', async () => {
    const unlisten = vi.fn()
    let resolveListen!: (fn: () => void) => void
    listenMock.mockReturnValue(
      new Promise<() => void>((res) => {
        resolveListen = res
      }),
    )

    const scope = effectScope()
    scope.run(() => useTauriListen(EVENTS.MEDIA_ENRICHED, () => {}))

    // 组件在 listen 落定前卸载
    scope.stop()
    expect(unlisten).not.toHaveBeenCalled()

    // 句柄此刻才就绪 → 应就地解绑(旧写法在此永久泄漏)
    resolveListen(unlisten)
    await flushMicrotasks()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('listen 失败:不抛出、销毁安全', async () => {
    listenMock.mockRejectedValue(new Error('ipc down'))

    const scope = effectScope()
    expect(() => scope.run(() => useTauriListen(EVENTS.MEDIA_ENRICHED, () => {}))).not.toThrow()
    await flushMicrotasks()
    expect(() => scope.stop()).not.toThrow()
  })
})
