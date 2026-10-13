<template>
  <div :class="['dynamic-control', { compact }]">
    <!-- ── 特例:AI 批大小(固定 batch 钳制 + 分级风险提示,注册表 control='custom')── -->
    <template v-if="settingKey === 'aiBatchSize'">
      <div class="batch-size-stack">
        <input
          type="number"
          :aria-label="$t('settings.aiBatchSize')"
          v-model.number="aiBatchSizeLocal"
          @change="onBatchChange"
          min="0"
          max="512"
          class="input-number"
          :class="{ 'compact-input': compact }"
          :placeholder="$t('settings.aiBatchAutoPlaceholder')"
        />
        <span v-if="aiBatchSizeLocal === 0" class="batch-hint batch-hint--ok"
          >{{ $t('settings.aiBatchAutoAllocated')
          }}<template v-if="ai.status.activeFixedBatch">{{
            $t('settings.aiBatchNotLowerThan', { k: ai.status.activeFixedBatch })
          }}</template></span
        >
        <span
          v-else-if="ai.status.activeFixedBatch && aiBatchSizeLocal < ai.status.activeFixedBatch"
          class="batch-hint batch-hint--error"
          >{{ $t('settings.aiBatchFixedCannotBeLower', { k: ai.status.activeFixedBatch }) }}</span
        >
        <span v-else-if="aiBatchSizeLocal > 200" class="batch-hint batch-hint--error">{{
          $t('settings.aiBatchHighRisk')
        }}</span>
        <span v-else-if="aiBatchSizeLocal > 128" class="batch-hint batch-hint--warn">{{
          $t('settings.aiBatchWarning')
        }}</span>
        <span v-else-if="ai.status.activeFixedBatch" class="batch-hint batch-hint--muted">{{
          $t('settings.aiBatchFixedMin', { k: ai.status.activeFixedBatch })
        }}</span>
      </div>
    </template>

    <!-- ── 特例:缩略图尺寸档 segmented ─────────────────────────── -->
    <template v-else-if="settingKey === 'thumbSize'">
      <UiSelect
        v-if="compact"
        :model-value="String(config.thumbSize)"
        :label="$t('settings.thumbSize')"
        class="compact-select-wrap"
        @update:model-value="config.setThumbSize(Number($event))"
      >
        <option v-for="tier in THUMB_SIZE_TIERS" :key="tier" :value="String(tier)">
          {{ getTierLabel(tier) }}
        </option>
      </UiSelect>
      <div v-else class="segmented-control">
        <button
          v-for="tier in THUMB_SIZE_TIERS"
          :key="tier"
          class="segmented-btn"
          :class="{ active: config.thumbSize === tier }"
          @click="config.setThumbSize(tier)"
        >
          {{ getTierLabel(tier) }}
        </button>
      </div>
    </template>

    <!-- ── 开关类(注册表 control='toggle',绑定见 toggleBindings)──── -->
    <template v-else-if="spec?.control === 'toggle' && hasToggleBinding">
      <UiToggle v-model="toggleModel" :label="$t(spec.label)" />
    </template>

    <!-- ── 下拉类(control='select',选项表来自注册表)────────────── -->
    <template v-else-if="spec?.control === 'select' && hasSelectBinding">
      <UiSelect v-model="selectModel" :label="$t(spec.label)" :class="{ 'compact-select-wrap': compact }">
        <option v-for="opt in spec.options ?? []" :key="opt.value" :value="opt.value">
          {{ opt.labelKey ? $t(opt.labelKey) : opt.label }}
        </option>
      </UiSelect>
    </template>

    <!-- ── 数字类(control='number',边界来自注册表;本地缓冲,change 时提交)── -->
    <template v-else-if="spec?.control === 'number' && hasNumberBinding">
      <div class="number-control">
      <input
        type="number"
        v-model.number="numberLocal"
        :aria-label="[$t(spec.label), spec.unit].filter(Boolean).join(' ')"
        @change="commitNumber"
        :min="spec.min"
        :max="spec.max"
        class="input-number"
        :class="{ 'compact-input': compact }"
      />
      <span v-if="spec.unit" class="number-control__unit" aria-hidden="true">{{ spec.unit }}</span>
      </div>
    </template>

    <!-- ── 危险清理按钮类(compact 统一 RotateCcw 图标,全尺寸按键取各自图标)── -->
    <template v-else-if="dangerSpec && spec">
      <UiIconButton
        v-if="compact"
        class="danger-icon"
        :label="$t(spec.label)"
        @click="dangerSpec.onClick"
      >
        <RotateCcw :size="14" />
      </UiIconButton>
      <button v-else class="btn btn-danger" @click="dangerSpec.onClick">
        <component :is="dangerSpec.icon" :size="14" /> {{ $t(dangerSpec.btnLabelKey) }}
      </button>
    </template>

    <!-- ── 普通动作按钮类(非破坏性,如 cache-busting 重载;不给危险视觉——§8.4 Error 只用于 destructive)── -->
    <template v-else-if="plainSpec && spec">
      <UiIconButton v-if="compact" :label="$t(spec.label)" @click="plainSpec.onClick">
        <component :is="plainSpec.icon" :size="14" />
      </UiIconButton>
      <button v-else class="btn btn-secondary" @click="plainSpec.onClick">
        <component :is="plainSpec.icon" :size="14" /> {{ $t(plainSpec.btnLabelKey) }}
      </button>
    </template>

    <!-- 复杂项复用设置页完整控件，侧栏提供所属分类入口。 -->
    <template v-else>
      <UiButton
        v-if="compact && settingsSection"
        class="btn-sm related-settings-button"
        @click="router.push(`/settings/${settingsSection}`)"
      >
        {{ $t('settings.openRelatedSettings') }}
      </UiButton>
      <span v-else class="unsupported-hint">{{ $t('settings.unsupportedQuickAction') }}</span>
    </template>
  </div>
