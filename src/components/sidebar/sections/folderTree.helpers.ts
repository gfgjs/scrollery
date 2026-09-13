// FoldersSection 显示行拍平逻辑（与组件分离以便单测，承接 timelineScrubber.helpers 惯例）。
// 纯函数、无 DOM / 无响应式：把「目录前序 DFS + 各目录自身文件」拍平成线性行数组。
// 这是文件树虚拟化（T1）改造的地基与零测试命门路径 —— 先抽出锁行为（characterization），
// 再在其上做窗口化。行为须与 FoldersSection 原 inline displayRows 逐字节一致。
import { File, Film, Music, FileText, Image } from '@lucide/vue'
import type { DirNode, DirFile, MediaType } from '../../../types/media'
import { isMobilePlatform } from '../../../utils/platform'

// ── FS-only 能力边界（S 线 §4.1 / D-013）──────────────────────────────────────
//
// 「所有文件」模式让**磁盘上有、库里没有**的目录与文件出现在树里。它们只有路径身份，没有
// 实体身份，故一切「拿 id 去查库」的能力对它们一律不开放。设计文档把这叫「FS-only 目录的
// 能力边界」——那是个有名字的概念，代码里也该有名字：散成十几处 `node.id === null` 既测不了、
// 也搜不到，新增调用点时更没人提醒该判。
//
// 🔴 铁律：**禁止伪造** `id`（路径 hash、临时序号皆不可）。伪造不会报错 —— `mediaRoute.ts`
// 对非 doc/audio 兜底到 `/view/{id}`，一个假 id 会把 `.exe` 静默送进图片查看器。

/**
 * 该目录节点是否具备**实体身份**（媒体库里确有这一行）。
 *
 * `false` ⇒ 禁止：进入 `/folder/:id`、画廊滚动锚点、拖拽源/目标、移动/复制、active 高亮。
 * 仍允许：展开、键盘导航、刷新、复制路径 —— 这些只需要路径身份。
 *
 * 写成**类型谓词**而非返回 `boolean`：那样调用方还得靠 `node.id!` 把 id 断言出来，等于
 * 把类型系统刚给的保护又扔掉。谓词收窄之后，`id` 只有过了这道门才拿得到 —— 「先判后用」
 * 从约定变成编译期强制。
 */
export function hasEntityIdentity<T extends { id: number | null }>(
  node: T,
): node is T & { id: number } {
  return node.id !== null
}

/**
 * 实体 id 判等 —— **任一侧为 null 恒不等,含两侧皆 null**。
 *
 * 样式绑定(active 底色 / drag-over 内环 / drag-source 半透明)的判等专用。裸 `===` 对
 * `number | null` 类型完全合法,但 `null === null` 恒真:无任何选中/拖拽时各静止值都是 null,
 * 每个 FS-only 目录行会同时命中三类实体轴样式(2026-07-16 真机问题 1,审查 R-01)。
 * 类型谓词(`hasEntityIdentity`)守得住动作面,守不住判等 —— 编译器对「语义要求非空」的
 * 比较零信号(F-020),故收敛成具名函数,由单测钉死 null-null 不等。
 */
export function sameEntityId(a: number | null, b: number | null): boolean {
  return a !== null && a === b
}

/**
 * 该文件是否可在**应用内**打开。
 *
 * `false` ⇒ 单击只选中；Desktop 双击经 `reveal_tree_entry` 在文件管理器中显示（§4.2）。
 * 两个字段都要判：`id` 是路由目标，`mediaType` 决定分发到哪个查看器 —— 缺任一都无从打开。
 */
export function isOpenableInApp<T extends { id: number | null; mediaType: MediaType | null }>(
  file: T,
): file is T & { id: number; mediaType: MediaType } {
  return file.id !== null && file.mediaType !== null
}

/**
 * 纯文本预览白名单(问题②方案 B v1,D-002)。**镜像后端 `TEXT_PREVIEW_EXTS`,后端才是安全
 * 边界** —— 这里只做 UX 预筛(决定双击走预览还是 reveal、tooltip 提示哪种动作)。放宽/收窄
 * 必须两侧同步,否则要么白发起必败请求,要么可预览文件被误导去文件管理器。
 */
