<script setup lang="ts">
// UiDialog（S1 原语）：全库模态对话框的唯一实现。
// 此前 7 个对话框各自逐字重复 ~120 行骨架,且 CloseConfirmDialog 漏用 Teleport(原位渲染,
// z-index 可被祖先层叠上下文裁剪)。本原语收敛「模态外壳 + Teleport + 焦点陷阱接线 +
// Escape/点遮罩关闭」为单一可测源:外壳走全局 .dialog-overlay/.dialog-content 基座
// + 本地 scoped 补齐 header/body/footer(全局基座见 index.css A2 层);焦点陷阱复用既有 useFocusTrap
// (初始焦点目标 data-autofocus 落在消费方插槽,querySelector 跨插槽命中——见 useFocusTrap:70)。
import { ref } from 'vue'
import { X } from '@lucide/vue'
import { useFocusTrap } from '../../composables/useFocusTrap'

const props = withDefaults(
  defineProps<{
    open: boolean // 开阖(v-if 存在性切换,驱动焦点陷阱 engage/release)
    title: string // 标题栏文本(渲染进 <h2>)
    closeLabel?: string // 右上角关闭键的 title 提示;缺省 'Close'，真实使用应传本地化文案
    showClose?: boolean // 是否渲染右上角关闭键(默认 true)
    maxWidth?: string // content 最大宽度特化(如 '460px');缺省走全局基座的 420px
    bodyPadding?: string // 正文内边距特化(如 '0');列表/树/表格类对话框需满幅时传 '0',缺省走 scoped 的 spacing-lg
    maxHeight?: string // content 最大高度特化(如 '84vh');设则 content 加 --capped 类,正文 flex 撑满并可滚、头/脚固定
    closeOnOverlay?: boolean // 点遮罩是否关闭(默认 true);首启向导等强制显式关闭的模态传 false
    closeOnEsc?: boolean // 按 Escape 是否关闭(默认 true);同上
    floatSurface?: boolean // 显式标记 float 材质修饰类;基座默认已走同一 recipe,保留既有 API
  }>(),
  { showClose: true, closeLabel: 'Close', closeOnOverlay: true, closeOnEsc: true },
)

const emit = defineEmits<{
  (e: 'close'): void // 点遮罩 / Escape / 关闭键触发,由消费方决定关闭语义(取消 vs 副作用)
}>()

// 遮罩点击 / Escape 的关闭经开关门控:非可关闭模态(如首启向导)置 false 只留显式按钮出口。
// 关闭键(X)不受此门控——它本身即显式关闭入口。
function onOverlayClick() {
  if (props.closeOnOverlay) emit('close')
}
function onEsc() {
  if (props.closeOnEsc) emit('close')
}

// 焦点陷阱:与旧各对话框同款 useFocusTrap,overlay ref 由本原语持有;open 取值器驱动 engage/release。
const overlayEl = ref<HTMLElement | null>(null)
useFocusTrap(overlayEl, () => props.open)
</script>

<template>
  <!-- 恒 Teleport 到 body:统一脱离组件原位的层叠上下文,根治「漏 Teleport 被祖先裁剪」类隐患。 -->
  <Teleport to="body">
    <!-- 只补关闭(leave)动画:enter 不接管——外壳既有挂载 keyframes(index.css dialog-fade-in/
         dialog-slide-up,绑在下方两个元素的 class 上)继续生效,避免与 Vue 过渡类双重动画。 -->
    <Transition name="ui-dialog">
      <div
        v-if="open"
        ref="overlayEl"
        class="dialog-overlay"
        tabindex="-1"
        @click.self="onOverlayClick"
        @keydown.esc.stop="onEsc"
      >
        <div
          class="dialog-content"
          :class="{
            'dialog-content--capped': maxHeight !== undefined,
            'dialog-content--float': floatSurface,
          }"
          :style="[maxWidth ? { maxWidth } : {}, maxHeight ? { maxHeight } : {}]"
        >
          <slot name="header">
            <header class="dialog-header">
              <h2 class="dialog-title">{{ title }}</h2>
              <button
                v-if="showClose"
                type="button"
                class="btn-close"
                :title="closeLabel"
                @click="emit('close')"
              >
                <X :size="18" />
              </button>
            </header>
          </slot>

          <main
            class="dialog-body"
            :style="bodyPadding !== undefined ? { padding: bodyPadding } : undefined"
          >
            <slot />
          </main>

          <!-- 页脚仅在消费方提供 #footer 时渲染,避免空 border-top 分隔线。 -->
          <footer v-if="$slots.footer" class="dialog-footer">
            <slot name="footer" />
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* 外壳 .dialog-overlay/.dialog-content 的定位/遮罩/尺寸/动画由全局基座(index.css A2 层)提供;
   此处仅补齐全局尚未覆盖的 header/title/关闭键/body/footer——它们此前在各对话框 scoped 里重复。 */
.dialog-overlay {
  background: var(--color-bg-overlay);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
}

.dialog-content {
  background-color: var(--color-bg-elevated);
  border-color: var(--color-border-strong);
  border-radius: var(--radius-xl);
  box-shadow: var(--theme-dialog-shadow);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  animation: ui-dialog-slide var(--duration-normal) var(--ease-out);
}

/* float 配方只用位移动画，避免 scale 造成文字与布局抖动。 */
@keyframes ui-dialog-slide {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.ui-dialog-leave-active .dialog-content {
  animation: ui-dialog-slide var(--duration-fast) ease-in reverse;
}

.dialog-header {
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border-strong);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.dialog-title {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}

.btn-close {
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  /* 显式列举过渡属性(勿 transition:all,S1 机械批) */
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast),
    box-shadow var(--transition-fast);
}
.btn-close:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.btn-close:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.dialog-body {
  padding: var(--spacing-lg);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.dialog-footer {
  padding: var(--spacing-md) var(--spacing-lg);
  border-top: 1px solid var(--color-border-strong);
  background: transparent;
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
}

/* 显式 float 标记保留给既有消费方；基座默认已采用同一材质，避免新旧调用方出现两套表面。 */
.dialog-content--float {
  background-color: var(--color-bg-elevated);
  border-color: var(--color-border-strong);
  box-shadow: var(--theme-dialog-shadow);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
}
.dialog-content--float .dialog-footer {
  background: transparent;
}

/* maxHeight 特化(仅传 maxHeight 的对话框走此路,其余对话框 .dialog-body 布局零变化):content 封顶高度后,
   正文成为唯一滚动区(flex 撑满 + min-height:0 允许收缩 + 溢出滚),头/脚 flex-shrink:0 固定不滚。
   自定义头部(#header 插槽)由消费方自行加 flex-shrink:0——它是消费方元素,本原语选择器够不着。 */
.dialog-content--capped .dialog-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.dialog-content--capped .dialog-header,
.dialog-content--capped .dialog-footer {
  flex-shrink: 0;
}
</style>
