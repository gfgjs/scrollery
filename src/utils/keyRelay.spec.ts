// src/utils/keyRelay.spec.ts
// iframe → 父文档 键盘中继的契约(2026-07-16 真机 #2a:焦点进书里后按住 Esc 退全屏失效)。
//
// node 环境无 DOM,故以最小的鸭子类型替身驱动:被测函数只用到 addEventListener / activeElement /
// dispatchEvent / preventDefault 四个面,替身能忠实复现。真跨 iframe 的行为(事件是否真不冒泡、
// 同源是否真够得到 contentDocument)必须真机验收(experience §19)。

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { relayKeyboardEvents } from './keyRelay'

type Listener = (e: unknown) => void

/** 假 iframe 文档:记录中继装了哪些监听,并可代为触发。 */
function makeFakeDoc(activeElement: unknown = null) {
  const listeners = new Map<string, Listener[]>()
  const doc = {
    activeElement,
    addEventListener(type: string, fn: Listener, capture?: boolean) {
      calls.push({ type, capture: capture === true })
      if (!listeners.has(type)) listeners.set(type, [])
      listeners.get(type)!.push(fn)
    },
    removeEventListener(type: string, fn: Listener, capture?: boolean) {
      removals.push({ type, capture: capture === true })
      const arr = listeners.get(type)
      if (arr) listeners.set(type, arr.filter((f) => f !== fn))
    },
  }
  const calls: { type: string; capture: boolean }[] = []
  const removals: { type: string; capture: boolean }[] = []
  function fire(type: string, e: Record<string, unknown>) {
    ;(listeners.get(type) ?? []).forEach((fn) => fn(e))
  }
  return { doc, fire, calls, removals, listenerCount: () => (listeners.get('keydown') ?? []).length }
}

/** 假父页宿主:dispatchEvent 返回 false 即模拟「父页 preventDefault 了」。 */
function makeFakeHost(consume = false) {
  const dispatched: { type: string; init: Record<string, unknown> }[] = []
  const host = {
    dispatchEvent(e: unknown) {
      const ev = e as { type: string; __init: Record<string, unknown> }
      dispatched.push({ type: ev.type, init: ev.__init })
      return !consume
    },
  }
  return { host, dispatched }
}

/** 造一个 iframe 内的假 keydown。preventDefault 被调用则置 prevented。 */
function makeSrcEvent(over: Record<string, unknown> = {}) {
  const e: Record<string, unknown> = {
    type: 'keydown',
    key: 'Escape',
    code: 'Escape',
    location: 0,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    repeat: false,
    isComposing: false,
    defaultPrevented: false,
    prevented: false,
    preventDefault() {
      e.prevented = true
    },
    ...over,
  }
  return e
}

beforeEach(() => {
  // 被测函数在父页 realm 里 new KeyboardEvent(...)。node 无该构造器 → 以最小替身顶上,
  // 并把 init 原样挂到实例上供断言(真浏览器里这些字段由构造器落到只读属性)。
  vi.stubGlobal(
    'KeyboardEvent',
    class {
      type: string
      __init: Record<string, unknown>
      constructor(type: string, init: Record<string, unknown>) {
        this.type = type
        this.__init = init
      }
    },
  )
})

describe('装配', () => {
  it('keydown 与 keyup 都中继 —— 少一个,按住 Esc 的退全屏就填满后退不回来', () => {
    const { doc, calls } = makeFakeDoc()
    const { host } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    expect(calls.map((c) => c.type).sort()).toEqual(['keydown', 'keyup'])
  })

  it('用 capture 阶段装 —— 书内脚本(EPUB 可带 JS)冒泡期 stopPropagation 也拦不住中继', () => {
    const { doc, calls } = makeFakeDoc()
    const { host } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    expect(calls.every((c) => c.capture)).toBe(true)
  })

  it('返回的摘除函数把两条监听都摘干净(capture 标志须匹配,否则摘不掉)', () => {
    const { doc, removals, listenerCount } = makeFakeDoc()
    const { host } = makeFakeHost()
    const dispose = relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    expect(listenerCount()).toBe(1)
    dispose()
    expect(listenerCount()).toBe(0)
    expect(removals.map((r) => r.type).sort()).toEqual(['keydown', 'keyup'])
    expect(removals.every((r) => r.capture)).toBe(true)
  })
})

describe('中继保真', () => {
  it('按键字段与修饰键逐个带过去(丢 ctrlKey 则 mod+z 在书里变成裸 z)', () => {
    const { doc, fire } = makeFakeDoc()
    const { host, dispatched } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    fire('keydown', makeSrcEvent({ key: 'z', code: 'KeyZ', ctrlKey: true, repeat: true }))
    expect(dispatched).toHaveLength(1)
    expect(dispatched[0].type).toBe('keydown')
    expect(dispatched[0].init).toMatchObject({
      key: 'z',
      code: 'KeyZ',
      ctrlKey: true,
      shiftKey: false,
      repeat: true,
      bubbles: true,
      cancelable: true,
    })
  })

  it('合成事件必须 bubbles+cancelable:不冒泡则只有宿主自己收到(window 级守卫全瞎),不可取消则回传抑制失效', () => {
    const { doc, fire } = makeFakeDoc()
    const { host, dispatched } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    fire('keydown', makeSrcEvent())
    expect(dispatched[0].init.bubbles).toBe(true)
    expect(dispatched[0].init.cancelable).toBe(true)
  })
})

describe('回传抑制', () => {
  it('父页消费了(preventDefault) → iframe 内也 preventDefault,避免空格既翻页又滚 iframe', () => {
    const { doc, fire } = makeFakeDoc()
    const { host } = makeFakeHost(true) // dispatchEvent 返回 false = 被消费
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    const src = makeSrcEvent({ key: ' ' })
    fire('keydown', src)
    expect(src.prevented).toBe(true)
  })

  it('父页没消费 → 不动 iframe 内的默认行为', () => {
    const { doc, fire } = makeFakeDoc()
    const { host } = makeFakeHost(false)
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    const src = makeSrcEvent({ key: 'a' })
    fire('keydown', src)
    expect(src.prevented).toBe(false)
  })
})

describe('不该中继的情形', () => {
  it('书内输入框聚焦时不中继 —— 父页的 e.target 守卫看不进 iframe,不在源头拦就会边打字边翻页', () => {
    for (const el of [
      { tagName: 'INPUT' },
      { tagName: 'TEXTAREA' },
      { tagName: 'SELECT' },
      { tagName: 'DIV', isContentEditable: true },
    ]) {
      const { doc, fire } = makeFakeDoc(el)
      const { host, dispatched } = makeFakeHost()
      relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
      fire('keydown', makeSrcEvent({ key: 'ArrowRight' }))
      expect(dispatched).toHaveLength(0)
    }
  })

  it('普通元素聚焦(书正文)照常中继', () => {
    const { doc, fire } = makeFakeDoc({ tagName: 'DIV', isContentEditable: false })
    const { host, dispatched } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    fire('keydown', makeSrcEvent({ key: 'ArrowRight' }))
    expect(dispatched).toHaveLength(1)
  })

  it('iframe 内已被消费的键不再转出 —— 否则书内处理器与父页处理器双执行', () => {
    const { doc, fire } = makeFakeDoc()
    const { host, dispatched } = makeFakeHost()
    relayKeyboardEvents(doc as unknown as Document, host as unknown as HTMLElement)
    fire('keydown', makeSrcEvent({ defaultPrevented: true }))
    expect(dispatched).toHaveLength(0)
  })
})
