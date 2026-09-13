// useThumbLoader defer 状态机 characterization(审查回补 F2)。
// 锁住 B(快滚甩滚低保真)接入后的加载状态机:闸门抑制启动 → deferredByGate 挂标 →
// 放行 watch 恰好补起一次;以及既有的换代守卫(decodingImg)与 404 自愈(regenerateThumb)。
//
// 项目无 DOM 测试环境(全 node,不引 jsdom):window / Image 用最小桩;组件生命周期外以
// effectScope 承载 watch。onMounted 在组件外为 no-op(Vue 会告警,已静音),故一切
// loadThumb 触发都经「[thumbPath,thumbStatus] watch」与「闸门放行 watch」驱动——这正是
// 本测试要锁的两条命门链。诚实边界:onBeforeUnmount 的 cancelThumb 语义绑定组件实例,
// 组件外测不到,不在此覆盖(⏸ 组件级/真机)。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { ref, nextTick, effectScope, type EffectScope } from 'vue'
import { setDeferThumbLoad } from './useThumbLoadGate'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}))

// useThumbLoader 模块顶层读 window.location.search(?clear= 强刷串)→ node 下须先桩
// window 再动态 import(静态 import 会被提升到桩之前而炸)。
vi.stubGlobal('window', { location: { search: '' } })

/** 可控 Image 桩:每个实例的 decode 行为由 nextBehaviors 队列注入(默认 ok)。
 *  'error404' 模拟真·加载失败:先触发 onerror(loader 以此区分 404 与偶发解码拒绝)再 reject。
 *  'pending' 模拟慢解码:decode 挂起,由测例手动 resolveDecode(换代守卫用)。 */
class FakeImage {
  static instances: FakeImage[] = []
  static nextBehaviors: Array<'ok' | 'error404' | 'pending'> = []
  onerror: (() => void) | null = null
  src = ''
  resolveDecode!: () => void
  private rejectDecode!: (e: unknown) => void
  private behavior: 'ok' | 'error404' | 'pending'
  private readonly decodePromise: Promise<void>
  constructor() {
    this.behavior = FakeImage.nextBehaviors.shift() ?? 'ok'
    this.decodePromise = new Promise<void>((res, rej) => {
      this.resolveDecode = res
      this.rejectDecode = rej
    })
    FakeImage.instances.push(this)
  }
  decode(): Promise<void> {
    if (this.behavior === 'ok') this.resolveDecode()
    else if (this.behavior === 'error404') {
      this.onerror?.()
      this.rejectDecode(new Error('404'))
    }
    return this.decodePromise
  }
}
vi.stubGlobal('Image', FakeImage)

const { useThumbLoader, buildThumbUrl } = await import('./useThumbLoader')

/** 最小宿主:响应式 src getter(驱动 status/path watch)+ emit 记录 + effectScope 承载。 */
function makeHost() {
  const status = ref(0)
  const path = ref<string | null>(null)
  const emitted = { request: [] as number[], cancel: [] as number[], regen: [] as number[] }
  const scope: EffectScope = effectScope()
  let api!: ReturnType<typeof useThumbLoader>
  scope.run(() => {
    api = useThumbLoader(
      {
        id: () => 42,
        thumbStatus: () => status.value,
        thumbPath: () => path.value,
        cacheDir: () => 'C:/cache',
      },
      {
        requestThumb: (id) => emitted.request.push(id),
        cancelThumb: (id) => emitted.cancel.push(id),
        regenerateThumb: (id) => emitted.regen.push(id),
      },
    )
  })
  scopes.push(scope)
  return { status, path, emitted, scope, api }
}

/** 冲刷 watch(pre flush 微任务)+ loadThumb 内部 await 链。 */
async function flush() {
  await nextTick()
  await new Promise((r) => setTimeout(r, 0))
}

