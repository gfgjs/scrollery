<template>
  <!-- 设置页「阅读」节(阅读器方案 R3):阅读主题日/夜配对的全局家。
       与阅读器内设置面板的分工:字号/排版等「边读边调」项留在书内面板(见 ReaderSettingsPanel);
       此处只放**开书前就想定**的默认阅读主题,且是唯一能同时配日/夜两槽的地方
       (书内面板据当前 app 明暗只改一槽)。二者写同一组 app_config 键(doc_reader_theme_*),非双源。 -->
  <CollapsibleCard id="reading" class="reader-surface-trial" :title="$t('settings.reading')">
    <!-- 提示与分组行各自补横向 var(--spacing-lg)/纵向 var(--spacing-md) 内边距，
         与上下 SettingRow 节(.settings-card__item)的插入对齐。 -->
    <div class="reader-sec">
      <p class="reader-sec__hint">{{ $t('settings.readerThemeHint') }}</p>
      <div v-for="group in groups" :key="group.kind" class="reader-sec__group">
        <div class="reader-sec__group-label">{{ $t(group.labelKey) }}</div>
        <div class="reader-sec__grid" :style="{ '--reader-cols': maxCols }">
          <button
            v-for="opt in group.options"
            :key="opt.id"
            class="reader-card"
            :class="{ selected: pick(group.kind) === opt.id }"

            @click="select(group.kind, opt.id)"
          >
            <!-- 阅读预览:真实正文/背景一对色 + 样张字(reader theme 只有 text/bg 两色) -->
            <span class="reader-card__swatch" :style="{ background: opt.bg, color: opt.text }">{{
              SAMPLE_GLYPH
            }}</span>
            <span class="reader-card__name">
              {{ opt.name }}
              <Check v-if="pick(group.kind) === opt.id" :size="13" class="reader-card__check" />
            </span>
          </button>
        </div>
      </div>
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { Check } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import CollapsibleCard from './CollapsibleCard.vue'
import {
  readerThemesByKind,
  READER_THEME_FOLLOW,
  normalizeReaderThemeId,
} from '../../themes/readerThemes'

const { t } = useI18n()

// 预览样张字(非界面文案,不随语言变;走 const 绑定以避开模板裸文本规则)。
const SAMPLE_GLYPH = '文'

// 日/夜槽当前选择(FOLLOW 或 reader theme id);持久化键与 DocumentViewer 同源。
const lightPick = ref(READER_THEME_FOLLOW)
const darkPick = ref(READER_THEME_FOLLOW)

/** 某槽的可选项:FOLLOW 置顶(预览用 app 当前 --color-* 变量,自证「跟随应用」)+ 该 kind 调色板。 */
function optionsFor(kind: 'light' | 'dark') {
  return [
    {
      id: READER_THEME_FOLLOW,
      name: t('doc.followApp'),
      bg: 'var(--color-bg-primary)',
      text: 'var(--color-text-primary)',
    },
    ...readerThemesByKind(kind).map((r) => ({
      id: r.id,
      name: t(r.nameKey),
      bg: r.background,
      text: r.text,
    })),
  ]
}

const groups = computed(() => [
  { kind: 'light' as const, labelKey: 'settings.readerLightThemes', options: optionsFor('light') },
  { kind: 'dark' as const, labelKey: 'settings.readerDarkThemes', options: optionsFor('dark') },
])

// 两槽用同一列数(取各组选项数最大值),日/夜行的卡片逐列对齐、铺满整行,不留右侧死白;
// 未来增删 reader theme 也自动跟随,无需改写死列数。
const maxCols = computed(() => Math.max(...groups.value.map((g) => g.options.length)))

function pick(kind: 'light' | 'dark'): string {
  return kind === 'light' ? lightPick.value : darkPick.value
}
function select(kind: 'light' | 'dark', id: string) {
  const key = kind === 'light' ? 'doc_reader_theme_light' : 'doc_reader_theme_dark'
  if (kind === 'light') lightPick.value = id
  else darkPick.value = id
  invokeIpc(IPC.SET_APP_CONFIG, { key, value: id }).catch(() => {})
}

onMounted(async () => {
  const [l, d] = await Promise.all([
    invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_theme_light' }).catch(
      () => null,
    ),
    invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'doc_reader_theme_dark' }).catch(
      () => null,
    ),
  ])
  lightPick.value = normalizeReaderThemeId(l, 'light')
  darkPick.value = normalizeReaderThemeId(d, 'dark')
})
</script>

<style scoped>
/* 阅读主题选项与普通设置行共用页面流，不再叠加独立卡片材质。 */
.reader-surface-trial {
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  overflow: visible;
}

/* 提示与主题组共处一个表面；各行自行承担内边距，hover 可覆盖完整行宽。 */
.reader-sec {
  padding-top: var(--spacing-md);
}
.reader-sec__hint {
  margin: 0;
  padding: 0 var(--spacing-lg) var(--spacing-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  line-height: 1.5;
}
.reader-sec__group {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  border-top: 1px solid var(--color-divider);
  transition: background-color var(--transition-fast);
}
.reader-sec__group:hover {
  background-color: var(--color-bg-hover);
}
.reader-sec__group-label {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.reader-sec__grid {
  display: grid;
  /* 固定列数 = 各组选项最大值(由 --reader-cols 注入),日/夜行卡片逐列对齐并铺满整行;
     minmax(0,1fr) 允许列在窄面板下正常收缩,不溢出。 */
  grid-template-columns: repeat(var(--reader-cols, 4), minmax(0, 1fr));
  gap: var(--spacing-sm);
}
.reader-card {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-md);
  background: var(--color-bg-surface);
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast);
}
.reader-card:hover {
  border-color: var(--color-border-strong);
  background: var(--color-bg-hover);
}
.reader-card.selected {
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
  box-shadow: inset 0 0 0 2px var(--color-accent);
}
.reader-card:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}
/* 预览样张:真实阅读色对(reader theme 的 text/bg,或 FOLLOW 的 app 变量),故允许内联。 */
.reader-card__swatch {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 44px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-subtle);
  font-size: 20px;
  font-weight: 600;
}
.reader-card__name {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-primary);
}
.reader-card__check {
  color: var(--color-accent);
  flex-shrink: 0;
}
</style>
