<template>
  <div class="rs-panel">
    <div class="rs-panel__head">
      <span class="rs-panel__title">{{ t('doc.readerSettings') }}</span>
      <button
        class="rs-panel__x"
        @click="emit('close')"
        :title="t('common.close')"

      >
        <X :size="16" />
      </button>
    </div>

    <div class="rs-panel__body">
      <ReaderSettingsGroup id="theme" :title="t('doc.readerGroupTheme')">
        <div class="rs-group-rows">
          <!-- 阅读主题（R3）：跟随应用 + 当前明暗槽可选的阅读专属调色板。编辑的是当前 app 明暗对应的那一槽
           （日/夜两槽的完整配置在设置页「阅读」节）。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.readerTheme') }}</span>
            <select
              class="rs-select"
              :value="currentThemePick"
              @change="setReaderTheme(($event.target as HTMLSelectElement).value)"
            >
              <option :value="READER_THEME_FOLLOW">{{ t('doc.followApp') }}</option>
              <option v-for="rt in kindThemes" :key="rt.id" :value="rt.id">
                {{ t(rt.nameKey) }}
              </option>
            </select>
          </div>
        </div>
      </ReaderSettingsGroup>

      <ReaderSettingsGroup id="typography" :title="t('doc.readerGroupTypography')">
        <div class="rs-group-rows">
          <!-- 字号：−/+ 步进 1px，钳制 [FONT_MIN, FONT_MAX]。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.fontSize') }}</span>
            <div class="rs-stepper">
              <button
                @click="setFontSize(fontSizePx - 1)"
                :disabled="fontSizePx <= FONT_MIN"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ fontSizePx }}px</span>
              <button
                @click="setFontSize(fontSizePx + 1)"
                :disabled="fontSizePx >= FONT_MAX"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 行距：−/+ 步进 0.05，钳制 [LH_MIN, LH_MAX]，显示两位小数。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.lineHeight') }}</span>
            <div class="rs-stepper">
              <button
                @click="setLineHeight(lineHeight - LH_STEP)"
                :disabled="lineHeight <= LH_MIN + 1e-9"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ lineHeight.toFixed(2) }}</span>
              <button
                @click="setLineHeight(lineHeight + LH_STEP)"
                :disabled="lineHeight >= LH_MAX - 1e-9"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 段距（R5-c）：−/+ 步进 0.1em，钳制 [PS_MIN, PS_MAX]，控制段落之间的间隔。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.paragraphSpacing') }}</span>
            <div class="rs-stepper">
              <button
                @click="setParagraphSpacing(paragraphSpacingEm - PS_STEP)"
                :disabled="paragraphSpacingEm <= PS_MIN + 1e-9"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ paragraphSpacingEm.toFixed(1) }}em</span>
              <button
                @click="setParagraphSpacing(paragraphSpacingEm + PS_STEP)"
                :disabled="paragraphSpacingEm >= PS_MAX - 1e-9"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 栏宽（max-inline-size）：−/+ 步进 40px，钳制 [W_MIN, W_MAX]，控制单行长度。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.pageWidth') }}</span>
            <div class="rs-stepper">
              <button
                @click="setWidth(maxInlineSizePx - W_STEP)"
                :disabled="maxInlineSizePx <= W_MIN"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ maxInlineSizePx }}px</span>
              <button
                @click="setWidth(maxInlineSizePx + W_STEP)"
                :disabled="maxInlineSizePx >= W_MAX"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 字体族：衬线 / 无衬线。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.fontFamily') }}</span>
            <div class="rs-seg">
              <button :class="{ active: fontFamily === 'serif' }" @click="setFontFamily('serif')">
                {{ t('doc.fontSerif') }}
              </button>
              <button :class="{ active: fontFamily === 'sans' }" @click="setFontFamily('sans')">
                {{ t('doc.fontSans') }}
              </button>
            </div>
          </div>

          <!-- 字重（R3）：常规 / 中 / 粗。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.fontWeight') }}</span>
            <div class="rs-seg">
              <button
                v-for="w in FONT_WEIGHTS"
                :key="w.value"
                :class="{ active: fontWeight === w.value }"
                @click="setFontWeight(w.value)"
              >
                {{ t(w.labelKey) }}
              </button>
            </div>
          </div>

          <!-- 字距（R3）：−/+ 步进 0.01em，钳制 [LS_MIN, LS_MAX]。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.letterSpacing') }}</span>
            <div class="rs-stepper">
              <button
                @click="setLetterSpacing(letterSpacingEm - LS_STEP)"
                :disabled="letterSpacingEm <= LS_MIN + 1e-9"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ letterSpacingEm.toFixed(2) }}em</span>
              <button
                @click="setLetterSpacing(letterSpacingEm + LS_STEP)"
                :disabled="letterSpacingEm >= LS_MAX - 1e-9"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 对齐（R3）：两端 / 左。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.textAlign') }}</span>
            <div class="rs-seg">
              <button :class="{ active: textAlign === 'justify' }" @click="setTextAlign('justify')">
                {{ t('doc.alignJustify') }}
              </button>
              <button :class="{ active: textAlign === 'left' }" @click="setTextAlign('left')">
                {{ t('doc.alignLeft') }}
              </button>
            </div>
          </div>

          <!-- 章题缩放（R3「章题独立」）：−/+ 步进 0.1×，钳制 [TS_MIN, TS_MAX]。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.titleScale') }}</span>
            <div class="rs-stepper">
              <button
                @click="setTitleScale(titleScale - TS_STEP)"
                :disabled="titleScale <= TS_MIN + 1e-9"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ titleScale.toFixed(1) }}×</span>
              <button
                @click="setTitleScale(titleScale + TS_STEP)"
                :disabled="titleScale >= TS_MAX - 1e-9"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <!-- 版式（R5 竖排竹简）：横排 / 竖排右起 / 竖排左起。切换重挂载渲染器(渲染器 getDirection 仅在 load 探测)。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.layoutMode') }}</span>
            <select
              class="rs-select"
              :value="vertical"
              @change="setVertical(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="o in VERTICAL_OPTIONS" :key="o.value" :value="o.value">
                {{ t(o.labelKey) }}
              </option>
            </select>
          </div>
        </div>
      </ReaderSettingsGroup>

      <ReaderSettingsGroup id="paging" :default-open="false" :title="t('doc.readerGroupPaging')">
        <div class="rs-group-rows">
          <!-- 翻页动画：关 / 滑动 / 仿真翻页。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.pageTurn') }}</span>
            <div class="rs-seg">
              <button :class="{ active: pageTurn === 'none' }" @click="setPageTurn('none')">
                {{ t('doc.turnNone') }}
              </button>
              <button :class="{ active: pageTurn === 'slide' }" @click="setPageTurn('slide')">
                {{ t('doc.turnSlide') }}
              </button>
              <button :class="{ active: pageTurn === 'curl' }" @click="setPageTurn('curl')">
                {{ t('doc.turnCurl') }}
              </button>
            </div>
          </div>

          <!-- 自动翻页间隔（秒）：−/+ 步进 1，钳制 [AS_MIN, AS_MAX]。开关在工具栏（▶/⏸）。 -->
          <div class="rs-row">
            <span class="rs-row__label">{{ t('doc.autoScrollInterval') }}</span>
            <div class="rs-stepper">
              <button
                @click="setAutoScrollSec(autoScrollSec - AS_STEP)"
                :disabled="autoScrollSec <= AS_MIN"

              >
                <Minus :size="14" />
              </button>
              <span class="rs-stepper__val">{{ autoScrollSec }}s</span>
              <button
                @click="setAutoScrollSec(autoScrollSec + AS_STEP)"
                :disabled="autoScrollSec >= AS_MAX"

              >
                <Plus :size="14" />
              </button>
            </div>
          </div>
        </div>
      </ReaderSettingsGroup>

      <!-- 每书设置：简繁（三格式通用）+ 编码/重排（仅 txt）。改动经 set_reader_book_prefs 持久化并重挂载渲染器。 -->
      <template v-if="book">
        <ReaderSettingsGroup id="book" :default-open="false" :title="t('doc.readerGroupBook')">
          <div class="rs-group-rows">
            <!-- 简繁转换（R4）：纯显示层，txt/md/epub 通用。 -->
            <div class="rs-row">
              <span class="rs-row__label">{{ t('doc.zhConvert') }}</span>
              <select
                class="rs-select"
                :value="book.zhConvert"
                @change="setZhConvert(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="o in ZH_OPTIONS" :key="o.value" :value="o.value">
                  {{ t(o.labelKey) }}
                </option>
              </select>
            </div>
            <!-- 每书阅读主题（R3）：'' = 跟随全局槽；否则本书专用（当前明暗槽的调色板，三格式通用）。 -->
            <div class="rs-row">
              <span class="rs-row__label">{{ t('doc.bookTheme') }}</span>
              <select
                class="rs-select"
                :value="book.theme"
                @change="setBookTheme(($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ t('doc.bookThemeGlobal') }}</option>
                <option v-for="rt in kindThemes" :key="rt.id" :value="rt.id">
                  {{ t(rt.nameKey) }}
                </option>
              </select>
            </div>
            <!-- 编码覆盖 + 二级重排：仅 txt 有意义。 -->
            <template v-if="isTxt">
              <div class="rs-row">
                <span class="rs-row__label">{{ t('doc.encoding') }}</span>
                <select
                  class="rs-select"
                  :value="book.encoding"
                  @change="setEncoding(($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="e in ENCODINGS" :key="e.value" :value="e.value">
                    {{ e.value === '' ? t('doc.encodingAuto') : e.label }}
                  </option>
                </select>
              </div>
              <label class="rs-row rs-row--toggle">
                <span class="rs-row__label">{{ t('doc.reflow') }}</span>
                <input
                  type="checkbox"
                  :checked="book.reflow"
                  @change="setReflow(($event.target as HTMLInputElement).checked)"
                />
              </label>
              <p class="rs-hint">{{ t('doc.reflowHint') }}</p>
            </template>
          </div>
        </ReaderSettingsGroup>
      </template>

      <button class="rs-panel__reset" @click="resetDefaults">
        <RotateCcw :size="14" /> {{ t('doc.resetDefaults') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
// 阅读器设置面板（阅读器方案 R2-6a/b）。字号 / 行距 / 栏宽 / 字体族（全局阅读偏好）。变更即 emit 'change'，
// 由 DocumentViewer 持久化并透传给 BookReader（watch 后经 setStyles / setAttribute 实时重排，无需 remount）。
// 每书区（R2-6c / R4）：简繁（zhConvert，三格式通用）/ 编码 / 重排（后二者仅 txt，由 isTxt 门控）。
import { computed } from 'vue'
import { X, Plus, Minus, RotateCcw } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import ReaderSettingsGroup from './ReaderSettingsGroup.vue'
import type { ReaderFontFamily, ReaderTextAlign } from '../../utils/readerStyles'
import type { ZhConvertConfig } from '../../types/reader'
import { readerThemesByKind, READER_THEME_FOLLOW } from '../../themes/readerThemes'

/** 面板一次性回传的完整全局阅读偏好（避免分字段 emit 的竞态）。 */
export interface ReaderPrefs {
  fontSizePx: number
  lineHeight: number
  fontFamily: ReaderFontFamily
  maxInlineSizePx: number
  /** 翻页动画（R6）：none 瞬切 / slide 滑动 / curl 仿真翻页。 */
  pageTurn: 'none' | 'slide' | 'curl'
  /** 自动翻页间隔（秒，R4）：开关在工具栏，此处调速率。 */
  autoScrollSec: number
  /** 阅读主题（R3）日/夜两槽（reader theme id 或 FOLLOW）。面板只改当前 app 明暗对应的那槽。 */
  readerThemeLight: string
  readerThemeDark: string
  /** 排版全参数（R3）：字重 / 字距（em）/ 正文对齐 / 章题缩放倍数。 */
  fontWeight: number
  letterSpacingEm: number
  textAlign: ReaderTextAlign
  titleScale: number
  /** 段距（R5-c）：段落之间的间距（em）。横竖排共用。 */
  paragraphSpacingEm: number
  /** 版式（R5 竖排竹简）：off 横排 / vertical-rl 竖排右起 / vertical-lr 竖排左起。切换经父层 remount 生效。 */
  vertical: 'off' | 'vertical-rl' | 'vertical-lr'
}

/** 每书设置。简繁（zhConvert）三格式通用；编码/重排仅 txt（由 isTxt 门控显示）。null → 不显示该区。 */
export interface BookPrefsState {
  encoding: string // WHATWG label；'' = 自动检测（仅 txt）
  reflow: boolean // 二级重排（仅 txt）
  zhConvert: ZhConvertConfig | '' // 简繁档位；'' = 关（txt/md/epub 通用）
  theme: string // 每书阅读主题 id；'' = 跟随全局槽（R3，三格式通用）
}

const props = defineProps<
  ReaderPrefs & {
    /** 每书设置状态；foliate 全格式传入（承载简繁），非 foliate 为 null。 */
    book?: BookPrefsState | null
    /** 是否 txt：仅 txt 显示编码/重排（简繁对全格式显示）。 */
    isTxt?: boolean
    /** app 当前是否暗色（R3）：决定阅读主题选择器编辑哪一槽 + 列出哪一 kind 的调色板。 */
    isDark?: boolean
  }
>()

// 阅读主题（R3）：当前 app 明暗对应的槽位选择 + 该 kind 可选调色板（setter 见下方，与其它 emit 同域）。
const kindThemes = computed(() => readerThemesByKind(props.isDark ? 'dark' : 'light'))
const currentThemePick = computed(() =>
  props.isDark ? props.readerThemeDark : props.readerThemeLight,
)

const emit = defineEmits<{
  (e: 'change', v: ReaderPrefs): void
  (e: 'bookChange', v: BookPrefsState): void
  (e: 'close'): void
}>()

// 每书编码选项：自动 + 常见 CJK 编码（WHATWG label）。
const ENCODINGS: { label: string; value: string }[] = [
  { label: 'auto', value: '' },
  { label: 'UTF-8', value: 'utf-8' },
  { label: 'GBK', value: 'gbk' },
  { label: 'GB18030', value: 'gb18030' },
  { label: 'Big5', value: 'big5' },
  { label: 'Shift_JIS', value: 'shift_jis' },
]

// 简繁档位:关 + 常用四档（简↔繁 / 台湾正体 / 台湾正体含词汇）。value 即 convert_chinese 的 config。
const ZH_OPTIONS: { value: ZhConvertConfig | ''; labelKey: string }[] = [
  { value: '', labelKey: 'doc.zhOff' },
  { value: 's2t', labelKey: 'doc.zhS2t' },
  { value: 't2s', labelKey: 'doc.zhT2s' },
  { value: 's2tw', labelKey: 'doc.zhS2tw' },
  { value: 's2twp', labelKey: 'doc.zhS2twp' },
]

// 每书变更单一出口:以当前 book 状态为基线合并单项 patch 后整体回传（防分字段竞态覆盖）。
function emitBook(patch: Partial<BookPrefsState>) {
  if (props.book) emit('bookChange', { ...props.book, ...patch })
}
function setEncoding(encoding: string) {
  emitBook({ encoding })
}
function setReflow(reflow: boolean) {
  emitBook({ reflow })
}
function setZhConvert(zhConvert: string) {
  emitBook({ zhConvert: zhConvert as ZhConvertConfig | '' })
}
function setBookTheme(theme: string) {
  emitBook({ theme })
}

const { t } = useI18n()

// 与 readerStyles 默认值对齐；范围为常见阅读舒适区。
const FONT_MIN = 12
const FONT_MAX = 32
const DEFAULT_FONT = 19
const LH_MIN = 1.2
const LH_MAX = 2.4
const LH_STEP = 0.05
const DEFAULT_LH = 1.75
// 段距（R5-c）:0..2em 步进 0.1,默认 0.4（比原 0.15 更明显,与 readerStyles 默认对齐）。
const PS_MIN = 0
const PS_MAX = 2
const PS_STEP = 0.1
const DEFAULT_PS = 0.4
const W_MIN = 480
const W_MAX = 1040
const W_STEP = 40
const DEFAULT_W = 720
const AS_MIN = 2
const AS_MAX = 30
const AS_STEP = 1
const DEFAULT_AS = 5
// R3 排版全参数范围/默认。
const FONT_WEIGHTS: { value: number; labelKey: string }[] = [
  { value: 400, labelKey: 'doc.weightNormal' },
  { value: 500, labelKey: 'doc.weightMedium' },
  { value: 700, labelKey: 'doc.weightBold' },
]
const DEFAULT_FW = 400
const LS_MIN = 0
const LS_MAX = 0.15
const LS_STEP = 0.01
const DEFAULT_LS = 0
const TS_MIN = 0.8
const TS_MAX = 2.0
const TS_STEP = 0.1
const DEFAULT_TS = 1
// R5 版式:横排 + 竖排右起(CJK 传统)/ 左起。value 即 BookReader vertical prop / writing-mode。
const VERTICAL_OPTIONS: { value: ReaderPrefs['vertical']; labelKey: string }[] = [
  { value: 'off', labelKey: 'doc.layoutHorizontal' },
  { value: 'vertical-rl', labelKey: 'doc.layoutVerticalRl' },
  { value: 'vertical-lr', labelKey: 'doc.layoutVerticalLr' },
]

const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n))

