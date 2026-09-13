<template>
  <!-- 画廊「视图控件」(行高/布局/分组排序/导出视图): 顶栏重构 Phase G。G8: 拆成 4 个独立折叠项
       (行高 0 / 布局 1 / 分组排序 2 / 导出当前视图 3, U-2 方案 A ②补入), 各自作一个 data-toolbar-item,
       由统一 useToolbarOverflow 按优先序依次折(globalIndex = baseIndex + offset, baseIndex=chip 数)。
       复用 G7 的 .fold-item(CSS Grid 收缩)。内联横向收缩; 菜单(视图 ⋯)竖排靠 v-show 显折叠项。
       状态全在 store, 两实例天然同步。 -->
  <!-- 行高 (offset 0) -->
  <div
    v-show="inDom(0)"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(0) }"
  >
    <div class="fold-item__inner">
      <span v-if="variant === 'menu'" class="fold-menu-label">{{ $t('toolbar.rowHeight') }}</span>
      <div class="toolbar-row-height">
        <span class="toolbar-icon" :title="$t('toolbar.rowHeight')">
          <Rows3 :size="16" />
        </span>
        <input
          type="range"
          class="row-height-slider"
          :min="ROW_HEIGHT_MIN"
          :max="ROW_HEIGHT_MAX"
          :step="ROW_HEIGHT_STEP"
          :value="sliderRowHeight"
          @input="onRowHeightInput"
        />
        <span class="row-height-value">{{ sliderRowHeight }}px</span>
      </div>
    </div>
  </div>
  <!-- 布局切换 (offset 1，T20)：宫格 ⇄ 等高行 -->
  <div
    v-show="inDom(1)"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(1) }"
  >
    <div class="fold-item__inner">
      <span v-if="variant === 'menu'" class="fold-menu-label">{{ $t('toolbar.layoutLabel') }}</span>
      <UiIconButton
        :label="
          ui.layoutMode === 'grid'
            ? $t('toolbar.layoutGridTitle')
            : $t('toolbar.layoutJustifiedTitle')
        "
        @click="toggleLayoutMode"
      >
        <LayoutGrid v-if="ui.layoutMode === 'grid'" :size="16" />
        <GalleryVertical v-else :size="16" />
      </UiIconButton>
    </div>
  </div>
  <!-- 分组 + 组内排序 + 升降序 (offset 2)。镜头激活时本项被镜头排列控件替换(2026-09-02 方案
       §4.1「普通分组、组内排序和无缝分组控件被镜头排列控件替换」);fold-item 外壳数量保持 4 个,
       useToolbarOverflow 的位次切分不受镜头态影响。 -->
  <div
    v-show="inDom(2)"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(2) }"
  >
    <div class="fold-item__inner">
      <span v-if="variant === 'menu'" class="fold-menu-label">{{ $t('toolbar.groupLabel') }}</span>
      <!-- 镜头排列分段控件(§4.2):有文本的两段式,两段均可点(P3 起 folders 放行),
           切换经 setMode 的 URL replace,不制造历史节点。 -->
      <div
        v-if="lensActive"
        class="lens-mode-seg"
      >
        <button
          class="lens-mode-seg__btn"
          :class="{ active: lens.mode === 'groups' }"

          @click="lens.setMode('groups')"
        >
          {{ $t('toolbar.lensModeGroups') }}
        </button>
        <button
          class="lens-mode-seg__btn"
          :class="{ active: lens.mode === 'folders' }"

          @click="lens.setMode('folders')"
        >
          {{ $t('toolbar.lensModeFolders') }}
        </button>
      </div>
      <!-- 「显示独有项」开关(§4.2):有文本,仅 folders 模式渲染(groups 无此概念,§4.4
           白名单不编码);绑定 duplicateLensStore.showUniqueItems/setShowUniqueItems(URL replace)。 -->
      <label v-if="lens.mode === 'folders'" class="lens-unique-toggle">
        <input
          type="checkbox"
          class="lens-unique-toggle__box"
          :checked="lens.showUniqueItems"
          @change="onShowUniqueChange"
        />
        <span class="lens-unique-toggle__text">{{ $t('duplicatesLens.showUnique') }}</span>
      </label>
      <div v-else class="toolbar-group">
        <select class="toolbar__select" :value="ui.groupBy" @change="onGroupByChange">
          <option value="date">{{ $t('toolbar.groupByDate') }}</option>
          <option value="folder">{{ $t('toolbar.groupByFolder') }}</option>
          <option value="none">{{ $t('toolbar.noGroup') }}</option>
        </select>

        <!-- 组内排序（仅在文件夹分组或 AI 搜索时可见） -->
        <select
          v-if="ui.groupBy === 'folder' || ai.isSemanticMode"
          class="toolbar__select"
          :value="ui.sortWithinGroup"
          @change="onSortWithinGroupChange"
        >
          <option value="datetime">{{ $t('toolbar.sortByTime') }}</option>
          <option value="filename">{{ $t('toolbar.sortByName') }}</option>
          <option v-if="ai.isSemanticMode" value="similarity">
            {{ $t('toolbar.sortBySimilarity') }}
          </option>
        </select>

        <!-- 无缝分组(#1):排序仍按组聚合,视觉连续无分隔符。仅真实分组下有意义。 -->
        <UiIconButton
          v-if="ui.groupBy !== 'none'"
          :label="$t('toolbar.seamlessGroups')"
          :title="$t('toolbar.seamlessGroupsTitle')"
          :active="ui.seamlessGroups"
          toggle
          @click="ui.setSeamlessGroups(!ui.seamlessGroups)"
        >
          <Ungroup :size="16" />
        </UiIconButton>

        <UiIconButton
          :label="
            ui.sortOrder === 'desc' ? $t('toolbar.sortDescTitle') : $t('toolbar.sortAscTitle')
          "
          @click="toggleSortOrder"
        >
          <ArrowDown v-if="ui.sortOrder === 'desc'" :size="16" />
          <ArrowUp v-else :size="16" />
        </UiIconButton>
      </div>
    </div>
  </div>
  <!-- 导出当前视图(offset 3,U-2 方案 A ②):按当前筛选/排序/搜索状态导出全部命中项(非当前选区),
       与选区工具条「导出选区」并列的第二导出入口,复用同一全局 ExportDialog(exportStore 唤出)。
       镜头态不渲染(§8.1 browse-only:批量命令阻断):「当前视图」导出走 buildCurrentViewDescriptor
       的普通画廊语义,与镜头画面(重复组集合)不是同一集合,静默导出会漂移;镜头感知的导出
       (需 descriptor 携带 duplicateLens)属未来切片。置于折叠序列末位,整项卸载不影响其余 offset。 -->
  <div
    v-if="!lensActive"
    v-show="inDom(3)"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(3) }"
  >
    <div class="fold-item__inner">
      <span v-if="variant === 'menu'" class="fold-menu-label">{{ $t('toolbar.exportView') }}</span>
      <UiIconButton :label="$t('toolbar.exportView')" @click="exportCurrentView">
        <Download :size="16" />
      </UiIconButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Rows3,
  ArrowDown,
  ArrowUp,
  LayoutGrid,
  GalleryVertical,
  Ungroup,
  Download,
} from '@lucide/vue'
import UiIconButton from '../ui/UiIconButton.vue'
import { useUiStore } from '../../stores/uiStore'
import { useAiStore } from '../../stores/aiStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useExportStore } from '../../stores/exportStore'
import { useExportEntries } from '../../composables/useExportEntries'

