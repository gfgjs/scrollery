<template>
  <AccordionSection id="folders" :order="order" :title="$t('sidebar.folders')">
    <!-- 右对齐的标题操作：全部 / 导入文件夹 / 新建文件夹 -->
    <template #actions>
      <button
        class="show-all-btn"
        :class="{ active: viewStore.activeSmartAlbum === 'all' && !viewStore.activeDirectoryId }"
        :title="$t('sidebar.showAllTitle')"
        @click="showAll"
      >
        {{ $t('sidebar.showAll') }}
      </button>
      <UiIconButton
        ref="scopeBtnRef"
        class="folders-action"
        :label="scopeButtonLabel"
        :active="scopeMenuOpen || ui.treeDisplayMode !== 'registeredOnly' || treeFilter.hasActiveCategoryFilter"
        @click="toggleScopeMenu"
      >
        <ListTree :size="16" />
      </UiIconButton>
      <UiIconButton
        class="folders-action folders-action--narrow-hide"
        :label="anyExpanded ? t('sidebar.collapseAll') : t('sidebar.expandAll')"
        @click="toggleExpandAll"
      >
        <component :is="anyExpanded ? ChevronsDownUp : ChevronsUpDown" :size="16" />
      </UiIconButton>
      <UiIconButton
        class="folders-action folders-action--narrow-hide"
        :label="t('sidebar.newBlankFolder')"
        @click="createNewGlobalFolder"
      >
        <FolderPlus :size="16" />
      </UiIconButton>
      <UiIconButton class="folders-action" :label="$t('sidebar.addFolder')" @click="addRoot">
        <FolderSearch :size="16" />
      </UiIconButton>
    </template>

    <!-- 空态：无可见根时显示（全部根被隐藏也算空——树里确实没有可展示的目录）。 -->
    <div v-if="folderTree.nodes.value.length === 0" class="empty">
      {{
        scan.visibleScanRoots.length === 0
          ? $t('sidebar.noFolders')
          : treeFilter.hasActiveCategoryFilter
            ? $t('sidebar.noFoldersForFilter')
            : $t('sidebar.noFolders')
      }}
    </div>

    <!-- folder tree — virtualized (方案 B, T1-a): 行由 `flattenFolderRows` 拍平后窗口化,
         只有 visibleRows 渲染进 `.tree-layer`(按 offsetY 平移),`.tree` 为全高占位撑出滚动条。
         树复用侧栏共享滚动区(.sidebar__scroll-area),不自持 scrollTop。逐行 sticky 已移除;
         粘性目录头改独立覆盖层,留待 T1-b。DOM 从 O(展开行) 降到 O(视口)。 -->
    <!-- 虚拟化键盘导航:容器持焦,active 行由键盘导航先滚入窗口(必在 DOM)。
         行内 button 全部 tabindex=-1 移出 Tab 序,鼠标交互不变。 -->
    <div
      v-if="folderTree.nodes.value.length > 0"
      ref="treeRef"
      class="tree"

      tabindex="0"
      :style="{ height: spacerHeight + 'px' }"
      @focus="onTreeFocus"
      @keydown="onTreeKeydown"
    >
      <!-- 粘性目录头覆盖层(T1-b):钉住视口顶所在行的祖先目录链,复刻 T1-a 移除的逐行 sticky
           的堆叠面包屑效果。点击滚到该目录,箭头就地展开/折叠。位于滚动内容层之上(z-index)。
           top 须避开粘顶的区块标题堆叠(不透明、z-index 更高):top:0 会整链藏在标题栈底下。 -->
      <!-- 粘性链是下方真实行的视觉复制品;其内 button 一并 tabindex=-1。 -->
      <div
        v-if="stickyRows.length"
        class="tree-sticky"

        :style="{ top: stackTopPx + 'px' }"
      >
        <button
          v-for="(node, i) in stickyRows"
          :key="'st' + node.nodeKey"
          tabindex="-1"
          class="tree-item sticky-item"
          :class="{ 'sticky-item--last': i === stickyRows.length - 1 }"
          :style="{ paddingLeft: node.depth * 16 + TREE_INDENT + 'px' }"
          :title="node.relPath"
          @click="scrollTreeToNodeKey(node.nodeKey)"
        >
          <span class="tree-arrow" @click.stop="folderTree.toggleNode(node)">
            <ChevronRight
              v-if="isExpandable(node)"
              :size="14"
              class="tree-chevron"
              :class="{ expanded: node.expanded }"
            />
            <span v-else class="tree-chevron-spacer" />
          </span>
          <span class="tree-icon"><Folder :size="15" /></span>
          <span class="tree-label">{{ node.name }}</span>
          <span v-if="node.mediaCount !== null" class="tree-count">{{ node.mediaCount }}</span>
        </button>
      </div>
      <div class="tree-layer" :style="{ transform: `translateY(${offsetY}px)` }">
        <template
          v-for="(row, i) in visibleRows"
          :key="
            row.kind === 'dir'
              ? 'd' + row.node.nodeKey
              : row.kind === 'more'
                ? 'm' + row.dirKey
                : 'f' + row.file.nodeKey
          "
        >
          <!-- 目录行——粘性标题，将其文件钉在下方。 -->
          <button
            v-if="row.kind === 'dir'"

            tabindex="-1"
            class="tree-item"
            :data-dir-id="row.node.id"
            :class="{
              active:
                ui.groupBy === 'folder'
                  ? sameEntityId(row.node.id, ui.scrolledDirectoryId)
                  : sameEntityId(row.node.id, viewStore.activeDirectoryId),
              'drag-over':
                sameEntityId(row.node.id, dropId) ||
                sameEntityId(row.node.id, ui.mediaDragHoverDirId),
              'drag-source': sameEntityId(row.node.id, dragId),
              'kb-active': startIndex + i === activeIndex,
              'is-hidden': row.node.hidden,
            }"
            :style="{ paddingLeft: row.node.depth * 16 + TREE_INDENT + 'px' }"
            @click="onNodeClick(row.node, startIndex + i)"
            @contextmenu.prevent="onNodeContextMenu($event, row.node)"
            @pointerdown="onTreePointerDown(row.node, $event)"
          >
            <span class="tree-arrow" @click.stop="folderTree.toggleNode(row.node)">
              <!-- 有子文件夹或直接文件即可展开。 -->
              <ChevronRight
                v-if="isExpandable(row.node)"
                :size="14"
                class="tree-chevron"
                :class="{ expanded: row.node.expanded }"
              />
              <span v-else class="tree-chevron-spacer" />
            </span>
            <span class="tree-icon"><Folder :size="15" /></span>
            <span class="tree-label" :title="row.node.relPath">{{ row.node.name }}</span>
            <span v-if="row.node.mediaCount !== null" class="tree-count">{{
              row.node.mediaCount
            }}</span>
          </button>

          <!-- 文件行——叶子节点，从钉住的文件夹标题下方滚过。 -->
          <button
            v-else-if="row.kind === 'file'"

            tabindex="-1"

            class="file-item"
            :class="{
              'kb-active': startIndex + i === activeIndex,
              selected: startIndex + i === activeIndex,
              'is-hidden': row.file.hidden,
              'drag-source': sameEntityId(row.file.id, dragFileId),
            }"
            :data-file-id="row.file.id"
            :style="{ paddingLeft: row.depth * 16 + TREE_INDENT + 'px' }"
            :title="fileTitle(row.file)"
            @click="onFileClick(row.file, startIndex + i)"
            @dblclick="onFileDblClick(row.file)"
            @pointerdown="onFilePointerDown(row.file, $event)"
          >
            <span class="file-icon"
              ><component :is="fileIcon(row.file.mediaType)" :size="14"
            /></span>
            <span class="file-label">{{ row.file.fileName }}</span>
            <Heart v-if="row.file.isFavorited" :size="11" class="file-fav" />
          </button>

          <!-- 「加载更多」行(T2 单目录分页):滚进窗口即自动追加下一页(见 maybeAutoLoadMore),
             加载中显示转圈;点击保留为失败兜底(重试并解除自动加载拉黑)。 -->
          <button
            v-else

            tabindex="-1"

            class="more-item"
            :class="{ 'kb-active': startIndex + i === activeIndex }"
            :style="{ paddingLeft: row.depth * 16 + TREE_INDENT + 'px' }"
            :disabled="loadingMoreDirKey === row.dirKey"
            @click="onLoadMore(row.dirKey, startIndex + i)"
          >
            <span class="more-icon">
              <RefreshCw v-if="loadingMoreDirKey === row.dirKey" :size="14" class="spin-anim" />
              <MoreHorizontal v-else :size="14" />
            </span>
            <span class="more-label">{{
              loadingMoreDirKey === row.dirKey
                ? t('common.loading')
                : t('sidebar.loadMoreFiles', { count: row.loadedCount })
            }}</span>
          </button>
        </template>
      </div>
    </div>
  </AccordionSection>

  <!-- 「显示内容」菜单：scope 仍是三态互斥单选；分类是独立的多选 checkbox group。
       刷新移入分类操作行右侧，避免菜单继续向下增长；快照的真相源是磁盘,没有 FS 监听时用户主动刷新是唯一收敛手段。 -->
  <UiPopover v-model:open="scopeMenuOpen" :anchor="scopeAnchor" placement="bottom-end">
    <div class="scope-menu">
      <!-- 三态互斥单选集与「刷新」普通项分组显示。 -->
      <div>
        <button
          v-for="opt in SCOPE_OPTIONS"
          :key="opt.mode"
          class="scope-item"
          :class="{ active: ui.treeDisplayMode === opt.mode }"
          @click="chooseScope(opt.mode)"
        >
          <Check v-if="ui.treeDisplayMode === opt.mode" :size="14" class="scope-check" />
          <span v-else class="scope-check-spacer" />
          <span class="scope-text">
            <span class="scope-label">{{ t(opt.label) }}</span>
            <span class="scope-hint">{{ t(opt.hint) }}</span>
          </span>
        </button>
      </div>
      <div class="scope-sep" />
      <div
        class="tree-category-group"
      >
        <div class="tree-category-heading">
          <span class="scope-label">{{ t('sidebar.treeCategories') }}</span>
          <span class="scope-hint">{{ treeCategorySummary }}</span>
        </div>
        <div class="tree-category-actions">
          <button
            type="button"

            class="tree-category-action"
            :disabled="treeFilter.allCategoriesSelected"
            @click="treeFilter.selectAllCategories"
          >
            {{ t('sidebar.treeCategorySelectAll') }}
          </button>
          <button
            type="button"

            class="tree-category-action"
            :title="t('sidebar.treeCategoryDirectoriesOnlyHint')"
            :disabled="treeFilter.directoriesOnly"
            @click="treeFilter.showDirectoriesOnly"
          >
            {{ t('sidebar.treeCategoryDirectoriesOnly') }}
          </button>
          <button
            type="button"

            class="tree-category-action tree-category-refresh"
            @click="refreshTree"
          >
            <RefreshCw :size="14" />
            <span>{{ t('sidebar.refreshTree') }}</span>
          </button>
        </div>
        <div class="tree-category-subheading">
          <span class="scope-label">{{ t('sidebar.treeCategoryShared') }}</span>
          <span class="scope-hint">{{ t('sidebar.treeCategorySharedHint') }}</span>
        </div>
        <button
          v-for="category in MEDIA_CATEGORY_DESCRIPTORS"
          :key="category.id"
          type="button"

          class="scope-item tree-category-item"
          :class="{
            active: treeFilter.isCategorySelected(category.id),
          }"

          @click="treeFilter.toggleCategory(category.id)"
        >
          <Check
            v-if="treeFilter.isCategorySelected(category.id)"
            :size="14"
            class="scope-check"
          />
          <span v-else class="scope-check-spacer" />
          <component :is="category.icon" :size="14" class="tree-category-icon" />
          <span class="scope-text">
            <span class="scope-label">{{ t(category.labelKey) }}</span>
          </span>
        </button>
        <div class="tree-category-subheading tree-category-subheading--other">
          <span class="scope-label">{{ t('sidebar.treeCategoryOtherOnly') }}</span>
          <span class="scope-hint">{{ t('sidebar.treeCategoryOtherOnlyHint') }}</span>
        </div>
        <button
          type="button"

          class="scope-item tree-category-item"
          :class="{
            active: treeFilter.isCategorySelected(TREE_OTHER_CATEGORY_DESCRIPTOR.id),
            'tree-category-item--other-registered': ui.treeDisplayMode === 'registeredOnly',
          }"

          @click="treeFilter.toggleCategory(TREE_OTHER_CATEGORY_DESCRIPTOR.id)"
        >
          <Check
            v-if="treeFilter.isCategorySelected(TREE_OTHER_CATEGORY_DESCRIPTOR.id)"
            :size="14"
            class="scope-check"
          />
          <span v-else class="scope-check-spacer" />
          <component
            :is="TREE_OTHER_CATEGORY_DESCRIPTOR.icon"
            :size="14"
            class="tree-category-icon"
          />
          <span class="scope-text">
            <span class="scope-label">{{ t(TREE_OTHER_CATEGORY_DESCRIPTOR.labelKey) }}</span>
            <span class="scope-hint">
              {{
                ui.treeDisplayMode === 'registeredOnly'
                  ? t('sidebar.treeCategoryOtherRegisteredHint')
                  : t(TREE_OTHER_CATEGORY_DESCRIPTOR.hintKey)
              }}
            </span>
          </span>
        </button>
      </div>
    </div>
  </UiPopover>

  <!-- 树节点的右键菜单（自身 teleport 到 body）。 -->
  <ContextMenu
    :items="contextMenu.items"
    :visible="contextMenu.visible"
    :x="contextMenu.x"
    :y="contextMenu.y"
    @update:visible="contextMenu.visible = $event"
  />

  <!-- 树内纯文本只读预览(问题②方案 B v1)。UiDialog 自带 Teleport,无需再包。 -->
  <FilePreviewDialog :request="filePreview" @close="filePreview = null" />

  <!-- 新建文件夹对话框 -->
  <Teleport to="body">
    <FolderCreateDialog
      v-if="createDialog.isOpen"
      :base-path="createDialog.basePath"
      @close="createDialog.isOpen = false"
      @created="onFolderCreated"
    />
  </Teleport>

  <!-- Floating drag preview for move/copy — pointer-events:none so it never
       blocks elementFromPoint. | 移动/复制的浮动拖拽预览——pointer-events:none，
       绝不挡住 elementFromPoint。 -->
  <Teleport to="body">
    <div
      v-if="ghost.visible"
      class="drag-ghost"
      :style="{ left: ghost.x + 12 + 'px', top: ghost.y + 8 + 'px' }"
    >
      <Folder :size="13" />
      <span class="drag-ghost__name">{{ ghost.label }}</span>
      <span class="drag-ghost__mode">{{ ghost.copy ? t('common.copy') : t('common.move') }}</span>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { invokeIpc } from '../../../utils/ipc'
