// src/composables/useFolderTreeDragDrop.ts
// 目录 + 文件两路指针拖拽、共用的 ghost/边缘自动滚动/落点高亮——从 FoldersSection.vue 域 16、
// 17、18(以及域 15 的 canDropOnId/recomputeDrop)迁出(见
// docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
//
// §3 风险④(不变量):autoScrollRaf/autoScrollDir/autoScrollRecompute/lastDragPoint 四个状态
// 必须整体留在本 composable 内(不可再切分目录拖拽与文件拖拽到两个文件),否则两路拖拽会各自
// 起一个 rAF 循环,失去「贴边滚动全局单飞」的不变量;本 composable 自己注册
// onBeforeUnmount(stopAutoScroll),避免卸载时残留悬空 rAF(T1-c)。
import { ref, onBeforeUnmount, type ComputedRef, type ShallowRef } from 'vue'
import { beginPointerDrag, DRAG_THRESHOLD } from './usePointerDrag'
import {
  hasEntityIdentity,
  canDropFileOnDir,
} from '../components/sidebar/sections/folderTree.helpers'
import type { DirNode, DirFile } from '../types/media'
import type { useHistoryStore } from '../stores/historyStore'
import type { useToastStore } from '../stores/toastStore'

export interface FolderTreeDragDropDeps {
  nodesById: ComputedRef<Map<number, DirNode>>
  nodesByKey: ComputedRef<Map<string, DirNode>>
  isDescendant: (ancestorId: number, nodeId: number) => boolean
  scrollAreaEl: ShallowRef<HTMLElement | null>
  history: ReturnType<typeof useHistoryStore>
  toast: ReturnType<typeof useToastStore>
  t: (key: string, params?: Record<string, unknown>) => string
}

