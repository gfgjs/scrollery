<template>
  <!-- 选区动作簇(SelectionBar 合并/分离方案 C2):折叠命令流 + ⋯ 溢出菜单 + 固定件(计数/✕)。
       浮动壳(SelectionToolbar)与 docked 壳(AppStatusBar,C3)各放一实例,variant 决定尺寸。
       两实例而非一实例换容器——useToolbarOverflow 的 ResizeObserver 只在 onMounted 绑一次容器,
       v-if 换壳会让 observer 钉在死元素上;每壳独立实例 mount 即 measure+observe,天然正确。 -->
  <!-- 折叠流内含 ColorLabelPicker(内联 8 色块 button 的复合控件)+ 固定件在流容器外 +
       折叠测量标记挂在 .fold-item 包裹 span 而非按钮,故不套 roving tabindex。 -->
  <div
    class="selection-actions"
    :class="[`selection-actions--${variant}`, `selection-actions--align-${align}`]"
  >
    <!-- 计数:壳固定件,不进 commands、不参与折叠(逃生口/上下文恒可见)。 -->
    <span class="selection-count">
      {{ $t('selection.selected', { count: selection.selectedCount.value }) }}
    </span>

    <!-- 折叠流容器:useToolbarOverflow 的测量对象(带折叠标记属性的折叠单元)全在此。flex:0 1 auto +
         min-width:0 使其成为壳内唯一可收缩区 → 窄容器时命令从尾折入 ⋯ 菜单;计数/✕ 在容器外不占测量预算。
         测量帧(isMeasuring)全渲染取自然宽,用 visibility:hidden 遮该帧(而非 overflow:hidden——后者
         会裁掉条外上方弹出的 data-tooltip);offsetWidth 在 visibility:hidden 下仍可测。 -->
    <div
      ref="flowRef"
      class="selection-actions__flow"
      :class="{ 'is-measuring': isMeasuring, 'is-settling': isSettling }"
    >
      <template v-for="(cmd, i) in commands" :key="cmd.key">
        <!-- 折叠单元(Route A 平滑收纳): 外层 .fold-item(grid 1fr↔0fr 收缩真实宽 + margin 随之收拢) +
             内层 __inner(overflow:hidden 裁内容 + translateX 横向收入), 与顶栏 GalleryViewControls 同款。
             folded 键于 visibleCount(非 isMeasuring→仅真跨阈值才过渡); 折叠项恒留 DOM 靠 grid 收拢,
             ⋯ 菜单另渲一份。测量帧(isMeasuring, mount/locale)全展开取自然宽, 那一两帧禁过渡。 -->
        <span
          class="fold-item"
          :class="{ 'fold-item--folded': !isMeasuring && i >= visibleCount }"
          data-toolbar-item
        >
          <span class="fold-item__inner">
            <span v-if="cmd.groupStart" class="divider"></span>
            <ColorLabelPicker
              v-if="cmd.kind === 'colors'"
              class="toolbar-colors"
              :model-value="0"
              :size="colorSize"
              :allow-clear="false"
              @change="(v: number) => cmd.run(v)"
            />
            <button
              v-else
              class="selection-action"
              :class="{ 'selection-action--danger': cmd.danger }"
              :data-tooltip="$t(cmd.labelKey)"

              @click="cmd.run()"
            >
              <component :is="cmd.icon" :size="iconSize" />
            </button>
          </span>
        </span>
      </template>

      <!-- ⋯ 溢出触发钮:引擎经 overflowButtonWidth 预留其宽,仅 hasOverflow 时显示。
           原生 button 直接作 UiPopover 锚点(v-show 保持 DOM 在位,ref 恒指向元素)。 -->
      <button
        v-show="hasOverflow"
        ref="moreBtnEl"
        class="selection-action selection-more"
        :style="{ width: `${overflowButtonWidth}px` }"
        :data-tooltip="$t('selection.more')"
        @click="toggleMenu"
      >
        <MoreHorizontal :size="iconSize" />
      </button>
    </div>

    <!-- 停靠/浮起一键切换:壳固定件,不折叠(逃生口/切换口恒可达)。选区瞬态,条上直切=「一键」字面兑现。 -->
    <button
      class="selection-action selection-dock-toggle"
      :data-tooltip="dockToggleLabel"

      @click="toggleDock"
    >
      <component :is="dockToggleIcon" :size="iconSize" />
    </button>

    <!-- ✕ 取消选择:壳固定件,逃生口恒可达,不折叠。 -->
    <div class="divider"></div>
    <button
      class="selection-action"
      :data-tooltip="$t('selection.cancel')"

      @click="selection.clearSelection()"
    >
      <X :size="iconSize" />
    </button>
  </div>

  <!-- 溢出菜单 = UiPopover(已建成原语):top 上弹**居中对齐 ⋯ 按钮**(条在底部),近边缘 flip/shift 由
       @floating-ui 处理;焦点陷阱/backdrop 点击/Esc dismiss/autoUpdate 滚动跟随全部免费。菜单表面视觉由本 slot 自带。 -->
  <UiPopover v-model:open="menuOpen" :anchor="moreBtnEl" placement="top">
    <div class="selection-overflow-menu">
      <template v-for="(cmd, idx) in overflowCommands" :key="cmd.key">
        <!-- 组首分隔线(菜单里):折叠集首项若恰是组首则不加前导线(idx>0)。 -->
        <div v-if="cmd.groupStart && idx > 0" class="menu-divider"></div>
        <!-- colors 单元:整行 ColorLabelPicker(点色块即设色并关菜单)。 -->
        <div v-if="cmd.kind === 'colors'" class="menu-row menu-row--colors">
          <component :is="cmd.icon" class="menu-row__icon" :size="16" />
          <span class="menu-row__label">{{ $t(cmd.labelKey) }}</span>
          <ColorLabelPicker
            :model-value="0"
            :size="14"
            :allow-clear="false"
            @change="(v: number) => runFromMenu(cmd, v)"
          />
        </div>
        <!-- 普通命令行:图标 + 可见文字标签。 -->
        <button
          v-else
          class="menu-row"
          :class="{ 'menu-row--danger': cmd.danger }"
          @click="runFromMenu(cmd)"
        >
          <component :is="cmd.icon" class="menu-row__icon" :size="16" />
          <span class="menu-row__label">{{ $t(cmd.labelKey) }}</span>
        </button>
      </template>
    </div>
  </UiPopover>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { X, MoreHorizontal, PanelBottom, PanelTop } from '@lucide/vue'
