// src/composables/reader/useReaderProgress.ts
// 阅读进度去抖保存。
// 配对捕获 itemId 与位置：延迟写入时路由可能已切换，不能将旧文档位置写入新文档。
import type { Ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'
import { createLatestWriteQueue } from '../../utils/latestWrite'

// 退出后的写入仍可能在途；同一窗口重开阅读器须共用按文档串行的队列，避免旧进度最后落库。
const writes = createLatestWriteQueue<number, string>(async (itemId, position) => {
  // 单次失败仍允许队列写入后来位置；旧位置本身不重试。
  await invokeIpc(IPC.SET_READING_PROGRESS, { itemId, position }).catch(() => {})
})

export function useReaderProgress(id: Ref<number>) {
  let lastProgress: { itemId: number; pos: string } | null = null
  let saveTimer: ReturnType<typeof setTimeout> | null = null

  function flushProgress() {
    if (saveTimer) {
      clearTimeout(saveTimer)
      saveTimer = null
    }
    if (lastProgress) {
      const { itemId, pos } = lastProgress
      lastProgress = null
      writes.push(itemId, pos)
    }
  }

  function onProgress(pos: string) {
    if (!Number.isFinite(id.value)) return
    lastProgress = { itemId: id.value, pos }
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = setTimeout(flushProgress, 1200)
  }

  /** 换文档时调用：丢弃未冲刷的旧进度（load() 已先 flushProgress() 写库,这里只清引用）。 */
  function reset() {
    lastProgress = null
  }

  return { onProgress, flushProgress, reset }
}
