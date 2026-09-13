// src/composables/useExoticStore.spec.ts
// Part5 T11：锁死插件商店数据层的「registry×installed 合并模型」与安装生命周期的 IPC 契约。
// 用普通可变 handler（非 vi.fn）按 cmd 分发——规避 vi.fn/tinyspy 把 catch 掉的 mock 抛出误报为
// unhandled（vitest v3.2.6，详见 usePluginEntitlement.spec 注释）。

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

import { dedupeBuiltinOfferings, useExoticStore, mergeStorePlugins } from './useExoticStore'
import type { ExoticRegistryEntry, FormatResolution, InstalledExoticPlugin } from '../types/exotic'

function regEntry(overrides: Partial<ExoticRegistryEntry> = {}): ExoticRegistryEntry {
  return {
    pluginId: 'exotic-image-psd',
    version: '1.2.0',
    formats: ['psd'],
    capabilities: ['thumbnail'],
    sku: 'psd-engine-2026',
    target: 'x86_64-pc-windows-msvc',
    packageSequence: 5,
    storeUrl: 'https://store.example/psd',
    registryExpired: false,
    ...overrides,
  }
}

function installedEntry(overrides: Partial<InstalledExoticPlugin> = {}): InstalledExoticPlugin {
  return {
    pluginId: 'exotic-image-psd',
    version: '1.0.0',
    packageSequence: 3,
    installState: 'installed',
    installedAt: 1000,
    updatedAt: 1000,
    ...overrides,
  }
}

function resolution(overrides: Partial<FormatResolution> = {}): FormatResolution {
  return {
    format: 'cr2',
    mediaKind: 'image',
    pluginId: 'exotic-image-raw',
    capabilities: ['thumbnail'],
    availability: 'authorized',
    storeUrl: null,
    installedVersion: null,
    builtin: true,
    ...overrides,
  }
}

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
})

describe('mergeStorePlugins', () => {
  it('registry-only → 可装（installState=null）', () => {
    const rows = mergeStorePlugins([regEntry()], [])
    expect(rows).toHaveLength(1)
    expect(rows[0].installState).toBeNull()
    expect(rows[0].availableVersion).toBe('1.2.0')
    expect(rows[0].installedVersion).toBeNull()
    expect(rows[0].upgradable).toBe(false)
  })

  it('installed-only（registry 无此条目）→ 仍展示以支持卸载/修复', () => {
    const rows = mergeStorePlugins([], [installedEntry()])
    expect(rows).toHaveLength(1)
    expect(rows[0].availableVersion).toBeNull()
    expect(rows[0].installedVersion).toBe('1.0.0')
    expect(rows[0].installState).toBe('installed')
    expect(rows[0].upgradable).toBe(false)
  })

  it('both + registry 更高 packageSequence → upgradable', () => {
    const rows = mergeStorePlugins([regEntry({ packageSequence: 5 })], [installedEntry({ packageSequence: 3 })])
    expect(rows).toHaveLength(1)
    expect(rows[0].installState).toBe('installed')
    expect(rows[0].availableVersion).toBe('1.2.0')
    expect(rows[0].upgradable).toBe(true)
  })

  it('both + 同 packageSequence → 不可升级', () => {
    const rows = mergeStorePlugins([regEntry({ packageSequence: 3 })], [installedEntry({ packageSequence: 3 })])
    expect(rows[0].upgradable).toBe(false)
  })

  it('registry 过期 → 即便序号更高也不导为可升级（过期不允许新装）', () => {
    const rows = mergeStorePlugins(
      [regEntry({ packageSequence: 9, registryExpired: true })],
      [installedEntry({ packageSequence: 3 })],
    )
    expect(rows[0].upgradable).toBe(false)
  })
})

describe('dedupeBuiltinOfferings', () => {
  it('同一 builtin 插件声明多个格式时只保留一张展示卡片', () => {
    const offerings = dedupeBuiltinOfferings([
      resolution({ format: 'cr2' }),
      resolution({ format: 'cr3' }),
      resolution({ format: 'nef' }),
      resolution({ pluginId: 'video-extended', format: 'rmvb' }),
      resolution({ pluginId: 'video-extended', format: 'vob' }),
      resolution({ builtin: false, pluginId: 'exotic-image-psd', format: 'psd' }),
    ])

    expect(offerings.map((offering) => offering.pluginId)).toEqual([
      'exotic-image-raw',
      'video-extended',
    ])
    expect(offerings.map((offering) => offering.format)).toEqual(['cr2', 'rmvb'])
  })
})

