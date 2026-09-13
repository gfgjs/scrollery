// CommandRegistry 贡献点范式的类型契约(设计文档 §3.4 / L3),以现有 ContextMenuItem
// ({ id, label, icon, action })为原型扩展。一套注册表被多处消费:上下文工具栏(L4)、右键
// 菜单、未来命令面板、统一 keymap。本文件只定「骨架」类型契约;registry.ts(注册/查询/求值/
// 执行)与 builtins/*(具体命令)属 Phase 2。

import type { Component } from 'vue'
import type { MediaType } from '../types/media'
import type { ActiveViewer } from '../stores/viewerStore'

/**
 * 命令所属分组。**group==='navigation' → 顶栏主图标按钮(点击直达);其余组一律进 ⋯ 溢出菜单**
 * (直接落地需求 2「常用直达、不常用进二级」)。非 navigation 组名用于组织溢出菜单的分区。
 *
 * 承接「开发期不冻结契约」:组集为候选,真机手感调优前不定死。
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
 * 只暴露谓词所需字段,不泄露 useSelection 内部实现。Phase 2 按实际谓词需要扩(如选中项 id 集)。
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
 * 命令执行/求值上下文 —— when/isEnabled/isActive/run 的**唯一入参**。只穿**可变调用上下文**
 * (view/activeViewer/selection/contextTarget),使命令成为可测纯函数。
 *
 * **环境服务(store)不入 context**:命令 run/when 内直接 `useXxxStore()` 取 pinia 单例(惯用法,
 * 且消费方 computed 内读 store 仍响应式追踪)。此前把 stores 塞进 context 会逼每次 ctx 构造实例化
 * 全部 store(含有 window 副作用的 uiStore),测试即炸——遂改直接访问(2026-07-08 P2 修正)。
 * 查看器类命令经 activeViewer.api 调用具体查看器动作。
 */
export interface CommandContext {
  view: ViewContext
  activeViewer: ActiveViewer | null
  selection: SelectionContext
  /** 上下文动作的目标资产;null=无特定目标(如网格工具栏的全局命令)。 */
  contextTarget: ContextTarget | null
}

/**
 * 命令贡献点。稳定 id 是自定义/插件持久化的锚(一经发布冻结,命名规范见 §4.4)。
 * title 支持惰性求值以适配 i18n(locale 切换后须重算,函数形式让渲染层每次拿当前语言串)。
 */
export interface Command {
  /** 稳定唯一 id(命名规范 `<域>[.<子域>].<动作>`,子域按需:泛域动作两段如 'viewer.zoomIn' /
   *  'grid.undo',真属子域才加段如 'viewer.reader.toc' / 'viewer.audio.togglePlay'。§4.4,
   *  2026-07-10 修订;开发期可重命名,发布后冻结)。 */
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
  /** 快捷键(如 'mod+z');tooltip 显示与实际监听共用此常量(P5-6 键位同源)。组合键以 '+' 连接,
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
