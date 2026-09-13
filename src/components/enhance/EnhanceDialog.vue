<!-- 影像增强对话框（降噪/超分子系统 P0 批 5，design.md §F）：任务勾选 + 每任务模型档 + 强度滑杆 +
     前后对比预览 + 输出格式/尺寸预估 + 门控引导。执行序固定 降噪→去伪影→超分（host 排序契约），
     UI 明示该固定序。授权/模型下载门控走 D-OCR-6 同型：按钮常显，点击后按态引导（未授权→插件商店、
     未下载→设置分节）。预览 P0 后端返 enhance_not_implemented → 分割拖杆显示占位态，不阻塞主流程。 -->
<template>
  <UiDialog
    :open="open"
    :title="$t('enhance.dialogTitle')"
    :close-label="$t('common.close')"
    max-width="620px"
    max-height="86vh"
    @close="emit('close')"
  >
    <div class="enh">
      <!-- 固定执行序提示。 -->
      <p class="enh__order-hint">{{ $t('enhance.orderHint') }}</p>

      <!-- 队列进度（批 5.5）：提交后本对话框可留看进度；关闭对话框也不丢——设置分节面板同款
           数据继续可见，终态另有 toast 即时反馈（即使已关闭对话框）。 -->
      <EnhanceQueuePanel />

      <!-- ── 任务勾选 + 每任务模型档 ─────────────────────────────────────── -->
      <section class="enh__section">
        <h3 class="enh__section-title">{{ $t('enhance.tasksTitle') }}</h3>

        <!-- 降噪 -->
        <div class="enh__task">
          <label class="enh__task-head">
            <input type="checkbox" v-model="denoise" />
            <span>{{ $t('enhance.taskDenoise') }}</span>
          </label>
          <select v-if="denoise" v-model="denoiseModel" class="enh__select">
            <option v-for="m in denoiseModels" :key="m.id" :value="m.id">
              {{ modelLabel(m) }}
            </option>
          </select>
        </div>
        <!-- DRUNet σ：仅选 drunet 档时显示。 -->
        <div v-if="denoise && denoiseModel === 'drunet'" class="enh__slider">
          <span class="enh__slider-label">{{ $t('enhance.sigmaLabel') }}</span>
          <input type="range" min="0" max="50" step="1" v-model.number="sigma" />
          <span class="enh__slider-val">{{ sigma }}</span>
        </div>

        <!-- 去 JPEG 伪影 -->
        <div class="enh__task">
          <label class="enh__task-head">
            <input type="checkbox" v-model="dejpeg" />
            <span>{{ $t('enhance.taskDejpeg') }}</span>
          </label>
          <select v-if="dejpeg" v-model="dejpegModel" class="enh__select">
            <option v-for="m in dejpegModels" :key="m.id" :value="m.id">
              {{ modelLabel(m) }}
            </option>
          </select>
        </div>
        <!-- FBCNN QF 强度：勾去伪影时显示。 -->
        <div v-if="dejpeg" class="enh__slider">
          <span class="enh__slider-label">{{ $t('enhance.qfLabel') }}</span>
          <input type="range" min="0" max="100" step="1" v-model.number="qf" />
          <span class="enh__slider-val">{{ qf }}</span>
        </div>

        <!-- 超分 -->
        <div class="enh__task">
          <label class="enh__task-head">
            <input type="checkbox" v-model="upscale" />
            <span>{{ $t('enhance.taskUpscale') }}</span>
          </label>
          <select v-if="upscale" v-model="upscaleModel" class="enh__select">
            <option v-for="m in upscaleModels" :key="m.id" :value="m.id">
              {{ modelLabel(m) }}
            </option>
          </select>
        </div>
      </section>

      <!-- ── 前后对比预览（P0 占位容错）─────────────────────────────────── -->
      <section class="enh__section">
        <div class="enh__preview-head">
          <h3 class="enh__section-title">{{ $t('enhance.previewTitle') }}</h3>
          <button class="enh__ghost-btn" :disabled="previewLoading || !canSubmit" @click="generatePreview">
            {{ $t('enhance.previewGenerate') }}
          </button>
        </div>
        <BeforeAfterSlider
          :before-src="beforeSrc"
          :after-src="afterSrc"
          :placeholder="previewPlaceholder"
        />
      </section>

      <!-- ── 输出 ───────────────────────────────────────────────────────── -->
      <section class="enh__section">
        <h3 class="enh__section-title">{{ $t('enhance.outputTitle') }}</h3>
        <div class="enh__output-row">
          <span class="enh__output-label">{{ $t('enhance.outputFormat') }}</span>
          <select v-model="outputFormat" class="enh__select">
            <option value="follow_source">{{ $t('enhance.formatFollowSource') }}</option>
            <option value="jpeg">JPEG</option>
            <option value="png">PNG</option>
          </select>
          <span class="enh__output-fmt">{{ resolvedFormatLabel }}</span>
        </div>
        <p v-if="outputSizeText" class="enh__output-size">{{ outputSizeText }}</p>
        <p class="enh__persist-note">{{ $t('enhance.paramsMemoryNote') }}</p>
      </section>
    </div>

    <template #footer>
      <button class="enh__ghost-btn" @click="emit('close')">{{ $t('common.cancel') }}</button>
      <button class="enh__primary-btn" :disabled="!canSubmit || submitting" @click="submit">
        {{ multi ? $t('enhance.startMulti', { n: itemCount }) : $t('enhance.start') }}
      </button>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import UiDialog from '../ui/UiDialog.vue'