// G8: 4 个折叠项(offset 0 行高 / 1 布局 / 2 分组排序 / 3 导出当前视图), globalIndex = baseIndex + offset。
// baseIndex = chip 数(视图控件排在 chips 之后); visibleCount/measuring 来自统一 useToolbarOverflow。
const props = defineProps<{
  variant: 'inline' | 'menu'
  visibleCount: number
  measuring: boolean
  baseIndex: number
}>()

const ui = useUiStore()
const ai = useAiStore()
const exportStore = useExportStore()
const exportEntries = useExportEntries()
const { t } = useI18n()

// 重复镜头(2026-09-02 方案 §4.1/§4.2):激活时普通分组/组内排序/无缝/升降序控件被镜头排列
// 分段控件替换;mode 由 useGalleryQuerySync 从 URL 收敛(URL 是事实源),本组件只发动作。
const lens = useDuplicateLensStore()
const lensActive = computed(() => lens.isLensActive)

// data-toolbar-item 仅内联实例挂(测量对象); 菜单实例不参与测量。
const itemAttr = computed(() => (props.variant === 'inline' ? '' : null))
/** 折叠项是否留在 DOM: 内联恒在(靠 fold-item--folded 收缩); 菜单只渲染溢出项。 */
function inDom(offset: number): boolean {
  return props.variant === 'inline' ? true : props.baseIndex + offset >= props.visibleCount
}
/** 内联折叠态: 非测量帧且 globalIndex >= visibleCount。菜单为竖排不横向收缩, 恒 false。 */
function folded(offset: number): boolean {
  return (
    props.variant === 'inline' && !props.measuring && props.baseIndex + offset >= props.visibleCount
  )
}

