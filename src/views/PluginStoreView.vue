<!-- 插件商店（Part5 T11）：浏览签名 Registry 可安装条目 + 安装生命周期 + 处理进度。 -->
<!--
  前后端职责：本视图只调后端命令（列表 / 安装 / 激活 / 进度），
     验签 / 防回滚 / 完整性校验全在后端；命令只传已校验 pluginId，不碰下载坐标，不持验签逻辑。
  数据/动作经 useExoticStore；激活复用 ExoticActivateDialog。
-->
<template>
  <div class="plugin-store">
    <header class="ps-header">
      <div class="ps-header__text">
        <h2 class="ps-title">{{ $t('exotic.storeTitle') }}</h2>
        <p class="ps-subtitle">{{ $t('exotic.storeSubtitle') }}</p>
      </div>
      <button class="btn btn-primary" :disabled="refreshing" @click="onRefresh">
        <RefreshCw :size="15" :class="{ 'spin-anim': refreshing }" />
        {{ refreshing ? $t('exotic.storeRefreshing') : $t('exotic.storeRefresh') }}
      </button>
    </header>

    <!-- 目录过期横幅：仍可展示但不允许新装。 -->
    <div v-if="anyExpired" class="ps-banner ps-banner--warn">
      {{ $t('exotic.storeExpiredHint') }}
    </div>

    <!-- 加载/刷新失败横幅：避免静默空列表。 -->
    <div v-if="store.error.value" class="ps-banner ps-banner--error">
      {{ storeErrorText }}
    </div>

    <!-- 处理进度：有 exotic 任务时才显（进度条 + 计数 + 控制）。 -->
    <section v-if="proc && procTotal > 0" class="ps-proc">
      <div class="ps-proc__head">
        <span class="ps-proc__title">{{ $t('exotic.procTitle') }}</span>
        <div class="ps-proc__ctrls">
          <button
            v-if="!proc.running"
            class="btn btn-ghost btn-sm"
            :disabled="procBusy"
            @click="ctrl(store.startProcessing)"
          >
            <Play :size="14" /> {{ $t('exotic.procStart') }}
          </button>
          <button
            v-else
            class="btn btn-ghost btn-sm"
            :disabled="procBusy"
            @click="ctrl(store.pauseProcessing)"
          >
            <Pause :size="14" /> {{ $t('exotic.procPause') }}
          </button>
          <button
            class="btn btn-ghost btn-sm"
            :disabled="procBusy || (!proc.running && proc.processing === 0)"
            @click="ctrl(store.stopProcessing)"
          >
            <Square :size="14" /> {{ $t('exotic.procStop') }}
          </button>
        </div>
      </div>
      <div class="ps-proc__bar">
        <div class="ps-proc__bar-fill" :style="{ width: procDonePct + '%' }" />
      </div>
      <div class="ps-proc__meta">
        <span class="ps-proc__count">{{ proc.done }} / {{ procTotal }}</span>
        <span v-if="proc.blockedByAvailability > 0" class="ps-proc__blocked">
          {{ $t('exotic.procBlocked', { n: proc.blockedByAvailability }) }}
        </span>
        <span v-if="proc.error > 0" class="ps-proc__err"><X :size="12" /> {{ proc.error }}</span>
        <button class="ps-proc__toggle" @click="toggleDetails">
          <component :is="detailsOpen ? ChevronUp : ChevronDown" :size="13" />
          {{ detailsOpen ? $t('exotic.procDetailsHide') : $t('exotic.procDetails') }}
        </button>
      </div>

      <!-- 展开详情：文件级任务列表（筛选桶 + 活动优先排序 + 加载更多）。 -->
      <div v-if="detailsOpen" class="ps-proc__details">
        <div class="ps-proc__filters">
          <button
            v-for="b in bucketDefs"
            :key="b.key ?? 'all'"
            class="ps-filter"
            :class="{ 'ps-filter--active': detailBucket === b.key }"
            @click="setBucket(b.key)"
          >
            {{ b.label }}<span class="ps-filter__n">{{ b.count }}</span>
          </button>
        </div>
        <ul v-if="details.length" class="ps-proc__list">
          <li v-for="row in details" :key="row.itemId" class="ps-task">
            <span
              class="ps-task__dot"
              :class="'ps-task__dot--' + row.status"
              :title="statusText(row.status)"
            />
            <span
              class="ps-task__file"
              :title="(row.dirPath ? row.dirPath + '/' : '') + row.fileName"
              >{{ row.fileName }}</span
            >
            <span class="ps-task__fmt">{{ row.format.toUpperCase() }}</span>
            <span class="ps-task__path">{{ row.dirPath || '/' }}</span>
            <span
              v-if="row.lastErrorCode"
              class="ps-task__errcode"
              :title="row.lastErrorMessage ?? ''"
              >{{ row.lastErrorCode }}</span
            >
          </li>
        </ul>
        <p v-else class="ps-proc__empty">{{ $t('exotic.procDetailEmpty') }}</p>
        <div class="ps-proc__details-foot">
          <button
            v-if="canLoadMore"
            class="btn btn-ghost btn-sm"
            :disabled="detailsLoading"
            @click="loadMoreDetails"
          >
            {{ $t('exotic.procLoadMore') }}
          </button>
          <span v-if="details.length >= DETAIL_CAP" class="ps-proc__cap">
            {{ $t('exotic.procDetailCap', { n: DETAIL_CAP }) }}
          </span>
        </div>
      </div>
    </section>

    <!-- 首次加载占位:骨架卡片(S5),替代空转 spinner。 -->
    <div v-if="store.loading.value && rows.length === 0" class="ps-skeleton">
      <div v-for="i in 3" :key="i" class="skeleton-block ps-skeleton__card" />
    </div>

    <!-- 空态：无可装、无已装。 -->
    <div v-else-if="rows.length === 0" class="ps-empty">
      <PackageOpen :size="48" />
      <p class="ps-empty__title">{{ $t('exotic.storeEmpty') }}</p>
      <p class="ps-empty__hint">{{ $t('exotic.storeEmptyHint') }}</p>
    </div>

    <!-- 插件列表。 -->
    <div v-else class="ps-list">
      <div v-for="row in rows" :key="row.pluginId" class="ps-card">
        <div class="ps-card__icon"><Puzzle :size="22" /></div>

        <div class="ps-card__body">
          <div class="ps-card__title-row">
            <span class="ps-card__name">{{ row.name || row.pluginId }}</span>
            <span class="ps-badge" :class="'ps-badge--' + statusKey(row)">{{
              statusLabel(row)
            }}</span>
          </div>

          <div v-if="row.formats.length" class="ps-card__formats">
            <span v-for="f in row.formats" :key="f" class="ps-chip">{{ f.toUpperCase() }}</span>
          </div>

          <div class="ps-card__meta">
            <span v-if="row.installedVersion">
              {{ $t('exotic.version') }} {{ row.installedVersion }}
              <template v-if="row.upgradable && row.availableVersion">
                → {{ row.availableVersion }}</template
              >
            </span>
            <span v-else-if="row.availableVersion">
              {{ $t('exotic.version') }} {{ row.availableVersion }}
            </span>
            <span v-if="row.sku" class="ps-card__sku"><code>{{ row.sku }}</code></span>
          </div>
        </div>

        <div class="ps-card__actions">
          <span v-if="busy[row.pluginId]" class="ps-busy">
            <RefreshCw :size="14" class="spin-anim" />
          </span>
          <template v-else>
            <!-- 未安装 → 安装（目录过期时禁用）。 -->
            <button
              v-if="row.installState === null"
              class="btn btn-primary btn-sm"
              :disabled="!row.availableVersion || row.registryExpired"
              @click="onInstall(row.pluginId)"
            >
              <Download :size="14" /> {{ $t('exotic.install') }}
            </button>
            <template v-else>
              <!-- 可升级 → 升级（同 install 命令拉更高版本）。 -->
              <button
                v-if="row.upgradable"
                class="btn btn-primary btn-sm"
                :disabled="row.registryExpired"
                @click="onInstall(row.pluginId)"
              >
                <ArrowUpCircle :size="14" /> {{ $t('exotic.upgrade') }}
              </button>
              <!-- 损坏 → 修复。 -->
              <button
                v-if="row.installState === 'broken'"
                class="btn btn-primary btn-sm"
                @click="onRepair(row.pluginId)"
              >
                <Wrench :size="14" /> {{ $t('exotic.repair') }}
              </button>
              <!-- 已授权 → 状态标识；否则显激活入口（激活后即时切换，2026-07-05 内测）。 -->
              <span
                v-if="entitlements[row.pluginId] === 'authorized'"
                class="ps-activated"
              >
                <CheckCircle2 :size="14" /> {{ $t('exotic.activated') }}
              </span>
              <button
                v-else
                class="btn btn-ghost btn-sm"
                @click="activateTarget = row.pluginId"
              >
                <KeyRound :size="14" /> {{ $t('exotic.activateAction') }}
              </button>
              <!-- 卸载（危险，带确认 + 可选移除授权）。 -->
              <button class="btn btn-ghost btn-sm ps-danger" @click="onUninstall(row.pluginId)">
                <Trash2 :size="14" /> {{ $t('exotic.uninstall') }}
              </button>
            </template>
          </template>
        </div>
      </div>
    </div>

    <!-- 内置能力插件（T11：D-OCR-5——无安装包，直接走 license 门控，如 OCR）。 -->
    <section v-if="store.builtinOfferings.value.length" class="ps-builtin">
      <h3 class="ps-builtin__title">{{ $t('store.builtinTitle') }}</h3>
      <div class="ps-list">
        <div
          v-for="off in store.builtinOfferings.value"
          :key="off.format"
          class="ps-card"
        >
          <div class="ps-card__icon"><Puzzle :size="22" /></div>
          <div class="ps-card__body">
            <div class="ps-card__title-row">
              <span class="ps-card__name">{{ off.displayName || off.pluginId }}</span>
              <span
                v-if="off.availability === 'authorized'"
                class="ps-badge ps-badge--installed"
                >{{ $t('exotic.activated') }}</span
              >
            </div>
            <p v-if="off.availability === 'authorized'" class="ps-builtin__hint">
              {{ $t('store.builtinNoInstall') }}
            </p>
            <!-- 视频格式扩展子系统(design.md §3.4):FFmpeg 是进程边界外的独立可执行文件,不链接
                 进本应用(全链路无链接,连 LGPL 动态链接义务都不触发),但仍按 LGPL 精神展示声明 +
                 源码获取渠道。仅本插件卡片显示(硬编 pluginId 判断,数据驱动的 catalog 无 description
                 字段可扩,见 findings)。 -->
            <!-- 源码链钉死到具体 release tag(V7 项11),与 tools.rs BTBN_RELEASE_TAG 同步改——
                 该常量升级时须同时改这里的 href,否则用户点开的源码页与实际内置版本不一致。 -->
            <p v-if="off.pluginId === 'video-extended'" class="ps-builtin__ffmpeg-notice">
              {{ $t('store.videoExtendedFfmpegNotice') }}
              <a
                class="ps-builtin__ffmpeg-link"
                href="https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-07-24-13-32"
                target="_blank"
                rel="noopener noreferrer"
                >{{ $t('store.videoExtendedSourceLink') }}</a
              >
            </p>
          </div>
          <div class="ps-card__actions">
            <span v-if="off.pluginId ? busy[off.pluginId] : false" class="ps-busy">
              <RefreshCw :size="14" class="spin-anim" />
            </span>
            <template v-else>
              <span v-if="off.availability === 'authorized'" class="ps-activated">
                <CheckCircle2 :size="14" /> {{ $t('exotic.activated') }}
              </span>
              <template
                v-else-if="off.availability === 'availableUninstalled' || off.availability === 'licenseExpired'"
              >
                <button
                  v-if="off.pluginId"
                  class="btn btn-ghost btn-sm"
                  @click="activateTarget = off.pluginId"
                >
                  <KeyRound :size="14" /> {{ $t('exotic.activateAction') }}
                </button>
                <a
                  v-if="off.storeUrl"
                  class="btn btn-ghost btn-sm"
                  :href="off.storeUrl"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {{ $t('exotic.gateBuy') }}
                </a>
              </template>
              <!-- 兜底态(平台不支持/版本不兼容/已禁用等):禁用态徽章,不留空白操作区。 -->
              <span v-else class="ps-badge ps-badge--disabled">{{ $t('exotic.stateDisabled') }}</span>
            </template>
          </div>
        </div>
      </div>
    </section>

    <!-- 激活对话框：由某行「激活」触发。 -->
    <ExoticActivateDialog
      :open="activateTarget !== null"
      :plugin-id="activateTarget ?? ''"
      :feature-name="activateTarget ?? ''"
      @close="activateTarget = null"
      @activated="onActivated"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useTauriListen } from '../composables/useTauriListen'
