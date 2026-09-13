<template>
  <!-- path-based 只读文本预览(问题②方案 B v1,D-001/D-002)。
       仅纯文本白名单文件可达此处;内容渲染进 <pre> 文本节点——无 HTML 注入面,markdown 不渲染。 -->
  <UiDialog
    :open="request !== null"
    :title="request?.fileName ?? ''"
    :close-label="t('common.close')"
    max-width="720px"
    max-height="84vh"
    body-padding="0"
    @close="emit('close')"
  >
    <div v-if="loading" class="preview-status">{{ t('common.loading') }}</div>
    <div v-else-if="errorKey" class="preview-status preview-status--error">{{ t(errorKey) }}</div>
    <template v-else>
      <!-- 截断提示先于正文,滚到底才发现被截会误当「文件就这么长」。 -->
      <div v-if="truncated" class="preview-truncated">{{ t('sidebar.previewTruncated') }}</div>
      <pre class="preview-content">{{ content }}</pre>
    </template>

    <template #footer>
      <!-- 逃生口:预览满足不了(要编辑/看全文)时去文件管理器。移动端 opener 不支持 reveal
           (D-001),按钮整个不渲染,与树内双击的门同姿态。 -->
      <UiButton v-if="!isMobilePlatform && request" variant="secondary" @click="revealInManager">
        {{ t('sidebar.previewReveal') }}
      </UiButton>
      <UiButton variant="primary" @click="emit('close')">{{ t('common.close') }}</UiButton>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
// 预览弹层自持取数(loading/error/内容),父组件(FoldersSection)只喂 request 三元组——
// 树侧代码不为预览增加状态机。request 换代时用代际守卫丢弃迟到响应(快速连续双击两个文件,
// 慢的那个先发后至会覆盖新内容,同 useFolderTree loadingId 姿态)。
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiDialog from '../../ui/UiDialog.vue'
import UiButton from '../../ui/UiButton.vue'
import { invokeIpc } from '../../../utils/ipc'
import { logger } from '../../../utils/logger'
import { IPC } from '../../../constants/ipc'
import { isMobilePlatform } from '../../../utils/platform'
import { useToastStore } from '../../../stores/toastStore'

/** 预览目标:三元组全部取自后端产出的 DirFile/父节点字段,前端不推导(D-013)。 */
export interface FilePreviewRequest {
  fileName: string
  rootId: number
  relPath: string
}

const props = defineProps<{
  request: FilePreviewRequest | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const { t } = useI18n()
const toast = useToastStore()

const loading = ref(false)
const content = ref('')
const truncated = ref(false)
const errorKey = ref<string | null>(null)

let fetchGeneration = 0

watch(
  () => props.request,
  async (req) => {
    fetchGeneration++
    if (!req) return
    const myGeneration = fetchGeneration
    loading.value = true
    content.value = ''
    truncated.value = false
    errorKey.value = null
    try {
      const got = await invokeIpc<{ content: string; truncated: boolean }>(
        IPC.GET_TREE_TEXT_PREVIEW,
        { rootId: req.rootId, relPath: req.relPath },
      )
      if (myGeneration !== fetchGeneration) return // 已换目标/已关闭,丢弃迟到响应
      content.value = got.content
      truncated.value = got.truncated
    } catch (err) {
      if (myGeneration !== fetchGeneration) return
      logger.error('[FilePreviewDialog] preview failed', { error: err })
      // 按稳定 code 分流(R-08 姿态,匹配 message 文案是脆弱耦合)。unsupported_type 正常
      // 到不了这里(前端 isTextPreviewable 预筛过),收到即为兜底,仍给专属文案。
      const code = (err as { code?: string } | null)?.code
      errorKey.value =
        code === 'preview_unsupported_type'
          ? 'sidebar.previewUnsupportedType'
          : 'sidebar.previewFailed'
    } finally {
      if (myGeneration === fetchGeneration) loading.value = false
    }
  },
)

function revealInManager() {
  const req = props.request
  if (!req) return
  invokeIpc(IPC.REVEAL_TREE_ENTRY, { rootId: req.rootId, relPath: req.relPath }).catch((err) => {
    logger.error('[FilePreviewDialog] reveal failed', { error: err })
    const code = (err as { code?: string } | null)?.code
    toast.addToast(
      'error',
      t(code === 'unsupported_platform' ? 'sidebar.revealUnsupported' : 'sidebar.revealFailed'),
    )
  })
}
</script>

<style scoped>
.preview-status {
  padding: var(--spacing-lg);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.preview-status--error {
  color: var(--color-error);
}

.preview-truncated {
  padding: var(--spacing-sm) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  background: var(--color-bg-secondary);
}

/* pre-wrap:长行(md 段落是单行)折行显示,预览不该出现横向滚动条;
   等宽字体保 txt 对齐语义(日志/表格状文本)。 */
.preview-content {
  margin: 0;
  padding: var(--spacing-lg);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-family: var(--font-mono);
  font-size: var(--font-size-sm);
  line-height: 1.6;
  color: var(--color-text-primary);
}
</style>
