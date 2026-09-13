// ManagementSection 扫描入口渲染（P1-4）。
//
// 关注点：自动按钮（重新扫描/停止）与显式快速/完整两个入口都在位、都有可读提示（UiIconButton 的
// label/title 落到原生 title），且运行中两个显式入口 disabled——运行中重启会打断正在跑的轮次，
// 而「完整扫描」正是用户在别处改过文件内容后要用的那个入口。
// 环境：SSR renderToString + 真实 i18n（具体文案即被测面：用户靠提示分辨两个入口的差别）。
// 项目未装 @vue/test-utils，故与其它 UI contract 测试同款只做渲染断言。
import { describe, it, expect, vi } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createI18n } from 'vue-i18n'
import zhCN from '../../../i18n/locales/zh-CN'

const mock = vi.hoisted(() => ({
  progress: { current: null as null | { isRunning: boolean } },
  startScan: vi.fn(() => Promise.resolve()),
}))

// useSidebarSections 在 setup 里读 app_config（provides 由本测试的根组件补上）。
vi.mock('../../../utils/ipc', () => ({ invokeIpc: () => Promise.resolve(null) }))

vi.mock('../../../stores/scanStore', () => ({
  useScanStore: () => ({
    hasScanRoots: true,
    scanRoots: [{ id: 7, path: '/photos/2024', alias: '2024', isHidden: false }],
    getProgress: () => mock.progress.current,
    startScan: mock.startScan,
    stopScan: vi.fn(() => Promise.resolve()),
    setScanRootHidden: vi.fn(() => Promise.resolve()),
    removeScanRoot: vi.fn(() => Promise.resolve({ cleared_count: 0 })),
    loadScanRoots: vi.fn(() => Promise.resolve(true)),
    markScanStopped: vi.fn(),
  }),
}))
vi.mock('../../../stores/toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))
vi.mock('../../../stores/mediaStore', () => ({ useMediaStore: () => ({ loadStats: vi.fn() }) }))
// 目录移动半完成恢复列表（P0-1）：本测试无 pinia，且扫描入口与恢复清单无关 → 空清单桩，
// 列表块整体不渲染（其自身契约见 DirectoryMoveRecoveryList.spec.ts）。
vi.mock('../../../stores/directoryMoveRecoveryStore', () => ({
  useDirectoryMoveRecoveryStore: () => ({
    items: [],
    loading: false,
    isRetrying: () => false,
    load: vi.fn(() => Promise.resolve()),
    retry: vi.fn(() => Promise.resolve(false)),
  }),
  dirMoveRecoveryDetailKey: (detail: string) => `sidebar.dirRecovery.detail.${detail}`,
}))
vi.mock('../../../composables/useConfirm', () => ({
  useConfirm: () => ({ confirm: async () => ({ confirmed: false, checkboxValue: false }) }),
}))

import ManagementSection from './ManagementSection.vue'
import { provideSidebarSections } from '../../../composables/useSidebarSections'

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })
const t = i18n.global.t

async function render(): Promise<string> {
  const app = createSSRApp({
    setup() {
      provideSidebarSections()
      return () => h(ManagementSection as Component, { order: 3 })
    },
  })
  app.use(i18n)
  return renderToString(app)
}

/** 取出 title 命中该文案的那个按钮的开标签（SSR 输出里属性顺序固定）。 */
function buttonTag(html: string, title: string): string {
  const at = html.indexOf(`title="${title}"`)
  if (at < 0) throw new Error(`渲染结果里找不到 title 为 ${title} 的按钮`)
  return html.slice(html.lastIndexOf('<button', at), html.indexOf('>', at))
}

const quickTitle = `${t('sidebar.quickScan')} — ${t('sidebar.quickScanHint')}`
const fullTitle = `${t('sidebar.fullScan')} — ${t('sidebar.fullScanHint')}`

describe('ManagementSection 扫描入口（P1-4）', () => {
  it('自动入口与显式快速/完整入口同时在位，提示取自各自 i18n 键且可点', async () => {
    mock.progress.current = null
    const html = await render()

    expect(buttonTag(html, t('sidebar.rescan'))).toBeTruthy()
    // 提示必须真的说明差异（两个入口的 title 不同且都非空），否则用户无从分辨。
    expect(quickTitle).not.toBe(fullTitle)
    expect(buttonTag(html, quickTitle)).not.toContain('disabled')
    expect(buttonTag(html, fullTitle)).not.toContain('disabled')
  })

  it('扫描进行中：自动入口变为停止，两个显式入口 disabled（不重启在跑的轮次）', async () => {
    mock.progress.current = { isRunning: true }
    const html = await render()

    expect(buttonTag(html, t('sidebar.stopScan'))).toBeTruthy()
    expect(buttonTag(html, quickTitle)).toContain('disabled')
    expect(buttonTag(html, fullTitle)).toContain('disabled')
  })
})
