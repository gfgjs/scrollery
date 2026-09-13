<template>
  <!-- 网络存储（需求8 8B, §3.8）：管理 WebDAV / 远程存储后端。 -->
  <CollapsibleCard id="networkStorage" :title="$t('settings.nsTitle')">
    <div class="ns-intro" v-html="$t('settings.nsIntro')"></div>

    <!-- 已配置后端列表 -->
    <div v-if="backends.length" class="ns-list">
      <div v-for="b in backends" :key="b.id" class="ns-item">
        <component :is="kindIcon(b.kind)" :size="18" class="ns-item__icon" />
        <div class="ns-item__info">
          <div class="ns-item__name">{{ b.name }}</div>
          <div class="ns-item__sub">
            <span class="ns-badge">{{ b.kind.toUpperCase() }}</span>
            <span v-if="b.host" class="ns-item__host">{{ b.host }}</span>
            <span v-if="b.basePath">/{{ b.basePath }}</span>
            <span v-if="b.username" class="ns-item__user">· {{ b.username }}</span>
            <span v-if="b.hasPassword" class="ns-item__lock" :title="$t('settings.nsSavedPassword')"
              ><Lock :size="11"
            /></span>
          </div>
        </div>
        <button
          class="ns-btn ns-btn--danger"
          @click="remove(b.id)"
          :title="$t('settings.nsRemove')"

        >
          <Trash2 :size="15" />
        </button>
      </div>
    </div>
    <div v-else class="ns-empty">{{ $t('settings.nsEmpty') }}</div>

    <!-- 添加表单 -->
    <div class="ns-form">
      <div class="ns-row">
        <UiField :label="$t('settings.nsType')" class="ns-field--kind">
          <select v-model="form.kind">
            <option value="webdav">WebDAV</option>
            <option value="smb">{{ $t('settings.nsTypeSmb') }}</option>
            <option value="local">{{ $t('settings.nsTypeLocal') }}</option>
          </select>
        </UiField>
        <UiField :label="$t('settings.nsName')" class="ns-field--grow">
          <input v-model="form.name" type="text" :placeholder="$t('settings.nsNamePlaceholder')" />
        </UiField>
      </div>

      <UiField v-if="form.kind === 'webdav'" :label="$t('settings.nsAddress')">
        <input
          v-model="form.host"
          type="text"
          placeholder="https://dav.example.com/remote.php/dav/files/me"
        />
      </UiField>
      <UiField v-else :label="$t('settings.nsPath')">
        <input
          v-model="form.basePath"
          type="text"
          :placeholder="form.kind === 'smb' ? '\\\\NAS\\media' : 'D:\\Media'"
        />
      </UiField>

      <div v-if="form.kind === 'webdav'" class="ns-row">
        <UiField :label="$t('settings.nsSubPath')" class="ns-field--grow">
          <input v-model="form.basePath" type="text" placeholder="photos" />
        </UiField>
      </div>

      <div v-if="form.kind === 'webdav'" class="ns-row">
        <UiField :label="$t('settings.nsUsername')" class="ns-field--grow">
          <input v-model="form.username" type="text" autocomplete="off" />
        </UiField>
        <UiField :label="$t('settings.nsPassword')" class="ns-field--grow">
          <input v-model="form.password" type="password" autocomplete="new-password" />
        </UiField>
      </div>

      <div class="ns-actions">
        <button class="ns-btn" :disabled="busy" @click="test">
          <Plug :size="14" /> {{ $t('settings.nsTestConn') }}
        </button>
        <button class="ns-btn ns-btn--primary" :disabled="busy || !canSave" @click="save">
          <Plus :size="14" /> {{ $t('settings.nsAdd') }}
        </button>
        <span v-if="message" class="ns-msg" :class="'ns-msg--' + messageKind">{{ message }}</span>
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
// 网络存储后端管理（需求8 8B, §3.8）：列出 / 测试 / 添加 / 移除 WebDAV（及 SMB/本地）后端。
// 密码经后端存入系统 keyring，不落库（前端永不回显密码）。
import { ref, reactive, computed, onMounted } from 'vue'
import { invokeIpc } from '../../utils/ipc'
import { HardDrive, Server, FolderOpen, Trash2, Plug, Plus, Lock } from '@lucide/vue'
import { IPC } from '../../constants/ipc'
import CollapsibleCard from './CollapsibleCard.vue'
import UiField from '../ui/UiField.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

interface BackendInfo {
  id: number
  kind: string
  name: string
  host?: string | null
  basePath?: string | null
  username?: string | null
  hasPassword: boolean
  createdAt: number
}

const backends = ref<BackendInfo[]>([])
const busy = ref(false)
const message = ref('')
const messageKind = ref<'ok' | 'err'>('ok')

const form = reactive({
  kind: 'webdav',
  name: '',
  host: '',
  basePath: '',
  username: '',
  password: '',
})

const canSave = computed(() => {
  if (form.kind === 'webdav') return form.host.trim().length > 0
  return form.basePath.trim().length > 0
})

function kindIcon(kind: string) {
  if (kind === 'webdav') return Server
  if (kind === 'smb') return HardDrive
  return FolderOpen
}

