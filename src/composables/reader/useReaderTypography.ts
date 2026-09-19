// src/composables/reader/useReaderTypography.ts
// 排版全参数 + 变更处理（结构拆分自 DocumentViewer.vue §S4 + §S31 + §S33 对应 10 项设置的启动引导）。
//
// 红线（§3 风险 1）：readerTypography computed 的字段名/结构必须与 BookReader.vue 的 :typography
// prop 精确匹配（deep watch 消费），仅实变时才产新对象——不可在此边界改变 identity 语义。
// 红线（§3 风险 2）：verticalChanged 是**仅有的两条**需 capture-first 的 remount 路径之一，必须先
// captureCurrentPosition() 再 reloadToken++，顺序不可颠倒；该顺序由 root 注入的 requestRemount
// 统一实现（本文件不自写 capture+bump，只调用 deps.requestRemount(true)）。
//
// 存储（设置集中保存，批次B）：14 个 doc_reader_* 键由后端 schema 注册。读入分两路——启动水合与
// 「恢复默认/外部编辑」到达都走 applySettings()（只应用，不回写）；用户改动经本文件显式批量提交，
// 且只提交真正变化的键（防步进连点时的冗余写）。
import { ref, computed, watch, type Ref } from 'vue'
import { readSetting, writeSettings } from '../../stores/settingsPersistence'
import { normalizeReaderThemeId } from '../../themes/readerThemes'

export interface TypographyChangePayload {
  fontSizePx: number
  lineHeight: number
  fontFamily: 'serif' | 'sans'
  maxInlineSizePx: number
  pageTurn: 'none' | 'slide' | 'curl'
  autoScrollSec: number
  readerThemeLight: string
  readerThemeDark: string
  fontWeight: number
  letterSpacingEm: number
  textAlign: 'justify' | 'left'
  titleScale: number
  paragraphSpacingEm: number
  vertical: 'off' | 'vertical-rl' | 'vertical-lr'
}

export interface UseReaderTypographyDeps {
  /** ReaderSettingsPanel 的变更事件同时携带自动翻页速率与主题槽（S5/S6，留根） */
  readerAutoScrollSec: Ref<number>
  readerThemeLight: Ref<string>
  readerThemeDark: Ref<string>
  /** 仅有的 capture+bump 实现在 root（红线：不可在此另写一份）。 */
  requestRemount: (captureFirst: boolean) => void
}

/** 排版参数设置键（与后端 schema 同名）。 */
const KEYS = {
  fontSize: 'doc_reader_font_size',
  lineHeight: 'doc_reader_line_height',
  fontFamily: 'doc_reader_font_family',
  pageWidth: 'doc_reader_page_width',
  pageTurn: 'doc_reader_page_turn',
  fontWeight: 'doc_reader_font_weight',
  letterSpacing: 'doc_reader_letter_spacing',
  textAlign: 'doc_reader_text_align',
  titleScale: 'doc_reader_title_scale',
  paraSpacing: 'doc_reader_para_spacing',
  vertical: 'doc_reader_vertical',
  autoScrollSec: 'doc_reader_autoscroll_sec',
  themeLight: 'doc_reader_theme_light',
  themeDark: 'doc_reader_theme_dark',
} as const

const FONT_FAMILIES = ['serif', 'sans'] as const
const PAGE_TURNS = ['none', 'slide', 'curl'] as const
const TEXT_ALIGNS = ['justify', 'left'] as const
const VERTICAL_MODES = ['off', 'vertical-rl', 'vertical-lr'] as const

/** 带范围守卫的数值读取：缺键/非法/越界一律 undefined（调用方保持当前值，不写回）。 */
function readNumberInRange(key: string, min: number, max: number, round = false) {
  const raw = readSetting(key)
  if (raw === undefined) return undefined
  const n = Number(raw)
  if (!Number.isFinite(n) || n < min || n > max) return undefined
  return round ? Math.round(n) : n
}

/** 枚举读取：缺键或不在候选集内一律 undefined。 */
function readChoice<T extends string>(key: string, allowed: readonly T[]): T | undefined {
  const raw = readSetting(key)
  return raw !== undefined && (allowed as readonly string[]).includes(raw) ? (raw as T) : undefined
}

