// src/stores/themeStore.ts
// 主题运行时(方案 docs/designs/2026-09-16-主题配色全新设计.md §6/§7):配置适配 + 临时草稿 +
// 有效模式 + 当前色板,是 DOM 与 Canvas 的唯一色板来源。
//
// 三条职责边界:
//  - 读一律经中央设置快照,写一律经 writeSettings 的批量 patch;本 store 不自持持久化,
//    也不把草稿写进配置(草稿是内存态,只有「应用/保存」才落盘,且一次 patch 原子提交)。
//  - 发布(生成色板 + 写 CSS 变量/材质)最多每动画帧一次,不写盘、不触发资产与布局;
//    首帧缓存只由已确认值驱动,草稿与未确认写入永不进缓存。
//  - 草稿与「已应用」是两层:草稿可见时呈现草稿;应用失败回退到已确认呈现,草稿保留供重试。

import { computed, ref, shallowRef, watch } from 'vue'
import { defineStore } from 'pinia'
import { getAppWindow } from '../utils/appWindow'
import { isMac, isWindows } from '../utils/platform'
import { logger } from '../utils/logger'
import type { AppearanceMode } from '../types/ui'
import { readSettingEnum } from '../composables/settingsValues'
import {
  settingsConfirmedValues,
  settingsGeneration,
  settingsReady,
  settingsValues,
  writeSettings,
} from './settingsPersistence'
import { generateTheme } from '../themes/generate'
import { applyMaterial, applyPalette } from '../themes/apply'
import { buildThemeCache, writeThemeCache } from '../themes/snapshot'
import { DEFAULT_THEME_DEFINITION } from '../themes/presets'
// 主题配置的键名与结构转换集中在 themes/config.ts(config.toml 结构的唯一转写点),本 store 只做
// 「设置快照 → 具名结构 → 批量 patch」的编排,不自行拼 TOML/JSON 结构、不重复校验字段。
import {
  THEME_SETTING_KEYS,
  parseOpacity,
  parseSavedThemesText,
  parseThemeDefinition,
  serializeSavedThemes,
  toThemeDefinitionPatch,
} from '../themes/config'
import type {
  SavedTheme,
  ThemeDefinition,
  ThemeMaterial,
  ThemeMode,
  ThemePalette,
  ThemeSeed,
} from '../themes/types'

const APPEARANCE_MODES: readonly AppearanceMode[] = ['light', 'dark', 'system']

/** 命名操作的失败原因:空名/重名由本 store 在写入前判定,目标不存在由 ID 定位判定。 */
export type ThemeNameError = 'emptyName' | 'duplicateName' | 'notFound'

export type ThemeSaveResult =
  | { ok: true; theme: SavedTheme }
  | { ok: false; reason: ThemeNameError }

/** 草稿被外部原因丢弃的信号:C 用它给一次提示(草稿内容已不在,store 不自己造文案)。 */
export interface DraftInvalidation {
  /** external = 配置热更新/跨窗口同步改变了主题参数或材质;reset = 恢复默认设置。 */
  reason: 'external' | 'reset'
  at: number
}

/** 已删除的个人主题 + 原位置,供一次撤销复原(顺序不因撤销而漂移)。 */
export interface DeletedTheme {
  theme: SavedTheme
  index: number
}

function cloneDefinition(definition: ThemeDefinition): ThemeDefinition {
  return {
    light: { ...definition.light },
    dark: { ...definition.dark },
    material: definition.material,
    opacity: definition.opacity,
  }
}

/** 设置快照文本 → 具名主题参数(结构转换与字段校验都在 config.ts,这里只做键名映射)。 */
function definitionFrom(values: Record<string, string>): ThemeDefinition {
  return parseThemeDefinition({
    lightPalette: values[THEME_SETTING_KEYS.lightPalette] ?? null,
    darkPalette: values[THEME_SETTING_KEYS.darkPalette] ?? null,
    windowMaterial: values[THEME_SETTING_KEYS.windowMaterial] ?? null,
    windowOpacity: values[THEME_SETTING_KEYS.windowOpacity] ?? null,
  })
}

/** 设置快照文本 → 命名主题列表(非法条目与重复 id 由 config.ts 收敛)。 */
function savedThemesFrom(values: Record<string, string>): SavedTheme[] {
  return parseSavedThemesText(values[THEME_SETTING_KEYS.savedThemes] ?? null)
}

