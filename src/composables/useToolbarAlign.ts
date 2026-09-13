// src/composables/useToolbarAlign.ts
// 顶栏(AppToolbar)中部 chips 簇的水平对齐偏好——模块级响应式单例 + localStorage 持久化。
//
// 默认居中(对齐 Win11 任务栏 / macOS 系统级操作习惯):标题恒左、搜索恒右不动,只有中间的
//   筛选/视图 chips 簇在其 flex:1 折叠区内居中/靠左/靠右。留白在簇两侧(居中)或一侧(靠左/靠右);
//   收窄窗口时先吃掉留白(justify 空间收缩),再由 useToolbarOverflow 折叠——「先减留白再折叠」天然成立。
// 与选区栏对齐(useSelectionBarMode.align)**各自独立**(用户裁决:两栏各一设置)。
import { ref } from 'vue'

/** 工具栏水平对齐三态(顶栏 chips 簇 / 选区浮动胶囊共用此字面联合)。 */
export type BarAlign = 'center' | 'left' | 'right'

const ALIGN_KEY = 'toolbar_align'

/** 合法值守卫:持久化串被篡改/旧值一律回落 'center'(默认居中,零惊讶)。 */
function isBarAlign(v: unknown): v is BarAlign {
  return v === 'center' || v === 'left' || v === 'right'
}

function readAlign(): BarAlign {
  try {
    const raw = localStorage.getItem(ALIGN_KEY)
    return isBarAlign(raw) ? raw : 'center'
  } catch {
    // localStorage 不可用(SSR / 测试 node 环境):回落默认。
    return 'center'
  }
}

// 模块级单例:AppToolbar 消费 + 设置页开关同源。
const align = ref<BarAlign>(readAlign())

function setAlign(value: BarAlign) {
  align.value = value
  try {
    localStorage.setItem(ALIGN_KEY, value)
  } catch {
    // localStorage 不可用只损失持久化,本次会话切换仍生效,静默降级(同 useSelectionBarMode)。
  }
}

export function useToolbarAlign() {
  return { align, setAlign }
}