import {
  RefreshCw,
  Puzzle,
  Download,
  ArrowUpCircle,
  Wrench,
  KeyRound,
  Trash2,
  PackageOpen,
  Play,
  Pause,
  Square,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  X,
} from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import ExoticActivateDialog from '../components/exotic/ExoticActivateDialog.vue'
import { useExoticStore, mergeStorePlugins, type StorePluginRow } from '../composables/useExoticStore'
import { resetOcrStatusCache } from '../composables/useOcr'
import { useToastStore } from '../stores/toastStore'
import { useConfirm } from '../composables/useConfirm'
import { IPC, EVENTS } from '../constants/ipc'
import type {
  Availability,
  ExoticTaskBucket,
  ExoticTaskDetail,
  ExoticTaskDetailStatus,
  PluginEntitlement,
} from '../types/exotic'
import { invokeIpc, type IpcError } from '../utils/ipc'

const { t } = useI18n()
const store = useExoticStore()
const toast = useToastStore()
const { confirm } = useConfirm()

// 合并 registry × installed 为展示行（纯函数，见 useExoticStore）。
const rows = computed<StorePluginRow[]>(() =>
  mergeStorePlugins(store.registry.value, store.installed.value),
)
const anyExpired = computed(() => rows.value.some((r) => r.registryExpired))

