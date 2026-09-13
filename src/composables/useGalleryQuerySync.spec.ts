// useGalleryQuerySync 的 wiring 级对拍(S 线审查 R-02 回归钉子)。
//
// ## 为什么存在
// writeUrl 的 watch 源是手工枚举,与 snapshot()/GalleryFilterSnapshot 字段集无编译期关联——
// `8161b12` 改了 snapshot()/applySnapshot() 却漏了 watch 列表的 fileFormats,症状是「选格式
// 后刷新 = 筛选静默丢失」,全链 unit 测试却全绿(experience §19:集合级契约证明不了接线对)。
// 本 spec 钉「接线」本身:逐字段改 filterStore → URL 必须跟着变。
//
// ## 覆盖完备性靠类型
// 用例表键类型是 keyof GalleryFilterSnapshot 的 Record——快照加字段而用例未加,vue-tsc 直接红;
// 用例在而 watch 源漏配,运行时断言红。两道门夹住 F-021(修枚举点时另一处同形枚举复发)。
//
// ## 环境
// vitest 为 node 环境、无 @vue/test-utils:mock vue-router 的 useRoute/useRouter 为受控 fake,
// 在 effectScope 里直接跑 composable。uiStore 真实现初始化即碰 window.matchMedia(node 下炸),
// 故三个旁路 store 换成最小 reactive fake,filterStore 用真 store(被测正是这条接线)。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { effectScope, nextTick, reactive } from 'vue'
import { createPinia, setActivePinia } from 'pinia'

const fakeRoute = reactive({
  path: '/',
  fullPath: '/',
  query: {} as Record<string, string>,
})
const replaceCalls: Array<Record<string, string>> = []
const fakeRouter = {
  isReady: () => Promise.resolve(),
  replace: (to: { query: Record<string, string> }) => {
    replaceCalls.push(to.query)
    fakeRoute.query = to.query
    return Promise.resolve()
  },
}

vi.mock('vue-router', () => ({
  useRoute: () => fakeRoute,
  useRouter: () => fakeRouter,
}))

// 重复镜头(2026-09-02 方案 §10.1)的镜头键接线走**真实** duplicateLensStore;它依赖应用路由
// 单例(动作要 push/replace 并读 currentRoute),node 下不可加载真模块 → mock '../router' 为受控
// fake:导航目标写回 fakeRoute(与 useRoute 的 fake 共享同一状态),模拟真实导航落地。
vi.mock('../router', () => {
  const pushes: unknown[] = []
  const replaces: unknown[] = []
  function applyNav(to: string | { path?: string; query?: Record<string, string> }) {
    if (typeof to === 'string') {
      const [path, qs] = to.split('?')
      fakeRoute.path = path
      fakeRoute.query = Object.fromEntries(new URLSearchParams(qs))
      fakeRoute.fullPath = to
      return
    }
    if (to.path !== undefined) fakeRoute.path = to.path
    // 对齐 vue-router 语义:query 未给 = 丢弃现有 query。
    fakeRoute.query = { ...(to.query ?? {}) }
    const qs = new URLSearchParams(fakeRoute.query).toString()
    fakeRoute.fullPath = fakeRoute.path + (qs ? `?${qs}` : '')
  }
  return {
    default: {
      isReady: () => Promise.resolve(),
      get currentRoute() {
        return { value: fakeRoute }
      },
      push(to: string | { path?: string; query?: Record<string, string> }) {
        pushes.push(to)
        applyNav(to)
        return Promise.resolve()
      },
      replace(to: string | { path?: string; query?: Record<string, string> }) {
        replaces.push(to)
        applyNav(to)
        return Promise.resolve()
      },
    },
  }
})

