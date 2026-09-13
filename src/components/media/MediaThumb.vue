<template>
  <!-- 平均色占位（后端 hydrate 由 thumbhash 算好过桥；null → CSS 变量回退） -->
  <div
    class="media-thumb"
    :style="thumbStyle"
    :class="{
      loaded: isLoaded,
      'media-thumb--placeholder': !isLoaded,
      'media-thumb--selected': isSelected,
      'media-thumb--selection-mode': isSelectionMode && !compact,
      'media-thumb--drag-hover': isDragHover,
      'media-thumb--compact': compact,
      'media-thumb--missing': availability === 'missing',
      'media-thumb--offline': availability === 'offline',
    }"
    @mouseenter="isHovering = true; onHoverEnter()"
    @mouseleave="isHovering = false; onHoverLeave()"
    @mousemove="onCellMove"
  >
    <!-- 可用态角标（缺失检测 Part2 §3.2）：missing/offline 始终可见（含 compact），一眼可辨。 -->
    <div
      v-if="availability !== 'online'"
      class="media-thumb__avail"
      :class="'avail--' + availability"
      :title="
        availability === 'missing' ? $t('media.availMissingTitle') : $t('media.availOfflineTitle')
      "
    >
      {{ availability === 'missing' ? $t('media.availMissing') : $t('settings.volOffline') }}
    </div>
    <!-- 颜色标签色条（T16）：缩略图顶缘细条（Lightroom 式）。直接挂 .media-thumb（非 overlays），
         故 compact 下也显（零 SVG、仅一个 div，利于快速 culling）。 -->
    <div v-if="colorStrip" class="media-thumb__color-strip" :style="{ background: colorStrip }"></div>
    <!-- 拖拽手柄(需求3):已选中格左上角的抓取点,按住它拖到文件夹树 = 移动(Shift = 复制)。
         刻意置于 overlays(v-if !compact) 块之外、仅由 isSelected 门控 —— 使 Canvas 悬停卡在
         compact+选择态(MediaThumb 内部 compact=true)下也显手柄;DOM 网格 compact 走 MediaThumbCompact
         (无此组件)→ 无手柄,符合「Canvas 主推、DOM compact 不提供拖到文件夹」的裁决。
         命中判定见 useMediaDragToFolder.onCardPointerDown(closest('.media-thumb__drag-handle'))。
         §8.1 browse-only:重复镜头禁用拖拽,手柄一并隐藏。 -->
    <div
      v-if="isSelected && ui.showDragHandle && !browseOnly"
      class="media-thumb__drag-handle"
      :title="$t('common.move')"

    >
      <GripVertical :size="14" />
    </div>
    <!-- Text-document card (txt/md/office) — CSS「文本卡」，零解码，比灰底占位更清晰（§3.4） -->
    <div
      v-if="!isLoaded && isTextCard"
      class="media-thumb__textcard"
      :class="'fmt-' + (fileFormat || '').toLowerCase()"
    >
      <div class="media-thumb__textcard-lines">
        <span></span>
        <span></span>
        <span></span>
        <span></span>
      </div>
      <span class="media-thumb__textcard-ext">{{ (fileFormat || '').toUpperCase() }}</span>
    </div>
    <!-- RAW 专属占位符(已注册未解码,thumb_status=2):区别于损坏文件的通用空白——
         RAW 图标 + 扩展名,告知用户「非损坏,预览即将支持」而非「软件坏了」。 -->
    <div
      v-else-if="!isLoaded && isRawNoThumbCell"
      class="media-thumb__placeholder media-thumb__placeholder--raw"
      :style="{ backgroundColor: placeholderBgColor }"
    >
      <span class="media-thumb__raw-icon">RAW</span>
      <span v-if="fileFormat" class="media-thumb__ext">{{ fileFormat.toUpperCase() }}</span>
    </div>
    <!-- 纯色占位符 + 文件格式文本 -->
    <div
      v-else-if="!isLoaded"
      class="media-thumb__placeholder"
      :class="{ 'is-fallback': !placeholderColor }"
      :style="{ backgroundColor: placeholderBgColor }"
    >
      <span v-if="fileFormat" class="media-thumb__ext">{{ fileFormat.toUpperCase() }}</span>
    </div>
    <!-- 实际图片 -->
    <img
      v-if="displaySrc"
      class="media-thumb__img thumb-loaded"
      :src="displaySrc"
      :width="w"
      :height="h"
      loading="lazy"
      @error="onError"
    />

    <!-- Hover auto-play preview (video / live photo) — 共享池，同时仅一个在播放（需求1） -->
    <!-- is-painted：首帧真正合成呈现(rVFC)后才渐显，之前 opacity:0 让封面图透出，避免「移入闪黑/灰白」 -->
    <video
      v-if="isHoverPreview"
      class="media-thumb__video"
      :class="{ 'is-painted': previewPainted }"
      :src="hoverSrc"
      loop
      autoplay
      playsinline
      preload="metadata"
      @loadedmetadata="onPreviewReady"
      @loadeddata="onPreviewFirstFrame"
      @error="onPreviewError"
    />

    <!-- Hover scrub sprite — 鼠标横移切关键帧，不解码视频。横移事件由格根 @mousemove 统一处理
         (onCellMove)，此处不再单挂 handler(避免与格根双触发；scrub 态下本 div 覆盖全格，
         mousemove 冒泡到格根即可)。-->
    <div v-if="isScrubbing" class="media-thumb__sprite" :style="scrubStyle" />

    <!-- 覆盖层 -->
    <!-- T0(极密网格 §5):compact(60px 类)整个覆盖层不渲染——含选择态。此前 `|| isSelectionMode`
         会在进入选择态时给全窗口每格瞬间新建 overlays+checkbox 节点(数千格一次 flush 内建节点+
         布局+全文档 style recalc = 用户报告的「进选择态卡顿」)。现 compact 选中态由
         .media-thumb--selected 的内描边表达,选择经卡片点击切换(handleCardClick §5.4)。 -->
    <div v-if="!compact" class="media-thumb__overlays">
      <!-- Advanced Info Overlay & Badges -->
      <div
        v-if="
          !compact &&
          (thumbInfoLines.length > 0 ||
            (similarity == null &&
              isLoaded &&
              thumbStatus === 3 &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('status')) ||
            (similarity == null &&
              isLoaded &&
              thumbStatus === 1 &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('status')) ||
            (similarity == null &&
              fileSize &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('size')) ||
            similarity != null ||
            isLivePhoto ||
            showTypeBadge)
        "
        class="media-thumb__info-overlay"
      >
        <div
          class="media-thumb__badges"
        >
          <span
            v-if="
              similarity == null &&
              isLoaded &&
              thumbStatus === 3 &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('status')
            "
            class="badge badge-source"
            :title="$t('media.badgeOrigTitle')"
            >ORIG</span
          >
          <span
            v-if="
              similarity == null &&
              isLoaded &&
              thumbStatus === 1 &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('status')
            "
            class="badge badge-thumb"
            :title="$t('media.badgeThumbTitle')"
            >THUMB</span
          >
          <span
            v-if="
              similarity == null &&
              fileSize &&
              ui.showThumbInfo &&
              ui.thumbInfoElements.includes('size')
            "
            class="badge badge-size"
            >{{ formatFileSize(fileSize) }}</span
          >
          <span v-if="similarity != null" class="badge badge-similarity"
            >{{ Math.round(similarity * 100) }}%</span
          >
          <span v-if="isLivePhoto" class="badge badge-live">LIVE</span>
          <span
            v-if="showTypeBadge"
            class="badge"
            :class="`badge-${typeBadge}`"
            :title="$t(typeBadgeTitleKey)"
            >{{ typeBadgeLabel }}</span
          >
        </div>
        <div v-for="(line, idx) in thumbInfoLines" :key="idx" class="info-line">{{ line }}</div>
      </div>

      <!-- 视频播放角标:撑到预览首帧真正呈现(previewPainted)才隐藏。此前挂载即隐——
           角标在视频还不可见的加载空窗先消失,多一次状态闪(用户报告闪烁链一环)。 -->
      <span
        v-if="mediaType === 'video' && !compact && !previewPainted && !isScrubbing"
        class="badge badge-video"
        ><Play :size="20" fill="#fff"
      /></span>
      <!-- 时长 -->
      <span v-if="durationMs && !compact" class="badge badge-duration">{{
        formatDuration(durationMs)
      }}</span>
      <!-- 收藏（极小尺寸下隐藏 —— 点不动且是常驻的高成本 SVG）。
           §8.1 browse-only:重复镜头隐藏卡片收藏快捷动作(含已收藏常显态——镜头卡面不含收藏语义)。 -->
      <button
        v-if="!compact && !browseOnly"
        class="media-thumb__fav"
        :class="{
          active: isFavorited,
          'fav-always-visible':
            isFavorited && ui.showThumbInfo && ui.thumbInfoElements.includes('favorite'),
        }"
        @pointerdown.stop
        @click.stop="toggleFav"
        :title="$t('selection.favorite')"

      >
        <Heart
          :size="14"
          :fill="isFavorited ? '#ff4757' : 'none'"
          :color="isFavorited ? '#ff4757' : '#fff'"
          :stroke-width="isFavorited ? 0 : 2"
        />
      </button>
      <!-- 评分星级（左下，经典桌面相册式）：已评分时显示只读填充星角标；hover 整格出完整 5 星
           交互条供快捷打分/清零。!compact 守门（5 个 SVG 在极小尺寸下昂贵，同收藏纪律）。
           §8.1 browse-only:重复镜头隐藏卡片评分快捷动作(整槽含只读角标——镜头卡面按 §6.2
           只呈现组位次/路径/可用态,不含评分)。 -->
      <div v-if="!compact && !browseOnly" class="media-thumb__rating-slot">
        <StarRating
          v-if="isHovering"
          class="media-thumb__rating media-thumb__rating--edit"
          tone="rating"
          :model-value="rating"
          :size="13"
          @change="onRate"
          @pointerdown.stop
          @click.stop
        />
        <StarRating
          v-else-if="rating > 0"
          class="media-thumb__rating"
          tone="rating"
          :model-value="rating"
          :max="rating"
          :size="12"
          readonly
          @pointerdown.stop
        />
      </div>
      <!-- 选择复选框 -->
      <div
        v-if="isSelected || isSelectionMode"
        class="media-thumb__checkbox"
        @pointerdown.stop
        @click.stop="emit('select', id)"
      >
        <div class="checkbox" :class="{ checked: isSelected }">
          <Check v-if="isSelected" :size="12" />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { Play, Heart, Check, GripVertical } from '@lucide/vue'
