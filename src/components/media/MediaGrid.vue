<template>
  <div class="media-grid-layout">
    <div ref="mediaGridWrapperRef" class="media-grid-wrapper" :style="sidebarViewportLockStyle">
      <!-- 语义子视图返回栏：处于「某人物」或「某收藏夹」的照片视图时出现，点击或 ESC 回其总览页。 -->
      <button v-if="backBar" class="view-back-bar" @click="exitToOverview">
        <ChevronLeft :size="18" />
        <span class="view-back-bar__text">{{ backBar.label }}</span>
      </button>
      <!-- 重复镜头状态条(方案 §5.1 第二行/§9 状态表):镜头激活期常驻于工具栏与网格之间;
           文案/动作判定单源在 duplicateLensStatus.ts,此处只挂载与消费。 -->
      <DuplicateLensStatusBar v-if="lensActive" />
      <!-- tabindex=0:容器可聚焦,bucket 引擎的键盘滚动(onGridKeydown)才能在焦点不在
           卡片内时收到事件(点击空白区后 PageUp/Down 失灵的修复);鼠标点击走 :focus 非
           :focus-visible,不出焦点环(全局 reset 已区分)。 -->
      <div
        ref="gridRef"
        class="media-grid"
        tabindex="0"
        :class="{
          'is-scrolling': isScrolling,
          'is-compact': compactCells,
          'media-grid--bucket': bucketActive,
        }"
        @scroll.passive="onGridScroll"
        @wheel.passive="onGridWheel"
        @keydown="onGridKeydown"
        @touchmove.passive="onGridTouchmove"
      >
        <!-- 空状态:统一走 UiEmptyState 原语(结构)+ resolveGalleryEmptyState(消息/动作决策)。
             下一步动作按 emptyState.action 分派:add-folder 触发 FoldersSection 加目录流程,
             clear-filters 清全局筛选(修此前「筛选无匹配却引导添加文件夹」的错 CTA)。
             重复镜头空态(§9 加载反馈规则)优先于普通空态:零布局内容时按运行态区分
             「从未分析/分析中/已完成无重复/失败」,动作=开始(重新)分析/退出镜头(§9 表格)。 -->
        <UiEmptyState
          v-if="lensActive && media.totalRows === 0 && !media.isComputingLayout"
          :title="lensStatusTitle"
          :description="lensStatusDesc"
        >
          <template #icon><CopyCheck :size="48" /></template>
          <template #actions>
            <UiButton variant="primary" @click="onLensPrimaryAction">
              <Square v-if="lensStatusView.primaryAction === 'stop'" :size="16" />
              <Play v-else :size="16" />
              {{ lensStatusPrimaryLabel }}
            </UiButton>
            <UiButton variant="ghost" @click="duplicateLens.exitLens()">
              {{ t('duplicatesLens.exit') }}
            </UiButton>
          </template>
        </UiEmptyState>
        <UiEmptyState
          v-else-if="!lensActive && media.totalRows === 0 && !media.isComputingLayout"
          :title="emptyTitle"
          :description="emptyDescription"
        >
          <template #icon><ImageIcon :size="48" /></template>
          <template v-if="emptyState.action" #actions>
            <UiButton variant="primary" @click="onEmptyAction">
              <FolderPlus v-if="emptyState.action === 'add-folder'" :size="16" />
              <FilterX v-else :size="16" />
              {{ emptyActionLabel }}
            </UiButton>
          </template>
        </UiEmptyState>

        <!-- Loading: compute_layout 首屏阶段用骨架屏占位(S5),视觉上预演网格落位;
             文案仍播报计算中状态,骨架本身不可交互。
             仅「无既有布局」时显示(2026-07-10 深审问题1):已有旧布局的重算(重挂载回画廊/
             切筛选/增强批)走 stale-while-revalidate——旧行留在原位等新结果整体替换。此前
             无条件显示,骨架与下方内容分支非同链 v-if,重算期间 12 块骨架插在真实内容之上,
             正是「退出查看器画廊闪一下、行高变大再变回」的骨架闪帧根源。 -->
        <div
          v-if="showSkeleton && media.totalRows === 0"
          class="media-grid__skeleton"
        >
          <div
            v-for="i in skeletonCount"
            :key="i"
            class="skeleton-block media-grid__skeleton-cell"
          />
        </div>

        <!-- T16 方案B(B1.5):bucket 分段渲染。容器总高 = 真实逻辑高、零坐标平移;
             等高算术分段(useBucketVirtualScroll),仅渲染愿望窗口内的 1-3 个段——可见性
             是纯算术,无 IntersectionObserver、无全量占位 div、无内联函数 ref(B1 真机
             根因 C/D 由此结构性消除)。段行未到时以骨架条纹占位(--loading)。与下方
             方案 A 分支经 bucketActive 互斥,方案 A 零改动保留、开关即回退。卡片标记与
             方案 A 保持一致(data-item-id 与全部 handlers),使选区/拖拽/可视 patch
             消费面两边等价;B2 再抽公共行组件去重。 -->
        <!-- Canvas 渲染模式(§9 T4 最简原型):与下方 DOM 分支互斥,canvasMode 时接管。
             底层引擎(bucket/方案 A)不变,canvas 消费 activeRows 同源数据。
             §8.1 browse-only:is-selection-mode 在镜头态强制 false(Canvas 不画 checkbox/拖拽手柄)。 -->
        <MediaGridCanvas
          v-if="canvasMode"
          :rows="canvasRows"
          :current-y="currentLogicalY"
          :spacer-height="canvasSpacerHeight"
          :cache-dir="cacheDir"
          :compact-cells="compactCells"
          :is-selected="selection.isSelected"
          :is-pending-delete="isPendingDelete"
          :pending-delete-label="t('selection.pendingDelete')"
          :avail-missing-label="t('media.availMissing')"
          :avail-offline-label="t('settings.volOffline')"
          :selection-version="canvasSelectionVersion"
          :is-selection-mode="selection.isSelectionMode.value && !lensActive"
          :scrolling="isScrolling"
          :enable-hover-scale="config.enableHoverScale"
          :show-thumb-info="ui.showThumbInfo"
          :show-drag-handle="ui.showDragHandle"
          :patch-tick="canvasPatchTick"
          :thumb-info-elements="ui.thumbInfoElements"
          :viewport-meta="media.viewportMeta"
          :group-by="ui.groupBy"
          :separator-counts="separatorCounts"
          :lens-active="lensActive"
          :lens-group-label="lensGroupLabel"
          :lens-card-badge-text="lensCardBadgeText"
          :lens-folder-header-lines="lensFolderHeaderLines"
          :theme-token="ui.resolvedThemeId"
          :tint-token="ui.themeTintStrength"
          :text-token="ui.themeTextStrength"
          :glass-background="galleryUsesGlass"
          @cell-click="handleCardClick"
          @cell-contextmenu="(item, e) => onContextMenu(e, item.id)"
          @cell-pointerdown="onCardPointerDown"
          @request-thumb="onRequestThumb"
          @cancel-thumb="onCancelThumb"
          @regenerate-thumb="onRegenerateThumb"
          @cell-favorite="handleFavorite"
          @cell-rate="handleRate"
          @cell-select="selection.toggleSelect"
        />

        <div
          v-else-if="media.totalRows > 0 && bucketActive"
          ref="bucketContentRef"
          class="media-grid__content media-grid__content--bucket"
          :style="{ height: bucketSpacerHeight + 'px', position: 'relative' }"
        >
          <div
            v-for="seg in bucketSegments"
            :key="seg.index"
            class="media-grid__segment"
            :class="{ 'media-grid__segment--loading': seg.state !== 'ready' }"
            :style="{
              position: 'absolute',
              top: seg.start - bucketAnchorDelta + 'px',
              left: 0,
              right: 0,
              height: seg.end - seg.start + 'px',
            }"
          >
            <template v-if="seg.rows">
              <!-- 行体 = 双引擎公共组件 MediaGridRow(T16 收尾抽取,DOM 与原内联模板
                   逐字节等价);offset-y = 段起点。§8.1 browse-only:selection-mode 在
                   镜头态强制 false(行内不渲染 checkbox/选中样式)。 -->
              <MediaGridRow
                v-for="row in seg.rows"
                :key="rowKey(row)"
                :row="row"
                :offset-y="seg.start"
                :gap="GAP"
                :group-by="ui.groupBy"
                :separator-counts="separatorCounts"
                :lens-active="lensActive"
                :lens-group-label="lensGroupLabel"
                :lens-cluster-label="lensClusterLabel"
                :compact-cells="compactCells"
                :selection-mode="compactCells ? false : selection.isSelectionMode.value && !lensActive"
                :cache-dir="cacheDir"
                :pending-delete-label="t('selection.pendingDelete')"
                :is-selected="selection.isSelected"
                :is-pending-delete="isPendingDelete"
                :lens-folder-stats-text="lensFolderStatsText"
                :lens-card-badge-text="lensCardBadgeText"
                :on-card-click="handleCardClick"
                :on-card-pointer-down="onCardPointerDown"
                :on-card-context-menu="onContextMenu"
                :on-request-thumb="onRequestThumb"
                :on-cancel-thumb="onCancelThumb"
                :on-regenerate-thumb="onRegenerateThumb"
                :on-favorite="handleFavorite"
                :on-rate="handleRate"
                :on-select="selection.toggleSelect"
              />
            </template>
          </div>
        </div>

        <!-- 虚拟滚动包装器 (绝对定位) -->
        <div
          v-else-if="media.totalRows > 0"
          class="media-grid__content"
          :style="{ height: spacerHeight + 'px', position: 'relative' }"
        >
          <!-- 渲染层：平移模式（>SAFE_MAX）下其 transform 把可视窗口钉到视口；普通模式下为静态偏移。 -->
          <div
            ref="layerRef"
            class="media-grid__layer"
            :style="{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              willChange: 'transform',
            }"
          >
            <!-- 行体 = 双引擎公共组件 MediaGridRow(T16 收尾抽取,DOM 与原内联模板
                 逐字节等价);offset-y = renderAnchor,行加 will-change(平移模式
                 高频重钉合成层)。§8.1 browse-only:selection-mode 在镜头态强制 false。 -->
            <MediaGridRow
              v-for="row in visibleRows"
              :key="rowKey(row)"
              :row="row"
              :offset-y="renderAnchor"
              :row-will-change="!compactCells"
              :gap="GAP"
              :group-by="ui.groupBy"
              :separator-counts="separatorCounts"
              :lens-active="lensActive"
              :lens-group-label="lensGroupLabel"
              :lens-cluster-label="lensClusterLabel"
              :compact-cells="compactCells"
              :selection-mode="selection.isSelectionMode.value && !lensActive"
              :cache-dir="cacheDir"
              :pending-delete-label="t('selection.pendingDelete')"
              :is-selected="selection.isSelected"
              :is-pending-delete="isPendingDelete"
              :on-card-click="handleCardClick"
              :on-card-pointer-down="onCardPointerDown"
              :on-card-context-menu="onContextMenu"
              :on-request-thumb="onRequestThumb"
              :on-cancel-thumb="onCancelThumb"
              :on-regenerate-thumb="onRegenerateThumb"
              :on-favorite="handleFavorite"
              :on-rate="handleRate"
              :on-select="selection.toggleSelect"
            />
          </div>
        </div>
      </div>

      <!-- T16 B3.2:bucket 引擎自研逻辑滚动条(原生条已隐藏,见 .media-grid--bucket)。
           拇指渲染纯逻辑百分比,与画廊逐帧同步;映射态停稳偿债只动物理 scrollTop,
           对本条零感知——原生拇指「急速滚动往回跳」由此根治。 -->
      <MediaScrollbar
        v-if="bucketActive && media.totalRows > 0"
        :total-height="media.totalHeight"
        :current-y="currentLogicalY"
        :active="isScrolling"
        :min-thumb="config.scrollThumbMinHeight"
        :axis-visible="timelineVisible || minimapVisible"
        :scrubbing="axisScrubbing"
        @jump="onScrollbarJump"
      />

      <!-- 悬浮滚动按钮:迁入 wrapper 内(2026-07-24),消除与轴控制簇(z-101)的层叠遮挡。 -->
      <div v-if="media.totalRows > 0" class="scroll-fab">
        <button
          class="fab-btn"
          @click="scrollGridToTop"
          :title="$t('empty.scrollToTop')"

        >
          ↑
        </button>
        <button
          class="fab-btn"
          @click="scrollGridToBottom"
          :title="$t('empty.scrollToBottom')"

        >
          ↓
        </button>
      </div>
    </div>

    <!-- 无 scrubber 数据时自动挂 minimap；有数据时允许用户在时间轴/minimap 间切换。
         显隐经 <Transition name="axis-slide">:wrapper 宽度折叠(画廊平滑回流)+ 轴体右滑淡出
         (规则见 .axis-slide-*);时间轴↔minimap 的宽度差也走 wrapper 的 width transition。 -->
    <Transition name="axis-slide">
      <div
        v-if="timelineVisible || minimapVisible"
        class="timeline-sidebar-wrapper"
        :class="{ 'timeline-sidebar-wrapper--minimap': minimapVisible }"
      >
        <div class="timeline-sidebar" :class="{ 'timeline-sidebar--minimap': minimapVisible }">
          <!-- 真·时间 scrubber（Part5 §3.3）：消费后端 monthBuckets（时间均布 + 密度热力），
               monthBuckets 空（folder/none 分组）时组件内回退到分隔符圆点。跳转经 @jump→scrollToY。 -->
          <TimelineScrubber
            v-if="timelineVisible && timelineRenderMode === 'dom'"
            :month-buckets="media.layoutSummary?.monthBuckets || []"
            :separators="timelineSeparators"
            :total-height="media.totalHeight"
            :current-y="currentLogicalY"
            :min-thumb="config.scrollThumbMinHeight"
            @jump="scrollToY"
            @scrubbing="axisScrubbing = $event"
          />
          <!-- Canvas 版原型(单画布 + hover 放大镜,§9 二期对比):同 props/emit 可直接替换。 -->
          <TimelineScrubberCanvas
            v-else-if="timelineVisible"
            ref="timelineCanvasRef"
            :month-buckets="media.layoutSummary?.monthBuckets || []"
            :separators="timelineSeparators"
            :total-height="media.totalHeight"
            :current-y="currentLogicalY"
            :min-thumb="config.scrollThumbMinHeight"
            @jump="scrollToY"
            @scrubbing="axisScrubbing = $event"
          />
          <!-- 无缝 minimap 轴(2026-07-17):微缩内容预览 + 视口框,双引擎经 currentLogicalY/
               scrollToY 通吃(与 MediaScrollbar 同理由,对引擎无感知)。 -->
          <MinimapAxis
            v-else-if="minimapVisible"
            :total-height="media.totalHeight"
            :current-y="currentLogicalY"
            :viewport-height="gridViewportHeight"
            :container-width="containerWidth"
            :active="isScrolling"
            :cache-dir="cacheDir"
            :render-mode="ui.minimapRenderMode"
            @jump="onMinimapJump"
            @scrubbing="axisScrubbing = $event"
          />
        </div>
      </div>
    </Transition>

    <!-- 轴控制簇(2026-07-24 迁底栏):chevron/形态钮/渲染药丸 Teleport 到底栏
         #statusbar-axis-outlet(与选区 outlet 同模式),底栏内恒显、不再依赖 hover 才现,
         且天然脱离画廊层叠上下文——不必再靠 z-index 硬压侧栏 wrapper/hover 卡片。
         仅画廊视图激活期挂载(galleryViewActive),进查看器随 Teleport 卸载(与
         selection-outlet 对称)。 -->
    <Teleport to="#statusbar-axis-outlet" v-if="canShowMinimap && galleryViewActive">
      <button
        class="timeline-toggle-btn"
        :class="{ 'is-open': axisOpen }"
        @click="toggleAxis"
        :title="axisToggleTitle"

      >
        <ChevronRight v-if="axisOpen" :size="16" />
        <ChevronLeft v-else :size="16" />
      </button>

      <!-- 同时具备时间数据与 minimap 时显示当前轴形态；点击切换并自动展开目标轴。 -->
      <button
        v-if="canShowScrubber"
        class="axis-mode-btn"
        :class="{ 'is-minimap': activeAxis === 'minimap' }"
        :title="axisModeToggleTitle"

        @click="switchAxisMode"
      >
        {{ activeAxis === 'timeline' ? timelineAxisLabel : t('toolbar.axisMinimap') }}
      </button>

      <!-- 时间轴渲染方式一键切换(DOM ↔ Canvas 原型,§9 二期对比;测试用小药丸,显当前模式)。 -->
      <button
        v-if="showRenderModeDebug && timelineVisible"
        class="timeline-mode-btn"
        :class="{ 'is-canvas': timelineRenderMode === 'canvas' }"
        :title="t('toolbar.switchTimelineRender')"

        @click="toggleTimelineRenderMode"
      >
        {{
          timelineRenderMode === 'canvas'
            ? t('toolbar.timelineRenderCanvas')
            : t('toolbar.timelineRenderDom')
        }}
      </button>

      <!-- Canvas 时间轴专属:视觉形态循环切换(条→包络→热力→光谱)。DOM 形态/minimap 无此概念,
           经 isCanvasTimeline 门控;driven via template ref,状态留组件内(方案 B)。 -->
      <button
        v-if="isCanvasTimeline"
        class="axis-mode-btn"
        :title="
          t('toolbar.switchTimelineVisual', {
            mode: t('toolbar.timelineVisual.' + (timelineCanvasRef?.visualMode ?? 'bars')),
          })
        "

        @click="timelineCanvasRef?.cycleVisualMode()"
      >
        {{ t('toolbar.timelineVisualBadge.' + (timelineCanvasRef?.visualMode ?? 'bars')) }}
      </button>

      <!-- Canvas 时间轴专属:坐标系切换(项累计 ↔ 日历时间)。 -->
      <button
        v-if="isCanvasTimeline && timelineCanvasRef?.coordApplies"
        class="axis-mode-btn"
        :title="
          t('toolbar.switchTimelineCoord', {
            coord: t('toolbar.timelineCoord.' + (timelineCanvasRef?.effectiveCoord ?? 'item')),
          })
        "

        @click="timelineCanvasRef?.toggleCoordMode()"
      >
        {{ t('toolbar.timelineCoordBadge.' + (timelineCanvasRef?.effectiveCoord ?? 'item')) }}
      </button>
    </Teleport>

    <!-- 画廊渲染方式一键切换(§9 T4:DOM ↔ Canvas 原型对比,显当前模式)。 -->
    <button
      v-if="showRenderModeDebug && media.totalRows > 0 && canvasCapable"
      class="gallery-mode-btn"
      :class="{ 'is-canvas': galleryRenderMode === 'canvas' }"
      :title="t('toolbar.switchGalleryRender')"

      @click="toggleGalleryRenderMode"
    >
      {{
        galleryRenderMode === 'canvas'
          ? t('toolbar.galleryRenderCanvas')
          : t('toolbar.galleryRenderDom')
      }}
    </button>

    <ContextMenu
      :visible="ctxMenu.visible"
      :x="ctxMenu.x"
      :y="ctxMenu.y"
      :items="ctxMenu.items"
      @update:visible="ctxMenu.visible = $event"
    />

    <!-- 选择工具栏（动作经 :commands 数据驱动,见 selectionCommands）。
         §8.1 browse-only:重复镜头显式阻断——不在此 v-if 卸载(组件卸载/重挂会让
         useSelectionBarMode.hostActive 的 activate/deactivate 生命周期错拍,docked 形态
         Teleport 门控失灵),改传 forceHidden 把两形态显隐条件共同置 false。 -->
    <SelectionToolbar :commands="selectionCommands" :force-hidden="lensActive" />

    <FolderTreeSelectorDialog
      v-if="moveCopyDialog.isOpen"
      :title="moveCopyDialog.mode === 'move' ? t('common.moveToFolder') : t('common.copyToFolder')"
      @close="moveCopyDialog.isOpen = false"
      @confirm="onMoveCopyConfirm"
    />

    <!-- 拖动媒体到文件夹树时的浮动幽灵。常驻挂载、命令式定位（transform），逐帧移动不重渲染
         网格（问题3）。pointer-events:none，绝不挡住 elementFromPoint。 -->
    <Teleport to="body">
      <div ref="mediaGhostEl" class="media-drag-ghost">
        <span ref="mediaGhostBadgeEl" class="media-drag-ghost__badge">{{ t('common.move') }}</span>
        <ImageIcon :size="14" />
        <span ref="mediaGhostTextEl">{{ t('common.itemCount', { count: 0 }) }}</span>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  onMounted,
  onBeforeUnmount,
  computed,
  nextTick,
  watch,
} from 'vue'
import { invokeIpc } from '../../utils/ipc'
import { logger } from '../../utils/logger'
import { getThumbCacheDir } from '../../utils/thumbCacheDir'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { preloadViewerComponent } from '../../router/viewerRouteLoader'

