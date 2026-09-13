// src/composables/useWindowMode.spec.ts
// 窗口三态所有者的契约测试(2026-07-16 真机 round11 #8)。
//
// 这些测试钉的是**不变量**,不是实现细节:三态互斥、全屏必从标准窗口态起步、失败必回读 OS。
// 真机侧(Tauri 窗口/tao/Windows)无法在 node 环境复现,故本文件只覆盖「本模块对 AppWindowApi
// 发出的调用序列与由此得到的对外状态」——即所有**我们自己**决定的部分。真机行为(黑边是否消失、
// 拖拽是否惰性)必须真机验收,不可由本文件冒充(见 docs/experience.md §19)。

import { describe, it, expect, vi, beforeEach } from 'vitest'

// OS 侧窗口的可变模拟态。测试可直接改它来模拟「OS 这么报」。
const os = { fullscreen: false, maximized: false }
// 本次用例对 AppWindowApi 的调用序列(判定顺序契约,如 unmaximize 必先于 setFullscreen(true))。
let calls: string[] = []
// setFullscreen 的人为闸门:非 null 时转换悬停于此,用于测重入丢弃。
let gate: Promise<void> | null = null
// setFullscreen 是否抛错:测「转换失败必回读 OS 自愈」。
let throwOnSetFullscreen = false
// setResizable 是否抛错:测「死区补丁自身失败只降级,不拖垮全屏转换」。
let throwOnSetResizable = false

vi.mock('../utils/appWindow', () => ({
  getAppWindow: () => ({
    async isMaximized() {
      return os.maximized
    },
    async minimize() {},
    async toggleMaximize() {
      calls.push('toggleMaximize')
      os.maximized = !os.maximized
    },
    async maximize() {
      calls.push('maximize')
      os.maximized = true
    },
    async unmaximize() {
      calls.push('unmaximize')
      os.maximized = false
    },
    async close() {},
    async startDragging() {},
    async setTheme() {},
    async isFullscreen() {
      return os.fullscreen
    },
    async setFullscreen(on: boolean) {
      calls.push(`setFullscreen(${on})`)
      if (gate) await gate
      if (throwOnSetFullscreen) throw new Error('ACL denied')
      os.fullscreen = on
    },
    async setResizable(resizable: boolean) {
      calls.push(`setResizable(${resizable})`)
      if (throwOnSetResizable) throw new Error('ACL denied')
    },
  }),
}))

import {
  windowMode,
  isFullscreen,
  isMaximized,
  canDragWindow,
  setFullscreen,
  toggleFullscreen,
  toggleMaximize,
  initWindowMode,
  __resetWindowModeForTest,
} from './useWindowMode'

beforeEach(() => {
  os.fullscreen = false
  os.maximized = false
  calls = []
  gate = null
  throwOnSetFullscreen = false
  throwOnSetResizable = false
  __resetWindowModeForTest()
})

describe('三态互斥(结构性不变量)', () => {
  it('OS 同时报 fullscreen 与 maximized 时,mode 判全屏、isMaximized 恒 false', async () => {
    // Windows 下 undecorated 窗口进全屏后 IsZoomed 仍可能为真。三态互斥必须由**我们的**
    // computed 保证,而不是指望 OS 两个标志天然互斥。
    os.fullscreen = true
    os.maximized = true
    await initWindowMode()
    expect(windowMode.value).toBe('fullscreen')
    expect(isFullscreen.value).toBe(true)
    expect(isMaximized.value).toBe(false)
  })

  it('仅 maximized → mode=maximized;两者皆无 → normal', async () => {
    os.maximized = true
    await initWindowMode()
    expect(windowMode.value).toBe('maximized')

    os.maximized = false
    await initWindowMode()
    expect(windowMode.value).toBe('normal')
  })
})

describe('全屏必从标准窗口态起步(真机 #8-1 黑边根因)', () => {
  it('最大化态进全屏:unmaximize 必先于 setFullscreen(true)', async () => {
    os.maximized = true
    await setFullscreen(true)
    expect(calls).toEqual(['unmaximize', 'setFullscreen(true)', 'setResizable(false)'])
    expect(isFullscreen.value).toBe(true)
  })

  it('标准窗口进全屏:不卸最大化(无谓的 unmaximize 会白闪一下)', async () => {
    os.maximized = false
    await setFullscreen(true)
    expect(calls).toEqual(['setFullscreen(true)', 'setResizable(false)'])
  })
})

