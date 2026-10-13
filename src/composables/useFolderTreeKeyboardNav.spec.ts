import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope, nextTick, ref, shallowRef, type EffectScope } from 'vue'
import type { DirRow, FileRow, MoreRow, TreeRow } from '../components/sidebar/sections/folderTree.helpers'
import { useFolderTreeKeyboardNav } from './useFolderTreeKeyboardNav'

const scopes: EffectScope[] = []
afterEach(() => {
  scopes.splice(0).forEach((scope) => scope.stop())
  vi.unstubAllGlobals()
})

function dir(id: number | null, path: string): DirRow {
  return {
    kind: 'dir',
    node: {
      id, nodeKey: `1:${path}`, parentKey: '1:', rootId: 1, parentId: null,
      name: path, relPath: path, depth: 0, mediaCount: 0, hasChildren: false,
    },
  }
}

function file(id: number | null, path: string): FileRow {
  return {
    kind: 'file', depth: 1, dirKey: '1:',
    file: {
      id, nodeKey: `1:${path}`, parentKey: '1:', relPath: path,
      fileName: path, mediaType: null, isFavorited: false,
    },
  }
}

function more(dirKey: string, loadedCount = 1): MoreRow {
  return { kind: 'more', dirKey, depth: 1, loadedCount }
}

function harness(initial: TreeRow[], active = -1) {
  const rows = ref(initial)
  const onNodeClick = vi.fn()
  const onFileClick = vi.fn()
  const onLoadMore = vi.fn()
  // 只替换 DOM 边界；响应式、行身份和按键决策均执行真实实现。
  vi.stubGlobal('document', { activeElement: null })
  const scope = effectScope()
  scopes.push(scope)
  const nav = scope.run(() => useFolderTreeKeyboardNav({
    displayRows: computed(() => rows.value),
    treeRef: ref<HTMLElement | null>(null),
    scrollAreaEl: shallowRef<HTMLElement | null>(null),
    rowH: ref(24), stackTopPx: computed(() => 0), stackBottomPx: computed(() => 0),
    treeOffsetTop: () => 0, toggleNode: vi.fn(), onNodeClick, onFileClick,
    onFileDblClick: vi.fn(), onLoadMore, isOpenableInApp: () => false,
    isTextPreviewable: () => false,
  }))!
  nav.setActiveIndex(active)
  const enter = () => nav.onTreeKeydown({ key: 'Enter', preventDefault: vi.fn() } as unknown as KeyboardEvent)
  return { rows, ...nav, enter, onNodeClick, onFileClick, onLoadMore }
}

