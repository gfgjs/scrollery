import type Vditor from 'vditor'

const formatSelector = 'strong[data-newline="1"], em[data-newline="1"]'
const markerSelector = '.vditor-ir__marker'
const wrapperSelector = 'span[data-type="strong"], span[data-type="em"]'

function isPlainFormatParagraph(paragraph: HTMLElement): boolean {
  return Array.from(paragraph.querySelectorAll('*')).every(element => {
    if (element.matches(formatSelector) || element.matches(wrapperSelector)) return true
    return element.tagName === 'SPAN' && element.matches(markerSelector) &&
      element.parentElement?.matches(wrapperSelector) === true && element.children.length === 0
  })
}

function visibleOffset(paragraph: HTMLElement, node: Node, offset: number): number {
  const prefix = paragraph.ownerDocument.createRange()
  prefix.selectNodeContents(paragraph)
  prefix.setEnd(node, offset)
  const fragment = prefix.cloneContents()
  fragment.querySelectorAll(markerSelector).forEach(marker => marker.remove())
  return fragment.textContent?.length ?? 0
}

function textPoint(element: HTMLElement, offset: number): [Node, number] {
  const walker = element.ownerDocument.createTreeWalker(element, NodeFilter.SHOW_TEXT, {
    acceptNode: node => node.parentElement?.closest(markerSelector) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  })
  let node: Node | null
  let last: Node | null = null
  while ((node = walker.nextNode())) {
    const length = node.textContent?.length ?? 0
    if (offset <= length) return [node, offset]
    offset -= length
    last = node
  }
  return last ? [last, last.textContent?.length ?? 0] : [element, 0]
}

function mergeFormats(element: Element): void {
  element.normalize()
  for (const child of Array.from(element.children)) {
    mergeFormats(child)
    if (!child.textContent) {
      child.remove()
      continue
    }
    // 相邻斜体若各自保留标记会成为 *FI**ST*，须先合并语义节点再交给 Lute。
    while (child.nextSibling instanceof Element && child.nextSibling.tagName === child.tagName) {
      const next = child.nextSibling
      child.append(...next.childNodes)
      next.remove()
      mergeFormats(child)
    }
  }
}

/**
 * 修复 IR 同一顶层段落内，嵌套粗体／斜体跨边界选区的删除和单字符替换。
 * 只接受纯文本及 strong/em；返回 true 时已消费键盘事件，并通过 onChange 通知源文变化。
 * 撤销沿用 Vditor 既有契约：恢复原文和选区起点，重做恢复新光标。
 */
export function handleVditorNestedSelection(
  editor: Vditor,
  event: KeyboardEvent,
  onChange: (source: string) => void,
): boolean {
  const state = editor.vditor
  const ir = state.ir
  const undo = state.undo
  if (state.currentMode !== 'ir' || event.defaultPrevented || event.isComposing ||
    !ir || !undo || ir.composingLock ||
    event.ctrlKey || event.metaKey || event.altKey || event.shiftKey ||
    (event.key !== 'Backspace' && event.key !== 'Delete' && event.key.length !== 1)) return false
  const root = ir.element
  if (!(event.target instanceof Node) || !root.contains(event.target) || !root.isContentEditable) return false
  const selection = root.ownerDocument.getSelection()
  if (!selection?.rangeCount) return false
  const selected = selection.getRangeAt(0)
  if (selected.collapsed || selected.startContainer.nodeType !== Node.TEXT_NODE ||
    selected.endContainer.nodeType !== Node.TEXT_NODE) return false
  const startElement = selected.startContainer.parentElement
  const endElement = selected.endContainer.parentElement
  const paragraph = startElement?.closest('p')
  if (!paragraph || paragraph.parentElement !== root || paragraph !== endElement?.closest('p') ||
    startElement?.closest(markerSelector) || endElement?.closest(markerSelector) || !isPlainFormatParagraph(paragraph)) return false
  const first = startElement?.closest(formatSelector)
  const last = endElement?.closest(formatSelector)
  if (first === last || !(first?.parentElement?.closest(formatSelector) || last?.parentElement?.closest(formatSelector))) return false

  // 在副本上完成语义修改和转换，确认能原位替换一个段落后才触碰真实正文与撤销栈。
  const start = visibleOffset(paragraph, selected.startContainer, selected.startOffset)
  const end = visibleOffset(paragraph, selected.endContainer, selected.endOffset)
  const copy = paragraph.cloneNode(true) as HTMLElement
  copy.querySelectorAll(markerSelector).forEach(marker => marker.remove())
  copy.querySelectorAll(wrapperSelector).forEach(wrapper => wrapper.replaceWith(...wrapper.childNodes))
  const edit = root.ownerDocument.createRange()
  edit.setStart(...textPoint(copy, start))
  edit.setEnd(...textPoint(copy, end))
  edit.deleteContents()
  mergeFormats(copy)
  let caretOffset = start
  if (event.key.length === 1) {
    const insertion = root.ownerDocument.createRange()
    insertion.setStart(...textPoint(copy, start))
    insertion.collapse(true)
    insertion.insertNode(root.ownerDocument.createTextNode(event.key))
    caretOffset += event.key.length
    copy.normalize()
  }
  const converted = root.ownerDocument.createElement('div')
  converted.innerHTML = state.lute.Md2VditorIRDOM(state.lute.HTML2Md(copy.outerHTML))
  const replacement = converted.firstElementChild
  if (converted.childElementCount !== 1 || !(replacement instanceof HTMLElement) || replacement.tagName !== 'P') return false

  event.preventDefault()
  event.stopImmediatePropagation()
  const scrollTop = root.scrollTop
  const scrollLeft = root.scrollLeft
  undo.addToUndoStack(state)
  paragraph.replaceWith(replacement)
  const caret = root.ownerDocument.createRange()
  caret.setStart(...textPoint(replacement, caretOffset))
  caret.collapse(true)
  selection.removeAllRanges()
  selection.addRange(caret)
  // 前模式的延迟任务不能在本事务之后追加一个只有光标差异的撤销项。
  window.clearTimeout(ir.processTimeoutId)
  window.clearTimeout(state.sv?.processTimeoutId)
  window.clearTimeout(state.wysiwyg?.afterRenderTimeoutId)
  undo.addToUndoStack(state)
  root.scrollTop = scrollTop
  root.scrollLeft = scrollLeft
  onChange(editor.getValue())
  return true
}
