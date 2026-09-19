<!-- src/components/media/player/VideoPreparingOverlay.vue -->
<!-- 视频格式扩展子系统 · 播放准备覆盖层(design.md §5.3)。单一职责:据 useVideoSource 的
     非直播态渲染对应引导卡片,不持任何解析/IPC 逻辑——事件全经 emit 交给 ContentViewer/
     useVideoSource 处理。挂在 VideoPlayer 之上(ContentViewer 层),`mode` 为 direct/derived
     时父组件不渲染本组件(由 v-if 控制),故本组件内部不必再判这两态。 -->
<template>
  <div class="vpo" @mousedown.stop @click.stop @contextmenu.stop>
    <!-- 准备中:remux/半转码/全转码进度(§5.3 每 ≤2s 一帧;stage=finalize 时展示「正在收尾」)。 -->
    <div v-if="mode === 'preparing'" class="vpo-card">
      <span class="vpo-spinner" />
      <p class="vpo-title">{{ stageLabel }}</p>
      <div v-if="percent !== null" class="vpo-bar">
        <div class="vpo-bar__fill" :style="{ width: percent + '%' }" />
      </div>
      <p v-if="percent !== null" class="vpo-percent">{{ percent }}%</p>
      <button type="button" class="vpo-btn vpo-btn--ghost" @click="emit('cancel')">
        {{ t('player.prep.cancel') }}
      </button>
    </div>

    <!-- 取消后不留白(V7 项5):显式给出重新准备入口,而非静默退回空白视频区。 -->
    <div v-else-if="mode === 'cancelled'" class="vpo-card">
      <p class="vpo-title">{{ t('player.prep.cancelledTitle') }}</p>
      <button
        type="button"
        class="vpo-btn vpo-btn--primary"
        :disabled="localPending"
        @click="onCancelledRetryClick"
      >
        {{ t('player.prep.cancelledRetry') }}
      </button>
    </div>

    <!-- 组件未就绪:引导下载 FFmpeg 视频扩展组件。 -->
    <div v-else-if="mode === 'needsComponent'" class="vpo-card">
      <p class="vpo-title">{{ t('player.prep.needsComponentTitle') }}</p>
      <p class="vpo-hint">{{ t('player.prep.needsComponentHint') }}</p>
      <button
        type="button"
        class="vpo-btn vpo-btn--primary"
        :disabled="downloading"
        @click="emit('download')"
      >
        {{ downloading ? t('player.prep.downloading') : t('player.prep.downloadAction') }}
      </button>
    </div>

    <!-- 系统缺 HEVC 解码器(前端实测已排除可解情形):引导装扩展 / 改本地转码。 -->
    <div v-else-if="mode === 'needsHevcExt'" class="vpo-card">
      <p class="vpo-title">{{ t('player.prep.needsHevcTitle') }}</p>
      <p class="vpo-hint">{{ t('player.prep.needsHevcHint') }}</p>
      <div class="vpo-actions">
        <a
          class="vpo-btn vpo-btn--primary"
          :href="HEVC_EXTENSION_STORE_URL"
          target="_blank"
          rel="noopener noreferrer"
        >
          {{ t('player.prep.installHevcExt') }}
        </a>
        <button type="button" class="vpo-btn vpo-btn--ghost" @click="emit('transcodeInstead')">
          {{ t('player.prep.useLocalTranscode') }}
        </button>
      </div>
    </div>

    <!-- 产物预估超池 50%:确认框(放行/放弃,§5.4)。 -->
    <div v-else-if="mode === 'needsConfirm'" class="vpo-card">
      <p class="vpo-title">{{ t('player.prep.needsConfirmTitle') }}</p>
      <p class="vpo-hint">{{ t('player.prep.needsConfirmHint', { size: estimateLabel }) }}</p>
      <div class="vpo-actions">
        <button
          type="button"
          class="vpo-btn vpo-btn--primary"
          :disabled="localPending"
          @click="onConfirmClick"
        >
          {{ t('player.prep.proceed') }}
        </button>
        <button type="button" class="vpo-btn vpo-btn--ghost" @click="emit('cancel')">
          {{ t('player.prep.giveUp') }}
        </button>
      </div>
    </div>

    <!-- 终态失败(IPC 异常 / worker 失败等):稳定 code + 重试。 -->
    <div v-else-if="mode === 'error'" class="vpo-card">
      <p class="vpo-title">{{ t('player.prep.errorTitle') }}</p>
      <p v-if="errorCode" class="vpo-hint vpo-code">{{ errorCode }}</p>
      <button
        type="button"
        class="vpo-btn vpo-btn--primary"
        :disabled="localPending"
        @click="onRetryClick"
      >
        {{ t('player.prep.retryAction') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
// 覆盖层引导展示层:纯 props in / emit out,复杂状态机全归 useVideoSource composable。
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { formatFileSize } from '../../../utils/format'
import type { VideoSourceMode } from '../../../composables/player/useVideoSource'

/** Windows「HEVC 视频扩展」商店项(微软官方扩展,系统层解码,责任在 OS 层——design §3.4)。
 * productid 待真机验证(V7 项9):当前 `9n4wgh0z6vhq` 为免费版「HEVC 视频扩展」常见 productid;
 * 微软账号区域/商店后台变更可能使其失效或需换成付费版 `9NMZLZ57R3T7`——GUI 手测清单须核实
 * 该链接在真机 Microsoft Store 客户端能正确落地到对应商品页,而非泛化到搜索结果/404。 */
const HEVC_EXTENSION_STORE_URL = 'ms-windows-store://pdp/?productid=9n4wgh0z6vhq'

const props = defineProps<{
  /** 非 direct/derived 的引导态;父组件按此值 v-if 挂载/卸载本组件。 */
  mode: VideoSourceMode
  /** preparing 态百分比(0-100);未知时 null(仅显 spinner + 阶段文案)。 */
  percent: number | null
  /** preparing 态 worker 阶段串(如 remux/transcode/finalize)。 */
  stage: string | null
  /** needsConfirm 态的单文件产物字节预估。 */
  estimateBytes: number | null
  /** error 态稳定错误 code(不含内部串)。 */
  errorCode: string | null
  /** needsComponent 态:组件下载/安装中(禁用按钮 + 文案切换)。 */
  downloading: boolean
}>()

const emit = defineEmits<{
  (e: 'cancel'): void
  (e: 'confirm'): void
  (e: 'download'): void
  (e: 'retry'): void
  /** needsHevcExt「改用本地转码」:语义按后端 confirm 契约(design §5.3 开放问题 4)。 */
  (e: 'transcodeInstead'): void
}>()

const { t } = useI18n()

const mode = computed(() => props.mode)
const percent = computed(() => props.percent)
const downloading = computed(() => props.downloading)
const errorCode = computed(() => props.errorCode)

const stageLabel = computed(() =>
  props.stage === 'finalize' ? t('player.prep.stageFinalize') : t('player.prep.stagePreparing'),
)
const estimateLabel = computed(() =>
  props.estimateBytes !== null ? formatFileSize(props.estimateBytes) : '—',
)

// ── 「继续生成」/「重试」pending 禁用(V7 项8,仿 needsComponent 的 downloading 守卫)────────
// 覆盖层本身不持解析逻辑、拿不到 IPC 是否在途,故用「点击后立即禁用,mode 变化(意味着一轮
// resolve 已有结果落地)才复位」的本地状态,防抖连点造成重复 IPC 调用。
const localPending = ref(false)
watch(
  () => props.mode,
  () => {
    localPending.value = false
  },
)
function onConfirmClick(): void {
  if (localPending.value) return
  localPending.value = true
  emit('confirm')
}
function onRetryClick(): void {
  if (localPending.value) return
  localPending.value = true
  emit('retry')
}
function onCancelledRetryClick(): void {
  if (localPending.value) return
  localPending.value = true
  emit('retry')
}
</script>

<style scoped>
/* 覆盖层容器:铺满宿主(ContentViewer 的 .detail-viewer 相对定位),半透明遮罩托底卡片居中。 */
.vpo {
  position: absolute;
  inset: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--color-bg-surface) 55%, transparent);
}