import { formatDuration, formatFileSize } from '../../utils/format'
import StarRating from '../common/StarRating.vue'
import { colorLabelHex } from '../../constants/colorLabels'

import { useUiStore } from '../../stores/uiStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useHoverPreview } from '../../composables/useHoverPreview'
import { useThumbLoader } from '../../composables/useThumbLoader'
import {
  buildThumbInfoLines,
  isTextCardFormat,
  isTextCardFallback,
  isRawNoThumb,
  typeBadgeOf,
} from './mediaGrid.helpers'
import type { LayoutRowItem } from '../../types/layout'

interface Props {
  id: number
  w: number
  h: number
  mediaType: string
  isLivePhoto?: boolean
  durationMs?: number | null
  thumbStatus: number
  thumbPath?: string | null
  /** 占位平均色 CSS `#rrggbb`（后端 hydrate 算好；null → 回退 CSS 变量）。 */
  placeholderColor?: string | null
  fileFormat?: string
  fileSize?: number
  similarity?: number
  isFavorited?: boolean
  /** 用户评分 0-5（0 = 未评分）。扁平 prop（镜像 isFavorited），保证乐观更新的响应式与收藏路径一致。 */
  rating?: number
  /** 用户颜色标签 0-7（0 = 未标，T16）。扁平 prop，镜像 rating。 */
  colorLabel?: number
  isSelected?: boolean
  isSelectionMode?: boolean
  isDragHover?: boolean
  /** 重复镜头 browse-only(§8.1):隐藏卡片收藏/评分快捷动作与拖拽手柄。
   *  由宿主(MediaGridRow / Canvas 悬停卡)从 duplicateLensStore.isLensActive 透传;
   *  本组件不直接读镜头 store——它也被文档阅读器等非画廊场景复用,镜头语义不外溢。 */
  browseOnly?: boolean
  /** 强制全功能渲染(跳过 compact 门控)。Canvas 悬停卡专用:单例实例无「×数百格」的
   *  成本乘法,倍率封顶后卡可小于 100px,仍需完整控件(星级/红心/checkbox)。网格内勿用。 */
  forceFull?: boolean
  cacheDir: string
  item?: LayoutRowItem
}

