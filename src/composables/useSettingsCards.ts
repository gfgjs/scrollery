// src/composables/useSettingsCards.ts
// 设置页可折叠卡片的全局协调器（模块级单例）。
//
// 为什么用单例而非 provide/inject：CollapsibleCard 既直接用于 SettingsView，也嵌套在
// ModelLibrary / NetworkStorageSection 等子组件内。单例无需层层 provide 即可让「一键
// 全部折叠/展开」作用于所有当前已挂载的卡片。
//
// 存储（设置集中保存，批次B）：settings_cards_expanded 是「卡片 ID → 布尔」的内联表设置；
// 各卡默认展开与否由后端 schema 固化，故前端不再按 defaultOpen 自动补种写入（那会与 schema
// 默认值形成两份真源）。快照未到达前按调用方声明的 defaultOpen 呈现，不落盘。

import { computed, reactive } from 'vue'
import { readSetting, settingsReady, writeSettings } from '../stores/settingsPersistence'
import { parseSettingJson } from './settingsValues'

/** 保存展开映射的设置键 */
const CARD_KEY = 'settings_cards_expanded'

/** 读展开映射（规范 JSON 文本 → 表）；缺键或非法文本一律空表（回落各自缺省）。 */
function readOpenMap(): Record<string, boolean> {
  return parseSettingJson<Record<string, boolean>>(readSetting(CARD_KEY), {})
}

// 当前已挂载（可见）的卡片 id —— 「全部折叠/展开」只作用于这些。
const mounted = reactive<Set<string>>(new Set())
// 各卡声明的缺省展开：仅用于权威快照未到达时的呈现，不落盘。
const declaredDefault = reactive<Record<string, boolean>>({})

/** 卡片是否展开（快照缺该键时用调用方声明的缺省，默认展开） */
function isOpen(id: string): boolean {
  const stored = readOpenMap()[id]
  if (typeof stored === 'boolean') return stored
  return declaredDefault[id] ?? true
}

function commit(map: Record<string, boolean>) {
  // 权威快照未到达前读到的是空表,此刻提交会把其他卡片的展开态一并抹掉;故未就绪不提交。
  if (!settingsReady.value) return
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [CARD_KEY]: JSON.stringify(map) }).catch(() => {})
}

function toggle(id: string) {
  commit({ ...readOpenMap(), [id]: !isOpen(id) })
}

/** 搜索定位时只展开目标分组，保留其他分组的用户选择。 */
function expand(id: string) {
  if (!isOpen(id)) commit({ ...readOpenMap(), [id]: true })
}

/** 挂载时登记；缺省只记在本地用于首帧呈现。 */
function register(id: string, defaultOpen = true) {
  mounted.add(id)
  declaredDefault[id] = defaultOpen
}

function unregister(id: string) {
  mounted.delete(id)
  delete declaredDefault[id]
}

/** 一键设置所有已挂载卡片的展开状态。 */
function setAll(open: boolean) {
  const map = readOpenMap()
  for (const id of mounted) map[id] = open
  commit(map)
}

// 全部已展开 / 全部已折叠（仅统计已挂载的卡片，空集合时视为已展开）。
const allOpen = computed(() => [...mounted].every((id) => isOpen(id)))
const allClosed = computed(() => mounted.size > 0 && [...mounted].every((id) => !isOpen(id)))

export function useSettingsCards() {
  return { isOpen, toggle, expand, register, unregister, setAll, allOpen, allClosed }
}
