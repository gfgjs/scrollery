// useGalleryVirtualEngine 浅层 characterization spec(方案 analysis/MediaGrid-vue.md 末节补测要求)。
//
// 覆盖面:只钉四条派生读数/透传面——
//  - bucketActive:全场唯一 computed,由 ui.bucketSegmentedScroll && media.totalRows > 0 决定;
//  - canvasCapable:!isIOSLike && media.totalHeight <= resolveSafeMax() 的判据分支。
//  - canvasActive:渲染偏好 + canvasCapable + 非空视图的合成判据(§4.3 S3 取行窗的开关源);
//  - 两引擎均收到 canvasMode 透传 getter,且运行时切偏好即翻(引擎据它即时重取)。
// 不覆盖引擎切换 watcher 的副作用(滚动位回设)——那是 DOM 副作用,浅层测试按计划只钉 computed 输出。
//
// 环境:node,无 DOM。useGalleryLayoutSource / useVirtualScroll / useBucketVirtualScroll / useSelection
// 均触发 IPC 或依赖组件生命周期,与本测试目标(纯 computed 判据)无关,故整体 mock 为最小 fake——
// 唯独 useVirtualScroll 的 resolveSafeMax 保留真实实现(canvasCapable 的判据边界就是它)。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { effectScope, reactive, ref } from 'vue'

/// 两引擎的构造选项捕获(断言 canvasMode 透传):工厂被 mock,故记录 opts 即可。
const engineOpts = vi.hoisted(() => ({ virtual: null, bucket: null })) as {
  virtual: { canvasMode?: () => boolean } | null
  bucket: { canvasMode?: () => boolean } | null
}
/// 渲染偏好(受控):真实 useRenderMode 是模块级单例 + localStorage 持久,测试用可切 ref 替代。
/// ref 在 mock 工厂内创建后存回此处,便于用例翻转并断言响应性。
const renderMode = vi.hoisted(() => ({
  gallery: null as { value: 'dom' | 'canvas' } | null,
}))

const ui = reactive({
  bucketSegmentedScroll: true,
  gridRowHeight: 200,
})
const media = reactive({
  totalRows: 1,
  totalHeight: 100,
  layoutVersion: 0,
  fetchBucketRows: vi.fn(async () => []),
})

vi.mock('../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => media }))

vi.mock('./galleryLayoutSource', () => ({
  useGalleryLayoutSource: () => ({
    totalHeight: () => media.totalHeight,
    totalRows: () => media.totalRows,
    fetchRowsByY: async () => [],
    recompute: vi.fn(async () => {}),
    onResize: vi.fn(),
    cancelPendingResize: vi.fn(),
    flushIfDeferred: vi.fn(async () => false),
  }),
}))

vi.mock('./useVirtualScroll', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./useVirtualScroll')>()
  return {
    ...actual,
    // resolveSafeMax 保留真实实现——被测判据边界所在,不可 mock。
    useVirtualScroll: (opts: { canvasMode?: () => boolean }) => {
      engineOpts.virtual = opts
      return {
        visibleRows: ref([]),
        updateVisible: vi.fn(),
        onScroll: vi.fn(),
        spacerHeight: ref(0),
        renderAnchor: ref(null),
        logicalScrollTop: ref(0),
        isTranslated: ref(false),
        logicalToPhysical: (y: number) => y,
        scrollToTop: vi.fn(),
      }
    },
  }
})

vi.mock('./useBucketVirtualScroll', () => ({
  useBucketVirtualScroll: (opts: { canvasMode?: () => boolean }) => {
    engineOpts.bucket = opts
    return {
      segments: ref([]),
      logicalScrollTop: ref(0),
      anchorDelta: ref(0),
      spacerHeight: ref(0),
      onScroll: vi.fn(),
      onWheel: vi.fn(),
      onKeydown: vi.fn(),
      onTouchmove: vi.fn(),
      scrollToLogicalY: vi.fn(async () => {}),
      mountedRows: () => [],
      whenSettled: vi.fn(async () => {}),
    }
  },
}))

vi.mock('./useRenderMode', async () => {
  const { ref } = await import('vue')
  const galleryRenderMode = ref<'dom' | 'canvas'>('dom')
  renderMode.gallery = galleryRenderMode
  return { useRenderMode: () => ({ galleryRenderMode }) }
})

vi.mock('./useSelection', () => ({
  useSelection: () => ({ selectionEpoch: ref(0) }),
}))

import { useGalleryVirtualEngine } from './useGalleryVirtualEngine'
import { resolveSafeMax } from './useVirtualScroll'

/** 无 DOM 依赖的最小 deps:gridRef/layerRef 恒 null,onScreen 恒真。 */
function baseDeps() {
  return {
    gridRef: () => null,
    layerRef: () => null,
    onScreen: () => true,
  }
}