</template>

<script setup lang="ts">
// SETTINGS_MAP.control 决定控件形态；下方绑定表决定各设置项的读写源。
// 数字输入先写本地缓冲，change 时才提交，避免逐键触发 IPC。
import { ref, computed, watch } from 'vue'
import type { Component } from 'vue'
import { useRouter } from 'vue-router'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useUiStore } from '../../stores/uiStore'
import { useThemeStore } from '../../stores/themeStore'
import type { MinimapRenderMode } from '../../stores/uiStore'
import { useToastStore } from '../../stores/toastStore'
import { useConfigStore } from '../../stores/configStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useScanStore } from '../../stores/scanStore'
import { useAiStore } from '../../stores/aiStore'
import { useI18n } from 'vue-i18n'
import { THUMB_SIZE_TIERS } from '../../constants/defaults'
import { getSettingSpec } from '../../constants/settingsMap'
import { SETTINGS_GROUPS } from '../../constants/settingsLayout'
import { RotateCcw, Database, Gauge, Paintbrush, Terminal } from '@lucide/vue'
import UiIconButton from '../ui/UiIconButton.vue'
import UiToggle from '../ui/UiToggle.vue'
import UiSelect from '../ui/UiSelect.vue'
import UiButton from '../ui/UiButton.vue'
import { useConfirm } from '../../composables/useConfirm'
import { useRenderMode, type RenderMode } from '../../composables/useRenderMode'
import { useTitlebarMode } from '../../composables/useTitlebarMode'
import { useSelectionBarMode } from '../../composables/useSelectionBarMode'
import { useToolbarAlign, type BarAlign } from '../../composables/useToolbarAlign'
import { usePerformanceMonitor } from '../../composables/usePerformanceMonitor'
import { resetSettings } from '../../stores/settingsPersistence'
import { logger } from '../../utils/logger'

