// src/composables/selection/types.ts
// Part5 T4a · 选区协议层类型 —— 把选区从「DOM 节点集合」改为「稳定 id 的判别联合」。
//
// 🔴 开发期不冻结契约（用户约束 no-contract-freeze-during-dev）：
//   本文件定义的 SelectionState / SelectionIntent / SelectionMode 外形均为「当前临时协议」，
//   开发期可随实测/反馈演进。两层结构（协议层 Intent / 策略层 Mode）的目的是让这类改动
//   影响面收敛、让多方案能并存比较，**不是**为了焊死其中一种语义。
//
// 设计依据：docs/refactor_2026/2026-06-30-Part5-选区契约与可插拔多模式设计.md §3/§5/§6。

// ── 选区状态（脱离 DOM 的根）─────────────────────────────────────────────
//
// 镜像后端 SelectionDescriptor 的两态：显式枚举 vs 全选语义。
// 消费方只通过下方抽象访问器读选区，**永不直接 for..of**，故不感知 kind 差异。
export type SelectionState =
  // 显式枚举：中小规模，直接持有 id 集
  | { readonly kind: 'explicit'; readonly ids: ReadonlySet<number> }
  // 全选语义：百万级不物化 id，只存「全选 + 排除集」标记 → Ctrl+A 内存恒定
  | { readonly kind: 'all'; readonly excluded: ReadonlySet<number> }

/** 空选区（显式态、零元素）。clear 意图归一到此值。 */
export const EMPTY_SELECTION: SelectionState = { kind: 'explicit', ids: new Set() }

// ── 抽象访问器（消费方唯一入口）─────────────────────────────────────────

/** 某 id 是否在选区内。explicit → ids.has；all → !excluded.has。 */
export function isSelected(state: SelectionState, id: number): boolean {
  return state.kind === 'explicit' ? state.ids.has(id) : !state.excluded.has(id)
}

/**
 * 选区基数。
 * @param totalViewCount 当前视图全集元素数 —— 仅 all 态需要（explicit 态忽略）。
 *   explicit → ids.size；all → totalViewCount - excluded.size（下界裁到 0，防排除集含越界 id）。
 */
export function selectionSize(state: SelectionState, totalViewCount: number): number {
  if (state.kind === 'explicit') return state.ids.size
  return Math.max(0, totalViewCount - state.excluded.size)
}

/**
 * 选区是否为空。
 * 注：all 态按构造恒非空（清空会归一为 explicit 空态），故 all 直接返回 false，无需 totalCount。
 */
export function isEmptySelection(state: SelectionState): boolean {
  return state.kind === 'explicit' && state.ids.size === 0
}

// ── 后端描述符出口（toDescriptor）───────────────────────────────────────
//
// R1-2（T4c）已定形：kind 取值对齐后端 serde tag 的**小驼峰变体名**（'explicit' / 'selectAll'），
// 由后端锁测试钉死（queries.rs::selection_descriptor_wire_format_locks_camel_case）。
// 泛型 V 保留 —— 协议层不依赖具体 ViewDescriptorDto，调用侧（useSelection）绑定实型。
export type SelectionDescriptor<V> =
  | { kind: 'explicit'; ids: number[] }
  | { kind: 'selectAll'; view: V; excludedIds: number[] }

/**
 * 把选区状态转为后端 SelectionDescriptor（批量命令的稳定出口）。
 * explicit → explicit{ids}；all → selectAll{view, excludedIds}。
 * @param view all 态所需的视图描述符，由调用侧（useViewDescriptor 装配）供给。
 */
export function toDescriptor<V>(state: SelectionState, view: V): SelectionDescriptor<V> {
  if (state.kind === 'explicit') {
    return { kind: 'explicit', ids: [...state.ids] }
  }
  return { kind: 'selectAll', view, excludedIds: [...state.excluded] }
}

// ── 抽象意图（协议层）───────────────────────────────────────────────────
//
// 物理手势经「手势 → 意图」映射(useSelection 调度层)归一到此封闭集，再交策略层解释。
// range 显式携带 anchorId：既服务 Shift+单击(anchor=lastClicked)，也服务框选(anchor=拖拽起点)。
export type SelectionIntent =
  | { type: 'replace'; id: number } // 单击：清空 + 选中单项
  | { type: 'toggle'; id: number } // Ctrl/Cmd+单击：翻转单项
  | { type: 'range'; anchorId: number; toId: number } // Shift+单击 / 框选：布局序区间并入
  | { type: 'selectAll' } // Ctrl/Cmd+A：全选语义标记（不物化）
  | { type: 'clear' } // Esc / 点空白：清空
  | { type: 'invert' } // 反选

// ── 策略层上下文与接口（可插拔多模式）────────────────────────────────────

/** 策略 apply 所需、不在 SelectionState 内的外部信息（由 useViewIds + 调用侧供给）。 */
export interface SelectionContext {
  /** 当前视图布局序全集 id（invert / 全集运算用）。 */
  readonly viewIds: readonly number[]
  /** 布局序区间 [anchor, to]（含端点），跨视口稳定；由 useViewIds.rangeBetween 提供。 */
  readonly rangeBetween: (anchorId: number, toId: number) => number[]
  /** 视图全集计数（all 态基数）。 */
  readonly totalCount: number
}

/**
 * 一个选择模式 = 把 SelectionIntent 解释为对 SelectionState 的**纯函数**变换。
 * 纯函数（旧态 + 意图 + 上下文 → 新态，无副作用）→ 易测、易换、可与其它模式并存比较。
 */
export interface SelectionMode {
  /** 唯一 id：'classic' | 'rubber-band' | 'lasso' | …（未来） */
  readonly id: string
  /** 设置页展示名 */
  readonly label: string
  apply(state: SelectionState, intent: SelectionIntent, ctx: SelectionContext): SelectionState
}
