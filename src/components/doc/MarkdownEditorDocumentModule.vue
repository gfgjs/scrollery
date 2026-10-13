<template>
  <div class="markdown-editor-document">
    <div ref="host" class="markdown-editor-document__host" />
    <div v-if="unavailableMessage" class="markdown-editor-document__unavailable">
      {{ unavailableMessage }}
    </div>
    <div
      v-else
      class="markdown-editor-document__status"
      :class="{ 'markdown-editor-document__status--error': saveState === 'error' }"
    >
      {{ saveStatusText }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router'
import {
  EditorController,
  EditorModel,
  EditorView,
  OffsetRange,
  Selection,
  StringEdit,
  StringValue,
} from '@vscode/markdown-editor'
import { runOnChange } from '@vscode/observables'
import { ipcErrorMessage } from '../../utils/ipc'
import { hasEditContext } from '../../utils/markdownEditorCapability'
import { useToastStore } from '../../stores/toastStore'
import { useMarkdownAutoSave } from '../../composables/reader/useMarkdownAutoSave'
import '@vscode/markdown-editor/editor.css'
import '@vscode/markdown-editor/themes/default.css'

const props = defineProps<{
  itemId: number
  value: string
}>()

const { t } = useI18n()
const toast = useToastStore()

const emit = defineEmits<{
  (event: 'change', value: string): void
}>()

const host = ref<HTMLElement | null>(null)
const sourceLength = ref(props.value.length)
const unavailableMessage = ref('')
const autoSave = useMarkdownAutoSave({ itemId: props.itemId, initialSource: props.value })
const { saveState, saveError, saveNow } = autoSave
defineExpose({ saveNow })
interface HistoryEntry {
  source: string
  beforeSelection: Selection | undefined
  afterSelection: Selection | undefined
}

const historyEntries: HistoryEntry[] = []
let historyIndex = 0
let pendingSelectionEntry: HistoryEntry | null = null

const saveStatusText = computed(() => {
  if (saveState.value === 'saving') return t('edit.saving')
  if (saveState.value === 'dirty') {
    return t('doc.markdownEditorUnsaved', { length: sourceLength.value })
  }
  if (saveState.value === 'error') {
    return t('doc.overwriteFailed', { error: ipcErrorMessage(saveError.value) })
  }
  return t('edit.savedNeedsIndexTitle')
})

let model: EditorModel | null = null
let view: EditorView | null = null
let controller: EditorController | null = null
let sourceSubscription: { dispose(): void } | null = null
let selectionSubscription: { dispose(): void } | null = null

watch(saveError, (error) => {
  if (error !== null) toast.addToast('error', t('doc.overwriteFailed', { error: ipcErrorMessage(error) }))
})

// 同一文档切换阅读引擎也会卸载编辑器，必须与离开文档一样等最新稿落盘。
onBeforeRouteLeave(() => saveNow())
onBeforeRouteUpdate(() => saveNow())

function resetHistory(source: string): void {
  const selection = Selection.collapsed(0)
  historyEntries.splice(0, historyEntries.length, {
    source,
    beforeSelection: selection,
    afterSelection: selection,
  })
  historyIndex = 0
  pendingSelectionEntry = null
}

function recordSource(source: string): void {
  if (historyEntries[historyIndex]?.source === source) return

  // EditorModel 先同步发布源码变化，再发布该次编辑后的选区。
  const selection = model?.selection.get()
  const entry = { source, beforeSelection: selection, afterSelection: selection }
  historyEntries.splice(historyIndex + 1)
  historyEntries.push(entry)
  historyIndex = historyEntries.length - 1
  pendingSelectionEntry = entry
}

function restoreHistoryEntry(index: number, selection: Selection | undefined): void {
  if (!model || index < 0 || index >= historyEntries.length) return

  pendingSelectionEntry = null
  historyIndex = index
  model.sourceText.set(new StringValue(historyEntries[index].source), undefined, undefined)
  model.selection.set(selection, undefined, undefined)
}

function undo(): void {
  if (historyIndex > 0) {
    restoreHistoryEntry(historyIndex - 1, historyEntries[historyIndex].beforeSelection)
  }
}

function redo(): void {
  if (historyIndex + 1 < historyEntries.length) {
    restoreHistoryEntry(historyIndex + 1, historyEntries[historyIndex + 1].afterSelection)
  }
}

function currentLine(source: string, offset: number): {
  start: number
  end: number
  text: string
} {
  const start = source.lastIndexOf('\n', offset - 1) + 1
  const newlineOffset = source.indexOf('\n', offset)
  const end = newlineOffset === -1 ? source.length : newlineOffset
  const textEnd = end > start && source[end - 1] === '\r' ? end - 1 : end

  return {
    start,
    end,
    text: source.slice(start, textEnd),
  }
}

function listEnterEdit(source: string, selection: Selection): StringEdit | undefined {
  if (!selection.isCollapsed || model?.activeBlock.get()?.kind === 'codeBlock') {
    return undefined
  }

  const line = currentLine(source, selection.active)
  const lineBeforeCursor = source.slice(line.start, selection.active)
  const match = /^([ \t]*)([-+*]|\d+[.)])([ \t]+)(?:\[([ xX])\]([ \t]+))?/.exec(
    line.text,
  )
  if (!match || !lineBeforeCursor.startsWith(match[0])) return undefined

  const newline = source.includes('\r\n') ? '\r\n' : '\n'
  if (line.text.slice(match[0].length).trim().length === 0) {
    // 空项按 Enter 退出列表；保留原有行尾，让后续正文与新段落保持分隔。
    const end = line.start + line.text.length
    return StringEdit.replace(new OffsetRange(line.start, end), newline)
  }

  const marker = match[2]
  const nextMarker = /^\d/.test(marker)
    ? `${Number.parseInt(marker, 10) + 1}${marker.endsWith('.') ? '.' : ')'}`
    : marker
  const taskPrefix = match[4] === undefined ? '' : '[ ] '
  return StringEdit.replace(selection.range, `${newline}${match[1]}${nextMarker}${match[3]}${taskPrefix}`)
}

