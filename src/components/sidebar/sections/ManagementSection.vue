<template>
  <!-- 仅在有可管理的扫描根目录时存在——不存在时区块会自行注销，使粘性堆叠偏移不留空档。 -->
  <AccordionSection
    v-if="scan.hasScanRoots"
    id="management"
    :order="order"
    :title="$t('sidebar.management')"
  >
    <template #actions>
      <!-- 有未完成移动时标题上直接可见：恢复清单主体默认折叠在区块内，重启后不点开也看得到入口。 -->
      <span
        v-if="recovery.pendingCount > 0"
        class="acc-recovery"
        :title="$t('sidebar.dirRecovery.title')"
      >
        <AlertTriangle :size="12" />
        {{ recovery.pendingCount }}
      </span>
    </template>
    <div class="scan-status">
      <div
        v-for="root in scan.scanRoots"
        :key="root.id"
        class="scan-root"
        :class="{ 'scan-root--hidden': root.isHidden }"
      >
        <div class="scan-root__info">
          <!-- split 用 [/\\] 兼容 Windows 反斜杠路径(2026-07-06 审查 F13):原 split('/') 在
               C:\Users\... 上返回整条路径而非末段目录名。 -->
          <span class="scan-root__alias">{{
            root.alias ?? root.path.split(/[/\\]/).filter(Boolean).pop()
          }}</span>
          <div class="scan-root__actions">
            <UiIconButton
              :label="
                scan.getProgress(root.id)?.isRunning ? $t('sidebar.stopScan') : $t('sidebar.rescan')
              "
              :active="scan.getProgress(root.id)?.isRunning"
              @click="toggleScan(root.id)"
            >
              <Square
                v-if="scan.getProgress(root.id)?.isRunning"
                :size="14"
                color="var(--color-error)"
                fill="var(--color-error)"
              />
              <RefreshCw v-else :size="14" />
            </UiIconButton>
            <!-- 显式快速/完整入口（P1-4）：自动按钮按 mtime 基线自选模式，这里让用户明确指定。
                 快速只处理目录增删（mtime 未变目录被剪枝），完整逐文件复查——在其他软件里
                 改过文件内容（同大小改写）后必须用完整扫描才能刷新到位。运行中禁用：先停止
                 再选模式，避免误重启正在跑的扫描。 -->
            <UiIconButton
              :label="$t('sidebar.quickScanHint')"
              :title="`${$t('sidebar.quickScan')} — ${$t('sidebar.quickScanHint')}`"
              :disabled="scan.getProgress(root.id)?.isRunning"
              @click="startExplicitScan(root.id, true)"
            >
              <Zap :size="14" />
            </UiIconButton>
            <UiIconButton
              :label="$t('sidebar.fullScanHint')"
              :title="`${$t('sidebar.fullScan')} — ${$t('sidebar.fullScanHint')}`"
              :disabled="scan.getProgress(root.id)?.isRunning"
              @click="startExplicitScan(root.id, false)"
            >
              <SearchCheck :size="14" />
            </UiIconButton>
            <!-- 库级显隐(V21):与设置页同一后端翻转,隐藏根从画廊/时间轴/搜索/文件树消失,再点恢复。
                 隐藏态 active 高亮告知「此根现被隐藏」。 -->
            <UiIconButton
              class="scan-root__hide"
              :label="root.isHidden ? $t('sidebar.showFolder') : $t('sidebar.hideFolder')"
              toggle
              :active="root.isHidden"
              @click="toggleHidden(root.id, !root.isHidden)"
            >
              <Eye v-if="root.isHidden" :size="14" />
              <EyeOff v-else :size="14" />
            </UiIconButton>
            <UiIconButton
              class="scan-root__remove"
              :label="$t('sidebar.removeFolder')"
              @click="removeRoot(root.id)"
            >
              <Trash2 :size="14" />
            </UiIconButton>
          </div>
        </div>

        <div v-if="scan.getProgress(root.id)?.isRunning" class="scan-root__progress">
          <div class="progress-bar">
            <div
              class="progress-bar__fill progress-shimmer"
              :class="{ 'progress-bar__fill--discovering': isIndeterminate(root.id) }"
              :style="{ width: (isIndeterminate(root.id) ? 100 : progressPercent(root.id)) + '%' }"
            />
          </div>
          <span class="scan-root__count">
            <template v-if="scan.getProgress(root.id)?.status === 'discovering'">
              {{
                $t('sidebar.discoveringFiles', { count: scan.getProgress(root.id)?.scanned ?? 0 })
              }}
            </template>
            <template v-else-if="scan.getProgress(root.id)?.status === 'enriching'">
              <template v-if="(scan.getProgress(root.id)?.total ?? 0) > 0">
                {{
                  $t('sidebar.enrichingFiles', {
                    scanned: scan.getProgress(root.id)?.scanned ?? 0,
                    total: scan.getProgress(root.id)?.total ?? 0,
                  })
                }}
              </template>
              <template v-else>{{ $t('sidebar.enrichingStart') }}</template>
            </template>
            <template v-else>
              {{
                $t('sidebar.indexingFiles', {
                  scanned: scan.getProgress(root.id)?.scanned ?? 0,
                  total: scan.getProgress(root.id)?.total ?? 0,
                })
              }}
            </template>
          </span>
        </div>
      </div>
    </div>

    <!-- 目录移动半完成恢复（P0-1）：有未完成项才出现，重试按阶段日志 id 发起。 -->
    <DirectoryMoveRecoveryList />
  </AccordionSection>
