<template>
  <div class="pr-panel">
    <div class="pr-panel__head">
      <span class="pr-panel__title">{{ t('doc.proofreadTitle') }}</span>
      <button
        class="pr-panel__x"
        @click="emit('close')"
        :title="t('common.close')"

      >
        <X :size="16" />
      </button>
    </div>

    <!-- 配置（未配置或点击设置时展开）-->
    <div v-if="showConfig" class="pr-config">
      <UiField :label="t('doc.proofreadBaseUrl')">
        <input v-model="form.baseUrl" placeholder="https://api.openai.com/v1" />
      </UiField>
      <UiField :label="t('doc.proofreadModel')">
        <input v-model="form.model" placeholder="gpt-4o-mini" />
      </UiField>
      <UiField :label="cfg?.hasKey ? `API Key ${t('doc.proofreadKeySet')}` : 'API Key'">
        <input v-model="form.key" type="password" placeholder="sk-..." />
      </UiField>
      <div class="pr-config__row">
        <button class="pr-btn pr-btn--primary" @click="saveConfig">{{ t('doc.save') }}</button>
        <button v-if="cfg?.hasKey" class="pr-btn" @click="clearKey">
          {{ t('doc.proofreadClearKey') }}
        </button>
        <button v-if="configured" class="pr-btn" @click="showConfig = false">
          {{ t('onboarding.finish') }}
        </button>
      </div>
      <p class="pr-hint">{{ t('doc.proofreadKeyHint') }}</p>
    </div>

    <!-- 操作区 -->
    <div v-else class="pr-body">
      <div class="pr-actions">
        <button class="pr-btn" @click="showConfig = true" :title="t('settings.title')">
          <Settings :size="14" /> {{ t('settings.title') }}
        </button>
        <button
          class="pr-btn pr-btn--primary"
          :disabled="!configured || running || !text"
          @click="run"
        >
          <Sparkles :size="14" />
          {{
            running
              ? t('doc.proofreadRunning', { done: progress.done, total: progress.total })
              : t('doc.proofreadStart')
          }}
        </button>
      </div>

      <p v-if="error" class="pr-error">{{ error }}</p>
      <p v-if="!configured" class="pr-hint">{{ t('doc.proofreadNotConfigured') }}</p>

      <!-- track-changes 预览 -->
      <div v-if="diff" class="pr-diff">
        <div class="pr-diff__bar">
          <span>{{ t('doc.proofreadDiffTitle') }}</span>
        </div>
        <div class="pr-diff__body">
          <div v-for="(op, i) in diff" :key="i" class="pr-diff__line" :class="'op-' + op.tag">
            <span class="pr-diff__sign">{{
              op.tag === 'insert' ? '+' : op.tag === 'delete' ? '−' : ' '
            }}</span>
            <span class="pr-diff__text">{{ op.value }}</span>
          </div>
        </div>
        <div class="pr-diff__foot">
          <button class="pr-btn pr-btn--primary" @click="accept">
            {{ t('doc.proofreadAccept') }}
          </button>
          <button class="pr-btn" @click="discard">{{ t('doc.proofreadDiscard') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// AI 校对面板（§5.4，远程）。配置（base_url/model/key）→ 分块逐块校对 → track-changes 预览 →
// 接受后存为新版本（source='ai-remote'，接 §5.3）。本期为「全部接受」；逐条接受/拒绝为后续增强。
import { ref, reactive, computed, onMounted } from 'vue'
import { X, Settings, Sparkles } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import UiField from '../ui/UiField.vue'

interface DiffOp {
  tag: string
  value: string
}
interface ProofreadCfg {
  baseUrl: string
  model: string
  hasKey: boolean
}

const props = defineProps<{ itemId: number; text: string; currentVersionId: number | null }>()
const emit = defineEmits<{ (e: 'changed'): void; (e: 'close'): void }>()

const { t } = useI18n()
const toast = useToastStore()

const cfg = ref<ProofreadCfg | null>(null)
const form = reactive({ baseUrl: '', model: '', key: '' })
const showConfig = ref(false)
const running = ref(false)
const progress = reactive({ done: 0, total: 0 })
const corrected = ref<string | null>(null)
const diff = ref<DiffOp[] | null>(null)
const error = ref('')

const configured = computed(() => !!cfg.value?.baseUrl && !!cfg.value?.model && !!cfg.value?.hasKey)

// 单块上限（字符）。按行边界切，保证拼接可还原原文结构。
const CHUNK_MAX = 2000
function chunkText(text: string): string[] {
  if (text.length <= CHUNK_MAX) return text ? [text] : []
  const lines = text.split(/(?<=\n)/) // 保留换行，每段末尾带 \n
  const chunks: string[] = []
  let buf = ''
  for (const ln of lines) {
    if (buf && buf.length + ln.length > CHUNK_MAX) {
      chunks.push(buf)
      buf = ''
    }
    buf += ln
  }
  if (buf) chunks.push(buf)
  return chunks
}

async function loadCfg() {
  cfg.value = await invokeIpc<ProofreadCfg>(IPC.GET_PROOFREAD_CONFIG).catch(() => null)
  if (cfg.value) {
    form.baseUrl = cfg.value.baseUrl
    form.model = cfg.value.model
  }
  if (!configured.value) showConfig.value = true
}

async function saveConfig() {
  // P1-16:此前无 try/catch,配置写失败静默,用户以为已保存。
  try {
    await invokeIpc(IPC.SET_PROOFREAD_CONFIG, {
      baseUrl: form.baseUrl.trim(),
      model: form.model.trim(),
    })
    if (form.key) {
      await invokeIpc(IPC.SET_PROOFREAD_KEY, { key: form.key })
      form.key = ''
    }
    await loadCfg()
    if (configured.value) showConfig.value = false
  } catch (e) {
    toast.addToast('error', t('doc.saveConfigFailed', { error: ipcErrorMessage(e) }))
  }
}

async function clearKey() {
  try {
    await invokeIpc(IPC.CLEAR_PROOFREAD_KEY)
    await loadCfg()
  } catch (e) {
    toast.addToast('error', t('doc.clearKeyFailed', { error: ipcErrorMessage(e) }))
  }
}

async function run() {
  error.value = ''
  diff.value = null
  corrected.value = null
  const chunks = chunkText(props.text)
  if (!chunks.length) return
  running.value = true
  progress.done = 0
  progress.total = chunks.length
  try {
    const out: string[] = []
    for (const c of chunks) {
      out.push(await invokeIpc<string>(IPC.PROOFREAD_CHUNK, { text: c }))
      progress.done++
    }
    corrected.value = out.join('')
    diff.value = await invokeIpc<DiffOp[]>(IPC.DIFF_TEXTS, { a: props.text, b: corrected.value })
  } catch (e) {
    error.value = t('doc.proofreadFailed', { error: (e as Error)?.message ?? e })
  } finally {
    running.value = false
  }
}

async function accept() {
  if (corrected.value == null) return
  // P1-16:接受 AI 校对稿并存版本,失败须告警(否则用户以为已采纳,实则未落库)。
  try {
    const newId = await invokeIpc<number>(IPC.SAVE_VERSION, {
      itemId: props.itemId,
      content: corrected.value,
      label: t('doc.proofreadVersionLabel'),
      parentId: props.currentVersionId,
      target: 'version',
      source: 'ai-remote',
    })
    await invokeIpc(IPC.SET_CURRENT_VERSION, { itemId: props.itemId, versionId: newId })
    diff.value = null
    corrected.value = null
    emit('changed')
  } catch (e) {
    toast.addToast('error', t('doc.saveVersionFailed', { error: ipcErrorMessage(e) }))
  }
}

function discard() {
  diff.value = null
  corrected.value = null
}

onMounted(loadCfg)
</script>

<style scoped>
.pr-panel {
  display: flex;
  flex-direction: column;
  width: 360px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-divider);
}
.pr-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
}
.pr-panel__title {
  font-weight: 600;
}
.pr-panel__x {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}

