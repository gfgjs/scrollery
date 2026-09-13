// 加载闸门单例(B:快滚甩滚低保真)的契约测试(审查回补 F2 之一)。
// 模块级单例:各测例开头必须复位到 false,防跨测例串态。
import { describe, it, expect, beforeEach } from 'vitest'
import { effectScope, watch, nextTick } from 'vue'
import { setDeferThumbLoad, useThumbLoadGate, isThumbLoadDeferred } from './useThumbLoadGate'

beforeEach(() => setDeferThumbLoad(false))

describe('useThumbLoadGate(模块级单例闸门)', () => {
  it('isThumbLoadDeferred 即时读与置位一致', () => {
    expect(isThumbLoadDeferred()).toBe(false)
    setDeferThumbLoad(true)
    expect(isThumbLoadDeferred()).toBe(true)
    setDeferThumbLoad(false)
    expect(isThumbLoadDeferred()).toBe(false)
  })

  it('幂等写:同值重置不触发 watcher,仅真翻转触发(数千卡 watch 的齐发次数下限)', async () => {
    let fires = 0
    const scope = effectScope()
    scope.run(() => {
      watch(useThumbLoadGate(), () => fires++)
    })
    setDeferThumbLoad(false) // 同值 → 不触发
    await nextTick()
    expect(fires).toBe(0)
    setDeferThumbLoad(true)
    await nextTick()
    expect(fires).toBe(1)
    setDeferThumbLoad(true) // 同值 → 不触发
    await nextTick()
    expect(fires).toBe(1)
    setDeferThumbLoad(false)
    await nextTick()
    expect(fires).toBe(2)
    scope.stop()
  })

  it('useThumbLoadGate 恒返回同一只读句柄(零每卡代理分配)', () => {
    expect(useThumbLoadGate()).toBe(useThumbLoadGate())
  })

  it('只读句柄与写入端同源:写入即刻反映到句柄值', () => {
    const gate = useThumbLoadGate()
    setDeferThumbLoad(true)
    expect(gate.value).toBe(true)
  })
})
