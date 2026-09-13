// src/composables/useContentViewerFaces.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):人脸框状态与投影计算。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。
// 红线(风险章节):faceToken 并发丢弃判定须逐字保留,不得简化为取消/AbortController 等替代实现。

import { ref, watch, nextTick, type Ref } from 'vue'
import type { FaceBox } from '../types/person'
import type { usePersonStore } from '../stores/personStore'

export function useContentViewerFaces(options: {
  viewerRef: Ref<HTMLElement | null>
  currentImageElement: () => HTMLImageElement | null
  transform: () => string
  personStore: ReturnType<typeof usePersonStore>
}) {
  const { viewerRef, currentImageElement, transform, personStore } = options

  // ── 人脸框(F6)─── 把检测到的人脸叠加到图上。bbox 是相对图像自身像素的归一化 [0,1],故投影到
  // 图像的**内容矩形**(object-fit:contain 会留黑边),用 getBoundingClientRect 实时测量——它已含
  // 缩放/平移 transform,所以框会自动跟随缩放与拖拽。
  const faces = ref<FaceBox[]>([])
  // 人脸蓝框显隐开关(问题5)。默认显示;纯前端查看偏好,持久化到 localStorage(无需 IPC)。
  const showFaces = ref(localStorage.getItem('detail_show_faces') !== 'false')
  function toggleFaces() {
    showFaces.value = !showFaces.value
    localStorage.setItem('detail_show_faces', String(showFaces.value))
  }
  // Image content rect in viewer-local coords {x,y,w,h}; null when not measurable.
  // 图像内容矩形(viewer 局部坐标 {x,y,w,h});不可测时为 null。
  const faceContentRect = ref<{ x: number; y: number; w: number; h: number } | null>(null)
  let faceRaf: number | null = null
  // in-flight token(2026-07-06 审查 P1-10):方向键快速翻图时多个 getFacesForItem 并发在途,
  // 无 token 则「后落地者赢」——慢的旧图应答会把新图的人脸集覆盖(bbox 投影到新图坐标系位置全错)。
  let faceToken = 0

  async function loadFacesFor(item: { id: number; mediaType: string } | null) {
    const my = ++faceToken
    // 切图先清空:新图渲染期间不残留旧图的框(image→image 切换原先不清,旧框错位可见)。
    faces.value = []
    if (!item || item.mediaType !== 'image') {
      return
    }
    const result = await personStore.getFacesForItem(item.id)
    if (my !== faceToken) return // 迟到的旧图应答,丢弃
    faces.value = result
    await nextTick()
    recomputeFaceLayout()
  }

  function recomputeFaceLayout() {
    const img = currentImageElement()
    const viewer = viewerRef.value
    if (!img || !viewer || !img.naturalWidth || !img.naturalHeight) {
      faceContentRect.value = null
      return
    }
    const ir = img.getBoundingClientRect()
    const vr = viewer.getBoundingClientRect()
    const natRatio = img.naturalWidth / img.naturalHeight
    const boxRatio = ir.width / Math.max(ir.height, 1)
    let cw: number, ch: number
    if (natRatio > boxRatio) {
      cw = ir.width
      ch = ir.width / natRatio
    } else {
      ch = ir.height
      cw = ir.height * natRatio
    }
    faceContentRect.value = {
      x: ir.left - vr.left + (ir.width - cw) / 2,
      y: ir.top - vr.top + (ir.height - ch) / 2,
      w: cw,
      h: ch,
    }
  }

  // rAF-throttled recompute (transform changes fire rapidly while dragging).
  // rAF 节流重算(拖拽时 transform 高频变化)。
  function scheduleFaceRecompute() {
    if (faceRaf != null) return
    faceRaf = requestAnimationFrame(() => {
      faceRaf = null
      recomputeFaceLayout()
    })
  }

  watch(transform, scheduleFaceRecompute)

  function faceBoxStyle(f: FaceBox): Record<string, string> {
    const r = faceContentRect.value
    if (!r) return { display: 'none' }
    return {
      left: `${r.x + f.bbox[0] * r.w}px`,
      top: `${r.y + f.bbox[1] * r.h}px`,
      width: `${f.bbox[2] * r.w}px`,
      height: `${f.bbox[3] * r.h}px`,
    }
  }

  return {
    faces,
    showFaces,
    toggleFaces,
    faceContentRect,
    loadFacesFor,
    recomputeFaceLayout,
    faceBoxStyle,
  }
}
