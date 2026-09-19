<template>
  <!--
    ⚠️ MULTI-ROOT FRAGMENT — a sticky header + a collapsible body (+ a zero-height
    flow marker, see below), with NO wrapping element. This is load-bearing, do not
    "tidy" it into a single <div>.
    ⚠️ 多根片段——一个粘性标题 + 一个可折叠主体（外加一个零高文档流标记，见下），
    外层无包裹元素。这是承重设计，请勿「整理」成单个 <div>。

    Because component boundaries are transparent in the DOM, every section's
    header ends up a *direct child* of `.sidebar__scroll-area`. That is what lets
    `position: sticky` pin AND stack all headers across the whole scroll range
    (dual top + bottom). Wrapping header+body in a <div> would scope each header's
    sticky to that wrapper, so headers could only stick within their own section
    and cross-section stacking would break.
    由于组件边界在 DOM 中是透明的，每个区块的标题最终都是 `.sidebar__scroll-area`
    的*直接子元素*。这正是 `position: sticky` 能在整个滚动范围内粘住并堆叠所有标题
    （top + bottom 双向）的原因。若把 标题+主体 包进 <div>，会把每个标题的 sticky
    限制在该包裹元素内，导致标题只能在各自区块内粘住，破坏跨区块堆叠。
  -->
  <!--
    零高文档流标记:只用来报告标题的文档流位置(吸顶判据见 useSidebarSections)。
    它是标题的兄弟节点,不是包裹元素,所以不影响上面那条 sticky 堆叠约束;
    自身不参与粘性、不占高度,不改变任何布局与滚动数学。
  -->
  <div ref="flowRef" class="acc-flow-marker" aria-hidden="true"></div>
  <!-- 折叠开关是内部 <button>,而非整头 div——
       #actions 插槽里是真按钮,button 嵌 button 属非法 HTML(浏览器解析会拆散嵌套)。
       toggle flex:1 覆盖 actions 之外全部空白,原「点头部任意空白折叠」体感不变
       (仅左右 padding 边缘 ~8px 不再可点);actions 点击不再冒泡进 toggle,原 @click.stop
       随之退役。Enter/Space 走原生 button 语义,手工 keydown 处理器删除。 -->
  <div
    ref="headerRef"
    class="acc-header"
    :class="{ 'acc-header--first': index === 0 }"
    :style="{ top: stickyTop, bottom: stickyBottom }"
  >
    <button
      class="acc-header__toggle"
      @click="onHeaderClick"
    >
      <ChevronRight
        :size="14"
        class="acc-header__chevron"
        :class="{ expanded }"

      />
      <span class="acc-header__title">{{ title }}</span>
    </button>
    <!-- optional right-aligned actions | 可选的右侧操作区 -->
    <span v-if="$slots.actions" class="acc-header__actions">
      <slot name="actions" />
    </span>
  </div>

  <transition name="acc-collapse" @enter="onEnter" @after-enter="onAfterEnter" @leave="onLeave">
    <!-- v-show（非 v-if）：折叠仅隐藏主体但保留其 DOM，使嵌套状态（如文件夹树展开）
         得以保留——「多级展开状态记忆」。 -->
    <div v-show="expanded" :id="'acc-body-' + id" ref="bodyRef" class="acc-body">
      <!-- 内层包裹元素承载 padding——见下方 .acc-body__inner 说明。 -->
      <div class="acc-body__inner">
        <slot />
      </div>
    </div>
  </transition>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { useSidebarSections } from '../../composables/useSidebarSections'
import { clampRevealScrollTop, bodyRevealNeeded } from './accordion.helpers'

const props = defineProps<{
  /** stable section key for expand-state persistence | 用于持久化展开状态的稳定区块键 */
  id: string
  /** display order among sections; drives sticky stacking | 区块间的显示顺序；驱动粘性堆叠 */
  order: number
  title: string
}>()

const sections = useSidebarSections()
const expanded = computed(() => sections.isExpanded(props.id))

// 仅在挂载期间登记，使条件区块（通过 v-if 挂载）恰好在可见时参与粘性偏移——不留空档。
onMounted(() => {
  sections.register(props.id, props.order)
  // 标记 + 标题 + 主体交给控制器做吸顶判定（玻璃模式据此决定是否补局部衬底；
  // 主体高度变化——折叠动画、树异步展开——会挪动下方标题，由控制器复判）。
  if (flowRef.value && headerRef.value && bodyRef.value) {
    sections.registerSticky(props.id, {
      marker: flowRef.value,
      header: headerRef.value,
      body: bodyRef.value,
    })
  }
})
onUnmounted(() => sections.unregister(props.id))

const index = computed(() => {
  const i = sections.visibleIds.value.indexOf(props.id)
  return i < 0 ? 0 : i
})
const total = computed(() => sections.visibleIds.value.length || 1)

