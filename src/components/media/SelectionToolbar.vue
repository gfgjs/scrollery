<template>
  <!-- 分离(浮动,默认):可拖胶囊。仅非 docked 时渲染。
       §8.1 browse-only:重复镜头激活(forceHidden)时显式阻断——两形态显隐条件共同置 false。
       宿主不 v-if 卸载本组件:卸载/重挂会让 onActivated/onDeactivated 的 hostActive 生命周期
       错拍(镜头期失活再激活,docked Teleport 门控失灵),保持挂载、纯隐显最稳。 -->
  <Transition name="slide-up">
    <div
      v-if="selection.isSelectionMode.value && !mode.docked.value && !forceHidden"
      ref="wrapperEl"
      class="selection-toolbar-wrapper"
      :class="`selection-toolbar-wrapper--${mode.align.value}`"
    >
      <div
        ref="pillEl"
        class="selection-toolbar"
        :class="{ 'is-dragging': isDragging }"
        :style="{ transform: `translate(${offsetX}px, ${offsetY}px)` }"
      >
        <div class="drag-handle" :title="$t('selection.drag')" @pointerdown="onDragStart">
          <GripVertical :size="16" />
        </div>
        <!-- 动作簇(计数 + 折叠命令流 + ⋯ 溢出菜单 + 停靠切换 + ✕)抽入 SelectionActions;浮动壳只余拖拽手柄。
             docked 壳复用同一 SelectionActions(variant='docked'),两实例各自持折叠引擎。 -->
        <SelectionActions :commands="commands" variant="floating" />
      </div>
    </div>
  </Transition>

  <!-- 合并(docked):把动作簇 Teleport 进状态栏 outlet。Teleport 只搬 DOM 不搬组件层级——9 个批量 handler
       原地留 MediaGrid,组件层级/事件链/依赖注入全不变(§3.2)。
       gate 含 isSelectionMode(挂载期选区必空 → Teleport 不求值目标;用户首次进选区时状态栏早就位,勿改
       成挂载期常驻渲染)+ kaActive(MediaGrid 被 KeepAlive deactivate 时,teleported DOM 逃逸出缓存子树
       不会随之摘除 → 显式守卫卸载,防 docked 条在查看器里残留)+ forceHidden(§8.1 browse-only:
       重复镜头显式阻断批量工具条)。 -->
  <Teleport
    v-if="selection.isSelectionMode.value && mode.docked.value && mode.hostActive.value && !forceHidden"
    to="#statusbar-selection-outlet"
  >
    <SelectionActions :commands="commands" variant="docked" />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onActivated, onBeforeUnmount, onDeactivated } from 'vue'
import { GripVertical } from '@lucide/vue'
import SelectionActions from './SelectionActions.vue'
import { useSelection } from '../../composables/useSelection'
import { useSelectionBarMode, clampOffset } from '../../composables/useSelectionBarMode'
import type { SelectionCommand } from '../../types/selectionCommand'

// 动作数据驱动化(C1):MediaGrid 组装 SelectionCommand[] 传入,原样转交 SelectionActions。
withDefaults(
  defineProps<{
    commands: SelectionCommand[]
    /** 重复镜头 browse-only(§8.1):激活时显式阻断两形态的批量工具条显隐。 */
    forceHidden?: boolean
  }>(),
  { forceHidden: false },
)

const selection = useSelection()
// 分离/合并形态单例(设置页开关 + 条上切换钮同源)。
const mode = useSelectionBarMode()

// KeepAlive 守卫:MediaGrid 被保活,进查看器时 deactivate。docked 形态经 Teleport 逃逸到状态栏,不随
// MediaGrid 缓存子树摘除 → 用 hostActive 显式卸载(初值 true 兼容非 KeepAlive 上下文)。写入共享单例,
// 使 AppStatusBar 的 info 让位与本 Teleport gate 同条件同步(防「选区残留进查看器」时 outlet 空白)。
onActivated(() => {
  mode.setHostActive(true)
})
onDeactivated(() => {
  mode.setHostActive(false)
})

