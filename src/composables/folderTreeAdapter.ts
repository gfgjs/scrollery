// src/composables/folderTreeAdapter.ts
// 「所有文件」模式的取数结果 → 文件树统一模型（S 线 §4.1 / D-013）。
//
// 纯函数、无 IPC 无响应式：`useFolderTree` 负责发请求与写 store，本文件只负责「后端给的
// TreeEntry 怎么变成 DirNode/DirFile」。抽出来是为了可单测 —— 两种数据源汇成同一棵树的
// 正确性全在这层，而它不该需要一个 Tauri 运行时才能验。
//
// **不推导 nodeKey**：`nodeKey`/`parentKey` 一律照抄后端（唯一实现在 `crate::tree::node_key`）。
// 前端自己拼 `${rootId}:${relPath}` 就是跨语言的第二份实现，分歧不报错，只是同一目录在两种
// 模式下变成两个节点。

import type { DirFile, DirNode, TreeEntry } from '../types/media'

/**
 * FS 目录条目 → `DirNode`。
 *
 * `mediaCount`/`hasChildren` 恒为 `null`（**未知/不适用**，见 `types/media.ts` 的说明）：
 * - 递归媒体数在一棵正在列全部文件的树上是「媒体数冒充文件数」（§4.2 明禁）；
 * - 有没有子目录，不 `read_dir` 它就不知道 —— 而那对一个上万项的目录意味着上万次枚举。
 *
 * 两者都**不用 0/false 冒充**：那是断言「没有」，而事实是「不知道」。
 */
export function fsEntryToDirNode(entry: TreeEntry, depth: number): DirNode {
  return {
    nodeKey: entry.nodeKey,
    parentKey: entry.parentKey,
    // 实体身份：只有库里确有这一行才给。后端 `skip_serializing_if` 使字段缺席，
    // `?? null` 把「缺席」归一为 null（DirNode.id 的契约是 `number | null`，不是可选属性）。
    id: entry.directoryId ?? null,
    rootId: entry.rootId,
    // parentId 是**父目录的库行 id**。后端对目录条目下发 parentDirectoryId（= 被列目录自身的
    // 库行 id，父是 FS-only 时缺席）——FS 模式下库内目录因此保有完整实体父链，拖拽移动/复制
    // 照常可用（R-07 修复：此前恒 null，连库内目录也被拖拽入口第一关挡掉，比 §4.1 更紧）。
    // FS-only 目录自身仍被 hasEntityIdentity（id===null）拦在动作面之外，不受此字段影响。
    // 结构关系仍一律走 parentKey（D-013）。
    parentId: entry.parentDirectoryId ?? null,
    name: entry.name,
    relPath: entry.relPath,
    depth,
    mediaCount: null,
    hasChildren: null,
    // 隐藏项标记透传（R-06）：后端契约明言「前端要据此加样式」，adapter 丢弃它就是端到端断链
    // ——语义（模式过滤）后端已正确，样式端零信号。
    hidden: entry.hidden,
  }
}

/**
 * FS 文件条目 → `DirFile`。
 *
 * `mediaType` 缺席 ⇒ `null` ⇒ `isOpenableInApp()` 为假 ⇒ 单击只选中、双击 reveal（§4.2）。
 * 🔴 绝不给它编一个默认类型（比如 `'image'`）：`mediaRoute.ts` 会照单全收，把一个 `.exe`
 * 送进图片查看器。
 */
export function fsEntryToDirFile(entry: TreeEntry): DirFile {
  return {
    nodeKey: entry.nodeKey,
    parentKey: entry.parentKey,
    relPath: entry.relPath,
    id: entry.mediaId ?? null,
    fileName: entry.name,
    mediaType: entry.mediaType ?? null,
    // FS 枚举不查收藏表。未入库的文件本就不可能被收藏；已入库的（隐藏目录里的 PNG 除外）
    // 在「所有文件」模式下丢失收藏心形属于已知取舍 —— 为一页文件多查一次 favorites 不值当，
    // 且该模式的用途是「看磁盘上有什么」，不是浏览收藏。
    isFavorited: false,
    hidden: entry.hidden, // 同目录侧（R-06）：淡化样式的唯一数据源
  }
}

/**
 * 一页 FS 条目 → 子目录与文件两组。
 *
 * 之所以能分两组而不是混着给：后端 `list_tree_entries` 接 `kind` 过滤，且 `cmp_entries`
 * 保证目录恒排在文件之前 —— 前端模型（子目录注入拍平数组 / 文件挂 `node.files` 分页）与
 * DB 模式因此完全平行，只有一套模型。
 *
 * @param depth 子项深度（= 父目录 depth + 1）
 */
export function adaptTreeEntries(
  entries: readonly TreeEntry[],
  depth: number,
): { dirs: DirNode[]; files: DirFile[] } {
  const dirs: DirNode[] = []
  const files: DirFile[] = []
  for (const e of entries) {
    if (e.kind === 'dir') dirs.push(fsEntryToDirNode(e, depth))
    else files.push(fsEntryToDirFile(e))
  }
  return { dirs, files }
}
