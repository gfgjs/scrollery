// enhanceStore 单测（P0 批 5 + 批 5.5）：错误码→消息 key 映射、task 口径映射、队列拉取 +
// `enhance:queue-changed` 事件订阅刷新、start 记住参数并回拉队列、订阅计数守卫（多消费方共存
// 不互相打断监听）、非终态→终态跃迁弹 toast（仅在有订阅者时）。
// 环境镜像 exportStore.spec.ts：mock invokeIpc + @tauri-apps/api/event(listen)；
// 另 mock @tauri-apps/api/core 的 Channel（store 顶层导入，虽本测不走下载路径仍需可加载）。

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { IPC, EVENTS } from '../constants/ipc'
import type { EnhanceJob, EnhanceParams } from '../types/enhance'

const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
  ipcErrorMessage: (e: unknown) => String(e),
}))

// Channel 桩：store 顶层 `import { Channel }`，需可构造（本测不触发下载路径）。
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: unknown = null
  },
}))

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }))
const eventListeners = new Map<string, (e: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

// toast 桩：断言终态跃迁弹 toast 的类型/文案 key，而不依赖真实 toastStore/pinia 装配。
const { addToastMock } = vi.hoisted(() => ({ addToastMock: vi.fn() }))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast: addToastMock }) }))

// i18n 桩：只需 t() 回传 key（+ 参数，便于断言），不装配真实 vue-i18n 实例。
vi.mock('../i18n', () => ({
  default: {
    global: {
      t: (key: string, params?: Record<string, unknown>) =>
        params ? `${key}(${JSON.stringify(params)})` : key,
    },
  },
}))

import {
  useEnhanceStore,
  enhanceErrorMessageKey,
  modelTaskToStepTask,
  ENHANCE_DEFAULT_MODEL,
} from './enhanceStore'

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(() => Promise.resolve(undefined))
  eventListeners.clear()
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, cb: (e: { payload: unknown }) => void) => {
    eventListeners.set(name, cb)
    return Promise.resolve(() => {})
  })
  addToastMock.mockReset()
})

describe('enhanceErrorMessageKey 稳定码映射', () => {
  it('每个稳定码映射到独立的 enhance.* key', () => {
    expect(enhanceErrorMessageKey('enhance_input_too_large')).toBe('enhance.errInputTooLarge')
    expect(enhanceErrorMessageKey('enhance_unlicensed')).toBe('enhance.errUnlicensed')
    expect(enhanceErrorMessageKey('enhance_input_unsupported')).toBe('enhance.errInputUnsupported')
    expect(enhanceErrorMessageKey('enhance_model_missing')).toBe('enhance.errModelMissing')
    expect(enhanceErrorMessageKey('enhance_busy')).toBe('enhance.errBusy')
    expect(enhanceErrorMessageKey('enhance_download_failed')).toBe('enhance.errDownloadFailed')
    expect(enhanceErrorMessageKey('enhance_io')).toBe('enhance.errIo')
    expect(enhanceErrorMessageKey('enhance_invalid_params')).toBe('enhance.errInvalidParams')
    expect(enhanceErrorMessageKey('enhance_not_implemented')).toBe('enhance.errNotImplemented')
  })
  it('未知/空码回退通用 key', () => {
    expect(enhanceErrorMessageKey('something_else')).toBe('enhance.errUnknown')
    expect(enhanceErrorMessageKey(null)).toBe('enhance.errUnknown')
    expect(enhanceErrorMessageKey(undefined)).toBe('enhance.errUnknown')
  })
})

describe('modelTaskToStepTask 口径映射', () => {
  it('去伪影 camelCase → snake_case，其余不变', () => {
    expect(modelTaskToStepTask('dejpegArtifact')).toBe('dejpeg_artifact')
    expect(modelTaskToStepTask('denoise')).toBe('denoise')
    expect(modelTaskToStepTask('upscale')).toBe('upscale')
  })
  it('默认档常量与设计一致', () => {
    expect(ENHANCE_DEFAULT_MODEL.denoise).toBe('scunet')
    expect(ENHANCE_DEFAULT_MODEL.dejpegArtifact).toBe('fbcnn')
    expect(ENHANCE_DEFAULT_MODEL.upscale).toBe('realesrgan-x4plus')
  })
})

function makeQueue(overrides: Partial<EnhanceJob> = {}): EnhanceJob[] {
  return [
    {
      id: 1,
      status: 'running',
      total: 3,
      done: 1,
      tileDone: 5,
      tilesTotal: 20,
      errorCode: null,
      ...overrides,
    },
  ]
}

describe('enhanceStore 队列拉取与事件订阅', () => {
  it('fetchQueue 用快照回填 queue（含 tilesTotal/tileDone 渲染数据映射）', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? makeQueue() : undefined),
    )
    const store = useEnhanceStore()
    await store.fetchQueue()
    expect(store.queue).toHaveLength(1)
    expect(store.queue[0].id).toBe(1)
    expect(store.queue[0].tileDone).toBe(5)
    expect(store.queue[0].tilesTotal).toBe(20)
    expect(store.hasActiveJob).toBe(true)
  })

  it('subscribeQueue 注册 enhance:queue-changed 监听并初拉一次；事件到达再刷新', async () => {
    let snapshot: EnhanceJob[] = []
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? snapshot : undefined),
    )
    const store = useEnhanceStore()
    await store.subscribeQueue()
    // 订阅即挂上监听 + 初拉一次（此刻空快照）。
    expect(listenMock).toHaveBeenCalledWith(EVENTS.ENHANCE_QUEUE_CHANGED, expect.any(Function))
    expect(store.queue).toHaveLength(0)

    // 后端发事件 → 前端拉全量刷新。
    snapshot = makeQueue()
    eventListeners.get(EVENTS.ENHANCE_QUEUE_CHANGED)?.({ payload: undefined })
    await vi.waitFor(() => expect(store.queue).toHaveLength(1))
  })
})

