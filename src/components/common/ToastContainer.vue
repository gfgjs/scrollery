<template>
  <Teleport to="body">
    <!-- toast 是全应用异步操作的完成/失败通知汇聚点。 -->
    <div class="toast-container">
      <TransitionGroup name="toast">
        <div
          v-for="toast in toastStore.toasts"
          :key="toast.id"
          class="toast"
          :class="`toast--${toast.type}`"
        >
          <div class="toast__row">
            <component :is="iconMap[toast.type]" :size="16" class="toast__icon" />
            <span class="toast__msg">{{ toast.message }}</span>
            <!-- 关闭是真按钮:此前是裸 svg 挂 click,键盘不可达。 -->
            <button
              type="button"
              class="toast__close"

              @click="toastStore.removeToast(toast.id)"
            >
              <X :size="14" />
            </button>
          </div>
          <!-- 交互式快捷 chips（如「加入收藏夹」），点击后执行并关闭该 toast -->
          <div v-if="toast.actions?.length" class="toast__actions">
            <button
              v-for="(action, i) in toast.actions"
              :key="i"
              class="toast__chip"
              @click="onAction(toast.id, action)"
            >
              {{ action.label }}
            </button>
          </div>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { useToastStore } from '../../stores/toastStore'
import { Check, X, AlertTriangle, Info } from '@lucide/vue'
import type { Component } from 'vue'
import type { ToastAction } from '../../types/ui'

const toastStore = useToastStore()
const iconMap: Record<string, Component> = {
  success: Check,
  error: X,
  warning: AlertTriangle,
  info: Info,
}

// 执行 chip 动作后关闭该 toast。
async function onAction(toastId: string, action: ToastAction) {
  try {
    await action.onClick()
  } finally {
    toastStore.removeToast(toastId)
  }
}
</script>

<style scoped>
.toast-container {
  position: fixed;
  bottom: 48px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  z-index: 99999;
  pointer-events: none;
}
.toast {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  border-radius: var(--radius-xl);
  font-size: var(--font-size-sm);
  font-weight: 500;
  pointer-events: auto;
  cursor: default;
  background: var(--material-recipe-float-background-color);
  border: 1px solid var(--material-recipe-float-border-color);
  backdrop-filter: var(--material-recipe-float-backdrop-filter);
  -webkit-backdrop-filter: var(--material-recipe-float-backdrop-filter);
  box-shadow: var(--material-recipe-float-box-shadow);
  max-width: 460px;
  user-select: text;
}
.toast__row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.toast__actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-xs);
  padding-left: 24px; /* 与消息对齐，让过图标 */
}
.toast__chip {
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-full);
  font-size: var(--font-size-xs);
  font-weight: 600;
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
  border: 1px solid var(--color-border);
  cursor: pointer;
  transition: background var(--transition-fast);
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.toast__chip:hover {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
.toast__msg {
  flex-grow: 1;
  word-break: break-all;
}
.toast__close {
  display: inline-flex;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  padding: 0; /* 压掉 UA button 默认内边距(全局 reset 未清 padding) */
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  cursor: pointer;
  opacity: 0.7;
  transition: opacity var(--transition-normal);
  flex-shrink: 0;
  margin-left: 8px;
}
.toast__close:hover {
  background: var(--color-bg-hover);
  opacity: 1;
}
/* Toast 统一使用浮层材质；状态只负责边线与图标，避免高饱和色块夺走内容焦点。 */
.toast--success {
  border-inline-start: 3px solid var(--color-success);
}
.toast--error {
  border-inline-start: 3px solid var(--color-error);
}
.toast--warning {
  border-inline-start: 3px solid var(--color-warning);
}
.toast--info {
  border-inline-start: 3px solid var(--color-info);
}
.toast--success .toast__icon { color: var(--color-success); }
.toast--error .toast__icon { color: var(--color-error); }
.toast--warning .toast__icon { color: var(--color-warning); }
.toast--info .toast__icon { color: var(--color-info); }

.toast-enter-from {
  opacity: 0;
  transform: translateY(12px);
}
.toast-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}
.toast-enter-active,
.toast-leave-active {
  /* 显式列举(勿 transition:all):进出场只动 opacity/transform 两个合成器友好属性。 */
  transition:
    opacity 200ms ease,
    transform 200ms ease;
}
</style>
