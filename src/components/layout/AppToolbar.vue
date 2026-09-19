<template>
  <!-- 顶栏重构 Phase G: 结构容器。移窗拖拽(2026-07-14 改)已由 WindowChrome 根上的 useWindowDrag
       委托实现(整行含按钮按住越阈值即拖、轻点即用),故各容器不再挂 data-tauri-drag-region;
       行高滑块/搜索框/下拉等原生表单控件由 useWindowDrag 的排除选择器自动排除,保各自手势。 -->
  <!-- data-window-drag-surface:各 flex 容器的间隙是「纯空隙拖拽面」——命中容器本体即 pointerdown 即时
       移窗(useWindowDrag 路径 B / VSCode 级);容器内的按钮/可点 div 靠 matches 判定落进阈值路径,不受影响。 -->
  <div class="toolbar__left" data-window-drag-surface>
    <!-- 画廊侧栏显隐开关(2026-07-14):画廊态收起/展开左侧栏。状态在 uiStore(gallerySidebarVisible),
         与查看器侧栏各自独立、互不干扰。轻点触发切换,按住拖动则移窗(useWindowDrag 阈值判定)。 -->
    <UiIconButton
      class="toolbar__sidebar-toggle"
      :label="ui.gallerySidebarVisible ? $t('sidebar.hideSidebar') : $t('sidebar.showSidebar')"
      @click="ui.toggleGallerySidebar()"
    >
      <PanelLeftClose v-if="ui.gallerySidebarVisible" :size="17" />
      <PanelLeftOpen v-else :size="17" />
    </UiIconButton>
    <!-- Title / breadcrumb -->
    <!-- 标题 / 面包屑 -->
    <div class="toolbar__breadcrumb" data-window-drag-surface>
      <span class="toolbar__title">{{ title }}</span>
      <!-- 计数区(2026-07-14 源头解耦):视图计数(viewTotalItems)恒 ≤ 全库计数(totalItems),故用**全库计数**
           作隐藏 sizer 撑出最大宽度、实际值与之同格(inline-grid)叠放 → 计数变窄不改本区宽度 → 不撑动右侧
           flex:1 的 foldable → 不触发折叠 recompute(根除「筛选致计数 543,449→2 变窄 → 顶栏闪展再收」的耦合源)。 -->
      <span v-if="media.stats" class="toolbar__count">
        <span class="toolbar__count-sizer">{{
          $t('statusbar.items', { count: media.totalItems.toLocaleString() })
        }}</span>
        <span class="toolbar__count-actual">{{
          $t('statusbar.items', { count: media.viewTotalItems.toLocaleString() })
        }}</span>
      </span>
    </div>
  </div>

  <!-- 统一折叠容器(Phase G G5): chips + 视图控件在同一个 flex:1 容器内, 一套 useToolbarOverflow
       按优先序逐项折——视图控件整块作一个 data-toolbar-item 放在末尾(优先级最低→最先折), chips
       从尾折。单容器 → 折叠某项不改容器自身宽度(始终填满), 可用宽只随窗口单调变化, 根除 G4 真机
       暴露的振荡(视图控件折叠释放 flex 空间致已折 chips 弹回)。 -->
  <div
    ref="foldableRef"
    class="toolbar__foldable"
    :class="[
      `toolbar__foldable--${toolbarAlign}`,
      { 'is-measuring': measuring, 'is-settling': settling },
    ]"
    data-window-drag-surface
  >
    <GalleryFilterChips variant="inline" :visible-count="visibleCount" :measuring="measuring" />
    <!-- 视图控件(G8): 拆成 3 个独立折叠项(行高/布局/分组排序), 各自 data-toolbar-item 在组件内部
         (非透传), 由统一 useToolbarOverflow 依次折。baseIndex=chipCount(排在 chips 之后→最先折)。
         组件是多根 fold-item 片段, 只传 props 不透传 class/attr(避免 __vnode 踩空)。 -->
    <GalleryViewControls
      variant="inline"
      :visible-count="visibleCount"
      :measuring="measuring"
      :base-index="chipCount"
    />
    <!-- 「筛选 ⋯」: 折叠的低频筛选 chip。用 v-show(非 v-if)保持 DOM 稳定——避免在含 Teleport 的
         GalleryFilterChips 片段旁做结构性插入/移除, 消除 Vue __vnode 踩空隐患。 -->
    <UiIconButton
      v-show="filterChipsFolded"
      ref="filterBtnRef"
      :label="$t('toolbar.filterOptions')"
      :active="filterMenuOpen"
      @click="toggleFilterMenu"
    >
      <ListFilter :size="16" />
    </UiIconButton>
    <!-- 「视图 ⋯」: 折叠的视图控件(同上 v-show 保持 DOM 稳定) -->
    <UiIconButton
      v-show="viewControlsFolded"
      ref="viewBtnRef"
      :label="$t('toolbar.viewOptions')"
      :active="viewMenuOpen"
      @click="toggleViewMenu"
    >
      <SlidersHorizontal :size="18" />
    </UiIconButton>
  </div>

  <!-- Right controls -->
  <!-- 右侧控件 -->
  <div class="toolbar__right" data-window-drag-surface>
    <!-- H-Lab 横向画廊实验室入口(多布局候选真人调研;docs/designs/2026-07-02-horizontal-gallery-lab.md) -->
    <UiIconButton
      v-if="showHGalleryLab"
      :label="$t('toolbar.hgalleryLab')"
      @click="router.push('/hgallery-lab')"
    >
      <FlaskConical :size="18" />
    </UiIconButton>
    <!-- Canvas 支持时可开启打码；已开启时保留关闭入口，避免切换引擎或卸载画廊后
         文件树仍在打码却无法关闭。DOM 画廊本身没有打码实现。 -->
    <UiIconButton
      v-if="demoPrivacySupported || demoPrivacyEnabled"
      :label="demoPrivacyEnabled ? $t('demoPrivacy.disable') : $t('demoPrivacy.enable')"
      :active="demoPrivacyEnabled"
      @click="toggleDemoPrivacy"
    >
      <EyeOff v-if="demoPrivacyEnabled" :size="18" />
      <ScanEye v-else :size="18" />
    </UiIconButton>
    <!-- 撤销/重做/全屏已迁标题栏 ContextualToolbar(顶栏重构 P3);键盘 Ctrl+Z/Y(本组件 keydown)
         与 F11(AppShell)保持不变。 -->

    <!-- Search -->
    <!-- 搜索 -->
    <div
      v-if="!lens.isLensActive"
      class="toolbar__search-wrap"
      :class="{ focused: isSearchFocused, 'semantic-mode': ai.isSemanticMode }"
      style="position: relative"
    >
      <!-- Mode toggle button -->
      <!-- 模式切换按钮 -->
      <button
        class="toolbar__search-mode-btn"
        :class="['mode-' + ai.searchMode]"
        :title="
          ai.searchMode === 'mixed'
            ? $t('toolbar.searchModeMixed')
            : ai.searchMode === 'semantic'
              ? $t('toolbar.searchModeSemantic')
              : $t('toolbar.searchModeNormal')
        "

        @click="toggleSearchMode"
      >
        <Sparkles v-if="ai.searchMode === 'mixed'" :size="14" />
        <Bot v-else-if="ai.searchMode === 'semantic'" :size="14" />
        <Search v-else :size="14" />
      </button>

      <!-- 搜索框：混合模式下输入驱动显示下方建议，方向键/Enter 选择仍由既有键盘路径处理。 -->
      <input
        ref="searchInputRef"
        class="toolbar__search"
        v-model="currentSearchQuery"
        type="search"
        :placeholder="searchPlaceholder"
        @keydown.esc.prevent="onEscape"
        @keydown.down="onKeydownDown"
        @keydown.up="onKeydownUp"
        @keydown.enter="onKeydownEnter"
        @focus="onSearchFocus"
        @blur="onSearchBlur"
      />

      <!-- Scope selector (only for normal search mode) -->
      <!-- 搜索范围选择器（仅限普通搜索模式） -->
      <select
        v-show="ai.searchMode === 'normal'"
        class="toolbar__search-scope"
        v-model="ui.searchScope"
      >
        <option value="filename">{{ $t('toolbar.searchScopeFilename') }}</option>
        <option value="folder">{{ $t('toolbar.searchScopeFolder') }}</option>
        <option value="date">{{ $t('toolbar.searchScopeDate') }}</option>
        <option value="device">{{ $t('toolbar.searchScopeDevice') }}</option>
        <option value="location">{{ $t('toolbar.searchScopeLocation') }}</option>
        <option value="global">{{ $t('toolbar.searchScopeGlobal') }}</option>
      </select>

      <!-- AI searching indicator -->
      <!-- AI 搜索中指示器 -->
      <span v-if="ai.isSearching" class="toolbar__search-spinner" />

      <!-- Mixed mode dropdown -->
      <Transition name="dropdown-fade">
        <!-- data-no-window-drag:下拉项是 <div @click> 而非 button,须显式排除出移窗拖拽(逃生舱)——
             否则「拖拽式点选项」会被判为移窗、吞掉选项点击(useWindowDrag 路径 A)。 -->
        <div
          v-if="isMixedDropdownOpen"
          class="mixed-search-dropdown"
          data-no-window-drag
          @mousedown.prevent
        >
          <div
            class="dropdown-item"
            :class="{ selected: mixedDropdownIndex === 0 }"
            @click="executeMixedSearch(0)"
          >
            <div class="dropdown-icon-wrap"><Sparkles :size="14" /></div>
            <div class="dropdown-text">
              <!-- 中英语序不同，前后缀直译无法成句：用 <i18n-t> 让高亮 query 落进各语言的正确语序位置 -->
              <i18n-t keypath="toolbar.mixedSearchAiHint" scope="global">
                <template #query>
                  <span class="query">{{ search.draftMixedQuery }}</span>
                </template>
              </i18n-t>
            </div>
          </div>
          <div
            class="dropdown-item"
            :class="{ selected: mixedDropdownIndex === 1 }"
            @click="executeMixedSearch(1)"
          >
            <div class="dropdown-icon-wrap"><Search :size="14" /></div>
            <div class="dropdown-text">
              <i18n-t keypath="toolbar.mixedSearchFilenameHint" scope="global">
                <template #query>
                  <span class="query">{{ search.draftMixedQuery }}</span>
                </template>
              </i18n-t>
            </div>
          </div>
        </div>
      </Transition>
    </div>

    <!-- 视图控件已移入统一折叠容器(Phase G G5);「视图 ⋯」入口随之移入该容器。 -->
  </div>

  <!-- 「视图 ⋯」弹层(Phase G G5): 迁 UiPopover 原语(Teleport + @floating-ui 定位[bottom 居中对齐 ⋯ 按钮]
       + backdrop/Esc dismiss + 焦点陷阱);menu 实例经 store 与内联同步,open 关时随 UiPopover v-if 卸载(承 code-review F5)。 -->
  <UiPopover v-model:open="viewMenuOpen" :anchor="viewAnchor" placement="bottom">
    <div class="view-popover">
      <GalleryViewControls
        variant="menu"
        :visible-count="visibleCount"
        :measuring="false"
        :base-index="chipCount"
      />
    </div>
  </UiPopover>

  <!-- 「筛选 ⋯」弹层(Phase G G4): 迁 UiPopover 原语(同视图弹层, bottom 居中对齐 ⋯ 按钮);溢出 chips 的
       menu 实例与内联经 filterStore 同步,open 关时随 UiPopover v-if 卸载(承 code-review F5, 不常驻隐藏实例)。 -->
  <UiPopover v-model:open="filterMenuOpen" :anchor="filterAnchor" placement="bottom">
    <div class="filter-popover">
      <GalleryFilterChips variant="menu" :visible-count="visibleCount" :measuring="false" />
    </div>
  </UiPopover>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import {
  Sparkles,
  Search,
  Bot,
  FlaskConical,
  SlidersHorizontal,
  ListFilter,
  PanelLeftClose,
  PanelLeftOpen,
  ScanEye,
  EyeOff,
} from '@lucide/vue'
import UiIconButton from '../ui/UiIconButton.vue'
import UiPopover from '../ui/UiPopover.vue'
import { useToolbarOverflow } from '../../composables/useToolbarOverflow'
import { useDemoPrivacy } from '../../composables/useDemoPrivacy'
import { useToolbarAlign } from '../../composables/useToolbarAlign'
import GalleryViewControls from './GalleryViewControls.vue'
import { chipCountOf, chipStateOf, chipWidthKey } from './filterChips.descriptors'
import GalleryFilterChips from './GalleryFilterChips.vue'
import { useUiStore } from '../../stores/uiStore'
import { useViewStore } from '../../stores/viewStore'
import { useFilterStore } from '../../stores/filterStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useAiStore } from '../../stores/aiStore'
import { useSearchStore } from '../../stores/searchStore'
import { dispatchKeybinding } from '../../commands/keybinding'
import { buildCommandContext } from '../../commands/context'

