<template>
  <!-- 首启向导（Part5 T17, §3.8）：3 步 onboarding —— 加扫描目录 / 选主题 / 选语言。外壳迁 UiDialog。
       刻意不支持点遮罩 / Escape 关闭（close-on-overlay/esc=false + show-close=false）：首启须显式「完成」或
       「跳过」，避免误触留下空图库且 flag 未写。自定义头部（副标题 + 步骤点）走 #header 插槽整体覆盖默认标题行。 -->
  <UiDialog
    :open="true"
    :title="t('onboarding.welcome')"
    :show-close="false"
    :close-on-overlay="false"
    :close-on-esc="false"
    max-width="520px"
    body-padding="0"
  >
    <template #header>
      <!-- 顶部：欢迎语 + 步骤进度点 -->
      <header class="onboarding-header">
        <h2 class="onboarding-title">{{ t('onboarding.welcome') }}</h2>
        <p class="onboarding-subtitle">{{ t('onboarding.subtitle') }}</p>
        <div
          class="step-dots"
        >
          <span
            v-for="n in 3"
            :key="n"
            class="step-dot"
            :class="{ active: n === step, done: n < step }"
          ></span>
        </div>
      </header>
    </template>

    <div class="onboarding-body">
      <!-- 步骤 ①：添加扫描目录（核心，否则空图库） -->
      <section v-if="step === 1" class="step">
        <div class="step-icon"><FolderPlus :size="32" /></div>
        <h3 class="step-title">{{ t('onboarding.step1.title') }}</h3>
        <p class="step-desc">{{ t('onboarding.step1.desc') }}</p>

        <button class="btn btn-primary add-folder-btn" @click="pickFolder">
          <FolderPlus :size="16" />
          <span>{{ t('onboarding.step1.addBtn') }}</span>
        </button>

        <ul v-if="addedFolders.length" class="folder-list">
          <li v-for="p in addedFolders" :key="p" class="folder-item">
            <Folder :size="14" />
            <span class="folder-name" :title="p">{{ basename(p) }}</span>
            <Check :size="14" class="folder-check" />
          </li>
        </ul>
        <p v-else class="step-hint">{{ t('onboarding.step1.empty') }}</p>
      </section>

      <!-- 步骤 ②：选主题（即时 live 应用，所见即所得） -->
      <section v-else-if="step === 2" class="step">
        <div class="step-icon"><Palette :size="32" /></div>
        <h3 class="step-title">{{ t('onboarding.step2.title') }}</h3>
        <p class="step-desc">{{ t('onboarding.step2.desc') }}</p>

        <div class="option-grid">
          <button
            v-for="opt in themeOptions"
            :key="opt.value"
            class="option-card"
            :class="{ selected: ui.appearance === opt.value }"
            @click="chooseTheme(opt.value)"
          >
            <component :is="opt.icon" :size="22" />
            <span>{{ t(opt.labelKey) }}</span>
            <Check v-if="ui.appearance === opt.value" :size="15" class="option-check" />
          </button>
        </div>
      </section>

      <!-- 步骤 ③：选语言（即时 live 应用，向导文案自身随之切换） -->
      <section v-else class="step">
        <div class="step-icon"><Languages :size="32" /></div>
        <h3 class="step-title">{{ t('onboarding.step3.title') }}</h3>
        <p class="step-desc">{{ t('onboarding.step3.desc') }}</p>

        <div class="option-grid">
          <button
            v-for="opt in langOptions"
            :key="opt.value"
            class="option-card"
            :class="{ selected: ui.language === opt.value }"
            @click="chooseLang(opt.value)"
          >
            <span class="lang-label">{{ opt.label }}</span>
            <Check v-if="ui.language === opt.value" :size="15" class="option-check" />
          </button>
        </div>
      </section>
    </div>

    <template #footer>
      <!-- UiDialog footer 为 flex-end,故用 width:100% 的 footer-split 自撑「跳过 ↔ 导航」两端布局。 -->
      <div class="footer-split">
        <UiButton variant="ghost" @click="complete">{{ t('onboarding.skip') }}</UiButton>
        <div class="footer-nav">
          <UiButton v-if="step > 1" variant="secondary" @click="back">
            <ArrowLeft :size="15" />
            <span>{{ t('onboarding.back') }}</span>
          </UiButton>
          <UiButton v-if="step < 3" variant="primary" @click="next">
            <span>{{ t('onboarding.next') }}</span>
            <ArrowRight :size="15" />
          </UiButton>
          <UiButton v-else variant="primary" @click="complete">
            <Check :size="15" />
            <span>{{ t('onboarding.finish') }}</span>
          </UiButton>
        </div>
      </div>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import {
  FolderPlus,
  Folder,
  Check,
  Palette,
  Sun,
  Moon,
  Monitor,
  Languages,
  ArrowLeft,
  ArrowRight,
} from '@lucide/vue'
import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import { invokeIpc } from '../../utils/ipc'
import { logger } from '../../utils/logger'
import { IPC } from '../../constants/ipc'
import { useUiStore } from '../../stores/uiStore'
import { useToastStore } from '../../stores/toastStore'
import { useScanStore } from '../../stores/scanStore'
import type { AppearanceMode } from '../../types/ui'

const emit = defineEmits<{ (e: 'done'): void }>()

const { t } = useI18n()
const ui = useUiStore()
const toast = useToastStore()
const scan = useScanStore()

const step = ref(1)
const addedFolders = ref<string[]>([])

