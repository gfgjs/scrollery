<template>
  <!-- 画廊筛选 chips(顶栏重构 Phase G G4)。从 AppToolbar 抽出以支持 Priority+ 逐项折叠:
       内联实例(variant=inline)承载 useToolbarOverflow 的测量对象(data-toolbar-item), 按
       visibleCount 逐项显隐;溢出项由「筛选 ⋯」弹层里的 menu 实例(variant=menu)渲染。
       两实例状态全经 filterStore 同步, 无重复真值源(同 GalleryViewControls 双渲染模式)。
       chip 顺序即优先级(靠前越晚折叠), 顺序与位次的**唯一事实源**是
       filterChips.descriptors.ts —— 模板一律经 idx('xxx') 取位次, 不再手写 0..7 字面量:
       插一个 chip 曾要把后面每个数字手工加一, 错了不报错只是折错 chip(S 线 §5)。 -->
  <!-- ── 重复镜头组(2026-09-02 方案 §4.1,固定在筛选 chip 集合首段)──────────────────── -->
  <!-- 重复项入口:未激活点击进入 groups;激活时选中态高亮,再点退出镜头。
       激活反馈即时(store 动作同步改状态),不等后端布局完成。 -->
  <div
    v-show="inDom(idx('duplicates'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('duplicates')) }"
  >
    <div class="fold-item__inner">
      <button
        class="chip"
        :class="{ active: lensActive }"

        @click="toggleLens"
      >
        <CopyCheck :size="14" /> {{ $t('toolbar.duplicatesChip') }}
      </button>
    </div>
  </div>
  <!-- 「已暂停 N 个筛选」摘要 chip:镜头激活且有普通筛选时存在(v-if 决定存在),只读——弹层
       列出暂停项可读名,无修改入口(§4.1 镜头内不可改筛选);位次紧随 duplicates(descriptors 第 2 位)。 -->
  <div
    v-if="lensActive && pausedCount > 0"
    v-show="inDom(idx('lensPaused'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('lensPaused')) }"
  >
    <div class="fold-item__inner">
      <button
        ref="pausedChipRef"
        class="chip"
        :title="$t('toolbar.lensPausedHint')"
        @click="togglePausedPopover"
      >
        <Pause :size="14" />
        <span>{{ $t('toolbar.lensPausedFilters', { n: pausedCount }) }}</span>
      </button>
    </div>
  </div>

  <!-- ── 普通筛选 chips:镜头激活时整体暂停(§4.1)——不卸载 fmt 实例(useFormatFilter 的剪枝
       watch 在 script setup 常驻),仅从 DOM 摘除渲染与交互入口;存在性同步收束在 descriptors 里,
       DOM 序与 CHIPS 序保持逐项对应(useToolbarOverflow 位次前提)。 -->
  <template v-if="!lensActive">
  <!-- 每个 chip 包进 .fold-item(CSS Grid 折叠格): 折叠时 grid 1fr→0fr 收缩真实宽度全程(无 max-width
       空转), 配 opacity + translateX 横向收入。wrapper 承载 v-show/data-toolbar-item/折叠态,
       chip 本体在 .fold-item__inner 内保原样(class/handler 不变)。G7 动画。 -->
  <!-- 四类媒体展示元数据与文件树共享；筛选状态仍只来自 gallery filterStore。 -->
  <div
    v-for="category in MEDIA_CATEGORY_DESCRIPTORS"
    :key="category.id"
    v-show="inDom(idx(category.id))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx(category.id)) }"
  >
    <div class="fold-item__inner">
      <button
        class="chip"
        :class="{ active: filter.mediaTypes.includes(category.id) }"

        @click="filter.toggleMediaType(category.id)"
      >
        <component :is="category.icon" :size="14" /> {{ $t(category.labelKey) }}
      </button>
    </div>
  </div>
  <!-- Live: 不是媒体类型, 是与媒体类型相交的布尔 facet(仅 UI 同列, 领域模型分离)。 -->
  <div
    v-show="inDom(idx('live'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('live')) }"
  >
    <div class="fold-item__inner">
      <button
        class="chip"
        :class="{ active: filter.livePhotoOnly }"

        @click="filter.livePhotoOnly = !filter.livePhotoOnly"
      >
        <Sparkles :size="14" /> {{ $t('toolbar.filterLive') }}
      </button>
    </div>
  </div>
  <!-- 收藏: 同 Live, 布尔 facet 而非媒体类型。 -->
  <div
    v-show="inDom(idx('favorite'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('favorite')) }"
  >
    <div class="fold-item__inner">
      <button
        class="chip"
        :class="{ active: filter.favoritedOnly }"

        @click="filter.favoritedOnly = !filter.favoritedOnly"
      >
        <Heart :size="14" /> {{ $t('toolbar.filterFavorites') }}
      </button>
    </div>
  </div>
  <!-- 细分格式（S 线 §7）: 一级触发器, 不把几十个格式 chip 铺在顶栏。位次紧随「收藏」= §5
       字面顺序(R-17 挪位;DOM 序必须与 filterChips.descriptors 的 CHIPS 序逐项对应)。
       文案「格式」→「格式 N」随已选**个数**变宽 —— 该反应源已在描述符里随 chip 一同声明。 -->
  <div
    v-show="inDom(idx('format'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('format')) }"
  >
    <div class="fold-item__inner">
      <button
        ref="formatChipRef"
        class="chip"
        :class="{ active: filter.fileFormats.length > 0 }"
        :title="$t('toolbar.filterByFormat')"
        @click="toggleFormatPopover"
      >
        <FileType :size="14" />
        <span>{{
          filter.fileFormats.length > 0
            ? $t('toolbar.filterFormatN', { count: filter.fileFormats.length })
            : $t('toolbar.filterFormat')
        }}</span>
      </button>
    </div>
  </div>
  <!-- 评分筛选：内联星级直接绑 minRating（"≥N 星"语义）；点当前星级清空。 -->
  <div
    v-show="inDom(idx('rating'))"
    :data-toolbar-item="itemAttr"
    class="fold-item fold-item--wide"
    :class="{ 'fold-item--folded': folded(idx('rating')) }"
  >
    <div class="fold-item__inner">
      <div
        class="chip chip--rating"
        :class="{ active: filter.minRating > 0 }"
        :title="$t('toolbar.filterByRating')"
      >
        <StarRating v-model="filter.minRating" :size="14" />
        <span v-if="filter.minRating > 0" class="chip__rating-suffix">+</span>
      </div>
    </div>
  </div>
  <!-- 颜色标签筛选（T16）：内联色块直接绑 colorLabel（精确匹配某色档）；点当前色清空。 -->
  <div
    v-show="inDom(idx('color'))"
    :data-toolbar-item="itemAttr"
    class="fold-item fold-item--wide"
    :class="{ 'fold-item--folded': folded(idx('color')) }"
  >
    <div class="fold-item__inner">
      <div
        class="chip chip--color"
        :class="{ active: filter.colorLabel > 0 }"
        :title="$t('toolbar.filterByColorLabel')"
      >
        <ColorLabelPicker v-model="filter.colorLabel" :size="13" />
      </div>
    </div>
  </div>
  <!-- 日期范围筛选（T15）：chip 切换弹层，弹层内两个原生日期输入绑 from/to。
       两端皆备方下发 date_range 谓词（见 filterStore.toApiFilter）；active 态同步。 -->
  <div
    v-show="inDom(idx('date'))"
    :data-toolbar-item="itemAttr"
    class="fold-item"
    :class="{ 'fold-item--folded': folded(idx('date')) }"
  >
    <div class="fold-item__inner">
      <button
        ref="dateChipRef"
        class="chip chip--date"
        :class="{ active: isDateActive }"
        :title="$t('toolbar.filterByDateRange')"
        @click="toggleDatePopover"
      >
        <Calendar :size="14" />
        <span v-if="isDateActive" class="chip__date-label">{{ dateRangeLabel }}</span>
        <span v-else>{{ $t('toolbar.searchScopeDate') }}</span>
      </button>
    </div>
  </div>
  <!-- 清除筛选：仅有活动筛选时存在(v-if 决定存在, v-show 决定是否折叠)。
       描述符里它是唯一 present 有条件的项, 且**必须留在末位**: 中间项时有时无会让位次随状态跳动。 -->
  <div
    v-if="filter.hasActiveFilters"
    v-show="inDom(idx('clear'))"
    :data-toolbar-item="itemAttr"
    class="fold-item fold-item--wide"
    :class="{ 'fold-item--folded': folded(idx('clear')) }"
  >
    <div class="fold-item__inner">
      <button class="chip chip--clear" @click="clearAllFilters">
        <X :size="14" /> {{ $t('toolbar.clearFilters') }}
      </button>
    </div>
  </div>
  </template>
  <!-- </template v-if="!lensActive"> 普通筛选 chips 到此为止(含上方两个弹层) -->

  <!-- 已暂停筛选摘要弹层:只读陈列,无修改入口(§4.1);定位/dismiss 交 UiPopover 原语。 -->
  <UiPopover
    v-model:open="showPausedPopover"
    :anchor="pausedChipRef ?? null"
    placement="bottom-start"
  >
    <div class="paused-popover">
      <p class="paused-popover__hint">{{ $t('toolbar.lensPausedHint') }}</p>
      <ul class="paused-popover__list">
        <li v-for="label in pausedFilterLabels" :key="label">{{ label }}</li>
      </ul>
    </div>
  </UiPopover>

  <!-- 日期范围弹层：迁 UiPopover 原语(Teleport 逃逸 .toolbar__filters 的 overflow 裁切 + @floating-ui
       定位[含 flip/shift，一并修掉旧手写漏水平钳制的越界 bug] + backdrop/Esc dismiss + 焦点陷阱)。
       每实例各持一份(内联/菜单), 同一时刻只有可见 chip 可点开；表面视觉(.date-popover 的 bg/border/
       shadow)保留在 slot 内容里，原语只负责定位与 dismiss。 -->
  <!-- 格式弹层: 每实例各持一份(内联/菜单), 同一时刻只有可见 chip 可点开(同日期弹层惯例)。
       fmt 实例由本组件**常驻**持有并下传 —— 剪枝 watch 必须常驻(用户取消大类 chip 时弹层通常
       是关的), 挂在 v-if 卸载的弹层里就只有开着弹层时才剪。 -->
  <UiPopover
    v-model:open="showFormatPopover"
    :anchor="formatChipRef ?? null"
    placement="bottom-start"
  >
    <FormatFilterPopover :fmt="fmt" />
  </UiPopover>

  <UiPopover v-model:open="showDatePopover" :anchor="dateChipRef ?? null" placement="bottom-start">
    <div class="date-popover">
      <label class="date-popover__field">
        <span>{{ $t('toolbar.dateFrom') }}</span>
        <input type="date" v-model="dateFromInput" :max="dateToInput || undefined" />
      </label>
      <label class="date-popover__field">
        <span>{{ $t('toolbar.dateTo') }}</span>
        <input type="date" v-model="dateToInput" :min="dateFromInput || undefined" />
      </label>
      <div class="date-popover__actions">
        <button class="date-popover__btn" @click="clearDateRange">
          {{ $t('toolbar.dateClear') }}
        </button>
        <button
          class="date-popover__btn date-popover__btn--primary"
          @click="showDatePopover = false"
        >
          {{ $t('onboarding.finish') }}
        </button>
      </div>
    </div>
  </UiPopover>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Sparkles,
  Heart,
  X,
  Calendar,
  FileType,
  CopyCheck,
  Pause,
} from '@lucide/vue'
import StarRating from '../common/StarRating.vue'
import ColorLabelPicker from '../common/ColorLabelPicker.vue'
import UiPopover from '../ui/UiPopover.vue'
import FormatFilterPopover from './FormatFilterPopover.vue'
import { useFilterStore } from '../../stores/filterStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useFormatFilter } from '../../composables/useFormatFilter'
import {
  chipIndexOf,
  chipStateOf,
  pausedFilterCount,
  type FilterChipId,
  type FilterChipState,
} from './filterChips.descriptors'
import { MEDIA_CATEGORY_DESCRIPTORS } from '../../constants/mediaCategoryDescriptors'