const props = defineProps<{
  settingKey: string
  compact?: boolean
}>()

const ui = useUiStore()
const router = useRouter()
// 外观模式归主题域(见 stores/themeStore):本组件只读偏好、写偏好,不碰主题参数与草稿。
const theme = useThemeStore()
const toast = useToastStore()
const config = useConfigStore()
const media = useMediaStore()
const scan = useScanStore()
const ai = useAiStore()
const { t } = useI18n()
// 危险操作确认走 app 内的全局 ConfirmDialog(useConfirm),而非原生 window.confirm——
// 后者在 Tauri webview 不可靠(不弹框却返回 truthy,危险操作会无提示直接执行,见真机事故)。
const { confirm } = useConfirm()
// DOM↔Canvas 渲染引擎偏好(实验性设置项 galleryRenderMode/timelineRenderMode 的读写源)。
const renderMode = useRenderMode()
// 标题栏合并/分离布局开关(设置项 titlebarMerged 的读写源)。
const titlebarMode = useTitlebarMode()
// 选区操作条停靠/浮动开关(设置项 selectionBarDocked 的读写源)。
const selectionBarMode = useSelectionBarMode()
// 顶栏中部 chips 簇水平对齐(设置项 toolbarAlign 的读写源;选区胶囊对齐读 selectionBarMode.align)。
const toolbarAlign = useToolbarAlign()
const performanceMonitor = usePerformanceMonitor()

const spec = computed(() => getSettingSpec(props.settingKey))
const settingsSection = computed(
  () => SETTINGS_GROUPS.find((group) => group.keys.some((key) => key === props.settingKey))?.section,
)

/* ── 绑定表:声明各键「读哪、写哪」;控件形态由注册表声明 ─────────────── */

const toggleBindings: Record<string, { get: () => boolean; set: (v: boolean) => void }> = {
  galleryBlendEnabled: {
    get: () => config.galleryBlendEnabled,
    set: (v) => void config.setGalleryBlendEnabled(v),
  },
  hoverScale: {
    get: () => config.enableHoverScale,
    set: (v) => void config.setEnableHoverScale(v),
  },
  hoverAutoplay: { get: () => ui.hoverAutoplay, set: (v) => ui.setHoverAutoplay(v) },
  showThumbInfo: { get: () => ui.showThumbInfo, set: (v) => ui.setShowThumbInfo(v) },
  showDragHandle: { get: () => ui.showDragHandle, set: (v) => ui.setShowDragHandle(v) },
  autoHideChromeWindowed: {
    get: () => ui.autoHideChromeWindowed,
    set: (v) => ui.setAutoHideChromeWindowed(v),
  },
  enableVideoCover: {
    get: () => config.enableVideoCover,
    set: (v) => void config.setEnableVideoCover(v),
  },
  enableVideoKeyframes: {
    get: () => config.enableVideoKeyframes,
    set: (v) => void config.setEnableVideoKeyframes(v),
  },
  aiHqCache: { get: () => config.aiHqCache, set: (v) => void config.setAiHqCache(v) },
  // 标题栏合并/分离:经 useTitlebarMode 共享单例读写(非 config store);setter 提交到中央设置服务。
  titlebarMerged: {
    get: () => titlebarMode.merged.value,
    set: (v) => titlebarMode.setMerged(v),
  },
  // 选区操作条停靠/浮动:经 useSelectionBarMode 共享单例读写;setter 提交到中央设置服务。
  selectionBarDocked: {
    get: () => selectionBarMode.docked.value,
    set: (v) => selectionBarMode.setDocked(v),
  },
}