import { useMediaStore } from '../../stores/mediaStore'
import { useConfigStore } from '../../stores/configStore'
import { useUiStore } from '../../stores/uiStore'
import { useViewStore } from '../../stores/viewStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useViewportDimPriority } from '../../composables/useViewportDimPriority'
import { useRequestQueue } from '../../composables/useRequestQueue'
import { useGalleryPerfProbe } from '../../composables/useGalleryPerfProbe'
import { registerGalleryBenchmark } from '../../composables/usePerformanceMonitor'

// ── P4 结构拆分(2026-07-25):逻辑域下沉为 composable,根组件只留「胶水时序」──────
// 拆分契约:composable 返回值原名导出、响应性(ref/computed)与模板绑定一致,模板 0 改动;
// 越界 scoped CSS(:deep(.media-card*) 等)仍留宿主 MediaGrid.styles.css,不随任何逻辑迁移。
import { useGalleryEmptyState } from '../../composables/useGalleryEmptyState'
import { useGallerySkeleton } from '../../composables/useGallerySkeleton'
import { useGalleryAxisControls } from '../../composables/useGalleryAxisControls'
import { useGalleryVirtualEngine } from '../../composables/useGalleryVirtualEngine'
import { useReflowAnchor } from '../../composables/useReflowAnchor'
import { useGridItemPatching } from '../../composables/useGridItemPatching'
import { useGallerySelectionOps } from '../../composables/useGallerySelectionOps'
import { useGalleryContextMenu } from '../../composables/useGalleryContextMenu'
import { useGalleryKeyboard } from '../../composables/useGalleryKeyboard'
import { useGalleryScrollGate } from '../../composables/useGalleryScrollGate'
import { useGalleryScrollToDir } from '../../composables/useGalleryScrollToDir'
import { useGalleryTauriSync } from '../../composables/useGalleryTauriSync'
import { useLensBrowseGate } from '../../composables/useLensBrowseGate'
import { useLensFocusRestore } from '../../composables/useLensFocusRestore'
import { resolveCardClickAction } from '../../composables/cardPointerAction'
import { useKeepAliveForeground } from '../../composables/useKeepAliveForeground'

