<template>
  <AccordionSection id="tools" :order="order" :title="$t('sidebar.tools')">
    <ul class="tool-list">
      <li
        v-for="(key, index) in ui.pinnedSettings"
        :key="key"
        class="tool"
        :class="{ 'tool--drop': dropIndex === index && dragIndex !== null && dragIndex !== index }"
        :data-tool-index="index"
      >
        <!-- 拖拽手柄——仅此处发起排序，使卡片内控件仍可使用。
             真按钮 + 方向键即按即移(↑↓ 一位/Home End 首末),
             键盘用户与指针拖拽同能力;pointer 拖拽逻辑在 button 上原样成立。 -->
        <button
          type="button"
          class="tool__handle"
          :title="t('sidebar.dragToReorder')"

          @pointerdown="onPointerDown(index, $event)"
          @keydown="onHandleKeydown(index, $event)"
        >
          <GripVertical :size="14" />
        </button>

        <!-- 特殊：生成缩略图(增量 + 全量重建) -->
        <div v-if="key === 'fullThumbGen'" class="tool__card">
          <div class="tool__row">
            <div class="tool__main">
              <span class="tool__icon"><Zap :size="18" /></span>
              <span class="tool__label">{{ t('settings.fullThumbGen') }}</span>
            </div>
            <!-- 运行中只剩停止;空闲时 增量(Play,只补缺失)/全量重建(RotateCcw,重置全表)
                 双钮并列——镜像 AI 卡片「开始/重新开始」的动词分工。 -->
            <div class="thumb-actions">
              <UiIconButton
                v-if="scan.thumbGenProgress.isRunning"
                :label="t('settings.stopGen')"
                @click="scan.stopFullThumbnailGeneration()"
              >
                <Square :size="14" color="var(--color-error)" fill="var(--color-error)" />
              </UiIconButton>
              <template v-else>
                <UiIconButton
                  :label="t('settings.startGenIncremental')"
                  @click="scan.startIncrementalThumbnailGeneration()"
                >
                  <Play :size="14" />
                </UiIconButton>
                <UiIconButton
                  :label="t('settings.startGenFullRebuild')"
                  @click="scan.startFullThumbnailGeneration()"
                >
                  <RotateCcw :size="14" />
                </UiIconButton>
              </template>
            </div>
          </div>
          <div
            v-if="scan.thumbGenProgress.isRunning || scan.thumbGenProgress.status === 'completed'"
            class="tool__progress"
          >
            <div
              v-if="scan.thumbGenProgress.isRunning"
              class="progress-bar"
            >
              <div class="progress-bar__fill" :style="{ width: thumbPercent + '%' }" />
            </div>
            <div class="tool__progress-meta">
              <span>{{ scan.thumbGenProgress.generated }} / {{ scan.thumbGenProgress.total }}</span>
              <span v-if="thumbElapsedStr" class="mono">{{ thumbElapsedStr }}</span>
            </div>
          </div>
        </div>

        <!-- 特殊：视频封面/关键帧提取(增量/全量/停止 + 关键帧勾选) -->
        <div v-else-if="key === 'videoDeriveGen'" class="tool__card">
          <div class="tool__row">
            <div class="tool__main">
              <span class="tool__icon"><Video :size="18" /></span>
              <span class="tool__label">{{ t('settings.videoDeriveGen') }}</span>
            </div>
            <!-- 与缩略图卡同款动词分工:运行中只剩停止;空闲时 增量(Play)/全量(RotateCcw)。 -->
            <div class="thumb-actions">
              <UiIconButton
                v-if="derive.isVideoRunning"
                :label="t('settings.videoDeriveStop')"
                @click="derive.stopVideoExtraction()"
              >
                <Square :size="14" color="var(--color-error)" fill="var(--color-error)" />
              </UiIconButton>
              <template v-else>
                <UiIconButton
                  :label="t('settings.videoDeriveIncremental')"
                  @click="derive.startVideoIncremental()"
                >
                  <Play :size="14" />
                </UiIconButton>
                <UiIconButton
                  :label="t('settings.videoDeriveFull')"
                  @click="derive.startVideoFull()"
                >
                  <RotateCcw :size="14" />
                </UiIconButton>
              </template>
            </div>
          </div>
          <!-- 关键帧勾选:单一事实源 = enable_video_keyframes(与设置页开关同源;写入即重启
               流水线生效,运行中禁改避免中途重启抖动)。勾选才连带关键帧,否则只提封面。 -->
          <label class="video-derive-keyframes">
            <input
              type="checkbox"
              :checked="config.enableVideoKeyframes"
              :disabled="derive.isVideoRunning"
              @change="onKeyframesToggle"
            />
            {{ t('settings.enableVideoKeyframes') }}
          </label>
          <div v-if="derive.isVideoRunning" class="tool__progress">
            <div
              class="progress-bar"
            >
              <div class="progress-bar__fill" :style="{ width: videoDerivePercent + '%' }" />
            </div>
            <div class="tool__progress-meta">
              <span>{{ derive.videoFinished }} / {{ derive.videoTotal }}</span>
            </div>
          </div>
          <!-- 失败面:全量提取会把 error 行一并退回重做(reset 含 status=3),增量不会。 -->
          <div v-if="derive.videoStatus.error > 0" class="tool__error-row">
            <AlertTriangle :size="12" />
            <span>{{
              t('settings.videoDeriveErrorCount', { count: derive.videoStatus.error })
            }}</span>
          </div>
        </div>

        <!-- 特殊：全量 AI 分析（常驻核心项） -->
        <div v-else-if="key === 'aiFullAnalysis'" class="tool__card">
          <div class="tool__row">
            <div class="tool__main">
              <span class="tool__icon"><Sparkles :size="18" /></span>
              <span class="tool__label">{{ t('sidebar.aiFullAnalysis') }}</span>
            </div>
            <div class="ai-actions">
              <!-- 暂停（运行中）/ 等待共享分析会话（已排队续跑）/ 继续（已暂停）/ 开始 -->
              <UiIconButton
                v-if="ai.status.isAnalyzing"
                :label="t('common.pause')"
                @click="ai.pauseAnalysis()"
              >
                <Pause :size="14" />
              </UiIconButton>
              <!-- 资源等待（P1-1）：共享 GPU 分析会话被对端持有，本地已排队，对端释放后自动续跑 -->
              <UiIconButton
                v-else-if="ai.waitingForSession"
                disabled
                :label="t('sidebar.waitingOnAnalysisBusy')"
              >
                <RefreshCw :size="14" class="spin-anim" />
              </UiIconButton>
              <UiIconButton
                v-else-if="aiResumable"
                :disabled="isAiInitialising"
                :label="aiPaused ? t('sidebar.resume') : t('sidebar.start')"
                @click="startOrResume"
              >
                <RefreshCw v-if="isAiInitialising" :size="14" class="spin-anim" />
                <Play v-else :size="14" />
              </UiIconButton>

              <!-- 停止（清除续传，不再自动继续）；资源等待中同样给此按钮以撤回排队 -->
              <UiIconButton
                v-if="ai.status.isAnalyzing || aiPaused || ai.waitingForSession"
                :label="t('sidebar.stopNoResume')"
                @click="ai.stopAnalysis()"
              >
                <Square :size="14" color="var(--color-error)" fill="var(--color-error)" />
              </UiIconButton>

              <!-- 重新开始（清空并全量重做） -->
              <UiIconButton
                v-if="ai.status.totalItems > 0"
                :disabled="isAiInitialising"
                :label="t('sidebar.restartFull')"
                @click="restartAnalysis"
              >
                <RotateCcw :size="14" />
              </UiIconButton>
            </div>
          </div>
          <div v-if="ai.status.isAnalyzing || ai.status.totalItems > 0" class="tool__progress">
            <div
              v-if="ai.status.isAnalyzing"
              class="progress-bar"
            >
              <div class="progress-bar__fill" :style="{ width: ai.analyzeProgress + '%' }" />
            </div>
            <div class="tool__progress-meta">
              <span>{{ ai.status.analyzedItems }} / {{ ai.status.totalItems }}</span>
              <span v-if="aiElapsedStr" class="mono push">{{ aiElapsedStr }}</span>
              <span class="mono">{{ ai.analyzeProgress }}%</span>
            </div>
          </div>
          <!-- 等待原因（可观测性三修 #3）：运行中 + 有具体阻塞源 + 进度确实停滞才现身，
               不在正常吞吐间隙误报（isWaitingBlocked 判定见 useAnalysisController）。 -->
          <div v-if="ai.isWaitingBlocked" class="tool__waiting-row">
            {{ waitingOnText(ai.status.waitingOn) }}
          </div>
          <!-- 资源等待（P1-1）：未运行但共享 GPU 分析会话被对端持有、本地已排队——与「手动暂停」
               （用户意图，不自动唤醒）分开显示，也与上面运行中的让步等待分开。 -->
          <div v-if="ai.waitingForSession" class="tool__waiting-row">
            {{ waitingOnText([ANALYSIS_BUSY_WAITING_KEY]) }}
          </div>
          <!-- 失败面（审查 A11/F9）：Error 项可见 + 非破坏重试（不动已完成向量） -->
          <div v-if="ai.status.errorItems > 0" class="tool__error-row">
            <AlertTriangle :size="12" />
            <span>{{ t('sidebar.failedCount', { count: ai.status.errorItems }) }}</span>
            <button class="tool__retry-btn" @click="ai.retryFailedItems()">
              {{ t('sidebar.retryFailed') }}
            </button>
          </div>
        </div>

        <!-- 特殊：全量人脸识别（常驻核心项） -->
        <div v-else-if="key === 'faceFullAnalysis'" class="tool__card">
          <div class="tool__row">
            <div class="tool__main">
              <span class="tool__icon"><ScanFace :size="18" /></span>
              <span class="tool__label">{{ t('sidebar.faceFullAnalysis') }}</span>
            </div>
            <div class="ai-actions">
              <!-- 暂停（运行中）/ 等待共享分析会话（已排队续跑）/ 继续（已暂停）/ 开始 -->
              <UiIconButton
                v-if="face.status.isAnalyzing"
                :label="t('common.pause')"
                @click="face.pauseAnalysis()"
              >
                <Pause :size="14" />
              </UiIconButton>
              <!-- 资源等待（P1-1）：共享 GPU 分析会话被对端持有，本地已排队，对端释放后自动续跑 -->
              <UiIconButton
                v-else-if="face.waitingForSession"
                disabled
                :label="t('sidebar.waitingOnAnalysisBusy')"
              >
                <RefreshCw :size="14" class="spin-anim" />
              </UiIconButton>
              <UiIconButton
                v-else-if="faceResumable"
                :disabled="isFaceStarting"
                :label="facePaused ? t('sidebar.resume') : t('sidebar.start')"
                @click="startOrResumeFace"
              >
                <RefreshCw v-if="isFaceStarting" :size="14" class="spin-anim" />
                <Play v-else :size="14" />
              </UiIconButton>

              <!-- 停止（清除续传，不再自动继续）；资源等待中同样给此按钮以撤回排队 -->
              <UiIconButton
                v-if="face.status.isAnalyzing || facePaused || face.waitingForSession"
                :label="t('sidebar.stopNoResume')"
                @click="face.stopAnalysis()"
              >
                <Square :size="14" color="var(--color-error)" fill="var(--color-error)" />
              </UiIconButton>

              <!-- 重新开始（清空并全量重做） -->
              <UiIconButton
                v-if="face.status.faceCount > 0 || face.status.processedItems > 0"
                :label="t('sidebar.restartFaceFull')"
                @click="restartFaceAnalysis"
              >
                <RotateCcw :size="14" />
              </UiIconButton>
            </div>
          </div>
          <div
            v-if="face.status.isAnalyzing || face.status.processedItems > 0"
            class="tool__progress"
          >
            <div
              v-if="face.status.isAnalyzing"
              class="progress-bar"
            >
              <div class="progress-bar__fill" :style="{ width: face.analyzeProgress + '%' }" />
            </div>
            <div class="tool__progress-meta">
              <span>{{ face.status.processedItems }} / {{ face.status.totalItems }}</span>
              <span v-if="face.status.personCount > 0" class="mono push">{{
                t('sidebar.peopleFacesCount', {
                  persons: face.status.personCount,
                  faces: face.status.faceCount,
                })
              }}</span>
              <span class="mono">{{ face.analyzeProgress }}%</span>
            </div>
          </div>
          <!-- 等待原因（可观测性三修 #3），与 AI 卡同款判定与文案映射。 -->
          <div v-if="face.isWaitingBlocked" class="tool__waiting-row">
            {{ waitingOnText(face.status.waitingOn) }}
          </div>
          <!-- 资源等待（P1-1）：与 AI 卡同款判定；与「手动暂停」分开显示。 -->
          <div v-if="face.waitingForSession" class="tool__waiting-row">
            {{ waitingOnText([ANALYSIS_BUSY_WAITING_KEY]) }}
          </div>
          <!-- 失败面（审查 A11/F9）：与 restart（销毁命名/确认）区分的零破坏补跑通道 -->
          <div v-if="face.status.errorItems > 0" class="tool__error-row">
            <AlertTriangle :size="12" />
            <span>{{ t('sidebar.failedCount', { count: face.status.errorItems }) }}</span>
            <button class="tool__retry-btn" @click="face.retryFailedItems()">
              {{ t('sidebar.retryFailed') }}
            </button>
          </div>
        </div>

        <!-- 通用：置顶的设置项，使用其紧凑控件渲染。 -->
        <div v-else-if="getSettingSpec(key)" class="tool__card">
          <div class="tool__main">
            <span class="tool__icon"><component :is="getSettingSpec(key)!.icon" :size="18" /></span>
            <span class="tool__label tool__label--ellipsis">{{
              $t(getSettingSpec(key)!.label)
            }}</span>
          </div>
          <DynamicSettingControl :setting-key="key" compact />
        </div>
      </li>
    </ul>
  </AccordionSection>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  GripVertical,
  Zap,
  Square,
  Play,
  Pause,
  RotateCcw,
  Sparkles,
  RefreshCw,
  ScanFace,
  AlertTriangle,
  Video,
} from '@lucide/vue'
import AccordionSection from '../AccordionSection.vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import DynamicSettingControl from '../../settings/DynamicSettingControl.vue'
import { useUiStore } from '../../../stores/uiStore'
import { useScanStore } from '../../../stores/scanStore'
import { useAiStore } from '../../../stores/aiStore'
import { useFaceStore } from '../../../stores/faceStore'
import { useDerivationStore } from '../../../stores/derivationStore'
import { useConfigStore } from '../../../stores/configStore'
import { getSettingSpec } from '../../../constants/settingsMap'
import { beginPointerDrag, DRAG_THRESHOLD } from '../../../composables/usePointerDrag'
import { ANALYSIS_BUSY_WAITING_KEY } from '../../../composables/useAnalysisController'
import { useConfirm } from '../../../composables/useConfirm'

