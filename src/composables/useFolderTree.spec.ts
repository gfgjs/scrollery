// src/composables/useFolderTree.spec.ts
// 回归网:锁死「并发 loadChildren(sameParent) 只注入一次子节点」不变量。
// 背景 bug:groupBy=folder 滚动画廊时,FoldersSection 的两个 watch(activeDirectoryId /
// scrolledDirectoryId)对同一未展开祖先并发 expandToNode → loadChildren;旧实现在 await IPC
// 完成前才置 parent.expanded=true 且无在途去重,两次都读到 !expanded 各 splice 一批相同子节点,
// 于是拍平树出现重复目录行 → Vue "Duplicate keys found during update: dXXXX" 控制台刷屏。

import { describe, it, expect, beforeEach, vi } from 'vitest'

// mock 最外层 Tauri invoke(而非 utils/ipc 的 invokeIpc),让真实 invokeIpc 走完整路径(与
// usePluginEntitlement.spec 同款惯例)。用普通可变 handler 而非 vi.fn,避免 tinyspy 把 mock
// 拒绝值误报为 unhandled rejection。
type InvokeHandler = (cmd: string, args: unknown) => unknown
const { state } = vi.hoisted(() => ({ state: { handler: (() => undefined) as InvokeHandler } }))
const calls: Array<{ cmd: string; args: unknown }> = []
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: unknown) => {
    calls.push({ cmd, args })
    return state.handler(cmd, args)
  },
}))

import { useFolderTree } from './useFolderTree'
import type { DirNode } from '../types/media'

// 最小合法 DirNode 工厂(只填 useFolderTree 会读到的字段)。
// nodeKey/parentKey 照抄后端 `crate::tree::node_key` 的真实格式(`{rootId}:{relPath}`):本工厂
// 扮演的是后端响应,键必须与真后端同形(D-013)。
function node(
  id: number,
  parentId: number | null,
  depth: number,
  extra: Partial<DirNode> = {},
): DirNode {
  const relPath = 'dir' + id
  return {
    nodeKey: '1:' + relPath,
    parentKey: parentId === null ? null : '1:dir' + parentId,
    id,
    rootId: 1,
    parentId,
    name: relPath,
    relPath,
    depth,
    mediaCount: 0,
    hasChildren: false,
    ...extra,
  }
}

const PARENT_ID = 10
const CHILD_IDS = [20, 21, 22, 23]

// 默认路由:get_directory_tree 返回一个可展开的父目录;get_directory_children 返回其子目录。
function installDefaultHandler(childrenDelayMs = 0) {
  state.handler = (cmd, args) => {
    if (cmd === 'get_directory_tree') {
      return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
    }
    if (cmd === 'get_directory_children') {
      const a = args as { parentId: number }
      const kids = a.parentId === PARENT_ID ? CHILD_IDS.map((id) => node(id, PARENT_ID, 1)) : []
      if (childrenDelayMs <= 0) return Promise.resolve(kids)
      // 人为延迟,拉开两次并发调用的重叠窗口,逼真复现竞态。
      return new Promise((resolve) => setTimeout(() => resolve(kids), childrenDelayMs))
    }
    return Promise.resolve([])
  }
}

beforeEach(() => {
  calls.length = 0
  installDefaultHandler()
})


// loadChildren 收**节点**(取数只需路径身份,FS-only 目录没有 id)。测试按 id 找节点纯属方便 ——
// 这些用例造的都是 DB 模式的普通节点,id 与 nodeKey 一一对应。
const expandOf = (tree: ReturnType<typeof useFolderTree>, id: number) =>
  tree.loadChildren(tree.nodes.value.find((n) => n.id === id)!)

// ── 折叠 / 后代收集(2026-07-16 补:此前这条路径零覆盖)────────────────────────────
//
// characterization + 轴契约。补测的直接动因:P1-b 把树的结构轴从 DB id 换成 nodeKey(路径身份,
// D-013),而 collapseNode/collapseAll/getDescendantKeys 当时**没有任何测试**——改未测的关键行为
// 前先补网(项目规则)。

const loadRootsOf = (tree: ReturnType<typeof useFolderTree>) =>
  tree.loadRoots([{ id: 1, path: '/root' }] as unknown as Parameters<typeof tree.loadRoots>[0])

// ── 「所有文件」模式取数(S 线 §4 / P1-b-4)──────────────────────────────────────
//
// useFolderTree 是两种取数路径的**唯一分发点**:registeredOnly 走 DB 目录树命令(零回归),
// allFiles/allFilesWithHidden 走受限 FS 枚举 list_tree_entries。下面锁的是分发本身与 FS 路径的
// 分页契约。

/** 造一条后端 TreeEntry(可选字段默认**缺席**,与 Rust `skip_serializing_if` 同形)。 */
function fsEntry(name: string, kind: 'dir' | 'file', over: Record<string, unknown> = {}) {
  return {
    nodeKey: `1:dir${PARENT_ID}/${name}`,
    parentKey: `1:dir${PARENT_ID}`,
    rootId: 1,
    relPath: `dir${PARENT_ID}/${name}`,
    name,
    kind,
    hidden: false,
    registered: false,
    isSymlink: false,
    ...over,
  }
}

const treeCalls = () => calls.filter((c) => c.cmd === 'list_tree_entries')
const fsTree = () => useFolderTree(() => 'allFiles')


describe('useFolderTree 「所有文件」模式取数', () => {

  /**
   * 子**目录**必须一次给全:前端模型里目录不分页(分页的只有文件),这与 DB 模式
   * `get_directory_children` 返回全部子目录是同一个契约。故 FS 路径要循环翻页直到取尽。
   *
   * 可证伪性:后端分两页给,只取首页的实现会漏掉 `b`。
   */
  it('子目录循环翻页直到取尽(不止首页)', async () => {
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') {
        const cursor = (args as { cursor: number }).cursor
        return cursor === 0
          ? Promise.resolve({ entries: [fsEntry('a', 'dir')], nextCursor: 1, total: 2 })
          : Promise.resolve({ entries: [fsEntry('b', 'dir')], total: 2 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)

    expect(tree.nodes.value.map((n) => n.name)).toEqual(['dir' + PARENT_ID, 'a', 'b'])
    expect(treeCalls().filter((c) => (c.args as { kind: string }).kind === 'dirs')).toHaveLength(2)
  })

  /**
   * 翻页循环有意**不设页数上限**(DB 模式也不设,凭空封顶就是静默截断)。真正要防的死循环是
   * 后端游标不前进 —— 那是协议违约,单独判、单独喊。
   *
   * 可证伪性:没有该守卫的实现在此**永不返回**,用例直接超时。
   */
  it('后端游标不前进时中止翻页并报错(不死循环)', async () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    let served = 0
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') {
        // 熔断:没有守卫的实现会一直要下一页,直到把 node 的堆吃光(实测 8 秒后 OOM 崩测试进程)。
        // 那也算「被抓到」,但失败信息是一堆 V8 栈,读不出所以然。故 fixture 自己在第 5 页断供 ——
        // 变异体于是**干净地**撞在下面那句 toHaveLength(1) 上,一眼看得出是翻页没停。
        if (++served > 5) return Promise.reject(new Error('fixture 熔断:翻页未停止'))
        // 永远回同一个游标 0:协议违约。
        return Promise.resolve({ entries: [fsEntry('a', 'dir')], nextCursor: 0, total: 99 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)

    expect(treeCalls()).toHaveLength(1) // 拿到不前进的游标即止,不再请求
    expect(err).toHaveBeenCalled() // 静默中止 = 用户看到目录莫名少几个而无信号
    err.mockRestore()
  })
})
