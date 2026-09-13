import { describe, it, expect } from 'vitest'
import {
  activeSeparatorAtY,
  scrollVelocity,
  shouldDeferThumbLoad,
  thumbGateThresholds,
  THUMB_LOAD_DEFER_VELOCITY,
  THUMB_LOAD_RELEASE_VELOCITY,
  buildThumbInfoLines,
  isTextCardFormat,
  isTextCardFallback,
  docBadgeKind,
  isRawFormat,
  isRawNoThumb,
  typeBadgeOf,
  needsViewportMeta,
  META_DRIVEN_INFO_ELEMENTS,
  pickReflowAnchor,
  buildViewportLockStyle,
  hasMaterialWidthChange,
  pickRouteReturnViewportLockWidth,
} from './mediaGrid.helpers'
import type { MediaMeta } from '../../types/layout'

describe('查看器返回侧栏几何守卫', () => {
  it('锁住 wrapper 的 width、min/max-width、flex-basis 与可选高度，避免路由 chrome 改写保留帧几何', () => {
    expect(buildViewportLockStyle(960, 640)).toEqual({
      width: '960px',
      minWidth: '960px',
      maxWidth: '960px',
      flex: '0 0 960px',
      height: '640px',
      minHeight: '640px',
      maxHeight: '640px',
    })
  })

  it('未传有效高度时保持原有的仅宽度锁语义', () => {
    expect(buildViewportLockStyle(960)).toEqual({
      width: '960px',
      minWidth: '960px',
      maxWidth: '960px',
      flex: '0 0 960px',
    })
  })

  it('优先使用当前有效 wrapper 宽度，暂时不可测时退回上一帧稳定宽度', () => {
    expect(pickRouteReturnViewportLockWidth(812, 960)).toBe(812)
    expect(pickRouteReturnViewportLockWidth(0, 960)).toBe(960)
    expect(pickRouteReturnViewportLockWidth(0, 0)).toBeNull()
  })

  it('最终宽度未实质改变时不重排，改变时只交给一次最终同步', () => {
    expect(hasMaterialWidthChange(960, 960)).toBe(false)
    expect(hasMaterialWidthChange(960, 960.8)).toBe(false)
    expect(hasMaterialWidthChange(960, 700)).toBe(true)
    expect(hasMaterialWidthChange(960, 0)).toBe(false)
  })
})

// ── 缩略图信息浮窗行组装(DOM MediaThumb 与 Canvas 网格共享单源)────────────────
describe('buildThumbInfoLines', () => {
  const item = { sortDatetime: 1700000000, originalWidth: 4000, originalHeight: 3000 }
  const meta: MediaMeta = {
    id: 1,
    fileName: 'IMG_0001.jpg',
    dirPath: 'D:/photos/2026',
    gpsLat: 31.2304,
    gpsLng: 121.4737,
    exifMake: 'SONY',
    exifModel: 'ILCE-7M4',
    exifLens: null,
    exifFocalLength: 35,
    exifAperture: 1.8,
    exifShutter: '1/250',
    exifIso: 100,
  }
  const ALL = ['filename', 'date', 'resolution', 'path', 'geo', 'camera', 'params']

  it('全元素勾选 + meta 齐备 → 7 行,顺序固定', () => {
    const lines = buildThumbInfoLines(item, meta, ALL)
    expect(lines).toHaveLength(7)
    expect(lines[0]).toBe('IMG_0001.jpg')
    expect(lines[2]).toBe('4000 × 3000')
    expect(lines[3]).toBe('D:/photos/2026')
    expect(lines[4]).toBe('31.2304, 121.4737')
    expect(lines[5]).toBe('SONY ILCE-7M4')
    expect(lines[6]).toBe('35mm f/1.8 1/250s ISO100')
  })
  it('按 elements 过滤:只勾 resolution → 单行', () => {
    expect(buildThumbInfoLines(item, meta, ['resolution'])).toEqual(['4000 × 3000'])
  })
  it('meta 未到达(可视区懒加载在途)→ 只出 item 自带的轻量行', () => {
    const lines = buildThumbInfoLines(item, undefined, ALL)
    expect(lines).toEqual([new Date(1700000000 * 1000).toLocaleString(), '4000 × 3000'])
  })
  it('缺失字段逐项跳过:无 GPS/相机 → 对应行缺席', () => {
    const bare: MediaMeta = { ...meta, gpsLat: null, gpsLng: null, exifMake: null, exifModel: null }
    const lines = buildThumbInfoLines(item, bare, ALL)
    // 日期格式可能包含逗号；精确核对保留行，避免把日期误认成 GPS。
    expect(lines).toEqual([
      'IMG_0001.jpg',
      new Date(1700000000 * 1000).toLocaleString(),
      '4000 × 3000',
      'D:/photos/2026',
      '35mm f/1.8 1/250s ISO100',
    ])
    expect(lines).not.toContain('SONY ILCE-7M4')
  })
  it('拍摄参数部分缺失 → 只拼在场项', () => {
    const partial: MediaMeta = { ...meta, exifAperture: null, exifIso: null }
    const lines = buildThumbInfoLines(item, partial, ['params'])
    expect(lines).toEqual(['35mm 1/250s'])
  })
  it('sortDatetime=0(未知日期)→ date 行缺席', () => {
    const lines = buildThumbInfoLines({ ...item, sortDatetime: 0 }, meta, ['date'])
    expect(lines).toEqual([])
  })
})

