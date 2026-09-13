// src/composables/reader/useReaderProgress.ts
// 阅读进度去抖保存（结构拆分自 DocumentViewer.vue §S18）。
// 捕获 (itemId, pos) 配对而非只存 pos(2026-07-10 审查 B12):flush 时 route id 可能已变——
// 退出阅读器时 id=NaN 会把最后 1.2s 窗内的位置静默丢弃(翻页即退必丢);doc→doc 复用导航时
// load() 的冲刷会把 A 的 cfi 写进 B 的进度行。写库一律用捕获时刻的 itemId（红线，不可简化）。
import type { Ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'

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
      invokeIpc(IPC.SET_READING_PROGRESS, { itemId, position: pos }).catch(() => {})
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
