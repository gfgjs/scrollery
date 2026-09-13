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
    <div v-if="phase === 'validating'" class="restore-busy">
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

      <p v-if="summary.needsMigration" class="restore-note">
        {{ bt('backup.restoreMigratedNote') }}
      </p>
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
      <template v-if="phase === 'summary'">
        <UiButton @click="closeWizard">{{ $t('common.cancel') }}</UiButton>
        <UiButton variant="primary" :disabled="!summaryConfirmed" @click="phase = 'confirm'">
          {{ bt('backup.restoreContinue') }}
        </UiButton>
      </template>
      <template v-else-if="phase === 'confirm'">
        <UiButton @click="phase = 'summary'">{{ $t('common.back') }}</UiButton>
        <UiButton
          variant="danger"
          :disabled="typedConfirmation !== bt('backup.restoreConfirmWord')"
          @click="armAndRestart"
        >
          {{ bt('backup.restoreAndRestart') }}
        </UiButton>
      </template>
      <template v-else-if="phase === 'error'">
        <UiButton v-if="!armed" @click="closeWizard">{{ $t('common.close') }}</UiButton>
        <UiButton v-if="armed" variant="primary" @click="retryRelaunch">
          {{ bt('backup.restoreRetryRestart') }}
        </UiButton>
        <UiButton v-else variant="primary" @click="validatePackage">
          {{ bt('backup.retryValidation') }}
        </UiButton>
      </template>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../../i18n/backupMessages'
import UiButton from '../ui/UiButton.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'
import UiDialog from '../ui/UiDialog.vue'
import { useBackupStore, type RestoreStageResult } from '../../stores/backupStore'
import { IpcError } from '../../utils/ipc'
import { restoreErrorKey } from '../../utils/backupPresentation'

const props = defineProps<{ open: boolean; packagePath: string }>()
const emit = defineEmits<{ close: [] }>()
const { locale } = useI18n()
const { t: bt } = useBackupI18n()
const backup = useBackupStore()

type RestorePhase = 'validating' | 'summary' | 'confirm' | 'arming' | 'error'
const phase = ref<RestorePhase>('validating')
const summary = ref<RestoreStageResult | null>(null)
const summaryConfirmed = ref(false)
const typedConfirmation = ref('')
const errorCode = ref<string>()
const armed = ref(false)
const errorKey = computed(() =>
  armed.value ? 'backup.restoreErrorRelaunch' : restoreErrorKey(errorCode.value),
)
const busy = computed(() => phase.value === 'validating' || phase.value === 'arming' || armed.value)
const packageName = computed(() => props.packagePath.split(/[\\/]/).pop() || props.packagePath)

watch(
  () => [props.open, props.packagePath] as const,
  ([open]) => {
    if (open) void validatePackage()
  },
)

function resetConfirmation() {
  summaryConfirmed.value = false
  typedConfirmation.value = ''
  errorCode.value = undefined
  armed.value = false
}

async function validatePackage() {
  if (!props.open || !props.packagePath) return
  resetConfirmation()
  summary.value = null
  phase.value = 'validating'
  try {
    summary.value = await backup.restoreStage(props.packagePath)
    phase.value = 'summary'
  } catch (error) {
    errorCode.value = error instanceof IpcError ? error.code : 'unknown'
    phase.value = 'error'
  }
}

async function armAndRestart() {
  if (!summary.value || typedConfirmation.value !== bt('backup.restoreConfirmWord')) return
  phase.value = 'arming'
  errorCode.value = undefined
  try {
    await backup.restoreArm(summary.value)
    armed.value = true
    await backup.relaunchApp()
  } catch (error) {
    errorCode.value = error instanceof IpcError ? error.code : 'unknown'
    phase.value = 'error'
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

function closeWizard() {
  if (busy.value) return
  emit('close')
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
.restore-danger,
.restore-note {
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
.restore-note {
  color: var(--color-text-secondary);
  background: var(--color-bg-elevated);
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
