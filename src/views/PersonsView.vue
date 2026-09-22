<template>
  <!-- 人物墙（F6）：聚类出的人物簇卡片。点卡片进入该人物的照片；可命名/合并/隐藏。 -->
  <div ref="scrollEl" class="persons-view">
    <div ref="headerEl" class="persons-header">
      <div class="persons-header__text">
        <h2 class="persons-title">{{ t('sidebar.persons') }}</h2>
        <p class="persons-subtitle">{{ t('persons.subtitle') }}</p>
      </div>
      <div class="persons-header__actions">
        <button
          v-if="hiddenCount > 0"
          type="button"
          class="btn btn-secondary persons-hidden-toggle"
          :class="{ active: showHidden }"

          @click="toggleHidden"
        >
          <Eye :size="14" />
          {{
            showHidden
              ? t('persons.showVisible')
              : t('persons.showHidden', { count: hiddenCount })
          }}
        </button>
        <!-- 误检桶管理入口（ignored 历史管理）：有已忽略人物时出现，切换到误检桶查看/恢复。 -->
        <button
          v-if="ignoredCount > 0"
          type="button"
          class="btn btn-secondary persons-ignored-toggle"
          :class="{ active: showIgnored }"

          @click="toggleIgnored"
        >
          <Ban :size="14" />
          {{
            showIgnored
              ? t('persons.showVisible')
              : t('persons.showIgnored', { count: ignoredCount })
          }}
        </button>
        <!-- 审批入口（T10）：有 likely-match 分组待审批时出现，打开批量审批面板。 -->
        <button
          v-if="store.likelyMatches.length > 0 && !showIgnored"
          class="btn btn-primary persons-approve"
          :title="t('persons.approveTitle')"
          @click="showApproval = true"
        >
          <UserCheck :size="14" />
          {{ t('persons.approveSuggestions', { count: store.likelyMatches.length }) }}
        </button>
        <button
          v-if="visiblePersons.length > 0 && !showIgnored"
          class="btn btn-secondary persons-recluster"
          :disabled="reclustering"
          :title="t('persons.reclusterTitle')"
          @click="onRecluster"
        >
          <RefreshCw :size="14" :class="{ spin: reclustering }" />
          {{ reclustering ? t('persons.reclustering') : t('persons.recluster') }}
        </button>
      </div>
    </div>

    <!-- 合并操作条：选中 ≥2 时出现 -->
    <div v-if="selectedIds.size >= 2" ref="mergeBarEl" class="merge-bar">
      <span>{{ t('persons.selectedCount', { count: selectedIds.size }) }}</span>
      <button class="btn btn-primary" @click="mergeSelected">{{ t('persons.mergeIntoOne') }}</button>
      <button class="btn btn-secondary" @click="clearSelection">{{ t('common.cancel') }}</button>
    </div>

    <div v-if="!store.isLoading && displayPersons.length === 0" class="persons-empty">
      {{ t('persons.empty') }}
    </div>

    <div
      ref="gridEl"
      class="persons-grid"
      :style="{ height: `${virtualizer.getTotalSize()}px` }"
    >
      <div
        v-for="row in virtualRows"
        :key="row.key as number"
        :ref="measureRow"
        :data-index="row.index"
        class="persons-row"
        :style="{
          gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
          transform: `translateY(${row.start - scrollMargin}px)`,
        }"
      >
      <article
        v-for="p in displayPersons.slice(row.index * columns, (row.index + 1) * columns)"
        :key="p.id"
        :data-person-id="p.id"
        class="person-card"
        :class="{ 'person-card--selected': selectedIds.has(p.id) }"
        @focusin="focusedId = p.id"
        @focusout="onCardFocusOut"
      >
        <button
          type="button"
          class="person-card__primary"

          @click="onCardClick(p)"
          @dblclick.stop="startEdit(p)"
        />
        <!-- 墙/隐藏模式:选择合并 + 隐藏/恢复 + 移入误检桶(误检桶管理模式下隐藏这三者) -->
        <!-- 选择复选框（hover 或已选时显示） -->
        <button
          v-if="!showIgnored"
          type="button"
          class="person-card__check"
          :class="{ 'person-card__check--on': selectedIds.has(p.id) }"
          :title="t('persons.selectToMerge')"
          @click.stop="toggleSelect(p.id)"
        >
          <Check v-if="selectedIds.has(p.id)" :size="13" />
        </button>

        <!-- 隐藏/恢复按钮 -->
        <button
          v-if="!showIgnored"
          type="button"
          class="person-card__hide"
          :title="p.isHidden ? t('persons.restorePerson') : t('persons.hidePerson')"

          @click.stop="p.isHidden ? restorePerson(p) : hidePerson(p)"
        >
          <Eye v-if="p.isHidden" :size="14" />
          <EyeOff v-else :size="14" />
        </button>

        <!-- 误检桶按钮（审查 G1）：非人脸误检 → is_ignored,不上墙且重建按锚定保护 -->
        <button
          v-if="!showIgnored"
          type="button"
          class="person-card__ignore"
          :title="t('persons.markIgnored')"

          @click.stop="onMarkIgnored(p)"
        >
          <Ban :size="13" />
        </button>

        <!-- 误检桶管理模式:移出误检桶(恢复上墙)按钮，仅「显示已忽略」时出现 -->
        <button
          v-if="showIgnored"
          type="button"
          class="person-card__restore"
          :title="t('persons.restoreFromIgnored')"

          @click.stop="restoreIgnored(p)"
        >
          <RotateCcw :size="14" />
        </button>

        <!-- 封面脸：整图缩略图 cover + 脸中心定位（v1 近似，未做精确 bbox 放大裁剪） -->
        <div class="person-card__avatar" :style="avatarStyle(p)">
          <ScanFace v-if="!coverSrc(p)" :size="32" class="person-card__avatar-fallback" />
        </div>

        <!-- 名字：双击编辑 -->
        <div class="person-card__name">
          <input
            v-if="editingId === p.id"
            ref="nameInput"
            v-model="editName"
            :readonly="submitted"
            class="person-card__input"
            :placeholder="t('persons.namePlaceholder')"
            maxlength="40"
            @keydown.enter="submitEdit(p)"
            @keydown.esc="cancelEdit"
            @blur="submitEdit(p)"
            @click.stop
          />
          <span v-else :class="{ 'person-card__name--unnamed': !p.isNamed }">
            {{ p.name || t('persons.unnamed') }}
          </span>
        </div>
        <div class="person-card__count">{{ t('persons.faceCount', { count: p.faceCount }) }}</div>
      </article>
      </div>
    </div>

    <!-- 批量审批面板（T10）：模态覆盖层，按需打开。 -->
    <FaceApprovalPanel v-if="showApproval" @close="onApprovalClose" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onBeforeUnmount, watch } from 'vue'
