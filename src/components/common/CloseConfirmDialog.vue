<template>
  <!-- 外壳(Teleport/遮罩/标题栏/关闭键/焦点陷阱/Escape/点遮罩关闭)已收敛进 UiDialog 原语;
       迁移顺带修掉此前漏用 Teleport 的隐患——原位渲染时 z-index 可被祖先 overflow/transform 层叠上下文裁剪。 -->
  <UiDialog
    :open="ui.showCloseConfirmDialog"
    :title="t('closeConfirm.title')"
    :close-label="t('common.cancel')"
    @close="cancel"
  >
    <p id="close-confirm-message" class="dialog-message">{{ t('closeConfirm.message') }}</p>

    <!-- flush 失败(设置未保存成功)时的裁决区:后端拒绝了退出,这里给出重试与明确放弃两个出口,
         不把未保存说成已保存,也不让弹窗卡死(方案 §5.4/§7「保存失败可重试或明确放弃」)。 -->
    <p v-if="flushFailed" class="dialog-message flush-failed" role="alert">
      {{ t('closeConfirm.flushFailed') }}
    </p>

    <UiCheckbox
      v-model="rememberChoice"
      :label="t('closeConfirm.remember')"
      class="close-remember-mt"
    />

    <template #footer>
      <template v-if="flushFailed">
        <button class="btn btn-secondary" data-autofocus :disabled="busy" @click="retryExit">
          {{ t('closeConfirm.retry') }}
        </button>
        <button class="btn btn-danger" :disabled="busy" @click="exitAnyway">
          {{ t('closeConfirm.exitAnyway') }}
        </button>
      </template>
      <template v-else>
        <!-- 初始焦点落在「最小化到托盘」(最不具破坏性的选项);data-autofocus 由 UiDialog 焦点陷阱跨插槽命中。 -->
        <button class="btn btn-secondary" data-autofocus :disabled="busy" @click="minimizeToTray">
          {{ t('closeConfirm.minimize') }}
        </button>
        <button class="btn btn-danger" :disabled="busy" @click="exitApp">
          {{ t('closeConfirm.exit') }}
        </button>
      </template>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useUiStore } from '../../stores/uiStore'
import { useToastStore } from '../../stores/toastStore'
import { flushPendingSettings } from '../../composables/useSettingsLifecycle'
import UiDialog from '../ui/UiDialog.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'

const { t } = useI18n()
const ui = useUiStore()
const toast = useToastStore()
const rememberChoice = ref(false)
/** 退出前 flush 未完成:后端拒绝了退出,弹窗转为「重试 / 放弃并退出」两态。 */
const flushFailed = ref(false)
/** 在途动作闸:避免重复点击灌出多次 EXIT_APP/hide。 */
const busy = ref(false)

// 弹窗每次打开都从干净状态开始:上一次退出尝试的 flush 失败结论只对那一次尝试有效。
// 不重置的话,用户点「取消」后再次关窗仍会看到「保存失败」告警,且只剩重试/放弃两个出口,
// 正常的「最小化到托盘」选项不会再出现(本组件常驻挂载,ref 不随弹窗关闭销毁)。
watch(
  () => ui.showCloseConfirmDialog,
  (open) => {
    if (open) flushFailed.value = false
  },
)

function cancel() {
  ui.showCloseConfirmDialog = false
}

async function minimizeToTray() {
  if (rememberChoice.value) {
    ui.setCloseBehavior('minimize_to_tray')
  }
  busy.value = true
  try {
    await invokeIpc(IPC.HIDE_WINDOW)
    ui.showCloseConfirmDialog = false
  } catch (e) {
    toast.addToast('error', t('closeConfirm.actionFailed', { error: ipcErrorMessage(e) }))
  } finally {
    busy.value = false
  }
}

/**
 * 退出:先把前端在途设置落盘,再请求后端退出。
 *
 * 后端 exit_app 现在也会等一次 flush(它要与窗口几何的待保存值一起收口),因此这里的前置 flush
 * 是**同一套协议的前端一侧**,不是重复劳动:写盘失败时后端会以 settings_flush_failed 拒绝退出,
 * 弹窗保持打开并进入重试/放弃两态,而不是静默失败。
 */
async function exitApp() {
  if (rememberChoice.value) {
    ui.setCloseBehavior('exit')
  }
  busy.value = true
  try {
    await flushPendingSettings()
  } catch {
    // 落盘失败:进入「重试 / 放弃并退出」两态,并给一次可见提示(不静默失败)。
    flushFailed.value = true
    toast.addToast('warning', t('closeConfirm.flushFailed'))
    busy.value = false
    return
  }
  try {
    // 成功即退出进程,后一行不可达;失败则弹窗保持打开并提示(P1-16)。
    await invokeIpc(IPC.EXIT_APP)
    ui.showCloseConfirmDialog = false
  } catch (e) {
    if (isFlushFailure(e)) {
      flushFailed.value = true
    } else {
      toast.addToast('error', t('closeConfirm.actionFailed', { error: ipcErrorMessage(e) }))
    }
  } finally {
    busy.value = false
  }
}

/** 重试:再走一轮「落盘 → 退出」。 */
async function retryExit() {
  flushFailed.value = false
  await exitApp()
}

/** 明确放弃未保存修改并退出:只有用户显式选择才走到这里(force 分支)。 */
async function exitAnyway() {
  busy.value = true
  try {
    await invokeIpc(IPC.EXIT_APP, { force: true })
    ui.showCloseConfirmDialog = false
  } catch (e) {
    toast.addToast('error', t('closeConfirm.actionFailed', { error: ipcErrorMessage(e) }))
  } finally {
    busy.value = false
  }
}

/** 后端「设置未保存成功」的稳定码(error.rs 的 Config { code: 'settings_flush_failed' })。 */
function isFlushFailure(e: unknown): boolean {
  return (e as { code?: string } | null)?.code === 'settings_flush_failed'
}
</script>

<style scoped>
/* 对话框外壳样式已迁至 UiDialog + 全局基座;「记住选择」复选框已迁 UiCheckbox 原语,此处仅余:
   正文消息段 + 复选框较 ConfirmDialog 多的 margin-top(与消息段拉开间距,经 class fallthrough 落到
   UiCheckbox 根 label,child-root 携本组件 data-v 仍命中)。scoped 作用于 UiDialog 插槽内容。 */
.dialog-message {
  margin: 0;
  font-size: var(--font-size-base);
  color: var(--color-text-secondary);
  line-height: 1.5;
}

.flush-failed {
  margin-top: var(--spacing-sm);
  color: var(--color-error);
}

.close-remember-mt {
  margin-top: var(--spacing-sm);
}
</style>
