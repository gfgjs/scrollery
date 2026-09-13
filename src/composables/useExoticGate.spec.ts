// src/composables/useExoticGate.spec.ts
// Part5 T12 增量3：锁死逐项 gate 的「格式判定 → item-state 取数 → 适配 entitlement」链路与激活封装。
// 授权真相在后端；本 spec 只验前端映射/分流（哪些格式发 IPC、resolution 如何适配）不跑偏。
//
// 用**普通可变 handler**（非 vi.fn）按 cmd 分发——vi.fn/tinyspy 会把 mock 抛出/拒绝的值经内部机制
// 上报为 unhandled，即便下游已 catch（vitest v3.2.6 实测误判，见 usePluginEntitlement.spec 注释）。

import { describe, it, expect, beforeEach, vi } from 'vitest'

type InvokeHandler = (cmd: string, args: unknown) => unknown
const { state } = vi.hoisted(() => ({ state: { handler: (() => undefined) as InvokeHandler } }))
const calls: Array<{ cmd: string; args: unknown }> = []
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: unknown) => {
    calls.push({ cmd, args })
    return state.handler(cmd, args)
  },
}))

import {
  useExoticGate,
  resolutionToEntitlement,
  resetExoticFormatCache,
} from './useExoticGate'
import type { FormatResolution } from '../types/exotic'

function res(overrides: Partial<FormatResolution> = {}): FormatResolution {
  return {
    format: 'psd',
    mediaKind: 'image',
    pluginId: 'exotic-image-psd',
    capabilities: ['thumbnail'],
    availability: 'installedUnlicensed',
    storeUrl: 'https://store.example/psd',
    installedVersion: null,
    builtin: false,
    ...overrides,
  }
}

/** 按命令名分发 mock 返回值（未列出的命令返回 undefined）。 */
function route(map: Record<string, unknown>) {
  state.handler = (cmd) => {
    if (cmd in map) {
      const v = map[cmd]
      if (v instanceof Error) throw v
      return v
    }
    return undefined
  }
}

beforeEach(() => {
  calls.length = 0
  state.handler = () => undefined
  resetExoticFormatCache() // 模块级格式集缓存跨用例持久，须显式清
})

describe('resolutionToEntitlement', () => {
  it('适配 FormatResolution → PluginEntitlement（sku=null / sourceTag 空 / 透出 storeUrl）', () => {
    const e = resolutionToEntitlement(res({ availability: 'licenseExpired' }))
    expect(e).toEqual({
      pluginId: 'exotic-image-psd',
      availability: 'licenseExpired',
      sourceTag: '',
      sku: null,
      storeUrl: 'https://store.example/psd',
    })
  })

  it('pluginId 为 null → 空串（不 crash）', () => {
    expect(resolutionToEntitlement(res({ pluginId: null })).pluginId).toBe('')
  })
})

