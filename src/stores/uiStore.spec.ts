// uiStore 轴开合/形态契约单测(2026-07-24 画廊轴 minimap 解耦线)。
// 只钉本线新增的三条易回归契约,不试图覆盖 uiStore 全表:
//   ①水合枚举守卫:axis_mode 是持久化的外部输入,损坏/陌生值必须留默认 'timeline';
//   ②写盘键复用:axisVisible 写的仍是老键 seamless_minimap(语义扩为「轴整体开合」的
//     零迁移前提)——键名一旦被「顺手改成 axis_visible」,老用户的收起状态会静默丢失;
//   ③默认方向:seamless_minimap 缺位=开(保轴可发现性),仅显式 'false' 才隐。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

// 中和 setup 期的 get_startup_config:默认返回永不 resolve 的 promise,让启动 .then 不执行,
// 水合路径改由测试显式调 refreshFromBackend 驱动(mockResolvedValueOnce 插队)。
vi.mock('../utils/ipc', () => ({
  invokeIpc: vi.fn(() => new Promise(() => {})),
  invokeIpcRaw: vi.fn(() => new Promise(() => {})),
  ipcErrorMessage: (e: unknown) => String(e),
}))

import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { useUiStore, type StartupConfig } from './uiStore'

/**
 * 只给关心的键:hydrateFromStartupConfig 对每个字段都有 `!= null` / 白名单守卫,
 * 缺字段(undefined)按「未持久化」走默认分支,正是本测试要的初始态。
 */
function cfg(partial: Partial<StartupConfig>): StartupConfig {
  return partial as StartupConfig
}

describe('uiStore 主题风格模型', () => {
  beforeEach(() => {
    vi.stubGlobal('window', {
      matchMedia: () => ({
        matches: false,
        addEventListener: () => {},
        removeEventListener: () => {},
      }),
      location: { search: '' },
      addEventListener: () => {},
      removeEventListener: () => {},
    })
    vi.stubGlobal('document', {
      documentElement: {
        setAttribute: vi.fn(),
        removeAttribute: vi.fn(),
        style: { setProperty: vi.fn() },
      },
    })
    vi.stubGlobal('localStorage', { setItem: () => {}, getItem: () => null })
    vi.mocked(invokeIpc).mockClear()
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('选择风格时同时更新并写入亮暗两个槽位', () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockClear()

    ui.setThemeStyle('tech')

    expect(ui.lightThemeId).toBe('tech-light')
    expect(ui.darkThemeId).toBe('tech-dark')
    expect(ui.themeStyle).toBe('tech')
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_light',
      value: 'tech-light',
    })
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_dark',
      value: 'tech-dark',
    })
  })

  it('启动时以实际生效的浅色槽位风格配对明暗主题', async () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockResolvedValueOnce(
      cfg({ appearance: 'light', themeLight: 'porcelain', themeDark: 'dai' }),
    )

    await ui.refreshFromBackend()

    expect(ui.lightThemeId).toBe('minimal-light')
    expect(ui.darkThemeId).toBe('minimal-dark')
    expect(ui.resolvedThemeId).toBe('minimal-light')
  })

  it('启动时以实际生效的深色槽位风格配对明暗主题', async () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockResolvedValueOnce(
      cfg({ appearance: 'dark', themeLight: 'porcelain', themeDark: 'ink' }),
    )

    await ui.refreshFromBackend()

    expect(ui.lightThemeId).toBe('fresh-light')
    expect(ui.darkThemeId).toBe('fresh-dark')
    expect(ui.resolvedThemeId).toBe('fresh-dark')
  })

  it('迁移旧双槽后写回配对，切暗并刷新仍保持同一风格', async () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockClear()
    vi.mocked(invokeIpc).mockResolvedValueOnce(
      cfg({ appearance: 'light', themeLight: 'porcelain', themeDark: 'ink' }),
    )

    await ui.refreshFromBackend()

    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_light',
      value: 'minimal-light',
    })
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_dark',
      value: 'minimal-dark',
    })

    ui.setAppearance('dark')
    expect(ui.resolvedThemeId).toBe('minimal-dark')

    vi.mocked(invokeIpc).mockClear()
    vi.mocked(invokeIpc).mockResolvedValueOnce(
      cfg({ appearance: 'dark', themeLight: 'minimal-light', themeDark: 'minimal-dark' }),
    )
    await ui.refreshFromBackend()

    expect(ui.lightThemeId).toBe('minimal-light')
    expect(ui.darkThemeId).toBe('minimal-dark')
    expect(ui.resolvedThemeId).toBe('minimal-dark')
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_light',
      value: 'minimal-light',
    })
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_dark',
      value: 'minimal-dark',
    })
  })

  it('单槽位旧值迁移时补写缺失的另一槽位', async () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockClear()
    vi.mocked(invokeIpc).mockResolvedValueOnce(
      cfg({ appearance: 'light', themeLight: 'porcelain', themeDark: null }),
    )

    await ui.refreshFromBackend()

    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_light',
      value: 'minimal-light',
    })
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
      key: 'theme_dark',
      value: 'minimal-dark',
    })
  })

  it('两个主题槽位都缺失时使用 fresh 默认且不额外写盘', async () => {
    const ui = useUiStore()
    vi.mocked(invokeIpc).mockClear()
    vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ appearance: 'light' }))

    await ui.refreshFromBackend()

    expect(ui.lightThemeId).toBe('fresh-light')
    expect(ui.darkThemeId).toBe('fresh-dark')
    expect(invokeIpc).not.toHaveBeenCalledWith(
      IPC.SET_APP_CONFIG,
      expect.objectContaining({ key: 'theme_light' }),
    )
    expect(invokeIpc).not.toHaveBeenCalledWith(
      IPC.SET_APP_CONFIG,
      expect.objectContaining({ key: 'theme_dark' }),
    )
  })
})