const { t, locale } = useI18n()
const router = useRouter()
const ui = useUiStore()
const viewStore = useViewStore()
const filter = useFilterStore()
// 重复镜头(2026-09-02 方案 §4.1):激活时普通筛选 chip 整体暂停、摘要 chip 出现 —— chip 存在性
// 与视图控件内容都随镜头态换代,进 remeasureKey 防折叠切分停在旧宽度上。
const lens = useDuplicateLensStore()
const media = useMediaStore()
const ai = useAiStore()
// 搜索单源门面（S2-a）：草稿 / 已提交查询 / 提交路由集中于此，AppToolbar 只保留输入 UI 与防抖。
const search = useSearchStore()
// 演示打码开关(2026-09-16):状态与「当前引擎是否支持」都在共享单例里,本组件只读+切换。
const { demoPrivacyEnabled, demoPrivacySupported, toggleDemoPrivacy } = useDemoPrivacy()
// 顶栏中部 chips 簇水平对齐(居中默认/靠左/靠右)——仅挪中部折叠区,标题恒左、搜索恒右不动。
// 收窄窗口时 justify 留白先被吃掉,再由 useToolbarOverflow 折叠(「先减留白再折叠」天然成立)。
const { align: toolbarAlign } = useToolbarAlign()
const showHGalleryLab =
  import.meta.env.DEV && localStorage.getItem('scrollery.debug.hgalleryLab') === '1'

