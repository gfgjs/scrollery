<template>
  <section class="official-panel" aria-live="polite">
    <header class="official-panel__header">
      <div>
        <h3>{{ $t('official.title') }}</h3>
        <p>{{ $t('official.description') }}</p>
      </div>
      <button class="btn btn-ghost btn-sm" :disabled="loading" @click="refresh">
        {{ $t('official.retry') }}
      </button>
    </header>
    <p v-if="failed" role="alert">{{ $t('official.queryFailed') }}</p>
    <p v-else-if="!summary">{{ $t('exotic.gateChecking') }}</p>
    <template v-else>
      <div class="official-panel__actions">
        <span>{{ $t(licenseLabel) }}</span>
        <button class="btn btn-primary btn-sm" :disabled="busy" @click="activationOpen = true">
          {{ $t('exotic.activateAction') }}
        </button>
        <button v-if="summary.entitlement.availability !== 'installedUnlicensed'" class="btn btn-ghost btn-sm" :disabled="busy" @click="removeLicense">
          {{ $t('official.remove') }}
        </button>
        <button v-if="canPurchase" class="btn btn-ghost btn-sm" :disabled="busy" @click="buy">
          {{ $t('official.buy') }}
        </button>
        <span v-else>{{ $t('official.notOnSale') }}</span>
      </div>
      <div class="official-panel__features">
        <article v-for="feature in summary.features" :key="feature.id" class="official-panel__feature">
          <div class="official-panel__name">
            <strong>{{ featureName(feature) }}</strong>
            <span>{{ $t(feature.paid ? 'official.includedLicense' : 'official.free') }}</span>
          </div>
          <p>{{ $t(resourceLabel(feature)) }}</p>
          <RouterLink v-if="feature.resources === 'modelMissing' && supported(feature)" to="/settings">
            {{ $t('official.prepareModels') }}
          </RouterLink>
          <p v-if="feature.id === 'video-extended'" class="official-panel__notice">
            {{ $t('store.videoExtendedFfmpegNotice') }}
            <a href="https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-07-24-13-32" target="_blank" rel="noopener noreferrer">{{ $t('store.videoExtendedSourceLink') }}</a>
          </p>
        </article>
      </div>
    </template>
    <ExoticActivateDialog :open="activationOpen" :feature-name="$t('official.title')" @close="activationOpen = false" @activated="refresh" />
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { IPC, EVENTS } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'
import { openPurchase, purchaseUrl } from '../../utils/openPurchase'
import { useTauriListen } from '../../composables/useTauriListen'
import { useConfirm } from '../../composables/useConfirm'
import { useToastStore } from '../../stores/toastStore'
import type { FeatureOffering, FeatureOfferings } from '../../types/exotic'
import ExoticActivateDialog from './ExoticActivateDialog.vue'

const { t } = useI18n()
const { confirm } = useConfirm()
const toast = useToastStore()
const summary = ref<FeatureOfferings | null>(null)
const loading = ref(false)
const failed = ref(false)
const busy = ref(false)
const activationOpen = ref(false)
let generation = 0

async function refresh(): Promise<void> {
  const current = ++generation
  loading.value = true
  failed.value = false
  summary.value = null
  try {
    const result = await invokeIpc<FeatureOfferings>(IPC.LIST_FEATURE_OFFERINGS)
    if (current === generation) summary.value = result
  } catch {
    if (current === generation) failed.value = true
  } finally {
    if (current === generation) loading.value = false
  }
}

function supported(feature: FeatureOffering): boolean {
  return !['unsupportedPlatform', 'incompatibleHost', 'invalidInstallation', 'disabled'].includes(feature.availability)
}
const canPurchase = computed(() => !!purchaseUrl(summary.value?.entitlement.storeUrl ?? null) &&
  summary.value?.features.filter((f) => f.paid).every((f) => supported(f) && f.resources !== 'manifestUnready'))
const licenseLabel = computed(() => summary.value?.entitlement.availability === 'authorized'
  ? 'official.activated' : summary.value?.entitlement.availability === 'licenseExpired' ? 'official.expired' : 'official.notActivated')

function featureName(feature: FeatureOffering): string {
  const keys: Record<string, string> = {
    'feature-editing': 'edit.premiumName', 'exotic-ocr': 'official.ocr',
    'exotic-enhance': 'official.enhance', 'exotic-image-psd': 'official.psd',
    'exotic-image-raw': 'official.raw', 'video-extended': 'official.video',
  }
  return keys[feature.id] ? t(keys[feature.id]) : feature.name
}
function resourceLabel(feature: FeatureOffering): string {
  if (feature.availability === 'unsupportedPlatform') return 'exotic.blockedUnsupportedPlatform'
  if (feature.availability === 'incompatibleHost') return 'exotic.blockedIncompatibleHost'
  if (feature.availability === 'invalidInstallation') return 'exotic.blockedInvalidInstallation'
  if (feature.availability === 'disabled') return 'exotic.blockedDisabled'
  if (feature.resources !== 'ready') return `official.${feature.resources}`
  return feature.paid && summary.value?.entitlement.availability !== 'authorized' ? 'official.needsActivation' : 'official.ready'
}
async function buy(): Promise<void> {
  if (!canPurchase.value) return
  busy.value = true
  try { await openPurchase(summary.value?.entitlement.storeUrl ?? null) }
  catch { toast.addToast('error', t('official.openFailed')) }
  finally { busy.value = false }
}
async function removeLicense(): Promise<void> {
  const result = await confirm({ title: t('official.remove'), message: t('official.removeWarning'), confirmText: t('official.remove') })
  if (!result.confirmed) return
  busy.value = true
  try {
    await invokeIpc(IPC.DEACTIVATE_OFFICIAL_LICENSE)
    await refresh()
  } catch { toast.addToast('error', t('official.removeFailed')) }
  finally { busy.value = false }
}
useTauriListen(EVENTS.OFFICIAL_LICENSE_CHANGED, () => { void refresh() })
onMounted(refresh)
defineExpose({ refresh })
</script>

<style scoped>
.official-panel { padding: var(--spacing-lg); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-lg); background: var(--color-bg-surface); }
.official-panel__header, .official-panel__actions, .official-panel__name { display: flex; align-items: center; gap: var(--spacing-md); flex-wrap: wrap; }
.official-panel__header { justify-content: space-between; }
.official-panel h3 { margin: 0; }
.official-panel p { color: var(--color-text-secondary); line-height: 1.6; }
.official-panel__features { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--spacing-md); margin-top: var(--spacing-lg); }
.official-panel__feature { padding: var(--spacing-md); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-md); }
.official-panel__name { justify-content: space-between; }
.official-panel__name span, .official-panel__notice { font-size: var(--font-size-xs); color: var(--color-text-secondary); }
.official-panel a { color: var(--color-accent); }
</style>
