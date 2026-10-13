// 仅 UI 的状态类型

/**
 * 外观模式：亮/暗/跟随系统。实际主题色板由 themeStore 按明暗槽位决定。
 */
export type AppearanceMode = 'dark' | 'light' | 'system'

export type SortBy = 'sort_datetime' | 'file_name' | 'file_size' | 'created_at'
export type SortOrder = 'asc' | 'desc'

/** toast 内的交互式快捷 chip（如「加入收藏夹」）。 */
export interface ToastAction {
  label: string
  onClick: () => void | Promise<void>
}

export interface ToastMessage {
  id: string
  type: 'success' | 'error' | 'warning' | 'info'
  message: string
  duration: number
  /** 可选动作 chips（如加入收藏夹）。 */
  actions?: ToastAction[]
}

export type SmartAlbum = 'all' | 'favorites' | 'trash' | 'live-photos' | 'recent'
