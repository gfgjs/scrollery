// src/composables/useConfirm.ts
// 基于 Promise 的确认对话框单例（取代各组件内联弹窗）。
//
// 任意组件调用 `useConfirm().confirm(opts)` 并 await 结果；仅挂载一次的
// <ConfirmDialog>（在 AppSidebar 中）渲染共享状态。这使同级区块（文件夹 / 管理）
// 无需层层传 prop 或重复弹窗标记即可共用一个对话框。

import { reactive } from 'vue'
import i18n from '../i18n'

export interface ConfirmOptions {
  title: string
  message: string
  confirmText?: string
  cancelText?: string
  /** 显示额外复选框（如「同时清除缩略图」）。 */
  showCheckbox?: boolean
  checkboxLabel?: string
  /** 复选框初始值。 */
  checkboxValue?: boolean
  /** 危险操作:确认按钮用 danger(红色)变体,用于不可恢复的破坏性操作。 */
  danger?: boolean
  /** 输入确认:非空时须原样键入该文本才启用确认按钮——反射式点击无法穿透(用于极度危险操作)。 */
  requireText?: string
}

export interface ConfirmResult {
  /** 用户确认为 true，取消为 false。 */
  confirmed: boolean
  /** 复选框最终值（仅在 showCheckbox 时有意义）。 */
  checkboxValue: boolean
}

interface ConfirmState extends Required<ConfirmOptions> {
  isOpen: boolean
  resolve: ((r: ConfirmResult) => void) | null
}

// 模块级单例，被每个 useConfirm() 调用方与对话框共享。
// confirmText/cancelText 初始为空串而非 t()：模块顶层调 t 会把语言冻结在 import 时刻，
// 且 confirm() 打开对话框前必然覆写这两个值，空串永远不会被渲染。
const state = reactive<ConfirmState>({
  isOpen: false,
  title: '',
  message: '',
  confirmText: '',
  cancelText: '',
  showCheckbox: false,
  checkboxLabel: '',
  checkboxValue: true,
  danger: false,
  requireText: '',
  resolve: null,
})

export function useConfirm() {
  /** 打开对话框，用户确认或取消后解析。 */
  function confirm(opts: ConfirmOptions): Promise<ConfirmResult> {
    // 打开新对话框前先结算未完成的旧对话框。
    state.resolve?.({ confirmed: false, checkboxValue: state.checkboxValue })
    return new Promise<ConfirmResult>((resolve) => {
      state.isOpen = true
      state.title = opts.title
      state.message = opts.message
      state.confirmText = opts.confirmText ?? i18n.global.t('common.confirm')
      state.cancelText = opts.cancelText ?? i18n.global.t('common.cancel')
      state.showCheckbox = opts.showCheckbox ?? false
      state.checkboxLabel = opts.checkboxLabel ?? ''
      state.checkboxValue = opts.checkboxValue ?? true
      state.danger = opts.danger ?? false
      state.requireText = opts.requireText ?? ''
      state.resolve = resolve
    })
  }
  return { confirm }
}

/** 仅供 <ConfirmDialog> 使用的内部访问器。 */
export function useConfirmDialogState() {
  function close(confirmed: boolean) {
    state.resolve?.({ confirmed, checkboxValue: state.checkboxValue })
    state.resolve = null
    state.isOpen = false
  }
  return { state, close }
}