.vpo-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  max-width: 360px;
  padding: var(--spacing-lg) var(--spacing-xl);
  border-radius: var(--radius-xl);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  text-align: center;
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
}

.vpo-title {
  margin: 0;
  font-weight: 600;
  font-size: var(--font-size-base);
  color: var(--color-text-primary);
}
.vpo-hint {
  margin: 0;
  font-size: var(--font-size-sm);
  line-height: var(--leading-normal);
  color: var(--color-text-secondary);
}
.vpo-code {
  font-family: var(--font-mono);
  color: var(--color-text-tertiary);
}

.vpo-spinner {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 2px solid var(--color-border-strong);
  border-top-color: var(--color-accent);
  animation: vpo-spin var(--duration-spin) linear infinite;
}
@keyframes vpo-spin {
  to {
    transform: rotate(360deg);
  }
}

.vpo-bar {
  width: 100%;
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
  background: var(--color-bg-hover);
}
.vpo-bar__fill {
  height: 100%;
  background: var(--color-accent);
  transition: width var(--duration-fast) linear;
}
.vpo-percent {
  margin: 0;
  font-size: var(--font-size-xs);
  font-variant-numeric: tabular-nums;
  color: var(--color-text-tertiary);
}

.vpo-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
}

.vpo-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-xs);
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  font-size: var(--font-size-sm);
  font-weight: 600;
  cursor: pointer;
  text-decoration: none;
}
.vpo-btn--primary {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.vpo-btn--ghost {
  background: transparent;
  border-color: var(--color-border);
  color: var(--color-text-secondary);
}
.vpo-btn--ghost:hover {
  background: var(--color-bg-hover);
}
.vpo-btn:disabled {
  opacity: var(--opacity-disabled, 0.5);
  cursor: not-allowed;
}
</style>
