// src/composables/useContentViewerZoomRotation.ts
// 查看器缩放比例 + 旋转(含持久化复原)。
// 尺寸就绪后先更新缩放基准，再重算人脸框；待恢复旋转只在此时应用一次。

import { ref, computed, watch, type Ref } from 'vue'
import type { useMediaDetail } from './useMediaDetail'
import type { useMediaStore } from './../stores/mediaStore'
import type { MediaDetail } from '../types/media'
import type VideoPlayer from '../components/media/player/VideoPlayer.vue'

export function useContentViewerZoomRotation(options: {
  state: ReturnType<typeof useMediaDetail>
  viewerRef: Ref<HTMLElement | null>
  currentImageElement: () => HTMLImageElement | null
  /** 当前原图已完成加载；占位缩略图不能作为恢复旋转的尺寸基准。 */
  isImageLoaded: () => boolean
  videoRef: Ref<InstanceType<typeof VideoPlayer> | null>
  detail: () => MediaDetail | null
  media: ReturnType<typeof useMediaStore>
  t: (key: string) => string
  /** 缩放基准更新后通知主组件重算人脸框投影。 */
  onSizeReady: () => void
}) {
  const { state, viewerRef, currentImageElement, isImageLoaded, videoRef, detail, media, t, onSizeReady } = options

  // 获取媒体真实宽高
  function getMediaDimensions() {
    const image = currentImageElement()
    if (image) {
      if (!isImageLoaded()) return { w: 0, h: 0 }
      return { w: image.naturalWidth, h: image.naturalHeight }
    }
    const videoEl = videoRef.value?.videoEl
    if (videoEl) {
      return { w: videoEl.videoWidth, h: videoEl.videoHeight }
    }
    return { w: 1, h: 1 }
  }

  function handleToggleZoom() {
    if (!viewerRef.value || (!currentImageElement() && !videoRef.value?.videoEl)) {
      state.resetZoom()
      return
    }
    const { w: iw, h: ih } = getMediaDimensions()
    const cw = viewerRef.value.clientWidth
    const ch = viewerRef.value.clientHeight
    if (!iw || !ih || !cw || !ch) {
      state.resetZoom()
      return
    }
    state.cycleZoomMode(cw, ch, iw, ih)
  }

  // 归一化当前展示角度(0/90/180/270)——底栏角标与持久化都用它;state.rotation 本身单向累进不回绕。
  const rotationDeg = computed(() => ((state.rotation.value % 360) + 360) % 360)
  const rotateLabel = computed(() =>
    rotationDeg.value === 0 ? t('detail.rotate') : `${t('detail.rotate')} · ${rotationDeg.value}°`,
  )

  // 旋转(顶栏重构 P5):取容器/图像尺寸后调 state.rotate(转 90° + 含旋转 auto 适应,不溢出)。
  // 尺寸未知(未加载)则不动——旋转按钮仅在媒体加载后有意义。
  // 持久化(V20):落库归一角度并乐观回写 detail.viewRotation。旋转只作用大图、不进网格,故不发
  // itemPatchSignal(那是评分/收藏/色标回灌画廊用的)。
  function handleRotate() {
    if (!viewerRef.value || (!currentImageElement() && !videoRef.value?.videoEl)) return
    const { w: iw, h: ih } = getMediaDimensions()
    const cw = viewerRef.value.clientWidth
    const ch = viewerRef.value.clientHeight
    if (!iw || !ih || !cw || !ch) return
    state.rotate(cw, ch, iw, ih)
    const norm = ((state.rotation.value % 360) + 360) % 360
    const item = media.detailItem
    if (item) {
      item.viewRotation = norm // 乐观回写:翻页离开再回来经 snapshot 复原,即时手感不等落库
      // 落库经 store 的 latest-write-wins 队列(审查 F-09):连点不乱序,失败在队列内捕获。
      media.setViewRotation(item.id, norm)
    }
  }

  // 打开大图时待复原的持久旋转(V20)。resetZoom 已把 rotation 清 0,此处记下目标角度,待图像/视频
  // 尺寸就绪(updateZoomRatio,由 @load/@loadedmetadata 触发)再落角 + 按含旋转 auto 适应足迹,一次性。
  const pendingInitialRotation = ref<number | null>(null)

  const zoomRatio = ref(1.0)

  // 当查看的项目更改时重置缩放
  watch(detail, () => {
    state.resetZoom()
    state.isPlayingLive.value = false
    state.liveVideoSrc.value = null
    zoomRatio.value = 1.0
    // 复原上次持久化的展示旋转:0 视作无需复原(pending=null)。
    const item = detail()
    pendingInitialRotation.value = item?.viewRotation ? item.viewRotation : null
  })

  function updateZoomRatio() {
    if (!viewerRef.value || (!currentImageElement() && !videoRef.value?.videoEl)) return
    const { w: iw, h: ih } = getMediaDimensions()
    const cw = viewerRef.value.clientWidth
    const ch = viewerRef.value.clientHeight
    // ResizeObserver 可能先于 load/元数据回调，零尺寸时保留待恢复角度。
    if (!iw || !ih || !cw || !ch) return
    const base_w = Math.min(iw, cw, ch * (iw / ih))
    zoomRatio.value = base_w / iw
    // 尺寸就绪的首帧复原持久旋转:设角后按含旋转 auto 适应足迹,随即清 pending——只做一次,不干扰用户
    // 之后的手动缩放/旋转(setZoomMode 改 scale 会再次触发本函数,但 pending 已 null,不重入)。
    if (pendingInitialRotation.value !== null) {
      state.setRotation(pendingInitialRotation.value)
      state.setZoomMode('auto', cw, ch, iw, ih)
      pendingInitialRotation.value = null
    }
    onSizeReady()
  }

  const isZoomChanged = ref(false)
  let zoomHighlightTimer: ReturnType<typeof setTimeout> | null = null

  watch(
    () => state.scale.value,
    () => {
      isZoomChanged.value = true
      if (zoomHighlightTimer) clearTimeout(zoomHighlightTimer)
      zoomHighlightTimer = setTimeout(() => {
        isZoomChanged.value = false
      }, 2000)
      updateZoomRatio()
    },
  )

  return {
    getMediaDimensions,
    handleToggleZoom,
    zoomRatio,
    updateZoomRatio,
    isZoomChanged,
    rotationDeg,
    rotateLabel,
    handleRotate,
    pendingInitialRotation,
  }
}