const props = withDefaults(defineProps<Props>(), {
  isLivePhoto: false,
  durationMs: null,
  thumbPath: null,
  placeholderColor: null,
  isFavorited: false,
  rating: 0,
  colorLabel: 0,
  isSelected: false,
  isSelectionMode: false,
  isDragHover: false,
  browseOnly: false,
  forceFull: false,
})

const emit = defineEmits<{
  (e: 'click', id: number): void
  (e: 'select', id: number): void
  (e: 'favorite', id: number): void
  (e: 'rate', id: number, value: number): void
  (e: 'request-thumb', id: number): void
  (e: 'cancel-thumb', id: number): void
  (e: 'regenerate-thumb', id: number): void
}>()

const ui = useUiStore()
const media = useMediaStore()

// 重型元数据（fileName/dirPath/EXIF/GPS）不再随布局行项携带；改为按可视区懒加载到 store 中。
const meta = computed(() => (props.id != null ? media.viewportMeta.get(props.id) : undefined))

const favAnimating = ref(false)
// hover 态：驱动评分星条「只读角标 ↔ 交互 5 星」切换 —— 仅当前 hover 的那一格渲染交互条，
// 全屏同时只存在一组交互 SVG（同收藏红心的成本纪律）。未 hover 且 rating>0 时仅显填充星角标。
const isHovering = ref(false)

