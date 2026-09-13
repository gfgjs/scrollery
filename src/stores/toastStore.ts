// src/stores/toastStore.ts
// Toast 通知队列 —— 从 uiStore 上帝 store 拆出的独立域(P1-21 渐进拆分第一刀)。
// 全应用唯一的瞬时提示来源:任意异步操作的成功/失败反馈经 addToast 入队,ToastContainer 渲染,
// 到期(duration + 300ms 退场动画余量)自动 removeToast 出队。状态零耦合其他 UI 域,故独立成 store。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ToastMessage, ToastAction } from '../types/ui'

export const useToastStore = defineStore('toast', () => {
  // ── 提示框队列 ─────────────────────────────────────────────────────────
  const toasts = ref<ToastMessage[]>([])
  // 单调自增序号,保证 id 唯一(即便同一毫秒连发多条);模块级闭包变量,不需响应式。
  let toastSeq = 0

  /**
   * 入队一条 toast,到期自动出队。
   * @param type 语义类型(success/error/warning/info),决定配色与图标
   * @param message 已本地化的展示文本(调用方负责 i18n)
   * @param duration 展示时长(ms),默认 3000;实际保留 duration+300ms 以容纳退场动画
   * @param actions 可选的行动按钮(如「撤销」),点击回调由调用方提供
   */
  function addToast(
    type: ToastMessage['type'],
    message: string,
    duration = 3000,
    actions?: ToastAction[],
  ) {
    const id = `toast-${++toastSeq}`
    toasts.value.push({ id, type, message, duration, actions })
    setTimeout(() => removeToast(id), duration + 300)
  }

  /** 按 id 移除一条 toast(到期自动调用,或用户点关闭按钮手动调用)。 */
  function removeToast(id: string) {
    const idx = toasts.value.findIndex((t) => t.id === id)
    if (idx >= 0) toasts.value.splice(idx, 1)
  }

  return { toasts, addToast, removeToast }
})