import ColorLabelPicker from '../common/ColorLabelPicker.vue'
import UiPopover from '../ui/UiPopover.vue'
import { useSelection } from '../../composables/useSelection'
import { useSelectionBarMode } from '../../composables/useSelectionBarMode'
import { useToolbarOverflow } from '../../composables/useToolbarOverflow'
import type { SelectionCommand } from '../../types/selectionCommand'

const props = withDefaults(
  defineProps<{
    commands: SelectionCommand[]
    /** 壳形态:floating=浮动胶囊(大圆钮);docked=状态栏内(24px 紧凑,C3)。 */
    variant?: 'floating' | 'docked'
  }>(),
  { variant: 'floating' },
)

const selection = useSelection()
const { t, locale } = useI18n()

// 停靠/浮起一键切换(条上主入口):切换 useSelectionBarMode 单例,壳随之换血(状态在单例不丢)。
// 图标/标签随当前形态:docked→浮起(PanelTop 上弹);floating→停靠(PanelBottom 下沉)。
// align:水平对齐单例。docked 态浮动 wrapper 不渲染(Teleport 进状态栏 outlet),对齐 class 须由本组件
// 在 outlet 内的 .selection-actions 上施加(浮动态对齐由 SelectionToolbar 的 wrapper 承担,见下 CSS 注)。
const { docked, setDocked, align } = useSelectionBarMode()
const dockToggleIcon = computed(() => (docked.value ? PanelTop : PanelBottom))
const dockToggleLabel = computed(() => t(docked.value ? 'selection.floatBar' : 'selection.dockBar'))
function toggleDock() {
  setDocked(!docked.value)
}

// 变体尺寸:浮动=紧凑(32px 圆钮/18 图标);docked=状态栏内(24×24/14 图标)。
const iconSize = computed(() => (props.variant === 'docked' ? 14 : 18))
const colorSize = computed(() => (props.variant === 'docked' ? 12 : 16))