const scopes: EffectScope[] = []
beforeEach(() => {
  setDeferThumbLoad(false)
  FakeImage.instances = []
  FakeImage.nextBehaviors = []
  // onMounted 在组件外的 Vue 告警静音(no-op 行为正是本套测试的前提,见文件头注)。
  vi.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => {
  while (scopes.length) scopes.pop()!.stop()
  vi.restoreAllMocks()
})

describe('useThumbLoader × 加载闸门(defer 状态机)', () => {
  it('browser harness 的 data URL 不经过 Tauri asset protocol', () => {
    const dataUrl = 'data:image/svg+xml,%3Csvg%2F%3E'
    expect(buildThumbUrl(3, dataUrl, '')).toBe(dataUrl)
  })

  it('飞掠中 status 0→1:不启动解码;放行恰好补起一次并出图', async () => {
    const h = makeHost()
    setDeferThumbLoad(true)
    h.path.value = 'ab/x.webp'
    h.status.value = 1
    await flush()
    expect(FakeImage.instances.length).toBe(0) // 闸门拦下,零 Image 启动
    setDeferThumbLoad(false)
    await flush()
    expect(FakeImage.instances.length).toBe(1) // 放行只补一次
    expect(FakeImage.instances[0].src).toBe('asset://C:/cache/thumbnails/ab/x.webp')
    expect(h.api.isLoaded.value).toBe(true)
    expect(h.api.displaySrc.value).toBe('asset://C:/cache/thumbnails/ab/x.webp')
  })

  it('无待载的卡:闸门翻转不误触发加载(deferredByGate 未挂标)', async () => {
    const h = makeHost()
    setDeferThumbLoad(true)
    await flush()
    setDeferThumbLoad(false)
    await flush()
    expect(FakeImage.instances.length).toBe(0)
    expect(h.emitted.request).toEqual([])
  })

  it('飞掠中掠过的 status 3 无路径卡不入队;放行后恰请求一次且不重复(hasRequested 守卫)', async () => {
    const h = makeHost()
    setDeferThumbLoad(true)
    h.status.value = 3 // watch(newStatus===3)→ loadThumb → 闸门拦下挂标
    await flush()
    expect(h.emitted.request).toEqual([]) // 飞掠中不入队(设计意图:掠过项不触发生成)
    setDeferThumbLoad(false)
    await flush()
    expect(h.emitted.request).toEqual([42]) // 放行补起 → 恰一次解析请求
    setDeferThumbLoad(true)
    await flush()
    setDeferThumbLoad(false)
    await flush()
    expect(h.emitted.request).toEqual([42]) // 再翻转不重复(deferredByGate 已复位)
  })

  it('defer 期间数据迁移(3→1)不丢补起:放行加载的是最新 status/path', async () => {
    const h = makeHost()
    setDeferThumbLoad(true)
    h.status.value = 3
    await flush()
    h.path.value = 'ab/y.webp'
    h.status.value = 1 // 仍飞掠:再次被拦,deferredByGate 保持
    await flush()
    expect(FakeImage.instances.length).toBe(0)
    setDeferThumbLoad(false)
    await flush()
    expect(FakeImage.instances.length).toBe(1)
    expect(h.api.displaySrc.value).toContain('ab/y.webp')
  })

  it('已出图的卡在飞掠中数据迁移 → 立即重载(闸门只拦未出图,isLoaded 豁免)', async () => {
    const h = makeHost()
    h.path.value = 'ab/x.webp'
    h.status.value = 1
    await flush()
    expect(h.api.isLoaded.value).toBe(true)
    setDeferThumbLoad(true)
    h.path.value = 'ab/x2.webp' // 飞掠中路径迁移
    await flush()
    expect(FakeImage.instances.length).toBe(2) // 不被闸门拦(已出图者刷新不延迟)
    expect(h.api.displaySrc.value).toContain('ab/x2.webp')
  })

  it('404 → 先 onerror 后 decode reject:恰一次自愈 regenerateThumb,不出图,守卫防重复', async () => {
    const h = makeHost()
    FakeImage.nextBehaviors = ['error404']
    h.path.value = 'ab/z.webp'
    h.status.value = 1
    await flush()
    expect(h.emitted.regen).toEqual([42])
    expect(h.api.isLoaded.value).toBe(false)
    expect(h.api.displaySrc.value).toBe('')
    FakeImage.nextBehaviors = ['error404']
    h.path.value = 'ab/z2.webp' // 再迁一次路径 → 再失败
    await flush()
    expect(FakeImage.instances.length).toBe(2)
    expect(h.emitted.regen).toEqual([42]) // hasRequestedHeal 守卫:每挂载至多自愈一次
  })

  it('decodingImg 换代守卫:慢解码被新加载超越,旧结果迟到不回滚', async () => {
    const h = makeHost()
    FakeImage.nextBehaviors = ['pending', 'ok']
    h.path.value = 'ab/a1.webp'
    h.status.value = 1
    await flush() // img1 decode 挂起
    h.path.value = 'ab/a2.webp'
    await flush() // img2 立即完成 → displaySrc = a2
    expect(h.api.displaySrc.value).toContain('ab/a2.webp')
    FakeImage.instances[0].resolveDecode() // 旧 img1 迟到完成
    await flush()
    expect(h.api.displaySrc.value).toContain('ab/a2.webp') // 不被回滚到 a1
  })

  it('scope 销毁后闸门翻转不再触发(watch 随作用域清理,无泄漏)', async () => {
    const h = makeHost()
    setDeferThumbLoad(true)
    h.status.value = 3
    await flush()
    h.scope.stop()
    setDeferThumbLoad(false)
    await flush()
    expect(h.emitted.request).toEqual([]) // 已销毁:放行不补起、不入队
    expect(FakeImage.instances.length).toBe(0)
  })
})