const selectBindings: Record<string, { get: () => string; set: (v: string) => void }> = {
  theme: {
    get: () => theme.appearance,
    set: (v) => theme.setAppearance(v as typeof theme.appearance),
  },
  language: { get: () => ui.language, set: (v) => ui.setLanguage(v) },
  closeBehavior: {
    get: () => ui.closeBehavior,
    set: (v) => ui.setCloseBehavior(v as typeof ui.closeBehavior),
  },
  minimapRenderMode: {
    get: () => ui.minimapRenderMode,
    set: (v) => ui.setMinimapRenderMode(v as MinimapRenderMode),
  },
  thumbDecodeStrategy: {
    get: () => config.thumbStrategy,
    set: (v) => void config.setThumbStrategy(v as typeof config.thumbStrategy),
  },
  aiHardwareStrategy: {
    get: () => config.aiProviderOverride,
    set: (v) => void config.setAiProviderOverride(v as typeof config.aiProviderOverride),
  },
  logLevel: {
    get: () => config.logLevel,
    set: (v) => void config.setLogLevel(v as typeof config.logLevel),
  },
  // 查看器渲染色域(方案 B §0⑤):setter 不调 restartDerivation(非派生流水线 kind,见 configStore)。
  viewerColorTarget: {
    get: () => config.viewerColorTarget,
    set: (v) => void config.setViewerColorTarget(v),
  },
  // 两栏水平对齐:经各自单例读写(非 config store);setter 提交到中央设置服务。三态 center/left/right。
  toolbarAlign: {
    get: () => toolbarAlign.align.value,
    set: (v) => toolbarAlign.setAlign(v as BarAlign),
  },
  selectionBarAlign: {
    get: () => selectionBarMode.align.value,
    set: (v) => selectionBarMode.setAlign(v as BarAlign),
  },
  // 实验性渲染引擎:经 useRenderMode 共享单例读写(非 config store);setter 提交到中央设置服务。
  galleryRenderMode: {
    get: () => renderMode.galleryRenderMode.value,
    set: (v) => renderMode.setGalleryRenderMode(v as RenderMode),
  },
  timelineRenderMode: {
    get: () => renderMode.timelineRenderMode.value,
    set: (v) => renderMode.setTimelineRenderMode(v as RenderMode),
  },
}

const numberBindings: Record<string, { get: () => number; set: (v: number) => void }> = {
  galleryBlendEdgeWidth: {
    get: () => config.galleryBlendEdgeWidth,
    set: (v) => void config.setGalleryBlendEdgeWidth(v),
  },
  galleryBlendBlurRadius: {
    get: () => config.galleryBlendBlurRadius,
    set: (v) => void config.setGalleryBlendBlurRadius(v),
  },
  galleryBlendStrength: {
    get: () => config.galleryBlendStrength,
    set: (v) => void config.setGalleryBlendStrength(v),
  },
  uiFontSize: { get: () => config.uiFontSize, set: (v) => void config.setUiFontSize(v) },
  // 跳过阈值变更须同步失效布局(缩略图形态随之改变)。
  thumbSkipMaxKb: {
    get: () => config.thumbSkipMaxKb,
    set: (v) => {
      void config.setThumbSkipMaxKb(v)
      media.invalidateLayout()
    },
  },
  thumbCacheMaxMb: {
    get: () => config.thumbCacheMaxMb,
    set: (v) => void config.setThumbCacheMaxMb(v),
  },
  // 编码质量(100=无损):后端复位存量项后 data_version 已 bump,布局出口自会取到新状态。
  thumbWebpQuality: {
    get: () => config.thumbWebpQuality,
    set: (v) => void config.setThumbWebpQuality(v),
  },
  timelineScrollWidth: {
    get: () => config.timelineScrollWidth,
    set: (v) => void config.setTimelineScrollWidth(v),
  },
  timelineAxisWidth: {
    get: () => config.timelineAxisWidth,
    set: (v) => void config.setTimelineAxisWidth(v),
  },
  scrollThumbMinHeight: {
    get: () => config.scrollThumbMinHeight,
    set: (v) => void config.setScrollThumbMinHeight(v),
  },
  axisViewportOpacity: {
    get: () => config.axisViewportOpacity,
    set: (v) => void config.setAxisViewportOpacity(v),
  },
}

