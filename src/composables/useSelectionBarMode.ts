// src/composables/useSelectionBarMode.ts
// 选区操作条「分离(浮动) / 合并(docked)」形态偏好 + 拖拽位置——模块级响应式单例,值来自中央设置。
//
// 分离(默认):浮动可拖胶囊(拖拽位置持久化);合并(docked):动作条 Teleport 进 AppStatusBar,替换式
//   共存(28px 恒高、不可拖)。默认分离=零惊讶(浮动可拖是现状且被裁决为核心能力,docked 是新增选项)。
//   与 useTitlebarMode 默认合并的不对称是有意的(各自被推翻/保留的「原设计」不同)。
// 状态经本模块级单例共享(SelectionToolbar 形态分支 + AppStatusBar 让位判断 + 设置页开关同源)。
//
// 存储(设置集中保存,批次B):selection_bar_docked(bool) / selection_bar_offset(内联表 {x,y}) /
// selection_bar_align(枚举)三键均在后端 schema 注册;offset 在 IPC 上是规范 JSON 文本,形状校验
// 复用 parseStoredOffset(纯函数,spec 穷举)。本文件不再自持 localStorage,重置后经中央快照回落默认。
import { computed, ref } from 'vue'
import { readSetting, settingsReady, writeSettings } from '../stores/settingsPersistence'
import type { BarAlign } from './useToolbarAlign'
import { readSettingBool, readSettingEnum } from './settingsValues'

const DOCKED_KEY = 'selection_bar_docked'
const OFFSET_KEY = 'selection_bar_offset'
const ALIGN_KEY = 'selection_bar_align'

/** 三态候选集(schema 已限定枚举;此处仅用于快照未到/异常文本时的回落判定)。 */
const BAR_ALIGNS = ['center', 'left', 'right'] as const

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

/**
 * 解析 offset 的规范 JSON 文本:非法/残缺/非有限数/解析失败一律回落原点(相对居中基线)。
 * 纯函数(不触存储),便于 spec 穷举各类残缺输入。
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

// 模块级单例:多消费者共享同一响应式态(均为只读计算值,写入经下方 setter 显式提交)。
const docked = computed(() => readSettingBool(DOCKED_KEY, false))
const offset = computed(() => parseStoredOffset(readSetting(OFFSET_KEY) ?? null))
// 浮动胶囊水平对齐(居中默认/靠左/靠右)——决定胶囊静止基位,拖拽 offset 叠加其上(用户裁决:
// 对齐=默认位、拖拽仍可覆盖)。与顶栏对齐(useToolbarAlign)各自独立(两栏各一设置)。
const align = computed(() => readSettingEnum<BarAlign>(ALIGN_KEY, BAR_ALIGNS, 'center'))

// 宿主(MediaGrid)KeepAlive 活跃度:由 SelectionToolbar 在 onActivated/onDeactivated 写入。
// docked 条的 Teleport gate 与 AppStatusBar 的 info 让位共读此信号,确保二者同条件同步——
// 否则「选区残留进查看器(MediaGrid deactivate)」时会出现「info 已让位但 outlet 空」的空白。
// 初值 true 兼容非 KeepAlive 上下文。运行期信号,不入设置。
const hostActive = ref(true)
function setHostActive(value: boolean) {
  hostActive.value = value
}

function setDocked(value: boolean) {
  writeSettings({ [DOCKED_KEY]: String(value) }).catch(() => {})
}

function setOffset(value: SelectionBarOffset) {
  // 权威快照未到达前读到的 offset 是占位默认值(原点),此刻回写会把用户真实位置抹成 {0,0};
  // 故未就绪一律不采集、不提交,等快照到达后由组件重新钳制。
  if (!settingsReady.value) return
  // 拖拽结束/resize 钳制回写:连续操作走防抖合并,避免拖动期逐次落盘。
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [OFFSET_KEY]: JSON.stringify(value) }, { debounce: true }).catch(() => {})
}

function setAlign(value: BarAlign) {
  writeSettings({ [ALIGN_KEY]: value }).catch(() => {})
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
