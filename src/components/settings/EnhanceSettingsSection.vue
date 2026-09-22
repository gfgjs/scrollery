<!-- 影像增强模型分节（降噪/超分子系统 P0 批 5）：模型卡片列表（名称/任务标签/体量占位/下载进度/
     下载/删除）。模板照 OcrModelSection.vue（固定模型组下载形态）。未授权态分节仍显示——模型下载
     免费，enhance_start 才验 license（D-OCR-6 同型）；顶部小字提示授权态 + 商店链接。 -->
<template>
  <CollapsibleCard id="enhanceModels" :title="$t('settings.enhanceTitle')">
    <!-- 队列进度（批 5.5）：提交后即使关闭对话框，此处仍可见结果——终态另有 toast 即时反馈。
         面板内部按 queue 是否非空自行显隐；本处始终挂载以维持 subscribeQueue 订阅。 -->
    <EnhanceQueuePanel class="enh-models__queue" />
    <p class="enh-models__hint">{{ $t('settings.enhanceHint') }}</p>

    <!-- 授权态提示：未授权也可预下载模型，enhance_start 才验 license（D-OCR-6）。 -->
    <div
      v-if="status?.models.some((model) => model.readiness === 'unlicensed')"
      class="enh-models__auth"
    >
      {{
        status?.availability === 'licenseExpired'
          ? $t('enhance.errUnlicensed')
          : $t('enhance.gateUnlicensed')
      }}
      <a
        v-if="status?.storeUrl"
        :href="status.storeUrl"
        target="_blank"
        rel="noopener noreferrer"
        class="enh-models__auth-link"
        >{{ $t('store.builtinTitle') }}</a
      >
    </div>

    <!-- 逐档判定：任一档清单未就绪即提示（URL 待回填）。 -->
    <div v-if="!status" class="enh-models__unready">
      {{ $t('enhance.errStatusUnavailable') }}
    </div>
    <div v-else-if="!status.workerReady" class="enh-models__unready">
      {{ $t('enhance.errWorkerMissing') }}
    </div>
    <div v-if="anyManifestUnready" class="enh-models__unready">
      {{ $t('settings.enhanceManifestUnready') }}
    </div>

    <div v-for="model in status?.models ?? []" :key="model.id" class="enh-model">
      <div class="enh-model__head">
        <span class="enh-model__name">{{ model.id }}</span>
        <span class="enh-model__tag">{{ taskLabel(model.task) }}</span>
      </div>
      <div class="enh-model__meta">
        <!-- 体量占位：后端 EnhanceModelDto 暂无 size 字段（资产清单 URL 待批 6 回填）。 -->
        <span>{{ $t('settings.enhanceModelSizePlaceholder') }}</span>
      </div>
      <div class="enh-model__status">
        <span v-if="model.installed" class="status--installed">{{
          $t('settings.fmFilesReady')
        }}</span>
        <span v-else class="status--missing">{{ $t('settings.fmNotInstalled') }}</span>

        <template v-if="!model.installed">
          <button
            v-if="!downloading[model.id]"
            class="enh-model__dl-btn"
            :disabled="!model.canDownload"
            @click="download(model.id)"
          >
            {{ $t('settings.enhanceDownload') }}
          </button>
          <span v-else class="enh-model__dl-progress">
            <span class="enh-model__dl-bar">
              <span class="enh-model__dl-fill" :style="{ width: pct(model.id) + '%' }"></span>
            </span>
            <span class="enh-model__dl-pct">{{ pct(model.id) }}%</span>
          </span>
        </template>
        <button
          v-else
          class="enh-model__del-btn"
          :disabled="deleting[model.id]"
          @click="remove(model.id)"
        >
          {{ $t('settings.enhanceDelete') }}
        </button>
      </div>
      <div v-if="errors[model.id]" class="enh-model__err">
        {{ $t('settings.enhanceDownloadFailed', { error: errors[model.id] }) }}
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { reactive, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import CollapsibleCard from './CollapsibleCard.vue'
import EnhanceQueuePanel from '../enhance/EnhanceQueuePanel.vue'
import { useEnhanceStore } from '../../stores/enhanceStore'
import { useToastStore } from '../../stores/toastStore'
import { ipcErrorMessage } from '../../utils/ipc'
import type { EnhanceModelTask, ModelDownloadProgress } from '../../types/enhance'

const { t } = useI18n()
const enhance = useEnhanceStore()
const toast = useToastStore()
const { status } = storeToRefs(enhance)

const downloading = reactive<Record<string, boolean>>({})
const deleting = reactive<Record<string, boolean>>({})
const progress = reactive<Record<string, { received: number; total: number }>>({})
const errors = reactive<Record<string, string>>({})

const anyManifestUnready = computed(
  () => status.value?.models.some((m) => !m.manifestReady) ?? false,
)

const TASK_LABEL_KEY: Record<EnhanceModelTask, string> = {
  denoise: 'settings.enhanceTaskDenoise',
  dejpegArtifact: 'settings.enhanceTaskDejpeg',
  upscale: 'settings.enhanceTaskUpscale',
}
function taskLabel(task: EnhanceModelTask): string {
  return t(TASK_LABEL_KEY[task])
}

function pct(id: string): number {
  const p = progress[id]
  if (!p || p.total === 0) return 0
  return Math.min(100, Math.round((p.received / p.total) * 100))
}

async function download(modelId: string) {
  downloading[modelId] = true
  errors[modelId] = ''
  progress[modelId] = { received: 0, total: 0 }
  try {
    await enhance.downloadModel(modelId, (ev: ModelDownloadProgress) => {
      progress[modelId] = { received: ev.received, total: ev.total }
      if (ev.error) errors[modelId] = ev.error
    })
    toast.addToast('success', t('settings.enhanceDownloadComplete'))
    await enhance.fetchStatus()
  } catch (e) {
    errors[modelId] = ipcErrorMessage(e)
    toast.addToast('error', t('settings.enhanceDownloadFailed', { error: ipcErrorMessage(e) }))
  } finally {
    downloading[modelId] = false
  }
}

async function remove(modelId: string) {
  if (deleting[modelId]) return
  deleting[modelId] = true
  try {
    await enhance.deleteModel(modelId)
    await enhance.fetchStatus()
  } catch (e) {
    toast.addToast('error', ipcErrorMessage(e))
  } finally {
    deleting[modelId] = false
  }
}

onMounted(async () => {
  await enhance.fetchStatus()
})
</script>

<style scoped>
.enh-models__queue {
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.enh-models__hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0 0 var(--spacing-sm);
  padding: 0 var(--spacing-lg);
  line-height: 1.6;
}
.enh-models__auth {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.enh-models__auth-link {
  margin-left: 6px;
  color: var(--color-accent);
}
.enh-models__unready {
  margin: var(--spacing-sm) var(--spacing-lg);
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-warning) 14%, transparent);
  color: var(--color-warning);
  font-size: var(--font-size-xs);
  line-height: 1.6;
}
.enh-model {
  padding: var(--spacing-md);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.enh-model__head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}