const hasToggleBinding = computed(() => !!toggleBindings[props.settingKey])
const hasSelectBinding = computed(() => !!selectBindings[props.settingKey])
const hasNumberBinding = computed(() => !!numberBindings[props.settingKey])

const toggleModel = computed<boolean>({
  get: () => toggleBindings[props.settingKey]?.get() ?? false,
  set: (v) => toggleBindings[props.settingKey]?.set(v),
})

const selectModel = computed<string>({
  get: () => selectBindings[props.settingKey]?.get() ?? '',
  set: (v) => selectBindings[props.settingKey]?.set(v),
})

// number 类本地缓冲:输入不触发提交,change 才写 store(并跟随 store 外部变更回同步)。
const numberLocal = ref<number | string>(0)
watch(
  () => numberBindings[props.settingKey]?.get(),
  (v) => {
    if (typeof v === 'number') numberLocal.value = v
  },
  { immediate: true },
)
function commitNumber() {
  const binding = numberBindings[props.settingKey]
  if (!binding) return
  // 数字框清空时 v-model.number 仍返回空字符串，不能让数值转换把它提交成 0。
  if (typeof numberLocal.value !== 'number' || !Number.isFinite(numberLocal.value)) {
    numberLocal.value = binding.get()
    return
  }
  // HTML min/max 不阻止手工超限输入；先同步本地缓冲，避免保存值不变时 watch 无法回填。
  numberLocal.value = Math.min(
    spec.value?.max ?? Infinity,
    Math.max(spec.value?.min ?? -Infinity, Math.round(numberLocal.value)),
  )
  binding.set(numberLocal.value)
}

/* ── 危险清理按钮表(icon=全尺寸按钮图标;compact 统一 RotateCcw)────── */

const dangerButtons: Record<string, { icon: Component; btnLabelKey: string; onClick: () => void }> =
  {
    clearDb: {
      icon: Database,
      btnLabelKey: 'settings.clearDbBtn',
      onClick: () => void handleClearDb(),
    },
    clearSettings: {
      icon: Paintbrush,
      btnLabelKey: 'settings.resetSettingsBtn',
      onClick: () => void handleClearSettings(),
    },
    clearAllThumbnails: {
      icon: RotateCcw,
      btnLabelKey: 'settings.clearAllThumbnailsBtn',
      onClick: () => void handleClearAllThumbnails(),
    },
    clearLogs: {
      icon: RotateCcw,
      btnLabelKey: 'settings.clearLogsBtn',
      onClick: () => void handleClearLogs(),
    },
  }
const dangerSpec = computed(() => dangerButtons[props.settingKey])

/* ── 普通动作按钮表(非破坏性 action;与危险表分离,视觉语义随归属自动正确)────── */

const plainButtons: Record<string, { icon: Component; btnLabelKey: string; onClick: () => void }> =
  {
    performancePanel: {
      icon: Gauge,
      btnLabelKey: 'settings.performancePanelBtn',
      onClick: performanceMonitor.openPanel,
    },
    clearBrowserCache: {
      icon: RotateCcw,
      btnLabelKey: 'settings.clearBrowserCacheBtn',
      onClick: handleClearBrowserCache,
    },
    openLogWindow: {
      icon: Terminal,
      btnLabelKey: 'settings.openLogWindowBtn',
      onClick: () => void handleOpenLogWindow(),
    },
  }
const plainSpec = computed(() => plainButtons[props.settingKey])

/* ── AI 批大小特例(钳制 + 提示)──────────────────────────────────── */

const aiBatchSizeLocal = ref(config.aiBatchSize)
watch(
  () => config.aiBatchSize,
  (v) => (aiBatchSizeLocal.value = v),
)

