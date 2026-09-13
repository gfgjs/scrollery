// src/composables/useFolderTree.ts
// 文件夹树懒加载

import { ref, shallowRef, triggerRef } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import type {
  DirNode,
  DirFile,
  ScanRoot,
  TreeCategory,
  TreeDisplayMode,
  TreePage,
} from '../types/media'
import { ALL_TREE_CATEGORIES } from '../constants/mediaCategoryDescriptors'
import { isExpandable } from '../components/sidebar/sections/folderTree.helpers'
import { adaptTreeEntries } from './folderTreeAdapter'
import { IPC } from '../constants/ipc'

// 单目录文件分页页大小(T2 百万级)。每次向后端多取一条(PAGE + 1)以探测是否还有更多,
// 避免额外的 count 查询(node.mediaCount 是子树聚合,不能当直接文件数用)。
// 仅 DB 模式用:FS 模式的「还有更多」由后端 `TreePage.nextCursor` 直接给,无需探测条。
const FILE_PAGE = 200

/**
 * 侧栏文件树的取数与树状态。
 *
 * @param getMode 显示模式取值器（S 线 §3）。**取数路径由它分发**：
 *   - `registeredOnly` → 现有 DB 目录树命令（零回归）；
 *   - `allFiles` / `allFilesWithHidden` → 受限 FS 枚举 `list_tree_entries`。
 *
 *   默认 `registeredOnly` 不是「随便挑个默认值」，而是**钉死选择器对话框的契约**：
 *   `FolderTreeSelectorDialog` 挑的是移动/复制的目标目录，而那两个 IPC 收的是 DB 目录 id ——
 *   FS-only 目录根本没有 id，让它出现在选择器里就是让用户选一个选不中的东西（§4.1）。
 *   写成参数而非在内部读全局，正是为了让这条约束是**显式**的：不传 = 明确要 DB 模式。
 * @param onLoadError 展开取数失败的上报口（R-10）。不传只落 console——组件侧（FoldersSection）
 *   接 toast：失败若只进全局 error 通道而 `expanded` 已置 true，呈现为「展开后空无一物」，
 *   与「确实为空」不可分，§4.2 要求的空态成了意外副产品。
 */
