import { describe, it, expect } from 'vitest'
import {
  flattenFolderRows,
  computeTreeWindow,
  treeScrollTopForIndex,
  stickyHeaderChain,
  firstAutoLoadTarget,
  treeKeyTarget,
  createFolderTreeSyncQueue,
  hasEntityIdentity,
  isOpenableInApp,
  isTextPreviewable,
  canDropFileOnDir,
  isExpandable,
  sameEntityId,
} from './folderTree.helpers'
import type { DirNode, DirFile } from '../../../types/media'

// characterization：锁 FoldersSection displayRows 拍平行为（文件夹优先 + 自身文件排在子树之后 +
// FileRow.dirKey 真实归属），为文件树虚拟化（T1）改造铺安全网。fa1 陷阱是 sticky 覆盖头正确性地基。

// nodeKey 有意取 'k<id>' 而**不是** String(id)：若键与 id 同形，一个误用 id 的实现照样能
// 让本文件全绿（样本无区分度）。形态不同才使「拍平行走的是路径身份」成为可证伪断言（D-013）。
const file = (id: number, name = 'f' + id): DirFile =>
  ({ id, nodeKey: 'fk' + id, fileName: name }) as unknown as DirFile
const dir = (
  id: number,
  depth: number,
  opts: {
    expanded?: boolean
    files?: DirFile[]
    filesHasMore?: boolean
    hasChildren?: boolean
    mediaCount?: number
  } = {},
): DirNode =>
  ({
    id,
    nodeKey: 'k' + id,
    depth,
    expanded: opts.expanded ?? false,
    files: opts.files,
    filesHasMore: opts.filesHasMore ?? false,
    hasChildren: opts.hasChildren ?? false,
    mediaCount: opts.mediaCount ?? 0,
  }) as unknown as DirNode

// 便于断言：dir 行渲成 'D<id>'、file 行渲成 'f<fileId>'、more 行渲成 'M<dirKey>'。
const shape = (rows: ReturnType<typeof flattenFolderRows>) =>
  rows.map((r) =>
    r.kind === 'dir' ? 'D' + r.node.id : r.kind === 'more' ? 'M' + r.dirKey : 'f' + r.file.id,
  )

// ── FS-only 能力边界(§4.1 / D-013)────────────────────────────────────────────
describe('FS-only 能力边界', () => {
  it('有实体 id 的目录:允许实体操作', () => {
    expect(hasEntityIdentity({ id: 7 })).toBe(true)
  })

  it('FS-only 目录(id 为 null):禁止实体操作', () => {
    expect(hasEntityIdentity({ id: null })).toBe(false)
  })

  /**
   * `id: 0` 是 SQLite 不会发的行号,但**判据必须是「是不是 null」而不是「真不真」** ——
   * 写成 `!!node.id` / `node.id ? ... : ...` 的实现会把 0 判成「无实体身份」。这类真值判断
   * 在数值 id 上是经典哑弹:平时不响,遇到 0 就静默禁掉一个本该可用的目录。
   */
  it('id 为 0 仍算有实体身份(判据是 null 而非真值)', () => {
    expect(hasEntityIdentity({ id: 0 })).toBe(true)
  })

  it('已入库文件:可在应用内打开', () => {
    expect(isOpenableInApp({ id: 5, mediaType: 'image' })).toBe(true)
  })

  /**
   * 🔴 两个字段**各自**都是否决项。分开钉是因为「id 有而 mediaType 无」是真实可达的形态
   * (库里有行、但类型未知),而 `openMediaRoute` 正是按 mediaType 分发 —— 只判 id 的实现
   * 会让它走到兜底分支 `/view/{id}`,把一个类型未知的文件送进图片查看器。
   */
  it('未入库文件(id 与 mediaType 皆无):不可在应用内打开', () => {
    expect(isOpenableInApp({ id: null, mediaType: null })).toBe(false)
  })

  it('只缺 mediaType 也不可打开(仅判 id 的实现会漏)', () => {
    expect(isOpenableInApp({ id: 5, mediaType: null })).toBe(false)
  })

  it('只缺 id 也不可打开(仅判 mediaType 的实现会漏)', () => {
    expect(isOpenableInApp({ id: null, mediaType: 'image' })).toBe(false)
  })

  it('id 为 0 且类型已知:可打开(同上,判据是 null 非真值)', () => {
    expect(isOpenableInApp({ id: 0, mediaType: 'image' })).toBe(true)
  })
})

