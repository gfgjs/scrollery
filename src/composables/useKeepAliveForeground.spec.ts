// useKeepAliveForeground contract 测试(2026-09-12 冷启动选择态 ESC 失效治本)。
//
// 范式:项目无 @vue/test-utils 也无 DOM 测试环境,故用 vue 运行时的 createRenderer 挂一棵假宿主
// 节点树,再以**生产拓扑**驱动真实 KeepAlive + defineAsyncComponent——KeepAlive include 命中的是
// 路由壳 GalleryRouteLayer,网格是壳内经 defineAsyncComponent 延迟加载的子组件(见 App.vue 的
// RouterView 与 GalleryRouteLayer.vue)。
//
// 断言口径与 MediaGrid 的接线同构:enter = 挂 document 级 keydown 监听 + 打开底栏轴控件门控,
// leave = 摘监听 + 关掉门控,onReactivate = 滚动位/几何恢复。监听侧用与 DOM EventTarget 相同的
// 去重语义(同类型同函数重复注册是空操作、按函数身份摘除)建模,这样「监听是否泄漏」直接被观察,
// 不必依赖调用次数记账。
import { describe, expect, it } from 'vitest'
import {
  KeepAlive,
  createRenderer,
  defineAsyncComponent,
  defineComponent,
  h,
  nextTick,
  onActivated,
  ref,
  type Component,
} from 'vue'
import { useKeepAliveForeground } from './useKeepAliveForeground'

// ── 假宿主节点树 ───────────────────────────────────────────────────────────
// insert 必须先把节点从旧父节点摘掉(真 DOM 的 appendChild 即如此),否则 KeepAlive 失活
// 「移入缓存容器」后新旧两处同时可见,摘除/销毁路径会失真。
interface FakeNode {
  tag: string
  parent: FakeNode | null
  children: FakeNode[]
}

function fakeNode(tag: string): FakeNode {
  return { tag, parent: null, children: [] }
}

const nodeOps = {
  patchProp() {},
  insert(el: FakeNode, parent: FakeNode, anchor?: FakeNode | null) {
    if (el.parent) {
      const oldIndex = el.parent.children.indexOf(el)
      if (oldIndex >= 0) el.parent.children.splice(oldIndex, 1)
    }
    const index = anchor ? parent.children.indexOf(anchor) : -1
    if (index >= 0) parent.children.splice(index, 0, el)
    else parent.children.push(el)
    el.parent = parent
  },
  remove(el: FakeNode) {
    if (!el.parent) return
    const index = el.parent.children.indexOf(el)
    if (index >= 0) el.parent.children.splice(index, 1)
    el.parent = null
  },
  createElement: (tag: string) => fakeNode(tag),
  createText: () => fakeNode('#text'),
  createComment: () => fakeNode('#comment'),
  setText() {},
  setElementText() {},
  parentNode: (node: FakeNode) => node.parent,
  nextSibling(node: FakeNode) {
    if (!node.parent) return null
    return node.parent.children[node.parent.children.indexOf(node) + 1] ?? null
  },
  querySelector: () => null,
  setScopeId() {},
}

const { createApp } = createRenderer<FakeNode, FakeNode>(nodeOps)

// ── 假 document(EventTarget 去重语义)────────────────────────────────────────
class FakeEventTarget {
  private readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>()

  addEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    const set = this.listeners.get(type) ?? new Set()
    set.add(listener)
    this.listeners.set(type, set)
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    this.listeners.get(type)?.delete(listener)
  }

  listenerCount(type: string): number {
    return this.listeners.get(type)?.size ?? 0
  }

  dispatch(type: string): void {
    for (const listener of [...(this.listeners.get(type) ?? [])]) {
      if (typeof listener === 'function') listener({ type } as Event)
    }
  }
}

