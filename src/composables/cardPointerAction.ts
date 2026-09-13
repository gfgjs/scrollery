// src/composables/cardPointerAction.ts
// 画廊卡片 pointerdown / click 的分流决策(纯函数,便于穷举单测)。
//
// 交互路由是回归高发区——右键拖拽曾因「拖到文件夹一律要求命中手柄」被误伤消失(真机反馈)。
// 故把按钮×是否已选中格×是否命中手柄的判定抽为纯函数并穷举钉死,防后续"简化"条件时再度回归。
//
// 语义:
//  - 左键(button 0):按在「已选中格的手柄」→ drag(拖到文件夹);否则(本体/未选中)→ sweep(反转扫选)。
//  - 右键(button 2):已选中格「整卡(手柄或本体)」→ drag(落点弹移动/复制菜单,保持旧行为);
//    右键单击未越阈由上层不拦、交给常规右键菜单;未选中格右键 → none。
//  - 其余按钮(如中键)不满足 drag 且非左键 → none。

/** 卡片 pointerdown 的三种去向。 */
export type CardPointerAction = 'drag' | 'sweep' | 'none'

/**
 * 依「按钮 / 是否已选中格 / 是否命中手柄」决定 pointerdown 去向。
 * @param button PointerEvent.button(0=左,2=右)。
 * @param onSelectedItem 是否按在「选择模式下的已选中格」。
 * @param onHandle 是否命中左上拖拽手柄。
 */
export function resolveCardPointerAction(
  button: number,
  onSelectedItem: boolean,
  onHandle: boolean,
): CardPointerAction {
  // 左键需命中手柄(本体左键=反转扫选);右键整卡皆可拖(本体右键拖=落点菜单,保持旧行为)。
  const canDrag = onSelectedItem && ((button === 0 && onHandle) || button === 2)
  if (canDrag) return 'drag'
  // 左键未命中拖拽 → 反转扫选;右键(及其他按钮)未命中拖拽 → none(交给常规右键菜单/不处理)。
  if (button === 0) return 'sweep'
  return 'none'
}

/** 卡片 click(含键盘 Enter/Space 激活)的四种去向。 */
export type CardClickAction = 'toggle' | 'range' | 'select' | 'open'

/** resolveCardClickAction 的输入:事件修饰键 × 选择态 × 布局形态 × 镜头态。 */
export interface CardClickInput {
  /** 重复镜头激活(§8.1 browse-only)。由宿主从 duplicateLensStore.isLensActive 注入。 */
  lensActive: boolean
  /** Ctrl/Cmd 按下(普通点击 = 切换选中)。 */
  modifier: boolean
  /** Shift 按下(选择态下 = 范围选中)。 */
  shift: boolean
  /** 当前是否处于选择模式。 */
  selectionMode: boolean
  /** 极密网格(compact,<100px):无逐格 checkbox,选择态里普通点击直接切换选中。 */
  compact: boolean
}

/**
 * 依「修饰键 × 选择态 × 布局形态 × 镜头态」决定卡片 click 去向(自 MediaGrid.handleCardClick
 * 的内联分流抽出,判定顺序逐字保留)。
 *
 * 语义:
 *  - §8.1 browse-only:重复镜头是明确浏览态——单击/修饰键单击**一律打开查看器**,不进入选择、
 *    不触发范围选择(Ctrl/Cmd、Shift 在镜头中被显式忽略)。
 *  - Ctrl/Cmd+单击:切换选中。
 *  - 选择态 + Shift+单击:范围选中(锚点缺失由 selectRange 兜底)。
 *  - 选择态 + compact:普通点击切换选中(T0:60px 尺度点不动 checkbox,点击即选)。
 *  - 其余:打开查看器。
 */
export function resolveCardClickAction(i: CardClickInput): CardClickAction {
  if (i.lensActive) return 'open'
  if (i.modifier) return 'toggle'
  if (i.selectionMode && i.shift) return 'range'
  if (i.selectionMode && i.compact) return 'select'
  return 'open'
}
