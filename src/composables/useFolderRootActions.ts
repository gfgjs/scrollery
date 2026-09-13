// src/composables/useFolderRootActions.ts
// 根目录管理:导入/迁移/新建子文件夹 + 右键菜单构建——从 FoldersSection.vue 域 19、20、21、22
// 迁出(见 docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
// 依赖 useFolderTreeSync 暴露的 reloadTreePreserveExpansion(迁移/新建后都要重载保留展开态)与
// setPendingSelectRootId(新增根后要选中新根节点);方向单向,Sync 不反向依赖本 composable。
import { ref, markRaw, onMounted, onBeforeUnmount } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderPlus, FolderSymlink } from '@lucide/vue'
import { invokeIpc, parseAppError, ipcErrorMessage } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import type { ContextMenuItem } from '../components/common/ContextMenu.vue'
import type { DirNode } from '../types/media'
import type { useScanStore } from '../stores/scanStore'
import type { useMediaStore } from '../stores/mediaStore'
import type { useToastStore } from '../stores/toastStore'
import type { useConfirm } from './useConfirm'

export interface FolderRootActionsDeps {
  scan: ReturnType<typeof useScanStore>
  media: ReturnType<typeof useMediaStore>
  toast: ReturnType<typeof useToastStore>
  confirm: ReturnType<typeof useConfirm>['confirm']
  t: (key: string, params?: Record<string, unknown>) => string
  reloadTreePreserveExpansion: (selectDirId?: number | null) => Promise<void>
  setPendingSelectRootId: (id: number | null) => void
}

