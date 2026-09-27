import { beforeEach, expect, it, vi } from 'vitest'
import type { PluginEntitlement } from '../types/exotic'
import { useOfficialEntitlement } from './useOfficialEntitlement'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc: invoke }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
beforeEach(() => invoke.mockReset())

const licensed: PluginEntitlement = {
  pluginId: 'scrollery-official', availability: 'authorized',
  sku: 'scrollery-official-onetime', sourceTag: 'test', storeUrl: null,
}

it('移除授权后的新查询先完成时，旧授权结果不可覆盖新态或触发打开编辑器', async () => {
  let finishOld!: (result: PluginEntitlement) => void
  invoke.mockImplementationOnce(() => new Promise<PluginEntitlement>((resolve) => { finishOld = resolve }))
  const gate = useOfficialEntitlement()
  const oldRequest = gate.fetchEntitlement()
  invoke.mockResolvedValueOnce({ ...licensed, availability: 'installedUnlicensed' })
  await gate.fetchEntitlement()
  finishOld(licensed)
  expect(await oldRequest).toBeNull()
  expect(gate.entitlement.value?.availability).toBe('installedUnlicensed')
  expect(gate.isAuthorized.value).toBe(false)
  expect(gate.loading.value).toBe(false)
})
