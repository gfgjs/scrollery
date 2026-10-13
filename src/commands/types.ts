// 命令类型供注册表、上下文工具栏、右键菜单与快捷键分发共用。

import type { Component } from 'vue'
import type { MediaType } from '../types/media'
import type { ActiveViewer } from '../stores/viewerStore'

/**
 * navigation 组显示为顶栏主按钮；其余组在溢出菜单中分区。
 */
export type CommandGroup =
  | 'navigation' // 顶栏主图标按钮(直达)
  | 'view' // 查看/缩放/显示切换(网格布局、图像缩放模式…)
  | 'organize' // 数据类:收藏/评分/颜色标签/删除/移动/复制
  | 'reader' // 阅读器专属:TOC/搜索/书签/排版/主题…
  | 'playback' // 音视频播放控制
  | 'window' // 窗口三键(最小化/最大化/关闭)
  | 'overflow' // 显式杂项(不成组的低频项)

/** 当前顶层视图种类。activeViewer 非空时以其 kind 细分;'grid' = 网格主视图。 */
export type ViewContext = 'grid' | 'viewer'

/**
 * 选区快照 —— 命令 when/isEnabled 谓词读取的最小面。网格多选上下文;查看器内通常单资产。
 * 只暴露谓词所需字段，不泄露 useSelection 内部实现。
 */
export interface SelectionContext {
  /** 已选数量(0 = 无选区)。 */
  count: number
  /** 是否有且仅有一个选中项(许多命令仅单选可用)。 */
  isSingle: boolean
}

/**
 * 上下文动作(右键菜单 / 单项操作)的目标资产 —— 区别于 selection(多选集)。右键点某卡片时
 * 该卡片即目标(未必在选区内);查看器内即当前打开资产。when 谓词读 mediaType(如壁纸仅图片),
 * run 读 id。
 */
export interface ContextTarget {
  id: number
  /** 目标媒体类型;可能未知(如目标项查找失败的降级)。when 谓词用 `?.mediaType === 'image'`
   *  这类可选判定,使「有 id 即显示的通用命令」与「需已知类型的条件命令(壁纸)」各得其所。 */
  mediaType?: MediaType
}

/**
 * 命令执行与求值上下文。store 不放入 context：命令按需读取 store，避免构造上下文时
 * 实例化无关 store；查看器命令通过 activeViewer.api 调用当前查看器。
 */
export interface CommandContext {
  view: ViewContext
  activeViewer: ActiveViewer | null
  selection: SelectionContext
  /** 上下文动作的目标资产;null=无特定目标(如网格工具栏的全局命令)。 */
  contextTarget: ContextTarget | null
}

/**
 * 命令贡献点。稳定 id 是持久化引用的锚。
 * title 支持惰性求值以适配 i18n(locale 切换后须重算,函数形式让渲染层每次拿当前语言串)。
 */
export interface Command {
  /** 稳定唯一 id：`<域>[.<子域>].<动作>`；发布后不得重命名。 */
  id: string
  /** 显示标题;函数形式支持 i18n 惰性求值。 */
  title: string | (() => string)
  /** lucide 图标组件(注册时须 markRaw,避免 Vue 把组件当响应式对象代理)。 */
  icon?: Component
  /** 所属分组;navigation=顶栏主按钮,其余进溢出。 */
  group: CommandGroup
  /** 组内排序(升序;缺省按注册顺序)。 */
  order?: number
  /** 显隐谓词:返回 false 则该命令在当前上下文不出现(基于 activeViewer/选区/视图)。 */
  when?: (ctx: CommandContext) => boolean
  /** 可用谓词:返回 false 则显示但置灰(区别于 when 的彻底隐藏)。 */
  isEnabled?: (ctx: CommandContext) => boolean
  /** 高亮态谓词:如已收藏 / 信息面板已开,用于按钮 active 视觉。 */
  isActive?: (ctx: CommandContext) => boolean
  /** 快捷键(如 'mod+z');tooltip 显示与实际监听共用此常量。组合键以 '+' 连接,
   *  修饰符规范序 mod→shift→alt(与 eventToCombo 产出一致,乱序不命中)。 */
  keybinding?: string
  /** 隐藏别名键位:分发器同样匹配、但不进 tooltip(如 redo 的 'mod+shift+z' 惯用别名)。 */
  keybindingAliases?: string[]
  /**
   * 忽略键盘自动重复(KeyboardEvent.repeat):按住键时只在**首发**执行一次。
   * 缺省 false = 保持既有语义(按住 mod+z 连续撤销是**想要**的行为,勿一刀切)。
   * 只有「按住会把状态反复翻转」的开关类命令需要置真——如 F11 全屏:重复键每 ~30ms 一发、
   * 而窗口转换约需 ~100ms,不挡则按住 F11 会让窗口全屏/退出来回抽搐(2026-07-16)。
   */
  ignoreKeyRepeat?: boolean
  /** 执行体。数据类调 stores.*.action;查看器类调 ctx.activeViewer.api.*。 */
  run: (ctx: CommandContext) => void | Promise<void>
}
