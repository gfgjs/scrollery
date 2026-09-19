// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect } from 'vitest'
import { flattenFolderRows, computeTreeWindow, treeScrollTopForIndex, stickyHeaderChain, treeKeyTarget, createFolderTreeSyncQueue, isOpenableInApp, isExpandable, sameEntityId } from './folderTree.helpers'
import { type DirNode, type DirFile } from '../../../types/media'

describe('目录结构与导航', () => {
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

  describe('FS-only 能力边界', () => {
    it('只缺 mediaType 也不可打开(仅判 id 的实现会漏)', () => {
      expect(isOpenableInApp({ id: 5, mediaType: null })).toBe(false)
    })

    it('只缺 id 也不可打开(仅判 mediaType 的实现会漏)', () => {
      expect(isOpenableInApp({ id: null, mediaType: 'image' })).toBe(false)
    })
  })

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
  })

  describe('isExpandable', () => {
    /**
     * 🔴 「所有文件」模式的前提:`hasChildren` 未知时**假定可展开**。
     *
     * 反面代价不对称 —— 不给箭头会让真有内容的目录**永远点不开**(用户无从发现里面有东西);
     * 给了箭头而里面是空的,代价只是一次空展开。资源管理器/VSCode 同样如此。
     */
    it('hasChildren 未知(null)→ 假定可展开,即便 mediaCount 也不适用', () => {
      expect(isExpandable({ hasChildren: null, mediaCount: null })).toBe(true)
    })
  })

  describe('flattenFolderRows', () => {
    it('未展开目录只出目录行、不发射文件', () => {
      const rows = flattenFolderRows([dir(1, 0, { files: [file(11)], expanded: false })])
      expect(rows).toHaveLength(1)
      expect(rows[0]).toMatchObject({ kind: 'dir' })
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

    it('分页：filesHasMore 时在文件之后追加「加载更多」行，dirKey/depth/loadedCount 正确', () => {
      const rows = flattenFolderRows([
        dir(1, 0, { expanded: true, files: [file(11), file(12)], filesHasMore: true }),
      ])
      expect(shape(rows)).toEqual(['D1', 'f11', 'f12', 'Mk1'])
      expect(rows[3]).toMatchObject({ kind: 'more', dirKey: 'k1', depth: 1, loadedCount: 2 })
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

  describe('computeTreeWindow', () => {
    it('rowCount=0 → 全零', () => {
      expect(computeTreeWindow(500, 100, 280, 0, 28, 6)).toEqual({
        startIndex: 0,
        endIndex: 0,
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

    it('视口顶为 fb(B 的自身文件)→ [A,B],C(B 的子树)不入链', () => {
      // fb 排在 C 整个子树之后(fa1 陷阱同款),回扫须靠深度递减跳过 C
      expect(ids(4)).toEqual([1, 2])
    })

    it('maxDepth 封顶:保留最近(最深)祖先,丢弃最浅根', () => {
      expect(ids(3, 2)).toEqual([2, 3]) // 只留 B, C
    })
  })

  describe('treeScrollTopForIndex', () => {
    it('topInset:目标行须落在粘顶浮层之下,而非视口物理顶', () => {
      // index=0 top=100;topInset=108(3 个区块标题)→ 对齐到 100-108=-8(由滚动区自行钳 0)
      expect(treeScrollTopForIndex(0, 100, 500, 280, 28, 108)).toBe(-8)
      // 行在物理视口内但被浮层盖住(top=156 < 浮层底沿 cur+inset=208)→ 仍需上滚对齐浮层之下
      expect(treeScrollTopForIndex(2, 100, 100, 280, 28, 108)).toBe(156 - 108)
      // 行在浮层底沿之下(top=240 ≥ 208)→ 已可见,不动
      expect(treeScrollTopForIndex(5, 100, 100, 280, 28, 108)).toBe(100)
    })
  })

  describe('treeKeyTarget', () => {
    // A(d0,展开,自身文件 fa,还有更多) → B(A 子,d1,展开,自身文件 fb) → C(A 子,d1,折叠可展开) → L(A 子,d1,纯叶)
    // 拍平:[0:D1, 1:D2, 2:f21(d2), 3:D3, 4:D4, 5:f11(d1), 6:M1(d1)]
    const A = dir(1, 0, { expanded: true, files: [file(11, 'fa')], filesHasMore: true, hasChildren: true, mediaCount: 1 })

    const B = dir(2, 1, { expanded: true, files: [file(21, 'fb')], mediaCount: 1 })

    const C = dir(3, 1, { hasChildren: true })

    const L = dir(4, 1, {})

    // 无子无媒体:不可展开
    const rows = flattenFolderRows([A, B, C, L])

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

    it('→:折叠可展开目录 → toggle;已展开 → 进首子行;纯叶目录/文件/more → 无动作', () => {
      expect(treeKeyTarget(rows, 3, 'ArrowRight')).toEqual({ kind: 'toggle', node: C }) // C 折叠可展开
      expect(treeKeyTarget(rows, 0, 'ArrowRight')).toEqual({ kind: 'move', index: 1 }) // A 展开 → 首子 B
      expect(treeKeyTarget(rows, 1, 'ArrowRight')).toEqual({ kind: 'move', index: 2 }) // B 展开 → 首子 fb
      expect(treeKeyTarget(rows, 4, 'ArrowRight')).toBeNull() // L 不可展开
      expect(treeKeyTarget(rows, 2, 'ArrowRight')).toBeNull() // 文件行
      expect(treeKeyTarget(rows, 6, 'ArrowRight')).toBeNull() // more 行
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

    it('Enter/Space → activate;无关键 → null', () => {
      expect(treeKeyTarget(rows, 2, 'Enter')).toEqual({ kind: 'activate' })
      expect(treeKeyTarget(rows, 2, ' ')).toEqual({ kind: 'activate' })
      expect(treeKeyTarget(rows, 2, 'a')).toBeNull()
      expect(treeKeyTarget(rows, 2, 'Tab')).toBeNull()
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
})
