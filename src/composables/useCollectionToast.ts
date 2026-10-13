// src/composables/useCollectionToast.ts
// 收藏后右下角的「加入收藏夹」提示，含最近使用收藏夹的快捷 chips 与「新建」。

import { useCollectionStore } from '../stores/collectionStore'
import { useToastStore } from '../stores/toastStore'
import i18n from '../i18n'
import { ipcErrorMessage } from '../utils/ipc'
import type { ToastAction } from '../types/ui'

export function useCollectionToast() {
  const store = useCollectionStore()
  const toast = useToastStore()

  async function addToCollection(albumId: number, name: string, ids: number[]): Promise<void> {
    try {
      await store.addItems(albumId, ids)
      toast.addToast('success', i18n.global.t('collections.addedTo', { name }))
    } catch (e) {
      // 原提示会在动作结束后关闭；新提示保留同一目标与批量，重试不再创建收藏夹。
      toast.addToast('error', `${name}: ${ipcErrorMessage(e)}`, 10000, [
        { label: i18n.global.t('common.retry'), onClick: () => addToCollection(albumId, name, ids) },
      ])
    }
  }

  /** 为一组项弹出加入收藏夹提示。`prefixLabel` 覆盖默认「已收藏…」前缀（如工具栏传「选中 N 项」，
   *  因其非收藏动作）。 */
  async function showAddToCollection(itemIds: number[], prefixLabel?: string) {
    if (itemIds.length === 0) return
    // 提示与重试可晚于选区变化，始终提交触发时的这一批资产。
    const ids = [...itemIds]

    const recents = await store.recent(3)
    const actions: ToastAction[] = recents.map((c) => ({
      label: c.name,
      onClick: () => addToCollection(c.id, c.name, ids),
    }))

    // 「新建」chip：弹名输入 → 新建 → 加入。WebView2 支持 window.prompt；取消/留空即空操作。
    actions.push({
      label: i18n.global.t('collections.newChip'),
      onClick: async () => {
        let name: string | null = null
        try {
          name = window.prompt(i18n.global.t('collections.newNamePrompt'))
        } catch {
          name = null
        }
        name = name?.trim() ?? ''
        if (!name) return
        const id = await store.create(name)
        if (id != null) {
          await addToCollection(id, name, ids)
        }
      },
    })

    const prefix =
      prefixLabel ??
      (ids.length > 1
        ? i18n.global.t('selection.favorited', { count: ids.length })
        : i18n.global.t('selection.favoritedOne'))
    // 时长更久，给用户挑选收藏夹 chip 的时间。
    toast.addToast('success', i18n.global.t('collections.addToPrompt', { prefix }), 6000, actions)
  }

  return { showAddToCollection }
}
