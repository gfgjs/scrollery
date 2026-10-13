import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref, shallowRef } from 'vue'
import { useVideoFrameCapture } from './useVideoFrameCapture'
import { IPC } from '../../constants/ipc'

const { save, invokeIpc, addToast, logError } = vi.hoisted(() => ({
  save: vi.fn<(...args: unknown[]) => Promise<string | null>>(),
  invokeIpc: vi.fn<(...args: unknown[]) => Promise<void>>(),
  addToast: vi.fn(),
  logError: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save }))
vi.mock('../../utils/ipc', () => ({ invokeIpc }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('../../utils/logger', () => ({ logger: { error: logError } }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

function setup() {
  const video = {
    videoWidth: 1920, videoHeight: 1080, currentTime: 65,
    readyState: 4, frame: 'frame-A',
  }
  const fileName = ref('视频A.mp4')
  let pixels = ''
  const drawImage = vi.fn((source: typeof video) => {
    // HAVE_METADATA 只有尺寸；没有当前帧时 drawImage 不绘制视频像素。
    if (source.readyState >= 2) pixels = source.frame
  })
  const toBlob = vi.fn<(callback: BlobCallback, type: string) => void>((callback) => {
    callback(new Blob([pixels], { type: 'image/png' }))
  })
  const canvas = { width: 0, height: 0, getContext: () => ({ drawImage }), toBlob }
  vi.stubGlobal('document', { createElement: vi.fn(() => canvas) })
  const capture = useVideoFrameCapture(shallowRef(video as unknown as HTMLVideoElement), fileName)
  return { video, fileName, drawImage, toBlob, canvas, capture, pixels: () => pixels }
}

beforeEach(() => {
  vi.resetAllMocks()
  save.mockResolvedValue('/mock/capture.png')
  invokeIpc.mockResolvedValue(undefined)
  // 只编码内存中的测试数据；保存对话框及写文件 IPC 都是替身。
  vi.stubGlobal('FileReader', class {
    result: string | null = null
    onloadend: (() => void) | null = null
    readAsDataURL(blob: Blob) {
      void blob.arrayBuffer().then((bytes) => {
        this.result = `data:image/png;base64,${Buffer.from(bytes).toString('base64')}`
        this.onloadend?.()
      })
    }
  })
})
afterEach(() => vi.unstubAllGlobals())

describe('视频截帧就绪状态与媒体归属', () => {
  it('已有尺寸但未解码当前帧时不导出空白画布，帧就绪后可正常重试', async () => {
    const { video, drawImage, capture } = setup()
    video.readyState = 1
    await capture.captureFrame()
    expect(save).not.toHaveBeenCalled()
    expect(invokeIpc).not.toHaveBeenCalled()
    expect(drawImage).not.toHaveBeenCalled()
    expect(capture.busy.value).toBe(false)

    video.readyState = 2
    await capture.captureFrame()
    expect(drawImage).toHaveBeenCalledExactlyOnceWith(video, 0, 0, 1920, 1080)
    expect(invokeIpc).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_FRAME_PNG, {
      targetPath: '/mock/capture.png', dataBase64: Buffer.from('frame-A').toString('base64'),
    })
    expect(addToast).toHaveBeenCalledExactlyOnceWith('success', 'player.captureSaved')
  })

  it('编码和保存对话框等待期间切项，仍保存点击时的像素、文件名与时间', async () => {
    const { video, fileName, toBlob, capture, pixels } = setup()
    const selectedPath = deferred<string | null>()
    save.mockReturnValueOnce(selectedPath.promise)
    let finishEncoding!: BlobCallback
    toBlob.mockImplementationOnce((callback) => { finishEncoding = callback })
    const pending = capture.captureFrame()
    expect(capture.busy.value).toBe(true)
    expect(pixels()).toBe('frame-A')

    fileName.value = '视频B.mp4'
    video.frame = 'frame-B'
    video.currentTime = 200
    finishEncoding(new Blob([pixels()], { type: 'image/png' }))
    await Promise.resolve()
    expect(save).toHaveBeenCalledExactlyOnceWith({
      defaultPath: '视频A_00-01-05.png', filters: [{ name: 'PNG', extensions: ['png'] }],
    })

    fileName.value = '视频C.mp4'
    video.frame = 'frame-C'
    await capture.captureFrame()
    expect(toBlob).toHaveBeenCalledOnce()
    selectedPath.resolve('/mock/selected.png')
    await pending
    expect(invokeIpc).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_FRAME_PNG, {
      targetPath: '/mock/selected.png', dataBase64: Buffer.from('frame-A').toString('base64'),
    })
    expect(capture.busy.value).toBe(false)
  })

  it('取消保存不写入文件并释放忙态，下次截帧仍可保存', async () => {
    const { capture } = setup()
    save.mockResolvedValueOnce(null)
    await capture.captureFrame()
    expect(invokeIpc).not.toHaveBeenCalled()
    expect(addToast).not.toHaveBeenCalled()
    expect(capture.busy.value).toBe(false)
    await capture.captureFrame()
    expect(invokeIpc).toHaveBeenCalledOnce()
    expect(capture.busy.value).toBe(false)
  })
})
