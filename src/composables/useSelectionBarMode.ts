// src/composables/useSelectionBarMode.ts
// 选区操作条「分离(浮动) / 合并(docked)」形态偏好 + 拖拽位置——模块级响应式单例 + localStorage 持久化。
//
// 分离(默认):浮动可拖胶囊(拖拽位置持久化);合并(docked):动作条 Teleport 进 AppStatusBar,替换式
//   共存(28px 恒高、不可拖)。默认分离=零惊讶(浮动可拖是现状且被裁决为核心能力,docked 是新增选项)。
//   与 useTitlebarMode 默认合并的不对称是有意的(各自被推翻/保留的「原设计」不同)。
// 状态经本模块级单例共享(SelectionToolbar 形态分支 + AppStatusBar 让位判断 + 设置页开关同源),localStorage 持久。
import { ref } from 'vue'
import type { BarAlign } from './useToolbarAlign'

const DOCKED_KEY = 'selection_bar_docked'
const OFFSET_KEY = 'selection_bar_offset'
const ALIGN_KEY = 'selection_bar_align'

/** 胶囊底边距(与 .selection-toolbar-wrapper 的 bottom:32px 同源;竖向钳制以此为基线)。 */
const BOTTOM_INSET = 32
/** 钳制留边:恢复/resize 时保证胶囊四缘各留此呼吸,拖拽手柄恒可达。 */
const CLAMP_MARGIN = 8
/** 左/右对齐时胶囊距边界的视觉内缩(与 .selection-toolbar-wrapper 的 padding-inline:12px 同源);
 *  clampOffset 据此算左/右对齐的静止基位 baseLeft。居中对齐对称留白,不受此值影响。 */
export const SELECTION_BAR_SIDE_INSET = 12

export interface SelectionBarOffset {
  x: number
  y: number
}

function readDocked(): boolean {
  try {
    // 缺省/非法一律回落 false(分离=现状,零惊讶);仅显式 '1' 走 docked。
    return localStorage.getItem(DOCKED_KEY) === '1'
  } catch {
    return false
  }
}

/**
 * 解析 localStorage 里的 offset 串:非法/残缺/非有限数/解析失败一律回落原点(相对居中基线)。
 * 纯函数(不触 localStorage),便于 spec 穷举各类残缺输入。
 */
export function parseStoredOffset(raw: string | null): SelectionBarOffset {
  if (!raw) return { x: 0, y: 0 }
  try {
    const parsed = JSON.parse(raw)
    if (
      parsed &&
      typeof parsed.x === 'number' &&
      typeof parsed.y === 'number' &&
      Number.isFinite(parsed.x) &&
      Number.isFinite(parsed.y)
    ) {
      return { x: parsed.x, y: parsed.y }
    }
    return { x: 0, y: 0 }
  } catch {
    return { x: 0, y: 0 }
  }
}

function readOffset(): SelectionBarOffset {
  try {
    return parseStoredOffset(localStorage.getItem(OFFSET_KEY))
  } catch {
    // localStorage 不可用(如 SSR/测试 node 环境):回落原点。
    return { x: 0, y: 0 }
  }
}

/** 合法值守卫:持久化串被篡改/旧值一律回落 'center'(默认居中,零惊讶)。 */
function isBarAlign(v: unknown): v is BarAlign {
  return v === 'center' || v === 'left' || v === 'right'
}

function readAlign(): BarAlign {
  try {
    const raw = localStorage.getItem(ALIGN_KEY)
    return isBarAlign(raw) ? raw : 'center'
  } catch {
    return 'center'
  }
}

// 模块级单例:多消费者共享同一响应式态。
const docked = ref<boolean>(readDocked())
const offset = ref<SelectionBarOffset>(readOffset())
// 浮动胶囊水平对齐(居中默认/靠左/靠右)——决定胶囊静止基位,拖拽 offset 叠加其上(用户裁决:
// 对齐=默认位、拖拽仍可覆盖)。与顶栏对齐(useToolbarAlign)各自独立(两栏各一设置)。
const align = ref<BarAlign>(readAlign())