function setMessage(text: string, kind: 'ok' | 'err') {
  message.value = text
  messageKind.value = kind
}

async function loadBackends() {
  try {
    backends.value = await invokeIpc<BackendInfo[]>(IPC.LIST_BACKENDS)
  } catch (e) {
    setMessage(t('settings.nsLoadFailed', { error: (e as Error)?.message ?? e }), 'err')
  }
}

function payload() {
  return {
    input: {
      kind: form.kind,
      name: form.name || null,
      host: form.host || null,
      basePath: form.basePath || null,
      username: form.username || null,
      password: form.password || null,
    },
  }
}

async function test() {
  busy.value = true
  setMessage(t('settings.nsTesting'), 'ok')
  try {
    const count = await invokeIpc<number>(IPC.TEST_BACKEND, payload())
    setMessage(t('settings.nsTestSuccess', { count }), 'ok')
  } catch (e) {
    setMessage(t('settings.nsTestFailed', { error: (e as Error)?.message ?? e }), 'err')
  } finally {
    busy.value = false
  }
}

async function save() {
  busy.value = true
  try {
    await invokeIpc<BackendInfo>(IPC.ADD_BACKEND, payload())
    setMessage(t('settings.nsAdded'), 'ok')
    form.name = ''
    form.host = ''
    form.basePath = ''
    form.username = ''
    form.password = ''
    await loadBackends()
  } catch (e) {
    setMessage(t('settings.nsAddFailed', { error: (e as Error)?.message ?? e }), 'err')
  } finally {
    busy.value = false
  }
}

async function remove(id: number) {
  try {
    await invokeIpc(IPC.REMOVE_BACKEND, { id })
    await loadBackends()
  } catch (e) {
    setMessage(t('settings.nsRemoveFailed', { error: (e as Error)?.message ?? e }), 'err')
  }
}

onMounted(loadBackends)
</script>

<style scoped>
/* 注:此处曾有一份自绘 `.settings-card` / `.settings-card__header`(CollapsibleCard 迁移前的遗留)。
   本节根节点已是 <CollapsibleCard>,卡片外观归全局 .settings-card(index.css)统一,故两块删除。
   删因不是「重复」而是**它盖掉了全局卡**:scoped CSS 的 data-v 会打在子组件根节点上,那条
   `.settings-card` 因而命中 CollapsibleCard 的根 div,让本卡在六套主题下都取 bg-surface 而非
   --card-bg(=bg-elevated)、圆角 md 而非 lg、外加 16px 下边距 —— 全设置页 9 张卡里唯独这张不同。
   (`.settings-card__header` 则相反:CollapsibleCard 内部 DOM 拿不到本组件 data-v,是死代码。) */
.ns-intro {
  padding: 12px 16px;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  line-height: 1.6;
}
.ns-intro code {
  font-family: var(--font-mono);
  background: var(--color-bg-elevated);
  padding: 1px 5px;
  border-radius: var(--radius-sm);
}
.ns-list {
  padding: 0 16px;
}
.ns-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--color-border);
}
.ns-item__icon {
  color: var(--color-text-secondary);
  flex: 0 0 auto;
}
.ns-item__info {
  flex: 1;
  min-width: 0;
}
.ns-item__name {
  font-weight: 600;
  color: var(--color-text-primary);
}
.ns-item__sub {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  margin-top: 2px;
}
.ns-item__host {
  font-family: var(--font-mono);
}
.ns-badge {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
}
.ns-item__lock {
  display: inline-flex;
  color: var(--color-text-secondary);
}
.ns-empty {
  padding: 14px 16px;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.ns-form {
  padding: 12px 16px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ns-row {
  display: flex;
  gap: 10px;
}
/* 字段 label 关联/朝向/caption 由 UiField 原语提供（原 .ns-field 基座 + > span 已删）；行内布局用的
   --grow/--kind 修饰类经 class passthrough 落到 UiField 根（仍命中：子根带父 data-v）。输入框视觉从
   `.ns-field 后代`重锚到不变的 `.ns-form 后代`（迁移后 .ns-field 类消失，原选择器会失配）。 */
.ns-field--grow {
  flex: 1;
}
.ns-field--kind {
  flex: 0 0 180px;
}
.ns-form input,
.ns-form select {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  padding: 6px 9px;
  font-size: var(--font-size-sm);
  width: 100%;
  box-sizing: border-box;
}
.ns-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 4px;
}
.ns-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text-primary);
  padding: 6px 12px;
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.ns-btn:hover:not(:disabled) {
  background: var(--color-bg-elevated);
}
.ns-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ns-btn--primary {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  border-color: transparent;
}
.ns-btn--danger {
  border-color: transparent;
  color: var(--color-error);
  padding: 6px;
}
.ns-btn--danger:hover {
  background: color-mix(in srgb, var(--color-error) 12%, transparent);
}
.ns-msg {
  font-size: var(--font-size-xs);
}
.ns-msg--ok {
  color: var(--color-accent);
}
.ns-msg--err {
  color: var(--color-error);
}
</style>
