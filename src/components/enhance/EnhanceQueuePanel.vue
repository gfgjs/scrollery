<!-- 影像增强队列进度面板（降噪/超分子系统 P0 批 5.5，复核缺口收口）：队列态此前无消费方——
     `enhanceStore` 已有 subscribeQueue/fetchQueue/queue，但无任何组件挂载订阅、无 UI 展示。
     本组件展示队列内各 job：状态标签 + 进度条(tilesDone/tilesTotal，估算总数为 0 时退化为
     不定长态) + 失败行文案(enhanceErrorMessageKey 稳定码 → i18n) + 取消按钮(排队/处理中态)。
     生命周期内 subscribeQueue/unsubscribeQueue（计数守卫，见 store 注释）；两处挂载
     （设置分节顶部 + 对话框内）互不干扰监听。 -->
<template>
  <div v-if="enhance.queue.length" class="enh-queue">
    <h3 class="enh-queue__title">{{ $t('enhance.queueTitle') }}</h3>
    <div v-for="job in enhance.queue" :key="job.id" class="enh-queue__row">
      <div class="enh-queue__head">
        <span class="enh-queue__label">{{ $t('enhance.queueJobLabel', { id: job.id }) }}</span>
        <span class="enh-queue__tag" :class="`enh-queue__tag--${job.status}`">
          {{ statusLabel(job.status) }}
        </span>
      </div>
      <div class="enh-queue__progress-row">
        <span class="enh-queue__items">
          {{ $t('enhance.queueProgressItems', { done: job.done, total: job.total }) }}
        </span>
        <span
          class="enh-queue__bar"
          :class="{
            'enh-queue__bar--indeterminate': !job.tilesTotal && job.status === 'running',
          }"
        >
          <span
            v-if="job.tilesTotal"
            class="enh-queue__bar-fill"
            :style="{ width: tilePct(job) + '%' }"
          ></span>
        </span>
      </div>
      <p v-if="job.status === 'error'" class="enh-queue__err">
        {{ $t(enhanceErrorMessageKey(job.errorCode)) }}
      </p>
      <button
        v-if="job.status === 'queued' || job.status === 'running'"
        class="enh-queue__cancel-btn"
        @click="enhance.cancel(job.id)"
      >
        {{ $t('enhance.queueCancel') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useEnhanceStore, enhanceErrorMessageKey } from '../../stores/enhanceStore'
import type { EnhanceJob, EnhanceJobStatus } from '../../types/enhance'

const enhance = useEnhanceStore()
const { t } = useI18n()

const STATUS_LABEL_KEY: Record<EnhanceJobStatus, string> = {
  queued: 'enhance.queueStatusQueued',
  running: 'enhance.queueStatusRunning',
  done: 'enhance.queueStatusDone',
  error: 'enhance.queueStatusError',
  cancelled: 'enhance.queueStatusCancelled',
}

function statusLabel(status: EnhanceJobStatus): string {
  return t(STATUS_LABEL_KEY[status])
}

/** tilesTotal=0（估算不可用）时不计算百分比——模板走不定长条纹态。 */
function tilePct(job: EnhanceJob): number {
  if (!job.tilesTotal) return 0
  return Math.min(100, Math.round((job.tileDone / job.tilesTotal) * 100))
}

onMounted(() => {
  void enhance.subscribeQueue()
})
onUnmounted(() => {
  enhance.unsubscribeQueue()
})
</script>

<style scoped>
.enh-queue {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  margin-bottom: var(--spacing-md);
}
.enh-queue__title {
  margin: 0;
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.enh-queue__row {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--spacing-sm) 0;
  border-top: 1px solid var(--color-border);
}
.enh-queue__row:first-of-type {
  border-top: none;
}
.enh-queue__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}
.enh-queue__label {
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}
.enh-queue__tag {
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-text-tertiary) 16%, transparent);
  color: var(--color-text-secondary);
}
.enh-queue__tag--running {
  background: color-mix(in srgb, var(--color-accent) 16%, transparent);
  color: var(--color-accent);
}
.enh-queue__tag--done {
  background: var(--color-success-subtle);
  color: var(--color-success);
}
.enh-queue__tag--error {
  background: var(--color-error-subtle);
  color: var(--color-error);
}
.enh-queue__progress-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.enh-queue__items {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  min-width: 64px;
  font-variant-numeric: tabular-nums;
}
.enh-queue__bar {
  flex: 1;
  height: 4px;
  background: var(--color-bg-primary);
  border-radius: 2px;
  overflow: hidden;
}
.enh-queue__bar-fill {
  display: block;
  height: 100%;
  background: var(--color-accent);
  transition: width var(--transition-fast);
}
/* 总数未知时的不定长条纹动画（回收动画预算，只在需要时启用）。 */
.enh-queue__bar--indeterminate {
  background: repeating-linear-gradient(
    90deg,
    color-mix(in srgb, var(--color-accent) 45%, transparent) 0,
    color-mix(in srgb, var(--color-accent) 45%, transparent) 10px,
    transparent 10px,
    transparent 20px
  );
  background-size: 200% 100%;
  animation: enh-queue-stripe 1.2s linear infinite;
}
@keyframes enh-queue-stripe {
  from {
    background-position: 0 0;
  }
  to {
    background-position: -40px 0;
  }
}
.enh-queue__err {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-error);
}
.enh-queue__cancel-btn {
  align-self: flex-start;
  padding: 2px 10px;
  font-size: var(--font-size-xs);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}
.enh-queue__cancel-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
</style>
