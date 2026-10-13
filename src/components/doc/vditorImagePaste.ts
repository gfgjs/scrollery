import type Vditor from 'vditor'

interface ImagePasteCallbacks {
  onChange: (source: string) => void
  onRetry: () => void
}

export type ImagePasteBarrierResult = 'idle' | 'settled' | 'failed'

export interface VditorImagePasteController {
  handle: (event: ClipboardEvent | DragEvent) => boolean
  /**
   * 等待已接收但尚未落定的图片输入并消费其落定结果：
   * `idle` 无新输入；`settled` 新输入已插入正文；`failed` 有输入未进入正文（读取失败、冲突或取消）。
   * 失败结果保留到下一次调用消费，避免写盘期间新接收并快速失败的图片被 pending 集合漏过。
   */
  waitForPendingInputs: () => Promise<ImagePasteBarrierResult>
  dispose: () => void
}

/**
 * 将二进制图片读取绑定到当前编辑器及原选区；沿用内嵌 data URI，不改变文字、HTML 或代码粘贴。
 * 原文／模式变化时提示重试，原实例卸载时取消读取并安静丢弃迟到结果。
 * 在途读取纳入保存屏障：保存与导航先等已接收输入落定，失败或取消时不以旧正文误判为已保存。
 */
export function createVditorImagePasteController(editor: Vditor, callbacks: ImagePasteCallbacks): VditorImagePasteController {
  let disposed = false
  const readers = new Set<FileReader>()
  // 已接收但尚未落定的图片输入；保存与导航据此等待，旧正文不能代表全部输入已保存。
  const pending = new Set<Promise<boolean>>()
  // 已落定但尚未被保存屏障消费的失败结果；不能只靠 pending 判断写盘期间新接收的快速失败。
  let failedUnconsumed = false

  function trackInput(input: Promise<boolean>): void {
    pending.add(input)
    void input.then((ok) => {
      pending.delete(input)
      if (!ok) failedUnconsumed = true
    })
  }

  function handle(event: ClipboardEvent | DragEvent): boolean {
    if (disposed || event.defaultPrevented) return false
    const data = 'clipboardData' in event ? event.clipboardData : event.dataTransfer
    // HTML、纯文本和多文件的取舍保持上游优先级；仅接管既有 files[0] 图片路径。
    if (!data || data.getData('text/html').trim() || data.getData('text/plain')) return false
    const file = data.files[0]
    if (!file?.type.startsWith('image/')) return false
    const state = editor.vditor
    const mode = state.currentMode
    const root = state[mode]?.element
    const undo = state.undo
    if (!root?.isConnected || !root.isContentEditable || !undo || !(event.target instanceof Node) || !root.contains(event.target)) return false
    const selection = root.ownerDocument.getSelection()
    if (!selection?.rangeCount) return false
    const range = selection.getRangeAt(0).cloneRange()
    const startNode = range.startContainer
    const endNode = range.endContainer
    if (!root.contains(startNode) || !root.contains(endNode)) return false
    const startElement = startNode instanceof Element ? startNode : startNode.parentElement
    const endElement = endNode instanceof Element ? endNode : endNode.parentElement
    if (startElement?.closest('code, [data-type="code-block"]') || endElement?.closest('code, [data-type="code-block"]')) return false
    const before = editor.getValue()
    event.preventDefault()
    event.stopImmediatePropagation()
    const reader = new FileReader()
    readers.add(reader)
    const alive = () => !disposed && root.isConnected
    // 每个已接收输入以一条 Promise 表示落定结果；读取失败、冲突与取消都结算为 false，供保存屏障拒绝误放行。
    let settleInput!: (ok: boolean) => void
    trackInput(new Promise<boolean>((settle) => {
      settleInput = settle
      reader.onerror = () => {
        readers.delete(reader)
        if (alive()) callbacks.onRetry()
        settle(false)
      }
      reader.onabort = () => {
        readers.delete(reader)
        settle(false)
      }
      reader.onload = () => {
        readers.delete(reader)
        if (!alive()) {
          settle(false)
          return
        }
        // Range 会随 DOM 变动自动迁移；原节点与源文必须同时仍有效，不能只检查 range 是否还在根内。
        if (state.currentMode !== mode || !root.isContentEditable || !root.contains(startNode) || !root.contains(endNode) ||
          editor.getValue() !== before || typeof reader.result !== 'string') {
          callbacks.onRetry()
          settle(false)
          return
        }
        const image = root.ownerDocument.createElement('img')
        image.alt = file.name
        image.src = reader.result
        const markdown = state.lute.HTML2Md(image.outerHTML).trimEnd()
        root.focus({ preventScroll: true })
        selection.removeAllRanges()
        selection.addRange(range)
        undo.addToUndoStack(state)
        // 使用原实例的公开 Markdown 插入流程，图片在当前模式中立即渲染并替换已捕获选区。
        editor.insertMD(markdown)
        window.clearTimeout(state.ir?.processTimeoutId)
        window.clearTimeout(state.sv?.processTimeoutId)
        window.clearTimeout(state.wysiwyg?.afterRenderTimeoutId)
        undo.addToUndoStack(state)
        callbacks.onChange(editor.getValue())
        settle(true)
      }
    }))
    try {
      reader.readAsDataURL(file)
    } catch {
      // readAsDataURL 对无效 blob 会同步抛出；按读取失败结算并提示，不能让输入永不落定而挂住保存。
      readers.delete(reader)
      callbacks.onRetry()
      settleInput(false)
    }
    return true
  }

  async function waitForPendingInputs(): Promise<ImagePasteBarrierResult> {
    let settledInputs = false
    // 等待期间仍可能接收新的图片输入；直到没有在途输入且全部落定才算完成。
    while (pending.size) {
      await Promise.all([...pending])
      settledInputs = true
    }
    // 失败结果只消费一次：本次阻止放行，下一次保存可重新尝试。
    if (failedUnconsumed) {
      failedUnconsumed = false
      return 'failed'
    }
    return settledInputs ? 'settled' : 'idle'
  }

  return {
    handle,
    waitForPendingInputs,
    dispose: () => {
      disposed = true
      for (const reader of readers) {
        if (reader.readyState === FileReader.LOADING) reader.abort()
      }
      readers.clear()
    },
  }
}
