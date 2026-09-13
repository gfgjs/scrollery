// viewerStore 单测(顶栏重构 P0-3)。重点锁 token 时序防御:旧查看器迟到的 clear 不得误清
// 新查看器刚 populate 的状态。同时建立本项目 pinia setup store 的测试范式。
import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useViewerStore, type ActiveViewer } from './viewerStore'

function makeViewer(overrides: Partial<ActiveViewer> = {}): ActiveViewer {
  return {
    kind: 'image',
    mediaType: 'image',
    fileFormat: 'jpg',
    id: 1,
    path: null,
    title: 'a.jpg',
    api: null,
    immersive: false,
    fileInfo: {
      rating: 0,
      isFavorited: false,
      colorLabel: 0,
      fileSize: 1024,
      width: 800,
      height: 600,
      durationMs: null,
    },
    ...overrides,
  }
}

describe('viewerStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('populate 写入 activeViewer,hasActiveViewer 转真', () => {
    const s = useViewerStore()
    expect(s.hasActiveViewer).toBe(false)
    s.populate(makeViewer({ id: 7, title: '7.jpg' }))
    expect(s.hasActiveViewer).toBe(true)
    expect(s.activeViewer?.id).toBe(7)
  })

  it('clear(token) 在 token 匹配时清空', () => {
    const s = useViewerStore()
    const token = s.populate(makeViewer())
    s.clear(token)
    expect(s.activeViewer).toBeNull()
    expect(s.hasActiveViewer).toBe(false)
  })

  it('时序防御:旧 token 的迟到 clear 不误清新查看器', () => {
    const s = useViewerStore()
    const oldToken = s.populate(makeViewer({ id: 1 }))
    const newToken = s.populate(makeViewer({ id: 2 })) // 新查看器抢占
    s.clear(oldToken) // 旧查看器迟到的 clear —— 应被 token 不匹配挡下
    expect(s.activeViewer?.id).toBe(2) // 新状态不受影响
    s.clear(newToken) // 当前 owner 的 clear 生效
    expect(s.activeViewer).toBeNull()
  })

  it('setImmersive 走不可变替换,isImmersive 反映', () => {
    const s = useViewerStore()
    s.populate(makeViewer({ immersive: false }))
    const before = s.activeViewer
    s.setImmersive(true)
    expect(s.isImmersive).toBe(true)
    expect(s.activeViewer).not.toBe(before) // 不可变替换:引用已换(shallowRef 触发响应式)
  })

  it('patch 仅在 token 匹配时增量更新', () => {
    const s = useViewerStore()
    const token = s.populate(makeViewer({ api: null }))
    const api = { zoomIn: () => {} }
    s.patch(token, { api })
    expect(s.activeViewer?.api).toBe(api)
    // 新查看器抢占后,旧 token 的 patch 无效
    s.populate(makeViewer({ id: 99, title: 'fresh' }))
    s.patch(token, { title: 'stale' })
    expect(s.activeViewer?.title).toBe('fresh')
  })

  // ── applyFieldPatch(底栏文件信息线):itemPatchSignal 桥的回写落点 ──────────
  it('applyFieldPatch:id 匹配时不可变回写对应标量', () => {
    const s = useViewerStore()
    s.populate(makeViewer({ id: 5 }))
    const before = s.activeViewer
    s.applyFieldPatch(5, 'rating', 4)
    expect(s.activeViewer?.fileInfo?.rating).toBe(4)
    expect(s.activeViewer).not.toBe(before) // shallowRef 不可变替换才触发响应式
    s.applyFieldPatch(5, 'isFavorited', true)
    expect(s.activeViewer?.fileInfo?.isFavorited).toBe(true)
    s.applyFieldPatch(5, 'colorLabel', 3)
    expect(s.activeViewer?.fileInfo?.colorLabel).toBe(3)
  })

  it('applyFieldPatch:id 不匹配丢弃(迟到的旧项改动不污染当前项)', () => {
    const s = useViewerStore()
    s.populate(makeViewer({ id: 5 }))
    s.applyFieldPatch(6, 'rating', 4)
    expect(s.activeViewer?.fileInfo?.rating).toBe(0)
  })

  it('applyFieldPatch:fileInfo 未就绪(null)时 no-op 不崩', () => {
    const s = useViewerStore()
    s.populate(makeViewer({ id: 5, fileInfo: null }))
    s.applyFieldPatch(5, 'rating', 4)
    expect(s.activeViewer?.fileInfo).toBeNull()
  })
})
