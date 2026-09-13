// duplicateLensStore 的快照语义与 URL 写入方向钉子（2026-09-02 方案 §10.1/§4.2/§4.3）。
//
// 覆盖：enter/exit 快照只拍一次、exit 原样恢复并清空、深链退出回「/」、push/replace 的历史语义、
// groups 下 duplicateUnique 白名单不落 URL、mode null 往返、isLensActive 谓词、快照随 mode 归
// null 清场（系统返回等不经 exitLens 的路径,§4.3「快照只属当前会话」）。
//
// 环境：vitest node、无 DOM。store 依赖应用路由单例（动作要 push/replace 并读 currentRoute），
// mock '../router' 为受控 fake（push/replace 记录调用并把目标写回 currentRoute,模拟导航落地）。

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const nav = vi.hoisted(() => {
  const state = {
    fullPath: '/',
    path: '/',
    query: {} as Record<string, string>,
  }
  const pushes: unknown[] = []
  const replaces: unknown[] = []
  const toQuery = (to: unknown): Record<string, string> => {
    if (typeof to === 'string') {
      const qs = to.split('?')[1] ?? ''
      return Object.fromEntries(new URLSearchParams(qs))
    }
    const q = (to as { query?: Record<string, string> }).query ?? {}
    return { ...q }
  }
  return {
    state,
    toQuery,
    pushes,
    replaces,
    reset() {
      state.fullPath = '/'
      state.path = '/'
      state.query = {}
      pushes.length = 0
      replaces.length = 0
    },
  }
})

vi.mock('../router', () => ({
  default: {
    isReady: () => Promise.resolve(),
    get currentRoute() {
      return { value: nav.state }
    },
    push(to: unknown) {
      nav.pushes.push(to)
      // 模拟导航落地：目标写回 currentRoute,后续动作读到的是新状态。
      nav.state.query = nav.toQuery(to)
      nav.state.path = typeof to === 'string' ? to.split('?')[0] : (to as { path?: string }).path ?? nav.state.path
      nav.state.fullPath =
        typeof to === 'string'
          ? to
          : nav.state.path +
            (Object.keys(nav.state.query).length
              ? `?${new URLSearchParams(nav.state.query).toString()}`
              : '')
      return Promise.resolve()
    },
    replace(to: unknown) {
      nav.replaces.push(to)
      nav.state.query = nav.toQuery(to)
      nav.state.fullPath =
        nav.state.path +
        (Object.keys(nav.state.query).length
          ? `?${new URLSearchParams(nav.state.query).toString()}`
          : '')
      return Promise.resolve()
    },
  },
}))

import { nextTick } from 'vue'
import { useDuplicateLensStore } from './duplicateLensStore'

beforeEach(() => {
  setActivePinia(createPinia())
  nav.reset()
})

describe('duplicateLensStore:isLensActive 谓词(§8.1 browse-only 单源开关)', () => {
  it('mode null → false;mode 非空 → true', () => {
    const lens = useDuplicateLensStore()
    expect(lens.isLensActive).toBe(false)
    lens.enterLens('groups')
    expect(lens.isLensActive).toBe(true)
    lens.exitLens()
    expect(lens.isLensActive).toBe(false)
  })
})

describe('duplicateLensStore:enterLens 快照语义(§10.1)', () => {
  it('进入时拍下完整路由,并 push 带 duplicates 键的 query(制造历史节点)', () => {
    nav.state.fullPath = '/?types=image&rating=3'
    nav.state.query = { types: 'image', rating: '3' }

    const lens = useDuplicateLensStore()
    lens.enterLens('groups')

    expect(lens.mode).toBe('groups')
    expect(lens.showUniqueItems).toBe(false)
    expect(lens.returnFullPath).toBe('/?types=image&rating=3')
    expect(nav.pushes).toHaveLength(1)
    expect(nav.replaces).toHaveLength(0)
    expect(nav.toQuery(nav.pushes[0])).toEqual({ types: 'image', rating: '3', duplicates: 'groups' })
  })

  it('镜头内切模式不重拍快照(返回锚点始终指向进入前状态)', () => {
    nav.state.fullPath = '/?types=image'
    nav.state.query = { types: 'image' }

    const lens = useDuplicateLensStore()
    lens.enterLens('groups')
    lens.setMode('folders')

    expect(lens.returnFullPath).toBe('/?types=image')
    // 排列切换走 replace(§4.2 不制造历史节点)
    expect(nav.replaces).toHaveLength(1)
    expect(nav.toQuery(nav.replaces[0])).toEqual({ types: 'image', duplicates: 'folders' })
  })
})