import MediaGridRow from './MediaGridRow.vue'
import MediaGridCanvas from './MediaGridCanvas.vue'
import DuplicateLensStatusBar from './DuplicateLensStatusBar.vue'
import {
  activeSeparatorAtY,
  buildViewportLockStyle,
  hasMaterialWidthChange,
  pickRouteReturnViewportLockWidth,
} from './mediaGrid.helpers'
import { useDuplicateLensStatus } from './useDuplicateLensStatus'
import { setDeferThumbLoad } from '../../composables/useThumbLoadGate'
import { thumbhashToAverageColor } from '../../utils/thumbhash'
import TimelineScrubber from './TimelineScrubber.vue'
import TimelineScrubberCanvas from './TimelineScrubberCanvas.vue'
import MediaScrollbar from './MediaScrollbar.vue'
import MinimapAxis from './MinimapAxis.vue'
import SelectionToolbar from './SelectionToolbar.vue'
import ContextMenu from '../common/ContextMenu.vue'
import FolderTreeSelectorDialog from '../common/FolderTreeSelectorDialog.vue'
import UiEmptyState from '../ui/UiEmptyState.vue'
import UiButton from '../ui/UiButton.vue'
import { ImageIcon, FolderPlus, ChevronLeft, ChevronRight, FilterX, CopyCheck, Play, Square } from '@lucide/vue'
import { useSelection } from '../../composables/useSelection'
import { useViewIds } from '../../composables/useViewIds'
import { useHistoryStore } from '../../stores/historyStore'
import { useMediaDragToFolder } from '../../composables/useMediaDragToFolder'
import type { LayoutRow, LayoutRowItem, LayoutRowSeparator, LayoutSeparatorInfo } from '../../types/layout'
import {
  formatLensClusterLabel,
  formatLensFolderStats,
  formatLensGroupLabel,
  getLensFolderStats,
  isLensClusterStart,
  resolveLensCardBadge,
} from './lensSeparator'
import { DEFAULTS } from '../../constants/defaults'
import { IPC } from '../../constants/ipc'

import { scrollCache, galleryScrollKey } from '../../utils/scrollCache'
import { isContentViewerRoute, openMediaRoute } from '../../utils/mediaRoute'
import { isGalleryRoute } from '../../utils/galleryQuery'
import { isWindows } from '../../utils/platform'

// KeepAlive include 按组件名匹配(App.vue 只保活本组件);显式命名不依赖 SFC 文件名推断。
defineOptions({ name: 'MediaGrid' })

const GAP = DEFAULTS.GRID_GAP

/** 让重排后的行尽量复用已有组件；y 会随宽度变化，不能作为唯一 key。 */
function rowKey(row: LayoutRow): string {
  if (row.rowType === 'separator') return `sep-${row.groupId ?? row.separatorLabel}`
  return `row-${row.items[0]?.id ?? row.y}`
}

const ui = useUiStore()
const config = useConfigStore()
const viewStore = useViewStore()
const media = useMediaStore()
const duplicateLens = useDuplicateLensStore()
const queue = useRequestQueue()
const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const contentViewerRoute = computed(() => isContentViewerRoute(route.path))

// 重复镜头激活(duplicateLensStore.isLensActive,§4.4):组头/位次角标/状态条/查看器镜头序/滚动键
// 的统一开关,并承担 §8.1 browse-only gate 的「进入镜头清空普通选区」(useLensBrowseGate watcher)。
// mode 为 URL 镜像,深链直开也会正确置位。
const { lensActive } = useLensBrowseGate()

// 查看器仍保持路由级分包，但在画廊首屏稳定后的空闲帧预取；首次点击只需等待媒体本身，
// 不再把查看器代码下载/解析时间叠加到点击反馈上。点击路径也主动触发一次，覆盖用户立即点图。
let viewerPreloadIdleId: number | null = null
let viewerPreloadTimer: ReturnType<typeof setTimeout> | null = null

