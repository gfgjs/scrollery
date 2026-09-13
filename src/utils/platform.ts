// src/utils/platform.ts
// L0 平台探测:自绘标题栏(WindowChrome)按 mac/windows 分叉窗口装饰(mac 保原生红绿灯 +
// titleBarStyle:Overlay;Windows 走 decorations:false 自绘三键 + Snap Layouts),因此需要一个
// 可靠、同步、零 IPC 往返的平台判定单源。
//
// 经官方 @tauri-apps/plugin-os 的 platform():Tauri v2 中该值在启动期由插件注入 webview
// (window.__TAURI_OS_PLUGIN_INTERNALS__),故 platform() 是**同步**读取(区别于 locale()/
// hostname() 这类异步项),可直接缓存为模块级不可变常量。消费方只读派生布尔量,不再各自
// 重复探测(前端此前零平台判断,分支全在 Rust cfg——见设计文档 §1.1)。

import { platform, type Platform } from '@tauri-apps/plugin-os'

/**
 * 同步探测当前平台并缓存。非 Tauri 上下文(vitest / 纯浏览器工具链)无注入对象,platform()
 * 会抛 —— 降级为 'windows'(主开发平台)以免连带炸间接 import 本模块的单测。真机恒有注入值,
 * 降级分支不可达。
 */
function detectPlatform(): Platform {
  try {
    return platform()
  } catch {
    return 'windows'
  }
}

/** 当前运行平台(启动期探测一次,进程内不可变)。 */
export const currentPlatform: Platform = detectPlatform()

/** macOS:保留系统红绿灯,标题栏用 titleBarStyle:Overlay(设计文档 §3.2)。 */
export const isMac = currentPlatform === 'macos'

/** Windows:decorations:false 自绘三键 + tauri-plugin-frame 补 Snap Layouts。 */
export const isWindows = currentPlatform === 'windows'

/** Linux:本期标题栏能力降级(Snap Layouts no-op),仍走自绘分支但不叠 native child HWND。 */
export const isLinux = currentPlatform === 'linux'

/** Android/iOS:opener 不支持 reveal(D-001)——「在文件管理器中显示」类动作须隐藏/禁用,
 *  后端直调返回稳定 `unsupported_platform` 仅是兜底(S 线审查 R-09)。 */
export const isMobilePlatform = currentPlatform === 'android' || currentPlatform === 'ios'
