// 视频换源时，浏览器会以 abort / emptied / loadstart 收尾上一条请求，不保证触发
// playing 或 seeked。这里钉住：这些终止事件与 error 都必须清掉上一条遗留的加载态，
// 否则快速翻过坏视频后，spinner 会错误地留在下一条视频上。
import { afterEach, describe, expect, it } from 'vitest'
import { effectScope, ref, type EffectScope } from 'vue'
import { useVideoPlayback, type VideoPlayback } from './useVideoPlayback'

function emptyTimeRanges(): TimeRanges {
  return {
    length: 0,
    start: () => 0,
    end: () => 0,
  }
}

class FakeVideoElement extends EventTarget {
  currentTime = 0
  duration = 60
  buffered = emptyTimeRanges()
  seekable = emptyTimeRanges()
  paused = false
  playbackRate = 1
  volume = 1
  muted = false
  loop = false

  play(): Promise<void> {
    this.paused = false
    return Promise.resolve()
  }

  pause(): void {
    this.paused = true
  }
}

interface PlaybackHost {
  element: FakeVideoElement
  playback: VideoPlayback
}

const scopes: EffectScope[] = []

function makeHost(): PlaybackHost {
  const element = new FakeVideoElement()
  const videoRef = ref<HTMLVideoElement | null>(element as unknown as HTMLVideoElement)
  const scope = effectScope()
  let playback!: VideoPlayback
  scope.run(() => {
    playback = useVideoPlayback(videoRef)
  })
  scopes.push(scope)
  return { element, playback }
}

function beginLoading(host: PlaybackHost): void {
  host.element.dispatchEvent(new Event('waiting'))
  host.element.dispatchEvent(new Event('seeking'))
  expect(host.playback.waiting.value).toBe(true)
  expect(host.playback.seeking.value).toBe(true)
}

afterEach(() => {
  while (scopes.length) scopes.pop()!.stop()
})

describe('useVideoPlayback', () => {
  it.each(['abort', 'emptied', 'loadstart'] as const)(
    '换源触发 %s 时清理上一条遗留的加载态',
    (eventName) => {
      const host = makeHost()
      beginLoading(host)

      host.element.dispatchEvent(new Event(eventName))

      expect(host.playback.waiting.value).toBe(false)
      expect(host.playback.seeking.value).toBe(false)
    },
  )

  it('解码/格式错误时清理加载态，交给上层错误面板收尾', () => {
    const host = makeHost()
    beginLoading(host)

    host.element.dispatchEvent(new Event('error'))

    expect(host.playback.waiting.value).toBe(false)
    expect(host.playback.seeking.value).toBe(false)
  })
})