const TEXT_PREVIEW_EXTS = new Set(['txt', 'md', 'markdown'])

/**
 * 该文件名(按扩展名)是否可走应用内**只读文本预览**。
 *
 * 仅在 `isOpenableInApp` 为 false 的文件行上有意义:可正常打开的文件走查看器路由,
 * 轮不到预览。大小写不敏感,与后端 `to_lowercase` 同姿态。
 */
export function isTextPreviewable(fileName: string): boolean {
  const dot = fileName.lastIndexOf('.')
  if (dot <= 0 || dot === fileName.length - 1) return false // 无扩展名/点开头/点结尾
  return TEXT_PREVIEW_EXTS.has(fileName.slice(dot + 1).toLowerCase())
}

/**
 * 文件行拖拽的落点合法性(文件版 `canDropOnId`,D-003)。文件无子树 ⇒ 无环检测;仅拒
 * 「落回当前所在目录」(后端 relocate 对同目录本就 no-op,这里预筛只为不显示误导性落点高亮)。
 *
 * @param fileParentDirId 文件当前所在目录的实体 id;FS 模式下父目录可能取不到实体 id,传 null
 *                        即放行(宁可让后端 no-op,不可误杀合法落点)
 * @param targetDirId 候选落点目录的实体 id(调用方已保证来自实体索引)
 */
export function canDropFileOnDir(fileParentDirId: number | null, targetDirId: number): boolean {
  return fileParentDirId === null || targetDirId !== fileParentDirId
}

/**
 * 该目录是否可展开（决定是否画 chevron、方向键 → 是否有展开语义）。
 *
 * 此前这条判据以 `hasChildren || mediaCount > 0` 的形态**散在 4 处**（模板 3 处 + `treeKeyTarget`
 * + `expandAll`），而「所有文件」模式恰恰要改的就是它 —— 散着改必漏，漏了还没有编译/测试信号，
 * 只在真机上表现为「某些目录点不开」。故收敛为单一事实源。
 *
 * 三种情形：
 * - `hasChildren === null`（**未知**，「所有文件」模式未枚举前）→ **假定可展开**，展开时才知道。
 *   实际为空则展开后无子行 —— 与资源管理器/VSCode 一致，代价只是一次空展开，而反面
 *   （不给箭头）会让真有内容的目录**永远点不开**。
 * - `hasChildren === true` → 确有子目录。
 * - `hasChildren === false` → 无子目录，但仍可能有直接文件（`mediaCount > 0`）。
 *
 * `mediaCount` 用 `?? 0` 而非真值判断：null（不适用）与 0（确实没有）都不构成可展开理由，
 * 但二者语义不同，别让 `!mediaCount` 把它们揉成一个。
 */
export function isExpandable(node: Pick<DirNode, 'hasChildren' | 'mediaCount'>): boolean {
  if (node.hasChildren === null) return true
  return node.hasChildren || (node.mediaCount ?? 0) > 0
}

export interface DirRow {
  kind: 'dir'
  node: DirNode
}
export interface FileRow {
  kind: 'file'
  file: DirFile
  depth: number
  /** 归属目录的**路径身份**(D-013)。用 nodeKey 而非 DB id:FS-only 目录没有 id。 */
  dirKey: string
}
// 「加载更多」占位行(T2 单目录文件分页):某目录还有未加载的直接文件时,排在其已加载文件之后。
// 点击后追加下一页。loadedCount 供展示「已加载 N 个」。
export interface MoreRow {
  kind: 'more'
  /** 归属目录的路径身份(D-013)。 */
  dirKey: string
  depth: number
  loadedCount: number
}
export type TreeRow = DirRow | FileRow | MoreRow

