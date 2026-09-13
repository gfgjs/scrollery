// src/composables/reader/useDocEditVersion.ts
// 编辑 / 版本态（结构拆分自 DocumentViewer.vue §S8 + §S27，仅文本）。
import { ref, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { confirm } from '@tauri-apps/plugin-dialog'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'

export interface UseDocEditVersionDeps {
  id: Ref<number>
  /** refreshText 用（reloadToken++，不捕获位置——原逻辑就没有 captureCurrentPosition）。 */
  requestRemount: (captureFirst: boolean) => void
}

export function useDocEditVersion(deps: UseDocEditVersionDeps) {
  // 编辑/版本（§5.3，仅文本）：当前生效文本、编辑态与缓冲、当前版本 id、版本面板开关。
  const textContent = ref<string | null>(null)
  const editing = ref(false)
  const editBuffer = ref('')
  const editLabel = ref('')
  const currentVersionId = ref<number | null>(null)
  const showVersions = ref(false)
  const showProofread = ref(false)
  const toast = useToastStore()
  const { t } = useI18n()

  function startEdit() {
    editBuffer.value = textContent.value ?? ''
    editLabel.value = ''
    editing.value = true
  }
  function cancelEdit() {
    editing.value = false
  }

  // 另存为新版本（默认，不进画廊）→ 设为当前 → 重渲染。
  async function saveNewVersion() {
    // P1-16:此前无 try/catch,保存失败静默且 editing 仍开,用户以为已存实则丢失。
    try {
      const newId = await invokeIpc<number>(IPC.SAVE_VERSION, {
        itemId: deps.id.value,
        content: editBuffer.value,
        label: editLabel.value || null,
        parentId: currentVersionId.value,
        target: 'version',
      })
      await invokeIpc(IPC.SET_CURRENT_VERSION, { itemId: deps.id.value, versionId: newId })
      editing.value = false
      await refreshText()
    } catch (e) {
      toast.addToast('error', t('doc.saveVersionFailed', { error: ipcErrorMessage(e) }))
    }
  }

  // 覆盖源文件（高级，二次确认；后端自动先备份旧源为一个版本）。
  async function overwriteSource() {
    const ok = await confirm(t('doc.overwriteConfirmMsg'), {
      title: t('doc.overwriteSource'),
      kind: 'warning',
    })
    if (!ok) return
    // P1-16:覆盖用户源文件失败尤须告警(否则用户以为已改,实则原文未动)。
    try {
      await invokeIpc(IPC.SAVE_VERSION, {
        itemId: deps.id.value,
        content: editBuffer.value,
        label: null,
        parentId: currentVersionId.value,
        target: 'overwrite',
      })
      editing.value = false
      await refreshText()
    } catch (e) {
      toast.addToast('error', t('doc.overwriteFailed', { error: ipcErrorMessage(e) }))
    }
  }

  // 重新拉取生效文本 + 当前版本，并重建渲染器。
  async function refreshText() {
    textContent.value = await invokeIpc<string>(IPC.GET_DOCUMENT_TEXT, {
      itemId: deps.id.value,
    }).catch(() => null)
    const cur = await invokeIpc<{ id: number } | null>(IPC.GET_CURRENT_VERSION, {
      itemId: deps.id.value,
    }).catch(() => null)
    currentVersionId.value = cur?.id ?? null
    deps.requestRemount(false)
  }

  return {
    textContent,
    editing,
    editBuffer,
    editLabel,
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
