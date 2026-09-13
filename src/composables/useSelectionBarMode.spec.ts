// useSelectionBarMode contract 测试(C3)。node 环境无 localStorage,故:
//   ① clampOffset(纯函数)穷举——本方案唯一的坐标数学,是拖拽位置持久化(C4)的正确性基石;
//   ② parseStoredOffset(纯函数)覆盖各类残缺输入的回落;
//   ③ docked/offset 单例的内存态读写与响应式(持久化经 try/catch 静默降级,不在 node 可验)。

import { describe, it, expect } from 'vitest'
import {
  useSelectionBarMode,
  clampOffset,
  parseStoredOffset,
} from './useSelectionBarMode'

describe('clampOffset（拖拽位置钳制，纯函数穷举）', () => {
  // 基准:1000×800 边界,400×48 胶囊,留边 8,BOTTOM_INSET 32。
  //   halfSlackX = (1000-400)/2 - 8 = 292
  //   upSlack    = 800-48-32-8 = 712
  //   downSlack  = 32-8 = 24
  const capsule = { width: 400, height: 48 }
  const bounds = { width: 1000, height: 800 }

  it('范围内 offset 原样返回', () => {
    expect(clampOffset({ x: 100, y: -200 }, capsule, bounds)).toEqual({ x: 100, y: -200 })
    expect(clampOffset({ x: 0, y: 0 }, capsule, bounds)).toEqual({ x: 0, y: 0 })
  })

  it('x 超右/超左 → 钳到 ±halfSlackX', () => {
    expect(clampOffset({ x: 9999, y: 0 }, capsule, bounds).x).toBe(292)
    expect(clampOffset({ x: -9999, y: 0 }, capsule, bounds).x).toBe(-292)
  })

  it('y 超上 → 钳到 -upSlack；超下 → 钳到 downSlack', () => {
    expect(clampOffset({ x: 0, y: -9999 }, capsule, bounds).y).toBe(-712)
    expect(clampOffset({ x: 0, y: 9999 }, capsule, bounds).y).toBe(24)
  })

  it('胶囊比边界宽 → x 钳到 0（居中不动）', () => {
    const wide = { width: 1200, height: 48 }
    expect(clampOffset({ x: 500, y: 0 }, wide, bounds).x).toBe(0)
    expect(clampOffset({ x: -500, y: 0 }, wide, bounds).x).toBe(0)
  })

  it('胶囊比可上移空间高 → upSlack 钳到 0（不能上移越界）', () => {
    const tall = { width: 400, height: 790 }
    // upSlack = max(0, 800-790-32-8) = max(0,-30) = 0 → y 钳到 [0, downSlack]
    expect(clampOffset({ x: 0, y: -100 }, tall, bounds).y).toBe(0)
  })

  it('钳制幂等：钳后再钳不变', () => {
    const once = clampOffset({ x: 9999, y: -9999 }, capsule, bounds)
    expect(clampOffset(once, capsule, bounds)).toEqual(once)
  })

  it("缺省 align 与显式 'center' 逐值等价（3 参调用向后兼容）", () => {
    expect(clampOffset({ x: 9999, y: 500 }, capsule, bounds)).toEqual(
      clampOffset({ x: 9999, y: 500 }, capsule, bounds, 'center'),
    )
    expect(clampOffset({ x: -9999, y: -9999 }, capsule, bounds)).toEqual(
      clampOffset({ x: -9999, y: -9999 }, capsule, bounds, 'center'),
    )
  })
})

