import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, ref, shallowRef, type App } from 'vue'
import { useVideoSubtitles } from './useVideoSubtitles'

const { invokeIpc, addToast, logError } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
  addToast: vi.fn(),
  logError: vi.fn(),
}))

vi.mock('../../utils/ipc', () => ({ invokeIpc, IpcError: class extends Error {} }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('../../utils/logger', () => ({ logger: { error: logError } }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}),
  createText: () => ({}),
  createComment: () => ({}),
  setText() {},
  setElementText() {},
  patchProp() {},
  insert() {},
  remove() {},
  parentNode: () => null,
  nextSibling: () => null,
})

interface TestTrack {
  mode: TextTrackMode
  label: string
  language: string
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

function subtitle(fileName: string) {
  return { fileName, content: 'WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n字幕\n' }
}

let app: App | null = null

function setup() {
  const embedded: TestTrack = { mode: 'disabled', label: '内嵌字幕', language: 'zh' }
  const events = new EventTarget()
  const tracks = Object.assign([embedded], {
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
  })
  const createTrack = vi.fn(() => {
    const track: TestTrack = { mode: 'disabled', label: '', language: '' }
    return {
      kind: '', label: '', src: '', track,
      remove() {
        const index = tracks.indexOf(track)
        if (index >= 0) tracks.splice(index, 1)
      },
    }
  })
  vi.stubGlobal('document', { createElement: createTrack })
  const video = {
    textTracks: tracks,
    appendChild(element: ReturnType<typeof createTrack>) {
      tracks.push(element.track)
    },
  }
  const itemId = ref<number | null>(1)
  let subtitles!: ReturnType<typeof useVideoSubtitles>
  app = renderer.createApp({
    setup() {
      subtitles = useVideoSubtitles(shallowRef(video as unknown as HTMLVideoElement), itemId)
      return () => null
    },
  })
  app.mount({})
  return { subtitles, itemId, tracks, embedded, createTrack }
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:subtitle')
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
})

afterEach(() => {
  app?.unmount()
  app = null
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('字幕加载与用户选择的异步隔离', () => {
  it('自动探测期间关闭字幕，迟到的字幕仍可选但不能自动启用', async () => {
    const pending = deferred<ReturnType<typeof subtitle>>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const { subtitles, tracks } = setup()
    const loading = subtitles.autoDetectForItem()
    subtitles.turnOff()
    pending.resolve(subtitle('同名.vtt'))
    await loading

    expect(subtitles.externalLabel.value).toBe('同名.vtt')
    expect(subtitles.activeKey.value).toBe('off')
    expect(tracks.map((track) => track.mode)).toEqual(['disabled', 'disabled'])
    subtitles.selectTrack('external')
    expect(tracks[1].mode).toBe('showing')
  })

  it('手动加载期间选择内嵌轨，迟到结果保留内嵌轨的实际显示状态', async () => {
    const pending = deferred<ReturnType<typeof subtitle>>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const { subtitles, tracks } = setup()
    const loading = subtitles.loadFromPath('selected.vtt')
    subtitles.selectTrack('embedded-0')
    pending.resolve(subtitle('selected.vtt'))
    await loading

    expect(subtitles.externalLabel.value).toBe('selected.vtt')
    expect(subtitles.activeKey.value).toBe('embedded-0')
    expect(tracks.map((track) => track.mode)).toEqual(['showing', 'disabled'])
  })

  it('没有后续选择时自动启用加载结果，再次手动加载会替换旧外挂轨并自动启用', async () => {
    invokeIpc.mockResolvedValueOnce(subtitle('同名.vtt'))
    const { subtitles, tracks } = setup()
    await subtitles.autoDetectForItem()
    expect(subtitles.activeKey.value).toBe('external')
    expect(tracks.map((track) => track.mode)).toEqual(['disabled', 'showing'])

    subtitles.turnOff()
    invokeIpc.mockResolvedValueOnce(subtitle('手选.vtt'))
    await subtitles.loadFromPath('手选.vtt')
    expect(subtitles.externalLabel.value).toBe('手选.vtt')
    expect(subtitles.activeKey.value).toBe('external')
    expect(tracks.map((track) => track.mode)).toEqual(['disabled', 'showing'])
    expect(URL.revokeObjectURL).toHaveBeenCalledOnce()
  })

  it('切换资源再返回后，旧请求成功不能替换新一轮已加载的字幕', async () => {
    const old = deferred<ReturnType<typeof subtitle>>()
    invokeIpc.mockReturnValueOnce(old.promise)
    const { subtitles, itemId, tracks, createTrack } = setup()
    const loading = subtitles.autoDetectForItem()
    itemId.value = 2
    itemId.value = 1
    invokeIpc.mockResolvedValueOnce(subtitle('新一轮.vtt'))
    await subtitles.autoDetectForItem()
    old.resolve(subtitle('旧响应.vtt'))
    await loading

    expect(subtitles.externalLabel.value).toBe('新一轮.vtt')
    expect(subtitles.activeKey.value).toBe('external')
    expect(tracks.map((track) => track.mode)).toEqual(['disabled', 'showing'])
    expect(createTrack).toHaveBeenCalledOnce()
  })

  it('切换资源后的旧请求失败不会向新资源报告字幕错误', async () => {
    const old = deferred<ReturnType<typeof subtitle>>()
    invokeIpc.mockReturnValueOnce(old.promise)
    const { subtitles, itemId, tracks } = setup()
    const loading = subtitles.autoDetectForItem()
    itemId.value = 2
    invokeIpc.mockResolvedValueOnce(subtitle('新资源.vtt'))
    await subtitles.autoDetectForItem()
    old.reject(new Error('旧资源读取失败'))
    await loading

    expect(subtitles.externalLabel.value).toBe('新资源.vtt')
    expect(tracks.map((track) => track.mode)).toEqual(['disabled', 'showing'])
    expect(addToast).not.toHaveBeenCalled()
    expect(logError).not.toHaveBeenCalled()
  })
})
