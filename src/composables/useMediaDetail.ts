// src/composables/useMediaDetail.ts
// 媒体详情覆盖层的组件级组合式函数 (§12.3)
//
// 重要：在组件 setup 时调用此函数一次，不要在 computed() 内部调用——否则会在每次响应式依赖
// 变化时重新创建事件监听器，导致 mousemove/mouseup 监听器永久泄漏。

import { ref, computed } from 'vue'

export function useMediaDetail() {
  // 图像查看器状态
  const scale = ref(1.0)
  const translateX = ref(0)
  const translateY = ref(0)
  const isDragging = ref(false)

  type ZoomMode = 'auto' | 'original' | 'fit-width' | 'fit-height' | 'custom'
  const zoomMode = ref<ZoomMode>('auto')

  // 旋转角(顶栏重构 P5:看图台旋转)。0/90/180/270,顺时针 90° 步进。transform 末端 rotate,
  // 缩放/适应数学在 90/270 时按「显示足迹宽高对换」处理(见 containScale / setZoomMode)。
  const rotation = ref(0)

  // Live 照片状态
  const isPlayingLive = ref(false)
  const liveVideoSrc = ref<string | null>(null)

  // transform 顺序:translate(屏幕坐标) scale rotate——rotate 最内先作用于图,scale 再放大,
  // translate 最后在屏幕空间平移(故旋转下拖拽仍是屏幕方向,直觉一致)。
  const transform = computed(
    () =>
      `translate(${translateX.value}px, ${translateY.value}px) scale(${scale.value}) rotate(${rotation.value}deg)`,
  )

  function zoomIn() {
    zoomMode.value = 'custom'
    scale.value = Math.min(scale.value * 1.25, 10)
  }
  function zoomOut() {
    zoomMode.value = 'custom'
    scale.value = Math.max(scale.value * 0.8, 0.1)
  }
  function resetZoom() {
    zoomMode.value = 'auto'
    scale.value = 1.0
    translateX.value = 0
    translateY.value = 0
    rotation.value = 0
  }

  function cycleZoomMode(cw: number, ch: number, iw: number, ih: number) {
    const modes: ('auto' | 'original' | 'fit-width' | 'fit-height')[] = [
      'auto',
      'original',
      'fit-width',
      'fit-height',
    ]
    let currentIdx = modes.indexOf(zoomMode.value as (typeof modes)[number])
    if (currentIdx === -1) currentIdx = 3 // custom 态下一个循环到 auto(3+1=4=0)

    const nextMode = modes[(currentIdx + 1) % modes.length]
    setZoomMode(nextMode, cw, ch, iw, ih)
  }

  // 令(可能旋转的)图在容器内 contain(恰好铺满不超出、且不放大)的 scale。base = object-fit:
  // contain 且不超原生的渲染尺寸(与 setZoomMode 的 base_w/h 同义);旋转 90/270 时足迹宽高对换后
  // 求 fit,末位钳 1 保证「auto 不放大」——未旋转恒得 1,与旧 auto=scale1 行为逐字节一致。
  function containScale(cw: number, ch: number, iw: number, ih: number): number {
    const safeIw = Math.max(iw, 1)
    const safeIh = Math.max(ih, 1)
    const safeCw = Math.max(cw, 1)
    const safeCh = Math.max(ch, 1)
    const rw = Math.min(safeIw, safeCw, safeCh * (safeIw / safeIh))
    const rh = Math.min(safeIh, safeCh, safeCw * (safeIh / safeIw))
    const rotated = rotation.value % 180 !== 0
    const fw = rotated ? rh : rw
    const fh = rotated ? rw : rh
    return Math.min(safeCw / fw, safeCh / fh, 1)
  }

  function setZoomMode(mode: ZoomMode, cw: number, ch: number, iw: number, ih: number) {
    zoomMode.value = mode
    translateX.value = 0
    translateY.value = 0

    if (mode === 'custom') return // custom 由 zoomIn/zoomOut 直接设 scale,不经此分支

    const safeIw = Math.max(iw, 1)
    const safeIh = Math.max(ih, 1)
    const safeCw = Math.max(cw, 1)
    const safeCh = Math.max(ch, 1)

    if (mode === 'auto') {
      // auto = 适应容器(含旋转);未旋转恒 1(object-fit:contain 已就位),旋转 90/270 按足迹重算。
      scale.value = containScale(safeCw, safeCh, safeIw, safeIh)
      return
    }

    // 旋转 90/270:显示足迹宽高对换 → 适应数学用对换后的有效尺寸。
    const rotated = rotation.value % 180 !== 0
    const eiw = rotated ? safeIh : safeIw
    const eih = rotated ? safeIw : safeIh
    const base_w = Math.min(eiw, safeCw, safeCh * (eiw / eih))
    const base_h = Math.min(eih, safeCh, safeCw * (eih / eiw))

    if (mode === 'original') {
      scale.value = eiw / base_w
    } else if (mode === 'fit-width') {
      scale.value = safeCw / base_w
    } else if (mode === 'fit-height') {
      scale.value = safeCh / base_h
    }
  }

  /** 将图像适应容器，仅缩小（从不放大）。在已知实际图像尺寸后调用。 */
  function fitToWindow(containerW: number, containerH: number, imgW: number, imgH: number) {
    const s = Math.min(containerW / Math.max(imgW, 1), containerH / Math.max(imgH, 1), 1)
    scale.value = s
    translateX.value = 0
    translateY.value = 0
  }

  /**
   * 顺时针旋转 90°(顶栏重构 P5;单向累进 2026-07-18)。旋转后自动以 auto 适应新朝向(含旋转的
   * contain),使旋转后的图恰好铺满容器不溢出。需容器/图像尺寸(与 setZoomMode 同参)。
   *
   * **单向累进不取模**:`rotation` 一直加 90(0→90→…→270→360→450…),不回绕到 0。原 `%360` 会让
   * 第 4 次点击把角度从 270 跳回 0,而 `.detail-viewer__img` 的 transform transition 会把这段插值成
   * **逆时针 270° 大回旋**(视觉「倒转回去」,别扭)。累进后每次都是 +90 平滑顺时针。缩放/适应数学
   * 用 `rotation % 180` 判断足迹是否对换,对任意累进值恒正确(360%180=0 未转、450%180=90 已转)。
   * 持久化与「按钮上显示的角度」由调用方用 `((r % 360) + 360) % 360` 归一(见 ContentViewer)。
   */
  function rotate(cw: number, ch: number, iw: number, ih: number) {
    rotation.value += 90
    setZoomMode('auto', cw, ch, iw, ih)
  }

  /**
   * 直接设定旋转角(用于打开大图时复原持久化的展示旋转,V20)。不做适应——调用方在图像尺寸就绪后
   * 自行 `setZoomMode('auto', …)` 适配足迹。传入值应已归一(0/90/180/270)。
   */
  function setRotation(deg: number) {
    rotation.value = deg
  }

  // 拖动平移
  let dragStartX = 0
  let dragStartY = 0
  let dragInitX = 0
  let dragInitY = 0

  function startDrag(e: MouseEvent) {
    if (scale.value <= 1) return
    isDragging.value = true
    dragStartX = e.clientX
    dragStartY = e.clientY
    dragInitX = translateX.value
    dragInitY = translateY.value
    document.addEventListener('mousemove', onDrag)
    document.addEventListener('mouseup', stopDrag)
  }

  function onDrag(e: MouseEvent) {
    if (!isDragging.value) return
    translateX.value = dragInitX + (e.clientX - dragStartX)
    translateY.value = dragInitY + (e.clientY - dragStartY)
  }

  function stopDrag() {
    isDragging.value = false
    document.removeEventListener('mousemove', onDrag)
    document.removeEventListener('mouseup', stopDrag)
  }

  /** 必须从 onBeforeUnmount 调用以避免监听器泄漏。 */
  function cleanup() {
    document.removeEventListener('mousemove', onDrag)
    document.removeEventListener('mouseup', stopDrag)
  }

  function onWheel(e: WheelEvent): boolean {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault()
      zoomMode.value = 'custom'
      const factor = e.deltaY < 0 ? 1.1 : 0.9
      scale.value = Math.max(0.1, Math.min(10, scale.value * factor))
      return true
    }
    return false
  }

  return {
    scale,
    translateX,
    translateY,
    isDragging,
    transform,
    zoomMode,
    rotation,
    isPlayingLive,
    liveVideoSrc,
    zoomIn,
    zoomOut,
    resetZoom,
    cycleZoomMode,
    setZoomMode,
    rotate,
    setRotation,
    fitToWindow,
    startDrag,
    onWheel,
    cleanup,
  }
}
