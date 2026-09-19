// aiStore 首次使用「AI 模型未下载」引导回归（真实 useAnalysisController + IPC 契约）。
//
// 契约：start/restart 被 AiModelNotLoaded 拒绝 → 弹 useConfirm 指引「设置 → AI → 模型库」，
// 确认后只导航（不代下载），取消即静默；其它拒绝照旧走 error toast。侧栏与语义搜索面板的
// 起步命令都经 aiStore 这两条动作，故分流在此覆盖两侧入口。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'
import { IpcError } from '../utils/ipc'

const invokeIpc = vi.hoisted(() => vi.fn())
const addToast = vi.hoisted(() => vi.fn())
const push = vi.hoisted(() => vi.fn(() => Promise.resolve()))

vi.mock('../utils/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/ipc')>()
  return { ...actual, invokeIpc }
})
vi.mock('../utils/logger', () => ({
  logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}))
// 起步预检（startAnalysis 的扫描根守卫）用桩：本例覆盖模型缺失引导，不覆盖扫描根。
vi.mock('../stores/scanStore', () => ({ useScanStore: () => ({ hasScanRoots: true }) }))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('../router', () => ({ default: { push } }))
// i18n 桩返回键本身：断言比对键（键是否存在于两份字典由 localeIntegrity.spec.ts 锁）。
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
import { useAiStore } from './aiStore'
import { useConfirmDialogState } from '../composables/useConfirm'

const MODEL_NOT_LOADED = new IpcError('AiModelNotLoaded', 'AI 模型尚未下载:vit-b-16.img.fp16.onnx')

/** 引导是 onError（同步回调）里 fire-and-forget 的 promise 链，且按需 import 路由：多等一拍。 */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

function statusSnapshot() {
  return {
    provider: 'cpu',
    gpuName: '',
    vramGb: 0,
    batchSize: 8,
    activeFixedBatch: null,
    clipLoaded: false,
    totalItems: 5,
    analyzedItems: 0,
    pendingItems: 5,
    errorItems: 0,
    isAnalyzing: false,
    analysisActive: false,
    waitingOn: [],
  }
}

/** 假后端：start/restart 按给定错误拒绝（undefined = 接受）。 */
function installBackend(start: unknown, restart: unknown = start) {
  invokeIpc.mockImplementation((cmd: string) => {
    if (cmd === IPC.GET_AI_STATUS) return Promise.resolve(statusSnapshot())
    if (cmd === IPC.START_AI_ANALYSIS) {
      return start ? Promise.reject(start) : Promise.resolve(undefined)
    }
    if (cmd === IPC.RESTART_AI_ANALYSIS) {
      return restart ? Promise.reject(restart) : Promise.resolve(undefined)
    }
    return Promise.resolve(undefined)
  })
}

describe('aiStore 模型未下载引导', () => {
  const dialog = useConfirmDialogState()

  beforeEach(() => {
    // uiStore setup 同步读 window.matchMedia（node 环境无 window，同 aiStore.spec 的桩）。
    vi.stubGlobal('window', {
      matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
      location: { search: '' },
      addEventListener: () => {},
      removeEventListener: () => {},
    })
    setActivePinia(createPinia())
    invokeIpc.mockReset()
    addToast.mockReset()
    push.mockClear()
    dialog.close(false) // 归零上一次用例可能遗留的共享单例状态
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('模型缺失：不启动、不报 error toast，弹出下载引导确认框', async () => {
    installBackend(MODEL_NOT_LOADED)
    const ai = useAiStore()

    await ai.startAnalysis()

    expect(dialog.state.isOpen).toBe(true)
    expect(dialog.state.title).toBe('semantic.modelMissingTitle')
    expect(dialog.state.message).toBe('semantic.modelMissingMsg')
    expect(dialog.state.confirmText).toBe('semantic.modelMissingGo')
    expect(dialog.state.cancelText).toBe('common.cancel')
    expect(ai.status.isAnalyzing).toBe(false)
    expect(ai.waitingForSession).toBe(false)
    expect(addToast).not.toHaveBeenCalled()
  })

  it('确认 → 导航到设置 AI 模型库，且不代用户下载', async () => {
    installBackend(MODEL_NOT_LOADED)
    const ai = useAiStore()

    await ai.startAnalysis()
    dialog.close(true)
    await flush()

    expect(push).toHaveBeenCalledWith('/settings/ai')
    expect(invokeIpc.mock.calls.map((c) => c[0])).not.toContain(IPC.DOWNLOAD_MODEL)
  })

  it('取消 → 不导航', async () => {
    installBackend(MODEL_NOT_LOADED)
    const ai = useAiStore()

    await ai.startAnalysis()
    dialog.close(false)
    await flush()

    expect(push).not.toHaveBeenCalled()
  })

  it('重新开始（破坏性重置）被同一 code 拒绝时同样走引导', async () => {
    installBackend(new IpcError('Internal', 'never'), MODEL_NOT_LOADED)
    const ai = useAiStore()

    await ai.restartAnalysis()

    expect(dialog.state.isOpen).toBe(true)
    expect(addToast).not.toHaveBeenCalled()
  })

  it('模型齐全：正常启动，不弹引导', async () => {
    installBackend(undefined)
    const ai = useAiStore()

    await ai.startAnalysis()

    expect(dialog.state.isOpen).toBe(false)
    expect(ai.status.isAnalyzing).toBe(true)
    expect(addToast).not.toHaveBeenCalled()
  })

  it('其它拒绝不被引导劫持：仍走 error toast', async () => {
    installBackend(new IpcError('Internal', 'dispatch failed'))
    const ai = useAiStore()

    await ai.startAnalysis()

    expect(dialog.state.isOpen).toBe(false)
    expect(addToast).toHaveBeenCalledWith('error', 'dispatch failed')
  })
})
