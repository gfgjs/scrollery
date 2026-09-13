<template>
  <!-- 人脸批量审批面板（Part5 T10, §3.6.2）：消费 list_likely_face_matches 的 likely-match 分组，
       用户对整组/选中脸一次性 确认 / 拒绝 / 建新人物 / 移出。默认全选 → 「确认」一键接受整组建议。
       外壳迁 UiDialog(此前无 Teleport/无焦点陷阱,原位流内块)。自定义头部(标题+副标题+关闭键)走 #header;
       分组列表为可滚正文,故 max-height='84vh' 封顶 + 正文滚动。范式② v-if 挂载,open 恒 true。 -->
  <UiDialog
    :open="true"
    :title="t('faces.approvalTitle')"
    max-width="680px"
    max-height="84vh"
    :show-close="false"
    @close="$emit('close')"
  >
    <template #header>
      <header class="approval-header">
        <div>
          <h2 id="face-approval-title" class="approval-title">{{ t('faces.approvalTitle') }}</h2>
          <p class="approval-subtitle">{{ t('faces.approvalSubtitle') }}</p>
        </div>
        <button
          class="btn-close"
          :title="t('common.close')"

          @click="$emit('close')"
        >
          <X :size="18" />
        </button>
      </header>
    </template>

    <div v-if="loading" class="approval-state">{{ t('common.loading') }}</div>
    <div v-else-if="error" class="approval-state approval-state--error">{{ error }}</div>
    <div v-else-if="groups.length === 0" class="approval-state">
      <PartyPopper :size="28" />
      <span>{{ t('faces.approvalEmpty') }}</span>
    </div>

    <div v-else class="group-list">
      <section v-for="g in groups" :key="g.personId" class="group-card">
        <div class="group-head">
          <div class="group-id">
            <span class="group-name" :class="{ 'is-unnamed': !g.personName }">
              {{ g.personName || t('persons.unnamedPerson') }}
            </span>
            <span class="group-meta">
              {{
                t('faces.groupMeta', {
                  id: g.personId,
                  pct: confidencePct(g),
                  count: g.candidateFaces.length,
                })
              }}
            </span>
          </div>
          <button class="link-btn" @click="toggleAll(g)">
            {{ allSelected(g) ? t('common.deselectAll') : t('common.selectAll') }}
          </button>
        </div>

        <div class="face-grid">
          <button
            v-for="f in g.candidateFaces"
            :key="f.faceId"
            type="button"
            class="face-cell"
            :class="{ selected: selected.has(f.faceId) }"
            :title="t('faces.similarityTitle', { pct: Math.round(f.similarity * 100) })"

            @click="toggleFace(f.faceId)"
          >
            <FaceAvatar
              :thumb-path="f.thumbPath"
              :thumb-status="f.thumbStatus"
              :bbox="f.bbox"
              :cache-dir="store.cacheDir"
              :size="64"
            />
            <Check v-if="selected.has(f.faceId)" :size="14" class="face-check" />
          </button>
        </div>

        <div class="group-actions">
          <button
            class="btn btn-primary"
            :disabled="busy || countIn(g) === 0"
            @click="act('confirm', g)"
          >
            <Check :size="14" /> {{ t('faces.confirmCount', { count: countIn(g) }) }}
          </button>
          <button
            class="btn btn-secondary"
            :disabled="busy || countIn(g) === 0"
            @click="act('reject', g)"
          >
            <X :size="14" /> {{ t('faces.notThisPerson') }}
          </button>
          <button
            class="btn btn-secondary"
            :disabled="busy || countIn(g) === 0"
            @click="openReassign(g)"
          >
            <ArrowRightLeft :size="14" /> {{ t('faces.reassignTo') }}
          </button>
          <button
            class="btn btn-secondary"
            :disabled="busy || countIn(g) === 0"
            @click="act('create', g)"
          >
            <UserPlus :size="14" /> {{ t('faces.createPerson') }}
          </button>
          <button
            class="btn btn-secondary action-danger"
            :disabled="busy || countIn(g) === 0"
            @click="act('unassign', g)"
          >
            <Ban :size="14" /> {{ t('faces.unassign') }}
          </button>
        </div>

        <!-- 改派选择器（内联展开）：把选中脸改归另一个现有人物。搜索过滤,排除本组候选自身。 -->
        <div v-if="reassignFor === g.personId" class="reassign-picker">
          <input
            v-model="reassignQuery"
            class="reassign-search"
            :placeholder="t('faces.reassignPlaceholder')"
          />
          <div class="reassign-list">
            <button
              v-for="p in reassignCandidates"
              :key="p.id"
              type="button"
              class="reassign-item"
              :disabled="busy"
              @click="doReassign(g, p.id)"
            >
              <span :class="{ 'is-unnamed': !p.isNamed }">
                {{ p.name || t('persons.unnamedPerson') }}
              </span>
              <span class="reassign-count">
                {{ t('persons.faceCount', { count: p.faceCount }) }}
              </span>
            </button>
            <div v-if="reassignCandidates.length === 0" class="reassign-empty">
              {{ t('faces.noMatch') }}
            </div>
          </div>
        </div>
      </section>
    </div>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { X, Check, UserPlus, Ban, PartyPopper, ArrowRightLeft } from '@lucide/vue'
