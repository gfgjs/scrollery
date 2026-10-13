// useFolderTree.refreshDirectories 真行为测试：files:changed 原位刷新已加载父目录。
// 只覆盖与验收直接相关的行为：整段复用(保后代)/消失分支删除/文件范围回填/
// 实体 id 命中(同盘移动)权威子树键/整树换代时在途响应被丢弃。
import { expect, it, vi } from 'vitest'
import { computed, effectScope, nextTick } from 'vue'
import type { EventCallback } from '@tauri-apps/api/event'
import { IPC, EVENTS } from '../constants/ipc'
import type { DirFile, DirNode, ScanRoot } from '../types/media'

const { invokeIpc, eventHandlers } = vi.hoisted(() => ({
  invokeIpc: vi.fn(),
  eventHandlers: new Map<string, (payload: unknown) => void>(),
}))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('vue', async (importOriginal) => ({
  ...await importOriginal<typeof import('vue')>(),
  onMounted: vi.fn(),
  onBeforeUnmount: vi.fn(),
}))
vi.mock('./useTauriListen', () => ({
  useTauriListen: <T>(event: string, handler: EventCallback<T>) => {
    eventHandlers.set(event, (payload) => handler({ event, id: 1, payload: payload as T }))
  },
}))

import { useFolderTree } from './useFolderTree'
import { useFolderTreeSync, type FolderTreeSyncDeps } from './useFolderTreeSync'

function dir(partial: Partial<DirNode> & Pick<DirNode, 'nodeKey' | 'relPath' | 'depth'>): DirNode {
  return {
    parentKey: null,
    id: null,
    rootId: 1,
    parentId: null,
    name: partial.relPath.split('/').pop() ?? '',
    mediaCount: null,
    hasChildren: null,
    ...partial,
  }
}

function file(name: string): DirFile {
  return {
    nodeKey: '1:A/' + name,
    parentKey: '1:A',
    relPath: 'A/' + name,
    id: null,
    fileName: name,
    mediaType: null,
    isFavorited: false,
  }
}

const scanRoot = (id: number): ScanRoot => ({
  id,
  path: 'C:/r' + id,
  alias: null,
  scanStatus: 'idle',
  scanProgress: 0,
  totalFiles: 0,
  lastScanAt: null,
  isActive: true,
  createdAt: 0,
  updatedAt: 0,
  isHidden: false,
})

/** 构造含 root(已展开) / A(已展开,已加载文件) / A/B / C 的树。 */
async function buildTree(tree: ReturnType<typeof useFolderTree>) {
  const root = dir({ nodeKey: '1:', relPath: '', depth: 0, id: 1, parentKey: null })
  root.expanded = true
  root.hasChildren = true
  const a = dir({ nodeKey: '1:A', relPath: 'A', depth: 1, id: 10, parentKey: '1:', name: 'A' })
  a.expanded = true
  a.hasChildren = true
  a.filesLoaded = true
  a.files = [file('a1')]
  a.filesHasMore = false
  const ab = dir({ nodeKey: '1:A/B', relPath: 'A/B', depth: 2, id: 11, parentKey: '1:A', name: 'B' })
  const c = dir({ nodeKey: '1:C', relPath: 'C', depth: 1, id: 12, parentKey: '1:', name: 'C' })
  invokeIpc.mockResolvedValue([])
  await tree.loadRoots([scanRoot(1)])
  tree.nodes.value = [root, a, ab, c]
  return { root, a, ab, c }
}