function scheduleViewerPreload(): void {
  if (viewerPreloadIdleId !== null || viewerPreloadTimer !== null) return
  if ('requestIdleCallback' in window) {
    viewerPreloadIdleId = window.requestIdleCallback(
      () => {
        viewerPreloadIdleId = null
        preloadViewerComponent()
      },
      { timeout: 1200 },
    )
  } else {
    viewerPreloadTimer = setTimeout(() => {
      viewerPreloadTimer = null
      preloadViewerComponent()
    }, 250)
  }
}

function cancelViewerPreload(): void {
  if (viewerPreloadIdleId !== null && 'cancelIdleCallback' in window) {
    window.cancelIdleCallback(viewerPreloadIdleId)
    viewerPreloadIdleId = null
  }
  if (viewerPreloadTimer !== null) {
    clearTimeout(viewerPreloadTimer)
    viewerPreloadTimer = null
  }
}

const selection = useSelection()
// 布局序全集 id:选区 range/全选/反选/物化 的顺序与全集来源（脱离可视 DOM）。
// 随 layoutVersion 失效重取（见 useGalleryTauriSync + onMounted）。
const viewIds = useViewIds()
const history = useHistoryStore()

// 拖图的「尾随 click 抑制」统一走 selection 的拖拽标志（T5 消除 mediaWasDrag 双轨）:
// 拖图越阈 → selection.markDragMoved();尾随 click 读 selection.wasDrag() 抑制。
// ── 拖动画廊媒体 → 文件夹树（问题5/问题2/问题3） ──────────────────────────────
// 浮动幽灵以命令式定位（对模板 ref 写 transform），使 120fps 拖拽不会触发巨大虚拟网格的
// Vue 重渲染（问题3 —— 此前每次 pointermove 改动本模板读取的响应式 ref，拖拽掉到约 30fps）。
// 只有悬停目标 dir id 是响应式，且仅在真正变化时写。
const mediaGhostEl = ref<HTMLElement | null>(null)
const mediaGhostBadgeEl = ref<HTMLElement | null>(null)
const mediaGhostTextEl = ref<HTMLElement | null>(null)

// 极小单元尺寸下视口可容纳约 1000 个单元；对每个卡片启用 `content-visibility`，
// 让浏览器跳过离屏单元的渲染（绘制/布局），保持滚动顺滑。仅在小尺寸启用：
// content-visibility 会强制 paint 包含，裁掉大尺寸下的 hover 放大+阴影外溢。
const compactCells = computed(() => ui.gridRowHeight < 100)

// 分隔行本身只携带标签与几何,数量从 LayoutSummary 共享索引读取,避免每行重复传输统计字段。
const separatorCounts = computed(() => {
  const counts = new Map<string, number>()
  for (const separator of media.layoutSummary?.separators ?? []) {
    counts.set(separator.groupId ?? separator.label, separator.count)
  }
  return counts
})

function lensGroupLabel(row: LayoutRowSeparator): string {
  return formatLensGroupLabel(
    {
      ordinal: row.duplicateGroupOrdinal,
      memberCount: row.duplicateMemberCount,
      folderCount: row.duplicateFolderCount,
      unitSize: row.duplicateUnitSize,
    },
    row.separatorLabel,
    (params) => t('duplicatesLens.groupLabel', params),
  )
}

function lensClusterLabel(row: LayoutRowSeparator): string | null {
  return formatLensClusterLabel(
    {
      start: row.parentGroupStart,
      ordinal: row.parentGroupOrdinal,
      folderCount: row.parentGroupFolderCount,
      groupCount: row.parentGroupGroupCount,
    },
    (params) => t('duplicatesLens.clusterLabel', params),
  )
}

function formatSummarySeparatorLabel(separator: LayoutSeparatorInfo): string {
  if (separator.separatorKind !== 'duplicateGroup') return separator.label
  return formatLensGroupLabel(
    {
      ordinal: separator.duplicateGroupOrdinal,
      memberCount: separator.duplicateMemberCount,
      folderCount: separator.duplicateFolderCount,
      unitSize: separator.duplicateUnitSize,
    },
    separator.label,
    (params) => t('duplicatesLens.groupLabel', params),
  )
}

const timelineSeparators = computed(() =>
  (media.layoutSummary?.separators ?? []).map((separator) => ({
    ...separator,
    label: formatSummarySeparatorLabel(separator),
  })),
)

// 空状态决策(消息 + 下一步动作的单一源)。
const { emptyState, emptyTitle, emptyDescription, emptyActionLabel, onEmptyAction } =
  useGalleryEmptyState()

// ── 重复镜头状态判定(§9 状态表)───────────────────────────────────────────
// 状态条与镜头空态共用同一单源 useDuplicateLensStatus(内部走 duplicateLensStatus 纯函数);
// 普通画廊(off)时 view 恒为 notAnalyzed,两处模板都被 lensActive 门控不渲染,零开销。
const {
  view: lensStatusView,
  title: lensStatusTitle,
  desc: lensStatusDesc,
  primaryLabel: lensStatusPrimaryLabel,
  runPrimaryAction: onLensPrimaryAction,
} = useDuplicateLensStatus()

const mediaGridWrapperRef = ref<HTMLElement | null>(null)
const gridRef = ref<HTMLElement | null>(null)
const cacheDir = ref('')
const isScrolling = ref(false)
// `/view` 覆盖层存续与查看器返回侧栏动画都固定 wrapper 的当前几何；底层 Canvas/DOM 因而保留
// 原有帧，不会在用户看大图时被 chrome 尺寸改成另一张布局。
const sidebarViewportLockWidth = ref<number | null>(null)
const sidebarViewportLockHeight = ref<number | null>(null)
const sidebarViewportLockStyle = computed(() =>
  buildViewportLockStyle(sidebarViewportLockWidth.value ?? 0, sidebarViewportLockHeight.value ?? 0),
)
let stableViewportWidth = 0
let stableViewportHeight = 0
// 画廊视口高(内容盒):minimap 视口框高度与点击居中的依据;由 gridRef 的 ResizeObserver 回填。
const gridViewportHeight = ref(0)
let scrollTimeout: ReturnType<typeof setTimeout> | null = null

async function refreshCacheDir() {
  try {
    cacheDir.value = await getThumbCacheDir()
  } catch (e) {
    logger.error('[MediaGrid] get_thumb_cache_dir failed', { error: e })
  }
}

// 轴控制簇(时间轴 / minimap 显隐与形态 + DOM↔Canvas 渲染偏好)。
// 🔴 轴红线(写盘键 / 四道防线)全部在 useGalleryAxisControls 内,勿在此复制状态。
const {
  showRenderModeDebug,
  galleryRenderMode,
  timelineRenderMode,
  toggleTimelineRenderMode,
  toggleGalleryRenderMode,
  timelineCanvasRef,
  galleryViewActive,
  canShowScrubber,
  canShowMinimap,
  activeAxis,
  timelineVisible,
  minimapVisible,
  isCanvasTimeline,
  axisOpen,
  toggleAxis,
  timelineAxisLabel,
  axisToggleTitle,
  axisModeToggleTitle,
  switchAxisMode,
} = useGalleryAxisControls()

// 轴侧(时间轴/minimap)拖拽进行中:中转给 MediaScrollbar,标尺线随轴 scrub 同步展开。
const axisScrubbing = ref(false)

// ── 虚拟滚动 ─────────────────────────────────────────────────────────

function getViewKey() {
  // 键拼装委托 scrollCache.galleryScrollKey(单一事实源,勿在此复制拼装)。镜头态传 mode →
  // `lens-groups` 类键,与普通 `all`/`dir-N` 空间隔离,防镜头滚动位串写进普通画廊缓存(§8.3);
  // folders 再按「显示独有项」开关分键(`lens-folders-u1`),开关切换即换集合,滚动位不串。
  return galleryScrollKey(
    viewStore.activeDirectoryId,
    viewStore.activeSmartAlbum,
    duplicateLens.mode,
    duplicateLens.showUniqueItems,
  )
}

const layerRef = ref<HTMLElement | null>(null)
// bucket 分支的内容容器(B2):FLIP/fadeOut 在 bucket 模式以它为根查询 [data-item-id]。
const bucketContentRef = ref<HTMLElement | null>(null)

// KeepAlive 在屏标志:驱动布局源的取数闸门(详见 useGalleryVirtualEngine 的 onScreen 注)。
// 初值 true:首次挂载时 onMounted 的首算先于 onActivated,必须放行。
let gridOnScreen = true

// 滚动闸门 + 程序化滚动守卫(纯判定状态机;onGridScroll 主体按拆分方案 §3.3 方案 A 留本组件)。
const {
  beginProgrammaticScroll,
  endProgrammaticScroll,
  isProgrammaticScroll,
  noteAxisJump,
  sampleScrollGate,
  clearGateTimer,
} = useGalleryScrollGate({ rowHeight: () => ui.gridRowHeight })

