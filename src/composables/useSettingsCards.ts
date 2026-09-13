// src/composables/useSettingsCards.ts
// 设置页可折叠卡片的全局协调器（模块级单例）。
//
// 为什么用单例而非 provide/inject：CollapsibleCard 既直接用于 SettingsView，也嵌套在
// ModelLibrary / NetworkStorageSection 等子组件内。单例无需层层 provide 即可让「一键
// 全部折叠/展开」作用于所有当前已挂载的卡片。

import { reactive, computed } from 'vue'

/** localStorage 键：保存 `{ [cardId]: open }` 映射 */
const STORE_KEY = 'settingsCardsExpanded'

function load(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) || '{}')
  } catch {
    return {}
  }
}

// cardId -> open。缺失的键表示「使用默认值（展开）」。
const openState = reactive<Record<string, boolean>>(load())
// 当前已挂载（可见）的卡片 id —— 「全部折叠/展开」只作用于这些。
const mounted = reactive<Set<string>>(new Set())

function persist() {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(openState))
  } catch {
    /* 忽略配额/序列化错误 */
  }
}

/** 卡片是否展开（默认展开） */
function isOpen(id: string): boolean {
  return openState[id] !== false
}

function toggle(id: string) {
  openState[id] = !isOpen(id)
  persist()
}

/** 挂载时登记；若无持久化值且声明默认折叠，则补种 false。 */
function register(id: string, defaultOpen = true) {
  mounted.add(id)
  if (!(id in openState) && defaultOpen === false) openState[id] = false
}

function unregister(id: string) {
  mounted.delete(id)
}

/** 一键设置所有已挂载卡片的展开状态。 */
function setAll(open: boolean) {
  for (const id of mounted) openState[id] = open
  persist()
}

// 全部已展开 / 全部已折叠（仅统计已挂载的卡片，空集合时视为已展开）。
const allOpen = computed(() => [...mounted].every((id) => isOpen(id)))
const allClosed = computed(() => mounted.size > 0 && [...mounted].every((id) => !isOpen(id)))

export function useSettingsCards() {
  return { isOpen, toggle, register, unregister, setAll, allOpen, allClosed }
}