// 加载/刷新失败展示文案：优先后端 message，其次稳定 code，最后 i18n 兜底。
const storeErrorText = computed(() => {
  const e = store.error.value
  if (!e) return ''
  return (e as IpcError)?.message || (e as IpcError)?.code || t('exotic.storeLoadFailed')
})

// 处理进度（可能为 null=未取到）。
const proc = computed(() => store.status.value)
const procTotal = computed(() => {
  const s = proc.value
  return s ? s.pending + s.processing + s.done + s.error : 0
})
const procDonePct = computed(() =>
  procTotal.value > 0 ? Math.round(((proc.value?.done ?? 0) / procTotal.value) * 100) : 0,
)

// 每插件操作忙态（安装/卸载/修复期间禁用该行按钮并显 spinner）。
const busy = reactive<Record<string, boolean>>({})
const procBusy = ref(false)
const refreshing = ref(false)
const activateTarget = ref<string | null>(null)

// ── 授权态（2026-07-05 内测：激活后卡片须显「已激活」，而非永远的激活按钮）────────
// 按已装插件逐个取 entitlement；取失败（含 'no_offering' 预期分支）静默不显徽章、保留激活入口。
const entitlements = reactive<Record<string, Availability | undefined>>({})

async function loadEntitlements() {
  await Promise.all(
    store.installed.value.map(async (p) => {
      try {
        const e = await invokeIpc<PluginEntitlement>(IPC.GET_PLUGIN_ENTITLEMENT, {
          pluginId: p.pluginId,
        })
        entitlements[p.pluginId] = e.availability
      } catch {
        entitlements[p.pluginId] = undefined
      }
    }),
  )
}
// 已装列表每次重载（挂载 / 安装 / 卸载 / 修复 / 激活后）都换新数组引用 → 重取授权态。
watch(() => store.installed.value, loadEntitlements)