// 拖拽位置持久化(C4):live 位移用本地 offsetX/Y 驱动 transform(拖拽期每帧更新,不落库);
// 初值取设置里的 offset(useSelectionBarMode 单例,存 config.toml),拖拽结束/恢复/resize 时钳制并回写。
const wrapperEl = ref<HTMLElement | null>(null)
const pillEl = ref<HTMLElement | null>(null)
const offsetX = ref(mode.offset.value.x)
const offsetY = ref(mode.offset.value.y)
const isDragging = ref(false)
let startX = 0
let startY = 0
let initOffsetX = 0
let initOffsetY = 0

/**
 * 把当前 live offset 钳到「胶囊完整可见且手柄可达」范围并回写持久化。三时机调用:恢复(条出现)/
 * 拖拽结束/窗口 resize。边界=胶囊定位上下文(.selection-toolbar-wrapper 的 offsetParent=
 * .media-grid-layout);拖拽中不钳(勿与手势互搏)。
 */
function clampToBounds() {
  if (isDragging.value) return
  const pill = pillEl.value
  const bounds = wrapperEl.value?.offsetParent as HTMLElement | null
  if (!pill || !bounds) return
  const clamped = clampOffset(
    { x: offsetX.value, y: offsetY.value },
    { width: pill.offsetWidth, height: pill.offsetHeight },
    { width: bounds.clientWidth, height: bounds.clientHeight },
    // 传当前对齐:靠左/靠右的静止基位非居中,可拖范围随之非对称(靠左几乎只能右移)。
    mode.align.value,
  )
  offsetX.value = clamped.x
  offsetY.value = clamped.y
  // 仅在钳制真的改变了值时回写,避免无谓的写盘请求。
  if (clamped.x !== mode.offset.value.x || clamped.y !== mode.offset.value.y) {
    mode.setOffset(clamped)
  }
}

function onDragStart(e: PointerEvent) {
  // Ignore right clicks
  if (e.button !== 0) return
  isDragging.value = true
  startX = e.clientX
  startY = e.clientY
  initOffsetX = offsetX.value
  initOffsetY = offsetY.value

  const target = e.currentTarget as HTMLElement
  target.setPointerCapture(e.pointerId)

  target.addEventListener('pointermove', onDragMove)
  target.addEventListener('pointerup', onDragEnd)
  target.addEventListener('pointercancel', onDragEnd)
}

function onDragMove(e: PointerEvent) {
  if (!isDragging.value) return
  offsetX.value = initOffsetX + (e.clientX - startX)
  offsetY.value = initOffsetY + (e.clientY - startY)
}

function onDragEnd(e: PointerEvent) {
  isDragging.value = false
  const target = e.currentTarget as HTMLElement
  target.removeEventListener('pointermove', onDragMove)
  target.removeEventListener('pointerup', onDragEnd)
  target.removeEventListener('pointercancel', onDragEnd)
  target.releasePointerCapture(e.pointerId)
  // 拖拽结束:钳制并持久化(不再退出选区即复位——持久化语义与 reset-on-exit 互斥,有意的行为变更)。
  clampToBounds()
}

// 后端只应用:权威 offset 变化(启动水合 / 恢复默认 / 外部改文件)→ 同步本地拖拽位移。
// 拖拽中不打断手势(以本地位移为准),结束后由 clampToBounds 归位;钳制幂等,不产生循环回写。
watch(
  () => [mode.offset.value.x, mode.offset.value.y] as const,
  ([x, y]) => {
    if (isDragging.value) return
    offsetX.value = x
    offsetY.value = y
    nextTick(clampToBounds)
  },
)

// 恢复:条出现时(选区态且非 docked)钳制持久化位置——防「大窗口拖到角落 → 小窗口打开后条在屏外失踪」。
watch(
  () => selection.isSelectionMode.value && !mode.docked.value,
  (visible) => {
    if (visible) nextTick(clampToBounds)
  },
  { immediate: true },
)

