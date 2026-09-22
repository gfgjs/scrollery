<!-- 插件授权 gate（Part5 T12）：包裹一个受授权门控的功能。已授权→渲染 slot（真实功能）；
     未授权但有产品→显功能说明 + 购买/激活引导；纯不可用→信息提示；不确定→放行。 -->
<!--
  前后端职责：本组件**纯展示**——授权判定全来自后端（经 `entitlement` prop 传入），
     组件不持任何验签逻辑。分类规则复用 composable 的 `gateModeFor`（单一事实源）。
  设计取舍：gate 做「展示型」（接 DTO、不自取数据），由各触点拥有 fetch 生命周期 → 单一职责、可复用。
-->
<template>
  <!-- 检查中：轻量占位，避免闪现购买引导。 -->
  <div v-if="loading" class="gate gate--muted">
    <span class="gate__spinner" />
    <span class="gate__muted-text">{{ $t('exotic.gateChecking') }}</span>
  </div>

  <div v-else-if="failed" class="gate gate--blocked">
    <span>{{ $t('official.queryFailed') }}</span>
    <button class="btn btn-ghost" @click="emit('retry')">{{ $t('official.retry') }}</button>
  </div>

  <!-- 放行 / 已授权：直接渲染被包裹的真实功能。 -->
  <slot v-else-if="mode === 'passthrough' || mode === 'authorized'" />

  <!-- 购买 / 激活引导：未授权但有产品可领。 -->
  <div v-else-if="mode === 'purchase'" class="gate gate--purchase">
    <div class="gate__icon">
      <Lock :size="22" />
    </div>
    <div class="gate__body">
      <div class="gate__title">{{ featureName || $t('exotic.gateTitle') }}</div>
      <p class="gate__desc">{{ featureDesc || $t('exotic.gateDescGeneric') }}</p>

      <div class="gate__actions">
        <!-- 购买：跳商店页；无链接则禁用并给出说明（不隐藏，保持可预期）。 -->
        <button
          class="btn btn-primary gate__buy"
          :disabled="!storeUrl || openingStore"
          :title="storeUrl ? undefined : $t('exotic.gateNoStore')"
          @click="openStore"
        >
          <ExternalLink :size="15" />
          {{ buyLabel }}
        </button>
        <!-- 已购买 → 激活：交给父组件处理激活流程（组件本身不碰密钥/token）。 -->
        <button class="btn btn-ghost gate__activate" @click="emit('activate')">
          <KeyRound :size="15" />
          {{ $t('exotic.gateActivate') }}
        </button>
      </div>
    </div>
  </div>

  <!-- 纯不可用（平台 / 版本 / 损坏 / 禁用）：只做信息提示，不引导购买。 -->
  <div v-else class="gate gate--blocked">
    <div class="gate__icon gate__icon--warn">
      <AlertTriangle :size="22" />
    </div>
    <div class="gate__body">
      <div class="gate__title">{{ $t('exotic.blockedTitle') }}</div>
      <p class="gate__desc">{{ blockedReason }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { AlertTriangle, ExternalLink, KeyRound, Lock } from '@lucide/vue'
import { openPurchase, purchaseUrl } from '../../utils/openPurchase'
import { useI18n } from 'vue-i18n'

import type { PluginEntitlement } from '../../types/exotic'
import { gateModeFor } from '../../composables/usePluginEntitlement'
import { useToastStore } from '../../stores/toastStore'

/**
 * Props：授权判定（后端已定）+ 可选的功能展示信息。
 * `entitlement` 为 null 时 gate 放行（不确定不藏功能）。
 */
interface Props {
  /** 后端授权判定 DTO（来自 `get_plugin_entitlement`）；null = 未取到 → 放行。 */
  entitlement: PluginEntitlement | null
  /** 判定尚在拉取中（显示轻量占位）。 */
  loading?: boolean
  failed?: boolean
  /** 受门控功能的人类可读名称（如「PSD 预览」）；缺省用通用标题。 */
  featureName?: string
  /** 功能说明（缺省用通用文案）。 */
  featureDesc?: string
}

const props = withDefaults(defineProps<Props>(), {
  loading: false,
  failed: false,
  featureName: '',
  featureDesc: '',
})

/** 用户点「已购买 · 激活」——父组件据此打开激活流程。 */
const emit = defineEmits<{ (e: 'activate'): void; (e: 'retry'): void }>()

const { t } = useI18n()
const toast = useToastStore()

// 渲染分支：复用 composable 的纯分类函数（单一事实源）。
const mode = computed(() => gateModeFor(props.entitlement))

const storeUrl = computed(() => purchaseUrl(props.entitlement?.storeUrl ?? null))

// 购买按钮文案随可用态微调：未安装→获取；已装未授权→购买授权；过期→续订。
const buyLabel = computed(() => {
  switch (props.entitlement?.availability) {
    case 'licenseExpired':
      return t('exotic.gateRenew')
    case 'installedUnlicensed':
      return t('exotic.gateBuyLicense')
    default:
      return t('exotic.gateBuy')
  }
})

// blocked 分支的具体原因文案（按后端 availability 精确指路）。
const blockedReason = computed(() => {
  switch (props.entitlement?.availability) {
    case 'unsupportedPlatform':
      return t('exotic.blockedUnsupportedPlatform')
    case 'incompatibleHost':
      return t('exotic.blockedIncompatibleHost')
    case 'invalidInstallation':
      return t('exotic.blockedInvalidInstallation')
    case 'disabled':
      return t('exotic.blockedDisabled')
    default:
      return t('exotic.blockedTitle')
  }
})

// 防重复点击（外部打开是异步的）。
const openingStore = ref(false)

/**
 * 打开购买 / 商店页。走已注册且已授权的 `shell:allow-open`（与 DocumentViewer 一致），
 * **不**用 `@tauri-apps/plugin-opener`——其 Rust 端未注册、capability 未授权，会撞 v2 ACL 拒绝。
 */
async function openStore() {
  const url = storeUrl.value
  if (!url || openingStore.value) return
  openingStore.value = true
  try {
    await openPurchase(url)
  } catch {
    toast.addToast('error', t('official.openFailed'))
  } finally {
    openingStore.value = false
  }
}
</script>

<style scoped>
/* gate 通用容器：卡片式，留白舒展，与设置卡风格一致。 */
.gate {
  display: flex;
  gap: var(--spacing-md);
  padding: var(--spacing-lg);
  border-radius: var(--radius-lg);
  border: 1px solid var(--color-border-subtle);
  background: var(--color-bg-surface);
}

/* 检查中：单行轻量占位。 */
.gate--muted {
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
}
.gate__muted-text {
  min-width: 0;
}
.gate__spinner {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 2px solid var(--color-border-strong);
  border-top-color: var(--color-accent);
  animation: gate-spin var(--duration-spin) linear infinite;
}
@keyframes gate-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 图标徽标：强调色底 + accent 前景。 */
.gate__icon {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  border-radius: var(--radius-md);
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}
.gate__icon--warn {
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}

.gate__body {
  min-width: 0;
  flex: 1;
}
.gate__title {
  font-weight: 700;
  font-size: var(--font-size-base);
  color: var(--color-text-primary);
}
.gate__desc {
  margin: var(--spacing-xs) 0 0;
  font-size: var(--font-size-sm);
  line-height: var(--leading-normal);
  color: var(--color-text-secondary);
}

/* 产品编号：等宽 code 徽章。 */
.gate__sku {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-sm);
  font-size: var(--font-size-xs);
}
.gate__sku-label {
  color: var(--color-text-tertiary);
}
.gate__sku-code {
  font-family: var(--font-mono);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}

.gate__actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-md);
}
/* 购买按钮：主色 CTA；无链接时禁用（半透明 + 禁止光标）。 */
.gate__buy:disabled {
  opacity: var(--opacity-disabled);
  cursor: not-allowed;
}
</style>
