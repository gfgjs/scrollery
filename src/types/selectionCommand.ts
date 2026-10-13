// 选区批量动作的数据描述子；同一命令集供工具条与溢出菜单渲染。
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