// ── 统一 Priority+ 逐项折叠(Phase G G5: chips + 视图控件一套测量)──────────────
// foldableRef(flex:1)内含: 8 个 chip(data-toolbar-item, 索引 0..chipCount-1) + 视图控件整块
// (1 个 data-toolbar-item, 索引 chipCount, 优先级最低→DOM 末→最先折)。useToolbarOverflow 返回
// visibleCount(前 N 项可见)。单容器 → 折叠某项不改容器 flex 宽度, 可用宽随窗口单调, 无振荡。
// remeasureKey: 影响各项自然宽的反应源(容器宽不变时 RO 不触发, 须显式重测):
//   chip 宽 — locale 文案 / 评分"+"后缀 / 清除 chip 出现 / 颜色 / 日期标签;
//   视图控件宽 — groupBy·语义模式(决定组内排序 select 是否出现)。
// ⚠ 勿把 gridRowHeight 列入: 行高控件宽度值无关(滑块固定 80px、读数 min-width:40px+tabular-nums,
//   "60px"~"960px" 恒占 40px), 值变不改切分。列入只会在拖行高提交时空转 measure() —— 折叠态下量尺帧
//   全展开再带过渡收回, 且视图弹层若开着, ⋯ 锚点漂移会让弹层横跳(2026-07-14 真机 bug 根因)。
// (标题/搜索聚焦改的是「预留区」宽 → foldable 作为 flex:1 兄弟随之 resize → RO 自动 recompute,
//  无需显式重测。)
// chip 侧的反应源**不再手工枚举**(S 线 §5 三处描述符化之三,也是最危险的一处):漏一项 → 该 chip
// 文案变宽却不重测 → 顶栏闪展再收,而且无编译/SSR/测试信号,只在真机抖动。改由描述符自报
// (chipWidthKey 含「存在性」+ 各 chip 的 widthDeps),新增 chip 时反应源与 chip 在同一处声明。
// 留在此处的两类:locale(影响每一个 chip,不是某一个的私事)、视图控件侧(groupBy·语义模式决定
// 组内排序 select 是否出现)。
const foldableRef = ref<HTMLElement | null>(null)
// chip 侧状态唯一构造点:普通筛选维度 + 镜头激活态在 chipStateOf 合流(descriptors 内),与
// GalleryFilterChips 的 idx() 用同一份,勿在此另拼字段表。
const chipState = computed(() => chipStateOf(filter, lens.isLensActive))
const overflowRemeasureKey = computed(() =>
  [locale.value, chipWidthKey(chipState.value), ui.groupBy, ai.isSemanticMode, lens.mode].join('|'),
)
// 溢出弹层开阖态提前声明: 供 useToolbarOverflow 的 deferWhile 引用(弹层开启期暂停重测, 防 ⋯ 锚点
// 漂移致弹层横跳)。各弹层的按钮/锚点 ref 仍在下方对应处声明。
const filterMenuOpen = ref(false)
const viewMenuOpen = ref(false)
const {
  visibleCount,
  hasOverflow,
  isMeasuring: measuring,
  isSettling: settling,
} = useToolbarOverflow({
  containerRef: foldableRef,
  // 两个 ⋯ 按钮(筛选/视图)可能并存, 保守预留其总宽避免末项被 overflow:hidden 裁。
  overflowButtonWidth: 76,
  remeasureKey: overflowRemeasureKey,
  // 溢出弹层开启期间暂停重测: 弹层锚点是折叠项旁的 ⋯ 按钮, 量尺帧的收放会漂移锚点致弹层横跳; 关闭时补测。
  deferWhile: computed(() => viewMenuOpen.value || filterMenuOpen.value),
})

