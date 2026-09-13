// src/composables/useImageEditor.ts
// 图片简单编辑(方案 C §7):几何与状态机,不碰 DOM——像素测量/手柄拖拽在 EditOverlay.vue。
//
// 坐标契约(与后端 editing::geometry 严格对应,见该模块文档):裁剪矩形 `crop` 存在**变换后
// 坐标系**里(文件 EXIF orientation 只应用一次 → 合并 view_rotation 与本次 rotate → flip →
// fine rotate 最大内接裁剪之后),
// 用归一化 [0,1] 分数存储(与显示分辨率无关,窗口缩放不必重算)。EditOverlay 负责测量该坐标系
// 的显示尺寸(postWidth/postHeight,见 setNaturalSize)并做手柄拖拽 ↔ 归一化互转。
//
// 简化取舍(v1):旋转或翻转变化时清空已选裁剪框(不做「旋转后重投影裁剪框」的矩阵变换)——
// 用户需要在敲定朝向后重新拉框。心智模型简单、零重投影 bug 面,足够覆盖"顺手转正后裁剪"的
// 主流程;真正「先裁后转还想保留裁剪框」的复杂序列本就少见。

import { ref, computed } from 'vue'
import { invokeIpc, ipcErrorMessage, IpcError } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { useConfirm } from './useConfirm'
import i18n from '../i18n'
import type { MediaDetail } from '../types/media'
import {
  maximumAspectInscribedSize,
  STRAIGHTEN_MAX_DEGREES,
  STRAIGHTEN_MIN_DEGREES,
} from './straightenGeometry'
import { ADJUST_MAX, ADJUST_MIN } from './adjustFormula'

const t = (key: string, params?: Record<string, unknown>) => i18n.global.t(key, params ?? {})

export type EditOutputFormat = 'jpeg' | 'png'
export type EditStatus = 'idle' | 'editing' | 'saving' | 'done' | 'partial' | 'error'
export type CropRatioId = 'free' | 'original' | '1:1' | '4:3' | '3:4' | '16:9' | '9:16'

/** 归一化裁剪矩形(变换后坐标系,[0,1] 分数)。 */
export interface CropRectNorm {
  x: number
  y: number
  width: number
  height: number
}

export interface EditSaveResultSaved {
  status: 'saved'
  newItemId: number
}
export interface EditSaveResultPartial {
  status: 'savedNeedsIndex'
  pathHint: string
  recoveryCode: string
  rootId: number
}
export type EditSaveResult = EditSaveResultSaved | EditSaveResultPartial

const RATIO_VALUES: Record<Exclude<CropRatioId, 'free' | 'original'>, number> = {
  '1:1': 1,
  '4:3': 4 / 3,
  '3:4': 3 / 4,
  '16:9': 16 / 9,
  '9:16': 9 / 16,
}

/** 后端稳定 code → i18n 文案(方案 §6 码集;`file_job_busy` 为 A/B/C 共用)。 */
function messageForCode(code: string, fallback: string): string {
  switch (code) {
    case 'edit_source_unavailable':
      return t('edit.sourceUnavailable')
    case 'edit_format_unsupported':
      return t('edit.formatUnsupported')
    case 'edit_decode_failed':
      return t('edit.decodeFailed')
    case 'edit_encode_failed':
      return t('edit.encodeFailed')
    case 'edit_image_too_large':
      return t('edit.tooLarge')
    case 'edit_crop_empty':
      return t('edit.cropEmpty')
    case 'edit_invalid_ops':
      return t('edit.invalidOps')
    case 'edit_target_conflict':
      return t('edit.targetConflict')
    case 'file_job_busy':
      return t('edit.jobBusy')
    default:
      return fallback
  }
}

