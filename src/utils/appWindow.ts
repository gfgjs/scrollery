import { getCurrentWindow } from '@tauri-apps/api/window'
import { isUiHarness } from '../harness/runtime'

export interface AppWindowApi {
  isMaximized(): Promise<boolean>
  minimize(): Promise<void>
  toggleMaximize(): Promise<void>
  // maximize / unmaximize:与 toggleMaximize 并存的**幂等**入口。窗口三态转换(useWindowMode)需要
  // 「确保处于/离开最大化」而非「翻转」——toggle 在并发或状态不确定时会翻错方向。
  // ACL 已授 core:window:allow-maximize / allow-unmaximize(capabilities/default.json)。
  maximize(): Promise<void>
  unmaximize(): Promise<void>
  close(): Promise<void>
  // 自绘标题栏「整行按住拖动移窗」入口(useWindowDrag 越阈值时调用)。Tauri Window 原生自带,
  // browser harness 为 no-op。ACL 已授 core:window:allow-start-dragging(capabilities/default.json)。
  startDragging(): Promise<void>
  setTheme(theme: 'light' | 'dark' | null): Promise<void>
  isFullscreen(): Promise<boolean>
  setFullscreen(fullscreen: boolean): Promise<void>
  // 全屏期间禁用(useWindowMode.applyFullscreen):tauri-runtime-wry 给 undecorated+resizable 窗口
  // 盖了一个 HWND_TOP 的隐形 drag-resize 子窗口,在屏幕顶缘拦一条 ≈4px 输入带(全屏也不撤),
  // SetResizable(false) 是它唯一的公开 detach 入口。ACL 已授 core:window:allow-set-resizable。
  setResizable(resizable: boolean): Promise<void>
}

const browserWindow: AppWindowApi = {
  async isMaximized() {
    return false
  },
  async minimize() {},
  async toggleMaximize() {},
  async maximize() {},
  async unmaximize() {},
  async close() {},
  async startDragging() {},
  async setTheme() {},
  async isFullscreen() {
    return !!document.fullscreenElement
  },
  async setFullscreen(fullscreen) {
    if (fullscreen && !document.fullscreenElement) await document.documentElement.requestFullscreen()
    if (!fullscreen && document.fullscreenElement) await document.exitFullscreen()
  },
  async setResizable() {},
}

/** Tauri 真机窗口与 browser harness 的单一适配入口。 */
export function getAppWindow(): AppWindowApi {
  return isUiHarness ? browserWindow : getCurrentWindow()
}
