<template>
  <!-- docked 选区操作条的 Teleport 目标(恒存在,SelectionToolbar 的 docked 形态投送 DOM 于此)。
       docked && 选区态时接管左侧(is-active → flex:1),info 让位(替换式共存,§3.7);空/非激活时零宽。 -->
  <div
    id="statusbar-selection-outlet"
    class="statusbar__selection-outlet"
    :class="{ 'is-active': dockedSelectionActive }"
  ></div>
  <div v-if="!dockedSelectionActive" class="statusbar__info">
    <!-- 内容页(/view /doc /audio,hasActiveViewer 判定)→ 当前文件信息接管左区,替换画廊统计与
         进度链(2026-07-17 底栏重构;共存裁决:内容页聚焦当前文件,库级进度回画廊再看)。 -->
    <StatusBarFileInfo v-if="viewer.hasActiveViewer" />
    <span v-else-if="scan.isAnyScanRunning" class="statusbar__scanning">
      <span class="spinner" />
      {{ $t('statusbar.scanningSimple') }}
    </span>
    <span
      v-else-if="scan.thumbGenProgress.isRunning"
      class="statusbar__scanning statusbar__hint"
      :title="$t('statusbar.thumbGenBgTitle')"
    >
      <span class="spinner" />
      {{
        $t('settings.genStatusRunning', {
          generated: scan.thumbGenProgress.generated,
          total: scan.thumbGenProgress.total,
        })
      }}
      <span
        v-if="scan.thumbGenProgress.phase"
        class="statusbar__phase"
        >{{ scan.thumbGenProgress.phase }}</span
      >
      <span
        v-if="scan.thumbGenProgress.currentItem"
        class="statusbar__current-item"
        >({{ scan.thumbGenProgress.currentItem }})</span
      >
      <button
        @click="scan.stopFullThumbnailGeneration()"
        class="statusbar__stop-btn"
        :title="$t('settings.stopGen')"

      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="currentColor"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
        </svg>
      </button>
    </span>

    <!-- AI 分析指示器（AI 正在主动建索时显示） -->
    <span
      v-else-if="ai.status.isAnalyzing"
      class="statusbar__scanning"
      :title="$t('statusbar.aiAnalyzingTitle')"
    >
      <span class="spinner" />
      {{
        $t('statusbar.aiProgress', {
          analyzed: ai.status.analyzedItems,
          total: ai.status.totalItems,
        })
      }}
       <span class="statusbar__secondary">({{ ai.analyzeProgress }}%)</span>
    </span>
    <span v-else-if="media.stats">
      {{ $t('statusbar.items', { count: media.viewTotalItems.toLocaleString() }) }}
      <template v-if="media.stats.totalImages > 0"
        >·
        {{ $t('statusbar.images', { count: media.stats.totalImages.toLocaleString() }) }}</template
      >
      <template v-if="media.stats.totalVideos > 0"
        >·
        {{ $t('statusbar.videos', { count: media.stats.totalVideos.toLocaleString() }) }}</template
      >
    </span>
    <!-- 视口缩略图段与上链并列(可同屏),内容页同样让位于文件信息。 -->
    <span
      v-if="!viewer.hasActiveViewer && scan.autoThumbInFlight > 0"
      class="statusbar__scanning statusbar__hint"
      :title="$t('statusbar.viewportThumbTitle')"
    >
      <span class="spinner" />
      {{ $t('statusbar.viewportThumbProgress', { count: scan.autoThumbInFlight }) }}
      <template v-if="scan.autoThumbQueueSize > 0"
        ><span class="statusbar__secondary statusbar__secondary--spaced">{{
          $t('statusbar.viewportThumbQueued', { count: scan.autoThumbQueueSize })
        }}</span></template
      >
    </span>
  </div>

  <div class="statusbar__right">
    <ScanProgressIndicator />
    <span v-if="media.isComputingLayout" class="statusbar__computing">
      <span class="spinner" />
      {{ $t('statusbar.computingLayout') }}
    </span>
    <!-- A/B 共用文件任务指示器：导出显示确定进度，备份显示粗粒度运行态。 -->
    <component :is="backgroundFileJobIndicator" v-if="backgroundFileJobIndicator" />
    <span class="statusbar__version">v0.1.0</span>
    <!-- 设置入口兜底:唯一常规入口在侧栏底部(SidebarFooter),侧栏收起时不可达——
         此处仅在侧栏隐藏时补一个恒在入口,侧栏可见时不重复出现。 -->
    <UiIconButton
      v-if="!ui.gallerySidebarVisible"
      class="statusbar__settings-btn"
      :label="$t('sidebar.settings')"
      :active="route.path.startsWith('/settings')"
      @click="toggleSettings"
    >
      <Settings :size="14" />
    </UiIconButton>
  </div>

  <!-- 轴控制簇的 Teleport 目标(2026-07-24 画廊轴/minimap 重构):恒存在,MediaGrid 的
       chevron/形态钮/渲染药丸投送 DOM 于此(与 selection-outlet 同模式,非 slot)。
       版本号(.statusbar__version)留原位——outlet 成末根元素后版本天然居其左,无需迁版本本身。 -->
  <div id="statusbar-axis-outlet" class="statusbar__axis-outlet"></div>
