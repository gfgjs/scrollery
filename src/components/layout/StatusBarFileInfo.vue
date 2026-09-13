<template>
  <!-- 内容页底栏文件信息(2026-07-17 用户需求):进入 /view /doc /audio 后替换画廊统计。
       结构三区:①交互三件(星级/收藏/颜色标签,用户拍板可交互,**永不折叠**)②展示段
       (文件名/尺寸/时长/大小/格式,Priority+ 从尾折)③⋯ 弹层(被折段的标签+值)。
       同步契约:改动不本地回写,统一走 mediaStore.itemPatchSignal → viewerStore.applyFieldPatch
       (查看器信息面板原位改 detail 不重发 snapshot patch,信号桥是唯一全 surface 可靠源,见 findings)。 -->
  <div class="sfi">
    <template v-if="editable">
      <StarRating :model-value="fileInfo!.rating" :size="11" @change="onRating" />
      <button
        type="button"
        class="sfi__fav"
        :class="{ 'is-on': fileInfo!.isFavorited }"

        :title="$t(fileInfo!.isFavorited ? 'selection.unfavorite' : 'selection.favorite')"
        @click="onToggleFav"
      >
        <Heart :size="12" :fill="fileInfo!.isFavorited ? 'currentColor' : 'none'" />
      </button>
      <!-- 颜色标签(2026-07-18):默认全展开在底栏(原为单色块点开 UiPopover 选色)。点色即设、点当前
           色清零;另补一个显式「清除颜色」钮(仅已标色时可点)——「点当前色清零」是隐性操作,易被漏发现。 -->
      <ColorLabelPicker
        class="sfi__colors"
        :model-value="fileInfo!.colorLabel ?? 0"
        :size="11"
        @change="onColor"
      />
      <button
        type="button"
        class="sfi__clear"
        :disabled="(fileInfo!.colorLabel ?? 0) === 0"

        :title="$t('selection.clearColor')"
        @click="onColor(0)"
      >
        <Ban :size="12" />
      </button>
      <span class="sfi__divider"></span>
    </template>

    <!-- 展示段容器:useToolbarOverflow 测量对象(data-toolbar-item)。首测前(isMeasuring)全渲染,
         此后 idx ≥ visibleCount 的段折进 ⋯。无折叠过渡(28px 信息文本,瞬时切换即可,不订 0.28s 动画契约)。 -->
    <div ref="segsEl" class="sfi__segs">
      <span
        v-for="(s, i) in segments"
        :key="s.key"
        data-toolbar-item
        class="sfi__seg"
        :class="{
          'sfi__seg--folded': !isMeasuring && i >= visibleCount,
          'sfi__seg--filename': s.key === 'fileName',
        }"
        :title="s.key === 'fileName' ? s.text : undefined"
        >{{ s.text }}</span
      >
    </div>

    <button
      v-show="hasOverflow"
      ref="moreBtnEl"
      type="button"
      class="sfi__more"

      :title="$t('statusbar.moreInfo')"
      @click="moreOpen = !moreOpen"
    >
      <MoreHorizontal :size="14" />
    </button>
  </div>

  <!-- ⋯ 弹层:仅被折叠的段,带 i18n 标签行。 -->
  <UiPopover v-model:open="moreOpen" :anchor="moreBtnEl" placement="top">
    <div class="sfi__pop sfi__pop--rows">
      <div v-for="s in foldedSegments" :key="s.key" class="sfi__pop-row">
        <span class="sfi__pop-label">{{ $t(s.labelKey) }}</span>
        <span class="sfi__pop-value">{{ s.text }}</span>
      </div>
    </div>
  </UiPopover>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Heart, MoreHorizontal, Ban } from '@lucide/vue'
import StarRating from '../common/StarRating.vue'
import ColorLabelPicker from '../common/ColorLabelPicker.vue'
import UiPopover from '../ui/UiPopover.vue'
import { useViewerStore } from '../../stores/viewerStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useToolbarOverflow } from '../../composables/useToolbarOverflow'
import { buildInfoSegments } from './statusBarFileInfo.helpers'

const viewer = useViewerStore()
const media = useMediaStore()
const { locale } = useI18n()

const fileInfo = computed(() => viewer.activeViewer?.fileInfo ?? null)
const itemId = computed(() => viewer.activeViewer?.id ?? null)
// 交互三件仅对库内资产开放:外部文件(id=null)与标量未就绪(音频补齐前)只出展示段。
const editable = computed(() => itemId.value !== null && fileInfo.value !== null)

const segments = computed(() =>
  buildInfoSegments(
    viewer.activeViewer?.title ?? '',
    viewer.activeViewer?.fileFormat ?? '',
    fileInfo.value,
  ),
)