it('原位刷新:复用整段保留已展开后代，删除消失分支，新增目录出现', async () => {
  const tree = useFolderTree()
  const scope = effectScope()
  eventHandlers.clear()
  invokeIpc.mockResolvedValue([])
  scope.run(() => useFolderTreeSync({
    folderTree: tree,
    scan: { visibleScanRoots: [] } as unknown as FolderTreeSyncDeps['scan'],
    ui: { groupBy: 'date', scrolledDirectoryId: null, treeDisplayMode: 'registeredOnly' } as unknown as FolderTreeSyncDeps['ui'],
    viewStore: { activeDirectoryId: null } as unknown as FolderTreeSyncDeps['viewStore'],
    toast: { addToast: vi.fn() } as unknown as FolderTreeSyncDeps['toast'],
    t: (key) => key,
    nodesById: computed(() => new Map<number, DirNode>()),
    scrollTreeToNodeKey: vi.fn().mockResolvedValue(undefined),
    navigateToFolder: vi.fn(),
  }))
  try {
    await nextTick()
    const { root, a, ab } = await buildTree(tree)
    const freshA = { ...a, mediaCount: 9 }
    const d = dir({ nodeKey: '1:D', relPath: 'D', depth: 1, id: 13, parentKey: '1:' })
    invokeIpc.mockImplementation((cmd: string, args: Record<string, unknown>) => Promise.resolve(
      cmd === IPC.GET_DIRECTORY_TREE ? [{ ...root, mediaCount: 10 }] :
        cmd === IPC.GET_DIRECTORY_CHILDREN ? (args.parentId === root.id ? [freshA, d] : [ab]) : [file('a1')],
    ))
    // 补全收尾的权威结果须自动落到已加载目录，保留分支身份与后代。
    eventHandlers.get(EVENTS.ENRICHMENT_COMPLETED)?.({ rootId: 1, runId: 'fs-watch-1', elapsedMs: 1 })
    await vi.waitFor(() => {
      expect(tree.nodes.value.map((node) => node.nodeKey)).toEqual(['1:', '1:A', '1:A/B', '1:D'])
    })
    expect(tree.nodes.value[1]).toBe(a)
    expect(tree.nodes.value[2]).toBe(ab)
    expect(a.mediaCount).toBe(9)
    expect(root.mediaCount).toBe(10)
  } finally {
    scope.stop()
    eventHandlers.clear()
  }
})

it('原位刷新:按已加载条数回填文件范围，旧分页不能追加到新范围', async () => {
  const tree = useFolderTree()
  const { root, a, c } = await buildTree(tree)
  a.filesHasMore = true
  let release: (rows: DirFile[]) => void = () => {}
  const oldPage = new Promise<DirFile[]>((resolve) => { release = resolve })
  invokeIpc.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === IPC.GET_DIRECTORY_TREE) return Promise.resolve([root])
    if (cmd === IPC.GET_DIRECTORY_CHILDREN) return Promise.resolve(args.parentId === root.id ? [a, c] : [])
    if (args.offset) return oldPage
    return Promise.resolve([file('new1'), file('a1')])
  })
  const pending = tree.loadMoreFiles(a)
  await tree.refreshDirectories(1, ['A'])
  release([file('obsolete')])
  await pending
  expect(a.filesLoaded).toBe(true)
  expect(a.files?.map((entry) => entry.fileName)).toEqual(['new1'])
  expect(a.filesHasMore).toBe(true)
})

it('原位刷新:跨父移动按实体身份保留分支，从后端读取权威路径键和文件', async () => {
  const tree = useFolderTree()
  const { root, a, ab, c } = await buildTree(tree)
  c.expanded = true
  const a2 = dir({ nodeKey: 'opaque-moved-folder', relPath: 'C/A2', depth: 2,
    id: a.id, parentKey: c.nodeKey, parentId: c.id, hasChildren: true })
  const ab2 = dir({ nodeKey: 'opaque-moved-child', relPath: 'C/A2/B', depth: 3,
    id: ab.id, parentKey: a2.nodeKey, parentId: a.id })
  const movedFile = { ...file('a1'), nodeKey: 'opaque-file', parentKey: a2.nodeKey, relPath: 'C/A2/a1' }
  invokeIpc.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === IPC.GET_DIRECTORY_TREE) return Promise.resolve([root])
    if (cmd === IPC.LIST_DIRECTORY_FILES) return Promise.resolve([movedFile])
    return Promise.resolve(args.parentId === root.id ? [c] : args.parentId === c.id ? [a2] : [ab2])
  })
  await tree.refreshDirectories(1, ['', 'A', 'C', 'C/A2'])
  expect(tree.nodes.value).toEqual([root, c, a, ab])
  expect(a.nodeKey).toBe(a2.nodeKey)
  expect(a.parentKey).toBe(c.nodeKey)
  expect(a.expanded).toBe(true)
  expect(ab.nodeKey).toBe(ab2.nodeKey)
  expect(ab.parentKey).toBe(a2.nodeKey)
  expect(ab.relPath).toBe(ab2.relPath)
  expect(ab.depth).toBe(3)
  expect(a.files?.[0]).toEqual(movedFile)
})