import { logger } from '../../../utils/logger'
import { IPC } from '../../../constants/ipc'
import {
  ChevronRight,
  Folder,
  FolderSearch,
  FolderPlus,
  Heart,
  ChevronsDownUp,
  ChevronsUpDown,
  MoreHorizontal,
  RefreshCw,
  ListTree,
  Check,
} from '@lucide/vue'
import AccordionSection from '../AccordionSection.vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import UiPopover from '../../ui/UiPopover.vue'
import ContextMenu from '../../common/ContextMenu.vue'
import FolderCreateDialog from '../../common/FolderCreateDialog.vue'
import FilePreviewDialog, { type FilePreviewRequest } from './FilePreviewDialog.vue'
import { useUiStore } from '../../../stores/uiStore'
import { useViewStore } from '../../../stores/viewStore'
import { useTreeFilterStore } from '../../../stores/treeFilterStore'
import { useToastStore } from '../../../stores/toastStore'
import { useScanStore } from '../../../stores/scanStore'
import { useMediaStore } from '../../../stores/mediaStore'
import { useHistoryStore } from '../../../stores/historyStore'
import { useFolderTree } from '../../../composables/useFolderTree'
import { useConfirm } from '../../../composables/useConfirm'
import {
  useFolderTreeVirtualization,
  TREE_INDENT,
} from '../../../composables/useFolderTreeVirtualization'
import { useFolderTreeIndices } from '../../../composables/useFolderTreeIndices'
import { useFolderTreeKeyboardNav } from '../../../composables/useFolderTreeKeyboardNav'
import { useFolderTreeAutoLoadMore } from '../../../composables/useFolderTreeAutoLoadMore'
import { useFolderTreeDragDrop } from '../../../composables/useFolderTreeDragDrop'
import { useFolderTreeSync } from '../../../composables/useFolderTreeSync'
import { useFolderRootActions } from '../../../composables/useFolderRootActions'
import type { DirNode, DirFile, TreeDisplayMode } from '../../../types/media'
import {
  MEDIA_CATEGORY_DESCRIPTORS,
  TREE_OTHER_CATEGORY_DESCRIPTOR,
} from '../../../constants/mediaCategoryDescriptors'
import { openMediaRoute } from '../../../utils/mediaRoute'
import { folderToPath } from '../../../utils/viewRoute'
import {
  flattenFolderRows,
  hasEntityIdentity,
  isOpenableInApp,
  isTextPreviewable,
  isExpandable,
  sameEntityId,
  fileIcon,
  fileTitle as fileTitleImpl,
} from './folderTree.helpers'
import { isMobilePlatform } from '../../../utils/platform'
import { summarizeTreeCategories } from './treeCategoryMenu.helpers'

