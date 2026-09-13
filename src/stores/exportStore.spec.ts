// exportStore 单测(方案 A):镜像 scanStore 缩略图生成进度恢复的测试姿态——
// 快照 + app 事件的组合,是「Channel 随 webview 一起死」根治方案的同一套契约,须同等验证。
//  1. restoreExportProgress —— 先订阅事件、再查快照回填;running 快照能续上,事件到达能推进终态。
//  2. openExportDialog/closeExportDialog —— 对话框触发态的读写。
//  3. stopExport —— 只在当前 jobId 非空时才发 IPC(避免对空闲态发送无意义的取消请求)。

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { IPC, EVENTS } from '../constants/ipc'

const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
}))

// Tauri 事件桩:记录 handler 供测试内手动投递事件(同 scanStore.spec.ts 姿态)。listenMock 用
// vi.hoisted 声明(vi.mock 工厂会被提升到导入之前,只能引用同样被提升的绑定)——包成 vi.fn()
// 而非固定箭头函数,使个别用例(P3 防死锁回归)可用 mockImplementationOnce 模拟单次注册失败。
const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }))
const eventListeners = new Map<string, (e: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
}))

import { useExportStore } from './exportStore'

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
})

describe('exportStore 进度恢复(app 事件 + 快照,同缩略图生成姿态)', () => {
  it('快照 running → 回填运行态;随后 completed 事件到达 → 终态推进', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.EXPORT_STATUS
          ? { jobId: 'job1', status: 'running', processed: 3, total: 10 }
          : undefined,
      ),
    )
    const store = useExportStore()
    await store.restoreExportProgress()

    expect(store.isRunning).toBe(true)
    expect(store.progress.processed).toBe(3)
    expect(store.progress.total).toBe(10)

    // 事件流接续(监听已在 restore 时注册):completed 到达 → 终态。
    eventListeners.get(EVENTS.EXPORT_PROGRESS)?.({
      payload: { jobId: 'job1', status: 'completed', succeeded: 10, finalDir: '/out' },
    })
    expect(store.isRunning).toBe(false)
    expect(store.progress.status).toBe('completed')
    expect(store.progress.finalDir).toBe('/out')
  })

  it('快照 idle → 不覆盖初始态(避免用空快照抹掉尚未落地的运行态)', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.EXPORT_STATUS ? { jobId: '', status: 'idle' } : undefined),
    )
    const store = useExportStore()
    await store.restoreExportProgress()
    expect(store.progress.status).toBe('idle')
  })

  it('事件先到(completed)、同 jobId 的迟到 running 快照不得回退覆盖终态(P2 竞态防护)', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.EXPORT_STATUS
          ? { jobId: 'job1', status: 'running', processed: 3, total: 10 }
          : undefined,
      ),
    )
    const store = useExportStore()
    const restorePromise = store.restoreExportProgress()
    // ensureListener() 内 listen() 在异步函数体首个 await 前同步执行,此刻监听已挂上——
    // 在 EXPORT_STATUS 的 mock promise 落地前先投递 completed 事件,模拟"事件先到快照晚到"。
    eventListeners.get(EVENTS.EXPORT_PROGRESS)?.({
      payload: { jobId: 'job1', status: 'completed', succeeded: 10, finalDir: '/out' },
    })
    await restorePromise
    expect(store.progress.status).toBe('completed')
    expect(store.progress.finalDir).toBe('/out')
  })

  it('listen 注册单次失败后不永久死锁,下次调用会重新尝试(P3 防死锁)', async () => {
    listenMock.mockImplementationOnce(() => Promise.reject(new Error('listen boom')))
    const store = useExportStore()
    await expect(store.restoreExportProgress()).rejects.toThrow('listen boom')

    // 第二次调用:若失败的 Promise 被永久缓存,这里会复用同一个 rejected promise 再次抛错。
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.EXPORT_STATUS ? { jobId: '', status: 'idle' } : undefined),
    )
    await expect(store.restoreExportProgress()).resolves.toBeUndefined()
    expect(listenMock).toHaveBeenCalledTimes(2)
  })
})

