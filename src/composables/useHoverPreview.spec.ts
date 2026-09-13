// useHoverPreview 悬停预览状态机 characterization(审查修复批 2026-07-18)。
// 锁住:判「重」双轴边界(恰好 4K 放行/超 4K scrub/10GB 兜底)、loadedmetadata 真实分辨率
// 补判(DB 占位 1280×720 漏判的运行时兜底)、sprite 负缓存 TTL(手动补产后 scrub 可恢复)、
// 防抖窗内 eligible 复查、播放态首移才预取 sprite、横移死区单向锁 scrub、@error 源缓存失效、
// 池容量 1 抢占、窗口失焦/页签隐藏收尾。
//
// 项目无 DOM 测试环境(全 node):window/document 用最小桩捕获模块级监听器,组件生命周期外以
// effectScope 承载(onBeforeUnmount/onDeactivated 为 no-op,Vue 告警已静音)。uiStore 实例化
// 摸 matchMedia/documentElement,与本测试无关 → 整模块 mock 成只暴露 hoverAutoplay 的桩。
// 诚实边界:模块级缓存(srcCache/spriteCache/runtimeHeavy)跨测例存续,各测例用独占 id 隔离。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { effectScope, ref, type EffectScope } from 'vue'

const hoisted = vi.hoisted(() => ({
  calls: [] as string[],
  sprite: null as string | null,
  autoplay: true,
}))

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}))
vi.mock('../utils/ipc', () => ({
  invokeIpc: async (cmd: string) => {
    hoisted.calls.push(cmd)
    if (cmd === 'get_keyframe_sprite') return hoisted.sprite
    if (cmd === 'get_media_detail') return { absPath: 'C:/vids/a.mp4' }
    if (cmd === 'get_companion_video_url') return 'C:/vids/live.mov'
    return null
  },
}))
vi.mock('../stores/uiStore', () => ({
  useUiStore: () => ({
    get hoverAutoplay() {
      return hoisted.autoplay
    },
    // 配置重构批次C:悬停延迟/「重」视频判据改由 uiStore 响应式下发——桩回生产默认值
    // (与批次C前 useHoverPreview.ts 自持的模块级常量同值),使本文件既有特征化测试的判据
    // 边界(恰好4K/超4K、10GB 兜底、200ms 防抖)保持不变。
    get heavyVideoMaxPixels() {
      return PIXELS_4K.w * PIXELS_4K.h
    },
    get heavyVideoMaxBytes() {
      return 10 * GB
    },
    get hoverDelayMs() {
      return 200
    },
    // 小批 C2:雪碧图切帧列数——桩回生产默认值 10,使既有 scrub 帧映射断言(cols=10)保持不变。
    get videoKeyframeCount() {
      return 10
    },
  }),
}))

// 模块级 blur/visibilitychange 监听在 import 时注册 → 先桩 window/document 捕获 handler,
// 再动态 import(静态 import 会被提升到桩之前而炸)。
const winHandlers: Record<string, Array<() => void>> = {}
const docHandlers: Record<string, Array<() => void>> = {}
const docState = { visibilityState: 'visible' as 'visible' | 'hidden' }
vi.stubGlobal('window', {
  addEventListener: (t: string, fn: () => void) => (winHandlers[t] ??= []).push(fn),
})
vi.stubGlobal('document', {
  addEventListener: (t: string, fn: () => void) => (docHandlers[t] ??= []).push(fn),
  get visibilityState() {
    return docState.visibilityState
  },
})

const { useHoverPreview } = await import('./useHoverPreview')

const PIXELS_4K = { w: 3840, h: 2160 }
const GB = 1024 * 1024 * 1024

let nextId = 1
/** 最小宿主:独占 id + 可变宿主态,effectScope 承载。 */
function makeHost(init?: {
  mediaType?: string
  isLivePhoto?: boolean
  fileSize?: number
  vw?: number
  vh?: number
}) {
  const id = nextId++
  const selectionMode = ref(false)
  const fileSize = ref(init?.fileSize ?? 0)
  const vw = ref(init?.vw ?? PIXELS_4K.w)
  const vh = ref(init?.vh ?? PIXELS_4K.h)
  const scope: EffectScope = effectScope()
  let api!: ReturnType<typeof useHoverPreview>
  scope.run(() => {
    api = useHoverPreview({
      id: () => id,
      mediaType: () => init?.mediaType ?? 'video',
      isLivePhoto: () => init?.isLivePhoto ?? false,
      fileSize: () => fileSize.value,
      videoWidth: () => vw.value,
      videoHeight: () => vh.value,
      compact: () => false,
      isSelectionMode: () => selectionMode.value,
    })
  })
  scopes.push(scope)
  return { id, api, selectionMode, fileSize, vw, vh }
}