// 双引擎交汇层(方案 A ↔ bucket 分段 + Canvas 派生读数面)。🔴 见该文件头注的三条红线。
const {
  containerWidth,
  layoutSource,
  bucketActive,
  bucketScroll,
  visibleRows,
  updateVisible,
  onScroll,
  spacerHeight,
  renderAnchor,
  logicalToPhysical,
  bucketSegments,
  bucketAnchorDelta,
  bucketSpacerHeight,
  currentLogicalY,
  activeRows,
  canvasCapable,
  canvasActive,
  canvasRows,
  canvasPatchTick,
  bumpCanvasPatchTick,
  canvasSpacerHeight,
  canvasSelectionVersion,
  compute,
  onResize,
  cancelPendingResize,
  scrollToY,
} = useGalleryVirtualEngine({
  gridRef: () => gridRef.value,
  layerRef: () => layerRef.value,
  // route-return 锁窗、`/view` 覆盖层存续与 KeepAlive 失活均不允许布局 IPC；结束后只按最终
  // 宽度回放一次，避免旧防抖或查看器 chrome 的中间几何提交。
  onScreen: () => gridOnScreen && !contentViewerRoute.value && !ui.routeReturnSidebarTransitioning,
})

// canvas 是原型:偏好为 canvas 时若 canvasCapable 不满足(iOS/超大库)仍自动回退 DOM。
// 判据单点在 useGalleryVirtualEngine(两引擎的取行窗也按它自驱,§4.3 S3),此处只别名消费。
const canvasMode = canvasActive
// 画廊底面与 html[data-glass] 同源：只在 Windows 原生 Mica/Acrylic 已就绪时透出背板。
// 非玻璃模式保留 Canvas 的不透明合成快路径，避免把性能开销带给默认浏览体验。
const galleryUsesGlass = computed(() => isWindows && ui.windowMaterial !== 'none')
// 全局性能面板只持有稳定 getter；失活时注销，避免在查看器路由误跑画廊基准。
// (与 onActivated/onDeactivated 同属 KeepAlive 胶水,故留根组件。)
let unregisterGalleryBenchmark: (() => void) | null = null

function activatePerformanceBenchmark() {
  if (unregisterGalleryBenchmark) return
  unregisterGalleryBenchmark = registerGalleryBenchmark({
    getScroller: () => gridRef.value,
    getContext: () => ({
      galleryMode: canvasMode.value ? 'canvas' : 'dom',
      galleryPreference: galleryRenderMode.value,
      timelineMode: canShowScrubber.value ? timelineRenderMode.value : 'hidden',
      groupBy: ui.groupBy,
      totalItems: media.layoutSummary?.totalItems ?? media.totalItems,
      bucketScroll: bucketActive.value,
    }),
  })
}

function deactivatePerformanceBenchmark() {
  unregisterGalleryBenchmark?.()
  unregisterGalleryBenchmark = null
}

// 首屏骨架屏(S5)可见性防抖 + 铺满视口。
const { showSkeleton, skeletonCount } = useGallerySkeleton({
  containerWidth: () => containerWidth.value,
  viewportHeight: () => gridViewportHeight.value,
})

// 重排锚点(行高/分组/排序/宽度/布局模式整体重排时把浏览项钉在屏内)。
const { captureReflowAnchor, restoreReflowAnchor } = useReflowAnchor({
  gridRef: () => gridRef.value,
  activeRows,
  currentLogicalY: () => currentLogicalY.value,
  getViewKey,
  bucketActive: () => bucketActive.value,
  scrollToLogicalY: (y) => bucketScroll.scrollToLogicalY(y),
  logicalToPhysical,
})

// 镜头排列切换的焦点恢复(§8.1):切换 groups/folders 或开关独有项时按聚焦卡片 item ID 恢复滚动位
// 与焦点;id 已不在新布局则回顶部并聚焦镜头状态条主标题。挂进布局重算的滚动恢复链(见下方
// restoreReflowOrLensFocus 注),机制通用,P3 folders 排列切换直接生效。
const { restoreLensFocus } = useLensFocusRestore({
  gridRef: () => gridRef.value,
  bucketActive: () => bucketActive.value,
  scrollToLogicalY: (y) => bucketScroll.scrollToLogicalY(y),
  logicalToPhysical,
  getViewKey,
})

// 布局重算的滚动恢复链单一顺序:reflow 锚点 → 镜头焦点锚点(§8.1)→ (useGalleryTauriSync 内的)
// scrollCache 回退。镜头排列切换不捕 reflow 锚点(其 watch 面不含镜头键,且锚点按 viewKey 校验、
// 排列切换即换键)而必然落入镜头焦点恢复;非镜头路径 pending 恒空、restoreLensFocus 零开销直返。
async function restoreReflowOrLensFocus(): Promise<boolean> {
  if (await restoreReflowAnchor()) return true
  return restoreLensFocus()
}

// 可视项就地 patch(乐观 UI 回写的单一实现)。
const { patchVisibleRating, patchVisibleFavorite, patchVisibleColorLabel, patchVisibleSelected } =
  useGridItemPatching({
    activeRows,
    bumpCanvasPatchTick,
    isSelected: (id: number) => selection.isSelected(id),
  })

// 选区批量操作全集(含暂存删除 undo/redo + FLIP 重排时序契约)。
const {
  moveCopyDialog,
  handleFavorite,
  handleRate,
  selectionDescriptor,
  isPendingDelete,
  startBatchMove,
  startBatchCopy,
  startExportSelection,
  selectionCommands,
  onMoveCopyConfirm,
} = useGallerySelectionOps({
  compute,
  updateVisible,
  bucketActive: () => bucketActive.value,
  whenSettled: () => bucketScroll.whenSettled(),
  flipRootEl: () => (bucketActive.value ? bucketContentRef.value : layerRef.value),
  patchVisibleRating,
  patchVisibleFavorite,
  patchVisibleColorLabel,
  patchVisibleSelected,
})

// ── Context Menu ───────────────────────────────────────────────────────────
const { ctxMenu, onContextMenu } = useGalleryContextMenu({
  activeRows,
  // §8.1 browse-only:镜头态不构建右键菜单,谓词由宿主注入(composable 不直接依赖镜头 store)。
  lensActive: () => lensActive.value,
  startBatchMove,
  startBatchCopy,
  startExportSelection,
})

// 画廊媒体拖拽到文件夹树（T18：整簇抽到 useMediaDragToFolder）。幽灵 DOM + ctxMenu 仍在本模板，
// 经 deps 注入其 ref；composable 持拖拽状态机与命中/落点逻辑，只回吐模板绑定的 onCardPointerDown。
const { onCardPointerDown } = useMediaDragToFolder({
  selection,
  ui,
  history,
  ctxMenu,
  // §8.1 browse-only:镜头态禁用框选(反转扫选)与拖图起手,谓词由宿主注入。
  lensActive: () => lensActive.value,
  ghostEl: mediaGhostEl,
  ghostBadgeEl: mediaGhostBadgeEl,
  ghostTextEl: mediaGhostTextEl,
})

// 语义子视图返回栏（人物 / 收藏夹）+ document 级键盘处理(ESC / 数字键批量评分)。
const { backBar, exitToOverview, onKeyDown } = useGalleryKeyboard({
  selectionDescriptor,
  patchVisibleSelected,
  compute,
  updateVisible,
  // §8.1 browse-only:镜头态旁路 document 级选择语义(Ctrl+A/ESC 清选区),谓词由宿主注入。
  lensActive: () => lensActive.value,
})

/** `/view` 保留本画廊 DOM 时，文档级键盘分发必须让给覆盖层查看器。 */
function onDocumentKeyDown(event: KeyboardEvent): void {
  if (contentViewerRoute.value) return
  onKeyDown(event)
}

// 自研逻辑滚动条的拖拽/轨道点击跳转(B3.2,仅 bucket 引擎挂载):即时落点——拖拽中
// 平滑动画只会让拇指跟手性变差;远跳/近跳分流由 scrollToLogicalY 统一处理。
// 拖拽链关闸细节见 useGalleryScrollGate.noteAxisJump。
function onScrollbarJump(y: number) {
  noteAxisJump()
  void bucketScroll.scrollToLogicalY(y)
}

// minimap 轴跳转:与 onScrollbarJump 同一拖拽链关闸(共用时间戳,释放路径在
// sampleScrollGate 的 scrollbarDragUntil 判定,与引擎无关);经 scrollToY 双引擎通吃,
// 即时落点(smooth=false——拖拽/滚轮要跟手,VSCode minimap 点击也是瞬移语义)。
function onMinimapJump(y: number) {
  noteAxisJump()
  scrollToY(y, false)
}

// B3.1 输入源分类转发(仅 bucket 引擎消费):wheel/滚动键/触屏盖 1:1 印记,与滚动条
// 拖动区分;wheel 另担映射态物理钉边后的边缘续滚。全部不阻断,对原生滚动零干预。
function onGridWheel(e: WheelEvent) {
  if (contentViewerRoute.value) return
  if (bucketActive.value) bucketScroll.onWheel(e)
}
function onGridKeydown(e: KeyboardEvent) {
  if (contentViewerRoute.value) return
  if (bucketActive.value) bucketScroll.onKeydown(e)
}
function onGridTouchmove() {
  if (contentViewerRoute.value) return
  if (bucketActive.value) bucketScroll.onTouchmove()
}

