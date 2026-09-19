<script setup lang="ts">
// UiPopover（S1 原语）：全库锚定弹层的唯一定位实现。
// 此前各弹层各自手写 getBoundingClientRect + 手算 top/left,水平钳制/垂直 flip 各处漏配不一;
// 定位数学委托 @floating-ui/vue(Popper 继任者),本原语只持有项目契约:Teleport 逃逸祖先
// overflow 裁切 + 焦点陷阱 + backdrop/Esc dismiss。
// 与 UiDialog 的差异:锚定定位(非居中)、非 modal、dismiss 走 backdrop+Esc(而非遮罩)。
// 全库无 .popover 全局基座类,本原语自带定位外壳与默认 float 表面;已有 slot 可继续自持尺寸/内边距。
import { computed, ref } from 'vue'
import { useFloating, autoUpdate, offset, flip, shift, type Placement } from '@floating-ui/vue'
import { useFocusTrap } from '../../composables/useFocusTrap'

const props = withDefaults(
  defineProps<{
    open: boolean // 开阖(v-if 存在性切换，驱动焦点陷阱 + autoUpdate 生命周期)
    anchor: HTMLElement | null // 定位锚点=触发元素的根 DOM(UiIconButton 经 defineExpose({el}) 提供)
    placement?: Placement // 放置方位，默认 'bottom-start'
    offsetPx?: number // 与锚点主轴间距，默认 6(沿用旧 positionMenu 的 rect.bottom + 6)
    trapFocus?: boolean // 是否启用焦点陷阱，默认 true(open 期圈焦点、关闭归还锚点)
  }>(),
  { placement: 'bottom-start', offsetPx: 6, trapFocus: true },
)

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
}>()

const anchorRef = computed(() => props.anchor)
const floatingEl = ref<HTMLElement | null>(null)

// @floating-ui 引擎:strategy 'fixed' 使 top/left 相对视口(与 Teleport-to-body + autoUpdate 契合)。
// middleware 顺序:offset(主轴间距) → flip(放不下则翻面) → shift(沿轴滑动留在视口内,padding 为视口留边)。
// shift 一举替代旧 positionMenu 的手写水平钳制,并修掉 date 弹层漏钳制的越界 bug。
// whileElementsMounted: autoUpdate 仅在两元素都挂载时(客户端)运行,使滚动/resize/尺寸变化自动重定位,
// 替代旧「resize 即关闭再重开」的降级处理。
// transform:false → 用 top/left 定位,把 transform 让给入场动画的位移(否则两者互相覆盖,
// 旧版 translateY(-4px) 从未真正生效);并配合 isPositioned 就位前隐藏,消除首帧左上角闪现再跳到锚点的观感。
const { floatingStyles, placement, isPositioned } = useFloating(anchorRef, floatingEl, {
  placement: computed(() => props.placement),
  strategy: 'fixed',
  transform: false,
  middleware: computed(() => [offset(props.offsetPx), flip(), shift({ padding: 8 })]),
  whileElementsMounted: autoUpdate,
})

/** 抽屉拉出距离(px)：入场时弹层自锚点侧偏移此距离滑到位；越大越有「被拉开」的行程感。 */
const DRAWER_PULL_PX = 24

// 抽屉推拉动画的入场偏移(用户定:纯位移滑出,去 scale):弹层从贴着锚点那一侧偏移 DRAWER_PULL_PX
// 滑出到位,方向恒「从锚点朝外」——上弹(top,菜单在按钮上方)自下方滑入、下弹(bottom)自上方滑入、
// 左/右弹同理横向;据**解析后**的 placement 取值(flip 可能翻转 top↔bottom,偏移须跟着翻)。
// 经 CSS 变量注入供 scoped 过渡类取用。scale 一去,transform-origin 对纯 translate 无作用,随之删除。
const popOffset = computed<{ x: string; y: string }>(() => {
  const side = placement.value.split('-')[0]
  if (side === 'top') return { x: '0px', y: `${DRAWER_PULL_PX}px` }
  if (side === 'bottom') return { x: '0px', y: `-${DRAWER_PULL_PX}px` }
  if (side === 'left') return { x: `${DRAWER_PULL_PX}px`, y: '0px' }
  if (side === 'right') return { x: `-${DRAWER_PULL_PX}px`, y: '0px' }
  return { x: '0px', y: '0px' }
})