/**
 * 把目录前序 DFS（folderTree.nodes）拍平成线性显示行：文件夹优先（VSCode 风格）——某目录内先列
 * 子文件夹（及其已展开子树），再列该目录「自身」的文件。
 *
 * 关键不变量（fa1 陷阱的地基，§9.4 M5）：目录的自身文件排在其整个子树「之后」（靠 pending 栈延迟
 * 发射），因此 `FileRow.dirKey` 才是文件的真实归属目录 —— 「向上扫描最近的 dir 行」会误判（一个目录
 * 的尾随文件其前一个 dir 行往往是它的子文件夹）。虚拟化后 sticky 覆盖头必须用 dirKey 反查。
 *
 * @param nodes 目录前序 DFS 数组（每节点带 depth / expanded / files）
 */
export function flattenFolderRows(nodes: readonly DirNode[]): TreeRow[] {
  const out: TreeRow[] = []
  // 待发射文件的展开目录栈：其文件在自身子树之后发射。
  const pending: DirNode[] = []
  const flush = (depth: number) => {
    while (pending.length && pending[pending.length - 1].depth >= depth) {
      const dir = pending.pop()!
      for (const f of dir.files!) {
        out.push({ kind: 'file', file: f, depth: dir.depth + 1, dirKey: dir.nodeKey })
      }
      // 分页:本目录仍有未加载文件 → 在已加载文件之后追加「加载更多」行(T2)。
      if (dir.filesHasMore) {
        out.push({
          kind: 'more',
          dirKey: dir.nodeKey,
          depth: dir.depth + 1,
          loadedCount: dir.files!.length,
        })
      }
    }
  }
  for (const node of nodes) {
    flush(node.depth) // 冲刷在此节点前结束的子树
    out.push({ kind: 'dir', node })
    if (node.expanded && node.files && node.files.length) pending.push(node)
  }
  flush(-1) // 冲刷剩余全部
  return out
}

// ── 方案 B（共享滚动）虚拟化坐标数学（T1-a，§9.4 M4）───────────────────────────
// 文件树不自持 scrollTop，复用侧栏共享滚动区（.sidebar__scroll-area）。窗口由「共享 scrollTop
// 减去树在滚动内容中的顶部偏移」折算，避免改动 accordion 的单一块级流 + 跨区 sticky 架构。
// 定高（ROW_H 统一）→ 无需 prefix-sum/二分，startIndex = floor(相对滚动 / ROW_H)。

export interface TreeWindow {
  /** 可视窗口起始行下标（含）。 */
  startIndex: number
  /** 可视窗口结束行下标（不含，供 slice）。 */
  endIndex: number
  /** 渲染层整体下移量（= startIndex * ROW_H），钉住可视行的绝对位置。 */
  offsetY: number
}

/**
 * 计算可视窗口（定高，方案 B 共享滚动）。
 * @param scrollTop 共享滚动区当前 scrollTop
 * @param treeOffsetTop 树容器顶部在滚动内容坐标系中的偏移（= treeRect.top − scrollRect.top + scrollTop）
 * @param viewportH 共享滚动区可视高（clientHeight；略偏大即多渲几行，由 buffer 吸收，无害）
 * @param rowCount 总行数（displayRows.length）
 * @param rowH 统一行高 ROW_H
 * @param buffer 上下预渲染行数（防快滚边缘瞬白）
 */
export function computeTreeWindow(
  scrollTop: number,
  treeOffsetTop: number,
  viewportH: number,
  rowCount: number,
  rowH: number,
  buffer: number,
): TreeWindow {
  if (rowCount <= 0 || rowH <= 0) return { startIndex: 0, endIndex: 0, offsetY: 0 }
  // 相对滚动：视口顶落在树内的第几像素（负=尚未滚到树，取 0 起）。
  const relTop = scrollTop - treeOffsetTop
  const firstVisible = Math.floor(relTop / rowH)
  const rowsInView = Math.ceil(viewportH / rowH)
  const startIndex = Math.max(0, Math.min(rowCount, firstVisible - buffer))
  const endIndex = Math.max(startIndex, Math.min(rowCount, firstVisible + rowsInView + buffer))
  return { startIndex, endIndex, offsetY: startIndex * rowH }
}

