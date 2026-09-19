// src/composables/player/usePlayerPrefs.ts
// 全局播放偏好(音量/静音/倍速/循环)存中央设置(config.toml 的 player_volume / player_muted /
// player_rate / player_loop 四键)。
// 逐条目播放进度不在此(归 DB / GA 批 setPlaybackPosition)——这里只存「跨视频通用」的偏好。
//
// 读写分离(设置集中保存,批次B):后端正典值 → 本地显示态(ref),用户改动 → setPrefs 显式提交。
// 这样后端快照(启动水合/恢复默认/外部编辑文件)只施加到显示态、不回写设置,避免「应用即保存」的
// 反馈环;拖动音量/倍速属连续操作,提交时走中央防抖合并。

import { ref, watch } from 'vue'
import { readSetting, writeSettings } from '../../stores/settingsPersistence'
import { readSettingBool, readSettingNumber } from '../settingsValues'
import { clampRate } from './useVideoPlayback'

/** 持久化的播放偏好。volume ∈ [0,1]、rate ∈ [0.25,3]。 */
export interface PlayerPrefs {
  volume: number
  muted: boolean
  rate: number
  loop: boolean
}

/** 四个标量键:独立修改、可批量提交。 */
const KEYS = {
  volume: 'player_volume',
  muted: 'player_muted',
  rate: 'player_rate',
  loop: 'player_loop',
} as const

const DEFAULTS: PlayerPrefs = { volume: 1, muted: false, rate: 1, loop: false }

function clamp01(n: number): number {
  return Math.min(1, Math.max(0, n))
}

/** 从中央设置读四键并逐字段守卫(越界/异常文本回落默认)。 */
function readPrefs(): PlayerPrefs {
  return {
    volume: clamp01(readSettingNumber(KEYS.volume, DEFAULTS.volume)),
    muted: readSettingBool(KEYS.muted, DEFAULTS.muted),
    rate: clampRate(readSettingNumber(KEYS.rate, DEFAULTS.rate)),
    loop: readSettingBool(KEYS.loop, DEFAULTS.loop),
  }
}

/**
 * 返回显示态 ref 与显式提交入口。
 * 上层在 loadedmetadata 后据此把偏好施加到 <video>,并在用户交互改动镜像态时调用 setPrefs 提交。
 */
export function usePlayerPrefs() {
  const prefs = ref<PlayerPrefs>(readPrefs())

  // 后端只应用:所跟踪的任一键变化(启动水合 / 恢复默认 / 外部编辑)即整份替换显示态。
  watch(
    () => [
      readSetting(KEYS.volume),
      readSetting(KEYS.muted),
      readSetting(KEYS.rate),
      readSetting(KEYS.loop),
    ],
    () => {
      prefs.value = readPrefs()
    },
  )

  /**
   * 提交用户改动:立即更新显示态,只把本次涉及的键写回设置。
   * @param options.debounce 连续操作(拖动音量/倍速)传 true,交由中央集合合并提交。
   */
  function setPrefs(patch: Partial<PlayerPrefs>, options?: { debounce?: boolean }) {
    prefs.value = { ...prefs.value, ...patch }
    const values: Record<string, string> = {}
    if (patch.volume !== undefined) values[KEYS.volume] = String(clamp01(patch.volume))
    if (patch.muted !== undefined) values[KEYS.muted] = String(patch.muted)
    if (patch.rate !== undefined) values[KEYS.rate] = String(clampRate(patch.rate))
    if (patch.loop !== undefined) values[KEYS.loop] = String(patch.loop)
    if (Object.keys(values).length === 0) return
    // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
    writeSettings(values, options).catch(() => {})
  }

  return { prefs, setPrefs }
}