// 缩略图加载(status 1/3/0 解码 + 懒自愈)抽到 useThumbLoader 单源(T1 §6.3)，与 MediaThumbCompact /
// MediaGridCanvas 共享同一逻辑，杜绝多份 loadThumb 漂移。displaySrc/isLoaded 供模板；onError 兜底自愈。
const { isLoaded, displaySrc, onError } = useThumbLoader(
  {
    id: () => props.id,
    thumbStatus: () => props.thumbStatus,
    thumbPath: () => props.thumbPath,
    cacheDir: () => props.cacheDir,
  },
  {
    requestThumb: (id) => emit('request-thumb', id),
    cancelThumb: (id) => emit('cancel-thumb', id),
    regenerateThumb: (id) => emit('regenerate-thumb', id),
  },
)

const thumbStyle = computed(() => ({
  width: `${props.w}px`,
  height: `${props.h}px`,
}))

// 低于此单元尺寸时，砍掉非必要的覆盖层/徽章 —— 关键是其 `backdrop-filter` 模糊与常驻
// 的收藏 SVG。它们在此尺度下看不清/点不动，但数百个同屏时（如极小网格 + 全量缩略图
// 生成）开销极大。选择复选框保留（仍可用）。
const COMPACT_THUMB_PX = 100
const compact = computed(
  () => !props.forceFull && (props.w < COMPACT_THUMB_PX || props.h < COMPACT_THUMB_PX),
)

// 悬停自动播放预览（需求1）。共享池容量 1 —— 同一时刻只有一个格子在播放。
const {
  isPreviewing: isHoverPreview,
  previewSrc: hoverSrc,
  isScrubbing,
  scrubStyle,
  onMove: onHoverMove,
  onEnter: onHoverEnter,
  onLeave: onHoverLeave,
  onPreviewMeta,
  onPreviewError,
} = useHoverPreview({
  id: () => props.id,
  mediaType: () => props.mediaType,
  isLivePhoto: () => !!props.isLivePhoto,
  fileSize: () => props.fileSize ?? 0,
  // 内在分辨率取自布局行项(originalWidth/Height,Rust hydrate 时从 media_items 真填);判「重」用。
  // item 缺失(理论上网格外调用)时传 0 → 按不重处理、安全默认为播放。
  videoWidth: () => props.item?.originalWidth ?? 0,
  videoHeight: () => props.item?.originalHeight ?? 0,
  compact: () => compact.value,
  isSelectionMode: () => !!props.isSelectionMode,
})

// 悬停预览「首帧已呈现」标志：解决「移入后 灰-透明-白 闪几下」。
// video 元素在 isHoverPreview 变 true 时立即渲染（z-index 盖住封面图），但 loadeddata 只保证
// 首帧**数据**就绪、不保证已**合成呈现**——WebView2 硬解视频层在首帧真正呈现前可闪灰/白，
// 渐显若在这段空窗期开跑就是用户看到的闪烁链。改为 requestVideoFrameCallback：首帧确已
// 合成后才渐显（封面图在此之前一直垫底）；无 rVFC 的环境回退 loadeddata 即显。
// 预览结束（isHoverPreview→false）即复位，保证下一次进入的新 video 仍从 opacity:0 开始。
const previewPainted = ref(false)
watch(isHoverPreview, (v) => {
  if (!v) previewPainted.value = false
})
function onPreviewFirstFrame(e: Event) {
  const v = e.target as HTMLVideoElement
  const rvfc = (
    v as HTMLVideoElement & { requestVideoFrameCallback?: (cb: () => void) => number }
  ).requestVideoFrameCallback
  // 护栏:划走后 video 已卸载而回调滞后到达时不得置位——否则 previewPainted 悬空为 true,
  // 会把下一次静态态的播放角标误隐掉。
  if (rvfc) {
    rvfc.call(v, () => {
      if (isHoverPreview.value) previewPainted.value = true
    })
  } else {
    previewPainted.value = true
  }
}

