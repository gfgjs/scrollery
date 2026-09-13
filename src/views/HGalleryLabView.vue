<template>
  <div class="hlab">
    <!-- 实验控制条:模式切换 + 逐模式参数 + 公共参数 + 摘要。即改即算(300ms 防抖)。 -->
    <div class="hlab__bar">
      <button
        class="hlab__back"
        :title="$t('hlab.back')"

        @click="router.push('/')"
      >
        <ArrowLeft :size="16" />
      </button>
      <span class="hlab__title">{{ $t('hlab.title') }}</span>

      <div class="hlab__modes">
        <button
          v-for="m in MODE_OPTIONS"
          :key="m.kind"
          class="hlab__mode-btn"
          :class="{ active: modeKind === m.kind }"
          @click="modeKind = m.kind"
        >
          {{ $t(m.labelKey) }}
        </button>
      </div>

      <!-- A:分屏 justified 参数 -->
      <template v-if="modeKind === 'paged'">
        <label class="hlab__param">
          {{ $t('hlab.pageFactor') }} {{ pageFactor.toFixed(2) }}
          <input v-model.number="pageFactor" type="range" min="1" max="2" step="0.05" />
        </label>
        <label class="hlab__param">
          {{ $t('hlab.targetRowHeight') }} {{ targetRowHeight }}px
          <input v-model.number="targetRowHeight" type="range" min="100" max="400" step="10" />
        </label>
      </template>

      <!-- B:等高泳道参数 -->
      <template v-else-if="modeKind === 'lanes'">
        <label class="hlab__param">
          {{ $t('hlab.laneCount') }} {{ laneCount }}
          <input v-model.number="laneCount" type="range" min="2" max="6" step="1" />
        </label>
        <label class="hlab__check">
          <input v-model="balance" type="checkbox" />
          {{ $t('hlab.balance') }}
        </label>
      </template>

      <!-- C:转置 justified 参数 -->
      <template v-else>
        <label class="hlab__param">
          {{ $t('hlab.targetColWidth') }} {{ targetColWidth }}px
          <input v-model.number="targetColWidth" type="range" min="120" max="600" step="10" />
        </label>
      </template>

      <label class="hlab__param">
        {{ $t('hlab.gap') }} {{ gapPx }}px
        <input v-model.number="gapPx" type="range" min="0" max="16" step="1" />
      </label>
      <label class="hlab__check">
        <input v-model="timeAsc" type="checkbox" />
        {{ $t('hlab.timeAsc') }}
      </label>

      <span class="hlab__stats">
        <template v-if="isComputing">{{ $t('hlab.computing') }}</template>
        <template v-else-if="summary">
          {{
            $t('hlab.stats', {
              items: summary.totalItems,
              blocks: summary.blockCount,
              width: Math.round(summary.totalWidth).toLocaleString(),
              ms: summary.computeMs,
            })
          }}
        </template>
      </span>
      <span class="hlab__hint">{{ $t('hlab.hint') }}</span>
    </div>

    <div v-if="hv.overCap.value" class="hlab__cap">{{ $t('hlab.overCap') }}</div>

    <!-- 横向滚动视口:竖滚轮已被 composable 转译为平滑横滚;tabindex 供键盘导航;
         滚动进行中挂 is-scrolling 抑制格内 hover(掉帧修复之一)。 -->
    <div
      ref="containerEl"
      class="hlab__viewport"
      :class="{ 'is-scrolling': hv.isScrolling.value }"
      tabindex="0"
      @scroll="hv.onScroll"
      @keydown="onKeydown"
    >
      <div class="hlab__canvas" :style="{ width: canvasWidth + 'px' }">
        <!-- 渲染层不感知布局模式:HItem 均为全局绝对坐标,直接 flatMap 定位(plan §2-2)。
             left/top 定位而非 translate3d——后者逐格提升合成层是横向掉帧主因之一。 -->
        <div
          v-for="it in visibleItems"
          :key="it.id"
          class="hlab__cell"
          :style="{
            left: it.x + 'px',
            top: it.y + 'px',
            width: it.w + 'px',
            height: it.h + 'px',
          }"
        >
          <!-- 无 :item:lab 独立后端缓存(HItem)不含 originalWidth/Height 等行项字段,悬停判「重」
               的静态分辨率轴在此失效——由 useHoverPreview.onPreviewMeta 的真实 metadata 运行时
               补判兜底(超 4K 起播即降级),勿在此伪造 LayoutRowItem。 -->
          <MediaThumb
            :id="it.id"
            :w="it.w"
            :h="it.h"
            :media-type="it.mediaType"
            :is-live-photo="it.isLivePhoto"
            :duration-ms="it.durationMs"
            :thumb-status="it.thumbStatus"
            :thumb-path="it.thumbPath"
            :thumbhash="it.thumbhash"
            :file-format="it.fileFormat"
            :file-size="it.fileSize"
            :cache-dir="cacheDir"
            @request-thumb="onRequestThumb"
            @cancel-thumb="onCancelThumb"
          />
        </div>
      </div>
      <div v-if="summary && summary.totalItems === 0" class="hlab__empty">
        {{ $t('hlab.empty') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// H-Lab 横向画廊实验室(docs/designs/2026-07-02-horizontal-gallery-lab.md)。
// 定位:多种横向布局候选的真人调研载体——仅布局 + 滚动,多选/收藏/详情等附加能力显式推迟。
// 与生产 MediaGrid 完全平行:独立路由、独立后端缓存、独立滚动 composable,零共享可变状态;
// 唯二复用 = MediaThumb(缩略图状态机)与 useRequestQueue(批量请求,槽位生命周期已测锁定)。
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowLeft } from '@lucide/vue'
import MediaThumb from '../components/media/MediaThumb.vue'
import { useHVirtualScroll } from '../composables/useHVirtualScroll'
import { useRequestQueue } from '../composables/useRequestQueue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { getThumbCacheDir } from '../utils/thumbCacheDir'
import { IPC } from '../constants/ipc'
import { DEFAULTS } from '../constants/defaults'
import { useUiStore } from '../stores/uiStore'
import type { HBlock, HItem, HLayoutMode, HLayoutModeKind, HLayoutSummary } from '../types/hgallery'

const router = useRouter()
const ui = useUiStore()
const containerEl = ref<HTMLElement | null>(null)

const MODE_OPTIONS: { kind: HLayoutModeKind; labelKey: string }[] = [
  { kind: 'paged', labelKey: 'hlab.modePaged' },
  { kind: 'lanes', labelKey: 'hlab.modeLanes' },
  { kind: 'columns', labelKey: 'hlab.modeColumns' },
]

// ── 实验参数(默认值=调研起点;lanes 为当前倾向方案故作默认)────────────────────
const modeKind = ref<HLayoutModeKind>('lanes')
const pageFactor = ref(1.2)
const targetRowHeight = ref(200)
const laneCount = ref(3)
const balance = ref(false)
const targetColWidth = ref(280)
const gapPx = ref<number>(DEFAULTS.GRID_GAP)
const timeAsc = ref(false)

const summary = ref<HLayoutSummary | null>(null)
const isComputing = ref(false)
const cacheDir = ref('')

function currentMode(): HLayoutMode {
  if (modeKind.value === 'paged') {
    return {
      mode: 'paged',
      pageFactor: pageFactor.value,
      targetRowHeight: targetRowHeight.value,
    }
  }
  if (modeKind.value === 'columns') {
    return { mode: 'columns', targetColWidth: targetColWidth.value }
  }
  return { mode: 'lanes', laneCount: laneCount.value, balance: balance.value }
}

async function fetchBlocksByX(leftX: number, rightX: number): Promise<HBlock[]> {
  if (!summary.value) return []
  try {
    return await invokeIpc<HBlock[]>(IPC.GET_H_BLOCKS_BY_X, {
      leftX,
      rightX,
      layoutVersion: summary.value.layoutVersion,
    })
  } catch (e) {
    // LayoutNotReady(版本换代竞态)→ 空集;下一次滚动/重算会带新版本重取。
    logger.warn('[HLab] get_h_blocks_by_x failed', { error: e })
    return []
  }
}

const hv = useHVirtualScroll({
  totalWidth: () => summary.value?.totalWidth ?? 0,
  fetchBlocksByX,
  containerRef: () => containerEl.value,
})

async function compute() {
  const el = containerEl.value
  if (!el || isComputing.value) return
  const vw = el.clientWidth
  const vh = el.clientHeight
  if (vw < 100 || vh < 100) return
  isComputing.value = true
  try {
    summary.value = await invokeIpc<HLayoutSummary>(IPC.COMPUTE_H_LAYOUT, {
      params: {
        directoryId: null,
        filters: null,
        viewportWidth: vw,
        viewportHeight: vh,
        gap: gapPx.value,
        sortOrder: timeAsc.value ? 'asc' : 'desc',
        mode: currentMode(),
      },
    })
    // 几何整体换代 → 回到起点重取(实验期不做锚点保持,plan §6)。
    hv.scrollToStart()
    await hv.updateVisible(true)
  } catch (e) {
    logger.error('[HLab] compute_h_layout FAILED', { error: e })
  } finally {
    isComputing.value = false
  }
}

// 参数变化 → 防抖重算;视口「高」变化亦然(高是横向布局的输入,而非取数参数)。
let recomputeTimer: ReturnType<typeof setTimeout> | null = null
function scheduleCompute() {
  if (recomputeTimer) clearTimeout(recomputeTimer)
  recomputeTimer = setTimeout(compute, ui.resizeDebounceMs)
}
watch(
  [modeKind, pageFactor, targetRowHeight, laneCount, balance, targetColWidth, gapPx, timeAsc],
  scheduleCompute,
)
watch(
  () => hv.containerHeight.value,
  (_h, oldH) => {
    // oldH=0 是挂载首测(onMounted 已显式 compute),仅真实 resize 才重算。
    if (oldH > 0) scheduleCompute()
  },
)

const visibleItems = computed<HItem[]>(() => hv.visibleBlocks.value.flatMap((b) => b.items))
const canvasWidth = computed(() => Math.max(summary.value?.totalWidth ?? 0, 0))

// ── 缩略图请求:批量队列 + 就地 patch(同 MediaGrid onRequestThumb 模式)。
// h 缓存有意不接生产回写,滚回后靠「已生成快路径」自愈(plan §2-4)。
const queue = useRequestQueue()

async function onRequestThumb(id: number) {
  try {
    const result = await queue.request(id)
    for (const block of hv.visibleBlocks.value) {
      const item = block.items.find((it) => it.id === id)
      if (item) {
        item.thumbStatus = result.thumbStatus
        item.thumbPath = result.thumbPath
        item.thumbhash = result.thumbhash
        break
      }
    }
  } catch {
    // 取消/失败 → 保留占位符
  }
}

function onCancelThumb(id: number) {
  queue.cancel(id)
}

function onKeydown(e: KeyboardEvent) {
  switch (e.key) {
    case 'ArrowLeft':
      e.preventDefault()
      hv.scrollByViewport(-0.2, false)
      break
    case 'ArrowRight':
      e.preventDefault()
      hv.scrollByViewport(0.2, false)
      break
    case 'PageUp':
      e.preventDefault()
      hv.scrollByViewport(-0.9)
      break
    case 'PageDown':
    case ' ':
      e.preventDefault()
      hv.scrollByViewport(0.9)
      break
    case 'Home':
      e.preventDefault()
      hv.scrollToStart()
      break
    case 'End':
      e.preventDefault()
      hv.scrollToEnd()
      break
  }
}

onMounted(async () => {
  try {
    cacheDir.value = await getThumbCacheDir()
  } catch (e) {
    logger.error('[HLab] GET_THUMB_CACHE_DIR failed', { error: e })
  }
  containerEl.value?.focus()
  await compute()
})

onBeforeUnmount(() => {
  if (recomputeTimer) clearTimeout(recomputeTimer)
})
</script>

<style scoped>
.hlab {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  /* 原 var(--color-bg-base, #111) 引用不存在的幽灵 token,一直走 fallback 的固定深色底
     (与 AudioPlayer/DocumentViewer 两处根视图衬底同族修法一致)(S7 修)。 */
  background: var(--color-bg-primary);
}

/* ── 控制条 ─────────────────────────────────────────────────────────── */
.hlab__bar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 8px 12px;
  border-bottom: 1px solid var(--color-border, rgba(255, 255, 255, 0.08));
  font-size: 12px;
  color: var(--color-text-secondary, #aaa);
  flex-shrink: 0;
}

.hlab__back {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: var(--radius-sm, 4px);
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.hlab__back:hover {
  background: var(--color-bg-elevated, rgba(255, 255, 255, 0.08));
}

.hlab__title {
  font-weight: 700;
  color: var(--color-text-primary, #eee);
}

.hlab__modes {
  display: flex;
  border: 1px solid var(--color-border, rgba(255, 255, 255, 0.12));
  border-radius: var(--radius-md, 6px);
  overflow: hidden;
}
.hlab__mode-btn {
  padding: 4px 10px;
  border: none;
  background: transparent;
  color: inherit;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.hlab__mode-btn.active {
  background: var(--color-accent, #4b8bf4);
  color: #fff;
}

.hlab__param {
  display: flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
}
.hlab__param input[type='range'] {
  width: 110px;
}

.hlab__check {
  display: flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  cursor: pointer;
}

.hlab__stats {
  font-family: var(--font-mono, monospace);
  font-size: 11px;
  margin-left: auto;
}

.hlab__hint {
  font-size: 11px;
  opacity: 0.55;
  white-space: nowrap;
}

/* 超出实验滚动上限告警(plan §2-3:未移植坐标压缩的已知限制) */
.hlab__cap {
  padding: 4px 12px;
  background: rgba(220, 53, 69, 0.18);
  color: #ff8a95;
  font-size: 12px;
  flex-shrink: 0;
}

/* ── 横向视口 ────────────────────────────────────────────────────────── */
.hlab__viewport {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-x: auto;
  overflow-y: hidden;
  outline: none;
  /* 横滚到边不外溢成页面手势/导航。 */
  overscroll-behavior-x: contain;
}

.hlab__canvas {
  position: relative;
  height: 100%;
}

/* 性能纪律(2026-07-02 掉帧修复):left/top 定位、无 transform/will-change——
   逐格 translate3d 会给每个可见格子(约 40-50 个)各提升一个合成层,滚动时层树
   开销远超生产网格(生产仅逐「行」提层,约 10-15 个)。改为绘入滚动内容层后,
   滚动全程由合成器搬运,零逐格层。contain: strict(格子有显式尺寸)隔离重排/重绘。 */
.hlab__cell {
  position: absolute;
  contain: strict;
}

/* 滚动进行中抑制格内 hover(样式重算/悬停视频预览),对齐生产网格 isScrolling 纪律。 */
.hlab__viewport.is-scrolling .hlab__cell {
  pointer-events: none;
}

/* 实验范围裁剪(plan §2-4):隐藏 MediaThumb 的交互 overlay(收藏/评分/勾选),
   仅保留缩略图本体与状态角标——「无附加能力」是范围决定,不 fork 组件。 */
.hlab__cell :deep(.media-thumb__fav),
.hlab__cell :deep(.media-thumb__rating-slot),
.hlab__cell :deep(.media-thumb__checkbox) {
  display: none !important;
}

.hlab__empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary, #888);
  font-size: 13px;
  pointer-events: none;
}
</style>