// AI 批处理大小变更:固定 batch 模型下,非自动(0)的值不得小于其固定 k —— 自动钳制并提示。
// 0=自动 仍允许(后端会把有效 batch 抬到 ≥k)。
function onBatchChange() {
  const k = ai.status.activeFixedBatch
  if (k && aiBatchSizeLocal.value > 0 && aiBatchSizeLocal.value < k) {
    aiBatchSizeLocal.value = k
    toast.addToast('warning', t('settings.aiBatchAdjustedToFixed', { k }))
  }
  void config.setAiBatchSize(aiBatchSizeLocal.value)
}

function getTierLabel(tier: number): string {
  const labels: Record<number, string> = {
    64: t('settings.thumbTierXS'),
    128: t('settings.thumbTierS'),
    256: t('settings.thumbTierM'),
    512: t('settings.thumbTierL'),
    1024: t('settings.thumbTierXL'),
  }
  return labels[tier] ?? `${tier}px`
}

/* 危险清理操作。 */

async function handleClearDb() {
  // 清库=极度危险且不可恢复,走「输入确认」强门(requireText):须原样键入库名关键词才放行,
  // 杜绝反射式点击穿透。仍经全局 ConfirmDialog(非原生 confirm)。
  const { confirmed } = await confirm({
    title: t('settings.clearDbBtn'),
    message: t('sidebar.clearDbConfirm'),
    danger: true,
    requireText: t('settings.clearDbConfirmWord'),
    confirmText: t('settings.clearDbBtn'),
  })
  if (!confirmed) return
  try {
    await scan.clearDatabase()
    media.loadStats()
    toast.addToast('success', t('sidebar.clearDbSuccess'))
  } catch (e) {
    toast.addToast('error', t('sidebar.clearDbFailed', { error: e }))
  }
}

async function handleClearSettings() {
  const { confirmed } = await confirm({
    title: t('settings.resetSettingsConfirmTitle'),
    message: t('settings.resetSettingsConfirmMessage'),
    danger: true,
    confirmText: t('settings.resetSettingsBtn'),
  })
  if (!confirmed) return
  // 重置只刷新设置:资产、阅读进度、任务状态与引导标记都留在各自业务表里,不重放首启向导。
  try {
    const change = await resetSettings()
    // 需重启才生效的项如实告知(热应用项已成默认值并即刻生效)。
    if (change.restart_required.length > 0) {
      toast.addToast(
        'info',
        t('settings.resetSettingsRestartRequired', { keys: change.restart_required.join('、') }),
        6000,
      )
    } else {
      toast.addToast('success', t('settings.resetSettingsSuccess'))
    }
  } catch (e) {
    // 失败提示由中央服务统一发出(含「保留重置前最后确认值」),此处不再重复弹一条。
    logger.error('reset settings failed', { error: e })
  }
}

async function handleClearLogs() {
  // 日志删除不可恢复;与同区其它项一致走全局 ConfirmDialog(原生 window.confirm 在 Tauri webview
  // 不可靠——不弹框却返回 truthy,危险操作会无提示直接执行)。
  const { confirmed } = await confirm({
    title: t('settings.clearLogsBtn'),
    message: t('settings.clearLogsConfirm'),
    danger: true,
  })
  if (!confirmed) return
  try {
    await invokeIpc(IPC.CLEAR_LOGS)
    toast.addToast('success', t('settings.clearLogsSuccess'))
  } catch (e) {
    toast.addToast('error', t('settings.clearLogsFailed', { error: e }))
  }
}

async function handleClearAllThumbnails() {
  const { confirmed } = await confirm({
    title: t('settings.clearAllThumbnailsBtn'),
    message: t('sidebar.clearThumbnailsConfirm'),
    danger: true,
  })
  if (!confirmed) return
  try {
    await invokeIpc(IPC.CLEAR_ALL_THUMBNAILS)
    media.invalidateLayout()
    toast.addToast('success', t('sidebar.clearThumbnailsSuccess'))
  } catch (e) {
    toast.addToast('error', t('sidebar.clearThumbnailsFailed', { error: e }))
  }
}

