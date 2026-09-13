<template>
  <div class="repl-panel">
    <div class="repl-panel__head">
      <span class="repl-panel__title">{{ t('doc.replace') }}</span>
      <button
        class="repl-panel__x"
        @click="emit('close')"
        :title="t('common.close')"

      >
        <X :size="16" />
      </button>
    </div>

    <div class="repl-panel__tabs">
      <button :class="{ active: scope === 'item' }" @click="setScope('item')">
        {{ t('doc.replScopeItem') }}
      </button>
      <button :class="{ active: scope === 'global' }" @click="setScope('global')">
        {{ t('toolbar.searchScopeGlobal') }}
      </button>
    </div>

    <div class="repl-panel__list">
      <div v-if="!rules.length" class="repl-panel__empty">{{ t('doc.replEmpty') }}</div>
      <div v-for="r in rules" :key="r.id" class="repl-row" :class="{ disabled: !r.enabled }">
        <input
          type="checkbox"
          :checked="r.enabled"
          :title="t('doc.replToggleEnabled')"
          @change="toggleEnabled(r, ($event.target as HTMLInputElement).checked)"
        />
        <input
          class="repl-row__find"
          v-model="r.find"
          :placeholder="t('doc.replFind')"
          @change="save(r)"
        />
        <span class="repl-row__arrow">→</span>
        <input
          class="repl-row__rep"
          v-model="r.replace"
          :placeholder="t('doc.replReplaceWith')"
          @change="save(r)"
        />
        <label class="repl-row__re" :title="t('doc.replRegex')">
          <input
            type="checkbox"
            :checked="r.isRegex"
            @change="toggleRegex(r, ($event.target as HTMLInputElement).checked)"
          />
          .*
        </label>
        <button
          class="repl-row__del"
          @click="remove(r)"
          :title="t('selection.delete')"

        >
          <Trash2 :size="14" />
        </button>
      </div>
    </div>

    <div class="repl-panel__add">
      <input v-model="draft.find" :placeholder="t('doc.replFind')" @keyup.enter="add" />
      <span class="repl-row__arrow">→</span>
      <input v-model="draft.replace" :placeholder="t('doc.replReplaceWith')" @keyup.enter="add" />
      <label class="repl-row__re" :title="t('doc.replRegex')"
        ><input type="checkbox" v-model="draft.isRegex" />.*</label
      >
      <button class="repl-panel__addbtn" :disabled="!draft.find" @click="add">
        <Plus :size="14" /> {{ t('settings.nsAdd') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
// 替换规则管理面板（§5.2）。两套作用域：本文档（item）与全局（global）。任何增删改后 emit
// 'changed'，由 DocumentViewer 重新拉取生效规则并重渲染。
import { ref, reactive, watch } from 'vue'
import { X, Trash2, Plus } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import type { ReplacementRule } from '../../utils/replacements'

const props = defineProps<{ itemId: number }>()
const emit = defineEmits<{ (e: 'changed'): void; (e: 'close'): void }>()

const { t } = useI18n()
const toast = useToastStore()

const scope = ref<'item' | 'global'>('item')
const rules = ref<ReplacementRule[]>([])
const draft = reactive({ find: '', replace: '', isRegex: false })

function scopeArgs() {
  return scope.value === 'item'
    ? { scopeKind: 'item', scopeId: props.itemId }
    : { scopeKind: 'global', scopeId: null as number | null }
}

async function reload() {
  rules.value = await invokeIpc<ReplacementRule[]>(IPC.LIST_REPLACEMENTS, scopeArgs()).catch(() => [])
}

function setScope(s: 'item' | 'global') {
  scope.value = s
  reload()
}

async function persist(
  rule: Partial<ReplacementRule> & { find: string; replace: string; isRegex: boolean },
) {
  const a = scopeArgs()
  await invokeIpc(IPC.UPSERT_REPLACEMENT, {
    rule: {
      id: rule.id ?? null,
      scopeKind: a.scopeKind,
      scopeId: a.scopeId,
      find: rule.find,
      replace: rule.replace,
      isRegex: rule.isRegex,
      enabled: rule.enabled ?? true,
      sortOrder: rule.sortOrder ?? 0,
    },
  })
  emit('changed')
}

async function save(r: ReplacementRule) {
  if (!r.find) return
  try {
    await persist(r)
  } catch (e) {
    toast.addToast('error', t('doc.replacementOpFailed', { error: ipcErrorMessage(e) }))
  }
}
async function toggleEnabled(r: ReplacementRule, v: boolean) {
  // P1-16:先改 UI 后 await,失败须回滚,否则开关显示已启用而 DB 未改(UI/DB 分叉)。
  const prev = r.enabled
  r.enabled = v
  try {
    await persist(r)
  } catch (e) {
    r.enabled = prev
    toast.addToast('error', t('doc.replacementOpFailed', { error: ipcErrorMessage(e) }))
  }
}
async function toggleRegex(r: ReplacementRule, v: boolean) {
  const prev = r.isRegex
  r.isRegex = v
  try {
    await persist(r)
  } catch (e) {
    r.isRegex = prev
    toast.addToast('error', t('doc.replacementOpFailed', { error: ipcErrorMessage(e) }))
  }
}
async function remove(r: ReplacementRule) {
  try {
    await invokeIpc(IPC.DELETE_REPLACEMENT, { id: r.id })
    await reload()
    emit('changed')
  } catch (e) {
    toast.addToast('error', t('doc.replacementOpFailed', { error: ipcErrorMessage(e) }))
  }
}
async function add() {
  if (!draft.find) return
  try {
    await persist({ find: draft.find, replace: draft.replace, isRegex: draft.isRegex, enabled: true })
    draft.find = ''
    draft.replace = ''
    draft.isRegex = false
    await reload()
  } catch (e) {
    toast.addToast('error', t('doc.replacementOpFailed', { error: ipcErrorMessage(e) }))
  }
}

watch(() => props.itemId, reload, { immediate: true })
</script>

<style scoped>
.repl-panel {
  display: flex;
  flex-direction: column;
  width: 340px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-divider);
}
.repl-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
}
.repl-panel__title {
  font-weight: 600;
}
.repl-panel__x {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.repl-panel__tabs {
  display: flex;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm) var(--spacing-md);
}
.repl-panel__tabs button {
  flex: 1;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  background: var(--color-bg-hover);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.repl-panel__tabs button.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  border-color: transparent;
}
.repl-panel__list {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xs) var(--spacing-md);
}
.repl-panel__empty {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  padding: var(--spacing-xl) 0;
  text-align: center;
}
.repl-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) 0;
}
.repl-row.disabled {
  opacity: 0.45;
}
.repl-row__find,
.repl-row__rep {
  flex: 1;
  min-width: 0;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  height: var(--control-size-compact);
  padding: 0 var(--spacing-xs);
  font-size: var(--font-size-xs);
}
.repl-row__arrow {
  color: var(--color-text-secondary);
}
.repl-row__re {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-family: var(--font-mono);
  font-size: var(--font-size-2xs);
  color: var(--color-text-secondary);
}
.repl-row__del {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.repl-row__del:hover {
  /* 原 var(--color-danger, #e5484d) 引用不存在的幽灵 token,一直走 fallback(S5 修) */
  color: var(--color-error);
}
.repl-panel__add {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm) var(--spacing-md);
  border-top: 1px solid var(--color-border);
  flex-wrap: wrap;
}
.repl-panel__add input[type='text'],
.repl-panel__add input:not([type]) {
  flex: 1;
  min-width: 0;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  height: var(--control-size-compact);
  padding: 0 var(--spacing-xs);
  font-size: var(--font-size-xs);
}
.repl-panel__addbtn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  border: none;
  border-radius: var(--radius-sm);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  cursor: pointer;
  font-size: var(--font-size-xs);
}
.repl-panel__addbtn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
