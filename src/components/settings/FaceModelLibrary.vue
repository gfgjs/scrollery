<template>
  <!-- 人脸模型库（F7+）：展示两条内置轨 + 安装/下载。默认轨可一键下载；切换仍未开放——见下方说明。 -->
  <CollapsibleCard id="faceModels" :title="$t('settings.fmTitle')">
    <!-- 文案含 <strong> 内联标记,与 nsIntro 同法走 v-html(内容来自本地 locale,非用户输入)。 -->
    <p class="face-models__hint">
      <span v-html="$t('settings.fmIntro')"></span>
      <span class="face-models__warn" v-html="$t('settings.fmWarn')"></span>
    </p>

    <div
      v-for="m in models"
      :key="m.id"
      class="face-model"
      :class="{ 'face-model--active': m.active }"
    >
      <div class="face-model__head">
        <span class="face-model__name">{{ m.displayName }}</span>
        <span class="face-model__badge" :class="m.commercialOk ? 'badge--ok' : 'badge--nc'">{{
          m.commercialOk ? $t('settings.fmCommercialOk') : $t('settings.fmNonCommercial')
        }}</span>
        <span v-if="m.active" class="face-model__badge badge--active">{{
          $t('settings.mlActive')
        }}</span>
      </div>
      <div class="face-model__desc">{{ m.description }}</div>
      <div class="face-model__meta">
        <span>{{
          $t('settings.fmMeta', {
            detector: m.detector,
            embedder: m.embedder,
            dim: m.embedDim,
            mb: m.sizeMb,
          })
        }}</span>
        <span class="face-model__lic">{{ m.license }}</span>
      </div>
      <div class="face-model__status">
        <span v-if="m.installed" class="status--installed">{{ $t('settings.fmFilesReady') }}</span>
        <span v-else-if="!m.downloadable" class="status--missing">{{
          $t('settings.fmManualOnly')
        }}</span>
        <span v-else class="status--missing">{{ $t('settings.fmNotInstalled') }}</span>

        <!-- 下载：仅可下载且未就位的轨显示（默认轨）。下载中显示进度。 -->
        <template v-if="m.downloadable && !m.installed">
          <button v-if="!downloading[m.id]" class="face-model__dl-btn" @click="download(m)">
            {{ $t('settings.mlDownload') }}
          </button>
          <span v-else class="face-model__dl-progress">
            <span class="face-model__dl-bar">
              <span class="face-model__dl-fill" :style="{ width: pct(m.id) + '%' }"></span>
            </span>
            <span class="face-model__dl-pct">{{ pct(m.id) }}%</span>
          </span>
        </template>
      </div>
      <div v-if="errors[m.id]" class="face-model__err">
        {{ $t('settings.mlDownloadFailed', { error: errors[m.id] }) }}
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import CollapsibleCard from './CollapsibleCard.vue'
import { useFaceStore } from '../../stores/faceStore'
import { ipcErrorMessage } from '../../utils/ipc'
import type { FaceModelInfo } from '../../types/face'

const face = useFaceStore()
const models = ref<FaceModelInfo[]>([])

// 每条轨的下载态：是否进行中 / 累计字节-总字节 / 错误信息。按 profile id 键。
const downloading = reactive<Record<string, boolean>>({})
const progress = reactive<Record<string, { received: number; total: number }>>({})
const errors = reactive<Record<string, string>>({})

function pct(id: string): number {
  const p = progress[id]
  if (!p || p.total === 0) return 0
  return Math.round((p.received / p.total) * 100)
}

async function refresh() {
  models.value = await face.listFaceModels()
}

async function download(m: FaceModelInfo) {
  downloading[m.id] = true
  errors[m.id] = ''
  progress[m.id] = { received: 0, total: m.sizeMb * 1024 * 1024 } // sizeMb 仅占位，回调到达即覆盖
  try {
    await face.downloadFaceModel(m.id, (ev) => {
      progress[m.id] = { received: ev.received, total: ev.total }
      if (ev.error) errors[m.id] = ev.error
    })
    await refresh() // 下载完成 → installed 翻绿
  } catch (e) {
    // 文案统一走 ipcErrorMessage(2026-07-10 审查 U7):String(e) 带 "IpcError: " 前缀。
    errors[m.id] = ipcErrorMessage(e)
  } finally {
    downloading[m.id] = false
  }
}

onMounted(refresh)
</script>

<style scoped>
.face-models__hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  /* 横向内缩 --spacing-lg,与卡片头 padding、上方 ModelLibrary 内容同一左缘对齐;
     此前内容贴 0px 左缘,比卡片头标题/AI 模型库内容更靠左,整段错位。 */
  margin: 0 0 var(--spacing-md);
  padding: 0 var(--spacing-lg);
  line-height: 1.6;
}
.face-models__warn {
  color: var(--color-text-tertiary);
}
.face-model {
  padding: var(--spacing-md);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  /* 卡片左右各内缩 --spacing-lg,左边框与卡片头 padding 同缘(与 hint、ModelLibrary 对齐)。 */
  margin: 0 var(--spacing-lg) var(--spacing-sm);
}
.face-model--active {
  border-color: var(--color-accent);
}
.face-model__head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}
.face-model__name {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.face-model__badge {
  /* 与 ModelLibrary 徽章统一为胶囊形:二者常同屏(见设置页),矩形/胶囊混排显杂。 */
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 7px;
  border-radius: 999px;
}
/* 实底状态徽章使用各自的 paired foreground，避免暗色主题上的白字失去对比。 */
.badge--ok {
  background: var(--color-success);
  color: var(--color-text-on-success);
}
.badge--nc {
  background: var(--color-warning);
  color: var(--color-text-on-warning);
}
.badge--active {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.face-model__desc {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  line-height: 1.5;
  margin-bottom: 6px;
}
.face-model__meta {
  /* 规格 + 许可证左对齐同排:此前 space-between 把许可证甩到卡片最右缘,
     与其余左对齐内容(名称/描述/状态)错位成一个孤立的右浮块。 */
  display: flex;
  flex-wrap: wrap;
  gap: 4px 8px;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-bottom: 4px;
}
.face-model__lic::before {
  /* 左对齐后规格段与许可证相邻,补一个分隔点保持可读。 */
  content: '·';
  margin-right: 8px;
  color: var(--color-text-tertiary);
}
.face-model__status {
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
.face-model__dl-btn {
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
.face-model__dl-btn:hover {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.face-model__dl-progress {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1;
}
.face-model__dl-bar {
  flex: 1;
  height: 4px;
  max-width: 160px;
  background: var(--color-bg-primary);
  border-radius: 2px;
  overflow: hidden;
}
.face-model__dl-fill {
  display: block;
  height: 100%;
  background: var(--color-accent);
  transition: width var(--transition-fast);
}
.face-model__dl-pct {
  color: var(--color-text-secondary);
  font-variant-numeric: tabular-nums;
}
.face-model__err {
  margin-top: 4px;
  font-size: var(--font-size-xs);
  color: var(--color-error);
}
</style>