describe('退全屏还原入全屏前的态', () => {
  it('最大化 → F11 → F11 回到最大化,而非标准窗口', async () => {
    os.maximized = true
    await setFullscreen(true)
    calls = []
    await setFullscreen(false)
    expect(calls).toEqual(['setResizable(true)', 'setFullscreen(false)', 'maximize'])
    expect(windowMode.value).toBe('maximized')
  })

  it('标准窗口 → F11 → F11 回到标准窗口,不擅自最大化', async () => {
    os.maximized = false
    await setFullscreen(true)
    calls = []
    await setFullscreen(false)
    expect(calls).toEqual(['setResizable(true)', 'setFullscreen(false)'])
    expect(windowMode.value).toBe('normal')
  })

  it('还原记忆不跨轮泄漏:最大化进出一轮后,再从标准窗口进出不应 maximize', async () => {
    os.maximized = true
    await setFullscreen(true)
    await setFullscreen(false) // 还原到最大化
    await toggleMaximize() // 手动退最大化 → 标准窗口
    expect(windowMode.value).toBe('normal')
    calls = []
    await setFullscreen(true)
    await setFullscreen(false)
    expect(calls).toEqual([
      'setFullscreen(true)',
      'setResizable(false)',
      'setResizable(true)',
      'setFullscreen(false)',
    ])
    expect(windowMode.value).toBe('normal')
  })
})

describe('全屏态下的移窗与最大化', () => {
  it('canDragWindow 在全屏态为 false(useWindowDrag 据此惰性)', async () => {
    expect(canDragWindow.value).toBe(true)
    await setFullscreen(true)
    expect(canDragWindow.value).toBe(false)
  })

  it('全屏态 toggleMaximize → 退出全屏,而非向 OS 发 toggleMaximize', async () => {
    await setFullscreen(true)
    calls = []
    await toggleMaximize()
    expect(calls).not.toContain('toggleMaximize')
    expect(calls).toContain('setFullscreen(false)')
    expect(isFullscreen.value).toBe(false)
  })

  it('非全屏态 toggleMaximize 照常透传 OS', async () => {
    await toggleMaximize()
    expect(calls).toEqual(['toggleMaximize'])
    expect(windowMode.value).toBe('maximized')
  })
})

describe('转换闸门与自愈', () => {
  it('转换未落地时重入被丢弃(F11 按住自动重复不致抽搐)', async () => {
    let release!: () => void
    gate = new Promise<void>((r) => (release = r))
    const first = setFullscreen(true)
    // 闸住期间的第二次请求:丢弃,不排队。
    await setFullscreen(false)
    release()
    await first
    // 收尾后断言(而非闸中断言):此刻第一次转换已跑完、push 必已落地,故「只有一次 setFullscreen(true)、
    // 没有 (false)」是确定性结论,不依赖数微任务节拍。
    expect(calls.filter((c) => c.startsWith('setFullscreen'))).toEqual(['setFullscreen(true)'])
    expect(isFullscreen.value).toBe(true)
  })

  it('转换失败不停在臆测值,回读 OS 真相', async () => {
    // 旧 uiStore 是 `isFullscreen.value = !current` 乐观赋值 + 出错也不纠正 —— 正是
    // 「窗口被拖出全屏后 isFullscreen 仍为 true、从此贴顶最大化失效」(真机 #8-1b)的来源。
    throwOnSetFullscreen = true
    await setFullscreen(true)
    expect(os.fullscreen).toBe(false) // OS 侧确实没进全屏
    expect(isFullscreen.value).toBe(false) // 我们的态也没停在 true
  })

  it('OS 侧背着我们退了全屏,下一次 toggle 从真相出发(而非翻转陈旧值)', async () => {
    await setFullscreen(true)
    expect(isFullscreen.value).toBe(true)
    // 模拟 OS/外力把窗口退出全屏但没通知我们。
    os.fullscreen = false
    await initWindowMode() // 回同步(真机走 DOM resize → scheduleSync → syncFromOs)
    expect(isFullscreen.value).toBe(false)
    calls = []
    await toggleFullscreen()
    expect(calls).toContain('setFullscreen(true)') // 进全屏,而非把陈旧的 true 翻成 false
  })
})

describe('全屏期间禁用 resizable(根除 tauri drag-resize 顶缘输入死区,2026-07-16 真机根因)', () => {
  // 顺序契约(禁在 setFullscreen(true) 成功之后、恢复在 setFullscreen(false) 之前)已由上面各
  // toEqual 序列钉死;本块钉的是两个失败模式的安全性。

  it('进全屏失败 → 不禁 resizable:死区补丁不脱离全屏语义单独存在', async () => {
    throwOnSetFullscreen = true
    await setFullscreen(true)
    expect(calls.filter((c) => c.startsWith('setResizable'))).toEqual([])
  })

  it('setResizable 自身失败只降级(死区回归),全屏转换照常落地', async () => {
    throwOnSetResizable = true
    await setFullscreen(true)
    expect(isFullscreen.value).toBe(true)
    calls = []
    await setFullscreen(false)
    expect(isFullscreen.value).toBe(false)
    // 退出路径同样容错:setResizable(true) 抛错后 setFullscreen(false) 仍然执行了。
    expect(calls).toContain('setFullscreen(false)')
  })
})
