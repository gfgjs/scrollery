// 画廊右键菜单构建(自 MediaGrid.vue 结构拆分抽出,判据与选区语义逐字保留)。
import { markRaw, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSelection } from './useSelection'
import {
  buildCommandContext,
  resolveMediaContextCommands,
  resolveCommandTitle,
  type Command,
} from '../commands'
import { Copy, FolderInput, Download } from '@lucide/vue'
import type { ContextMenuItem } from '../components/common/ContextMenu.vue'
import type { LayoutRow, LayoutRowItem } from '../types/layout'
import type { MediaType } from '../types/media'

export interface GalleryContextMenuDeps {
  activeRows: () => LayoutRow[]
  /** §8.1 browse-only 谓词:重复镜头激活时不构建右键菜单（宿主从 duplicateLensStore 注入,
   *  本 composable 不直接依赖镜头 store）。 */
  lensActive: () => boolean
  startBatchMove: () => void
  startBatchCopy: () => void
  startExportSelection: () => void
}

export function useGalleryContextMenu(deps: GalleryContextMenuDeps) {
  const { t } = useI18n()
  const selection = useSelection()

  const ctxMenu = ref({
    visible: false,
    x: 0,
    y: 0,
    items: [] as ContextMenuItem[],
    targetId: null as number | null,
  })

  function onContextMenu(e: MouseEvent, id: number) {
    e.preventDefault()
    // §8.1 browse-only:重复镜头是浏览态,不提供右键菜单（菜单里全是收藏/move/copy/导出等
    // 选择与批量入口,§2.2 非目标）,直接吞掉。
    if (deps.lensActive()) return
    ctxMenu.value.targetId = id
    ctxMenu.value.x = e.clientX
    ctxMenu.value.y = e.clientY

    // 判别式收窄(R2-3,镜像 patchVisibleRating 示范):rowType 守卫后 items 已正确类型化。
    let item: LayoutRowItem | null = null
    for (const row of deps.activeRows()) {
      if (row.rowType !== 'normal') continue
      item = row.items.find((i) => i.id === id) ?? null
      if (item) break
    }

    // 收敛(P2-3):共享项(复制图像/资源管理器显示/壁纸-仅图片)从命令注册表按 when 生成——
    // 「壁纸仅图片」条件此前在本组件与 MediaDetailOverlay 各硬编码一份,现单点定义于 global.ts。
    // move/copy 是本地 Command(批量流→批量对话框):右键项**已在选区内**则保留整个选区、作用于所有已选;
    // 右键项**不在选区**才替换为它(经典「右键未选项 = 选中并作用于它」)。此前无条件清选区→单张,
    // 令多选右键 move/copy 被误塌缩为单张(真机反馈修复)。共享项仍读 contextTarget 单目标,不受影响。
    // LayoutRowItem.mediaType 是后端来源的宽松 string(见 types/layout.ts),但恒持 MediaType 值;
    // 局部断言收窄以喂 contextTarget(壁纸命令 when 据此判 image),非 as any。
    const ctx = buildCommandContext({
      contextTarget: { id, mediaType: item?.mediaType as MediaType | undefined },
    })
    const localOrganizeCommands: Command[] = [
      {
        id: 'grid.contextMove',
        title: () => t('common.moveTo'),
        icon: markRaw(FolderInput),
        group: 'organize',
        order: 30,
        run: () => {
          // 右键项已在选区内 → 保留整个选区,move 作用于所有已选;不在选区才替换为该项。
          if (!selection.isSelected(id)) {
            selection.clearSelection()
            selection.toggleSelect(id)
          }
          deps.startBatchMove()
        },
      },
      {
        id: 'grid.contextCopy',
        title: () => t('common.copyTo'),
        icon: markRaw(Copy),
        group: 'organize',
        order: 40,
        run: () => {
          // 右键项已在选区内 → 保留整个选区,copy 作用于所有已选;不在选区才替换为该项。
          if (!selection.isSelected(id)) {
            selection.clearSelection()
            selection.toggleSelect(id)
          }
          deps.startBatchCopy()
        },
      },
      {
        id: 'grid.contextExport',
        title: () => t('selection.export'),
        icon: markRaw(Download),
        group: 'organize',
        order: 50,
        run: () => {
          // 同 move/copy:右键项已在选区内则保留整个选区,否则替换为该项(方案 A §4)。
          if (!selection.isSelected(id)) {
            selection.clearSelection()
            selection.toggleSelect(id)
          }
          deps.startExportSelection()
        },
      },
    ]

    ctxMenu.value.items = resolveMediaContextCommands(ctx, localOrganizeCommands).map((cmd) => ({
      id: cmd.id,
      label: resolveCommandTitle(cmd),
      icon: cmd.icon,
      action: () => cmd.run(ctx),
    }))
    ctxMenu.value.visible = true
  }

  return { ctxMenu, onContextMenu }
}
