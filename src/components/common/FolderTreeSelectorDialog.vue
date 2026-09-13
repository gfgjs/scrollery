<template>
  <!-- 外壳迁 UiDialog:恒 Teleport(此前原位渲染无 Teleport)+ 焦点陷阱(此前仅 focus 遮罩、Tab 会逃逸)。
       树列表须满幅(hover 高亮到边)→ body-padding='0';树自身的定高滚动移到插槽内 .tree-scroll。
       本组件由父 v-if 条件挂载,open 恒 true;焦点陷阱经 useFocusTrap immediate 挂载即 engage。 -->
  <UiDialog
    :open="true"
    :title="title"
    :close-label="t('common.cancel')"
    max-width="500px"
    body-padding="0"
    @close="cancel"
  >
    <div class="tree-scroll">
      <div
        v-if="folderTree.nodes.value.length === 0 && !folderTree.loading.value"
        class="empty-state"
      >
        {{ t('folderPicker.empty') }}
      </div>
      <div class="tree-container" v-else>
        <button
          v-for="node in folderTree.nodes.value"
          :key="node.nodeKey"
          class="tree-item"
          :class="{ active: selectedNode?.nodeKey === node.nodeKey }"
          :style="{ paddingLeft: node.depth * 16 + 12 + 'px' }"
          @click="pickNode(node)"
        >
          <span class="tree-arrow" @click.stop="folderTree.toggleNode(node)">
            <ChevronRight
              v-if="node.hasChildren"
              :size="14"
              class="tree-chevron"
              :class="{ expanded: node.expanded }"
            />
            <span v-else class="tree-chevron-spacer" />
          </span>
          <span class="tree-icon"><Folder :size="15" /></span>
          <span class="tree-label" :title="getNodePath(node)">
            <span class="name">{{ node.name }}</span>
            <span class="path">{{ getNodePath(node) }}</span>
          </span>
        </button>
      </div>
    </div>

    <!-- 自定义左右分栏页脚:UiDialog 的 .dialog-footer 是 flex-end,故用 width:100% 的 footer-split 自撑 space-between。 -->
    <template #footer>
      <div class="footer-split">
        <div class="footer-group">
          <UiButton
            variant="secondary"
            :disabled="!selectedNodeId"
            :title="t('folderPicker.newHereTitle')"
            @click="createNewFolderHere"
          >
            {{ t('folderPicker.newHere') }}
          </UiButton>
          <UiButton
            variant="secondary"
            :title="t('folderPicker.newElsewhereTitle')"
            @click="createNewGlobalFolder"
          >
            {{ t('folderPicker.newElsewhere') }}
          </UiButton>
        </div>
        <div class="footer-group">
          <UiButton variant="secondary" @click="cancel">{{ t('common.cancel') }}</UiButton>
          <UiButton variant="primary" :disabled="!selectedNodeId" @click="confirm">
            {{ t('common.ok') }}
          </UiButton>
        </div>
      </div>
    </template>
  </UiDialog>

  <!-- 嵌套「新建文件夹」对话框:UiDialog 迁移后本组件为多根,它作同级根、各自独立 Teleport 到 body
       (后挂载者在 DOM 靠后 → 叠于树选择器之上)。 -->
  <FolderCreateDialog
    v-if="folderCreateDialog.isOpen"
    :base-path="folderCreateDialog.basePath"
    @close="folderCreateDialog.isOpen = false"
    @created="onFolderCreated"
  />
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronRight, Folder } from '@lucide/vue'
import { useFolderTree } from '../../composables/useFolderTree'
import { useScanStore } from '../../stores/scanStore'
import FolderCreateDialog from './FolderCreateDialog.vue'
import UiDialog from '../ui/UiDialog.vue'
import UiButton from '../ui/UiButton.vue'
import type { DirNode } from '../../types/media'

defineProps<{
  title: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'confirm', targetNode: DirNode): void
}>()

const { t } = useI18n()
const scan = useScanStore()
const folderTree = useFolderTree()

