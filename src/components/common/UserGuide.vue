<template>
  <!-- 详细首次使用引导手册(设计文档):分章图文,可随时从设置头部重开,故不像 OnboardingWizard
       那样强门控——允许点遮罩/Escape 关闭,关闭键常开。关闭统一走 ui.dismissUserGuide()
       (只在首次写 guide_seen 标记,重开不受影响)。 -->
  <UiDialog
    :open="true"
    :title="t('guide.title')"
    :close-label="t('common.close')"
    max-width="820px"
    body-padding="0"
    @close="ui.dismissUserGuide()"
  >
    <div class="guide-body">
      <!-- 左栏:章节列表,点击跳章。 -->
      <nav class="guide-nav">
        <button
          v-for="(chapter, idx) in chapters"
          :id="`guide-tab-${chapter.id}`"
          :key="chapter.id"
          type="button"

          class="guide-nav__item"
          :class="{ active: idx === current }"
          @click="goTo(idx)"
        >
          <component :is="chapter.icon" :size="16" />
          <span>{{ t(`guide.chapters.${chapter.id}.title`) }}</span>
        </button>
      </nav>

      <!-- 右栏:当前章内容,可滚动。tabpanel 回指当前 tab,tabindex=0 让键盘用户可聚焦滚动。 -->
      <div
        id="guide-panel"
        ref="contentEl"
        class="guide-content"

        tabindex="0"

      >
        <div class="chapter-icon"><component :is="chapters[current].icon" :size="32" /></div>
        <h3 class="chapter-title">{{ t(`guide.chapters.${chapters[current].id}.title`) }}</h3>
        <p class="chapter-intro">{{ t(`guide.chapters.${chapters[current].id}.intro`) }}</p>
        <ul class="chapter-points">
          <li v-for="(p, i) in points" :key="i">{{ rt(p) }}</li>
        </ul>
      </div>
    </div>

    <template #footer>
      <!-- UiDialog footer 为 flex-end,故用 width:100% 的 footer-split 自撑「跳过 ↔ 导航」两端布局
           (同 OnboardingWizard 范式)。 -->
      <div class="footer-split">
        <UiButton variant="ghost" @click="ui.dismissUserGuide()">{{ t('guide.skip') }}</UiButton>
        <div class="footer-nav">
          <span class="chapter-progress">
            {{ t('guide.chapterProgress', { current: current + 1, total: chapters.length }) }}
          </span>
          <UiButton v-if="current > 0" variant="secondary" @click="back">
            <ArrowLeft :size="15" />
            <span>{{ t('guide.prev') }}</span>
          </UiButton>
          <UiButton v-if="current < chapters.length - 1" variant="primary" @click="next">
            <span>{{ t('guide.next') }}</span>
            <ArrowRight :size="15" />
          </UiButton>
          <UiButton v-else variant="primary" @click="ui.dismissUserGuide()">
            <Check :size="15" />
            <span>{{ t('guide.finish') }}</span>
          </UiButton>
        </div>
      </div>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Sparkles,
  LayoutGrid,
  AlignVerticalJustifyStart,
  Search,
  MousePointerSquareDashed,
  Image,
  FileText,
  Users,
  Puzzle,
  Palette,
  Settings,
  GraduationCap,
  ArrowLeft,
  ArrowRight,
  Check,
} from '@lucide/vue'
import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import { useUiStore } from '../../stores/uiStore'

const { t, tm, rt } = useI18n()
const ui = useUiStore()

// tm() 的类型签名会针对全量消息 schema 做模板字面量联合匹配,动态拼接的 key 触发
// TS2589(类型实例化过深)——窄化成简单函数类型规避,不引入 any。
const tmDynamic = tm as (key: string) => unknown

// 章节顺序严格照设计文档 12 章;图标语义匹配(全部来自 @lucide/vue)。
const chapters: { id: string; icon: typeof Sparkles }[] = [
  { id: 'welcome', icon: Sparkles },
  { id: 'gallery', icon: LayoutGrid },
  { id: 'timeline', icon: AlignVerticalJustifyStart },
  { id: 'searchFilter', icon: Search },
  { id: 'selection', icon: MousePointerSquareDashed },
  { id: 'viewer', icon: Image },
  { id: 'docAudio', icon: FileText },
  { id: 'people', icon: Users },
  { id: 'plugins', icon: Puzzle },
  { id: 'themes', icon: Palette },
  { id: 'settingsShortcuts', icon: Settings },
  { id: 'wrapUp', icon: GraduationCap },
]

const current = ref(0)
const contentEl = ref<HTMLElement | null>(null)

// 当前章的要点列表(i18n 数组):tm 取原始消息数组,rt 逐项渲染为最终字符串。
const points = computed(
  () => tmDynamic(`guide.chapters.${chapters[current.value].id}.points`) as string[],
)

/** 切章后把右栏滚动位置复位到顶,避免长章节切到短章节时残留滚动偏移。 */
async function resetScroll() {
  await nextTick()
  if (contentEl.value) contentEl.value.scrollTop = 0
}

function goTo(idx: number) {
  if (idx === current.value) return
  current.value = idx
  void resetScroll()
}

function next() {
  if (current.value < chapters.length - 1) {
    current.value += 1
    void resetScroll()
  }
}

function back() {
  if (current.value > 0) {
    current.value -= 1
    void resetScroll()
  }
}
</script>

<style scoped>
/* 外壳(overlay/content/关闭键/动画)由 UiDialog + 全局 Modal 基座提供。本组件只管左右两栏布局。 */
.guide-body {
  display: flex;
  min-height: 360px;
  max-height: 60vh;
}

.guide-nav {
  flex: 0 0 200px;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-2xs);
  padding: var(--spacing-md);
  border-right: 1px solid var(--color-divider);
  overflow-y: auto;
}

.guide-nav__item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  text-align: left;
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}

.guide-nav__item:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.guide-nav__item.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  font-weight: 500;
}

.guide-content {
  flex: 1;
  min-height: 360px;
  overflow-y: auto;
  padding: var(--spacing-lg);
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}

.chapter-icon {
  width: 60px;
  height: 60px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  margin-bottom: var(--spacing-md);
}

.chapter-title {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}

.chapter-intro {
  margin: var(--spacing-xs) 0 var(--spacing-lg);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  line-height: 1.5;
  max-width: 480px;
}

.chapter-points {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  max-width: 480px;
  width: 100%;
  text-align: left;
}

.chapter-points li {
  position: relative;
  padding-left: 18px;
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  line-height: 1.5;
}

.chapter-points li::before {
  content: '';
  position: absolute;
  left: 4px;
  top: 8px;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--color-accent);
}

/* 页脚外壳由 UiDialog .dialog-footer 提供(flex-end);footer-split 用 width:100% 自撑
   「跳过 ↔ 进度+导航」两端布局,同 OnboardingWizard 范式。 */
.footer-split {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.footer-nav {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.chapter-progress {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-right: var(--spacing-xs);
}
</style>