export function useFolderTreeDragDrop(deps: FolderTreeDragDropDeps) {
  const { nodesById, nodesByKey, isDescendant, scrollAreaEl, history, toast, t } = deps

  // ── Tree drag move / copy (pointer-based) ───────────────────────────────────
  // ── 树拖拽移动 / 复制（基于指针） ────────────────────────────────────────────
  const dragId = ref<number | null>(null)
  const dropId = ref<number | null>(null)
  // 文件行拖拽源的**媒体 id**(D-003)。与 dragId(目录 id)分开存:两个 id 空间,混用会把
  // 媒体 id 误入 nodesById/canDropOnId 的目录索引。二者不会同时非空(一次只有一种拖拽在途)。
  const dragFileId = ref<number | null>(null)
  const ghost = ref<{ visible: boolean; x: number; y: number; label: string; copy: boolean }>({
    visible: false,
    x: 0,
    y: 0,
    label: '',
    copy: false,
  })

  // §2.3-②:suppressClick 现状是组件作用域共享的 let 布尔,拆分后由本 composable 内部持有,
  // 对外只暴露 consumeSuppressClick()(读取并复位,语义与现状完全一致——§3 风险⑦:必须是原子
  // 的一步,不能拆成先读后写两次调用)。
  let suppressClick = false // set when a press became a drag, so the trailing click is ignored
  // 当按下变成拖拽时置位，以忽略尾随的 click
  function consumeSuppressClick(): boolean {
    if (suppressClick) {
      suppressClick = false
      return true
    }
    return false
  }

  /** Whether dragged dir `srcId` may drop onto dir `targetId`. | 被拖目录 `srcId` 能否落到 `targetId`。 */
  function canDropOnId(srcId: number, targetId: number): boolean {
    if (targetId === srcId) return false
    const src = nodesById.value.get(srcId)
    if (!src) return false
    if (src.parentId === targetId) return false // already there | 已在目标中
    if (isDescendant(srcId, targetId)) return false // would create a cycle | 会成环
    return true
  }

  // 据指针位置(命中 [data-dir-id] 行)重算落点。虚拟化后目标行可能因滚动进出 DOM,故拖拽期间
  // 与「边缘自动滚动」每帧都重算(T1-c)。
  function recomputeDrop(srcId: number, ev: { clientX: number; clientY: number }) {
    const item = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest(
      '[data-dir-id]',
    ) as HTMLElement | null
    const targetId = item ? Number(item.dataset.dirId) : null
    dropId.value = targetId != null && canDropOnId(srcId, targetId) ? targetId : null
  }

  // ── 拖拽边缘自动滚动(T1-c)──────────────────────────────────────────────────────
  // 虚拟化后视口外的目标目录不在 DOM,无法作为落点。拖拽时指针靠近共享滚动区上/下边缘,启动独立
  // rAF 循环持续滚动(速度随贴边加深),把视口外目标滚进 DOM;每帧重算落点。指针静止贴边也持续滚。
  const EDGE_ZONE = 52 // 触发自动滚动的边缘带宽(px)
  const EDGE_MAX_SPEED = 16 // 最贴边时每帧滚动量(px)
  let autoScrollRaf: number | null = null
  let autoScrollDir = 0 // -1 上滚 / +1 下滚 / 0 关
  // 每帧落点重算回调:由在途拖拽注入(目录拖拽=recomputeDrop、文件拖拽=recomputeFileDrop,D-003)。
  // 此前写死 recomputeDrop(autoScrollSrcId, …),文件行拖拽接入后按回调解耦,行为不变。
  let autoScrollRecompute: ((pt: { clientX: number; clientY: number }) => void) | null = null
  let lastDragPoint: { clientX: number; clientY: number } | null = null

  function autoScrollStep() {
    if (!scrollAreaEl.value || autoScrollDir === 0 || !lastDragPoint) {
      autoScrollRaf = null
      return
    }
    const rect = scrollAreaEl.value.getBoundingClientRect()
    const y = lastDragPoint.clientY
    // 贴边越深越快:dist 为指针到该边缘的距离(0=紧贴),归一化取速度。
    const dist = autoScrollDir < 0 ? y - rect.top : rect.bottom - y
    const speed = Math.max(2, EDGE_MAX_SPEED * (1 - Math.max(0, dist) / EDGE_ZONE))
    scrollAreaEl.value.scrollTop += autoScrollDir * speed
    // 滚动改变 DOM 内容(下一帧渲染新窗口行),据静止指针重算落点。
    if (autoScrollRecompute) autoScrollRecompute(lastDragPoint)
    autoScrollRaf = requestAnimationFrame(autoScrollStep)
  }

  // 据指针 y 更新自动滚动方向;进入边缘带则启动 rAF,离开则停。
  function updateAutoScroll(
    recompute: (pt: { clientX: number; clientY: number }) => void,
    ev: PointerEvent,
  ) {
    if (!scrollAreaEl.value) return
    lastDragPoint = { clientX: ev.clientX, clientY: ev.clientY }
    autoScrollRecompute = recompute
    const rect = scrollAreaEl.value.getBoundingClientRect()
    if (ev.clientY < rect.top + EDGE_ZONE) autoScrollDir = -1
    else if (ev.clientY > rect.bottom - EDGE_ZONE) autoScrollDir = 1
    else autoScrollDir = 0
    if (autoScrollDir !== 0 && autoScrollRaf === null) {
      autoScrollRaf = requestAnimationFrame(autoScrollStep)
    }
  }

  function stopAutoScroll() {
    if (autoScrollRaf !== null) cancelAnimationFrame(autoScrollRaf)
    autoScrollRaf = null
    autoScrollDir = 0
    autoScrollRecompute = null
    lastDragPoint = null
  }
  onBeforeUnmount(stopAutoScroll) // 拖拽中卸载兜底,避免悬空 rAF(T1-c)

  function onTreePointerDown(node: DirNode, e: PointerEvent) {
    if (e.button !== 0 || node.parentId === null) return // left button only; scan roots aren't movable
    // FS-only 目录不可作拖拽源(§4.1):移动/复制 IPC 收的是 DB 目录 id,它没有。
    // 就地捕获成局部常量而非在闭包里反复判空——闭包内 TS 无法据外层判空收窄可变字段。
    if (!hasEntityIdentity(node)) return
    const srcId = node.id
    suppressClick = false
    const startX = e.clientX,
      startY = e.clientY
    let dragging = false

    beginPointerDrag(
      (ev) => {
        if (!dragging) {
          if (Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY) < DRAG_THRESHOLD) return
          dragging = true
          suppressClick = true
          dragId.value = srcId
          document.body.style.userSelect = 'none'
          document.body.style.cursor = 'grabbing'
          ghost.value = {
            visible: true,
            x: ev.clientX,
            y: ev.clientY,
            label: node.name,
            copy: ev.ctrlKey || ev.metaKey,
          }
        }
        ghost.value.x = ev.clientX
        ghost.value.y = ev.clientY
        ghost.value.copy = ev.ctrlKey || ev.metaKey
        recomputeDrop(srcId, ev)
        updateAutoScroll((pt) => recomputeDrop(srcId, pt), ev) // 贴边则启动/维持自动滚动,把视口外目标滚进 DOM(T1-c)
      },
      (ev, cancelled) => {
        stopAutoScroll()
        const srcId = dragId.value,
          targetId = dropId.value
        const copy = ev.ctrlKey || ev.metaKey
        dragId.value = null
        dropId.value = null
        ghost.value.visible = false
        if (!cancelled && dragging && srcId != null && targetId != null) {
          performTreeDrop(srcId, targetId, copy)
        }
      },
    )
  }

  async function performTreeDrop(srcId: number, targetId: number, copy: boolean) {
    const src = nodesById.value.get(srcId)
    const target = nodesById.value.get(targetId)
    if (!src || !target || src.parentId == null || !canDropOnId(srcId, targetId)) return
    // src/target 取自 nodesById——那张索引里只有具实体身份的节点,故这里 id 必非 null。
    // 但把「查得到 ⇒ id 非空」当成类型事实是靠不住的(索引的构造与本处相隔千行,§4.1 要求
    // **显式**区分两种身份而非靠碰巧有值)。故用入参 srcId/targetId:它们本就是 number。
    try {
      if (copy) {
        await history.copy(srcId, src.name, targetId)
        toast.addToast('success', t('sidebar.copiedTo', { name: src.name, target: target.name }))
      } else {
        // 半完成（物理已搬、索引/清理未完）已由恢复清单提示真实落点与重试：这里不报「已移动」。
        const outcome = await history.move(srcId, src.name, src.parentId, targetId)
        if (outcome === 'complete')
          toast.addToast('success', t('sidebar.movedTo', { name: src.name, target: target.name }))
      }
    } catch (err) {
      const e = err as { code?: string; message?: string } | null | undefined
      if (e && e.code === 'DirectoryExists') {
        toast.addToast('error', t('sidebar.dirExistsNoMerge', { name: e.message }))
      } else {
        toast.addToast('error', t('sidebar.opFailed', { error: e?.message ?? err }))
      }
    }
  }

  // ── 文件行拖拽(D-003):拖到树内目录 = 移动物理文件,Ctrl/⌘ = 复制 ─────────────
  // 与目录行同手势(阈值/ghost/边缘自动滚动/落点高亮),执行链复用画廊「拖到文件夹」的
  // history.moveMedia/copyMedia(relocate_media_items:同 item id 保留 → 缩略图/AI 嵌入
  // 不失效,undo/redo 现成,树快照失效由后端 InvalidateOnWrite 覆盖)——零后端改动。

  // 文件版落点重算:命中面同 [data-dir-id](FS-only 目录 id 为 null,Vue 不渲染该属性,
  // 天然不在命中面);无环检测(文件无子树),只拒落回当前所在目录。
  function recomputeFileDrop(
    fileParentDirId: number | null,
    ev: { clientX: number; clientY: number },
  ) {
    const item = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest(
      '[data-dir-id]',
    ) as HTMLElement | null
    const targetId = item ? Number(item.dataset.dirId) : null
    dropId.value =
      targetId != null &&
      nodesById.value.has(targetId) &&
      canDropFileOnDir(fileParentDirId, targetId)
        ? targetId
        : null
  }

  function onFilePointerDown(file: DirFile, e: PointerEvent) {
    if (e.button !== 0) return // left button only
    // 未入库文件没有实体身份(§4.1):relocate 收的是媒体 id,它没有——不起手(D-003 v1;
    // 未入库文件的移动叠加问题②身份缺失,显式后置)。
    if (!hasEntityIdentity(file)) return
    const mediaId = file.id
    const fileName = file.fileName
    // 当前所在目录的实体 id:FS 模式下父目录行可能取不到(null)→ 落点判定放行,
    // 后端 relocate 对同目录 no-op 兜底(canDropFileOnDir 注释)。
    const parentDirId = nodesByKey.value.get(file.parentKey)?.id ?? null
    suppressClick = false
    const startX = e.clientX,
      startY = e.clientY
    let dragging = false

    beginPointerDrag(
      (ev) => {
        if (!dragging) {
          if (Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY) < DRAG_THRESHOLD) return
          dragging = true
          suppressClick = true
          dragFileId.value = mediaId
          document.body.style.userSelect = 'none'
          document.body.style.cursor = 'grabbing'
          ghost.value = {
            visible: true,
            x: ev.clientX,
            y: ev.clientY,
            label: fileName,
            copy: ev.ctrlKey || ev.metaKey,
          }
        }
        ghost.value.x = ev.clientX
        ghost.value.y = ev.clientY
        ghost.value.copy = ev.ctrlKey || ev.metaKey
        recomputeFileDrop(parentDirId, ev)
        updateAutoScroll((pt) => recomputeFileDrop(parentDirId, pt), ev)
      },
      (ev, cancelled) => {
        stopAutoScroll()
        const srcMediaId = dragFileId.value,
          targetId = dropId.value
        const copy = ev.ctrlKey || ev.metaKey
        dragFileId.value = null
        dropId.value = null
        ghost.value.visible = false
        if (!cancelled && dragging && srcMediaId != null && targetId != null) {
          void performFileDrop(srcMediaId, fileName, targetId, copy)
        }
      },
    )
  }

  async function performFileDrop(
    mediaId: number,
    fileName: string,
    targetId: number,
    copy: boolean,
  ) {
    const target = nodesById.value.get(targetId)
    if (!target) return
    try {
      if (copy) {
        // moveMedia/copyMedia 返回实际生效条数:0 = 后端 no-op(已在目标目录/同名冲突跳过),
        // 不弹成功 toast——「已复制」而实际什么都没发生是误报。
        const n = await history.copyMedia([mediaId], targetId, fileName)
        if (n > 0)
          toast.addToast('success', t('sidebar.copiedTo', { name: fileName, target: target.name }))
      } else {
        const n = await history.moveMedia([mediaId], targetId, fileName)
        if (n > 0)
          toast.addToast('success', t('sidebar.movedTo', { name: fileName, target: target.name }))
      }
    } catch (err) {
      const e = err as { message?: string } | null | undefined
      toast.addToast('error', t('sidebar.opFailed', { error: e?.message ?? err }))
    }
  }

  return {
    dragId,
    dropId,
    dragFileId,
    ghost,
    onTreePointerDown,
    onFilePointerDown,
    consumeSuppressClick,
  }
}
