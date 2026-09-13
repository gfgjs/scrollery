// useMediaDetail characterization 测试(顶栏重构 P4 迁移前锁行为)。看图台缩放/平移/缩放模式/
// 滚轮/拖拽的现状行为,作为「覆盖层→路由」迁移(P4-2)的安全网:该组合式内核迁移中**保留不变**
// (仅换宿主与去 body Teleport),故本测试确保迁移后行为零漂移。
//
// 项目无 DOM 测试环境(全 node,遵「控制依赖膨胀」不引 jsdom)——拖拽用最小 document 桩测。
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { useMediaDetail } from './useMediaDetail'

describe('useMediaDetail 缩放数学(纯,node)', () => {
  it('zoomIn/zoomOut:×1.25/×0.8,钳位 [0.1,10],mode→custom', () => {
    const m = useMediaDetail()
    expect(m.scale.value).toBe(1)
    m.zoomIn()
    expect(m.scale.value).toBeCloseTo(1.25)
    expect(m.zoomMode.value).toBe('custom')
    m.zoomOut()
    expect(m.scale.value).toBeCloseTo(1.0)
    for (let i = 0; i < 20; i++) m.zoomIn()
    expect(m.scale.value).toBe(10) // 上钳位
    for (let i = 0; i < 40; i++) m.zoomOut()
    expect(m.scale.value).toBe(0.1) // 下钳位
  })

  it('resetZoom:scale=1 / translate=0 / mode=auto', () => {
    const m = useMediaDetail()
    m.zoomIn()
    m.resetZoom()
    expect(m.scale.value).toBe(1)
    expect(m.translateX.value).toBe(0)
    expect(m.translateY.value).toBe(0)
    expect(m.zoomMode.value).toBe('auto')
  })

  it('setZoomMode:original/fit-width/fit-height 缩放数学(容器 1000×800,图 2000×1000)', () => {
    const m = useMediaDetail()
    // base_w=min(2000,1000,800*2=1600)=1000; base_h=min(1000,800,1000*0.5=500)=500
    m.setZoomMode('original', 1000, 800, 2000, 1000) // 2000/1000
    expect(m.scale.value).toBeCloseTo(2)
    m.setZoomMode('fit-width', 1000, 800, 2000, 1000) // 1000/1000
    expect(m.scale.value).toBeCloseTo(1)
    m.setZoomMode('fit-height', 1000, 800, 2000, 1000) // 800/500
    expect(m.scale.value).toBeCloseTo(1.6)
    m.setZoomMode('auto', 1000, 800, 2000, 1000)
    expect(m.scale.value).toBe(1)
    expect(m.translateX.value).toBe(0)
  })

  it('cycleZoomMode 循环 auto→original→fit-width→fit-height→auto', () => {
    const m = useMediaDetail()
    expect(m.zoomMode.value).toBe('auto')
    for (const expected of ['original', 'fit-width', 'fit-height', 'auto']) {
      m.cycleZoomMode(1000, 800, 2000, 1000)
      expect(m.zoomMode.value).toBe(expected)
    }
  })

  it('fitToWindow:min(cw/iw, ch/ih, 1),从不放大', () => {
    const m = useMediaDetail()
    m.fitToWindow(500, 500, 1000, 1000) // 大图缩小 → 0.5
    expect(m.scale.value).toBe(0.5)
    m.fitToWindow(1000, 1000, 500, 500) // 小图钳到 1(不放大)
    expect(m.scale.value).toBe(1)
  })

  it('onWheel:ctrl+滚轮缩放返回 true;普通滚轮返回 false 不缩放', () => {
    const m = useMediaDetail()
    const ctrlWheelUp = {
      ctrlKey: true,
      metaKey: false,
      deltaY: -100,
      preventDefault() {},
    } as unknown as WheelEvent
    expect(m.onWheel(ctrlWheelUp)).toBe(true)
    expect(m.scale.value).toBeCloseTo(1.1)
    const before = m.scale.value
    const plainWheel = {
      ctrlKey: false,
      metaKey: false,
      deltaY: -100,
      preventDefault() {},
    } as unknown as WheelEvent
    expect(m.onWheel(plainWheel)).toBe(false)
    expect(m.scale.value).toBe(before)
  })

  it('transform 计算串:translate → scale → rotate 串联', () => {
    // 信息面板显隐(showInfo/toggleInfo)已上收 uiStore(2026-07-14),不再属本组合式内核。
    const m = useMediaDetail()
    m.zoomIn()
    expect(m.transform.value).toBe(`translate(0px, 0px) scale(${m.scale.value}) rotate(0deg)`)
  })
})