// ── 处理详情（2026-07-05 内测需求：展开查看处理了哪些冷门格式文件）──────────────
// 桶筛选与摘要四桶对齐；列表活动优先排序（后端定序）；「加载更多」按窗口整取
// （实时数据下 offset 追加会漂移，重取整窗幂等且封顶 DETAIL_CAP）。
const DETAIL_CAP = 200
const DETAIL_PAGE = 50
const detailsOpen = ref(false)
const detailBucket = ref<ExoticTaskBucket | null>(null)
const details = ref<ExoticTaskDetail[]>([])
const detailsLoading = ref(false)

const bucketDefs = computed(
  (): { key: ExoticTaskBucket | null; label: string; count: number }[] => [
    { key: null, label: t('exotic.procFilterAll'), count: procTotal.value },
    { key: 'pending', label: t('exotic.procStatusPending'), count: proc.value?.pending ?? 0 },
    {
      key: 'processing',
      label: t('exotic.procStatusProcessing'),
      count: proc.value?.processing ?? 0,
    },
    { key: 'done', label: t('exotic.procStatusDone'), count: proc.value?.done ?? 0 },
    { key: 'error', label: t('exotic.procStatusError'), count: proc.value?.error ?? 0 },
  ],
)

function statusText(s: ExoticTaskDetailStatus): string {
  return t('exotic.procStatus' + s.charAt(0).toUpperCase() + s.slice(1))
}

