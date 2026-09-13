// src/composables/useExportEntries.spec.ts
// U-2(方案 A):覆盖两个新导出入口的 descriptor 组装(store 层可测部分——GUI 点击面见任务回执的
// 手动验收步骤)。
//  1. buildAlbumExportPayload —— scope=collection、filter 恒空(不叠加当前画廊筛选)、排序沿用
//     当前 UI 排序偏好、source 为 album{id,name};非 'user' kind 一律拒绝(返回 null,防御——
//     系统夹不承载 album_items,scope=collection 的相册导出对它会错集)。
//  2. buildCurrentViewExportPayload —— 非语义模式:selectAll{view=buildCurrentViewDescriptor()};
//     语义模式(buildCurrentViewDescriptor 返回 null):回退 explicit{ids=当前视图布局序全集}。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

// 中和 uiStore 的 setup 期 IPC(get_startup_config,同 searchStore.spec.ts 既有姿态)。
vi.mock('../utils/ipc', () => ({
  invokeIpc: vi.fn(() => new Promise(() => {})),
  invokeIpcRaw: vi.fn(() => new Promise(() => {})),
  ipcErrorMessage: (e: unknown) => String(e),
}))

import { useExportEntries } from './useExportEntries'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { useAiStore } from '../stores/aiStore'
import type { Collection } from '../types/media'

// 最小 Collection 夹具:仅补 buildAlbumExportPayload 用到的字段之外的必填字段,凑满接口。
function makeCollection(overrides: Partial<Collection>): Collection {
  return { id: 9, name: '精选', kind: 'user', itemCount: 0, sortOrder: 0, ...overrides }
}

beforeEach(() => {
  // node 环境无 window,而 uiStore setup 同步读 window.matchMedia——同 searchStore.spec.ts 既有姿态,
  // 给最小桩即可实例化真实 uiStore。
  vi.stubGlobal('window', {
    matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
    location: { search: '' },
    addEventListener: () => {},
    removeEventListener: () => {},
  })
  setActivePinia(createPinia())
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useExportEntries.buildAlbumExportPayload', () => {
  it('整相册 scope,filter 恒空,排序沿用当前 UI 偏好,source 携带相册 id/name', () => {
    const ui = useUiStore()
    // persist=false:避免测试触发真实 invokeIpc 持久化写(同 aiStore 临时切态的既有调用姿态)。
    ui.setGroupBy('folder', false)
    ui.setSortWithinGroup('filename', false)
    ui.sortOrder = 'asc'

    const { buildAlbumExportPayload } = useExportEntries()
    const payload = buildAlbumExportPayload(makeCollection({}))
    expect(payload).not.toBeNull()
    const { selection, source } = payload!

    expect(selection).toEqual({
      kind: 'selectAll',
      view: {
        scope: { kind: 'collection', albumId: 9 },
        filter: {},
        sort: { groupBy: 'folder', sortWithinGroup: 'filename', sortOrder: 'asc' },
        layoutVersion: useMediaStore().layoutVersion,
      },
      excludedIds: [],
    })
    expect(source).toEqual({ kind: 'album', id: 9, name: '精选' })
  })

  it('系统夹被拒:kind 非 user 返回 null(系统夹不承载 album_items,防错集导出)', () => {
    const { buildAlbumExportPayload } = useExportEntries()
    const payload = buildAlbumExportPayload(makeCollection({ kind: 'system', mediaTypeFilter: 'image' }))
    expect(payload).toBeNull()
  })
})

describe('useExportEntries.buildCurrentViewExportPayload', () => {
  it('非语义模式:selectAll 携带当前视图描述符(默认全部视图 + 当前排序)', () => {
    const { buildCurrentViewExportPayload } = useExportEntries()
    const { selection, source } = buildCurrentViewExportPayload('当前视图')

    expect(selection).toEqual({
      kind: 'selectAll',
      view: {
        scope: { kind: 'all' },
        filter: {},
        sort: { groupBy: 'date', sortWithinGroup: 'datetime', sortOrder: 'desc' },
        layoutVersion: 0,
      },
      excludedIds: [],
    })
    expect(source).toEqual({ kind: 'view', name: '当前视图' })
  })

  it('语义搜索模式(view_to_sql 拒绝解析):回退 explicit{当前视图布局序全集}', () => {
    const ai = useAiStore()
    ai.setSearchMode('semantic')
    expect(ai.isSemanticMode).toBe(true)

    const { buildCurrentViewExportPayload } = useExportEntries()
    const { selection, source } = buildCurrentViewExportPayload('当前视图')

    // 模块级 viewIds 单例未经 refresh() 时全集为空数组(同 useViewIds.spec.ts 默认态)。
    expect(selection).toEqual({ kind: 'explicit', ids: [] })
    expect(source).toEqual({ kind: 'view', name: '当前视图' })
  })
})