/**
 * 求「把第 index 行滚入可视区」应设的共享 scrollTop（block:'nearest' 语义，替代虚拟化后失效的
 * scrollIntoView，§9.4 M5）。已可见则不动（返回 curScrollTop）。
 *
 * insets:视口上/下边缘被不透明浮层占据的高度——上=粘顶的区块标题堆叠+粘性目录头链、
 * 下=粘底的区块标题堆叠。不扣除的话「对齐顶」会把目标行恰好塞到浮层底下(可见性判定同理)。
 *
 * @param topInset 视口顶不可用高(px),默认 0
 * @param bottomInset 视口底不可用高(px),默认 0
 */
export function treeScrollTopForIndex(
  index: number,
  treeOffsetTop: number,
  curScrollTop: number,
  viewportH: number,
  rowH: number,
  topInset = 0,
  bottomInset = 0,
): number {
  const top = treeOffsetTop + index * rowH
  const bottom = top + rowH
  if (top - topInset < curScrollTop) return top - topInset // 在(有效)视口上方 → 对齐浮层之下
  if (bottom + bottomInset > curScrollTop + viewportH) return bottom + bottomInset - viewportH // 下方 → 对齐浮层之上
  return curScrollTop // 已可见 → 不动
}

/**
 * 自动加载更多(T2 分页免手点):从当前渲染窗口行中找第一个可自动触发的「加载更多」行。
 * 窗口含上下 buffer,行刚接近视口即触发,相当于就近预取;一页加载完拍平行变化后再次调用,
 * 行若仍在窗口内则接力下一页,直到取尽(hasMore 翻 false,more 行消失)或滚出窗口。
 * blocked 中的目录(加载失败/无进展)不参与自动加载,只允许手点重试——防「滚进视口→失败→
 * 再触发」的无限循环。
 * @returns 目标目录的**路径身份**(nodeKey);窗口内无可触发行时 null
 */
export function firstAutoLoadTarget(
  rows: readonly TreeRow[],
  blocked: ReadonlySet<string>,
): string | null {
  for (const r of rows) {
    if (r.kind === 'more' && !blocked.has(r.dirKey)) return r.dirKey
  }
  return null
}

// ── 画廊→文件树同步队列（latest-wins）────────────────────────────────────────

/**
 * 串行消费目录定位请求，并在当前请求尚未完成时只保留最后一个新目标。
 *
 * 大库拖动 timeline slider 时，`scrolledDirectoryId` 可在一帧内跨过多个目录；每次定位又可能
 * 需要异步加载多级祖先。若全部并发，较早的深路径可能比新目标更晚 resolve，最终用旧目录覆盖
 * 新目录的滚动位置。这里把并发扇出收敛为「一个在途 + 一个最新候选」，`isLatest` 同时让调用方
 * 在 expand 后和真正写 scrollTop 前各做一次失效检查。
 */
export function createFolderTreeSyncQueue(
  sync: (dirId: number, isLatest: () => boolean) => Promise<void>,
): { request: (dirId: number | null) => Promise<void> } {
  let generation = 0
  let queued: { dirId: number; generation: number } | null = null
  let draining: Promise<void> | null = null

  const drain = async () => {
    try {
      while (queued) {
        const current = queued
        queued = null
        await sync(current.dirId, () => current.generation === generation)
      }
    } finally {
      // 与最后一次 queued 检查处于同一 microtask，避免外层 Promise.finally 尚未执行时的新请求丢失。
      draining = null
    }
  }

  const request = (dirId: number | null): Promise<void> => {
    const requestGeneration = ++generation
    queued = dirId === null ? null : { dirId, generation: requestGeneration }
    if (!draining && queued) {
      // 先把 Promise 写入 draining，再在 microtask 启动 drain；防 sync 在首次 await 前重入 request。
      draining = Promise.resolve().then(drain)
    }
    return draining ?? Promise.resolve()
  }

  return { request }
}

