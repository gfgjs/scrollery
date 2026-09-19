<template>
  <!-- 真·时间 scrubber:月刻度/年份按「逻辑 y 比例」定位(与滚动条同源同步)——文件多的月占更多轨道
       空间,密度由文件数决定;密度热力条长度 = 该月项数占最热月比例;年份标签防挤叠;整轨点击/拖拽跳转。 -->
  <div
    ref="trackRef"
    class="tl-scrubber"
    :class="{ 'tl-scrubber--dragging': dragging }"

    tabindex="0"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @pointerleave="onTrackLeave"
    @keydown="onKeydown"
    @focus="onFocus"
    @blur="onBlur"
  >
    <!-- 月密度模式（date 分组才有 monthBuckets） -->
    <template v-if="hasMonths">
      <div
        v-for="(b, i) in monthBuckets"
        :key="b.groupId"
        class="tl-month"
        :class="{ 'tl-month--active': i === activeIndex, 'tl-month--year-start': showYear(i) }"
        :style="{ top: `${(b.y / trackTotal) * 100}%` }"
        @click.stop="emit('jump', b.y)"
        @pointerenter="hoverIndex = i"
      >
        <!-- 密度热力条：长度 = 该月项数占最热月的比例；右对齐，越长越热。 -->
        <span class="tl-month__bar" :style="{ width: `${barWidth(b.count)}%` }"></span>
        <!-- 年份标记：仅每年首月（最新→最旧排列，故年份变化处）显示，浮在轨道左侧。 -->
        <span v-if="showYear(i)" class="tl-month__year">{{ b.year }}</span>
      </div>
    </template>

    <!-- 回退模式：folder/none 分组无 monthBuckets → 沿用「按逻辑高度均布的分隔符圆点」旧行为，
         保证非 date 视图不回归（仍可点分隔符跳转）。 -->
    <template v-else>
      <div
        v-for="sep in displaySeparators"
        :key="sep.y"
        class="tl-sep-node"
        :style="{ top: `${(sep.y / Math.max(1, totalHeight)) * 100}%` }"
        :title="sep.label"
        @click.stop="emit('jump', sep.y)"
      ></div>
    </template>

    <!-- 当前滚动位置指示线(与 canvas 版对齐):横贯全轨、accent 半透明,标示当前所在逻辑 y。 -->
    <div v-if="showIndicator" class="tl-indicator" :style="{ top: `${indicatorPct}%` }"></div>

    <!-- 半透明视窗(VSCode minimap 式):矩形 = 当前可视区,按住拖动 = 连续即时滚动(不吸附)。
         pointerdown.stop 防触发轨道单击跳转;pointer capture 令拖出边界仍跟手。 -->
    <div
      v-if="viewportGeom"
      class="tl-viewport"
      :style="{ top: `${viewportGeom.top}px`, height: `${viewportGeom.height}px` }"
      @pointerdown.stop="onViewportPointerdown"
      @pointermove="onViewportPointermove"
      @pointerup="onViewportPointerup"
      @pointercancel="onViewportPointerup"
    ></div>

    <!-- hover/拖拽浮层：显示当前指向的「年-月 · 张数」，浮在轨道左侧跟随光标月。 -->
    <div
      v-if="hasMonths && hoverIndex !== null"
      class="tl-flyout"
      :style="{ top: `${(monthBuckets[hoverIndex].y / trackTotal) * 100}%`, marginRight: flyoutShift }"
    >
      {{ monthBuckets[hoverIndex].year }}-{{ String(monthBuckets[hoverIndex].month).padStart(2, '0') }}
      <span class="tl-flyout__count">· {{ monthBuckets[hoverIndex].count }}</span>
    </div>

    <!-- folder/none 模式 hover/拖拽浮层：显示最近分组（文件夹）名，浮在轨道左侧（T0，§9.4）。 -->
    <div
      v-if="!hasMonths && sepHoverIndex !== null"
      class="tl-flyout"
      :style="{ top: `${(separators[sepHoverIndex].y / Math.max(1, totalHeight)) * 100}%`, marginRight: flyoutShift }"
    >
      {{ separators[sepHoverIndex].label }}
    </div>

    <!-- 放大镜(DOM 版,方案乙):光标最近 K 项富标签列表(label + count + 密度底条),中心项高亮。
         pointer-events:none 磁化预览。按 index 键复用固定 K 行,快速飞掠只 patch 文本不建删节点。 -->
    <div v-show="loupeVisible" class="tl-loupe" :style="{ top: `${loupeTop}px` }">
      <div
        v-for="(it, idx) in loupeItems"
        :key="idx"
        class="tl-loupe-row"
        :class="{ 'is-center': it.isCenter }"
      >
        <span class="tl-loupe-row__fill" :style="{ transform: `scaleX(${it.bar})` }"></span>
        <span class="tl-loupe-row__label" :title="it.label">{{ it.label }}</span>
        <span class="tl-loupe-row__count">{{ it.count }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// 独立时间轴 scrubber 子组件（Part5 §3.3 要求从 MediaGrid 巨组件抽离）。
// 纯展示 + 交互：消费父层传入的 monthBuckets/separators，跳转意图经 emit('jump', y) 上抛，
// 不内嵌 store/IPC——与 MediaThumb 评分一致的「视图组件不内嵌副作用」约定。
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import type { MonthBucket } from '../../types/layout'
import {
  maxBucketCount,
  densityBarWidth,
  findActiveMonthIndex,
  nearestSeparatorIndex,
  nearestSeparatorWindow,
  downsampleSeparators,
  visibleYearLabelSet,
  stepScrubberIndex,
} from './timelineScrubber.helpers'
// 复用自研滚动条的拇指几何(已单测锁 round-trip):时间轴半透明视窗 = 同一套「可视区矩形」数学。
import { thumbGeometry, thumbTopToLogicalY } from './mediaScrollbar.helpers'

const props = withDefaults(
  defineProps<{
    /** 月密度桶（date 分组才非空）：年/月/张数/逻辑 y/groupId。 */
    monthBuckets: MonthBucket[]
    /** 分隔符：date 分组=每日一项(label=日期串,count=该日项数),folder 分组=每文件夹一项(count=文件数)。
     *  folder/none 模式主轴用其均布圆点;放大镜两模式都用它取最近 K 项显示 label+count。 */
    separators: { label: string; y: number; groupId?: string; count: number }[]
    /** 布局总逻辑高度（回退模式按比例定位用）。 */
    totalHeight: number
    /** 当前逻辑滚动位置（高亮所在月）。 */
    currentY?: number
    /** 拖动视窗最小高(px):与滚动条 thumb 共享同一设置,二者同高(默认 48)。 */
    minThumb?: number
  }>(),
  { currentY: 0, minThumb: 48 },
)

const emit = defineEmits<{
  /** 跳转到某逻辑 y。第二参 smooth:指针拖拽=false 即时落点(跟手),键盘/单击省略=平滑。 */
  jump: [y: number, smooth?: boolean]
  /** 轴上拖拽(轨道 scrub / 视窗拖动)起止:宿主中转给 MediaScrollbar 展开标尺线。 */
  scrubbing: [on: boolean]
}>()

const trackRef = ref<HTMLElement | null>(null)
const dragging = ref(false)
const hoverIndex = ref<number | null>(null)
// folder/none 模式：光标/拖拽最近的分组索引（T0，§9.4）——吸附点轨 + 驱动分组名浮层。
const sepHoverIndex = ref<number | null>(null)

// ── 放大镜(DOM 版,方案乙:光标最近 K 项富标签列表)────────────────────────────
// 不再「显示固定 y 窗口内所有节点」(密集区节点爆炸且不可读),改「取光标最近 K 个分隔符,各显
// label + count」——节点数固定 = K×少数几个,与密度无关,始终可读。date 分组分隔符=每日(label=
// 日期串、count=该日项数),folder 分组=每文件夹(count=文件数)——两模式统一走 separators。
// pointer-events:none 磁化预览不抢指针;诚实边界:放大镜只帮看清,点击仍走轨道连续映射。
const LOUPE_K = 9 // 显示最近项数(奇数,中心项≈光标)
const loupeVisible = ref(false)
const loupeTop = ref(0) // 浮窗垂直中心(轨道内 px)
const loupeCenterY = ref(0) // 浮窗中心对应逻辑 y
const loupeItems = computed(() => {
  if (!loupeVisible.value || props.separators.length === 0) return []
  const w = nearestSeparatorWindow(
    props.separators,
    loupeCenterY.value / trackTotal.value,
    props.totalHeight,
    LOUPE_K,
  )
  // 窗口内最大 count 作密度条归一化分母(仅归一窗口内相对量,聚焦局部对比)。
  let maxC = 1
  for (let i = w.start; i < w.end; i++) maxC = Math.max(maxC, props.separators[i].count)
  const out: { label: string; count: number; bar: number; isCenter: boolean }[] = []
  for (let i = w.start; i < w.end; i++) {
    const s = props.separators[i]
    out.push({ label: s.label, count: s.count, bar: s.count / maxC, isCenter: i === w.center })
  }
  return out
})
// 放大镜定位(已知 frac + 轨道内像素,不自读 rect——由调用方每帧一次统一读)。
function applyLoupe(frac: number, topPx: number) {
  loupeTop.value = topPx
  loupeCenterY.value = frac * props.totalHeight
  loupeVisible.value = true
}
// 放大镜可见时把浮层推到放大镜左侧,避免被 156px 宽的放大镜遮住文字(用户反馈)。
// 178 = 14(放大镜 margin-right)+ 156(放大镜宽)+ 8(间隙);不可见时回落原 8px 贴轨。
const flyoutShift = computed(() => (loupeVisible.value ? '178px' : '8px'))

const monthCount = computed(() => props.monthBuckets.length)
const hasMonths = computed(() => monthCount.value > 0)

// S1（§9.9）：folder/none 模式 DOM 降采样——全量分隔符桶合并到固定槽数，渲染节点从 O(分组) 降到
// O(条像素），2042 → <=SEP_SLOT_COUNT。全量 props.separators 仍供点轨吸附全精度，仅「显示」降采样。
// SEP_SLOT_COUNT 取 300（轨道约 600-900px，300 点已密到视觉连续）；分组数 <=300 的小库零改动零回归。
const SEP_SLOT_COUNT = 300
const displaySeparators = computed(() =>
  downsampleSeparators(props.separators, props.totalHeight, SEP_SLOT_COUNT),
)

// 纯映射逻辑抽到 timelineScrubber.helpers（带单测），此处仅做响应式包裹。
const maxCount = computed(() => maxBucketCount(props.monthBuckets))
const barWidth = (count: number): number => densityBarWidth(count, maxCount.value)
const activeIndex = computed(() => findActiveMonthIndex(props.monthBuckets, props.currentY))
const trackTotal = computed(() => Math.max(1, props.totalHeight))
// 轨道像素高(年份标签防挤叠需换算像素);ResizeObserver 跟随高度变化。
const trackH = ref(0)
// 当前滚动位置指示线(与 canvas 版对齐):currentY/可滚动行程(totalHeight−trackH)映射轨道
// 比例,顶/底恰贴两端(与 tl-viewport 矩形同分母)——分母取 totalHeight 时贴底会留一屏比例
// 的空档(用户回报);不足一屏不显。
const indicatorPct = computed(() => {
  const frac = props.currentY / Math.max(1, props.totalHeight - trackH.value)
  return Math.min(1, Math.max(0, frac)) * 100
})
const showIndicator = computed(() => props.totalHeight > trackH.value)

// ── 半透明视窗(VSCode minimap 式拖动把手)────────────────────────────────────
// 复用 thumbGeometry:矩形 = 当前可视区在总高中的比例;拖它 = 连续即时滚动(emit smooth=false),
// 不吸附分隔符、不跳(指针以前的吸附路径留给轨道单击)。内容不足一屏(geom=null)时不显。
// 视窗最小高由 minThumb prop 决定,与滚动条 thumb 同值 → 二者同高;两端恒对齐(top=frac×(trackH−h),
// frac=0/1 处与 h 无关,恒贴顶/底)。
const viewportGeom = computed(() =>
  thumbGeometry(props.currentY, props.totalHeight, trackH.value, props.minThumb),
)
let vpDragging = false
let vpGrabOffset = 0 // 抓点在矩形内的偏移,拖动全程保持,矩形不跳到指针下
function onViewportPointerdown(e: PointerEvent) {
  if (e.button !== 0) return
  const g = viewportGeom.value
  const el = trackRef.value
  if (!g || !el) return
  vpDragging = true
  emit('scrubbing', true)
  vpGrabOffset = e.clientY - el.getBoundingClientRect().top - g.top
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  e.preventDefault()
}
function onViewportPointermove(e: PointerEvent) {
  if (!vpDragging) return
  const g = viewportGeom.value
  const el = trackRef.value
  if (!g || !el) return
  const thumbTop = e.clientY - el.getBoundingClientRect().top - vpGrabOffset
  emit('jump', thumbTopToLogicalY(thumbTop, props.totalHeight, trackH.value, g.height), false)
}
function onViewportPointerup(e: PointerEvent) {
  if (vpDragging) emit('scrubbing', false)
  vpDragging = false
  ;(e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId)
}
let trackRO: ResizeObserver | null = null
onMounted(() => {
  const el = trackRef.value
  if (!el) return
  trackH.value = el.clientHeight
  if (typeof ResizeObserver !== 'undefined') {
    trackRO = new ResizeObserver(() => {
      if (trackRef.value) trackH.value = trackRef.value.clientHeight
    })
    trackRO.observe(el)
  }
})
onBeforeUnmount(() => {
  trackRO?.disconnect()
  trackRO = null
  cancelPendingMove() // 清 pointermove 节流的悬空 rAF
  // 拖拽中被卸载(切轴形态/关轴)不让宿主的 scrub 态卡在 true。
  if (dragging.value || vpDragging) emit('scrubbing', false)
})
const yearLabelSet = computed(() =>
  visibleYearLabelSet(props.monthBuckets, props.totalHeight, trackH.value),
)
const showYear = (i: number): boolean => yearLabelSet.value.has(i)

// ── 点击/拖拽跳转 + hover（均按 frac，逻辑与旧 jumpToPointer 一致）──────────────
// 轨道单击/拖拽跳转(平滑动画,指针「以前的样子」):date 模式按比例(frac*totalHeight,与滚动条同源)
// 跳转;folder/none 模式吸附最近真实分组边界(便于定位分组)。连续无吸附的跟手拖动改由半透明视窗
// (见下)承担,不再走此吸附路径。
function applyJump(frac: number) {
  if (hasMonths.value) {
    emit('jump', frac * props.totalHeight)
    hoverIndex.value = findActiveMonthIndex(props.monthBuckets, frac * props.totalHeight)
  } else {
    // T0（§9.4）：folder/none 模式吸附到最近真实分组边界；空分隔符时回退按比例。
    const idx = nearestSeparatorIndex(props.separators, frac, props.totalHeight)
    if (idx >= 0) {
      sepHoverIndex.value = idx
      emit('jump', props.separators[idx].y)
    } else {
      emit('jump', frac * props.totalHeight)
    }
  }
}
function applyHover(frac: number) {
  if (hasMonths.value) {
    hoverIndex.value = findActiveMonthIndex(props.monthBuckets, frac * props.totalHeight)
  } else {
    const idx = nearestSeparatorIndex(props.separators, frac, props.totalHeight)
    sepHoverIndex.value = idx >= 0 ? idx : null
  }
}
function fracOf(clientY: number, rect: DOMRect): number {
  return Math.min(1, Math.max(0, (clientY - rect.top) / Math.max(1, rect.height)))
}

// pointermove rAF 节流:pointermove 频率 = 输入设备采样率(游戏鼠标 500-1000Hz,远超屏幕刷新)。
// 屏幕每帧只绘一次,处理超过「每帧一个」的事件是纯浪费(中间位置在下次绘制前就被覆盖)。故只暂存
// 最新事件、每帧处理一个,rect 每帧只读一次。配合放大镜内节点 transform 定位(避布局)+ index 键
// 复用,单帧成本已很低,节流延迟感不明显。
let moveRaf: number | null = null
let pendingMove: PointerEvent | null = null
function processMove(e: PointerEvent) {
  const el = trackRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const frac = fracOf(e.clientY, rect)
  if (dragging.value) applyJump(frac)
  else applyHover(frac)
  applyLoupe(frac, e.clientY - rect.top)
}
function cancelPendingMove() {
  if (moveRaf !== null) {
    cancelAnimationFrame(moveRaf)
    moveRaf = null
  }
  pendingMove = null
}

function onPointerDown(e: PointerEvent) {
  dragging.value = true
  emit('scrubbing', true)
  // 捕获指针：拖出轨道边界仍持续收到 move/up，scrub 不中断。
  trackRef.value?.setPointerCapture(e.pointerId)
  // 按下即时响应(不进节流队列),避免首帧延迟。
  const el = trackRef.value
  if (el) {
    const rect = el.getBoundingClientRect()
    const frac = fracOf(e.clientY, rect)
    applyJump(frac)
    applyLoupe(frac, e.clientY - rect.top)
  }
}

function onPointerMove(e: PointerEvent) {
  // 只暂存最新事件,rAF 里统一处理(合并同帧内的高频事件)。原生 PointerEvent 不被池化,跨帧读取安全。
  pendingMove = e
  if (moveRaf !== null) return
  moveRaf = requestAnimationFrame(() => {
    moveRaf = null
    const ev = pendingMove
    pendingMove = null
    if (ev) processMove(ev)
  })
}

function onPointerUp(e: PointerEvent) {
  if (dragging.value) {
    dragging.value = false
    emit('scrubbing', false)
    trackRef.value?.releasePointerCapture(e.pointerId)
  }
}

function onTrackLeave() {
  // 非拖拽时离开轨道才清 hover 浮层；拖拽中即便移出也保留（配合指针捕获持续 scrub）。
  if (!dragging.value) {
    cancelPendingMove() // 否则已排队的帧会在离开后又把放大镜重新点亮
    hoverIndex.value = null
    sepHoverIndex.value = null
    loupeVisible.value = false
  }
}

// ── 键盘导航（§9 二期）─────────────────────────────────────────────────────
// scrubber 轨道本是滑块：加方向键/Home/End/PageUp/Down 逐分隔跳转,补齐纯指针的键盘缺口。
// date 模式步进月桶,folder/none 模式步进真实分隔符(全精度,非降采样的显示节点)。
const stepCount = computed(() => (hasMonths.value ? props.monthBuckets.length : props.separators.length))

// 当前滚动位置对应的项索引(date=活动月;folder=最近分隔符),供聚焦初值。
const currentIndex = computed(() => {
  if (hasMonths.value) return Math.max(0, activeIndex.value)
  if (props.separators.length === 0) return -1
  return nearestSeparatorIndex(props.separators, props.currentY / trackTotal.value, props.totalHeight)
})

// 聚焦后键盘位置的独立真值(避免依赖 emit→父滚动→currentY 回传的延迟);失焦清空回落 currentIndex。
const keyboardIndex = ref<number | null>(null)
const activeKbIndex = computed(() => keyboardIndex.value ?? currentIndex.value)

// 跳到第 idx 项:emit 逻辑 y + 同步 hover 浮层显示落点(与指针 scrub 一致的反馈)。
function stepTo(idx: number) {
  keyboardIndex.value = idx
  if (hasMonths.value) {
    hoverIndex.value = idx
    emit('jump', props.monthBuckets[idx].y)
  } else {
    sepHoverIndex.value = idx
    emit('jump', props.separators[idx].y)
  }
}

function onKeydown(e: KeyboardEvent) {
  const next = stepScrubberIndex(activeKbIndex.value, e.key, stepCount.value)
  if (next === null) return // 非导航键 → 不拦截
  e.preventDefault()
  stepTo(next)
}

function onFocus() {
  if (keyboardIndex.value === null) keyboardIndex.value = Math.max(0, currentIndex.value)
}
function onBlur() {
  keyboardIndex.value = null
  if (!dragging.value) {
    hoverIndex.value = null
    sepHoverIndex.value = null
  }
}
</script>

<style scoped>
.tl-scrubber {
  position: absolute;
  left: 0;
  right: 0;
  /* 满高(非内缩):与 MediaScrollbar(top:0/bottom:0)同坐标系,视窗才能与滚动条 thumb 两端对齐。 */
  top: 0;
  bottom: 0;
  /* 整轨可点/可拖（旧 mini-timeline 是 pointer-events:none，仅圆点可点；scrubber 需整轨拖拽）。 */
  cursor: pointer;
  touch-action: none; /* 防止移动端拖拽被浏览器手势抢走 */
}
/* 键盘聚焦轮廓：仅键盘导航时显示，指针点击不打扰视觉。 */
.tl-scrubber:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
  border-radius: var(--radius-sm);
}