// 堆叠粘性偏移，全部以单个 CSS 变量表达，使算术永不与标题真实高度脱节：
//   top    = H * index            → 粘顶，向下堆叠
//   bottom = H * (total-1-index)  → 粘底，向上堆叠
const stickyTop = computed(() => `calc(var(--sidebar-header-h) * ${index.value})`)
const stickyBottom = computed(
  () => `calc(var(--sidebar-header-h) * ${total.value - 1 - index.value})`,
)

// 折叠与展开的高度/透明度过渡（配合 v-show 使用）。
function onEnter(el: Element) {
  const h = el as HTMLElement
  h.style.height = '0'
  h.style.opacity = '0'
  void h.offsetHeight // force reflow | 强制回流
  h.style.height = h.scrollHeight + 'px'
  h.style.opacity = '1'
}
function onAfterEnter(el: Element) {
  const h = el as HTMLElement
  h.style.height = '' // back to auto so content can grow freely | 恢复 auto，使内容自由增长
  h.style.opacity = ''
  revealBody()
}

const headerRef = ref<HTMLElement | null>(null)
const bodyRef = ref<HTMLElement | null>(null)
const flowRef = ref<HTMLElement | null>(null)

// 主体在滚动内容坐标系中的几何量 + 上下粘性堆叠 inset,供揭示判定/揭示滚动共用一套数字。
// 主体非 sticky,rect 反映真实文档流位置(标题的 rect 是钉住后的视觉位置,不可用)。
interface BodyGeom {
  scroller: HTMLElement
  cur: number
  bodyFlowTop: number
  bodyH: number
  viewportH: number
  topInset: number
  bottomInset: number
}
function bodyGeometry(): BodyGeom | null {
  const body = bodyRef.value
  const scroller = body?.closest('.sidebar__scroll-area') as HTMLElement | null
  if (!body || !scroller) return null
  const headerH = headerRef.value?.offsetHeight || 36
  const bodyFlowTop =
    body.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
  return {
    scroller,
    cur: scroller.scrollTop,
    bodyFlowTop,
    bodyH: body.offsetHeight,
    viewportH: scroller.clientHeight,
    topInset: (index.value + 1) * headerH,
    bottomInset: (total.value - 1 - index.value) * headerH,
  }
}

// 把主体滚进可用视口带(nearest 语义,扣除上下粘性堆叠 inset);已可见则不动。
function revealTo(g: BodyGeom) {
  const target = clampRevealScrollTop(
    g.cur,
    g.bodyFlowTop,
    g.bodyH,
    g.viewportH,
    g.topInset,
    g.bottomInset,
  )
  if (target === g.cur) return
  // 长距离(如从树深处跳回图库)平滑滚动既慢又抖,直接跳转。
  const behavior = Math.abs(target - g.cur) > 4000 ? 'auto' : 'smooth'
  g.scroller.scrollTo({ top: target, behavior })
}

// 揭示刚展开的主体。被钉住(粘顶/粘底)的标题与其文档流中的主体可能相隔整个滚动
// 范围,展开动画在画面外播放,看起来「菜单原地没动」。在高度过渡结束后执行,此时
// bodyH 已定、目标可达。
function revealBody() {
  const g = bodyGeometry()
  if (g) revealTo(g)
}

// 标题点击「意图化」:滚进超长文件树后,被粘性钉住的标题会把展开态区块(图库/工具/管理)的主体
// 挤出可视区——看起来像被折叠了,但 expanded 仍为 true。若此时直接 toggle,第 1 次点击是「真折叠」
// (主体本就看不见,视觉无变化),得再点一次才展开并揭示,凭空多一次点击 + 揭示前 0.28s 空转动画顿挫。
// 改为:主体已被挤出可用带 → 点击=揭示(单击平滑滚回、不改状态、无高度动画),与「滚动揭示」路径统一;
// 主体可见时才照常 toggle 折叠。
function onHeaderClick() {
  if (expanded.value) {
    const g = bodyGeometry()
    if (
      g &&
      bodyRevealNeeded(g.cur, g.bodyFlowTop, g.bodyH, g.viewportH, g.topInset, g.bottomInset)
    ) {
      revealTo(g)
      return
    }
  }
  sections.toggle(props.id)
}
function onLeave(el: Element) {
  const h = el as HTMLElement
  h.style.height = h.offsetHeight + 'px'
  h.style.opacity = '1'
  void h.offsetHeight // force reflow | 强制回流
  h.style.height = '0'
  h.style.opacity = '0'
}
</script>

<style scoped>
.acc-collapse-enter-active,
.acc-collapse-leave-active {
  transition:
    height 0.28s cubic-bezier(0.4, 0, 0.2, 1),
    opacity 0.28s cubic-bezier(0.4, 0, 0.2, 1);
  overflow: hidden;
}