// ── 粘性目录头栈(T1-b,§9.4)──────────────────────────────────────────────────
// T1-a 移除逐行 position:sticky 后,滚长文件列表时文件夹名不再钉顶(UX 回退)。此函数复刻原
// 堆叠面包屑效果:给定视口顶所在行,回扫求其「已展开祖先目录链」,由覆盖层依深度堆叠钉住。

/** 取某行的缩进深度(dir=node.depth,file/more 已按 dir.depth+1 存于 row.depth)。 */
function rowDepth(row: TreeRow): number {
  return row.kind === 'dir' ? row.node.depth : row.depth
}

/**
 * 计算视口顶行的「粘性祖先目录链」(根/最浅在前)。从 topIndex 向上回扫,收集**严格递减深度**的目录
 * 行——即视口顶那一行的各层祖先目录。凡出现在拍平行中且其后有后代行的目录必然已展开,故无需额外
 * 判 expanded。深度递减保证只取真祖先、跳过前面的兄弟子树(承接 fa1 陷阱同款不变量)。
 *
 * 性能:回扫在遇到深度 0 祖先时终止;T2 分页把单目录行数封顶(≤FILE_PAGE+1),故到最近父目录的回扫
 * 距离有界(≤一页),整体 O(一页 × 树深),不随目录文件总数增长。
 *
 * @param rows 拍平行数组
 * @param topIndex 视口顶所在行下标(firstVisible = floor(相对滚动 / rowH));超界自动钳
 * @param maxDepth 最多钉几层(防极深树占满视口),默认 6
 * @returns 目录节点链,depth 递增(根在前);无祖先(如滚到顶/根目录行在顶)时为空
 */
// ── 键盘导航(树模式)──────────────────────────────────────────────────────
// 纯决策函数:给定拍平行、当前 active 行下标与按键,返回「该做什么」;DOM/滚动/展开副作用
// 全留在组件侧。虚拟化下组件容器持焦承接本函数的 move 结果。

export type TreeKeyAction =
  /** 把 active 行移到 index(组件负责滚入可视区)。 */
  | { kind: 'move'; index: number }
  /** 展开/折叠该目录(方向键语义已保证:→ 只对折叠态发出、← 只对展开态发出,组件直接 toggle)。 */
  | { kind: 'toggle'; node: DirNode }
  /** 激活当前行(dir=导航、file=打开、more=加载,由组件按行类分发)。 */
  | { kind: 'activate' }

/**
 * APG tree 键盘语义 → 动作。无动作(键不相关/已到边界/叶子无展开语义)返回 null,
 * 组件据此决定是否 preventDefault。
 *
 * ← 跳父的正确性依据 fa1 不变量:当前行上方「首个严格更浅的行」必是其祖先目录——兄弟及其
 * 子树同深或更深,而父目录自身文件排在父的整个子树之后(即当前行之下),不会挡在扫描路径上。
 *
 * @param rows 拍平显示行(displayRows)
 * @param activeIndex 当前 active 行下标;-1/越界视为「尚无 active」(初次聚焦由组件落 0,
 *                    此处仅对 ↓/↑/Home 落首行、End 落末行,其余键不动作)
 * @param key KeyboardEvent.key
 */