// 以当前 props 为基线合并单项变更后整体回传（单一出口，防止分字段竞态覆盖）。
function emitChange(patch: Partial<ReaderPrefs>) {
  emit('change', {
    fontSizePx: props.fontSizePx,
    lineHeight: props.lineHeight,
    fontFamily: props.fontFamily,
    maxInlineSizePx: props.maxInlineSizePx,
    pageTurn: props.pageTurn,
    autoScrollSec: props.autoScrollSec,
    readerThemeLight: props.readerThemeLight,
    readerThemeDark: props.readerThemeDark,
    fontWeight: props.fontWeight,
    letterSpacingEm: props.letterSpacingEm,
    textAlign: props.textAlign,
    titleScale: props.titleScale,
    paragraphSpacingEm: props.paragraphSpacingEm,
    vertical: props.vertical,
    ...patch,
  })
}
function setParagraphSpacing(em: number) {
  const snapped = Math.round(em / PS_STEP) * PS_STEP
  emitChange({ paragraphSpacingEm: clamp(Number(snapped.toFixed(1)), PS_MIN, PS_MAX) })
}
function setVertical(v: string) {
  emitChange({ vertical: v as ReaderPrefs['vertical'] })
}
// 阅读主题（R3）：只改当前 app 明暗对应的槽（另一槽保持不变，随 app 切换时由该槽接管）。
function setReaderTheme(id: string) {
  emitChange(props.isDark ? { readerThemeDark: id } : { readerThemeLight: id })
}
function setPageTurn(pageTurn: ReaderPrefs['pageTurn']) {
  emitChange({ pageTurn })
}
function setAutoScrollSec(sec: number) {
  emitChange({ autoScrollSec: clamp(Math.round(sec), AS_MIN, AS_MAX) })
}