</template>

<script setup lang="ts">
import { computed, shallowRef, type Component } from 'vue'
import { Settings } from '@lucide/vue'
import { useRoute, useRouter } from 'vue-router'
import { useScanStore } from '../../stores/scanStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useAiStore } from '../../stores/aiStore'
import { useUiStore } from '../../stores/uiStore'
import { useViewerStore } from '../../stores/viewerStore'
import { useSelection } from '../../composables/useSelection'
import { useSelectionBarMode } from '../../composables/useSelectionBarMode'
import UiIconButton from '../ui/UiIconButton.vue'
import StatusBarFileInfo from './StatusBarFileInfo.vue'
import ScanProgressIndicator from './ScanProgressIndicator.vue'

// 文件任务不是首屏关键路径；异步组件把 export/backup 状态层留在独立懒块，避免抬高入口预算。
const backgroundFileJobIndicator = shallowRef<Component | null>(null)
void import('./BackgroundFileJobIndicator.vue').then((module) => {
  backgroundFileJobIndicator.value = module.default
})

const scan = useScanStore()
const media = useMediaStore()
const ai = useAiStore()
const ui = useUiStore()
const viewer = useViewerStore()
const route = useRoute()
const router = useRouter()
const selection = useSelection()
const selectionBarMode = useSelectionBarMode()

function toggleSettings() {
  if (route.path.startsWith('/settings')) {
    // 再点关闭:回进设置前的页面(与设置页返回钮同语义;直达无历史则回首页)。
    if (window.history.state?.back) void router.back()
    else void router.replace('/')
  } else {
    void router.push('/settings')
  }
}

// docked 且处于选区态且宿主(MediaGrid)活跃:info 让位、outlet 接管左侧(替换式共存,§3.7)。
// hostActive 与 SelectionToolbar 的 docked Teleport gate 同条件——避免「选区残留进查看器(宿主 deactivate)」
// 时 Teleport 已摘除但 info 仍让位,导致 outlet 空白。切换/进出选区/进出查看器 live 生效。
const dockedSelectionActive = computed(
  () =>
    selectionBarMode.docked.value &&
    selection.isSelectionMode.value &&
    selectionBarMode.hostActive.value,
)
</script>

<style scoped>
/* docked 选区操作条 outlet:非激活时无 flex-grow + 无内容 → 零宽,不挤占 info;激活时 flex:1 接管左侧。 */
.statusbar__selection-outlet {
  display: flex;
  align-items: center;
  min-width: 0;
}
.statusbar__selection-outlet.is-active {
  flex: 1 1 auto;
}
.statusbar__info {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex: 1;
  overflow: hidden;
}
.statusbar__scanning {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-accent);
}
.statusbar__right {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.statusbar__right > * + * {
  border-left: 1px solid var(--color-divider);
  padding-left: var(--spacing-sm);
}
.statusbar__computing {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-text-tertiary);
}
.statusbar__version {
  color: var(--color-text-tertiary);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}
.statusbar__hint {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  cursor: help;
}
.statusbar__phase {
  margin-left: var(--spacing-xs);
  padding: 0 var(--spacing-xs);
  border: 1px solid currentColor;
  border-radius: var(--radius-xs);
  color: var(--color-warning);
  font-size: var(--font-size-2xs);
  font-weight: 500;
  line-height: var(--leading-tight);
}
.statusbar__current-item {
  display: inline-block;
  max-width: 150px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  opacity: 0.8;
}
.statusbar__secondary {
  opacity: 0.7;
}
.statusbar__secondary--spaced {
  margin-left: var(--spacing-xs);
}
.statusbar__stop-btn {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border-radius: var(--radius-xs);
  color: var(--color-error);
  cursor: pointer;
  opacity: 0.8;
}
/* 状态栏仅 26px 高,全局 .btn-icon 的 32px 触达目标装不下——压成 22px 紧凑档。 */
.statusbar__settings-btn {
  min-width: 22px;
  min-height: 22px;
  padding: 3px;
}
.statusbar__stop-btn:hover {
  background: var(--color-bg-hover);
  opacity: 1;
}
/* 轴控制簇 outlet:空时零宽,不留缝(同 selection-outlet 惯例)。 */
.statusbar__axis-outlet {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 0;
}
</style>