.enh-model__name {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.enh-model__tag {
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-accent) 16%, transparent);
  color: var(--color-accent);
}
.enh-model__meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-bottom: 4px;
}
.enh-model__status {
  font-size: var(--font-size-xs);
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.status--installed {
  color: var(--color-success);
}
.status--missing {
  color: var(--color-text-tertiary);
}
.enh-model__dl-btn,
.enh-model__del-btn {
  padding: 2px 10px;
  font-size: var(--font-size-xs);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-accent);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}
.enh-model__del-btn {
  border-color: color-mix(in srgb, var(--color-error) 55%, var(--color-border));
  color: var(--color-error);
}
.enh-model__dl-btn:hover {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.enh-model__del-btn:hover {
  background: var(--color-error);
  color: var(--color-text-on-error);
}
.enh-model__dl-btn:disabled,
.enh-model__del-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.enh-model__dl-progress {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1;
}
.enh-model__dl-bar {
  flex: 1;
  height: 4px;
  max-width: 160px;
  background: var(--color-bg-primary);
  border-radius: 2px;
  overflow: hidden;
}
.enh-model__dl-fill {
  display: block;
  height: 100%;
  background: var(--color-accent);
  transition: width var(--transition-fast);
}
.enh-model__dl-pct {
  color: var(--color-text-secondary);
  font-variant-numeric: tabular-nums;
}
.enh-model__err {
  margin-top: 4px;
  font-size: var(--font-size-xs);
  color: var(--color-error);
}
</style>