import BeforeAfterSlider from './BeforeAfterSlider.vue'
import EnhanceQueuePanel from './EnhanceQueuePanel.vue'
import {
  useEnhanceStore,
  ENHANCE_DEFAULT_MODEL,
  modelTaskToStepTask,
  enhanceErrorMessageKey,
} from '../../stores/enhanceStore'
import { useToastStore } from '../../stores/toastStore'
import { ipcErrorMessage, IpcError } from '../../utils/ipc'
import { resolveAssetUrl } from '../../utils/assetUrl'
import { logger } from '../../utils/logger'
import type {
  EnhanceSource,
  EnhanceModelInfo,
  EnhanceModelTask,
  EnhanceOutputFormat,
  EnhanceParams,
  EnhanceStepInput,
} from '../../types/enhance'

const props = defineProps<{
  open: boolean
  source: EnhanceSource | null
}>()
const emit = defineEmits<{ (e: 'close'): void }>()

const { t } = useI18n()
const router = useRouter()
const enhance = useEnhanceStore()
const toast = useToastStore()

// ── 任务勾选 + 模型选择 + 强度 ──────────────────────────────────────────────
const denoise = ref(false)
const dejpeg = ref(false)
const upscale = ref(false)
const denoiseModel = ref(ENHANCE_DEFAULT_MODEL.denoise)
const dejpegModel = ref(ENHANCE_DEFAULT_MODEL.dejpegArtifact)
const upscaleModel = ref(ENHANCE_DEFAULT_MODEL.upscale)
const sigma = ref(15)
const qf = ref(50)
const outputFormat = ref<EnhanceOutputFormat>('follow_source')

const submitting = ref(false)

// ── 预览态（P0 数据源容错）───────────────────────────────────────────────────
const beforeSrc = ref<string | null>(null)
const afterSrc = ref<string | null>(null)
const previewLoading = ref(false)
const previewPlaceholder = ref('')
let previewRevision = 0

// ── 派生 ────────────────────────────────────────────────────────────────────
const denoiseModels = computed(() => enhance.modelsForTask('denoise'))
const dejpegModels = computed(() => enhance.modelsForTask('dejpegArtifact'))
const upscaleModels = computed(() => enhance.modelsForTask('upscale'))

const itemCount = computed(() => props.source?.itemIds.length ?? 0)
const multi = computed(() => itemCount.value > 1)

const canSubmit = computed(
  () => (denoise.value || dejpeg.value || upscale.value) && itemCount.value > 0,
)

/** 选中超分档的 scale（未选超分=1）。 */
const outputScale = computed(() => {
  if (!upscale.value) return 1
  return enhance.modelById(upscaleModel.value)?.scale ?? 1
})

const outputSizeText = computed(() => {
  const w = props.source?.width
  const h = props.source?.height
  if (!w || !h) return ''
  const s = outputScale.value
  return t('enhance.outputSize', { w: w * s, h: h * s })
})