// variant: 内联(测量+按 visibleCount 显隐) / 菜单(只渲染溢出项)。
// visibleCount: 内联可见的项数(useToolbarOverflow 算出); measuring: 测量帧, 内联须全渲染。
const props = defineProps<{
  variant: 'inline' | 'menu'
  visibleCount: number
  measuring: boolean
}>()

const filter = useFilterStore()
const { t } = useI18n()

// ── 重复镜头(2026-09-02 方案 §4.1)──────────────────────────────────────────────
// URL 是镜头模式的事实源(§10.1),store.mode 由 useGalleryQuerySync 从路由收敛;本组件只发动作。
const lens = useDuplicateLensStore()
const lensActive = computed(() => lens.isLensActive)

function toggleLens() {
  // 未激活点击进入 groups(§4.1 首次默认);已激活再点退出镜头、恢复返回快照。
  if (lensActive.value) lens.exitLens()
  else lens.enterLens('groups')
}

/**
 * chip 位次的状态源 —— `chipStateOf` 是 FilterChipState 的唯一构造点(descriptors 内),普通筛选
 * 维度 + 镜头激活态在此合流。镜头激活时普通 chip 整体暂停(存在性在描述符里收束),位次随之重排。
 */
const chipState = computed<FilterChipState>(() => chipStateOf(filter, lensActive.value))