// 折叠引擎(本实例自持):测量对象=flowRef 内 data-toolbar-item。
// remeasureKey=locale:命令集静态,仅语言切换改标签宽 → 触发重测(容器宽变化由 ResizeObserver 兜)。
const flowRef = ref<HTMLElement | null>(null)
const remeasureKey = computed(() => locale.value)
// ⋯ 溢出按钮宽:折叠引擎据此预留空间。**与按钮实际渲染宽同源**——下方 inline 绑到按钮 style 上, 使
// 「引擎预留的数」与「按钮占的数」由构造保证相等, 结构上杜绝声明/实际漂移。variant 每实例静态(浮动与
// docked 是两个实例, 切换时一卸一挂), 故取常量而非 computed。
//
// 真机 round10 #3 教训:溢出按钮宽度必须与 CSS 实际尺寸同源。这一数值在 AppToolbar 那种 flex:1
// 全宽容器里**完全无害**(available 外生, 高估只是早折一项, 稳定);放进内容宽容器就要命——那里
// available=f(visibleCount) 是**内生**的, 高估 → budget 比当前内容少 4px → 掉一项 → 内容更窄 → 再掉
// 一项, 一路吞到全折叠, **且与拉宽/收窄方向无关**(真机症状:「收窄到一半再拉宽, 还会继续把剩余按钮收进
// 折叠」)。同一个数字, 在一种拓扑下是安全裕度, 在另一种拓扑下是正反馈的点火器。
const overflowButtonWidth = props.variant === 'docked' ? 24 : 32

// 可用宽探针(Route A 平滑折叠): 临时 flex-grow 撑到可用宽, 同步读 clientWidth 后立即还原。全程一次
// microtask 内(直接操作 classList、不经 reactive/paint), 折叠项的 grid 宽在此期间不变 → 不触发项过渡。
// 避开「展开全部项量宽」那套(会让折叠项每帧闪动)。
//
// **必须连胶囊一起撑**(round10 #3 修复):flow 的 flex-grow 只能瓜分**它自己那个 flex 容器**
// (.selection-actions)内的自由空间;而 .selection-actions 的宽由**内容宽的胶囊**(.selection-toolbar,
// flex:0 1 auto)决定——命令一折起胶囊就缩、自由空间归零, 探针读回的仍是折叠后的内容宽 = round6 #1 那个
// 自锁棘轮原样复现(旧注释曾断言只撑 flow 即可回弹, 系误判:flex-grow 不向上传导)。给胶囊也加探针类 →
// 它被 max-width 钳到 wrapper 内容宽 → .selection-actions 随之变宽 → flow 才读得到真可用宽。
// docked 态无胶囊祖先(outlet 本就是 flex:1 全宽 → 探针一直有效, 故 round6 #2 早已好), closest 返回 null。
function measureAvailable(): number {
  const el = flowRef.value
  if (!el) return 0
  const capsule = el.closest<HTMLElement>('.selection-toolbar')
  capsule?.classList.add('is-probing-width')
  el.classList.add('is-probing-width')
  const w = el.clientWidth // 强制同步 reflow, 读填满态可用宽
  el.classList.remove('is-probing-width')
  capsule?.classList.remove('is-probing-width')
  return w
}
const { visibleCount, hasOverflow, isMeasuring, isSettling } = useToolbarOverflow({
  containerRef: flowRef,
  overflowButtonWidth,
  remeasureKey,
  // flow 是内容宽(flex:0 1 auto):折叠后自身变窄,窗口再变宽时它不会跟着变宽 → 折叠自锁棘轮(窄窗折叠后
  // 拉宽不回弹)。containerFillsWidth:false → 监听 window resize;配 measureAvailable 探针, resize 走廉价
  // 重算(不全展开各项)→ 折叠/放出动画干净无闪, 并借探针的 flex-grow 读到真可用宽以回弹(见探针注释)。
  containerFillsWidth: false,
  measureAvailable,
})

// 溢出集=第 visibleCount 项起(菜单里以「图标+文字标签」行再渲染一遍)。
const overflowCommands = computed(() => props.commands.slice(visibleCount.value))

// ⋯ 菜单开阖 + 锚点(原生 button ref 直接作 anchor,定位/钳制/翻转交 UiPopover)。
const menuOpen = ref(false)
const moreBtnEl = ref<HTMLElement | null>(null)
function toggleMenu() {
  menuOpen.value = !menuOpen.value
}
// 容器变宽、溢出集清空(hasOverflow 转 false)时自动关空菜单(§3.5)。
watch(hasOverflow, (v) => {
  if (!v) menuOpen.value = false
})
// 菜单项点击=执行动作后关菜单(colors 传色值)。
function runFromMenu(cmd: SelectionCommand, value?: number) {
  cmd.run(value)
  menuOpen.value = false
}
</script>

