// src/composables/useContentViewerContextMenu.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):右键菜单 + 移动/复制对话框。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。

import { ref, markRaw } from 'vue'
import type { ContextMenuItem } from '../components/common/ContextMenu.vue'
import { Copy, FolderInput } from '@lucide/vue'
import {
  buildCommandContext,
  resolveMediaContextCommands,
  resolveCommandTitle,
  type Command,
} from '../commands'
import type { DirNode, MediaDetail } from '../types/media'
import type { useMediaStore } from '../stores/mediaStore'
import type { useHistoryStore } from '../stores/historyStore'
import type { useToastStore } from '../stores/toastStore'

export function useContentViewerContextMenu(options: {
  detail: () => MediaDetail | null
  media: ReturnType<typeof useMediaStore>
  history: ReturnType<typeof useHistoryStore>
  toast: ReturnType<typeof useToastStore>
  t: (key: string, params?: Record<string, unknown>) => string
  navigate: (offset: number) => Promise<void>
  close: () => void
}) {
  const { detail, media, history, toast, t, navigate, close } = options

  const ctxMenu = ref({
    visible: false,
    x: 0,
    y: 0,
    items: [] as ContextMenuItem[],
  })

  const moveCopyDialog = ref({
    isOpen: false,
    mode: 'move' as 'move' | 'copy',
    targetId: null as number | null,
  })

  function onContextMenu(e: MouseEvent) {
    if (!detail()) return
    const item = detail()!
    const id = item.id

    // 收敛(P2-3):共享项(复制图像/资源管理器显示/壁纸-仅图片)从命令注册表按 when 生成,与 MediaGrid
    // 同源(壁纸-仅图片条件单点定义于 global.ts)。move/copy 是本地 Command(捕获覆盖层单项流:直接把
    // 该项塞进 moveCopyDialog)。
    const ctx = buildCommandContext({ contextTarget: { id, mediaType: item.mediaType } })
    const localMoveCopy: Command[] = [
      {
        id: 'viewer.contextMove',
        title: () => t('common.moveTo'),
        icon: markRaw(FolderInput),
        group: 'organize',
        order: 30,
        run: () => {
          moveCopyDialog.value.mode = 'move'
          moveCopyDialog.value.targetId = id
          moveCopyDialog.value.isOpen = true
        },
      },
      {
        id: 'viewer.contextCopy',
        title: () => t('common.copyTo'),
        icon: markRaw(Copy),
        group: 'organize',
        order: 40,
        run: () => {
          moveCopyDialog.value.mode = 'copy'
          moveCopyDialog.value.targetId = id
          moveCopyDialog.value.isOpen = true
        },
      },
    ]

    ctxMenu.value = {
      visible: true,
      x: e.clientX,
      y: e.clientY,
      items: resolveMediaContextCommands(ctx, localMoveCopy).map((cmd) => ({
        id: cmd.id,
        label: resolveCommandTitle(cmd),
        icon: cmd.icon,
        action: () => cmd.run(ctx),
      })),
    }
  }

  async function onMoveCopyConfirm(targetNode: DirNode) {
    const id = moveCopyDialog.value.targetId
    // targetNode 为选中的 DirNode,以 id(目录 id)为落点。
    if (!id || targetNode?.id == null) return
    moveCopyDialog.value.isOpen = false
    const mode = moveCopyDialog.value.mode

    // T6:经 historyStore 走 relocate_media_items / copy_media_items_db(DB 级、可撤销),
    // 与画廊 performMediaDrop / MediaGrid 对话框同一路径。history 内部 refresh() 已重载树(实时计数)
    // + 重算网格,故不再手动调整计数 / loadStats / startScan。
    try {
      const n =
        mode === 'copy'
          ? await history.copyMedia([id], targetNode.id, `复制 1 项`)
          : await history.moveMedia([id], targetNode.id, `移动 1 项`)
      if (n === 0) return // 无实际移动/复制(已在目标 / 冲突)

      toast.addToast(
        'success',
        mode === 'move' ? t('contextMenu.moveSuccess') : t('contextMenu.copySuccess'),
      )

      // 移动后当前项已离开视图 → navigate(1) 跳下一张并同步路由;若仍是被移动项(邻接无下一张,
      // 它原是最后一张)→ 离开查看器回网格。await 修原覆盖层未 await 致「恒判最后一张」的时序 bug。
      if (mode === 'move') {
        await navigate(1)
        if (media.detailItem?.id === id) close()
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      toast.addToast(
        'error',
        mode === 'copy'
          ? t('common.copyFailed', { error: msg })
          : t('common.moveFailed', { error: msg }),
      )
    }
  }

  return { ctxMenu, moveCopyDialog, onContextMenu, onMoveCopyConfirm }
}
