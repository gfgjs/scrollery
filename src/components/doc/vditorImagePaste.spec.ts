import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type Vditor from 'vditor'
import { createVditorImagePasteController, type ImagePasteBarrierResult } from './vditorImagePaste'

const IMAGE_MARKDOWN = '![粘贴图片](data:image/png;base64,QUJD)'

// 控制器依赖 Node/Element、FileReader 与 window.clearTimeout；Node 环境下手动替身，由用例显式控制读取时序。
class TestNode {}

class TestElement extends TestNode {
  closest(): Element | null { return null }
}

const createdReaders: TestFileReader[] = []

class TestFileReader {
  static readonly EMPTY = 0
  static readonly LOADING = 1
  static readonly DONE = 2
  // 模拟 readAsDataURL 对无效 blob 的同步抛错。
  static throwOnRead = false
  readyState = TestFileReader.EMPTY
  result: string | null = null
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  onabort: (() => void) | null = null
  readAsDataURL = vi.fn(() => {
    if (TestFileReader.throwOnRead) throw new Error('invalid blob')
    this.readyState = TestFileReader.LOADING
  })
  abort = vi.fn(() => {
    this.readyState = TestFileReader.DONE
    this.onabort?.()
  })
  constructor() { createdReaders.push(this) }
  /** 模拟读取成功；结果仍由控制器自行校验。 */
  finish(result: string): void {
    this.result = result
    this.readyState = TestFileReader.DONE
    this.onload?.()
  }
  fail(): void {
    this.readyState = TestFileReader.DONE
    this.onerror?.()
  }
}

beforeEach(() => {
  createdReaders.length = 0
  TestFileReader.throwOnRead = false
  vi.stubGlobal('Node', TestNode)
  vi.stubGlobal('Element', TestElement)
  vi.stubGlobal('FileReader', TestFileReader)
  vi.stubGlobal('window', { clearTimeout: vi.fn() })
})

afterEach(() => {
  createdReaders.length = 0
  TestFileReader.throwOnRead = false
  vi.unstubAllGlobals()
})

function createHarness(initial = '# 原文') {
  const startNode = new TestElement()
  const endNode = new TestElement()
  const selection = {
    rangeCount: 1,
    ranges: [] as unknown[],
    getRangeAt: () => ({ startContainer: startNode, endContainer: endNode, cloneRange: () => ({ startContainer: startNode, endContainer: endNode }) }),
    removeAllRanges() { selection.ranges = [] },
    addRange(range: unknown) { selection.ranges = [range] },
  }
  const root = {
    isConnected: true,
    isContentEditable: true,
    focus: vi.fn(),
    contains: () => true,
    ownerDocument: {
      getSelection: () => selection,
      createElement: () => ({ alt: '', src: '', outerHTML: '<img>' }),
    },
  }
  const state = {
    currentMode: 'ir' as 'ir' | 'sv' | 'wysiwyg',
    ir: { element: root },
    undo: { addToUndoStack: vi.fn() },
    lute: { HTML2Md: () => IMAGE_MARKDOWN },
  }
  let value = initial
  const editor = {
    vditor: state,
    getValue: () => value,
    insertMD: vi.fn((markdown: string) => { value = `${value}\n\n${markdown}` }),
  } as unknown as Vditor
  return {
    editor,
    state,
    get value() { return value },
    setValue: (next: string) => { value = next },
  }
}

function pasteEvent(target: unknown): ClipboardEvent {
  return {
    defaultPrevented: false,
    clipboardData: { files: [{ type: 'image/png', name: 'shot.png' }], getData: () => '' },
    target,
    preventDefault: vi.fn(),
    stopImmediatePropagation: vi.fn(),
  } as unknown as ClipboardEvent
}

it('读取完成前屏障保持未决，完成后才插入图片并更新正文', async () => {
  const harness = createHarness()
  const onChange = vi.fn()
  const onRetry = vi.fn()
  const controller = createVditorImagePasteController(harness.editor, { onChange, onRetry })
  expect(controller.handle(pasteEvent(new TestElement()))).toBe(true)
  expect(createdReaders[0]?.readAsDataURL).toHaveBeenCalledWith(expect.objectContaining({ type: 'image/png' }))

  let resolved: ImagePasteBarrierResult | null = null
  const barrier = controller.waitForPendingInputs().then((result) => { resolved = result; return result })
  await Promise.resolve()
  expect(resolved).toBeNull()

  createdReaders[0]!.finish('data:image/png;base64,QUJD')
  expect(await barrier).toBe('settled')
  expect(onRetry).not.toHaveBeenCalled()
  expect(onChange).toHaveBeenLastCalledWith(`# 原文\n\n${IMAGE_MARKDOWN}`)
  expect(await controller.waitForPendingInputs()).toBe('idle')
})

it('读取失败时提示重试且屏障判定失败', async () => {
  const harness = createHarness()
  const onChange = vi.fn()
  const onRetry = vi.fn()
  const controller = createVditorImagePasteController(harness.editor, { onChange, onRetry })
  controller.handle(pasteEvent(new TestElement()))
  const barrier = controller.waitForPendingInputs()
  createdReaders[0]!.fail()
  expect(await barrier).toBe('failed')
  expect(onRetry).toHaveBeenCalledTimes(1)
  expect(onChange).not.toHaveBeenCalled()
  expect(harness.value).toBe('# 原文')
})

it('读取完成前正文已变时丢弃过期结果并提示重试', async () => {
  const harness = createHarness()
  const onChange = vi.fn()
  const onRetry = vi.fn()
  const controller = createVditorImagePasteController(harness.editor, { onChange, onRetry })
  controller.handle(pasteEvent(new TestElement()))
  harness.setValue('# 变化后的正文')
  const barrier = controller.waitForPendingInputs()
  createdReaders[0]!.finish('data:image/png;base64,QUJD')
  expect(await barrier).toBe('failed')
  expect(onRetry).toHaveBeenCalledTimes(1)
  expect(onChange).not.toHaveBeenCalled()
  expect(harness.value).toBe('# 变化后的正文')
})

it('卸载取消在途读取并安静丢弃迟到结果', async () => {
  const harness = createHarness()
  const onChange = vi.fn()
  const onRetry = vi.fn()
  const controller = createVditorImagePasteController(harness.editor, { onChange, onRetry })
  controller.handle(pasteEvent(new TestElement()))
  const barrier = controller.waitForPendingInputs()
  const reader = createdReaders[0]!
  controller.dispose()
  expect(reader.abort).toHaveBeenCalledTimes(1)
  expect(await barrier).toBe('failed')
  // 迟到的读取结果不能进入已卸载编辑器。
  reader.finish('data:image/png;base64,QUJD')
  expect(onChange).not.toHaveBeenCalled()
  expect(onRetry).not.toHaveBeenCalled()
  expect(harness.value).toBe('# 原文')
})

