<template>
  <!-- 共享的、基于 Promise 的确认对话框。仅挂载一次;由 useConfirm() 驱动。
       外壳(Teleport/遮罩/标题栏/关闭键/焦点陷阱/Escape/点遮罩关闭)已收敛进 UiDialog 原语。 -->
  <UiDialog
    :open="state.isOpen"
    :title="state.title"
    :close-label="state.cancelText"
    float-surface
    @close="close(false)"
  >
    <p id="confirm-dialog-message" class="dialog-message">{{ state.message }}</p>

    <UiCheckbox
      v-if="state.showCheckbox"
      v-model="state.checkboxValue"
      :label="state.checkboxLabel"
    />

    <!-- 输入确认强门:极度危险且不可恢复的操作(如清库),须原样键入指定文本才启用确认按钮,
         杜绝反射式点击/键盘穿透。data-autofocus 落此输入(在正文、DOM 序早于页脚,焦点陷阱优先命中)。 -->
    <div v-if="state.requireText" class="confirm-require">
      <label :for="requireInputId" class="confirm-require__hint">
        {{ t('common.typeToConfirm', { word: state.requireText }) }}
      </label>
      <input
        :id="requireInputId"
        v-model="typed"
        type="text"
        class="confirm-require__input"
        autocomplete="off"
        autocorrect="off"
        spellcheck="false"
        :placeholder="state.requireText"
        data-autofocus
      />
    </div>

    <template #footer>
      <!-- 初始焦点落在「取消」(最不具破坏性的选项),避免键盘用户误触确认;
           requireText 场景下焦点改落输入框(见上 data-autofocus)。
           data-autofocus 由 UiDialog 的焦点陷阱跨插槽命中(useFocusTrap querySelector)。 -->
      <button class="btn btn-secondary" data-autofocus @click="close(false)">
        {{ state.cancelText }}
      </button>
      <button
        class="btn"
        :class="state.danger ? 'btn-danger' : 'btn-primary'"
        :disabled="confirmDisabled"
        @click="close(true)"
      >
        {{ state.confirmText }}
      </button>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfirmDialogState } from '../../composables/useConfirm'
import UiDialog from '../ui/UiDialog.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'

const { state, close } = useConfirmDialogState()
const { t } = useI18n()

const requireInputId = useId()
// 输入确认门的当前键入值;每次打开对话框都重置(上次残留不得延续放行)。
const typed = ref('')
watch(
  () => state.isOpen,
  (open) => {
    if (open) typed.value = ''
  },
)

// requireText 非空时,须精确键入该文本才启用确认按钮;否则(普通/danger 确认)恒启用。
const confirmDisabled = computed(() => !!state.requireText && typed.value !== state.requireText)
</script>

<style scoped>
/* 对话框外壳样式已迁至 UiDialog(header/title/关闭键/body/footer)+ 全局基座(overlay/content/keyframes);
   「记住选择」复选框已迁 UiCheckbox 原语;此处仅保留本对话框正文特有的消息段(保留 \n 换行)与输入确认门。
   注:该选择器作用于 UiDialog 的插槽内容,但插槽在本组件渲染作用域内编译、带本组件 data-v,scoped 依旧命中。 */
.dialog-message {
  margin: 0;
  font-size: var(--font-size-base);
  color: var(--color-text-secondary);
  line-height: 1.5;
  white-space: pre-line; /* 保留确认信息中的换行 */
}

/* 输入确认门:提示 + 文本框(危险操作强确认)。 */
.confirm-require {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  margin-top: var(--spacing-md);
}
.confirm-require__hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
.confirm-require__input {
  width: 100%;
  height: var(--control-size-default);
  min-height: var(--control-size-default);
  padding: 0 var(--control-padding-inline);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
}
.confirm-require__input:focus {
  outline: none;
  border-color: var(--color-input-border-focus);
  box-shadow: var(--control-focus-ring);
}
</style>