function onGridScroll() {
  if (contentViewerRoute.value) return
  let internalHop = false
  if (bucketActive.value) internalHop = bucketScroll.onScroll()
  else onScroll()
  if (!isScrolling.value) {
    isScrolling.value = true
  }

  // B(快滚甩滚低保真):按物理滚动速度置加载闸门。飞掠中(velocity > 阈值)抑制新缩略图
  // 解码/请求的启动,把段落地爆发里的解码工作挪到降速/停稳帧;慢滚(≤阈值)照常出图。
  // 停稳兜底见下方 scrollTimeout(必放行)。用 performance.now() 取单调时钟,免受系统时间跳变。
  const st = gridRef.value?.scrollTop ?? 0
  const now = performance.now()
  sampleScrollGate(st, now, internalHop)

  // 飞滚期间跳过 画廊→侧栏 的文件夹联动，否则树会来回跳（问题3）、被点击的目标会被挤出
  // 可视区（问题2）。目标已在 scrollToDir() 中钉好；滚动停稳后恢复真实滚动的联动。
  // 画廊→侧栏文件夹高亮联动:仅 folder 分组需要,date/none 直接跳过(此前无谓地每帧线性
  // 扫全部 separators,深处大库快滚时是一处逐 scroll-event 的 O(深度) 开销 → 症状①的次要来源)。
  // folder 分组下用 activeSeparatorAtY 二分定位当前滚动位所在的文件夹分隔符(O(log n))。
  // separator y 是逻辑坐标 → 与 currentLogicalY 比较(而非物理 scrollTop,平移态二者不同)。
  if (!isProgrammaticScroll() && gridRef.value) {
    const separators = media.layoutSummary?.separators
    if (ui.groupBy === 'folder' && separators && separators.length) {
      const sep = activeSeparatorAtY(separators, currentLogicalY.value + 100)
      ui.scrolledDirectoryId = sep?.groupId ? parseInt(sep.groupId, 10) : null
    } else {
      ui.scrolledDirectoryId = null
    }
  }

  if (scrollTimeout !== null) clearTimeout(scrollTimeout)
  scrollTimeout = setTimeout(() => {
    isScrolling.value = false
    // B:停稳兜底放行加载闸门——无论最后一帧速度如何,停下后必须让可视窗口补起被推迟的图。
    setDeferThumbLoad(false)
    // 平滑滚动已停稳 — 恢复 画廊→侧栏 联动。
    endProgrammaticScroll()
    if (gridRef.value) {
      // bucket 缓存逻辑 y(B3 映射态下物理 scrollTop 不自足);方案 A 仍缓存物理。
      scrollCache.set(
        getViewKey(),
        bucketActive.value ? currentLogicalY.value : gridRef.value.scrollTop,
      )
    }
  }, 150)
}

function scrollGridToTop() {
  if (!gridRef.value) return
  gridRef.value.scrollTo({ top: 0, behavior: 'smooth' })
}

function scrollGridToBottom() {
  if (!gridRef.value) return
  gridRef.value.scrollTo({ top: gridRef.value.scrollHeight, behavior: 'smooth' })
}

// ── 布局 ─────────────────────────────────────────────────────────────────
// 挂载期的布局初始化 + ResizeObserver 是「把上面十余个 composable 的初始化时序拼起来」的
// 胶水(拆分方案 §2.1 明列不下沉):改一行时序即可能复现骨架闪帧 / 滚动位丢失的既往真机问题。

let resizeObserver: ResizeObserver | null = null

function measureContainerWidth(): number {
  const el = gridRef.value
  if (!el) return 0
  const cs = getComputedStyle(el)
  const pad = (parseFloat(cs.paddingLeft) || 0) + (parseFloat(cs.paddingRight) || 0)
  return el.clientWidth - pad
}

function measureViewportWidth(): number {
  const width = mediaGridWrapperRef.value?.getBoundingClientRect().width ?? 0
  return width > 0 ? width : 0
}

function measureViewportHeight(): number {
  const height = mediaGridWrapperRef.value?.getBoundingClientRect().height ?? 0
  return height > 0 ? height : 0
}

function lockRouteReturnViewport(): void {
  sidebarViewportLockWidth.value = pickRouteReturnViewportLockWidth(
    measureViewportWidth(),
    stableViewportWidth,
  )
  const currentHeight = measureViewportHeight()
  sidebarViewportLockHeight.value =
    currentHeight > 0 ? currentHeight : stableViewportHeight > 0 ? stableViewportHeight : null
}

function applyContainerWidth(width: number, scheduleResize = true): boolean {
  if (!hasMaterialWidthChange(containerWidth.value, width)) return false
  // 宽度真实改变才押锚；route-return 的动画中间帧不会进入此处。
  captureReflowAnchor()
  containerWidth.value = width
  if (scheduleResize) onResize(width)
  return true
}

/** 解锁后的唯一同步点：deferred 计算与最终宽度变化二选一，避免同一路径双发 IPC。 */
async function settleRouteReturnLayout(): Promise<void> {
  if (!gridOnScreen || ui.routeReturnSidebarTransitioning) return
  const finalWidth = measureContainerWidth()
  const widthChanged = applyContainerWidth(finalWidth, false)
  const recomputed = await layoutSource.flushIfDeferred()
  if (ui.routeReturnSidebarTransitioning) return
  if (recomputed) {
    updateVisible()
  } else if (widthChanged) {
    onResize(finalWidth)
  }
}

function installResizeObserver(): void {
  const el = gridRef.value
  if (typeof ResizeObserver === 'undefined' || !el) return
  resizeObserver?.disconnect()
  resizeObserver = new ResizeObserver((entries) => {
    const entry = entries[0]
    if (!entry) return
    // 视口高只影响 minimap 几何，不触发布局重算。
    const vh = entry.contentRect.height
    if (vh > 0) gridViewportHeight.value = vh
    // `/view` 覆盖层存续和查看器返回的侧栏动画都可能逐帧报告中间几何；底层画廊必须保留
    // 打开查看器前的帧，只在 AppShell 解锁后读最终宽度。
    if (contentViewerRoute.value || ui.routeReturnSidebarTransitioning) return
    const width = entry.contentRect.width
    const viewportWidth = measureViewportWidth()
    if (viewportWidth > 0) stableViewportWidth = viewportWidth
    const viewportHeight = measureViewportHeight()
    if (viewportHeight > 0) stableViewportHeight = viewportHeight
    applyContainerWidth(width)
  })
  resizeObserver.observe(el)
}

watch(
  [
    () => ui.routeReturnSidebarTransitioning,
    () => ui.routeReturnSidebarTransitionGeneration,
  ],
  ([transitioning]) => {
    if (transitioning) {
      cancelPendingResize()
      lockRouteReturnViewport()
      return
    }
    // 查看器仍覆盖在上方时不允许旧 transitionend 提前解锁底层帧。
    if (contentViewerRoute.value) return
    sidebarViewportLockWidth.value = null
    sidebarViewportLockHeight.value = null
    void nextTick(() => {
      void settleRouteReturnLayout()
    })
  },
)

// 前台装配(幂等)：document 级键盘监听是选择态 ESC / 数字键批量评分 / Ctrl+A 的入口,
// galleryViewActive 是底栏轴控件(时间轴 / minimap)Teleport 的门控,性能基准不得在后台空跑。
// mounted 首拍与 activated 共用本入口——异步子树收不到首拍 activated(见 useKeepAliveForeground 头注),
// 只靠激活会把冷启动首个会话的这些装配整段留在缺席状态。
function enterGalleryForeground(): void {
  gridOnScreen = true
  galleryViewActive.value = true
  activatePerformanceBenchmark()
  document.addEventListener('keydown', onDocumentKeyDown)
}

/**
 * 焦点收回:方向键/PageUp/Down 滚动是浏览器对「聚焦滚动容器」的原生行为(见模板 tabindex 注释),
 * document 级监听救不了它。等下一拍让路由交换/挂载收尾后,仅在焦点确实无主(body)时收回,
 * 不抢输入框/侧栏按钮/首启向导等真实焦点持有者。
 */
function focusGridIfUnowned(): void {
  void nextTick(() => {
    if (!gridOnScreen || contentViewerRoute.value) return
    const activeElement = document.activeElement
    if (gridRef.value && (activeElement === null || activeElement === document.body)) {
      gridRef.value.focus({ preventScroll: true })
    }
  })
}

