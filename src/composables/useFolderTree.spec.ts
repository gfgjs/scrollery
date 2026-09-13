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
import type { DirFile, DirNode, TreeCategory } from '../types/media'

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

const childCalls = () => calls.filter((c) => c.cmd === 'get_directory_children').length
const idsOf = (tree: ReturnType<typeof useFolderTree>) => tree.nodes.value.map((n) => n.id)

// loadChildren 收**节点**(取数只需路径身份,FS-only 目录没有 id)。测试按 id 找节点纯属方便 ——
// 这些用例造的都是 DB 模式的普通节点,id 与 nodeKey 一一对应。
const expandOf = (tree: ReturnType<typeof useFolderTree>, id: number) =>
  tree.loadChildren(tree.nodes.value.find((n) => n.id === id)!)

describe('useFolderTree 分类参数', () => {
  it('registeredOnly 的根目录与子目录查询都携带当前分类', async () => {
    const tree = useFolderTree(
      () => 'registeredOnly',
      undefined,
      () => ['document'] as const,
    )

    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)

    const directoryCalls = calls.filter(
      ({ cmd }) => cmd === 'get_directory_tree' || cmd === 'get_directory_children',
    )
    expect(directoryCalls).toHaveLength(2)
    expect(directoryCalls.map(({ args }) => (args as { categories: TreeCategory[] }).categories)).toEqual([
      ['document'],
      ['document'],
    ])
  })
})

describe('useFolderTree.loadChildren 并发去重', () => {
  it('并发调用同一 parentId:子节点只注入一次,IPC 只发一次', async () => {
    installDefaultHandler(5) // 延迟放大重叠窗口
    const tree = useFolderTree()
    await tree.loadRoots([{ id: 1, path: '/root' }] as unknown as Parameters<typeof tree.loadRoots>[0])

    // 两次并发加载同一父目录(模拟两个 watch 同帧触发 expandToNode)。
    const parent = tree.nodes.value.find((n) => n.id === PARENT_ID)!
    await Promise.all([tree.loadChildren(parent), tree.loadChildren(parent)])

    const ids = idsOf(tree)
    // 每个子 id 恰好出现一次(重复注入会让某 id 出现两次 → 旧实现在此炸)。
    for (const cid of CHILD_IDS) {
      expect(ids.filter((x) => x === cid)).toHaveLength(1)
    }
    // 拍平树里全体 id 无重复。
    expect(new Set(ids).size).toBe(ids.length)
    // 去重后底层 IPC 只发一次(第二个调用复用在途 Promise,不再打后端)。
    expect(childCalls()).toBe(1)
    // 父目录被标记展开。
    expect(tree.nodes.value.find((n) => n.id === PARENT_ID)?.expanded).toBe(true)
  })

  it('第二道防线:加载完成后再次直接 loadChildren 同一父目录,不产生重复行', async () => {
    const tree = useFolderTree()
    await tree.loadRoots([{ id: 1, path: '/root' }] as unknown as Parameters<typeof tree.loadRoots>[0])

    await expandOf(tree, PARENT_ID) // 第一次:注入子节点
    await expandOf(tree, PARENT_ID) // 第二次(串行,在途表已清):existing 过滤兜底

    const ids = idsOf(tree)
    expect(new Set(ids).size).toBe(ids.length)
    for (const cid of CHILD_IDS) {
      expect(ids.filter((x) => x === cid)).toHaveLength(1)
    }
  })

  /**
   * 🔴 轴契约(D-013):注入判重必须按 **nodeKey(路径身份)** 而非实体 id。
   *
   * 判重是拿**已在树中**的节点建 `existing`,故要证伪就必须让**新来的子节点**与**已在树中的
   * 父节点**实体 id 相同、路径不同 —— 这与「所有文件」模式下 FS-only 子树的退化**同构**:
   * 父与子都没有 DB 行、id 全是空值,按 id 判重时 `existing.has(空值)` 恒真,于是**整批子节点
   * 被当成父的重复全部丢弃**,目录展开后空无一物。
   *
   * 补这条的直接原因:变异验证发现「判重退回 id 轴」**无人捕获** —— 正确答案早就写在
   * useFolderTree.ts 的注释里,却没有任何断言接住它(F-016 同款)。DB 模式下 id 与 nodeKey
   * 一一对应,普通 fixture 对两条轴都通过,必须专门造出二者分歧的形状才谈得上可证伪。
   */
  it('注入判重走 nodeKey:子节点与已在树中的父节点实体 id 相同时不得被当成重复丢弃', async () => {
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'get_directory_children') {
        const a = args as { parentId: number }
        if (a.parentId !== PARENT_ID) return Promise.resolve([])
        // 子节点复用父的 id(模拟「实体身份缺失 → 退化成同一个值」),但路径身份各不相同。
        return Promise.resolve([
          node(PARENT_ID, PARENT_ID, 1, { nodeKey: '1:dir10/alpha' }),
          node(PARENT_ID, PARENT_ID, 1, { nodeKey: '1:dir10/beta' }),
        ])
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)

    const keys = tree.nodes.value.map((n) => n.nodeKey)
    expect(keys).toContain('1:dir10/alpha')
    expect(keys).toContain('1:dir10/beta')
  })

  it('单次加载正常注入全部子节点(未回归基本功能)', async () => {
    const tree = useFolderTree()
    await tree.loadRoots([{ id: 1, path: '/root' }] as unknown as Parameters<typeof tree.loadRoots>[0])
    await expandOf(tree, PARENT_ID)

    const ids = idsOf(tree)
    expect(ids).toContain(PARENT_ID)
    for (const cid of CHILD_IDS) expect(ids).toContain(cid)
    expect(childCalls()).toBe(1)
  })
})

