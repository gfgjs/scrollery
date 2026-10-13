const blockTags = new Set(['P', 'DIV', 'PRE', 'LI', 'TR', 'TD', 'TH', 'BLOCKQUOTE', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'BR', 'HR'])
const previewSelector = '.vditor-ir__preview, .vditor-wysiwyg__preview'

/** 查找当前编辑模式的可见正文，结果不修改编辑器 DOM、选区或撤销历史。 */
export function findVditorMatches(root: HTMLElement, query: string): Range[] {
  if (!query) return []
  const expression = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu')
  const matches: Range[] = []
  const visible = new WeakMap<Element, boolean>()
  let nodes: Text[] = []
  let text = ''

  const isVisible = (element: Element): boolean => {
    const cached = visible.get(element)
    if (cached !== undefined) return cached
    const style = root.ownerDocument.defaultView!.getComputedStyle(element)
    const result = style.display !== 'none' && style.visibility !== 'hidden' && style.visibility !== 'collapse'
    visible.set(element, result)
    return result
  }
  const flush = () => {
    expression.lastIndex = 0
    let nodeIndex = 0
    let nodeStart = 0
    for (const match of text.matchAll(expression)) {
      const start = match.index
      const end = start + match[0].length
      while (nodeStart + nodes[nodeIndex]!.nodeValue!.length <= start) {
        nodeStart += nodes[nodeIndex++]!.nodeValue!.length
      }
      const range = root.ownerDocument.createRange()
      range.setStart(nodes[nodeIndex]!, start - nodeStart)
      let endIndex = nodeIndex
      let endStart = nodeStart
      while (endStart + nodes[endIndex]!.nodeValue!.length < end) {
        endStart += nodes[endIndex++]!.nodeValue!.length
      }
      range.setEnd(nodes[endIndex]!, end - endStart)
      matches.push(range)
    }
    nodes = []
    text = ''
  }
  const visit = (node: Node): void => {
    if (node.nodeType === 3) {
      if (node.nodeValue) {
        nodes.push(node as Text)
        text += node.nodeValue
      }
      return
    }
    if (node.nodeType !== 1) return
    const element = node as Element
    if (!isVisible(element) || element.tagName === 'SCRIPT' || element.tagName === 'STYLE') return
    const preview = element.nextElementSibling
    // IR 与所见即所得均可能同时显示代码源文和预览，只搜索可见预览一次。
    if (preview?.matches(previewSelector) && isVisible(preview)) return
    if (element.classList.contains('vditor-ir__marker') && !element.classList.contains('vditor-ir__marker--pre')) return
    const block = blockTags.has(element.tagName)
    if (block) flush()
    for (const child of element.childNodes) visit(child)
    if (block) flush()
  }
  visit(root)
  flush()
  return matches
}
