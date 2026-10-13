// src/composables/player/useVideoResume.ts
// 播放进度记忆(播放器线 GE①):loadedmetadata 时按记忆位置复位，未知有效时长则等
// durationchange/progress 补齐后复位(边界规则见 resolveResumePosition
// 纯函数,独立可测);写入走 mediaStore.setPlaybackPosition(latest-write-wins 队列,GA 批已备)——
// 播放中 5s 节流 + pause/seek 落定即写(immediate) + 切条目/卸载 flush;ended(自然播完,非循环)写 0。
//
// 竞态:写通道本身按 itemId 经 mediaStore 的 latest-write-wins 队列串行化(边界 8「进度写按 id
// 队列」),故本 composable 不需要额外 token——只需在切条目时 flush 掉挂起的旧条目写、并复位
// `resumeDone` 让新条目的 loadedmetadata 重新判定一次。媒体事件读取当前 itemId,
// 所以新条目完成续播判定前禁止写盘,避免加载中的暂停把原有记忆位置覆盖成 0。

import { watch, onBeforeUnmount, type Ref } from 'vue'
import { useMediaStore } from '../../stores/mediaStore'

/** 播放中定时写盘的节流窗口(ms)。 */
const WRITE_THROTTLE_MS = 5000

/**
 * 复位判定(纯函数,供上层与 spec 独立验证)。savedMs 折算成秒后,满足下列任一条件即
 * **不复位**(返回 null):
 *  - `<5s`(几乎未看,复位无意义)
 *  - `≥duration`(异常记录,如竞态写入脏值)
 *  - `>98%×duration`(基本看完,复位到接近末尾无价值,直接从头看更自然)
 * `durationSec` 非有限(NaN/Infinity,直播流或未就绪)或非正时直接跳过——无法判定「看完」。
 */
export function resolveResumePosition(savedMs: number, durationSec: number): number | null {
  if (!Number.isFinite(durationSec) || durationSec <= 0) return null
  if (!Number.isFinite(savedMs) || savedMs <= 0) return null
  const savedSec = savedMs / 1000
  if (savedSec < 5) return null
  if (savedSec >= durationSec) return null
  if (savedSec > durationSec * 0.98) return null
  return savedSec
}

/**
 * 接线音视频元素的进度记忆：loadedmetadata 同步复位；非有限时长回退到 seekable 末端。
 * 无有效时长时等待 durationchange/progress 补齐。
 * 复位判定完成前不写进度；完成后通过 timeupdate/pause/seeked/ended 写盘。
 * @param videoEl 播放器内部 `<video>` 或 `<audio>` 元素(可能后挂载,watch immediate 装配)
 * @param itemId 当前库内媒体资产 id;未就绪为 null 时全部动作 no-op
 * @param playbackPositionMs 该条目上次退出时的记忆位置(ms;来自 detail.playbackPositionMs)
 */
export function useVideoResume(
  videoEl: Ref<HTMLMediaElement | null>,
  itemId: Ref<number | null>,
  playbackPositionMs: Ref<number>,
) {
  const mediaStore = useMediaStore()

  let lastWriteAt = 0
  // 待落盘的写入(切条目/卸载 flush 用);id 与写入时的 itemId 绑定,不依赖调用时的 itemId.value
  // (watch(itemId) 回调触发时 itemId.value 已是新条目)。
  let pendingId: number | null = null
  let pendingMs = 0
  // 本条目是否已用有效时长完成复位判定(媒体事件可能多次触发,只做一次)。
  let resumeDone = false
  let awaitingDuration = false

  function flush(): void {
    if (pendingId != null) {
      mediaStore.setPlaybackPosition(pendingId, pendingMs)
      pendingId = null
    }
  }

  function recordWrite(id: number, ms: number, immediate: boolean): void {
    pendingId = id
    pendingMs = ms
    const now = Date.now()
    if (immediate || now - lastWriteAt >= WRITE_THROTTLE_MS) {
      lastWriteAt = now
      flush()
    }
  }

  function onLoadedMetadata(): void {
    const el = videoEl.value
    const id = itemId.value
    if (!el || id == null || resumeDone) return
    // 与 useVideoPlayback 的有效时长口径一致，兼容 duration 恒为 Infinity 的本地媒体。
    let durationSec = el.duration
    if (!Number.isFinite(durationSec) && el.seekable.length > 0) {
      durationSec = el.seekable.end(el.seekable.length - 1)
    }
    // 元数据已到但时长尚未知时，不能把“暂不能判断”当作已恢复并覆盖原进度。
    if (!Number.isFinite(durationSec) || durationSec <= 0) {
      awaitingDuration = true
      return
    }
    awaitingDuration = false
    resumeDone = true
    const resumeSec = resolveResumePosition(playbackPositionMs.value, durationSec)
    if (resumeSec != null) el.currentTime = resumeSec
  }

  function onTimingAvailable(): void {
    if (awaitingDuration) onLoadedMetadata()
  }

  function onTimeUpdate(): void {
    const el = videoEl.value
    const id = itemId.value
    // 暂停中的 timeupdate(如 seek 后)不走节流写——由 onPauseOrSeeked 落定写接住。
    if (!el || id == null || !resumeDone || el.paused) return
    recordWrite(id, Math.round(el.currentTime * 1000), false)
  }

  function onPauseOrSeeked(): void {
    const el = videoEl.value
    const id = itemId.value
    if (!el || id == null || !resumeDone) return
    recordWrite(id, Math.round(el.currentTime * 1000), true)
  }

  function onEnded(): void {
    // 看完(非循环,循环态 loop=true 时浏览器不派发 ended)→ 写 0,下次重开从头看。
    const id = itemId.value
    if (id == null || !resumeDone) return
    recordWrite(id, 0, true)
  }

  function attach(el: HTMLMediaElement): void {
    el.addEventListener('loadedmetadata', onLoadedMetadata)
    el.addEventListener('durationchange', onTimingAvailable)
    el.addEventListener('progress', onTimingAvailable)
    el.addEventListener('timeupdate', onTimeUpdate)
    el.addEventListener('pause', onPauseOrSeeked)
    el.addEventListener('seeked', onPauseOrSeeked)
    el.addEventListener('ended', onEnded)
  }
  function detach(el: HTMLMediaElement): void {
    el.removeEventListener('loadedmetadata', onLoadedMetadata)
    el.removeEventListener('durationchange', onTimingAvailable)
    el.removeEventListener('progress', onTimingAvailable)
    el.removeEventListener('timeupdate', onTimeUpdate)
    el.removeEventListener('pause', onPauseOrSeeked)
    el.removeEventListener('seeked', onPauseOrSeeked)
    el.removeEventListener('ended', onEnded)
  }

  watch(
    videoEl,
    (el, prev) => {
      if (prev) detach(prev)
      if (el) attach(el)
    },
    { immediate: true },
  )

  // 切条目:先 flush 挂起的旧条目写,再复位 resumeDone 供新条目的 loadedmetadata 重新判定。
  watch(
    itemId,
    () => {
      flush()
      resumeDone = false
      awaitingDuration = false
      lastWriteAt = 0
    },
    // id 一变即封口,同一 tick 内的媒体事件也不能沿用上一条目的就绪状态。
    { flush: 'sync' },
  )

  onBeforeUnmount(() => {
    if (videoEl.value) detach(videoEl.value)
    flush()
  })
}