// ── 折叠 / 后代收集(2026-07-16 补:此前这条路径零覆盖)────────────────────────────
//
// characterization + 轴契约。补测的直接动因:P1-b 把树的结构轴从 DB id 换成 nodeKey(路径身份,
// D-013),而 collapseNode/collapseAll/getDescendantKeys 当时**没有任何测试**——改未测的关键行为
// 前先补网(项目规则)。

const loadRootsOf = (tree: ReturnType<typeof useFolderTree>) =>
  tree.loadRoots([{ id: 1, path: '/root' }] as unknown as Parameters<typeof tree.loadRoots>[0])

describe('useFolderTree 折叠', () => {
  it('折叠目录:移除其全部后代行,自身留下且 expanded=false', async () => {
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    expect(idsOf(tree)).toHaveLength(1 + CHILD_IDS.length)

    const parent = tree.nodes.value.find((n) => n.id === PARENT_ID)!
    await tree.toggleNode(parent) // 已 expanded → 折叠

    expect(idsOf(tree)).toEqual([PARENT_ID])
    expect(tree.nodes.value[0].expanded).toBe(false)
  })

  it('折叠递归吃掉多层后代(孙节点也移除)', async () => {
    // 20 之下再挂一层孙节点 30/31。
    const GRAND = [30, 31]
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'get_directory_children') {
        const a = args as { parentId: number }
        if (a.parentId === PARENT_ID) {
          return Promise.resolve([node(20, PARENT_ID, 1, { hasChildren: true })])
        }
        if (a.parentId === 20) return Promise.resolve(GRAND.map((id) => node(id, 20, 2)))
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    await expandOf(tree, 20)
    expect(idsOf(tree)).toEqual([PARENT_ID, 20, 30, 31])

    await tree.toggleNode(tree.nodes.value.find((n) => n.id === PARENT_ID)!)
    // 孙节点必须随祖先折叠一并移除。
    expect(idsOf(tree)).toEqual([PARENT_ID])
  })

  it('collapseAll:只留扫描根行,且各根 expanded 清零', async () => {
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    tree.collapseAll()

    expect(idsOf(tree)).toEqual([PARENT_ID])
    expect(tree.nodes.value[0].expanded).toBe(false)
  })

  /**
   * 🔴 轴契约(D-013):后代收集必须走 **parentKey(路径身份)** 而非 parentId。
   *
   * 本用例的 fixture 有意造出 **parentId 为 null 而 parentKey 有值** 的子节点 —— 这正是
   * 「所有文件」模式下 FS-only 目录的父链形态(磁盘上有、库里没有 → 没有 DB 行 id,父子关系
   * 只由路径表达)。用 parentId 轴走后代的实现在这里会**收不到任何后代**,折叠后子行残留。
   *
   * 这是本文件里唯一能在**今天**证伪轴选择的断言:DB 模式下 id 与 nodeKey 一一对应,任何
   * 只用正常 DB 节点的测试对两条轴都会通过,区分不出来。
   */
  it('后代收集走 parentKey:父链只由路径表达(parentId 为 null)时仍能正确折叠', async () => {
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'get_directory_children') {
        const a = args as { parentId: number }
        if (a.parentId !== PARENT_ID) return Promise.resolve([])
        // parentId: null(无 DB 父行) + parentKey 指向 PARENT_ID 的路径键。
        return Promise.resolve(
          CHILD_IDS.map((id) => node(id, null, 1, { parentKey: '1:dir' + PARENT_ID })),
        )
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    expect(idsOf(tree)).toHaveLength(1 + CHILD_IDS.length)

    await tree.toggleNode(tree.nodes.value.find((n) => n.id === PARENT_ID)!)
    expect(idsOf(tree)).toEqual([PARENT_ID])
  })

  /**
   * 同上的另一半:`collapseAll` 的「是扫描根」判据也必须走 parentKey。
   * parentId 恒 null 的子节点若被当成根行留下,collapseAll 就成了 no-op。
   */
  it('collapseAll 的根判据走 parentKey:parentId 恒 null 的子节点不得被当成根留下', async () => {
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'get_directory_children') {
        const a = args as { parentId: number }
        if (a.parentId !== PARENT_ID) return Promise.resolve([])
        return Promise.resolve(
          CHILD_IDS.map((id) => node(id, null, 1, { parentKey: '1:dir' + PARENT_ID })),
        )
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    tree.collapseAll()

    expect(idsOf(tree)).toEqual([PARENT_ID])
  })
})

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

