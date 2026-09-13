<!-- OCR 模型分节（B′ 路线，T10）：预下载两档 + 档位切换。模板仿 FaceModelLibrary.vue
     （固定小模型组下载形态），进度/下载调用姿态照 ModelLibrary.vue:260-284。
     未授权态分节仍显示（D-OCR-6：模型下载免费，识别命令才验 license）；顶部小字提示授权态 + 商店链接。 -->
<template>
  <CollapsibleCard id="ocrModels" :title="$t('settings.ocrTitle')">
    <p class="ocr-models__hint">{{ $t('settings.ocrHint') }}</p>

    <!-- 授权态提示：未授权也可预下载模型,识别命令才验 license(D-OCR-6)。文案复用 T9 已写入的
         ocr.unlicensed/ocr.licenseExpired 顶层 key(settings 节未新增专用 key,避免与 T9 独占的
         locale 文件产生写入冲突)。 -->
    <div v-if="status && status.availability !== 'authorized'" class="ocr-models__auth">
      {{
        status.availability === 'licenseExpired' ? $t('ocr.licenseExpired') : $t('ocr.unlicensed')
      }}
      <a
        v-if="status.storeUrl"
        :href="status.storeUrl"
        target="_blank"
        rel="noopener noreferrer"
        class="ocr-models__auth-link"
        >{{ $t('store.builtinTitle') }}</a
      >
    </div>

    <!-- 逐档判定(J14):任一档清单未就绪即提示,不再取单一全局标志。 -->
    <div v-if="anyManifestUnready" class="ocr-models__unready">
      {{ $t('settings.ocrManifestUnready') }}
    </div>

    <div
      v-for="tier in status?.tiers ?? []"
      :key="tier.id"
      class="ocr-model"
      :class="{ 'ocr-model--active': tier.id === config.ocrTier }"
    >
      <div class="ocr-model__head">
        <span class="ocr-model__name">{{ tierLabel(tier) }}</span>
        <span v-if="tier.id === config.ocrTier" class="ocr-model__badge badge--active">{{
          $t('settings.ocrActive')
        }}</span>
      </div>
      <div class="ocr-model__meta">
        <span>{{ $t('settings.ocrTierMeta', { mb: tier.sizeMb }) }}</span>
      </div>
      <div class="ocr-model__status">
        <span v-if="tier.installed" class="status--installed">{{
          $t('settings.fmFilesReady')
        }}</span>
        <span v-else class="status--missing">{{ $t('settings.fmNotInstalled') }}</span>

        <template v-if="!tier.installed">
          <button
            v-if="!downloading[tier.id]"
            class="ocr-model__dl-btn"
            :disabled="!tier.manifestReady"
            @click="download(tier)"
          >
            {{ $t('settings.ocrDownload') }}
          </button>
          <span v-else class="ocr-model__dl-progress">
            <span class="ocr-model__dl-bar">
              <span class="ocr-model__dl-fill" :style="{ width: pct(tier.id) + '%' }"></span>
            </span>
            <span class="ocr-model__dl-pct">{{ pct(tier.id) }}%</span>
          </span>
        </template>
        <button
          v-else-if="tier.id !== config.ocrTier"
          class="ocr-model__use-btn"
          :disabled="switching"
          @click="useTier(tier)"
        >
          {{ $t('settings.ocrUse') }}
        </button>
      </div>
      <div v-if="errors[tier.id]" class="ocr-model__err">
        {{ $t('settings.ocrDownloadFailed', { error: errors[tier.id] }) }}
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import CollapsibleCard from './CollapsibleCard.vue'
import { useConfigStore } from '../../stores/configStore'
import { useToastStore } from '../../stores/toastStore'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
// T9 并行在飞:resetOcrStatusCache 从 useOcr 导入。若 T9 尚未落地,该导入会使
// vue-tsc/构建报错——此为已知依赖态,非本卡缺陷(见任务卡 T10 回执)。
import { resetOcrStatusCache } from '../../composables/useOcr'
import type { OcrStatus, OcrTier } from '../../types/ocr'
import type { ModelDownloadProgress } from '../../types/ai'

