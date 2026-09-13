// src/composables/player/useVideoPlayback.ts
// 视频播放核心:把 <video> 元素的命令式媒体事件模型收敛为一组响应式镜像 + 操作方法。
// 唯一职责=DOM ↔ 响应式状态的双向桥接;不持有偏好持久化(usePlayerPrefs)、不管全屏(useVideoFullscreen)、
// 不做字幕/截帧(GE 批)。单一数据流:方法写 DOM → media 事件回读 → 镜像 ref 更新 → 视图渲染。

import { onScopeDispose, ref, watch, type Ref } from 'vue'

/** 倍速档位(升序);UI 菜单与 rateStep 步进共用单一事实源。 */
export const PLAYBACK_RATES = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3] as const

export const RATE_MIN = 0.25
export const RATE_MAX = 3

/** 倍速钳制到合法区间 [0.25, 3]。 */
export function clampRate(rate: number): number {
  if (!Number.isFinite(rate)) return 1
  return Math.min(RATE_MAX, Math.max(RATE_MIN, rate))
}

/**
 * 响应式播放状态镜像 + 命令方法。传入的 videoRef 可后挂载(watch immediate 装配),
 * 组件卸载时自动摘监听。
 */
export function useVideoPlayback(videoRef: Ref<HTMLVideoElement | null>) {
  // ── 状态镜像(全部由 media 事件回读驱动,方法只写 DOM)──────────────────────
  const currentTime = ref(0)
  /** 时长;未就绪/NaN/Infinity(直播流)时上层据此禁 seek。 */
  const duration = ref(Number.NaN)
  /** 覆盖 currentTime 的已缓冲区末端(秒),供 seekbar 画缓冲层。 */
  const bufferedEnd = ref(0)
  const paused = ref(true)
  const ended = ref(false)
  /** 缓冲等待中(waiting) 或 seek 落定前(seeking):上层显 spinner。 */
  const waiting = ref(false)
  const seeking = ref(false)
  const rate = ref(1)
  const volume = ref(1)
  const muted = ref(false)
  const loop = ref(false)
  const pipActive = ref(false)

  // ── 时长计算:el.duration 非有限(直播流/部分容器元数据缺失)时,退而取
  // seekable range 末端作为有效时长(边界 2 恢复:seekable 非空即可 seek/显示进度)。
  // el.duration=Infinity 但 seekable 有段是常见于分块传输的本地大文件场景。
  function computeDuration(): number {
    const el = videoRef.value
    if (!el) return Number.NaN
    const d = el.duration
    if (Number.isFinite(d)) return d
    const sk = el.seekable
    if (sk.length > 0) return sk.end(sk.length - 1)
    return d
  }

  // ── 缓冲区计算:取覆盖当前播放头的连续缓冲段末端;无覆盖段则取最大末端 ──────────
  function computeBuffered(): void {
    const el = videoRef.value
    if (!el) return
    const b = el.buffered
    let end = 0
    for (let i = 0; i < b.length; i++) {
      if (b.start(i) <= el.currentTime && el.currentTime <= b.end(i)) {
        end = b.end(i)
        break
      }
      end = Math.max(end, b.end(i))
    }
    bufferedEnd.value = end
  }

  // ── 事件 → 镜像 ────────────────────────────────────────────────────────────
  function onTimeUpdate(): void {
    const el = videoRef.value
    if (!el) return
    currentTime.value = el.currentTime
    computeBuffered()
  }
  function onDurationChange(): void {
    duration.value = computeDuration()
  }
  function onLoadedMetadata(): void {
    const el = videoRef.value
    if (!el) return
    duration.value = computeDuration()
    currentTime.value = el.currentTime
  }
  function onProgress(): void {
    computeBuffered()
    // progress 事件伴随 seekable range 增长(边界 2):重算一次有效时长,
    // 覆盖 el.duration 恒 Infinity 但 seekable 逐步扩展的场景。
    duration.value = computeDuration()
  }
  function onPlay(): void {
    paused.value = false
    ended.value = false
  }
  function onPause(): void {
    paused.value = true
  }
  function onEnded(): void {
    ended.value = true
    paused.value = true
  }
  function onWaiting(): void {
    waiting.value = true
  }
  function onSeeking(): void {
    seeking.value = true
  }
  function onSeeked(): void {
    seeking.value = false
    waiting.value = false
    computeBuffered()
  }
  function onPlaying(): void {
    waiting.value = false
    paused.value = false
    ended.value = false
  }
  // 换源/损坏/不支持格式并不保证会走到 seeked 或 playing。若只在那两个事件里清状态，
  // 快速翻过坏视频时上一条的 waiting/seeking 会被复用的播放器带到下一条，spinner 因而常驻。
  function clearTransientLoadingState(): void {
    waiting.value = false
    seeking.value = false
  }
  function onRateChange(): void {
    rate.value = videoRef.value?.playbackRate ?? 1
  }
  function onVolumeChange(): void {
    const el = videoRef.value
    if (!el) return
    volume.value = el.volume
    muted.value = el.muted
  }
  function onEnterPip(): void {
    pipActive.value = true
  }
  function onLeavePip(): void {
    pipActive.value = false
  }

  // 监听表:统一装配/摘除,避免逐条遗漏。
  const listeners: Array<[keyof HTMLMediaElementEventMap | string, EventListener]> = [
    ['timeupdate', onTimeUpdate as EventListener],
    ['durationchange', onDurationChange as EventListener],
    ['loadedmetadata', onLoadedMetadata as EventListener],
    ['progress', onProgress as EventListener],
    ['play', onPlay as EventListener],
    ['pause', onPause as EventListener],
    ['ended', onEnded as EventListener],
    ['loadstart', clearTransientLoadingState as EventListener],
    ['waiting', onWaiting as EventListener],
    ['seeking', onSeeking as EventListener],
    ['seeked', onSeeked as EventListener],
    ['playing', onPlaying as EventListener],
    ['abort', clearTransientLoadingState as EventListener],
    ['emptied', clearTransientLoadingState as EventListener],
    ['error', clearTransientLoadingState as EventListener],
    ['ratechange', onRateChange as EventListener],
    ['volumechange', onVolumeChange as EventListener],
    ['enterpictureinpicture', onEnterPip as EventListener],
    ['leavepictureinpicture', onLeavePip as EventListener],
  ]

  function attach(el: HTMLVideoElement): void {
    for (const [name, fn] of listeners) el.addEventListener(name, fn)
    // 元素可能已带初值(autoplay 已开始):立即同步一次镜像。
    paused.value = el.paused
    rate.value = el.playbackRate
    volume.value = el.volume
    muted.value = el.muted
    loop.value = el.loop
    duration.value = computeDuration()
    currentTime.value = el.currentTime
    computeBuffered()
  }
  function detach(el: HTMLVideoElement): void {
    for (const [name, fn] of listeners) el.removeEventListener(name, fn)
  }

  watch(
    videoRef,
    (el, prev) => {
      if (prev) detach(prev)
      if (el) attach(el)
    },
    { immediate: true },
  )

  onScopeDispose(() => {
    if (videoRef.value) detach(videoRef.value)
  })

  // ── 命令方法(只写 DOM;事件回读更新镜像)────────────────────────────────────
  function play(): void {
    const el = videoRef.value
    if (!el) return
    // play() 返回 Promise:autoplay 策略拦截时 reject,此时不会触发 'play' 事件,
    // 故显式把镜像置为 paused,让上层显大播放键覆层(边界 5)。
    const p = el.play()
    if (p && typeof p.then === 'function') {
      p.catch(() => {
        paused.value = true
      })
    }
  }
  function pause(): void {
    videoRef.value?.pause()
  }
  function playPause(): void {
    if (!videoRef.value) return
    if (videoRef.value.paused) play()
    else pause()
  }
  /** 绝对跳转,钳制到 [0, 有效时长](边界 2:el.duration 非有限时退而用 seekable 末端)。 */
  function seekTo(seconds: number): void {
    const el = videoRef.value
    if (!el) return
    const d = computeDuration()
    const max = Number.isFinite(d) ? d : seconds
    el.currentTime = Math.min(Math.max(0, seconds), max)
  }
  /** 相对 seek ±秒(边界 3:clamp)。 */
  function seekBy(seconds: number): void {
    const el = videoRef.value
    if (!el) return
    seekTo(el.currentTime + seconds)
  }
  function setVolume(v: number): void {
    const el = videoRef.value
    if (!el) return
    const clamped = Math.min(1, Math.max(0, v))
    el.volume = clamped
    // 拉动音量即隐含解除静音(除非音量归零)。
    if (clamped > 0 && el.muted) el.muted = false
  }
  /** 相对调节音量(键盘 ↑/↓ ±5%)。 */
  function volumeBy(delta: number): void {
    const el = videoRef.value
    if (!el) return
    setVolume(el.volume + delta)
  }
  function toggleMute(): void {
    const el = videoRef.value
    if (!el) return
    el.muted = !el.muted
  }
  function setRate(r: number): void {
    const el = videoRef.value
    if (!el) return
    el.playbackRate = clampRate(r)
  }
  /** 倍速按档步进(< / >);dir=+1 提速、-1 降速。 */
  function rateStep(dir: 1 | -1): void {
    const el = videoRef.value
    if (!el) return
    const cur = el.playbackRate
    // 取最接近当前值的档位索引,再步进——避免自由倍速时步进落到错档。
    let idx = 0
    let best = Number.POSITIVE_INFINITY
    for (let i = 0; i < PLAYBACK_RATES.length; i++) {
      const diff = Math.abs(PLAYBACK_RATES[i] - cur)
      if (diff < best) {
        best = diff
        idx = i
      }
    }
    const next = Math.min(PLAYBACK_RATES.length - 1, Math.max(0, idx + dir))
    el.playbackRate = PLAYBACK_RATES[next]
  }
  function toggleLoop(): void {
    const el = videoRef.value
    if (!el) return
    // loop 无对应 media 事件,直接同步镜像。
    el.loop = !el.loop
    loop.value = el.loop
  }
  function setLoop(on: boolean): void {
    const el = videoRef.value
    if (!el) return
    el.loop = on
    loop.value = on
  }
  /** PiP 切换;pictureInPictureEnabled===false 时上层已隐按钮,此处兜底忽略失败。 */
  async function togglePip(): Promise<void> {
    const el = videoRef.value
    if (!el) return
    try {
      if (document.pictureInPictureElement === el) {
        await document.exitPictureInPicture()
      } else if (document.pictureInPictureEnabled) {
        await el.requestPictureInPicture()
      }
    } catch {
      // 用户手势缺失/格式不支持等:静默忽略,不打断播放。
    }
  }
  /** 切条目/卸载前退出 PiP(边界 7),避免浮窗残留指向已卸载的视频。 */
  async function exitPipIfActive(): Promise<void> {
    try {
      if (videoRef.value && document.pictureInPictureElement === videoRef.value) {
        await document.exitPictureInPicture()
      }
    } catch {
      /* 忽略 */
    }
  }

  return {
    // 状态
    currentTime,
    duration,
    bufferedEnd,
    paused,
    ended,
    waiting,
    seeking,
    rate,
    volume,
    muted,
    loop,
    pipActive,
    // 方法
    play,
    pause,
    playPause,
    seekTo,
    seekBy,
    setVolume,
    volumeBy,
    toggleMute,
    setRate,
    rateStep,
    toggleLoop,
    setLoop,
    togglePip,
    exitPipIfActive,
  }
}

export type VideoPlayback = ReturnType<typeof useVideoPlayback>