describe('useFolderTreeKeyboardNav 焦点身份', () => {
  it('T14-01 前插行后 Enter 仍选择原文件', async () => {
    const h = harness([file(1, 'a'), file(2, 'b')], 1)
    h.rows.value = [file(3, 'new'), file(1, 'a'), file(2, 'b')]
    await nextTick()
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(2, 'b').file)
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-02 删除前方行后 Enter 仍选择原文件', async () => {
    const h = harness([file(1, 'a'), file(2, 'b'), file(3, 'c')], 1)
    h.rows.value = [file(2, 'b'), file(3, 'c')]
    await nextTick()
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(2, 'b').file)
    expect(h.activeIndex.value).toBe(0)
  })

  it('T14-03 库内目录换路径按 id 保焦点且不匹配同 id 文件', async () => {
    const h = harness([dir(1, 'old')], 0)
    h.rows.value = [file(1, 'new'), dir(2, 'old'), dir(1, 'new')]
    await nextTick()
    h.enter()
    expect(h.onNodeClick).toHaveBeenCalledExactlyOnceWith(dir(1, 'new').node)
    expect(h.onFileClick).not.toHaveBeenCalled()
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-04 库内文件换路径按 id 保焦点且不匹配同 id 目录', async () => {
    const h = harness([file(1, 'old')], 0)
    h.rows.value = [dir(1, 'new'), file(2, 'old'), file(1, 'new')]
    await nextTick()
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(1, 'new').file)
    expect(h.onNodeClick).not.toHaveBeenCalled()
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-05 FS-only 目录按 nodeKey 保焦点且 null id 不互相匹配', async () => {
    const h = harness([dir(null, 'target')], 0)
    h.rows.value = [dir(null, 'other'), file(null, 'target'), dir(null, 'target')]
    await nextTick()
    h.enter()
    expect(h.onNodeClick).toHaveBeenCalledExactlyOnceWith(dir(null, 'target').node)
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-06 FS-only 文件按 nodeKey 保焦点且 null id 不互相匹配', async () => {
    const h = harness([file(null, 'target')], 0)
    h.rows.value = [file(null, 'other'), dir(null, 'target'), file(null, 'target')]
    await nextTick()
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(null, 'target').file)
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-07 加载更多行按 dirKey 保焦点', async () => {
    const h = harness([more('1:target')], 0)
    h.rows.value = [more('1:other'), file(1, 'a'), more('1:target', 2)]
    await nextTick()
    h.enter()
    expect(h.onLoadMore).toHaveBeenCalledExactlyOnceWith('1:target')
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-08 活动行消失且旧索引可用时保留临近行', async () => {
    const h = harness([file(1, 'a'), file(2, 'b'), file(3, 'c')], 1)
    h.rows.value = [file(1, 'a'), file(3, 'c')]
    await nextTick()
    h.enter()
    expect(h.activeIndex.value).toBe(1)
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(3, 'c').file)
  })

  it('T14-09 活动末行消失时钳位到剩余末行', async () => {
    const h = harness([file(1, 'a'), file(2, 'b')], 1)
    h.rows.value = [file(1, 'a')]
    await nextTick()
    h.enter()
    expect(h.activeIndex.value).toBe(0)
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(1, 'a').file)
  })

  it('T14-10 行集清空后无活动行且 Enter 不选择资产', async () => {
    const h = harness([file(1, 'a')], 0)
    h.rows.value = []
    await nextTick()
    h.enter()
    expect(h.activeIndex.value).toBe(-1)
    expect(h.onFileClick).not.toHaveBeenCalled()
    expect(h.onNodeClick).not.toHaveBeenCalled()
    expect(h.onLoadMore).not.toHaveBeenCalled()
  })

  it('T14-11 尚未激活时刷新不自动激活首行', async () => {
    const h = harness([file(1, 'a')])
    h.rows.value = [file(2, 'b'), file(1, 'a')]
    await nextTick()
    expect(h.activeIndex.value).toBe(-1)
    h.onTreeFocus()
    expect(h.activeIndex.value).toBe(0)
  })

  it('T14-12 同 tick 连续刷新后立即 Enter 使用最新焦点位置', async () => {
    const h = harness([file(1, 'a'), file(2, 'b')], 1)
    h.rows.value = [file(3, 'c'), file(1, 'a'), file(2, 'b')]
    expect(h.activeIndex.value).toBe(2)
    h.rows.value = [file(2, 'b'), file(3, 'c'), file(1, 'a')]
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(2, 'b').file)
    expect(h.activeIndex.value).toBe(0)
    await nextTick()
    expect(h.activeIndex.value).toBe(0)
  })

  it('T14-13 刷新后手动 setActiveIndex 不被待执行 watch 覆盖', async () => {
    const h = harness([file(1, 'a'), file(2, 'b')], 1)
    h.rows.value = [file(3, 'c'), file(1, 'a'), file(2, 'b')]
    h.setActiveIndex(0)
    await nextTick()
    h.rows.value = [file(1, 'a'), file(2, 'b'), file(3, 'c')]
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(3, 'c').file)
    expect(h.activeIndex.value).toBe(2)
  })

  it('T14-14 鼠标回调直接设置 activeIndex 后刷新保留新选择', async () => {
    const h = harness([file(1, 'a'), file(2, 'b')], 1)
    h.rows.value = [file(3, 'c'), file(1, 'a'), file(2, 'b')]
    h.activeIndex.value = 0
    await nextTick()
    h.rows.value = [file(1, 'a'), file(2, 'b'), file(3, 'c')]
    h.enter()
    expect(h.onFileClick).toHaveBeenCalledExactlyOnceWith(file(3, 'c').file)
    expect(h.activeIndex.value).toBe(2)
  })
})
