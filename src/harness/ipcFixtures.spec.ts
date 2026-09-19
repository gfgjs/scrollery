// src/harness/ipcFixtures.spec.ts
// 开发 harness 的 IPC fixture 契约测试(设置集中保存,2026-09-16):启动批与设置读写必须走新契约
// ——get_startup_config 返回 { settings 快照, state 内部状态 }、设置提交走快照 + set_app_settings/
// clear_settings。本文件同时是「旧扁平设置契约已彻底退场」的看门测试:任何一条旧设置 IPC 兼容
// 分支回归,这里都会红。
//
// 2026-09-16 精简:只留契约主干(启动批两半与旧契约退场、批量提交只报真变键、提交代次守卫、
// 内部状态通道拒绝设置键);主题种子参数、结构类键逐字段落值、get/set_app_config 正常读写与
// 重置默认等 fixture 自身的数据搬运用例整删。每条场景仍是独立 it。
//
// 环境:node,直接调用 invokeHarness(不经过 invokeIpc 的 isUiHarness 分流,故无需浏览器)。
import { describe, expect, it } from 'vitest'
import { IPC } from '../constants/ipc'
import { invokeHarness } from './ipcFixtures'
import type { SettingsChange, SettingsSnapshot, StartupPayload } from '../types/config'

function snapshot(): Promise<SettingsSnapshot> {
  return invokeHarness<SettingsSnapshot>(IPC.GET_SETTINGS_SNAPSHOT)
}

/** 带当前代次的提交(正常路径)。 */
async function patchSettingsNow(patch: Record<string, string>): Promise<SettingsChange> {
  const current = await snapshot()
  return invokeHarness<SettingsChange>(IPC.SET_APP_SETTINGS, {
    patch,
    generation: current.generation,
  })
}

function resetSettings(): Promise<SettingsChange> {
  return invokeHarness<SettingsChange>(IPC.CLEAR_SETTINGS)
}

describe('harness fixture:启动批契约', () => {
  it('get_startup_config 返回设置快照 + 内部状态两半', async () => {
    const payload = await invokeHarness<StartupPayload>(IPC.GET_STARTUP_CONFIG)
    expect(Object.keys(payload).sort()).toEqual(['settings', 'state'])
    expect(Object.keys(payload.settings).sort()).toEqual(['generation', 'revision', 'values'])
    expect(payload.state).toEqual({ firstLaunch: 'false', guideSeen: 'true' })
    // 视觉基线依赖的键在快照里;内部状态不再混进设置值表。
    expect(payload.settings.values.ui_font_size).toBe('15')
    expect(payload.settings.values.first_launch).toBeUndefined()
    expect(payload.settings.values.guide_seen).toBeUndefined()
    // 置顶清单用 schema 默认值(两项常驻工具),而非空数组:重置后的结果要与真机默认一致。
    expect(JSON.parse(payload.settings.values.pinned_settings)).toEqual([
      'aiFullAnalysis',
      'faceFullAnalysis',
    ])
  })

  it('旧扁平启动契约已退场:设置名不再是顶层字段', async () => {
    const payload = (await invokeHarness<StartupPayload>(IPC.GET_STARTUP_CONFIG)) as unknown as Record<
      string,
      unknown
    >
    for (const legacy of ['language', 'themeLight', 'appearance', 'uiFontSize', 'groupBy']) {
      expect(payload[legacy]).toBeUndefined()
    }
  })
})

describe('harness fixture:设置批量提交', () => {
  it('set_app_settings 落值并只报真正变化的键,revision 递增', async () => {
    const before = await snapshot()
    const change = await patchSettingsNow({ ui_font_size: '17', log_level: 'info' })
    // log_level 与现值相同 → 不算变化;ui_font_size 变了 → 进 keys。
    expect(change.keys).toEqual(['ui_font_size'])
    expect(change.snapshot.revision).toBe(before.revision + 1)
    expect(change.snapshot.values.ui_font_size).toBe('17')
    expect(change.restart_required).toEqual([])
    expect(change.apply_failed).toEqual([])
    // 回执即最新快照:后续读取能看到刚落的值。
    expect((await snapshot()).values.ui_font_size).toBe('17')
    await resetSettings()
  })
})

describe('harness fixture:提交代次契约(重置后旧代次被拒)', () => {
  it('缺失 generation 的提交被拒(不能凭默认代次蒙过去)', async () => {
    await expect(
      invokeHarness(IPC.SET_APP_SETTINGS, { patch: { ui_font_size: '23' } }),
    ).rejects.toThrow(/代次不匹配/)
    expect((await snapshot()).values.ui_font_size).toBe('15')
  })

  it('重置后旧代次 patch 被拒,且不改变 fixture 的任何值', async () => {
    const before = await snapshot()
    // 模拟「窗口 A 准备提交时,窗口 B 已重置」:拿重置前的代次发 patch。
    await resetSettings()
    const stale = before.generation
    const current = await snapshot()
    expect(current.generation).toBe(stale + 1)
    await expect(
      invokeHarness(IPC.SET_APP_SETTINGS, {
        patch: { ui_font_size: '27' },
        generation: stale,
      }),
    ).rejects.toThrow(/代次不匹配/)
    // 被拒的旧值不得落进 fixture:字体仍为默认,revision 也未因这次拒绝而推进。
    const after = await snapshot()
    expect(after.values.ui_font_size).toBe('15')
    expect(after.revision).toBe(current.revision)
  })
})

describe('harness fixture:内部状态通道只认状态键', () => {
  it('设置类键走内部状态通道被拒(指向快照/批量提交入口)', async () => {
    await expect(invokeHarness(IPC.GET_APP_CONFIG, { key: 'ui_font_size' })).rejects.toThrow(
      /get_settings_snapshot/,
    )
    await expect(
      invokeHarness(IPC.SET_APP_CONFIG, { key: 'ui_font_size', value: '20' }),
    ).rejects.toThrow(/set_app_settings/)
  })
})
