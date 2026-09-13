<!-- AI 模型库：按「架构 → batch 变体」列出可下载/可切换的 ONNX。架构来自后端动态发现（新仓库）+ 静态 fp16 B/16。 -->
<template>
  <CollapsibleCard id="modelLibrary" :title="$t('settings.mlTitle')">
    <!-- 下载源选择：决定下载/重新下载时优先连接的服务器（失败自动回退另一源）。 -->
    <div class="ml-source">
      <div class="ml-source__info">
        <div class="ml-source__label">{{ $t('settings.mlSource') }}</div>
        <div class="ml-source__hint">{{ $t('settings.mlSourceDesc') }}</div>
      </div>
      <select class="ml-source__select" :value="config.aiDownloadSource" @change="onSourceChange">
        <option value="official">{{ $t('settings.mlSourceOfficial') }}</option>
        <option value="mirror">{{ $t('settings.mlSourceMirror') }}</option>
      </select>
    </div>

    <div v-if="loading" class="ml-loading">{{ $t('settings.mlLoading') }}</div>

    <template v-else>
      <!-- 在线发现失败时的离线提示：仍可在已安装变体间切换。 -->
      <div v-if="!online" class="ml-offline">
        {{ $t('settings.mlOfflineDesc') }}
      </div>

      <!-- 每个架构一组（可折叠），组内列出各 batch 变体。 -->
      <div
        v-for="arch in archs"
        :key="arch.id"
        class="ml-arch"
        :class="{ 'ml-arch--open': isOpen(arch.id) }"
      >
        <!-- 标题行：点击整行折叠/展开；chevron 旋转表达状态。 -->
        <div
          class="ml-arch__head"

          tabindex="0"

          @click="toggleArch(arch.id)"
          @keydown.enter.prevent="toggleArch(arch.id)"
          @keydown.space.prevent="toggleArch(arch.id)"
        >
          <ChevronRight
            :size="16"
            class="ml-arch__chevron"
            :class="{ expanded: isOpen(arch.id) }"
          />
          <div class="ml-arch__head-main">
            <div class="ml-arch__title">
              {{ arch.displayName }}
              <span v-if="arch.active" class="ml-badge ml-badge--active">{{
                $t('settings.mlActive')
              }}</span>
              <span v-if="arch.fp16" class="ml-badge ml-badge--ok">fp16</span>
            </div>
            <div class="ml-arch__meta">
              {{
                $t('settings.mlArchMeta', {
                  dim: arch.embedDim,
                  size: arch.imageSize,
                  mb: arch.sizeMb,
                  count: arch.variants.length,
                })
              }}
            </div>
          </div>
          <span v-if="archInstalled(arch)" class="ml-arch__pill">{{
            $t('settings.mlInstalledCount', { count: archInstalled(arch) })
          }}</span>
        </div>

        <!-- 折叠主体：grid 0fr↔1fr 平滑展开；内层 overflow:hidden。DOM 常驻，
             下载进度不因折叠而丢失。 -->
        <div class="ml-arch__body">
          <div class="ml-arch__body-inner">
            <div v-if="arch.description" class="ml-arch__desc">{{ arch.description }}</div>

            <div v-for="v in arch.variants" :key="v.imageFile" class="ml-item">
              <div class="ml-info">
                <div class="ml-name">
                  {{ variantLabel(v) }}
                  <span v-if="v.active" class="ml-badge ml-badge--active">{{
                    $t('settings.mlActive')
                  }}</span>
                  <span v-else-if="v.installed" class="ml-badge ml-badge--ok">{{
                    $t('settings.mlInstalled')
                  }}</span>
                </div>
                <div class="ml-meta">
                  {{ v.imageFile
                  }}<template v-if="v.sizeBytes">
                    · {{ $t('settings.mlApproxMb', { mb: fmtMB(v.sizeBytes) }) }}</template
                  >
                </div>

                <!-- 下载进度 -->
                <div v-if="dl[v.imageFile]" class="ml-progress">
                  <div class="ml-progress__track">
                    <div class="ml-progress__fill" :style="{ width: pct(v.imageFile) + '%' }"></div>
                  </div>
                  <div class="ml-progress__text">
                    {{ dl[v.imageFile]?.currentFile || $t('settings.mlPreparing') }} ·
                    {{ fmtMB(dl[v.imageFile]?.received || 0) }} /
                    {{ fmtMB(dl[v.imageFile]?.total || 0) }} MB ({{ pct(v.imageFile) }}%)
                    <span v-if="(dl[v.imageFile]?.fileCount || 0) > 0">
                      ·
                      {{
                        $t('settings.mlFileProgress', {
                          index: dl[v.imageFile]?.fileIndex,
                          count: dl[v.imageFile]?.fileCount,
                        })
                      }}
                    </span>
                  </div>
                </div>
              </div>

              <div class="ml-actions">
                <span v-if="dl[v.imageFile]" class="ml-busy">{{
                  $t('settings.mlDownloading')
                }}</span>
                <!-- 切换需重载引擎（大模型可能耗时数秒），显示即时「切换中…」状态。 -->
                <span v-else-if="switching === v.imageFile" class="ml-busy">{{
                  $t('settings.mlSwitching')
                }}</span>
                <template v-else>
                  <button
                    v-if="v.installed && !v.active"
                    class="btn btn-primary"
                    :disabled="!!switching"
                    @click="switchTo(v)"
                  >
                    {{ $t('settings.mlSwitch') }}
                  </button>
                  <button class="btn btn-secondary" :disabled="!!switching" @click="download(v)">
                    {{ v.installed ? $t('settings.mlRedownload') : $t('settings.mlDownload') }}
                  </button>
                </template>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>

    <div class="ml-foot">
      {{ $t('settings.mlFooter') }}
    </div>
  </CollapsibleCard>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { useAiStore } from '../../stores/aiStore'