interface Harness {
  /** 生产接线里的 document 级 keydown 监听(选择态 ESC / 数字键评分 / Ctrl+A 的入口)。 */
  docKeydownListeners: () => number
  /** 派发一次 document keydown,用于验证监听确实活着。 */
  pressEscape: () => void
  /** 底栏轴控件门控(galleryViewActive)。 */
  axisGate: () => boolean
  /** 复激活恢复次数(几何/焦点/滚动位)。 */
  restores: () => number
  /** 组件外触发路径(MediaGrid 的 contentViewerRoute watcher)共用的两个入口。 */
  api: () => ReturnType<typeof useKeepAliveForeground>
  setForeground: (value: boolean) => void
  show: (view: 'layer' | 'other') => Promise<void>
  unmount: () => void
}

/**
 * 以生产拓扑挂载一个按 MediaGrid 方式接线 useKeepAliveForeground 的组件。
 *
 * @param asyncSubtree true = 网格经 defineAsyncComponent 落在路由壳内(生产现状);
 *                     false = 网格直连 KeepAlive(对照,证明差异只来自异步子树)。
 */
function mountHarness(asyncSubtree: boolean): Harness {
  const doc = new FakeEventTarget()
  const axisGate = ref(false)
  const restores = ref(0)
  const foreground = ref(true)
  const escapeHits = ref(0)
  let api: ReturnType<typeof useKeepAliveForeground> | null = null

  const onDocumentKeyDown = (event: Event): void => {
    if (event.type === 'keydown') escapeHits.value++
  }

  const Grid = defineComponent({
    name: 'MediaGrid',
    setup() {
      const foregroundApi = useKeepAliveForeground({
        isForeground: () => foreground.value,
        enter: () => {
          axisGate.value = true
          doc.addEventListener('keydown', onDocumentKeyDown)
        },
        leave: () => {
          axisGate.value = false
          doc.removeEventListener('keydown', onDocumentKeyDown)
        },
        onReactivate: () => {
          restores.value++
        },
      })
      api = foregroundApi
      return () => h('div')
    },
  })

  const layerContent: Component = asyncSubtree
    ? defineAsyncComponent(() => Promise.resolve(Grid))
    : Grid
  const Layer = defineComponent({
    name: 'GalleryRouteLayer',
    setup: () => () => h(layerContent),
  })
  const Other = defineComponent({ name: 'OtherRoute', setup: () => () => h('div') })

  const view = ref<'layer' | 'other'>('layer')
  const Root = defineComponent({
    name: 'TestRootShell',
    setup: () => () =>
      h(KeepAlive, { include: ['GalleryRouteLayer'] }, () =>
        view.value === 'layer' ? h(Layer) : h(Other),
      ),
  })

  const app = createApp(Root)
  app.mount(fakeNode('#root'))

  return {
    docKeydownListeners: () => doc.listenerCount('keydown'),
    pressEscape: () => doc.dispatch('keydown'),
    axisGate: () => axisGate.value,
    restores: () => restores.value,
    api: () => {
      if (!api) throw new Error('组件尚未挂载')
      return api
    },
    setForeground: (value) => {
      foreground.value = value
    },
    async show(next) {
      view.value = next
      await settle()
    },
    unmount: () => app.unmount(),
  }
}

/** 冲净 KeepAlive 与异步 wrapper 的多拍排队(mounted 与 activated 都排在 post-flush 队列里)。 */
async function settle(rounds = 8): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await nextTick()
    await Promise.resolve()
  }
}

/** 统计网格真实收到的 activated 次数(对照 asyncSubtree 两种拓扑)。 */
async function countActivations(asyncSubtree: boolean): Promise<number> {
  const counter = ref(0)
  const Grid = defineComponent({
    name: 'MediaGrid',
    setup() {
      onActivated(() => counter.value++)
      return () => h('div')
    },
  })
  const layerContent: Component = asyncSubtree
    ? defineAsyncComponent(() => Promise.resolve(Grid))
    : Grid
  const Layer = defineComponent({
    name: 'GalleryRouteLayer',
    setup: () => () => h(layerContent),
  })
  const Root = defineComponent({
    name: 'TestRootShell',
    setup: () => () => h(KeepAlive, { include: ['GalleryRouteLayer'] }, () => h(Layer)),
  })
  createApp(Root).mount(fakeNode('#root'))
  await settle()
  return counter.value
}