export function useFolderTree(
  getMode: () => TreeDisplayMode = () => 'registeredOnly',
  onLoadError?: (err: unknown) => void,
  getCategories: () => readonly TreeCategory[] = () => ALL_TREE_CATEGORIES,
) {
  const roots = ref<ScanRoot[]>([])
  // `nodes` 用 shallowRef(CLAUDE.md 大数组红线):百万级下展开行可达数万,深 ref 会对每个
  // DirNode 及其 files 内每个 DirFile 建响应式 Proxy 并逐属性追踪,CPU/内存开销巨大。改 shallow
  // 后 Vue 只跟踪 `.value` 身份替换 → 所有「原地 mutate」(splice 注子节点 / node.expanded /
  // node.files 赋值)都不再自动触发,须在每处 mutate 后显式 triggerRef(nodes)。整棵树的写入
  // 都封装在本 composable 内,故触发点集中可控;消费者只读不外部 mutate。
  const nodes = shallowRef<DirNode[]>([])
  const loading = ref(false)
  let loadingId = 0 // 防止并发 loadRoots 调用的守卫
  // 同一父目录子节点加载的「在途去重表」:并发的 loadChildren(sameParent) 复用同一在途 Promise,
  // 避免两次 splice 注入同一批子节点造成重复目录行——即 Vue "Duplicate keys found during update"
  // 刷屏的根因(两个 watch:activeDirectoryId / scrolledDirectoryId 在 groupBy=folder 滚动时对同一
  // 未展开祖先并发 expandToNode → loadChildren,均在 await 完成前读到 !expanded 故各插一次)。
  //
  // 键是**路径身份**(nodeKey)而非 DB id(D-013):去重的对象是「树上的同一个节点」,而树的结构轴
  // 是路径。FS-only 目录的 id 全是 null,按 id 建表会让它们**全部撞成同一个键** —— 两个毫不相干的
  // 目录并发展开时互相顶掉,后到的那个直接拿前一个的 Promise 当自己的结果,子节点永远不出现。
  const childLoadInflight = new Map<string, Promise<void>>()
  // 文件页的独立代际号。分类切换只重置文件分页，不换目录树；旧 IPC 返回时必须不能把
  // 上一组分类的 rows 写回当前节点。
  let fileLoadGeneration = 0

  function categoryArgs() {
    const selected = new Set(getCategories())
    // 运行时仍做一次白名单收敛，避免外部持有 getter 时把未知字符串传入 IPC。
    const categories = ALL_TREE_CATEGORIES.filter((category) => selected.has(category))
    return { categories }
  }

  // 绝对路径由「根路径 + relPath」拼出,供右键「新建子文件夹」等按路径动作用。两种模式同源:
  // relPath 与 rootId 都是路径身份,FS-only 目录一样有。
  function attachAbsPath(list: DirNode[]) {
    list.forEach((c) => {
      const r = roots.value.find((root) => root.id === c.rootId)
      if (r) c.absPath = c.relPath ? `${r.path}/${c.relPath}` : r.path
    })
  }

  // ── FS 取数(「所有文件」两态,S 线 §4)────────────────────────────────────────
  // 入参是**路径身份**(rootId + relPath)而非 DB id —— 这正是 FS-only 目录(磁盘上有、库里没有)
  // 也能展开的原因。
  function fetchTreePage(node: DirNode, kind: 'dirs' | 'files', cursor: number) {
    return invokeIpc<TreePage>(IPC.LIST_TREE_ENTRIES, {
      rootId: node.rootId,
      relPath: node.relPath,
      mode: getMode(),
      kind,
      cursor,
      ...categoryArgs(),
    })
  }

  // FS 模式的子**目录**要一次给全:前端模型里目录不分页(分页的只有文件),这与 DB 模式
  // get_directory_children「返回全部子目录、不分页」是同一个契约。故循环翻页直到取尽。代价可
  // 接受——快照在后端已整份物化并缓存(键不含 kind),后续页是纯切片,不重跑 read_dir。
  //
  // 有意**不设页数上限**:DB 模式同样不设(SQL 返回多少是多少),凭空给个上限就是静默截断,
  // 用户看到的是目录莫名少了几个而没有任何信号。真正要防的死循环是**后端游标不前进**,那是协议
  // 违约,单独判、单独喊。
  async function fetchAllChildDirs(parent: DirNode): Promise<DirNode[]> {
    const out: DirNode[] = []
    let cursor: number | undefined = 0
    while (cursor !== undefined) {
      const page = await fetchTreePage(parent, 'dirs', cursor)
      out.push(...adaptTreeEntries(page.entries, parent.depth + 1).dirs)
      const next: number | undefined = page.nextCursor
      if (next !== undefined && next <= cursor) {
        logger.error(
          `[useFolderTree] list_tree_entries 游标未前进(${cursor} → ${next}),中止翻页以免死循环`,
        )
        break
      }
      cursor = next
    }
    return out
  }

  // 按模式取某目录的子目录。两种模式在此**汇流**:调用方只管拿 DirNode[],不知道数据从哪来。
  async function fetchChildDirs(parent: DirNode): Promise<DirNode[]> {
    if (getMode() !== 'registeredOnly') return fetchAllChildDirs(parent)
    // DB 快路径需要实体身份。该模式下树全部来自 directories 表,节点必有 id;读到 null 只可能是
    // 切模式的瞬态(树里还留着上一模式的 FS-only 行,模式 watch 的整树重载尚未跑完)——此时没有
    // 可用的 DB 取数路径,返回空而非报错:那不是失败,是这条路径不适用于它。
    if (parent.id === null) return []
    return invokeIpc<DirNode[]>(IPC.GET_DIRECTORY_CHILDREN, {
      parentId: parent.id,
      ...categoryArgs(),
    })
  }

  // 加载所有根子树并「原子」替换 `nodes`。先累积到局部数组，最后只在仍是胜出调用时一次性
  // 赋值，使两次重载竞争时树不会变空或半成品——例如移除根目录触发的 scanRoots 重载与另一个
  // reloadTreePreserveExpansion 同时在途（问题6）。
  async function loadRoots(scanRoots: ScanRoot[]) {
    const myId = ++loadingId // 如果另一个调用开始，这个调用的结果将被丢弃
    fileLoadGeneration++ // 换树代际时，旧文件页也不能回写同键的新节点
    const fsMode = getMode() !== 'registeredOnly' // 本次加载的模式快照(与 myId 同期)
    roots.value = scanRoots
    loading.value = true
    try {
      const acc: DirNode[] = []
      for (const root of scanRoots) {
        // 🔴 根节点**两种模式都从 DB 来**,不是漏了分发。扫描根永远有 directories 行
        // (scan_commands 建根时就写,rel_path='')，故它同一个生产者、同一个 nodeKey ——
        // 前端因此一个键都不用推导(D-013)。只有**子项**才换成 FS 枚举。
        // 「所有文件」模式的根行只是磁盘枚举锚点：根目录本身可能没有已注册媒体，
        // 不能先用 DB 分类把它裁掉；真正的分类筛选由 FS 子项查询负责。注册库模式
        // 才在这里启用后代聚合裁剪。
        const children = await invokeIpc<DirNode[]>(IPC.GET_DIRECTORY_TREE, {
          rootId: root.id,
          ...(fsMode ? {} : categoryArgs()),
        })
        if (myId !== loadingId) return // 已被取代 — 丢弃且不动 nodes
        // R-03:根行取数同源 DB,但 FS 模式下其 DB 轴字段**语义失效**,必须按模式归一为
        // null(与 adapter 对 FS 条目的语义一致),否则三处静默漂移:
        // ① DB hasChildren=false 而磁盘有 FS-only 子目录(如整夹未注册格式)→ toggleNode 的
        //    `hasChildren !== false` 判据跳过 loadChildren,该层子目录永不出现且无信号;
        // ② hasChildren=false + mediaCount=0 的根被 isExpandable 判为不可展开(无 chevron)
        //    ——「所有文件」模式的核心场景整根不可浏览;
        // ③ 根行角标照显递归**媒体**数,违反 §4.2「媒体数不冒充文件数」。
        // 模式分发要覆盖**字段语义**,不只取数路径(F-022)。
        if (fsMode) {
          for (const n of children) {
            n.hasChildren = null
            n.mediaCount = null
          }
        }
        attachAbsPath(children)
        acc.push(...children)
      }
      if (myId !== loadingId) return
      nodes.value = acc // 单次原子替换 — 中途不留空
      // 新一代树就位 → 丢弃上一代的在途去重表:旧在途任务已被代际守卫(见 loadChildren)判废,
      // 留着只会让新一代同键加载错复用一个「必然空产出」的 Promise。
      childLoadInflight.clear()
    } finally {
      if (myId === loadingId) loading.value = false
    }
  }

  // 加载并注入某目录的子目录。
  //
  // 入参是**节点**而非 DB id:取数在 FS 模式下只需要路径身份(rootId + relPath),而 FS-only 目录
  // 根本没有 id —— 收 id 就等于把「必须有库行」写死进了签名。
  //
  // (顺带清掉了原先的 `(parentId: number | null, rootId?, loadId?)` 三参形态:后两参**从无调用方
  // 传过**,故 `parentId === null && rootId !== undefined` 分支恒不进、`loadId !== loadingId` 竞态
  // 守卫恒不触发,整段是死代码。换签名时才暴露出来。
  //  ⤷ 2026-07-16 更正:那次删除删对了「当时」——但 549861c 的模式 watch 整树重载重新制造了
  //    它要挡的竞态,守卫已按新形态回归(函数体内 myLoadId,R-04/F-023)。)
  async function loadChildren(parent: DirNode) {
    loading.value = true
    // 代际守卫(R-04):捕获发起时的 loadingId,响应到达时不符即整批丢弃。要挡的竞态:
    // 「FS 模式展开根(大目录枚举在途)→ 切回 registeredOnly → loadRoots 换掉整树 → 旧 FS
    // 响应此刻到达」——扫描根 nodeKey 跨模式同键(D-013 的卖点),下面按 nodeKey 找插入位
    // **能找到**,FS-only 子目录就被 splice 进了 DB 模式的新树。前身守卫曾因「恒不触发」被当
    // 死代码删除;549861c 引入模式 watch 整树重载后恰恰重新制造了它要挡的场景(F-023:删死
    // 守卫前先问引入它的竞态会不会被本批变更复活)。
    const myLoadId = loadingId
    try {
      // 并发去重:同一节点的加载若已在途,复用其 Promise 并「等待」(而非早退)——保证 resolve
      // 后子节点确已注入,这样 expandToNode 顺序展开祖先链时,读到的每一层都已就绪(否则可能
      // 在子节点尚未 splice 时就去找更深祖先而漏展开)。
      const inflight = childLoadInflight.get(parent.nodeKey)
      if (inflight) {
        await inflight
        return
      }
      const task = (async () => {
        const children = await fetchChildDirs(parent)
        if (myLoadId !== loadingId) return // 整树已换代(切模式/换根)——丢弃,不动新树
        attachAbsPath(children)
        // 在其父节点之后注入子节点
        const idx = nodes.value.findIndex((n) => n.nodeKey === parent.nodeKey)
        if (idx >= 0) {
          // 第二道防线(防御式):滤掉已在树中的节点再 splice。判重用 **nodeKey(路径身份)** 而非
          // id——「一个节点在拍平树中至多一行」这条硬不变量的正确表述是按树的结构轴,而结构轴是
          // 路径(D-013);FS-only 目录根本没有 id,用 id 判重会让它们全部塌成同一条(null === null)。
          const existing = new Set(nodes.value.map((n) => n.nodeKey))
          const fresh = children.filter((c) => !existing.has(c.nodeKey))
          if (fresh.length) nodes.value.splice(idx + 1, 0, ...fresh)
        }
        // 标记父节点为展开状态
        const inTree = nodes.value.find((n) => n.nodeKey === parent.nodeKey)
        if (inTree) inTree.expanded = true
        triggerRef(nodes) // splice + expanded 均原地 mutate,shallowRef 需显式触发
      })()
      childLoadInflight.set(parent.nodeKey, task)
      try {
        await task
      } finally {
        // 只删**自己**注册的那条:loadRoots 换代时会 clear 本表,新一代可能已对同键注册了
        // 新任务——旧任务的 finally 无条件 delete 会把新任务的在途去重顶掉。
        if (childLoadInflight.get(parent.nodeKey) === task) {
          childLoadInflight.delete(parent.nodeKey)
        }
      }
    } finally {
      loading.value = false
    }
  }

  // 懒加载某目录自身的文件，用于树的可展开文件列表。结果缓存在
  // 节点上（`filesLoaded`），使重新展开瞬时完成。仅直接文件——子文件夹是独立节点。
  async function loadFiles(node: DirNode, generation = fileLoadGeneration) {
    if (node.filesLoaded) return
    if (generation !== fileLoadGeneration) return
    if (getMode() === 'registeredOnly') {
      if (node.id === null) return // 同 fetchChildDirs:切模式瞬态,无 DB 取数路径
      // 首页:多取一条(FILE_PAGE + 1)探测是否还有更多。命中则丢弃探测条、置 filesHasMore,
      // 由 flattenFolderRows 在文件之后补「加载更多」行(T2 单目录分页)。
      const rows = await invokeIpc<DirFile[]>(IPC.LIST_DIRECTORY_FILES, {
        directoryId: node.id,
        limit: FILE_PAGE + 1,
        offset: 0,
        ...categoryArgs(),
      })
      if (generation !== fileLoadGeneration) return
      node.filesHasMore = rows.length > FILE_PAGE
      node.files = node.filesHasMore ? rows.slice(0, FILE_PAGE) : rows
    } else {
      // FS 模式无需探测条:后端直接给 nextCursor,「有没有下一页」是它的原话而不是我们的推断。
      const page = await fetchTreePage(node, 'files', 0)
      if (generation !== fileLoadGeneration) return
      node.files = adaptTreeEntries(page.entries, node.depth + 1).files
      node.filesCursor = page.nextCursor
      node.filesHasMore = page.nextCursor !== undefined
    }
    node.filesLoaded = true
    triggerRef(nodes) // 原地给 node 赋 files/属性,shallowRef 需显式触发拍平重算
  }

  // 追加下一页文件(点击「加载更多」行时调用,T2)。同 loadChildren:收节点而非 id。
  async function loadMoreFiles(node: DirNode) {
    if (!node.filesLoaded || !node.filesHasMore || !node.files) return
    const generation = fileLoadGeneration
    if (getMode() === 'registeredOnly') {
      if (node.id === null) return
      // offset = 已加载数,同样多取一条探测。
      const rows = await invokeIpc<DirFile[]>(IPC.LIST_DIRECTORY_FILES, {
        directoryId: node.id,
        limit: FILE_PAGE + 1,
        offset: node.files.length,
        ...categoryArgs(),
      })
      if (generation !== fileLoadGeneration) return
      node.filesHasMore = rows.length > FILE_PAGE
      node.files = [...node.files, ...(node.filesHasMore ? rows.slice(0, FILE_PAGE) : rows)]
    } else {
      // 游标取自 node.filesCursor(后端上一页给的原话),不拿 files.length 反推——见 DirNode.filesCursor。
      if (node.filesCursor === undefined) return
      const page = await fetchTreePage(node, 'files', node.filesCursor)
      if (generation !== fileLoadGeneration) return
      node.files = [...node.files, ...adaptTreeEntries(page.entries, node.depth + 1).files]
      node.filesCursor = page.nextCursor
      node.filesHasMore = page.nextCursor !== undefined
    }
    triggerRef(nodes)
  }

  /**
   * 重新读取当前筛选下所有已展开目录的文件页。
   *
   * 分类只影响文件，不影响目录骨架，所以保留展开态；但必须丢掉旧页和游标。代际号在
   * 发起新请求前递增，旧请求即使晚到也只能被丢弃，不能把上一组分类的结果写回节点。
   */
  async function reloadFilesForFilter() {
    const generation = ++fileLoadGeneration
    const expandedKeys = nodes.value.filter((node) => node.expanded).map((node) => node.nodeKey)
    for (const node of nodes.value) {
      node.files = undefined
      node.filesLoaded = false
      node.filesHasMore = false
      node.filesCursor = undefined
    }
    triggerRef(nodes)

    try {
      for (const key of expandedKeys) {
        if (generation !== fileLoadGeneration) return
        const node = nodes.value.find((item) => item.nodeKey === key)
        if (node?.expanded) await loadFiles(node, generation)
      }
    } catch (err) {
      // 只报告仍属于当前分类的错误；过时请求的失败不能覆盖用户刚选的新分类。
      if (generation === fileLoadGeneration) {
        logger.error('[useFolderTree] 分类切换后文件页加载失败', { error: err })
        onLoadError?.(err)
      }
    }
  }

  async function toggleNode(node: DirNode) {
    if (node.expanded) {
      // 折叠：删除后代「目录」行。缓存的 `files` 仍留在节点上，但模板在折叠时隐藏它们，
      // 因此重新展开能瞬时显示。
      collapseNode(node)
      return
    }
    // 展开：先展开主体（使缓存文件立即出现），再并行懒加载子文件夹与本目录自身的文件。
    // 仅含文件的目录（hasChildren=false 但 mediaCount>0）同样可展开——只是没有子文件夹。
    node.expanded = true
    triggerRef(nodes) // 早显:缓存 files 立即出现 + 箭头旋转,不等异步加载(shallowRef 手动触发)
    const tasks: Promise<unknown>[] = []
    // `hasChildren !== false` 而非 isExpandable():这里问的是「要不要去拉子目录」,不是「能不能
    // 展开」。hasChildren=false + mediaCount>0 的目录可展开(有直接文件)但确定没有子目录,不必
    // 白跑一次 IPC。null(FS 模式未知)则必须去拉——不拉就永远不知道。
    // 「有没有实体身份」已下沉进 loadChildren 的模式分发,此处不再判 id:FS 模式下 FS-only 目录
    // 正是要能展开的那批。
    if (node.hasChildren !== false) tasks.push(loadChildren(node))
    if (!node.filesLoaded) tasks.push(loadFiles(node))
    if (tasks.length) {
      node.loading = true
      try {
        await Promise.all(tasks)
      } catch (err) {
        // R-10:展开取数失败(含卷不可用)不能停在「expanded=true + 空树」——那与「确实为空」
        // 不可分。回退折叠(collapseNode 顺带移除可能已注入的部分子行)+ 上报,用户重试有路径。
        collapseNode(node)
        logger.error('[useFolderTree] 展开取数失败,已回退折叠', { error: err })
        onLoadError?.(err)
      } finally {
        node.loading = false
        triggerRef(nodes) // loading 置位/回退折叠都是原地 mutate,统一在此触发
      }
    }
  }

  // 确保目标目录及其祖先都已加载进树(使其出现在拍平行中)。**只负责展开,不负责滚动**——
  // 虚拟化(T1-a)后目标行可能不在 DOM,滚动改由组件按索引算共享 scrollTop(scrollTreeToDirId,
  // M5);调用方在 expandToNode resolve 后自行 .then(scrollTreeToDirId)。
  // (T1-c 清理:原 scrollToNode 走 querySelector + scrollIntoView,虚拟化后目标常不在 DOM →
  //  恒 no-op,且组件已接管滚动,故连同其两处调用[早退 + setTimeout]一并删除。)
  async function expandToNode(targetId: number) {
    if (nodes.value.find((n) => n.id === targetId)) return // 已在树中,滚动交调用方
    try {
      const ancestors = await invokeIpc<number[]>(IPC.GET_DIRECTORY_ANCESTORS, { id: targetId })
      for (const id of ancestors) {
        if (id === targetId) continue
        const node = nodes.value.find((n) => n.id === id)
        if (node && !node.expanded) {
          await loadChildren(node)
        }
      }
    } catch (e) {
      logger.error('[useFolderTree] expandToNode failed', { error: e })
    }
  }

  function collapseNode(node: DirNode) {
    const descendants = getDescendantKeys(node.nodeKey)
    // 先改属性再 reassign:reassign 是 shallowRef 的触发点,重算拍平时读到的 expanded 已为 false。
    node.expanded = false
    nodes.value = nodes.value.filter((n) => !descendants.has(n.nodeKey))
  }

  // 后代收集走**路径身份**(parentKey)而非 parentId:折叠是纯结构操作,而树的结构轴是路径
  // (D-013)。FS-only 目录没有 parentId,用 id 轴走后代会在「所有文件」模式下漏掉整棵子树
  // (折叠后子行残留)。
  function getDescendantKeys(parentKey: string): Set<string> {
    const set = new Set<string>()
    const stack = [parentKey]
    while (stack.length) {
      const pk = stack.pop()!
      nodes.value.forEach((n) => {
        if (n.parentKey === pk) {
          set.add(n.nodeKey)
          stack.push(n.nodeKey)
        }
      })
    }
    return set
  }

  // 递归展开所有目录(边展开边懒加载)——用于在最大行数下压测虚拟化树。反复扫描直到没有可展开
  // 的折叠目录;新加载出的子目录下一轮拾取。串行(非并行)以免 loadChildren 的 splice 注入竞争。
  async function expandAll() {
    loading.value = true
    try {
      let changed = true
      let guard = 0
      while (changed && guard++ < 10000) {
        changed = false
        const pending = nodes.value.filter((n) => !n.expanded && isExpandable(n))
        for (const n of pending) {
          if (n.expanded) continue
          n.expanded = true
          const tasks: Promise<unknown>[] = []
          if (n.hasChildren !== false) tasks.push(loadChildren(n))
          if (!n.filesLoaded) tasks.push(loadFiles(n))
          if (tasks.length) await Promise.all(tasks)
          changed = true
        }
      }
      triggerRef(nodes) // 兜底:loadChildren/loadFiles 已各自触发,此处覆盖无 task 的 expanded 置位
    } finally {
      loading.value = false
    }
  }

  // 折叠所有,只留扫描根。
  function collapseAll() {
    // 先在旧数组上清 expanded,再 reassign(reassign 即 shallowRef 触发点)。
    // 「是扫描根」的判据用 parentKey === null(路径身份):FS 模式下顶层节点同样 parentKey 为 null,
    // 而 parentId 在那里恒为 null(连非根节点也是),用它会把整棵树都当成根行留下。
    const rootRows = nodes.value.filter((n) => n.parentKey === null)
    rootRows.forEach((n) => {
      n.expanded = false
    })
    nodes.value = rootRows
  }

  return {
    roots,
    nodes,
    loading,
    loadRoots,
    loadChildren,
    toggleNode,
    loadMoreFiles,
    reloadFilesForFilter,
    expandToNode,
    expandAll,
    collapseAll,
  }
}
