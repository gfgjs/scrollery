import { ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'

export interface MarkdownAutoSaveOptions {
  itemId: number
  initialSource: string
}

/** Markdown 编辑器的防抖保存与落盘状态。 */
export function useMarkdownAutoSave({ itemId, initialSource }: MarkdownAutoSaveOptions) {
  const saveState = ref<'saved' | 'dirty' | 'saving' | 'error'>('saved')
  const saveError = ref<unknown>(null)
  let latestSource = initialSource
  let lastSavedSource: string | null = initialSource
  let timer: ReturnType<typeof setTimeout> | null = null
  let inFlight: Promise<boolean> | null = null

  function cancelTimer(): void {
    if (timer !== null) clearTimeout(timer)
    timer = null
  }

  async function writePendingSource(): Promise<boolean> {
    while (latestSource !== lastSavedSource) {
      const source = latestSource
      saveState.value = 'saving'
      saveError.value = null
      try {
        await invokeIpc(IPC.SAVE_VERSION, {
          itemId, content: source, label: null, parentId: null,
          target: 'overwrite', source: 'user',
        })
      } catch (error) {
        // 文件可能已替换而后续数据库提交失败，不能再假定磁盘仍等于上次成功稿。
        lastSavedSource = null
        // 旧稿失败时最新稿仍可继续保存；只有当前稿失败才阻止离开。
        if (latestSource !== source) continue
        saveError.value = error
        saveState.value = 'error'
        return false
      }
      // 基线代表磁盘已写入的内容；即使用户已经撤销，也必须记下本次写入并接着保存最新稿。
      lastSavedSource = source
    }
    saveState.value = 'saved'
    return true
  }

  /** 立即保存并等待最新稿落盘；失败返回 false，供导航守卫保留编辑器。 */
  function saveNow(): Promise<boolean> {
    cancelTimer()
    if (inFlight) return inFlight
    if (latestSource === lastSavedSource) {
      saveState.value = 'saved'
      return Promise.resolve(true)
    }
    inFlight = writePendingSource().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  function updateSource(source: string): void {
    latestSource = source
    cancelTimer()
    saveError.value = null
    if (inFlight) {
      saveState.value = 'saving'
      return
    }
    if (source === lastSavedSource) {
      saveState.value = 'saved'
      return
    }
    saveState.value = 'dirty'
    timer = setTimeout(() => { timer = null; void saveNow() }, 1000)
  }

  function resetSource(source: string): void {
    cancelTimer()
    latestSource = lastSavedSource = source
    saveState.value = inFlight ? 'saving' : 'saved'
    saveError.value = null
  }

  function dispose(): void {
    cancelTimer()
  }

  return { saveState, saveError, updateSource, saveNow, resetSource, dispose }
}
