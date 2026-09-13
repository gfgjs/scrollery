// src/types/selectionCommand.ts
// 选区批量动作的数据驱动描述子（SelectionBar 合并/分离一键切换方案 C1）。
//
// 背景:SelectionToolbar 原本以 9 条写死的 @emit 暴露动作,折叠(收窄时溢出到 ⋯ 菜单)要求
// 同一组动作既能作为条上按钮、又能作为菜单里的「图标+文字标签」行再渲染一遍 → 动作必须是
// 数据而非模板按钮。MediaGrid 组装本数组(handler=既有函数引用,零迁移),经单一 :commands
// prop 传入 SelectionToolbar/SelectionActions,tooltip 文案/图标两处内联收敛单源。
import type { Component } from 'vue'

/** 单条选区批量动作。 */
export interface SelectionCommand {
  /** 稳定键,用作 v-for key 与折叠测量项标识。 */
  key: string
  /** lucide 图标组件(colors 单元用作折叠后菜单行的前导图标)。 */
  icon: Component
  /** i18n key;tooltip、溢出菜单行文字标签共用单源。 */
  labelKey: string
  /** 危险动作(删除)→ 红色按钮 / 菜单红行。 */
  danger?: boolean
  /**
   * 渲染形态。'button'(默认)=常规圆形动作按钮;'colors'=整块 ColorLabelPicker 作为一个
   * 折叠单元(点色块即设色,run 传入色值,0=清除)。
   */
  kind?: 'button' | 'colors'
  /** 组首:其前渲染 divider,并与之同属一个折叠单元(divider 不独立成测量项)。 */
  groupStart?: boolean
  /** 执行动作;colors 单元传入色值(0=清除),其余无参调用。 */
  run: (value?: number) => void
}
