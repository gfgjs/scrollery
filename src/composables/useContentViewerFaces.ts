// src/composables/useContentViewerFaces.ts
// 人脸框状态与投影计算。
// faceToken 丢弃切图后迟到的请求结果，避免旧图人脸框覆盖新图。

import { computed, ref, watch, nextTick, type Ref } from 'vue'
import type { FaceBox } from '../types/person'
import type { usePersonStore } from '../stores/personStore'
import { writeSettings } from '../stores/settingsPersistence'
import { readSettingBool } from './settingsValues'

/** 人脸蓝框显隐(全局查看偏好,后端 schema 注册键 detail_show_faces)。 */
const SHOW_FACES_KEY = 'detail_show_faces'

export function useContentViewerFaces(options: {
  viewerRef: Ref<HTMLElement | null>
  currentImageElement: () => HTMLImageElement | null
  transform: () => string
  personStore: ReturnType<typeof usePersonStore>
}) {
  const { viewerRef, currentImageElement, transform, personStore } = options

  // bbox 是相对图像自身像素的归一化 [0,1],故投影到
  // 图像的**内容矩形**(object-fit:contain 会留黑边),用 getBoundingClientRect 实时测量——它已含
  // 缩放/平移 transform,所以框会自动跟随缩放与拖拽。
  const faces = ref<FaceBox[]>([])
  // 全局人脸框显隐偏好存中央设置，重置时随快照恢复默认。
  const showFaces = computed(() => readSettingBool(SHOW_FACES_KEY, true))
  function toggleFaces() {
    // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
    writeSettings({ [SHOW_FACES_KEY]: String(!showFaces.value) }).catch(() => {})
  }
  // 图像内容矩形(viewer 局部坐标 {x,y,w,h});不可测时为 null。
  const faceContentRect = ref<{ x: number; y: number; w: number; h: number } | null>(null)
  let faceRaf: number | null = null
  // 并发翻图请求以代次区分，只允许当前图片的结果落地。
  let faceToken = 0

  async function loadFacesFor(item: { id: number; mediaType: string } | null) {
    const my = ++faceToken
    // 切图先清空，避免新图渲染期间残留旧图人脸框。
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