import { defaultRangeExtractor, useVirtualizer, type Range } from '@tanstack/vue-virtual'
import { useRouter } from 'vue-router'
import { ScanFace, Eye, EyeOff, Check, RefreshCw, UserCheck, Ban, RotateCcw } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import { usePersonStore } from '../stores/personStore'
import { useViewStore } from '../stores/viewStore'
import { useToastStore } from '../stores/toastStore'
import { useHistoryStore } from '../stores/historyStore'
import { useConfirm } from '../composables/useConfirm'
import { ipcErrorMessage } from '../utils/ipc'
import { buildThumbUrl } from '../composables/useThumbLoader'
import FaceApprovalPanel from '../components/media/FaceApprovalPanel.vue'
import type { PersonSummary } from '../types/person'

const { t } = useI18n()
const store = usePersonStore()
const viewStore = useViewStore()
const toast = useToastStore()
const history = useHistoryStore()
const router = useRouter()
const { confirm } = useConfirm()

const showHidden = ref(false)
const showIgnored = ref(false)
const hiddenCount = computed(() => store.persons.filter((p) => p.isHidden).length)
const ignoredCount = computed(() => store.ignoredPersons.length)
const visiblePersons = computed(() =>
  store.persons.filter((p) => (showHidden.value ? p.isHidden : !p.isHidden)),
)
// 三态展示:误检桶(独立列表,来源不同于墙)> 隐藏 > 普通墙。
const displayPersons = computed(() =>
  showIgnored.value ? store.ignoredPersons : visiblePersons.value,
)