// chip 项数由描述符推导(清除 chip 仅在有活动筛选时存在);视图控件是其后的第 chipCount 项。
// 原为手写 `hasActiveFilters ? 8 : 7` —— 与模板里的 0..7 字面量各写一遍,插一个 chip 要两处同步改,
// 漏改则视图控件 baseIndex 偏移、折叠位次全错(S 线 §5)。镜头激活时普通 chip 暂停、计数随存在性收敛。
const chipCount = computed(() => chipCountOf(chipState.value))
// 视图控件在末尾 → 任何溢出都先折它: hasOverflow ⟺ 视图控件已折。
const viewControlsFolded = computed(() => hasOverflow.value)
// 至少一个 chip 折叠(视图控件之外还溢出) ⟺ visibleCount < chipCount。
const filterChipsFolded = computed(() => visibleCount.value < chipCount.value)

// 「筛选 ⋯」弹层(body-teleport; backdrop v-if 关时卸载 menu 实例, 承 code-review F5)。
// filterMenuOpen 已在上方(deferWhile 需前置引用)声明。
// UiIconButton 经 defineExpose 暴露根 <button>,故 ref 是组件实例;锚点取 .value?.el 传给 UiPopover。
const filterBtnRef = ref<InstanceType<typeof UiIconButton>>()
// 锚点在 toggle 打开时抓取(避免依赖经 defineExpose 暴露属性的响应式),传 UiPopover :anchor。
const filterAnchor = ref<HTMLElement | null>(null)

// 「视图 ⋯」弹层(同上模式);viewMenuOpen 亦在上方声明。
const viewBtnRef = ref<InstanceType<typeof UiIconButton>>()
const viewAnchor = ref<HTMLElement | null>(null)