/** 设置快照文本 → 外观偏好(枚举白名单与默认值口径同 readSettingEnum,但读的是**确认**文本)。 */
function appearanceFrom(values: Record<string, string>): AppearanceMode {
  const raw = values[THEME_SETTING_KEYS.appearance]
  return raw !== undefined && (APPEARANCE_MODES as readonly string[]).includes(raw)
    ? (raw as AppearanceMode)
    : 'system'
}

/**
 * 主题参数签名:**只**覆盖两套配色与材质/不透明度。
 *
 * 「我的主题」列表变化不算主题变化——重命名/删除个人主题不得打断正在进行的草稿。
 */
function signatureOf(definition: ThemeDefinition): string {
  return JSON.stringify([definition.light, definition.dark, definition.material, definition.opacity])
}

/** 稳定 ID:crypto.randomUUID 不可用时退回时间戳 + 计数(仍需唯一且不含颜色/名称信息)。 */
let themeIdSeq = 0
function newThemeId(): string {
  const cryptoApi = globalThis.crypto
  if (cryptoApi && typeof cryptoApi.randomUUID === 'function') return cryptoApi.randomUUID()
  themeIdSeq += 1
  return `theme-${Date.now().toString(36)}-${themeIdSeq}`
}

/**
 * 发布节流:有 requestAnimationFrame 时按帧合流(拖动最多每帧一次),无 rAF 的环境退化到宏任务。
 * 只影响发布时机,不影响正确性——最终总会发布最后一份色板。
 */
function scheduleFrame(callback: () => void): void {
  if (typeof requestAnimationFrame === 'function') {
    requestAnimationFrame(() => callback())
    return
  }
  setTimeout(callback, 0)
}