// 格级横移单一入口：play 态越死区切 scrub、scrub 态映射帧（逻辑在 useHoverPreview.onMove）。
// 挂在格根而非 sprite div——play 态时上层是 <video>、sprite 尚未渲染，须在格根就能感知横移意图；
// 且用「clientX − 格 rect.left」而非 e.offsetX：offsetX 原点随 e.target（视频/雪碧图/覆盖层）漂移，
// 相对格左缘的 clientX 稳定。仅在本格确处预览/scrub 时测量（getBoundingClientRect 读会触发回流，
// 非活动格/图片格直接跳过，避免每次 mousemove 无谓回流）。
function onCellMove(e: MouseEvent) {
  if (!isHoverPreview.value && !isScrubbing.value) return
  const el = e.currentTarget as HTMLElement
  const rect = el.getBoundingClientRect()
  onHoverMove(e.clientX - rect.left, rect.width)
}

// 元数据加载后置静音并播放 —— 用属性方式设 muted 规避 Vue 对 `muted` 特性绑定的已知问题，
// 同时满足浏览器自动播放策略。
function onPreviewReady(e: Event) {
  const v = e.target as HTMLVideoElement
  v.muted = true
  v.play().catch(() => {})
  // 真实分辨率补判：DB 尺寸在 enrich 前是占位（1280×720），静态判据漏掉的超 4K 在此降级。
  onPreviewMeta(v.videoWidth, v.videoHeight)
}

const thumbInfoLines = computed(() => {
  // compact 模式下直接跳过，避免 800+ 组件追踪 6+ 响应式依赖
  if (compact.value || !ui.showThumbInfo || !props.item) return []
  // 组装逻辑与 Canvas 网格共享单源(mediaGrid.helpers.buildThumbInfoLines)。
  return buildThumbInfoLines(props.item, meta.value, ui.thumbInfoElements)
})

// 系统可用态（缺失检测 Part2 §3.2）：从布局行项读取，旧缓存项缺该字段时默认 'online'。
const availability = computed<string>(() => props.item?.availability ?? 'online')

// 颜色标签色条颜色（T16）：0/未标 → null（不渲染色条）。色档→hex 映射在前端 colorLabels。
const colorStrip = computed<string | null>(() => colorLabelHex(props.colorLabel))

// 回退 bg-elevated:与 app chrome(侧栏等)同画风,网格常态不吃画格暗灰(真机反馈:
// 整片暗灰占位显脏)。canvas 悬停卡内的回退占位另由 MediaGridCanvas 侧按 .is-fallback
// 覆盖为画格同源色,避免弹卡瞬间从画格色跳 elevated(「中间插一次主题色」)。
const placeholderBgColor = computed(() => props.placeholderColor ?? 'var(--color-bg-elevated)')

// 不栅格化的文本文档格式（pdf/svg/epub 有真实缩略图）。用 CSS「文本卡」（仿文本行 + 扩展名角标）
// 呈现，替代灰底占位（§3.4）。compact 尺寸下退回居中扩展名占位即可。
// 格式表与 Canvas 网格共享单源(mediaGrid.helpers.TEXT_CARD_FORMATS)。
// epub 补充降级(⑤):封面提取盖棺失败(status=2)也出文本卡,替代灰底占位。
const isTextCard = computed(
  () =>
    !compact.value &&
    (isTextCardFormat(props.mediaType, props.fileFormat) ||
      isTextCardFallback(props.mediaType, props.fileFormat, props.thumbStatus)),
)

// 类型角标(AUDIO/DOC/RAW):判定走 helpers 单源与 canvas 网格共享,防两路条件漂移。
const typeBadge = computed(() =>
  typeBadgeOf(props.mediaType, props.fileFormat, props.thumbStatus),
)
const showTypeBadge = computed(
  () => typeBadge.value != null && ui.showThumbInfo && ui.thumbInfoElements.includes('type'),
)
const typeBadgeLabel = computed(() => {
  if (typeBadge.value === 'audio') return 'AUDIO'
  if (typeBadge.value === 'raw') return 'RAW'
  return 'DOC'
})
const typeBadgeTitleKey = computed(() => {
  if (typeBadge.value === 'audio') return 'media.badgeAudioTitle'
  if (typeBadge.value === 'raw') return 'media.badgeRawTitle'
  return 'media.badgeDocTitle'
})

// RAW 且尚无缩略图(占位符专属分支用):判定走 helpers 单源,与角标判据一致。
const isRawNoThumbCell = computed(() =>
  isRawNoThumb(props.mediaType, props.fileFormat, props.thumbStatus),
)

async function toggleFav() {
  favAnimating.value = true
  setTimeout(() => {
    favAnimating.value = false
  }, 400)
  emit('favorite', props.id)
}

// 缩略图内 hover 快捷评分：把用户点选值上抛父层（MediaGrid 落 setRating + 乐观更新布局行）。
// 与收藏一致走 emit 而非直接调 store，保持「视图组件不内嵌 IPC」的一致性。
function onRate(value: number) {
  emit('rate', props.id, value)
}
</script>