export function useImageEditor() {
  const status = ref<EditStatus>('idle')

  // 本次编辑会话的几何操作(在 detail.viewRotation 之上叠加;不改 view_rotation 本身)。
  const rotate = ref<0 | 90 | 180 | 270>(0)
  const flipH = ref(false)
  const flipV = ref(false)
  const rotateFine = ref(0)
  const crop = ref<CropRectNorm | null>(null)
  const cropRatio = ref<CropRatioId>('free')
  // E3 调色三滑杆(设计 §4.3):整数域 [-100,100];全零时保存载荷不携带 adjust(D-106)。
  const brightness = ref(0)
  const contrast = ref(0)
  const saturation = ref(0)

  const format = ref<EditOutputFormat>('jpeg')
  const quality = ref(92)

  const errorMessage = ref<string | null>(null)
  const savedNeedsIndex = ref<{ pathHint: string; rootId: number } | null>(null)

  // 源图在「文件 orientation 已应用一次」坐标系里的原始尺寸(EditOverlay 的 <img> onload 回填)。
  const naturalWidth = ref(0)
  const naturalHeight = ref(0)
  // 会话开始时的 view_rotation(已持久化;与本次 rotate 分离,后端 combine_rotation 同款语义)。
  const baseViewRotation = ref(0)

  /** 本次 rotate 与 baseViewRotation 合并后的净旋转步数(镜像后端 geometry::combine_rotation)。 */
  const combinedRotation = computed<0 | 90 | 180 | 270>(() => {
    const combined = (((baseViewRotation.value % 360) + 360) % 360) + rotate.value
    const mod = combined % 360
    return (mod === 90 || mod === 180 || mod === 270 ? mod : 0) as 0 | 90 | 180 | 270
  })

  /** fine rotate 前的尺寸；90/270 时宽高对换，flip 不改变尺寸。 */
  const preFineWidth = computed(() =>
    combinedRotation.value === 90 || combinedRotation.value === 270
      ? naturalHeight.value
      : naturalWidth.value,
  )
  const preFineHeight = computed(() =>
    combinedRotation.value === 90 || combinedRotation.value === 270
      ? naturalWidth.value
      : naturalHeight.value,
  )

  /** fine rotate 最大内接后的真实整数像素尺寸，与 Rust 共享闭式解和向下取整语义。 */
  const postFineSize = computed(() =>
    maximumAspectInscribedSize(preFineWidth.value, preFineHeight.value, rotateFine.value),
  )
  const postWidth = computed(() => postFineSize.value.width)
  const postHeight = computed(() => postFineSize.value.height)

  const hasChanges = computed(
    () =>
      rotate.value !== 0 ||
      flipH.value ||
      flipV.value ||
      rotateFine.value !== 0 ||
      crop.value !== null ||
      brightness.value !== 0 ||
      contrast.value !== 0 ||
      saturation.value !== 0,
  )

  function setNaturalSize(w: number, h: number): void {
    naturalWidth.value = w
    naturalHeight.value = h
  }

  /** 打开编辑会话:按当前 item 复位全部状态(方案 §7「打开编辑时预览初始状态含 view_rotation」)。 */
  function open(detail: MediaDetail): void {
    status.value = 'editing'
    rotate.value = 0
    flipH.value = false
    flipV.value = false
    rotateFine.value = 0
    crop.value = null
    cropRatio.value = 'free'
    brightness.value = 0
    contrast.value = 0
    saturation.value = 0
    format.value = 'jpeg'
    quality.value = 92
    errorMessage.value = null
    savedNeedsIndex.value = null
    naturalWidth.value = 0
    naturalHeight.value = 0
    baseViewRotation.value = detail.viewRotation
  }

  function close(): void {
    status.value = 'idle'
  }

  function resetOps(): void {
    rotate.value = 0
    flipH.value = false
    flipV.value = false
    rotateFine.value = 0
    crop.value = null
    cropRatio.value = 'free'
    brightness.value = 0
    contrast.value = 0
    saturation.value = 0
  }

  function rotateCw(): void {
    rotate.value = ((rotate.value + 90) % 360) as 0 | 90 | 180 | 270
    crop.value = null // 简化取舍:见文件头注释
  }
  function rotateCcw(): void {
    rotate.value = ((rotate.value + 270) % 360) as 0 | 90 | 180 | 270
    crop.value = null
  }
  function toggleFlipH(): void {
    flipH.value = !flipH.value
    crop.value = null
  }
  function toggleFlipV(): void {
    flipV.value = !flipV.value
    crop.value = null
  }

  /** 调色滑杆写入:非有限值归 0,clamp 到 [-100,100] 后取整(IPC 契约为 i8 整数域)。
   * 调色不改变几何坐标系,不清空裁剪框。 */
  function sanitizeAdjust(value: number): number {
    const finite = Number.isFinite(value) ? value : 0
    const clamped = Math.min(ADJUST_MAX, Math.max(ADJUST_MIN, finite))
    const next = Math.round(clamped)
    return Object.is(next, -0) ? 0 : next
  }
  function setBrightness(value: number): void {
    brightness.value = sanitizeAdjust(value)
  }
  function setContrast(value: number): void {
    contrast.value = sanitizeAdjust(value)
  }
  function setSaturation(value: number): void {
    saturation.value = sanitizeAdjust(value)
  }

  /** UI 只产生 0.1° 步进；改变拉直角度会改变 crop 坐标系，故与 90°/flip 同样清空旧框。 */
  function setRotateFine(value: number): void {
    const finite = Number.isFinite(value) ? value : 0
    const clamped = Math.min(
      STRAIGHTEN_MAX_DEGREES,
      Math.max(STRAIGHTEN_MIN_DEGREES, finite),
    )
    const next = Math.round(clamped * 10) / 10
    if (next === rotateFine.value) return
    rotateFine.value = Object.is(next, -0) ? 0 : next
    crop.value = null
  }

  /** 供拖拽手柄直接写入(已在 EditOverlay 里按比例锁/边界 clamp 好)。 */
  function setCropRect(rect: CropRectNorm | null): void {
    crop.value = rect
  }

  /** 键盘微调(方案 §7):按变换后坐标系的真实像素步进平移裁剪框,越界 clamp。 */
  function nudgeCrop(dxPixels: number, dyPixels: number): void {
    const c = crop.value
    const pw = postWidth.value
    const ph = postHeight.value
    if (!c || !pw || !ph) return
    const dx = dxPixels / pw
    const dy = dyPixels / ph
    const x = Math.min(Math.max(c.x + dx, 0), 1 - c.width)
    const y = Math.min(Math.max(c.y + dy, 0), 1 - c.height)
    crop.value = { x, y, width: c.width, height: c.height }
  }

  /** 按预设比例把裁剪框重置为居中最大内接矩形。`'free'` 只清比例锁,不强加新框。 */
  function setCropRatio(ratio: CropRatioId): void {
    cropRatio.value = ratio
    if (ratio === 'free') return
    const pw = postWidth.value
    const ph = postHeight.value
    if (!pw || !ph) return
    const target = ratio === 'original' ? pw / ph : RATIO_VALUES[ratio]
    let w: number
    let h: number
    if (pw / ph > target) {
      h = 1
      w = target / (pw / ph)
    } else {
      w = 1
      h = pw / ph / target
    }
    crop.value = { x: (1 - w) / 2, y: (1 - h) / 2, width: w, height: h }
  }

  /** 归一化裁剪矩形 → 变换后坐标系里的整数像素矩形(送后端 `EditOps.crop`)。 */
  function cropToPixelRect(): { x: number; y: number; width: number; height: number } | null {
    const c = crop.value
    const pw = postWidth.value
    const ph = postHeight.value
    if (!c || !pw || !ph) return null
    const x = Math.round(c.x * pw)
    const y = Math.round(c.y * ph)
    const width = Math.max(1, Math.round(c.width * pw))
    const height = Math.max(1, Math.round(c.height * ph))
    return { x, y, width, height }
  }

  async function save(itemId: number): Promise<EditSaveResult | null> {
    status.value = 'saving'
    errorMessage.value = null
    try {
      // D-106:三滑杆全零时不携带 adjust 字段,后端走 v1 字节级直通路径。
      const adjust =
        brightness.value === 0 && contrast.value === 0 && saturation.value === 0
          ? undefined
          : {
              brightness: brightness.value,
              contrast: contrast.value,
              saturation: saturation.value,
            }
      const result = await invokeIpc<EditSaveResult>(IPC.SAVE_EDITED_IMAGE, {
        itemId,
        ops: {
          rotate: rotate.value,
          flipH: flipH.value,
          flipV: flipV.value,
          rotateFine: rotateFine.value,
          crop: cropToPixelRect(),
          adjust,
        },
        output: {
          format: format.value,
          quality: format.value === 'jpeg' ? quality.value : undefined,
          fileName: undefined,
        },
      })
      if (result.status === 'savedNeedsIndex') {
        savedNeedsIndex.value = { pathHint: result.pathHint, rootId: result.rootId }
        status.value = 'partial'
      } else {
        status.value = 'done'
      }
      return result
    } catch (e) {
      const err = e instanceof IpcError ? e : null
      errorMessage.value = err
        ? messageForCode(err.code, ipcErrorMessage(e))
        : ipcErrorMessage(e)
      status.value = 'error'
      return null
    }
  }

  return {
    status,
    rotate,
    flipH,
    flipV,
    rotateFine,
    crop,
    cropRatio,
    brightness,
    contrast,
    saturation,
    format,
    quality,
    errorMessage,
    savedNeedsIndex,
    naturalWidth,
    naturalHeight,
    combinedRotation,
    preFineWidth,
    preFineHeight,
    postWidth,
    postHeight,
    hasChanges,
    setNaturalSize,
    open,
    close,
    resetOps,
    rotateCw,
    rotateCcw,
    toggleFlipH,
    toggleFlipV,
    setRotateFine,
    setBrightness,
    setContrast,
    setSaturation,
    setCropRect,
    setCropRatio,
    nudgeCrop,
    cropToPixelRect,
    save,
  }
}

export type ImageEditor = ReturnType<typeof useImageEditor>

/**
 * Esc / 取消按钮共用的退出判定(方案 §7:「Esc 退出时,仅在有改动时二次确认」)。
 * 无改动直接放行;有改动弹确认对话框,返回用户是否确认放弃。
 */
export async function requestDiscardEdits(editor: ImageEditor): Promise<boolean> {
  if (!editor.hasChanges.value) return true
  const { confirm } = useConfirm()
  const result = await confirm({
    title: t('edit.discardConfirmTitle'),
    message: t('edit.discardConfirmMessage'),
    confirmText: t('edit.discardConfirmOk'),
    cancelText: t('edit.discardConfirmCancel'),
    danger: true,
  })
  return result.confirmed
}
