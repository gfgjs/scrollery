import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import * as Vue from 'vue'
import { createRenderer, nextTick, reactive, ssrContextKey, type App, type RenderFunction } from 'vue'
import { compile } from '@vue/compiler-dom'
import { compileScript, parse } from '@vue/compiler-sfc'
import ts from 'typescript'
import { createPinia } from 'pinia'
import { useRoute } from 'vue-router'
import AudioPlayer from './AudioPlayer.vue'
import { IPC } from '../constants/ipc'
import { useViewerStore } from '../stores/viewerStore'
import VideoRateMenu from '../components/media/player/VideoRateMenu.vue'
import UiIconButton from '../components/ui/UiIconButton.vue'
import { PLAYBACK_RATES } from '../composables/player/useVideoPlayback'

const { invokeIpc, setPlaybackPosition } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  setPlaybackPosition: vi.fn(),
}))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => ({ setPlaybackPosition }) }))
vi.mock('vue-router', () => ({
  useRoute: vi.fn(),
  useRouter: () => ({ options: { history: { state: {} } }, push: vi.fn() }),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => `asset:${path}` }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('../utils/format', () => ({ formatDuration: (ms: number) => String(ms / 1000) }))
vi.mock('../commands/context', () => ({ buildCommandContext: vi.fn() }))
vi.mock('../commands/keybinding', () => ({ dispatchKeybinding: vi.fn() }))
const settings = reactive<Record<string, string>>({})
const writeSettings = vi.fn<(patch: Record<string, string>) => Promise<void>>()
vi.mock('../stores/settingsPersistence', () => ({
  readSetting: (key: string) => settings[key],
  writeSettings: (patch: Record<string, string>) => writeSettings(patch),
}))
// 弹层仅替换定位外壳，保留真实倍速菜单的展开、档位按钮和选择事件。
vi.mock('../components/ui/UiPopover.vue', () => ({
  default: Vue.defineComponent({
    props: { open: Boolean, anchor: Object, placement: String },
    setup: (props, { slots }) => () => props.open ? Vue.h('div', slots.default?.()) : null,
  }),
}))
vi.mock('@lucide/vue', () => {
  const icon = { render: () => null }
  return {
    ChevronLeft: icon, ExternalLink: icon, Music: icon, Play: icon,
    Pause: icon, SkipBack: icon, SkipForward: icon, Volume2: icon,
  }
})

// 保留真实模板的媒体事件和控件绑定，只替换宿主节点及音频解码器。
class TestNode extends EventTarget {
  props: Record<string, unknown> = {}
  children: TestNode[] = []
  parent: TestNode | null = null
  text = ''
  currentTime = 0
  duration = Number.NaN
  paused = true
  volume = 1
  muted = false
  playbackRate = 1
  defaultPlaybackRate = 1
  rateAtPlay: number[] = []
  error: { code: number } | null = null
  constructor(readonly tag: string) { super() }
  emit(prop: string) {
    const handler = this.props[prop]
    if (typeof handler === 'function') handler(new Event(prop))
    this.dispatchEvent(new Event(prop.slice(2).toLowerCase()))
  }
  play = vi.fn(async () => {
    this.rateAtPlay.push(this.playbackRate)
    this.paused = false
    this.emit('onPlay')
  })
  pause = vi.fn(() => {
    this.paused = true
    this.emit('onPause')
  })
  load = vi.fn(() => {
    // 重载清除媒体错误并归零播放位置；其后写 currentTime 可指定待加载资源的起播位置。
    this.error = null
    this.currentTime = 0
    this.duration = Number.NaN
    this.paused = true
    this.playbackRate = this.defaultPlaybackRate
  })
  scrollIntoView() {}
}

const nodes: TestNode[] = []
// Vitest 导入的是 SSR 组件；为自定义宿主编译同一份客户端模板。
function clientRender(path: string): RenderFunction {
  const { descriptor } = parse(readFileSync(new URL(path, import.meta.url), 'utf8'))
  const { code } = compile(descriptor.template!.content, {
    prefixIdentifiers: true,
    bindingMetadata: compileScript(descriptor, { id: 'audio-player-test' }).bindings,
    expressionPlugins: ['typescript'],
  })
  return new Function('Vue', ts.transpile(code))(Vue) as RenderFunction
}
const render = clientRender('./AudioPlayer.vue')
VideoRateMenu.render = clientRender('../components/media/player/VideoRateMenu.vue')
UiIconButton.render = clientRender('../components/ui/UiIconButton.vue')
const renderer = createRenderer<TestNode, TestNode>({
  createElement(tag) {
    const node = new TestNode(tag)
    nodes.push(node)
    return node
  },
  createText: (text) => Object.assign(new TestNode('#text'), { text }),
  createComment: () => new TestNode('#comment'),
  setText: (node, text) => { node.text = text },
  setElementText: (node, text) => { node.text = text },
  patchProp: (node, key, _previous, value) => {
    node.props[key] = value
    // 媒体换源会按 defaultPlaybackRate 重设即时倍速。
    if (node.tag === 'audio' && key === 'src') node.playbackRate = node.defaultPlaybackRate
  },
  insert(node, parent, anchor) {
    node.parent = parent
    const index = anchor ? parent.children.indexOf(anchor) : -1
    if (index < 0) parent.children.push(node)
    else parent.children.splice(index, 0, node)
  },
  remove(node) {
    const index = node.parent?.children.indexOf(node) ?? -1
    if (index >= 0) node.parent!.children.splice(index, 1)
    node.parent = null
  },
  parentNode: (node) => node.parent,
  nextSibling: (node) => {
    const siblings = node.parent?.children ?? []
    return siblings[siblings.indexOf(node) + 1] ?? null
  },
})

function audioDetail(id: number) {
  return { id, playbackPositionMs: 0, fileName: `${id}.mp3`, fileFormat: 'mp3', absPath: `/${id}.mp3`, meta: {}, lyricsSynced: false }
}

async function settle() {
  await new Promise<void>((resolve) => setImmediate(resolve))
  await nextTick()
}

let app: App | null = null
const route = reactive({ params: { id: '1' } })

beforeEach(() => {
  vi.clearAllMocks()
  Object.assign(settings, {
    player_volume: '0.4', player_muted: 'false', player_rate: '1', player_loop: 'false',
  })
  writeSettings.mockImplementation(async (patch) => { Object.assign(settings, patch) })
  nodes.length = 0
  route.params.id = '1'
  vi.stubGlobal('document', new EventTarget())
  vi.mocked(useRoute).mockReturnValue(route as unknown as ReturnType<typeof useRoute>)
  invokeIpc.mockImplementation(async (command, args) => {
    if (command === IPC.GET_AUDIO_DETAIL) return audioDetail(Number(args?.id))
    return { rating: 0, isFavorited: false, colorLabel: 0, fileSize: 100, width: 0, height: 0 }
  })
})

async function rateOption(rate: number) {
  const trigger = [...nodes].reverse().find((node) => node.tag === 'button' && node.props.title === 'player.playbackRate')
  expect(trigger, '音频模板应包含现有倍速菜单').toBeDefined()
  trigger!.emit('onClick')
  await nextTick()
  const options = nodes.filter((node) => node.parent && node.tag === 'button' && String(node.props.class).includes('video-rate__option'))
  expect(options.map((node) => node.text.trim())).toEqual(PLAYBACK_RATES.map((value) => `${value}×`))
  return options.find((node) => node.text.trim() === `${rate}×`)!
}

function rateLabel() {
  return [...nodes].reverse().find((node) => node.props.class === 'video-rate__label')?.text.trim()
}

describe('音频进度记忆', () => {
  it('播放中出错后点击重试保留当前进度，元数据重载不退回旧续播位置', async () => {
    settings.player_rate = '1.5'
    invokeIpc.mockResolvedValueOnce({}).mockResolvedValueOnce({ ...audioDetail(1), playbackPositionMs: 32000 })
    const { audio, seekbar } = await mountPlayer()
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    expect(audio.currentTime).toBe(32)
    audio.currentTime = 71
    audio.emit('onTimeupdate')
    audio.error = { code: 2 }
    audio.emit('onError')
    await nextTick()
    expect(nodes.find((node) => node.parent && node.props.role === 'alert')?.text).toBe('audio.playFailed')

    const playButton = nodes.find((node) => node.parent && node.props.class === 'ap-icon ap-icon--play')!
    playButton.emit('onClick')
    expect(audio.load).toHaveBeenCalledOnce()
    expect(audio.currentTime).toBe(71)
    expect(audio.play).toHaveBeenCalledTimes(2)
    expect(audio.rateAtPlay).toEqual([1.5, 1.5])
    expect(audio.volume).toBe(0.4)

    audio.duration = 120
    audio.emit('onLoadedmetadata')
    audio.emit('onTimeupdate')
    audio.emit('onSeeked')
    await settle()
    expect(seekbar().props.value).toBe(71)
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(1, 71000)
    expect(nodes.find((node) => node.parent && node.props.role === 'alert')).toBeUndefined()
  })

  it('长音频元数据就绪后恢复保存位置，暂停时持久化当前进度', async () => {
    invokeIpc.mockResolvedValueOnce({}).mockResolvedValueOnce({ ...audioDetail(1), playbackPositionMs: 32000 })
    const { audio } = await mountPlayer()
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    expect(audio.currentTime).toBe(32)
    expect(setPlaybackPosition).not.toHaveBeenCalled()
    audio.currentTime = 41
    audio.pause()
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(1, 41000)
  })

  it('换歌加载期间的旧暂停事件不覆盖新歌记忆位置', async () => {
    const { audio } = await mountPlayer()
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    audio.currentTime = 24
    audio.emit('onTimeupdate')
    let finish!: (value: unknown) => void
    invokeIpc.mockImplementation((command, args) => command === IPC.GET_AUDIO_DETAIL
      ? new Promise((resolve) => { finish = resolve })
      : Promise.resolve({ id: args?.id }))
    route.params.id = '2'
    await nextTick()
    setPlaybackPosition.mockClear()
    audio.pause()
    expect(setPlaybackPosition).not.toHaveBeenCalled()
    finish({ ...audioDetail(2), playbackPositionMs: 60000 })
    await settle()
    audio.duration = 180
    audio.emit('onLoadedmetadata')
    expect(audio.currentTime).toBe(60)
    audio.currentTime = 65
    audio.emit('onSeeked')
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(2, 65000)
  })

  it('音频自然播放结束后清零记忆位置', async () => {
    const { audio } = await mountPlayer()
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    audio.currentTime = 120
    audio.emit('onEnded')
    expect(setPlaybackPosition).toHaveBeenLastCalledWith(1, 0)
  })
})

describe('音频倍速偏好与模板操作', () => {
  it('首次自动播放前应用已保存倍速且不反向保存', async () => {
    settings.player_rate = '1.5'
    const { audio } = await mountPlayer()
    expect(audio.playbackRate).toBe(1.5)
    expect(audio.defaultPlaybackRate).toBe(1.5)
    expect(audio.rateAtPlay).toEqual([1.5])
    expect(rateLabel()).toBe('1.5×')
    expect(writeSettings).not.toHaveBeenCalled()
  })

  it('模板选择倍速立即作用于音频并只保存 player_rate，换歌继续使用该倍速', async () => {
    const { audio } = await mountPlayer()
    const option = await rateOption(2)
    option.emit('onClick')
    expect(audio.playbackRate).toBe(2)
    expect(audio.defaultPlaybackRate).toBe(2)
    expect(writeSettings).toHaveBeenCalledExactlyOnceWith({ player_rate: '2' })
    await nextTick()
    expect(rateLabel()).toBe('2×')
    expect(audio.volume).toBe(0.4)
    expect(audio.muted).toBe(false)

    route.params.id = '2'
    await settle()
    expect(audio.props.src).toBe('asset:/2.mp3')
    expect(audio.playbackRate).toBe(2)
    expect(audio.rateAtPlay).toEqual([1, 2])
    expect(rateLabel()).toBe('2×')
    expect(writeSettings).toHaveBeenCalledTimes(1)
  })

  it('保存失败的设置回滚会恢复音频倍速及菜单显示，不重新提交', async () => {
    settings.player_rate = '1.5'
    const { audio } = await mountPlayer()
    let rejectSave!: (error: Error) => void
    writeSettings.mockImplementationOnce((patch) => {
      Object.assign(settings, patch)
      return new Promise<void>((_resolve, reject) => { rejectSave = reject }).catch((error: unknown) => {
        settings.player_rate = '1.5'
        throw error
      })
    })
    const option = await rateOption(2)
    option.emit('onClick')
    expect(audio.playbackRate).toBe(2)
    await nextTick()
    rejectSave(new Error('模拟保存失败'))
    await settle()
    expect(audio.playbackRate).toBe(1.5)
    expect(audio.defaultPlaybackRate).toBe(1.5)
    expect(rateLabel()).toBe('1.5×')
    expect(writeSettings).toHaveBeenCalledExactlyOnceWith({ player_rate: '2' })
  })

  it('恢复默认设置同步音量、静音和倍速，不写回设置', async () => {
    settings.player_rate = '2'
    settings.player_muted = 'true'
    const { audio } = await mountPlayer()
    expect(audio.playbackRate).toBe(2)
    expect(audio.muted).toBe(true)
    Object.assign(settings, { player_rate: '1', player_volume: '1', player_muted: 'false' })
    await nextTick()
    expect(audio.playbackRate).toBe(1)
    expect(audio.defaultPlaybackRate).toBe(1)
    expect(audio.volume).toBe(1)
    expect(audio.muted).toBe(false)
    expect(rateLabel()).toBe('1×')
    expect(writeSettings).not.toHaveBeenCalled()
  })
})

afterEach(() => {
  app?.unmount()
  app = null
  vi.unstubAllGlobals()
})

async function mountPlayer() {
  const pinia = createPinia()
  app = renderer.createApp({ ...AudioPlayer, render })
  app.use(pinia)
  app.provide(ssrContextKey, {})
  app.mount(new TestNode('root'))
  await settle()
  const audio = nodes.find((node) => node.tag === 'audio')!
  const seekbar = () => [...nodes].reverse().find((node) => node.tag === 'input' && node.props.step === '0.1')!
  return { audio, seekbar, viewer: useViewerStore(pinia) }
}

describe('音频实际时长更新', () => {
  it('元数据后的时长增长会更新进度条，快进能越过原来的终点', async () => {
    const { audio, seekbar, viewer } = await mountPlayer()
    audio.duration = 60
    audio.emit('onLoadedmetadata')
    await nextTick()
    expect(seekbar().props.max).toBe(60)

    audio.duration = 120
    audio.emit('onDurationchange')
    audio.currentTime = 55
    viewer.activeViewer?.api?.seekBy?.(10)
    expect(audio.currentTime).toBe(65)
    await nextTick()
    expect(seekbar().props.max).toBe(120)
  })

  it('时长缩短后进度条和快进使用修正后的终点', async () => {
    const { audio, seekbar, viewer } = await mountPlayer()
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    audio.duration = 60
    audio.emit('onDurationchange')
    audio.currentTime = 55
    viewer.activeViewer?.api?.seekBy?.(10)
    expect(audio.currentTime).toBe(60)
    await nextTick()
    expect(seekbar().props.max).toBe(60)
  })

  it('正常自动播放使用保存音量，换歌重新读取时长且播放暂停继续可用', async () => {
    const { audio, seekbar, viewer } = await mountPlayer()
    expect(audio.props.src).toBe('asset:/1.mp3')
    expect(audio.play).toHaveBeenCalledOnce()
    expect(audio.volume).toBe(0.4)
    audio.duration = 120
    audio.emit('onLoadedmetadata')
    audio.currentTime = 55
    audio.emit('onTimeupdate')
    await nextTick()
    expect(seekbar().props.value).toBe(55)

    route.params.id = '2'
    await settle()
    expect(audio.props.src).toBe('asset:/2.mp3')
    expect(seekbar().props.max).toBe(0)
    expect(seekbar().props.value).toBe(0)
    audio.currentTime = 0
    audio.duration = 30
    audio.emit('onLoadedmetadata')
    await nextTick()
    expect(seekbar().props.max).toBe(30)
    expect(audio.play).toHaveBeenCalledTimes(2)
    viewer.activeViewer?.api?.togglePlay?.()
    expect(audio.paused).toBe(true)
    viewer.activeViewer?.api?.togglePlay?.()
    await settle()
    expect(audio.paused).toBe(false)
  })
})