describe('exportStore 对话框触发态', () => {
  it('openExportDialog 写入选区/来源并置 open=true;closeExportDialog 翻 open + 清空 rebuild', () => {
    const store = useExportStore()
    const selection = { kind: 'explicit' as const, ids: [1, 2, 3] }
    const rebuild = () => selection
    store.openExportDialog(selection, { kind: 'album', id: 9, name: '精选' }, rebuild)

    expect(store.dialogOpen).toBe(true)
    expect(store.dialogSelection).toEqual(selection)
    expect(store.dialogSource).toEqual({ kind: 'album', id: 9, name: '精选' })
    expect(store.dialogSelectionRebuild).toBe(rebuild)

    store.closeExportDialog()
    expect(store.dialogOpen).toBe(false)
    // 关闭不清空已记录的选区/来源(对话框可能被同一轮再次打开复用最近一次上下文)——
    // 但 rebuild 是"当次入口"的闭包,复用价值为负,须清空(见 exportStore.ts closeExportDialog 注释)。
    expect(store.dialogSelection).toEqual(selection)
    expect(store.dialogSelectionRebuild).toBeNull()
  })

  // 外部审查【严重】修复:ViewStale 重试此前无条件用当前选区重建,会把相册/视图导出静默换成
  // 选区。三入口(选区/相册/视图)各自注册 rebuild 回调,store 只管存取——分流逻辑落在
  // ExportDialog.start() 里,这里只验证 store 层的存取契约本身正确。
  it('openExportDialog 第三参按入口注册 rebuild 回调,写入 dialogSelectionRebuild', () => {
    const store = useExportStore()
    const rebuild = () => ({ kind: 'explicit' as const, ids: [4, 5] })
    store.openExportDialog({ kind: 'explicit', ids: [1] }, { kind: 'view', name: 'v' }, rebuild)
    expect(store.dialogSelectionRebuild).toBe(rebuild)
    expect(store.dialogSelectionRebuild?.()).toEqual({ kind: 'explicit', ids: [4, 5] })
  })

  it('rebuild 缺失(null)时 dialogSelectionRebuild 为 null——重试须走既有错误显示,不得回退借用选区', () => {
    const store = useExportStore()
    store.openExportDialog({ kind: 'explicit', ids: [1] }, { kind: 'selection' }, null)
    expect(store.dialogSelectionRebuild).toBeNull()
  })
})

// P3 特征化(复审要求补齐的自动化覆盖点):resolveViewStaleRetry 是从 ExportDialog.start() 的
// catch 分支抽出的可测种子,唯一输入是 dialogSelectionRebuild 的注册态——完全不接触任何选区
// 单例(useSelection),因此「不得借用当前选区」在这里是结构性保证,不是靠约定。旧写法(重试
// 无条件调 selection.toBackendDescriptor())若被无意间恢复,以下三例仍会全绿而测不出回归——
// 真正锁住回归的是 ExportDialog.vue:135 那行改成 store.resolveViewStaleRetry() 本身;这里锁住
// 的是"给定注册态,解析结果必须是什么",防止解析函数自己长出借用选区的分支。
describe('exportStore resolveViewStaleRetry(ViewStale 重试描述符解析)', () => {
  it('回调已注册且返回有效描述符:直接透传该描述符', () => {
    const store = useExportStore()
    const rebuilt = { kind: 'explicit' as const, ids: [7, 8] }
    store.openExportDialog(
      { kind: 'explicit', ids: [1] },
      { kind: 'album', id: 1, name: 'x' },
      () => rebuilt,
    )
    expect(store.resolveViewStaleRetry()).toEqual(rebuilt)
  })

  it('回调已注册但自身返回 null(如回调内语义搜索边缘态):原样透传 null,不做任何兜底', () => {
    const store = useExportStore()
    store.openExportDialog({ kind: 'explicit', ids: [1] }, { kind: 'selection' }, () => null)
    expect(store.resolveViewStaleRetry()).toBeNull()
  })

  it('回调未注册(该入口未接线,dialogSelectionRebuild 为 null):返回 null——重试失败,不借用选区', () => {
    const store = useExportStore()
    store.openExportDialog({ kind: 'explicit', ids: [1] }, { kind: 'view', name: 'v' }, null)
    expect(store.resolveViewStaleRetry()).toBeNull()
  })
})

describe('exportStore stopExport', () => {
  it('无运行中任务(jobId 为空)时不发 IPC', async () => {
    const store = useExportStore()
    await store.stopExport()
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.STOP_EXPORT, expect.anything())
  })

  it('有运行中任务时以当前 jobId 发 stop_export', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.EXPORT_STATUS ? { jobId: 'job7', status: 'running', total: 5 } : undefined,
      ),
    )
    const store = useExportStore()
    await store.restoreExportProgress()
    await store.stopExport()
    expect(invokeIpc).toHaveBeenCalledWith(IPC.STOP_EXPORT, { jobId: 'job7' })
  })
})
