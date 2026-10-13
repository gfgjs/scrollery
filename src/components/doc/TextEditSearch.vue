<template>
  <div v-if="isOpen" ref="searchBar" class="text-edit-search" role="search" @keydown.esc="onEscape">
    <span v-for="(rect, i) in highlights" :key="i" class="text-edit-search__highlight" :style="rect" aria-hidden="true" />
    <div class="text-edit-search__row">
      <input ref="queryInput" v-model="query" class="text-edit-search__query" type="search" :aria-label="t('doc.editorSearchPlaceholder')" :placeholder="t('doc.editorSearchPlaceholder')" @keydown.enter="onEnter">
      <span class="text-edit-search__count" role="status">{{ t('doc.editorSearchCount', { current: matches.length ? currentIndex + 1 : 0, total: matches.length }) }}</span>
      <button class="btn btn-ghost btn-sm" :disabled="!matches.length" :aria-label="t('doc.editorSearchPrevious')" @click="move(-1)">↑</button>
      <button class="btn btn-ghost btn-sm" :disabled="!matches.length" :aria-label="t('doc.editorSearchNext')" @click="move(1)">↓</button>
      <label><input v-model="matchCase" type="checkbox">{{ t('doc.editorMatchCase') }}</label>
      <button class="btn btn-ghost btn-sm" :aria-expanded="showReplace" @click="showReplace = !showReplace">{{ t('doc.editorReplace') }}</button>
      <button class="btn btn-ghost btn-sm" @click="close">{{ t('common.close') }}</button>
    </div>
    <div v-if="showReplace" class="text-edit-search__row">
      <input v-model="replacement" class="text-edit-search__replacement" :aria-label="t('doc.editorReplacement')" :placeholder="t('doc.editorReplacement')" @keydown.enter="onReplaceEnter">
      <button class="btn btn-ghost btn-sm text-edit-search__replace-one" :disabled="disabled || !matches.length" @click="replaceCurrent">{{ t('doc.editorReplace') }}</button>
      <button class="btn btn-ghost btn-sm text-edit-search__replace-all" :disabled="disabled || !matches.length" @click="replaceAll">{{ t('doc.editorReplaceAll') }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useToastStore } from '../../stores/toastStore'

const props = defineProps<{ value: string; textarea: HTMLTextAreaElement | null; disabled: boolean; textStyle: Record<string, string> }>()
const { t } = useI18n()
const toast = useToastStore()
// textarea 会将 CRLF 和裸 CR 显示为 LF，查找/替换必须使用相同的 UTF-16 偏移。
const source = computed(() => props.value.replace(/\r\n?/g, '\n'))
const isOpen = ref(false)
const searchBar = ref<HTMLElement | null>(null)
const queryInput = ref<HTMLInputElement | null>(null)
const query = ref('')
const replacement = ref('')
const matchCase = ref(false)
const showReplace = ref(false)
const index = ref(0)
const highlights = shallowRef<{ left: string; top: string; width: string; height: string }[]>([])
let matchRects: { x: number; y: number; width: number; height: number }[] = []
const matches = computed(() => {
  if (!isOpen.value || !query.value) return []
  // 使用转义后的正则取得原文偏移，避免大小写转换改变字符长度而选错正文。
  const pattern = new RegExp(query.value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), matchCase.value ? 'gu' : 'giu')
  return [...source.value.matchAll(pattern)].map(match => ({ start: match.index, end: match.index + match[0].length }))
})
const currentIndex = computed(() => Math.min(index.value, Math.max(0, matches.value.length - 1)))

// textarea 失焦后不绘制原生选区；独立显示当前命中，使查询输入框保持焦点时仍能定位。
function paintHighlights() {
  const area = props.textarea
  if (!area || !isOpen.value) { highlights.value = []; return }
  const bounds = area.getBoundingClientRect()
  highlights.value = matchRects.flatMap(rect => {
    const left = Math.max(bounds.left, bounds.left + rect.x - area.scrollLeft)
    const right = Math.min(bounds.right, bounds.left + rect.x + rect.width - area.scrollLeft)
    const top = Math.max(bounds.top, bounds.top + rect.y - area.scrollTop)
    const bottom = Math.min(bounds.bottom, bounds.top + rect.y + rect.height - area.scrollTop)
    return right > left && bottom > top ? [{ left: `${left}px`, top: `${top}px`, width: `${right - left}px`, height: `${bottom - top}px` }] : []
  })
}

function measureMatch() {
  const area = props.textarea
  const match = matches.value[currentIndex.value]
  matchRects = []
  if (!area || !match) { paintHighlights(); return }
  // textarea 不提供文本坐标；按实际字体和折行宽度测量匹配行，查找远处内容时仍能看见命中。
  const mirror = document.createElement('div')
  const style = getComputedStyle(area)
  Object.assign(mirror.style, {
    position: 'fixed', visibility: 'hidden', pointerEvents: 'none', top: '0', left: '0',
    boxSizing: 'border-box', width: `${area.clientWidth}px`, whiteSpace: 'pre-wrap', overflowWrap: 'break-word',
    font: style.font, lineHeight: style.lineHeight, letterSpacing: style.letterSpacing,
    padding: style.padding, tabSize: style.tabSize,
  })
  // 同一逻辑行的后续文字会决定整个单词是否换行；截到命中末尾会把高亮画在上一行。
  const lineEnd = source.value.indexOf('\n', match.end)
  mirror.textContent = source.value.slice(0, lineEnd < 0 ? source.value.length : lineEnd)
  document.body.append(mirror)
  const range = document.createRange()
  range.setStart(mirror.firstChild!, match.start)
  range.setEnd(mirror.firstChild!, match.end)
  matchRects = [...range.getClientRects()].map(rect => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height }))
  mirror.remove()
  paintHighlights()
}