// ── 实体 id 判等(R-01 回归钉子)──────────────────────────────────────────────
describe('sameEntityId', () => {
  /**
   * 🔴 **null-null 恒不等**是本函数存在的全部理由:模板样式绑定原先裸写
   * `viewStore.activeDirectoryId === row.node.id`,对 `number | null` 类型完全合法,但
   * 无任何选中/拖拽时两边都是 null → 恒真 → 每个 FS-only 目录行同时挂上
   * active+drag-over+drag-source 三类样式(2026-07-16 真机问题 1)。
   */
  it('两侧皆 null:不等(裸 === 在此恒真,正是真机问题 1 根因)', () => {
    expect(sameEntityId(null, null)).toBe(false)
  })

  it('任一侧 null:不等', () => {
    expect(sameEntityId(null, 3)).toBe(false)
    expect(sameEntityId(3, null)).toBe(false)
  })

  it('两侧同为非空同值:相等', () => {
    expect(sameEntityId(7, 7)).toBe(true)
  })

  it('两侧非空不同值:不等', () => {
    expect(sameEntityId(7, 8)).toBe(false)
  })

  it('id 为 0 参与判等(判据是 null 非真值,同 hasEntityIdentity)', () => {
    expect(sameEntityId(0, 0)).toBe(true)
    expect(sameEntityId(0, null)).toBe(false)
  })
})

// ── 纯文本预览白名单(问题②方案 B v1,D-002)─────────────────────────────────
describe('isTextPreviewable', () => {
  it('白名单扩展名命中(txt/md/markdown)', () => {
    expect(isTextPreviewable('note.txt')).toBe(true)
    expect(isTextPreviewable('README.md')).toBe(true)
    expect(isTextPreviewable('a.markdown')).toBe(true)
  })

  it('大小写不敏感(与后端 to_lowercase 同姿态)', () => {
    expect(isTextPreviewable('README.MD')).toBe(true)
    expect(isTextPreviewable('X.TXT')).toBe(true)
  })

  /**
   * 🔴 白名单外一律 false —— 这只是 UX 预筛,但预筛错了会把 .exe 引去发预览请求
   * (后端会拒,但 tooltip 已经承诺了「双击预览」,是承诺失信)。
   */
  it('白名单外拒(exe/图片/无扩展名)', () => {
    expect(isTextPreviewable('evil.exe')).toBe(false)
    expect(isTextPreviewable('a.png')).toBe(false)
    expect(isTextPreviewable('noext')).toBe(false)
  })

  it('点边界形态:点开头(隐藏文件无扩展名)与点结尾不算命中', () => {
    expect(isTextPreviewable('.txt')).toBe(false) // 整个名字就是 .txt,无主干
    expect(isTextPreviewable('weird.')).toBe(false)
  })

  it('多段扩展名取最后一段(x.txt.exe 是 exe,不是 txt)', () => {
    expect(isTextPreviewable('x.txt.exe')).toBe(false)
    expect(isTextPreviewable('x.exe.txt')).toBe(true) // 真实扩展名确是 txt,后端同判
  })
})

// ── 文件行拖拽落点(D-003)────────────────────────────────────────────────────
describe('canDropFileOnDir', () => {
  it('落到别的目录:允许', () => {
    expect(canDropFileOnDir(3, 5)).toBe(true)
  })

  it('落回当前所在目录:拒(不显示误导性落点高亮)', () => {
    expect(canDropFileOnDir(3, 3)).toBe(false)
  })

  /**
   * FS 模式下父目录可能取不到实体 id —— 放行而非拒:后端 relocate 对同目录 no-op 兜底,
   * 误杀合法落点则没有任何兜底。
   */
  it('父目录实体 id 未知(null):放行,后端 no-op 兜底', () => {
    expect(canDropFileOnDir(null, 5)).toBe(true)
  })

  it('id 为 0 参与判定(判据是 null 非真值)', () => {
    expect(canDropFileOnDir(0, 0)).toBe(false)
    expect(canDropFileOnDir(0, 1)).toBe(true)
  })
})