<style scoped>
/* 动作簇容器:计数 | 折叠流 | ✕,横向排布。flex:1 min-width:0 使其在壳内可收缩(触发流容器折叠)。
   簇内 8px 紧凑间距;计数与命令间靠 count 的 margin 拉开(复刻改造前 left↔actions 的 16px)。 */
.selection-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex: 1 1 auto;
  min-width: 0;
}

.selection-count {
  font-size: var(--font-size-sm);
  font-weight: 600;
  white-space: nowrap;
  flex-shrink: 0;
  margin-inline-end: var(--spacing-sm);
}

/* 折叠流容器:壳内唯一可收缩区(flex:0 1 auto + min-width:0),clientWidth 成为命令的真实预算。
   gap:0——项间距改由各 .fold-item 的 margin-inline-end 承担(折叠时随之收拢归零, 无碎屑; 与顶栏 G6 同理,
   避免 gap 在 grid 收成 0fr 的折叠项两侧留下幽灵间距把 ⋯ 按钮推远)。 */
.selection-actions__flow {
  display: flex;
  align-items: center;
  gap: 0;
  flex: 0 1 auto;
  min-width: 0;
}
/* 测量帧:全渲染取自然宽的一帧用 visibility:hidden 遮住(不裁条外 tooltip,offsetWidth 仍可测)。 */
.selection-actions__flow.is-measuring {
  visibility: hidden;
}
/* 可用宽探针帧(Route A): 临时 flex-grow 使 flow 填满可用宽以读真值。同步加/去类, 不经 paint;
   只改 flow 自身宽度, 折叠项 grid 宽不变 → 不触发项过渡(见 script 的 measureAvailable)。 */
.selection-actions__flow.is-probing-width {
  flex-grow: 1;
}

/* 折叠单元(Route A 平滑收纳, 与顶栏 GalleryViewControls 同款): 外层 grid(1fr↔0fr 收缩真实宽 + margin
   随之收拢) + 内层 __inner(overflow:hidden 裁内容 + translateX 横向收入)。一命令(+ groupStart 前置
   divider)一格。折叠/展开经 CSS 过渡平滑; 测量帧禁过渡(取自然宽的一两帧不动画)。 */
.fold-item {
  display: grid;
  grid-template-columns: 1fr;
  flex-shrink: 0;
  margin-inline-end: 8px;
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
  display: inline-flex;
  align-items: center;
  transition: transform var(--duration-moderate) var(--ease-out);
}
.fold-item--folded .fold-item__inner {
  transform: translateX(10px);
}
/* 量尺态(is-measuring): useToolbarOverflow.measure() 命令式短暂加此类(同步段内加→读宽→移),据此
   **强制所有 fold-item 展开到自然宽**(含已折叠项 → 覆盖 --folded 的 0fr/opacity:0/margin:0/内层
   translateX)以量真实宽 + 禁过渡。measure() 不再响应式 show-all(那次渲染帧会被绘制=闪动根因),故此处
   的强制展开是折叠项能被量到的唯一途径(缺它则 locale 重测读折叠项 offsetWidth≈0 → 折叠算错)。因量尺
   全程同步、无 await, 浏览器不绘制此展开态; 叠加下方 visibility:hidden(遮首挂 mount 的响应式初始展开帧)。 */
.selection-actions__flow.is-measuring .fold-item {
  grid-template-columns: 1fr;
  opacity: 1;
  margin-inline-end: 8px;
}
.selection-actions__flow.is-measuring .fold-item__inner {
  transform: none;
}
/* 折叠过渡抑制:量尺帧(is-measuring)与内容驱动折叠落定帧(is-settling)都禁过渡。前者使加/移量尺类不触发
   动画;后者使内容驱动重测(locale 换文案)引发的折叠**瞬时**完成,不走 0.28s 过渡——那条过渡只留给窗口
   resize 平滑折叠(走 measureAvailable 探针路径,不加此二类)。is-settling 只禁过渡、**不**强制展开(故折叠
   真会发生),与 is-measuring 的强制展开区分。与顶栏 AppToolbar 同一不变量(共享 useToolbarOverflow)。 */
.selection-actions__flow.is-measuring .fold-item,
.selection-actions__flow.is-measuring .fold-item__inner,
.selection-actions__flow.is-settling .fold-item,
.selection-actions__flow.is-settling .fold-item__inner {
  transition: none;
}

/* 批量色块在动作条里垂直居中,与圆形动作按钮对齐。 */
.toolbar-colors {
  align-self: center;
}

