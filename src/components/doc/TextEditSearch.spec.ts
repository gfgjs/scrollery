import { afterEach, expect, it, vi } from 'vitest'
import { createRenderer, h, nextTick, ref, shallowRef, ssrContextKey } from 'vue'
import TextEditSearch from './TextEditSearch.vue'

vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
afterEach(() => vi.unstubAllGlobals())

it.each(['red red', '第一行\r\nred red'])('从选区打开替换只修改选中的重复词：%j', async (initial) => {
  const value = ref(initial)
  const display = initial.replace(/\r\n?/g, '\n')
  const selectedStart = display.lastIndexOf('red')
  const area = {
    value: display, selectionStart: selectedStart, selectionEnd: selectedStart + 3,
    scrollLeft: 0, scrollTop: 0, clientWidth: 400, clientHeight: 300,
    addEventListener() {}, removeEventListener() {}, focus: vi.fn(),
    getBoundingClientRect: () => ({ left: 0, top: 0, right: 400, bottom: 300 }),
    setSelectionRange(start: number, end: number) { this.selectionStart = start; this.selectionEnd = end },
    dispatchEvent() { value.value = this.value; return true },
  }
  // 模拟浏览器 insertText 按 textarea 的当前 UTF-16 选区替换，并让真实组件派发 input 同步父层。
  const execCommand = vi.fn((_command: string, _showUI: boolean, text: string) => {
    area.value = area.value.slice(0, area.selectionStart) + text + area.value.slice(area.selectionEnd)
    return true
  })
  vi.stubGlobal('HTMLElement', class {})
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('getComputedStyle', () => ({}))
  vi.stubGlobal('document', {
    activeElement: null, execCommand,
    createElement: () => ({ style: {}, textContent: '', firstChild: {}, remove() {} }),
    body: { append() {} },
    createRange: () => ({ setStart() {}, setEnd() {}, getClientRects: () => [] }),
  })
  const component = Object.assign({}, TextEditSearch, { render: () => null })
  const panel = shallowRef<InstanceType<typeof TextEditSearch> | null>(null)
  const app = renderer.createApp({
    setup: () => () => h(component, {
      ref: panel, value: value.value, textarea: area as unknown as HTMLTextAreaElement,
      disabled: false, textStyle: {},
    }),
  })
  app.provide(ssrContextKey, {})
  app.mount({})
  const state = (panel.value!.$ as unknown as {
    setupState: { query: string; replacement: string; replaceCurrent(): void }
  }).setupState
  try {
    panel.value!.open(true)
    await nextTick()
    expect(state.query).toBe('red')
    expect([area.selectionStart, area.selectionEnd]).toEqual([selectedStart, selectedStart + 3])
    state.replacement = 'blue'
    state.replaceCurrent()
    await nextTick()
    expect(execCommand).toHaveBeenCalledExactlyOnceWith('insertText', false, 'blue')
    expect(value.value).toBe(display.slice(0, selectedStart) + 'blue')
    expect(area.value).toBe(value.value)
  } finally {
    app.unmount()
  }
})