defineProps<{ order: number }>()

const ui = useUiStore()
const viewStore = useViewStore()
const treeFilter = useTreeFilterStore()
const toast = useToastStore()
const scan = useScanStore()
const media = useMediaStore()
const history = useHistoryStore()
const { confirm } = useConfirm()
const router = useRouter()
const route = useRoute()
const { t } = useI18n()

// 取数路径由显示模式分发(唯一分发点在 useFolderTree 内)。传取值器而非值:composable 每次取数时
// 读当下的模式,不需要自己订阅响应式。
// 第二参 = 展开失败上报(R-10):composable 已回退折叠,这里补用户可见信号——否则「卷拔了/
// 权限拒了」呈现为点一下箭头又弹回去,无任何解释。
const folderTree = useFolderTree(
  () => ui.treeDisplayMode,
  () => toast.addToast('error', t('sidebar.expandFailed')),
  () => treeFilter.selectedCategories,
)

// ── Flattened display rows: dirs interleaved with their (lazy-loaded) files ──────
// Folders-first (VSCode-style): within a directory, subfolders (and their expanded
// subtrees) come first, then the directory's OWN files. `folderTree.nodes` is already
// a pre-order DFS of dirs; we emit a dir's files only once its whole subtree has been
// walked — i.e. when we hit the next node at the same-or-shallower depth — so a
// folder's structure stays navigable even when it holds many loose files.
// 拍平的显示行：文件夹优先（VSCode 风格）——某目录内先列子文件夹（及其已展开子树），
// 再列该目录「自身」的文件。`folderTree.nodes` 已是目录的前序 DFS；我们仅在某目录整个
// 子树遍历完毕后（即遇到同级或更浅深度的下一个节点）才发射其文件，使文件夹结构在含大量
// 散落文件时仍可导航。
const displayRows = computed(() => flattenFolderRows(folderTree.nodes.value))