.pr-config {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
}
/* 字段 label 关联/朝向由 UiField 原语提供(原 .pr-config label 已删);label 字号统一到原语的
   font-size-sm(原 xs,微增)。此处仅保留输入框视觉。 */
.pr-config input {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  font-size: var(--font-size-sm);
}
.pr-config__row {
  display: flex;
  gap: 8px;
}
.pr-hint {
  font-size: var(--font-size-2xs);
  color: var(--color-text-secondary);
  margin: 0;
  padding: 0 var(--spacing-md);
  line-height: 1.5;
}

.pr-body {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}
.pr-actions {
  display: flex;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
}
.pr-error {
  /* 原 var(--color-danger, #e5484d) 引用不存在的幽灵 token,一直走 fallback(S5 修) */
  color: var(--color-error);
  font-size: var(--font-size-xs);
  padding: 0 12px;
}

.pr-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  font-size: var(--font-size-xs);
  cursor: pointer;
}
.pr-btn--primary {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  border-color: transparent;
}
.pr-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.pr-diff {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  border-top: 1px solid var(--color-border);
}
.pr-diff__bar {
  padding: var(--spacing-xs) var(--spacing-md);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.pr-diff__body {
  flex: 1;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: var(--font-size-2xs);
}
.pr-diff__line {
  display: flex;
  gap: var(--spacing-xs);
  padding: 0 var(--spacing-sm);
  white-space: pre-wrap;
  word-break: break-word;
}
.pr-diff__sign {
  flex: 0 0 12px;
  color: var(--color-text-secondary);
}
.op-insert {
  background: var(--color-success-subtle);
}
.op-delete {
  background: var(--color-error-subtle);
}
.op-insert .pr-diff__sign {
  color: var(--color-success);
}
.op-delete .pr-diff__sign {
  color: var(--color-error);
}
.pr-diff__foot {
  display: flex;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  border-top: 1px solid var(--color-border);
}
</style>