describe('isTextCardFormat / docBadgeKind', () => {
  it('document + 文本格式 → 文本卡;大小写不敏感', () => {
    expect(isTextCardFormat('document', 'txt')).toBe(true)
    expect(isTextCardFormat('document', 'DOCX')).toBe(true)
  })
  it('pdf/svg/epub 有真实缩略图 → 非文本卡;非 document 类型恒否', () => {
    expect(isTextCardFormat('document', 'pdf')).toBe(false)
    expect(isTextCardFormat('image', 'txt')).toBe(false)
    expect(isTextCardFormat('document', undefined)).toBe(false)
  })
  it('徽章配色档:Office 按品牌归档,未知格式落 generic', () => {
    expect(docBadgeKind('md')).toBe('md')
    expect(docBadgeKind('DOCX')).toBe('word')
    expect(docBadgeKind('ods')).toBe('excel')
    expect(docBadgeKind('pptx')).toBe('ppt')
    expect(docBadgeKind('rtf')).toBe('generic')
    expect(docBadgeKind(undefined)).toBe('generic')
  })
  it('epub 降级(⑤):仅封面盖棺失败(status=2)出文本卡;pending/有封面/其他格式恒否', () => {
    expect(isTextCardFallback('document', 'epub', 2)).toBe(true)
    expect(isTextCardFallback('document', 'EPUB', 2)).toBe(true) // 大小写不敏感
    expect(isTextCardFallback('document', 'epub', 0)).toBe(false) // pending:等封面,保持占位
    expect(isTextCardFallback('document', 'epub', 1)).toBe(false) // 有封面:正常缩略图
    expect(isTextCardFallback('document', 'pdf', 2)).toBe(false) // pdf 前端渲染兜底,不降级
    expect(isTextCardFallback('image', 'epub', 2)).toBe(false) // 非 document 恒否
  })
})

