// 侧栏 route-return 会撤销一个已经排队、但尚未落地的旧宽度防抖；否则它可能在过渡结束后
// 以中间几何调用 compute_layout。本测试只钉这一条计时器契约，不覆盖完整布局输入映射。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, reactive, ref } from 'vue'

const media = {
  computeLayout: vi.fn(async () => {}),
}
const filter = {
  apiFilterKey: '{}',
  toApiFilter: () => ({}),
}
const ui = reactive({
  layoutMode: 'justified',
  resizeDebounceMs: 300,
  gridRowHeight: 200,
  groupBy: 'date',
  sortWithinGroup: 'datetime',
  sortOrder: 'desc',
  seamlessGroups: false,
  searchQuery: '',
  searchScope: 'all',
})
const viewStore = reactive({
  activeSmartAlbum: 'all',
  activeDirectoryId: null as number | null,
  activeCollection: null as { id: number } | null,
  activePersonId: null as number | null,
})
const ai = reactive({ isSemanticMode: false, similarityThreshold: 0.5 })
const scan = reactive({ isAnyScanRunning: false })
const lensState = reactive<{ mode: 'groups' | 'folders' | null; showUniqueItems: boolean }>({
  mode: null,
  showUniqueItems: false,
})

// 本用例只验证防抖计时器，不挂真实组件；中和 composable 注册生命周期时的 node 环境告警。
vi.mock('vue', async (importOriginal) => {
  const actual = await importOriginal<typeof import('vue')>()
  return { ...actual, onBeforeUnmount: () => {} }
})
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => media }))
vi.mock('../stores/filterStore', () => ({ useFilterStore: () => filter }))
vi.mock('../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../stores/viewStore', () => ({ useViewStore: () => viewStore }))
vi.mock('../stores/aiStore', () => ({ useAiStore: () => ai }))
vi.mock('../stores/scanStore', () => ({ useScanStore: () => scan }))
// 重复镜头(2026-09-02 方案):useJustifiedLayout 现依赖 duplicateLensStore(mode/showUniqueItems)。
// mock 掉以免拖入真实 store 的路由单例依赖(../router 在 node 环境不可加载);本 spec 只测防抖
// 计时器,镜头恒关闭(mode:null)即等价于旧行为。
vi.mock('../stores/duplicateLensStore', () => ({
  useDuplicateLensStore: () => lensState,
}))

import { useJustifiedLayout } from './useJustifiedLayout'

describe('useJustifiedLayout: cancelPendingResize', () => {
  const enabled = ref(true)

  beforeEach(() => {
    enabled.value = true
    media.computeLayout.mockClear()
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('撤销尚未执行的旧宽度防抖，不向布局 IPC 提交中间几何', async () => {
    const scope = effectScope()
    const layout = scope.run(() => useJustifiedLayout(() => 960, { enabled: () => enabled.value }))
    expect(layout).toBeDefined()

    layout?.onResize(700)
    layout?.cancelPendingResize()
    await vi.advanceTimersByTimeAsync(300)

    expect(media.computeLayout).not.toHaveBeenCalled()
    scope.stop()
  })
})