function selectCurrent() {
  const area = props.textarea
  const match = matches.value[currentIndex.value]
  if (!area || !match) { measureMatch(); return }
  area.setSelectionRange(match.start, match.end)
  measureMatch()
  if (matchRects[0]) area.scrollTop = Math.max(0, matchRects[0].y - area.clientHeight / 2)
  paintHighlights()
}

watch([query, matchCase], () => { index.value = 0; selectCurrent() }, { flush: 'post' })
watch(() => source.value, measureMatch, { flush: 'post' })
watch(() => props.textStyle, measureMatch, { flush: 'post' })
let unwatchArea = () => {}
watch(() => props.textarea, area => {
  unwatchArea()
  if (!area) return
  area.addEventListener('scroll', paintHighlights)
  const observer = new ResizeObserver(measureMatch)
  observer.observe(area)
  unwatchArea = () => { observer.disconnect(); area.removeEventListener('scroll', paintHighlights) }
}, { immediate: true, flush: 'post' })
onBeforeUnmount(() => unwatchArea())

/** 打开当前草稿的查找；选中文本可直接作为查询，重复快捷键选中现有查询。 */
function open(replace = false) {
  let selectedStart: number | null = null
  if (!isOpen.value && props.textarea) {
    const area = props.textarea
    const selected = source.value.slice(area.selectionStart, area.selectionEnd)
    if (selected && !selected.includes('\n')) {
      selectedStart = area.selectionStart
      query.value = selected
    }
  }
  isOpen.value = true
  showReplace.value = replace
  void nextTick(() => {
    // 等查询监听完成后恢复原选中命中，避免替换误作用于前面的同名词。
    if (selectedStart !== null) {
      const selectedIndex = matches.value.findIndex(match => match.start === selectedStart)
      if (selectedIndex >= 0) index.value = selectedIndex
    }
    selectCurrent()
    queryInput.value?.focus()
    queryInput.value?.select()
  })
}

/** 关闭查找并回到当前命中处继续编辑。 */
function close(): boolean {
  if (!isOpen.value) return false
  isOpen.value = false
  props.textarea?.focus({ preventScroll: true })
  return true
}

function move(direction: number) {
  if (!matches.value.length) return
  index.value = (currentIndex.value + direction + matches.value.length) % matches.value.length
  selectCurrent()
}

function onEnter(event: KeyboardEvent) {
  if (event.isComposing) return
  event.preventDefault()
  move(event.shiftKey ? -1 : 1)
}

function onEscape(event: KeyboardEvent) {
  if (event.isComposing) return
  event.preventDefault()
  event.stopPropagation()
  close()
}

async function replaceRange(start: number, end: number, value: string) {
  const area = props.textarea
  if (!area || props.disabled || source.value.slice(start, end) === value) return
  const active = document.activeElement
  const previousControl = active instanceof HTMLElement && searchBar.value?.contains(active) ? active : null
  area.focus({ preventScroll: true })
  area.setSelectionRange(start, end)
  try {
    // 原生编辑命令保留浏览器撤销栈；直接赋 value / setRangeText 会令替换无法撤销。
    if (!document.execCommand('insertText', false, value)) {
      toast.addToast('error', t('doc.editorReplaceFailed'))
      return
    }
    // 部分 WebView 不发 input；统一让父层 v-model 收到实际值，不重写 DOM。
    area.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    const next = matches.value.findIndex(match => match.start >= start + value.length)
    index.value = next >= 0 ? next : 0
    selectCurrent()
  } finally {
    // execCommand 只临时借用正文焦点，连续 Enter 仍应替换，不能误向正文插入换行。
    // 用户已转移焦点或关闭查找时不抢回；最后一项替换使按钮禁用时回到查询框。
    if (isOpen.value && previousControl?.isConnected && props.textarea === area && document.activeElement === area) {
      const control = previousControl.matches(':disabled') ? queryInput.value : previousControl
      control?.focus({ preventScroll: true })
    }
  }
}

function replaceCurrent() {
  const match = matches.value[currentIndex.value]
  if (match) void replaceRange(match.start, match.end, replacement.value)
}

function replaceAll() {
  if (!matches.value.length) return
  const chunks: string[] = []
  let previous = 0
  for (const match of matches.value) {
    chunks.push(source.value.slice(previous, match.start), replacement.value)
    previous = match.end
  }
  chunks.push(source.value.slice(previous))
  void replaceRange(0, source.value.length, chunks.join(''))
}

function onReplaceEnter(event: KeyboardEvent) {
  if (event.isComposing) return
  event.preventDefault()
  replaceCurrent()
}

defineExpose({ open, close, isOpen })
</script>

<style scoped>
.text-edit-search { flex: 0 0 auto; padding: 8px 12px; border-bottom: 1px solid var(--color-divider); background: var(--color-bg-secondary); }
.text-edit-search__row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
.text-edit-search__row + .text-edit-search__row { margin-top: 6px; }
.text-edit-search__query, .text-edit-search__replacement { flex: 1; min-width: 130px; max-width: 360px; padding: 5px 8px; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-bg-primary); color: var(--color-text-primary); }
.text-edit-search__row label { display: flex; align-items: center; gap: 4px; font-size: var(--font-size-sm); }
.text-edit-search__count { min-width: 42px; font-size: var(--font-size-sm); color: var(--color-text-secondary); }
.text-edit-search__highlight { position: fixed; z-index: 1; pointer-events: none; background: var(--color-accent); opacity: 0.24; }
</style>
