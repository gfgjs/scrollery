<template>
  <div class="theme-settings">
    <!-- 外观模式:只切偏好,不动主题参数与草稿。 -->
    <div class="theme-settings__mode" role="group" :aria-label="t('settings.themeMode')">
      <button
        v-for="option in modeOptions"
        :key="option.value"
        type="button"
        class="theme-settings__mode-btn"
        :class="{ active: theme.appearance === option.value }"
        :aria-pressed="theme.appearance === option.value"
        @click="theme.setAppearance(option.value)"
      >
        <component :is="option.icon" :size="14" aria-hidden="true" />
        <span>{{ t(option.labelKey) }}</span>
      </button>
    </div>

    <!-- 主题选择:内置预设与我的主题都是完整参数快照,选中只复制进草稿。 -->
    <section class="theme-settings__group">
      <h4 class="theme-settings__group-title">
        {{ t('settings.themeBuiltinGroup') }}
        <span class="theme-settings__current">{{ currentNameText }}</span>
      </h4>
      <div class="theme-settings__grid">
        <button
          v-for="preset in BUILTIN_PRESETS"
          :key="preset.id"
          type="button"
          class="theme-tile"
          :class="{ selected: currentThemeKey === preset.id }"
          :aria-pressed="currentThemeKey === preset.id"
          @click="theme.loadDefinition(preset.definition)"
        >
          <span class="theme-tile__previews">
            <ThemePreview
              v-for="mode in THEME_MODES"
              :key="mode"
              class="theme-tile__preview"
              :definition="preset.definition"
              :mode="mode"
            />
          </span>
          <span class="theme-tile__name">{{ t(preset.nameKey) }}</span>
        </button>
      </div>
    </section>

    <section class="theme-settings__group">
      <h4 class="theme-settings__group-title">{{ t('settings.themeSavedGroup') }}</h4>
      <p v-if="!theme.savedThemes.length" class="theme-settings__empty">
        {{ t('settings.themeSavedEmpty') }}
      </p>
      <ul v-else class="theme-saved-list">
        <li
          v-for="saved in theme.savedThemes"
          :key="saved.id"
          class="theme-saved"
          :class="{ selected: currentThemeKey === saved.id }"
        >
          <button type="button" class="theme-saved__pick" @click="theme.loadSavedTheme(saved.id)">
            <span class="theme-saved__previews">
              <ThemePreview
                v-for="mode in THEME_MODES"
                :key="mode"
                class="theme-saved__preview"
                :definition="saved"
                :mode="mode"
              />
            </span>
            <span class="theme-saved__name">{{ saved.name }}</span>
          </button>
          <span class="theme-saved__actions">
            <UiIconButton :label="t('settings.themeRename')" @click="openRename(saved)">
              <Pencil :size="14" />
            </UiIconButton>
            <UiIconButton :label="t('settings.themeDelete')" @click="removeTheme(saved)">
              <Trash2 :size="14" />
            </UiIconButton>
          </span>
        </li>
      </ul>
    </section>

    <details class="theme-settings__customize">
      <summary>{{ t('settings.themeCustomize') }}</summary>
    <!-- 两张配色卡共用同一份草稿,可来回调整;窄屏纵排、宽屏并排。 -->
    <div class="theme-settings__cards">
      <ThemePaletteCard
        v-for="mode in THEME_MODES"
        :key="mode"
        :mode="mode"
        :seed="editable[mode]"
        :definition="editable"
        :previewing="theme.previewMode === mode"
        @update="onSeedUpdate(mode, $event)"
        @preview="onPreviewMode(mode)"
        @reset="theme.resetSeedToDefault(mode)"
      />
    </div>

    <section class="theme-settings__group">
      <h4 class="theme-settings__group-title">{{ t('settings.themeVisualStyle') }}</h4>
      <UiSelect
        :model-value="editable.visualStyle"
        @update:model-value="theme.updateVisualStyle($event as ThemeVisualStyle)"
      >
        <option v-for="style in THEME_VISUAL_STYLES" :key="style" :value="style">
          {{ t(VISUAL_STYLE_LABEL_KEYS[style]) }}
        </option>
      </UiSelect>
    </section>

