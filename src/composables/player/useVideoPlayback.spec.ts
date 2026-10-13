import { afterEach, describe, expect, it, vi } from 'vitest'
import { effectScope, shallowRef, type EffectScope } from 'vue'
import { useVideoPlayback } from './useVideoPlayback'

function ranges(entries: Array<[number, number]>): TimeRanges {
  return {
    length: entries.length,
    start: (index) => entries[index][0],
    end: (index) => entries[index][1],
  }
}

// 用原生事件分发驱动真实 composable；不读取或写入用户媒体。
class VideoElement extends EventTarget {
  currentTime = 30
  duration = 120
  buffered = ranges([[0, 90]])
  seekable = ranges([[0, 120]])
  paused = false
  ended = false
  playbackRate = 1.5
  defaultPlaybackRate = 1.5
  volume = 0.4
  muted = false
  loop = false
  play = vi.fn(async () => {
    this.paused = false
    this.emit('play')
    this.emit('playing')
  })
  pause = vi.fn(() => {
    this.paused = true
    this.emit('pause')
  })

  emit(name: string) {
    this.dispatchEvent(new Event(name))
  }

  empty() {
    this.currentTime = 0
    this.duration = Number.NaN
    this.buffered = ranges([])
    this.seekable = ranges([])
    this.paused = true
    this.ended = false
    // 换源清空元素不依赖 pause 或 durationchange，先派发 emptied。
    this.emit('emptied')
  }
}

let scope: EffectScope | null = null
afterEach(() => {
  scope?.stop()
  scope = null
})

function setup() {
  const video = new VideoElement()
  const videoRef = shallowRef(video as unknown as HTMLVideoElement)
  scope = effectScope()
  const playback = scope.run(() => useVideoPlayback(videoRef))!
  return { video, playback }
}

describe('视频换源后的播放状态', () => {
  it('播放中清空源立即撤销旧进度、缓冲和播放态，新源元数据到达后重新同步', () => {
    const { video, playback } = setup()
    video.emit('waiting')
    video.emit('seeking')
    expect(playback.paused.value).toBe(false)
    expect(playback.duration.value).toBe(120)
    expect(playback.bufferedEnd.value).toBe(90)

    video.empty()
    expect(playback.paused.value).toBe(true)
    expect(playback.currentTime.value).toBe(0)
    expect(playback.duration.value).toBeNaN()
    expect(playback.bufferedEnd.value).toBe(0)
    expect(playback.waiting.value).toBe(false)
    expect(playback.seeking.value).toBe(false)
    expect(playback.rate.value).toBe(1.5)
    expect(playback.volume.value).toBe(0.4)

    video.duration = 60
    video.seekable = ranges([[0, 60]])
    video.emit('loadedmetadata')
    expect(playback.duration.value).toBe(60)
    expect(playback.paused.value).toBe(true)
    video.buffered = ranges([[0, 10]])
    video.emit('progress')
    expect(playback.bufferedEnd.value).toBe(10)
  })

  it('上一段播放结束后切源，不把已结束状态带给新源', () => {
    const { video, playback } = setup()
    video.ended = true
    video.paused = true
    video.emit('ended')
    expect(playback.ended.value).toBe(true)

    video.empty()
    expect(playback.ended.value).toBe(false)
    expect(playback.paused.value).toBe(true)
  })

  it('正常播放暂停及 seek 保留事件回读和首尾边界', async () => {
    const { video, playback } = setup()
    playback.pause()
    expect(video.paused).toBe(true)
    expect(playback.paused.value).toBe(true)
    playback.playPause()
    await Promise.resolve()
    expect(video.play).toHaveBeenCalledOnce()
    expect(playback.paused.value).toBe(false)

    playback.seekBy(5)
    expect(video.currentTime).toBe(35)
    video.emit('seeking')
    video.emit('timeupdate')
    video.emit('seeked')
    expect(playback.currentTime.value).toBe(35)
    expect(playback.seeking.value).toBe(false)
    playback.seekTo(-5)
    expect(video.currentTime).toBe(0)
    playback.seekTo(200)
    expect(video.currentTime).toBe(120)
  })

  it('旧源播放请求迟到拒绝，不覆盖新源已经开始播放的状态', async () => {
    const { video, playback } = setup()
    let rejectPlay!: (error: Error) => void
    video.play.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => {
      rejectPlay = reject
    }))
    playback.play()
    video.empty()
    video.duration = 60
    video.emit('loadedmetadata')
    video.paused = false
    video.emit('playing')
    rejectPlay(new Error('旧源播放被换源中断'))
    await Promise.resolve()
    expect(playback.paused.value).toBe(false)
    expect(playback.duration.value).toBe(60)
    expect(playback.waiting.value).toBe(false)
  })
})
