// 管理区「未完成的移动」清单的渲染契约（P0-1）。
//
// 关注点：这条清单是用户唯一能看到「文件到底在哪、还剩什么没做完、怎么重试」的地方，
// 故断言的是**用户实际读到的内容**：真实落点路径、仍需收尾的原因、重试按钮，以及同一日志 id
// 重试运行中按钮禁用（防重复点击）。环境：SSR renderToString + 真实 i18n（项目未装
// @vue/test-utils，与其它 UI contract 测试同款只做渲染断言）。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import zhCN from '../../../i18n/locales/zh-CN'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))
vi.mock('../../../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

import { IPC } from '../../../constants/ipc'
import type { DirectoryMoveRecovery } from '../../../types/ipc'
import { useDirectoryMoveRecoveryStore } from '../../../stores/directoryMoveRecoveryStore'
import DirectoryMoveRecoveryList from './DirectoryMoveRecoveryList.vue'

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })
const t = i18n.global.t

const PENDING: DirectoryMoveRecovery = {
  recoveryId: 42,
  stage: 'published',
  sourceName: '旅行',
  sourceAbsPath: 'C:/photos/旅行',
  targetAbsPath: 'D:/archive/旅行',
  targetRelPath: '归档/旅行',
  targetRootId: 5,
  needsRetry: true,
  detail: 'target_conflict',
}

async function render(): Promise<string> {
  const app = createSSRApp({
    setup: () => () => h(DirectoryMoveRecoveryList as Component),
  })
  app.use(i18n)
  return renderToString(app)
}

beforeEach(() => {
  setActivePinia(createPinia())
  invoke.mockReset()
  vi.stubGlobal('window', { dispatchEvent: vi.fn() })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('DirectoryMoveRecoveryList', () => {
  it('没有未完成项时整块不出现', async () => {
    const html = await render()
    expect(html).not.toContain(t('sidebar.dirRecovery.title'))
  })

  it('显示文件真实落点、仍需收尾的原因与重试入口', async () => {
    useDirectoryMoveRecoveryStore().items = [PENDING]

    const html = await render()

    expect(html).toContain(t('sidebar.dirRecovery.title'))
    expect(html).toContain('D:/archive/旅行') // 真实落点，不是「操作失败」
    expect(html).toContain(t('sidebar.dirRecovery.detail.target_conflict'))
    expect(html).toContain(t('sidebar.dirRecovery.retry'))
    // 路径可能被省略号截断，完整值必须仍可从 title 读到。
    expect(html).toContain('title="D:/archive/旅行"')
  })

  it('同一日志 id 重试运行中：按钮禁用并显示进行中文案（不重复发起）', async () => {
    invoke.mockImplementation(() => new Promise(() => {})) // 重试悬在途
    const recovery = useDirectoryMoveRecoveryStore()
    recovery.items = [PENDING]
    void recovery.retry(42)

    const html = await render()

    // 重试按钮同时承载进行中文案与禁用态：切片取到含该文案的那个 <button> 开标签。
    const at = html.indexOf(t('sidebar.dirRecovery.retrying'))
    const open = html.lastIndexOf('<button', at)
    const button = html.slice(open, html.indexOf('>', open))
    expect(button).toContain('disabled')
    expect(button).toContain('btn-secondary')
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke.mock.calls[0]?.[0]).toBe(IPC.RETRY_DIRECTORY_MOVE)
  })
})