describe('uiStore 轴开合/形态契约', () => {
  beforeEach(() => {
    // node 环境:uiStore setup 同步读 window.matchMedia;水合尾部 applyAppearance 触
    // document.documentElement 与 localStorage。给最小桩即可跑真实 store。
    vi.stubGlobal('window', {
      matchMedia: () => ({
        matches: false,
        addEventListener: () => {},
        removeEventListener: () => {},
      }),
      location: { search: '' },
      addEventListener: () => {},
      removeEventListener: () => {},
    })
    vi.stubGlobal('document', {
      documentElement: {
        setAttribute: vi.fn(),
        removeAttribute: vi.fn(),
        style: { setProperty: vi.fn() },
      },
    })
    vi.stubGlobal('localStorage', { setItem: () => {}, getItem: () => null })
    vi.mocked(invokeIpc).mockClear()
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('水合:axisMode 枚举守卫', () => {
    it('认可值 minimap 落地', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ axisMode: 'minimap' }))
      await ui.refreshFromBackend()
      expect(ui.axisMode).toBe('minimap')
    })

    it('陌生值不灌入,保持默认 timeline', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ axisMode: 'graph' }))
      await ui.refreshFromBackend()
      expect(ui.axisMode).toBe('timeline')
    })

    it('缺位(null)保持默认 timeline', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ axisMode: null }))
      await ui.refreshFromBackend()
      expect(ui.axisMode).toBe('timeline')
    })
  })

  describe('水合:axisVisible 默认方向', () => {
    it('缺位=开(轴可发现性)', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ seamlessMinimap: null }))
      await ui.refreshFromBackend()
      expect(ui.axisVisible).toBe(true)
    })

    it("仅显式 'false' 才隐", async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ seamlessMinimap: 'false' }))
      await ui.refreshFromBackend()
      expect(ui.axisVisible).toBe(false)
    })

    it("非 'false' 的任意值仍视为开", async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ seamlessMinimap: 'yes' }))
      await ui.refreshFromBackend()
      expect(ui.axisVisible).toBe(true)
    })
  })

  describe('写盘:键名契约', () => {
    it('setAxisVisible 写老键 seamless_minimap(零迁移前提)', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()
      ui.setAxisVisible(false)
      expect(ui.axisVisible).toBe(false)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'seamless_minimap',
        value: 'false',
      })
    })

    it('setAxisMode 写新键 axis_mode', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()
      ui.setAxisMode('minimap')
      expect(ui.axisMode).toBe('minimap')
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'axis_mode',
        value: 'minimap',
      })
    })
  })

  describe('查看器返回侧栏过渡', () => {
    it('新一代接管后，旧 transitionend 不得解除当前几何锁', () => {
      const ui = useUiStore()
      const first = ui.beginRouteReturnSidebarTransition()
      const second = ui.beginRouteReturnSidebarTransition()

      expect(ui.routeReturnSidebarTransitioning).toBe(true)
      expect(second).toBeGreaterThan(first)
      expect(ui.finishRouteReturnSidebarTransition(first)).toBe(false)
      expect(ui.routeReturnSidebarTransitioning).toBe(true)
      expect(ui.finishRouteReturnSidebarTransition(second)).toBe(true)
      expect(ui.routeReturnSidebarTransitioning).toBe(false)
    })
  })

  // ── 窗口材质(毛玻璃,2026-08-24)─────────────────────────────────────────────
  // 平台姿态:本文件未 mock '../utils/platform',节点环境探测兜底返回 'windows'(见 platform.ts),
  // 故默认 isWindows=true;用例内如需非 Windows 分支,须 doMock + 重载模块链后动态取 store——
  // isWindows 是模块级 const,且 vitest 手动 mock 的命名导出在 mock 模块实例化时一次性快照,
  // 不能原地改。

  describe('水合:windowMaterial 枚举守卫', () => {
    it('认可值 acrylic 落地', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ windowMaterial: 'acrylic' }))
      await ui.refreshFromBackend()
      expect(ui.windowMaterial).toBe('acrylic')
    })

    it('损坏值不灌入,保持默认 mica', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ windowMaterial: 'glass' }))
      await ui.refreshFromBackend()
      expect(ui.windowMaterial).toBe('mica')
    })

    it('缺位(null)保持默认 mica', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ windowMaterial: null }))
      await ui.refreshFromBackend()
      expect(ui.windowMaterial).toBe('mica')
    })
  })

  describe('applyWindowMaterial:data-glass 属性读写', () => {
    it('Windows + 合法值设置 data-glass 属性', () => {
      const ui = useUiStore()
      ui.setWindowMaterial('mica')
      expect(ui.windowMaterial).toBe('mica')
      expect(document.documentElement.setAttribute).toHaveBeenCalledWith('data-glass', 'mica')
    })

    it('none 值移除属性(即使 Windows)', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ windowMaterial: 'none' }))
      await ui.refreshFromBackend()
      expect(ui.windowMaterial).toBe('none')
      expect(document.documentElement.removeAttribute).toHaveBeenCalledWith('data-glass')
    })

    it('非 Windows 平台一律移除属性', async () => {
      vi.doMock('../utils/platform', () => ({
        currentPlatform: 'macos',
        isMac: false,
        isLinux: false,
        isWindows: false,
        isMobilePlatform: false,
      }))
      vi.resetModules()
      const { useUiStore: freshUseUiStore } = await import('./uiStore')
      const ui = freshUseUiStore()
      ui.setWindowMaterial('mica')
      expect(ui.windowMaterial).toBe('mica')
      expect(document.documentElement.removeAttribute).toHaveBeenCalledWith('data-glass')
      expect(document.documentElement.setAttribute).not.toHaveBeenCalledWith('data-glass', 'mica')
    })
  })

  describe('水合与写盘:毛玻璃分层不透明度', () => {
    it('水合时按范围收敛并写入四组 CSS 缩放变量', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(
        cfg({
          glassChromeOpacity: '150',
          glassStickyOpacity: '80',
          glassSurfaceOpacity: '0',
          glassControlOpacity: 'bad',
          glassGalleryOpacity: '45',
        }),
      )

      await ui.refreshFromBackend()

      expect(ui.glassChromeOpacity).toBe(120)
      expect(ui.glassStickyOpacity).toBe(80)
      expect(ui.glassSurfaceOpacity).toBe(20)
      expect(ui.glassControlOpacity).toBe(100)
      expect(ui.glassGalleryOpacity).toBe(45)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-chrome-scale',
        '1.2',
      )
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-sticky-scale',
        '0.8',
      )
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-gallery-opacity',
        '45%',
      )
    })

    it('设置器写对应 config.toml 键并即时更新 CSS 缩放', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()

      ui.setGlassControlOpacity(10)

      expect(ui.glassControlOpacity).toBe(20)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'glass_control_opacity',
        value: '20',
      })
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-control-scale',
        '0.2',
      )
    })

    it('设置器将画廊底色遮罩限制在 0–100 并写对应 config.toml 键', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()

      ui.setGlassGalleryOpacity(160)

      expect(ui.glassGalleryOpacity).toBe(100)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'glass_gallery_opacity',
        value: '100',
      })
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-gallery-opacity',
        '100%',
      )
    })

    it('水合时内容底面缩放损坏值回 100 并写 content scale', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ glassContentOpacity: 'bad' }))

      await ui.refreshFromBackend()

      expect(ui.glassContentOpacity).toBe(100)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-content-scale',
        '1',
      )
    })

    it('设置器写内容底面 config.toml 键并即时更新 CSS 缩放', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()

      ui.setGlassContentOpacity(10)

      expect(ui.glassContentOpacity).toBe(20)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'glass_content_opacity',
        value: '20',
      })
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--glass-content-scale',
        '0.2',
      )
    })
  })

  describe('水合与写盘:主题色浓度', () => {
    it('水合合法值并写 --theme-tint-scale', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTintStrength: '80' }))

      await ui.refreshFromBackend()

      expect(ui.themeTintStrength).toBe(80)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-tint-scale',
        '0.8',
      )
    })

    it('水合损坏值回默认 60,越界值钳到 0–100', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTintStrength: 'bad' }))
      await ui.refreshFromBackend()
      expect(ui.themeTintStrength).toBe(60)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-tint-scale',
        '0.6',
      )

      const ui2 = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTintStrength: '150' }))
      await ui2.refreshFromBackend()
      expect(ui2.themeTintStrength).toBe(100)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-tint-scale',
        '1',
      )
    })

    it('设置器写 theme_tint_strength 键、更新 CSS 并刷新首帧快照', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()
      const setItem = vi.spyOn(localStorage, 'setItem')

      ui.setThemeTintStrength(-10)

      expect(ui.themeTintStrength).toBe(0)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'theme_tint_strength',
        value: '0',
      })
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-tint-scale',
        '0',
      )
      // 浓度进首帧快照(theme-snapshot.js 据此 pre-paint 写 CSS 变量),避免启动底色闪变。
      expect(setItem).toHaveBeenCalledWith(
        'scrollery.themeSnapshot.v1',
        expect.stringContaining('"tint":0'),
      )
    })
  })

  describe('水合与写盘:文字浓度', () => {
    it('水合合法值并写 --theme-text-scale', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTextStrength: '80' }))

      await ui.refreshFromBackend()

      expect(ui.themeTextStrength).toBe(80)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-text-scale',
        '0.8',
      )
    })

    it('水合损坏值回默认 100,越界值钳到 40–100', async () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTextStrength: 'bad' }))
      await ui.refreshFromBackend()
      expect(ui.themeTextStrength).toBe(100)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-text-scale',
        '1',
      )

      const ui2 = useUiStore()
      vi.mocked(invokeIpc).mockResolvedValueOnce(cfg({ themeTextStrength: '150' }))
      await ui2.refreshFromBackend()
      expect(ui2.themeTextStrength).toBe(100)
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-text-scale',
        '1',
      )
    })

    it('设置器写 theme_text_strength 键、更新 CSS 并刷新首帧快照', () => {
      const ui = useUiStore()
      vi.mocked(invokeIpc).mockClear()
      const setItem = vi.spyOn(localStorage, 'setItem')

      ui.setThemeTextStrength(30)

      expect(ui.themeTextStrength).toBe(40)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_APP_CONFIG, {
        key: 'theme_text_strength',
        value: '40',
      })
      expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
        '--theme-text-scale',
        '0.4',
      )
      // 文字浓度同进首帧快照,避免启动文字闪变。
      expect(setItem).toHaveBeenCalledWith(
        'scrollery.themeSnapshot.v1',
        expect.stringContaining('"text":40'),
      )
    })
  })
})
