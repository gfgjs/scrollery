// src/composables/useTitlebarMode.ts
// 标题栏与 Gallery 工具栏「合并 / 分离」布局偏好——模块级响应式单例 + localStorage 持久化。
//
// merged(默认):Gallery 工具栏并入自绘标题栏同一行(Phase G 合并设计,commit e036787),与窗口三键
//   共享一条 bar 与拖拽区,竖向更省空间。
// separated:Gallery 工具栏独立成标题栏下方第二条 bar(S3 分层,commit bcf0934),富筛选/搜索不与
//   窗口三键争横向空间。
//
// 背景:S3 把布局从合并硬切成分离,推翻了用户原本的合并设计。用户裁决(2026-07-12):做成一键切换,
//   默认恢复合并。状态经本模块级单例共享(App.vue 布局 + 设置页开关同源,切换 live 生效),localStorage 持久。
import { ref } from 'vue'

const KEY = 'titlebar_merged'

function read(): boolean {
  try {
    // 缺省 / 非法值一律回落 merged(默认合并=用户原设计);仅显式 '0' 走分离。
    return localStorage.getItem(KEY) !== '0'
  } catch {
    return true
  }
}

// 模块级单例:App.vue 与设置页开关共享同一响应式布尔。
const merged = ref<boolean>(read())

function setMerged(value: boolean) {
  merged.value = value
  try {
    localStorage.setItem(KEY, value ? '1' : '0')
  } catch {
    // localStorage 不可用只损失持久化,本次会话内切换仍生效,静默降级。
  }
}

export function useTitlebarMode() {
  return { merged, setMerged }
}