// 与原线性扫描等价:找最后一个 y ≤ targetY 的分隔符(targetY 所在区段)。
describe('typeBadgeOf', () => {
  it('audio 恒出角标(网格内无播放键无时长,角标是唯一类型标识)', () => {
    expect(typeBadgeOf('audio', 'mp3', 1)).toBe('audio')
    expect(typeBadgeOf('audio', undefined, 0)).toBe('audio')
  })

  it('video/photo 不出角标(video 已有播放键+时长两重标识)', () => {
    expect(typeBadgeOf('video', 'mp4', 1)).toBeNull()
    expect(typeBadgeOf('image', 'jpg', 1)).toBeNull()
  })

  it('走真实缩略图的文档才出 DOC(pdf/svg/有封面 epub)', () => {
    expect(typeBadgeOf('document', 'pdf', 1)).toBe('document')
    expect(typeBadgeOf('document', 'svg', 1)).toBe('document')
    // epub 有封面(status≠2)→ 真实缩略图 → 出 DOC
    expect(typeBadgeOf('document', 'epub', 1)).toBe('document')
  })

  it('文本卡文档不出 DOC(纸张卡自带扩展名角标,再叠属重复标记)', () => {
    for (const fmt of ['txt', 'md', 'docx', 'xlsx', 'pptx', 'odt']) {
      expect(typeBadgeOf('document', fmt, 1), fmt).toBeNull()
    }
    // epub 封面盖棺失败(status=2)降级为文本卡 → 同样不出 DOC
    expect(typeBadgeOf('document', 'epub', 2)).toBeNull()
  })

  it('RAW 且无缩略图(status=2)出 RAW 角标;有缩略图/非 RAW 恒否(RAW 半成品体验修复线)', () => {
    expect(typeBadgeOf('image', 'cr2', 2)).toBe('raw')
    expect(typeBadgeOf('image', 'ARW', 2)).toBe('raw') // 大小写不敏感
    expect(typeBadgeOf('image', 'cr2', 1)).toBeNull() // 有缩略图 → 正常照片,不出角标
    expect(typeBadgeOf('image', 'cr2', 0)).toBeNull() // pending → 不出角标
    expect(typeBadgeOf('image', 'jpg', 2)).toBeNull() // 非 RAW 恒否
  })
})

describe('isRawFormat / isRawNoThumb', () => {
  it('RAW 扩展名判定大小写不敏感,非 RAW/undefined 恒否', () => {
    expect(isRawFormat('cr2')).toBe(true)
    expect(isRawFormat('NEF')).toBe(true)
    expect(isRawFormat('dng')).toBe(true)
    expect(isRawFormat('jpg')).toBe(false)
    expect(isRawFormat(undefined)).toBe(false)
  })

  it('isRawNoThumb:image + RAW + status=2 才为真', () => {
    expect(isRawNoThumb('image', 'cr2', 2)).toBe(true)
    expect(isRawNoThumb('image', 'cr2', 1)).toBe(false)
    expect(isRawNoThumb('document', 'cr2', 2)).toBe(false)
    expect(isRawNoThumb('image', 'jpg', 2)).toBe(false)
  })
})

describe('needsViewportMeta', () => {
  it('只有取用 meta 的元素才触发拉取', () => {
    for (const el of META_DRIVEN_INFO_ELEMENTS) {
      expect(needsViewportMeta([el]), el).toBe(true)
    }
  })

  it('纯 item 字段的元素不触发拉取(此前只看总开关致白拉)', () => {
    for (const el of ['status', 'size', 'favorite', 'type', 'date', 'resolution']) {
      expect(needsViewportMeta([el]), el).toBe(false)
    }
    expect(needsViewportMeta([])).toBe(false)
    expect(needsViewportMeta(['size', 'status', 'type'])).toBe(false)
  })

  it('混选中只要有一个要 meta 就拉', () => {
    expect(needsViewportMeta(['size', 'camera'])).toBe(true)
  })

  it('meta 取用面与 buildThumbInfoLines 实际一致(改一处须同步另一处)', () => {
    // 每个 META_DRIVEN 元素单勾时,喂 meta 应能产出行;不喂 meta 则产不出——
    // 以此钉住「该元素确实依赖 meta」,防两处定义漂移。
    const item = { sortDatetime: 0, originalWidth: 0, originalHeight: 0 }
    const meta = {
      fileName: 'a.jpg',
      dirPath: 'D:/x',
      gpsLat: 1,
      gpsLng: 2,
      exifMake: 'Canon',
      exifModel: 'R5',
      exifFocalLength: 50,
    }
    for (const el of META_DRIVEN_INFO_ELEMENTS) {
      expect(buildThumbInfoLines(item, meta as never, [el]), `${el} 有 meta`).not.toEqual([])
      expect(buildThumbInfoLines(item, undefined, [el]), `${el} 无 meta`).toEqual([])
    }
  })
})

