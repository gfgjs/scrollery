// src/composables/useDemoPrivacy.ts
// 演示打码开关(2026-09-16):模块级响应式单例,值来自中央设置(config.toml 键 demo_privacy)。
//
// 存储(设置集中保存,批次B):演示打码是全局用户偏好,与其它设置同库同理,故不再走 localStorage。
// **首帧不露真实内容**由启动门控保证:App 在中央设置快照到达前不渲染画廊,故本模块读到权威值
// (默认关闭)才开始出图;此前的占位值只用于状态,不写回设置。
// 消费方:AppToolbar(顶栏开关入口)、MediaGrid(引擎可用性上报 + 透传)、FoldersSection(树名/文件树显示别名)。
import { computed, ref } from 'vue'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingBool } from './settingsValues'

const SETTING_KEY = 'demo_privacy'

const demoPrivacyEnabled = computed(() => readSettingBool(SETTING_KEY, false))

// 当前画廊是否由 Canvas 引擎接管(canvasActive 判据的镜像)。打码能力只存在于 Canvas 渲染路径,
// 顶栏据此决定是否给出入口——DOM 模式下不提供会误导「已保护」的开关。由 MediaGrid 置位。
const demoPrivacySupported = ref<boolean>(false)

function setDemoPrivacy(enabled: boolean) {
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [SETTING_KEY]: String(enabled) }).catch(() => {})
}

function toggleDemoPrivacy() {
  setDemoPrivacy(!demoPrivacyEnabled.value)
}

function setDemoPrivacySupported(supported: boolean) {
  demoPrivacySupported.value = supported
}

export function useDemoPrivacy() {
  return {
    demoPrivacyEnabled,
    demoPrivacySupported,
    setDemoPrivacy,
    toggleDemoPrivacy,
    setDemoPrivacySupported,
  }
}