/* ── 月刻度(按逻辑 y 比例定位)─────────────────────────────── */
.tl-month {
  position: absolute;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
}
.tl-month__bar {
  height: 3px;
  border-radius: 2px;
  background: var(--color-border);
  transition:
    background var(--transition-fast),
    height var(--transition-fast);
}
.tl-month--active .tl-month__bar {
  background: var(--color-accent);
  height: 5px;
}
.tl-scrubber:hover .tl-month:hover .tl-month__bar {
  background: var(--color-text-secondary);
}
/* 年首月：热力条上方叠一根更亮的年分隔线，并显年份。 */
.tl-month--year-start .tl-month__bar {
  background: var(--color-text-secondary);
}
.tl-month__year {
  position: absolute;
  right: 100%;
  top: 50%;
  transform: translateY(-50%);
  margin-right: 4px;
  font-size: 9px;
  line-height: 1;
  /* 年份标签浮在画廊底上:取画廊底派生的辅助文字色。 */
  color: var(--color-canvas-text-secondary);
  white-space: nowrap;
  pointer-events: none;
  opacity: 0.9;
  /* 标签左浮进画廊区、可能压在照片上 → 用背景色描边做光晕,保证任意底色下可读。 */
  text-shadow:
    0 0 3px var(--color-bg-primary),
    0 0 2px var(--color-bg-primary),
    0 0 1px var(--color-bg-primary);
}

