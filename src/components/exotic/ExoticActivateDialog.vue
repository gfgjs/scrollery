<!-- 插件激活对话框（Part5 T12 增量3）：粘贴授权码 → 后端验签存 keyring。触点自持的聚焦弹窗。 -->
<!--
  前后端职责：本弹窗只把用户输入的 token 原样交后端；验签/存储全在后端
     （activate_exotic_plugin 内先验后存，失败不覆盖现有有效 token）。前端不解析、不校验 token。
-->
<template>
  <!-- 外壳迁 UiDialog:恒 Teleport + 焦点陷阱(此前仅聚焦 textarea、Tab 会逃逸)+ 点遮罩/Escape/关闭键三路统一走 onCancel。
       本对话框「常驻 + open 翻转」范式:父恒挂载、以 open prop 开阖,故 watch(open) 复位输入、焦点陷阱在 false→true 自然 engage。
       激活在途禁关闭:onCancel 内的 activating 守卫拦截遮罩/Escape/关闭键三路,态不错乱。 -->
  <UiDialog
    :open="open"
    :title="$t('exotic.activateTitle')"
    :close-label="$t('common.close')"
    max-width="460px"
    @close="onCancel"
  >
    <p class="dialog-message">
      {{
        featureName
          ? $t('exotic.activateDesc', { name: featureName })
          : $t('exotic.activateDescGeneric')
      }}
    </p>
    <textarea
      v-model="token"
      data-autofocus
      class="activate-token"
      :placeholder="$t('exotic.activateTokenPlaceholder')"
      rows="4"
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      @keydown.enter.exact.prevent="onSubmit"
    />
    <!-- 错误只回后端稳定 code（不含 token 材料），此处按 code 提示。 -->
    <p v-if="errorCode" class="activate-error">
      {{ $t('exotic.activateFailed', { code: errorCode }) }}
    </p>

    <template #footer>
      <UiButton variant="ghost" :disabled="activating" @click="onCancel">
        {{ $t('detail.close') }}
      </UiButton>
      <UiButton variant="primary" :disabled="!canSubmit" @click="onSubmit">
        {{ activating ? $t('exotic.activating') : $t('exotic.activateSubmit') }}
      </UiButton>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import { useExoticGate } from '../../composables/useExoticGate'
import { useToastStore } from '../../stores/toastStore'
import type { IpcError } from '../../utils/ipc'

interface Props {
  open: boolean
  /** 待激活插件 id（取自已解析的 entitlement，非用户任意输入）。 */
  pluginId: string
  /** 功能名（对话框文案用）。 */
  featureName?: string
  /** 内建 feature 可注入专用激活命令；缺省仍走 catalog 绑定的 exotic 激活命令。 */
  activationHandler?: (pluginId: string, token: string) => Promise<void>
}
const props = withDefaults(defineProps<Props>(), { featureName: '' })

const emit = defineEmits<{
  (e: 'close'): void
  /** 激活成功 → 父组件据此重解析授权态（关闭 gate）。 */
  (e: 'activated'): void
}>()

const { t } = useI18n()
const toast = useToastStore()
const gate = useExoticGate()

const token = ref('')
const errorCode = ref<string | null>(null)
const activating = ref(false)

// 非空且未在激活中才可提交（防重复提交）。
const canSubmit = computed(() => token.value.trim().length > 0 && !activating.value)

// 每次打开：清空上次输入/错误（组件常驻,须手动复位）。初始聚焦交 UiDialog 焦点陷阱——textarea 标了 data-autofocus。
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      token.value = ''
      errorCode.value = null
    }
  },
)

function onCancel() {
  if (activating.value) return // 激活在途不允许中途关闭，避免态错乱
  emit('close')
}

async function onSubmit() {
  if (!canSubmit.value) return
  errorCode.value = null
  activating.value = true
  try {
    const activate = props.activationHandler ?? gate.activate
    await activate(props.pluginId, token.value.trim())
    toast.addToast('success', t('exotic.activateSuccess'))
    emit('activated')
    emit('close')
  } catch (e) {
    // IpcError 带后端稳定 code（bad_token / no_sku / …）；无 code 兜底 'unknown'。
    errorCode.value = (e as IpcError)?.code ?? 'unknown'
  } finally {
    activating.value = false
  }
}
</script>

<style scoped>
/* 外壳(overlay/content/header/关闭键/footer/动画)由 UiDialog + 全局 Modal 基座提供,宽度 460px 经
   max-width prop 传入(基座默认 420px)。本组件只保留正文文案 + 授权码输入 + 错误提示三处特化。 */
.dialog-message {
  margin: 0;
  font-size: var(--font-size-base);
  color: var(--color-text-secondary);
  line-height: 1.5;
}

/* 授权码输入：等宽字体（token 多为 base64url），可换行不横向溢出。 */
.activate-token {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  min-height: 84px;
  padding: var(--spacing-sm) var(--spacing-md);
  font-family: var(--font-mono);
  font-size: var(--font-size-sm);
  line-height: 1.5;
  color: var(--color-text-primary);
  background: var(--color-input-bg);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  word-break: break-all;
}
.activate-token:focus {
  outline: none;
  border-color: var(--color-input-border-focus);
  box-shadow: var(--control-focus-ring);
}

.activate-error {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--color-error);
}
</style>
