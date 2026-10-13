import { expect, it, vi } from 'vitest'
import { IPC } from '../constants/ipc'
import { useFolderRootActions, type FolderRootActionsDeps } from './useFolderRootActions'

const { invokeIpc, open } = vi.hoisted(() => ({ invokeIpc: vi.fn(), open: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }))
vi.mock('../utils/ipc', () => ({ invokeIpc, ipcErrorMessage: String, parseAppError: vi.fn() }))
vi.mock('vue', async (load) => ({
  ...await load<typeof import('vue')>(),
  onMounted: vi.fn(),
  onBeforeUnmount: vi.fn(),
}))

it('合并父文件夹只提交一次保留数据的导入，不逐个删除已有子库', async () => {
  open.mockResolvedValue('/parent')
  invokeIpc.mockResolvedValue({ children: [{ id: 1, path: '/parent/child', alias: null }], parents: [] })
  const addScanRoot = vi.fn().mockResolvedValue({ id: 2, path: '/parent' })
  const loadScanRoots = vi.fn().mockResolvedValue(true)
  const startScan = vi.fn().mockResolvedValue(undefined)
  const setPendingSelectRootId = vi.fn()
  const addToast = vi.fn()
  const actions = useFolderRootActions({
    scan: { addScanRoot, loadScanRoots, startScan },
    media: { loadStats: vi.fn() },
    toast: { addToast },
    confirm: vi.fn().mockResolvedValue({ confirmed: true, checkboxValue: true }),
    t: (key: string) => key,
    reloadTreePreserveExpansion: vi.fn(),
    setPendingSelectRootId,
  } as unknown as FolderRootActionsDeps)

  await actions.addRoot()

  expect(invokeIpc.mock.calls).toEqual([[IPC.CHECK_FOLDER_OVERLAP, { newPath: '/parent' }]])
  expect(addScanRoot).toHaveBeenCalledExactlyOnceWith('/parent', undefined, true)
  expect(setPendingSelectRootId).toHaveBeenCalledExactlyOnceWith(2)
  expect(loadScanRoots).toHaveBeenCalledOnce()
  expect(startScan).toHaveBeenCalledExactlyOnceWith(2, expect.any(Function))
  expect(addToast).not.toHaveBeenCalled()
})

it('关闭重叠确认会取消导入，不把取消解释为继续添加', async () => {
  open.mockResolvedValue('/parent')
  invokeIpc.mockResolvedValue({ children: [{ id: 1, path: '/parent/child', alias: null }], parents: [] })
  const addScanRoot = vi.fn()
  const actions = useFolderRootActions({
    scan: { addScanRoot },
    media: {},
    toast: { addToast: vi.fn() },
    confirm: vi.fn().mockResolvedValue({ confirmed: false, checkboxValue: true }),
    t: (key: string) => key,
    reloadTreePreserveExpansion: vi.fn(),
    setPendingSelectRootId: vi.fn(),
  } as unknown as FolderRootActionsDeps)

  await actions.addRoot()

  expect(addScanRoot).not.toHaveBeenCalled()
})