describe('duplicateLensStore:exitLens 恢复与清空(§4.3)', () => {
  it('从有快照的路径退出 → push 回原 fullPath(含筛选),快照清空、mode 归 null', () => {
    nav.state.fullPath = '/?types=image'
    nav.state.query = { types: 'image' }

    const lens = useDuplicateLensStore()
    lens.enterLens('groups')
    lens.exitLens()

    expect(nav.pushes[1]).toBe('/?types=image')
    expect(lens.mode).toBeNull()
    expect(lens.showUniqueItems).toBe(false)
    expect(lens.returnFullPath).toBeNull()
  })

  it('深链直开镜头(无快照)退出 → 回普通「/」', () => {
    const lens = useDuplicateLensStore()
    // 深链态:mode 由 useGalleryQuerySync 的 URL→store 同步写入,快照为空。
    lens.$patch({ mode: 'groups' })

    lens.exitLens()

    expect(nav.pushes).toHaveLength(1)
    expect(nav.pushes[0]).toEqual({ path: '/' })
    expect(lens.mode).toBeNull()
  })

  it('mode null 往返:enter→exit 后再次 enter,快照重新拍摄', () => {
    nav.state.fullPath = '/?q=x'
    nav.state.query = { q: 'x' }

    const lens = useDuplicateLensStore()
    lens.enterLens('groups')
    lens.exitLens()
    expect(lens.mode).toBeNull()

    lens.enterLens('groups')
    expect(lens.mode).toBe('groups')
    expect(lens.returnFullPath).toBe('/?q=x')
  })

  it('镜头关闭时 exitLens 是 no-op(不发导航)', () => {
    const lens = useDuplicateLensStore()
    lens.exitLens()
    expect(nav.pushes).toHaveLength(0)
    expect(nav.replaces).toHaveLength(0)
  })

  it('系统返回(mode 经 URL→store 同步收敛为 null,不经 exitLens)→ 快照随之清场,' +
    '后续深链退出回「/」而非陈旧快照(§4.3 快照只属当前会话)', async () => {
    nav.state.fullPath = '/?types=image'
    nav.state.query = { types: 'image' }

    const lens = useDuplicateLensStore()
    lens.enterLens('groups')
    expect(lens.returnFullPath).toBe('/?types=image')

    // 模拟浏览器返回:querySync 的 syncLensFromUrl 直接把 mode 写回 null。
    lens.$patch({ mode: null })
    await nextTick()
    expect(lens.returnFullPath).toBeNull()

    // 再深链进入(快照为空)→ 退出必须回普通「/」,不得把上一会话的旧路由推回历史。
    lens.$patch({ mode: 'groups' })
    lens.exitLens()
    expect(nav.pushes[nav.pushes.length - 1]).toEqual({ path: '/' })
  })
})

describe('duplicateLensStore:URL 写侧白名单(§4.4 写读闭合)', () => {
  it('setShowUniqueItems 仅 folders 生效;folders 下写 duplicateUnique=1', () => {
    const lens = useDuplicateLensStore()
    lens.$patch({ mode: 'folders' })

    lens.setShowUniqueItems(true)
    expect(nav.replaces).toHaveLength(1)
    expect(nav.toQuery(nav.replaces[0])).toEqual({ duplicates: 'folders', duplicateUnique: '1' })
    expect(lens.showUniqueItems).toBe(true)

    lens.setShowUniqueItems(true)
    expect(nav.replaces).toHaveLength(1)
  })

  it('groups 下 setShowUniqueItems no-op(避免写出读不回的非法组合)', () => {
    const lens = useDuplicateLensStore()
    lens.$patch({ mode: 'groups' })

    lens.setShowUniqueItems(true)
    expect(nav.replaces).toHaveLength(0)
    expect(lens.showUniqueItems).toBe(false)
  })

  it('setMode 切回 groups 时丢弃 showUniqueItems(groups 恒 false)', () => {
    const lens = useDuplicateLensStore()
    lens.$patch({ mode: 'folders', showUniqueItems: true })

    lens.setMode('groups')

    expect(lens.showUniqueItems).toBe(false)
    expect(nav.replaces).toHaveLength(1)
    expect(nav.toQuery(nav.replaces[0])).toEqual({ duplicates: 'groups' })
  })
})