defineProps<{ order: number }>()

const ui = useUiStore()
const scan = useScanStore()
const ai = useAiStore()
const face = useFaceStore()
const derive = useDerivationStore()
const config = useConfigStore()
const { confirm } = useConfirm()
const { t } = useI18n()

// ── 拖拽排序（基于指针，见 usePointerDrag） ─────────────────────────────────
const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

// 等待原因映射（可观测性三修 #3）：后端 waitingOn 阻塞源键 → 中文提示；未知键原样显示
// （后端新增阻塞源时旧前端不会显示空白，只是暂时看到英文键名）。展示时机（running &&
// 非空 && 进度停滞）由 useAnalysisController 的 isWaitingBlocked 判定，这里只管文案。
function waitingOnLabel(key: string): string {
  switch (key) {
    case ANALYSIS_BUSY_WAITING_KEY:
      return t('sidebar.waitingOnAnalysisBusy')
    case 'derivation':
      return t('sidebar.waitingOnDerivation')
    case 'scan':
      return t('sidebar.waitingOnScan')
    case 'thumbnail':
      return t('sidebar.waitingOnThumbnail')
    case 'exotic':
      return t('sidebar.waitingOnExotic')
    case 'interaction':
      return t('sidebar.waitingOnInteractive')
    default:
      return key
  }
}
function waitingOnText(keys: string[]): string {
  return keys.map(waitingOnLabel).join(t('sidebar.waitingOnSeparator'))
}

