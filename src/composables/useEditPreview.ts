// E0 编辑预览资源：消费 Rust raw packet，创建/释放 blob URL，并暴露全尺寸坐标元数据。

import { ref } from 'vue'

import { IPC } from '../constants/ipc'
import { invokeIpc, type IpcError } from '../utils/ipc'

const HEADER_LEN = 28

export interface EditPreviewPacket {
  mimeType: 'image/jpeg' | 'image/png'
  sourceWidth: number
  sourceHeight: number
  previewWidth: number
  previewHeight: number
  encoded: Uint8Array
}

function asBytes(raw: ArrayBuffer | Uint8Array | number[]): Uint8Array {
  if (raw instanceof Uint8Array) return raw
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw)
  return Uint8Array.from(raw)
}

export function parseEditPreviewPacket(
  raw: ArrayBuffer | Uint8Array | number[],
): EditPreviewPacket {
  const bytes = asBytes(raw)
  if (bytes.byteLength < HEADER_LEN) throw new Error('edit preview packet is truncated')
  if (bytes[0] !== 0x53 || bytes[1] !== 0x45 || bytes[2] !== 0x50 || bytes[3] !== 0x32) {
    throw new Error('edit preview packet magic mismatch')
  }
  if (bytes[4] !== 1) throw new Error('edit preview packet version unsupported')
  const mimeType = bytes[5] === 1 ? 'image/jpeg' : bytes[5] === 2 ? 'image/png' : null
  if (!mimeType) throw new Error('edit preview packet format unsupported')

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const sourceWidth = view.getUint32(8, true)
  const sourceHeight = view.getUint32(12, true)
  const previewWidth = view.getUint32(16, true)
  const previewHeight = view.getUint32(20, true)
  const bodyLength = view.getUint32(24, true)
  if (
    !sourceWidth ||
    !sourceHeight ||
    !previewWidth ||
    !previewHeight ||
    bodyLength !== bytes.byteLength - HEADER_LEN
  ) {
    throw new Error('edit preview packet dimensions or body length invalid')
  }
  return {
    mimeType,
    sourceWidth,
    sourceHeight,
    previewWidth,
    previewHeight,
    encoded: bytes.slice(HEADER_LEN),
  }
}

/** 把预览像素坐标映射回 orientation 已烤入的全尺寸源坐标。 */
export function previewPointToSource(
  x: number,
  y: number,
  packet: Pick<
    EditPreviewPacket,
    'sourceWidth' | 'sourceHeight' | 'previewWidth' | 'previewHeight'
  >,
): { x: number; y: number } {
  return {
    x: (x / packet.previewWidth) * packet.sourceWidth,
    y: (y / packet.previewHeight) * packet.sourceHeight,
  }
}

export function useEditPreview() {
  const url = ref<string | null>(null)
  const sourceWidth = ref(0)
  const sourceHeight = ref(0)
  const previewWidth = ref(0)
  const previewHeight = ref(0)
  const loading = ref(false)
  const error = ref<IpcError | Error | null>(null)

  function dispose(): void {
    if (url.value) URL.revokeObjectURL(url.value)
    url.value = null
  }

  async function load(itemId: number): Promise<void> {
    dispose()
    loading.value = true
    error.value = null
    try {
      const raw = await invokeIpc<ArrayBuffer | Uint8Array | number[]>(IPC.GET_EDIT_PREVIEW, {
        itemId,
      })
      const packet = parseEditPreviewPacket(raw)
      sourceWidth.value = packet.sourceWidth
      sourceHeight.value = packet.sourceHeight
      previewWidth.value = packet.previewWidth
      previewHeight.value = packet.previewHeight
      // 复制到独立 ArrayBuffer，避免某些 WebView 对带 offset 的 Uint8Array BlobPart 兼容差异。
      const blobBytes = Uint8Array.from(packet.encoded)
      url.value = URL.createObjectURL(new Blob([blobBytes], { type: packet.mimeType }))
    } catch (cause) {
      error.value = cause as IpcError | Error
    } finally {
      loading.value = false
    }
  }

  return {
    url,
    sourceWidth,
    sourceHeight,
    previewWidth,
    previewHeight,
    loading,
    error,
    load,
    dispose,
  }
}
