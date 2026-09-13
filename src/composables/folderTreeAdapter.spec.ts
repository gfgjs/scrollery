import { describe, it, expect } from 'vitest'
import { adaptTreeEntries, fsEntryToDirFile, fsEntryToDirNode } from './folderTreeAdapter'
import type { TreeEntry } from '../types/media'

// 后端 TreeEntry 的最小合法形态。可选字段（directoryId/mediaId/mediaType）**默认缺席** ——
// Rust 侧 `skip_serializing_if = "Option::is_none"` 意味着它们在 JSON 里根本不出现，
// 而不是 `null`。fixture 必须复现这一点，否则测的是一个真后端不会发的形状。
function entry(over: Partial<TreeEntry> = {}): TreeEntry {
  return {
    nodeKey: '1:a/b',
    parentKey: '1:a',
    rootId: 1,
    relPath: 'a/b',
    name: 'b',
    kind: 'dir',
    hidden: false,
    registered: false,
    isSymlink: false,
    ...over,
  }
}

describe('fsEntryToDirNode', () => {
  it('照抄后端的 nodeKey/parentKey,不自行推导', () => {
    const n = fsEntryToDirNode(entry({ nodeKey: '9:x/y', parentKey: '9:x' }), 2)
    expect(n.nodeKey).toBe('9:x/y')
    expect(n.parentKey).toBe('9:x')
  })

  /**
   * 🔴 「未知」必须表达成 null,不能用 0/false 冒充。
   *
   * `mediaCount: 0` 会渲染出一个「0」角标(读起来像「这里没东西」),而设计 §4.2 明禁在列全部
   * 文件的树上显示递归媒体数;`hasChildren: false` 会让真有子目录的目录**永远点不开**。
   */
  it('mediaCount 与 hasChildren 恒为 null(未知/不适用),不用 0/false 冒充', () => {
    const n = fsEntryToDirNode(entry(), 1)
    expect(n.mediaCount).toBeNull()
    expect(n.hasChildren).toBeNull()
  })

  it('库里有对应目录行 → 赋实体身份', () => {
    expect(fsEntryToDirNode(entry({ directoryId: 42 }), 1).id).toBe(42)
  })

  /**
   * FS-only 目录:后端**不发** directoryId 字段(不是发 null)。适配器须把「字段缺席」
   * 归一为 `null` —— DirNode.id 的契约是 `number | null`,`undefined` 会让
   * `hasEntityIdentity`(判 `!== null`)误放行,进而拿 undefined 去拼 `/folder/undefined`。
   */
  it('FS-only 目录(后端不发 directoryId)→ id 为 null 而非 undefined', () => {
    const n = fsEntryToDirNode(entry(), 1)
    expect(n.id).toBeNull()
    expect(n.id).not.toBeUndefined()
  })

  /**
   * parentId 透传后端的 parentDirectoryId(R-07 修复)。此前恒 null,连**库内**目录也被拖拽
   * 入口第一关(`parentId === null` 挡扫描根)静默禁拖 —— 比设计 §4.1「只禁 FS-only」更紧。
   * 父是 FS-only 时后端不发该字段 → 归一为 null(同 id:缺席≠undefined 透传)。
   */
  it('parentId 透传 parentDirectoryId;缺席归一为 null 而非 undefined', () => {
    expect(fsEntryToDirNode(entry({ directoryId: 42, parentDirectoryId: 10 }), 1).parentId).toBe(10)
    const orphan = fsEntryToDirNode(entry({ directoryId: 42 }), 1)
    expect(orphan.parentId).toBeNull()
    expect(orphan.parentId).not.toBeUndefined()
  })

  it('depth 由调用方给(= 父 depth + 1)', () => {
    expect(fsEntryToDirNode(entry(), 3).depth).toBe(3)
  })

  /**
   * hidden 透传(R-06 修复)。后端契约明言「点前缀项必须带上 hidden 标记……前端要据此加样式」,
   * adapter 丢弃它 = 样式端到端断链:隐藏项与普通项长得一样,叠上判等 bug 时更无从辨认
   * (真机问题 1 的 UX 侧)。
   */
  it('hidden 标记透传(目录与文件两侧)', () => {
    expect(fsEntryToDirNode(entry({ hidden: true }), 1).hidden).toBe(true)
    expect(fsEntryToDirNode(entry(), 1).hidden).toBe(false)
    expect(fsEntryToDirFile(entry({ kind: 'file', hidden: true })).hidden).toBe(true)
    expect(fsEntryToDirFile(entry({ kind: 'file' })).hidden).toBe(false)
  })
})