// 键盘排序:即按即移,无「抓取/放下」两段式。reorderPinnedSetting 自带
// 越界守卫(端点再按 = no-op);v-for :key 稳定 → 重排仅移动 DOM 节点,焦点随手柄同行迁移。
function onHandleKeydown(index: number, e: KeyboardEvent) {
  let to: number | null = null
  if (e.key === 'ArrowUp') to = index - 1
  else if (e.key === 'ArrowDown') to = index + 1
  else if (e.key === 'Home') to = 0
  else if (e.key === 'End') to = ui.pinnedSettings.length - 1
  if (to === null) return
  e.preventDefault() // 防方向键滚动侧栏
  ui.reorderPinnedSetting(index, to)
}

function onPointerDown(index: number, e: PointerEvent) {
  if (e.button !== 0) return
  e.preventDefault() // 抑制手柄上的文本选择
  const startX = e.clientX,
    startY = e.clientY
  let dragging = false

  beginPointerDrag(
    (ev) => {
      if (!dragging) {
        if (Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY) < DRAG_THRESHOLD) return
        dragging = true
        dragIndex.value = index
        document.body.style.userSelect = 'none'
        document.body.style.cursor = 'grabbing'
      }
      const li = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest(
        '[data-tool-index]',
      ) as HTMLElement | null
      dropIndex.value = li ? Number(li.dataset.toolIndex) : null
    },
    (_ev, cancelled) => {
      const from = dragIndex.value,
        to = dropIndex.value
      dragIndex.value = null
      dropIndex.value = null
      if (!cancelled && dragging && from != null && to != null && from !== to) {
        ui.reorderPinnedSetting(from, to)
      }
    },
  )
}

