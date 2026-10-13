<template>
  <UiDialog :open="open" :title="$t('exotic.activateTitle')" :close-label="$t('common.close')" max-width="460px" @close="onCancel">
    <p class="dialog-message">{{ $t('exotic.activateDesc', { name: featureName }) }}</p>
    <p class="dialog-message">{{ $t('exotic.activateFreeHint') }}</p>
    <p v-if="errorCode" class="activate-error" role="alert">{{ $t('exotic.activateFailed', { code: errorCode }) }}</p>
    <template #footer>
      <UiButton variant="ghost" :disabled="activating" @click="onCancel">{{ $t('detail.close') }}</UiButton>
      <UiButton data-autofocus variant="primary" :disabled="activating || !featureId" @click="onSubmit">
        {{ activating ? $t('exotic.activating') : $t('exotic.activateSubmit') }}
      </UiButton>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import { activateFeature } from '../../utils/featureActivation'
import { useToastStore } from '../../stores/toastStore'
import type { IpcError } from '../../utils/ipc'

const props = defineProps<{ open: boolean; featureId: string; featureName: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'activated'): void }>()
const { t } = useI18n()
const toast = useToastStore()
const errorCode = ref<string | null>(null)
const activating = ref(false)
watch(() => props.open, (open) => { if (open) errorCode.value = null })
function onCancel() { if (!activating.value) emit('close') }
async function onSubmit() {
  if (activating.value || !props.featureId) return
  activating.value = true
  errorCode.value = null
  try {
    await activateFeature(props.featureId)
    toast.addToast('success', t('exotic.activateSuccess'))
    emit('activated')
    emit('close')
  } catch (error) {
    errorCode.value = (error as IpcError)?.code ?? 'unknown'
  } finally {
    activating.value = false
  }
}
</script>

<style scoped>
.dialog-message { margin: 0; font-size: var(--font-size-base); color: var(--color-text-secondary); line-height: 1.5; }
.activate-error { margin: 0; font-size: var(--font-size-sm); color: var(--color-error); }
</style>