/** 冲刷防抖计时器(200ms)+ 其回调内的 await 链。 */
async function enterAndSettle(h: ReturnType<typeof makeHost>) {
  h.api.onEnter()
  await vi.advanceTimersByTimeAsync(200)
  await flushMicro()
}
async function flushMicro() {
  for (let i = 0; i < 8; i++) await Promise.resolve()
}
function spriteCalls() {
  return hoisted.calls.filter((c) => c === 'get_keyframe_sprite').length
}

const scopes: EffectScope[] = []
beforeEach(() => {
  vi.useFakeTimers()
  hoisted.calls.length = 0
  hoisted.sprite = null
  hoisted.autoplay = true
  docState.visibilityState = 'visible'
  // 生命周期钩子在组件外的 Vue 告警静音(no-op 正是本套测试前提,见文件头注)。
  vi.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => {
  // 逐宿主收尾:onLeave 复位模块级 activeId/activeLeave,避免跨测例污染。
  while (scopes.length) scopes.pop()!.stop()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('useHoverPreview 判「重」双轴', () => {
  it('恰好 4K(3840×2160)放行播放;超 4K 且有 sprite 走 scrub-only', async () => {
    hoisted.sprite = 'C:/cache/sprites/s.webp'
    const exact4k = makeHost({ vw: 3840, vh: 2160 })
    await enterAndSettle(exact4k)
    expect(exact4k.api.isPreviewing.value).toBe(true)
    expect(exact4k.api.previewSrc.value).toBe('asset://C:/vids/a.mp4')
    exact4k.api.onLeave()

    const over4k = makeHost({ vw: 4096, vh: 2160 })
    await enterAndSettle(over4k)
    expect(over4k.api.isScrubbing.value).toBe(true)
    expect(over4k.api.isPreviewing.value).toBe(false)
    expect(over4k.api.spriteSrc.value).toBe('asset://C:/cache/sprites/s.webp')
  })

  it('fileSize 兜底:恰 10GB 播放,超 10GB scrub', async () => {
    hoisted.sprite = 'C:/cache/sprites/s.webp'
    const at10g = makeHost({ vw: 1920, vh: 1080, fileSize: 10 * GB })
    await enterAndSettle(at10g)
    expect(at10g.api.isPreviewing.value).toBe(true)
    at10g.api.onLeave()

    const over10g = makeHost({ vw: 1920, vh: 1080, fileSize: 10 * GB + 1 })
    await enterAndSettle(over10g)
    expect(over10g.api.isScrubbing.value).toBe(true)
  })

  it('超 4K 但无 sprite:回退直接播放(降级链的兜底)', async () => {
    hoisted.sprite = null
    const h = makeHost({ vw: 7680, vh: 4320 })
    await enterAndSettle(h)
    expect(h.api.isPreviewing.value).toBe(true)
  })
})

describe('loadedmetadata 真实分辨率补判(DB 占位漏判兜底)', () => {
  it('占位 1280×720 起播 → onPreviewMeta 报超 4K → 当场切 scrub;再次 hover 直进 scrub', async () => {
    hoisted.sprite = 'C:/cache/sprites/s.webp'
    const h = makeHost({ vw: 1280, vh: 720 }) // 扫描占位,静态判据放行
    await enterAndSettle(h)
    expect(h.api.isPreviewing.value).toBe(true)
    h.api.onPreviewMeta(7680, 4320) // <video> 实测:8K
    await flushMicro()
    expect(h.api.isScrubbing.value).toBe(true) // 当场降级止损
    h.api.onLeave()
    await enterAndSettle(h) // runtimeHeavy 记住 → 不再起播
    expect(h.api.isScrubbing.value).toBe(true)
    expect(h.api.isPreviewing.value).toBe(false)
  })

  it('onPreviewMeta 报 ≤4K:不降级继续播放', async () => {
    const h = makeHost({ vw: 1280, vh: 720 })
    await enterAndSettle(h)
    h.api.onPreviewMeta(3840, 2160)
    await flushMicro()
    expect(h.api.isPreviewing.value).toBe(true)
  })
})

describe('sprite 负缓存 TTL', () => {
  it('TTL 内不重查;过期后重查,补产的 sprite 让 scrub 恢复', async () => {
    hoisted.sprite = null
    const h = makeHost({ vw: 7680, vh: 4320 })
    await enterAndSettle(h) // 查 1 次 → 无 sprite → 回退播放
    expect(spriteCalls()).toBe(1)
    expect(h.api.isPreviewing.value).toBe(true)
    h.api.onLeave()

    await enterAndSettle(h) // TTL 内:不重复 IPC
    expect(spriteCalls()).toBe(1)
    h.api.onLeave()

    await vi.advanceTimersByTimeAsync(31_000) // 越过 30s TTL
    hoisted.sprite = 'C:/cache/sprites/late.webp' // 流水线补产完成
    await enterAndSettle(h)
    expect(spriteCalls()).toBe(2)
    expect(h.api.isScrubbing.value).toBe(true)
    expect(h.api.spriteSrc.value).toBe('asset://C:/cache/sprites/late.webp')
  })
})

describe('防抖窗内 eligible 复查', () => {
  it('200ms 计时器到点前进入选择模式:不激活、零 IPC', async () => {
    const h = makeHost()
    h.api.onEnter()
    h.selectionMode.value = true // 防抖窗内条件漂移
    await vi.advanceTimersByTimeAsync(200)
    await flushMicro()
    expect(h.api.isPreviewing.value).toBe(false)
    expect(hoisted.calls.length).toBe(0)
  })
})

describe('播放态横移:首移预取 + 死区单向锁 scrub', () => {
  it('静止悬停不预取 sprite;首移才预取;越死区后切 scrub 且帧随 x 映射、不回切', async () => {
    hoisted.sprite = 'C:/cache/sprites/s.webp'
    const h = makeHost({ vw: 1920, vh: 1080 })
    await enterAndSettle(h)
    expect(h.api.isPreviewing.value).toBe(true)
    expect(spriteCalls()).toBe(0) // 静止观看:不白付 sprite IPC

    h.api.onMove(100, 200) // 首移:记录基准 + 触发预取
    await flushMicro()
    expect(spriteCalls()).toBe(1)
    h.api.onMove(110, 200) // 位移 10px < 死区 24px:维持播放
    expect(h.api.isPreviewing.value).toBe(true)
    h.api.onMove(130, 200) // 位移 30px ≥ 死区:锁定 scrub
    expect(h.api.isScrubbing.value).toBe(true)
    expect(h.api.isPreviewing.value).toBe(false)
    // x=130/w=200 → 比例 0.65 → 帧 6(共 10 帧) → background-position (6/9)*100%
    expect(h.api.scrubStyle.value.backgroundPosition).toBe(`${(6 / 9) * 100}% 0%`)
    h.api.onMove(101, 200) // 移回死区内:不回切播放(单向锁)
    expect(h.api.isScrubbing.value).toBe(true)
  })
})

describe('源缓存错误失效 + 池容量 1 + 全局收尾', () => {
  it('@error → 结束预览并失效 srcCache:下次 hover 重新解析路径', async () => {
    const h = makeHost({ vw: 1920, vh: 1080 })
    await enterAndSettle(h)
    expect(hoisted.calls.filter((c) => c === 'get_media_detail').length).toBe(1)
    h.api.onPreviewError()
    expect(h.api.isPreviewing.value).toBe(false)
    await enterAndSettle(h)
    expect(hoisted.calls.filter((c) => c === 'get_media_detail').length).toBe(2) // 缓存已失效 → 重解析
    expect(h.api.isPreviewing.value).toBe(true)
  })

  it('B 格进入即抢占:A 格 isPreviewing 立即为假(共享池容量 1)', async () => {
    const a = makeHost({ vw: 1920, vh: 1080 })
    await enterAndSettle(a)
    expect(a.api.isPreviewing.value).toBe(true)
    const b = makeHost({ vw: 1920, vh: 1080 })
    await enterAndSettle(b)
    expect(b.api.isPreviewing.value).toBe(true)
    expect(a.api.isPreviewing.value).toBe(false)
  })

  it('窗口 blur / 页签隐藏:当前预览收尾(不留后台解码)', async () => {
    const h = makeHost({ vw: 1920, vh: 1080 })
    await enterAndSettle(h)
    expect(h.api.isPreviewing.value).toBe(true)
    winHandlers['blur']?.forEach((fn) => fn())
    expect(h.api.isPreviewing.value).toBe(false)

    await enterAndSettle(h)
    expect(h.api.isPreviewing.value).toBe(true)
    docState.visibilityState = 'hidden'
    docHandlers['visibilitychange']?.forEach((fn) => fn())
    expect(h.api.isPreviewing.value).toBe(false)
  })
})