// ── 缩略图生成控制 + 计时 ───────────────────────────────────────────────────
const thumbPercent = computed(
  () => (scan.thumbGenProgress.generated / Math.max(scan.thumbGenProgress.total, 1)) * 100,
)

const { elapsedStr: thumbElapsedStr } = useElapsedTimer(
  () => scan.thumbGenProgress.isRunning,
  () => scan.thumbGenProgress.status === 'completed',
)

// ── 视频封面/关键帧提取控制 ──────────────────────────────────────────────────
const videoDerivePercent = computed(
  () => (derive.videoFinished / Math.max(derive.videoTotal, 1)) * 100,
)

function onKeyframesToggle(e: Event) {
  void config.setEnableVideoKeyframes((e.target as HTMLInputElement).checked)
}

// ── AI 分析控制 + 计时 ───────────────────────────────────────────────────────
const isAiInitialising = ref(false)

// 已暂停 = 未运行但仍「期望运行」且有剩余（问题7 的续传态），且不在资源等待中——后者另有
// 排队指示（waitingForSession），不算用户手动暂停。可开始 = 有待处理项。
const aiResumable = computed(() => ai.status.pendingItems > 0)
const aiPaused = computed(
  () =>
    !ai.status.isAnalyzing &&
    ai.status.analysisActive &&
    ai.status.pendingItems > 0 &&
    !ai.waitingForSession,
)