import { useToastStore } from '../../stores/toastStore'
import { useConfigStore } from '../../stores/configStore'
import CollapsibleCard from './CollapsibleCard.vue'
import type { ModelArch, ModelVariant, ModelDownloadProgress } from '../../types/ai'
import { useI18n } from 'vue-i18n'

const ai = useAiStore()
const toast = useToastStore()
const config = useConfigStore()
const { t } = useI18n()

const archs = ref<ModelArch[]>([])
// 在线发现是否成功（false = 离线回退，仅显示已安装项）。
const online = ref(true)
const loading = ref(true)
// 每个变体 imageFile → 进行中的下载进度（undefined = 未在下载）。
const dl = reactive<Record<string, ModelDownloadProgress | undefined>>({})
// 正在切换的变体 imageFile（null = 无切换进行中）；驱动「切换中…」状态并禁用按钮。
const switching = ref<string | null>(null)

// 各架构分组的展开状态（key = arch.id）。刷新时保留用户已有的折叠选择。
const expanded = reactive<Record<string, boolean>>({})
let seededOnce = false
function isOpen(id: string): boolean {
  return !!expanded[id]
}
function toggleArch(id: string) {
  expanded[id] = !expanded[id]
}
// 默认展开「使用中」的架构；首次加载若无任一使用中，则展开第一组以免整面板全部收起。
// 仅为尚未出现过的 arch.id 设默认，避免刷新覆盖用户手动折叠的结果。
function seedExpand() {
  for (const a of archs.value) {
    if (!(a.id in expanded)) expanded[a.id] = a.active
  }
  if (!seededOnce) {
    seededOnce = true
    if (archs.value.length && !archs.value.some((a) => a.active)) {
      expanded[archs.value[0].id] = true
    }
  }
}

// 该架构下已安装的变体数（用于标题行右侧小标签）。
function archInstalled(arch: ModelArch): number {
  return arch.variants.filter((v) => v.installed).length
}

async function refresh() {
  loading.value = true
  try {
    const r = await ai.listModelRegistry()
    archs.value = r.archs
    online.value = r.online
    seedExpand()
  } catch (e) {
    toast.addToast('error', t('settings.mlLoadFailed', { error: e }))
  } finally {
    loading.value = false
  }
}
onMounted(async () => {
  // 确保下载源偏好已就绪（loadConfig 幂等；本组件可能先于父视图挂载）。
  await config.loadConfig()
  await refresh()
})

// 切换下载源：立即持久化并提示，下次下载/刷新即生效。
async function onSourceChange(e: Event) {
  const val = (e.target as HTMLSelectElement).value
  try {
    await config.setAiDownloadSource(val)
    toast.addToast(
      'success',
      val === 'mirror'
        ? t('settings.mlSourceSwitchedMirror')
        : t('settings.mlSourceSwitchedOfficial'),
    )
  } catch (err) {
    toast.addToast('error', t('settings.mlSourceSwitchFailed', { error: err }))
  }
}

// 变体显示名：动态 / 固定 batch=k / 默认（fp16 单一变体）。
function variantLabel(v: ModelVariant): string {
  if (v.batchKind === 'dynamic') return t('settings.mlVariantDynamic')
  if (v.batchKind === 'fixed') return t('settings.mlVariantFixed', { k: v.fixedBatch })
  return t('settings.mlVariantDefault')
}

function pct(id: string): number {
  const p = dl[id]
  if (!p || !p.total) return 0
  return Math.min(100, Math.round((p.received / p.total) * 100))
}
function fmtMB(bytes: number): string {
  return (bytes / 1048576).toFixed(1)
}

async function download(v: ModelVariant) {
  const id = v.imageFile
  // 预置占位进度（用 sizeBytes 估算 total），首个真实事件到达后即被替换。
  dl[id] = {
    modelId: id,
    currentFile: '',
    fileIndex: 0,
    fileCount: 0,
    received: 0,
    total: v.sizeBytes || 0,
    done: false,
    error: null,
  }
  try {
    await ai.downloadModel(id, (p) => {
      dl[id] = p
    })
    toast.addToast('success', t('settings.mlDownloadComplete', { name: variantLabel(v) }))
    await refresh()
  } catch (e) {
    toast.addToast('error', t('settings.mlDownloadFailed', { error: e }))
  } finally {
    dl[id] = undefined
  }
}