// 宿主(MediaGrid)KeepAlive 活跃度:由 SelectionToolbar 在 onActivated/onDeactivated 写入。
// docked 条的 Teleport gate 与 AppStatusBar 的 info 让位共读此信号,确保二者同条件同步——
// 否则「选区残留进查看器(MediaGrid deactivate)」时会出现「info 已让位但 outlet 空」的空白。
// 初值 true 兼容非 KeepAlive 上下文。
const hostActive = ref(true)
function setHostActive(value: boolean) {
  hostActive.value = value
}

function setDocked(value: boolean) {
  docked.value = value
  try {
    localStorage.setItem(DOCKED_KEY, value ? '1' : '0')
  } catch {
    // localStorage 不可用只损失持久化,本次会话切换仍生效,静默降级(同 useTitlebarMode)。
  }
}

function setOffset(value: SelectionBarOffset) {
  offset.value = value
  try {
    localStorage.setItem(OFFSET_KEY, JSON.stringify(value))
  } catch {
    // 同上,静默降级。
  }
}

function setAlign(value: BarAlign) {
  align.value = value
  try {
    localStorage.setItem(ALIGN_KEY, value)
  } catch {
    // 同上,静默降级。
  }
}

/**
 * 把拖拽 offset 钳到「胶囊完整可见且拖拽手柄可达」范围,防「大窗口拖到角落 → 小窗口打开后条在屏外
 * 失踪」。offset 相对**当前对齐基位**(align 决定胶囊 CSS 静止位、底边距 BOTTOM_INSET)存储,对窗口
 * 尺寸变化天然稳健。纯函数(与 DOM 无关),spec 穷举。
 *
 * @param off     当前 offset(相对对齐基位的位移,px)
 * @param capsule 胶囊自身尺寸(width/height,px)
 * @param bounds  可拖边界尺寸(定位上下文 .selection-toolbar-wrapper 的 width/height,px)
 * @param align   胶囊水平对齐(决定静止基位;缺省 'center' 与旧行为逐值等价,故 3 参调用向后兼容)
 * @returns 钳制后的 offset
 */
export function clampOffset(
  off: SelectionBarOffset,
  capsule: { width: number; height: number },
  bounds: { width: number; height: number },
  align: BarAlign = 'center',
): SelectionBarOffset {
  // 水平:offset 相对当前对齐基位 baseLeft(胶囊左缘 CSS 静止位)。合法 offset 区间 = 使胶囊左缘落在
  // [CLAMP_MARGIN, bounds.width − capsule.width − CLAMP_MARGIN] 的位移集。
  //   居中:baseLeft=(bounds−capsule)/2 → 区间对称 ±((bounds−capsule)/2−margin),与旧居中逐值等价。
  //   靠左:baseLeft=SIDE_INSET(贴左内缩)→ 几乎只能右移;靠右:baseLeft=bounds−capsule−INSET → 只能左移。
  const baseLeft =
    align === 'left'
      ? SELECTION_BAR_SIDE_INSET
      : align === 'right'
        ? bounds.width - capsule.width - SELECTION_BAR_SIDE_INSET
        : (bounds.width - capsule.width) / 2
  let xMin = CLAMP_MARGIN - baseLeft
  let xMax = bounds.width - capsule.width - CLAMP_MARGIN - baseLeft
  // 胶囊比可用边界还宽(xMax<xMin):容不下,不位移,交由 CSS 对齐兜底。
  if (xMin > xMax) xMin = xMax = 0
  const x = Math.max(xMin, Math.min(xMax, off.x))
  // 竖直:基线底边在 bottom:BOTTOM_INSET。向上可动到顶部留边;向下最多到底边留边内。
  const upSlack = Math.max(0, bounds.height - capsule.height - BOTTOM_INSET - CLAMP_MARGIN)
  const downSlack = Math.max(0, BOTTOM_INSET - CLAMP_MARGIN)
  const y = Math.max(-upSlack, Math.min(downSlack, off.y))
  // 归一 -0 → 0:Math.max(-0, …) 可产负零,会污染 JSON 序列化并使 Object.is 断言意外失败。
  return { x: x === 0 ? 0 : x, y: y === 0 ? 0 : y }
}

export function useSelectionBarMode() {
  return { docked, setDocked, offset, setOffset, align, setAlign, hostActive, setHostActive }
}