import UiDialog from '../ui/UiDialog.vue'
import FaceAvatar from '../common/FaceAvatar.vue'
import { usePersonStore } from '../../stores/personStore'
import { useToastStore } from '../../stores/toastStore'
import { ipcErrorMessage } from '../../utils/ipc'
import type { LikelyMatchGroup } from '../../types/person'

defineEmits<{ (e: 'close'): void }>()

const { t } = useI18n()
const store = usePersonStore()
const toast = useToastStore()

const loading = ref(true)
const error = ref('')
const busy = ref(false)
// 选中的 faceId 集合（faceId 全局唯一，跨组不冲突）。默认加载后全选。
const selected = ref<Set<number>>(new Set())

// store.likelyMatches 驱动；动作成功后 store.dropResolvedFaces 重排数组 → 此 computed 自动刷新。
const groups = computed<LikelyMatchGroup[]>(() => store.likelyMatches)

onMounted(async () => {
  try {
    // 限 50 组：一次审批会话足够，避免超大库的全量分组查询拖慢首屏。
    await store.loadLikelyMatches(undefined, 50)
    const all = new Set<number>()
    for (const g of store.likelyMatches) for (const f of g.candidateFaces) all.add(f.faceId)
    selected.value = all
  } catch (e) {
    // 文案统一走 ipcErrorMessage(2026-07-10 审查 U7):String(e) 带 "IpcError: " 前缀。
    error.value = t('faces.loadFailed', { error: ipcErrorMessage(e) })
  } finally {
    loading.value = false
  }
})

function confidencePct(g: LikelyMatchGroup): number {
  return Math.round(g.confidence * 100)
}
function selectedIdsIn(g: LikelyMatchGroup): number[] {
  return g.candidateFaces.filter((f) => selected.value.has(f.faceId)).map((f) => f.faceId)
}
function countIn(g: LikelyMatchGroup): number {
  return selectedIdsIn(g).length
}
function allSelected(g: LikelyMatchGroup): boolean {
  return g.candidateFaces.length > 0 && g.candidateFaces.every((f) => selected.value.has(f.faceId))
}