<style scoped>
.media-thumb {
  /* position:relative 以便遵守 thumbStyle 的宽度/高度属性 */
  position: relative;
  overflow: hidden;
  border-radius: 2px;
  background: var(--color-bg-elevated);
  /* cursor 和 flex-shrink 存在于父组件 .media-card 上 */
  transition:
    border-radius 0.2s ease,
    box-shadow 0.2s ease;
}
.media-thumb:hover .media-thumb__fav,
.media-thumb:hover .media-thumb__checkbox,
.media-thumb:hover .media-thumb__drag-handle {
  opacity: 1;
}

/* 格内 1px 内描边(--color-thumb-outline):白底图贴亮主题底/暗图贴暗底时勾出图与底的边界,
   与 canvas 网格 strokeRect 同 token 同源(阴影方案在 canvas 每帧逐格模糊代价高,弃用)。
   零模糊 box-shadow 成本可忽略;z-index 6 盖过图(auto)与预览视频(5)、让位覆盖层(10);
   选中态内描边由宿主 .media-thumb--selected 提供,不冲突。inset 画在元素内缘,不占布局、不撑格缝。 */
.media-thumb::before {
  content: '';
  position: absolute;
  inset: 0;
  z-index: 6;
  box-shadow: inset 0 0 0 1px var(--color-thumb-outline);
  border-radius: inherit;
  pointer-events: none;
}

.media-thumb__placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.media-thumb__ext {
  font-family: var(--font-mono);
  font-size: 14px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.4);
  letter-spacing: 1px;
}

/* RAW 专属占位符(§ RAW 半成品体验修复线):在通用扩展名文本之上叠一枚「RAW」小标签,
   一眼区分「相机原片、待解码」与「文件损坏/通用无预览」——复用既有 --color-badge-doc-generic
   token(不引新色值),与 .badge-raw 同色系保持网格内一致观感。 */
.media-thumb__placeholder--raw {
  flex-direction: column;
  gap: 6px;
}
.media-thumb__raw-icon {
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  color: #fff;
  background: var(--color-badge-doc-generic);
  padding: 2px 7px;
  border-radius: 3px;
  letter-spacing: 0.06em;
}

/* ── 文本文档卡（§3.4）—— 纸张感卡片 + 仿文本行 + 扩展名角标 ── */
/* 硬编码色豁免说明(S5,设计 §6.2):本组件内叠在照片/视频/彩色徽章之上的
   #fff、黑系渐变与 drop-shadow 语义为「媒体上的永远白字黑纱」,属画布内容区,
   刻意不随主题——主题化会破坏照片观感中性红线。可主题化的(纸面/徽章底色)
   已全部收敛为 --color-doc-paper(-line) 与 --color-badge-doc-xxx token。
   (注意:注释内写 token 通配不能用「…paper* / …」——「*」贴着「/」会拼出「星斜杠」提前终止块注释) */
.media-thumb__textcard {
  position: absolute;
  inset: 0;
  background: var(--color-doc-paper);
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  padding: 14% 12%;
  /* 顶端细装订线，强化「文档」观感 */
  border-top: 3px solid rgba(0, 0, 0, 0.06);
}
.media-thumb__textcard-lines {
  display: flex;
  flex-direction: column;
  gap: 9%;
}
.media-thumb__textcard-lines span {
  display: block;
  height: 5px;
  border-radius: 2px;
  background: var(--color-doc-paper-line);
}
.media-thumb__textcard-lines span:nth-child(1) {
  width: 65%;
}
.media-thumb__textcard-lines span:nth-child(2) {
  width: 100%;
}
.media-thumb__textcard-lines span:nth-child(3) {
  width: 92%;
}
.media-thumb__textcard-lines span:nth-child(4) {
  width: 55%;
}
.media-thumb__textcard-ext {
  align-self: flex-start;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 700;
  color: #fff;
  background: var(--color-badge-doc-generic);
  padding: 2px 6px;
  border-radius: 3px;
  letter-spacing: 0.04em;
}
/* 按类型着色角标（Office 沿用其品牌色，便于一眼区分） */
.media-thumb__textcard.fmt-md .media-thumb__textcard-ext {
  background: var(--color-badge-doc-md);
}
.media-thumb__textcard.fmt-doc .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-docx .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-odt .media-thumb__textcard-ext {
  background: var(--color-badge-doc-word);
}
.media-thumb__textcard.fmt-xls .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-xlsx .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-ods .media-thumb__textcard-ext {
  background: var(--color-badge-doc-excel);
}
.media-thumb__textcard.fmt-ppt .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-pptx .media-thumb__textcard-ext,
.media-thumb__textcard.fmt-odp .media-thumb__textcard-ext {
  background: var(--color-badge-doc-ppt);
}