/** 前台副作用摘除(失活 / `/view` 覆盖层接管共用;幂等)。 */
function leaveGalleryForeground(): void {
  cancelPendingResize()
  gridOnScreen = false
  galleryViewActive.value = false
  deactivatePerformanceBenchmark()
  document.removeEventListener('keydown', onDocumentKeyDown)
  // 失活节点被 KeepAlive 移入缓存容器时不再观察其几何,避免 0px/过渡宽度变成回画廊的 deferred 重排。
  resizeObserver?.disconnect()
  resizeObserver = null
  clearGateTimer()
  // 失活后 150ms 停稳回调若照跑,会拿摘离 DOM 的 scrollTop=0 覆写 scrollCache,恢复位即丢。
  if (scrollTimeout !== null) {
    clearTimeout(scrollTimeout)
    scrollTimeout = null
  }
  isScrolling.value = false
  setDeferThumbLoad(false)
}

/** 真实复激活:观察几何 + 焦点收回 + 从 scrollCache 恢复滚动位(首帧即在恢复位算,避免 0→saved 闪跳)。 */
function onGalleryReactivate(): void {
  if (ui.routeReturnSidebarTransitioning) lockRouteReturnViewport()
  installResizeObserver()
  // 关查看器返回时旧组件卸载会把焦点掉到 body,网格从此对方向键失聪。
  focusGridIfUnowned()
  const saved = scrollCache.get(getViewKey()) || 0
  if (gridRef.value && saved > 0) {
    // bucket 缓存的是逻辑 y(映射态物理不自足),经 scrollToLogicalY 还原;方案 A 直写物理。
    if (bucketActive.value) void bucketScroll.scrollToLogicalY(saved)
    else gridRef.value.scrollTop = saved
  }
  updateVisible()
  // route-return 锁窗内不回放 deferred:等最终宽度落定后由唯一同步点决定是回放还是尺寸重排。
  if (!ui.routeReturnSidebarTransitioning) void settleRouteReturnLayout()
}

// KeepAlive 前台/后台编排(2026-07-10 深审问题1治本 + 2026-09-12 异步子树首拍缺席治本):
// `/view` 由路由驱动的覆盖层呈现,本网格始终留在 DOM;`/doc`、`/audio` 及异组件页仍会失活。
// 覆盖层的键盘/性能/几何冻结由 contentViewerRoute watcher 收口,失活则由本编排收口。
const galleryForeground = useKeepAliveForeground({
  // 画廊路由且非 `/view` 覆盖层存续:异组件路由的「后台」不是前台,异步子树在其间 resolve
  // 不得挂上文档级监听。
  isForeground: () => isGalleryRoute(route.path) && !contentViewerRoute.value,
  enter: enterGalleryForeground,
  leave: leaveGalleryForeground,
  onReactivate: onGalleryReactivate,
})

function enterContentViewerBackgroundMode(): void {
  // 先摘前台副作用(cancelPendingResize 在其内),再按最终几何固定 wrapper。
  galleryForeground.enterBackground()
  lockRouteReturnViewport()
}

function restoreGalleryAfterContentViewer(): void {
  if (!isGalleryRoute(route.path)) return
  // 覆盖层存续期底层画廊 DOM 未卸载、滚动位即当前帧,故只补前台装配,不走复激活的滚动位恢复。
  // 走统一入口而非直接调 enterGalleryForeground:同一条路径既服务路由 watcher 也服务激活钩子,
  // 分叉会让两条路径的前后台状态判断各说各话。
  galleryForeground.enterForeground()
  focusGridIfUnowned()
  void nextTick(() => {
    if (!gridOnScreen || contentViewerRoute.value) return
    installResizeObserver()
    // AppShell 的同步 route watcher 会在本 tick 内登记侧栏过渡；延后一拍再决定是否自行解锁，
    // 避免组件 watcher 注册顺序把底层画廊暴露给动画首帧。
    if (!ui.routeReturnSidebarTransitioning) {
      sidebarViewportLockWidth.value = null
      sidebarViewportLockHeight.value = null
      void settleRouteReturnLayout()
    }
  })
}

watch(
  contentViewerRoute,
  (active, wasActive) => {
    if (active) {
      enterContentViewerBackgroundMode()
      return
    }
    if (wasActive) restoreGalleryAfterContentViewer()
  },
  { flush: 'sync' },
)

onMounted(async () => {
  // 前台/后台装配由 useKeepAliveForeground 在本钩子之前的挂载首拍完成(见上)。
  // 冷启动的焦点收回同样归此处:异步子树拿不到首拍 activated,等到激活才 focus 就是永久缺位。
  focusGridIfUnowned()
  scheduleViewerPreload()
  // 缩略图缓存目录允许在设置中自定义；必须以后端运行时配置为准。
  await refreshCacheDir()

  // 立即读取容器宽度；若从查看器返回的侧栏过渡已开始，先固定 wrapper，避免首个 observer
  // 回调采到动画中间值。
  if (gridRef.value) {
    if (contentViewerRoute.value || ui.routeReturnSidebarTransitioning) lockRouteReturnViewport()
    containerWidth.value = measureContainerWidth()
    const viewportWidth = measureViewportWidth()
    if (viewportWidth > 0) stableViewportWidth = viewportWidth
    const viewportHeight = measureViewportHeight()
    if (viewportHeight > 0) stableViewportHeight = viewportHeight
    installResizeObserver()
  } else {
    logger.warn('[MediaGrid] onMounted: gridRef is null!')
  }

  // 初始布局计算 — 在知道宽度之后

  // 在 compute 前消费 dirty 标志，避免通过 layoutDirty watcher
  // 触发多余的二次重算（Vue watch 默认非 immediate，不会在挂载时触发）。
  media.consumeLayoutDirty()

  // 深链查看器或 KeepAlive 失活期只登记一次 deferred；底层 LayoutCache 保留当前帧，
  // 返回画廊时由 settleRouteReturnLayout/flushIfDeferred 用最终视图补算。
  if (gridOnScreen && !contentViewerRoute.value && !ui.routeReturnSidebarTransitioning) {
    await compute()
  } else {
    layoutSource.requestCompute()
  }

  // 初始/重挂载时主动拉一次布局序全集（layoutVersion watcher 仅在变化时触发,挂载不触发）。
  // 镜头是 browse-only，查看器顺序由后端按 layoutVersion 一步解析，不能物化全量 flat_ids。
  if (!lensActive.value) void viewIds.ensureFresh(media.layoutVersion)

  // 顶栏重构 P4-5:重挂载恢复滚动位。scrollCache 是模块级 Map(跨重挂存活,见 scrollCache.ts
  // 头注),此前仅在 layoutVersion watcher(布局变化时)读回;而组件重挂载(同视图 component
  // 重建——从 /collections·/persons·/doc·/audio 等异组件路由返回)走 onMounted 却因版本未变
  // 不触发该 watcher → 落回 scrollTop=0 丢位。覆盖层时代此路径被「Teleport 保活网格永不卸载」
  // 掩盖;P4 看图台路由化后开图即卸载网格,本恢复是「返回网格保位不重排」的支柱(计划 P4-5)。
  // 时序:await nextTick 等 spacerHeight 落到 DOM 后再设 scrollTop,否则大值被容器裁到当前可
  // 视高(T16 教训:物理 scrollTop 须在总高就绪后设)。恢复须先于 updateVisible,使首帧可视窗口
  // 就在恢复位算,避免 0→saved 的双渲染闪跳。saved=0(首次访问无缓存)时跳过,新网格本就在顶。
  await nextTick()
  if (gridRef.value) {
    const saved = scrollCache.get(getViewKey()) || 0
    if (saved > 0) {
      if (bucketActive.value) await bucketScroll.scrollToLogicalY(saved)
      else gridRef.value.scrollTop = saved
    }
  }

  updateVisible()
})

// ── 缩略图请求处理 ──────────────────────────────────────────────

function onCancelThumb(id: number) {
  queue.cancel(id)
}

async function onRequestThumb(id: number) {
  try {
    const result = await queue.request(id)
    // 查找并修补 visibleRows 中的项目
    for (const row of activeRows()) {
      if (row.rowType !== 'normal') continue
      const item = row.items.find((it) => it.id === id)
      if (item) {
        item.thumbStatus = result.thumbStatus
        item.thumbPath = result.thumbPath
        // 缩略图生成结果带回 thumbhash → 前端算占位色回填(单项、罕发,与后端 hydrate 的批量
        // 占位色在 ±2/通道内一致;此刻 status 多已迁 1,真图随即替换占位,差异不可辨)。
        item.placeholderColor = result.thumbhash ? thumbhashToAverageColor(result.thumbhash) : null
        bumpCanvasPatchTick()
        break
      }
    }
  } catch {
    // request cancelled or failed — leave placeholder
    // 请求取消或失败 — 保留占位符
  }
}

// 懒自愈：某项 thumb_status=1 但封面文件已被 LRU 缓存驱逐（持续 404）。MediaThumb 探针识别到
// 「真·加载失败」后上抛本事件。乐观地把该项标为待生成（占位替代裂图、停止对已删 URL 的重试），
// 再调后端复位（media_items + 封面派生行退回 pending，并发 db:media_enriched 让派生流水线重跑封面）。
async function onRegenerateThumb(id: number) {
  for (const row of activeRows()) {
    if (row.rowType !== 'normal') continue
    const item = row.items.find((it) => it.id === id)
    if (item) {
      item.thumbStatus = 0
      item.thumbPath = null
      bumpCanvasPatchTick()
      break
    }
  }
  try {
    await invokeIpc(IPC.REGENERATE_MISSING_THUMB, { id })
  } catch {
    // 自愈尽力而为：失败留待下次滚回视口 / 下次启动 reconcile 收敛。
  }
}