// ── 可展开判据(单一事实源)────────────────────────────────────────────────────
describe('isExpandable', () => {
  it('确有子目录 → 可展开', () => {
    expect(isExpandable({ hasChildren: true, mediaCount: 0 })).toBe(true)
  })

  it('无子目录但有直接媒体 → 仍可展开(只有文件的目录)', () => {
    expect(isExpandable({ hasChildren: false, mediaCount: 3 })).toBe(true)
  })

  it('无子目录且无媒体 → 不可展开(叶子)', () => {
    expect(isExpandable({ hasChildren: false, mediaCount: 0 })).toBe(false)
  })

  /**
   * 🔴 「所有文件」模式的前提:`hasChildren` 未知时**假定可展开**。
   *
   * 反面代价不对称 —— 不给箭头会让真有内容的目录**永远点不开**(用户无从发现里面有东西);
   * 给了箭头而里面是空的,代价只是一次空展开。资源管理器/VSCode 同样如此。
   */
  it('hasChildren 未知(null)→ 假定可展开,即便 mediaCount 也不适用', () => {
    expect(isExpandable({ hasChildren: null, mediaCount: null })).toBe(true)
  })

  /**
   * `mediaCount: null`(不适用)不得被当成「有媒体」。若实现写成 `node.mediaCount != null`
   * 或真值判断的反面,FS 模式下每个已知无子目录的空目录都会错误地显示箭头。
   */
  it('hasChildren=false 且 mediaCount 不适用(null)→ 不可展开', () => {
    expect(isExpandable({ hasChildren: false, mediaCount: null })).toBe(false)
  })

  /**
   * `mediaCount` 是**计数**不是布尔:负数不该出现,但判据必须是 `> 0` 而非真值 ——
   * 真值判断下 `-1` 为真会把一个空目录判成可展开。
   */
  it('mediaCount 为 0 不构成可展开理由(判据是 > 0 而非真值)', () => {
    expect(isExpandable({ hasChildren: false, mediaCount: 0 })).toBe(false)
  })
})

describe('flattenFolderRows', () => {
  it('空输入 → 空数组', () => {
    expect(flattenFolderRows([])).toEqual([])
  })

  it('未展开目录只出目录行、不发射文件', () => {
    const rows = flattenFolderRows([dir(1, 0, { files: [file(11)], expanded: false })])
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ kind: 'dir' })
  })

  it('展开目录：自身文件跟在目录行之后，depth = 目录 depth + 1，dirKey = 目录 nodeKey', () => {
    const rows = flattenFolderRows([dir(1, 0, { expanded: true, files: [file(11), file(12)] })])
    expect(shape(rows)).toEqual(['D1', 'f11', 'f12'])
    expect(rows[1]).toMatchObject({ kind: 'file', dirKey: 'k1', depth: 1 })
  })

  it('fa1 陷阱：目录 A 的自身文件排在子目录 B 整个子树之后，且 dirKey 各归其主', () => {
    // 前序 DFS：A(展开,自身文件 fa1) → B(A 的子,展开,自身文件 fb1)
    const A = dir(1, 0, { expanded: true, files: [file(11, 'fa1')] })
    const B = dir(2, 1, { expanded: true, files: [file(21, 'fb1')] })
    const rows = flattenFolderRows([A, B])
    // 顺序：A(dir) · B(dir) · fb1(file) · fa1(file)——fa1 前一个 dir 行是 B（会误判），真实归属靠 dirKey。
    expect(shape(rows)).toEqual(['D1', 'D2', 'f21', 'f11'])
    expect(rows[2]).toMatchObject({ kind: 'file', dirKey: 'k2' }) // fb1 → B
    expect(rows[3]).toMatchObject({ kind: 'file', dirKey: 'k1' }) // fa1 → A（非最近 dir 行 B）
  })

  it('兄弟子目录：各自文件正确归属，父目录文件排在所有子之后', () => {
    // 前序 DFS：A(展开,fa) → B(A 子,展开,fb) → C(A 子,展开,fc)
    const A = dir(1, 0, { expanded: true, files: [file(10, 'fa')] })
    const B = dir(2, 1, { expanded: true, files: [file(20, 'fb')] })
    const C = dir(3, 1, { expanded: true, files: [file(30, 'fc')] })
    const rows = flattenFolderRows([A, B, C])
    expect(shape(rows)).toEqual(['D1', 'D2', 'f20', 'D3', 'f30', 'f10'])
    expect(rows[5]).toMatchObject({ kind: 'file', dirKey: 'k1' }) // fa → A，排在 B/C 子树之后
  })

  it('展开但无文件的目录不发射文件行（含 files 为空数组）', () => {
    const rows = flattenFolderRows([dir(1, 0, { expanded: true, files: [] })])
    expect(shape(rows)).toEqual(['D1'])
  })

  it('分页：filesHasMore 时在文件之后追加「加载更多」行，dirKey/depth/loadedCount 正确', () => {
    const rows = flattenFolderRows([
      dir(1, 0, { expanded: true, files: [file(11), file(12)], filesHasMore: true }),
    ])
    expect(shape(rows)).toEqual(['D1', 'f11', 'f12', 'Mk1'])
    expect(rows[3]).toMatchObject({ kind: 'more', dirKey: 'k1', depth: 1, loadedCount: 2 })
  })

  it('分页：filesHasMore=false 不发射「加载更多」行', () => {
    const rows = flattenFolderRows([
      dir(1, 0, { expanded: true, files: [file(11)], filesHasMore: false }),
    ])
    expect(shape(rows)).toEqual(['D1', 'f11'])
  })

  it('分页：「加载更多」行随其目录归属，排在该目录文件之后、父目录文件之前（fa1 陷阱下仍正确）', () => {
    // A(展开,fa1,还有更多) → B(A 子,展开,fb1,还有更多)
    const A = dir(1, 0, { expanded: true, files: [file(11, 'fa1')], filesHasMore: true })
    const B = dir(2, 1, { expanded: true, files: [file(21, 'fb1')], filesHasMore: true })
    const rows = flattenFolderRows([A, B])
    // B 子树先冲刷(fb1 + M2),再 A 自身(fa1 + M1)——more 行紧跟各自文件、各归其主。
    expect(shape(rows)).toEqual(['D1', 'D2', 'f21', 'Mk2', 'f11', 'Mk1'])
  })
})