const { t } = useI18n()
const config = useConfigStore()
const toast = useToastStore()

const status = ref<OcrStatus | null>(null)
const downloading = reactive<Record<string, boolean>>({})
const progress = reactive<Record<string, { received: number; total: number }>>({})
const errors = reactive<Record<string, string>>({})
const switching = ref(false)

/** 逐档判定(J14):任一档清单未就绪即提示——顶部横幅不再取单一全局标志。 */
const anyManifestUnready = computed(
  () => status.value?.tiers.some((tier) => !tier.manifestReady) ?? false,
)

function tierLabel(tier: OcrTier): string {
  if (tier.id === 'pp-ocrv5-server') return t('settings.ocrTierServer')
  if (tier.id === 'pp-ocrv5-mobile') return t('settings.ocrTierMobile')
  return tier.displayName
}

function pct(id: string): number {
  const p = progress[id]
  if (!p || p.total === 0) return 0
  return Math.min(100, Math.round((p.received / p.total) * 100))
}

async function refresh() {
  status.value = await invokeIpc<OcrStatus>(IPC.OCR_STATUS)
}

async function download(tier: OcrTier) {
  downloading[tier.id] = true
  errors[tier.id] = ''
  progress[tier.id] = { received: 0, total: tier.sizeMb * 1024 * 1024 }
  try {
    const ch = new Channel<ModelDownloadProgress>()
    ch.onmessage = (ev) => {
      progress[tier.id] = { received: ev.received, total: ev.total }
      if (ev.error) errors[tier.id] = ev.error
    }
    await invokeIpc(IPC.DOWNLOAD_OCR_MODELS, { tier: tier.id, onProgress: ch })
    toast.addToast('success', t('settings.ocrDownloadComplete'))
    resetOcrStatusCache()
    await refresh()
  } catch (e) {
    errors[tier.id] = ipcErrorMessage(e)
    toast.addToast('error', t('settings.ocrDownloadFailed'))
  } finally {
    downloading[tier.id] = false
  }
}

async function useTier(tier: OcrTier) {
  if (switching.value) return
  switching.value = true
  try {
    await config.setOcrTier(tier.id)
    resetOcrStatusCache()
    await refresh()
  } finally {
    switching.value = false
  }
}

onMounted(async () => {
  await config.loadConfig()
  await refresh()
})
</script>

<style scoped>
.ocr-models__hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0 0 var(--spacing-sm);
  padding: 0 var(--spacing-lg);
  line-height: 1.6;
}
.ocr-models__auth {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.ocr-models__auth-link {
  margin-left: 6px;
  color: var(--color-accent);
}
.ocr-models__unready {
  margin: var(--spacing-sm) var(--spacing-lg);
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-warning) 14%, transparent);
  color: var(--color-warning);
  font-size: var(--font-size-xs);
  line-height: 1.6;
}
.ocr-model {
  padding: var(--spacing-md);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.ocr-model--active {
  border-color: var(--color-accent);
}
.ocr-model__head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}
.ocr-model__name {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.ocr-model__badge {
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 7px;
  border-radius: 999px;
}
.badge--active {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.ocr-model__meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-bottom: 4px;
}
.ocr-model__status {
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
.ocr-model__dl-btn,
.ocr-model__use-btn {
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
.ocr-model__dl-btn:hover,
.ocr-model__use-btn:hover {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.ocr-model__dl-btn:disabled,
.ocr-model__use-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ocr-model__dl-progress {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1;
}
.ocr-model__dl-bar {
  flex: 1;
  height: 4px;
  max-width: 160px;
  background: var(--color-bg-primary);
  border-radius: 2px;
  overflow: hidden;
}
.ocr-model__dl-fill {
  display: block;
  height: 100%;
  background: var(--color-accent);
  transition: width var(--transition-fast);
}
.ocr-model__dl-pct {
  color: var(--color-text-secondary);
  font-variant-numeric: tabular-nums;
}
.ocr-model__err {
  margin-top: 4px;
  font-size: var(--font-size-xs);
  color: var(--color-error);
}
</style>