// ── 树虚拟化(T1-a/T1-b)+ 双身份索引(D-013)基础设施 ──────────────────────────
// 装配顺序说明(拆分方案 §2.2 依赖方向):virtualization/indices 是基础设施层,dragDrop 消费
// 二者;KeyboardNav 需要 onNodeClick/onFileClick/onFileDblClick/AutoLoadMore.onLoadMore 作
// 回调,故放在这些函数与 autoLoadMore 之后创建——kbNav 产出的 activeIndex 被更早定义的
// onFileClick/onNodeClick 及 autoLoadMore 的 syncActiveIndex 以闭包形式前向引用,运行期由
// Vue setup 的同步执行顺序保证赋值先于任何用户交互触发,是标准的 composable 互引用写法。
const {
  treeRef,
  scrollAreaEl,
  spacerHeight,
  offsetY,
  visibleRows,
  startIndex,
  firstVisibleIndex,
  stickyRows,
  rowH,
  stackTopPx,
  stackBottomPx,
  treeOffsetTop,
  updateWindow,
  scrollTreeToNodeKey,
  getLastScrollTs,
} = useFolderTreeVirtualization(displayRows)

// 勿包 computed 转发:nodes 是原地 mutate + triggerRef,computed 同值不 bump 版本会掐死索引重算,见 useFolderTreeIndices.ts 头注释。
const { nodesById, nodesByKey, isDescendant } = useFolderTreeIndices(folderTree.nodes)