/* ── 回退：分隔符圆点（旧行为）────────────────────────────────────── */
.tl-sep-node {
  position: absolute;
  left: 50%;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--color-border);
  transform: translate(-50%, -50%);
  cursor: pointer;
}
.tl-sep-node:hover {
  background: var(--color-accent);
}

/* ── 当前位置指示线（与 canvas 版对齐）──────────────────────────────────
   translateY(-50%) 让 1px 边居中于 top 百分比点;z-index 压在浮层(2)/放大镜(4)之下、刻度之上。 */
.tl-indicator {
  position: absolute;
  left: 0;
  right: 0;
  height: 0;
  border-top: 1px solid var(--color-accent);
  opacity: 0.5;
  transform: translateY(-50%);
  pointer-events: none;
  z-index: 1;
}

/* ── 半透明视窗(VSCode minimap 式拖动把手)──────────────────────────────────
   不透明度可设(设置页「轴视窗不透明度」):--axis-viewport-opacity 为无量纲乘数(默认 1),
   各态 color-mix 百分比统一乘缩放;上限 2.0 时最大 45%×2=90% 仍合法。 */
.tl-viewport {
  position: absolute;
  left: 0;
  right: 0;
  background: color-mix(
    in srgb,
    var(--color-accent) calc(15% * var(--axis-viewport-opacity, 1)),
    transparent
  );
  border: 1px solid
    color-mix(in srgb, var(--color-accent) calc(45% * var(--axis-viewport-opacity, 1)), transparent);
  border-radius: 2px;
  cursor: grab;
  pointer-events: auto;
  z-index: 3; /* 压在刻度/指示线之上、放大镜(4)/浮层之下 */
  transition: background var(--transition-fast);
}
.tl-scrubber:hover .tl-viewport {
  background: color-mix(
    in srgb,
    var(--color-accent) calc(24% * var(--axis-viewport-opacity, 1)),
    transparent
  );
}
.tl-viewport:active {
  cursor: grabbing;
  background: color-mix(
    in srgb,
    var(--color-accent) calc(32% * var(--axis-viewport-opacity, 1)),
    transparent
  );
}

