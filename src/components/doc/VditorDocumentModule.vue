<template>
  <div class="vditor-document" :style="typographyStyle" @keydown.capture="handleKeyDown" @keydown="normalizeTableSelection" @paste.capture="routeEditorPaste" @drop.capture="routeImageDrop" @scroll.capture="captureReadingPosition" @click.capture="captureModePosition" @touchstart.capture="captureModePosition">
    <div v-if="cachedDraft !== null" class="vditor-document__draft" role="status">
      <span>{{ t('doc.vditorCachedDraft') }}</span>
      <button class="btn btn-primary btn-sm" :disabled="!ready" @click="restoreCachedDraft">
        {{ t('doc.vditorRestoreDraft') }}
      </button>
      <button class="btn btn-ghost btn-sm" @click="cachedDraft = null">
        {{ t('doc.vditorKeepSource') }}
      </button>
    </div>
    <div v-if="searchOpen" class="vditor-document__search" role="search">
      <input
        ref="searchInput"
        v-model="searchQuery"
        type="search"
        :placeholder="t('doc.editorSearchPlaceholder')"
        :aria-label="t('doc.editorSearchPlaceholder')"
        @keydown.enter="handleSearchEnter"
      >
      <span class="vditor-document__search-count" role="status">
        {{ t('doc.editorSearchCount', { current: searchTotal ? searchIndex + 1 : 0, total: searchTotal }) }}
      </span>
      <button class="btn btn-ghost btn-sm" :disabled="!searchTotal" :aria-label="t('doc.editorSearchPrevious')" :title="t('doc.editorSearchPrevious')" @click="moveSearch(-1)">↑</button>
      <button class="btn btn-ghost btn-sm" :disabled="!searchTotal" :aria-label="t('doc.editorSearchNext')" :title="t('doc.editorSearchNext')" @click="moveSearch(1)">↓</button>
      <button class="btn btn-ghost btn-sm vditor-document__replace-toggle" :aria-expanded="replaceOpen" @click="toggleReplace">{{ t('doc.editorReplace') }}</button>
      <button class="btn btn-ghost btn-sm" @click="closeSearch">{{ t('common.close') }}</button>
      <div v-if="replaceOpen" class="vditor-document__replace">
        <template v-if="searchMode === 'sv'">
          <input ref="replaceInput" v-model="replacement" :placeholder="t('doc.editorReplacement')" :aria-label="t('doc.editorReplacement')" @keydown.enter="handleReplaceEnter">
          <button class="btn btn-ghost btn-sm vditor-document__replace-one" :disabled="!searchTotal" @click="replaceSource(false)">{{ t('doc.editorReplace') }}</button>
          <button class="btn btn-ghost btn-sm vditor-document__replace-all" :disabled="!searchTotal" @click="replaceSource(true)">{{ t('doc.editorReplaceAll') }}</button>
        </template>
        <template v-else>
          <span class="vditor-document__replace-hint">{{ t('doc.editorReplaceSourceHint') }}</span>
          <button class="btn btn-ghost btn-sm vditor-document__replace-source" @click="switchToReplaceSource">{{ t('doc.editorSwitchToSource') }}</button>
        </template>
      </div>
    </div>
    <div ref="host" class="vditor-document__host" />
    <div
      class="vditor-document__status"
      :class="{ 'vditor-document__status--error': saveState === 'error' }"
      :role="saveState === 'error' ? 'alert' : 'status'"
    >
      <span>{{ saveStatusText }}</span>
      <button v-if="saveState === 'error'" class="btn btn-ghost btn-sm" @click="saveNow">
        {{ t('official.retry') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router'
import Vditor from 'vditor'
import { useMarkdownAutoSave } from '../../composables/reader/useMarkdownAutoSave'
import { useToastStore } from '../../stores/toastStore'
import { ipcErrorMessage } from '../../utils/ipc'
import { FONT_STACKS } from '../../utils/readerStyles'
import { formatVditorPosition, parseVditorPosition } from '../../utils/readerPosition'
import { findVditorMatches } from './vditorSearch'
import { indentSource, type SourceIndentResult } from './sourceIndent'
import { installVditorCodeWhitespace } from './vditorCodeWhitespace'
import { handleVditorNestedSelection } from './vditorNestedSelection'
import { createVditorImagePasteController, type VditorImagePasteController } from './vditorImagePaste'
import { captureVditorModeCaret, restoreVditorModeCaret, type VditorModeCaret } from './vditorModeCaret'
import { getVditorResourceBaseUrl, installVditorResourceSerialization, resolveVditorLinkUrl, scrollToVditorHeading } from './vditorResourceBase'
import 'vditor/dist/index.css'
import 'vditor/dist/js/icons/ant.js'

const props = defineProps<{
  itemId: number
  value: string
  documentUrl: string
  initial?: string | null
  isDark: boolean
  typography: {
    fontSizePx: number
    lineHeight: number
    fontFamily: 'serif' | 'sans'
    maxInlineSizePx: number
  }
}>()
const emit = defineEmits<{
  (event: 'change', value: string): void
  (event: 'panel-open'): void
  (event: 'progress', position: string, itemId: number): void
}>()
const { t } = useI18n()
const toast = useToastStore()
const autoSave = useMarkdownAutoSave({ itemId: props.itemId, initialSource: props.value })
const { saveState, saveError } = autoSave

const host = ref<HTMLElement | null>(null)
const ready = ref(false)
const sourceLength = ref(props.value.length)
const cachedDraft = ref<string | null>(null)
const cacheKey = `scrollery-vditor-${props.itemId}`
let editor: Vditor | null = null
let imagePaste: VditorImagePasteController | null = null
let undoMode: 'ir' | 'wysiwyg' | 'sv' | null = null
let disposed = false
let renderedSource = props.value
let restoredCacheSource: string | null = null
const positionItemId = props.itemId
let positionFraction = parseVditorPosition(props.initial)
let pendingPosition = positionFraction
let positionMode: 'ir' | 'wysiwyg' | 'sv' | null = null
let pendingModeCaret: VditorModeCaret | null = null
let positionFrame: number | undefined
let positionObserver: MutationObserver | undefined
let positionResizeObserver: ResizeObserver | undefined
let reportedPosition: string | null = null
const searchOpen = ref(false)
const searchInput = ref<HTMLInputElement | null>(null)
const replaceInput = ref<HTMLInputElement | null>(null)
const replaceOpen = ref(false)
const replacement = ref('')
const searchMode = ref<'ir' | 'wysiwyg' | 'sv'>('ir')
const searchQuery = ref('')
const searchMatches = shallowRef<Range[]>([])
const sourceMatches = shallowRef<{ start: number; end: number }[]>([])
const searchTotal = computed(() => replaceOpen.value && searchMode.value === 'sv' ? sourceMatches.value.length : searchMatches.value.length)
const searchIndex = ref(0)
let searchTimer: ReturnType<typeof setTimeout> | undefined
let searchObserver: MutationObserver | undefined
let searchSelection: Range | null = null
const searchHighlight = 'scrollery-vditor-search'
const currentHighlight = 'scrollery-vditor-search-current'
const typographyStyle = computed(() => ({
  '--vditor-reader-font-size': `${props.typography.fontSizePx}px`,
  '--vditor-reader-line-height': String(props.typography.lineHeight),
  '--vditor-reader-font-family': FONT_STACKS[props.typography.fontFamily],
  '--vditor-reader-width': `${props.typography.maxInlineSizePx}px`,
}))
const contentThemePath = '/vditor/dist/css/content-theme'
const editorAppearance = computed(() => ({
  theme: props.isDark ? 'dark' as const : 'classic' as const,
  content: props.isDark ? 'dark' : 'light',
  code: props.isDark ? 'github-dark' : 'github',
}))

watch([ready, editorAppearance], () => {
  if (!ready.value || !editor) return
  const appearance = editorAppearance.value
  // 仅更新主题 class 与本地样式链接，保留正文节点、选区和撤销历史。
  editor.setTheme(appearance.theme, appearance.content, appearance.code, contentThemePath)
})

/** 同步读取当前位置，供父层在先于子组件卸载的导航时机保存。 */
function getCurrentPosition(): string | null {
  if (disposed) return null
  const state = editor?.vditor
  const root = state?.[state.currentMode]?.element
  if (ready.value && state?.currentMode === positionMode && pendingPosition === null && positionFrame === undefined && root && root.clientHeight > 0) {
    const extent = root.scrollHeight - root.clientHeight
    // 短文和隐藏面板没有可比较的比例，不能用此时的 0 覆盖已有位置。
    if (extent > 0) positionFraction = Math.min(1, Math.max(0, root.scrollTop / extent))
  }
  return positionFraction === null ? null : formatVditorPosition(positionFraction)
}

function captureReadingPosition(): void {
  const position = getCurrentPosition()
  if (position !== null && position !== reportedPosition) {
    reportedPosition = position
    emit('progress', position, positionItemId)
  }
}

function captureModePosition(event: Event): void {
  const target = event.target instanceof Element ? event.target.closest('[data-mode]') : null
  if (!target) return
  captureReadingPosition()
  pendingModeCaret = ready.value && editor && target.getAttribute('data-mode') !== editor.vditor.currentMode
    ? captureVditorModeCaret(editor) : null
}

function restoreReadingPosition(): void {
  if (disposed || !ready.value || !editor) return
  const mode = editor.vditor.currentMode
  if (mode !== positionMode) {
    positionMode = mode
    pendingPosition = positionFraction
  }
  if ((pendingPosition === null && pendingModeCaret === null) || positionFrame !== undefined) return
  positionFrame = requestAnimationFrame(() => {
    positionFrame = undefined
    if (disposed || !editor || (pendingPosition === null && pendingModeCaret === null)) return
    const root = editor.vditor[editor.vditor.currentMode]?.element
    if (!root || root.clientHeight <= 0) return
    if (pendingPosition !== null && root.scrollHeight > root.clientHeight) {
      root.scrollTop = pendingPosition * (root.scrollHeight - root.clientHeight)
      pendingPosition = null
    }
    if (pendingModeCaret !== null) {
      restoreVditorModeCaret(editor, pendingModeCaret)
      pendingModeCaret = null
    }
    if (pendingPosition !== null) return
    // 上游切模式会聚焦并重建正文；等布局及程序滚动事件结束后才采集新比例。
    positionFrame = requestAnimationFrame(() => {
      positionFrame = undefined
      if (pendingPosition !== null) restoreReadingPosition()
    })
  })
}

function initializeReadingPosition(): void {
  if (!editor) return
  positionMode = editor.vditor.currentMode
  if (typeof MutationObserver !== 'undefined') {
    positionObserver = new MutationObserver(restoreReadingPosition)
    for (const root of [editor.vditor.ir?.element, editor.vditor.wysiwyg?.element, editor.vditor.sv?.element]) {
      if (!root) continue
      // IR/WYS 切换外层容器，SV 直接切换正文 display；不监听正文编辑与选区。
      positionObserver.observe(root, { attributes: true, attributeFilter: ['style'] })
      if (root.parentElement) positionObserver.observe(root.parentElement, { attributes: true, attributeFilter: ['style'] })
    }
  }
  if (typeof ResizeObserver !== 'undefined' && host.value) {
    positionResizeObserver = new ResizeObserver(restoreReadingPosition)
    positionResizeObserver.observe(host.value)
  }
  restoreReadingPosition()
}

function clearHighlights(): void {
  if (typeof CSS === 'undefined' || !CSS.highlights) return
  CSS.highlights.delete(searchHighlight)
  CSS.highlights.delete(currentHighlight)
}

function showSearchMatch(selectFallback = false): void {
  const range = searchMatches.value[searchIndex.value]
  if (!range) return
  if (typeof CSS !== 'undefined' && CSS.highlights && typeof Highlight !== 'undefined') {
    CSS.highlights.set(currentHighlight, new Highlight(range))
  } else if (selectFallback) {
    // 旧 WebView 不支持自定义高亮时，仅显式跳转才使用原生选区。
    const selection = range.startContainer.ownerDocument?.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
  }
  const state = editor?.vditor
  const scroller = state?.[state.currentMode]?.element
  if (scroller) {
    const bounds = range.getBoundingClientRect()
    scroller.scrollBy({ top: bounds.top - scroller.getBoundingClientRect().top - scroller.clientHeight / 2 })
  } else range.startContainer.parentElement?.scrollIntoView({ block: 'center', inline: 'nearest' })
}

function refreshSearch(): void {
  clearHighlights()
  searchMatches.value = []
  sourceMatches.value = []
  searchIndex.value = 0
  if (!searchOpen.value || !editor) return
  const state = editor.vditor
  searchMode.value = state.currentMode
  if (!searchQuery.value) return
  const root = state[state.currentMode]?.element
  if (!root) return
  searchMatches.value = findVditorMatches(root, searchQuery.value)
  if (replaceOpen.value && state.currentMode === 'sv') sourceMatches.value = findSourceMatches(editor.getValue())
  if (typeof CSS !== 'undefined' && CSS.highlights && typeof Highlight !== 'undefined') {
    const highlights = new Highlight()
    for (const range of searchMatches.value) highlights.add(range)
    CSS.highlights.set(searchHighlight, highlights)
  }
  showSearchMatch()
}

function scheduleSearch(): void {
  if (searchTimer !== undefined) clearTimeout(searchTimer)
  if (searchOpen.value) searchTimer = setTimeout(refreshSearch, 150)
}

function closeSearch(): void {
  const state = editor?.vditor
  const root = state?.[state.currentMode]?.element
  const returnToEditor = !disposed && root?.ownerDocument?.activeElement?.closest('.vditor-document__search')
  searchOpen.value = false
  replaceOpen.value = false
  if (searchTimer !== undefined) clearTimeout(searchTimer)
  searchObserver?.disconnect()
  searchObserver = undefined
  searchMatches.value = []
  sourceMatches.value = []
  clearHighlights()
  // 搜索框会接管原生选区；仅从查找栏返回且正文节点仍有效时恢复编辑位置。
  if (returnToEditor && searchSelection && root?.contains(searchSelection.startContainer) && root.contains(searchSelection.endContainer)) {
    root.focus({ preventScroll: true })
    const selection = root.ownerDocument.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(searchSelection)
  }
  searchSelection = null
}

/** 聚焦当前可见正文的文内查找，不改写 Markdown 或编辑器 DOM。 */
function openSearch(): void {
  if (searchOpen.value) {
    searchInput.value?.focus()
    searchInput.value?.select()
    return
  }
  if (!ready.value || !editor) return
  searchMode.value = editor.vditor.currentMode
  const root = editor.vditor[editor.vditor.currentMode]?.element
  const selection = root?.ownerDocument?.getSelection()
  if (selection?.rangeCount && root?.contains(selection.getRangeAt(0).commonAncestorContainer)) {
    searchSelection = selection.getRangeAt(0).cloneRange()
    const selected = searchSelection.toString()
    if (selected && !/[\r\n]/.test(selected)) searchQuery.value = selected
  }
  searchOpen.value = true
  emit('panel-open')
  if (typeof MutationObserver !== 'undefined') {
    searchObserver = new MutationObserver(scheduleSearch)
    // 只监听正文及模式容器，目录随滚动变更选中项不应重置当前匹配。
    for (const root of [editor.vditor.ir?.element, editor.vditor.wysiwyg?.element, editor.vditor.sv?.element]) {
      if (!root) continue
      // 源码模式直接切换 pre 的 display；同时监听正文与容器样式，切换后只高亮当前模式。
      searchObserver.observe(root, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['style'] })
      if (root.parentElement) searchObserver.observe(root.parentElement, { attributes: true, attributeFilter: ['style'] })
    }
  }
  scheduleSearch()
  void nextTick(() => { searchInput.value?.focus(); searchInput.value?.select() })
}

/** 全局工具按钮切换查找面板。 */
function toggleSearch(): void {
  if (searchOpen.value) closeSearch()
  else openSearch()
}

function moveSearch(direction: number): void {
  if (!searchTotal.value) return
  searchIndex.value = (searchIndex.value + direction + searchTotal.value) % searchTotal.value
  // 明确跳转后返回该命中继续编辑；仅输入查询时仍保留打开前的光标。
  searchSelection = searchMatches.value[searchIndex.value]?.cloneRange() ?? searchSelection
  showSearchMatch(true)
}

function handleSearchEnter(event: KeyboardEvent): void {
  if (event.isComposing) return
  event.preventDefault()
  moveSearch(event.shiftKey ? -1 : 1)
}

/** 打开替换入口；实际改写只允许源码模式，渲染预览不能作为可编辑源文。 */
function openReplace(): void {
  openSearch()
  if (!searchOpen.value) return
  replaceOpen.value = true
  refreshSearch()
}

function toggleReplace(): void {
  replaceOpen.value = !replaceOpen.value
  refreshSearch()
  if (replaceOpen.value) void nextTick(() => replaceInput.value?.focus())
}

function switchToReplaceSource(): void {
  const button = host.value?.querySelector<HTMLButtonElement>('.vditor-toolbar button[data-mode="sv"]')
  // 复用上游模式菜单的点击入口。
  button?.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }))
  refreshSearch()
  void nextTick(() => replaceInput.value?.focus())
}