const {
  dragId,
  dropId,
  dragFileId,
  ghost,
  onTreePointerDown,
  onFilePointerDown,
  consumeSuppressClick,
} = useFolderTreeDragDrop({ nodesById, nodesByKey, isDescendant, scrollAreaEl, history, toast, t })

// ── 文件行激活路由决策(组件对外行为契约核心,不再下沉)────────────────────────
// 从树中打开文件——与画廊卡片单击（handleCardClick）共用 openMediaRoute 按类型分发;
// 查看器已打开时用 replace 而非 push（树在查看器旁仍可点,push 会叠 history,
// 关闭要按 N+1 次 ESC 才能回画廊,2026-07-10 深审问题2）。
function onFileClick(file: DirFile, idx?: number) {
  if (idx !== undefined) activeIndex.value = idx // 鼠标点击同步键盘 active 行,方向键从此处接续
  // 拖拽(D-003)松手后的尾随 click 忽略,与 onNodeClick 同姿态——否则拖完一个**可打开**
  // 文件会顺手把它路由进查看器。
  if (consumeSuppressClick()) return
  // 🔴 未入库的文件(格式未注册,或在被扫描器剪掉的隐藏目录里)**单击只选中**,不路由(§4.2)。
  // 绝不伪造 id 顶上:mediaRoute 对非 doc/audio 兜底到 /view/{id},假 id 不会报错,
  // 会把一个 .exe 静默送进图片查看器。双击 reveal 由 P1-b-3 接入。
  if (!isOpenableInApp(file)) return
  openMediaRoute(router, file.id, file.mediaType)
}

