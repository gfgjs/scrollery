<!-- 已知卷面板（Part5 T13 §3.7 离线 UX）：列出应用登记的物理卷（在线态 + 媒体数）+ 重命名 / 忘记。 -->
<template>
  <CollapsibleCard id="knownVolumes" :title="$t('settings.volTitle')">
    <div class="kv-intro">{{ $t('settings.volIntro') }}</div>

    <div v-if="vol.loading.value && !vol.volumes.value.length" class="kv-empty">
      {{ $t('settings.volLoading') }}
    </div>
    <div v-else-if="!vol.volumes.value.length" class="kv-empty">{{ $t('settings.volEmpty') }}</div>

    <div v-else class="kv-list">
      <div v-for="v in vol.volumes.value" :key="v.id" class="kv-item">
        <component :is="kindIcon(v.kind)" :size="18" class="kv-item__icon" />

        <div class="kv-item__info">
          <!-- 名称：卷标 / 回退挂载点 / stableId；双击或点铅笔改名。 -->
          <div class="kv-item__name-row">
            <input
              v-if="editingId === v.id"
              ref="renameInput"
              v-model="editName"
              class="kv-item__input"
              :placeholder="$t('settings.volRenamePlaceholder')"
              maxlength="100"
              @keydown.enter="submitRename(v.id)"
              @keydown.esc="cancelRename"
              @blur="submitRename(v.id)"
            />
            <span v-else class="kv-item__name" @dblclick="startRename(v)">{{ displayName(v) }}</span>
            <span class="kv-badge" :class="v.isOnline ? 'kv-badge--on' : 'kv-badge--off'">
              {{ v.isOnline ? $t('settings.volOnline') : $t('settings.volOffline') }}
            </span>
          </div>
          <div class="kv-item__sub">
            <span class="kv-badge kv-badge--kind">{{ v.kind.toUpperCase() }}</span>
            <span v-if="v.lastMountPath" class="kv-item__mount">{{ v.lastMountPath }}</span>
            <span>· {{ $t('settings.volItems', { n: v.itemCount }) }}</span>
          </div>
        </div>

        <div class="kv-item__actions">
          <button
            class="kv-btn"
            :title="$t('settings.volRename')"

            @click="startRename(v)"
          >
            <Pencil :size="14" />
          </button>
          <button
            class="kv-btn kv-btn--danger"
            :title="$t('settings.volForget')"

            @click="onForget(v)"
          >
            <Trash2 :size="14" />
          </button>
        </div>
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { ref, nextTick, onMounted } from 'vue'
import { HardDrive, Server, FolderOpen, Pencil, Trash2 } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import CollapsibleCard from './CollapsibleCard.vue'
import { useKnownVolumes, type VolumeInfo } from '../../composables/useKnownVolumes'
import { useToastStore } from '../../stores/toastStore'
import { useConfirm } from '../../composables/useConfirm'
import type { IpcError } from '../../utils/ipc'

const { t } = useI18n()
const vol = useKnownVolumes()
const toast = useToastStore()
const { confirm } = useConfirm()

const editingId = ref<number | null>(null)
const editName = ref('')
const renameInput = ref<HTMLInputElement | null>(null)

onMounted(() => {
  void vol.load()
})

function kindIcon(kind: string) {
  if (kind === 'network') return Server
  if (kind === 'removable') return HardDrive
  return FolderOpen
}

// 显示名：卷标优先 → 挂载点 → stableId 兜底（永不空白）。
function displayName(v: VolumeInfo): string {
  return v.label || v.lastMountPath || v.stableId
}

function startRename(v: VolumeInfo) {
  editingId.value = v.id
  editName.value = v.label ?? ''
  void nextTick(() => renameInput.value?.focus())
}

function cancelRename() {
  editingId.value = null
  editName.value = ''
}

async function submitRename(id: number) {
  if (editingId.value !== id) return // blur 在 esc 取消后可能重入，守卫
  const name = editName.value.trim()
  editingId.value = null
  if (!name) return // 空名不提交（后端亦拒）
  try {
    await vol.rename(id, name)
    toast.addToast('success', t('settings.volRenamed'))
  } catch (e) {
    toast.addToast('error', t('settings.volOpFailedCode', { code: (e as IpcError)?.code ?? e }))
  }
}

async function onForget(v: VolumeInfo) {
  const { confirmed } = await confirm({
    title: t('settings.volForgetTitle'),
    message: t('settings.volForgetMsg', { name: displayName(v) }),
    confirmText: t('settings.volForget'),
  })
  if (!confirmed) return
  try {
    await vol.forget(v.id)
    toast.addToast('success', t('settings.volForgotten'))
  } catch (e) {
    toast.addToast('error', t('settings.volOpFailedCode', { code: (e as IpcError)?.code ?? e }))
  }
}
</script>

<style scoped>
.kv-intro {
  padding: 12px 16px;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  line-height: 1.6;
}
.kv-empty {
  padding: 14px 16px;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.kv-list {
  padding: 0 16px 8px;
}
.kv-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--color-border);
}
.kv-item:last-child {
  border-bottom: none;
}
.kv-item__icon {
  color: var(--color-text-secondary);
  flex: 0 0 auto;
}
.kv-item__info {
  flex: 1;
  min-width: 0;
}
.kv-item__name-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.kv-item__name {
  font-weight: 600;
  color: var(--color-text-primary);
  cursor: text;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.kv-item__input {
  font-weight: 600;
  color: var(--color-text-primary);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-sm);
  padding: 2px 6px;
  font-size: var(--font-size-sm);
  min-width: 0;
  flex: 1;
}
.kv-item__sub {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  margin-top: 2px;
}
.kv-item__mount {
  font-family: var(--font-mono);
}
.kv-badge {
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
  padding: 2px 6px;
  border-radius: 8px;
}
.kv-badge--on {
  background: color-mix(in srgb, var(--color-success) 20%, transparent);
  color: var(--color-success);
}
.kv-badge--off {
  background: var(--color-bg-hover);
  color: var(--color-text-tertiary);
}
.kv-badge--kind {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.kv-item__actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}
.kv-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text-secondary);
  padding: 6px;
  border-radius: var(--radius-md);
  cursor: pointer;
}
.kv-btn:hover {
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
}
.kv-btn--danger:hover {
  color: var(--color-error);
  background: var(--color-error-subtle);
}
</style>