<!-- 窗口材质:草稿即时预览界面填充,窗口原生材质在应用后生效。 -->
    <section class="theme-settings__group">
      <h4 class="theme-settings__group-title">{{ t('settings.windowMaterial') }}</h4>
      <div class="theme-settings__material">
        <UiSelect
          :model-value="editable.material"
          @update:model-value="theme.updateMaterial($event as ThemeMaterial)"
        >
          <option v-for="material in THEME_MATERIALS" :key="material" :value="material">
            {{ t(MATERIAL_LABEL_KEYS[material]) }}
          </option>
        </UiSelect>
        <label class="theme-settings__opacity">
          <span>{{ t('settings.windowOpacity') }}</span>
          <input
            type="range"
            min="0"
            max="100"
            step="1"
            :disabled="editable.material === 'none'"
            :value="editable.opacity"
            @input="theme.updateOpacity(Number(($event.target as HTMLInputElement).value))"
          />
          <span class="theme-settings__opacity-value">{{ editable.opacity }}</span>
        </label>
      </div>
      <p class="theme-settings__hint">{{ t('settings.windowMaterialDraftHint') }}</p>
    </section>

    </details>

    <p v-if="contrastWarningModes.length" class="theme-settings__warning">
      {{ t('settings.themeContrastWarning', { modes: contrastWarningModes.join('、') }) }}
    </p>

        <footer class="theme-settings__actions">
      <UiButton variant="secondary" :disabled="!theme.isEditing" @click="theme.cancelEdit()">
        {{ t('common.cancel') }}
      </UiButton>
      <UiButton variant="ghost" @click="openSave()">
        {{ t('settings.themeSaveAs') }}
      </UiButton>
      <UiButton
        variant="primary"
        :loading="applying"
        :disabled="!theme.isEditing || !theme.draftDirty"
        @click="apply"
      >
        {{ t('settings.themeApply') }}
      </UiButton>
    </footer>

    <UiDialog
      :open="dialog !== null"
      :title="dialogTitle"
      max-width="420px"
      :close-label="t('common.cancel')"
      @close="closeDialog"
    >
      <label class="theme-dialog__field">
        <span>{{ t('settings.themeNameLabel') }}</span>
        <input
          ref="nameInputEl"
          v-model="nameInput"
          type="text"
          maxlength="60"
          :disabled="updatingExisting"
          data-autofocus
          @keydown.enter.prevent="submitDialog"
        />
      </label>
      <!-- 更新已有主题保持原 ID 与名称:名称锁定,不让用户填了却被静默忽略。 -->
      <p v-if="updatingExisting" class="theme-dialog__hint">
        {{ t('settings.themeUpdateKeepsName') }}
      </p>
      <label v-if="dialog === 'save' && theme.savedThemes.length" class="theme-dialog__field">
        <span>{{ t('settings.themeUpdateTarget') }}</span>
        <UiSelect v-model="updateTargetId">
          <option value="">{{ t('settings.themeSaveNewEntry') }}</option>
          <option v-for="saved in theme.savedThemes" :key="saved.id" :value="saved.id">
            {{ saved.name }}
          </option>
        </UiSelect>
      </label>
      <p v-if="dialogError" class="theme-dialog__error">{{ dialogError }}</p>
      <template #footer>
        <UiButton variant="secondary" @click="closeDialog">{{ t('common.cancel') }}</UiButton>
        <UiButton variant="primary" :loading="saving" @click="submitDialog">
          {{ dialog === 'save' ? t('settings.themeSaveAndApply') : t('common.confirm') }}
        </UiButton>
      </template>
    </UiDialog>
  </div>