/** 在独立 effectScope 内跑 composable,便于逐用例复位、避免 watcher 跨用例泄漏。 */
function withEngine<T>(fn: (engine: ReturnType<typeof useGalleryVirtualEngine>) => T): T {
  const scope = effectScope()
  const result = scope.run(() => fn(useGalleryVirtualEngine(baseDeps())))
  scope.stop()
  return result as T
}

describe('useGalleryVirtualEngine:computed 判据(浅层 characterization)', () => {
  beforeEach(() => {
    ui.bucketSegmentedScroll = true
    media.totalRows = 1
    media.totalHeight = 100
    if (renderMode.gallery) renderMode.gallery.value = 'dom'
    // 默认非 iOS UA(桌面 Chrome),canvasCapable 分支测试各自覆盖。
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      platform: 'Win32',
      maxTouchPoints: 0,
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('bucketActive', () => {
    it('开关开 + totalRows > 0 → true', () => {
      withEngine((e) => expect(e.bucketActive.value).toBe(true))
    })

    it('开关关 → false(即便 totalRows > 0)', () => {
      ui.bucketSegmentedScroll = false
      withEngine((e) => expect(e.bucketActive.value).toBe(false))
    })

    it('totalRows = 0 → false(即便开关开)', () => {
      media.totalRows = 0
      withEngine((e) => expect(e.bucketActive.value).toBe(false))
    })
  })

  describe('canvasActive:取行窗开关(§4.3 S3)', () => {
    it('偏好 dom → false(即便能力满足)', () => {
      withEngine((e) => expect(e.canvasActive.value).toBe(false))
    })

    it('偏好 canvas + 能力满足 + 非空视图 → true', () => {
      if (renderMode.gallery) renderMode.gallery.value = 'canvas'
      withEngine((e) => expect(e.canvasActive.value).toBe(true))
    })

    it('偏好 canvas 但 totalRows = 0 → false(空视图不抬窗)', () => {
      if (renderMode.gallery) renderMode.gallery.value = 'canvas'
      media.totalRows = 0
      withEngine((e) => expect(e.canvasActive.value).toBe(false))
    })

    it('偏好 canvas 但超出 SAFE_MAX(超长库回退 DOM)→ false', () => {
      if (renderMode.gallery) renderMode.gallery.value = 'canvas'
      media.totalHeight = resolveSafeMax() + 1
      withEngine((e) => expect(e.canvasActive.value).toBe(false))
    })

    it('两引擎都收到 canvasMode 透传 getter,且随偏好翻转(运行时切换即重取之源)', () => {
      withEngine((e) => {
        expect(e.canvasActive.value).toBe(false)
        expect(engineOpts.virtual?.canvasMode?.()).toBe(false)
        expect(engineOpts.bucket?.canvasMode?.()).toBe(false)
        if (renderMode.gallery) renderMode.gallery.value = 'canvas'
        expect(e.canvasActive.value).toBe(true)
        expect(engineOpts.virtual?.canvasMode?.()).toBe(true)
        expect(engineOpts.bucket?.canvasMode?.()).toBe(true)
      })
    })
  })

  describe('canvasCapable', () => {
    it('非 iOS + totalHeight ≤ SAFE_MAX → true', () => {
      media.totalHeight = resolveSafeMax()
      withEngine((e) => expect(e.canvasCapable.value).toBe(true))
    })

    it('非 iOS + totalHeight > SAFE_MAX → false', () => {
      media.totalHeight = resolveSafeMax() + 1
      withEngine((e) => expect(e.canvasCapable.value).toBe(false))
    })

    it('iPad UA → false(即便高度在安全范围内)', () => {
      vi.stubGlobal('navigator', {
        userAgent: 'Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)',
        platform: 'iPad',
        maxTouchPoints: 5,
      })
      media.totalHeight = 100
      withEngine((e) => expect(e.canvasCapable.value).toBe(false))
    })

    it('MacIntel + maxTouchPoints > 1(触屏 Mac,iPadOS 桌面模式伪装)→ false', () => {
      vi.stubGlobal('navigator', {
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
        platform: 'MacIntel',
        maxTouchPoints: 5,
      })
      media.totalHeight = 100
      withEngine((e) => expect(e.canvasCapable.value).toBe(false))
    })

    it('MacIntel + maxTouchPoints = 0(真桌面 Mac,非触屏)→ 不落 iOS 分支', () => {
      vi.stubGlobal('navigator', {
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
        platform: 'MacIntel',
        maxTouchPoints: 0,
      })
      media.totalHeight = 100
      withEngine((e) => expect(e.canvasCapable.value).toBe(true))
    })
  })
})
