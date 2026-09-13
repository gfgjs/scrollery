// src/composables/useCollectionToast.ts
// 收藏后右下角的「加入收藏夹」提示（需求7, §3.7）—— 含最近使用收藏夹的快捷 chips 与「新建」。

import { useCollectionStore } from '../stores/collectionStore'
import { useToastStore } from '../stores/toastStore'
import i18n from '../i18n'
import type { ToastAction } from '../types/ui'

export function useCollectionToast() {
  const store = useCollectionStore()
  const toast = useToastStore()

  /** 为一组项弹出加入收藏夹提示。`prefixLabel` 覆盖默认「已收藏…」前缀（如工具栏传「选中 N 项」，
   *  因其非收藏动作）。 */
  async function showAddToCollection(itemIds: number[], prefixLabel?: string) {
    if (itemIds.length === 0) return

    const recents = await store.recent(3)
    const actions: ToastAction[] = recents.map((c) => ({
      label: c.name,
      onClick: async () => {
        await store.addItems(c.id, itemIds)
        toast.addToast('success', i18n.global.t('collections.addedTo', { name: c.name }))
      },
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
          await store.addItems(id, itemIds)
          toast.addToast('success', i18n.global.t('collections.addedTo', { name }))
        }
      },
    })

    const prefix =
      prefixLabel ??
      (itemIds.length > 1
        ? i18n.global.t('selection.favorited', { count: itemIds.length })
        : i18n.global.t('selection.favoritedOne'))
    // 时长更久，给用户挑选收藏夹 chip 的时间。
    toast.addToast('success', i18n.global.t('collections.addToPrompt', { prefix }), 6000, actions)
  }

  return { showAddToCollection }
}