</template>

<script setup lang="ts">
// 外观主题设置面板(方案 §2):外观模式 → 主题选择 → 浅／深配色卡 → 窗口材质 → 应用／取消／保存。
// 草稿、预览与提交全部由 themeStore 持有;本组件只做展示与命名操作编排,不自持状态副本。
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Monitor, Moon, Pencil, Sun, Trash2 } from '@lucide/vue'
import UiButton from '../ui/UiButton.vue'
import UiDialog from '../ui/UiDialog.vue'
import UiIconButton from '../ui/UiIconButton.vue'
import UiSelect from '../ui/UiSelect.vue'
import ThemePaletteCard from './ThemePaletteCard.vue'
import ThemePreview from './ThemePreview.vue'
import { useThemeStore } from '../../stores/themeStore'
import { useToastStore } from '../../stores/toastStore'
import { logger } from '../../utils/logger'
import { BUILTIN_PRESETS, definitionEquals } from '../../themes/presets'
import { THEME_VISUAL_STYLES } from '../../themes/visualStyles'
import { MIN_TEXT_CONTRAST } from '../../themes/generate'
import { contrastRatio } from '../../themes/colors'
import {
  THEME_MATERIALS,
  THEME_MODES,
  type SavedTheme,
  type ThemeDefinition,
  type ThemeMaterial,
  type ThemeMode,
  type ThemeSeed,
  type ThemeVisualStyle,
} from '../../themes/types'
import type { AppearanceMode } from '../../types/ui'

const { t } = useI18n()
const theme = useThemeStore()
const toast = useToastStore()

const modeOptions: { value: AppearanceMode; icon: typeof Sun; labelKey: string }[] = [
  { value: 'light', icon: Sun, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: Moon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: Monitor, labelKey: 'settings.themeSystem' },
]

const MATERIAL_LABEL_KEYS: Record<ThemeMaterial, string> = {
  none: 'settings.windowMaterialNone',
  mica: 'settings.windowMaterialMica',
  acrylic: 'settings.windowMaterialAcrylic',
}

const VISUAL_STYLE_LABEL_KEYS: Record<ThemeVisualStyle, string> = {
  standard: 'settings.themeVisualStandard',
  mint: 'settings.themeVisualMint',
  forest: 'settings.themeVisualForest',
}

/** 可编辑参数:草稿优先,未进编辑时即已应用值(store 不再单列 editableDefinition)。 */
const editable = computed<ThemeDefinition>(() => theme.draft ?? theme.appliedDefinition)

/** 当前参数与主题库的唯一匹配:唯一命中显示名称,无匹配或多条同配色命中显示「自定义」。 */
const currentThemeKey = computed<string | null>(() => {
  const definition = editable.value
  const hits: string[] = []
  for (const preset of BUILTIN_PRESETS) {
    if (definitionEquals(preset.definition, definition)) hits.push(preset.id)
  }
  for (const saved of theme.savedThemes) {
    if (definitionEquals(saved, definition)) hits.push(saved.id)
  }
  return hits.length === 1 ? hits[0] : null
})

const currentNameText = computed(() => {
  const key = currentThemeKey.value
  if (key) {
    const preset = BUILTIN_PRESETS.find((entry) => entry.id === key)
    if (preset) return t(preset.nameKey)
    const saved = theme.savedThemes.find((entry) => entry.id === key)
    if (saved) return saved.name
  }
  return t('settings.themeCustom')
})

/**
 * 对比不足的模式名列表:只比较该套种子的前景与背景(拖动中每事件都算,故不生成整份色板)。
 * 正文即种子前景色,这是实际承载面上最主要的一对读取关系。
 */
const contrastWarningModes = computed(() =>
  THEME_MODES.filter(
    (mode) =>
      contrastRatio(editable.value[mode].foreground, editable.value[mode].background) <
      MIN_TEXT_CONTRAST,
  ).map((mode) => t(mode === 'light' ? 'settings.themeLight' : 'settings.themeDark')),
)