describe('activeSeparatorAtY', () => {
  const seps = [
    { y: 0, groupId: 'a' },
    { y: 100, groupId: 'b' },
    { y: 250, groupId: 'c' },
    { y: 250, groupId: 'c2' }, // 同 y 重复:应返回靠后的那个(与线性扫描「后者覆盖」一致)
    { y: 900, groupId: 'd' },
  ]

  it('空数组 → null', () => {
    expect(activeSeparatorAtY([], 500)).toBeNull()
  })

  it('targetY 在首个之前 → null', () => {
    expect(activeSeparatorAtY([{ y: 10 }], 5)).toBeNull()
  })

  it('恰落在某分隔符 y 上 → 该分隔符', () => {
    expect(activeSeparatorAtY(seps, 100)?.groupId).toBe('b')
  })

  it('落在两分隔符之间 → 取靠前(更小 y)的那个', () => {
    expect(activeSeparatorAtY(seps, 200)?.groupId).toBe('b')
    expect(activeSeparatorAtY(seps, 899)?.groupId).toBe('c2')
  })

  it('同 y 重复 → 返回靠后者(与线性扫描 activeSep 覆盖语义一致)', () => {
    expect(activeSeparatorAtY(seps, 250)?.groupId).toBe('c2')
  })

  it('targetY 超过末项 → 末项', () => {
    expect(activeSeparatorAtY(seps, 100000)?.groupId).toBe('d')
  })

  it('与线性扫描逐点对拍(随机 y 序列)', () => {
    const arr = [0, 30, 64, 130, 260, 400, 777, 1200].map((y, i) => ({ y, groupId: `g${i}` }))
    const linear = (target: number) => {
      let ans: (typeof arr)[number] | null = null
      for (const s of arr) {
        if (s.y <= target) ans = s
        else break
      }
      return ans
    }
    for (let t = -10; t <= 1300; t += 7) {
      expect(activeSeparatorAtY(arr, t)?.groupId ?? null).toBe(linear(t)?.groupId ?? null)
    }
  })
})

// B(快滚甩滚低保真):速度闸门数学。慢滚照常出图、快速飞掠推迟加载。
describe('scrollVelocity', () => {
  it('正常采样 → |位移|/间隔(px/ms)', () => {
    expect(scrollVelocity(160, 16)).toBe(10)
    expect(scrollVelocity(8, 16)).toBe(0.5)
  })

  it('负位移(向上滚)取绝对值', () => {
    expect(scrollVelocity(-320, 16)).toBe(20)
  })

  it('dt<=0(同帧多次事件/时钟未推进)→ 0(不可判定,不抑制)', () => {
    expect(scrollVelocity(500, 0)).toBe(0)
    expect(scrollVelocity(500, -5)).toBe(0)
  })

  it('位移为 0 → 0', () => {
    expect(scrollVelocity(0, 16)).toBe(0)
  })
})