// 可视窗口优先取尺寸已抽到 useViewportDimPriority（自包含 feature：注入 visibleRows/isScrolling
// 与 recompute/refresh，内部自持去重集 + 防抖调度，resetKey 变化即清去重集）。
useViewportDimPriority({
  // B2:行源引擎感知——bucket 模式喂已挂载段的行(与 activeRows() 同源;computed 亦是
  // Ref,内部 watch 随段挂载/换代自然触发)。refresh 在 bucket 模式为 no-op(方案 A 门控),
  // 重算后的段回填由 layoutVersion watch 自驱。
  visibleRows: computed(() =>
    bucketActive.value ? bucketScroll.mountedRows() : visibleRows.value,
  ),
  isScrolling,
  recompute: compute,
  refresh: updateVisible,
  // 重排/优先级去重集的 resetKey 含镜头模式与「显示独有项」开关(§8.3):镜头⇄普通/排列
  // 切换/开关切换都是集合切换,旧的「已请求尺寸」去重集必须作废,否则新布局首帧拿不到视口尺寸。
  resetKey: () => [
    viewStore.activeDirectoryId,
    viewStore.activeSmartAlbum,
    duplicateLens.mode,
    duplicateLens.showUniqueItems,
  ],
})

// dev 门控性能探针(2026-07-09 画廊极密网格方案 §10):localStorage['scrollery.debug.perfProbe']=1
// 开启,默认零成本(关闭即死代码、不注册监听)。getter 注入,不改模板与热路径 render。
useGalleryPerfProbe({
  gridEl: () => gridRef.value,
  isScrolling: () => isScrolling.value,
  isSelectionMode: () => selection.isSelectionMode.value,
})

onBeforeUnmount(() => {
  cancelViewerPreload()
  // 性能基准/闸门/定时器/观察者全归 useKeepAliveForeground 的卸载出口(leaveGalleryForeground):
  // 该出口即 F4 契约的落点——闸门是模块级单例,卸载不复位就会钉死在 true,下次进画廊全员
  // deferred 直到用户滚动。
})

// ── 详情 ─────────────────────────────────────────────────────────────────

// 事件类型收 MouseEvent | KeyboardEvent(R1-8):键盘激活(Enter/Space)完整复用点击语义——
// 两类事件都携带 ctrlKey/metaKey/shiftKey,Ctrl+Enter 切选中、Shift+Enter 范围选中随之免费成立。
function handleCardClick(item: LayoutRowItem, event: MouseEvent | KeyboardEvent) {
  const id = item.id
  // 拖拽（框选或拖图）刚结束 → 吞掉尾随的单击,避免「拖完又触发单击」(开详情/翻转选中)。
  // 单一标志统一判定,下次交互起手 onCardPointerDown→beginInteraction() 复位（T5）。
  if (selection.wasDrag()) return

  // click 分流判定抽为纯函数 resolveCardClickAction(与 pointerdown 分流同域 cardPointerAction.ts,
  // 穷举单测钉死)。§8.1 browse-only:镜头态下 Ctrl/Cmd+Click 与 Shift+Click 不进入选择/范围选择,
  // 一律走打开查看器路径;分流返回 open 时落到下方查看器导航。
  const action = resolveCardClickAction({
    lensActive: lensActive.value,
    modifier: event.ctrlKey || event.metaKey,
    shift: event.shiftKey,
    selectionMode: selection.isSelectionMode.value,
    compact: compactCells.value,
  })
  if (action === 'toggle') {
    // Ctrl/Cmd+单击：切换选中
    selection.toggleSelect(id)
    return
  }
  if (action === 'range') {
    // Shift+单击：范围选中
    selection.selectRange(selection.lastClickedId.value, id)
    return
  }
  if (action === 'select') {
    // T0(极密网格 §5.4):compact(60px 类)下已移除逐格 checkbox(60px 尺度点不动),故选择态里
    // 普通点击直接切换选中(替代「点 checkbox」);非 compact 保留「点击开图、点 checkbox 选」原交互。
    // 进入选择态仍靠 Ctrl/Cmd+Click 或框选;此分支仅在「已在选择态 + compact」时改写普通点击语义。
    selection.toggleSelect(id)
    return
  }

  // 文档进入专用阅读器路由（§5.1）；音频进入播放器路由（§3.6）；图/视进入统一查看器路由 /view/:id。
  // 分发与 push/replace 分流统一在 openMediaRoute(与侧栏树 onFileClick 共用,2026-07-10 深审问题2)。
  // 重复镜头(§8.2):打开查看器前只记录布局版本/总数；上一项/下一项由后端布局缓存
  // 按 itemId+offset 解析，避免通过 GET_VIEW_IDS 把百万级 flat_ids 传到前端。
  if (lensActive.value) media.setLensNavContext(media.layoutVersion, media.viewTotalItems)
  if (item.mediaType === 'image' || item.mediaType === 'video') preloadViewerComponent()
  openMediaRoute(router, id, item.mediaType)
}

// 文件夹头统计行文案(§7.2):「{n} 重复 · {n} 尚未确认 · {n} 独有（已显示/已隐藏）」——
// 拼装逻辑单源在 lensSeparator(纯函数,lensSeparator.spec 锁测试),此处只注入 t;
// 仅 duplicateFolder 行返回非空,其余行/普通画廊返回 null(行组件不渲染统计行)。
function lensFolderStatsText(row: LayoutRow): string | null {
  const stats = getLensFolderStats(row)
  if (!stats) return null
  return formatLensFolderStats(stats, (key, n) => t(`duplicatesLens.${key}`, { n }))
}

// 镜头卡片徽标文本(§6.2/§7.3):选择逻辑单源在 lensSeparator.resolveLensCardBadge——
// groups= M/N(第 M/共 N 项);folders 重复卡=「组 N」、尚未确认= '?' 兜底文本
// (DOM 模板按 bucket 换 CircleHelp 图标,canvas 直接绘 '?' 文本);独有/普通画廊= null。
function lensCardBadgeText(item: LayoutRowItem): string | null {
  const badge = resolveLensCardBadge(item)
  if (!badge) return null
  switch (badge.kind) {
    case 'unconfirmed':
      return '?'
    case 'groupBadge':
      return t('duplicatesLens.badgeGroup', { n: badge.ordinal })
    case 'memberPosition':
      return `${badge.ordinal}/${badge.count ?? ''}`
  }
}

// canvas 文件夹头行文字组(§7.2):DOM(canvas overlay 同款)两/三行结构的文本由宿主组装。
function lensFolderHeaderLines(row: LayoutRowSeparator): { cluster: string | null; stats: string } | null {
  const stats = getLensFolderStats(row)
  if (!stats) return null
  // 簇首文件夹头并入关联簇标签(§7.2);数值由后端布局提供，句式由当前 locale 生成。
  return {
    cluster: isLensClusterStart(row) ? lensClusterLabel(row) : null,
    stats: formatLensFolderStats(stats, (key, n) => t(`duplicatesLens.${key}`, { n })),
  }
}

function onFolderStatsChanged() {
  // 重取画廊以显示新入库项(若适用于当前视图)
  layoutSource.requestCompute()
  media.loadStats()
}

onMounted(() => {
  window.addEventListener('folder-stats-changed', onFolderStatsChanged)
})
onBeforeUnmount(() => {
  window.removeEventListener('folder-stats-changed', onFolderStatsChanged)
})

// Tauri 事件(增强/卷插拔)+ 布局脏标/版本/视口元数据联动。
useGalleryTauriSync({
  requestCompute: layoutSource.requestCompute,
  updateVisible,
  refreshCacheDir,
  // §8.1:镜头排列切换的焦点恢复链在 reflow 锚点之后(见 restoreReflowOrLensFocus 注)。
  restoreReflowAnchor: restoreReflowOrLensFocus,
  gridRef: () => gridRef.value,
  bucketActive: () => bucketActive.value,
  scrollToLogicalY: (y) => bucketScroll.scrollToLogicalY(y),
  getViewKey,
  shouldLoadViewIds: () => !lensActive.value,
  isLensActive: () => lensActive.value,
  visibleRows,
  mountedRows: () => bucketScroll.mountedRows(),
  activeRows,
})

// 侧栏点击文件夹 → 画廊滚动定位(含「计算布局中点击」排队)。
useGalleryScrollToDir({
  gridRef: () => gridRef.value,
  bucketActive: () => bucketActive.value,
  scrollToLogicalY: (y, o) => bucketScroll.scrollToLogicalY(y, o),
  logicalToPhysical,
  getViewKey,
  beginProgrammaticScroll,
  endProgrammaticScroll,
})
</script>

<style scoped src="./MediaGrid.styles.css"></style>