function closeOverflowMenus() {
  filterMenuOpen.value = false
  viewMenuOpen.value = false
}

function toggleFilterMenu() {
  // 打开前抓取 defineExpose 暴露的根 <button> 作锚点;定位/钳制/滚动跟随交 UiPopover(@floating-ui)。
  if (!filterMenuOpen.value) {
    filterAnchor.value = filterBtnRef.value?.el ?? null
    // 两个溢出 ⋯ 菜单互斥: 开筛选先关视图, 避免二者叠层(backdrop 让出标题栏高度→⋯ 按钮始终可点,
    // 否则两弹层共存, 后 Teleport 的筛选弹层盖住视图弹层)。
    viewMenuOpen.value = false
  }
  filterMenuOpen.value = !filterMenuOpen.value
}

function toggleViewMenu() {
  if (!viewMenuOpen.value) {
    viewAnchor.value = viewBtnRef.value?.el ?? null
    // 互斥(同上): 开视图先关筛选。
    filterMenuOpen.value = false
  }
  viewMenuOpen.value = !viewMenuOpen.value
}

// ── 网格键盘快捷键:经注册表分发(P5-6 键位同源,2026-07-10 收敛网格路径)──────────
// 此前 undo/redo 在此硬编码 ctrl+z/y、tooltip 却读 grid.ts 的 keybinding 常量——正是 P5-6 要消灭
// 的双源漂移(ctrl+shift+z 可用但 tooltip 不显)。组合键分发(eventToCombo)落地后统一走注册表:
// grid.undo(mod+z)/grid.redo(mod+y,别名 mod+shift+z)/grid.toggleFullscreen(F11)。
function onGlobalKeydown(e: KeyboardEvent) {
  // Esc 关闭已开的 ⋯ 弹层(backdrop 点击关闭之外补一个键盘出口)。
  if (e.key === 'Escape' && (filterMenuOpen.value || viewMenuOpen.value)) {
    closeOverflowMenus()
    return
  }
  if (e.defaultPrevented) return // 更近的消费者已处理
  // 输入态不拦(搜索框内 ctrl+z 应是文本撤销)。
  const tgt = e.target as HTMLElement | null
  const tag = tgt?.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tgt?.isContentEditable) return
  if (dispatchKeybinding(e, buildCommandContext())) e.preventDefault()
}
onMounted(() => {
  document.addEventListener('keydown', onGlobalKeydown)
})
onBeforeUnmount(() => {
  document.removeEventListener('keydown', onGlobalKeydown)
  if (searchTimer) clearTimeout(searchTimer)
})

const isSearchFocused = ref(false)
const searchInputRef = ref<HTMLInputElement>()
let searchTimer: ReturnType<typeof setTimeout> | null = null

// 标题复用侧栏词条以保持入口一致；map 在 computed 内重建并调 t()，locale 切换时自动重算。
const title = computed(() => {
  const map: Record<string, string> = {
    all: t('sidebar.allPhotos'),
    favorites: t('sidebar.favorites'),
    'live-photos': t('sidebar.livePhotos'),
    recent: t('sidebar.recentlyAdded'),
    trash: t('sidebar.trash'),
  }
  return map[viewStore.activeSmartAlbum] ?? t('toolbar.titleDefault')
})

// 草稿态已提升至 searchStore.draftMixedQuery（混合模式单源）；此处仅保留纯 UI 下拉态。
const isMixedDropdownOpen = ref(false)
const mixedDropdownIndex = ref(0) // 0: AI, 1: Normal

// 占位符文案按模式切换；抽此 computed 一并消除 template 里原本重复的三目占位符。
const searchPlaceholder = computed(() =>
  ai.searchMode === 'mixed'
    ? t('toolbar.searchPlaceholderMixed')
    : ai.searchMode === 'semantic'
      ? t('toolbar.searchPlaceholderSemantic')
      : t('toolbar.searchPlaceholder'),
)

// 混合搜索防抖包装：AppToolbar 只负责输入节流，实际提交路由（AI vs 文件名 / 空串复位）委托门面。
function triggerMixedSearch(immediate = false) {
  if (searchTimer) clearTimeout(searchTimer)
  const query = search.draftMixedQuery
  if (!query.trim()) {
    search.commitMixed(mixedDropdownIndex.value as 0 | 1, '')
    return
  }
  searchTimer = setTimeout(
    () => {
      search.commitMixed(mixedDropdownIndex.value as 0 | 1, query)
    },
    immediate ? 0 : ui.searchDebounceMs,
  )
}

