<template>
  <section class="official-panel" aria-live="polite">
    <header class="official-panel__header">
      <div><h3>{{ $t('official.title') }}</h3><p>{{ $t('official.description') }}</p></div>
      <button class="btn btn-ghost btn-sm" :disabled="loading" @click="refresh">{{ $t('official.retry') }}</button>
    </header>
    <p v-if="failed" role="alert">{{ $t('official.queryFailed') }}</p>
    <p v-else-if="!summary">{{ $t('exotic.gateChecking') }}</p>
    <div v-else class="official-panel__features">
      <article v-for="feature in summary.features" :key="feature.id" class="official-panel__feature" :data-feature-id="feature.id">
        <div class="official-panel__name">
          <strong>{{ featureName(feature) }}</strong>
          <span>{{ $t(feature.activationRequired ? (feature.activated ? 'official.activated' : 'official.notActivated') : 'official.free') }}</span>
        </div>
        <p>{{ $t(resourceLabel(feature)) }}</p>
        <button v-if="feature.activationRequired && !feature.activated" class="btn btn-primary btn-sm" @click="activationTarget = feature">
          {{ $t('exotic.activateAction') }}
        </button>
        <RouterLink v-if="feature.resources === 'modelMissing' && supported(feature)" to="/settings">{{ $t('official.prepareModels') }}</RouterLink>
        <p v-if="feature.id === 'video-extended'" class="official-panel__notice">
          {{ $t('store.videoExtendedFfmpegNotice') }}
          <a href="https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-07-24-13-32" target="_blank" rel="noopener noreferrer">{{ $t('store.videoExtendedSourceLink') }}</a>
        </p>
      </article>
    </div>
    <FeatureActivationDialog v-if="activationTarget" :open="true" :feature-id="activationTarget.id" :feature-name="featureName(activationTarget)" @close="activationTarget = null" @activated="refresh" />
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { IPC, EVENTS } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'
import { useTauriListen } from '../../composables/useTauriListen'
import type { FeatureOffering, FeatureOfferings } from '../../types/exotic'
import FeatureActivationDialog from './FeatureActivationDialog.vue'
const { t } = useI18n()
const summary = ref<FeatureOfferings | null>(null)
const loading = ref(false)
const failed = ref(false)
const activationTarget = ref<FeatureOffering | null>(null)
let generation = 0
async function refresh(): Promise<void> {
  const current = ++generation
  loading.value = true
  failed.value = false
  summary.value = null
  try {
    const result = await invokeIpc<FeatureOfferings>(IPC.LIST_FEATURE_OFFERINGS)
    if (current === generation) summary.value = result
  } catch { if (current === generation) failed.value = true }
  finally { if (current === generation) loading.value = false }
}
function supported(feature: FeatureOffering): boolean {
  return !['unsupportedPlatform', 'incompatibleHost', 'invalidInstallation', 'disabled'].includes(feature.availability)
}
function featureName(feature: FeatureOffering): string {
  const keys: Record<string, string> = {
    'feature-editing': 'edit.premiumName', 'exotic-ocr': 'official.ocr', 'exotic-enhance': 'official.enhance',
    'exotic-image-psd': 'official.psd', 'exotic-image-raw': 'official.raw', 'video-extended': 'official.video',
  }
  return keys[feature.id] ? t(keys[feature.id]) : feature.name
}
function resourceLabel(feature: FeatureOffering): string {
  if (feature.availability === 'unsupportedPlatform') return 'exotic.blockedUnsupportedPlatform'
  if (feature.availability === 'incompatibleHost') return 'exotic.blockedIncompatibleHost'
  if (feature.availability === 'invalidInstallation') return 'exotic.blockedInvalidInstallation'
  if (feature.availability === 'disabled') return 'exotic.blockedDisabled'
  if (feature.resources !== 'ready') return 'official.' + feature.resources
  return feature.activationRequired && !feature.activated ? 'official.needsActivation' : 'official.ready'
}
useTauriListen(EVENTS.FEATURE_ACTIVATION_CHANGED, () => { void refresh() })
onMounted(refresh)
defineExpose({ refresh })
</script>

<style scoped>
.official-panel { padding: var(--spacing-lg); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-lg); background: var(--color-bg-surface); }
.official-panel__header, .official-panel__name { display: flex; align-items: center; gap: var(--spacing-md); flex-wrap: wrap; }
.official-panel__header { justify-content: space-between; }
.official-panel h3 { margin: 0; }
.official-panel p { color: var(--color-text-secondary); line-height: 1.6; }
.official-panel__features { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--spacing-md); margin-top: var(--spacing-lg); }
.official-panel__feature { padding: var(--spacing-md); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-md); }
.official-panel__name { justify-content: space-between; }
.official-panel__name span, .official-panel__notice { font-size: var(--font-size-xs); color: var(--color-text-secondary); }
.official-panel a { color: var(--color-accent); }
</style>