/* ── Sticky header ─────────────────────────────────────────────────────────
   双向粘性：滚出顶部时粘顶、滚出底部时粘底，按 index 堆叠互不遮挡。 */
/* 吸顶判据的位置基准:非 sticky,恒在文档流原位。零高块盒 ⇒ 不改变布局,也不会被
   sticky 挪动;绝不可改成 display:none/contents——那样就没有 rect,判据失去基准。 */
.acc-flow-marker {
  height: 0;
}
.acc-header {
  position: sticky;
  z-index: 10;
  display: flex;
  align-items: center;
  /* rail + 箭头 14px + gap 8px = 标题文字起点 30px(--sidebar-indent),子菜单内容对齐此轨。 */
  gap: var(--spacing-sm);
  height: var(--sidebar-header-h);
  padding: 0 var(--spacing-sm) 0 var(--sidebar-rail, 8px);
  /* 群组标签(排版重构):比子项更小更安静——层级靠「标签小 + 字距 + 内容嵌套缩进」表达,
     而非把标题做大(原 13px/700 与子项几乎同级,主次不分)。
     颜色必须是 secondary 而非 tertiary:标签是 12px 可交互文本,WCAG AA 要求 ≥4.5;
     而 tertiary 的主题契约(a44436b token 抬升)只保 ≥3.0、定位为非交互弱文本,
     对本底色 bg-secondary 实测仅 3.3~4.0。secondary 六主题实测 4.97~6.85 全过,
     门禁已钉 secondary × bg-secondary ≥4.5(check-theme-contrast.mjs)。 */
   font-size: var(--font-size-2xs);
  font-weight: 600;
  letter-spacing: 0.05em;
  color: var(--color-text-secondary);
  user-select: none;
  /* MUST stay opaque in EVERY state — body content scrolls UNDER pinned headers.
     A translucent bg (e.g. --color-bg-hover) would let that content bleed through.
     必须在任何状态下保持不透明——主体内容会从已固定的标题下方滚过。半透明背景
     （如 --color-bg-hover）会让下方内容透出来。 */
  background: var(--color-bg-secondary);
  /* 分隔线画在标题「上方」:区块与区块之间有界,标题与自己的子菜单之间无线(原 border-bottom
     恰好相反——与上一区块末项无分隔、却隔开自己的子菜单)。钉住堆叠时每条标题自带上边线,
     区块界限同样清晰。首个标题无上邻,线透明(保高度恒定,不动堆叠算术)。 */
  border-top: 1px solid var(--color-divider);
  transition:
    color 0.15s,
    background-color 0.15s;
}
.acc-header--first {
  border-top-color: transparent;
}
.acc-header:hover {
  color: var(--color-text-primary);
  /* opaque, theme-aware hover — NOT the translucent --color-bg-hover */
  /* 不透明、随主题变化的 hover 背景——非半透明的 --color-bg-hover */
  background: var(--color-bg-elevated);
}
/* 折叠开关按钮:flex:1 铺满 actions 之外全部头部区域(含标题右侧空白),视觉完全透明,
   排版样式(字号/字重/字距)从 .acc-header 继承——font:inherit 压掉 UA 按钮默认字体。 */
.acc-header__toggle {
  flex: 1;
  min-width: 0; /* 标题 ellipsis 收缩链 */
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  height: 100%;
  padding: 0;
  font: inherit;
  letter-spacing: inherit;
  text-align: left;
}
.acc-header__toggle:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
}
.acc-header__chevron {
  flex-shrink: 0;
  transition: transform 0.2s;
}
.acc-header__chevron.expanded {
  transform: rotate(90deg);
}
.acc-header__title {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  /* 标题保底宽:操作区(FoldersSection 5 控件)不收缩,窄侧栏下收缩链全压标题,
     曾把「文件夹」挤剩「文」。3.5em ≈ 3 个汉字 + 余量,标题永远可读;
     极端窄时宁可让操作区溢出被裁,也不牺牲标题。 */
  min-width: 3.5em;
}
.acc-header__actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 2px;
  cursor: default;
}

/* The animated element itself MUST stay zero-padding so that height→0 collapses
   to a true 0. A content-box with padding can't shrink below its padding height,
   so height→0 leaves a residual band that display:none snaps away in the last
   frame — a visible "顿一下" at the end of the collapse. Put the breathing room
   on the inner wrapper instead. | 被动画的元素本身必须零内边距，使 height→0 能干净
   收到真正的 0。带 padding 的盒子无法收缩到小于其 padding 高度，height→0 会残留一条，
   在 display:none 时于末帧瞬间消失——即折叠收尾那一「顿」。留白改放到内层包裹元素。 */
.acc-body__inner {
  /* 底部留白大于顶部:与下一区块标题的 border-top 一起构成区块间的视觉分隔带。 */
  padding: var(--spacing-xs) 0 12px;
}
</style>