// 旁路 store 的最小 fake:字段值取「encode 视为默认、不写 URL」的那组,保证基线 query 为空。
vi.mock('../stores/uiStore', async () => {
  const { reactive: r } = await import('vue')
  const ui = r({
    groupBy: 'date',
    sortWithinGroup: 'datetime',
    sortOrder: 'desc',
    layoutMode: 'justified',
    startupConfigPromise: Promise.resolve({}),
    setGroupBy(v: string) {
      ui.groupBy = v
    },
    setSortWithinGroup(v: string) {
      ui.sortWithinGroup = v
    },
  })
  return { useUiStore: () => ui }
})
vi.mock('../stores/aiStore', async () => {
  const { reactive: r } = await import('vue')
  const ai = r({ isSemanticMode: false })
  return { useAiStore: () => ai }
})
vi.mock('../stores/searchStore', async () => {
  const { reactive: r } = await import('vue')
  const s = r({
    mode: 'mixed',
    scope: 'filename',
    committedQuery: '',
    setMode() {},
    setScope() {},
    apply() {},
  })
  return { useSearchStore: () => s }
})

import { useGalleryQuerySync } from './useGalleryQuerySync'
import { useFilterStore } from '../stores/filterStore'
import { useDuplicateLensStore } from '../stores/duplicateLensStore'
import type { GalleryFilterSnapshot } from '../utils/galleryQuery'

/** 冲刷水合链(Promise.all → nextTick → readUrl)与 watch→replace 的微任务。 */
async function flush() {
  for (let i = 0; i < 4; i++) {
    await nextTick()
    await Promise.resolve()
  }
}

async function setup(initialQuery: Record<string, string> = {}) {
  setActivePinia(createPinia())
  fakeRoute.path = '/'
  fakeRoute.query = initialQuery
  // fullPath 与 query 保持一致(enterLens 的返回快照读的就是它,必须真实可解析回 query)。
  const qs = new URLSearchParams(initialQuery).toString()
  fakeRoute.fullPath = qs ? `/?${qs}` : '/'
  replaceCalls.length = 0
  const scope = effectScope()
  scope.run(() => useGalleryQuerySync())
  await flush() // 等 hydrated 置位,之后的 store 变更才会写 URL
  return { filter: useFilterStore(), scope }
}

describe('useGalleryQuerySync:filter → URL 接线(R-02)', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('fileFormats 变更必须触发 writeUrl(R-02 直接回归)', async () => {
    const { filter, scope } = await setup()
    filter.setFileFormats(['png'])
    await flush()
    expect(fakeRoute.query.formats).toBe('png')
    scope.stop()
  })

  // 类型完备门:GalleryFilterSnapshot 每个字段必须有一条「改它 → URL 变」的用例。
  // 快照加字段而这里没加用例 = vue-tsc 红;用例在而 watch 源漏配 = 下面的运行时断言红。
  const mutations: Record<
    keyof GalleryFilterSnapshot,
    (f: ReturnType<typeof useFilterStore>) => void
  > = {
    mediaTypes: (f) => f.setMediaTypes(['image']),
    fileFormats: (f) => f.setFileFormats(['png']),
    favoritedOnly: (f) => {
      f.favoritedOnly = true
    },
    livePhotoOnly: (f) => {
      f.livePhotoOnly = true
    },
    minRating: (f) => {
      f.minRating = 3
    },
    colorLabel: (f) => {
      f.colorLabel = 2
    },
    // 单端日期也必须写 URL(from/to 各自独立键)——这正是 watch 源不能收敛为 apiFilterKey 的
    // 原因:apiFilterKey 有意吞掉单端变化,URL 侧不能吞。
    dateFrom: (f) => {
      f.dateFrom = 1_700_000_000
    },
    dateTo: (f) => {
      f.dateTo = 1_800_000_000
    },
  }

  for (const [field, mutate] of Object.entries(mutations)) {
    it(`${field} 变更后 URL query 必须变化`, async () => {
      const { filter, scope } = await setup()
      const before = JSON.stringify(fakeRoute.query)
      mutate(filter)
      await flush()
      expect(JSON.stringify(fakeRoute.query), `watch 源疑似漏配 ${field}`).not.toBe(before)
      scope.stop()
    })
  }

  it('水合前不写 URL(hydrated 门)', async () => {
    setActivePinia(createPinia())
    fakeRoute.path = '/'
    fakeRoute.fullPath = '/'
    fakeRoute.query = {}
    replaceCalls.length = 0
    const scope = effectScope()
    scope.run(() => useGalleryQuerySync())
    // 不 flush 水合链,立即改 store:此时 hydrated=false,不得 replace。
    useFilterStore().setFileFormats(['png'])
    await nextTick()
    expect(replaceCalls.length).toBe(0)
    scope.stop()
    await flush() // 收尾冲刷,避免悬挂微任务泄漏到下一个用例
  })
})

