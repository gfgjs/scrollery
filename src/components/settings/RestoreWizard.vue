<template>
  <UiDialog
    :open="open"
    :title="bt('backup.restoreTitle')"
    :close-label="$t('common.close')"
    :show-close="!busy"
    :close-on-overlay="!busy"
    :close-on-esc="!busy"
    max-width="600px"
    max-height="84vh"
    @close="closeWizard"
  >
    <div v-if="closing" class="restore-busy">
      <span class="spinner" />
      <strong>{{ bt('backup.restoreDiscarding') }}</strong>
    </div>

    <div v-else-if="phase === 'validating'" class="restore-busy">
      <span class="spinner" />
      <div>
        <strong>{{ bt('backup.restoreValidating') }}</strong>
        <p>{{ packageName }}</p>
      </div>
    </div>

    <template v-else-if="phase === 'summary' && summary">
      <p class="restore-warning">
        {{ bt('backup.restorePrivacyWarning') }}
      </p>

      <dl class="restore-summary">
        <div>
          <dt>{{ bt('backup.createdAt') }}</dt>
          <dd>{{ formatCreatedAt(summary.createdAtUtc) }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.schemaVersion') }}</dt>
          <dd>{{ bt('backup.schemaValue', { version: summary.schemaVersion }) }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.itemCount') }}</dt>
          <dd>{{ summary.counts.items }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.albumCount') }}</dt>
          <dd>{{ summary.counts.albums }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.tagCount') }}</dt>
          <dd>{{ summary.counts.tags }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.personCount') }}</dt>
          <dd>{{ summary.counts.namedPersons }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.rootCount') }}</dt>
          <dd>{{ summary.roots.length }}</dd>
        </div>
        <div>
          <dt>{{ bt('backup.documentCount') }}</dt>
          <dd>{{ summary.appdataDocumentCount }}</dd>
        </div>
      </dl>

      <p v-if="summary.externalDocumentVersions > 0" class="restore-warning">
        {{
          bt('backup.restoreExternalWarning', {
            count: summary.externalDocumentVersions,
          })
        }}
      </p>
      <div v-if="summary.roots.length" class="restore-roots">
        <strong>{{ bt('backup.restoreRoots') }}</strong>
        <span v-for="root in summary.roots" :key="root.id" class="restore-root">
          {{ root.alias || bt('backup.unnamedRoot', { id: root.id }) }}
        </span>
      </div>

      <UiCheckbox v-model="summaryConfirmed" :label="bt('backup.restoreSummaryConfirm')" />
    </template>

    <template v-else-if="phase === 'confirm' && summary">
      <p class="restore-danger">
        {{ bt('backup.restoreFinalWarning') }}
      </p>
      <label class="restore-type-confirm">
        <span>{{ $t('common.typeToConfirm', { word: bt('backup.restoreConfirmWord') }) }}</span>
        <input
          v-model="typedConfirmation"
          type="text"
          autocomplete="off"
          autocorrect="off"
          spellcheck="false"
          :placeholder="bt('backup.restoreConfirmWord')"
          data-autofocus
        />
      </label>
    </template>

    <div v-else-if="phase === 'arming'" class="restore-busy">
      <span class="spinner" />
      <div>
        <strong>
          {{ bt(armed ? 'backup.restoreRelaunching' : 'backup.restorePreparingRestart') }}
        </strong>
        <p>
          {{ bt(armed ? 'backup.restoreArmedNote' : 'backup.restoreRollbackNote') }}
        </p>
      </div>
    </div>

    <div v-else-if="phase === 'error'" class="restore-error">
      <strong>{{ bt('backup.restoreFailed') }}</strong>
      <p>{{ bt(errorKey) }}</p>
      <code v-if="errorCode">{{ errorCode }}</code>
    </div>

    <template #footer>
      <template v-if="!closing && phase === 'summary'">
        <UiButton :disabled="busy" @click="closeWizard">{{ $t('common.cancel') }}</UiButton>
        <UiButton variant="primary" :disabled="busy || !summaryConfirmed" @click="phase = 'confirm'">
          {{ bt('backup.restoreContinue') }}
        </UiButton>
      </template>
      <template v-else-if="!closing && phase === 'confirm'">
        <UiButton :disabled="busy" @click="phase = 'summary'">{{ $t('common.back') }}</UiButton>
        <UiButton
          variant="danger"
          :disabled="busy || typedConfirmation !== bt('backup.restoreConfirmWord')"
          @click="armAndRestart"
        >
          {{ bt('backup.restoreAndRestart') }}
        </UiButton>
      </template>
      <template v-else-if="!closing && phase === 'error'">
        <UiButton v-if="!armed" :disabled="busy" @click="closeWizard">{{ $t('common.close') }}</UiButton>
        <UiButton v-if="armed" variant="primary" @click="retryRelaunch">
          {{ bt('backup.restoreRetryRestart') }}
        </UiButton>
        <UiButton v-else variant="primary" :disabled="busy" @click="validatePackage">
          {{ bt('backup.retryValidation') }}
        </UiButton>
      </template>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../../i18n/backupMessages'
import UiButton from '../ui/UiButton.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'
import UiDialog from '../ui/UiDialog.vue'
import { useBackupStore, type RestoreStageResult } from '../../stores/backupStore'
import { useToastStore } from '../../stores/toastStore'
import { IpcError } from '../../utils/ipc'
import { restoreErrorKey } from '../../utils/backupPresentation'

const props = defineProps<{ open: boolean; packagePath: string }>()
const emit = defineEmits<{ close: [] }>()
const { locale } = useI18n()
const { t: bt } = useBackupI18n()
const backup = useBackupStore()
const toast = useToastStore()

type RestorePhase = 'validating' | 'summary' | 'confirm' | 'arming' | 'error'
const phase = ref<RestorePhase>('validating')
const summary = ref<RestoreStageResult | null>(null)
const summaryConfirmed = ref(false)
const typedConfirmation = ref('')
const errorCode = ref<string>()
const armed = ref(false)
const closing = ref(false)
// summary 属于当前界面；暂存所有权独立保留，切包和关闭不能把待清理结果遗忘。
let ownedStage: RestoreStageResult | null = null
let stageWork = Promise.resolve()
let generation = 0
let disposed = false
let armInFlight = false
const errorKey = computed(() =>
  armed.value ? 'backup.restoreErrorRelaunch' : restoreErrorKey(errorCode.value),
)
const busy = computed(() => closing.value || phase.value === 'validating' || phase.value === 'arming' || armed.value)
const packageName = computed(() => props.packagePath.split(/[\\/]/).pop() || props.packagePath)

watch(
  () => [props.open, props.packagePath] as const,
  ([open]) => {
    if (open) void validatePackage()
    else void releaseSession()
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  disposed = true
  void releaseSession()
})

function isCurrent(request: number): boolean {
  return !disposed && props.open && request === generation
}

function reportError(error: unknown) {
  const code = error instanceof IpcError ? error.code : 'unknown'
  if (!disposed && props.open) {
    errorCode.value = code
    phase.value = 'error'
  } else {
    toast.addToast('error', bt(armed.value ? 'backup.restoreErrorRelaunch' : restoreErrorKey(code)))
  }
}

async function discardOwnedStage(): Promise<boolean> {
  if (!ownedStage) return true
  // arm 在途时后端可能已经写入 pending；成功后所有权完全交给启动恢复流程。
  if (armInFlight || armed.value) return false
  try {
    await backup.restoreDiscard(ownedStage)
    ownedStage = null
    return true
  } catch (error) {
    // 保留暂存所有权并停掉已排队的验证，等用户明确重试，不自动循环争抢文件任务闸门。
    generation++
    reportError(error)
    return false
  }
}

function releaseSession(): Promise<void> {
  const request = ++generation
  summary.value = null
  stageWork = stageWork.then(async () => {
    if (request === generation) await discardOwnedStage()
  })
  return stageWork
}

function resetConfirmation() {
  summaryConfirmed.value = false
  typedConfirmation.value = ''
  errorCode.value = undefined
}

function validatePackage(): Promise<void> {
  if (disposed || !props.open || !props.packagePath || armInFlight || armed.value) return Promise.resolve()
  const request = ++generation
  const packagePath = props.packagePath
  resetConfirmation()
  summary.value = null
  phase.value = 'validating'
  // 同一向导只运行一条预检/释放链；旧成功结果由下一次切包或关闭工作负责清理。
  stageWork = stageWork.then(async () => {
    if (!isCurrent(request) || !await discardOwnedStage() || !isCurrent(request)) return
    try {
      ownedStage = await backup.restoreStage(packagePath)
      if (!isCurrent(request)) return
      summary.value = ownedStage
      phase.value = 'summary'
    } catch (error) {
      if (isCurrent(request)) reportError(error)
    }
  })
  return stageWork
}

async function armAndRestart() {
  if (busy.value || disposed || !props.open || !summary.value || !ownedStage || typedConfirmation.value !== bt('backup.restoreConfirmWord')) return
  armInFlight = true
  phase.value = 'arming'
  errorCode.value = undefined
  try {
    await backup.restoreArm(ownedStage)
    ownedStage = null
    armed.value = true
    await backup.relaunchApp()
  } catch (error) {
    reportError(error)
  } finally {
    armInFlight = false
    if (!armed.value && (disposed || !props.open)) void releaseSession()
  }
}

async function retryRelaunch() {
  if (!armed.value) return
  phase.value = 'arming'
  errorCode.value = undefined
  try {
    await backup.relaunchApp()
  } catch (error) {
    errorCode.value = error instanceof IpcError ? error.code : 'unknown'
    phase.value = 'error'
  }
}

function formatCreatedAt(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale.value)
}

async function closeWizard() {
  if (busy.value) return
  closing.value = true
  const request = ++generation
  try {
    stageWork = stageWork.then(async () => {
      if (request !== generation || !await discardOwnedStage() || !isCurrent(request)) return
      summary.value = null
      emit('close')
    })
    await stageWork
  } finally {
    closing.value = false
  }
}
</script>

<style scoped>
.restore-busy {
  min-height: 150px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-md);
  color: var(--color-text-primary);
}
.restore-busy .spinner {
  width: 22px;
  height: 22px;
}
.restore-busy p,
.restore-error p {
  margin: 5px 0 0;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.restore-warning,
.restore-danger {
  margin: 0;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  line-height: 1.55;
}
.restore-warning {
  color: var(--color-warning);
  background: color-mix(in srgb, var(--color-warning) 10%, transparent);
}
.restore-danger {
  color: var(--color-error);
  background: color-mix(in srgb, var(--color-error) 10%, transparent);
}
.restore-summary {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1px;
  margin: 0;
  overflow: hidden;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-border);
}
.restore-summary > div {
  display: flex;
  justify-content: space-between;
  gap: var(--spacing-sm);
  padding: 9px 11px;
  background: var(--color-bg-surface);
}
.restore-summary dt {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}
.restore-summary dd {
  margin: 0;
  color: var(--color-text-primary);
  font-weight: 600;
  font-size: var(--font-size-sm);
}
.restore-roots {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.restore-root {
  padding: 3px 7px;
  border-radius: 999px;
  background: var(--color-bg-elevated);
}
.restore-type-confirm {
  display: flex;
  flex-direction: column;
  gap: 7px;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.restore-type-confirm input {
  padding: 9px 11px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font: inherit;
}
.restore-type-confirm input:focus {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 18%, transparent);
}
.restore-error {
  padding: var(--spacing-md);
  border: 1px solid color-mix(in srgb, var(--color-error) 35%, var(--color-border));
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-error) 8%, var(--color-bg-surface));
  color: var(--color-error);
}
.restore-error code {
  display: inline-block;
  margin-top: 9px;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}
@media (max-width: 620px) {
  .restore-summary {
    grid-template-columns: 1fr;
  }
}
</style>