function onSeedUpdate(mode: ThemeMode, patch: Partial<ThemeSeed>): void {
  theme.updateSeed(mode, patch)
}

/** 临时预览某一模式:改的是呈现,不动 appearance 偏好;再点一次回到偏好模式。 */
function onPreviewMode(mode: ThemeMode): void {
  if (!theme.isEditing) {
    theme.beginEdit(mode)
    return
  }
  theme.setPreviewMode(theme.previewMode === mode ? null : mode)
}

// ── 应用 ─────────────────────────────────────────────────────────────────
const applying = ref(false)

async function apply(): Promise<void> {
  applying.value = true
  try {
    await theme.applyTheme()
  } catch (error) {
    // 失败提示由中央保存服务统一发出(含回滚),此处只留日志。
    logger.error('apply theme failed', { error })
  } finally {
    applying.value = false
  }
}

// ── 命名保存／更新／重命名 ───────────────────────────────────────────────
type DialogMode = 'save' | 'rename'

const dialog = ref<DialogMode | null>(null)
const nameInput = ref('')
const nameInputEl = ref<HTMLInputElement | null>(null)
const updateTargetId = ref('')
const dialogError = ref('')
const saving = ref(false)

/** 更新已有个人主题:名称沿用原名,输入框锁定(改名走「重命名」)。 */
const updatingExisting = computed(() => dialog.value === 'save' && updateTargetId.value !== '')

const dialogTitle = computed(() =>
  dialog.value === 'rename' ? t('settings.themeRename') : t('settings.themeSaveAs'),
)

function openSave(): void {
  dialog.value = 'save'
  nameInput.value = ''
  updateTargetId.value = ''
  dialogError.value = ''
}

// 选到更新目标时名称栏显示该主题原名(锁定态下也不是空白一片)。
watch(updateTargetId, (id) => {
  if (dialog.value !== 'save') return
  nameInput.value = id ? (theme.savedThemes.find((entry) => entry.id === id)?.name ?? '') : ''
})

function openRename(saved: SavedTheme): void {
  dialog.value = 'rename'
  nameInput.value = saved.name
  updateTargetId.value = saved.id
  dialogError.value = ''
}

/** 关闭命名对话:只丢输入,主题草稿原样保留。 */
function closeDialog(): void {
  dialog.value = null
  dialogError.value = ''
}

function nameErrorText(code: 'emptyName' | 'duplicateName' | 'notFound'): string {
  if (code === 'emptyName') return t('settings.themeNameEmpty')
  if (code === 'duplicateName') return t('settings.themeNameDuplicate')
  return t('settings.themeNotFound')
}

async function submitDialog(): Promise<void> {
  if (dialog.value === null || saving.value) return
  saving.value = true
  dialogError.value = ''
  try {
    const result =
      dialog.value === 'rename'
        ? await theme.renameSavedTheme(updateTargetId.value, nameInput.value)
        : updateTargetId.value
          ? await theme.updateSavedTheme(updateTargetId.value, editable.value)
          : await theme.createSavedTheme(nameInput.value)
    if (!result.ok) {
      dialogError.value = nameErrorText(result.reason)
      return
    }
    toast.addToast('success', t('settings.themeSaved', { name: result.theme.name }))
    closeDialog()
  } catch (error) {
    logger.error('save theme failed', { error })
    dialogError.value = t('settings.themeSaveFailed')
  } finally {
    saving.value = false
  }
}

async function removeTheme(saved: SavedTheme): Promise<void> {
  try {
    await theme.deleteSavedTheme(saved.id)
    toast.addToast('success', t('settings.themeDeleted', { name: saved.name }), 6000, [
      { label: t('common.undo'), onClick: () => void theme.undoDeleteSavedTheme() },
    ])
  } catch (error) {
    logger.error('delete theme failed', { error })
    toast.addToast('error', t('settings.themeDeleteFailed'))
  }
}