describe('useMediaDetail 旋转(顶栏重构 P5)', () => {
  it('rotate:顺时针 90° 单向累进不取模 0→90→180→270→360,并入 transform', () => {
    // 单向累进(2026-07-18):第 4 次不回绕到 0 而是到 360——避免 CSS transition 把 270→0 插成逆时针
    // 大回旋。缩放数学用 rotation % 180,对累进值恒正确;归一化(供持久化/角标)由消费方做。
    const m = useMediaDetail()
    expect(m.rotation.value).toBe(0)
    m.rotate(1000, 800, 2000, 1000)
    expect(m.rotation.value).toBe(90)
    expect(m.transform.value).toContain('rotate(90deg)')
    m.rotate(1000, 800, 2000, 1000)
    expect(m.rotation.value).toBe(180)
    m.rotate(1000, 800, 2000, 1000)
    expect(m.rotation.value).toBe(270)
    m.rotate(1000, 800, 2000, 1000)
    expect(m.rotation.value).toBe(360)
    expect(m.rotation.value % 360).toBe(0) // 视觉等价未旋转,但角度值累进不回绕
  })

  it('setRotation:直接设角(打开大图复原持久旋转),不做适应', () => {
    const m = useMediaDetail()
    m.setRotation(270)
    expect(m.rotation.value).toBe(270)
    expect(m.transform.value).toContain('rotate(270deg)')
  })

  it('rotate 后 auto 适应新朝向(足迹宽高对换):2000×1000 图 / 1000×800 容器', () => {
    const m = useMediaDetail()
    m.setZoomMode('auto', 1000, 800, 2000, 1000) // 0° → 1
    expect(m.scale.value).toBeCloseTo(1)
    m.rotate(1000, 800, 2000, 1000) // 90°:足迹 500w×1000h → fit=min(1000/500,800/1000,1)=0.8
    expect(m.scale.value).toBeCloseTo(0.8)
    m.rotate(1000, 800, 2000, 1000) // 180°:足迹复原 → 1
    expect(m.scale.value).toBeCloseTo(1)
    m.rotate(1000, 800, 2000, 1000) // 270°:再对换 → 0.8
    expect(m.scale.value).toBeCloseTo(0.8)
  })

  it('resetZoom 复位旋转到 0', () => {
    const m = useMediaDetail()
    m.rotate(1000, 800, 2000, 1000)
    expect(m.rotation.value).toBe(90)
    m.resetZoom()
    expect(m.rotation.value).toBe(0)
  })

  it('旋转 90° 后 fit-height 用对换后尺寸(锁 setZoomMode 旋转分支)', () => {
    const m = useMediaDetail()
    m.rotate(1000, 800, 2000, 1000) // rotation=90
    m.setZoomMode('fit-height', 1000, 800, 2000, 1000)
    // @90°:eih=iw=2000,base_h=min(2000,800,1000*2)=800,scale=ch/base_h=800/800=1
    expect(m.scale.value).toBeCloseTo(1)
  })
})

describe('useMediaDetail 拖拽平移(最小 document 桩)', () => {
  const listeners: Record<string, Array<(e: unknown) => void>> = { mousemove: [], mouseup: [] }
  beforeEach(() => {
    listeners.mousemove = []
    listeners.mouseup = []
    ;(globalThis as unknown as { document: unknown }).document = {
      addEventListener: (t: string, h: (e: unknown) => void) => listeners[t]?.push(h),
      removeEventListener: (t: string, h: (e: unknown) => void) => {
        if (listeners[t]) listeners[t] = listeners[t].filter((x) => x !== h)
      },
    }
  })
  afterEach(() => {
    delete (globalThis as unknown as { document?: unknown }).document
  })
  function fire(type: string, e: unknown) {
    ;(listeners[type] || []).forEach((h) => h(e))
  }

  it('scale≤1 不拖拽;scale>1 拖拽更新 translate;stopDrag 清态并停更', () => {
    const m = useMediaDetail()
    // scale=1 → startDrag 直接返回
    m.startDrag({ clientX: 0, clientY: 0 } as MouseEvent)
    expect(m.isDragging.value).toBe(false)

    // scale>1 → 进入拖拽
    m.zoomIn() // 1.25
    m.startDrag({ clientX: 100, clientY: 100 } as MouseEvent)
    expect(m.isDragging.value).toBe(true)

    // mousemove:translate = 初值 0 + (新 − 起)
    fire('mousemove', { clientX: 150, clientY: 120 })
    expect(m.translateX.value).toBe(50)
    expect(m.translateY.value).toBe(20)

    // mouseup → stopDrag 清态
    fire('mouseup', {})
    expect(m.isDragging.value).toBe(false)

    // stopDrag 后 translate 不再随 mousemove 变(监听已摘)
    fire('mousemove', { clientX: 300, clientY: 300 })
    expect(m.translateX.value).toBe(50)

    m.cleanup()
  })
})
