<template>
  <div v-if="loading" class="gate gate--muted"><span class="gate__spinner" /><span>{{ $t('exotic.gateChecking') }}</span></div>
  <div v-else-if="failed" class="gate gate--blocked"><span>{{ $t('official.queryFailed') }}</span><button class="btn btn-ghost" @click="emit('retry')">{{ $t('official.retry') }}</button></div>
  <slot v-else-if="mode === 'passthrough' || mode === 'ready'" />
  <div v-else-if="mode === 'activation'" class="gate">
    <div class="gate__icon"><KeyRound :size="22" /></div>
    <div class="gate__body">
      <div class="gate__title">{{ featureName || $t('exotic.gateTitle') }}</div>
      <p class="gate__desc">{{ featureDesc || $t('exotic.gateDescGeneric') }}</p>
      <div class="gate__actions">
        <button v-if="entitlement?.activationRequired && !entitlement.activated" class="btn btn-primary" @click="emit('activate')">{{ $t('exotic.activateAction') }}</button>
        <span v-else-if="entitlement?.activated">{{ $t('official.activated') }}</span>
        <RouterLink v-if="entitlement?.availability === 'availableUninstalled'" to="/plugins">{{ $t('official.installPlugin') }}</RouterLink>
      </div>
    </div>
  </div>
  <div v-else class="gate gate--blocked">
    <div class="gate__icon gate__icon--warn"><AlertTriangle :size="22" /></div>
    <div class="gate__body"><div class="gate__title">{{ $t('exotic.blockedTitle') }}</div><p class="gate__desc">{{ blockedReason }}</p></div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { AlertTriangle, KeyRound } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import type { PluginAvailability } from '../../types/exotic'
import { gateModeFor } from '../../composables/usePluginAvailability'
const props = withDefaults(defineProps<{
  entitlement: PluginAvailability | null; loading?: boolean; failed?: boolean; featureName?: string; featureDesc?: string
}>(), { loading: false, failed: false, featureName: '', featureDesc: '' })
const emit = defineEmits<{ (e: 'activate'): void; (e: 'retry'): void }>()
const { t } = useI18n()
const mode = computed(() => gateModeFor(props.entitlement))
const blockedReason = computed(() => {
  switch (props.entitlement?.availability) {
    case 'unsupportedPlatform': return t('exotic.blockedUnsupportedPlatform')
    case 'incompatibleHost': return t('exotic.blockedIncompatibleHost')
    case 'invalidInstallation': return t('exotic.blockedInvalidInstallation')
    case 'disabled': return t('exotic.blockedDisabled')
    default: return t('exotic.blockedTitle')
  }
})
</script>

<style scoped>
/* gate 通用容器：卡片式，留白舒展，与设置卡风格一致。 */
.gate {
  display: flex;
  gap: var(--spacing-md);
  padding: var(--spacing-lg);
  border-radius: var(--radius-lg);
  border: 1px solid var(--color-border-subtle);
  background: var(--color-bg-surface);
}

/* 检查中：单行轻量占位。 */
.gate--muted {
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
}
.gate__muted-text {
  min-width: 0;
}
.gate__spinner {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 2px solid var(--color-border-strong);
  border-top-color: var(--color-accent);
  animation: gate-spin var(--duration-spin) linear infinite;
}
@keyframes gate-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 图标徽标：强调色底 + accent 前景。 */
.gate__icon {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  border-radius: var(--radius-md);
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}
.gate__icon--warn {
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}

.gate__body {
  min-width: 0;
  flex: 1;
}
.gate__title {
  font-weight: 700;
  font-size: var(--font-size-base);
  color: var(--color-text-primary);
}
.gate__desc {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-sm);
  line-height: var(--leading-normal);
  color: var(--color-text-secondary);
}

.gate__actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-md);
}
</style>