.media-thumb__img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

/* 悬停预览视频位于静态图之上、覆盖层（z-index:10）之下。 */
.media-thumb__video {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  z-index: 5;
  /* 首帧真正合成呈现（rVFC → .is-painted）前 opacity:0，让下层封面图/占位透出，
     移入不再闪黑/灰白；就绪后短暂渐显切到视频。不再用 background:#000（空窗期会盖成黑块）。 */
  opacity: 0;
  transition: opacity 0.18s ease;
}
.media-thumb__video.is-painted {
  opacity: 1;
}

/* 悬停 scrub 雪碧图 —— 铺满格子；通过 background-position 选帧（§3.3）。 */
.media-thumb__sprite {
  position: absolute;
  inset: 0;
  z-index: 5;
  /* 不用黑底：sprite 图加载前让下层封面图透出，与视频预览一致，避免移入闪黑。 */
  cursor: ew-resize;
}

/* ── 覆盖层 ─────────────────────────────────────────────────────────── */
.media-thumb__overlays {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 10;
}

.badge {
  position: absolute;
  border-radius: var(--radius-xs);
  font-size: var(--font-size-2xs);
  font-weight: 600;
  padding: 2px 5px;
  line-height: var(--leading-tight);
  letter-spacing: 0.03em;
  background: var(--color-badge-scrim);
  color: #fff;
}
.media-thumb__badges {
  display: flex;
  gap: var(--spacing-xs);
  margin-bottom: 2px;
  flex-wrap: wrap;
}
.badge-source {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-thumb {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-live {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-size {
  position: static;
  background: var(--color-badge-scrim);
}
/* 类别色只作为小面积语义点,文字仍统一压在 scrim 上。 */
.badge-live::before,
.badge-audio::before,
.badge-document::before {
  content: '';
  width: 5px;
  height: 5px;
  flex: 0 0 auto;
  border-radius: 50%;
}
.badge-live::before {
  background: var(--color-badge-mark-live);
}
.badge-audio {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-audio::before {
  background: var(--color-badge-mark-audio);
}
.badge-document {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-document::before {
  background: var(--color-badge-mark-document);
}
/* RAW 与 ORIG 一样是来源/处理状态文字,保持中性 scrim。 */
.badge-raw {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-similarity {
  position: static;
  background: var(--color-badge-scrim);
}
.badge-video {
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  background: var(--color-badge-scrim);
  color: #fff;
  /* 圆形碟底:与 canvas drawPlayIcon(半透明圆底+白三角)同形——此前圆角矩形药丸,
     canvas 格悬停弹卡瞬间圆钮变方钮(真机反馈)。44px = canvas 侧直径封顶值。 */
  width: 44px;
  height: 44px;
  padding: 0 0 0 3px; /* 三角视觉重心偏左,+3px 光学居中 */
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.badge-duration {
  bottom: 6px;
  right: 6px;
  background: var(--color-badge-scrim);
  color: #fff;
  font-family: var(--font-mono);
  font-size: var(--font-size-2xs);
}

.media-thumb__fav {
  position: absolute;
  bottom: 4px;
  right: 4px;
  background: transparent;
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  pointer-events: auto;
  transition:
    opacity var(--transition-fast),
    transform var(--transition-fast);
  padding: 4px;
  border: none;
  filter: drop-shadow(0 1px 3px rgba(0, 0, 0, 0.6));
}
.media-thumb__fav:hover {
  transform: scale(1.1);
}
.media-thumb__fav.fav-always-visible {
  opacity: 1;
}

/* ── 评分星级（左下，经典桌面相册式）─────────────────────────────────────── */
.media-thumb__rating-slot {
  position: absolute;
  bottom: 4px;
  left: 6px;
  /* 容器不挡事件；仅交互星条（--edit）显式开启 pointer-events，避免只读角标拦截下层 hover 预览。 */
  pointer-events: none;
  /* 琥珀星在亮图上易糊 —— 投影增强对比，与收藏红心同处理。 */
  filter: drop-shadow(0 1px 3px rgba(0, 0, 0, 0.7));
  z-index: 11;
}
.media-thumb__rating--edit {
  pointer-events: auto;
}

/* 左下角有可用态角标(missing/offline)时,评分星条上抬一档避让(角标占据左下角)。
   待删态的抬升由宿主 .media-card--pending-delete 规则负责(pending 是卡级状态,本组件不感知)。 */
.media-thumb--missing .media-thumb__rating-slot,
.media-thumb--offline .media-thumb__rating-slot {
  bottom: 24px;
}

/* 颜色标签色条（T16）：顶缘 4px 细条，盖过角标层但不挡交互。 */
.media-thumb__color-strip {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 4px;
  z-index: 11;
  pointer-events: none;
}

.media-thumb__checkbox {
  position: absolute;
  top: 4px;
  right: 4px;
  opacity: 0;
  pointer-events: auto;
  transition: opacity var(--transition-fast);
}

/* ── 拖拽手柄(需求3)：已选中格左上角抓取点 ─────────────────────────────
   选中即常显(半透明),悬停整卡加强到不透明(与右上 checkbox 的显现纪律对称)。
   z-index 12 高于 overlays(10)/avail(11),保证可抓取;pointer-events:auto 使
   e.target 落在手柄上,onCardPointerDown 据 closest 分流到拖到文件夹。 */
.media-thumb__drag-handle {
  position: absolute;
  top: 4px;
  left: 4px;
  z-index: 12;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-sm);
  background: var(--color-badge-scrim);
  color: #fff;
  cursor: grab;
  pointer-events: auto;
  opacity: 0.55;
  transition:
    opacity var(--transition-fast),
    background var(--transition-fast);
  filter: drop-shadow(0 1px 3px rgba(0, 0, 0, 0.5));
}
.media-thumb__drag-handle:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.72);
}
.media-thumb__drag-handle:active {
  cursor: grabbing;
}
.checkbox {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid rgba(255, 255, 255, 0.9);
  background: var(--color-badge-scrim);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  color: #fff;
  font-weight: 700;
  transition: background var(--transition-fast);
}
.checkbox.checked {
  background: var(--color-accent);
  border-color: var(--color-accent);
}

/* ── Selection visual states | 选择视觉状态 ─────────────────────── */

.media-thumb--selected {
  border-radius: var(--radius-lg);
  /* 选中态只做 2px 内描边,不缩小/压暗整张图片,避免内容跳动和卡片墙增重。 */
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

.media-thumb--drag-hover {
  border-radius: var(--radius-md);
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

.media-thumb--drag-hover::after {
  content: '';
  position: absolute;
  inset: 0;
  background: color-mix(in srgb, var(--color-bg-surface) 35%, transparent);
  pointer-events: none;
  z-index: 2;
  border-radius: inherit;
  transition: background-color 150ms ease;
}

/* 选择模式：始终显示 checkbox（不仅是 hover 时） */
.media-thumb--selection-mode .media-thumb__checkbox {
  opacity: 1;
}

/* 选择模式：也始终显示收藏按钮 */
.media-thumb--selection-mode .media-thumb__fav {
  opacity: 1;
}

.media-thumb__info-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  background: linear-gradient(
    to top,
    rgba(0, 0, 0, 0.85) 0%,
    rgba(0, 0, 0, 0.5) 70%,
    transparent 100%
  );
  color: #fff;
  padding: 24px 6px 6px 6px;
  font-size: 10px;
  font-family: var(--font-mono);
  display: flex;
  flex-direction: column;
  gap: 2px;
  pointer-events: none;
  opacity: 1;
  transition: opacity var(--transition-fast);
}

.info-line {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
}

/* ── 可用态：missing/offline 置灰 + 角标（缺失检测 Part2 §3.2）─────────────── */
/* 整格去色 + 降透明，一眼区分「不在场」；filter 作用于整个 thumb（含图/占位）。 */
.media-thumb--missing {
  filter: grayscale(1) brightness(0.85);
  opacity: 0.5;
}
.media-thumb--offline {
  filter: grayscale(0.7);
  opacity: 0.72;
}

.media-thumb__avail {
  position: absolute;
  bottom: var(--spacing-2xs); /* 左下角:让出左上给拖拽手柄(需求3) */
  left: var(--spacing-2xs);
  z-index: 11; /* 盖过 overlays，保证可见 */
  font-size: var(--font-size-2xs);
  font-weight: 700;
  line-height: 1;
  padding: var(--spacing-2xs) var(--spacing-xs);
  border-radius: var(--radius-xs);
  color: #fff;
  letter-spacing: 0.04em;
  pointer-events: auto; /* 允许 hover 出 title 提示 */
}
.media-thumb__avail.avail--missing {
  background: var(--color-error);
  color: var(--color-text-on-error);
} /* 红：文件没了 */
.media-thumb__avail.avail--offline {
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
} /* 灰：卷离线 */

/* ── Compact 模式极限优化 ─────────────────────────────────────────── */
/* 当单元尺寸 < 100px 时，禁用 transition 和额外合成层以极大减少 GPU 开销。
   contain: strict 告诉浏览器此元素内部变化不影响外部布局。 */
.media-thumb--compact {
  transition: none;
  contain: strict;
}
</style>