const editingId = ref<number | null>(null)
const editName = ref('')
const nameInput = ref<HTMLInputElement | HTMLInputElement[] | null>(null)
const submitted = ref(false)
let editGeneration = 0

const scrollEl = ref<HTMLElement | null>(null)
const headerEl = ref<HTMLElement | null>(null)
const mergeBarEl = ref<HTMLElement | null>(null)
const gridEl = ref<HTMLElement | null>(null)
const columns = ref(1)
const rowGap = ref(0)
const scrollMargin = ref(0)
const focusedId = ref<number | null>(null)
const pinnedRows = computed(() => {
  // 离屏编辑保留草稿；焦点前后各保留一行，让 Tab 自然跨越窗口边界。
  const rows = new Set<number>()
  for (const id of [editingId.value, focusedId.value]) {
    const index = id === null ? -1 : displayPersons.value.findIndex((p) => p.id === id)
    if (index < 0) continue
    const row = Math.floor(index / columns.value)
    for (const adjacent of [row - 1, row, row + 1]) {
      if (adjacent >= 0 && adjacent < Math.ceil(displayPersons.value.length / columns.value)) {
        rows.add(adjacent)
      }
    }
  }
  return [...rows]
})
const virtualizer = useVirtualizer(computed(() => {
  const pinned = pinnedRows.value
  return {
    count: Math.ceil(displayPersons.value.length / columns.value),
    getScrollElement: () => scrollEl.value,
    estimateSize: () => 280,
    overscan: 2,
    gap: rowGap.value,
    scrollMargin: scrollMargin.value,
    rangeExtractor: (range: Range) =>
      [...new Set([...defaultRangeExtractor(range), ...pinned])].sort((a, b) => a - b),
  }
}))
const virtualRows = computed(() => virtualizer.value.getVirtualItems())

function measureRow(el: Element | { $el?: Element } | null) {
  const node = el instanceof Element ? el : el?.$el
  if (node) virtualizer.value.measureElement(node)
}

function updateGridGeometry() {
  const grid = gridEl.value
  const scroll = scrollEl.value
  if (!grid || !scroll) return
  const gap = parseFloat(getComputedStyle(grid).columnGap) || 0
  columns.value = Math.max(1, Math.floor((grid.clientWidth + gap) / (140 + gap)))
  rowGap.value = gap
  scrollMargin.value = grid.getBoundingClientRect().top - scroll.getBoundingClientRect().top + scroll.scrollTop
}

function onCardFocusOut(event: FocusEvent) {
  const target = event.relatedTarget
  if (!(target instanceof Element) || !target.closest('.person-card')) focusedId.value = null
}

let gridObserver: ResizeObserver | undefined
onMounted(() => {
  gridObserver = new ResizeObserver(updateGridGeometry)
  for (const el of [scrollEl.value, headerEl.value, gridEl.value]) {
    if (el) gridObserver.observe(el)
  }
  updateGridGeometry()
})
watch(mergeBarEl, (el, previous) => {
  if (previous) gridObserver?.unobserve(previous)
  if (el) gridObserver?.observe(el)
  updateGridGeometry()
}, { flush: 'post' })
watch(columns, async () => {
  const input = Array.isArray(nameInput.value) ? nameInput.value[0] : nameInput.value
  const hadFocus = input === document.activeElement
  virtualizer.value.measure()
  await nextTick()
  // 列数改变可能让编辑卡片迁到另一行；恢复原焦点，不强制滚回离屏编辑项。
  if (hadFocus) {
    const current = Array.isArray(nameInput.value) ? nameInput.value[0] : nameInput.value
    current?.focus({ preventScroll: true })
  }
})
onBeforeUnmount(() => gridObserver?.disconnect())

