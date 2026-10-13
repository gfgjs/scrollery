import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, ref, shallowRef, type App } from 'vue'
import { useVideoResume } from './useVideoResume'

const { setPlaybackPosition } = vi.hoisted(() => ({ setPlaybackPosition: vi.fn() }))
vi.mock('../../stores/mediaStore', () => ({ useMediaStore: () => ({ setPlaybackPosition }) }))

function seekableUntil(end: number): TimeRanges {
  return { length: end > 0 ? 1 : 0, start: () => 0, end: () => end }
}

class MediaElement extends EventTarget {
  currentTime = 0
  duration = 120
  seekable = seekableUntil(0)
  paused = false

  emit(name: string) {
    this.dispatchEvent(new Event(name))
  }
}

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

let app: App | null = null
let now = 10_000
beforeEach(() => {
  setPlaybackPosition.mockReset()
  now = 10_000
  vi.spyOn(Date, 'now').mockImplementation(() => now)
})
afterEach(() => {
  app?.unmount()
  app = null
  vi.restoreAllMocks()
})

function setup(savedMs = 60_000) {
  const media = new MediaElement()
  const itemId = ref<number | null>(1)
  const position = ref(savedMs)
  app = renderer.createApp({
    setup() {
      useVideoResume(shallowRef(media as unknown as HTMLMediaElement), itemId, position)
      return () => null
    },
  })
  app.mount({})
  return { media, itemId, position }
}

describe('媒体续播等待有效时长', () => {
  it('duration 始终为 Infinity 时以可跳转范围恢复位置，并正常记忆及结束清零', () => {
    const { media } = setup()
    media.duration = Number.POSITIVE_INFINITY
    media.seekable = seekableUntil(120)
    media.emit('loadedmetadata')
    expect(media.currentTime).toBe(60)

    media.currentTime = 70
    media.emit('timeupdate')
    expect(setPlaybackPosition).toHaveBeenCalledExactlyOnceWith(1, 70_000)
    media.currentTime = 120
    media.paused = true
    media.emit('pause')
    media.emit('ended')
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(1, 0)
    expect(media.duration).toBe(Number.POSITIVE_INFINITY)
  })

  it('duration 不变化但 progress 带来可跳转范围时，解除等待且不会重复恢复', () => {
    const { media } = setup()
    media.duration = Number.POSITIVE_INFINITY
    media.emit('loadedmetadata')
    media.currentTime = 1
    media.emit('timeupdate')
    expect(setPlaybackPosition).not.toHaveBeenCalled()

    media.seekable = seekableUntil(120)
    media.emit('progress')
    expect(media.currentTime).toBe(60)
    media.currentTime = 70
    media.seekable = seekableUntil(130)
    media.emit('progress')
    expect(media.currentTime).toBe(70)
    media.paused = true
    media.emit('pause')
    expect(setPlaybackPosition).toHaveBeenCalledExactlyOnceWith(1, 70_000)
  })

  it('初始时长未知时保留原记录，时长就绪后恢复且只恢复一次', () => {
    const { media } = setup()
    media.duration = Number.POSITIVE_INFINITY
    media.emit('loadedmetadata')
    media.currentTime = 1
    media.emit('timeupdate')
    media.paused = true
    media.emit('pause')
    expect(setPlaybackPosition).not.toHaveBeenCalled()

    media.duration = 120
    media.emit('durationchange')
    expect(media.currentTime).toBe(60)
    media.emit('seeked')
    expect(setPlaybackPosition).toHaveBeenCalledExactlyOnceWith(1, 60_000)
    media.currentTime = 70
    media.duration = 130
    media.emit('durationchange')
    expect(media.currentTime).toBe(70)
  })

  it('等待时长期间切项，旧项待写位置与新项的恢复位置各归原资产', () => {
    const { media, itemId, position } = setup(20_000)
    media.emit('loadedmetadata')
    media.currentTime = 21
    media.emit('timeupdate')
    now += 100
    media.currentTime = 22
    media.emit('timeupdate')
    expect(setPlaybackPosition).toHaveBeenCalledExactlyOnceWith(1, 21_000)

    itemId.value = 2
    position.value = 45_000
    media.duration = Number.NaN
    media.currentTime = 0
    media.emit('pause')
    media.emit('loadedmetadata')
    media.currentTime = 1
    media.emit('timeupdate')
    expect(setPlaybackPosition.mock.calls).toEqual([[1, 21_000], [1, 22_000]])

    media.duration = 100
    media.emit('durationchange')
    expect(media.currentTime).toBe(45)
    media.emit('seeked')
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(2, 45_000)
  })

  it('有效元数据仍同步恢复，重复元数据不覆盖用户后续 seek，暂停立即记忆', () => {
    const { media } = setup()
    media.emit('durationchange')
    media.emit('progress')
    expect(media.currentTime).toBe(0)
    media.emit('loadedmetadata')
    expect(media.currentTime).toBe(60)
    media.currentTime = 75
    media.emit('loadedmetadata')
    expect(media.currentTime).toBe(75)
    media.paused = true
    media.emit('pause')
    expect(setPlaybackPosition).toHaveBeenCalledExactlyOnceWith(1, 75_000)
  })

  it('自然结束清零且卸载不恢复旧待写进度，同时移除时长监听', () => {
    const { media } = setup()
    media.emit('loadedmetadata')
    media.currentTime = 110
    media.emit('timeupdate')
    now += 100
    media.currentTime = 119
    media.emit('timeupdate')
    media.currentTime = 120
    media.paused = true
    media.emit('pause')
    media.emit('ended')
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(1, 0)
    const writes = setPlaybackPosition.mock.calls.length
    app!.unmount()
    app = null
    media.currentTime = 0
    media.emit('loadedmetadata')
    media.emit('durationchange')
    media.emit('seeked')
    expect(setPlaybackPosition).toHaveBeenCalledTimes(writes)
    expect(media.currentTime).toBe(0)
  })
})
