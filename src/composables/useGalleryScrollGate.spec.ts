import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useGalleryScrollGate } from './useGalleryScrollGate'
import { isThumbLoadDeferred, setDeferThumbLoad } from './useThumbLoadGate'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
  setDeferThumbLoad(false)
})

afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  setDeferThumbLoad(false)
})

describe('画廊滚动闸门释放', () => {
  it('连续轴跳转没有产生 scroll 事件时，最后一次跳转停稳后仍会放行', () => {
    const gate = useGalleryScrollGate({ rowHeight: () => 100 })
    // Minimap 在边界重复发出相同目标位置，父层会记跳转，但浏览器不会触发 scroll。
    gate.noteAxisJump()
    expect(isThumbLoadDeferred()).toBe(false)
    vi.advanceTimersByTime(16)
    gate.noteAxisJump()
    expect(isThumbLoadDeferred()).toBe(true)

    vi.advanceTimersByTime(48)
    gate.noteAxisJump()
    vi.advanceTimersByTime(16)
    expect(isThumbLoadDeferred()).toBe(true)
    vi.advanceTimersByTime(47)
    expect(isThumbLoadDeferred()).toBe(true)
    vi.advanceTimersByTime(1)
    expect(isThumbLoadDeferred()).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('轴跳转和滚动采样共用释放计时器，失活清理能撤销最后一次排程', () => {
    const gate = useGalleryScrollGate({ rowHeight: () => 100 })
    gate.noteAxisJump()
    vi.advanceTimersByTime(16)
    gate.noteAxisJump()
    gate.sampleScrollGate(100, performance.now(), false)
    expect(vi.getTimerCount()).toBe(1)

    vi.advanceTimersByTime(48)
    gate.noteAxisJump()
    vi.advanceTimersByTime(16)
    expect(isThumbLoadDeferred()).toBe(true)
    expect(vi.getTimerCount()).toBe(1)

    gate.clearGateTimer()
    expect(vi.getTimerCount()).toBe(0)
    vi.advanceTimersByTime(1000)
    // 失活调用方负责复位；已经撤销的回调不能再改变共享信号。
    expect(isThumbLoadDeferred()).toBe(true)
    setDeferThumbLoad(false)
  })
})
