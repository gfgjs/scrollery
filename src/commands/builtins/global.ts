// 通用右键命令从 contextTarget 取得目标；移动和复制文件的对话框由各宿主提供。

import { markRaw } from 'vue'
import { Copy, FolderOpen, Monitor } from '@lucide/vue'
import type { Command } from '../types'
import { invokeIpc } from '../../utils/ipc'
import { logger } from '../../utils/logger'
import { IPC } from '../../constants/ipc'
import { useToastStore } from '../../stores/toastStore'
import i18n from '../../i18n'

const t = (key: string) => i18n.global.t(key)

export const globalCommands: Command[] = [
  {
    id: 'global.copyImage',
    title: () => t('contextMenu.copyImage'),
    icon: markRaw(Copy),
    group: 'overflow',
    order: 10,
    // 目标由右键项 / 当前查看资产给;无 target 不出。
    when: (ctx) => ctx.contextTarget != null,
    run: (ctx) => {
      if (ctx.contextTarget) {
        invokeIpc(IPC.COPY_IMAGE_TO_CLIPBOARD, { itemId: ctx.contextTarget.id }).catch(() => {})
      }
    },
  },
  {
    id: 'global.showInExplorer',
    title: () => t('contextMenu.showInExplorer'),
    icon: markRaw(FolderOpen),
    group: 'overflow',
    order: 20,
    when: (ctx) => ctx.contextTarget != null,
    run: async (ctx) => {
      if (!ctx.contextTarget) return
      // 此前是 `.catch(() => {})` 静默吞——那时后端是 `Command::spawn` 发射后不管、几乎不会失败,
      // 吞掉无碍。reveal 统一到 opener 后(F-015)后端会校验路径存在并同步等结果,
      // 「文件已被外部删除」成了常见真实失败;继续静默吞 = 点了没反应也没解释。
      try {
        await invokeIpc(IPC.SHOW_IN_EXPLORER, { itemId: ctx.contextTarget.id })
      } catch (err) {
        logger.error('show_in_explorer failed', { error: err })
        // 按稳定 code 分流(R-08/R-09):unsupported_platform = 平台永不支持,给专属文案。
        const code = (err as { code?: string } | null)?.code
        useToastStore().addToast(
          'error',
          t(
            code === 'unsupported_platform'
              ? 'contextMenu.revealUnsupported'
              : 'contextMenu.showInExplorerFailed',
          ),
        )
      }
    },
  },
  {
    id: 'global.setWallpaper',
    title: () => t('contextMenu.setWallpaper'),
    icon: markRaw(Monitor),
    group: 'overflow',
    order: 50,
    // 仅图片——承接现状硬编码的 `mediaType === 'image'` 条件。这正是计划要收敛的「按媒体类型换项」
    // 逻辑:此前 MediaGrid / MediaDetailOverlay 各硬编码一份,现单点定义于此。
    when: (ctx) => ctx.contextTarget?.mediaType === 'image',
    run: async (ctx) => {
      if (!ctx.contextTarget) return
      // 对齐收敛前行为:成功弹 toast,失败仅 console.error(不弹错误 toast)。
      try {
        await invokeIpc(IPC.SET_AS_WALLPAPER, { itemId: ctx.contextTarget.id })
        useToastStore().addToast('success', t('contextMenu.wallpaperSet'))
      } catch (err) {
        logger.error('set_as_wallpaper failed', { error: err })
      }
    },
  },
]
