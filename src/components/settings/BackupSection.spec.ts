import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, nextTick, reactive, ref, ssrContextKey, type App } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import { open } from '@tauri-apps/plugin-dialog'
import BackupSection from './BackupSection.vue'
import { useBackupStore, type BackupListEntry, type BackupPreflight } from '../../stores/backupStore'

const settings = reactive<Record<string, string>>({
  backup_dir: 'D:/offline-backups',
  backup_auto_enabled: 'true',
  backup_retention: '5',
})
const writeSettings = vi.fn(async (patch: Record<string, string>) => {
  Object.assign(settings, patch)
})
const { confirm } = vi.hoisted(() => ({ confirm: vi.fn() }))
vi.mock('../../stores/settingsPersistence', () => ({
  readSetting: (key: string) => settings[key],
  writeSettings: (patch: Record<string, string>) => writeSettings(patch),
}))
vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: ref('zh-CN') }),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('../../utils/format', () => ({ formatFileSize: (size: number) => String(size) }))
vi.mock('../../utils/ipc', () => ({
  invokeIpc: vi.fn(async () => null),
  IpcError: class extends Error {},
}))
vi.mock('../../composables/useConfirm', () => ({ useConfirm: () => ({ confirm }) }))
vi.mock('../../composables/useSettingsCards', () => ({
  useSettingsCards: () => ({ register: vi.fn(), unregister: vi.fn(), isOpen: () => true }),
}))
vi.mock('./RestoreWizard.vue', () => ({ default: { render: () => null } }))

interface SectionState {
  setAutoEnabled: (enabled: boolean) => Promise<void>
  chooseDestination: () => Promise<void>
  startManualBackup: () => Promise<void>
  starting: boolean
  destination: string
  preflight: BackupPreflight | null
  backups: BackupListEntry[]
  canEnableAuto: boolean
  listLoading: boolean
}
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
let app: App | null = null
beforeEach(() => {
  vi.clearAllMocks()
  Object.assign(settings, {
    backup_dir: 'D:/offline-backups', backup_auto_enabled: 'true', backup_retention: '5',
  })
})
afterEach(() => {
  app?.unmount()
  app = null
  vi.restoreAllMocks()
})