/** 输出格式跟随源/指定的展示文案。 */
const resolvedFormatLabel = computed(() => {
  if (outputFormat.value === 'jpeg') return 'JPEG'
  if (outputFormat.value === 'png') return 'PNG'
  const ext = props.source?.ext ?? ''
  return ext === 'png' ? 'PNG' : 'JPEG'
})

function modelLabel(m: EnhanceModelInfo): string {
  return m.installed ? m.id : `${m.id} · ${t('enhance.modelNotDownloaded')}`
}

// ── 参数构造 ────────────────────────────────────────────────────────────────
function buildParams(): EnhanceParams {
  const steps: EnhanceStepInput[] = []
  if (denoise.value) {
    steps.push({
      task: modelTaskToStepTask('denoise'),
      model_id: denoiseModel.value,
      // DRUNet 需 σ；SCUNet 无强度参数（用模型默认）。
      strength: denoiseModel.value === 'drunet' ? sigma.value : undefined,
    })
  }
  if (dejpeg.value) {
    steps.push({
      task: modelTaskToStepTask('dejpegArtifact'),
      model_id: dejpegModel.value,
      strength: qf.value,
    })
  }
  if (upscale.value) {
    steps.push({ task: modelTaskToStepTask('upscale'), model_id: upscaleModel.value })
  }
  return { steps, outputFormat: outputFormat.value }
}

/** 选中任务对应模型是否都已下载（门控 UX 用；后端 enhance_start 亦有真门兜底）。 */
function selectedModelsInstalled(): boolean {
  const ids: string[] = []
  if (denoise.value) ids.push(denoiseModel.value)
  if (dejpeg.value) ids.push(dejpegModel.value)
  if (upscale.value) ids.push(upscaleModel.value)
  return ids.every((id) => enhance.modelById(id)?.installed)
}

// ── 「自动」规则映射建议（源为 JPEG 预勾去伪影、小图预勾超分；无源信息则不建议）──────
const SMALL_IMAGE_PIXELS = 2_000_000
function applyDefaultsAndSuggestion() {
  denoise.value = true // 降噪为通用默认，保证对话框非空。
  dejpeg.value = false
  upscale.value = false
  const src = props.source
  if (src?.ext === 'jpg' || src?.ext === 'jpeg') dejpeg.value = true
  if (src?.width && src?.height && src.width * src.height < SMALL_IMAGE_PIXELS) {
    upscale.value = true
  }
}

/** 把选档钉到「默认档在场则默认档，否则该任务首个可用档」。 */
function ensureModelSelected(
  current: string,
  task: EnhanceModelTask,
  fallback: string,
): string {
  const list = enhance.modelsForTask(task)
  if (list.some((m) => m.id === current)) return current
  if (list.some((m) => m.id === fallback)) return fallback
  return list[0]?.id ?? current
}

function hydrateSelections() {
  const last = enhance.lastParams
  if (last) {
    // 参数记忆：从上次快照还原勾选/档位/强度。
    denoise.value = false
    dejpeg.value = false
    upscale.value = false
    for (const step of last.steps) {
      if (step.task === 'denoise') {
        denoise.value = true
        denoiseModel.value = step.model_id
        if (step.strength != null) sigma.value = step.strength
      } else if (step.task === 'dejpeg_artifact') {
        dejpeg.value = true
        dejpegModel.value = step.model_id
        if (step.strength != null) qf.value = step.strength
      } else if (step.task === 'upscale') {
        upscale.value = true
        upscaleModel.value = step.model_id
      }
    }
    outputFormat.value = last.outputFormat
  } else {
    applyDefaultsAndSuggestion()
  }
  denoiseModel.value = ensureModelSelected(denoiseModel.value, 'denoise', ENHANCE_DEFAULT_MODEL.denoise)
  dejpegModel.value = ensureModelSelected(
    dejpegModel.value,
    'dejpegArtifact',
    ENHANCE_DEFAULT_MODEL.dejpegArtifact,
  )
  upscaleModel.value = ensureModelSelected(upscaleModel.value, 'upscale', ENHANCE_DEFAULT_MODEL.upscale)
}

// ── 打开时：拉状态 + 复位预览 + 水合选择 ──────────────────────────────────────
watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) return
    beforeSrc.value = null
    afterSrc.value = null
    previewPlaceholder.value = ''
    await enhance.fetchStatus()
    hydrateSelections()
  },
)