/** 已暂停筛选的维度数(§4.1「已暂停 N 个筛选」的 N;与 hasActiveFilters 同一套维度判定)。 */
const pausedCount = computed(() => pausedFilterCount(chipState.value))

/** data-toolbar-item 仅内联实例需要(测量对象在 .toolbar__filters 容器内 querySelector);
     菜单实例不参与测量, 绑 null 移除该属性避免混入其它容器的测量。 */
const itemAttr = computed(() => (props.variant === 'inline' ? '' : null))

/**
 * chip 位次 —— 唯一事实源是 `filterChips.descriptors.ts`,模板不再手写 0..7 字面量。
 *
 * 位次随「哪些 chip 存在」变化(清除 chip 有 v-if、镜头暂停普通 chip),故经 chipState 实时算。
 * AppToolbar 的 `chipCount` 与 `overflowRemeasureKey` 出自同一个数组 —— 三处再不会各自漂移(S 线 §5)。
 */
function idx(id: FilterChipId): number {
  return chipIndexOf(id, chipState.value)
}

/**
 * 某序号 chip 是否留在 DOM(G6 折叠动画: 内联项恒在 DOM, 靠 chip--folded 收拢隐藏以便过渡;
 * 菜单只渲染溢出项, 直接 v-show)。
 */