// 文本预览目标(问题②方案 B v1):非空即弹层展示;取数/错误态由 FilePreviewDialog 自持。
const filePreview = ref<FilePreviewRequest | null>(null)

// 双击 → 在系统文件管理器中**显示**(reveal),不是「用默认应用打开」(D-001,🔴 安全)。
//
// 「所有文件」模式让扫描根内的任意文件出现在树里,包括 .exe / .lnk / .bat。用默认应用打开它们
// = 双击即执行,而 canonicalize + 根边界**挡不住**这个:那些文件本就在根内,边界检查会放行 ——
// 威胁不是「路径越界」而是「根内文件本身可执行」。reveal 把最后一步交回系统文件管理器的信任链:
// 用户在那里点开是他自己的决定,不是我们代做的。
//
// 只对**应用内打不开**的文件生效:能打开的文件双击已经由第一次 click 路由进查看器了,再 reveal
// 一次会同时弹出文件管理器,属于两个动作打架。
function onFileDblClick(file: DirFile) {
  if (isOpenableInApp(file)) return
  // rootId 取自父目录节点(后端产出);relPath 直接用 file.relPath(后端产出)——两者都不推导。
  // 后端会把 relPath 当**不可信输入**过 resolve_within_root(逐段拒 `..` + canonicalize + 根边界)。
  const parent = nodesByKey.value.get(file.parentKey)
  if (!parent) return
  // 纯文本白名单 → 应用内只读预览(问题②方案 B v1)。移动端同样可用:预览是纯应用内动作,
  // 不依赖 opener。白名单外才落到 reveal。
  if (isTextPreviewable(file.fileName)) {
    filePreview.value = { fileName: file.fileName, rootId: parent.rootId, relPath: file.relPath }
    return
  }
  // 移动端 opener 不支持 reveal(D-001):UI 侧直接不发起,后端 unsupported_platform 只是
  // 被直调时的兜底(审查 R-09)。tooltip(fileTitle)同步不承诺该动作。
  if (isMobilePlatform) return
  invokeIpc(IPC.REVEAL_TREE_ENTRY, { rootId: parent.rootId, relPath: file.relPath }).catch(
    (err) => {
      logger.error('[FoldersSection] reveal tree entry failed', { error: err })
      // 按稳定 code 分流(同 exotic 姿态,匹配 message 文案是脆弱耦合):unsupported_platform
      // 意为「此平台永远不行」,给专属文案而非「失败请重试」。
      const code = (err as { code?: string } | null)?.code
      toast.addToast(
        'error',
        t(code === 'unsupported_platform' ? 'sidebar.revealUnsupported' : 'sidebar.revealFailed'),
      )
    },
  )
}

