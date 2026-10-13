import type Vditor from 'vditor'
import { findVditorMatches } from './vditorSearch'

export interface VditorModeCaret {
  text: string
  offset: number
  viewportY: number
}

function uniqueMatch(root: HTMLElement, text: string): Range | null {
  const matches = findVditorMatches(root, text).filter(range => range.toString() === text)
  return matches.length === 1 ? matches[0]! : null
}

/** 只记录当前可见的单光标；真实文字必须唯一，范围选区与不明确的位置不参与映射。 */
export function captureVditorModeCaret(editor: Vditor): VditorModeCaret | null {
  const state = editor.vditor
  const root = state[state.currentMode]?.element
  const selection = root?.ownerDocument.getSelection()
  if (!root?.isContentEditable || !selection?.rangeCount) return null
  const range = selection.getRangeAt(0)
  const node = range.startContainer
  if (!range.collapsed || node.nodeType !== Node.TEXT_NODE || !root.contains(node) ||
    node.parentElement?.closest('code, .vditor-ir__marker, .vditor-ir__preview, .vditor-wysiwyg__preview')) return null
  const caret = range.getBoundingClientRect()
  const viewport = root.getBoundingClientRect()
  if (caret.height <= 0 || caret.top < viewport.top || caret.bottom > viewport.bottom) return null
  const start = Math.max(0, range.startOffset - 64)
  const text = node.textContent!.slice(start, range.startOffset + 64)
  if (!text.trim()) return null
  const match = uniqueMatch(root, text)
  if (!match || match.comparePoint(node, range.startOffset) !== 0) return null
  return { text, offset: range.startOffset - start, viewportY: caret.top - viewport.top }
}

/** 在目标模式唯一命中的同一文字中恢复光标；只调整原生选区和滚动，不改正文或撤销历史。 */
export function restoreVditorModeCaret(editor: Vditor, anchor: VditorModeCaret): boolean {
  const state = editor.vditor
  const root = state[state.currentMode]?.element
  if (!root?.isContentEditable) return false
  const match = uniqueMatch(root, anchor.text)
  if (!match) return false
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  walker.currentNode = match.startContainer
  let node: Node | null = match.startContainer
  let offset = anchor.offset
  let start = match.startOffset
  while (node) {
    const end = node === match.endContainer ? match.endOffset : node.textContent!.length
    if (offset <= end - start) {
      const caret = root.ownerDocument.createRange()
      caret.setStart(node, start + offset)
      caret.collapse(true)
      root.focus({ preventScroll: true })
      const selection = root.ownerDocument.getSelection()
      selection?.removeAllRanges()
      selection?.addRange(caret)
      const bounds = caret.getBoundingClientRect()
      // 各模式行高和 Markdown 标记不同，保留原视口高度而非按全文比例猜输入位置。
      root.scrollTop += bounds.top - root.getBoundingClientRect().top - Math.min(anchor.viewportY, root.clientHeight - bounds.height)
      return true
    }
    offset -= end - start
    if (node === match.endContainer) return false
    node = walker.nextNode()
    start = 0
  }
  return false
}
