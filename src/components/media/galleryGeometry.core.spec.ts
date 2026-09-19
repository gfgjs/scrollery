// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect } from 'vitest'
import { hitTestCell, hitTestCellWithRow, visibleRowRange, coverRect, bitmapBucketH, bitmapPrepParams, computeHoverRect } from './mediaGridCanvas.helpers'
import { type LayoutRow, type LayoutRowItem } from '../../types/layout'
import { activeSeparatorAtY, shouldDeferThumbLoad, thumbGateThresholds, THUMB_LOAD_DEFER_VELOCITY, THUMB_LOAD_RELEASE_VELOCITY, pickReflowAnchor } from './mediaGrid.helpers'
import { thumbGeometry, thumbTopToLogicalY, MIN_THUMB_PX } from './mediaScrollbar.helpers'
import { minimapScale, minimapWindow, minimapSlider, sliderTopToLogicalY, clickToLogicalY, keyboardToLogicalY, MIN_SLIDER_PX, wheelToLogicalY } from './minimapAxis.helpers'
import { normalizedToSelection, selectionToNormalized } from './cropperCoordinates'
import { nearestSeparatorIndex, buildTimeBand, logicalYToTimeFrac } from './timelineScrubber.helpers'

describe('网格命中与位图几何', () => {
  /** 构造一个正常行的最小 item(只填命中相关字段)。 */
  function item(id: number, x: number, w: number): LayoutRowItem {
    return {
      id,
      x,
      w,
      h: 60,
      fileSize: 0,
      fileFormat: '',
      mediaType: 'image',
      isLivePhoto: false,
      durationMs: null,
      thumbStatus: 1,
      thumbPath: null,
      placeholderColor: null,
      isFavorited: false,
      rating: 0,
      colorLabel: 0,
      availability: 'online',
      originalWidth: 0,
      originalHeight: 0,
      sortDatetime: 0,
    }
  }

  const rows: LayoutRow[] = [
    { rowType: 'separator', y: 0, height: 24, separatorLabel: '2026-07' },
    // 行内两格,格间有 4px 空隙(x=0..90, gap, x=94..184)
    { rowType: 'normal', y: 24, height: 60, items: [item(10, 0, 90), item(11, 94, 90)] },
    { rowType: 'normal', y: 84, height: 60, items: [item(20, 0, 120)] },
  ]

  describe('hitTestCell', () => {
    it('落在格间空隙 → null', () => {
      expect(hitTestCell(rows, 92, 30)).toBeNull()
    })

    it('命中分隔符行 → null(无可选项)', () => {
      expect(hitTestCell(rows, 10, 5)).toBeNull()
    })

    it('y 越界(下方空白)→ null', () => {
      expect(hitTestCell(rows, 10, 500)).toBeNull()
    })

    it('x 越界(行右侧空白)→ null', () => {
      expect(hitTestCell(rows, 200, 30)).toBeNull()
    })

    it('大量有序行使用二分命中，不退化为从首行扫描', () => {
      const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
        rowType: 'normal' as const,
        y: i * 60,
        height: 60,
        items: [item(i, 0, 50)],
      }))
      let numericReads = 0
      const observed = new Proxy(manyRows, {
        get(target, prop, receiver) {
          if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
          return Reflect.get(target, prop, receiver)
        },
      })
      expect(hitTestCell(observed, 10, 7000 * 60 + 10)?.id).toBe(7000)
      expect(numericReads).toBeLessThan(50)
    })
  })

  describe('visibleRowRange', () => {
    it('返回与视口相交的半开行区间，并保留贴边行', () => {
      expect(visibleRowRange(rows, 24, 84)).toEqual({ start: 0, end: 3 })
      expect(visibleRowRange(rows, 25, 83)).toEqual({ start: 1, end: 2 })
    })

    it('空输入、反向范围与完全越界均返回空区间', () => {
      expect(visibleRowRange([], 0, 100)).toEqual({ start: 0, end: 0 })
      expect(visibleRowRange(rows, 100, 50)).toEqual({ start: 0, end: 0 })
      expect(visibleRowRange(rows, 500, 600)).toEqual({ start: rows.length, end: rows.length })
    })

    it('大量有序行只读取对数数量的行', () => {
      const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
        rowType: 'normal' as const,
        y: i * 60,
        height: 60,
        items: [item(i, 0, 50)],
      }))
      let numericReads = 0
      const observed = new Proxy(manyRows, {
        get(target, prop, receiver) {
          if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
          return Reflect.get(target, prop, receiver)
        },
      })
      expect(visibleRowRange(observed, 7000 * 60 + 1, 7002 * 60 - 1)).toEqual({
        start: 7000,
        end: 7002,
      })
      expect(numericReads).toBeLessThan(50)
    })
  })

  describe('coverRect', () => {
    it('横图铺正方形 → 裁两侧,高不裁', () => {
      const r = coverRect(200, 100, 50, 50) // scale=max(0.25,0.5)=0.5 → sw=100,sh=200? no
      // w/nw=0.25, h/nh=0.5 → scale=0.5 → sw=50/0.5=100, sh=50/0.5=100
      expect(r.sh).toBeCloseTo(100)
      expect(r.sw).toBeCloseTo(100)
      expect(r.sx).toBeCloseTo(50) // (200-100)/2
      expect(r.sy).toBeCloseTo(0)
    })

    it('竖图铺正方形 → 裁上下', () => {
      const r = coverRect(100, 200, 50, 50)
      expect(r.sx).toBeCloseTo(0)
      expect(r.sy).toBeCloseTo(50)
    })

    it('零/负尺寸 → 退化不裁剪,不产生 NaN', () => {
      const r = coverRect(0, 100, 50, 50)
      expect(Number.isNaN(r.sw)).toBe(false)
      expect(r).toEqual({ sx: 0, sy: 0, sw: 0, sh: 100 })
    })
  })

  describe('bitmapBucketH', () => {
    it('恰在桶边界 → 落本桶(<= 语义)', () => {
      expect(bitmapBucketH(96, 1)).toBe(96)
      expect(bitmapBucketH(64, 2)).toBe(128)
    })

    it('超顶桶(源上限 480)→ 取顶桶', () => {
      expect(bitmapBucketH(500, 2)).toBe(480)
      expect(bitmapBucketH(480, 1)).toBe(480)
    })
  })

  describe('bitmapPrepParams', () => {
    it('裁剪后已 ≤ 桶高 → 只裁不缩(outW/outH = null,只缩不放)', () => {
      const p = bitmapPrepParams(120, 80, 100, 100, 128)
      expect(p.outW).toBeNull()
      expect(p.outH).toBeNull()
      expect({ sx: p.sx, sy: p.sy, sw: p.sw, sh: p.sh }).toEqual({ sx: 20, sy: 0, sw: 80, sh: 80 })
    })

    it('裁剪矩形整数化且钳位在源界内(createImageBitmap 参数为 long)', () => {
      const p = bitmapPrepParams(333, 217, 90, 61, 96) // 刻意取不整除组合
      expect(Number.isInteger(p.sx) && Number.isInteger(p.sy)).toBe(true)
      expect(Number.isInteger(p.sw) && Number.isInteger(p.sh)).toBe(true)
      expect(p.sx + p.sw).toBeLessThanOrEqual(333)
      expect(p.sy + p.sh).toBeLessThanOrEqual(217)
      expect(p.sw).toBeGreaterThan(0)
      expect(p.sh).toBeGreaterThan(0)
    })

    it('格高 0(退化)→ 只裁不缩,不产生 NaN', () => {
      const p = bitmapPrepParams(480, 320, 100, 0, 128)
      expect(p.outW).toBeNull()
      expect(p.outH).toBeNull()
      expect(Number.isNaN(p.sw)).toBe(false)
    })
  })

  describe('hitTestCellWithRow', () => {
    it('命中返回项 + 所在行逻辑 y(悬停卡定位用)', () => {
      const hit = hitTestCellWithRow(rows, 10, 90)
      expect(hit?.item.id).toBe(20)
      expect(hit?.rowY).toBe(84)
    })

    it('未命中(空隙/分隔符/越界)→ null,与 hitTestCell 一致', () => {
      expect(hitTestCellWithRow(rows, 92, 30)).toBeNull()
      expect(hitTestCellWithRow(rows, 10, 5)).toBeNull()
      expect(hitTestCellWithRow(rows, 10, 500)).toBeNull()
    })
  })

  describe('computeHoverRect', () => {
    it('原位态(k=1)不钳位:贴视口缘的格保持在原格上(DOM 模式 hover 从不挪格)', () => {
      // 首行滚出视口顶 → 旧实现会把卡推到 y=0,卡与画格错位 = 关闭放大仍位移的几何来源。
      // 未缩放态逐坐标复刻画格(卡下即 canvas 同格位图),任何钳位平移都是可见位移。
      const top = computeHoverRect({ x: 100, y: -30, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
      expect(top).toEqual({ x: 100, y: -30, w: 200, h: 200, scale0: 1, originX: 100, originY: 100 })
      // 尾行探出视口底同理(旧实现 y 被钳到 viewH-h,整格上移)。
      const bottom = computeHoverRect({ x: 100, y: 550, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
      expect(bottom).toEqual({ x: 100, y: 550, w: 200, h: 200, scale0: 1, originX: 100, originY: 100 })
      // 左/右缘越界同理:钳位只在放大态(k>1)生效,未缩放态 x 不得被推回视口。
      const left = computeHoverRect({ x: -40, y: 100, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
      expect(left).toEqual({ x: -40, y: 100, w: 200, h: 200, scale0: 1, originX: 100, originY: 100 })
      const right = computeHoverRect({ x: 740, y: 100, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
      expect(right.x).toBe(740)
      expect(right.x + right.w).toBe(940) // 越过视口右缘也不内推
    })

    it('贴边格:视口内推挤钳位,基点偏离卡中心(动画起点仍与画格重合)', () => {
      const r = computeHoverRect({ x: 0, y: 0, w: 60, h: 60 }, 1920, 1080)
      expect(r.x).toBeGreaterThanOrEqual(0)
      expect(r.y).toBeGreaterThanOrEqual(0)
      expect(r.originX).toBeCloseTo(30 - r.x) // 格中心 30 − 钳位后矩形左缘
      expect(r.originY).toBeCloseTo(30 - r.y)
      expect(r.originX).not.toBeCloseTo(r.w / 2) // 钳位后确实偏离卡中心
      const r2 = computeHoverRect({ x: 1860, y: 1020, w: 60, h: 60 }, 1920, 1080)
      expect(r2.x + r2.w).toBeLessThanOrEqual(1920)
      expect(r2.y + r2.h).toBeLessThanOrEqual(1080)
    })
  })
})

describe('滚动与重排锚点', () => {
  describe('activeSeparatorAtY', () => {
    const seps = [
      { y: 0, groupId: 'a' },
      { y: 100, groupId: 'b' },
      { y: 250, groupId: 'c' },
      { y: 250, groupId: 'c2' }, // 同 y 重复:应返回靠后的那个(与线性扫描「后者覆盖」一致)
      { y: 900, groupId: 'd' },
    ]

    it('同 y 重复 → 返回靠后者(与线性扫描 activeSep 覆盖语义一致)', () => {
      expect(activeSeparatorAtY(seps, 250)?.groupId).toBe('c2')
    })
  })

  describe('shouldDeferThumbLoad(带滞回)', () => {
    it('滞回带 [release, engage] 内维持现状(防减速穿越反复翻转)', () => {
      const mid = (THUMB_LOAD_RELEASE_VELOCITY + THUMB_LOAD_DEFER_VELOCITY) / 2
      expect(shouldDeferThumbLoad(mid, true)).toBe(true) // 关闸态带内 → 保持关闸
      expect(shouldDeferThumbLoad(mid, false)).toBe(false) // 开闸态带内 → 保持开闸
    })

    it('关闸态:须降到放行阈值以下才放行(恰在阈值 = 维持关闸)', () => {
      expect(shouldDeferThumbLoad(THUMB_LOAD_RELEASE_VELOCITY, true)).toBe(true)
      expect(shouldDeferThumbLoad(THUMB_LOAD_RELEASE_VELOCITY - 0.01, true)).toBe(false)
    })
  })

  describe('thumbGateThresholds', () => {
    it('回归钉:滚轮连续拨动的峰值帧速度(2-4 px/ms)在 ≥120px 行高下不再关闸', () => {
      const { engage, release } = thumbGateThresholds(120)
      expect(shouldDeferThumbLoad(4, false, engage, release)).toBe(false) // 拨轮峰值越不过关闸线
      expect(shouldDeferThumbLoad(2.9, true, engage, release)).toBe(false) // 低于放行线即开
      expect(shouldDeferThumbLoad(4, true, engage, release)).toBe(true) // 带内维持(滞回语义)
      expect(shouldDeferThumbLoad(9, false, engage, release)).toBe(true) // 真飞掠仍关
    })
  })

  describe('pickReflowAnchor', () => {
    const row = (y: number, height: number, ids: number[], rowType = 'normal') => ({
      rowType,
      y,
      height,
      items: ids.map((id) => ({ id })),
    })

    it('顶部特判:vTop ≤ 0 不押锚(排序翻转时钉首项会把顶部浏览者甩到另一端)', () => {
      const rows = [row(0, 200, [1, 2])]
      expect(pickReflowAnchor(rows, 0)).toBeNull()
      expect(pickReflowAnchor(rows, -5)).toBeNull()
    })

    it('取首个与视口顶相交的 normal 行首项;偏移 = row.y - vTop(行顶已滚出为负)', () => {
      const rows = [row(0, 200, [1, 2]), row(204, 200, [3, 4]), row(408, 200, [5])]
      // vTop=250:首行(0-200)已整体滚出,第二行(204-404)与视口顶相交。
      expect(pickReflowAnchor(rows, 250)).toEqual({ id: 3, screenOffset: -46 })
      // vTop=100:首行仍占视口顶,偏移为负(行顶在视口上方 100px)。
      expect(pickReflowAnchor(rows, 100)).toEqual({ id: 1, screenOffset: -100 })
    })

    it('跳过 separator 行与空 items 行', () => {
      const rows = [
        row(100, 40, [], 'separator'),
        row(140, 200, []),
        row(344, 200, [7, 8]),
      ]
      expect(pickReflowAnchor(rows, 120)).toEqual({ id: 7, screenOffset: 224 })
    })

    it('无行/全部行已滚出 → null(走 scrollCache 兜底)', () => {
      expect(pickReflowAnchor([], 100)).toBeNull()
      expect(pickReflowAnchor([row(0, 50, [1])], 100)).toBeNull()
    })
  })
})

describe('逻辑滚动条', () => {
  describe('mediaScrollbar.helpers', () => {
    it('内容不足一屏/轨道无效 → null(含 NaN 防御)', () => {
      expect(thumbGeometry(0, 500, 1000)).toBeNull()
      expect(thumbGeometry(0, 1000, 1000)).toBeNull()
      expect(thumbGeometry(0, 1000, 0)).toBeNull()
      expect(thumbGeometry(0, NaN, 1000)).toBeNull()
    })

    it('百万级库:拇指钳到最小高,位置仍为行程比例(中点居中)', () => {
      const total = 30_000_000
      const trackH = 1000
      const mid = (total - trackH) / 2
      const g = thumbGeometry(mid, total, trackH)!
      expect(g.height).toBe(MIN_THUMB_PX) // 纯比例高 0.03px → 钳制
      expect(g.top).toBeCloseTo((trackH - MIN_THUMB_PX) / 2, 6)
    })

    it('拖拽映射与拇指几何互逆(round-trip,含钳高形态)', () => {
      const total = 30_000_000
      const trackH = 900
      for (const y of [0, 123_456, 15_000_000, total - trackH]) {
        const g = thumbGeometry(y, total, trackH)!
        expect(thumbTopToLogicalY(g.top, total, trackH, g.height)).toBeCloseTo(y, 4)
      }
    })

    it('拖拽映射:越界钳制与退化(拇指占满轨道 → 恒 0)', () => {
      expect(thumbTopToLogicalY(-50, 30_000_000, 900, 32)).toBe(0)
      expect(thumbTopToLogicalY(1e9, 30_000_000, 900, 32)).toBe(30_000_000 - 900)
      expect(thumbTopToLogicalY(10, 2000, 1000, 1000)).toBe(0)
    })
  })
})

describe('缩略图轴导航', () => {
  // 公共几何:轴宽 96 / 内容宽 1200 → scale 0.08;轨道 900;视口 1000。
  const SCALE = minimapScale(96, 1200)

  const TRACK = 900

  const VIEW = 1000

  describe('minimapAxis.helpers', () => {
    it('拖拽映射互逆:两形态 round-trip(未钳高)', () => {
      for (const [total, y] of [
        [5000, 0],
        [5000, 2000],
        [5000, 4000], // 非滑动含端点
        [100_000, 0],
        [100_000, 50_000],
        [100_000, 99_000], // 滑动含端点
      ]) {
        const g = minimapSlider(y, total, VIEW, TRACK, SCALE)!
        expect(sliderTopToLogicalY(g.top, total, VIEW, TRACK, SCALE)).toBeCloseTo(y, 4)
      }
    })

    it('拖拽映射:越界钳制 + 钳高形态端点仍可达全程', () => {
      expect(sliderTopToLogicalY(-50, 100_000, VIEW, TRACK, SCALE)).toBe(0)
      expect(sliderTopToLogicalY(1e9, 100_000, VIEW, TRACK, SCALE)).toBe(100_000 - VIEW)
      // 钳高深库:拖到轨道底(trackH − 钳制框高)逆映射经钳制到达可滚动量末端
      const s = minimapScale(96, 150_000)
      const total = 30_000_000
      expect(sliderTopToLogicalY(TRACK - MIN_SLIDER_PX, total, VIEW, TRACK, s)).toBe(total - VIEW)
      // 无效/无行程 → 0
      expect(sliderTopToLogicalY(100, 800, VIEW, TRACK, SCALE)).toBe(0)
      expect(sliderTopToLogicalY(100, 100_000, VIEW, TRACK, 0)).toBe(0)
    })

    it('点击:点击处内容居中,端点钳制', () => {
      // 非滑动:点击 160px → 内容 y 2000 → 目标 2000 − 500 = 1500
      const w = minimapWindow(0, 5000, VIEW, TRACK, SCALE)!
      expect(clickToLogicalY(160, w, SCALE, VIEW, 5000)).toBeCloseTo(1500, 8)
      expect(clickToLogicalY(0, w, SCALE, VIEW, 5000)).toBe(0) // −500 钳到 0
      expect(clickToLogicalY(TRACK, w, SCALE, VIEW, 5000)).toBe(4000) // 超底钳到 maxScroll
      // 滑动形态:窗顶参与换算
      const total = 100_000
      const w2 = minimapWindow(50_000, total, VIEW, TRACK, SCALE)!
      const mid = clickToLogicalY(TRACK / 2, w2, SCALE, VIEW, total)
      expect(mid).toBeCloseTo(w2.topY + TRACK / 2 / SCALE - VIEW / 2, 6)
    })

    it('滚轮:同帧多次输入可在待提交位置上连续累加,并钳制两端', () => {
      const total = 5000
      const first = wheelToLogicalY(100, 3, 1, total, VIEW)
      expect(first).toBe(148)
      expect(wheelToLogicalY(first, 3, 1, total, VIEW)).toBe(196)
      expect(wheelToLogicalY(3990, 3, 1, total, VIEW)).toBe(4000)
      expect(wheelToLogicalY(10, -3, 1, total, VIEW)).toBe(0)
    })

    it('键盘:方向键/Page/Home/End 导航并钳制端点', () => {
      const total = 5000
      expect(keyboardToLogicalY('ArrowDown', 100, total, VIEW)).toBe(140)
      expect(keyboardToLogicalY('ArrowUp', 20, total, VIEW)).toBe(0)
      expect(keyboardToLogicalY('PageDown', 100, total, VIEW)).toBe(1100)
      expect(keyboardToLogicalY('PageUp', 100, total, VIEW)).toBe(0)
      expect(keyboardToLogicalY('Home', 2000, total, VIEW)).toBe(0)
      expect(keyboardToLogicalY('End', 0, total, VIEW)).toBe(4000)
      expect(keyboardToLogicalY('Enter', 100, total, VIEW)).toBeNull()
    })
  })
})

describe('裁剪坐标', () => {
  describe('CropperSelectionOverlay 坐标适配', () => {
    it('归一化状态与 Cropper.js 显示坐标可无损往返', () => {
      const normalized = { x: 0.125, y: 0.2, width: 0.5, height: 0.6 }
      const selection = normalizedToSelection(normalized, 1600, 900)
      expect(selection).toEqual({ x: 200, y: 180, width: 800, height: 540 })
      expect(selectionToNormalized(selection, 1600, 900)).toEqual(normalized)
    })
  })
})

describe('时间轴映射', () => {
  describe('nearestSeparatorIndex', () => {
    // 分组边界按逻辑 y 升序：y = 0 / 100 / 300 / 600，totalHeight = 600。
    const seps = [{ y: 0 }, { y: 100 }, { y: 300 }, { y: 600 }]

    it('空数组返回 -1（调用方回退按比例）', () => {
      expect(nearestSeparatorIndex([], 0.5, 600)).toBe(-1)
    })

    it('frac=1 → 末个分组（不越界）', () => {
      expect(nearestSeparatorIndex(seps, 1, 600)).toBe(3)
    })

    it('正中间（等距）偏向更靠前者：targetY=200 → 索引 1', () => {
      expect(nearestSeparatorIndex(seps, 200 / 600, 600)).toBe(1)
    })
  })

  describe('buildTimeBand', () => {
    it('日历空隙:空日行 intensity=0,rowJumpY 在相邻真实日间线性插值(time 坐标命门)', () => {
      // 19802(y=0,c=10) 与 19792(y=300,c=30) 差 10 天;rows=11 → 每天 1 行。
      // rowOf(19802)=0、rowOf(19792)=10;中间 1..9 行无真实日 = 空隙。
      const r = buildTimeBand(
        [
          { y: 0, count: 10, epochDay: 19802 },
          { y: 300, count: 30, epochDay: 19792 },
        ],
        1000,
        11,
      )
      // 归一化 max=30:row0=10/30、row10=1,空隙行全 0。
      expect(r.intensity[0]).toBeCloseTo(1 / 3, 5)
      expect(r.intensity[10]).toBeCloseTo(1, 5)
      expect(r.intensity[5]).toBe(0)
      // rowJumpY:row0=0(最新日),row10=300(最旧日),空隙 row5 线性插值 = 0 + 0.5*300 = 150。
      expect(r.rowJumpY[0]).toBeCloseTo(0, 5)
      expect(r.rowJumpY[10]).toBeCloseTo(300, 5)
      expect(r.rowJumpY[5]).toBeCloseTo(150, 5)
    })

    it('ASC(旧→新)排序:朝向自适应,顶=最旧,rowJumpY 单调不减(评审 R1 回归)', () => {
      // y 升序对应 epochDay 升序(用户点了工具栏「升序」):最旧日在网格顶(y=0)。
      // 旧实现硬钉「顶=最新」→ rowJumpY=[200,100,0] 反向单调,击穿 logicalYToTimeFrac 二分。
      const r = buildTimeBand(
        [
          { y: 0, count: 10, epochDay: 100 },
          { y: 100, count: 20, epochDay: 101 },
          { y: 200, count: 30, epochDay: 102 },
        ],
        1000,
        3,
      )
      expect(Array.from(r.rowJumpY)).toEqual([0, 100, 200]) // 顶=最旧(y=0),与网格同向
      expect(r.intensity[0]).toBeCloseTo(1 / 3, 5)
      expect(r.intensity[1]).toBeCloseTo(2 / 3, 5)
      expect(r.intensity[2]).toBeCloseTo(1, 5)
      // 逆映射随之正确:y=50 → 首个 >=50 的行 1 → 1/2。
      expect(logicalYToTimeFrac(r.rowJumpY, 50)).toBe(0.5)
    })

    it('大库常态(天数 > 行数):同行多日去重取该行最靠上一日,rowJumpY 仍单调(此前零覆盖)', () => {
      // 5 天 DESC 压进 3 行:rowOf = round((104-d)/4*2) → d104→0、d103/102→1、d101/100→2。
      const r = buildTimeBand(
        [
          { y: 0, count: 1, epochDay: 104 },
          { y: 10, count: 1, epochDay: 103 },
          { y: 20, count: 1, epochDay: 102 },
          { y: 30, count: 1, epochDay: 101 },
          { y: 40, count: 1, epochDay: 100 },
        ],
        1000,
        3,
      )
      // 每行锚点 = 该行最小 y:row0=0、row1=min(10,20)=10、row2=min(30,40)=30。
      expect(Array.from(r.rowJumpY)).toEqual([0, 10, 30])
      // intensity 按行求和:行0=1、行1=2、行2=2 → 归一化 [0.5, 1, 1]。
      expect(r.intensity[0]).toBeCloseTo(0.5, 5)
      expect(r.intensity[1]).toBeCloseTo(1, 5)
      expect(r.intensity[2]).toBeCloseTo(1, 5)
    })

    it('同日多分隔符(span=0):坍缩为单锚点取最小 y,rowJumpY 恒定不越界', () => {
      const r = buildTimeBand(
        [
          { y: 0, count: 3, epochDay: 19802 },
          { y: 50, count: 4, epochDay: 19802 },
        ],
        1000,
        4,
      )
      expect(Array.from(r.rowJumpY)).toEqual([0, 0, 0, 0])
      expect(r.intensity[0]).toBe(1) // 3+4 同行求和后归一化
      expect(r.monthTicks).toEqual([]) // span=0 不产日历刻度
    })
  })

  describe('logicalYToTimeFrac', () => {
    it('单调 rowJumpY 上二分求行比例(逆映射,指示线定位)', () => {
      // rowJumpY = [0,100,200,300,400](5 行);y=200 → 第 2 行 → 2/4 = 0.5。
      const rj = [0, 100, 200, 300, 400]
      expect(logicalYToTimeFrac(rj, 0)).toBe(0)
      expect(logicalYToTimeFrac(rj, 200)).toBe(0.5)
      expect(logicalYToTimeFrac(rj, 400)).toBe(1)
      // y=150 落在 100 与 200 之间 → 第一个 >=150 的行是 index2 → 0.5。
      expect(logicalYToTimeFrac(rj, 150)).toBe(0.5)
      // 超出上界 → clamp 到末行。
      expect(logicalYToTimeFrac(rj, 999)).toBe(1)
    })
  })
})
