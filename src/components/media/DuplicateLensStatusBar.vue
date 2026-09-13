<template>
  <!-- 重复镜头状态条(方案 §5.1 第二行 + §9 状态表):置于画廊网格上方、不参与虚拟滚动
       (与 .view-back-bar 同位惯例)。文案/动作判定单源在 useDuplicateLensStatus
       (内部走 duplicateLensStatus.ts 纯函数),本组件只做呈现与动作转发。 -->
  <div class="lens-status-bar">
    <CopyCheck :size="16" class="lens-status-bar__icon" />
    <div class="lens-status-bar__text">
      <!-- 主标题:§8.1「焦点项已不在新布局 → 聚焦新布局主标题」的焦点落点(useLensFocusRestore
         按固定选择器 #lens-status-title 查询,勿单侧改名)。tabindex=-1 使 span 可编程聚焦,
         程序化 focus 不出焦点环(非交互元素),供键盘用户在重排后重新定位。 -->
      <span id="lens-status-title" class="lens-status-bar__title" tabindex="-1">{{ title }}</span>
      <span v-if="desc" class="lens-status-bar__desc">{{ desc }}</span>
    </div>
    <div class="lens-status-bar__actions">
      <!-- 主动作随状态切换(停止/开始/重新分析/重试);退出镜头恒可用(§9 全行都有「退出」)。 -->
      <UiButton variant="secondary" @click="runPrimaryAction">
        <Square v-if="view.primaryAction === 'stop'" :size="14" />
        <Play v-else-if="view.primaryAction === 'start'" :size="14" />
        <RefreshCw v-else :size="14" />
        {{ primaryLabel }}
      </UiButton>
      <UiButton variant="ghost" @click="lens.exitLens()">
        {{ t('duplicatesLens.exit') }}
      </UiButton>
    </div>
  </div>
</template>

<script setup lang="ts">
// 挂载时幂等恢复一次后端运行态(Idle vs Completed 可区分是 §9「从未分析/已完成」判定的前提);
// 此后状态经 DEDUP_PROGRESS 事件流保持新鲜,不再逐次 IPC。
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { CopyCheck, Play, RefreshCw, Square } from '@lucide/vue'
import UiButton from '../ui/UiButton.vue'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useDedupStore } from '../../stores/dedupStore'
import { useDuplicateLensStatus } from './useDuplicateLensStatus'

const lens = useDuplicateLensStore()
const dedup = useDedupStore()
const { t } = useI18n()
const { view, title, desc, primaryLabel, runPrimaryAction } = useDuplicateLensStatus()

onMounted(() => {
  void dedup.restoreStatusOnce().catch(() => {
    // 恢复失败不阻塞镜头浏览:状态按 idle 呈现,用户仍可主动开始分析。
  })
})
</script>

<style scoped>
.lens-status-bar {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: 40px;
  flex-shrink: 0; /* 与 .view-back-bar 同惯例:固定高条,不参与网格的弹性压缩 */
  padding: var(--spacing-xs) var(--spacing-md);
  background: var(--color-bg-primary);
  border-bottom: 1px solid var(--color-divider);
}

.lens-status-bar__icon {
  flex-shrink: 0;
  color: var(--color-text-secondary);
}

.lens-status-bar__text {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  min-width: 0;
  flex: 1;
}

.lens-status-bar__title {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  /* 程序化焦点落点(§8.1,tabindex=-1):非交互元素,消除浏览器默认焦点环。 */
  outline: none;
}

.lens-status-bar__desc {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lens-status-bar__actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  flex-shrink: 0;
}
</style>
