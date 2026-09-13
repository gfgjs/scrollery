// useLensBrowseGate 的 §8.1 行为钉子：进入镜头立即清空普通画廊选区（useSelection 模块级单例
// 状态断言），非镜头路径不清。MediaGrid 级组件测试在 vitest node（无 DOM）不可行，故把 gate
// 收敛为本 composable 直测——watcher 时序与宿主 setup 中调用完全一致。
//
// 环境：vitest node、无 DOM。store 依赖应用路由单例，换最小 fake（同
// duplicateLensStore.spec 的做法），避免拖入真实 router/i18n 链。

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick } from 'vue'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('../router', () => ({
  default: {
    isReady: () => Promise.resolve(),
    currentRoute: { value: { fullPath: '/', path: '/', query: {} } },
    push: vi.fn(async () => {}),
    replace: vi.fn(async () => {}),
  },
}))

import { useDuplicateLensStore } from '../stores/duplicateLensStore'
import { useLensBrowseGate } from './useLensBrowseGate'
import { useSelection } from './useSelection'

beforeEach(() => {
  setActivePinia(createPinia())
})

/** 宿主 setup 等效:在独立 effectScope 内装 gate(watcher 随 scope 销毁,不跨用例泄漏)。 */
function installGate() {
  const scope = effectScope()
  const api = scope.run(() => useLensBrowseGate())!
  return { scope, api }
}

describe('useLensBrowseGate:进入镜头清空普通选区(§8.1)', () => {
  it('普通画廊已有选区 → enterLens 即清空(isSelectionMode false、计数归零)', async () => {
    const selection = useSelection()
    selection.toggleSelect(1)
    selection.toggleSelect(2)
    expect(selection.isSelectionMode.value).toBe(true)
    expect(selection.selectedCount.value).toBe(2)

    installGate()
    const lens = useDuplicateLensStore()
    lens.enterLens('groups')
    await nextTick()

    expect(selection.isSelectionMode.value).toBe(false)
    expect(selection.selectedCount.value).toBe(0)
  })

  it('挂载即处于镜头态(深链直开/失活期 URL 同步进入)→ immediate 清掉既有选区', async () => {
    const lens = useDuplicateLensStore()
    lens.$patch({ mode: 'groups' })
    const selection = useSelection()
    selection.toggleSelect(9)
    // 现实时序:选区产生于**早前的 flush**(用户交互),本 tick 只模拟「带着选区挂载 gate」。
    // 同一 flush 内 toggle→clear 会被 useSelection 空选区 watch 的去重吞掉(新旧值相等),
    // 那是测试人为序列,非真实路径。
    await nextTick()
    expect(selection.isSelectionMode.value).toBe(true)

    installGate()
    // isSelectionMode=false 由 useSelection 内部的空选区 watch(异步)落定。
    await nextTick()

    expect(selection.isSelectionMode.value).toBe(false)
    expect(selection.selectedCount.value).toBe(0)
  })

  it('普通画廊下安装 gate(非镜头路径)不清选区;退出镜头也不误清', async () => {
    installGate()
    const selection = useSelection()
    selection.toggleSelect(3)
    await nextTick()
    expect(selection.isSelectionMode.value).toBe(true)

    const lens = useDuplicateLensStore()
    lens.$patch({ mode: 'groups' })
    await nextTick()
    expect(selection.isSelectionMode.value).toBe(false)

    // 退出镜头回普通画廊:此处新做的选择(模拟退出后的正常交互)不受 gate 影响。
    lens.exitLens()
    await nextTick()
    selection.toggleSelect(5)
    await nextTick()
    expect(selection.isSelectionMode.value).toBe(true)
  })
})