</template>

<script setup lang="ts">
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Square,
  RefreshCw,
  Trash2,
  Eye,
  EyeOff,
  Zap,
  SearchCheck,
  AlertTriangle,
} from '@lucide/vue'
import AccordionSection from '../AccordionSection.vue'
import DirectoryMoveRecoveryList from './DirectoryMoveRecoveryList.vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import { useToastStore } from '../../../stores/toastStore'
import { useScanStore } from '../../../stores/scanStore'
import { useMediaStore } from '../../../stores/mediaStore'
import { useDirectoryMoveRecoveryStore } from '../../../stores/directoryMoveRecoveryStore'
import { useConfirm } from '../../../composables/useConfirm'

defineProps<{ order: number }>()

const toast = useToastStore()
const scan = useScanStore()
const media = useMediaStore()
const recovery = useDirectoryMoveRecoveryStore()
const { confirm } = useConfirm()
const { t } = useI18n()

// 启动即读一次未完成移动清单（只读，不触发任何物理动作）：刷新/重启后恢复入口照常出现。
onMounted(() => {
  void recovery.load()
})

// ── 扫描进度显示 ───────────────────────────────────────────────────────────
function progressPercent(rootId: number): number {
  const p = scan.getProgress(rootId)
  if (!p || !p.total || p.status === 'discovering') return 0
  return Math.round((p.scanned / p.total) * 100)
}

// 尚无已知总数的阶段显示不确定（流光）进度条而非 0%：检索中（遍历），
// 或 enriching 在首个批次事件之前。
function isIndeterminate(rootId: number): boolean {
  const p = scan.getProgress(rootId)
  if (!p) return false
  return p.status === 'discovering' || (p.status === 'enriching' && !p.total)
}

// ── 扫描控制 ───────────────────────────────────────────────────────────────
// 快扫完成后的共同收尾：刷新统计 + 让 FoldersSection 重载树（树实例归它所有）。
function onScanFinished() {
  media.loadStats()
  window.dispatchEvent(new CustomEvent('folder-stats-changed'))
}

async function toggleScan(rootId: number) {
  const p = scan.getProgress(rootId)
  if (p?.isRunning) {
    await scan.stopScan(rootId)
  } else {
    // auto：不传 quickOverride，沿用 store 的自动策略（首轮全量、其后增量）。
    await scan.startScan(rootId, onScanFinished)
  }
}