function insertEnter(): void {
  if (!model) return

  const selection = model.selection.get()
  if (!selection) return

  const source = model.sourceText.get().value
  const newline = source.includes('\r\n') ? '\r\n' : '\n'
  const listEdit = listEnterEdit(source, selection)
  const text = (
    model.activeBlock.get()?.kind === 'codeBlock' ? newline : `${newline}${newline}`
  )

  model.applyEditForSelection(listEdit ?? StringEdit.replace(selection.range, text))
}

function handleKeyDown(event: KeyboardEvent): void {
  const modifier = event.ctrlKey || event.metaKey
  if (modifier && !event.altKey) {
    const key = event.key.toLowerCase()
    if (key === 's') {
      event.preventDefault()
      event.stopImmediatePropagation()
      void saveNow()
      return
    }
    if (key === 'z') {
      event.preventDefault()
      event.stopImmediatePropagation()
      if (event.shiftKey) redo()
      else undo()
      return
    }
    if (key === 'y' && !event.metaKey) {
      event.preventDefault()
      event.stopImmediatePropagation()
      redo()
      return
    }
  }

  if (
    event.key !== 'Enter' ||
    event.shiftKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.isComposing
  ) {
    return
  }

  event.preventDefault()
  event.stopImmediatePropagation()
  insertEnter()
}

function disposeEditor(): void {
  autoSave.dispose()
  sourceSubscription?.dispose()
  sourceSubscription = null
  selectionSubscription?.dispose()
  selectionSubscription = null
  view?.element.removeEventListener('keydown', handleKeyDown, true)
  controller?.dispose()
  controller = null
  view?.dispose()
  view = null
  model = null
}

onMounted(() => {
  autoSave.resetSource(props.value)
  resetHistory(props.value)

  if (!host.value) return

  if (!hasEditContext()) {
    unavailableMessage.value = t('doc.markdownEditorUnavailable')
    return
  }

  try {
    model = new EditorModel()
    model.sourceText.set(new StringValue(props.value), undefined, undefined)
    model.selection.set(Selection.collapsed(0), undefined, undefined)

    view = new EditorView(model, {
      classNames: ['md-theme-default'],
    })
    controller = new EditorController(model, view)
    view.element.addEventListener('keydown', handleKeyDown, true)
    host.value.appendChild(view.element)

    sourceSubscription = runOnChange(model.sourceText, (source) => {
      recordSource(source.value)
      sourceLength.value = source.value.length
      emit('change', source.value)
      autoSave.updateSource(source.value)
    })
    selectionSubscription = runOnChange(model.selection, (selection) => {
      if (!pendingSelectionEntry) return
      // 只记录紧随源码变更的选区；之后普通光标移动不能改写重做位置。
      pendingSelectionEntry.afterSelection = selection
      pendingSelectionEntry = null
    })
  } catch (error) {
    disposeEditor()
    unavailableMessage.value = t('doc.markdownEditorInitFailed', {
      error: error instanceof Error ? error.message : String(error),
    })
  }
})

watch(
  () => props.value,
  (value) => {
    if (model?.sourceText.get().value === value) return
    autoSave.resetSource(value)
    resetHistory(value)
    if (!model) {
      sourceLength.value = value.length
      return
    }
    model.sourceText.set(new StringValue(value), undefined, undefined)
    model.selection.set(Selection.collapsed(0), undefined, undefined)
    sourceLength.value = value.length
  },
)

onBeforeUnmount(() => {
  disposeEditor()
})
</script>

<style scoped>
.markdown-editor-document {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  background: var(--color-bg-primary);
}

.markdown-editor-document__host {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.markdown-editor-document__host :deep(.md-editor) {
  box-sizing: border-box;
  width: 100%;
  max-width: none;
  min-height: 100%;
  color: var(--color-text-primary);
}

.markdown-editor-document__host :deep(ul.md-list) {
  list-style-type: disc;
}

.markdown-editor-document__host :deep(ol.md-list) {
  list-style-type: decimal;
}

.markdown-editor-document__status,
.markdown-editor-document__unavailable {
  flex: 0 0 auto;
  padding: 6px 12px;
  border-top: 1px solid var(--color-border);
  color: var(--color-text-secondary);
  background: var(--color-bg-surface);
  font-size: var(--font-size-xs);
}

.markdown-editor-document__unavailable {
  color: var(--color-warning, var(--color-text-secondary));
}

.markdown-editor-document__status--error {
  color: var(--color-error, var(--color-warning, var(--color-text-secondary)));
}
</style>