// 方案 B 虚拟化坐标数学。ROW_H=28、buffer=6、viewportH=280(10 行)、treeOffsetTop=100。
describe('computeTreeWindow', () => {
  it('rowCount=0 → 全零', () => {
    expect(computeTreeWindow(500, 100, 280, 0, 28, 6)).toEqual({
      startIndex: 0,
      endIndex: 0,
      offsetY: 0,
    })
  })

  it('视口顶恰在树顶（relTop=0）→ startIndex 0、offsetY 0、endIndex=可视行+buffer', () => {
    expect(computeTreeWindow(100, 100, 280, 1000, 28, 6)).toEqual({
      startIndex: 0,
      endIndex: 16, // 0 + ceil(280/28)=10 + 6
      offsetY: 0,
    })
  })

  it('滚到中部 → startIndex 含上 buffer、offsetY=startIndex*ROW_H', () => {
    // scrollTop=1500, relTop=1400, firstVisible=50
    expect(computeTreeWindow(1500, 100, 280, 1000, 28, 6)).toEqual({
      startIndex: 44, // 50-6
      endIndex: 66, // 50+10+6
      offsetY: 44 * 28,
    })
  })

  it('尚未滚到树（relTop<0）→ startIndex 钳到 0、offsetY 0', () => {
    const w = computeTreeWindow(50, 100, 280, 1000, 28, 6)
    expect(w.startIndex).toBe(0)
    expect(w.offsetY).toBe(0)
  })

  it('滚过整棵树 → startIndex/endIndex 钳到 rowCount', () => {
    const w = computeTreeWindow(1_000_000, 100, 280, 1000, 28, 6)
    expect(w.startIndex).toBe(1000)
    expect(w.endIndex).toBe(1000)
    expect(w.offsetY).toBe(1000 * 28)
  })
})

