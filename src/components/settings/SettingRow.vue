<template>
  <div class="settings-card__item setting-row" :class="{ 'setting-row--full': noControl }"
    :data-setting-key="settingKey" role="group" :aria-labelledby="labelId">
    <div class="settings-card__info">
      <div class="setting-row__heading">
        <div :id="labelId" class="settings-card__label">{{ spec ? $t(spec.label) : settingKey }}</div>
        <button type="button" class="pin-btn" :class="{ active: pinned }" :aria-pressed="pinned"
          :aria-label="$t(pinned ? 'settings.unpinFromSidebar' : 'settings.pinToSidebar')"
          :title="$t(pinned ? 'settings.unpinFromSidebar' : 'settings.pinToSidebar')"
          @click="ui.togglePinnedSetting(settingKey)"><Pin :size="13" /></button>
      </div>
      <!-- 路径、状态等特例说明常驻；仅显式提供摘要的普通行折叠完整帮助。 -->
      <slot name="desc">
        <div v-if="descriptionKey" class="settings-card__desc">{{ $t(descriptionKey) }}</div>
        <details v-if="spec?.descKey && spec.summaryKey !== undefined" class="setting-row__help">
          <summary :aria-label="$t('settings.settingHelp', { name: $t(spec.label) })"
            :title="$t('settings.moreInfo')"><Info :size="14" /></summary>
          <p>{{ $t(spec.descKey) }}</p>
        </details>
      </slot>
    </div>
    <div v-if="!noControl" class="setting-row__control">
      <slot><DynamicSettingControl :setting-key="settingKey" /></slot>
    </div>
    <div v-if="$slots.extra" class="setting-row__extra"><slot name="extra" /></div>
  </div>
</template>

<script setup lang="ts">
import { computed, useId } from 'vue'
import { Pin, Info } from '@lucide/vue'
import { useUiStore } from '../../stores/uiStore'
import { getSettingSpec } from '../../constants/settingsMap'
import DynamicSettingControl from './DynamicSettingControl.vue'
const props = defineProps<{
  /** 注册表键；标签、帮助与固定状态使用同一个设置标识。 */
  settingKey: string
  /** 控件通过 extra 插槽占据整行，如主题预览和缩略图尺寸档。 */
  noControl?: boolean
}>()
const ui = useUiStore()
const labelId = useId()
const spec = computed(() => getSettingSpec(props.settingKey))
const pinned = computed(() => ui.pinnedSettings.includes(props.settingKey))
const descriptionKey = computed(() =>
  spec.value?.summaryKey === undefined ? spec.value?.descKey : spec.value.summaryKey,
)
</script>

<style scoped>
.setting-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--spacing-sm) var(--spacing-xl);
  padding: var(--spacing-md) var(--spacing-lg);
}
.setting-row--full { grid-template-columns: minmax(0, 1fr); }
.settings-card__info { position: relative; }
.settings-card__info:has(.setting-row__help) { padding-right: 28px; }
.setting-row__heading { display: flex; align-items: center; gap: var(--spacing-xs); }
.setting-row__control { min-width: 0; max-width: 260px; }
.setting-row__extra { grid-column: 1 / -1; min-width: 0; }
.setting-row__help { font-size: var(--font-size-xs); color: var(--color-text-secondary); }
.setting-row__help summary {
  position: absolute;
  right: 0;
  top: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  list-style: none;
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.setting-row__help summary::-webkit-details-marker { display: none; }
.setting-row__help summary:hover, .setting-row__help[open] summary { color: var(--color-accent); background: var(--color-accent-subtle); }
.setting-row__help p { margin: var(--spacing-xs) 0 0; line-height: var(--leading-relaxed); overflow-wrap: anywhere; }
.pin-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  flex-shrink: 0;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  background: transparent;
  cursor: pointer;
  opacity: 0;
}
.setting-row:hover .pin-btn, .setting-row:focus-within .pin-btn, .pin-btn.active { opacity: 1; }
.pin-btn:hover, .pin-btn.active { color: var(--color-accent); background: var(--color-accent-subtle); }
@media (pointer: coarse) { .pin-btn { opacity: 1; } }
@media (max-width: 600px) {
  .setting-row { gap: var(--spacing-sm) var(--spacing-md); }
  .setting-row:has(.select-wrap, .batch-size-stack, .setting-actions) { grid-template-columns: minmax(0, 1fr); }
  .setting-row__control { max-width: 100%; }
}
</style>