.selection-action {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-default);
  height: var(--control-size-default);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--color-text-primary);
  border: none;
  cursor: pointer;
  flex-shrink: 0;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.selection-action:hover {
  background: var(--color-bg-hover);
}
.selection-action--danger {
  color: var(--color-error);
}
.selection-action--danger:hover {
  background: var(--color-error);
  color: var(--color-text-on-error);
}

/* docked 紧凑规格(C3 状态栏 28px 行内):24×24 命中区(WCAG 2.2 AA 目标尺寸下限)。 */
.selection-actions--docked .selection-action {
  width: 24px;
  height: 24px;
}
.selection-actions--docked .selection-count {
  font-size: var(--font-size-xs);
}

/* docked 态水平对齐(补齐新需求 2 的 docked 缺口):浮动态胶囊靠 SelectionToolbar 的 .selection-toolbar-wrapper
   施 justify-content 移动内容宽胶囊;但 docked 态浮动 wrapper 不渲染(Teleport 进状态栏 outlet),那条 DOM
   路径无对齐宿主 → 此前改设置无作用。此处补:对填满 outlet(is-active 时 flex:1 1 auto)的 .selection-actions
   施 justify-content,决定「计数 + 折叠流 + 切换 + ✕」簇在 outlet 内的落位。仅 docked 变体应用(浮动态
   selection-actions 为内容宽、无自由空间,限定 --docked 以明确意图、且不与浮动 wrapper 对齐重叠)。
   折叠测量不受影响:折叠流 flex-grow:0,自由空间只进 justify 留白而非流内;窗口收窄先吃留白、再由折叠引擎
   从尾折入 ⋯(与浮动态「先减留白再折叠」一致)。align-left = 默认 flex-start,无需额外规则。
   注:居中/靠右是在 outlet 区(状态栏左侧、版本号之前)内对齐,非整条状态栏几何中心——outlet 即其可用轨道。 */
.selection-actions--docked.selection-actions--align-center {
  justify-content: center;
}
.selection-actions--docked.selection-actions--align-right {
  justify-content: flex-end;
}

.divider {
  width: 1px;
  height: 20px;
  background: var(--color-border);
  margin: 0 4px;
  flex-shrink: 0;
}
.selection-actions--docked .divider {
  height: 14px;
}

/* ── Custom CSS Tooltip(条外上方弹出;容器不设 overflow:hidden 以免被裁,见 flow 测量帧注)──── */
[data-tooltip] {
  position: relative;
}
[data-tooltip]::after {
  content: attr(data-tooltip);
  position: absolute;
  bottom: calc(100% + 10px);
  left: 50%;
  transform: translateX(-50%) translateY(4px);
  padding: var(--spacing-xs) var(--spacing-sm);
  background: var(--material-recipe-float-background-color);
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
  font-weight: 500;
  border-radius: var(--radius-sm);
  border: 1px solid var(--material-recipe-float-border-color);
  box-shadow: var(--material-recipe-float-box-shadow);
  backdrop-filter: var(--material-recipe-float-backdrop-filter);
  -webkit-backdrop-filter: var(--material-recipe-float-backdrop-filter);
  white-space: nowrap;
  pointer-events: none;
  opacity: 0;
  visibility: hidden;
  transition:
    opacity 0.2s cubic-bezier(0.16, 1, 0.3, 1),
    visibility 0.2s cubic-bezier(0.16, 1, 0.3, 1),
    transform 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  z-index: 1000;
}
[data-tooltip]:hover::after {
  opacity: 1;
  visibility: visible;
  transform: translateX(-50%) translateY(0);
}

/* ── 溢出 ⋯ 菜单(UiPopover slot 内容;表面视觉在此,定位/dismiss/焦点陷阱由原语持有)──────── */
.selection-overflow-menu {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
  min-width: 180px;
  padding: var(--spacing-xs);
}
.menu-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-align: left;
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}
.menu-row:hover {
  background: var(--color-bg-hover);
}
.menu-row__icon {
  flex-shrink: 0;
  color: var(--color-text-secondary);
}
.menu-row__label {
  flex: 1;
  white-space: nowrap;
}
.menu-row--danger {
  color: var(--color-error);
}
.menu-row--danger:hover {
  background: var(--color-error);
  color: var(--color-text-on-error);
}
.menu-row--danger:hover .menu-row__icon {
  color: var(--color-text-on-error);
}
.menu-row--colors .menu-row__label {
  flex: 0 0 auto;
}
.menu-divider {
  height: 1px;
  background: var(--color-border);
  margin: 4px 0;
}
</style>