/* ── hover/拖拽浮层 ─────────────────────────────────────────────────── */
.tl-flyout {
  position: absolute;
  right: 100%;
  margin-right: 8px;
  transform: translateY(-50%);
  padding: var(--spacing-2xs) var(--spacing-sm);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-xs);
  line-height: 1.4;
  color: var(--color-text-primary);
  white-space: nowrap;
  pointer-events: none;
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  z-index: 2;
}
.tl-flyout__count {
  color: var(--color-text-secondary);
}

/* ── 放大镜(DOM 版,方案乙:最近 K 项富标签列表)──────────────────────────────
   浮在轨道左侧、随光标垂直居中,固定 K 行(内容自适应高度),不吃指针事件(磁化预览)。 */
.tl-loupe {
  position: absolute;
  right: 100%;
  margin-right: 14px;
  transform: translateY(-50%);
  width: 156px;
  pointer-events: none;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  overflow: hidden;
  z-index: 4;
  font-size: 10px;
  line-height: 1;
}
.tl-loupe-row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 20px;
  padding: 0 8px;
  white-space: nowrap;
  overflow: hidden;
}
/* 密度底条:按窗口内相对 count 从左充填(scaleX 避布局),浮在文字之下作背景热力。 */
.tl-loupe-row__fill {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 100%;
  transform-origin: left center;
  background: var(--color-accent);
  opacity: 0.12;
  z-index: 0;
}
.tl-loupe-row__label {
  position: relative;
  z-index: 1;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--color-text-secondary);
}
.tl-loupe-row__count {
  position: relative;
  z-index: 1;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
.tl-loupe-row.is-center {
  background: var(--color-sidebar-active-bg, var(--color-bg-hover));
}
.tl-loupe-row.is-center .tl-loupe-row__label {
  color: var(--color-text-primary);
  font-weight: 600;
}
.tl-loupe-row.is-center .tl-loupe-row__count {
  color: var(--color-text-secondary);
}
</style>