describe('约束:KeepAlive 不向异步子树补发首拍 activated(本组合存在的理由)', () => {
  it('直连子组件首拍拿到 activated,异步孙组件拿不到', async () => {
    expect(await countActivations(false)).toBe(1)
    expect(await countActivations(true)).toBe(0)
  })
})

describe('useKeepAliveForeground:生产拓扑(KeepAlive → 路由壳 → 异步网格)', () => {
  it('冷启动首拍就装好文档级键盘与轴控件门控——修前这两项整段缺席,ESC 与底栏轴都收不到', async () => {
    const harness = mountHarness(true)
    await settle()
    expect(harness.docKeydownListeners()).toBe(1)
    expect(harness.axisGate()).toBe(true)
    harness.pressEscape()
    expect(harness.docKeydownListeners()).toBe(1)
  })

  it('异组件路由往返:失活摘监听,返回恰好一次复激活且不重复挂监听', async () => {
    const harness = mountHarness(true)
    await settle()
    await harness.show('other')
    expect(harness.docKeydownListeners()).toBe(0)
    expect(harness.axisGate()).toBe(false)
    expect(harness.restores()).toBe(0)
    await harness.show('layer')
    expect(harness.docKeydownListeners()).toBe(1)
    expect(harness.axisGate()).toBe(true)
    expect(harness.restores()).toBe(1)
    await harness.show('other')
    await harness.show('layer')
    expect(harness.docKeydownListeners()).toBe(1)
    expect(harness.restores()).toBe(2)
  })

  it('对照:直连 KeepAlive 时与挂载同拍的激活只补装配,不触发复激活恢复', async () => {
    const harness = mountHarness(false)
    await settle()
    expect(harness.docKeydownListeners()).toBe(1)
    expect(harness.axisGate()).toBe(true)
    expect(harness.restores()).toBe(0)
    await harness.show('other')
    await harness.show('layer')
    expect(harness.restores()).toBe(1)
  })

  it('初始即后台(深链 /view 或异组件路由后台 resolve)时不装配前台,且后台状态落地', async () => {
    const harness = mountHarness(true)
    harness.setForeground(false)
    await settle()
    expect(harness.docKeydownListeners()).toBe(0)
    expect(harness.axisGate()).toBe(false)
  })

  it('覆盖层多次进出(不换路由,watcher 直接调入口):每次返回都恢复监听且不泄漏', async () => {
    const harness = mountHarness(true)
    await settle()
    expect(harness.docKeydownListeners()).toBe(1)
    for (let round = 0; round < 3; round++) {
      // 开图:`/view` 覆盖层进入,底层画廊留 DOM 但交出文档级键盘。
      harness.setForeground(false)
      harness.api().enterBackground()
      await settle()
      expect(harness.docKeydownListeners()).toBe(0)
      expect(harness.axisGate()).toBe(false)
      // 返回:覆盖层撤下,重新接管键盘,且不重复挂监听。
      harness.setForeground(true)
      harness.api().enterForeground()
      await settle()
      expect(harness.docKeydownListeners()).toBe(1)
      expect(harness.axisGate()).toBe(true)
    }
    // 覆盖层撤下后走一次真实失活(异组件路由):仍恰好摘干净。
    await harness.show('other')
    expect(harness.docKeydownListeners()).toBe(0)
  })

  it('后台期复激活不接管前台:覆盖层存续时异组件路由往返也不挂监听', async () => {
    const harness = mountHarness(true)
    await settle()
    harness.setForeground(false)
    harness.api().enterBackground()
    await harness.show('other')
    await harness.show('layer')
    expect(harness.docKeydownListeners()).toBe(0)
    expect(harness.axisGate()).toBe(false)
    expect(harness.restores()).toBe(0)
  })

  it('KeepAlive 整树销毁时摘除一次(不留文档级监听)', async () => {
    const harness = mountHarness(true)
    await settle()
    harness.unmount()
    await settle()
    expect(harness.docKeydownListeners()).toBe(0)
    expect(harness.axisGate()).toBe(false)
  })
})
