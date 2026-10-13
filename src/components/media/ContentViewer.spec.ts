import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { compile, createRenderer, nextTick, ssrContextKey, type App } from 'vue'
import PluginGate from '../exotic/PluginGate.vue'
import { resetExoticFormatCache, useExoticGate } from '../../composables/useExoticGate'
import { IPC } from '../../constants/ipc'
import type { FormatResolution } from '../../types/exotic'
import { parse } from '@vue/compiler-sfc'
import { AlertTriangle, KeyRound } from '@lucide/vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../../utils/ipc', () => ({ invokeIpc: mocks.invoke }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

// 编译看图入口的真实绑定，保留 PluginGate 和查询状态逻辑，仅替换 DOM 与 IPC。
const source = readFileSync(new URL('./ContentViewer.vue', import.meta.url), 'utf8')
const gateTemplate = source.match(/<PluginGate\s[\s\S]*?\/>/)?.[0]
if (!gateTemplate) throw new Error('未找到看图门控入口')
const renderGate = compile(gateTemplate)
const pluginTemplate = parse(readFileSync(new URL('../exotic/PluginGate.vue', import.meta.url), 'utf8')).descriptor.template!.content
// Vitest 的 node 环境只生成 ssrRender；补编译真实客户端模板，并展开真实 setup 的返回值。
const ClientPluginGate = {
  ...PluginGate,
  components: { AlertTriangle, KeyRound },
  setup: (props: unknown, context: unknown) => ({
    ...(PluginGate as unknown as {
      setup: (props: unknown, context: unknown) => Record<string, unknown>
    }).setup(props, context),
  }),
  render: compile(pluginTemplate),
}

interface TestNode {
  tag: string
  text: string
  props: Record<string, unknown>
  children: TestNode[]
  parent: TestNode | null
}

function node(tag: string, text = ''): TestNode {
  return { tag, text, props: {}, children: [], parent: null }
}

const renderer = createRenderer<TestNode, TestNode>({
  createElement: (tag) => node(tag),
  createText: (text) => node('#text', text),
  createComment: () => node('#comment'),
  setText: (target, text) => { target.text = text },
  setElementText: (target, text) => { target.text = text; target.children = [] },
  patchProp: (target, key, _previous, value) => { target.props[key] = value },
  insert(child, parent, anchor) {
    child.parent = parent
    const index = anchor ? parent.children.indexOf(anchor) : -1
    parent.children.splice(index < 0 ? parent.children.length : index, 0, child)
  },
  remove(child) {
    const siblings = child.parent?.children
    if (siblings) siblings.splice(siblings.indexOf(child), 1)
    child.parent = null
  },
  parentNode: (target) => target.parent,
  nextSibling: (target) => {
    const siblings = target.parent?.children
    return siblings?.[siblings.indexOf(target) + 1] ?? null
  },
})

function textContent(target: TestNode): string {
  return target.text + target.children.map(textContent).join('')
}

function findButton(target: TestNode): TestNode | undefined {
  if (target.tag === 'button') return target
  return target.children.map(findButton).find(Boolean)
}

const resolution: FormatResolution = {
  format: 'psd', mediaKind: 'image', pluginId: 'psd', capabilities: [],
  availability: 'ready', activationRequired: true, activated: true,
  installedVersion: '1.0.0', builtin: false,
}
let app: App | undefined

beforeEach(() => {
  resetExoticFormatCache()
  mocks.invoke.mockReset()
})
afterEach(() => {
  app?.unmount()
  app = undefined
  resetExoticFormatCache()
})

function mountGate() {
  const exoticGate = useExoticGate()
  const refreshExoticGate = vi.fn(() => exoticGate.resolveForItem(42, 'psd'))
  const root = node('root')
  app = renderer.createApp({
    components: { PluginGate: ClientPluginGate },
    setup: () => ({ exoticGate, refreshExoticGate, exoticFeatureName: 'PSD', activateOpen: false }),
    render: renderGate,
  })
  app.config.globalProperties.$t = (key: string) => key
  app.component('RouterLink', { render: () => null })
  app.provide(ssrContextKey, {})
  app.mount(root)
  return { exoticGate, refreshExoticGate, root }
}

describe('看图入口的插件查询门控', () => {
  it('查询未完成时显示加载提示，完成后清除提示', async () => {
    let finish!: (value: FormatResolution[]) => void
    mocks.invoke.mockReturnValueOnce(new Promise<FormatResolution[]>((resolve) => { finish = resolve }))
    const { exoticGate, root } = mountGate()
    const resolving = exoticGate.resolveForItem(42, 'jpg')
    await nextTick()
    try {
      expect(textContent(root)).toContain('exotic.gateChecking')
    } finally {
      finish([])
      await resolving
      await nextTick()
    }
    expect(textContent(root)).not.toContain('exotic.gateChecking')
  })

  it('查询失败后显示重试，点击后重新查询当前资产并恢复就绪状态', async () => {
    mocks.invoke.mockRejectedValueOnce(new Error('查询失败'))
    const { exoticGate, refreshExoticGate, root } = mountGate()
    await exoticGate.resolveForItem(42, 'psd')
    await nextTick()
    expect(textContent(root)).toContain('official.queryFailed')
    const retry = findButton(root)
    expect(retry?.text).toBe('official.retry')

    let finish!: (value: FormatResolution[]) => void
    mocks.invoke.mockReturnValueOnce(new Promise<FormatResolution[]>((resolve) => { finish = resolve }))
    mocks.invoke.mockResolvedValueOnce({ itemId: 42, format: 'psd', resolution, taskState: 'done' })
    const clickRetry = retry?.props.onClick as () => void
    clickRetry()
    expect(refreshExoticGate).toHaveBeenCalledOnce()
    await nextTick()
    expect(textContent(root)).toContain('exotic.gateChecking')
    expect(textContent(root)).not.toContain('official.queryFailed')

    finish([resolution])
    await refreshExoticGate.mock.results[0]?.value
    await nextTick()
    expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.GET_EXOTIC_ITEM_STATE, { itemId: 42 })
    expect(exoticGate.entitlement.value?.availability).toBe('ready')
    expect(textContent(root)).not.toContain('official.retry')
  })
})
