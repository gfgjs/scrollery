// src/composables/useFolderTreeSync.ts
// 树的加载/重载/模式切换/刷新/展开折叠全部生命周期。
import { computed, watch, onMounted, onBeforeUnmount, type ComputedRef } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC, EVENTS } from '../constants/ipc'
import { useTauriListen } from './useTauriListen'
import { createFolderTreeSyncQueue } from '../components/sidebar/sections/folderTree.helpers'
import type { DirNode, TreeCategory } from '../types/media'
import type { EnrichmentCompletedPayload } from '../types/ipc'
import type { useFolderTree } from './useFolderTree'
import type { useScanStore } from '../stores/scanStore'
import type { useUiStore } from '../stores/uiStore'
import type { useViewStore } from '../stores/viewStore'
import type { useToastStore } from '../stores/toastStore'

export interface FolderTreeSyncDeps {
  folderTree: ReturnType<typeof useFolderTree>
  scan: ReturnType<typeof useScanStore>
  ui: ReturnType<typeof useUiStore>
  viewStore: ReturnType<typeof useViewStore>
  toast: ReturnType<typeof useToastStore>
  t: (key: string, params?: Record<string, unknown>) => string
  nodesById: ComputedRef<Map<number, DirNode>>
  scrollTreeToNodeKey: (nodeKey: string, isCurrent?: () => boolean) => Promise<void>
  /** 模式B「文件夹作筛选」导航助手(域 10,留组件本体)——新增根/移动后自动选中要用。 */
  navigateToFolder: (id: number) => void
  /** 树专用文件分类；不与 gallery filterStore 共享。 */
  getTreeCategories?: () => readonly TreeCategory[]
  /** 分类状态的稳定 key；优先于对数组做 deep watch。 */
  getTreeCategoryKey?: () => string
}