describe('fsEntryToDirFile', () => {
  it('已入库文件 → 赋 mediaId 与 mediaType', () => {
    const f = fsEntryToDirFile(entry({ kind: 'file', mediaId: 7, mediaType: 'video' }))
    expect(f.id).toBe(7)
    expect(f.mediaType).toBe('video')
  })

  /**
   * 🔴 未入库文件:两者都必须是 null,**绝不编一个默认类型**。
   *
   * `mediaRoute.ts` 对非 doc/audio 兜底到 `/view/{id}` —— 给一个 `.exe` 编上 `'image'`
   * 不会报错,会把它静默送进图片查看器。null 才能让 `isOpenableInApp` 拦住。
   */
  it('未入库文件(后端不发 mediaId/mediaType)→ 两者皆 null', () => {
    const f = fsEntryToDirFile(entry({ kind: 'file', name: 'x.exe' }))
    expect(f.id).toBeNull()
    expect(f.mediaType).toBeNull()
  })

  it('文件名取 entry.name', () => {
    expect(fsEntryToDirFile(entry({ kind: 'file', name: '照片.jpg' })).fileName).toBe('照片.jpg')
  })

  /**
   * relPath 照抄后端,不由 `parentKey + fileName` 拼、也不从 `nodeKey` 拆。
   *
   * 双击 reveal 拿它当 IPC 入参。自己拼就是在前端重造 Rust 的 `child_rel_path`;从 nodeKey 拆
   * 就是重造键格式的解析 —— 而后端两条产出侧本来都现成有这个值(DB 侧 queries.rs 算完塞进
   * node_key 就扔了,FS 侧 TreeEntry 本就带着)。
   *
   * 样本有区分度:`relPath` 深两层而 `name` 只是末段,二者形态不同 —— 任何「拿 name 顶上」或
   * 「只取末段」的实现都对不上。
   */
  it('relPath 照抄后端(不由 parentKey + 文件名拼出)', () => {
    const f = fsEntryToDirFile(
      entry({ kind: 'file', name: 'x.jpg', relPath: '相册/2024/x.jpg', parentKey: '1:相册/2024' }),
    )
    expect(f.relPath).toBe('相册/2024/x.jpg')
  })
})

describe('adaptTreeEntries', () => {
  const page: TreeEntry[] = [
    entry({ kind: 'dir', name: 'Zebra', nodeKey: '1:Zebra', relPath: 'Zebra' }),
    entry({ kind: 'dir', name: 'apple', nodeKey: '1:apple', relPath: 'apple', directoryId: 5 }),
    entry({ kind: 'file', name: 'Zebra.jpg', nodeKey: '1:Zebra.jpg', relPath: 'Zebra.jpg' }),
    entry({ kind: 'file', name: 'apple.jpg', nodeKey: '1:apple.jpg', relPath: 'apple.jpg', mediaId: 9, mediaType: 'image' }),
  ]

  it('按 kind 分成两组', () => {
    const { dirs, files } = adaptTreeEntries(page, 1)
    expect(dirs.map((d) => d.name)).toEqual(['Zebra', 'apple'])
    expect(files.map((f) => f.fileName)).toEqual(['Zebra.jpg', 'apple.jpg'])
  })

  /**
   * 🔴 分组**必须保序**(D-002:切模式时共有项相对序不变)。
   *
   * 样本有区分度:`Zebra`/`apple` 在 BINARY(后端目录序)与 NOCASE(后端文件序)下**序相反**
   * —— 任何「顺手按 name 排一下」的实现都会翻掉其中一组。断言直接钉「输入中同类项的出现序」,
   * 而非手写期望(手写期望是第二份实现,会带第二份 bug)。
   */
  it('分组保持后端给的序(不重排)', () => {
    const { dirs, files } = adaptTreeEntries(page, 1)
    expect(dirs.map((d) => d.name)).toEqual(
      page.filter((e) => e.kind === 'dir').map((e) => e.name),
    )
    expect(files.map((f) => f.fileName)).toEqual(
      page.filter((e) => e.kind === 'file').map((e) => e.name),
    )
  })

  it('全组同一 depth', () => {
    expect(adaptTreeEntries(page, 4).dirs.every((d) => d.depth === 4)).toBe(true)
  })

  it('空页 → 两组皆空', () => {
    expect(adaptTreeEntries([], 1)).toEqual({ dirs: [], files: [] })
  })
})