function toggleFace(id: number) {
  const s = new Set(selected.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selected.value = s
}
function toggleAll(g: LikelyMatchGroup) {
  const on = !allSelected(g)
  const s = new Set(selected.value)
  for (const f of g.candidateFaces) {
    if (on) s.add(f.faceId)
    else s.delete(f.faceId)
  }
  selected.value = s
}

// ── 改派到现有人物（内联选择器）────────────────────────────────────────────
// 哪个组的选择器展开（按 personId），及其搜索词。persons 由 PersonsView 进页 store.load() 备好。
const reassignFor = ref<number | null>(null)
const reassignQuery = ref('')

function openReassign(g: LikelyMatchGroup) {
  // 同组再点收起；切组则换开并清空搜索。
  reassignFor.value = reassignFor.value === g.personId ? null : g.personId
  reassignQuery.value = ''
}

// 候选人物列表：排除本组候选自身（改派到自己无意义），按名字模糊过滤,限 30 条防长列表。
const reassignCandidates = computed(() => {
  const q = reassignQuery.value.trim().toLowerCase()
  return store.persons
    .filter((p) => p.id !== reassignFor.value)
    .filter((p) => !q || (p.name ?? '').toLowerCase().includes(q))
    .slice(0, 30)
})

async function doReassign(g: LikelyMatchGroup, targetId: number) {
  const ids = selectedIdsIn(g)
  if (ids.length === 0 || busy.value) return
  busy.value = true
  try {
    await store.reassignFaces(ids, targetId)
    toast.addToast('success', t('faces.reassignedToast', { count: ids.length }))
    const s = new Set(selected.value)
    for (const id of ids) s.delete(id)
    selected.value = s
    reassignFor.value = null
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  } finally {
    busy.value = false
  }
}

type Action = 'confirm' | 'reject' | 'create' | 'unassign'

/** 对某组的选中脸执行审批动作。store 动作传播后端 reject（含中文错误，如跨模型）→ 此处 toast。
 *  成功后 store 已乐观移除这些脸（组随之收缩/消失），再清理本地选中集。 */
async function act(action: Action, g: LikelyMatchGroup) {
  const ids = selectedIdsIn(g)
  if (ids.length === 0 || busy.value) return
  busy.value = true
  try {
    if (action === 'confirm') {
      await store.confirmFaces(ids)
      toast.addToast('success', t('faces.confirmedToast', { count: ids.length }))
    } else if (action === 'reject') {
      await store.rejectFaces(ids, g.personId)
      toast.addToast('success', t('faces.rejectedToast', { count: ids.length }))
    } else if (action === 'create') {
      await store.createPerson(ids)
      toast.addToast('success', t('faces.createdToast', { count: ids.length }))
    } else {
      await store.unassignFaces(ids)
      toast.addToast('success', t('faces.unassignedToast', { count: ids.length }))
    }
    const s = new Set(selected.value)
    for (const id of ids) s.delete(id)
    selected.value = s
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
/* 外壳(overlay/content/动画)由 UiDialog + 全局 Modal 基座提供;宽度 680px 走 max-width prop、
   封顶高度 84vh 走 max-height prop(触发 --capped:正文可滚、头固定)。本组件保留头部/分组列表特化。 */
.approval-header {
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-divider);
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--spacing-md);
  flex-shrink: 0; /* --capped 布局下头部不随正文收缩,保持固定;正文(UiDialog .dialog-body)独占滚动 */
}
.approval-title {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}
.approval-subtitle {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
.btn-close {
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  border-radius: var(--radius-full);
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  flex-shrink: 0;
  /* 显式列举过渡属性(勿 transition:all,S1 机械批) */
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast),
    box-shadow var(--transition-fast);
}
.btn-close:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.btn-close:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.approval-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-lg);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.approval-state--error {
  color: var(--color-error);
}

.group-list {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}
.group-card {
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-lg);
  padding: var(--spacing-md);
  background: var(--color-bg-surface);
}
.group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--spacing-md);
}
.group-id {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-2xs);
}
.group-name {
  font-size: var(--font-size-base);
  font-weight: 600;
  color: var(--color-text-primary);
}
.group-name.is-unnamed {
  color: var(--color-text-tertiary);
  font-style: italic;
}
.group-meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
.link-btn {
  background: transparent;
  border: none;
  color: var(--color-accent);
  cursor: pointer;
  font-size: var(--font-size-sm);
  flex-shrink: 0;
}

.face-grid {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-sm);
  margin-bottom: var(--spacing-md);
}
.face-cell {
  position: relative;
  padding: 0;
  border: 2px solid transparent;
  border-radius: 50%;
  background: transparent;
  cursor: pointer;
  line-height: 0;
  transition: border-color var(--transition-fast);
  /* 未选中的脸半透明，凸显已选（默认全选）。 */
  opacity: 0.45;
}
.face-cell.selected {
  border-color: var(--color-accent);
  opacity: 1;
}
.face-cell:hover {
  opacity: 1;
}
.face-check {
  position: absolute;
  right: 0;
  bottom: 0;
  color: var(--color-text-on-accent);
  background: var(--color-accent);
  border-radius: 50%;
  padding: var(--spacing-2xs);
}

.group-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-sm);
}
.btn-primary,
.btn-secondary {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
/* 移出 = 软危险色，与「确认/不是此人」区分但不抢眼。 */
.action-danger {
  color: var(--color-error);
}
.action-danger:hover:not(:disabled) {
  background: var(--color-error-subtle);
}

/* 改派选择器：内联展开于动作行下方。 */
.reassign-picker {
  margin-top: var(--spacing-md);
  padding: var(--spacing-sm);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-lg);
  background: var(--color-bg-inset);
}
.reassign-search {
  width: 100%;
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  outline: none;
  margin-bottom: var(--spacing-sm);
}
.reassign-search:focus {
  border-color: var(--color-accent);
}
.reassign-list {
  display: flex;
  flex-direction: column;
  max-height: 180px;
  overflow-y: auto;
}
.reassign-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-md);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  text-align: left;
}
.reassign-item:hover:not(:disabled) {
  background: var(--color-bg-hover);
}
.reassign-item .is-unnamed {
  color: var(--color-text-tertiary);
  font-style: italic;
}
.reassign-count {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.reassign-empty {
  padding: var(--spacing-sm);
  text-align: center;
  font-size: var(--font-size-sm);
  color: var(--color-text-tertiary);
}
</style>
