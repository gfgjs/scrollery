// src/stores/viewStore.ts
// 当前视图筛选域 —— 从 uiStore 上帝 store 拆出的独立域(P1-21 渐进拆分第二刀)。
// 驱动画廊过滤维度的「当前视图上下文」:智能相册 / 目录 / 收藏夹 / 人物簇,**四者互斥**——
// 设定任一即清空其余三个,并清空照片多选集(视图切换后旧选区无意义,业界相册成熟做法)。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { SmartAlbum } from '../types/ui'
import type { Collection } from '../types/media'
import { useSelection } from '../composables/useSelection'

export const useViewStore = defineStore('view', () => {
  /** 初次 URL/设置恢复完成后才允许画廊提交布局。 */
  const galleryQueryReady = ref(false)
  // ── 四个互斥的视图筛选维度 ─────────────────────────────────────────────
  const activeSmartAlbum = ref<SmartAlbum>('all')
  const activeDirectoryId = ref<number | null>(null)
  // 当前打开的收藏夹，设置后驱动网格过滤。与智能相册/目录/人物视图互斥。
  const activeCollection = ref<Collection | null>(null)
  // 当前查看的人物簇（F6 人物墙 → 某人物的照片）。第四个互斥视图筛选。
  const activePersonId = ref<number | null>(null)

  // 视图上下文切换即清空照片多选集（业界相册成熟做法）。选区是 useSelection 模块级单例，
  // 与具体视图无关，切换过滤维度后旧选区无意义且易误操作 → 统一在四个切视图入口清掉。
  const { clearSelection } = useSelection()

  function setSmartAlbum(album: SmartAlbum) {
    activeSmartAlbum.value = album
    activeDirectoryId.value = null
    activeCollection.value = null
    activePersonId.value = null
    clearSelection()
  }

  function setActiveDirectory(id: number | null) {
    activeDirectoryId.value = id
    activeSmartAlbum.value = 'all'
    activeCollection.value = null
    activePersonId.value = null
    clearSelection()
  }

  function setActiveCollection(c: Collection | null) {
    activeCollection.value = c
    activeSmartAlbum.value = 'all'
    activeDirectoryId.value = null
    activePersonId.value = null
    clearSelection()
  }

  function setActivePerson(id: number | null) {
    activePersonId.value = id
    activeSmartAlbum.value = 'all'
    activeDirectoryId.value = null
    activeCollection.value = null
    clearSelection()
  }

  return {
    galleryQueryReady,
    activeSmartAlbum,
    activeDirectoryId,
    activeCollection,
    activePersonId,
    setSmartAlbum,
    setActiveDirectory,
    setActiveCollection,
    setActivePerson,
  }
})