// 文件行 tooltip:去状态化后的 fileTitle 纯函数需要 t,组件侧原样包一层保持模板调用签名不变。
function fileTitle(file: DirFile): string {
  return fileTitleImpl(file, t)
}

// ── Tree node click / selection ─────────────────────────────────────────────
// ── 树节点点击 / 选择 ─────────────────────────────────────────────────────────
// 模式B「文件夹作筛选」的统一导航助手(S2-c):同步设 viewStore(保 MediaGrid getViewKey 读取时序)+
// 导航到可寻址 /folder/:id(深链/刷新可恢复;watcher 相等守卫跳过重复设值)。三处入口(点击/新增根后/移动
// 后自动选中)共用本助手,避免各自手写 push 目标导致漂移。**关键**:这三处此前 push('/'),在 S2-c 后
// watcher 会把 '/' 回填为 smart-album 'all' → 清掉刚设的 directory;故必须改为 push('/folder/:id')。
function navigateToFolder(id: number) {
  viewStore.setActiveDirectory(id)
  const target = folderToPath(id)
  if (route.path !== target) router.push(target)
}

function onNodeClick(node: DirNode, idx?: number) {
  if (idx !== undefined) activeIndex.value = idx // 鼠标点击同步键盘 active 行
  if (consumeSuppressClick()) return
  // FS-only 目录的能力边界(§4.1):下面两条分支**都是实体操作**——滚动锚点
  // (ui.pendingScrollDirId)与 /folder/:id 都要拿 id 去查库。无实体身份则只保留
  // 「选中该行」,不做导航。
  if (!hasEntityIdentity(node)) return
  if (ui.groupBy === 'folder') {
    // 模式A:文件夹分组单列表——点文件夹是滚动锚点(pendingScrollDirId),非筛选;视图保持 all、停在 '/'。
    // 锚点滚动位置不进 URL(每滚一下改 history 属荒谬,见 findings 会话续23)。
    ui.pendingScrollDirId = node.id
    if (viewStore.activeSmartAlbum !== 'all' || viewStore.activeDirectoryId !== null) {
      viewStore.setSmartAlbum('all')
      viewStore.setActiveDirectory(null)
    }
    if (route.path !== '/') router.push('/')
  } else {
    // 模式B:文件夹作筛选 → 可寻址路径。
    navigateToFolder(node.id)
  }
}

function showAll() {
  // setSmartAlbum('all') 已保证互斥(清 directory/collection/person),去掉此前冗余的第二次
  // setActiveDirectory(null)(会二次 clearSelection);S2-c 补导航到 '/' 使 route 反映「全部」视图。
  viewStore.setSmartAlbum('all')
  if (route.path !== '/') router.push('/')
}

const {
  setPendingSelectRootId,
  reloadTreePreserveExpansion,
  refreshTree: refreshTreeImpl,
  anyExpanded,
  toggleExpandAll,
} = useFolderTreeSync({
  folderTree,
  scan,
  ui,
  viewStore,
  toast,
  t,
  nodesById,
  scrollTreeToNodeKey,
  navigateToFolder,
  getTreeCategories: () => treeFilter.selectedCategories,
  getTreeCategoryKey: () => treeFilter.categoryFilterKey,
})