async function switchTo(v: ModelVariant) {
  if (switching.value) return
  // 立即置位 → 模板渲染「切换中…」并禁用按钮（切换会重载引擎，大模型耗时数秒）。
  switching.value = v.imageFile
  try {
    // setActiveModel 内部已 fetchStatus，会刷新 activeFixedBatch 供「AI 批处理大小」最小限制使用。
    await ai.setActiveModel(v.imageFile)
    toast.addToast('success', t('settings.mlSwitchedTo', { name: variantLabel(v) }))
    await refresh()
  } catch (e) {
    toast.addToast('error', t('settings.mlSwitchFailed', { error: e }))
  } finally {
    switching.value = null
  }
}
</script>

<style scoped>
/* 卡片内容统一左右内边距，与 .settings-card__header 对齐（避免内容贴边）。 */
.ml-loading {
  padding: var(--spacing-md) var(--spacing-lg);
  color: var(--color-text-tertiary);
}

/* ── 下载源选择 ─────────────────────────────────────────────────────────── */
.ml-source {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-md);
  padding: 0 var(--spacing-lg) var(--spacing-md);
  margin-bottom: var(--spacing-xs);
  border-bottom: 1px solid var(--color-border);
}
.ml-source__info {
  min-width: 0;
  flex: 1;
}
.ml-source__label {
  font-weight: 600;
  color: var(--color-text-primary);
}
.ml-source__hint {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-top: 2px;
}
.ml-source__select {
  flex-shrink: 0;
  padding: 6px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
}
.ml-source__select:hover {
  background: var(--color-bg-hover);
}

/* ── 离线提示 ───────────────────────────────────────────────────────────── */
.ml-offline {
  margin: var(--spacing-sm) var(--spacing-lg);
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-warning) 14%, transparent);
  color: var(--color-warning);
  font-size: var(--font-size-xs);
  line-height: 1.6;
}

/* ── 架构分组（可折叠）───────────────────────────────────────────────────── */
.ml-arch {
  border-top: 1px solid var(--color-divider);
}
.ml-arch:first-of-type {
  border-top: none;
}

/* 标题行：整行可点击，hover 通栏高亮（含左右内边距）。 */
.ml-arch__head {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-lg);
  cursor: pointer;
  user-select: none;
  transition: background var(--transition-fast);
}
.ml-arch__head:hover {
  background: var(--color-bg-hover);
}
.ml-arch__head:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
}
.ml-arch__chevron {
  flex-shrink: 0;
  color: var(--color-text-tertiary);
  transition: transform 0.2s;
}
.ml-arch__chevron.expanded {
  transform: rotate(90deg);
}
.ml-arch__head-main {
  min-width: 0;
  flex: 1;
}
.ml-arch__title {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-weight: 700;
  color: var(--color-text-primary);
}
.ml-arch__meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-top: 2px;
}
.ml-arch__pill {
  flex-shrink: 0;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 8px;
  border-radius: 999px;
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}

/* 折叠动画：grid 行高 0fr↔1fr，内层裁剪溢出。 */
.ml-arch__body {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--duration-moderate) var(--ease-in-out);
}
.ml-arch--open .ml-arch__body {
  grid-template-rows: 1fr;
}
.ml-arch__body-inner {
  overflow: hidden;
  min-width: 0;
  /* 子项整体缩进到主标题文字右侧，形成层级感：
     头部内边距(--spacing-lg 24) + chevron(16) + 间隙(--spacing-sm 8) = 标题文字位 48px，
     再 +--spacing-sm(8) 使子项比标题更靠右。 */
  padding-left: calc(var(--spacing-lg) + 16px + var(--spacing-sm) + var(--spacing-sm));
}
.ml-arch__desc {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  padding: 0 var(--spacing-lg) var(--spacing-sm) 0;
}

.ml-item {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--spacing-md);
  padding: var(--spacing-sm) var(--spacing-lg) var(--spacing-sm) 0;
  border-top: 1px solid var(--color-divider);
}

.ml-info {
  min-width: 0;
  flex: 1;
}
.ml-name {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-weight: 600;
  color: var(--color-text-primary);
}
.ml-meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  margin-top: 4px;
  word-break: break-all;
}

.ml-badge {
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 3px 7px;
  border-radius: 999px;
}
.ml-badge--active {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.ml-badge--ok {
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}

.ml-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex-shrink: 0;
}
.ml-busy {
  font-size: var(--font-size-sm);
  color: var(--color-text-tertiary);
}

.ml-progress {
  margin-top: 8px;
}
.ml-progress__track {
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
  background: var(--color-bg-hover);
}
.ml-progress__fill {
  height: 100%;
  background: var(--color-accent);
  transition: width var(--duration-fast) linear;
}
.ml-progress__text {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  margin-top: 4px;
}

.ml-foot {
  margin-top: var(--spacing-xs);
  padding: var(--spacing-md) var(--spacing-lg);
  border-top: 1px solid var(--color-border);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  line-height: 1.6;
}
</style>
