// src/composables/useTitlebarMode.ts
// 标题栏与 Gallery 工具栏「合并 / 分离」布局偏好——模块级响应式单例,值来自中央设置(config.toml)。
//
// merged(默认):Gallery 工具栏并入自绘标题栏同一行(Phase G 合并设计,commit e036787),与窗口三键
//   共享一条 bar 与拖拽区,竖向更省空间。
// separated:Gallery 工具栏独立成标题栏下方第二条 bar(S3 分层,commit bcf0934),富筛选/搜索不与
//   窗口三键争横向空间。
//
// 背景:S3 把布局从合并硬切成分离,推翻了用户原本的合并设计。用户裁决(2026-07-12):做成一键切换,
//   默认恢复合并。状态经本模块级单例共享(App.vue 布局 + 设置页开关同源,切换 live 生效)。
//
// 存储(设置集中保存,批次B):键 titlebar_merged 由后端 schema 注册;读经 readSetting、写经
// writeSettings,本文件不再自持持久化,也不读旧 localStorage。写盘失败由中央服务统一提示,
// 调用方不再自行 catch(避免吞错与重复提示)。
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