// 定位样式 + 抽屉入场偏移 + 就位前隐藏：isPositioned=false 时(SSR / 客户端首帧定位未算出)
// top/left 尚为 0，先以 visibility:hidden 藏起避免左上角闪现；@floating-ui 算出位置后转可见，
// 与入场动画同帧衔接。
const popoverStyle = computed(() => ({
  ...floatingStyles.value,
  '--ui-popover-pop-x': popOffset.value.x,
  '--ui-popover-pop-y': popOffset.value.y,
  visibility: isPositioned.value ? undefined : ('hidden' as const),
}))

// 焦点陷阱：复用既有 useFocusTrap(挂载即开范式，含 SSR 守卫)。trapFocus=false 时取值器恒 false=禁用。
useFocusTrap(floatingEl, () => props.trapFocus && props.open)

function requestClose() {
  emit('update:open', false)
}
</script>

<template>
  <!-- 恒 Teleport 到 body：逃逸 .toolbar__filters 等祖先的 overflow 裁切 + 统一层叠上下文根治漏 Teleport。 -->
  <Teleport to="body">
    <!-- 透明 backdrop：铺满视口拦截外部点击即 dismiss；inset 让出标题栏高度(--titlebar-height)，
         使自绘标题栏的窗口三键在弹层开启时仍可一次点击(沿用旧 date/filter 弹层同款几何)。 -->
    <div v-if="open" class="ui-popover__backdrop" @click="requestClose" />
    <!-- 定位壳的 v-if 挂在 Transition 直接子上以驱动 enter/leave 动画(而非由祖先 v-if 整树拆除)。 -->
    <Transition name="ui-popover-fade" appear>
      <div
        v-if="open"
        ref="floatingEl"
        class="ui-popover"
        tabindex="-1"
        :style="popoverStyle"
        @keydown.esc.stop="requestClose"
      >
        <slot />
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* backdrop：透明拦截层，inset 让出标题栏高度(与旧 date-popover-backdrop 一致，保窗口三键可点)。 */
.ui-popover__backdrop {
  position: fixed;
  inset: var(--titlebar-height) 0 0 0;
  z-index: 300;
}
/* 定位壳：fixed 定位的 top/left/transform 由 @floating-ui 的 floatingStyles 注入(内联 style)；
   默认表面走 material float recipe。无内边距是刻意的：已有 slot 内容仍可保留自身布局与尺寸契约。 */
.ui-popover {
  z-index: calc(var(--z-toast) + 1);
  max-inline-size: var(--popover-max-inline-size);
  background-color: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  color: var(--color-text-primary);
}
/* 弹层进出过渡:抽屉推拉——自贴锚点那一侧平移拉出,无缩放,依赖 useFloating 的 transform:false。
   缓动用 expo-out:起步快、收尾稳,正是抽屉被拉开的手感。Teleport 产物仍带本组件 data-v 作用域属性,
   故 scoped 过渡类可命中。过渡只作用于本壳(backdrop 无需渐变)。 */
.ui-popover-fade-enter-active {
  transition:
    opacity 0.16s ease,
    transform 0.22s cubic-bezier(0.16, 1, 0.3, 1);
}
.ui-popover-fade-leave-active {
  transition:
    opacity 0.12s ease,
    transform 0.14s ease;
}
/* 抽屉起止态：自锚点侧偏移 --ui-popover-pop-x/y(由 popoverStyle 按解析后 placement 注入，含正负与轴向)
   平移到位。无 scale——用户真机后定为纯位移滑出(前身是 scale 0.95 的卡片弹出)。 */
.ui-popover-fade-enter-from,
.ui-popover-fade-leave-to {
  opacity: 0;
  transform: translate(var(--ui-popover-pop-x, 0), var(--ui-popover-pop-y, 0));
}
</style>