describe('useExoticGate.resolveForItem', () => {
  it('普通格式（不在 catalog）→ 返回 false 且不发 item-state IPC', async () => {
    route({ list_exotic_format_resolutions: [res({ format: 'psd' })] })
    const g = useExoticGate()
    const isExotic = await g.resolveForItem(1, 'jpg')
    expect(isExotic).toBe(false)
    expect(g.entitlement.value).toBeNull()
    // 只应发格式集查询，绝不为普通格式发 get_exotic_item_state。
    expect(calls.map((c) => c.cmd)).toEqual(['list_exotic_format_resolutions'])
  })

  it('exotic 格式 + 有 resolution → true 且 entitlement 适配就绪', async () => {
    route({
      list_exotic_format_resolutions: [res({ format: 'psd' })],
      get_exotic_item_state: {
        itemId: 7,
        format: 'psd',
        resolution: res({ availability: 'availableUninstalled' }),
        taskState: 'none',
      },
    })
    const g = useExoticGate()
    const isExotic = await g.resolveForItem(7, 'PSD') // 大小写不敏感
    expect(isExotic).toBe(true)
    expect(g.entitlement.value?.availability).toBe('availableUninstalled')
    expect(g.entitlement.value?.storeUrl).toBe('https://store.example/psd')
    // 参数契约：camelCase itemId。
    expect(calls[calls.length - 1]).toEqual({ cmd: 'get_exotic_item_state', args: { itemId: 7 } })
  })

  it('exotic 格式但 item resolution=null（竞态兜底）→ false、放行', async () => {
    route({
      list_exotic_format_resolutions: [res({ format: 'psd' })],
      get_exotic_item_state: { itemId: 7, format: 'psd', resolution: null, taskState: 'none' },
    })
    const g = useExoticGate()
    expect(await g.resolveForItem(7, 'psd')).toBe(false)
    expect(g.entitlement.value).toBeNull()
  })

  it('格式集拉取失败 → 空集放行（false），不误拦', async () => {
    route({ list_exotic_format_resolutions: new Error('boom') })
    const g = useExoticGate()
    expect(await g.resolveForItem(1, 'psd')).toBe(false)
    // 失败不写缓存：下次可重试（此处仅验不 crash、放行）。
  })

  it('item-state 拉取失败 → false、entitlement 归 null（不 crash）', async () => {
    route({
      list_exotic_format_resolutions: [res({ format: 'psd' })],
      get_exotic_item_state: new Error('ipc down'),
    })
    const g = useExoticGate()
    expect(await g.resolveForItem(7, 'psd')).toBe(false)
    expect(g.entitlement.value).toBeNull()
  })

  it('格式集缓存复用：多次解析只拉一次 list', async () => {
    route({
      list_exotic_format_resolutions: [res({ format: 'psd' })],
      get_exotic_item_state: { itemId: 1, format: 'psd', resolution: res(), taskState: 'none' },
    })
    const g = useExoticGate()
    await g.resolveForItem(1, 'psd')
    await g.resolveForItem(2, 'jpg')
    const listCalls = calls.filter((c) => c.cmd === 'list_exotic_format_resolutions')
    expect(listCalls).toHaveLength(1)
  })

  it('切换条目后，旧 item-state 响应不得覆盖新条目的 gate 状态', async () => {
    let resolveOld!: (value: unknown) => void
    const oldResponse = new Promise<unknown>((resolve) => {
      resolveOld = resolve
    })
    state.handler = (cmd, args) => {
      if (cmd === 'list_exotic_format_resolutions') return [res({ format: 'psd' })]
      if (cmd === 'get_exotic_item_state') {
        const itemId = (args as { itemId: number }).itemId
        if (itemId === 1) return oldResponse
        return {
          itemId,
          format: 'psd',
          resolution: res({ availability: 'licenseExpired' }),
          taskState: 'none',
        }
      }
      return undefined
    }

    const g = useExoticGate()
    const first = g.resolveForItem(1, 'psd')
    await vi.waitFor(() => {
      expect(calls).toContainEqual({ cmd: 'get_exotic_item_state', args: { itemId: 1 } })
    })
    expect(await g.resolveForItem(2, 'psd')).toBe(true)
    expect(g.entitlement.value?.availability).toBe('licenseExpired')

    resolveOld({
      itemId: 1,
      format: 'psd',
      resolution: res({ availability: 'authorized' }),
      taskState: 'none',
    })
    expect(await first).toBe(false)
    expect(g.entitlement.value?.availability).toBe('licenseExpired')
    expect(g.loading.value).toBe(false)
  })
})

describe('useExoticGate.activate', () => {
  it('以 camelCase pluginId+token 调激活命令', async () => {
    route({ activate_exotic_plugin: undefined })
    const g = useExoticGate()
    await g.activate('exotic-image-psd', 'TOKEN.abc')
    expect(calls[calls.length - 1]).toEqual({
      cmd: 'activate_exotic_plugin',
      args: { pluginId: 'exotic-image-psd', token: 'TOKEN.abc' },
    })
  })

  it('激活失败：错误向上抛（含稳定 code），供对话框展示', async () => {
    route({ activate_exotic_plugin: new Error('bad_token') })
    const g = useExoticGate()
    await expect(g.activate('p', 't')).rejects.toBeTruthy()
  })
})