describe('stickyHeaderChain', () => {
  // 三层嵌套:A(d0,展开,fa) → B(A 子,d1,展开,fb) → C(B 子,d2,展开,fc)
  // 拍平:[D1(d0), D2(d1), D3(d2), fc(d3,dirKey k3), fb(d2,dirKey k2), fa(d1,dirKey k1)]
  const A = dir(1, 0, { expanded: true, files: [file(10, 'fa')] })
  const B = dir(2, 1, { expanded: true, files: [file(20, 'fb')] })
  const C = dir(3, 2, { expanded: true, files: [file(30, 'fc')] })
  const rows = flattenFolderRows([A, B, C])
  const ids = (topIndex: number, maxDepth?: number) =>
    stickyHeaderChain(rows, topIndex, maxDepth).map((n) => n.id)

  it('空行 / topIndex<0 → 空链', () => {
    expect(stickyHeaderChain([], 0)).toEqual([])
    expect(stickyHeaderChain(rows, -1)).toEqual([])
  })

  it('视口顶为最深文件 fc → 完整祖先链(根在前)', () => {
    expect(ids(3)).toEqual([1, 2, 3]) // A, B, C
  })

  it('视口顶为 fb(B 的自身文件)→ [A,B],C(B 的子树)不入链', () => {
    // fb 排在 C 整个子树之后(fa1 陷阱同款),回扫须靠深度递减跳过 C
    expect(ids(4)).toEqual([1, 2])
  })

  it('视口顶为 fa(A 的自身文件)→ 仅 [A]', () => {
    expect(ids(5)).toEqual([1])
  })

  it('视口顶为目录头本身 D3 → 含自身 + 祖先(其头被顶部裁切,钉住覆盖)', () => {
    expect(ids(2)).toEqual([1, 2, 3])
  })

  it('topIndex 超界自动钳到末行', () => {
    expect(ids(999)).toEqual([1]) // 末行是 fa → [A]
  })

  it('maxDepth 封顶:保留最近(最深)祖先,丢弃最浅根', () => {
    expect(ids(3, 2)).toEqual([2, 3]) // 只留 B, C
  })
})

describe('treeScrollTopForIndex', () => {
  it('目标在视口上方 → 对齐顶', () => {
    // index=0 top=100；当前 scrollTop=500 → 100<500 → 返回 100
    expect(treeScrollTopForIndex(0, 100, 500, 280, 28)).toBe(100)
  })

  it('目标在视口下方 → 对齐底', () => {
    // index=50 top=1500 bottom=1528；scrollTop=0 viewport 0..280 → 1528>280 → 1528-280
    expect(treeScrollTopForIndex(50, 100, 0, 280, 28)).toBe(1528 - 280)
  })

  it('目标已可见 → 不动', () => {
    // index=5 top=240 bottom=268；scrollTop=100 viewport 100..380 → 已可见
    expect(treeScrollTopForIndex(5, 100, 100, 280, 28)).toBe(100)
  })

  it('topInset:目标行须落在粘顶浮层之下,而非视口物理顶', () => {
    // index=0 top=100;topInset=108(3 个区块标题)→ 对齐到 100-108=-8(由滚动区自行钳 0)
    expect(treeScrollTopForIndex(0, 100, 500, 280, 28, 108)).toBe(-8)
    // 行在物理视口内但被浮层盖住(top=156 < 浮层底沿 cur+inset=208)→ 仍需上滚对齐浮层之下
    expect(treeScrollTopForIndex(2, 100, 100, 280, 28, 108)).toBe(156 - 108)
    // 行在浮层底沿之下(top=240 ≥ 208)→ 已可见,不动
    expect(treeScrollTopForIndex(5, 100, 100, 280, 28, 108)).toBe(100)
  })

  it('bottomInset:目标行须露在粘底浮层之上', () => {
    // index=50 top=1500 bottom=1528;bottomInset=36 → 1528+36-280
    expect(treeScrollTopForIndex(50, 100, 0, 280, 28, 0, 36)).toBe(1528 + 36 - 280)
  })

  it('双 inset 下已可见 → 不动', () => {
    // viewport 100..380,有效区 208..344;index=8 top=324 bottom=352? 352>344 → 会动;
    // 改 index=7:top=296 bottom=324 ∈ [208, 344] → 不动
    expect(treeScrollTopForIndex(7, 100, 100, 280, 28, 108, 36)).toBe(100)
  })
})