function file(name: string, mediaType: DirFile['mediaType'] = 'image'): DirFile {
  return {
    nodeKey: `1:dir${PARENT_ID}/${name}`,
    parentKey: `1:dir${PARENT_ID}`,
    relPath: `dir${PARENT_ID}/${name}`,
    id: 1,
    fileName: name,
    mediaType,
    isFavorited: false,
  }
}

describe('useFolderTree 树分类筛选', () => {
  it('DB 文件页把独立分类传给 IPC，而不是在前端过滤已分页结果', async () => {
    const categories: TreeCategory[] = ['audio', 'other']
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { mediaCount: 1 })])
      }
      if (cmd === 'list_directory_files') return Promise.resolve([])
      return Promise.resolve([])
    }
    const tree = useFolderTree(() => 'registeredOnly', undefined, () => categories)
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])

    const call = calls.find((item) => item.cmd === 'list_directory_files')
    expect(call?.args).toMatchObject({
      directoryId: PARENT_ID,
      limit: 201,
      offset: 0,
      categories: ['audio', 'other'],
    })
  })

  it('FS 文件页同样携带分类，目录请求保持可导航', async () => {
    const categories: TreeCategory[] = ['other']
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') return Promise.resolve({ entries: [], total: 0 })
      return Promise.resolve([])
    }
    const tree = useFolderTree(() => 'allFiles', undefined, () => categories)
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])

    const treeCallsForCategory = treeCalls().filter((item) => {
      const args = item.args as { categories?: TreeCategory[] }
      return args.categories?.join(',') === 'other'
    })
    expect(treeCallsForCategory.length).toBeGreaterThanOrEqual(2)
    expect(
      treeCallsForCategory.every((item) =>
        Array.isArray((item.args as { categories: TreeCategory[] }).categories),
      ),
    ).toBe(true)
  })

  it('分类切换清空旧页并拒绝旧 IPC 回写', async () => {
    let categories: TreeCategory[] = ['image']
    const releases: Array<(rows: DirFile[]) => void> = []
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { mediaCount: 1 })])
      }
      if (cmd === 'list_directory_files') {
        return new Promise<DirFile[]>((resolve) => releases.push(resolve))
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree(() => 'registeredOnly', undefined, () => categories)
    await loadRootsOf(tree)
    const firstExpand = tree.toggleNode(tree.nodes.value[0])
    await vi.waitFor(() => expect(releases).toHaveLength(1))

    categories = ['video']
    const reload = tree.reloadFilesForFilter()
    await vi.waitFor(() => expect(releases).toHaveLength(2))
    releases[1]([file('new.mp4', 'video')])
    await reload
    // 旧响应最后到达，也不得覆盖新分类的结果。
    releases[0]([file('old.jpg', 'image')])
    await firstExpand

    expect(tree.nodes.value[0].files?.map((item) => item.fileName)).toEqual(['new.mp4'])
    const requests = calls.filter((item) => item.cmd === 'list_directory_files')
    expect((requests[0].args as { categories: TreeCategory[] }).categories).toEqual(['image'])
    expect((requests[1].args as { categories: TreeCategory[] }).categories).toEqual(['video'])
  })
})