function handleClearBrowserCache() {
  // 「清浏览器缓存」语义是纯前端:带 cache-busting 查询串重载,绕过 webview 已缓存的图片。
  window.location.href = window.location.pathname + '?clear=' + Date.now()
}

async function handleOpenLogWindow() {
  // 建窗或聚焦既有日志窗口由后端 open_log_window 处理，并更新环形缓冲
  // 订阅标志),前端只管调用 + 失败提示。
  try {
    await invokeIpc(IPC.OPEN_LOG_WINDOW)
  } catch (e) {
    toast.addToast('error', t('settings.openLogWindowFailed', { error: e }))
  }
}
</script>

<style scoped>
.number-control {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  min-width: 0;
  max-width: 100%;
}
.number-control__unit {
  flex-shrink: 0;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}
.compact .number-control {
  flex-wrap: wrap;
}
/* ── Segmented Control ─────────────────────────────────────────────────── */
.segmented-control {
  display: inline-flex;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  overflow: hidden;
}
.segmented-btn {
  padding: 8px 16px;
  font-size: 13px;
  background: transparent;
  color: var(--color-text-secondary);
  border: none;
  border-right: 1px solid var(--color-border);
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.segmented-btn:last-child {
  border-right: none;
}
.segmented-btn:hover {
  background: var(--color-bg-hover);
}
.segmented-btn.active {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}

.dynamic-control {
  display: flex;
  align-items: center;
  justify-content: flex-end;
}

.dynamic-control.compact {
  justify-content: flex-start;
  min-width: 0;
  width: 100%;
}

.compact-select-wrap {
  width: 100%;
  min-width: 0;
  max-width: 100%;
}
/* select 迁入 UiSelect 后携子组件 data-v(非本组件),故后代选择器须经 :deep() 穿透子组件边界重锚——
   否则 `.compact-select-wrap[data-v-本] .select[data-v-本]` 因内层 data-v 失配,紧凑面板 select 尺寸静默退回默认。
   .compact-select-wrap 类经 fallthrough 落在 UiSelect 根(仍带父 data-v),故左侧锚点不变、仅去 .select 的 data-v 约束。 */
.compact-select-wrap :deep(.select) {
  padding: 0 28px 0 var(--spacing-sm);
  font-size: var(--font-size-xs);
  height: var(--control-size-compact);
  min-height: var(--control-size-compact);
  min-width: 0;
  width: 100%;
  text-overflow: ellipsis;
}

.compact-input {
  width: 104px;
  min-width: 0;
  max-width: 100%;
  padding: 0 var(--spacing-sm);
  font-size: var(--font-size-xs);
  height: var(--control-size-compact);
  min-height: var(--control-size-compact);
}

.danger-icon {
  color: var(--color-error);
}

/* AI 批大小提示。 */
.batch-size-stack {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}
.compact .batch-size-stack {
  align-items: flex-start;
  min-width: 0;
  width: 100%;
}
.batch-hint {
  font-size: 11px;
  white-space: nowrap;
}
.compact .batch-hint {
  max-width: 100%;
  white-space: normal;
  overflow-wrap: anywhere;
}
.batch-hint--ok {
  color: var(--color-success);
}
.batch-hint--error {
  color: var(--color-error);
}
.batch-hint--warn {
  color: var(--color-warning);
}
.batch-hint--muted {
  color: var(--color-text-tertiary);
}

.unsupported-hint {
  font-size: 12px;
  color: var(--color-text-tertiary);
  overflow-wrap: anywhere;
}
.btn.related-settings-button {
  max-width: 100%;
  height: auto;
  padding-block: var(--spacing-xs);
  white-space: normal;
  line-height: 1.4;
  text-align: left;
}
</style>