// 主题三选项（图标 + i18n label）。点击即时应用——所见即所得。
const themeOptions: { value: AppearanceMode; icon: typeof Sun; labelKey: string }[] = [
  { value: 'light', icon: Sun, labelKey: 'onboarding.step2.light' },
  { value: 'dark', icon: Moon, labelKey: 'onboarding.step2.dark' },
  { value: 'system', icon: Monitor, labelKey: 'onboarding.step2.system' },
]

// 语言用各自母语自名（zh-CN→简体中文 / en-US→English），不经 i18n——避免切换后名字也变。
const langOptions: { value: string; label: string }[] = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'en-US', label: 'English' },
]

/** 从绝对路径取末段目录名（向导列表只展示名字，完整路径放 title）。跨平台同时切 / 与 \\。 */
function basename(p: string): string {
  const parts = p.split(/[/\\]/).filter(Boolean)
  return parts[parts.length - 1] || p
}

/** 选目录 → 加为扫描根。镜像 FoldersSection 的加目录范式（plugin-dialog open + addScanRoot）。 */
async function pickFolder() {
  try {
    const path = await open({ directory: true, multiple: false })
    if (typeof path !== 'string') return // 取消或多选返回数组——向导只收单目录
    if (addedFolders.value.includes(path)) return // 防重复加同一目录
    await scan.addScanRoot(path)
    addedFolders.value.push(path)
  } catch (e) {
    toast.addToast('error', t('onboarding.step1.addFailed', { error: String(e) }))
  }
}

function chooseTheme(value: AppearanceMode) {
  ui.setAppearance(value) // 即时应用 + 持久化 app_config 'appearance'
}

function chooseLang(lang: string) {
  ui.setLanguage(lang) // 即时切 i18n + 持久化 app_config 'language'
}

function next() {
  if (step.value < 3) step.value += 1
}

function back() {
  if (step.value > 1) step.value -= 1
}

/** 完成或跳过：写 first_launch=false（不再弹）后通知父层卸载。两路同一收尾——
 *  用户中途调过的主题/语言已即时持久化，跳过即沿用当前值（默认 system / zh-CN）。 */
async function complete() {
  try {
    await invokeIpc(IPC.SET_APP_CONFIG, { key: 'first_launch', value: 'false' })
  } catch (e) {
    // flag 写失败不致命（下次启动会再弹一次），仅记录不阻断关闭。
    logger.error('Failed to persist first_launch flag', { error: e })
  }
  emit('done')
}
</script>

<style scoped>
/* 外壳(overlay/content/关闭键/动画)由 UiDialog + 全局 Modal 基座提供;宽度 520px 走 max-width prop。
   本组件保留头部(副标题+步骤点)、正文步骤区、分栏页脚特化。 */
.onboarding-header {
  padding: var(--spacing-lg) var(--spacing-lg) var(--spacing-md);
  text-align: center;
  border-bottom: 1px solid var(--color-divider);
}

.onboarding-title {
  margin: 0;
  font-size: var(--font-size-xl);
  font-weight: 700;
  color: var(--color-text-primary);
}

.onboarding-subtitle {
  margin: 6px 0 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}

.step-dots {
  display: flex;
  justify-content: center;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-md);
}

.step-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--color-border);
  /* 圆点→胶囊形变刻意补间 width(S1 机械批:属性显式列举) */
  transition:
    width var(--transition-fast),
    border-radius var(--transition-fast),
    background-color var(--transition-fast);
}

.step-dot.active {
  width: 22px;
  border-radius: 4px;
  background: var(--color-accent);
}

.step-dot.done {
  background: var(--color-accent-subtle);
}

.onboarding-body {
  padding: var(--spacing-lg);
  min-height: 220px;
  display: flex;
  flex-direction: column;
  justify-content: center;
}

.step {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}

.step-icon {
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

.step-title {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}

.step-desc {
  margin: var(--spacing-xs) 0 var(--spacing-lg);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  line-height: 1.5;
  max-width: 380px;
}

.add-folder-btn {
  gap: var(--spacing-sm);
}

.folder-list {
  list-style: none;
  margin: var(--spacing-md) 0 0;
  padding: 0;
  width: 100%;
  max-width: 380px;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.folder-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border-radius: var(--radius-sm);
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
}

.folder-name {
  flex: 1;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.folder-check {
  color: var(--color-success);
  flex-shrink: 0;
}

.step-hint {
  margin: var(--spacing-md) 0 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

.option-grid {
  display: flex;
  gap: var(--spacing-md);
  width: 100%;
  justify-content: center;
}

.option-card {
  position: relative;
  flex: 1;
  max-width: 130px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-lg);
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast),
    color var(--transition-fast);
}

.option-card:hover {
  border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
  background: var(--color-bg-hover);
}

.option-card.selected {
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}

.option-check {
  position: absolute;
  top: 6px;
  right: 6px;
}

.lang-label {
  font-size: var(--font-size-base);
  font-weight: 600;
}

/* 页脚外壳(padding/border-top/背景)由 UiDialog .dialog-footer 提供;它是 flex-end,故 footer-split
   用 width:100% 自撑「跳过 ↔ 导航」space-between。跳过/导航按钮已迁 UiButton(ghost/secondary/primary),
   图标+文字对齐/间距走全局 .btn;原 scoped .btn-ghost(tertiary,违「交互文本禁用 tertiary」)一并清除。 */
.footer-split {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.footer-nav {
  display: flex;
  gap: var(--spacing-sm);
}
</style>