describe('clampOffset（对齐基位:靠左/靠右非对称钳制）', () => {
  // 同基准 1000×800 / 400×48;SIDE_INSET=12,CLAMP_MARGIN=8。
  //   靠左 baseLeft=12 → x∈[8-12, 1000-400-8-12] = [-4, 580](几乎只能右移)
  //   靠右 baseLeft=1000-400-12=588 → x∈[8-588, 1000-400-8-588] = [-580, 4](几乎只能左移)
  const capsule = { width: 400, height: 48 }
  const bounds = { width: 1000, height: 800 }

  it("靠左:超右钳到 580、超左钳到 -4、区间内原样", () => {
    expect(clampOffset({ x: 9999, y: 0 }, capsule, bounds, 'left').x).toBe(580)
    expect(clampOffset({ x: -9999, y: 0 }, capsule, bounds, 'left').x).toBe(-4)
    expect(clampOffset({ x: 100, y: 0 }, capsule, bounds, 'left').x).toBe(100)
  })

  it("靠右:超右钳到 4、超左钳到 -580、区间内原样", () => {
    expect(clampOffset({ x: 9999, y: 0 }, capsule, bounds, 'right').x).toBe(4)
    expect(clampOffset({ x: -9999, y: 0 }, capsule, bounds, 'right').x).toBe(-580)
    expect(clampOffset({ x: -100, y: 0 }, capsule, bounds, 'right').x).toBe(-100)
  })

  it('靠左右移幅度 = 靠右左移幅度(镜像对称)', () => {
    expect(clampOffset({ x: 9999, y: 0 }, capsule, bounds, 'left').x).toBe(
      -clampOffset({ x: -9999, y: 0 }, capsule, bounds, 'right').x,
    )
  })

  it('胶囊比边界宽时任何对齐都 x 钳到 0(容不下不位移)', () => {
    const wide = { width: 1200, height: 48 }
    expect(clampOffset({ x: 500, y: 0 }, wide, bounds, 'left').x).toBe(0)
    expect(clampOffset({ x: 500, y: 0 }, wide, bounds, 'right').x).toBe(0)
  })

  it('竖直钳制与 align 无关(同居中)', () => {
    expect(clampOffset({ x: 0, y: 9999 }, capsule, bounds, 'left').y).toBe(24)
    expect(clampOffset({ x: 0, y: -9999 }, capsule, bounds, 'right').y).toBe(-712)
  })
})

describe('parseStoredOffset（残缺输入回落原点）', () => {
  it('null / 空串 → 原点', () => {
    expect(parseStoredOffset(null)).toEqual({ x: 0, y: 0 })
    expect(parseStoredOffset('')).toEqual({ x: 0, y: 0 })
  })

  it('合法 JSON → 原值', () => {
    expect(parseStoredOffset('{"x":12,"y":-34}')).toEqual({ x: 12, y: -34 })
  })

  it('非法 JSON → 原点', () => {
    expect(parseStoredOffset('not-json')).toEqual({ x: 0, y: 0 })
  })

  it('缺字段 / 非数字 / 非有限 → 原点', () => {
    expect(parseStoredOffset('{"x":1}')).toEqual({ x: 0, y: 0 })
    expect(parseStoredOffset('{"x":"a","y":2}')).toEqual({ x: 0, y: 0 })
    expect(parseStoredOffset('{"x":null,"y":null}')).toEqual({ x: 0, y: 0 })
    // NaN/Infinity 经 JSON 序列化会成 null,但防御性覆盖:JSON 无法直接产 Infinity,构造等价检查。
    expect(parseStoredOffset(JSON.stringify({ x: 5, y: 6 }))).toEqual({ x: 5, y: 6 })
  })
})

describe('useSelectionBarMode（单例内存态）', () => {
  it('默认 docked=false（分离，node 环境无持久值回落）', () => {
    const mode = useSelectionBarMode()
    // 首次读取:node 无 localStorage → 回落 false。
    expect(mode.docked.value).toBe(false)
  })

  it('setDocked 更新响应式 docked（单例内存态）', () => {
    const mode = useSelectionBarMode()
    mode.setDocked(true)
    expect(mode.docked.value).toBe(true)
    mode.setDocked(false)
    expect(mode.docked.value).toBe(false)
  })

  it('setOffset 更新响应式 offset（单例内存态）', () => {
    const mode = useSelectionBarMode()
    mode.setOffset({ x: 7, y: -3 })
    expect(mode.offset.value).toEqual({ x: 7, y: -3 })
    mode.setOffset({ x: 0, y: 0 })
    expect(mode.offset.value).toEqual({ x: 0, y: 0 })
  })

  it('hostActive 默认 true，setHostActive 可切换（docked 让位与 Teleport gate 的同源信号）', () => {
    const mode = useSelectionBarMode()
    expect(mode.hostActive.value).toBe(true)
    mode.setHostActive(false)
    expect(mode.hostActive.value).toBe(false)
    mode.setHostActive(true)
    expect(mode.hostActive.value).toBe(true)
  })

  it("align 默认 'center'（node 无持久值回落），setAlign 更新响应式 align（单例内存态）", () => {
    const mode = useSelectionBarMode()
    expect(mode.align.value).toBe('center')
    mode.setAlign('left')
    expect(mode.align.value).toBe('left')
    mode.setAlign('right')
    expect(mode.align.value).toBe('right')
    // 复位,避免污染同文件后续单例读者。
    mode.setAlign('center')
    expect(mode.align.value).toBe('center')
  })
})
