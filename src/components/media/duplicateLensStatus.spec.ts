// 重复镜头 §9 状态表判定单源的锁死测试(2026-09-02 主画廊重复项浏览方案 §9)。
// 纯函数零依赖:枚举运行态 × 布局有无 → 视图 kind/文案键/主动作,防后续改判定时静默漂移。

import { describe, expect, it } from 'vitest'
import { resolveDuplicateLensStatusView } from './duplicateLensStatus'

const BASE = {
  lensMode: 'groups',
  hasLayout: false,
  separatorCount: 0,
  positionCount: 0,
  percent: 0,
  completedAt: null,
  safeErrorCode: null,
} as const

describe('resolveDuplicateLensStatusView(方案 §9 状态表)', () => {
  it('idle 且无布局内容 → 从未完成分析(开始分析)', () => {
    const view = resolveDuplicateLensStatusView({ ...BASE, runStatus: 'idle' })
    expect(view.kind).toBe('notAnalyzed')
    expect(view.titleKey).toBe('duplicatesLens.notAnalyzedTitle')
    expect(view.descKey).toBe('duplicatesLens.notAnalyzedDesc')
    expect(view.primaryAction).toBe('start')
  })

  it('running 且无已发布结果 → 正在分析(停止)', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'running',
      percent: 42,
    })
    expect(view.kind).toBe('analyzing')
    expect(view.titleParams).toEqual({ percent: 42 })
    expect(view.primaryAction).toBe('stop')
  })

  it('running 且有旧结果 → 正在更新(停止),stopping 同判', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'running',
      hasLayout: true,
      separatorCount: 3,
      positionCount: 9,
      percent: 80,
    })
    expect(view.kind).toBe('updating')
    expect(view.titleParams).toEqual({ percent: 80 })
    expect(view.primaryAction).toBe('stop')
    const stopping = resolveDuplicateLensStatusView({
      lensMode: 'groups',
      runStatus: 'stopping',
      hasLayout: true,
      separatorCount: 3,
      positionCount: 9,
      percent: 80,
      completedAt: null,
      safeErrorCode: null,
    })
    expect(stopping.kind).toBe('updating')
  })

  it('completed 且有重复 → 已分析(重新分析),有完成时刻时次文案为时间键', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'completed',
      hasLayout: true,
      separatorCount: 83,
      positionCount: 214,
      completedAt: 1725200000000,
    })
    expect(view.kind).toBe('analyzed')
    expect(view.titleParams).toEqual({ groups: 83, positions: 214 })
    expect(view.descKey).toBe('duplicatesLens.completedAt')
    expect(view.primaryAction).toBe('restart')
  })

  it('completed 且会话内无完成时刻 → 已分析但省略时间文案', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'completed',
      hasLayout: true,
      separatorCount: 1,
      positionCount: 2,
    })
    expect(view.kind).toBe('analyzed')
    expect(view.descKey).toBeNull()
  })

  it('folders 模式已分析 → 文件夹口径文案键与参数(separator 是文件夹头,非重复组)', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      lensMode: 'folders',
      runStatus: 'completed',
      hasLayout: true,
      separatorCount: 4,
      positionCount: 30,
    })
    expect(view.kind).toBe('analyzed')
    expect(view.titleKey).toBe('duplicatesLens.analyzedFoldersTitle')
    expect(view.titleParams).toEqual({ folders: 4, positions: 30 })
  })

  it('completed 且零组 → 未发现精确重复项(重新分析)', () => {
    const view = resolveDuplicateLensStatusView({ ...BASE, runStatus: 'completed' })
    expect(view.kind).toBe('noDuplicates')
    expect(view.titleKey).toBe('duplicatesLens.noDuplicatesTitle')
    expect(view.primaryAction).toBe('restart')
  })

  it('failed 有旧结果 → 更新失败继续显示(重试),次文案带稳定错误码', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'failed',
      hasLayout: true,
      separatorCount: 2,
      positionCount: 4,
      safeErrorCode: 'DEDUP_HASH_UNAVAILABLE',
    })
    expect(view.kind).toBe('failedWithResults')
    expect(view.descKey).toBe('duplicatesLens.failedDetail')
    expect(view.safeErrorCode).toBe('DEDUP_HASH_UNAVAILABLE')
    expect(view.primaryAction).toBe('retry')
  })

  it('failed 无旧结果 → 无法完成分析(重试),无错误码时省略次文案', () => {
    const view = resolveDuplicateLensStatusView({ ...BASE, runStatus: 'failed' })
    expect(view.kind).toBe('failedNoResults')
    expect(view.descKey).toBeNull()
    expect(view.primaryAction).toBe('retry')
  })

  it('cancelled 有旧结果 → 已停止显示上次结果(重新分析)', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'cancelled',
      hasLayout: true,
      separatorCount: 5,
      positionCount: 11,
    })
    expect(view.kind).toBe('stopped')
    expect(view.primaryAction).toBe('restart')
  })

  it('cancelled 无结果与 idle 同判为从未分析', () => {
    const view = resolveDuplicateLensStatusView({ ...BASE, runStatus: 'cancelled' })
    expect(view.kind).toBe('notAnalyzed')
    expect(view.primaryAction).toBe('start')
  })

  it('idle 但布局已有内容(后端状态归零)→ 以屏幕内容为准按已分析处理', () => {
    const view = resolveDuplicateLensStatusView({
      ...BASE,
      runStatus: 'idle',
      hasLayout: true,
      separatorCount: 7,
      positionCount: 15,
    })
    expect(view.kind).toBe('analyzed')
    expect(view.titleParams).toEqual({ groups: 7, positions: 15 })
    expect(view.primaryAction).toBe('restart')
  })
})
