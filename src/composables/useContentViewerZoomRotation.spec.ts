// useContentViewerZoomRotation characterization spec(超长文件拆分方案 §5 补测要求 / 复核建议)。
//
// 覆盖面:pendingInitialRotation 一次性复原——detail 切换后记下待复原角度,首个 updateZoomRatio
// (尺寸就绪时触发)消费该值(state.setRotation + setZoomMode('auto',…))并清空 pending;第二次
// updateZoomRatio 不再复原(pending 已 null,不重入),不干扰用户之后的手动缩放/旋转。
// 不覆盖:handleToggleZoom/handleRotate 的 DOM 尺寸分支(需要真实 viewerRef/currentImageElement
// 测量,超出本测试范围)。
//
// 环境:node,无 DOM。state 用真实 useMediaDetail()(其缩放数学已有独立 spec 覆盖);viewerRef/
// currentImageElement/videoRef 均为最小 stub,仅提供 updateZoomRatio 所需的尺寸读数。
import { describe, it, expect, vi } from 'vitest'
import { ref, effectScope, nextTick } from 'vue'
import { useContentViewerZoomRotation } from './useContentViewerZoomRotation'
import { useMediaDetail } from './useMediaDetail'
import type { MediaDetail } from '../types/media'

describe('useContentViewerZoomRotation: pendingInitialRotation 一次性复原', () => {
  it('reset 后首个 updateZoomRatio 消费该值并清空,第二次不再复原', async () => {
    const scope = effectScope()
    const state = scope.run(() => useMediaDetail())!

    const detailRef = ref<MediaDetail | null>(null)
    const viewerEl = { clientWidth: 800, clientHeight: 600 } as HTMLElement
    const imgEl = { naturalWidth: 400, naturalHeight: 300 } as HTMLImageElement
    const onSizeReady = vi.fn()
    const media = { detailItem: null, setViewRotation: vi.fn() } as unknown as Parameters<
      typeof useContentViewerZoomRotation
    >[0]['media']

    const c = scope.run(() =>
      useContentViewerZoomRotation({
        state,
        viewerRef: ref(viewerEl),
        currentImageElement: () => imgEl,
        videoRef: ref(null),
        detail: () => detailRef.value,
        media,
        t: (k: string) => k,
        onSizeReady,
      }),
    )!

    // detail 切换(打开一张持久化过 90° 旋转的图):watch(detail,…) 触发,记下待复原角度。
    detailRef.value = { id: 1, viewRotation: 90 } as unknown as MediaDetail
    await nextTick()
    expect(c.pendingInitialRotation.value).toBe(90)

    // 尺寸就绪的首帧 updateZoomRatio:消费 pending——落角 + 清空。
    c.updateZoomRatio()
    expect(state.rotation.value).toBe(90)
    expect(c.pendingInitialRotation.value).toBeNull()
    expect(onSizeReady).toHaveBeenCalledTimes(1)

    // 之后的 updateZoomRatio(如再次触发,例如手动缩放联动)不得再次复原:模拟用户手动旋转到 180,
    // 第二次调用不应把角度打回 90。
    state.setRotation(180)
    c.updateZoomRatio()
    expect(state.rotation.value).toBe(180) // 未被 pending 复原(pending 已空,不重入)
    expect(onSizeReady).toHaveBeenCalledTimes(2)

    scope.stop()
  })
})
