// src/composables/reader/useDocEditVersion.ts
// 编辑 / 版本态（仅文本）。
import { computed, onScopeDispose, ref, watch, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { confirm } from '@tauri-apps/plugin-dialog'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'

export interface UseDocEditVersionDeps {
  id: Ref<number>
  /** refreshText 重建渲染器时不捕获旧文本位置。 */
  requestRemount: (captureFirst: boolean) => void
  /** 覆盖前等原书偏好写入完成，避免旧编码快照在覆盖后重新写回。 */
  beforeSourceOverwrite?: (itemId: number) => Promise<void>
  /** 原文已成功写为 UTF-8，同步当前文档的本地编码快照。 */
  onSourceOverwritten?: (itemId: number) => void
}

export function useDocEditVersion(deps: UseDocEditVersionDeps) {
  const textContent = ref<string | null>(null)
  const editing = ref(false)
  const editBuffer = ref('')
  const editLabel = ref('')
  const baselineVerified = ref(true)
  // 原生textarea统一换行为LF；撤销回相同可见正文时，不把平台换行差异当作未保存修改。
  const textBaseline = computed(() => (textContent.value ?? '').replace(/\r\n?/g, '\n'))
  const dirty = computed(() => editing.value && (
    !baselineVerified.value || editBuffer.value.replace(/\r\n?/g, '\n') !== textBaseline.value || editLabel.value !== ''
  ))
  const saving = ref(false)
  let saveInFlight: Promise<boolean> | null = null
  const currentVersionId = ref<number | null>(null)
  const showVersions = ref(false)
  const showProofread = ref(false)
  const toast = useToastStore()
  const { t } = useI18n()
  // 切书（含切走又返回）与离开阅读器后，旧异步回执不得改动当前编辑状态。
  let documentGeneration = 0
  let refreshRequest = 0
  watch(deps.id, () => {
    documentGeneration++
    baselineVerified.value = true
  }, { flush: 'sync' })
  onScopeDispose(() => documentGeneration++)

  function startEdit() {
    editBuffer.value = textContent.value ?? ''
    editLabel.value = ''
    editing.value = true
  }
  function cancelEdit() {
    editing.value = false
  }

  function save(target: 'version' | 'overwrite', keepEditing = false): Promise<boolean> {
    if (saveInFlight) return saveInFlight
    const itemId = deps.id.value
    const generation = documentGeneration
    const content = editBuffer.value
    const label = editLabel.value
    const parentId = currentVersionId.value
    saving.value = true
    saveInFlight = (async () => {
      try {
        if (target === 'overwrite') {
          const ok = await confirm(t('doc.overwriteConfirmMsg'), {
            title: t('doc.overwriteSource'), kind: 'warning',
          })
          if (!ok || generation !== documentGeneration) return false
          if (deps.beforeSourceOverwrite) {
            await deps.beforeSourceOverwrite(itemId)
            if (generation !== documentGeneration) return false
          }
        }
        // 覆盖可能先替换文件再返回失败；刷新确认前，撤销到旧正文也不能视为已保存。
        if (target === 'overwrite') baselineVerified.value = false
        const newId = await invokeIpc<number>(IPC.SAVE_VERSION, {
          itemId, content, label: target === 'version' ? label || null : null,
          parentId, target,
        })
        if (target === 'version') {
          // 已开始的保存仍完成原文档版本激活，不能使用等待期间变化的路由 id。
          if (generation === documentGeneration) baselineVerified.value = false
          await invokeIpc(IPC.SET_CURRENT_VERSION, { itemId, versionId: newId })
        }
        if (target === 'overwrite' && generation === documentGeneration) {
          deps.onSourceOverwritten?.(itemId)
        }
        if (generation !== documentGeneration || !(await refreshText())) return false
        // 保存中后来产生的草稿必须继续留在编辑器，旧回执不能关闭它或放行导航。
        if (generation !== documentGeneration) return false
        const unchanged = content === editBuffer.value && label === editLabel.value
        // 已成功保存的标签只消费一次；正文后来变化仍保留草稿，供下一次快捷保存。
        if (keepEditing && label === editLabel.value) editLabel.value = ''
        if (!unchanged) return false
        editing.value = keepEditing
        return true
      } catch (e) {
        if (generation === documentGeneration) {
          toast.addToast('error', t(target === 'version' ? 'doc.saveVersionFailed' : 'doc.overwriteFailed', {
            error: ipcErrorMessage(e),
          }))
        }
        return false
      }
    })().finally(() => {
      saving.value = false
      saveInFlight = null
    })
    return saveInFlight
  }

  /**
   * 新建并激活版本，返回是否已保存当前草稿且可安全离开。重复请求共用在途保存。
   * @param options.keepEditing 成功后保留编辑会话，供快捷保存后继续输入；默认退出编辑。
   */
  function saveNewVersion(options: { keepEditing?: boolean } = {}): Promise<boolean> {
    return save('version', options.keepEditing ?? false)
  }

  /** 确认后覆盖源文件；取消、失败或草稿后来改变均不允许离开。 */
  function overwriteSource(): Promise<boolean> {
    return save('overwrite')
  }

  // 重新拉取生效文本 + 当前版本，并重建渲染器。
  async function refreshText(): Promise<boolean> {
    const itemId = deps.id.value
    const generation = documentGeneration
    // 同一本书也可能连续切版本；只按文档代次判断会把旧正文配到新的版本标识上。
    const request = ++refreshRequest
    const isCurrent = () => generation === documentGeneration && request === refreshRequest
    try {
      const content = await invokeIpc<string>(IPC.GET_DOCUMENT_TEXT, { itemId })
      if (!isCurrent()) return false
      const cur = await invokeIpc<{ id: number } | null>(IPC.GET_CURRENT_VERSION, { itemId })
      if (!isCurrent()) return false
      textContent.value = content
      currentVersionId.value = cur?.id ?? null
      baselineVerified.value = true
      deps.requestRemount(false)
      return true
    } catch (e) {
      if (isCurrent()) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
      return false
    }
  }

  return {
    textContent,
    editing,
    editBuffer,
    editLabel,
    dirty,
    saving,
    currentVersionId,
    showVersions,
    showProofread,
    startEdit,
    cancelEdit,
    saveNewVersion,
    overwriteSource,
    refreshText,
  }
}