// 两个管理切换互斥(同时只看一种):开一个即关另一个,并清合并选区(避免对不可见人物合并)。
function toggleHidden() {
  showHidden.value = !showHidden.value
  if (showHidden.value) {
    showIgnored.value = false
    clearSelection()
  }
}
function toggleIgnored() {
  showIgnored.value = !showIgnored.value
  if (showIgnored.value) {
    showHidden.value = false
    clearSelection()
  }
}

store.load()
store.loadIgnored() // 误检桶计数/列表(供「已忽略 (N)」入口 + 管理视图);失败非致命(store 内吞)。
// 进页拉一次 likely-match 分组（限 50）以显示「审批建议 (N)」入口；失败非致命。
store.loadLikelyMatches(undefined, 50).catch(() => {})

// ── 批量审批面板（T10）─────────────────────────────────────────────────────────
const showApproval = ref(false)

function onApprovalClose() {
  showApproval.value = false
  // 审批改了脸归属/计数 → 重载人物墙；likelyMatches 已在 store 内乐观更新（入口计数随之刷新）。
  store.load()
}

// ── 重新聚类（全量复核）─────────────────────────────────────────────────────────
const reclustering = ref(false)

// 误检桶（审查 G1）：非人脸误检置位后不上墙。除即时 undo toast 外,另经「已忽略 (N)」管理视图
// 可查看/恢复(本次补上,缺口 §2.6 闭环)。仍过确认闸防误点。
async function onMarkIgnored(p: PersonSummary) {
  const { confirmed } = await confirm({
    title: t('persons.markIgnored'),
    message: t('persons.markIgnoredMsg'),
    confirmText: t('persons.markIgnoredConfirm'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  try {
    await store.setIgnored(p.id, true)
    // 撤销无时限：登记进 historyStore（会话内长存 + Ctrl+Z 可达），toast 只是即时入口（round10 #7）。
    const undoId = history.pushUndoable({
      undo: () => store.setIgnored(p.id, false),
      redo: () => store.setIgnored(p.id, true),
      undoMessage: t('persons.restoredIgnoredDone'),
      redoMessage: t('persons.markIgnoredDone'),
    })
    toast.addToast('success', t('persons.markIgnoredDone'), 5000, [
      { label: t('common.undo'), onClick: () => history.undoIfTop(undoId) },
    ])
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  }
}

async function onRecluster() {
  const { confirmed } = await confirm({
    title: t('persons.recluster'),
    message: t('persons.reclusterConfirmMsg'),
    confirmText: t('persons.recluster'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  reclustering.value = true
  try {
    await store.recluster()
    toast.addToast('success', t('persons.reclusterDone'))
  } catch (e) {
    // 后端在分析运行中会拒绝 → 提示先暂停。文案走 ipcErrorMessage(2026-07-10 审查 U7)。
    toast.addToast('error', ipcErrorMessage(e))
  } finally {
    reclustering.value = false
  }
}

// ── 封面脸缩略图 URL(status=1 相对缓存 / 3 绝对源)。构造逻辑收敛到 buildThumbUrl
// 单源(useThumbLoader),防内联版漂移。─────────────────────────────────────────
function coverSrc(p: PersonSummary): string | null {
  if (p.coverThumbStatus === 1 && !store.cacheDir) return null
  try {
    return buildThumbUrl(p.coverThumbStatus ?? 0, p.coverThumbPath, store.cacheDir)
  } catch {
    return null
  }
}

// 整图 cover + 把脸中心对齐到容器中心区域（background-position 百分比）。bbox=[x,y,w,h] 归一化。
function avatarStyle(p: PersonSummary): Record<string, string> {
  const src = coverSrc(p)
  if (!src) return {}
  const bb = p.coverBbox
  const cx = bb ? (bb[0] + bb[2] / 2) * 100 : 50
  const cy = bb ? (bb[1] + bb[3] / 2) * 100 : 50
  return {
    backgroundImage: `url("${src}")`,
    backgroundPosition: `${cx}% ${cy}%`,
  }
}

// ── 进入该人物的照片 ─────────────────────────────────────────────────────────
function onCardClick(p: PersonSummary) {
  if (editingId.value === p.id) return
  viewStore.setActivePerson(p.id)
  router.push(`/persons/${p.id}`)
}

async function hidePerson(p: PersonSummary) {
  try {
    await store.setHidden(p.id, true)
    // 撤销无时限：登记进 historyStore（会话内长存 + Ctrl+Z 可达），toast 只是即时入口（round10 #7）。
    const undoId = history.pushUndoable({
      undo: () => store.setHidden(p.id, false),
      redo: () => store.setHidden(p.id, true),
      undoMessage: t('persons.restoredDone'),
      redoMessage: t('persons.hiddenDone'),
    })
    toast.addToast('success', t('persons.hiddenDone'), 5000, [
      { label: t('common.undo'), onClick: () => history.undoIfTop(undoId) },
    ])
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  }
}

async function restorePerson(p: PersonSummary) {
  try {
    await store.setHidden(p.id, false)
    if (hiddenCount.value === 0) showHidden.value = false
    toast.addToast('success', t('persons.restoredDone'))
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  }
}

// 移出误检桶(ignored 历史管理的恢复通路,镜像 restorePerson):清 is_ignored,人物重新上墙。
async function restoreIgnored(p: PersonSummary) {
  try {
    await store.setIgnored(p.id, false)
    if (ignoredCount.value === 0) showIgnored.value = false
    toast.addToast('success', t('persons.restoredIgnoredDone'))
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  }
}

// ── inline 命名 ───────────────────────────────────────────────────────────────

function startEdit(p: PersonSummary) {
  editGeneration++
  editingId.value = p.id
  editName.value = p.name ?? ''
  submitted.value = false
  nextTick(() => {
    const el = Array.isArray(nameInput.value) ? nameInput.value[0] : nameInput.value
    el?.focus()
    el?.select()
  })
}

async function submitEdit(p: PersonSummary) {
  if (submitted.value || editingId.value !== p.id) return
  submitted.value = true
  const generation = editGeneration
  const name = editName.value
  try {
    await store.rename(p.id, name)
    // 等待写入时可以切换编辑对象；旧请求不能关闭新的输入框。
    if (generation === editGeneration) editingId.value = null
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  } finally {
    if (generation === editGeneration) submitted.value = false
  }
}

function cancelEdit() {
  editGeneration++
  submitted.value = false
  editingId.value = null
}

// ── 多选合并 ─────────────────────────────────────────────────────────────────
const selectedIds = ref<Set<number>>(new Set())

function toggleSelect(id: number) {
  const s = new Set(selectedIds.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selectedIds.value = s
}

function clearSelection() {
  selectedIds.value = new Set()
}

async function mergeSelected() {
  const ids = [...selectedIds.value]
  if (ids.length < 2) return
  // 合并目标 = 选中里脸数最多的人物（保留它的身份/名字）；其余并入它。
  const chosen = ids
    .map((id) => store.persons.find((p) => p.id === id)!)
    .filter(Boolean)
    .sort((a, b) => b.faceCount - a.faceCount)
  const dst = chosen[0]
  const srcIds = chosen.slice(1).map((p) => p.id)
  const dstLabel = dst.name || t('persons.unnamedWithCount', { count: dst.faceCount })
  const { confirmed } = await confirm({
    title: t('persons.mergeTitle'),
    message: t('persons.mergeConfirmMsg', { count: ids.length, name: dstLabel }),
    confirmText: t('persons.merge'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  try {
    await store.merge(srcIds, dst.id)
    clearSelection()
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  }
}
</script>

<style scoped>
.persons-view {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xl);
}
.persons-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--spacing-sm);
  margin-bottom: var(--spacing-lg);
}
.persons-header__actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex-shrink: 0;
}
.persons-approve,
.persons-recluster,
.persons-hidden-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  flex-shrink: 0;
  white-space: nowrap;
}
.persons-recluster:disabled {
  opacity: 0.6;
  cursor: default;
}
.persons-recluster .spin {
  animation: persons-spin var(--duration-spin) linear infinite;
}
@keyframes persons-spin {
  to {
    transform: rotate(360deg);
  }
}
.persons-title {
  font-size: var(--font-size-xl);
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0 0 var(--spacing-xs);
}
.persons-subtitle {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0;
}
.merge-bar {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-sm) var(--spacing-md);
  margin-bottom: var(--spacing-md);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}
.persons-empty {
  padding: var(--spacing-lg);
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
}
.persons-grid {
  position: relative;
  column-gap: var(--spacing-md);
}
.persons-row {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  display: grid;
  gap: var(--spacing-md);
}
.person-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm);
  border-radius: var(--radius-lg);
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    border-color var(--transition-fast),
    transform var(--transition-fast);
}
.person-card:hover {
  background: var(--color-bg-hover);
  border-color: var(--color-border-strong);
  transform: none;
}
.persons-hidden-toggle.active {
  border-color: var(--color-accent);
  color: var(--color-accent);
  background: var(--color-accent-subtle);
}
.person-card:focus-within {
  border-color: var(--color-accent);
  box-shadow: var(--control-focus-ring);
}
.person-card__primary {
  position: absolute;
  inset: 0;
  z-index: 0;
  border: 0;
  border-radius: inherit;
  background: transparent;
  cursor: pointer;
}
.person-card__primary:focus-visible {
  outline: none;
}
.person-card > :not(.person-card__primary) {
  position: relative;
  z-index: 1;
  pointer-events: none;
}
.person-card--selected {
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}
.person-card__avatar {
  width: 96px;
  height: 96px;
  border-radius: 50%;
  background-color: var(--color-bg-primary);
  background-size: cover;
  background-repeat: no-repeat;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.person-card__avatar-fallback {
  color: var(--color-text-tertiary);
}
.person-card__name {
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--color-text-primary);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.person-card__name--unnamed {
  color: var(--color-text-tertiary);
  font-style: italic;
}
.person-card__count {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
.person-card__input {
  width: 100%;
  height: var(--control-size-default);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-input-border-focus);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-align: center;
  outline: none;
}
/* 复选框 + 隐藏/误检按钮：默认隐藏，hover 卡片时显现 */
.person-card__check,
.person-card__hide,
.person-card__ignore,
.person-card__restore {
  position: absolute;
  top: var(--spacing-sm);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  border: 0;
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  background: var(--color-bg-primary);
  opacity: 0;
  transition:
    opacity var(--transition-fast),
    color var(--transition-fast),
    background var(--transition-fast);
}
.person-card__check {
  left: var(--spacing-sm);
  border: 1px solid var(--color-border);
}
.person-card__hide {
  right: var(--spacing-sm);
}
.person-card__ignore {
  right: calc(var(--control-size-compact) + var(--spacing-sm));
}
/* 误检桶管理模式下唯一动作按钮:占右上角(此时隐藏/误检按钮不渲染)。 */
.person-card__restore {
  right: var(--spacing-sm);
}
.person-card:hover .person-card__check,
.person-card:hover .person-card__hide,
.person-card:hover .person-card__ignore,
.person-card:hover .person-card__restore,
.person-card:focus-within .person-card__check,
.person-card:focus-within .person-card__hide,
.person-card:focus-within .person-card__ignore,
.person-card:focus-within .person-card__restore,
.person-card__check--on {
  opacity: 1;
}
.person-card .person-card__check,
.person-card .person-card__hide,
.person-card .person-card__ignore,
.person-card .person-card__restore,
.person-card .person-card__input {
  z-index: 2;
  pointer-events: auto;
}
.person-card__check--on {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  border-color: var(--color-accent);
}
.person-card__hide:hover,
.person-card__ignore:hover {
  color: var(--color-error);
}
.person-card__restore:hover {
  color: var(--color-accent);
}
</style>
