// src/composables/player/usePlayerPrefs.ts
// 全局播放偏好(音量/静音/倍速/循环)持久化到 localStorage 单键 `player_prefs`。
// 逐条目播放进度不在此(归 DB / GA 批 setPlaybackPosition)——这里只存「跨视频通用」的会话偏好,
// 镜像 detail_show_faces 等既有 localStorage 偏好惯例。读写全包 try/catch:隐私模式 / 配额满时
// 静默降级为内存态,绝不因存储不可用而中断播放器。

import { ref, watch } from 'vue'
import { clampRate } from './useVideoPlayback'

const STORAGE_KEY = 'player_prefs'

/** 持久化的播放偏好。volume ∈ [0,1]、rate ∈ [0.25,3]。 */
export interface PlayerPrefs {
  volume: number
  muted: boolean
  rate: number
  loop: boolean
}

const DEFAULTS: PlayerPrefs = { volume: 1, muted: false, rate: 1, loop: false }

function clamp01(n: number): number {
  return Math.min(1, Math.max(0, n))
}

/** 从 localStorage 读取并逐字段校验(损坏/越界值回落默认),任何异常 → 全默认。 */
function load(): PlayerPrefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULTS }
    const parsed = JSON.parse(raw) as Partial<PlayerPrefs>
    return {
      volume: typeof parsed.volume === 'number' ? clamp01(parsed.volume) : DEFAULTS.volume,
      muted: typeof parsed.muted === 'boolean' ? parsed.muted : DEFAULTS.muted,
      rate: typeof parsed.rate === 'number' ? clampRate(parsed.rate) : DEFAULTS.rate,
      loop: typeof parsed.loop === 'boolean' ? parsed.loop : DEFAULTS.loop,
    }
  } catch {
    return { ...DEFAULTS }
  }
}

function persist(prefs: PlayerPrefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs))
  } catch {
    // 隐私模式 / 配额满:静默降级,本会话内存态仍有效。
  }
}

/**
 * 返回响应式偏好 ref;任何字段变更即写盘(深监听)。上层在 loadedmetadata 后据此把偏好施加到
 * <video>,并在用户交互改动镜像态时回写 prefs。
 */
export function usePlayerPrefs() {
  const prefs = ref<PlayerPrefs>(load())
  watch(prefs, (v) => persist(v), { deep: true })
  return { prefs }
}