// ── 预览──────────────────────────────────────────────────────────────────────
async function generatePreview() {
  const src = props.source
  if (!src || !canSubmit.value || previewLoading.value) return
  previewLoading.value = true
  previewPlaceholder.value = ''
  try {
    const res = await enhance.preview(src.itemIds[0], buildParams())
    // 后端固定名原子覆盖；每轮加版本参数，避免 WebView 沿用上一轮 asset 缓存。
    const revision = ++previewRevision
    beforeSrc.value = `${resolveAssetUrl(res.beforePath)}?v=${revision}`
    afterSrc.value = `${resolveAssetUrl(res.afterPath)}?v=${revision}`
  } catch (e) {
    // P0：后端返 enhance_not_implemented → 显示占位，不当错误弹。其余错误也降级为占位提示。
    const code = e instanceof IpcError ? e.code : null
    beforeSrc.value = null
    afterSrc.value = null
    previewPlaceholder.value =
      code === 'enhance_not_implemented'
        ? t('enhance.previewComingSoon')
        : t('enhance.previewFailed')
    logger.warn('[Enhance] 预览不可用 | preview unavailable', { error: e, code })
  } finally {
    previewLoading.value = false
  }
}

// ── 提交 ────────────────────────────────────────────────────────────────────
async function submit() {
  const src = props.source
  if (!src || !canSubmit.value || submitting.value) return

  // 门控 UX（后端亦有真门）：未授权 → 插件商店；模型未下载 → 设置分节。
  if (!enhance.isAuthorized) {
    const expired = enhance.status?.availability === 'licenseExpired'
    toast.addToast('info', expired ? t('enhance.errUnlicensed') : t('enhance.gateUnlicensed'))
    void router.push('/plugins')
    return
  }
  if (!selectedModelsInstalled()) {
    toast.addToast('info', t('enhance.gateModelMissing'))
    void router.push('/settings')
    return
  }

  submitting.value = true
  try {
    await enhance.start(src.itemIds, buildParams())
    toast.addToast('success', multi.value ? t('enhance.startedMulti', { n: itemCount.value }) : t('enhance.started'))
    emit('close')
  } catch (e) {
    const code = e instanceof IpcError ? e.code : null
    if (code === 'enhance_unlicensed') {
      toast.addToast('info', t('enhance.errUnlicensed'))
      void router.push('/plugins')
    } else if (code === 'enhance_model_missing') {
      toast.addToast('info', t('enhance.gateModelMissing'))
      void router.push('/settings')
    } else {
      toast.addToast('error', code ? t(enhanceErrorMessageKey(code)) : ipcErrorMessage(e))
    }
  } finally {
    submitting.value = false
  }
}
</script>

<style scoped>
.enh {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-lg);
}
.enh__order-hint {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  padding: var(--spacing-sm) var(--spacing-md);
  border-radius: var(--radius-sm);
  background: var(--color-accent-subtle);
}
.enh__section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}
.enh__section-title {
  margin: 0;
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.enh__task {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-md);
}
.enh__task-head {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  cursor: pointer;
}
.enh__select {
  min-width: 200px;
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
}
.enh__slider {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding-left: var(--spacing-xl);
}
.enh__slider-label {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  min-width: 96px;
}
.enh__slider input[type='range'] {
  flex: 1;
}
.enh__slider-val {
  font-size: var(--font-size-xs);
  color: var(--color-text-primary);
  font-variant-numeric: tabular-nums;
  min-width: 28px;
  text-align: right;
}
.enh__preview-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.enh__output-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.enh__output-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
.enh__output-fmt {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.enh__output-size {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.enh__persist-note {
  margin: 0;
  font-size: var(--font-size-2xs);
  color: var(--color-text-tertiary);
}
.enh__ghost-btn,
.enh__primary-btn {
  height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast),
    border-color var(--transition-fast);
}
.enh__ghost-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text-secondary);
}
.enh__ghost-btn:hover:not(:disabled) {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.enh__primary-btn {
  background: var(--color-accent);
  border: 1px solid var(--color-accent);
  color: var(--color-text-on-accent);
}
.enh__primary-btn:hover:not(:disabled) {
  background: var(--color-accent-hover);
}
.enh__ghost-btn:disabled,
.enh__primary-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