async function fetchDetails(windowSize?: number) {
  const size = windowSize ?? (details.value.length || DETAIL_PAGE)
  detailsLoading.value = true
  try {
    details.value = await invokeIpc<ExoticTaskDetail[]>(IPC.LIST_EXOTIC_TASK_DETAILS, {
      bucket: detailBucket.value,
      limit: Math.min(Math.max(size, DETAIL_PAGE), DETAIL_CAP),
      offset: 0,
    })
  } catch {
    // 详情为辅助面板：取失败静默保留旧数据（摘要区仍可用，避免 toast 噪音）。
  } finally {
    detailsLoading.value = false
  }
}

function toggleDetails() {
  detailsOpen.value = !detailsOpen.value
  if (detailsOpen.value && details.value.length === 0) void fetchDetails(DETAIL_PAGE)
}

function setBucket(b: ExoticTaskBucket | null) {
  if (detailBucket.value === b) return
  detailBucket.value = b
  details.value = []
  void fetchDetails(DETAIL_PAGE)
}

function loadMoreDetails() {
  void fetchDetails(details.value.length + DETAIL_PAGE)
}

const bucketTotal = computed(() => {
  const s = proc.value
  if (!s) return 0
  switch (detailBucket.value) {
    case 'pending':
      return s.pending
    case 'processing':
      return s.processing
    case 'done':
      return s.done
    case 'error':
      return s.error
    default:
      return procTotal.value
  }
})
const canLoadMore = computed(() => details.value.length < Math.min(bucketTotal.value, DETAIL_CAP))

