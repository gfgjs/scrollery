// src/composables/useChromeReveal.spec.ts
// chrome 自动隐藏/边缘唤出的判据契约(2026-07-16 新需求「F11 全屏沉浸式画廊」+ 同日真机回归修正)。
//
// 首版把「唤出」写成几何判据(window 级 pointermove 测 clientY)、「收起」写成拓扑判据(宿主的
// mouseleave),二者不可组合 —— 快速掠过唤出带时顶栏滑入到空无一人的位置,浏览器从未派发 enter,
// 于是 leave 永不派发、顶栏卡在展开态(真机原话「移出变显示、移入变隐藏」)。现两侧统一为几何判据。
// 本文件钉的就是这个对称性,以及它顺带消掉的分离模式双条边界。
//
// 真机侧(平移动画、贴边手感、capture 是否真绕过 .tlc-viewport 的 @pointermove.stop)node 环境
// 不可复现,必须真机验收(experience §19)。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

const os = { fullscreen: false, maximized: false }

vi.mock('../utils/appWindow', () => ({
  getAppWindow: () => ({
    async isMaximized() {
      return os.maximized
    },
    async minimize() {},
    async toggleMaximize() {
      os.maximized = !os.maximized
    },
    async maximize() {
      os.maximized = true
    },
    async unmaximize() {
      os.maximized = false
    },
    async close() {},
    async startDragging() {},
    async setTheme() {},
    async isFullscreen() {
      return os.fullscreen
    },
    async setFullscreen(on: boolean) {
      os.fullscreen = on
    },
    async setResizable() {},
  }),
}))

// node 测试环境无 window(vitest.config.ts environment: 'node'),真实 uiStore 在 setup 里读
// window.matchMedia 会直接崩;此处仅 mock 出 chromeAutoHidden 用到的唯一字段
// autoHideChromeWindowed,与 useGalleryQuerySync.spec.ts 同款「reactive 最小 fake」惯用法。
vi.mock('../stores/uiStore', async () => {
  const { reactive: r } = await import('vue')
  const ui = r({ autoHideChromeWindowed: false })
  return { useUiStore: () => ui }
})

import { setFullscreen, isFullscreen, __resetWindowModeForTest } from './useWindowMode'
import { useUiStore } from '../stores/uiStore'
import { useViewerStore, type ActiveViewer } from '../stores/viewerStore'
import {
  CHROME_REVEAL_EDGE_PX,
  CHROME_REVEAL_EDGE_PX_WINDOWED,
  CHROME_SIDE_SELECTOR,
  chromeAutoHidden,
  activeRevealEdgePx,
  shouldCollapseByPointer,
  shouldCollapseOnFocusOut,
  __resetChromeRevealForTest,
} from './useChromeReveal'

/** 最小 ActiveViewer 供测试驱动 isImmersive(populate 需要非 null 的完整对象)。 */
function enterImmersiveViewer(): void {
  const viewer: ActiveViewer = {
    kind: 'image',
    mediaType: 'image',
    fileFormat: 'jpg',
    id: 1,
    path: null,
    title: 'test',
    api: null,
    immersive: true,
    fileInfo: null,
  }
  useViewerStore().populate(viewer)
}

/**
 * 鸭子类型的假节点:node 环境无 DOM。判据本身就按鸭子类型写(见 withinSide 注),故最小对象即可驱动。
 */
function fakeNodeIn(selector: string): Node {
  return { closest: (s: string) => (s === selector ? {} : null) } as unknown as Node
}
const nodeOutside: Node = { closest: () => null } as unknown as Node

const VIEWPORT_H = 900
/** 合并模式(默认):顶侧只有标题栏 40px。 */
const TOP_MERGED = 40
/** 分离模式:顶侧是标题栏 40 + 页面工具栏 48 两条相邻的条。 */
const TOP_SPLIT = 88
const BOTTOM = 28

beforeEach(() => {
  setActivePinia(createPinia())
  os.fullscreen = false
  os.maximized = false
  useUiStore().autoHideChromeWindowed = false
  __resetWindowModeForTest()
  __resetChromeRevealForTest()
})

describe('chromeAutoHidden 的来源', () => {
  it('常规态为假;F11 进全屏即为真,退出即回假', async () => {
    expect(chromeAutoHidden.value).toBe(false)
    await setFullscreen(true)
    expect(chromeAutoHidden.value).toBe(true)
    await setFullscreen(false)
    expect(chromeAutoHidden.value).toBe(false)
  })

  it('窗口化沉浸设置开启、非全屏 → chromeAutoHidden 为真;关闭即回假', () => {
    expect(isFullscreen.value).toBe(false)
    expect(chromeAutoHidden.value).toBe(false)
    useUiStore().autoHideChromeWindowed = true
    expect(chromeAutoHidden.value).toBe(true)
    useUiStore().autoHideChromeWindowed = false
    expect(chromeAutoHidden.value).toBe(false)
  })
})