async function startOrResume() {
  if (isAiInitialising.value) return
  isAiInitialising.value = true
  try {
    if (!ai.status.clipLoaded) await ai.initEngine()
    await ai.startAnalysis() // 续传 / 开始 — 不重置
  } finally {
    isAiInitialising.value = false
  }
}

async function restartAnalysis() {
  const { confirmed } = await confirm({
    title: t('sidebar.restartAiTitle'),
    message: t('sidebar.restartAiMsg'),
    confirmText: t('sidebar.restartConfirm'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  if (isAiInitialising.value) return
  isAiInitialising.value = true
  try {
    if (!ai.status.clipLoaded) await ai.initEngine()
    await ai.restartAnalysis()
  } finally {
    isAiInitialising.value = false
  }
}

const { elapsedStr: aiElapsedStr } = useElapsedTimer(
  () => ai.status.isAnalyzing,
  () => ai.status.totalItems > 0,
)

// ── 人脸识别控制（F5）────────────────────────────────────────────────────────
const isFaceStarting = ref(false)

// 兜底放宽：除 pending>0 外，「有总量但尚未全部处理完」也视为可开始——避免状态拉取时机
// 缺口（加文件夹后 pending 未刷新）导致「开始」按钮该现不现（问题1）。
const faceResumable = computed(
  () =>
    face.status.pendingItems > 0 ||
    (face.status.totalItems > 0 && face.status.processedItems < face.status.totalItems),
)
const facePaused = computed(
  () =>
    !face.status.isAnalyzing &&
    face.status.analysisActive &&
    face.status.pendingItems > 0 &&
    !face.waitingForSession,
)

async function startOrResumeFace() {
  if (isFaceStarting.value) return
  isFaceStarting.value = true
  try {
    await face.startAnalysis() // 引擎启动与就绪检查在后端
  } finally {
    isFaceStarting.value = false
  }
}

async function restartFaceAnalysis() {
  // 不同于 CLIP 重做（向量可重算），此操作销毁用户劳动——明确告知。
  const { confirmed } = await confirm({
    title: t('sidebar.restartFaceTitle'),
    message: t('sidebar.restartFaceMsg'),
    confirmText: t('sidebar.restartConfirm'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  if (isFaceStarting.value) return
  isFaceStarting.value = true
  try {
    await face.restartAnalysis()
  } finally {
    isFaceStarting.value = false
  }
}

// ── 状态刷新时机（问题1）──────────────────────────────────────────────────────
// faceStore/aiStore 仅运行中轮询；加文件夹或扫描后 pending 会变，需主动补刷一次，
// 否则「开始」按钮（依赖 pending/total）该现不现。
onMounted(() => {
  face.fetchStatus()
  ai.fetchStatus()
  // 视频派生计数(挂载即取:流水线可能正被自动踢跑,fetch 内含运行中自动起表轮询)。
  void derive.fetchVideoStatus()
})

// 扫描+enrichment 全部结束（isAnyScanRunning true→false）是 pending 真正变化的时机 → 补刷状态。
watch(
  () => scan.isAnyScanRunning,
  (running, prev) => {
    if (prev && !running) {
      face.fetchStatus()
      ai.fetchStatus()
    }
  },
)

// ── 共享的计时助手 ───────────────────────────────────────────────────────────
// 一个 rAF 时钟：`isRunning()` 为真时走动，`keepAfter()` 为真时保留最后的值
//（如已完成的任务）。返回 "Xm Y.ZZZs"。
function useElapsedTimer(isRunning: () => boolean, keepAfter: () => boolean) {
  const ms = ref(0)
  let startedAt: number | null = null
  let frame: number | null = null

  function tick() {
    if (startedAt != null && isRunning()) {
      ms.value = Date.now() - startedAt
      frame = requestAnimationFrame(tick)
    }
  }

  // immediate:挂载时任务可能已在运行(如切视图回来),否则要等下一次状态翻转才起表。
  watch(
    isRunning,
    (running) => {
      if (running) {
        startedAt = Date.now()
        ms.value = 0
        if (frame) cancelAnimationFrame(frame)
        frame = requestAnimationFrame(tick)
      } else {
        if (frame) {
          cancelAnimationFrame(frame)
          frame = null
        }
        if (startedAt != null) ms.value = Date.now() - startedAt
      }
    },
    { immediate: true },
  )

  onUnmounted(() => {
    if (frame) cancelAnimationFrame(frame)
  })

  const elapsedStr = computed(() => {
    if (ms.value === 0 && !isRunning() && !keepAfter()) return ''
    const total = ms.value
    const secs = Math.floor(total / 1000)
    const m = Math.floor(secs / 60)
    const s = secs % 60
    const msPart = String(total % 1000).padStart(3, '0')
    return `${m}m ${s}.${msPart}s`
  })

  return { elapsedStr }
}
</script>

<style scoped>
.tool-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px var(--spacing-sm) 8px;
  list-style: none;
  margin: 0;
}

/* ── Tool row (handle + card) ────────────────────────────────────────────── */
.tool {
  display: flex;
  align-items: stretch;
  gap: 4px;
  position: relative;
}
.tool__handle {
  display: flex;
  align-items: center;
  justify-content: center;
  /* 列表 padding 8 + 手柄 18 + gap 4 = 卡片左缘 30,对齐 --sidebar-indent 内容轨。 */
  width: 18px;
  padding: 0; /* 压掉 UA button 默认内边距(全局 reset 未清 padding) */
  flex-shrink: 0;
  color: var(--color-text-tertiary);
  cursor: grab;
  opacity: 0;
  transition: opacity var(--transition-fast);
}
.tool:hover .tool__handle {
  opacity: 0.6;
}
/* 键盘聚焦时手柄必须可见(平时 opacity:0 只对指针悬停现身)。 */
.tool__handle:focus-visible {
  opacity: 1;
}
.tool__handle:hover {
  opacity: 1;
}
.tool__handle:active {
  cursor: grabbing;
}

/* 悬停目标上方的落点指示线 */
.tool--drop::before {
  content: '';
  position: absolute;
  top: -5px;
  left: 0;
  right: 0;
  height: 2px;
  border-radius: 1px;
  background: var(--color-accent);
}

/* ── Card ─────────────────────────────────────────────────────────────────
   每个工具是一张卡片：可含标题行 + 进度 / 控件。整张卡片不可点击，仅内部控件可交互。 */
.tool__card {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm);
  background: var(--color-bg-surface);
  border: none;
  border-radius: var(--radius-lg);
  box-shadow: none;
}
.tool__card:hover {
  background: var(--color-bg-hover);
}
.tool__row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}
.ai-actions,
.thumb-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-2xs);
  flex-shrink: 0;
}
.tool__main {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 0;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.tool__icon {
  width: 20px;
  flex-shrink: 0;
  display: inline-flex;
  justify-content: center;
}
.tool__label {
  white-space: nowrap;
  /* 一律可截断:长语言(如德语)标签在窄侧栏会撑破卡片;.tool__main 已有 min-width:0,
     此处补 overflow 使 flex 收缩链完整生效。 */
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tool__label--ellipsis {
  flex: 1;
}

/* ── Progress ─────────────────────────────────────────────────────────────── */
.tool__progress {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.tool__progress-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
/* 视频派生卡的关键帧勾选行:紧凑小字,与卡内进度元字号一致。 */
.video-derive-keyframes {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  cursor: pointer;
  user-select: none;
}
.video-derive-keyframes input {
  accent-color: var(--color-accent);
  margin: 0;
}
.video-derive-keyframes input:disabled {
  cursor: not-allowed;
}

/* 等待原因行（可观测性三修 #3）：中性提示色，区别于下方失败面的错误色。 */
.tool__waiting-row {
  margin-top: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

/* 失败面行（审查 A11/F9）：错误色文本 + 行内重试按钮。 */
.tool__error-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  margin-top: var(--spacing-sm);
  font-size: var(--font-size-xs);
  color: var(--color-error);
}
.tool__retry-btn {
  margin-left: auto;
  font-size: var(--font-size-xs);
  padding: var(--spacing-2xs) var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-error);
  background: transparent;
  color: var(--color-error);
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.tool__retry-btn:hover {
  background: var(--color-error);
  color: var(--color-text-on-error);
}
.mono {
  font-family: var(--font-mono);
}
.push {
  margin-right: auto;
  margin-left: var(--spacing-sm);
}
.progress-bar {
  height: 3px;
  border-radius: var(--radius-xs);
  background: var(--color-border);
  overflow: hidden;
  margin-bottom: var(--spacing-xs);
}
.progress-bar__fill {
  height: 100%;
  border-radius: 2px;
  background: var(--color-accent);
  transition: width var(--duration-fast) linear;
}

.spin-anim {
  animation: spin var(--duration-spin) linear infinite;
}
</style>