export function useFolderTreeSync(deps: FolderTreeSyncDeps) {
  const { folderTree, scan, ui, viewStore, toast, t, nodesById, scrollTreeToNodeKey, navigateToFolder } =
    deps

  // 文件树 active 条件与模板保持单源：folder 分组跟随画廊当前 separator，其他模式跟随目录筛选。
  // 两个旧 watcher 各自并发 expandToNode，会让较慢的旧深路径在新目标之后才滚动；统一为
  // latest-wins 串行队列后，拖动滑块期间只处理「一个在途 + 最后一个目标」。
  const treeSyncTargetId = computed(() =>
    ui.groupBy === 'folder' ? ui.scrolledDirectoryId : viewStore.activeDirectoryId,
  )
  const treeSyncQueue = createFolderTreeSyncQueue(async (dirId, isLatest) => {
    // 入口是**实体轴**(画廊给的是 DB 目录 id),展开靠 GET_DIRECTORY_ANCESTORS(按 id 溯源);
    // 展开后换算到结构轴再滚——滚动本身不需要实体身份。
    await folderTree.expandToNode(dirId)
    if (!isLatest()) return
    const node = nodesById.value.get(dirId)
    if (!node) return
    await scrollTreeToNodeKey(node.nodeKey, isLatest)
  })
  function requestTreeSync(dirId: number | null) {
    void treeSyncQueue.request(dirId).catch((error) => {
      logger.error('[FoldersSection] sync tree to directory failed', { error })
    })
  }

  // ── files:changed 监听(文件监听对账完成)──────────────────────────────────────
  // 后端 fs_watch 对账后广播 { rootId, directories }(directories 为根相对**父**目录路径)。
  // 事件可能连发(连续磁盘改动),用「串行合并」队列:一次只跑一个 refreshDirectories,
  // 期间到达的事件并入下一批,避免并发刷新同一父目录造成整段替换竞争。未加载的父目录由
  // refreshDirectories 内部直接跳过,故这里不预筛。
  interface FilesChangedPayload {
    rootId: number
    directories: string[]
  }
  const filesChangedPending = new Map<number, Set<string>>()
  let filesChangedDrain: Promise<void> | null = null
  function enqueueFilesChanged(rootId: number, directories: readonly string[]) {
    let paths = filesChangedPending.get(rootId)
    if (!paths) {
      paths = new Set()
      filesChangedPending.set(rootId, paths)
    }
    for (const dir of directories) paths.add(dir)
    if (filesChangedDrain) return
    filesChangedDrain = (async () => {
      try {
        while (filesChangedPending.size) {
          const batch = [...filesChangedPending]
          filesChangedPending.clear()
          for (const [id, pendingPaths] of batch) {
            // 单个根刷新失败不拖垮整批:其余根仍应刷新,失败单独记录。
            try {
              await folderTree.refreshDirectories(id, [...pendingPaths])
            } catch (error) {
              logger.error('[FoldersSection] files:changed 原位刷新失败', { error, rootId: id })
            }
          }
        }
      } finally {
        filesChangedDrain = null
      }
    })()
  }
  useTauriListen<FilesChangedPayload>(EVENTS.FILES_CHANGED, (event) => {
    const payload = event.payload
    if (!payload || typeof payload.rootId !== 'number' || !Array.isArray(payload.directories)) return
    enqueueFilesChanged(payload.rootId, payload.directories)
  })

  // 收集某根当前「已加载」的父目录路径:有子行已物化、或本目录文件已加载、或已展开。
  // 无已加载项时退回根自身(''),保证至少刷新一层。
  function loadedParentPaths(rootId: number): string[] {
    const childParents = new Set<string>()
    for (const node of folderTree.nodes.value) {
      if (node.parentKey !== null) childParents.add(node.parentKey)
    }
    const paths = new Set<string>()
    for (const node of folderTree.nodes.value) {
      if (node.rootId !== rootId) continue
      if (
        node.parentKey !== null &&
        !childParents.has(node.nodeKey) &&
        !node.filesLoaded &&
        !node.expanded
      )
        continue
      paths.add(node.relPath)
    }
    if (paths.size === 0) paths.add('')
    return [...paths]
  }

  // 后端补全收尾会执行 live photo 配对(pair_live_photos),改变 companion 归属与该目录
  // media_count;而 files:changed 在配对**之前**发出,故这里在 enrichment:completed 后按
  // 最新关系刷新该根已加载父目录——否则磁盘新增 MOV+JPG 被配对后,伴随行与计数会滞留到
  // 下一次手动刷新或其它事件。走 files:changed 同一队列,避免并发整段替换竞争。
  useTauriListen<EnrichmentCompletedPayload>(EVENTS.ENRICHMENT_COMPLETED, (event) => {
    const rootId = event.payload?.rootId
    if (typeof rootId !== 'number') return
    enqueueFilesChanged(rootId, loadedParentPaths(rootId))
  })

  // ── 文件夹树：唯一的加载来源 ─────────────────────────────────────────────────
  // 该 watch 是树从扫描根目录加载的唯一入口。它在初始化（immediate）以及每次
  // 根集合、路径或名称变化（添加/移除/清空）时触发，使增删空都集中处理。扫描后的
  // 计数刷新改由下方的 `folder-stats-changed` 事件驱动。
  //
  // pendingSelectRootId 由 useFolderRootActions 的 addRoot() 经 setPendingSelectRootId 写入
  // (新增扫描根后要选中新根节点)——两个 composable 跨界共享同一个「挂起选中」标记,故不能
  // 各自持有一份副本,必须经由本 composable 暴露的 setter 显式传递(§2.1 依赖方向:ROOT → SYNC)。
  let pendingSelectRootId: number | null = null
  function setPendingSelectRootId(id: number | null) {
    pendingSelectRootId = id
  }
  watch(
    // 显隐（V21）：树只加载**可见**根；隐藏根从树消失（其媒体也已被后端排除）。切换 isHidden 会
    // 改变 visibleScanRoots 这个 computed 的值 → 本 watch 重触发 → 整树按新可见集重载。
    // 扫描计数/进度更新不能换掉整树；仅根集合、路径或名称变化重新加载。
    () => JSON.stringify(scan.visibleScanRoots.map((root) => [root.id, root.path, root.alias])),
    async () => {
      const roots = scan.visibleScanRoots
      await folderTree.loadRoots(roots)
      // 初次加载/整树重载可能发生在目标 id 已经写入 store 之后；主动重放当前目标，避免 watcher
      // 因值未变化而不再触发。null 同时会使任何旧在途定位失效。
      requestTreeSync(treeSyncTargetId.value)
      // 树真正加载后，再执行 addRoot 请求的选中。
      if (pendingSelectRootId != null) {
        // 「是扫描根」是结构判据 → 走 parentKey(路径身份);id 另判(见下)。
        const node = folderTree.nodes.value.find(
          (n) => n.rootId === pendingSelectRootId && n.parentKey === null,
        )
        // 扫描根必然有库行(scan_commands 建根时就写 directories),故 id 实际不会为 null;
        // 此处仍显式判空——类型上它是可选的,而「碰巧有值」不该被当成契约(§4.1)。
        if (node && node.id !== null) {
          // 新增扫描根后自动选中:走统一助手导航到 /folder/:id(此前 push('/') 会被 watcher 回填成 all 而清掉选中)。
          navigateToFolder(node.id)
        }
        pendingSelectRootId = null
      }
    },
    { immediate: true },
  )

  // 让树与权威的当前选中/滚动目录保持同步（展开并滚动到）；null 也必须入队以取消旧请求。
  watch(treeSyncTargetId, requestTreeSync)

  // 保留展开态重载（扫描刷新计数后、移动后自动选中时使用）。
  async function reloadTreePreserveExpansion(selectDirId?: number | null) {
    await refreshTree()
    if (selectDirId != null) {
      await folderTree.expandToNode(selectDirId)
      // 入口是实体轴(移动后要选中的 DB 目录 id),展开后换算到结构轴再滚。
      const node = folderTree.nodes.value.find((n) => n.id === selectDirId)
      if (node) await scrollTreeToNodeKey(node.nodeKey)
      // 移动后自动选中该目录:走统一助手导航到 /folder/:id,使 route 与视图一致(此前只设 viewStore 不导航,
      // route 与视图脱节;在 S2-c 后若停在 '/' 更会被 watcher 回填 all 清掉)。
      navigateToFolder(selectDirId)
    }
  }

  function onFolderStatsChanged(e: Event) {
    const selectDirId = (e as CustomEvent).detail?.selectDirId ?? null
    void reloadTreePreserveExpansion(selectDirId).catch((error) => {
      logger.error('[FoldersSection] 刷新文件夹统计失败', { error })
    })
  }
  // §2.3-④:request-add-folder 监听随 addRoot 一起下沉到 useFolderRootActions 自己的
  // onMounted/onBeforeUnmount;本 composable 只保留天然属于「重载生命周期」的 folder-stats-changed。
  onMounted(() => {
    window.addEventListener('folder-stats-changed', onFolderStatsChanged)
  })
  onBeforeUnmount(() => {
    window.removeEventListener('folder-stats-changed', onFolderStatsChanged)
  })

  // 切模式 → 整树重载。
  //
  // **不保展开态**:换模式换的是节点集合本身(FS-only 目录在 DB 模式里根本不存在),把旧展开态硬套
  // 到新集合上,轻则重展开一批不存在的键、重则让用户以为「这个目录空了」。折回根、重新展开是诚实的。
  //
  // 该 watch 也是**水合的兜底**:uiStore 的启动批 .then 落值时会触发它。此时树通常只有根行(用户还
  // 来不及展开),重载近乎免费;真要是晚到了,重载也是自愈——比赌「肯定先水合再展开」的顺序稳。
  watch(
    () => ui.treeDisplayMode,
    () => {
      void folderTree.loadRoots(scan.visibleScanRoots)
    },
  )

  // 分类变化在注册库模式下会改变目录骨架：后端要隐藏没有匹配后代的目录，同时保留必要的
  // 祖先路径；因此原位核对已加载目录。两种「所有文件」模式的目录来自文件系统，
  // 这里只重载已展开目录的文件分页，避免把尚未枚举的磁盘目录误判为空。watch 使用稳定 key，
  // 避免 getter 返回新数组时 deep watch 带来的重复触发；具体 IPC 参数由 useFolderTree 统一注入。
  const treeCategoryKey = deps.getTreeCategoryKey
    ? () => deps.getTreeCategoryKey!()
    : deps.getTreeCategories
      ? () => deps.getTreeCategories!().join(',')
      : null
  async function reloadTreeForCategory() {
    try {
      if (ui.treeDisplayMode === 'registeredOnly') {
        await reloadTreePreserveExpansion()
        // 分类切换后重置文件页及游标，只保留当前分类的分页范围。
        await folderTree.reloadFilesForFilter()
      } else {
        await folderTree.reloadFilesForFilter()
      }
    } catch (err) {
      logger.error('[FoldersSection] reload tree for category failed', { error: err })
      toast.addToast('error', t('sidebar.expandFailed'))
    }
  }
  if (treeCategoryKey) {
    watch(
      treeCategoryKey,
      () => void reloadTreeForCategory(),
    )
  }

  // 显式刷新:丢掉后端目录快照缓存,再**原位刷新已加载父目录**。
  // 为什么需要它:快照的真相源是**磁盘**,而磁盘可被本应用之外的任何东西改动(用户在资源管理器里
  // 拖了个文件进来)。有文件监听时由 files:changed 驱动收敛,这里是用户主动刷新(§4.2)。
  // 走与 files:changed 同一队列，避免手动刷新和监听并发使其它根的刷新被作废。
  async function refreshTree() {
    try {
      await invokeIpc(IPC.INVALIDATE_TREE_CACHE, { rootId: null })
    } catch (err) {
      // 缓存失效失败仍继续刷新:刷新本身有价值(DB 侧数据会刷新),不该因为清缓存失败就整个不做。
      logger.error('[FoldersSection] invalidate tree cache failed', { error: err })
      toast.addToast('error', t('sidebar.refreshTreeFailed'))
    }
    // 收集「已加载」的父目录:有子行已物化、或本目录文件已加载、或已展开。按根分组后原位刷新。
    const rootIds = new Set<number>(scan.visibleScanRoots.map((root) => root.id))
    for (const node of folderTree.nodes.value) rootIds.add(node.rootId)
    for (const rootId of rootIds) enqueueFilesChanged(rootId, loadedParentPaths(rootId))
    await filesChangedDrain
  }

  // 一键展开全部 / 折叠全部(便于压测虚拟化树)。有任何目录展开时按钮切为「折叠全部」。
  const anyExpanded = computed(() => folderTree.nodes.value.some((n) => n.expanded))
  async function toggleExpandAll() {
    if (anyExpanded.value) folderTree.collapseAll()
    else await folderTree.expandAll()
  }

  return {
    treeSyncTargetId,
    requestTreeSync,
    setPendingSelectRootId,
    reloadTreePreserveExpansion,
    refreshTree,
    anyExpanded,
    toggleExpandAll,
  }
}