function findSourceMatches(source: string): { start: number; end: number }[] {
  if (!searchQuery.value) return []
  const expression = new RegExp(searchQuery.value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu')
  return Array.from(source.matchAll(expression), match => ({ start: match.index, end: match.index + match[0].length }))
}

/** 替换与缩进共用原子源码事务；选区在最终快照前恢复，避免额外的光标撤销项。 */
function commitSourceChange(change: () => void, restoreSelection?: () => void): void {
  if (!editor) return
  const state = editor.vditor
  const root = state.sv?.element
  if (!root || !state.undo) return
  if (searchTimer !== undefined) clearTimeout(searchTimer)
  root.focus({ preventScroll: true })
  state.undo.addToUndoStack(state)
  change()
  window.clearTimeout(state.sv?.processTimeoutId)
  window.clearTimeout(state.ir?.processTimeoutId)
  window.clearTimeout(state.wysiwyg?.afterRenderTimeoutId)
  restoreSelection?.()
  state.undo.addToUndoStack(state)
  // 上游快照会把反向选区改为正向；快照不变，再恢复一次用户选区方向。
  restoreSelection?.()
  applySource(editor.getValue())
}

function sourcePoints(root: HTMLElement, positions: number[], preferPrevious = false): [Node, number][] {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  let node = walker.nextNode()
  let last: Node | null = null
  let offset = 0
  return positions.map(position => {
    // 行首落在前一行隐藏换行文本的末尾时，选下一文本节点，确保新空格可见。
    while (node && (offset + (node.textContent?.length ?? 0) < position ||
      (!preferPrevious && offset + (node.textContent?.length ?? 0) === position))) {
      offset += node.textContent?.length ?? 0
      last = node
      node = walker.nextNode()
    }
    return node ? [node, position - offset] : last ? [last, last.textContent?.length ?? 0] : [root, 0]
  })
}

function restoreSourceSelection(root: HTMLElement, result: SourceIndentResult, backward: boolean): void {
  const [first] = sourcePoints(root, [result.start])
  // 选区末端停在正文末尾，避免落入后续隐藏换行后让浏览器多选一个可见换行。
  const last = result.start === result.end ? first : sourcePoints(root, [result.end], true)[0]
  root.ownerDocument.getSelection()?.setBaseAndExtent(...(backward ? last : first), ...(backward ? first : last))
}

function indentSourceSelection(event: KeyboardEvent): boolean {
  if (!ready.value || !editor || editor.vditor.currentMode !== 'sv') return false
  const root = editor.vditor.sv?.element
  if (!root) return false
  if (!(event.target instanceof Node) || !root.contains(event.target)) return false
  const selection = root.ownerDocument.getSelection()
  if (!selection?.rangeCount) return false
  const range = selection.getRangeAt(0)
  if (!root.contains(range.commonAncestorContainer)) return false
  const prefix = range.cloneRange()
  prefix.selectNodeContents(root)
  prefix.setEnd(range.startContainer, range.startOffset)
  const start = prefix.toString().length
  const end = start + range.toString().length
  const backward = !range.collapsed && selection.anchorNode === range.endContainer && selection.anchorOffset === range.endOffset
  const source = editor.getValue()
  const result = indentSource(source, start, end, event.shiftKey)
  // 顶层列表反缩进也消费按键，避开上游空前缀异常与浏览器跳焦点。
  event.preventDefault()
  event.stopImmediatePropagation()
  if (result.source === source) return true
  const scrollTop = root.scrollTop
  const scrollLeft = root.scrollLeft
  commitSourceChange(() => {
    // 仅编辑空白，保留正文和现有源码标记，不重新解析整篇文档。
    const points = sourcePoints(root, result.edits.flatMap(edit => [edit.position, edit.position + edit.remove]))
    const ranges = result.edits.map((edit, index) => {
      const range = root.ownerDocument.createRange()
      range.setStart(...points[index * 2])
      range.setEnd(...points[index * 2 + 1])
      return { range, edit }
    })
    for (const { range, edit } of ranges.reverse()) {
      range.deleteContents()
      if (edit.insert) range.insertNode(root.ownerDocument.createTextNode(edit.insert))
    }
  }, () => restoreSourceSelection(root, result, backward))
  refreshSearch()
  searchObserver?.takeRecords()
  if (searchOpen.value && selection.rangeCount) searchSelection = selection.getRangeAt(0).cloneRange()
  root.scrollTop = scrollTop
  root.scrollLeft = scrollLeft
  return true
}

function replaceSource(all: boolean): void {
  if (!ready.value || !editor || editor.vditor.currentMode !== 'sv') return
  const state = editor.vditor
  const root = state.sv?.element
  if (!state.undo || !root) return
  const before = editor.getValue()
  // 每次使用当前源码重新计算位置，避免第三方重绘使旧 Range 失效后写错位置。
  const matches = findSourceMatches(before)
  if (!matches.length) return
  const current = matches[Math.min(searchIndex.value, matches.length - 1)]
  const selected = all ? matches : [current]
  const chunks: string[] = []
  let previous = 0
  for (const match of selected) {
    chunks.push(before.slice(previous, match.start), replacement.value)
    previous = match.end
  }
  chunks.push(before.slice(previous))
  const expected = chunks.join('')
  if (expected === before) return
  commitSourceChange(() => editor!.setValue(expected, false))
  refreshSearch()
  if (!all) {
    const next = sourceMatches.value.findIndex(match => match.start >= current.start + replacement.value.length)
    searchIndex.value = Math.max(0, next)
    showSearchMatch()
  }
  // 丢弃本次已显式刷新的变更，避免延迟观察回调把“下一个匹配”重新跳回首项。
  searchObserver?.takeRecords()
  searchSelection = searchMatches.value[searchIndex.value]?.cloneRange() ?? root.ownerDocument.createRange()
  if (!root.contains(searchSelection.startContainer)) {
    searchSelection.selectNodeContents(root)
    searchSelection.collapse(false)
  }
  void nextTick(() => replaceInput.value?.focus())
}

function handleReplaceEnter(event: KeyboardEvent): void {
  if (event.isComposing) return
  event.preventDefault()
  replaceSource(false)
}

/** 返回是否收起了编辑器面板，供全局退出与沉浸模式先处理面板。 */
function closePanels(): boolean {
  const hadSearch = searchOpen.value
  const hadOutline = editor?.vditor?.outline.element.style.display === 'block'
  closeSearch()
  if (hadOutline && editor) {
    editor.vditor.options.outline!.enable = false
    editor.vditor.outline.toggle(editor.vditor, false, false)
  }
  return hadSearch || hadOutline
}

watch(searchQuery, () => { searchIndex.value = 0; scheduleSearch() })

const saveStatusText = computed(() => {
  if (saveState.value === 'saving') return t('edit.saving')
  if (saveState.value === 'dirty') return t('doc.markdownEditorUnsaved', { length: sourceLength.value })
  if (saveState.value === 'error') return t('doc.overwriteFailed', { error: ipcErrorMessage(saveError.value) })
  return t('edit.savedNeedsIndexTitle')
})

function applySource(source: string): void {
  renderedSource = source
  sourceLength.value = source.length
  emit('change', source)
  autoSave.updateSource(source)
}

/** 保存前直接读取编辑器，覆盖第三方 input 防抖尚未发出的最后一次输入。 */
async function saveNow(): Promise<boolean> {
  captureReadingPosition()
  for (;;) {
    // 每轮无条件消费图片输入屏障：既等待在途输入，也复核上一轮写盘期间落定的结果。
    // `failed` 表示已接收的图片未进入正文，不能用旧正文放行本轮导航。
    if (imagePaste && (await imagePaste.waitForPendingInputs()) === 'failed') return false
    if (ready.value && editor) {
      const source = editor.getValue()
      if (source !== renderedSource) applySource(source)
    }
    if (!(await autoSave.saveNow())) return false
    if (!ready.value || !editor) return true
    // 写盘期间可能又接收图片输入；先让其落定并保存，再决定是否放行。
    const trailing = imagePaste ? await imagePaste.waitForPendingInputs() : 'idle'
    if (trailing === 'failed') return false
    if (trailing === 'settled' || editor.getValue() !== renderedSource) continue
    return true
  }
}

/** 全局目录入口复用 Vditor 的标题解析、实时更新与点击定位。 */
function toggleToc(): void {
  if (!ready.value || !editor) return
  const state = editor.vditor
  if (state.currentMode === 'sv') {
    toast.addToast('info', t('doc.vditorOutlineSourceMode'))
    return
  }
  const show = state.outline.element.style.display !== 'block'
  if (show) emit('panel-open')
  state.options.outline!.enable = show
  state.outline.toggle(state, show, false)
  // 上游在 520px 以下直接隐藏大纲；本模块以覆盖式目录支持窄窗口。
  if (show && state.outline.element.style.display !== 'block') {
    state.outline.element.style.display = 'block'
    state.outline.render(state)
  }
}

function routeEditorPaste(event: ClipboardEvent): void {
  if (ready.value && imagePaste?.handle(event)) return
  routeCodePaste(event)
}

function routeImageDrop(event: DragEvent): void {
  if (ready.value) imagePaste?.handle(event)
}

function routeCodePaste(event: ClipboardEvent): void {
  if (!ready.value || !editor || editor.vditor.currentMode === 'sv' || !event.clipboardData?.getData('text/plain')) return
  const root = editor.vditor[editor.vditor.currentMode]?.element
  if (!root || root.getAttribute('contenteditable') !== 'true' || !(event.target instanceof Element) || !root.contains(event.target)) return
  const selection = root.ownerDocument.getSelection()
  if (!selection?.rangeCount) return
  const ancestor = selection.getRangeAt(0).commonAncestorContainer
  const code = (ancestor instanceof Element ? ancestor : ancestor.parentElement)?.closest('code')
  if (!code || !root.contains(code) || !code.parentElement?.matches('pre.vditor-ir__marker--pre, pre.vditor-wysiwyg__pre') || event.target.closest('code') === code) return
  // 原生事件指向编辑宿主，导致上游把代码当 Markdown；按完整选区重派到代码节点，复用原粘贴与撤销流程。
  event.preventDefault()
  event.stopImmediatePropagation()
  code.dispatchEvent(new ClipboardEvent('paste', { clipboardData: event.clipboardData, bubbles: true, cancelable: true }))
}

function normalizeTableSelection(event: KeyboardEvent): void {
  if (!ready.value || !editor || event.key !== 'Tab' || !event.defaultPrevented ||
    event.isComposing || event.ctrlKey || event.metaKey || event.altKey || editor.vditor.currentMode === 'sv') return
  const root = editor.vditor[editor.vditor.currentMode]?.element
  if (!root || !(event.target instanceof Node) || !root.contains(event.target)) return
  const selection = root.ownerDocument.getSelection()
  if (!selection?.rangeCount) return
  const range = selection.getRangeAt(0)
  const cell = range.startContainer
  if (!(cell instanceof HTMLElement) || !cell.matches('td,th') || !root.contains(cell) ||
    range.endContainer !== cell || range.startOffset !== 0 || range.endOffset !== cell.childNodes.length || !range.toString().trim()) return
  const walker = root.ownerDocument.createTreeWalker(cell, NodeFilter.SHOW_TEXT)
  let first: Node | null = null
  let last: Node | null = null
  while (walker.nextNode()) {
    if (!walker.currentNode.textContent) continue
    first ??= walker.currentNode
    last = walker.currentNode
  }
  // 上游跳格已结束；Chromium 把整格元素边界当成插入点，落到文字边界才能替换选中的旧值。
  if (first && last) selection.setBaseAndExtent(first, 0, last, last.textContent!.length)
}

function handleKeyDown(event: KeyboardEvent): void {
  if (ready.value && editor && handleVditorNestedSelection(editor, event, applySource)) return
  if (event.isComposing) return
  if (event.key === 'Tab' && !event.ctrlKey && !event.metaKey && !event.altKey && indentSourceSelection(event)) return
  if ((event.ctrlKey || event.metaKey) && event.altKey && !event.shiftKey && /^Digit[789]$/.test(event.code)) {
    captureReadingPosition()
    const mode = event.code === 'Digit7' ? 'wysiwyg' : event.code === 'Digit8' ? 'ir' : 'sv'
    pendingModeCaret = ready.value && editor && mode !== editor.vditor.currentMode ? captureVditorModeCaret(editor) : null
  }
  if (event.key === 'Escape' && closePanels()) {
    event.preventDefault()
    event.stopImmediatePropagation()
    return
  }
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return
  const key = event.key.toLowerCase()
  if (key !== 's' && key !== 'f' && key !== 'h') return
  event.preventDefault()
  event.stopImmediatePropagation()
  if (key === 's') void saveNow()
  else if (key === 'h') openReplace()
  else openSearch()
}

function restoreCachedDraft(): void {
  if (!ready.value || !editor || cachedDraft.value === null) return
  restoredCacheSource = cachedDraft.value
  editor.setValue(cachedDraft.value)
  cachedDraft.value = null
  applySource(editor.getValue())
}

watch(saveError, (error) => {
  if (error !== null) toast.addToast('error', t('doc.overwriteFailed', { error: ipcErrorMessage(error) }))
})
watch(saveState, (state) => {
  if (state !== 'saved' || restoredCacheSource === null) return
  try {
    // 恢复稿落盘后才清理；其它窗口后来写入的草稿仍保留。
    if (localStorage.getItem(cacheKey) === restoredCacheSource) localStorage.removeItem(cacheKey)
  } catch { /* 存储不可访问时保留缓存，不影响已经完成的文件保存。 */ }
  restoredCacheSource = null
})

onBeforeRouteLeave(() => saveNow())
onBeforeRouteUpdate(() => saveNow())

onMounted(() => {
  if (!host.value) return
  try {
    const cached = localStorage.getItem(cacheKey)
    if (cached !== null && cached !== props.value) cachedDraft.value = cached
  } catch { /* 禁用本地存储时仍可读取、编辑和保存文档。 */ }

  editor = new Vditor(host.value, {
    value: props.value,
    mode: 'ir',
    theme: editorAppearance.value.theme,
    preview: {
      theme: { current: editorAppearance.value.content, path: contentThemePath },
      hljs: { style: editorAppearance.value.code },
      markdown: { linkBase: getVditorResourceBaseUrl(props.documentUrl) },
    },
    link: {
      click: (element) => {
        const destination = element.getAttribute('href') ?? element.textContent ?? ''
        const url = resolveVditorLinkUrl(destination, props.documentUrl)
        if (!url) return
        const state = editor?.vditor
        const root = state?.[state.currentMode]?.element
        if (state && root && scrollToVditorHeading(url, props.documentUrl, root, state.currentMode)) return
        window.open(url)
      },
    },
    // 图标随本地模块加载，避开第三方同步 XHR + 内联脚本注入被 CSP 拦截。
    icon: undefined,
    height: '100%',
    cdn: '/vditor',
    // 当前没有图片上传或录音保存端点，避免展示必然失败的入口。
    toolbar: [
      'emoji', 'headings', 'bold', 'italic', 'strike', 'link', '|',
      'list', 'ordered-list', 'check', 'outdent', 'indent', '|',
      'quote', 'line', 'code', 'inline-code', 'insert-before', 'insert-after', '|',
      'table', '|', 'undo', 'redo', '|', 'fullscreen', 'edit-mode',
      { name: 'more', toolbar: ['both', 'code-theme', 'content-theme', 'export', 'outline', 'preview', 'devtools', 'info', 'help'] },
    ],
    outline: { enable: false, position: 'left' },
    cache: {
      id: cacheKey,
      // 旧草稿由用户明确恢复；第三方不得自行用缓存替换源文或覆盖尚未恢复的草稿。
      enable: false,
    },
    focus: () => {
      const state = editor?.vditor
      if (!state?.undo || state.currentMode === undoMode) return
      // 上游初始快照与输入共用防抖；首笔输入前立即建立该模式基线，避免取消计时器后原文无法撤销。
      state.undo.addToUndoStack(state)
      undoMode = state.currentMode
    },
    input: (source) => {
      if (ready.value && !disposed && source !== renderedSource) applySource(source)
    },
    // Vditor 初始化会异步加载本地语言脚本；组件可能在初始化完成前卸载。
    after: () => {
      if (disposed && editor?.vditor) {
        editor.destroy()
        editor = null
        return
      }
      if (editor) {
        if (editor.vditor.lute && host.value) {
          installVditorResourceSerialization(editor.vditor.lute, host.value.ownerDocument)
          installVditorCodeWhitespace(editor.vditor.lute)
        }
        // 渲染器会归一化 Markdown，以它的初始文本作比较，避免首次打开就触发文件改写。
        renderedSource = editor.getValue()
        imagePaste = createVditorImagePasteController(editor, {
          onChange: applySource,
          onRetry: () => toast.addToast('warning', t('doc.editorImagePasteRetry')),
        })
        ready.value = true
        initializeReadingPosition()
        const outline = editor.vditor.outline.element
        const heading = outline.firstElementChild
        if (heading) {
          heading.textContent = t('doc.toc')
          const closeButton = outline.ownerDocument.createElement('button')
          closeButton.type = 'button'
          closeButton.className = 'vditor-outline__close'
          closeButton.textContent = t('common.close')
          closeButton.onclick = toggleToc
          heading.appendChild(closeButton)
        }
      }
    },
  })
})

watch(() => props.value, (value) => {
  if (!ready.value || !editor || value === renderedSource) return
  editor.setValue(value)
  renderedSource = editor.getValue()
  sourceLength.value = value.length
  autoSave.resetSource(value)
})

onBeforeUnmount(() => {
  disposed = true
  imagePaste?.dispose()
  imagePaste = null
  ready.value = false
  if (positionFrame !== undefined) cancelAnimationFrame(positionFrame)
  positionObserver?.disconnect()
  positionResizeObserver?.disconnect()
  autoSave.dispose()
  closeSearch()
  if (editor?.vditor) {
    editor.destroy()
    editor = null
  }
})

defineExpose({ saveNow, toggleToc, toggleSearch, openSearch, openReplace, closePanels, getCurrentPosition })
</script>

<style scoped>
.vditor-document {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.vditor-document__host {
  flex: 1;
  min-height: 0;
  container-type: inline-size;
}

.vditor-document__draft,
.vditor-document__status {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  flex-wrap: wrap;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.vditor-document__status--error {
  color: var(--color-error);
}

.vditor-document__search {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  padding: var(--spacing-sm);
  border-bottom: 1px solid var(--color-border);
}

.vditor-document__search input {
  flex: 1;
  min-width: 100px;
  padding: 4px 8px;
  color: var(--color-text);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
}

.vditor-document__replace {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  width: 100%;
}

.vditor-document__replace-hint {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.vditor-document__search-count {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}

:global(::highlight(scrollery-vditor-search)) {
  background-color: #ffe083;
  color: #242424;
}

:global(::highlight(scrollery-vditor-search-current)) {
  background-color: #ff9a42;
  color: #242424;
}

:deep(.vditor) {
  height: 100%;
  border: 0;
  border-radius: 0;
}

:deep(.vditor-content) {
  position: relative;
}

:deep(.vditor-ir > pre.vditor-reset),
:deep(.vditor-wysiwyg > pre.vditor-reset) {
  font-size: var(--vditor-reader-font-size);
  line-height: var(--vditor-reader-line-height);
  font-family: var(--vditor-reader-font-family);
  width: 100%;
  box-sizing: border-box;
  /* 上游初始化、切模式与 resize 都会写内联 padding；只覆盖横向留白以实时限定正文宽度。 */
  padding-inline: max(16px, calc((100% - var(--vditor-reader-width)) / 2)) !important;
}

:deep(.vditor-sv) {
  font-family: var(--font-mono);
}

:deep(.vditor-outline__title) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}

:deep(.vditor-outline__close) {
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
  padding: 4px 6px;
  border-radius: var(--radius-sm);
}

@container (max-width: 640px) {
  :deep(.vditor-outline) {
    position: absolute;
    inset: 0 auto 0 0;
    width: min(280px, 85%);
    z-index: 2;
    box-shadow: 4px 0 12px rgb(0 0 0 / 15%);
  }

  :deep(.vditor-toolbar) {
    padding-left: 5px !important;
  }
}
</style>
