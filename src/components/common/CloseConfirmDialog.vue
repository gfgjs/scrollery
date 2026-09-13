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

    <UiCheckbox
      v-model="rememberChoice"
      :label="t('closeConfirm.remember')"
      class="close-remember-mt"
    />

    <template #footer>
      <!-- 初始焦点落在「最小化到托盘」(最不具破坏性的选项);data-autofocus 由 UiDialog 焦点陷阱跨插槽命中。 -->
      <button class="btn btn-secondary" data-autofocus @click="minimizeToTray">
        {{ t('closeConfirm.minimize') }}
      </button>
      <button class="btn btn-danger" @click="exitApp">{{ t('closeConfirm.exit') }}</button>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useUiStore } from '../../stores/uiStore'
import { useToastStore } from '../../stores/toastStore'
import UiDialog from '../ui/UiDialog.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'

const { t } = useI18n()
const ui = useUiStore()
const toast = useToastStore()
const rememberChoice = ref(false)

function cancel() {
  ui.showCloseConfirmDialog = false
}

async function minimizeToTray() {
  if (rememberChoice.value) {
    ui.setCloseBehavior('minimize_to_tray')
  }
  // 成功后再关弹窗(P1-16):此前先关后 await,托盘失败则弹窗已消失、用户零反馈。
  try {
    await invokeIpc(IPC.HIDE_WINDOW)
    ui.showCloseConfirmDialog = false
  } catch (e) {
    toast.addToast('error', t('closeConfirm.actionFailed', { error: ipcErrorMessage(e) }))
  }
}

async function exitApp() {
  if (rememberChoice.value) {
    ui.setCloseBehavior('exit')
  }
  // 成功即退出进程,后一行不可达;失败则弹窗保持打开并提示(P1-16)。
  try {
    await invokeIpc(IPC.EXIT_APP)
    ui.showCloseConfirmDialog = false
  } catch (e) {
    toast.addToast('error', t('closeConfirm.actionFailed', { error: ipcErrorMessage(e) }))
  }
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

.close-remember-mt {
  margin-top: var(--spacing-sm);
}
</style>