export const useThemeStore = defineStore('theme', () => {
  // ── 外观偏好(独立于主题:草稿与预设都不得改动它)───────────────────────────
  const appearance = computed<AppearanceMode>(() =>
    readSettingEnum(THEME_SETTING_KEYS.appearance, APPEARANCE_MODES, 'system'),
  )
  const systemIsDark = ref(window.matchMedia('(prefers-color-scheme: dark)').matches)
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (event) => {
    // 只有跟随系统时有效模式才会变;themeKey 的 watch 会据此重新发布。
    systemIsDark.value = event.matches
  })
  const appearanceIsDark = computed(() =>
    appearance.value === 'system' ? systemIsDark.value : appearance.value === 'dark',
  )

  // ── 草稿(内存态,不落盘)─────────────────────────────────────────────────
  const draft = ref<ThemeDefinition | null>(null)
  /** 草稿正在呈现的配色模式;null = 未在呈现草稿(回到偏好模式或应用失败后的回滚态)。 */
  const previewMode = ref<ThemeMode | null>(null)
  const draftInvalidation = shallowRef<DraftInvalidation | null>(null)
  /** 进入编辑时的参数快照:draftDirty 与「取消回到哪」都以它为基准。 */
  let draftBaseline: ThemeDefinition | null = null

  const isEditing = computed(() => draft.value !== null)
  const isPreviewing = computed(() => draft.value !== null && previewMode.value !== null)

  // ── 「我的主题」列表与删除撤销(列表变化不算主题变化,不打断草稿)──────────────
  const savedThemes = computed<SavedTheme[]>(() => savedThemesFrom(settingsValues.value))
  const lastDeletedTheme = shallowRef<DeletedTheme | null>(null)

  const isDark = computed(() =>
    previewMode.value !== null ? previewMode.value === 'dark' : appearanceIsDark.value,
  )
  const effectiveMode = computed<ThemeMode>(() => (isDark.value ? 'dark' : 'light'))

  // ── 已应用参数(settingsValues:含同代次未确认预览,应用后立刻跟手)──────────
  const appliedDefinition = computed<ThemeDefinition>(() => definitionFrom(settingsValues.value))
  /**
   * 正在呈现的完整参数(C 侧约定的 currentDefinition):草稿可见时是草稿,否则是已应用值。
   * 只返回参数本身,不做颜色生成——色板由下面的 currentPalette 按帧发布。
   */
  const currentDefinition = computed<ThemeDefinition>(() =>
    isPreviewing.value && draft.value !== null ? draft.value : appliedDefinition.value,
  )
  const draftDirty = computed(
    () => draft.value !== null && JSON.stringify(draft.value) !== JSON.stringify(draftBaseline),
  )

  // ── 发布:一帧内生成色板并写 CSS 变量与材质(最多每帧一次,不写盘)──────────────
  // 色板与材质是**发布产物**,不是读取时计算的派生值:组件与 Canvas 每次事件读 currentPalette
  // 只是取值,不会各自触发 generateTheme,故 DOM 与 Canvas 必定拿到同一帧的同一份结果;
  // 「最多每帧一次」也由这一处保证(拖动只排队,不逐事件重算)。
  const currentPalette = shallowRef<ThemePalette>(
    generateTheme(DEFAULT_THEME_DEFINITION[effectiveMode.value], effectiveMode.value),
  )
  const currentMaterial = shallowRef<ThemeMaterial>(DEFAULT_THEME_DEFINITION.material)
  const currentOpacity = shallowRef(DEFAULT_THEME_DEFINITION.opacity)

  function publishNow(): void {
    const definition = currentDefinition.value
    const mode = effectiveMode.value
    const palette = generateTheme(definition[mode], mode)
    currentPalette.value = palette
    currentMaterial.value = definition.material
    currentOpacity.value = definition.opacity
    applyPalette(palette)
    applyMaterial(definition.material, definition.opacity, { windows: isWindows })
  }

  let framePending = false
  function schedulePublish(): void {
    if (framePending) return
    framePending = true
    scheduleFrame(() => {
      framePending = false
      // 水合前 DOM 属于首帧脚本(缓存变量或注入的默认主题 CSS):本 store 不抢写,免得把自定义
      // 配色的缓存刷成默认色、随后又被水合刷回(首帧闪)。编辑必然晚于水合,有草稿即照常发布。
      if (!settingsReady.value && draft.value === null) return
      publishNow()
    })
  }

  // 主题参数或有效模式的**值**变化才发布:无关设置(语言、侧栏宽度…)不改这个键,不触发生成。
  const themeKey = computed(
    () => `${effectiveMode.value}|${JSON.stringify(currentDefinition.value)}`,
  )
  // 快照就绪也要发布一次:若磁盘上已是出厂默认(明暗参数与 store 占位默认逐字段相同)而本地旧缓存
  // 仍是上一份自定义配色,themeKey 不会变——只看它就没有任何一次机会把首帧的旧缓存校正过来。
  watch([themeKey, settingsReady], schedulePublish)

  /**
   * 原生窗口明暗同步(自 uiStore 原行为迁入):mac 保留原生红绿灯与原生弹层,须与**呈现**一致,
   * 否则预览深色配色时红绿灯区域仍是浅色。system 且未在预览时传 null 跟随系统(与旧命令同义)。
   * Windows 自绘标题栏无原生 caption,不需要;非 mac 平台直接跳过。
   */
  function syncNativeWindowTheme(): void {
    if (!isMac) return
    const mode =
      appearance.value === 'system' && previewMode.value === null ? null : effectiveMode.value
    getAppWindow()
      .setTheme(mode)
      .catch((error) => logger.warn('[themeStore] setTheme failed', { error }))
  }
  watch([effectiveMode, appearance, previewMode], syncNativeWindowTheme)

  // 首帧着色属于 index.html 的阻塞脚本(缓存变量或注入的默认主题 CSS):那时本 store 还不存在,
  // 也拿不到「生成的变量」(缓存里只有生成结果,没有种子)。故此处**不**抢先发布默认色板——
  // 那会在自定义配色的用户机上把首帧刷回默认色。仅当快照已就绪(本 store 晚于水合创建)时
  // 立即对齐一次;否则等水合带来的值变化经上面同一条发布路径生效。
  if (settingsReady.value) {
    publishNow()
    syncNativeWindowTheme()
  }

  // ── 首帧缓存:只认**已确认**值 ───────────────────────────────────────────
  // appearance 也必须取确认值:未确认的乐观外观一旦进缓存,下次启动的首帧就会用错的明暗
  // (而它自己还没被后端接受)。
  const confirmedAppearance = computed(() => appearanceFrom(settingsConfirmedValues.value))
  const confirmedThemeSignature = computed(() =>
    signatureOf(definitionFrom(settingsConfirmedValues.value)),
  )
  let cachedSignature: string | null = null
  let cachedGeneration = -1
  /** 是否已写过缓存:首次就绪必须写一次,不能因为「签名与占位默认相同」而跳过。 */
  let cacheWritten = false

  function refreshThemeCache(): void {
    // 未就绪时 confirmed 为空(等于默认值),写下去等于把默认值当成用户偏好缓存到下次启动。
    if (!settingsReady.value) return
    const definition = definitionFrom(settingsConfirmedValues.value)
    writeThemeCache(
      buildThemeCache(
        confirmedAppearance.value,
        generateTheme(definition.light, 'light'),
        generateTheme(definition.dark, 'dark'),
        definition.material,
        definition.opacity,
      ),
    )
  }

  watch(
    [confirmedThemeSignature, confirmedAppearance, settingsGeneration, settingsReady],
    () => {
      if (!settingsReady.value) return
      const signature = `${confirmedAppearance.value}|${confirmedThemeSignature.value}`
      const generation = settingsGeneration.value
      // 代次推进与**首次就绪**都必须重写缓存:重置后的参数可能恰好与之前相同,首次就绪时也可能与
      // store 占位默认相同(磁盘已恢复默认而旧缓存仍是自定义配色)。两种情况下跳过重写,都会把
      // 上一份偏好的颜色留给下次启动的首帧。
      if (cacheWritten && signature === cachedSignature && generation === cachedGeneration) return
      cachedSignature = signature
      cachedGeneration = generation
      cacheWritten = true
      refreshThemeCache()
    },
  )

  // ── 草稿失效:外部改主题参数/材质或重置时丢弃草稿并留下提示信号 ─────────────
  let lastConfirmedSignature = confirmedThemeSignature.value
  /** 本窗口自己发起的主题提交签名:回执到达时据此区分「自己的写入」与「外部变更」。 */
  let selfCommitSignature: string | null = null

  function dropDraft(reason: DraftInvalidation['reason']): void {
    draft.value = null
    previewMode.value = null
    draftBaseline = null
    draftInvalidation.value = { reason, at: Date.now() }
    schedulePublish()
  }

  watch(confirmedThemeSignature, (signature) => {
    if (signature === lastConfirmedSignature) return // 无关设置变化不打断草稿
    lastConfirmedSignature = signature
    if (selfCommitSignature !== null && signature === selfCommitSignature) return
    if (draft.value !== null) dropDraft('external')
  })

  watch(settingsGeneration, () => {
    // 全局重置会整份替换配置(含个人主题库),撤销入口随之失效。
    lastDeletedTheme.value = null
    if (draft.value !== null) dropDraft('reset')
  })

  // ── 草稿会话 ──────────────────────────────────────────────────────────────
  function startDraft(definition: ThemeDefinition, mode?: ThemeMode): void {
    draft.value = cloneDefinition(definition)
    draftBaseline = cloneDefinition(definitionFrom(settingsValues.value))
    previewMode.value = mode ?? (appearanceIsDark.value ? 'dark' : 'light')
    draftInvalidation.value = null
  }

  /**
   * 进入编辑:初值取**当前有效值**(settingsValues,含已保存与待提交值),而非仅已确认值
   * ——用户看到什么就从什么开始调,不能让未确认的预览在进编辑时被悄悄退回。
   */
  function beginEdit(mode?: ThemeMode): void {
    startDraft(definitionFrom(settingsValues.value), mode)
  }

  /** 草稿存在而预览被关掉(应用失败回滚后)时,任何一次编辑动作都重新开启预览。 */
  function ensurePreview(): void {
    if (draft.value === null || previewMode.value !== null) return
    previewMode.value = appearanceIsDark.value ? 'dark' : 'light'
  }

  function ensureDraft(): ThemeDefinition {
    if (draft.value === null) startDraft(definitionFrom(settingsValues.value))
    return draft.value as ThemeDefinition
  }

  function updateSeed(mode: ThemeMode, patch: Partial<ThemeSeed>): void {
    const current = ensureDraft()
    ensurePreview()
    draft.value = { ...current, [mode]: { ...current[mode], ...patch } }
    schedulePublish()
  }

  function updateMaterial(material: ThemeMaterial): void {
    const current = ensureDraft()
    ensurePreview()
    draft.value = { ...current, material }
    schedulePublish()
  }

  function updateOpacity(pct: number): void {
    const current = ensureDraft()
    ensurePreview()
    draft.value = { ...current, opacity: parseOpacity(pct) }
    schedulePublish()
  }

  /** 预设/我的主题 → 复制进草稿并预览;已保存配色不被静默覆盖(要落盘须走应用或保存)。 */
  function loadDefinition(definition: ThemeDefinition): void {
    if (draft.value === null) {
      startDraft(definition)
      return
    }
    draft.value = cloneDefinition(definition)
    ensurePreview()
    schedulePublish()
  }

  function loadSavedTheme(id: string): void {
    const theme = savedThemes.value.find((entry) => entry.id === id)
    if (theme) loadDefinition(theme)
  }

  /** 恢复该模式的权威默认配色:只换这一套种子,不动另一套配色、材质与「我的主题」。 */
  function resetSeedToDefault(mode: ThemeMode): void {
    updateSeed(mode, DEFAULT_THEME_DEFINITION[mode])
  }

  function setPreviewMode(mode: ThemeMode | null): void {
    if (draft.value === null) {
      previewMode.value = null
      return
    }
    previewMode.value = mode
    schedulePublish()
  }

  function finishDraft(): void {
    draft.value = null
    previewMode.value = null
    draftBaseline = null
  }

  function cancelEdit(): void {
    if (draft.value === null && previewMode.value === null) return
    finishDraft()
    schedulePublish() // 呈现回到已应用值
  }

  /** 主题参数的一次批量 patch(一次请求、一次写盘;两套配色与材质同批,不留中间态)。 */
  function definitionPatch(definition: ThemeDefinition): Record<string, string> {
    return toThemeDefinitionPatch(definition)
  }

  /**
   * 应用草稿:一次批量提交两套配色与材质。
   *
   * 失败时中央服务已把显示值回滚到最后确认快照;此处额外停掉预览,使**已应用视图**回到确认
   * 值,草稿仍在编辑器里(下一次编辑动作会自动恢复预览),可原样重试。
   */
  async function applyTheme(): Promise<void> {
    if (draft.value === null) return
    const definition = cloneDefinition(draft.value)
    selfCommitSignature = signatureOf(definition)
    try {
      await writeSettings(definitionPatch(definition))
    } catch (error) {
      // 提交根本没落盘:清掉回执守卫,免得后面一次外部变更被误当成自己的回执吞掉。
      selfCommitSignature = null
      previewMode.value = null
      schedulePublish()
      throw error
    }
    finishDraft()
  }

  // ── 我的主题(全部按稳定 ID 定位,一次 patch 原子保存)────────────────────
  function validateName(name: string, exceptId?: string): ThemeNameError | null {
    const trimmed = name.trim()
    if (!trimmed) return 'emptyName'
    // 去首尾空白后与已有个人主题一致即拒绝:绝不按名称自动覆盖(内置主题名称不参与比对)。
    if (savedThemes.value.some((entry) => entry.name === trimmed && entry.id !== exceptId)) {
      return 'duplicateName'
    }
    return null
  }

  function listPatch(list: SavedTheme[], definition?: ThemeDefinition): Record<string, string> {
    const patch: Record<string, string> = {
      [THEME_SETTING_KEYS.savedThemes]: serializeSavedThemes(list),
    }
    if (definition) Object.assign(patch, definitionPatch(definition))
    return patch
  }

  /**
   * 保存并应用:当前完整参数(草稿优先)与新条目在**同一个 patch** 内提交,不存在只存一半的结果;
   * 持久化成功后才算保存成功(失败 reject,草稿保留可重试)。
   */
  async function createSavedTheme(name: string): Promise<ThemeSaveResult> {
    const invalid = validateName(name)
    if (invalid) return { ok: false, reason: invalid }
    // 草稿优先;未进编辑时即已应用值(此时参数已在配置里,不再重复写一遍)。
    const definition = cloneDefinition(draft.value ?? appliedDefinition.value)
    const theme: SavedTheme = { id: newThemeId(), name: name.trim(), ...definition }
    // 未进入编辑时参数已是已应用值,不再重复写一遍参数。
    const definitionToCommit = draft.value !== null ? definition : undefined
    if (definitionToCommit) selfCommitSignature = signatureOf(definitionToCommit)
    try {
      await writeSettings(listPatch([...savedThemes.value, theme], definitionToCommit))
    } catch (error) {
      selfCommitSignature = null
      throw error
    }
    if (draft.value !== null) finishDraft()
    return { ok: true, theme }
  }

  /** 更新已有个人主题:保持 ID 与名称,替换参数;带草稿时同批把参数应用为当前配色。 */
  async function updateSavedTheme(
    id: string,
    params?: ThemeDefinition,
  ): Promise<ThemeSaveResult> {
    const index = savedThemes.value.findIndex((entry) => entry.id === id)
    if (index < 0) return { ok: false, reason: 'notFound' }
    const definition = cloneDefinition(params ?? draft.value ?? appliedDefinition.value)
    const target = savedThemes.value[index]
    const theme: SavedTheme = { ...target, ...definition }
    const next = [...savedThemes.value]
    next[index] = theme
    const definitionToCommit = draft.value !== null || params !== undefined ? definition : undefined
    if (definitionToCommit) selfCommitSignature = signatureOf(definitionToCommit)
    try {
      await writeSettings(listPatch(next, definitionToCommit))
    } catch (error) {
      selfCommitSignature = null
      throw error
    }
    if (draft.value !== null) finishDraft()
    return { ok: true, theme }
  }

  /** 重命名:只改名称,不动 ID 与配色,不改变正在使用的颜色。 */
  async function renameSavedTheme(id: string, name: string): Promise<ThemeSaveResult> {
    const index = savedThemes.value.findIndex((entry) => entry.id === id)
    if (index < 0) return { ok: false, reason: 'notFound' }
    const invalid = validateName(name, id)
    if (invalid) return { ok: false, reason: invalid }
    const theme: SavedTheme = { ...savedThemes.value[index], name: name.trim() }
    const next = [...savedThemes.value]
    next[index] = theme
    await writeSettings(listPatch(next))
    return { ok: true, theme }
  }

  /** 删除:只删已保存条目,不改变正在使用的配色;成功后留下一次撤销入口。 */
  async function deleteSavedTheme(id: string): Promise<void> {
    const index = savedThemes.value.findIndex((entry) => entry.id === id)
    if (index < 0) return
    const removed = savedThemes.value[index]
    const next = savedThemes.value.filter((entry) => entry.id !== id)
    await writeSettings(listPatch(next))
    lastDeletedTheme.value = { theme: removed, index }
  }

  async function undoDeleteSavedTheme(): Promise<void> {
    const deleted = lastDeletedTheme.value
    if (!deleted) return
    if (savedThemes.value.some((entry) => entry.id === deleted.theme.id)) {
      lastDeletedTheme.value = null
      return
    }
    const next = [...savedThemes.value]
    next.splice(Math.min(deleted.index, next.length), 0, deleted.theme)
    await writeSettings(listPatch(next))
    lastDeletedTheme.value = null
  }

  // ── 外观偏好的提交(独立于草稿;预览永不写它)─────────────────────────────
  function setAppearance(mode: AppearanceMode): void {
    writeSettings({ [THEME_SETTING_KEYS.appearance]: mode }).catch(() => {})
  }

  function cycleAppearance(): void {
    const order: AppearanceMode[] = ['light', 'dark', 'system']
    setAppearance(order[(order.indexOf(appearance.value) + 1) % order.length])
  }

  return {
    // 外观偏好
    appearance,
    appearanceIsDark,
    isDark,
    effectiveMode,
    setAppearance,
    cycleAppearance,
    // 当前主题(呈现)
    appliedDefinition,
    currentDefinition,
    currentPalette,
    currentMaterial,
    currentOpacity,
    // 草稿
    draft,
    isEditing,
    isPreviewing,
    previewMode,
    draftDirty,
    draftInvalidation,
    beginEdit,
    updateSeed,
    updateMaterial,
    updateOpacity,
    loadDefinition,
    loadSavedTheme,
    resetSeedToDefault,
    setPreviewMode,
    applyTheme,
    cancelEdit,
    // 我的主题
    savedThemes,
    lastDeletedTheme,
    createSavedTheme,
    updateSavedTheme,
    renameSavedTheme,
    deleteSavedTheme,
    undoDeleteSavedTheme,
  }
})
