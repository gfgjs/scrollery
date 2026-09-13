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
import { useRoute } from 'vue-router'
import {
  EditorController,
  EditorModel,
  EditorView,
  Selection,
  StringEdit,
  StringValue,
} from '@vscode/markdown-editor'
import { runOnChange } from '@vscode/observables'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import '@vscode/markdown-editor/editor.css'
import '@vscode/markdown-editor/themes/default.css'

const props = defineProps<{
  value: string
}>()

const { t } = useI18n()
const route = useRoute()
const toast = useToastStore()

const emit = defineEmits<{
  (event: 'change', value: string): void
}>()

const host = ref<HTMLElement | null>(null)
const sourceLength = ref(props.value.length)
const unavailableMessage = ref('')
const saveState = ref<'saved' | 'dirty' | 'saving' | 'error'>('saved')
const saveError = ref('')
const historySources: string[] = [props.value]
let historyIndex = 0
let latestSource = props.value
let lastSavedSource = props.value
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
let pendingSaveSource: string | null = null
let saveInFlight: Promise<void> | null = null
let disposed = false

const AUTO_SAVE_DELAY_MS = 1000

const saveStatusText = computed(() => {
  if (saveState.value === 'saving') return t('edit.saving')
  if (saveState.value === 'dirty') {
    return t('doc.markdownEditorInMemory', { length: sourceLength.value })
  }
  if (saveState.value === 'error') {
    return t('doc.overwriteFailed', { error: saveError.value })
  }
  return t('edit.savedNeedsIndexTitle')
})

let model: EditorModel | null = null
let view: EditorView | null = null
let controller: EditorController | null = null
let sourceSubscription: { dispose(): void } | null = null

function hasEditContext(): boolean {
  const globals = globalThis as typeof globalThis & {
    EditContext?: unknown
  }
  return typeof globals.EditContext === 'function'
}

function cancelAutoSave(): void {
  if (autoSaveTimer === null) return
  clearTimeout(autoSaveTimer)
  autoSaveTimer = null
}

function currentItemId(): number | null {
  // 平行编辑器不改父组件接口，直接复用当前 /doc/:id 路由作为保存目标。
  const rawId = route.params.id
  const idValue = Array.isArray(rawId) ? rawId[0] : rawId
  const itemId = Number(idValue)
  return Number.isSafeInteger(itemId) && itemId > 0 ? itemId : null
}

async function saveSource(source: string): Promise<void> {
  const itemId = currentItemId()
  if (itemId === null) throw new Error('Invalid document id')

  await invokeIpc<number>(IPC.SAVE_VERSION, {
    itemId,
    content: source,
    label: null,
    parentId: null,
    target: 'overwrite',
    source: 'user',
  })

  if (disposed || latestSource !== source) return
  lastSavedSource = source
  saveError.value = ''
  saveState.value = 'saved'
}

function flushSave(): void {
  if (disposed || saveInFlight || pendingSaveSource === null) return

  const source = pendingSaveSource
  pendingSaveSource = null
  if (source === lastSavedSource) {
    saveState.value = 'saved'
    if (pendingSaveSource !== null) flushSave()
    return
  }

  saveError.value = ''
  saveState.value = 'saving'
  saveInFlight = saveSource(source)
    .catch((error: unknown) => {
      if (disposed || latestSource !== source) return
      saveError.value = ipcErrorMessage(error)
      saveState.value = 'error'
      toast.addToast('error', t('doc.overwriteFailed', { error: saveError.value }))
    })
    .finally(() => {
      saveInFlight = null
      if (!disposed && pendingSaveSource !== null) flushSave()
    })
}

function scheduleAutoSave(source: string): void {
  latestSource = source
  if (source === lastSavedSource) {
    pendingSaveSource = null
    cancelAutoSave()
    saveError.value = ''
    saveState.value = 'saved'
    return
  }

  pendingSaveSource = source
  saveError.value = ''
  saveState.value = 'dirty'
  cancelAutoSave()
  autoSaveTimer = setTimeout(() => {
    autoSaveTimer = null
    flushSave()
  }, AUTO_SAVE_DELAY_MS)
}

