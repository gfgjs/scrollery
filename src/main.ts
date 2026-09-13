// src/main.ts
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { getCurrentWindow } from '@tauri-apps/api/window'
import App from './App.vue'
import router from './router'
import i18n from './i18n'
import { registerBuiltins } from './commands'
import { uiHarnessScene, isUiHarness } from './harness/runtime'
import { installGlobalErrorHandlers } from './utils/logger'
import { dismissStartupLayer } from './utils/startupLayer'
import { DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME } from './themes/registry'
import './assets/styles/index.css'

// 前端日志桥全局兜底(日志能力重构 S3,方案 §4/§9.4):越早挂越好,覆盖 mount 前抛出的错误。
installGlobalErrorHandlers()

// 兜底:防 FOUC 的首帧着色已由 index.html 内联脚本负责(读 localStorage 快照,两个窗口共享同一
// index.html、同源 localStorage);此处仅在内联脚本未生效的异常情况下按系统偏好补一份。
if (!document.documentElement.hasAttribute('data-theme')) {
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches
  document.documentElement.setAttribute(
    'data-theme',
    prefersDark ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME,
  )
  document.documentElement.setAttribute('data-color-scheme', prefersDark ? 'dark' : 'light')
}

// 独立日志窗口(S4,方案 §5「主体是独立日志窗口」):按 Tauri 窗口 label 分流,跳过主窗口的
// AppShell/路由整套——日志窗口只需要一个能挂 Pinia+i18n 的干净根。browser UI harness 没有
// Tauri 窗口 API,恒走主路径(isUiHarness 判断本就是本仓既有的环境守卫惯例)。
// isTauri 兜底(reviewer 深审 2026-07-20 修复):不带 ?ui-harness= 的裸浏览器打开 vite dev
// server 时 isUiHarness 为假,但 __TAURI_INTERNALS__ 同样不存在——getCurrentWindow() 会同步
// 抛错,整段脚本中断、白屏。此前该场景本就靠这条兜底路径挂载真实 shell(见下方注释「DEV
// browser 场景仍挂载真实 shell」),故此路径必须能正常跑通,不能被新引入的窗口分流带崩。
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const windowLabel = isUiHarness || !isTauri ? 'main' : getCurrentWindow().label

// UI harness 的最小 Tauri runtime 桩(须在上方 isTauri 判定之后装,否则窗口分流误入真机路径):
// harness 页面没有 __TAURI_INTERNALS__,但缩略图链路有两处在 invokeIpc 之前就依赖它——
// useRequestQueue 的 new Channel()(构造期调 transformCallback)与 resolveAssetUrl 的
// convertFileSrc(status 1/3 的 URL 构造)。缺桩时前者令批量生成请求整链死亡、后者在
// Canvas drawCell 热路径同步抛错(逐帧 rAF 异常,冷格/gauge 全空)。桩语义:
//  - transformCallback 只回递增 id,不注册回调——消息经 invokeHarness 直接触发 Channel.onmessage;
//  - convertFileSrc 直通(fixture 用 http/data/相对 URL,不经 asset 协议);
//  - invoke 拒绝——harness 一切 IPC 走 invokeIpc 的 isUiHarness 分支,误入此处的(如
//    @tauri-apps/api/event.listen)与缺桩时的 TypeError 同为失败,但更可读。
if (isUiHarness && !isTauri) {
  let harnessCallbackId = 0
  Object.defineProperty(window, '__TAURI_INTERNALS__', {
    value: {
      transformCallback: () => ++harnessCallbackId,
      convertFileSrc: (path: string) => path,
      invoke: () => Promise.reject(new Error('UI harness 无 Tauri runtime')),
    },
    configurable: false,
  })
}

if (windowLabel === 'logs') {
  // 动态 import(而非顶层静态 import):日志窗口(含 @tanstack/vue-virtual 虚拟列表)只在
  // 这一支被走到才需要,静态引入会把它连同主窗口一起塞进同一 entry chunk(main.ts 是两窗口
  // 共享的单一入口),把预算门(scripts/vite-plugin-bundle-budget.mjs)撑爆。
  void import('./views/LogWindowView.vue').then(({ default: LogWindowView }) => {
    const logApp = createApp(LogWindowView)
    logApp.use(createPinia())
    logApp.use(i18n)
    logApp.mount('#app')
    dismissStartupLayer()
  })
} else {
  const app = createApp(App)

  app.use(createPinia())
  app.use(router)
  app.use(i18n)

  // DEV browser 场景仍挂载真实 shell/route/store；这里只选择入口场景，不创建演示页面。
  if (uiHarnessScene === 'settings') void router.isReady().then(() => router.replace('/settings'))
  if (uiHarnessScene === 'viewer') void router.isReady().then(() => router.replace('/view/1'))

  // 命令注册表(顶栏重构 L3):装入内建命令,供右键菜单 / 上下文工具栏 / 命令面板消费。
  // 仅填充命令定义(标题惰性、图标 markRaw),不实例化 store,故可在 mount 前安全调用。
  registerBuiltins()

  app.mount('#app')
}
