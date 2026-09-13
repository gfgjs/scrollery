import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  useCanvasHoverCard,
  type CanvasHoverCardEmit,
} from '../../composables/useCanvasHoverCard'

const appSource = readFileSync(fileURLToPath(new URL('../../App.vue', import.meta.url)), 'utf8')
const thumbSource = readFileSync(fileURLToPath(new URL('./MediaThumb.vue', import.meta.url)), 'utf8')
const canvasSource = readFileSync(fileURLToPath(new URL('./MediaGridCanvas.vue', import.meta.url)), 'utf8')
const rendererSource = readFileSync(
  fileURLToPath(new URL('./mediaGridCanvas.cellRenderer.ts', import.meta.url)),
  'utf8',
)
const hoverCss = readFileSync(
  fileURLToPath(new URL('./MediaGridCanvas.styles.css', import.meta.url)),
  'utf8',
)

describe('Canvas hover card interaction contract', () => {
  it('does not re-hit the underlying grid anywhere inside the current hover card', () => {
    let hitCount = 0
    const emit = (() => {}) as CanvasHoverCardEmit
    const hover = useCanvasHoverCard({
      canvasRef: ref<HTMLCanvasElement | null>(null),
      scrolling: () => false,
      isSelectionMode: () => false,
      enableHoverScale: () => true,
      isPendingDelete: () => false,
      isSelected: () => false,
      cacheDir: () => '',
      currentY: () => 0,
      selectionVersion: () => 0,
      viewport: () => ({ w: 800, h: 600 }),
      hitTest: {
        pickWithRow: () => {
          hitCount++
          return null
        },
        hitHandleAt: () => false,
      },
      requestRedraw: () => {},
      emit,
    })

    hover.onHoverCardPointerMove({
      buttons: 0,
      target: {} as EventTarget,
    } as unknown as PointerEvent)
    expect(hitCount).toBe(0)
  })

  it('keeps actionable controls out of card-level pointer tracking', () => {
    expect(thumbSource).toContain('@pointerdown.stop')
  })

  it('hydrates the persisted hover-scale value into the Canvas source of truth', () => {
    expect(appSource).toContain(
      "config.enableHoverScale = cfg.enableThumbHoverScale !== 'false'",
    )
  })

  it('animates only when actually zooming; the non-zoom overlay paints inline without compositor snapping', () => {
    // 与 DOM .media-card:hover 同款时长/缓动,但仅放大态(scale0≠1)挂动画:
    // scale(1)→scale(1) 空转动画会白付合成开销,且合成层设备像素对齐与 canvas 亚像素
    // 绘制有落点差,图片整帧替换瞬间会产生可感跳变。
    const baseStart = hoverCss.indexOf('.mgc-hover-card {')
    const baseBlock = hoverCss.slice(baseStart, hoverCss.indexOf('}', baseStart) + 1)
    expect(baseBlock).not.toContain('animation')
    expect(baseBlock).not.toContain('will-change')
    expect(baseBlock).not.toContain('opacity')
    // 贴边钳位格:变换基点钉在原格中心,scale(scale0) 起点才与画格逐像素重合
    expect(baseBlock).toContain(
      'transform-origin: var(--mgc-origin-x, 50%) var(--mgc-origin-y, 50%);',
    )
    const zoomBlock = hoverCss.slice(
      hoverCss.indexOf('.mgc-hover-card--zoom {'),
      hoverCss.indexOf('@keyframes mgc-hover-enter'),
    )
    expect(zoomBlock).toContain(
      'animation: mgc-hover-enter 220ms cubic-bezier(0.34, 1.18, 0.64, 1) both;',
    )
  })

  it('fades only the ring/shadow layer; the ring exists only in the zoom state', () => {
    // 卡体(承载图片)不得整体淡入:半透明卡叠在 canvas 原格上会重影;
    // 淡入只允许出现在 ::after 装饰层。阴影缓动对齐 DOM box-shadow 过渡(220ms ease)。
    expect(hoverCss).toContain('animation: mgc-hover-fade 220ms ease both;')
    expect(hoverCss).not.toContain('mgc-hover-pop')
    const afterStart = hoverCss.indexOf('.mgc-hover-card::after {')
    const afterBlock = hoverCss.slice(afterStart, hoverCss.indexOf('}', afterStart) + 1)
    expect(afterBlock).not.toContain('border: 1px')
    expect(hoverCss).toContain('.mgc-hover-card--zoom::after')
  })

  it('disables the thumb fade-in inside the hover card', () => {
    // 卡内 <img> 带全局 thumb-appear 渐显,而卡下就是 canvas 已绘同图(两管线亚像素不同源):
    // 200ms 交叉淡化 = 两份微错位图互相透底,观感即「移入后图片轻微位移」。必须整帧替换。
    expect(hoverCss).toContain('.mgc-hover-card :deep(.thumb-loaded) {\n  animation: none;\n}')
  })

  it('mirrors the canvas bitmap in the non-zoom card instead of re-rendering it via <img>', () => {
    // 位移残留根因:卡内 <img> 是第二条解码管线(object-fit 浮点 cover 裁剪 + 布局盒设备
    // 像素吸附),与 canvas 管线(整数化裁剪 + 桶预缩放 + 亚像素抗锯齿)必有亚像素错位,
    // 整帧替换瞬间即「移入轻微位移」。未放大态必须透出画布位图(像素逐位不变),图像面
    // (img/占位/文本卡)全部隐藏;放大态内容有意重缩,不受此约束。
    const mirrorStart = hoverCss.indexOf('.mgc-hover-card--mirror :deep(.media-thumb__img)')
    expect(mirrorStart).toBeGreaterThan(-1)
    const mirrorBlock = hoverCss.slice(mirrorStart, hoverCss.indexOf('}', mirrorStart) + 1)
    expect(mirrorBlock).toContain('display: none;')
    expect(mirrorBlock).toContain('.media-thumb__placeholder')
    expect(mirrorBlock).toContain('.media-thumb__textcard')
    // 模板挂镜像类;放大态(scale0≠1)类不成立,由 computed 保证。
    expect(canvasSource).toContain("'mgc-hover-card--mirror': hoverCardMirror")
    // 镜像格的 canvas 侧 chrome 同步停画(卡内 DOM 重绘),防两引擎双重成像。
    expect(rendererSource).toContain('const mirroring = deps.mirrorCellId() === item.id')
    expect(rendererSource).toContain('if (d > 0.001 && !mirroring)')
    expect(rendererSource).toContain('deps.isSelectionMode() && !mirroring')
  })

  it('keeps the in-place (k=1) card unclamped so edge rows never shift off their cell', () => {
    // 贴视口缘的格(滚动停在中途的首/尾行)在旧实现里被钳回视口内,卡被平移出原格
    // =「关闭放大仍位移」的几何来源之一。原位态必须与画格逐坐标重合。
    const helpersSource = readFileSync(
      fileURLToPath(new URL('./mediaGridCanvas.helpers.ts', import.meta.url)),
      'utf8',
    )
    expect(helpersSource).toContain('if (k > 1) {')
  })
})