describe('treeKeyTarget', () => {
  // A(d0,展开,自身文件 fa,还有更多) → B(A 子,d1,展开,自身文件 fb) → C(A 子,d1,折叠可展开) → L(A 子,d1,纯叶)
  // 拍平:[0:D1, 1:D2, 2:f21(d2), 3:D3, 4:D4, 5:f11(d1), 6:M1(d1)]
  const A = dir(1, 0, { expanded: true, files: [file(11, 'fa')], filesHasMore: true, hasChildren: true, mediaCount: 1 })
  const B = dir(2, 1, { expanded: true, files: [file(21, 'fb')], mediaCount: 1 })
  const C = dir(3, 1, { hasChildren: true })
  const L = dir(4, 1, {}) // 无子无媒体:不可展开
  const rows = flattenFolderRows([A, B, C, L])

  it('夹具拍平序自检(后续下标断言的地基)', () => {
    expect(shape(rows)).toEqual(['D1', 'D2', 'f21', 'D3', 'D4', 'f11', 'Mk1'])
  })

  it('空行集 → null;无 active 时 ↓/↑/Home 落首行、End 落末行、其余键无动作', () => {
    expect(treeKeyTarget([], 0, 'ArrowDown')).toBeNull()
    expect(treeKeyTarget(rows, -1, 'ArrowDown')).toEqual({ kind: 'move', index: 0 })
    expect(treeKeyTarget(rows, -1, 'ArrowUp')).toEqual({ kind: 'move', index: 0 })
    expect(treeKeyTarget(rows, 999, 'Home')).toEqual({ kind: 'move', index: 0 })
    expect(treeKeyTarget(rows, -1, 'End')).toEqual({ kind: 'move', index: 6 })
    expect(treeKeyTarget(rows, -1, 'ArrowRight')).toBeNull()
    expect(treeKeyTarget(rows, -1, 'Enter')).toBeNull()
  })

  it('↓/↑ 线性步进,边界不越', () => {
    expect(treeKeyTarget(rows, 0, 'ArrowDown')).toEqual({ kind: 'move', index: 1 })
    expect(treeKeyTarget(rows, 6, 'ArrowDown')).toBeNull()
    expect(treeKeyTarget(rows, 3, 'ArrowUp')).toEqual({ kind: 'move', index: 2 })
    expect(treeKeyTarget(rows, 0, 'ArrowUp')).toBeNull()
  })

  it('Home/End 首末', () => {
    expect(treeKeyTarget(rows, 3, 'Home')).toEqual({ kind: 'move', index: 0 })
    expect(treeKeyTarget(rows, 3, 'End')).toEqual({ kind: 'move', index: 6 })
  })

  it('→:折叠可展开目录 → toggle;已展开 → 进首子行;纯叶目录/文件/more → 无动作', () => {
    expect(treeKeyTarget(rows, 3, 'ArrowRight')).toEqual({ kind: 'toggle', node: C }) // C 折叠可展开
    expect(treeKeyTarget(rows, 0, 'ArrowRight')).toEqual({ kind: 'move', index: 1 }) // A 展开 → 首子 B
    expect(treeKeyTarget(rows, 1, 'ArrowRight')).toEqual({ kind: 'move', index: 2 }) // B 展开 → 首子 fb
    expect(treeKeyTarget(rows, 4, 'ArrowRight')).toBeNull() // L 不可展开
    expect(treeKeyTarget(rows, 2, 'ArrowRight')).toBeNull() // 文件行
    expect(treeKeyTarget(rows, 6, 'ArrowRight')).toBeNull() // more 行
  })

  it('→:展开但子树未出行(加载中/空)→ 不动', () => {
    // 单根展开、可展开(hasChildren)但 children 尚未加载出行 → 下一行不存在
    const lone = flattenFolderRows([dir(9, 0, { expanded: true, hasChildren: true })])
    expect(treeKeyTarget(lone, 0, 'ArrowRight')).toBeNull()
  })

  it('←:展开目录 → toggle;折叠目录 → 跳最近祖先目录行', () => {
    expect(treeKeyTarget(rows, 0, 'ArrowLeft')).toEqual({ kind: 'toggle', node: A }) // A 展开 → 折叠
    expect(treeKeyTarget(rows, 1, 'ArrowLeft')).toEqual({ kind: 'toggle', node: B }) // B 展开 → 折叠
    expect(treeKeyTarget(rows, 3, 'ArrowLeft')).toEqual({ kind: 'move', index: 0 }) // C(折叠) → 父 A
  })

  it('←:文件/more 行跳其归属目录(fa1 陷阱:fa 的父是 A 而非上方最近的 dir 行)', () => {
    expect(treeKeyTarget(rows, 2, 'ArrowLeft')).toEqual({ kind: 'move', index: 1 }) // fb → B
    // fa(idx5,d1):上方最近 dir 行是 D4/D3(d1,同深兄弟,跳过)→ 首个更浅行 D1 ✓
    expect(treeKeyTarget(rows, 5, 'ArrowLeft')).toEqual({ kind: 'move', index: 0 })
    expect(treeKeyTarget(rows, 6, 'ArrowLeft')).toEqual({ kind: 'move', index: 0 }) // M1 → A
  })

  it('←:根层折叠目录无处可跳 → null', () => {
    const lone = flattenFolderRows([dir(9, 0, { hasChildren: true })])
    expect(treeKeyTarget(lone, 0, 'ArrowLeft')).toBeNull()
  })

  it('Enter/Space → activate;无关键 → null', () => {
    expect(treeKeyTarget(rows, 2, 'Enter')).toEqual({ kind: 'activate' })
    expect(treeKeyTarget(rows, 2, ' ')).toEqual({ kind: 'activate' })
    expect(treeKeyTarget(rows, 2, 'a')).toBeNull()
    expect(treeKeyTarget(rows, 2, 'Tab')).toBeNull()
  })
})