describe('enhanceStore.start 记住参数并回拉队列', () => {
  it('start 成功返回 jobId、记 lastParams、随后 fetchQueue', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.ENHANCE_START ? 7 : cmd === IPC.GET_ENHANCE_QUEUE ? makeQueue() : undefined,
      ),
    )
    const params: EnhanceParams = {
      steps: [{ task: 'denoise', model_id: 'scunet' }],
      outputFormat: 'follow_source',
    }
    const store = useEnhanceStore()
    const jobId = await store.start([10, 11], params)
    expect(jobId).toBe(7)
    expect(store.lastParams).toEqual(params)
    expect(store.queue).toHaveLength(1)
    expect(invokeIpc).toHaveBeenCalledWith(IPC.ENHANCE_START, { itemIds: [10, 11], params })
  })
})

describe('enhanceStore 订阅计数守卫（多消费方共存）', () => {
  it('两个订阅者叠加只挂一次监听；一方 unsubscribe 后监听仍在（另一方还在用）', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? [] : undefined),
    )
    const store = useEnhanceStore()
    await store.subscribeQueue() // 面板 A（如设置分节）
    await store.subscribeQueue() // 面板 B（如对话框）
    expect(listenMock).toHaveBeenCalledTimes(1) // 只注册一次真实监听

    store.unsubscribeQueue() // 面板 A 卸载
    // 监听未被拆——事件仍应能触发刷新（若被误拆，下方拉取不会发生）。
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? makeQueue() : undefined),
    )
    eventListeners.get(EVENTS.ENHANCE_QUEUE_CHANGED)?.({ payload: undefined })
    await vi.waitFor(() => expect(store.queue).toHaveLength(1))

    store.unsubscribeQueue() // 面板 B 卸载（最后一个）——不断言 unlisten 内部句柄，只验计数不下溢。
    expect(() => store.unsubscribeQueue()).not.toThrow() // 计数已 0，再次卸载不应下溢报错
  })
})

describe('enhanceStore 终态跃迁 toast', () => {
  it('job 从 running 跃迁到 done 时,有订阅者才弹 success toast', async () => {
    let snapshot: EnhanceJob[] = makeQueue({ status: 'running' })
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? snapshot : undefined),
    )
    const store = useEnhanceStore()
    await store.subscribeQueue() // 有订阅者
    expect(addToastMock).not.toHaveBeenCalled()

    snapshot = makeQueue({ status: 'done' })
    await store.fetchQueue()
    expect(addToastMock).toHaveBeenCalledWith('success', expect.stringContaining('enhance.toastJobDone'))
  })

  it('job 从 running 跃迁到 error 时弹 error toast,文案走 enhanceErrorMessageKey 映射', async () => {
    let snapshot: EnhanceJob[] = makeQueue({ status: 'running' })
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? snapshot : undefined),
    )
    const store = useEnhanceStore()
    await store.subscribeQueue()

    snapshot = makeQueue({ status: 'error', errorCode: 'enhance_busy' })
    await store.fetchQueue()
    expect(addToastMock).toHaveBeenCalledWith('error', 'enhance.errBusy')
  })

  it('无订阅者时(subscriberCount=0)不弹 toast——防 Dialog 关闭后静默触发', async () => {
    let snapshot: EnhanceJob[] = makeQueue({ status: 'running' })
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? snapshot : undefined),
    )
    const store = useEnhanceStore()
    await store.fetchQueue() // 未订阅，仅手动拉取，建立初始「running」已知态

    snapshot = makeQueue({ status: 'done' })
    await store.fetchQueue()
    expect(addToastMock).not.toHaveBeenCalled()
  })

  it('退订归零后台转终态，重订阅首帧不补爆 toast（不滞后补报裁决）', async () => {
    let snapshot: EnhanceJob[] = makeQueue({ status: 'running' })
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.GET_ENHANCE_QUEUE ? snapshot : undefined),
    )
    const store = useEnhanceStore()
    await store.subscribeQueue() // 面板挂载，已知态记为 running
    expect(addToastMock).not.toHaveBeenCalled()

    store.unsubscribeQueue() // 唯一订阅者卸载，计数归零 → lastKnownStatus 清空

    // 0 订阅窗口内，job 在后台转为终态（无消费方观察，快照未被拉取）。
    snapshot = makeQueue({ status: 'done' })

    // 重新挂载订阅：首帧 fetchQueue 因已知态被清空，不存在「running」旧值可比对，
    // 故不应把这次「从无到 done」误判为可弹的跃迁。
    await store.subscribeQueue()
    expect(addToastMock).not.toHaveBeenCalled()
    expect(store.queue[0].status).toBe('done')
  })
})
