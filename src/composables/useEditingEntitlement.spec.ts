import { beforeEach, describe, expect, it, vi } from 'vitest'

type InvokeHandler = (cmd: string, args: unknown) => unknown
const { state } = vi.hoisted(() => ({ state: { handler: (() => undefined) as InvokeHandler } }))
const calls: Array<{ cmd: string; args: unknown }> = []
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: unknown) => {
    calls.push({ cmd, args })
    return state.handler(cmd, args)
  },
}))

import { EDITING_PLUGIN_ID, useEditingEntitlement } from './useEditingEntitlement'
import type { PluginEntitlement } from '../types/exotic'

function entitlement(availability: PluginEntitlement['availability']): PluginEntitlement {
  return {
    pluginId: EDITING_PLUGIN_ID,
    availability,
    sourceTag: 'test',
    sku: 'editing-tools-2026',
    storeUrl: null,
  }
}

beforeEach(() => {
  calls.length = 0
  state.handler = () => undefined
})

describe('useEditingEntitlement', () => {
  it('授权查询成功后只对 authorized 放行', async () => {
    state.handler = () => entitlement('authorized')
    const gate = useEditingEntitlement()
    const result = await gate.fetchEntitlement()
    expect(result.availability).toBe('authorized')
    expect(gate.isAuthorized.value).toBe(true)
    expect(calls).toEqual([{ cmd: 'get_editing_entitlement', args: undefined }])
  })

  it('授权查询失败时合成未授权态，禁止 PluginGate 的 null fail-open', async () => {
    state.handler = () => {
      throw { code: 'System', message: 'keyring unavailable' }
    }
    const gate = useEditingEntitlement()
    const result = await gate.fetchEntitlement()
    expect(result.availability).toBe('installedUnlicensed')
    expect(gate.isAuthorized.value).toBe(false)
    expect(gate.error.value?.code).toBe('System')
  })

  it('激活命令只发送 token，不允许前端覆盖 plugin id 或 SKU', async () => {
    state.handler = () => undefined
    const gate = useEditingEntitlement()
    await gate.activate('ignored', 'license-token')
    expect(calls).toEqual([{ cmd: 'activate_editing_feature', args: { token: 'license-token' } }])
    expect(gate.activating.value).toBe(false)
  })
})
