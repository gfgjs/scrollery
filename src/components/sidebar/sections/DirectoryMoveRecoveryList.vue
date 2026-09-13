<template>
  <!-- 目录移动的半完成恢复清单（P0-1）：只在确有未完成项时出现。
       逐条显示文件的**真实落点**与仍需收尾的原因，重试按阶段日志 id 发起（同一 id 运行中禁用）。
       清单读自后端（读本身不改任何东西），刷新后由启动点重新读取。 -->
  <div v-if="recovery.items.length > 0" class="dir-recovery">
    <div class="dir-recovery__head">
      <span class="dir-recovery__title">{{ $t('sidebar.dirRecovery.title') }}</span>
      <UiIconButton
        :label="$t('sidebar.dirRecovery.reload')"
        :disabled="recovery.loading"
        @click="recovery.load()"
      >
        <RefreshCw :size="13" />
      </UiIconButton>
    </div>
    <p class="dir-recovery__hint">{{ $t('sidebar.dirRecovery.hint') }}</p>
    <div v-for="item in recovery.items" :key="item.recoveryId" class="dir-recovery__item">
      <span class="dir-recovery__name" :title="item.sourceName">{{ item.sourceName }}</span>
      <span class="dir-recovery__target">
        <span class="dir-recovery__label">{{ $t('sidebar.dirRecovery.target') }}</span>
        <span class="dir-recovery__path" :title="item.targetAbsPath">{{ item.targetAbsPath }}</span>
      </span>
      <span class="dir-recovery__detail">{{ $t(dirMoveRecoveryDetailKey(item.detail)) }}</span>
      <UiButton
        class="dir-recovery__retry"
        size="sm"
        variant="secondary"
        :loading="recovery.isRetrying(item.recoveryId)"
        :disabled="recovery.isRetrying(item.recoveryId)"
        @click="recovery.retry(item.recoveryId)"
      >
        <RotateCcw v-if="!recovery.isRetrying(item.recoveryId)" :size="12" />
        {{
          recovery.isRetrying(item.recoveryId)
            ? $t('sidebar.dirRecovery.retrying')
            : $t('sidebar.dirRecovery.retry')
        }}
      </UiButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { RefreshCw, RotateCcw } from '@lucide/vue'
import UiButton from '../../ui/UiButton.vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import {
  dirMoveRecoveryDetailKey,
  useDirectoryMoveRecoveryStore,
} from '../../../stores/directoryMoveRecoveryStore'

const recovery = useDirectoryMoveRecoveryStore()
</script>

<style scoped>
.dir-recovery {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  /* 与扫描根行同轨（左缘对齐 --sidebar-indent 内容轨）。 */
  margin-top: var(--spacing-sm);
  padding: 0 var(--spacing-md) 0 var(--sidebar-indent, 30px);
}
.dir-recovery__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}
.dir-recovery__title {
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--color-warning);
}
.dir-recovery__head .btn-icon {
  min-width: 0;
  min-height: 0;
  padding: 5px;
}
.dir-recovery__hint {
  margin: 0;
  font-size: 10px;
  line-height: 1.4;
  color: var(--color-text-tertiary);
}
.dir-recovery__item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--spacing-xs) var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
}
.dir-recovery__name {
  font-size: var(--font-size-xs);
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 真实落点必须能读：单行省略 + title 兜底完整路径。 */
.dir-recovery__target {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-xs);
  min-width: 0;
}
.dir-recovery__label {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--color-text-tertiary);
}
.dir-recovery__path {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dir-recovery__detail {
  font-size: 10px;
  color: var(--color-error);
}
.dir-recovery__retry {
  align-self: flex-start;
  margin-top: 2px;
}
</style>