const currentSearchQuery = computed({
  get() {
    // 混合模式输入框显示未决草稿；其余模式显示各自已提交查询（committedQuery 会把 mixed 归一，
    // 与输入框需要的草稿语义不同，故这里不复用 committedQuery）。
    if (ai.searchMode === 'mixed') return search.draftMixedQuery
    return ai.searchMode === 'semantic' ? ai.semanticQuery : ui.searchQuery
  },
  set(val: string) {
    if (ai.searchMode === 'mixed') {
      search.draftMixedQuery = val
      isMixedDropdownOpen.value = val.trim().length > 0
      triggerMixedSearch(false)
    } else if (ai.searchMode === 'semantic') {
      // 语义模式：立即写 semanticQuery 让输入即时可见（相当于语义模式的草稿），查询本身防抖提交。
      ai.semanticQuery = val
      if (searchTimer) clearTimeout(searchTimer)
      searchTimer = setTimeout(() => search.commitSemantic(val), ui.searchDebounceMs)
    } else {
      // 普通/文件名：写 uiStore.searchQuery，useJustifiedLayout 的 watch 即时重算（保持原每键即查行为）。
      search.commitNormal(val)
    }
  },
})

function onSearchFocus() {
  isSearchFocused.value = true
  if (ai.searchMode === 'mixed' && search.draftMixedQuery.trim().length > 0) {
    isMixedDropdownOpen.value = true
  }
}

function onSearchBlur() {
  isSearchFocused.value = false
  isMixedDropdownOpen.value = false
}

function onEscape() {
  if (isMixedDropdownOpen.value) {
    isMixedDropdownOpen.value = false
  } else {
    // maybe clear search?
  }
}

function onKeydownDown(e: KeyboardEvent) {
  if (ai.searchMode === 'mixed' && isMixedDropdownOpen.value) {
    e.preventDefault()
    mixedDropdownIndex.value = (mixedDropdownIndex.value + 1) % 2
    triggerMixedSearch(true)
  }
}

function onKeydownUp(e: KeyboardEvent) {
  if (ai.searchMode === 'mixed' && isMixedDropdownOpen.value) {
    e.preventDefault()
    mixedDropdownIndex.value = (mixedDropdownIndex.value + 1) % 2
    triggerMixedSearch(true)
  }
}

function onKeydownEnter(e: KeyboardEvent) {
  if (ai.searchMode === 'mixed' && isMixedDropdownOpen.value) {
    e.preventDefault()
    isMixedDropdownOpen.value = false
    triggerMixedSearch(true)
  }
}

function executeMixedSearch(index: number) {
  mixedDropdownIndex.value = index
  isMixedDropdownOpen.value = false
  triggerMixedSearch(true)
}

function toggleSearchMode() {
  ai.toggleSearchMode()
  if (ai.searchMode === 'mixed') {
    search.draftMixedQuery = ''
    isMixedDropdownOpen.value = false
  }
}

// 视图控件(行高/布局/分组/排序)及其 handler 已抽入 GalleryViewControls.vue(Phase G G3);
// 其折叠现由上方统一 useToolbarOverflow 驱动(viewControlsFolded), 不再有独立的 decideCompact/
// checkOverflow/ResizeObserver——单容器测量已替代旧「观内容容器溢出」的双系统(G5)。
</script>

<style scoped>
.toolbar__left {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 0;
  flex-shrink: 0;
}
/* 画廊侧栏开关:恒占固定尺寸、贴左端不被标题挤压 */
.toolbar__sidebar-toggle {
  flex-shrink: 0;
}
.toolbar__breadcrumb {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  overflow: hidden;
}
.toolbar__title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
}
/* 计数区源头解耦:sizer(全库计数,visibility:hidden 只撑宽)与 actual(视图计数)inline-grid 同格叠放,
   格宽恒取二者较大者=sizer → 视图计数变窄不改本区宽,右侧 foldable 不被 flex 撑动 → 不触发折叠 recompute。
   tabular-nums 使数字等宽,消除同位数不同数字的微小宽差残留耦合。actual 默认左对齐,变窄时右侧留白。 */