function setFontSize(px: number) {
  emitChange({ fontSizePx: clamp(Math.round(px), FONT_MIN, FONT_MAX) })
}
function setLineHeight(lh: number) {
  // 步进浮点累加有精度毛刺，四舍五入到 0.05 网格再钳制。
  const snapped = Math.round(lh / LH_STEP) * LH_STEP
  emitChange({ lineHeight: clamp(Number(snapped.toFixed(2)), LH_MIN, LH_MAX) })
}
function setWidth(px: number) {
  emitChange({ maxInlineSizePx: clamp(Math.round(px / W_STEP) * W_STEP, W_MIN, W_MAX) })
}
function setFontFamily(f: ReaderFontFamily) {
  emitChange({ fontFamily: f })
}
function setFontWeight(w: number) {
  emitChange({ fontWeight: w })
}
function setLetterSpacing(em: number) {
  // 步进浮点毛刺：四舍五入到 0.01 网格再钳制。
  const snapped = Math.round(em / LS_STEP) * LS_STEP
  emitChange({ letterSpacingEm: clamp(Number(snapped.toFixed(2)), LS_MIN, LS_MAX) })
}
function setTextAlign(a: ReaderTextAlign) {
  emitChange({ textAlign: a })
}
function setTitleScale(s: number) {
  const snapped = Math.round(s / TS_STEP) * TS_STEP
  emitChange({ titleScale: clamp(Number(snapped.toFixed(1)), TS_MIN, TS_MAX) })
}
function resetDefaults() {
  emit('change', {
    fontSizePx: DEFAULT_FONT,
    lineHeight: DEFAULT_LH,
    fontFamily: 'serif',
    maxInlineSizePx: DEFAULT_W,
    pageTurn: 'slide',
    autoScrollSec: DEFAULT_AS,
    // 重置阅读主题回「跟随应用」（两槽都还原，避免只重置当前明暗一侧）。
    readerThemeLight: READER_THEME_FOLLOW,
    readerThemeDark: READER_THEME_FOLLOW,
    fontWeight: DEFAULT_FW,
    letterSpacingEm: DEFAULT_LS,
    textAlign: 'justify',
    titleScale: DEFAULT_TS,
    paragraphSpacingEm: DEFAULT_PS,
    vertical: 'off',
  })
}
</script>