describe('useGalleryQuerySync:重复镜头键接线(2026-09-02 方案 §10.1/§4.4)', () => {
  // 镜头键是唯一「URL 是事实源、store 是镜像」的键组:深链/动作 push/浏览器返回都经路由 watcher
  // 收敛进 store;store→URL 侧只要求「其他键触发 writeUrl 时镜头键不被抹掉」。
  it('深链 ?duplicates=groups 水合即开镜头', async () => {
    const { scope } = await setup({ duplicates: 'groups' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBe('groups')
    expect(lens.showUniqueItems).toBe(false)
    scope.stop()
  })

  it('深链 folders+duplicateUnique=1 → mode/showUniqueItems 齐备', async () => {
    const { scope } = await setup({ duplicates: 'folders', duplicateUnique: '1' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBe('folders')
    expect(lens.showUniqueItems).toBe(true)
    scope.stop()
  })

  it('路由 query 变化(模拟系统返回)→ 镜头关闭', async () => {
    const { scope } = await setup({ duplicates: 'groups' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBe('groups')
    // 直接改路由 query(等价浏览器 Back:URL 丢掉镜头键)。
    fakeRoute.query = {}
    await flush()
    expect(lens.mode).toBeNull()
    scope.stop()
  })

  it('垃圾输入规范化:duplicates=banana&duplicateUnique=1 → 镜头关、URL 两键清除', async () => {
    const { scope } = await setup({ duplicates: 'banana', duplicateUnique: '1' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBeNull()
    expect(lens.showUniqueItems).toBe(false)
    // §4.4「其他组合规范化移除」:replace 只重写镜头两键。
    expect(fakeRoute.query.duplicates).toBeUndefined()
    expect(fakeRoute.query.duplicateUnique).toBeUndefined()
    scope.stop()
  })

  it('非法组合 duplicates=groups&duplicateUnique=1 → duplicateUnique 移除', async () => {
    const { scope } = await setup({ duplicates: 'groups', duplicateUnique: '1' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBe('groups')
    expect(lens.showUniqueItems).toBe(false)
    expect(fakeRoute.query.duplicateUnique).toBeUndefined()
    expect(fakeRoute.query.duplicates).toBe('groups')
    scope.stop()
  })

  it('进镜头/切模式/退出(经 store 动作):URL 与 store 双向一致', async () => {
    const { scope } = await setup({ types: 'image' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBeNull()

    lens.enterLens('groups')
    await flush()
    expect(lens.mode).toBe('groups')
    // 进入前已有的普通筛选键与镜头键共存(§4.4),enterLens 用 push 落 URL。
    expect(fakeRoute.query).toEqual({ types: 'image', duplicates: 'groups' })

    lens.setMode('folders')
    await flush()
    expect(lens.mode).toBe('folders')
    expect(fakeRoute.query).toEqual({ types: 'image', duplicates: 'folders' })

    lens.exitLens()
    await flush()
    expect(lens.mode).toBeNull()
    expect(lens.returnFullPath).toBeNull()
    // 退出恢复返回快照 fullPath(含筛选),无镜头键。
    expect(fakeRoute.query).toEqual({ types: 'image' })
    scope.stop()
  })

  it('镜头激活期间普通筛选变化 → writeUrl 共存重投影,镜头键不被抹掉', async () => {
    const { filter, scope } = await setup({ duplicates: 'groups' })
    const lens = useDuplicateLensStore()
    expect(lens.mode).toBe('groups')

    filter.setFileFormats(['png'])
    await flush()
    expect(fakeRoute.query.duplicates).toBe('groups')
    expect(fakeRoute.query.formats).toBe('png')

    // 镜头关闭(经路由变化)后再动筛选:镜头键不得被写回。
    fakeRoute.query = {}
    await flush()
    expect(lens.mode).toBeNull()
    filter.setFileFormats([])
    await flush()
    expect(fakeRoute.query.duplicates).toBeUndefined()
    expect(fakeRoute.query.duplicateUnique).toBeUndefined()
    scope.stop()
  })
})
