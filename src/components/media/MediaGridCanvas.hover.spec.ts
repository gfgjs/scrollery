import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import {
  useCanvasHoverCard,
  type CanvasHoverCardEmit,
} from '../../composables/useCanvasHoverCard'

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
})