describe('shouldCollapseByPointer —— 收起与唤出同为几何判据', () => {
  it('顶栏:指针仍在条内不收,越出即收', () => {
    expect(shouldCollapseByPointer('top', 0, TOP_MERGED, VIEWPORT_H)).toBe(false)
    expect(shouldCollapseByPointer('top', 20, TOP_MERGED, VIEWPORT_H)).toBe(false)
    expect(shouldCollapseByPointer('top', TOP_MERGED, TOP_MERGED, VIEWPORT_H)).toBe(false) // 边界含
    expect(shouldCollapseByPointer('top', TOP_MERGED + 1, TOP_MERGED, VIEWPORT_H)).toBe(true)
    expect(shouldCollapseByPointer('top', 300, TOP_MERGED, VIEWPORT_H)).toBe(true)
  })

  it('底栏对称:贴底不收,上移越出即收', () => {
    expect(shouldCollapseByPointer('bottom', VIEWPORT_H, BOTTOM, VIEWPORT_H)).toBe(false)
    expect(shouldCollapseByPointer('bottom', VIEWPORT_H - BOTTOM, BOTTOM, VIEWPORT_H)).toBe(false)
    expect(shouldCollapseByPointer('bottom', VIEWPORT_H - BOTTOM - 1, BOTTOM, VIEWPORT_H)).toBe(
      true,
    )
    expect(shouldCollapseByPointer('bottom', 300, BOTTOM, VIEWPORT_H)).toBe(true)
  })

  it('分离模式:顶侧两条(40+48)由同一边界覆盖 —— 指针在两条之间移动不收', () => {
    // 这正是几何判据免费换来的:首版靠 mouseleave 收起时,指针从标题栏移进页面工具栏会触发
    // 标题栏的 mouseleave → 把整个顶侧收掉,当时得专门加「同侧豁免」判据去打补丁。
    expect(shouldCollapseByPointer('top', 20, TOP_SPLIT, VIEWPORT_H)).toBe(false) // 标题栏内
    expect(shouldCollapseByPointer('top', 45, TOP_SPLIT, VIEWPORT_H)).toBe(false) // 两条交界
    expect(shouldCollapseByPointer('top', 80, TOP_SPLIT, VIEWPORT_H)).toBe(false) // 工具栏内
    expect(shouldCollapseByPointer('top', 89, TOP_SPLIT, VIEWPORT_H)).toBe(true) // 真越出
  })

  it('唤出带与收起边界之间留有死区 —— 不会在边界抖动', () => {
    // 唤出需 y ≤ 4,收起需 y > extent。二者之间(4, extent] 既不唤出也不收起 → 无振荡。
    for (const y of [5, 20, TOP_MERGED]) {
      expect(y > CHROME_REVEAL_EDGE_PX).toBe(true) // 不满足唤出
      expect(shouldCollapseByPointer('top', y, TOP_MERGED, VIEWPORT_H)).toBe(false) // 也不满足收起
    }
  })

  it('extent=0(该侧无 chrome)时:任何非贴边位置都收', () => {
    expect(shouldCollapseByPointer('top', 1, 0, VIEWPORT_H)).toBe(true)
    expect(shouldCollapseByPointer('top', 0, 0, VIEWPORT_H)).toBe(false)
  })
})

describe('shouldCollapseOnFocusOut', () => {
  it('非自动隐藏态:一律不收', () => {
    expect(shouldCollapseOnFocusOut('top', null)).toBe(false)
  })

  it('焦点仍落在同侧 chrome 内 → 不收(条内 Tab / 分离模式 标题栏→页面工具栏)', async () => {
    await setFullscreen(true)
    expect(shouldCollapseOnFocusOut('top', fakeNodeIn(CHROME_SIDE_SELECTOR.top))).toBe(false)
  })

  it('焦点穿出到内容区 → 收', async () => {
    await setFullscreen(true)
    expect(shouldCollapseOnFocusOut('top', nodeOutside)).toBe(true)
  })

  it('焦点去向为 null(点击非可聚焦区) → 收', async () => {
    await setFullscreen(true)
    expect(shouldCollapseOnFocusOut('top', null)).toBe(true)
  })

  it('异侧不豁免:顶栏的焦点落到底栏 → 顶栏照收', async () => {
    await setFullscreen(true)
    expect(shouldCollapseOnFocusOut('top', fakeNodeIn(CHROME_SIDE_SELECTOR.bottom))).toBe(true)
  })
})