<style scoped>
.rs-panel {
  display: flex;
  flex-direction: column;
  width: 300px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-divider);
}
.rs-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
}
.rs-panel__title {
  font-weight: 600;
}
.rs-panel__x {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.rs-panel__body {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-md);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}
.rs-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}
.rs-row__label {
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}
.rs-stepper {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.rs-stepper button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  background: var(--color-bg-hover);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  cursor: pointer;
}
.rs-stepper button:disabled {
  opacity: 0.4;
  cursor: default;
}
.rs-stepper__val {
  min-width: 52px;
  text-align: center;
  font-family: var(--font-mono);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}
.rs-seg {
  display: inline-flex;
  gap: var(--spacing-xs);
}
.rs-seg button {
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  background: var(--color-bg-hover);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.rs-seg button.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  border-color: transparent;
}
.rs-group-rows {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  padding: var(--spacing-sm);
}
.rs-select {
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  height: var(--control-size-compact);
  padding: 0 var(--spacing-xs);
  font-size: var(--font-size-sm);
}
.rs-row--toggle {
  cursor: pointer;
}
.rs-hint {
  margin: -6px 0 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  line-height: 1.4;
}
.rs-panel__reset {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-xs);
  margin-top: 4px;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  background: var(--color-bg-hover);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.rs-panel__reset:hover {
  color: var(--color-text-primary);
}
</style>