describe('shouldDeferThumbLoad(带滞回)', () => {
  it('开闸态:严格高于关闸阈值 → 抑制', () => {
    expect(shouldDeferThumbLoad(THUMB_LOAD_DEFER_VELOCITY + 0.01, false)).toBe(true)
    expect(shouldDeferThumbLoad(20, false)).toBe(true)
  })

  it('开闸态:等于/低于关闸阈值 → 保持放行(慢滚恒出图)', () => {
    expect(shouldDeferThumbLoad(THUMB_LOAD_DEFER_VELOCITY, false)).toBe(false)
    expect(shouldDeferThumbLoad(1, false)).toBe(false)
  })

  it('velocity=0(停稳/不可判定)→ 无论何态一律放行', () => {
    expect(shouldDeferThumbLoad(0, false)).toBe(false)
    expect(shouldDeferThumbLoad(0, true)).toBe(false)
  })

  it('滞回带 [release, engage] 内维持现状(防减速穿越反复翻转)', () => {
    const mid = (THUMB_LOAD_RELEASE_VELOCITY + THUMB_LOAD_DEFER_VELOCITY) / 2
    expect(shouldDeferThumbLoad(mid, true)).toBe(true) // 关闸态带内 → 保持关闸
    expect(shouldDeferThumbLoad(mid, false)).toBe(false) // 开闸态带内 → 保持开闸
  })

  it('关闸态:须降到放行阈值以下才放行(恰在阈值 = 维持关闸)', () => {
    expect(shouldDeferThumbLoad(THUMB_LOAD_RELEASE_VELOCITY, true)).toBe(true)
    expect(shouldDeferThumbLoad(THUMB_LOAD_RELEASE_VELOCITY - 0.01, true)).toBe(false)
  })

  it('减速序列只翻转一次:9 → 2.5 → 2.5 → 1.2 = 关、关、关、开', () => {
    let gate = false
    for (const [v, expected] of [
      [9, true],
      [2.5, true],
      [2.5, true],
      [1.2, false],
    ] as const) {
      gate = shouldDeferThumbLoad(v, gate)
      expect(gate).toBe(expected)
    }
  })

  it('自定义双阈值', () => {
    expect(shouldDeferThumbLoad(5, false, 10, 4)).toBe(false)
    expect(shouldDeferThumbLoad(15, false, 10, 4)).toBe(true)
    expect(shouldDeferThumbLoad(5, true, 10, 4)).toBe(true) // 带内保持
    expect(shouldDeferThumbLoad(3.9, true, 10, 4)).toBe(false)
  })

  it('端到端:慢滚一帧不抑制、快滚一帧抑制', () => {
    expect(shouldDeferThumbLoad(scrollVelocity(10, 16), false)).toBe(false) // ~0.6 px/ms
    expect(shouldDeferThumbLoad(scrollVelocity(240, 16), false)).toBe(true) // ~15 px/ms
  })
})

// 闸门阈值随行高缩放(2026-07-10 真机回归:滚轮式慢速浏览在大格下被误判飞掠出占位墙)。
describe('thumbGateThresholds', () => {
  it('60px 基线维持原妥协(3/1.5),更小行高不再收紧', () => {
    expect(thumbGateThresholds(60)).toEqual({
      engage: THUMB_LOAD_DEFER_VELOCITY,
      release: THUMB_LOAD_DEFER_VELOCITY / 2,
    })
    expect(thumbGateThresholds(40)).toEqual(thumbGateThresholds(60))
  })
  it('行高翻倍阈值翻倍(单位时间入视口格数等比),放行恒为关闸一半', () => {
    const t120 = thumbGateThresholds(120)
    expect(t120.engage).toBeCloseTo(6)
    expect(t120.release).toBeCloseTo(3)
    expect(thumbGateThresholds(240).engage).toBeCloseTo(12)
  })
  it('封顶 6 倍(≥360px):18/9 仍拦得住真·飞掠', () => {
    expect(thumbGateThresholds(360).engage).toBeCloseTo(18)
    expect(thumbGateThresholds(800)).toEqual(thumbGateThresholds(360))
  })
  it('回归钉:滚轮连续拨动的峰值帧速度(2-4 px/ms)在 ≥120px 行高下不再关闸', () => {
    const { engage, release } = thumbGateThresholds(120)
    expect(shouldDeferThumbLoad(4, false, engage, release)).toBe(false) // 拨轮峰值越不过关闸线
    expect(shouldDeferThumbLoad(2.9, true, engage, release)).toBe(false) // 低于放行线即开
    expect(shouldDeferThumbLoad(4, true, engage, release)).toBe(true) // 带内维持(滞回语义)
    expect(shouldDeferThumbLoad(9, false, engage, release)).toBe(true) // 真飞掠仍关
  })
})

// 重排锚点拾取(行高滑块/分组/排序/容器宽度/布局模式重排前捕获视口顶部项)。
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
