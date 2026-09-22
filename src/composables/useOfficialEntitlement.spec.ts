import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'
import type { PluginEntitlement } from '../types/exotic'
import { useOfficialEntitlement } from './useOfficialEntitlement'
import { resetExoticFormatCache, useExoticGate } from './useExoticGate'
import { useEnhanceStore } from '../stores/enhanceStore'
import type { EnhanceStatus } from '../types/enhance'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc: invoke }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
beforeEach(() => invoke.mockReset())

const licensed: PluginEntitlement = {
  pluginId: 'scrollery-official', availability: 'authorized',
  sku: 'scrollery-official-onetime', sourceTag: 'test', storeUrl: null,
}

it('读取失败清除既有授权展示，重试成功恢复；失败不伪造购买态', async () => {
  const gate = useOfficialEntitlement()
  invoke.mockResolvedValueOnce(licensed)
  await gate.fetchEntitlement()
  expect(gate.isAuthorized.value).toBe(true)
  const failure = { code: 'keyring_unavailable' }
  invoke.mockRejectedValueOnce(failure)
  expect(await gate.fetchEntitlement()).toBeNull()
  expect(gate.entitlement.value).toBeNull()
  expect(gate.error.value).toEqual(failure)
  expect(gate.isAuthorized.value).toBe(false)
  invoke.mockResolvedValueOnce(licensed)
  await gate.fetchEntitlement()
  expect(gate.error.value).toBeNull()
  expect(gate.isAuthorized.value).toBe(true)
  expect(invoke).toHaveBeenLastCalledWith(IPC.GET_OFFICIAL_ENTITLEMENT)
})

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

it('已知 PSD 查询失败时显示错误；切换普通图片后清除错误并继续查看', async () => {
  resetExoticFormatCache()
  const gate = useExoticGate()
  invoke.mockResolvedValueOnce([{ format: 'psd' }])
  invoke.mockRejectedValueOnce({ code: 'keyring_unavailable' })
  await gate.resolveForItem(1, 'psd')
  expect(gate.failed.value).toBe(true)
  expect(gate.entitlement.value).toBeNull()
  expect(gate.loading.value).toBe(false)
  invoke.mockClear()
  await gate.resolveForItem(2, 'jpg')
  expect(gate.failed.value).toBe(false)
  expect(gate.entitlement.value).toBeNull()
  expect(invoke).not.toHaveBeenCalled()
})

it('移除官方版授权后，迟到的增强状态不能重新显示为已授权', async () => {
  setActivePinia(createPinia())
  const store = useEnhanceStore()
  let finishOld!: (result: EnhanceStatus) => void
  invoke.mockImplementationOnce(() => new Promise<EnhanceStatus>((resolve) => { finishOld = resolve }))
  const oldRequest = store.fetchStatus()
  const current: EnhanceStatus = { availability: 'availableUninstalled', storeUrl: null, workerReady: true, provider: null, models: [] }
  invoke.mockResolvedValueOnce(current)
  await store.fetchStatus()
  finishOld({ ...current, availability: 'authorized' })
  await oldRequest
  expect(store.status?.availability).toBe('availableUninstalled')
  expect(store.isAuthorized).toBe(false)
})