// ── 进度实时化（2026-07-05 内测：后端每批进度都发 exotic:status-changed，此前无人监听，
//    进度区是挂载时刻的冻结快照）。事件高频（每批一发）→ 500ms 尾随节流。────────────
let statusTimer: ReturnType<typeof setTimeout> | null = null
function refreshStatusThrottled() {
  if (statusTimer) return
  statusTimer = setTimeout(() => {
    statusTimer = null
    void store.loadStatus()
    // 详情面板展开时同频刷新（重取当前窗口，列表随处理进展实时滚动）。
    if (detailsOpen.value) void fetchDetails()
  }, 500)
}

// EXOTIC_STATUS_CHANGED 监听解绑交由 useTauriListen(P1-11);onMounted 只留 loadAll。
useTauriListen(EVENTS.EXOTIC_STATUS_CHANGED, refreshStatusThrottled)

onMounted(() => {
  void (async () => {
    await store.loadAll()
    // 全新设备/缓存为空或目录已过期时，自动尝试拉取一次远程 Registry；
    // 失败不弹 toast，由顶部错误横幅展示，避免“空列表无提示”。
    const shouldAutoRefresh =
      store.registry.value.length === 0 || store.registry.value.some((r) => r.registryExpired)
    if (shouldAutoRefresh && !store.error.value) {
      try {
        await store.refreshRegistry()
        await store.loadAll()
      } catch (e) {
        store.error.value = e as IpcError
      }
    }
  })()
})
onBeforeUnmount(() => {
  if (statusTimer) clearTimeout(statusTimer)
})

// ── 状态标签 ────────────────────────────────────────────────────────────────
function statusKey(r: StorePluginRow): string {
  if (r.installState === 'broken') return 'broken'
  if (r.installState === 'disabled') return 'disabled'
  if (r.upgradable) return 'upgradable'
  if (r.installState) return 'installed'
  return 'installable'
}
function statusLabel(r: StorePluginRow): string {
  return t('exotic.state' + statusKey(r).charAt(0).toUpperCase() + statusKey(r).slice(1))
}

// ── 操作封装：忙态 + 成功/失败 toast（错误取后端稳定 code）─────────────────────
async function run(pluginId: string, fn: () => Promise<void>, okKey: string) {
  if (busy[pluginId]) return
  busy[pluginId] = true
  try {
    await fn()
    toast.addToast('success', t(okKey))
  } catch (e) {
    toast.addToast('error', t('exotic.opFailed', { code: (e as IpcError)?.code ?? 'unknown' }))
  } finally {
    busy[pluginId] = false
  }
}

function onInstall(pluginId: string) {
  void run(pluginId, () => store.install(pluginId), 'exotic.installedOk')
}
function onRepair(pluginId: string) {
  void run(pluginId, () => store.repair(pluginId), 'exotic.repairedOk')
}

async function onUninstall(pluginId: string) {
  const { confirmed, checkboxValue } = await confirm({
    title: t('exotic.uninstallTitle'),
    message: t('exotic.uninstallMsg'),
    confirmText: t('exotic.uninstall'),
    showCheckbox: true,
    checkboxLabel: t('exotic.uninstallRemoveLicense'),
    checkboxValue: false,
  })
  if (!confirmed) return
  void run(pluginId, () => store.uninstall(pluginId, checkboxValue), 'exotic.uninstalledOk')
}

// 激活成功 → 刷新已装/进度（授权态变化可能解阻处理）+ 内置能力区（T11）；
// resetOcrStatusCache 清 useOcr 的 60s 状态缓存，激活后立即反映新授权态。
function onActivated() {
  void store.loadInstalled()
  void store.loadStatus()
  void store.loadBuiltin()
  resetOcrStatusCache()
}

// ── 目录刷新 ────────────────────────────────────────────────────────────────
async function onRefresh() {
  if (refreshing.value) return
  refreshing.value = true
  try {
    const summary = await store.refreshRegistry()
    store.error.value = null
    toast.addToast('success', t('exotic.storeRefreshed', { count: summary.pluginCount }))
  } catch (e) {
    store.error.value = e as IpcError
    toast.addToast('error', t('exotic.storeRefreshFailed', { code: (e as IpcError)?.code ?? e }))
  } finally {
    refreshing.value = false
  }
}