export function treeKeyTarget(
  rows: readonly TreeRow[],
  activeIndex: number,
  key: string,
): TreeKeyAction | null {
  if (rows.length === 0) return null
  const last = rows.length - 1
  const cur = activeIndex >= 0 && activeIndex <= last ? rows[activeIndex] : null
  if (!cur) {
    if (key === 'ArrowDown' || key === 'ArrowUp' || key === 'Home') return { kind: 'move', index: 0 }
    if (key === 'End') return { kind: 'move', index: last }
    return null
  }
  switch (key) {
    case 'ArrowDown':
      return activeIndex < last ? { kind: 'move', index: activeIndex + 1 } : null
    case 'ArrowUp':
      return activeIndex > 0 ? { kind: 'move', index: activeIndex - 1 } : null
    case 'Home':
      return { kind: 'move', index: 0 }
    case 'End':
      return { kind: 'move', index: last }
    case 'ArrowRight': {
      if (cur.kind !== 'dir') return null // 文件/more 是叶子,无展开语义
      const expandable = isExpandable(cur.node) // 与模板 chevron 同一事实源
      if (!expandable) return null
      if (!cur.node.expanded) return { kind: 'toggle', node: cur.node }
      // 已展开 → 进首子行(拍平序中紧随其后且更深的行);展开但子树未出行(加载中/空)→ 不动。
      const next = rows[activeIndex + 1]
      return next && rowDepth(next) > cur.node.depth
        ? { kind: 'move', index: activeIndex + 1 }
        : null
    }
    case 'ArrowLeft': {
      if (cur.kind === 'dir' && cur.node.expanded) return { kind: 'toggle', node: cur.node }
      const depth = rowDepth(cur)
      for (let i = activeIndex - 1; i >= 0; i--) {
        if (rowDepth(rows[i]) < depth) {
          // 依 fa1 不变量必为祖先目录;kind 守卫纯防御。
          return rows[i].kind === 'dir' ? { kind: 'move', index: i } : null
        }
      }
      return null // 已在根层
    }
    case 'Enter':
    case ' ':
      return { kind: 'activate' }
    default:
      return null
  }
}

// Type icon for a file leaf. | 文件叶子的类型图标。
//
// `null` = **未入库的文件**(格式未注册,或在被扫描器剪掉的隐藏目录里)——我们从没解析过它,
// 不知道它是什么。必须给通用文件图标而**不能**落到 Image:那会给一个 .exe 画张照片图标,
// 暗示它能在应用内打开,而它只能 reveal(§4.2)。故 null 单列一支,不与 default 合并。
export function fileIcon(type: MediaType | null) {
  if (type === null) return File
  switch (type) {
    case 'video':
      return Film
    case 'audio':
      return Music
    case 'document':
      return FileText
    default:
      return Image
  }
}

// 文件行 tooltip:未入库的文件明说「为什么单击只选中、双击去了哪」——此前只有文件名,
// 用户面对「.md 明明认识却打不开」零线索(2026-07-16 真机问题 2,方案 A affordance)。
// 三分支:纯文本白名单 → 双击预览(问题②方案 B,移动端同样可用);其余按平台承诺
// reveal 或明说不支持(移动端 reveal 不可用,D-001)。
//
// @param t i18n 翻译函数,由调用方(组件)传入以保持本函数纯——去状态化(拆分方案 §2.1 补充下沉)。
export function fileTitle(
  file: DirFile,
  t: (key: string) => string,
  isMobile: boolean = isMobilePlatform,
): string {
  if (isOpenableInApp(file)) return file.fileName
  const hint = isTextPreviewable(file.fileName)
    ? t('sidebar.unregisteredFileHintPreviewable')
    : isMobile
      ? t('sidebar.unregisteredFileHintMobile')
      : t('sidebar.unregisteredFileHint')
  return `${file.fileName}\n${hint}`
}

export function stickyHeaderChain(
  rows: readonly TreeRow[],
  topIndex: number,
  maxDepth = 6,
): DirNode[] {
  if (rows.length === 0 || topIndex < 0) return []
  const chain: DirNode[] = []
  let neededDepth = Infinity
  const start = Math.min(topIndex, rows.length - 1)
  for (let i = start; i >= 0; i--) {
    const row = rows[i]
    const depth = rowDepth(row)
    if (depth >= neededDepth) continue // 同级或更深 → 是兄弟子树/自身文件,跳过
    neededDepth = depth
    if (row.kind === 'dir') {
      chain.push(row.node)
      if (chain.length >= maxDepth || depth === 0) break
    }
    // 非目录行(file/more)只收窄 neededDepth 继续向更浅祖先找,不入链。
  }
  return chain.reverse() // 根/最浅在前,供覆盖层自上而下堆叠
}