function saveNow(): void {
  cancelAutoSave()
  pendingSaveSource = latestSource
  if (latestSource === lastSavedSource) {
    pendingSaveSource = null
    saveState.value = 'saved'
    return
  }
  saveError.value = ''
  saveState.value = 'dirty'
  flushSave()
}

function resetHistory(source: string): void {
  historySources.splice(0, historySources.length, source)
  historyIndex = 0
}

function recordSource(source: string): void {
  if (historySources[historyIndex] === source) return

  historySources.splice(historyIndex + 1)
  historySources.push(source)
  historyIndex = historySources.length - 1
}

function restoreHistoryEntry(index: number): void {
  if (!model || index < 0 || index >= historySources.length) return

  const source = historySources[index]
  const currentSelection = model.selection.get()
  const currentOffset = currentSelection?.active ?? source.length

  historyIndex = index
  model.sourceText.set(new StringValue(source), undefined, undefined)
  model.selection.set(
    Selection.collapsed(Math.min(currentOffset, source.length)),
    undefined,
    undefined,
  )
}

function undo(): void {
  if (historyIndex > 0) restoreHistoryEntry(historyIndex - 1)
}

function redo(): void {
  if (historyIndex + 1 < historySources.length) {
    restoreHistoryEntry(historyIndex + 1)
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

function listContinuation(source: string, selection: Selection): string | undefined {
  if (!selection.isCollapsed || model?.activeBlock.get()?.kind === 'codeBlock') {
    return undefined
  }

  const line = currentLine(source, selection.active)
  const lineBeforeCursor = source.slice(line.start, selection.active)
  const match = /^([ \t]*)([-+*]|\d+[.)])([ \t]+)(?:\[([ xX])\]([ \t]+))?/.exec(
    line.text,
  )
  if (!match || !lineBeforeCursor.startsWith(match[0])) return undefined

  const contentBeforeCursor = lineBeforeCursor.slice(match[0].length)
  const newline = source.includes('\r\n') ? '\r\n' : '\n'
  if (contentBeforeCursor.trim().length === 0) {
    return newline
  }

  const marker = match[2]
  const nextMarker = /^\d/.test(marker)
    ? `${Number.parseInt(marker, 10) + 1}${marker.endsWith('.') ? '.' : ')'}`
    : marker
  const taskPrefix = match[4] === undefined ? '' : '[ ] '
  return `${newline}${match[1]}${nextMarker}${match[3]}${taskPrefix}`
}

function insertEnter(): void {
  if (!model) return

  const selection = model.selection.get()
  if (!selection) return

  const source = model.sourceText.get().value
  const newline = source.includes('\r\n') ? '\r\n' : '\n'
  const continuation = listContinuation(source, selection)
  const text = continuation ?? (
    model.activeBlock.get()?.kind === 'codeBlock' ? newline : `${newline}${newline}`
  )

  model.applyEditForSelection(StringEdit.replace(selection.range, text))
}

function handleKeyDown(event: KeyboardEvent): void {
  const modifier = event.ctrlKey || event.metaKey
  if (modifier && !event.altKey) {
    const key = event.key.toLowerCase()
    if (key === 's') {
      event.preventDefault()
      event.stopImmediatePropagation()
      saveNow()
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
  disposed = true
  cancelAutoSave()
  pendingSaveSource = null
  sourceSubscription?.dispose()
  sourceSubscription = null
  view?.element.removeEventListener('keydown', handleKeyDown, true)
  controller?.dispose()
  controller = null
  view?.dispose()
  view = null
  model = null
}

onMounted(() => {
  disposed = false
  latestSource = props.value
  lastSavedSource = props.value
  saveState.value = 'saved'
  saveError.value = ''
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
      latestSource = source.value
      sourceLength.value = source.value.length
      emit('change', source.value)
      scheduleAutoSave(source.value)
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
    cancelAutoSave()
    pendingSaveSource = null
    latestSource = value
    lastSavedSource = value
    saveState.value = 'saved'
    saveError.value = ''
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
