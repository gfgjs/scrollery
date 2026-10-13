import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { getCurrentWindow } from '@tauri-apps/api/window'
import App from './App.vue'
import router from './router'
import i18n from './i18n'
import { registerBuiltins } from './commands'
import { uiHarnessScene, isUiHarness } from './harness/runtime'
import { installSettingsLifecycle } from './composables/useSettingsLifecycle'
import { installGlobalErrorHandlers } from './utils/logger'
import { dismissStartupLayer } from './utils/startupLayer'
import { useThemeStore } from './stores/themeStore'
import './assets/styles/index.css'

// 尽早安装全局错误监听，覆盖挂载前的错误。
installGlobalErrorHandlers()

// 兜底:防 FOUC 的首帧着色由 index.html 的阻塞脚本负责(按缓存恢复已生成变量 + 解析系统明暗,
// 两个窗口共享同一 index.html 与同源 localStorage;脚本源码见 src/themes/bootstrapScript.ts)。
// 此处仅在该脚本未生效的异常情况下补一份明暗属性,让注入的默认主题 CSS 至少选对深浅。
if (!document.documentElement.hasAttribute('data-color-scheme')) {
  document.documentElement.setAttribute(
    'data-color-scheme',
    window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  )
}

// 按窗口 label 分流；普通浏览器没有 Tauri API，须先判定运行环境再读取窗口 label。
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
  // LogWindowView 自行读取设置并呈现读取失败；此处只安装退出 flush 协议。
  // 退出 flush 协议必须**每个窗口各装一次**(幂等):后端退出前向全部存活窗口要一次 flush 回执,
  // 日志窗口持有全局 preset 设置,不装就收不到请求、回执永远少一员,后端只能等超时按未完成处理。
  // 与主窗口 App.vue 的装配互不影响(各窗口有独立的模块状态)。
  void installSettingsLifecycle().catch(() => {})
  // 动态 import(而非顶层静态 import):日志窗口(含 @tanstack/vue-virtual 虚拟列表)只在
  // 这一支被走到才需要,静态引入会把它连同主窗口一起塞进同一 entry chunk(main.ts 是两窗口
  // 共享的单一入口),把预算门(scripts/vite-plugin-bundle-budget.mjs)撑爆。
  void import('./views/LogWindowView.vue').then(({ default: LogWindowView }) => {
    const logApp = createApp(LogWindowView)
    // 日志窗口与主窗口同源:主题色板按同一份已确认配置发布(它同样绘制侧栏底色与文字)。
    const logPinia = createPinia()
    logApp.use(logPinia)
    useThemeStore(logPinia)
    logApp.use(i18n)
    logApp.mount('#app')
    dismissStartupLayer()
  })
} else {
  const app = createApp(App)

  // 主题运行时在挂载前实例化:水合到达时它已在监听,主题键一旦就位即发布(DOM 与 Canvas 同源)。
  const pinia = createPinia()
  app.use(pinia)
  useThemeStore(pinia)
  app.use(router)
  app.use(i18n)

  // DEV browser 场景仍挂载真实 shell/route/store；这里只选择入口场景，不创建演示页面。
  if (uiHarnessScene === 'settings') void router.isReady().then(() => router.replace('/settings'))
  if (uiHarnessScene === 'viewer') void router.isReady().then(() => router.replace('/view/1'))

  // 注册内建命令，供右键菜单和上下文工具栏消费。
  // 仅填充命令定义(标题惰性、图标 markRaw),不实例化 store,故可在 mount 前安全调用。
  registerBuiltins()

  app.mount('#app')
}
