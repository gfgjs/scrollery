// src/utils/zhConvert.ts
// 简繁转换的「显示层」施加(阅读器方案 R4,§5.10/§5.13)。在替换规则之后、对已渲染的章文档 DOM
// 就地转换文本节点。关键契约:**只改渲染 DOM、不动后端 canonical 字符流**——故未来 loc1 的字符
// 偏移锚点不受简繁切换影响(见 types/reader.ts 的 ReaderLocator 契约)。转换走后端 ferrous-opencc
// (convert_chinese 命令,Vec<String>→Vec<String> 批量),一章一次 IPC(数组边界天然充当 opencc
// 短语转换的「不跨节点合并」隔离)。

/**
 * 其内文本不参与转换的元素:脚本/样式绝不动;代码块(含 md renderMarkdown 产出的 `<pre><code>`)
 * 保留原文,避免把代码里的中文注释/标识符也一并转换掉。
 */
const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'CODE', 'PRE', 'TEXTAREA'])

/** 文本节点是否因其祖先标签而应跳过转换。纯函数,便于 node 环境单测(vitest 无 DOM)。 */
export function shouldSkipByTag(tagName: string | null | undefined): boolean {
  return !!tagName && SKIP_TAGS.has(tagName.toUpperCase())
}

/**
 * 后端返回的转换结果是否可安全写回:必须是数组且长度与送出的文本节点数**严格相等**。
 * 不等(后端异常/协议漂移)则整章跳过,绝不半程写回污染 DOM。纯函数,可单测。
 */
export function isConvertResultUsable(
  sentCount: number,
  converted: unknown,
): converted is string[] {
  return Array.isArray(converted) && converted.length === sentCount
}

/** 文本节点是否位于「不转换」子树内(自其父元素向上,任一祖先命中 SKIP_TAGS 即跳过)。 */
function isInSkippedSubtree(node: Node): boolean {
  let el = node.parentElement
  while (el) {
    if (shouldSkipByTag(el.tagName)) return true
    el = el.parentElement
  }
  return false
}

/**
 * 对章文档根元素就地施加简繁转换。收集可转文本节点(跳过 SKIP_TAGS 子树)→ 整批交 convert
 * (一次 IPC)→ 按下标写回。长度不匹配整章跳过。convert 抛错交由调用方吞(单章失败不阻断渲染)。
 *
 * 注:vitest environment=node 无 DOM,本函数的 TreeWalker 遍历不被单测覆盖(薄封装);其两处
 * 风险点(跳过判定 shouldSkipByTag / 长度守卫 isConvertResultUsable)已由上方纯函数单测锁定。
 */
export async function applyZhConvertToDom(
  root: HTMLElement,
  convert: (texts: string[]) => Promise<string[]>,
): Promise<void> {
  const doc = root.ownerDocument
  if (!doc) return
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (node: Node) =>
      isInSkippedSubtree(node) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  })
  const nodes: Text[] = []
  for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text)
  if (nodes.length === 0) return

  const texts = nodes.map((n) => n.nodeValue ?? '')
  const converted = await convert(texts)
  // 写回前守卫:长度须严格相等,否则整章保持原文(宁可不转,不可错位)。
  if (!isConvertResultUsable(texts.length, converted)) return
  for (let i = 0; i < nodes.length; i++) nodes[i].nodeValue = converted[i]
}