it('整树换代:在途的原位刷新响应被丢弃，不回写新树', async () => {
  const tree = useFolderTree()
  const { a } = await buildTree(tree)
  let release: (rows: DirNode[]) => void = () => {}
  const gate = new Promise<DirNode[]>((resolve) => { release = resolve })
  invokeIpc.mockImplementation(() => gate)
  const pending = tree.refreshDirectories(1, [''])
  await tree.loadRoots([])
  release([dir({ nodeKey: '1:D', relPath: 'D', depth: 1, id: 13 })])
  await pending
  expect(tree.nodes.value).toEqual([])
  expect(a.files?.map((entry) => entry.fileName)).toEqual(['a1'])
})

it('祖先折叠重开后，旧子目录请求不能展开同路径的新节点', async () => {
  const tree = useFolderTree()
  const { root, a, ab } = await buildTree(tree)
  a.expanded = false
  root.filesLoaded = true
  tree.nodes.value = [root, a]
  const replacement = { ...a }
  let release: (rows: DirNode[]) => void = () => {}
  const oldChildren = new Promise<DirNode[]>((resolve) => { release = resolve })
  invokeIpc.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === IPC.GET_DIRECTORY_CHILDREN && args.parentId === a.id) return oldChildren
    return Promise.resolve([replacement])
  })

  const oldExpansion = tree.toggleNode(a)
  await tree.toggleNode(root)
  await tree.toggleNode(root)
  expect(tree.nodes.value).toEqual([root, replacement])
  expect(replacement.expanded).toBe(false)

  release([ab])
  await oldExpansion
  expect(tree.nodes.value).toEqual([root, replacement])
  expect(replacement.expanded).toBe(false)
})

it.each(['success', 'failure'])('祖先折叠重开后，旧请求 %s 不影响新节点展开及并发去重', async (outcome) => {
  const onLoadError = vi.fn()
  const tree = useFolderTree(undefined, onLoadError)
  const { root, a, ab } = await buildTree(tree)
  a.expanded = false
  root.filesLoaded = true
  tree.nodes.value = [root, a]
  const replacement = { ...a }
  const currentChild = { ...ab, nodeKey: '1:A/new', relPath: 'A/new', name: 'new', id: 99 }
  let releaseOld: (rows: DirNode[]) => void = () => {}
  let rejectOld: (error: Error) => void = () => {}
  let releaseCurrent: (rows: DirNode[]) => void = () => {}
  const oldChildren = new Promise<DirNode[]>((resolve, reject) => {
    releaseOld = resolve
    rejectOld = reject
  })
  const currentChildren = new Promise<DirNode[]>((resolve) => { releaseCurrent = resolve })
  let childRequests = 0
  invokeIpc.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === IPC.GET_DIRECTORY_CHILDREN && args.parentId === a.id) {
      return ++childRequests === 1 ? oldChildren : currentChildren
    }
    return Promise.resolve([replacement])
  })

  const oldExpansion = tree.toggleNode(a)
  await tree.toggleNode(root)
  await tree.toggleNode(root)
  const currentExpansion = tree.toggleNode(replacement)
  const duplicate = tree.loadChildren(replacement)
  expect(childRequests).toBe(2)

  if (outcome === 'success') releaseOld([ab])
  else rejectOld(new Error('旧目录请求失败'))
  await oldExpansion
  expect(onLoadError).not.toHaveBeenCalled()
  const stillDuplicate = tree.loadChildren(replacement)
  expect(childRequests).toBe(2)
  expect(tree.nodes.value).toEqual([root, replacement])

  releaseCurrent([currentChild])
  await Promise.all([currentExpansion, duplicate, stillDuplicate])
  expect(tree.nodes.value).toEqual([root, replacement, currentChild])
  expect(replacement.expanded).toBe(true)
})
