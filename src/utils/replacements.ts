// src/utils/replacements.ts
// 替换规则的前端应用（§5.2，Lite 路径用 JS；Perf 路径走原生 aho-corasick）。字面量规则合并为
// 单次扫描（按 find 长度降序 → 最长匹配优先，避免级联）；正则规则按 sort_order 顺序逐条应用。
// 中文无词边界，故不加 \b。

export interface ReplacementRule {
  id: number
  scopeKind: string
  scopeId: number | null
  find: string
  replace: string
  isRegex: boolean
  enabled: boolean
  sortOrder: number
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/// 由规则集构建一个 `(text) => text` 替换函数。无规则时返回恒等函数。
export function buildReplacer(rules: ReplacementRule[]): (text: string) => string {
  const literals = rules.filter((r) => r.enabled && !r.isRegex && r.find)
  const regexes = rules.filter((r) => r.enabled && r.isRegex && r.find)
  if (!literals.length && !regexes.length) return (t) => t

  let literalRe: RegExp | null = null
  const literalMap = new Map<string, string>()
  if (literals.length) {
    // 长度降序：正则交替优先尝试更长的 find（最长匹配优先）。
    const sorted = [...literals].sort((a, b) => b.find.length - a.find.length)
    for (const r of sorted) if (!literalMap.has(r.find)) literalMap.set(r.find, r.replace)
    literalRe = new RegExp(sorted.map((r) => escapeRegExp(r.find)).join('|'), 'g')
  }

  const compiled = regexes
    .map((r) => {
      try {
        return { re: new RegExp(r.find, 'g'), to: r.replace }
      } catch {
        return null // 非法正则 → 跳过
      }
    })
    .filter((x): x is { re: RegExp; to: string } => x !== null)

  return (text: string): string => {
    let out = text
    if (literalRe) out = out.replace(literalRe, (m) => literalMap.get(m) ?? m)
    for (const { re, to } of compiled) {
      re.lastIndex = 0
      out = out.replace(re, to)
    }
    return out
  }
}

/// 对一个 DOM 子树的所有文本节点就地应用替换（供 epub.js content 钩子使用）。
export function applyReplacerToDom(root: Node, replace: (t: string) => string): void {
  const doc = root.ownerDocument || document
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const nodes: Text[] = []
  let n: Node | null
  while ((n = walker.nextNode())) nodes.push(n as Text)
  for (const t of nodes) {
    const v = t.nodeValue ?? ''
    const nv = replace(v)
    if (nv !== v) t.nodeValue = nv
  }
}