const { contextMenu, createDialog, onNodeContextMenu, createNewGlobalFolder, onFolderCreated, addRoot } =
  useFolderRootActions({
    scan,
    media,
    toast,
    confirm,
    t,
    reloadTreePreserveExpansion,
    setPendingSelectRootId,
  })

const { loadingMoreDirKey, onLoadMore } = useFolderTreeAutoLoadMore({
  displayRows,
  visibleRows,
  nodesByKey,
  treeRef,
  scrollAreaEl,
  rowH,
  firstVisibleIndex,
  updateWindow,
  getLastScrollTs,
  loadMoreFiles: folderTree.loadMoreFiles,
  syncActiveIndex: (idx) => {
    activeIndex.value = idx
  },
})

const { activeIndex, onTreeFocus, onTreeKeydown } = useFolderTreeKeyboardNav({
  displayRows,
  treeRef,
  scrollAreaEl,
  rowH,
  stackTopPx,
  stackBottomPx,
  treeOffsetTop,
  toggleNode: folderTree.toggleNode,
  onNodeClick,
  onFileClick,
  onFileDblClick,
  onLoadMore,
  isOpenableInApp,
  isTextPreviewable,
})

// ── 「显示内容」三态菜单(S 线 §3 / D-009)──────────────────────────────────────
// 顺序 = 从窄到宽,与「显示得越多、代价越大」一致;三态互斥故用 menuitemradio(见模板注释)。
const treeCategorySummary = computed(() => {
  const summary = summarizeTreeCategories(treeFilter.selectedCategories)
  if (summary.kind === 'all') return t('sidebar.treeCategoryAll')
  if (summary.kind === 'none') return t('sidebar.treeCategoryDirectoriesOnly')
  if (summary.kind === 'some') {
    return treeFilter.otherOverride === 'off'
      ? t('sidebar.treeCategorySelectedWithoutOther', { count: summary.count })
      : t('sidebar.treeCategorySelected', { count: summary.count })
  }
  if (summary.kind === 'other') return t(TREE_OTHER_CATEGORY_DESCRIPTOR.labelKey)
  const descriptor = MEDIA_CATEGORY_DESCRIPTORS.find((item) => item.id === summary.category)
  return descriptor ? t(descriptor.labelKey) : t('sidebar.treeCategoryDirectoriesOnly')
})
const scopeButtonLabel = computed(() =>
  treeFilter.hasActiveCategoryFilter
    ? t('sidebar.displayScopeWithTreeCategory', { summary: treeCategorySummary.value })
    : t('sidebar.displayScope'),
)

const SCOPE_OPTIONS: ReadonlyArray<{ mode: TreeDisplayMode; label: string; hint: string }> = [
  {
    mode: 'registeredOnly',
    label: 'sidebar.scopeRegisteredOnly',
    hint: 'sidebar.scopeRegisteredOnlyHint',
  },
  { mode: 'allFiles', label: 'sidebar.scopeAllFiles', hint: 'sidebar.scopeAllFilesHint' },
  {
    mode: 'allFilesWithHidden',
    label: 'sidebar.scopeAllFilesWithHidden',
    hint: 'sidebar.scopeAllFilesWithHiddenHint',
  },
]
const scopeMenuOpen = ref(false)
const scopeBtnRef = ref<InstanceType<typeof UiIconButton>>()
// 锚点在打开时抓取(同 AppToolbar:不依赖 defineExpose 暴露属性的响应式)。
const scopeAnchor = ref<HTMLElement | null>(null)
function toggleScopeMenu() {
  scopeAnchor.value = scopeBtnRef.value?.el ?? null
  scopeMenuOpen.value = !scopeMenuOpen.value
}
function chooseScope(mode: TreeDisplayMode) {
  scopeMenuOpen.value = false
  if (mode === ui.treeDisplayMode) return
  ui.setTreeDisplayMode(mode)
}
// 显式刷新(委托 useFolderTreeSync.refreshTree):关菜单仍是三态菜单自己的 UI 状态,留在本体。
async function refreshTree() {
  scopeMenuOpen.value = false
  await refreshTreeImpl()
}
</script>

<style scoped src="./FoldersSection.styles.css"></style>