export function useReaderTypography(deps: UseReaderTypographyDeps) {
  // 阅读排版参数（R2-6a/b，全局偏好）：字号 / 行距 / 字体族 / 栏宽。与 readerStyles 默认对齐。
  const readerFontSize = ref(19)
  const readerLineHeight = ref(1.75)
  const readerFontFamily = ref<'serif' | 'sans'>('serif')
  const readerPageWidth = ref(720)
  // 排版全参数（R3）：字重 / 字距(em) / 正文对齐 / 章题缩放。默认即 readerStyles 内置默认（零回归）。
  const readerFontWeight = ref(400)
  const readerLetterSpacing = ref(0)
  const readerTextAlign = ref<'justify' | 'left'>('justify')
  const readerTitleScale = ref(1)
  // 段距（R5-c）：段落间距 em，默认 0.4（比原 0.15 更明显，与 readerStyles 默认对齐）。
  const readerParagraphSpacing = ref(0.4)
  // 翻页动画（R6）：none 瞬切 / slide 滑动（默认）/ curl 仿真翻页。透传 BookReader :page-turn。
  const readerPageTurn = ref<'none' | 'slide' | 'curl'>('slide')
  // 版式（R5 竖排竹简）：off 横排 / vertical-rl 竖排右起 / vertical-lr 左起。切换须 remount（进 reloadToken）：
  // foliate getDirection 仅在 section load 探测 writing-mode，改 prop 不重探测已加载 section。
  const readerVertical = ref<'off' | 'vertical-rl' | 'vertical-lr'>('off')
  // 传给 BookReader 的 typography（computed → 仅实变时产新对象，配合 deep watch 精准重排）。
  const readerTypography = computed(() => ({
    fontSizePx: readerFontSize.value,
    lineHeight: readerLineHeight.value,
    fontFamily: readerFontFamily.value,
    maxInlineSizePx: readerPageWidth.value,
    fontWeight: readerFontWeight.value,
    letterSpacingEm: readerLetterSpacing.value,
    textAlign: readerTextAlign.value,
    titleScale: readerTitleScale.value,
    paragraphSpacingEm: readerParagraphSpacing.value,
  }))

  // 排版全参数（R3）读入：各带范围守卫，越界/非法/缺键保持当前值（默认）。
  // 启动水合与恢复默认/外部编辑都经此路径——只应用，不写回设置。
  // 第一次调用是启动同步：此时阅读器尚未按用户位置工作，不能也不必重挂载（会把开卷位置冲掉）；
  // 之后的调用来自权威快照变化（恢复默认 / 外部改文件），版式实变必须走与用户切换同一条
  // capture-first remount 路径，否则 foliate 的 getDirection 不会重探测 writing-mode。
  function applySettings() {
    const verticalBefore = readerVertical.value
    const fontSize = readNumberInRange(KEYS.fontSize, 12, 32, true)
    if (fontSize !== undefined) readerFontSize.value = fontSize
    const lineHeight = readNumberInRange(KEYS.lineHeight, 1.2, 2.4)
    if (lineHeight !== undefined) readerLineHeight.value = lineHeight
    const fontFamily = readChoice(KEYS.fontFamily, FONT_FAMILIES)
    if (fontFamily !== undefined) readerFontFamily.value = fontFamily
    const pageWidth = readNumberInRange(KEYS.pageWidth, 480, 1040, true)
    if (pageWidth !== undefined) readerPageWidth.value = pageWidth
    const pageTurn = readChoice(KEYS.pageTurn, PAGE_TURNS)
    if (pageTurn !== undefined) readerPageTurn.value = pageTurn
    const fontWeight = readNumberInRange(KEYS.fontWeight, 400, 700)
    if (fontWeight === 400 || fontWeight === 500 || fontWeight === 700)
      readerFontWeight.value = fontWeight
    const letterSpacing = readNumberInRange(KEYS.letterSpacing, 0, 0.15)
    if (letterSpacing !== undefined) readerLetterSpacing.value = letterSpacing
    const textAlign = readChoice(KEYS.textAlign, TEXT_ALIGNS)
    if (textAlign !== undefined) readerTextAlign.value = textAlign
    const titleScale = readNumberInRange(KEYS.titleScale, 0.8, 2)
    if (titleScale !== undefined) readerTitleScale.value = titleScale
    const paraSpacing = readNumberInRange(KEYS.paraSpacing, 0, 2)
    if (paraSpacing !== undefined) readerParagraphSpacing.value = paraSpacing
    const vertical = readChoice(KEYS.vertical, VERTICAL_MODES)
    if (vertical !== undefined) readerVertical.value = vertical
    return verticalBefore !== readerVertical.value
  }

  // 首次同步：只取值，不 remount。
  applySettings()
  // 后端只应用:任一排版键变化(启动水合 / 恢复默认 / 外部编辑)→ 重读,不触发保存。
  watch(
    () => [
      readSetting(KEYS.fontSize),
      readSetting(KEYS.lineHeight),
      readSetting(KEYS.fontFamily),
      readSetting(KEYS.pageWidth),
      readSetting(KEYS.pageTurn),
      readSetting(KEYS.fontWeight),
      readSetting(KEYS.letterSpacing),
      readSetting(KEYS.textAlign),
      readSetting(KEYS.titleScale),
      readSetting(KEYS.paraSpacing),
      readSetting(KEYS.vertical),
    ],
    () => {
      // 版式实变(重置 / 外部编辑改了竖排) → 与用户切换同一路径:先捕获阅读位置再重挂载。
      if (applySettings()) deps.requestRemount(true)
    },
  )

  // 自动翻页速率与阅读主题日夜槽也随权威快照水合（不再逐键 GET_APP_CONFIG）：
  // 缺键/非法/越界保持当前值；只应用，不写回。
  function applySharedSlots() {
    const autoScrollSec = readNumberInRange(KEYS.autoScrollSec, 2, 30, true)
    if (autoScrollSec !== undefined) deps.readerAutoScrollSec.value = autoScrollSec
    // 归一化挡跨槽误存/已卸载主题(回落 FOLLOW),不产生无色变量。
    deps.readerThemeLight.value = normalizeReaderThemeId(readSetting(KEYS.themeLight), 'light')
    deps.readerThemeDark.value = normalizeReaderThemeId(readSetting(KEYS.themeDark), 'dark')
  }

  applySharedSlots()
  watch(
    () => [
      readSetting(KEYS.autoScrollSec),
      readSetting(KEYS.themeLight),
      readSetting(KEYS.themeDark),
    ],
    applySharedSlots,
  )

  // 排版设置变更：更新 refs（→ readerTypography 重算 → BookReader 实时重排）+ 一次性批量持久化。
  // remount 前用实时阅读位置刷新 initialPos(BookReader 重挂载据此从 :initial 恢复)。getCurrentLocation
  // 首次 relocate 前返回 null → 保持原 initialPos(开卷位置)。locator 形如 "cfi:...",与 restorePosition 同源。
  function onTypographyChange(v: TypographyChangePayload) {
    // 版式（R5）唯一需重挂载的项：foliate getDirection 仅在 section load 探测 writing-mode，
    // 改 prop 不会重探测已加载 section，故竖排↔横排切换须 remount 让 onLoad 重注 + getDirection 重探测。
    const verticalChanged = v.vertical !== readerVertical.value
    // 主题槽归一化(写库/比较均用归一化值,避免别名抖动导致假写入)。
    const nextThemeLight = normalizeReaderThemeId(v.readerThemeLight, 'light')
    const nextThemeDark = normalizeReaderThemeId(v.readerThemeDark, 'dark')
    // Int 类键(字号 / 栏宽 / 自动翻页秒)在后端 schema 是整数:取整后再写、并同步到显示态。
    // 否则小数会被整批拒绝(一次 patch 里任一键非法即全批不落盘),而预览仍显示小数,与已保存值不符。
    const nextFontSize = Math.round(v.fontSizePx)
    const nextPageWidth = Math.round(v.maxInlineSizePx)
    const nextAutoScrollSec = Math.round(v.autoScrollSec)
    // 逐键收集 (key,新值,旧值):旧值取当前 ref(须在下方落 ref 之前采集)。只写真正变化的键——否则
    // 每次步进点击都要 14 次写盘,连点即成倍冗余写(审查报告 §ASIDE)。
    const cfgWrites: { key: string; next: string; prev: string }[] = [
      { key: KEYS.fontSize, next: String(nextFontSize), prev: String(readerFontSize.value) },
      { key: KEYS.lineHeight, next: String(v.lineHeight), prev: String(readerLineHeight.value) },
      { key: KEYS.fontFamily, next: v.fontFamily, prev: readerFontFamily.value },
      { key: KEYS.pageWidth, next: String(nextPageWidth), prev: String(readerPageWidth.value) },
      { key: KEYS.pageTurn, next: v.pageTurn, prev: readerPageTurn.value },
      {
        key: KEYS.autoScrollSec,
        next: String(nextAutoScrollSec),
        prev: String(deps.readerAutoScrollSec.value),
      },
      { key: KEYS.themeLight, next: nextThemeLight, prev: deps.readerThemeLight.value },
      { key: KEYS.themeDark, next: nextThemeDark, prev: deps.readerThemeDark.value },
      { key: KEYS.fontWeight, next: String(v.fontWeight), prev: String(readerFontWeight.value) },
      {
        key: KEYS.letterSpacing,
        next: String(v.letterSpacingEm),
        prev: String(readerLetterSpacing.value),
      },
      { key: KEYS.textAlign, next: v.textAlign, prev: readerTextAlign.value },
      { key: KEYS.titleScale, next: String(v.titleScale), prev: String(readerTitleScale.value) },
      {
        key: KEYS.paraSpacing,
        next: String(v.paragraphSpacingEm),
        prev: String(readerParagraphSpacing.value),
      },
      { key: KEYS.vertical, next: v.vertical, prev: readerVertical.value },
    ]
    readerFontSize.value = nextFontSize
    readerLineHeight.value = v.lineHeight
    readerFontFamily.value = v.fontFamily
    readerPageWidth.value = nextPageWidth
    readerPageTurn.value = v.pageTurn
    deps.readerAutoScrollSec.value = nextAutoScrollSec
    readerFontWeight.value = v.fontWeight
    readerLetterSpacing.value = v.letterSpacingEm
    readerTextAlign.value = v.textAlign
    readerTitleScale.value = v.titleScale
    readerParagraphSpacing.value = v.paragraphSpacingEm
    readerVertical.value = v.vertical
    // 阅读主题槽（R3）：归一化后落 ref（BookReader watch 实时重着色，不 remount）+ 持久化。
    deps.readerThemeLight.value = nextThemeLight
    deps.readerThemeDark.value = nextThemeDark
    // 只提交真正变化的键(prev 已在函数顶部落 ref 之前采集):一次批量提交,连点时由中央集合合并。
    // 写盘失败由中央服务提示;此处 catch 只为收掉 promise,不让 rejection 漏成 unhandled。
    const patch: Record<string, string> = {}
    for (const w of cfgWrites) {
      if (w.next !== w.prev) patch[w.key] = w.next
    }
    if (Object.keys(patch).length > 0)
      writeSettings(patch, { debounce: true }).catch(() => {})
    if (verticalChanged) {
      // remount 前用实时位置刷新 initialPos,否则 BookReader 重挂载会跳回开卷位置(竖排 canonical 不变,CFI 仍有效)。
      deps.requestRemount(true)
    } // 竖排↔横排:重挂载让 getDirection 重探测 writing-mode。
  }

  return {
    readerFontSize,
    readerLineHeight,
    readerFontFamily,
    readerPageWidth,
    readerFontWeight,
    readerLetterSpacing,
    readerTextAlign,
    readerTitleScale,
    readerParagraphSpacing,
    readerPageTurn,
    readerVertical,
    readerTypography,
    onTypographyChange,
  }
}