describe('唤出读「本帧最接近边缘的采样点」而非派发点', () => {
  // 判据本身是 applyPointer 里的 nearestTop <= EDGE,它不导出;此处钉的是**驱动它的那个量**
  // 该如何从一次 pointermove 里取出 —— 即 sampleFrom 的语义,用同构的最小复现表达。
  //
  // 机制:Chromium 把每帧多个原始移动合并成一次派发,派发事件带的是**最后**那个位置。
  // 快速甩到顶时「撞 y=0 → 手回弹 y=25」可落在同一帧内 → 派发点 y=25,y≤4 只在 coalesced 里。
  // 只看派发点则漏判这一帧曾经贴边——与「真机唤出不稳定」是否同一根因**已被证伪**(2026-07-16
  // 真凶另有其人:tauri-runtime-wry 的顶缘输入死区,见 useWindowMode.ts,777824d);
  // 此判据本身仍是正确行为,独立成立,故予以保留。

  /** 与 sampleFrom 同构:取本帧全部采样点的纵向极值。 */
  function nearestTopOf(dispatchedY: number, coalescedYs: number[]): number {
    return Math.min(dispatchedY, ...coalescedYs)
  }

  it('派发点在带外、但本帧曾触顶 → 仍应判为「碰到过边缘」', () => {
    // 真机形状:光标撞到 y=0 后回弹到 25,整段被合并进一帧。
    expect(nearestTopOf(25, [500, 200, 40, 0, 12, 25])).toBe(0)
    expect(nearestTopOf(25, [500, 200, 40, 0, 12, 25]) <= CHROME_REVEAL_EDGE_PX).toBe(true)
    // 只看派发点则永远看不到本帧内曾经贴边的中间采样。
    expect(25 <= CHROME_REVEAL_EDGE_PX).toBe(false)
  })

  it('本帧从未触顶 → 不误唤出(加宽不是修法,精确才是)', () => {
    expect(nearestTopOf(25, [60, 45, 30, 25]) <= CHROME_REVEAL_EDGE_PX).toBe(false)
  })

  it('无 coalesced(退化路径)→ 与旧行为逐值一致,不更坏', () => {
    expect(nearestTopOf(3, [])).toBe(3)
    expect(nearestTopOf(25, [])).toBe(25)
  })

  it('收起仍读**派发点**(当前位置)而非极值 —— 否则甩过顶端后会被永久钉住', () => {
    // 唤出是「事件」(碰到过),收起是「状态」(现在在哪)。若收起也读 nearestTop,
    // 上例中 nearestTop=0 会让 shouldCollapseByPointer 恒假 → 指针早已远离却收不起来。
    expect(shouldCollapseByPointer('top', 25, TOP_MERGED, VIEWPORT_H)).toBe(false) // 25 仍在 40 内
    expect(shouldCollapseByPointer('top', 300, TOP_MERGED, VIEWPORT_H)).toBe(true) // 真远离 → 收
    expect(shouldCollapseByPointer('top', 0, TOP_MERGED, VIEWPORT_H)).toBe(false)
  })
})

describe('常量', () => {
  it('边缘唤出带宽在「不误触」与「不被高速指针跳过」之间', () => {
    // 1px 会被逐帧采样的快速移动跳过;过宽则画廊顶部行的正常悬停就会弹出顶栏。
    expect(CHROME_REVEAL_EDGE_PX).toBeGreaterThanOrEqual(2)
    expect(CHROME_REVEAL_EDGE_PX).toBeLessThanOrEqual(8)
  })

  it('唤出带必须窄于最小的一条 chrome —— 否则唤出即满足收起,顶栏永远弹不出来', () => {
    // 收起判据是 y > extent、唤出是 y ≤ EDGE。若 EDGE ≥ extent,则同一次 pointermove 里
    // 唤出刚置真就被收起判据抹掉。BOTTOM(28) 是全应用最矮的一条。
    expect(CHROME_REVEAL_EDGE_PX).toBeLessThan(BOTTOM)
  })

  it('两侧选择器互不相等(否则焦点豁免会跨侧误命中)', () => {
    expect(CHROME_SIDE_SELECTOR.top).not.toBe(CHROME_SIDE_SELECTOR.bottom)
  })

  it('非全屏唤出带(8px)宽于全屏(4px)——补偿 wry 隐形 drag-resize 死区', () => {
    // 非全屏(窗口态)resizable 恒为 true,死区常在;全屏区间已被 setResizable(false) 消区,
    // 4px 足够,不必分档加宽。
    expect(CHROME_REVEAL_EDGE_PX_WINDOWED).toBeGreaterThan(CHROME_REVEAL_EDGE_PX)
    expect(CHROME_REVEAL_EDGE_PX).toBe(4)
    expect(CHROME_REVEAL_EDGE_PX_WINDOWED).toBe(8)
  })
})

describe('activeRevealEdgePx —— 分档依据是窗口态,不是触发来源(D-423)', () => {
  it('全屏(不论谁触发)→ 4px:setResizable(false) 已消死区', async () => {
    await setFullscreen(true)
    expect(chromeAutoHidden.value).toBe(true)
    expect(activeRevealEdgePx()).toBe(CHROME_REVEAL_EDGE_PX)
  })

  it('非全屏 + 查看器沉浸 → 8px:窗口态死区仍在场,不因触发来源是"沉浸"而维持 4px', () => {
    enterImmersiveViewer()
    expect(isFullscreen.value).toBe(false)
    expect(chromeAutoHidden.value).toBe(true)
    expect(activeRevealEdgePx()).toBe(CHROME_REVEAL_EDGE_PX_WINDOWED)
  })

  it('非全屏 + 仅窗口化设置触发 → 8px', () => {
    useUiStore().autoHideChromeWindowed = true
    expect(isFullscreen.value).toBe(false)
    expect(chromeAutoHidden.value).toBe(true)
    expect(activeRevealEdgePx()).toBe(CHROME_REVEAL_EDGE_PX_WINDOWED)
  })
})