// 草稿被外部配置变更或重置丢弃时给一次可见提示(store 只发信号,文案由界面决定)。
watch(
  () => theme.draftInvalidation,
  (signal) => {
    if (!signal) return
    toast.addToast(
      'info',
      signal.reason === 'reset'
        ? t('settings.themeDraftDroppedReset')
        : t('settings.themeDraftDroppedExternal'),
    )
  },
)

// 命名对话打开后聚焦输入框(UiDialog 的 data-autofocus 已在首帧生效,此处兜住重复打开)。
watch(dialog, (value) => {
  if (value) void Promise.resolve().then(() => nameInputEl.value?.focus())
})
</script>

<style scoped>
.theme-settings__customize > summary {
  cursor: pointer;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  padding-block: var(--spacing-sm);
}
.theme-settings__customize[open] > summary { margin-bottom: var(--spacing-md); }
.theme-settings__customize .theme-settings__group { margin-top: var(--spacing-lg); }
.theme-settings {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  padding: var(--spacing-xs) 0 0;
}

.theme-settings__mode {
  display: inline-flex;
  align-self: flex-start;
  gap: 2px;
  padding: 2px;
  background: var(--color-bg-inset);
  border-radius: var(--radius-md);
}

.theme-settings__mode-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}

.theme-settings__mode-btn:hover {
  color: var(--color-text-primary);
}

.theme-settings__mode-btn.active {
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  box-shadow: var(--shadow-sm);
}

.theme-settings__group {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.theme-settings__group-title {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  margin: 0;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  font-weight: 600;
}

.theme-settings__current {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  font-weight: 400;
}

.theme-settings__grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: var(--spacing-sm);
}

.theme-tile {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  text-align: left;
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    background var(--transition-fast);
}

.theme-tile:hover {
  border-color: var(--color-border-strong);
  background: var(--color-bg-hover);
}

.theme-tile.selected {
  border-color: var(--color-accent);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.theme-tile__previews {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 2px;
}

.theme-tile__preview,
.theme-saved__preview {
  height: 48px;
  pointer-events: none;
}

.theme-tile__name,
.theme-saved__name {
  padding: 0 2px 2px;
  font-size: var(--font-size-xs);
}

.theme-settings__empty,
.theme-settings__hint {
  margin: 0;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}

.theme-saved-list {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  margin: 0;
  padding: 0;
  list-style: none;
}

.theme-saved {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-surface);
}

.theme-saved.selected {
  border-color: var(--color-accent);
}

.theme-saved__pick {
  display: flex;
  min-width: 0;
  flex: 1;
  align-items: center;
  gap: var(--spacing-sm);
  border: none;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.theme-saved__previews {
  display: grid;
  width: 108px;
  flex-shrink: 0;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 2px;
}

.theme-saved__name {
  overflow: hidden;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.theme-saved__actions {
  display: flex;
  flex-shrink: 0;
  gap: var(--spacing-2xs);
}

.theme-settings__cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--spacing-sm);
}

.theme-settings__warning {
  margin: 0;
  color: var(--color-warning);
  font-size: var(--font-size-xs);
}

.theme-settings__material {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--spacing-sm);
}

.theme-settings__opacity {
  display: flex;
  flex: 1;
  min-width: 200px;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}

.theme-settings__opacity input {
  flex: 1;
  min-width: 0;
}

.theme-settings__opacity-value {
  width: 2.5em;
  color: var(--color-text-tertiary);
  font-family: var(--font-mono);
  text-align: right;
}

.theme-settings__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
}

.theme-dialog__field {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.theme-dialog__field input {
  padding: 6px var(--spacing-sm);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
}

.theme-dialog__error {
  margin: 0;
  color: var(--color-error);
  font-size: var(--font-size-xs);
}

.theme-dialog__hint {
  margin: 0;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}
</style>
