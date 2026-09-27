// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect } from 'vitest'
import { flattenFolderRows,  } from './folderTree.helpers'
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

  describe('flattenFolderRows', () => {

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
  })
})
