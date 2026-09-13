<template>
  <!-- 外壳迁 UiDialog:恒 Teleport(修此前原位渲染被祖先 overflow/transform 裁剪的隐患)+
       焦点陷阱(此前仅 focus 遮罩、Tab 会逃逸;初始焦点由 data-autofocus 落到文件夹名输入)。
       本组件由父 v-if 条件挂载,故 open 恒 true;焦点陷阱经 useFocusTrap immediate 挂载即 engage。 -->
  <UiDialog
    :open="true"
    :title="isGlobal ? t('folderCreate.titleGlobal') : t('sidebar.newSubfolder')"
    :close-label="t('common.cancel')"
    @close="cancel"
  >
    <UiField v-if="isGlobal" :label="t('folderCreate.basePath')" associate="for" v-slot="{ id }">
      <div class="base-path-row">
        <input
          :id="id"
          type="text"
          class="input-text base-path-input"
          v-model="selectedBasePath"
          readonly
          :placeholder="t('folderCreate.basePathPlaceholder')"
        />
        <UiButton variant="secondary" @click="selectBasePath">
          {{ t('folderCreate.choose') }}
        </UiButton>
      </div>
    </UiField>
    <UiField v-else :label="t('folderCreate.parentPath')">
      <input type="text" class="input-text" :value="basePath" readonly disabled />
    </UiField>

    <UiField :label="t('folderCreate.folderName')">
      <input
        type="text"
        class="input-text"
        v-model="folderName"
        :placeholder="t('folderCreate.folderNamePlaceholder')"
        data-autofocus
        @keyup.enter="create"
      />
    </UiField>

    <div v-if="errorMessage" class="error-message">
      {{ errorMessage }}
    </div>

    <template #footer>
      <UiButton variant="secondary" @click="cancel">{{ t('common.cancel') }}</UiButton>
      <!-- 不用 :disabled——原生 disabled 按钮不在 Tab 焦点序,键盘用户无法聚焦到「创建」(真机反馈)。
           改为恒可聚焦,提交时在 create() 内校验并给出行级错误反馈。 -->
      <UiButton variant="primary" @click="create">
        {{ t('folderCreate.create') }}
      </UiButton>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useToastStore } from '../../stores/toastStore'
import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import UiField from '../ui/UiField.vue'

const props = defineProps<{
  basePath: string // 空则为全局创建
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'created'): void
}>()

const { t } = useI18n()
const toast = useToastStore()

const isGlobal = computed(() => !props.basePath)
const selectedBasePath = ref(props.basePath || '')
const folderName = ref('')
const errorMessage = ref('')

const canCreate = computed(() => {
  return selectedBasePath.value.trim() !== '' && folderName.value.trim() !== ''
})

async function selectBasePath() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('folderCreate.chooseBaseDir'),
    })
    if (selected) {
      selectedBasePath.value = typeof selected === 'string' ? selected : selected[0]
      errorMessage.value = ''
    }
  } catch (e) {
    errorMessage.value = t('sidebar.chooseDirFailed', { error: String(e) })
  }
}

function cancel() {
  emit('close')
}

async function create() {
  // 校验前置(替代按钮 disabled):缺项时给行级错误反馈而非静默无操作,键盘用户可感知原因。
  if (!canCreate.value) {
    errorMessage.value = !folderName.value.trim()
      ? t('folderCreate.nameRequired')
      : t('folderCreate.basePathRequired')
    return
  }
  errorMessage.value = ''

  try {
    await invokeIpc(IPC.CREATE_PHYSICAL_FOLDER, {
      basePath: selectedBasePath.value,
      folderName: folderName.value.trim(),
    })
    toast.addToast('success', t('folderCreate.createSuccess'))
    emit('created')
    emit('close')
  } catch (e) {
    errorMessage.value = String(e)
  }
}
</script>

<style scoped>
/* 外壳(overlay/content/header/title/关闭键/body/footer/动画)由 UiDialog + 全局 Modal 基座提供;
   字段 label 关联/朝向由 UiField 原语提供(原 .form-group + label 已删)。本组件只保留输入框视觉、
   基路径行布局、表单级错误特化;作用于插槽内容(插槽带本组件 data-v,scoped 仍命中)。 */
.input-text {
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
.input-text:disabled,
.input-text[readonly] {
  background: var(--color-bg-secondary);
  color: var(--color-text-tertiary);
}

/* 基路径行:输入占满 + 右侧「选择」按钮(原内联 style 收进 class,S1 机械批) */
.base-path-row {
  display: flex;
  gap: var(--spacing-sm);
}
.base-path-input {
  flex: 1;
}

.error-message {
  font-size: var(--font-size-sm);
  color: var(--color-error);
  margin-top: var(--spacing-xs);
}
</style>
