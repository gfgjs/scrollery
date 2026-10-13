// 标题栏与画廊工具栏的合并偏好由中央设置提供；App 布局和设置开关共用此响应式值。
// 合并时工具栏与窗口控件同排，分离时工具栏位于标题栏下方。
import { computed } from 'vue'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingBool } from './settingsValues'

const KEY = 'titlebar_merged'

// 模块级单例:App.vue 与设置页开关共享同一份响应式值(readSetting 追踪中央快照,重置后自动回落默认)。
const merged = computed(() => readSettingBool(KEY, true))

function setMerged(value: boolean) {
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise,不让 rejection 漏成 unhandled。
  writeSettings({ [KEY]: String(value) }).catch(() => {})
}

export function useTitlebarMode() {
  return { merged, setMerged }
}