// ── 处理控制（开始/暂停/停止）────────────────────────────────────────────────
async function ctrl(fn: () => Promise<void>) {
  if (procBusy.value) return
  procBusy.value = true
  try {
    await fn()
  } catch (e) {
    toast.addToast('error', t('exotic.opFailed', { code: (e as IpcError)?.code ?? 'unknown' }))
  } finally {
    procBusy.value = false
  }
}
</script>

<style scoped>
.plugin-store {
  height: 100%;
  overflow-y: auto;
  padding: var(--spacing-xl);
}

/* ── Header ───────────────────────────────────────────────────────────────── */
.ps-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--spacing-md);
  margin-bottom: var(--spacing-xl);
}
.ps-header__text {
  min-width: 0;
}
.ps-title {
  margin: 0;
  font-size: var(--font-size-xl);
  font-weight: 700;
  color: var(--color-text-primary);
}
.ps-subtitle {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}

.ps-banner {
  margin-bottom: var(--spacing-md);
  padding: var(--spacing-sm) var(--spacing-md);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-sm);
  line-height: 1.5;
}
.ps-banner--warn {
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}

.ps-banner--error {
  background: var(--color-error-subtle);
  color: var(--color-error);
}

/* ── Processing ───────────────────────────────────────────────────────────── */
.ps-proc {
  margin-bottom: var(--spacing-lg);
  padding: var(--spacing-md);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-lg);
  background: var(--color-bg-surface);
}
.ps-proc__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--spacing-sm);
}
.ps-proc__title {
  font-weight: 600;
  color: var(--color-text-primary);
}
.ps-proc__ctrls {
  display: flex;
  gap: var(--spacing-xs);
}
.ps-proc__bar {
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
  background: var(--color-bg-hover);
}
.ps-proc__bar-fill {
  height: 100%;
  background: var(--color-accent);
  transition: width var(--duration-fast) linear;
}
.ps-proc__meta {
  display: flex;
  gap: var(--spacing-md);
  margin-top: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.ps-proc__blocked {
  color: var(--color-warning);
}
.ps-proc__err {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-2xs);
  color: var(--color-error);
}
.ps-proc__toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-2xs);
  margin-left: auto;
  padding: var(--spacing-2xs) var(--spacing-xs);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  cursor: pointer;
}
.ps-proc__toggle:hover {
  background: var(--color-bg-hover);
}

