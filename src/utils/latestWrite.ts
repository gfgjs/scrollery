// src/utils/latestWrite.ts
// 按 key 串行化的 latest-write-wins 写队列(2026-07-18 审查 F-09)。
//
// 动机:fire-and-forget 的乐观写(如看图台快速连点旋转)并发发起时,后端各写请求取得
// DB 锁的顺序不保证等于调用顺序——旧值可能最后落库,单次失败也只是静默丢写。
// 本原语保证:任一时刻每 key 至多一个在途写;在途期间的新值只覆盖「最新目标」,在途
// 完成后补写(中间值跳过);最终落库值恒等于最后一次 push 的值。

/**
 * 建一条 latest-write-wins 写队列。
 * @param write 实际执行写的函数(生产 = IPC 调用;测试 = stub)。
 * @param onError write 抛错时回调(带 key 与当时的最新目标值)。链随错误终止,
 *                在途期间累积的目标值不再补写;下一次 push 重开新链。
 */
export function createLatestWriteQueue<K, V>(
  write: (key: K, value: V) => Promise<void>,
  onError?: (key: K, value: V, err: unknown) => void,
) {
  const pending = new Map<K, { latest: V }>()
  return {
    /** 提交 key 的最新目标值。同步返回;落库时机与结果由队列语义保证。 */
    push(key: K, value: V): void {
      const entry = pending.get(key)
      if (entry) {
        // 已有在途链:只更新目标,由链尾补写。
        entry.latest = value
        return
      }
      const state = { latest: value }
      pending.set(key, state)
      void (async () => {
        try {
          // 循环直到「写出的值」就是「当前最新目标」:连点期间 latest 可能被多次覆盖。
          let written: V
          do {
            written = state.latest
            await write(key, written)
          } while (written !== state.latest)
        } catch (e) {
          onError?.(key, state.latest, e)
        } finally {
          pending.delete(key)
        }
      })()
    },
  }
}