function inDom(i: number): boolean {
  return props.variant === 'inline' ? true : i >= props.visibleCount
}
/**
 * 内联折叠态(收拢): 非测量帧且 index >= visibleCount。菜单为纵向列表不横向折叠, 恒 false。
 * 测量帧全展(chip--folded 关)→ offsetWidth 取自然宽, 否则测失真。
 */
function folded(i: number): boolean {
  return props.variant === 'inline' && !props.measuring && i >= props.visibleCount
}

// ── 细分格式筛选（S 线 §7）──────────────────────────────────────────────────────
// fmt 实例在此**常驻**(本组件随顶栏常挂),而非放进 v-if 卸载的弹层:它内含「取消大类 → 剪掉该类
// 已选格式」的跨维度 watch,而用户取消大类 chip 时弹层通常是关的。弹层只是它的一个视图。
const fmt = useFormatFilter()
const showFormatPopover = ref(false)
const formatChipRef = ref<HTMLElement>()
function toggleFormatPopover() {
  showFormatPopover.value = !showFormatPopover.value
}

// ── 日期范围筛选（T15，从 AppToolbar 迁入）──────────────────────────────────────
// filterStore.dateFrom/dateTo 存 Unix epoch「秒」（与 sort_datetime 同单位）；原生
// <input type="date"> 用 'YYYY-MM-DD' 字符串。两者间按「本地民用日」互转：from 取当日 0 点、
// to 取当日 23:59:59，使范围对用户选的两天均为闭区间。
const showDatePopover = ref(false)
const dateChipRef = ref<HTMLElement>()