describe('BackupSection 自动备份失败恢复', () => {
  it('首次目录的自动备份确认期间切换目录，旧确认不得覆盖新目录策略', async () => {
    settings.backup_dir = ''
    settings.backup_auto_enabled = 'false'
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useBackupStore()
    vi.spyOn(store, 'preflightBackup').mockImplementation(async (dest) => ({
      destDir: dest!, writable: true, documentsConsistent: true, estimatedBytes: 100,
    }))
    vi.spyOn(store, 'listBackups').mockResolvedValue([])
    let finishConfirmation!: (result: { confirmed: boolean }) => void
    confirm.mockReturnValueOnce(new Promise((resolve) => { finishConfirmation = resolve }))
    vi.mocked(open).mockResolvedValueOnce('D:/first-backups')
    app = renderer.createApp({ ...BackupSection, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: SectionState }).setupState
    const choosing = state.chooseDestination()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(confirm).toHaveBeenCalledTimes(1)

    // 模拟中央设置收到外部配置更新；新目录已有独立的保留策略。
    Object.assign(settings, {
      backup_dir: 'E:/new-backups', backup_auto_enabled: 'false', backup_retention: '12',
    })
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(state.destination).toBe('E:/new-backups')
    expect(state.canEnableAuto).toBe(true)
    finishConfirmation({ confirmed: true })
    await choosing

    expect(writeSettings.mock.calls).toEqual([[{ backup_dir: 'D:/first-backups' }]])
    expect(settings.backup_auto_enabled).toBe('false')
    expect(settings.backup_retention).toBe('12')
  })

  it('首次目录未变化时确认每日备份，一次保存自动开关和保留份数', async () => {
    settings.backup_dir = ''
    settings.backup_auto_enabled = 'false'
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useBackupStore()
    vi.spyOn(store, 'preflightBackup').mockResolvedValue({
      destDir: 'D:/first-backups', writable: true, documentsConsistent: true, estimatedBytes: 100,
    })
    vi.spyOn(store, 'listBackups').mockResolvedValue([])
    confirm.mockResolvedValueOnce({ confirmed: true })
    vi.mocked(open).mockResolvedValueOnce('D:/first-backups')
    app = renderer.createApp({ ...BackupSection, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: SectionState }).setupState

    await state.chooseDestination()

    expect(writeSettings.mock.calls).toEqual([
      [{ backup_dir: 'D:/first-backups' }],
      [{ backup_retention: '5', backup_auto_enabled: 'true' }],
    ])
    expect(settings.backup_dir).toBe('D:/first-backups')
    expect(settings.backup_auto_enabled).toBe('true')
    expect(settings.backup_retention).toBe('5')
  })

  it('手动备份预检期间改选目录不会自动向新目录备份，重新点击才启动', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useBackupStore()
    const validPreflight: BackupPreflight = {
      destDir: 'D:/offline-backups', writable: true, documentsConsistent: true, estimatedBytes: 100,
    }
    const preflight = vi.spyOn(store, 'preflightBackup').mockResolvedValue(validPreflight)
    vi.spyOn(store, 'listBackups').mockResolvedValue([])
    const startBackup = vi.spyOn(store, 'startBackup').mockResolvedValue(undefined)
    app = renderer.createApp({ ...BackupSection, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: SectionState }).setupState
    await new Promise<void>((resolve) => setImmediate(resolve))

    let finishStartPreflight!: (value: BackupPreflight) => void
    preflight.mockReturnValueOnce(new Promise((resolve) => { finishStartPreflight = resolve }))
    const starting = state.startManualBackup()
    expect(preflight).toHaveBeenLastCalledWith('D:/offline-backups')
    expect(state.starting).toBe(true)

    const newPreflight = { ...validPreflight, destDir: 'E:/new-backups' }
    preflight.mockResolvedValue(newPreflight)
    vi.mocked(open).mockResolvedValueOnce('E:/new-backups')
    await state.chooseDestination()
    expect(state.destination).toBe('E:/new-backups')
    expect(state.preflight).toEqual(newPreflight)

    finishStartPreflight(validPreflight)
    await starting
    expect(startBackup).not.toHaveBeenCalled()
    expect(state.starting).toBe(false)

    await state.startManualBackup()
    expect(startBackup).toHaveBeenCalledTimes(1)
    expect(startBackup).toHaveBeenCalledWith('E:/new-backups')
    expect(state.starting).toBe(false)
  })

  it('目的地不可写时仍可关闭已开启的自动备份，关闭后禁止重新开启', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useBackupStore()
    const preflight = vi.spyOn(store, 'preflightBackup').mockResolvedValue({
      destDir: settings.backup_dir!, writable: false, documentsConsistent: true, estimatedBytes: 100,
    })
    vi.spyOn(store, 'listBackups').mockResolvedValue([])
    app = renderer.createApp({ ...BackupSection, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    app.config.globalProperties.$t = (key: string) => key
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: SectionState }).setupState
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(preflight).toHaveBeenCalledWith(settings.backup_dir)
    // 保留真实 BackupSection 模板与 UiToggle，检查原生 input 的交互门控。
    async function renderToggle() {
      const projection = createSSRApp({ ...BackupSection, setup: () => state })
      projection.config.globalProperties.$t = (key: string) => key
      const html = await renderToString(projection)
      const toggle = html.match(/<input[^>]*aria-label="backup.automaticTitle"[^>]*>/)?.[0]
      expect(toggle).toBeDefined()
      return toggle!
    }
    const enabledToggle = await renderToggle()
    expect(enabledToggle).toContain('checked')
    expect(enabledToggle).not.toContain('disabled')
    await state.setAutoEnabled(false)
    await nextTick()

    expect(writeSettings).toHaveBeenCalledWith({ backup_auto_enabled: 'false' })
    const disabledToggle = await renderToggle()
    expect(disabledToggle).not.toContain('checked')
    expect(disabledToggle).toContain('disabled')
  })

  it('改选目的地会立即刷新新目录，旧目录迟到结果不得覆盖可写性和备份列表', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useBackupStore()
    let finishOldPreflight!: (value: BackupPreflight) => void
    let finishNewPreflight!: (value: BackupPreflight) => void
    let finishOldList!: (value: BackupListEntry[]) => void
    const oldPreflight = new Promise<BackupPreflight>((resolve) => { finishOldPreflight = resolve })
    const newPreflight = new Promise<BackupPreflight>((resolve) => { finishNewPreflight = resolve })
    const oldList = new Promise<BackupListEntry[]>((resolve) => { finishOldList = resolve })
    const newEntry: BackupListEntry = {
      path: 'E:/new-backups/new.scrollerybackup', fileName: 'new.scrollerybackup', kind: 'backup',
      createdAtUtc: '2026-10-04T00:00:00Z', backupId: 'new-backup', bytes: 100,
    }
    const preflight = vi.spyOn(store, 'preflightBackup')
      .mockReturnValueOnce(oldPreflight).mockReturnValueOnce(newPreflight)
    const list = vi.spyOn(store, 'listBackups')
      .mockReturnValueOnce(oldList).mockResolvedValueOnce([newEntry])
    vi.mocked(open).mockResolvedValueOnce('E:/new-backups')
    app = renderer.createApp({ ...BackupSection, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: SectionState }).setupState
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(list).toHaveBeenCalledWith('D:/offline-backups')

    const changeDestination = state.chooseDestination()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(state.destination).toBe('E:/new-backups')
    expect(preflight).toHaveBeenLastCalledWith('E:/new-backups')
    expect(list).toHaveBeenLastCalledWith('E:/new-backups')
    expect(state.preflight).toBeNull()
    expect(state.canEnableAuto).toBe(false)
    const newResult: BackupPreflight = {
      destDir: 'E:/new-backups', writable: false, documentsConsistent: true, estimatedBytes: 200,
    }
    finishNewPreflight(newResult)
    await changeDestination
    expect(state.backups).toEqual([newEntry])
    expect(state.preflight).toEqual(newResult)
    expect(state.listLoading).toBe(false)

    finishOldPreflight({ ...newResult, destDir: 'D:/offline-backups', writable: true })
    finishOldList([{ ...newEntry, path: 'D:/offline-backups/old.scrollerybackup', backupId: 'old' }])
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(state.preflight).toEqual(newResult)
    expect(state.canEnableAuto).toBe(false)
    expect(state.backups).toEqual([newEntry])
    expect(state.listLoading).toBe(false)
  })
})