describe('firstAutoLoadTarget', () => {
  // A(展开,还有更多) → B(A 子,展开,还有更多):拍平出 M2(先)与 M1(后)两个 more 行。
  const A = dir(1, 0, { expanded: true, files: [file(11)], filesHasMore: true })
  const B = dir(2, 1, { expanded: true, files: [file(21)], filesHasMore: true })
  const rows = flattenFolderRows([A, B]) // [D1, D2, f21, M2, f11, M1]

  it('返回窗口内首个 more 行的 dirKey', () => {
    expect(firstAutoLoadTarget(rows, new Set())).toBe('k2')
  })

  it('拉黑的目录被跳过,取下一个可触发行', () => {
    expect(firstAutoLoadTarget(rows, new Set(['k2']))).toBe('k1')
  })

  it('全部拉黑 / 窗口内无 more 行 → null', () => {
    expect(firstAutoLoadTarget(rows, new Set(['k1', 'k2']))).toBeNull()
    expect(firstAutoLoadTarget(rows.slice(0, 3), new Set())).toBeNull()
    expect(firstAutoLoadTarget([], new Set())).toBeNull()
  })
})

describe('createFolderTreeSyncQueue', () => {
  const deferred = () => {
    let resolve!: () => void
    const promise = new Promise<void>((done) => {
      resolve = done
    })
    return { promise, resolve }
  }

  it('多级目录展开在途时合并中间目标，只允许最后目标滚动', async () => {
    const gate1 = deferred()
    const gate3 = deferred()
    const started1 = deferred()
    const started3 = deferred()
    const started: number[] = []
    const scrolled: number[] = []
    const queue = createFolderTreeSyncQueue(async (dirId, isLatest) => {
      started.push(dirId)
      if (dirId === 1) started1.resolve()
      if (dirId === 3) started3.resolve()
      if (dirId === 1) await gate1.promise
      if (dirId === 3) await gate3.promise
      if (isLatest()) scrolled.push(dirId)
    })

    const done = queue.request(1)
    await started1.promise
    void queue.request(2)
    void queue.request(3)
    gate1.resolve()
    await started3.promise

    expect(started).toEqual([1, 3]) // 中间目标 2 从未发起祖先查询/展开
    expect(scrolled).toEqual([]) // 旧目标 1 已失效，最新目标 3 尚未完成

    gate3.resolve()
    await done
    expect(scrolled).toEqual([3])
  })

  it('null 目标会使在途定位失效且不产生滚动', async () => {
    const gate = deferred()
    const started = deferred()
    const scrolled: number[] = []
    const queue = createFolderTreeSyncQueue(async (dirId, isLatest) => {
      started.resolve()
      await gate.promise
      if (isLatest()) scrolled.push(dirId)
    })

    const done = queue.request(9)
    await started.promise
    void queue.request(null)
    gate.resolve()
    await done

    expect(scrolled).toEqual([])
  })

  it('drain 收尾 microtask 中到达的新目标不会丢失', async () => {
    const started: number[] = []
    const queue = createFolderTreeSyncQueue(async (dirId) => {
      started.push(dirId)
      if (dirId === 1) {
        // 两层 microtask 让请求落在旧实现的 drain 已结束、外层 finally 尚未清理期间。
        queueMicrotask(() => {
          queueMicrotask(() => {
            void queue.request(2)
          })
        })
      }
    })

    await queue.request(1)
    await Promise.resolve()

    expect(started).toEqual([1, 2])
  })
})