const isDateActive = computed(() => filter.dateFrom !== null && filter.dateTo !== null)

/** epoch 秒 → 'YYYY-MM-DD'（本地）；null/非法 → ''。 */
function tsToInput(ts: number | null): string {
  if (ts == null) return ''
  const d = new Date(ts * 1000)
  if (Number.isNaN(d.getTime())) return ''
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

/** 'YYYY-MM-DD' → epoch 秒；endOfDay=true 取当日 23:59:59，否则 0 点。空/非法 → null。 */
function inputToTs(val: string, endOfDay: boolean): number | null {
  if (!val) return null
  const [y, m, d] = val.split('-').map(Number)
  if (!y || !m || !d) return null
  const date = endOfDay ? new Date(y, m - 1, d, 23, 59, 59) : new Date(y, m - 1, d, 0, 0, 0)
  const t = date.getTime()
  return Number.isNaN(t) ? null : Math.floor(t / 1000)
}

const dateFromInput = computed<string>({
  get: () => tsToInput(filter.dateFrom),
  set: (v: string) => {
    filter.dateFrom = inputToTs(v, false)
  },
})
const dateToInput = computed<string>({
  get: () => tsToInput(filter.dateTo),
  set: (v: string) => {
    filter.dateTo = inputToTs(v, true)
  },
})

// chip 上的紧凑区间标签：同年省略起始年份，仅显示「M/D – M/D」或「YYYY/M/D – M/D」。
const dateRangeLabel = computed(() => {
  const from = filter.dateFrom != null ? new Date(filter.dateFrom * 1000) : null
  const to = filter.dateTo != null ? new Date(filter.dateTo * 1000) : null
  if (!from || !to) return ''
  const sameYear = from.getFullYear() === to.getFullYear()
  const fromStr = sameYear
    ? `${from.getMonth() + 1}/${from.getDate()}`
    : `${from.getFullYear()}/${from.getMonth() + 1}/${from.getDate()}`
  const toStr = `${to.getMonth() + 1}/${to.getDate()}`
  return `${fromStr} – ${toStr}`
})

function toggleDatePopover() {
  // 定位/overflow 裁切规避/dismiss 已交 UiPopover(@floating-ui)：此处只切开阖，
  // 锚点经模板 :anchor="dateChipRef" 传入，v-model:open 双向同步开阖态。
  showDatePopover.value = !showDatePopover.value
}

function clearDateRange() {
  filter.dateFrom = null
  filter.dateTo = null
}

// 「清除筛选」统一出口：清空所有筛选并关闭日期弹层（否则弹层残留指向已清空状态）。
function clearAllFilters() {
  filter.clearFilters()
  showDatePopover.value = false
}

// ── 「已暂停 N 个筛选」摘要弹层(2026-09-02 方案 §4.1)─────────────────────────────
// 镜头内普通筛选只读:弹层仅陈列各暂停维度的可读名,无任何修改入口;退出镜头后原值经返回快照恢复。
const showPausedPopover = ref(false)
const pausedChipRef = ref<HTMLElement>()

function togglePausedPopover() {
  showPausedPopover.value = !showPausedPopover.value
}

/** 暂停维度的可读名列表(与 pausedFilterCount 同一套维度判定;文案随 locale 变)。 */
const pausedFilterLabels = computed<string[]>(() => {
  if (!lensActive.value || pausedCount.value === 0) return []
  const labels: string[] = []
  for (const category of MEDIA_CATEGORY_DESCRIPTORS) {
    if (filter.mediaTypes.includes(category.id)) labels.push(t(category.labelKey))
  }
  if (filter.fileFormats.length > 0) {
    labels.push(t('toolbar.filterFormatN', { count: filter.fileFormats.length }))
  }
  if (filter.livePhotoOnly) labels.push(t('toolbar.filterLive'))
  if (filter.favoritedOnly) labels.push(t('toolbar.filterFavorites'))
  if (filter.minRating > 0) labels.push(t('toolbar.filterByRating'))
  if (filter.colorLabel > 0) labels.push(t('toolbar.filterByColorLabel'))
  if (filter.dateFrom !== null && filter.dateTo !== null) {
    labels.push(t('toolbar.filterByDateRange'))
  }
  return labels
})
</script>

<style scoped>
/* G7 折叠动画: 每 chip 包进 .fold-item(CSS Grid 折叠格)。1fr→0fr 动画**真实宽度全程**(无 G6
   max-width 空转致「闪一下」); .fold-item__inner overflow:hidden 裁内容; translateX + opacity
   增强横向「收入」感。间距用 margin-inline-end(容器 gap:0), 折叠时 margin 归 0 无碎屑;
   引擎测量含 margin(useToolbarOverflow)。测量对象=.fold-item(data-toolbar-item)。 */
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
/* 收缩时内容保自然宽被裁(而非被压扁) */
.fold-item__inner > * {
  flex-shrink: 0;
}
.fold-item--folded .fold-item__inner {
  transform: translateX(10px);
}
/* 菜单变体(竖排): folded 恒 false → grid 保持 1fr(全宽行), 仅靠 v-show 显隐折叠项, 不做横向收缩。 */

.chip--clear {
  color: var(--color-accent);
  background: var(--color-accent-subtle);
  border-color: transparent;
}
.chip--clear:hover {
  background: color-mix(in srgb, var(--color-accent) 16%, transparent);
  border-color: transparent;
}

/* 评分筛选 chip：容纳内联星级 + "≥N" 的 "+" 后缀；active 高亮沿用全局 .chip.active。 */
.chip--rating {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.chip__rating-suffix {
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--color-accent-text);
  margin-left: 1px;
}

/* 颜色标签筛选 chip：容纳内联色块；active 高亮沿用全局 .chip.active。 */
.chip--color {
  display: inline-flex;
  align-items: center;
}

/* 日期范围筛选 chip：图标 + 文案/区间标签；active 高亮沿用全局 .chip.active。 */
.chip--date {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  white-space: nowrap;
}
.chip__date-label {
  font-variant-numeric: tabular-nums;
}

/* 日期弹层表面：定位/backdrop/dismiss/焦点陷阱已交 UiPopover 原语，此处仅保留弹层表面视觉。 */
.date-popover {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  min-width: 220px;
  padding: var(--spacing-md);
}
.date-popover__field {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
.date-popover__field > span {
  width: 1.5em;
  flex-shrink: 0;
}
.date-popover__field input[type='date'] {
  flex: 1;
  height: var(--control-size-default);
  padding: 0 var(--spacing-sm);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  outline: none;
  color-scheme: dark light; /* 让原生日期控件的弹出日历跟随主题明暗 */
}
.date-popover__field input[type='date']:focus {
  border-color: var(--color-input-border-focus);
  box-shadow: var(--control-focus-ring);
}
.date-popover__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
  margin-top: 2px;
}
.date-popover__btn {
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    color var(--transition-fast),
    background var(--transition-fast);
}
.date-popover__btn:hover {
  border-color: var(--color-border-strong);
  color: var(--color-text-primary);
}
/* accent 底上的按钮文字用 text-inverse:暗色主题 accent 是亮色(如 #818cf8),
   白字压上去 ~1.6:1 不可读——inverse 在暗主题自动落深字(S5 批2 实修)。 */
.date-popover__btn--primary {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: var(--color-text-on-accent);
}
.date-popover__btn--primary:hover {
  background: var(--color-accent-hover);
  color: var(--color-text-on-accent);
}

/* 已暂停筛选摘要弹层表面:定位/backdrop/dismiss 交 UiPopover,此处仅保留表面视觉。 */
.paused-popover {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  min-width: 200px;
  max-width: 280px;
  padding: var(--spacing-md);
}
.paused-popover__hint {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.paused-popover__list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-2xs);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
</style>