describe('useFolderTree 「所有文件」模式取数', () => {
  it('默认取值器是 registeredOnly:不传模式 = 明确要 DB 路径(选择器对话框的契约)', async () => {
    const tree = useFolderTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)
    expect(childCalls()).toBe(1)
    expect(treeCalls()).toHaveLength(0)
  })

  it('FS 模式子目录走 list_tree_entries(kind=dirs + 路径身份),不打 DB 子目录命令', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') return Promise.resolve({ entries: [], total: 0 })
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await expandOf(tree, PARENT_ID)

    expect(childCalls()).toBe(0) // DB 快路径一次都不该走
    const dirCall = treeCalls().find((c) => (c.args as { kind: string }).kind === 'dirs')
    // 入参是**路径身份**(rootId + relPath),不是 DB id —— 这正是 FS-only 目录也能展开的原因。
    expect(dirCall?.args).toMatchObject({
      rootId: 1,
      relPath: 'dir' + PARENT_ID,
      mode: 'allFiles',
      kind: 'dirs',
      cursor: 0,
    })
  })

  /**
   * 🔴 本模式的**核心能力**:磁盘上有、库里没有的目录必须能展开。
   *
   * 可证伪性:父节点 `id: null`。任何「先拿 id 再取数」的实现(包括改造前那句
   * `if (node.hasChildren !== false && node.id !== null)`)在这里一个子节点都拉不出来。
   */
  it('FS-only 目录(无 DB 行,id=null)照样能展开出子节点', async () => {
    const FS_ONLY = node(PARENT_ID, null, 0, { hasChildren: null })
    const orphan = { ...FS_ONLY, id: null } as DirNode
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') return Promise.resolve([orphan])
      if (cmd === 'list_tree_entries') {
        return Promise.resolve({ entries: [fsEntry('sub', 'dir')], total: 1 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await tree.loadChildren(tree.nodes.value[0])

    expect(tree.nodes.value.map((n) => n.nodeKey)).toContain(`1:dir${PARENT_ID}/sub`)
    // 子节点同样没有实体身份 —— 后端不发 directoryId,适配器归一为 null(绝不伪造)。
    expect(tree.nodes.value.find((n) => n.name === 'sub')?.id).toBeNull()
  })

  /**
   * 🔴 在途去重表的键也必须是**路径身份**(D-013 第三处)。
   *
   * 可证伪性:两个**互不相干**的 FS-only 目录并发展开,它们的 id 都是 null。按 id 建表时二者
   * 撞成同一个键,后到的那个直接拿前一个的在途 Promise 当自己的结果 —— 它的子节点**永远不会
   * 出现**,而且没有任何报错(await 正常 resolve)。这条与「注入判重走 nodeKey」是两张不同的表,
   * 各修各的,不能互相顶替。
   */
  it('在途去重按 nodeKey:两个 id 同为 null 的目录并发展开,各自的子节点都要到位', async () => {
    const mk = (rel: string) =>
      ({ ...node(PARENT_ID, null, 0, { hasChildren: null }), id: null, nodeKey: '1:' + rel, relPath: rel, name: rel }) as DirNode
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') return Promise.resolve([mk('alpha'), mk('beta')])
      if (cmd === 'list_tree_entries') {
        const rel = (args as { relPath: string }).relPath
        // 延迟拉开重叠窗口,逼真复现两次并发在途。
        return new Promise((resolve) =>
          setTimeout(
            () =>
              resolve({
                entries: [
                  { ...fsEntry('kid', 'dir'), nodeKey: `1:${rel}/kid`, parentKey: '1:' + rel, relPath: `${rel}/kid`, name: `kid-of-${rel}` },
                ],
                total: 1,
              }),
            5,
          ),
        )
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    const [alpha, beta] = tree.nodes.value
    await Promise.all([tree.loadChildren(alpha), tree.loadChildren(beta)])

    const names = tree.nodes.value.map((n) => n.name)
    expect(names).toContain('kid-of-alpha')
    expect(names).toContain('kid-of-beta')
  })

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

  /**
   * 🔴 「还有没有下一页」读后端的 `nextCursor`,**不能**拿 `entries.length === 页长` 推。
   *
   * 可证伪性(真实可达,非假想):总数恰为页长整数倍时,末页**装满 200 条且 nextCursor 缺席**。
   * 按条数推的实现会说「还有更多」,于是「加载更多」行永不消失、再点又拉回一页空 —— 后端
   * `exact_multiple_page_size_ends_without_extra_empty_page` 守的是同一个边界的另一半。
   */
  it('末页装满一页但无 nextCursor 时,filesHasMore=false', async () => {
    const full = Array.from({ length: 200 }, (_, i) => fsEntry(`f${i}.jpg`, 'file'))
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { mediaCount: 200 })])
      }
      if (cmd === 'list_tree_entries') return Promise.resolve({ entries: full, total: 200 })
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])

    const n = tree.nodes.value[0]
    expect(n.files).toHaveLength(200)
    expect(n.filesHasMore).toBe(false)
    expect(n.filesCursor).toBeUndefined()
  })

  /**
   * 分页接力:第二页带的游标是**后端上一页给的原话**。
   *
   * ⚠ 诚实标注:本用例**区分不出**「读 nextCursor」与「拿 files.length 反推」。后端
   * `slice_page` 恒有 `next_cursor = cursor + entries.len()`,而 files 视图不丢项,故二者今天
   * 逐值相等,构造不出合法的分歧输入。留 `filesCursor` 字段的理由不是它今天不同,而是它把
   * 「翻页游标」与「适配后剩几条」解耦 —— 后者取决于 adaptTreeEntries 怎么分组,那是另一个
   * 模块的自由。这条用例只钉协议(游标确实往下传),不冒充可证伪。
   */
  it('加载更多按后端给的游标请求下一页', async () => {
    state.handler = (cmd, args) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { mediaCount: 3 })])
      }
      if (cmd === 'list_tree_entries') {
        const a = args as { cursor: number; kind: string }
        if (a.kind === 'dirs') return Promise.resolve({ entries: [], total: 0 })
        return a.cursor === 0
          ? Promise.resolve({ entries: [fsEntry('a.jpg', 'file')], nextCursor: 1, total: 2 })
          : Promise.resolve({ entries: [fsEntry('b.jpg', 'file')], total: 2 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])
    expect(tree.nodes.value[0].filesCursor).toBe(1)

    await tree.loadMoreFiles(tree.nodes.value[0])

    const fileCalls = treeCalls().filter((c) => (c.args as { kind: string }).kind === 'files')
    expect(fileCalls.map((c) => (c.args as { cursor: number }).cursor)).toEqual([0, 1])
    expect(tree.nodes.value[0].files?.map((f) => f.fileName)).toEqual(['a.jpg', 'b.jpg'])
    expect(tree.nodes.value[0].filesHasMore).toBe(false)
  })
})