.toolbar__count {
  display: inline-grid;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.toolbar__count-sizer,
.toolbar__count-actual {
  grid-area: 1 / 1; /* 叠放同一格 */
}
.toolbar__count-sizer {
  visibility: hidden; /* 只撑最大宽、不可见 */
}

/* 统一折叠容器(Phase G G5): chips + 视图控件同处一个 flex:1 容器, 一套 useToolbarOverflow 测量。
   overflow:hidden 兜底测量帧(全渲染)时的一帧视觉溢出(offsetWidth 不受裁切影响, 测量仍准)。
   min-width 保底不被压没。
   G6: gap:0——间距改由各折叠项 margin-inline-end 承担, 折叠时 margin 随 max-width 一并收拢无碎屑。 */
.toolbar__foldable {
  display: flex;
  align-items: center;
  gap: 0;
  flex: 1;
  min-width: 140px;
  overflow: hidden;
  padding: 0 var(--spacing-sm);
}
/* chips 簇水平对齐(新需求 2):中部折叠区是 flex:1 全宽,justify-content 决定簇在其内的落位,
   多余空间即留白(居中→两侧、靠左→右侧、靠右→左侧)。窗口收窄先吃留白、再折叠,无需改测量逻辑。 */
.toolbar__foldable--center {
  justify-content: center;
}
.toolbar__foldable--left {
  justify-content: flex-start;
}
.toolbar__foldable--right {
  justify-content: flex-end;
}
/* ⋯ 按钮(筛选/视图)间距与不压缩 */
.toolbar__foldable > .btn-icon {
  flex-shrink: 0;
  margin-inline-end: var(--spacing-xs);
}
/* 量尺态(is-measuring): useToolbarOverflow.measure() 命令式短暂加此类(同步段内加→读宽→移,见其注释),
   据此**强制所有 fold-item 展开到自然宽**(含已折叠项 → 覆盖 --folded 的 0fr/opacity:0/margin:0/内层
   translateX)以量真实宽, 同时禁过渡使加/移类不触发动画。因量尺全程同步、无 await, 浏览器不绘制此展开态,
   故不闪。特异度: `.toolbar__foldable.is-measuring :deep(.fold-item)` 高于子组件 scoped 的 `.fold-item--folded`,
   无需 !important。.fold-item 在子组件 GalleryFilterChips/GalleryViewControls 内(:deep 穿透)。 */
.toolbar__foldable.is-measuring :deep(.fold-item) {
  grid-template-columns: 1fr;
  opacity: 1;
  margin-inline-end: var(--spacing-xs);
}
.toolbar__foldable.is-measuring :deep(.fold-item__inner) {
  transform: none;
}
/* 折叠过渡抑制:量尺帧(is-measuring)与内容驱动折叠落定帧(is-settling)都禁过渡。前者使加/移量尺类不触发
   动画;后者使内容变化(筛选/分组 chip 增减、组内排序 select 显隐)引发的折叠**瞬时**完成,不走 0.28s 过渡
   ——那条过渡只留给窗口 resize 的平滑折叠(走 recompute 路径,不加此二类)。is-settling 只禁过渡、**不**强制
   展开(故折叠真的会发生),与 is-measuring 的强制展开区分开。 */
.toolbar__foldable.is-measuring :deep(.fold-item),
.toolbar__foldable.is-measuring :deep(.fold-item__inner),
.toolbar__foldable.is-settling :deep(.fold-item),
.toolbar__foldable.is-settling :deep(.fold-item__inner) {
  transition: none;
}

/* chip 修饰符样式 + 日期弹层已随 chips 抽入 GalleryFilterChips.vue(Phase G G4)。 */

.toolbar__right {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex-shrink: 0;
}

.toolbar__search-wrap {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: var(--control-size-compact);
  background: var(--color-input-bg);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  padding: 0 var(--spacing-sm) 0 var(--spacing-xs);
  width: clamp(220px, 26vw, 320px);
  box-sizing: border-box;
  transition:
    background-color var(--transition-fast),
    border-color var(--transition-fast),
    box-shadow var(--transition-fast);
}
.toolbar__search-wrap.focused {
  border-color: var(--color-input-border-focus);
  box-shadow: var(--control-focus-ring);
}
.toolbar__search-icon {
  color: var(--color-text-tertiary);
  flex-shrink: 0;
}
.toolbar__search-scope {
  appearance: none;
  background: transparent;
  border: none;
  border-left: 1px solid var(--color-input-border);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  padding: 0 var(--spacing-2xs) 0 var(--spacing-xs);
  margin-left: var(--spacing-2xs);
  cursor: pointer;
  outline: none;
  transition: color var(--transition-fast);
}
.toolbar__search-scope:hover {
  color: var(--color-text-primary);
}
.toolbar__search {
  flex: 1;
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  background: transparent;
  border: none;
  outline: none;
}
.toolbar__search::placeholder {
  /* 占位符有专属 token(六主题一律比 tertiary 再暗一档),此前误用 tertiary 致该 token
     无人消费、check:contrast 守着一个不渲染的值(S7 接线)。 */
  color: var(--color-text-placeholder);
}

/* 视图控件样式(.toolbar__view-controls / .toolbar-row-height / .row-height-* / .toolbar-group /
   .toolbar__select 等)已随控件抽入 GalleryViewControls.vue(Phase G G3);
   .toolbar__sort 为迁移后遗留死码, 已删(ⓕ 健壮性收尾)。 */

/* 「视图 ⋯」弹层表面(Phase G G3): 定位/backdrop/dismiss/焦点陷阱已交 UiPopover 原语, 此处仅保留表面视觉。 */
.view-popover {
  /* G8/G9: 竖排陈列折叠的视图控件(3 个独立 fold-item), 每行「标签 : 控件」两端对齐(见下 :deep) */
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: var(--spacing-sm);
  min-width: 220px;
  padding: var(--spacing-sm);
}
/* G9 菜单行布局: fold-item 去内联折叠用的 margin(容器 gap 已管间距); 内层撑满行宽 + space-between
   使标签靠左、控件靠右 —— 单按钮项(版式)由此不再孤零, 成规整的「标签 : 控件」菜单行。 */
.view-popover :deep(.fold-item) {
  margin: 0;
}
.view-popover :deep(.fold-item__inner) {
  width: 100%;
  justify-content: space-between;
  gap: var(--spacing-sm);
}

/* 「筛选 ⋯」弹层表面(Phase G G4): 溢出 chips 收纳; 竖排陈列折叠的 chip。复用视图弹层外观。
   定位/backdrop/dismiss/焦点陷阱已交 UiPopover 原语, 此处仅保留表面视觉。 */
.filter-popover {
  /* G9: 3 列等宽网格。文字 chip 占 1 列(5 个同宽同高); 星级/颜色/清除 span 2 列 → 宽度 = 列宽×2 +
     gap×1(grid 自动把跨越的那道 gap 并入, 恰合规格)。按 DOM 序自动排成三排:
       图片 视频 Live | 收藏 [评分] | [颜色] 日期。
     3 列等宽须固定弹层宽度(CSS 无法自动取「最宽 chip」当列宽); 现值按中文标签定, 切英文需调大。 */
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  align-items: center; /* 星级/颜色保持自然(较矮)高度, 行内垂直居中, 不被拉高 */
  gap: var(--spacing-sm);
  width: 252px;
  padding: var(--spacing-sm);
}
/* 菜单里 fold-item 仅作陈列容器: 去内联折叠动画用的 margin; 内层撑满其列。 */
.filter-popover :deep(.fold-item) {
  margin: 0;
}
/* 星级/颜色/清除跨 2 列: 宽度 = 列宽×2 + 中间 gap×1。 */
.filter-popover :deep(.fold-item--wide) {
  grid-column: span 2;
}
.filter-popover :deep(.fold-item__inner) {
  overflow: visible;
  width: 100%;
}
/* 每个 chip 撑满其列/跨列宽度 + 内容居中 → 5 个文字 chip 同宽, 星级/颜色居中于 2 列跨度。 */
.filter-popover :deep(.fold-item__inner > *) {
  width: 100%;
  box-sizing: border-box;
  justify-content: center;
}
/* 星级/颜色是纯图形控件, 不需要文字 chip 的高度——菜单里收窄其纵向 padding, 使其明显比文字 chip 矮、
   贴合内容(仅菜单变体; 内联工具栏保持与文字 chip 等高的一致观感)。 */
.filter-popover :deep(.chip--rating),
.filter-popover :deep(.chip--color) {
  padding-top: 4px;
  padding-bottom: 4px;
}

/* ── AI semantic search toggle ────────────────────────────────────────────── */
.toolbar__search-wrap.semantic-mode {
  border-color: color-mix(in srgb, var(--color-accent) 60%, transparent);
  background: color-mix(in srgb, var(--color-accent) 6%, var(--color-input-bg));
}

.toolbar__search-mode-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: var(--radius-xs);
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  flex-shrink: 0;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast),
    opacity var(--transition-fast);
  padding: 0;
}
.toolbar__search-mode-btn .mode-text {
  font-size: var(--font-size-2xs);
  font-weight: 800;
  letter-spacing: 0.5px;
}
.toolbar__search-mode-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.toolbar__search-mode-btn.mode-mixed {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
.toolbar__search-mode-btn.mode-semantic {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.toolbar__search-mode-btn.mode-normal {
  background: transparent;
  color: var(--color-text-tertiary);
}
.toolbar__search-mode-btn.mode-mixed:hover,
.toolbar__search-mode-btn.mode-semantic:hover {
  opacity: 0.85;
}

/* Mixed Search Dropdown */
.mixed-search-dropdown {
  position: absolute;
  top: calc(100% + var(--spacing-sm));
  left: 0;
  right: 0;
  background-color: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-lg);
  padding: var(--spacing-xs);
  z-index: 100;
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
}

.dropdown-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
  color: var(--color-text-secondary);
}

.dropdown-item:hover,
.dropdown-item.selected {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.dropdown-item.selected {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
.dropdown-item.selected .query {
  color: var(--color-accent-text);
}

.dropdown-icon-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  border-radius: var(--radius-full);
  background: var(--color-bg-surface);
  flex-shrink: 0;
}
.dropdown-item.selected .dropdown-icon-wrap {
  background: var(--color-accent-subtle);
}

.dropdown-text {
  flex: 1;
  font-size: var(--font-size-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.query {
  font-weight: 600;
  margin: 0 4px;
}

.dropdown-fade-enter-active,
.dropdown-fade-leave-active {
  transition:
    opacity var(--transition-fast),
    transform var(--transition-fast);
}
.dropdown-fade-enter-from,
.dropdown-fade-leave-to {
  opacity: 0;
  transform: translateY(-5px);
}

.toolbar__search-spinner {
  display: inline-block;
  width: 14px;
  height: 14px;
  border: 2px solid color-mix(in srgb, var(--color-accent) 25%, transparent);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: toolbar-spin var(--duration-spin) linear infinite;
  flex-shrink: 0;
}
@keyframes toolbar-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