export function useFolderRootActions(deps: FolderRootActionsDeps) {
  const { scan, media, toast, confirm, t, reloadTreePreserveExpansion, setPendingSelectRootId } =
    deps

  // ── 右键菜单 + 文件夹创建 ────────────────────────────────────────────────────
  const contextMenu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })
  const createDialog = ref({ isOpen: false, basePath: '' })

  function onNodeContextMenu(event: MouseEvent, node: DirNode) {
    const items: ContextMenuItem[] = [
      {
        id: 'new_subfolder',
        label: t('sidebar.newSubfolder'),
        icon: markRaw(FolderPlus),
        action: () => {
          createDialog.value.basePath = node.absPath || ''
          createDialog.value.isOpen = true
        },
      },
    ]
    // 「文件夹已迁移…」只对**扫描根**有意义（重链接改的是 scan_roots.path 这一行）。
    // 判据用 parentKey === null 而非 parentId：FS 模式下非根节点的 parentId 也恒为 null
    // （见 useFolderTree.ts 同款判据的注释）。
    if (node.parentKey === null) {
      items.push({
        id: 'relink_root',
        label: t('sidebar.relinkFolder'),
        icon: markRaw(FolderSymlink),
        action: () => void relinkRoot(node),
      })
    }
    contextMenu.value.items = items
    contextMenu.value.x = event.clientX
    contextMenu.value.y = event.clientY
    contextMenu.value.visible = true
  }

  // ── #7 方案A：文件夹迁移后重链接根路径 ──────────────────────────────────────────
  interface RelinkResult {
    root: { id: number; path: string }
    sampled: number
    matched: number
  }

  /**
   * 把一个扫描根改指到新位置（整体迁移场景，如 D 盘 → C 盘）。
   *
   * 免重扫、免重生成的地基在后端（cache_key 不含盘符）；前端职责是选路径、二次确认、
   * 按稳定 code 分流错误话术，成功后补发一次增量重扫兜底——`start_scan` 要的进度
   * Channel 只有前端造得出，后端无法自调。
   */
  async function relinkRoot(node: DirNode) {
    const rootId = node.rootId
    const alias = node.name
    const rootWasPresent = scan.scanRoots.some((root) => root.id === rootId)
    let path: string
    try {
      const selected = await open({ directory: true, multiple: false, title: t('sidebar.chooseDir') })
      if (!selected) return
      path = typeof selected === 'string' ? selected : selected[0]
      if (!path) return
    } catch (e) {
      toast.addToast('error', t('sidebar.chooseDirFailed', { error: ipcErrorMessage(e) }))
      return
    }

    const { confirmed } = await confirm({
      title: t('sidebar.relinkTitle'),
      message: t('sidebar.relinkMessage', { alias, path }),
      confirmText: t('sidebar.relinkConfirm'),
    })
    if (!confirmed) return

    // 抽样核对要 stat 上百个文件，冷盘上可达秒级 —— 先给个 info 免得像卡死了。
    toast.addToast('info', t('sidebar.relinkChecking'))
    // 两个错误边界(审查 F-08):重链接提交是一个事务边界,follow-up 重扫是另一个。原实现
    // 单 try/catch 罩两者——重扫启动失败会落进 relinkFailed 分支,在 success toast 之后再弹
    // 「重链接失败」,用户收到互相矛盾的结论并可能重复迁移。
    const expectedRunId = scan.getProgress(rootId)?.runId
    // 后端 relink 会在校验前取消旧轮；先封口本地旧轮。带 expectedRunId 使用户在此期间
    // 启动的新轮不被失败收尾误杀。
    if (expectedRunId !== undefined) scan.markScanStopped(rootId, expectedRunId)
    let res: RelinkResult
    try {
      res = await invokeIpc<RelinkResult>(IPC.RELINK_SCAN_ROOT, { rootId, newPath: path })
    } catch (e) {
      // relink 的后端安全闸在校验前就已撤销旧扫描 token；失败也不能让前端保留
      // 一个永远不会再收到终态的 running 状态。
      if (expectedRunId !== undefined) scan.markScanStopped(rootId, expectedRunId)
      const err = parseAppError(e)
      // 按稳定 code 分流（而非匹配文案，见 utils/ipc.ts 头注释）。
      if (err.code === 'relink_not_a_dir') {
        toast.addToast('error', t('sidebar.relinkNotADir'), 6000)
      } else if (err.code === 'relink_path_taken') {
        toast.addToast('error', t('sidebar.relinkPathTaken'), 6000)
      } else if (err.code === 'relink_mismatch') {
        // message 形如 "sample match 12/100 below threshold"，只取数字给用户看统计。
        const m = /(\d+)\/(\d+)/.exec(err.message)
        toast.addToast(
          'error',
          t('sidebar.relinkMismatch', { matched: m?.[1] ?? '?', sampled: m?.[2] ?? '?' }),
          8000,
        )
      } else {
        toast.addToast('error', t('sidebar.relinkFailed', { error: err.message }))
      }
      return
    }

    if (expectedRunId !== undefined) scan.markScanStopped(rootId, expectedRunId)
    // relink 已经在后端成功；根列表刷新只是 UI 同步，失败不能改判 relink 结果，
    // 也不能阻断下面必要的兜底重扫。
    let rootsRefreshIsCurrent = true
    try {
      rootsRefreshIsCurrent = (await scan.loadScanRoots()) !== false // 重建 scanRoots → 触发树重载 watch
    } catch {
      // 保持成功边界；下一次根列表刷新或用户操作会再次同步 UI。
    }
    toast.addToast(
      'success',
      t('sidebar.relinkDone', { alias, matched: res.matched, sampled: res.sampled }),
      6000,
    )

    if (!rootsRefreshIsCurrent) return
    // 重链接期间可能有别的操作为同一根安装了新运行轮次；该轮已不再属于本次重链接，
    // 不能被这里的兜底重扫覆盖。若清库/删根使原根从列表消失，也不向不存在的根发起旧重扫。
    if (scan.getProgress(rootId)?.runId !== expectedRunId) return
    if (rootWasPresent && !scan.scanRoots.some((root) => root.id === rootId)) return

    // 兜底增量重扫：捡起迁移期间可能发生的增删改。绝大多数项 mtime 未变 → upsert 判
    // Unchanged → 零派生产物重做，所以这一趟很便宜（真正的重活早被抽样闸挡在门外了）。
    // 重扫失败不推翻已成立的重链接——明说「已成功、重扫没起来、可手动重试」。
    try {
      await scan.startScan(rootId, () => {
        media.loadStats()
        window.dispatchEvent(new CustomEvent('folder-stats-changed'))
      })
    } catch (e) {
      toast.addToast('error', t('sidebar.relinkRescanFailed', { error: ipcErrorMessage(e) }), 8000)
    }
  }

  function createNewGlobalFolder() {
    createDialog.value.basePath = ''
    createDialog.value.isOpen = true
  }

  function onFolderCreated() {
    // 新建（子）文件夹可能不改变 scan.scanRoots，因此直接重载树。
    reloadTreePreserveExpansion()
  }

  // ── 导入一个已有文件夹作为扫描根目录 ─────────────────────────────────────────
  interface OverlapInfo {
    id: number
    path: string
    alias: string | null
  }
  interface FolderOverlapResult {
    children: OverlapInfo[]
    parents: OverlapInfo[]
  }

  async function addRoot() {
    try {
      const selected = await open({ directory: true, multiple: false, title: t('sidebar.chooseDir') })
      if (!selected) return
      const path = typeof selected === 'string' ? selected : selected[0]
      if (!path) return

      // 第一步：检查与现有根目录是否重叠。
      const overlap = await invokeIpc<FolderOverlapResult>(IPC.CHECK_FOLDER_OVERLAP, {
        newPath: path,
      })
      if (overlap.children.length > 0) {
        const childNames = overlap.children.map((c) => c.alias || c.path).join(', ')
        const { confirmed: merge } = await confirm({
          title: t('sidebar.overlapDetected'),
          message: t('sidebar.overlapParentMsg', { path, children: childNames }),
          confirmText: t('sidebar.mergeAndReplace'),
          cancelText: t('sidebar.addAnyway'),
        })
        if (merge) {
          for (const child of overlap.children) {
            await invokeIpc(IPC.REMOVE_SCAN_ROOT_WITH_OPTIONS, {
              id: child.id,
              clearThumbnails: false,
            })
          }
        }
      } else if (overlap.parents.length > 0) {
        const parentNames = overlap.parents.map((p) => p.alias || p.path).join(', ')
        const { confirmed: proceed } = await confirm({
          title: t('sidebar.overlapDetected'),
          message: t('sidebar.overlapChildMsg', { path, parents: parentNames }),
          confirmText: t('sidebar.addAnyway'),
          cancelText: t('common.cancel'),
        })
        if (!proceed) return
      }

      // 第二步：添加根目录，随后由 scanRoots 的
      // watch 加载树，并在加载完成后选中新根节点。
      try {
        const root = await scan.addScanRoot(path)
        setPendingSelectRootId(root.id)
        await scan.loadScanRoots() // 重建 scanRoots → 触发上方 watch
        await scan.startScan(root.id, () => {
          media.loadStats()
          window.dispatchEvent(new CustomEvent('folder-stats-changed'))
        })
      } catch (e) {
        toast.addToast('error', t('sidebar.addFolderFailed') + ' ' + e)
      }
    } catch (e) {
      toast.addToast('error', t('sidebar.chooseDirFailed') + ' ' + e)
    }
  }

  // 空画廊「添加文件夹」引导按钮(MediaGrid 空状态,§6.3)经此事件复用完整 addRoot
  // 流程(重叠检测/自动扫描/树选中),避免在画廊侧复制这段逻辑。§2.3-④:随 addRoot 一起
  // 下沉到本 composable 自己的 onMounted/onBeforeUnmount,不与 folder-stats-changed 共用挂载点。
  function onRequestAddFolder() {
    addRoot()
  }
  onMounted(() => {
    window.addEventListener('request-add-folder', onRequestAddFolder)
  })
  onBeforeUnmount(() => {
    window.removeEventListener('request-add-folder', onRequestAddFolder)
  })

  return {
    contextMenu,
    createDialog,
    onNodeContextMenu,
    relinkRoot,
    createNewGlobalFolder,
    onFolderCreated,
    addRoot,
  }
}
