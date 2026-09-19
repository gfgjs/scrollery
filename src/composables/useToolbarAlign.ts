// src/composables/useToolbarAlign.ts
// 顶栏(AppToolbar)中部 chips 簇的水平对齐偏好——模块级响应式单例,值来自中央设置(config.toml)。
//
// 默认居中(对齐 Win11 任务栏 / macOS 系统级操作习惯):标题恒左、搜索恒右不动,只有中间的
//   筛选/视图 chips 簇在其 flex:1 折叠区内居中/靠左/靠右。留白在簇两侧(居中)或一侧(靠左/靠右);
//   收窄窗口时先吃掉留白(justify 空间收缩),再由 useToolbarOverflow 折叠——「先减留白再折叠」天然成立。
// 与选区栏对齐(useSelectionBarMode.align)**各自独立**(用户裁决:两栏各一设置)。
import { computed } from 'vue'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingEnum } from './settingsValues'

/** 工具栏水平对齐三态(顶栏 chips 簇 / 选区浮动胶囊共用此字面联合)。 */
export type BarAlign = 'center' | 'left' | 'right'

const ALIGN_KEY = 'toolbar_align'
/** 三态候选集:schema 已限定枚举,这里只用于「快照未到/异常文本」时的回落判定。 */
const BAR_ALIGNS = ['center', 'left', 'right'] as const

// 模块级单例:AppToolbar 消费 + 设置页开关同源(重置后经中央快照自动回默认)。
const align = computed(() => readSettingEnum<BarAlign>(ALIGN_KEY, BAR_ALIGNS, 'center'))

function setAlign(value: BarAlign) {
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [ALIGN_KEY]: value }).catch(() => {})
}

export function useToolbarAlign() {
  return { align, setAlign }
}