// ── R-03:FS 模式下根行的 DB 轴字段归一 ─────────────────────────────────────────
//
// 根行两种模式都从 DB 来(D-013 键统一),但 FS 模式下其 hasChildren/mediaCount **语义失效**
// (F-022:双数据源下共享节点沿用旧轴字段是静默语义漂移):
//  - DB hasChildren=false 而磁盘有 FS-only 子目录 → toggleNode 跳过 loadChildren,整层子目录
//    永不出现且无信号;
//  - hasChildren=false + mediaCount=0 的根无 chevron(isExpandable=false)——「所有文件」模式的
//    核心场景整根不可浏览;
//  - 根行角标照显递归媒体数,违反 §4.2「媒体数不冒充文件数」。
describe('useFolderTree FS 模式根行字段归一(R-03)', () => {
  // 可证伪样本:DB 说「无子目录、0 媒体」——正是①②两条会翻车的形状。
  const dbShapedRoot = () => node(PARENT_ID, null, 0, { hasChildren: false, mediaCount: 0 })

  it('FS 模式:根行 hasChildren/mediaCount 归一为 null(未知/不适用)', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') return Promise.resolve([dbShapedRoot()])
      if (cmd === 'list_tree_entries') return Promise.resolve({ entries: [], total: 0 })
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    expect(tree.nodes.value[0].hasChildren).toBeNull()
    expect(tree.nodes.value[0].mediaCount).toBeNull()
  })

  it('对照:registeredOnly 模式根行保留 DB 轴字段(零回归)', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') return Promise.resolve([dbShapedRoot()])
      return Promise.resolve([])
    }
    const tree = useFolderTree()
    await loadRootsOf(tree)
    expect(tree.nodes.value[0].hasChildren).toBe(false)
    expect(tree.nodes.value[0].mediaCount).toBe(0)
  })

  it('端到端:DB 报 hasChildren=false 的根,FS 模式展开仍会去拉子目录(①的行为面)', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') return Promise.resolve([dbShapedRoot()])
      if (cmd === 'list_tree_entries') {
        return Promise.resolve({ entries: [fsEntry('ghost', 'dir')], total: 1 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])
    // 归一前:hasChildren===false → toggleNode 直接跳过 loadChildren,ghost 永不出现。
    expect(tree.nodes.value.map((n) => n.name)).toContain('ghost')
  })
})

