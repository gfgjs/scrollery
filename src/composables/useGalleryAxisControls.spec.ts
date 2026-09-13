// useGalleryAxisControls 浅层 characterization spec(方案 analysis/MediaGrid-vue.md 末节补测要求)。
//
// 覆盖面(画廊轴 2026-07-24 裁决红线状态位):
//  - axisVisible 链路判据:timelineVisible / minimapVisible 均须 && ui.axisVisible;
//  - axis_mode 四道防线中的输出面:canShowScrubber → activeAxis 回退 → switchAxisMode 自动展开 →
//    minimapVisible 的 canShowMinimap 门控。
// 不覆盖 DOM/组件挂载(timelineCanvasRef / galleryViewActive 的生命周期钩子)——纯 setup 函数式测试。
//
// 环境:node,无 DOM。uiStore/mediaStore 换最小 reactive fake(参照 useGalleryQuerySync.spec.ts 范式);
// vue-i18n 换 t=>key 的最小 fake(参照 useOcr.spec.ts 范式);useRenderMode 用真实模块——
// localStorage 在 node 下访问即 ReferenceError,其内部 try/catch 已吞并回落 'dom',无需额外 mock。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { effectScope, reactive } from 'vue'

const ui = reactive({
  axisVisible: true,
  axisMode: 'timeline' as 'timeline' | 'minimap',
  setAxisVisible(v: boolean) {
    ui.axisVisible = v
  },
  setAxisMode(v: 'timeline' | 'minimap') {
    ui.axisMode = v
  },
})

const media = reactive<{
  totalRows: number
  layoutSummary: { monthBuckets?: unknown[]; separators?: unknown[] } | null
}>({
  totalRows: 1,
  layoutSummary: { monthBuckets: [1], separators: [] },
})

vi.mock('../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => media }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

import { useGalleryAxisControls } from './useGalleryAxisControls'

function withControls<T>(fn: (c: ReturnType<typeof useGalleryAxisControls>) => T): T {
  const scope = effectScope()
  const result = scope.run(() => fn(useGalleryAxisControls()))
  scope.stop()
  return result as T
}

describe('useGalleryAxisControls:computed 判据(浅层 characterization)', () => {
  beforeEach(() => {
    ui.axisVisible = true
    ui.axisMode = 'timeline'
    media.totalRows = 1
    media.layoutSummary = { monthBuckets: [1], separators: [] }
    // node 无 localStorage:被测文件顶层 showRenderModeDebug 在 import.meta.env.DEV=true 的
    // vitest 构建下直接读 localStorage.getItem(不经 useRenderMode 的 try/catch),故需最小 stub。
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => {},
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('canShowScrubber / canShowMinimap', () => {
    it('totalRows>0 且 monthBuckets 非空 → canShowScrubber true', () => {
      withControls((c) => expect(c.canShowScrubber.value).toBe(true))
    })

    it('totalRows>0 且 monthBuckets 空但 separators 非空 → canShowScrubber true', () => {
      media.layoutSummary = { monthBuckets: [], separators: [1] }
      withControls((c) => expect(c.canShowScrubber.value).toBe(true))
    })

    it('totalRows=0 → canShowScrubber false(即便 monthBuckets 非空)', () => {
      media.totalRows = 0
      withControls((c) => expect(c.canShowScrubber.value).toBe(false))
    })

    it('monthBuckets 与 separators 均空 → canShowScrubber false', () => {
      media.layoutSummary = { monthBuckets: [], separators: [] }
      withControls((c) => expect(c.canShowScrubber.value).toBe(false))
    })

    it('layoutSummary 为 null → canShowScrubber false(可选链回退空数组)', () => {
      media.layoutSummary = null
      withControls((c) => expect(c.canShowScrubber.value).toBe(false))
    })

    it('canShowMinimap 只看 totalRows>0,与 layoutSummary 内容无关', () => {
      media.layoutSummary = null
      withControls((c) => expect(c.canShowMinimap.value).toBe(true))
      media.totalRows = 0
      withControls((c) => expect(c.canShowMinimap.value).toBe(false))
    })
  })

  describe('activeAxis 回退(axis_mode 四道防线之一)', () => {
    it('axisMode=timeline 且 canShowScrubber → activeAxis=timeline', () => {
      withControls((c) => expect(c.activeAxis.value).toBe('timeline'))
    })

    it('axisMode=timeline 但 canShowScrubber=false → 回退 minimap', () => {
      media.totalRows = 0
      withControls((c) => expect(c.activeAxis.value).toBe('minimap'))
    })

    it('axisMode=minimap → activeAxis=minimap(与 canShowScrubber 无关)', () => {
      ui.axisMode = 'minimap'
      withControls((c) => expect(c.activeAxis.value).toBe('minimap'))
    })
  })

  describe('axisVisible 链路(timelineVisible / minimapVisible 均须 && ui.axisVisible)', () => {
    it('timeline 激活 + axisVisible=true → timelineVisible true, minimapVisible false', () => {
      withControls((c) => {
        expect(c.timelineVisible.value).toBe(true)
        expect(c.minimapVisible.value).toBe(false)
      })
    })

    it('timeline 激活但 axisVisible=false → timelineVisible 随之 false', () => {
      ui.axisVisible = false
      withControls((c) => expect(c.timelineVisible.value).toBe(false))
    })

    it('minimap 激活 + axisVisible=true → minimapVisible true, timelineVisible false', () => {
      ui.axisMode = 'minimap'
      withControls((c) => {
        expect(c.minimapVisible.value).toBe(true)
        expect(c.timelineVisible.value).toBe(false)
      })
    })

    it('minimap 激活但 axisVisible=false → minimapVisible 随之 false', () => {
      ui.axisMode = 'minimap'
      ui.axisVisible = false
      withControls((c) => expect(c.minimapVisible.value).toBe(false))
    })

    it('axisOpen 直接透传 ui.axisVisible', () => {
      ui.axisVisible = false
      withControls((c) => expect(c.axisOpen.value).toBe(false))
    })

    it('toggleAxis 翻转 ui.axisVisible', () => {
      withControls((c) => c.toggleAxis())
      expect(ui.axisVisible).toBe(false)
    })
  })

  describe('hasTimeBuckets', () => {
    it('monthBuckets 非空 → true', () => {
      withControls((c) => expect(c.hasTimeBuckets.value).toBe(true))
    })

    it('monthBuckets 空(即便 separators 非空)→ false', () => {
      media.layoutSummary = { monthBuckets: [], separators: [1] }
      withControls((c) => expect(c.hasTimeBuckets.value).toBe(false))
    })
  })

  describe('switchAxisMode(axis_mode 四道防线之一:自动展开)', () => {
    it('从 timeline 切到 minimap,axisVisible 原本为 true → 保持 true(不误触发 setAxisVisible)', () => {
      withControls((c) => c.switchAxisMode())
      expect(ui.axisMode).toBe('minimap')
      expect(ui.axisVisible).toBe(true)
    })

    it('从 timeline 切到 minimap,axisVisible 原本为 false → 自动展开为 true', () => {
      ui.axisVisible = false
      withControls((c) => c.switchAxisMode())
      expect(ui.axisMode).toBe('minimap')
      expect(ui.axisVisible).toBe(true)
    })

    it('从 minimap 切到 timeline,axisVisible 原本为 false → 自动展开为 true', () => {
      ui.axisMode = 'minimap'
      ui.axisVisible = false
      withControls((c) => c.switchAxisMode())
      expect(ui.axisMode).toBe('timeline')
      expect(ui.axisVisible).toBe(true)
    })
  })
})