// 对齐变更(用户裁决:对齐=默认位、拖拽仍可覆盖):切对齐即清空旧拖拽偏移——否则基位一变,旧 offset 会把
// 胶囊推到意料外甚至屏外。回到该对齐的静止基位(offset=0),下一 tick 再钳一次兜底。
watch(
  () => mode.align.value,
  () => {
    offsetX.value = 0
    offsetY.value = 0
    if (mode.offset.value.x !== 0 || mode.offset.value.y !== 0) mode.setOffset({ x: 0, y: 0 })
    nextTick(clampToBounds)
  },
)

// resize:窗口尺寸变化时重新钳制(条可见期恒挂监听,拖拽中由 clampToBounds 自身跳过)。
function onWindowResize() {
  clampToBounds()
}
onMounted(() => {
  window.addEventListener('resize', onWindowResize)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', onWindowResize)
})
</script>

<style scoped>
.selection-toolbar-wrapper {
  position: absolute;
  bottom: 32px;
  left: 0;
  right: 0;
  display: flex;
  justify-content: center;
  /* 靠左/靠右时胶囊距边界的呼吸(与 useSelectionBarMode 的 SELECTION_BAR_SIDE_INSET=12 同源,
     clampOffset 据此算左/右对齐的静止基位 baseLeft);居中对齐对称留白不受影响。 */
  padding-inline: 12px;
  z-index: 200;
  pointer-events: none; /* Let clicks pass through outside toolbar */
}
/* 胶囊水平对齐(新需求 2):对齐=默认停靠位,拖拽 transform 叠加其上(改对齐时 watch 清空旧偏移)。
   变体类在基类之后 → 覆盖基类的 center;靠左/靠右时窗口收窄先吃 justify 留白、后折叠。 */
.selection-toolbar-wrapper--center {
  justify-content: center;
}
.selection-toolbar-wrapper--left {
  justify-content: flex-start;
}
.selection-toolbar-wrapper--right {
  justify-content: flex-end;
}

/* 可用宽探针帧(round10 #3 修复):SelectionActions.measureAvailable 同步给胶囊加此类, 使其从内容宽撑到
   max-width 上限, 内部折叠流才读得到真可用宽。
   为何非撑胶囊不可:flex-grow 只瓜分**自身 flex 容器**内的自由空间、**不向上传导**。折叠流(flow)在
   .selection-actions 内, 而 .selection-actions 的宽由这个内容宽胶囊决定——命令一折起胶囊就缩、自由空间
   归零, 只给 flow 加 flex-grow 读回的仍是折叠后的内容宽 → 折叠自锁棘轮(窄窗折叠后拉宽不回弹)。
   同步加/去类、不经 paint(见 SelectionActions.measureAvailable)。 */
.selection-toolbar.is-probing-width {
  flex-grow: 1;
}

/* 胶囊:max-width 封顶(而非 width:max-content)——使内部 SelectionActions 的折叠流成为可收缩区,
   窄窗口时命令折入 ⋯ 菜单(§3.4 宽度约束链)。24px 让出左右各 12px 视觉呼吸。 */
.selection-toolbar {
  pointer-events: auto; /* Enable clicks on the toolbar itself */
  max-width: calc(100% - 24px);
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md) var(--spacing-sm) var(--spacing-sm);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  color: var(--color-text-primary);
}

.drag-handle {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
  cursor: grab;
  padding: 4px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}

.drag-handle:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.drag-handle:active {
  cursor: grabbing;
}

.selection-toolbar.is-dragging {
  transition: none; /* disable transition while dragging */
  box-shadow: var(--shadow-lg);
}

.slide-up-enter-active,
.slide-up-leave-active {
  transition:
    transform 0.3s cubic-bezier(0.16, 1, 0.3, 1),
    opacity 0.3s cubic-bezier(0.16, 1, 0.3, 1);
}
.slide-up-enter-from,
.slide-up-leave-to {
  transform: translateY(30px);
  opacity: 0;
}
</style>