// 布局模式切换（T20）：宫格 ⇄ 等高行。setLayoutMode 持久化；relayout watch 监听 layoutMode 触发重算。
function toggleLayoutMode() {
  ui.setLayoutMode(ui.layoutMode === 'grid' ? 'justified' : 'grid')
}

function toggleSortOrder() {
  ui.sortOrder = ui.sortOrder === 'desc' ? 'asc' : 'desc'
}

// 导出当前视图(U-2 方案 A ②):descriptor 组装收在 useExportEntries,本组件只管触发唤出既有对话框
// (同 MediaGrid.vue startExportSelection 的调用姿态)。
// ViewStale 重试回调(P3):重跑 buildCurrentViewExportPayload 取 selection,而非借用当前选区
// (严禁回退借用选区——此前 ExportDialog 的重试无条件用选区重建,会把视图导出静默换成选区)。
function exportCurrentView() {
  const { selection, source } = exportEntries.buildCurrentViewExportPayload(
    t('export.currentViewName'),
  )
  exportStore.openExportDialog(
    selection,
    source,
    () => exportEntries.buildCurrentViewExportPayload(t('export.currentViewName')).selection,
  )
}

function onGroupByChange(e: Event) {
  const value = (e.target as HTMLSelectElement).value as 'date' | 'folder' | 'none'
  ui.setGroupBy(value)
}

// 「显示独有项」开关(§4.2):checkbox change → store 动作(URL replace)。store 内已含
// mode!=='folders' 守卫,这里只透传勾选态。
function onShowUniqueChange(e: Event) {
  lens.setShowUniqueItems((e.target as HTMLInputElement).checked)
}

function onSortWithinGroupChange(e: Event) {
  const value = (e.target as HTMLSelectElement).value as 'datetime' | 'filename' | 'similarity'
  ui.setSortWithinGroup(value)
}

// 行高滑杆区间与步进:64–1024px,每 16px 一跳(与缩略图档位 THUMB_SIZE_TIERS 同量程,
// getOptimalThumbTier 据行高取首个 ≥ 的档位)。
const ROW_HEIGHT_MIN = 64
const ROW_HEIGHT_MAX = 1024
const ROW_HEIGHT_STEP = 16
// 5 个「档位」节点(对齐 THUMB_SIZE_TIERS):拖到附近自动吸附,便于精确落在常用尺寸。
const ROW_HEIGHT_SNAP_NODES = [64, 128, 256, 512, 1024] as const
// 距节点 ≤ 此像素即吸附。取 24(>step,略宽于一跳)使拖动接近时有明显磁性,又留出节点间自由区
// (最窄间隔 64→128 两侧各 24,中段 [88,104] 仍可自由停)。
const ROW_HEIGHT_SNAP_PX = 24

/** 将原始滑杆值吸附到最近的档位节点(在阈值内);否则原样返回。 */
function snapRowHeight(value: number): number {
  let snapped = value
  let best = ROW_HEIGHT_SNAP_PX + 1
  for (const node of ROW_HEIGHT_SNAP_NODES) {
    const d = Math.abs(value - node)
    if (d <= ROW_HEIGHT_SNAP_PX && d < best) {
      best = d
      snapped = node
    }
  }
  return snapped
}

// S4（Part2 重排提速）：拖动中只更新本地显示值，**不写 ui.gridRowHeight** ——
// useJustifiedLayout 的 relayout watch 吃的是 store 值，原实现每步进即触发一次
// 全量重排（百万库下串行排队数次 × 秒级）；原 300ms 防抖只防了持久化、没防重排。
// 停手 250ms 后一次性提交 store + 持久化（setGridRowHeight），重排恰好一次。
const sliderRowHeight = ref(ui.gridRowHeight)
// 外部变化（启动读配置等）→ 回同步本地显示值。
watch(
  () => ui.gridRowHeight,
  (v) => {
    sliderRowHeight.value = v
  },
)
let rowHeightTimer: ReturnType<typeof setTimeout> | null = null

function onRowHeightInput(e: Event) {
  const raw = parseInt((e.target as HTMLInputElement).value, 10)
  const value = snapRowHeight(raw) // 靠近档位节点则吸附
  sliderRowHeight.value = value // 即时更新滑块与数值显示（纯本地，不触发重排）
  if (rowHeightTimer) clearTimeout(rowHeightTimer)
  rowHeightTimer = setTimeout(() => {
    ui.setGridRowHeight(value) // 停手后一次：写 store（触发唯一一次重排）+ 持久化
  }, 250)
}