// 显式快速/完整入口（P1-4）：唯一差别就是 quickOverride——快速为 true（按目录 mtime 剪枝增量），
// 完整为 false（逐文件复查，兜住快速扫不到的「就地编辑」）。
async function startExplicitScan(rootId: number, quick: boolean) {
  // 扫描进行中不重启：两个按钮已 disabled，这里再挡一次键盘/程序触发。
  if (scan.getProgress(rootId)?.isRunning) return
  await scan.startScan(rootId, onScanFinished, quick)
}

// 库级显隐(V21):翻转 is_hidden。setScanRootHidden 内已 bump data_version + invalidateLayout +
// loadStats,画廊/统计/时间轴即时反映;侧栏文件树走 visibleScanRoots 自动排除。此处仅补 toast。
async function toggleHidden(id: number, hidden: boolean) {
  try {
    await scan.setScanRootHidden(id, hidden)
  } catch (e) {
    toast.addToast('error', String(e))
  }
}

async function removeRoot(id: number) {
  const { confirmed, checkboxValue } = await confirm({
    title: t('sidebar.removeFolder'),
    message: t('sidebar.confirmRemove'),
    confirmText: t('sidebar.removeFolder'),
    cancelText: t('common.cancel'),
    showCheckbox: true,
    checkboxLabel: t('sidebar.clearThumbnails'),
    checkboxValue: true,
  })
  if (!confirmed) return

  const expectedRunId = scan.getProgress(id)?.runId
  try {
    const result = await scan.removeScanRoot(id, checkboxValue)
    if (result.cleared_count > 0) {
      toast.addToast('success', t('sidebar.thumbnailsCleared', { count: result.cleared_count }))
    }
    // 移除根目录会改变 scan.scanRoots → FoldersSection 的 watch 自动重载树。此处刷新统计。
    await scan.loadScanRoots()
    media.loadStats()
  } catch (e) {
    // 后端在删除前已取消该根扫描；删除失败时没有正常完成事件可供前端收尾。
    if (expectedRunId !== undefined) scan.markScanStopped(id, expectedRunId)
    toast.addToast('error', t('sidebar.removeFolderFailed') + ' ' + e)
  }
}
</script>

<style scoped>
.scan-status {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  /* 左缘对齐 --sidebar-indent 内容轨(与其他区块子菜单同轨)。 */
  padding: 0 var(--spacing-md) 0 var(--sidebar-indent, 30px);
}
.scan-root__info {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}
.scan-root__alias {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.scan-root__actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
/* 侧栏动作用紧凑盒:去掉 --control-size-compact 的最小宽高,图标不再各自漂在大盒里显「散」。
   父 scoped 的 data-v 落到 UiIconButton 根 button(Vue 3 子组件根继承父作用域),故无需 :deep。 */
.scan-root__actions .btn-icon {
  min-width: 0;
  min-height: 0;
  padding: 5px;
}
.scan-root__remove {
  color: var(--color-error);
  opacity: 0.7;
}
.scan-root__remove:hover {
  opacity: 1;
}
/* 已隐藏根:整行淡化,一眼可辨「当前不显示」;显隐按钮 active 高亮进一步确认状态。 */
.scan-root--hidden .scan-root__alias {
  opacity: 0.5;
}
.scan-root__progress {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  margin-top: 2px;
}
.progress-bar {
  flex: 1;
  height: 3px;
  border-radius: 2px;
  background: var(--color-border);
  overflow: hidden;
}
.progress-bar__fill {
  height: 100%;
  border-radius: 2px;
  background: var(--color-accent);
  transition: width var(--duration-fast) linear;
}
.progress-bar__fill--discovering {
  width: 100% !important;
  animation: breathe 1.5s ease-in-out infinite;
}
@keyframes breathe {
  0%,
  100% {
    opacity: 0.4;
  }
  50% {
    opacity: 1;
  }
}
.scan-root__count {
  font-size: 10px;
  color: var(--color-text-tertiary);
  white-space: nowrap;
}
</style>
