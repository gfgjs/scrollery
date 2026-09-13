<!-- 根文件夹显隐（V21，设置页库级排除）：列出所有扫描根 + 每根一个显隐开关。隐藏后该根媒体从 -->
<!-- 画廊「全部」/时间轴/搜索/统计/侧栏文件树/全选全部消失，取消即恢复。列出**全部**根（含已隐藏， -->
<!-- 否则无从开关回来）；侧栏文件树只显示可见根（scan.visibleScanRoots）。 -->
<template>
  <CollapsibleCard id="rootVisibility" :title="$t('settings.rootVisTitle')">
    <div class="rv-intro">{{ $t('settings.rootVisIntro') }}</div>

    <div v-if="!scan.scanRoots.length" class="rv-empty">{{ $t('settings.rootVisEmpty') }}</div>

    <div v-else class="rv-list">
      <div v-for="root in scan.scanRoots" :key="root.id" class="rv-item" :class="{ hidden: root.isHidden }">
        <FolderOpen :size="18" class="rv-item__icon" />

        <div class="rv-item__info">
          <div class="rv-item__name">{{ displayName(root) }}</div>
          <div class="rv-item__path">{{ root.path }}</div>
        </div>

        <button
          class="rv-toggle"
          :class="{ 'rv-toggle--hidden': root.isHidden }"
          :title="root.isHidden ? $t('settings.rootVisShow') : $t('settings.rootVisHide')"
          @click="toggle(root)"
        >
          <component :is="root.isHidden ? EyeOff : Eye" :size="16" />
          <span>{{ root.isHidden ? $t('settings.rootVisHidden') : $t('settings.rootVisVisible') }}</span>
        </button>
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { onMounted } from 'vue'
import { Eye, EyeOff, FolderOpen } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import CollapsibleCard from './CollapsibleCard.vue'
import { useScanStore } from '../../stores/scanStore'
import { useToastStore } from '../../stores/toastStore'
import type { ScanRoot } from '../../types/media'
import type { IpcError } from '../../utils/ipc'

const { t } = useI18n()
const scan = useScanStore()
const toast = useToastStore()

onMounted(() => {
  // 设置页可能先于侧栏加载：若根列表尚空，主动拉一次（幂等，loadScanRoots 覆盖赋值）。
  if (!scan.scanRoots.length) void scan.loadScanRoots()
})

// 显示名：别名优先 → 路径末段（[/\\] 兼容 Windows 反斜杠，同 ManagementSection）→ 整条路径兜底。
function displayName(root: ScanRoot): string {
  return root.alias || root.path.split(/[/\\]/).filter(Boolean).pop() || root.path
}

async function toggle(root: ScanRoot) {
  try {
    await scan.setScanRootHidden(root.id, !root.isHidden)
  } catch (e) {
    toast.addToast('error', t('settings.rootVisFailed', { code: (e as IpcError)?.code ?? e }))
  }
}
</script>

<style scoped>
.rv-intro {
  padding: 12px 16px;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  line-height: 1.6;
}
.rv-empty {
  padding: 14px 16px;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.rv-list {
  padding: 0 16px 8px;
}
.rv-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--color-border);
}
.rv-item:last-child {
  border-bottom: none;
}
/* 隐藏根整行淡化，一眼可辨「当前不显示」。 */
.rv-item.hidden {
  opacity: 0.55;
}
.rv-item__icon {
  color: var(--color-text-secondary);
  flex: 0 0 auto;
}
.rv-item__info {
  flex: 1;
  min-width: 0;
}
.rv-item__name {
  font-weight: 600;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rv-item__path {
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-top: 2px;
}
.rv-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  padding: 6px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast),
    border-color var(--transition-fast);
}
.rv-toggle:hover {
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
}
/* 已隐藏态：用 warning 调，区别于「可见」的中性态（状态不只靠颜色——文案 + 图标 EyeOff 同时表达）。 */
.rv-toggle--hidden {
  color: var(--color-warning);
  border-color: color-mix(in srgb, var(--color-warning) 45%, var(--color-border));
}
</style>