/* ── 展开详情 ─────────────────────────────────────────────────────────────── */
.ps-proc__details {
  margin-top: var(--spacing-sm);
  border-top: 1px solid var(--color-border);
  padding-top: var(--spacing-sm);
}
.ps-proc__filters {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-xs);
  margin-bottom: var(--spacing-sm);
}
.ps-filter {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 24px;
  padding: 0 var(--spacing-sm);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  cursor: pointer;
}
.ps-filter:hover {
  background: var(--color-bg-hover);
}
.ps-filter--active {
  border-color: transparent;
  color: var(--color-accent-text);
  background: var(--color-accent-subtle);
}
.ps-filter__n {
  font-variant-numeric: tabular-nums;
  opacity: 0.75;
}
.ps-proc__list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 260px;
  overflow-y: auto;
}
.ps-task {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-xs) var(--spacing-2xs);
  font-size: var(--font-size-xs);
  border-bottom: 1px solid color-mix(in srgb, var(--color-border) 45%, transparent);
}
.ps-task:last-child {
  border-bottom: none;
}
.ps-task__dot {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--color-text-tertiary);
}
.ps-task__dot--retrying {
  background: var(--color-warning);
}
.ps-task__dot--processing {
  background: var(--color-accent);
}
.ps-task__dot--done {
  background: var(--color-success);
}
.ps-task__dot--error {
  background: var(--color-error);
}
.ps-task__file {
  color: var(--color-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 40%;
}
.ps-task__fmt {
  flex-shrink: 0;
  font-size: var(--font-size-2xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  padding: 1px var(--spacing-xs);
  border-radius: var(--radius-xs);
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}
.ps-task__path {
  flex: 1;
  min-width: 0;
  color: var(--color-text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ps-task__errcode {
  flex-shrink: 0;
  font-family: var(--font-mono);
  color: var(--color-error);
}
.ps-proc__empty {
  margin: var(--spacing-sm) 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.ps-proc__details-foot {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  margin-top: var(--spacing-xs);
}
.ps-proc__cap {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

/* ── Empty / loading ──────────────────────────────────────────────────────── */
.ps-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-2xl);
  color: var(--color-text-tertiary);
  text-align: center;
}
.ps-empty__title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-secondary);
  margin: 0;
}
.ps-empty__hint {
  margin: 0;
  font-size: var(--font-size-sm);
}
/* 首载骨架卡(S5,shimmer 基元见 animations.css .skeleton-block) */
.ps-skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  padding: var(--spacing-lg) var(--spacing-md);
}
.ps-skeleton__card {
  height: 88px;
  border-radius: var(--radius-md);
}

/* ── 内置能力插件区（T11）─────────────────────────────────────────────────── */
.ps-builtin {
  margin-top: var(--spacing-lg);
}
.ps-builtin__title {
  margin: 0 0 var(--spacing-sm);
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
}
.ps-builtin__hint {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
/* 视频格式扩展 FFmpeg/LGPL 声明(design.md §3.4):比 hint 更弱化的脚注语气,链接内联。 */
.ps-builtin__ffmpeg-notice {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-2xs);
  line-height: 1.5;
  color: var(--color-text-tertiary);
}
.ps-builtin__ffmpeg-link {
  color: var(--color-accent);
  text-decoration: underline;
}

/* ── Plugin cards ─────────────────────────────────────────────────────────── */
.ps-list {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}
.ps-card {
  display: flex;
  gap: var(--spacing-md);
  padding: var(--spacing-md);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-lg);
  background: var(--color-bg-surface);
}
.ps-card__icon {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: var(--radius-md);
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}
.ps-card__body {
  flex: 1;
  min-width: 0;
}
.ps-card__title-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex-wrap: wrap;
}
.ps-card__name {
  font-weight: 600;
  color: var(--color-text-primary);
  word-break: break-all;
}
.ps-card__formats {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-xs);
  margin-top: var(--spacing-xs);
}
.ps-chip {
  font-size: var(--font-size-2xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  padding: var(--spacing-2xs) var(--spacing-xs);
  border-radius: var(--radius-xs);
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}
.ps-card__meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-md);
  margin-top: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.ps-card__sku code {
  font-family: var(--font-mono);
}

.ps-badge {
  font-size: var(--font-size-2xs);
  font-weight: 600;
  line-height: 1;
  padding: var(--spacing-2xs) var(--spacing-xs);
  border-radius: var(--radius-full);
}
.ps-badge--installable {
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}
.ps-badge--installed {
  background: var(--color-success-subtle);
  color: var(--color-success);
}
.ps-badge--upgradable {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}
.ps-badge--disabled {
  background: var(--color-bg-hover);
  color: var(--color-text-tertiary);
}
.ps-badge--broken {
  background: var(--color-error-subtle);
  color: var(--color-error);
}

.ps-card__actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}
.btn-sm {
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-xs);
}
.ps-activated {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  min-height: 24px;
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-full);
  background: var(--color-success-subtle);
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--color-success);
}
.ps-danger {
  color: var(--color-error);
}
.ps-danger:hover {
  background: color-mix(in srgb, var(--color-error) 12%, transparent);
}
.ps-busy {
  display: inline-flex;
  color: var(--color-text-tertiary);
  padding: 0 var(--spacing-sm);
}

.spin-anim {
  animation: spin var(--duration-spin) linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