// selectedNodeId = **实体身份**:本对话框选的是「移动/复制的目标库目录」,emit 与 IPC 都要 DB id,
// 故 null 即「未选/不可作目标」,底部按钮据此禁用。
// 高亮判定另走 selectedNode 的 **nodeKey**(结构身份):用 id 比会踩 `null === null` 恒真——
// 一旦树里出现无实体身份的节点(D-013),它们会全部同时高亮。今天 FS-only 不会进本对话框
// (它只列库内目录),但「碰巧不会触发」不是不变量。
const selectedNodeId = ref<number | null>(null)
const selectedNode = ref<DirNode | null>(null)

const folderCreateDialog = ref({
  isOpen: false,
  basePath: '',
})

onMounted(async () => {
  if (scan.scanRoots.length === 0) {
    await scan.loadScanRoots()
  }
  await folderTree.loadRoots(scan.scanRoots)

  // 未选中任何节点时默认选首个
  if (!selectedNodeId.value && folderTree.nodes.value.length > 0) {
    selectedNodeId.value = folderTree.nodes.value[0].id
    selectedNode.value = folderTree.nodes.value[0]
  }
  // 初始焦点由 UiDialog 焦点陷阱接管(无 data-autofocus 时落到首个可聚焦元素);无需再手动聚焦遮罩。
})

// 选中某节点（抽成方法——Vue 内联多语句处理器会被 Prettier semi:false 拆行破坏）。
function pickNode(node: DirNode) {
  selectedNodeId.value = node.id
  selectedNode.value = node
}

function cancel() {
  emit('close')
}

function confirm() {
  if (selectedNode.value) {
    emit('confirm', selectedNode.value)
  }
}

function createNewFolderHere() {
  if (selectedNode.value && selectedNode.value.absPath) {
    folderCreateDialog.value.basePath = selectedNode.value.absPath
    folderCreateDialog.value.isOpen = true
  }
}

function createNewGlobalFolder() {
  folderCreateDialog.value.basePath = ''
  folderCreateDialog.value.isOpen = true
}

function getNodePath(node: DirNode): string {
  if (node.absPath) return node.absPath
  const r = scan.scanRoots.find((root) => root.id === node.rootId)
  if (r) {
    return node.relPath ? `${r.path}/${node.relPath}` : r.path
  }
  return node.relPath || ''
}

async function onFolderCreated() {
  await scan.loadScanRoots()
  await folderTree.loadRoots(scan.scanRoots)
}
</script>

<style scoped>
/* 外壳(overlay/content/header/title/关闭键/动画)由 UiDialog + 全局 Modal 基座提供;宽度 500px 经
   max-width prop 传入(基座默认 420px)。本组件只保留树列表正文 + 定高滚动 + 分栏页脚特化。 */
.tree-scroll {
  height: 320px;
  overflow-y: auto;
}

.empty-state {
  padding: var(--spacing-xl);
  text-align: center;
  color: var(--color-text-tertiary);
}

.tree-container {
  display: flex;
  flex-direction: column;
  padding: var(--spacing-xs) 0;
}

.tree-item {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  text-align: left;
}

.tree-item:hover {
  background: var(--color-bg-hover);
}

.tree-item.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}

.tree-arrow {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  cursor: pointer;
  border-radius: var(--radius-sm);
}

.tree-arrow:hover {
  background: color-mix(in srgb, currentColor 10%, transparent);
}

.tree-chevron {
  transition: transform var(--transition-normal);
}

.tree-chevron.expanded {
  transform: rotate(90deg);
}

.tree-chevron-spacer {
  width: 14px;
}

.tree-icon {
  margin: 0 8px 0 4px;
  display: flex;
  align-items: center;
}

.tree-label {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  flex: 1;
  text-align: left;
  overflow: hidden;
}

.tree-label .name {
  white-space: nowrap;
  flex-shrink: 0;
}

.tree-label .path {
  font-size: var(--font-size-2xs);
  color: var(--color-text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  direction: rtl; /* 超长时从左侧截断,保末尾文件夹名可见 */
  text-align: left;
}

/* 分栏页脚:UiDialog 的 .dialog-footer 为 flex + justify-end,故用 width:100% 的容器自撑 space-between。 */
.footer-split {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  gap: var(--spacing-sm);
}
.footer-group {
  display: flex;
  gap: var(--spacing-sm);
}
</style>