describe('useExoticStore.loadAll', () => {
  it('并发拉三态填充 registry/installed/status', async () => {
    route({
      list_exotic_registry: [regEntry()],
      list_installed_exotic_plugins: [installedEntry()],
      get_exotic_processing_status: {
        pending: 2,
        processing: 0,
        done: 1,
        error: 0,
        blockedByAvailability: 2,
        running: false,
        paused: false,
      },
      list_exotic_format_resolutions: [],
    })
    const s = useExoticStore()
    await s.loadAll()
    expect(s.registry.value).toHaveLength(1)
    expect(s.installed.value).toHaveLength(1)
    expect(s.status.value?.blockedByAvailability).toBe(2)
    expect(s.error.value).toBeNull()
    expect(s.loading.value).toBe(false)
  })

  it('任一失败 → 置 error、不抛（列表视图容错）', async () => {
    route({
      list_exotic_registry: new Error('cache read failed'),
      list_installed_exotic_plugins: [],
      get_exotic_processing_status: {},
      list_exotic_format_resolutions: [],
    })
    const s = useExoticStore()
    await s.loadAll()
    expect(s.error.value).toBeTruthy()
    expect(s.loading.value).toBe(false)
  })
})

describe('useExoticStore 安装生命周期', () => {
  it('install：以 camelCase pluginId 调命令并重载已装列表', async () => {
    route({ install_exotic_plugin: undefined, list_installed_exotic_plugins: [installedEntry()] })
    const s = useExoticStore()
    await s.install('exotic-image-psd')
    expect(calls[0]).toEqual({ cmd: 'install_exotic_plugin', args: { pluginId: 'exotic-image-psd' } })
    // 安装后重载已装列表。
    expect(calls[1].cmd).toBe('list_installed_exotic_plugins')
    expect(s.installed.value).toHaveLength(1)
  })

  it('uninstall：透传 removeLicense', async () => {
    route({ uninstall_exotic_plugin: undefined, list_installed_exotic_plugins: [] })
    const s = useExoticStore()
    await s.uninstall('exotic-image-psd', true)
    expect(calls[0]).toEqual({
      cmd: 'uninstall_exotic_plugin',
      args: { pluginId: 'exotic-image-psd', removeLicense: true },
    })
  })

  it('install 失败 → 错误上抛供视图 toast', async () => {
    route({ install_exotic_plugin: new Error('install_failed') })
    const s = useExoticStore()
    await expect(s.install('p')).rejects.toBeTruthy()
  })

  it('refreshRegistry：返回摘要并重载本地列表', async () => {
    route({
      fetch_exotic_registry: { pluginCount: 3, sequence: 7, expired: false },
      list_exotic_registry: [regEntry(), regEntry({ pluginId: 'exotic-image-raw' })],
    })
    const s = useExoticStore()
    const summary = await s.refreshRegistry()
    expect(summary.pluginCount).toBe(3)
    expect(calls[0].cmd).toBe('fetch_exotic_registry')
    expect(calls[1].cmd).toBe('list_exotic_registry')
    expect(s.registry.value).toHaveLength(2)
  })
})

describe('useExoticStore 处理控制', () => {
  it('startProcessing：调命令并刷新进度', async () => {
    route({
      start_exotic_processing: undefined,
      get_exotic_processing_status: {
        pending: 0,
        processing: 1,
        done: 0,
        error: 0,
        blockedByAvailability: 0,
        running: true,
        paused: false,
      },
    })
    const s = useExoticStore()
    await s.startProcessing()
    expect(calls[0].cmd).toBe('start_exotic_processing')
    expect(s.status.value?.running).toBe(true)
  })

  it('retryFailures：以 pluginId 调命令并刷新进度', async () => {
    route({ retry_exotic_plugin_failures: undefined, get_exotic_processing_status: {} })
    const s = useExoticStore()
    await s.retryFailures('exotic-image-psd')
    expect(calls[0]).toEqual({
      cmd: 'retry_exotic_plugin_failures',
      args: { pluginId: 'exotic-image-psd' },
    })
  })
})
