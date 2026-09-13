// src/composables/reader/useReaderTypography.ts
// 排版全参数 + 变更处理（结构拆分自 DocumentViewer.vue §S4 + §S31 + §S33 对应 10 项设置的启动引导）。
//
// 红线（§3 风险 1）：readerTypography computed 的字段名/结构必须与 BookReader.vue 的 :typography
// prop 精确匹配（deep watch 消费），仅实变时才产新对象——不可在此边界改变 identity 语义。
// 红线（§3 风险 2）：verticalChanged 是**仅有的两条**需 capture-first 的 remount 路径之一，必须先
// captureCurrentPosition() 再 reloadToken++，顺序不可颠倒；该顺序由 root 注入的 requestRemount
// 统一实现（本文件不自写 capture+bump，只调用 deps.requestRemount(true)）。
import { ref, computed, type Ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { invokeIpc } from '../../utils/ipc'
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

  // 排版设置变更：更新 refs（→ readerTypography 重算 → BookReader 实时重排）+ 分别持久化为字符串。
  // remount 前用实时阅读位置刷新 initialPos(BookReader 重挂载据此从 :initial 恢复)。getCurrentLocation
  // 首次 relocate 前返回 null → 保持原 initialPos(开卷位置)。locator 形如 "cfi:...",与 restorePosition 同源。
  function onTypographyChange(v: TypographyChangePayload) {
    // 版式（R5）唯一需重挂载的项：foliate getDirection 仅在 section load 探测 writing-mode，
    // 改 prop 不会重探测已加载 section，故竖排↔横排切换须 remount 让 onLoad 重注 + getDirection 重探测。
    const verticalChanged = v.vertical !== readerVertical.value
    // 主题槽归一化(写库/比较均用归一化值,避免别名抖动导致假写入)。
    const nextThemeLight = normalizeReaderThemeId(v.readerThemeLight, 'light')
    const nextThemeDark = normalizeReaderThemeId(v.readerThemeDark, 'dark')
    // 逐键收集 (key,新值,旧值):旧值取当前 ref(须在下方落 ref 之前采集)。只写真正变化的键——否则
    // 每次步进点击都要 14 次 SET_APP_CONFIG(14 次 DB 写),连点即成倍冗余写(审查报告 §ASIDE)。
    const cfgWrites: { key: string; next: string; prev: string }[] = [
      {
        key: 'doc_reader_font_size',
        next: String(v.fontSizePx),
        prev: String(readerFontSize.value),
      },
      {
        key: 'doc_reader_line_height',
        next: String(v.lineHeight),
        prev: String(readerLineHeight.value),
      },
      { key: 'doc_reader_font_family', next: v.fontFamily, prev: readerFontFamily.value },
      {
        key: 'doc_reader_page_width',
        next: String(v.maxInlineSizePx),
        prev: String(readerPageWidth.value),
      },
      { key: 'doc_reader_page_turn', next: v.pageTurn, prev: readerPageTurn.value },
      {
        key: 'doc_reader_autoscroll_sec',
        next: String(v.autoScrollSec),
        prev: String(deps.readerAutoScrollSec.value),
      },
      { key: 'doc_reader_theme_light', next: nextThemeLight, prev: deps.readerThemeLight.value },
      { key: 'doc_reader_theme_dark', next: nextThemeDark, prev: deps.readerThemeDark.value },
      {
        key: 'doc_reader_font_weight',
        next: String(v.fontWeight),
        prev: String(readerFontWeight.value),
      },
      {
        key: 'doc_reader_letter_spacing',
        next: String(v.letterSpacingEm),
        prev: String(readerLetterSpacing.value),
      },
      { key: 'doc_reader_text_align', next: v.textAlign, prev: readerTextAlign.value },
      {
        key: 'doc_reader_title_scale',
        next: String(v.titleScale),
        prev: String(readerTitleScale.value),
      },
      {
        key: 'doc_reader_para_spacing',
        next: String(v.paragraphSpacingEm),
        prev: String(readerParagraphSpacing.value),
      },
      { key: 'doc_reader_vertical', next: v.vertical, prev: readerVertical.value },
    ]
    readerFontSize.value = v.fontSizePx
    readerLineHeight.value = v.lineHeight
    readerFontFamily.value = v.fontFamily
    readerPageWidth.value = v.maxInlineSizePx
    readerPageTurn.value = v.pageTurn
    deps.readerAutoScrollSec.value = v.autoScrollSec
    readerFontWeight.value = v.fontWeight
    readerLetterSpacing.value = v.letterSpacingEm
    readerTextAlign.value = v.textAlign
    readerTitleScale.value = v.titleScale
    readerParagraphSpacing.value = v.paragraphSpacingEm
    readerVertical.value = v.vertical
    // 阅读主题槽（R3）：归一化后落 ref（BookReader watch 实时重着色，不 remount）+ 持久化。
    deps.readerThemeLight.value = nextThemeLight
    deps.readerThemeDark.value = nextThemeDark
    // 只写真正变化的键(prev 已在函数顶部落 ref 之前采集),避免步进点击时 14 次冗余 IPC/DB 写。
    for (const w of cfgWrites) {
      if (w.next !== w.prev)
        invokeIpc(IPC.SET_APP_CONFIG, { key: w.key, value: w.next }).catch(() => {})
    }
    if (verticalChanged) {
      // remount 前用实时位置刷新 initialPos,否则 BookReader 重挂载会跳回开卷位置(竖排 canonical 不变,CFI 仍有效)。
      deps.requestRemount(true)
    } // 竖排↔横排:重挂载让 getDirection 重探测 writing-mode。
  }

  // 排版全参数（R3）启动引导：各带范围守卫，越界/非法忽略用默认。
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_font_size' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 12 && n <= 32) readerFontSize.value = Math.round(n)
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_line_height' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 1.2 && n <= 2.4) readerLineHeight.value = n
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_font_family' })
    .then((v) => {
      if (v === 'serif' || v === 'sans') readerFontFamily.value = v
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_page_width' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 480 && n <= 1040) readerPageWidth.value = Math.round(n)
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_page_turn' })
    .then((v) => {
      if (v === 'none' || v === 'slide' || v === 'curl') readerPageTurn.value = v
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_font_weight' })
    .then((v) => {
      const n = Number(v)
      if (n === 400 || n === 500 || n === 700) readerFontWeight.value = n
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_letter_spacing' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 0 && n <= 0.15) readerLetterSpacing.value = n
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_text_align' })
    .then((v) => {
      if (v === 'justify' || v === 'left') readerTextAlign.value = v
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_title_scale' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 0.8 && n <= 2) readerTitleScale.value = n
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_para_spacing' })
    .then((v) => {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 0 && n <= 2) readerParagraphSpacing.value = n
    })
    .catch(() => {})
  invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_vertical' })
    .then((v) => {
      if (v === 'off' || v === 'vertical-rl' || v === 'vertical-lr') readerVertical.value = v
    })
    .catch(() => {})

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