// ── 交互三件动作:只落库,不本地回写——itemPatchSignal 桥(下方 watch)统一回灌 fileInfo,
//    与查看器面板/网格 hover 评分同源同步,避免双写漂移。 ──────────────────────────
async function onRating(next: number) {
  if (itemId.value !== null) await media.setRating(itemId.value, next)
}
async function onToggleFav() {
  if (itemId.value !== null) await media.toggleFavorite(itemId.value)
}
async function onColor(v: number) {
  if (itemId.value !== null) await media.setColorLabel(itemId.value, v)
}

// 信号桥:任一 surface 的单项标量改动 → 回写 viewerStore.fileInfo(id 不匹配在 store 侧丢弃)。
watch(
  () => media.itemPatchSignal,
  (p) => {
    if (p) viewer.applyFieldPatch(p.id, p.field, p.value)
  },
)

// ── Priority+ 折叠(useToolbarOverflow,与 AppToolbar/SelectionActions 同契约)──
const segsEl = ref<HTMLElement | null>(null)
const moreBtnEl = ref<HTMLElement | null>(null)
const moreOpen = ref(false)
// 内容变化(翻页换文件名/locale 切换)→ 重测;⋯ 弹层开启期暂缓(防锚点漂移致弹层横跳)。
const remeasureKey = computed(
  () => segments.value.map((s) => s.text).join('|') + locale.value,
)
// 操作区居中(2026-07-18):.sfi 改 justify-content:center 后 .sfi__segs 是**内容宽**(flex:0 1 auto),
// 其 clientWidth 不再等于可用宽(RO 会自锁,见 useToolbarOverflow 文档)。故走内容宽路径:提供可用宽
// 探针——临时给 segs flex-grow:1 撑满读 clientWidth 后立即还原(同步、不经 paint),得真可用宽。
function measureSegsAvailable(): number {
  const el = segsEl.value
  if (!el) return 0
  const prev = el.style.flexGrow
  el.style.flexGrow = '1'
  const w = el.clientWidth
  el.style.flexGrow = prev
  return w
}
const { visibleCount, hasOverflow, isMeasuring } = useToolbarOverflow({
  containerRef: segsEl,
  overflowButtonWidth: 26,
  remeasureKey,
  deferWhile: moreOpen,
  containerFillsWidth: false,
  measureAvailable: measureSegsAvailable,
})
const foldedSegments = computed(() =>
  isMeasuring.value ? [] : segments.value.slice(visibleCount.value),
)
</script>

<style scoped>
.sfi {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  /* 操作区居中(2026-07-18):整块(交互三件 + 展示段)在底栏水平居中,不再靠左。空间不足时展示段
     (flex:0 1 auto)先收缩/折叠,交互件保尺寸。 */
  justify-content: center;
}
/* 星级控件 18px 默认为详情页设计;28px 栏内压紧行高。 */
.sfi :deep(.star-rating) {
  gap: 1px;
}
.sfi__fav {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: color var(--transition-fast);
}
.sfi__fav:hover {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
}
.sfi__fav.is-on {
  color: var(--color-error);
}
/* 内联颜色标签选择器:全展开在底栏,不收缩(展示段先收缩)。 */
.sfi__colors {
  flex-shrink: 0;
}
/* 清除颜色钮:仅已标色时可点(disabled 时淡出、不可交互)。 */
.sfi__clear {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  flex-shrink: 0;
  transition: color var(--transition-fast);
}
.sfi__clear:hover:not(:disabled) {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
}
.sfi__clear:disabled {
  opacity: 0.35;
  cursor: default;
}
.sfi__divider {
  width: 1px;
  height: 12px;
  background: var(--color-divider);
  flex-shrink: 0;
}
.sfi__segs {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 0;
  /* 内容宽(非 flex:1):使整块可居中。可用宽由 measureSegsAvailable 探针临时撑满测量(见 script)。 */
  flex: 0 1 auto;
  overflow: hidden;
}
.sfi__seg {
  white-space: nowrap;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}
/* 文件名:唯一可弹性收缩的段,超长省略;offsetWidth 测量取的即钳后宽,切分口径一致。 */
.sfi__seg--filename {
  color: var(--color-text-primary);
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  flex-shrink: 1;
}
.sfi__seg--folded {
  display: none;
}
/* 量尺帧契约(useToolbarOverflow.measure):is-measuring 期强制全部段展开到自然宽以供读数,
   同步段内加类读宽去类、不经 paint,不会闪。 */
.sfi__segs.is-measuring .sfi__seg {
  display: inline !important;
}
.sfi__more {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}
.sfi__more:hover {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
}
/* 弹层表面(UiPopover 契约:视觉由消费方 slot 自带;token 对齐 SelectionActions 溢出菜单)。 */
.sfi__pop {
  padding: var(--spacing-sm);
}
.sfi__pop--rows {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 180px;
  max-width: 360px;
}
.sfi__pop-row {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  font-size: var(--font-size-xs);
}
.sfi__pop-label {
  color: var(--color-text-tertiary);
  flex-shrink: 0;
}
.sfi__pop-value {
  color: var(--color-text-primary);
  overflow-wrap: anywhere;
}
</style>