// 双渲染下本组件随 compact 切换 v-if 卸载/挂载。拖拽中(250ms 防抖未落 store)若窗口跨过
// 折叠阈值致本实例卸载, 悬挂的 timer 会在卸载后迟发 setGridRowHeight(触发一次多余全量重排)
// + 闭包滞留 store 引用。卸载即清, 未提交的拖拽值随之丢弃(下次挂载按 store 现值重建, 语义正确)。
onBeforeUnmount(() => {
  if (rowHeightTimer) clearTimeout(rowHeightTimer)
})
</script>

<style scoped>
.toolbar__select {
  appearance: none;
  height: var(--control-size-compact);
  background: var(--color-input-bg);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  padding: 0 var(--spacing-sm);
  cursor: pointer;
  outline: none;
  transition: border-color var(--transition-fast);
}
.toolbar__select:hover {
  border-color: var(--color-input-border-focus);
}

.toolbar-row-height {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}
.toolbar-icon {
  display: flex;
  color: var(--color-text-secondary);
}
.row-height-slider {
  width: 80px;
  height: 4px;
  -webkit-appearance: none;
  appearance: none;
  background: var(--color-border);
  border-radius: 2px;
  cursor: pointer;
}
.row-height-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--color-accent);
  cursor: pointer;
  transition: transform var(--transition-fast);
}
.row-height-slider::-webkit-slider-thumb:hover {
  background: var(--color-accent-hover);
}
.row-height-value {
  font-size: var(--font-size-2xs);
  color: var(--color-text-tertiary);
  min-width: 40px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

/* 镜头排列分段控件(§4.2):有文本的两段式;active 段用 accent 底,禁用段降透明度+禁指针。 */
.lens-mode-seg {
  display: inline-flex;
  align-items: center;
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}
.lens-mode-seg__btn {
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  white-space: nowrap;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}
.lens-mode-seg__btn + .lens-mode-seg__btn {
  border-left: 1px solid var(--color-input-border);
}
.lens-mode-seg__btn.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
.lens-mode-seg__btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

/* 「显示独有项」开关(§4.2):有文本的 checkbox,分段控件旁同密度排布;控件密度对齐
   .lens-mode-seg__btn(同高度/字号),勾选框用原生 input + accent-color 零自绘成本。 */
.lens-unique-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  margin-inline-start: var(--spacing-sm);
  cursor: pointer;
  white-space: nowrap;
}
.lens-unique-toggle__box {
  width: 13px;
  height: 13px;
  margin: 0;
  accent-color: var(--color-accent);
  cursor: pointer;
}
.lens-unique-toggle__text {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  user-select: none;
}

/* G9 菜单行标签: 仅 variant=menu 渲染(内联侧 v-if 不出→零测量影响)。左侧固定宽标签使「视图 ⋯」
   弹层成「标签 : 控件」自描述菜单行; 横排/右对齐(space-between)由父 .view-popover :deep 覆盖。 */
.fold-menu-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  flex-shrink: 0;
  min-width: 3em;
}

/* G8 折叠项(与 GalleryFilterChips 同款 CSS Grid 收缩): 每控件包 .fold-item(grid 1fr→0fr 收缩
   真实宽度全程)+ .fold-item__inner(overflow:hidden 裁内容 + translateX 横向收入)。间距 margin
   -inline-end(容器 gap:0), 折叠归 0 无碎屑; 引擎测量含 margin。菜单变体 folded 恒 false → grid 1fr
   全宽行, 靠 v-show 显折叠项, 竖排由父 .view-popover(flex column)承担。 */
.fold-item {
  display: grid;
  grid-template-columns: 1fr;
  flex-shrink: 0;
  margin-inline-end: var(--spacing-xs);
  transition:
    grid-template-columns 0.28s ease,
    opacity 0.28s ease,
    margin 0.28s ease;
}
.fold-item--folded {
  grid-template-columns: 0fr;
  opacity: 0;
  margin-inline-end: 0;
  pointer-events: none;
}
.fold-item__inner {
  overflow: hidden;
  min-width: 0;
  display: flex;
  align-items: center;
  transition: transform var(--duration-moderate) var(--ease-out);
}
.fold-item__inner > * {
  flex-shrink: 0;
}
.fold-item--folded .fold-item__inner {
  transform: translateX(10px);
}
</style>