// ── R-04:loadChildren 代际守卫(切模式竞态)────────────────────────────────────
describe('useFolderTree loadChildren 代际守卫(R-04)', () => {
  /**
   * 🔴 复现被删守卫本要挡的竞态(F-023):FS 模式展开在途 → 切回 registeredOnly →
   * loadRoots 换掉整树 → 旧 FS 响应此刻到达。扫描根 nodeKey 跨模式**同键**(D-013 卖点),
   * 按 nodeKey 找插入位能找到 → 无守卫时 FS-only 子目录被 splice 进 DB 模式的新树。
   */
  it('切模式后到达的旧 FS 响应必须整批丢弃,不得注入新树', async () => {
    let mode: 'allFiles' | 'registeredOnly' = 'allFiles'
    let releaseStale: (() => void) | null = null
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') {
        // 挂起的 FS 枚举:等测试显式放行,模拟大目录枚举在途。
        return new Promise((resolve) => {
          releaseStale = () =>
            resolve({ entries: [fsEntry('stale-fs-only', 'dir')], total: 1 })
        })
      }
      return Promise.resolve([])
    }
    const tree = useFolderTree(() => mode)
    await loadRootsOf(tree)

    const inflight = tree.loadChildren(tree.nodes.value[0]) // FS 枚举在途
    await Promise.resolve() // 让 fetch 真正发出去
    mode = 'registeredOnly'
    await loadRootsOf(tree) // 模式 watch 的整树重载(换代)
    releaseStale!() // 旧 FS 响应此刻才到
    await inflight

    // 新树(DB 模式)不得混入旧模式的 FS-only 行。
    expect(tree.nodes.value.map((n) => n.name)).not.toContain('stale-fs-only')
    // 且旧响应不得把新树里的同键节点标成 expanded(它根本没展开过)。
    expect(tree.nodes.value[0].expanded).not.toBe(true)
  })

  it('无换代时守卫不误伤:正常展开照常注入', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') {
        return Promise.resolve({ entries: [fsEntry('kid', 'dir')], total: 1 })
      }
      return Promise.resolve([])
    }
    const tree = fsTree()
    await loadRootsOf(tree)
    await tree.loadChildren(tree.nodes.value[0])
    expect(tree.nodes.value.map((n) => n.name)).toContain('kid')
  })
})

// ── R-10:展开失败不得停在「expanded=true + 空树」────────────────────────────────
describe('useFolderTree 展开失败回退(R-10)', () => {
  it('取数失败:expanded 回退 false + 上报回调收到错误(与「确实为空」可分)', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      // 卷不可用形态:结构化错误(设计 §4.2:报错,绝不回落 DB 模式)。
      if (cmd === 'list_tree_entries') {
        return Promise.reject({ code: 'PathResolution', message: 'volume gone' })
      }
      return Promise.resolve([])
    }
    const reported: unknown[] = []
    const tree = useFolderTree(
      () => 'allFiles',
      (e) => reported.push(e),
    )
    await loadRootsOf(tree)

    await tree.toggleNode(tree.nodes.value[0])

    expect(tree.nodes.value[0].expanded).toBe(false) // 回退,不是悬着的假展开
    expect(tree.nodes.value[0].loading).toBe(false)
    expect(reported).toHaveLength(1) // 组件侧据此弹 toast
  })

  it('对照:取数成功时不触发上报,正常展开', async () => {
    state.handler = (cmd) => {
      if (cmd === 'get_directory_tree') {
        return Promise.resolve([node(PARENT_ID, null, 0, { hasChildren: true })])
      }
      if (cmd === 'list_tree_entries') {
        return Promise.resolve({ entries: [fsEntry('ok', 'dir')], total: 1 })
      }
      return Promise.resolve([])
    }
    const reported: unknown[] = []
    const tree = useFolderTree(
      () => 'allFiles',
      (e) => reported.push(e),
    )
    await loadRootsOf(tree)
    await tree.toggleNode(tree.nodes.value[0])
    expect(tree.nodes.value[0].expanded).toBe(true)
    expect(reported).toHaveLength(0)
  })
})
