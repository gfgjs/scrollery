<template>
  <div class="text-edit-position">
    <button class="text-edit-position__status" type="button" :aria-expanded="isOpen" :title="t('doc.editorGoToLine')" @click="open">{{ t('doc.editorPosition', { ...position, total: lineIndex.starts.length }) }}</button>
    <form v-if="isOpen" class="text-edit-position__panel" role="dialog" :aria-label="t('doc.editorGoToLine')" @submit.prevent="jump" @keydown.esc="onEscape">
      <label class="text-edit-position__label">{{ t('doc.editorGoToLine') }}
        <input ref="lineInput" v-model="requestedLine" class="text-edit-position__input" type="text" inputmode="numeric" autocomplete="off" :aria-invalid="invalid" :aria-label="t('doc.editorGoToLine')" @input="invalid = false">
      </label>
      <button class="btn btn-ghost btn-sm" type="button" @click="close">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary btn-sm" type="submit">{{ t('doc.editorGo') }}</button>
      <span v-if="invalid" class="text-edit-position__error" role="alert">{{ t('doc.editorLineRange', { total: lineIndex.starts.length }) }}</span>
    </form>
  </div>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { indexTextLines, textPositionAt } from './textEditPosition'

const props = defineProps<{ value: string; textarea: HTMLTextAreaElement | null }>()
const emit = defineEmits<{ open: [] }>()
const { t } = useI18n()
const isOpen = ref(false)
const lineInput = ref<HTMLInputElement | null>(null)
const requestedLine = ref('1')
const invalid = ref(false)
const lineIndex = shallowRef(indexTextLines(props.value))
const position = ref({ line: 1, column: 1 })
let savedSelection: { area: HTMLTextAreaElement; start: number; end: number; direction: 'forward' | 'backward' | 'none'; top: number; left: number } | null = null

function updatePosition() {
  const area = props.textarea
  if (!area) return
  position.value = textPositionAt(lineIndex.value, area.selectionDirection === 'backward' ? area.selectionStart : area.selectionEnd)
}

function updateSource() {
  const source = props.textarea?.value ?? props.value
  if (source !== lineIndex.value.source) lineIndex.value = indexTextLines(source)
  updatePosition()
}

function dismiss() {
  isOpen.value = false
  savedSelection = null
}

function onTextInput() {
  dismiss()
  updateSource()
}

watch(() => props.value, updateSource, { flush: 'post' })
let stopListening = () => {}
watch(() => props.textarea, area => {
  stopListening()
  isOpen.value = false
  savedSelection = null
  updateSource()
  if (!area) return
  const selectionChanged = () => { if (area.ownerDocument.activeElement === area) updatePosition() }
  area.addEventListener('input', onTextInput)
  area.addEventListener('pointerdown', dismiss)
  area.addEventListener('select', updatePosition)
  area.addEventListener('keyup', updatePosition)
  area.addEventListener('click', updatePosition)
  area.ownerDocument.addEventListener('selectionchange', selectionChanged)
  stopListening = () => {
    area.removeEventListener('input', onTextInput)
    area.removeEventListener('pointerdown', dismiss)
    area.removeEventListener('select', updatePosition)
    area.removeEventListener('keyup', updatePosition)
    area.removeEventListener('click', updatePosition)
    area.ownerDocument.removeEventListener('selectionchange', selectionChanged)
  }
}, { immediate: true, flush: 'post' })
onBeforeUnmount(() => stopListening())

/** 打开行号输入，保留正文选区及滚动位置供取消恢复。 */
function open(): void {
  const area = props.textarea
  if (!area) return
  emit('open')
  if (!isOpen.value) {
    updatePosition()
    savedSelection = { area, start: area.selectionStart, end: area.selectionEnd, direction: area.selectionDirection, top: area.scrollTop, left: area.scrollLeft }
    requestedLine.value = String(position.value.line)
    invalid.value = false
  }
  isOpen.value = true
  void nextTick(() => { lineInput.value?.focus({ preventScroll: true }); lineInput.value?.select() })
}

/** 取消定位，恢复打开前的选区、焦点和滚动，不修改正文或浏览器撤销记录。 */
function close(): boolean {
  if (!isOpen.value) return false
  isOpen.value = false
  const saved = savedSelection
  savedSelection = null
  if (saved && props.textarea === saved.area) {
    saved.area.focus({ preventScroll: true })
    saved.area.setSelectionRange(saved.start, saved.end, saved.direction)
    saved.area.scrollTop = saved.top
    saved.area.scrollLeft = saved.left
    updatePosition()
  }
  return true
}

function jump() {
  const area = props.textarea
  const candidate = requestedLine.value.trim()
  const line = Number(candidate)
  if (!area || !/^\d+$/.test(candidate) || !Number.isInteger(line) || line < 1 || line > lineIndex.value.starts.length) {
    invalid.value = true
    return
  }
  const offset = lineIndex.value.starts[line - 1]!
  isOpen.value = false
  savedSelection = null
  area.focus({ preventScroll: true })
  area.setSelectionRange(offset, offset)
  // 重新聚焦让浏览器按实际折行把新光标滚入视口，不猜测字体行高。
  area.blur()
  area.focus()
  updatePosition()
}

function onEscape(event: KeyboardEvent) {
  if (event.isComposing) return
  event.preventDefault()
  event.stopPropagation()
  close()
}

defineExpose({ open, close })
</script>

<style scoped>
.text-edit-position { position: relative; flex: 1 1 auto; min-width: 0; text-align: right; }
.text-edit-position__status { padding: 2px 4px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--color-text-secondary); font: inherit; cursor: pointer; }
.text-edit-position__status:hover { background: var(--color-bg-hover); color: var(--color-text-primary); }
.text-edit-position__status:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
.text-edit-position__panel { position: absolute; z-index: 10; right: 0; bottom: calc(100% + 8px); display: flex; flex-wrap: wrap; align-items: center; gap: 8px; width: min(360px, calc(100vw - 32px)); padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg-primary); box-shadow: var(--shadow-lg); color: var(--color-text-primary); text-align: left; }
.text-edit-position__label { display: flex; flex: 1; min-width: 0; align-items: center; gap: 8px; }
.text-edit-position__input { min-width: 0; width: 80px; flex: 1; padding: 5px 8px; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-bg-secondary); color: var(--color-text-primary); font: inherit; }
.text-edit-position__error { flex-basis: 100%; color: var(--color-error); }
</style>
