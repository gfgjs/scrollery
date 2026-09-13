// src/composables/reader/useReaderProgress.spec.ts
// characterization:1200ms 去抖保存 + flush 捕获 (itemId,pos) 配对(方案 §5 风险面 1)。
// 红线断言(源文件头注释,2026-07-10 审查 B12):写库一律用**捕获时刻**的 itemId,不是 flush 时刻的
// id.value——route id 可能在去抖窗口内已变(doc→doc 复用导航/翻页即退)。fake timers 驱动去抖。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { ref } from 'vue'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn(() => Promise.resolve()) }))
vi.mock('../../utils/ipc', () => ({ invokeIpc }))
vi.mock('../../constants/ipc', () => ({ IPC: { SET_READING_PROGRESS: 'set_reading_progress' } }))

import { useReaderProgress } from './useReaderProgress'

describe('useReaderProgress:去抖保存 + 捕获时刻 itemId(characterization)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    invokeIpc.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('onProgress 1200ms 内不写库,到点后以捕获时刻的 (itemId,pos) 写一次', () => {
    const id = ref(1)
    const { onProgress } = useReaderProgress(id)
    onProgress('cfi:/1')
    expect(invokeIpc).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1199)
    expect(invokeIpc).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledWith('set_reading_progress', { itemId: 1, position: 'cfi:/1' })
  })

  it('去抖窗口内 id.value 已变,flush 仍写捕获时刻的旧 itemId(红线,不可用 flush 时刻的 id)', () => {
    const id = ref(1)
    const { onProgress } = useReaderProgress(id)
    onProgress('cfi:/1')
    id.value = 2 // 模拟窗口内路由切到另一本书
    vi.advanceTimersByTime(1200)
    expect(invokeIpc).toHaveBeenCalledWith('set_reading_progress', { itemId: 1, position: 'cfi:/1' })
  })

  it('flushProgress 手动调用立即写库并清计时器(load() 换文档前的显式冲刷路径)', () => {
    const id = ref(3)
    const { onProgress, flushProgress } = useReaderProgress(id)
    onProgress('cfi:/x')
    flushProgress()
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    // 计时器已被 flushProgress 内的 clearTimeout 清空:继续推进不应再触发第二次写库。
    vi.advanceTimersByTime(5000)
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })

  it('id 非 finite(NaN)时 onProgress 早退,不落 lastProgress,flush 无写库', () => {
    const id = ref(NaN)
    const { onProgress, flushProgress } = useReaderProgress(id)
    onProgress('cfi:/y')
    flushProgress()
    expect(invokeIpc).not.toHaveBeenCalled()
  })

  it('reset 只清引用,不冲刷:reset 后 flush 不再写库', () => {
    const id = ref(4)
    const { onProgress, flushProgress, reset } = useReaderProgress(id)
    onProgress('cfi:/z')
    reset()
    flushProgress()
    expect(invokeIpc).not.toHaveBeenCalled()
  })

  it('连续 onProgress 重置去抖计时器:只有最后一次的 pos 落库', () => {
    const id = ref(5)
    const { onProgress } = useReaderProgress(id)
    onProgress('cfi:/a')
    vi.advanceTimersByTime(600)
    onProgress('cfi:/b')
    vi.advanceTimersByTime(600)
    expect(invokeIpc).not.toHaveBeenCalled()
    vi.advanceTimersByTime(600)
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledWith('set_reading_progress', { itemId: 5, position: 'cfi:/b' })
  })
})
