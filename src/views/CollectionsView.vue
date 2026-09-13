<template>
  <!-- 收藏夹总览：4 个系统类型夹 + 用户自定义夹 + 新建（需求7, §3.7） -->
  <div class="collections-view">
    <div class="collections-header">
      <h2 class="collections-title">{{ t('sidebar.collections') }}</h2>
      <p class="collections-subtitle">{{ t('collections.subtitle') }}</p>
    </div>

    <div class="collections-grid">
      <!-- article 承载展示与独立次操作；覆盖式主按钮负责打开，避免 clickable div 与嵌套按钮。 -->
      <article
        v-for="c in store.collections"
        :key="c.id"
        class="collection-card"
        :class="{ 'collection-card--system': c.kind === 'system' }"
      >
        <button
          type="button"
          class="collection-card__primary"

          @click="onCardClick(c)"
          @dblclick.stop="startRename(c)"
          @contextmenu.prevent="onCardContextMenu(c, $event)"
        />
        <span class="collection-card__icon">
          <component :is="iconFor(c)" :size="28" />
        </span>

        <!-- 名字：用户夹可双击或点铅笔重命名（系统夹只读） -->
        <input
          v-if="editingId === c.id"
          ref="renameInput"
          v-model="editName"
          class="collection-card__input"
          :placeholder="t('collections.namePlaceholder')"
          maxlength="40"
          @keydown.enter="submitRename(c)"
          @keydown.esc="cancelRename"
          @blur="submitRename(c)"
          @click.stop
        />
        <span v-else class="collection-card__name">
          {{ c.name }}
        </span>

        <span class="collection-card__count">{{ t('settings.volItems', { n: c.itemCount }) }}</span>

        <!-- 用户夹：重命名（左上）+ 删除（右上）；系统夹受保护无此二者 -->
        <template v-if="c.kind === 'user'">
          <button
            type="button"
            class="collection-card__edit"
            :title="t('settings.volRename')"

            @click.stop="startRename(c)"
          >
            <Pencil :size="13" />
          </button>
          <button
            type="button"
            class="collection-card__del"
            :title="t('collections.deleteCollection')"

            @click.stop="onDelete(c)"
          >
            <Trash2 :size="14" />
          </button>
        </template>
      </article>

      <!-- 新建收藏夹卡片 -->
      <button
        v-if="!creating"
        type="button"
        class="collection-card collection-card--new"
        @click="startCreate"
      >
        <span class="collection-card__icon"><Plus :size="28" /></span>
        <span class="collection-card__name">{{ t('collections.newCollection') }}</span>
      </button>
      <div v-else class="collection-card collection-card--new collection-card--editing">
        <input
          ref="nameInput"
          v-model="newName"
          class="collection-card__input"
          :placeholder="t('collections.namePlaceholder')"
          maxlength="40"
          @keydown.enter="submitCreate"
          @keydown.esc="cancelCreate"
          @blur="submitCreate"
        />
      </div>
    </div>

    <!-- 回收站(2026-07-16):软删的夹此前只能靠 5 秒 toast / 会话内 undo 栈捞回,重启即永久不可达
         (行还在、无任何读路径)。这是它**唯一**的持久入口。仅在有软删夹时出现——恒显一个空回收站
         是噪音,而它需要被看见的时机恰好就是「有东西可捞」的时机。 -->
    <section v-if="store.deleted.length" class="collections-trash">
      <button
        type="button"
        class="collections-trash__toggle"

        @click="trashOpen = !trashOpen"
      >
        <ChevronRight
          :size="14"
          class="collections-trash__chevron"
          :class="{ 'is-open': trashOpen }"
        />
        {{ t('collections.deletedSection', { n: store.deleted.length }) }}
      </button>

      <template v-if="trashOpen">
        <p class="collections-trash__hint">{{ t('collections.deletedHint') }}</p>
        <div class="collections-grid">
          <article
            v-for="c in store.deleted"
            :key="c.id"
            class="collection-card collection-card--deleted"
          >
            <span class="collection-card__icon"><FolderHeart :size="28" /></span>
            <span class="collection-card__name">{{ c.name }}</span>
            <span class="collection-card__count">{{
              t('settings.volItems', { n: c.itemCount })
            }}</span>
            <!-- 恢复是本卡片**唯一**操作,故用具名按钮而非图标角标:回收站是低频入口,写明动作比省空间重要。 -->
            <button type="button" class="collection-card__restore" @click="onRestore(c)">
              <Undo2 :size="13" />
              {{ t('collections.restore') }}
            </button>
          </article>
        </div>
      </template>
    </section>

    <!-- 相册卡片右键菜单(U-2 方案 A ①):目前只这一项,不发明其它菜单命令。 -->
    <ContextMenu
      :visible="ctxMenu.visible"
      :x="ctxMenu.x"
      :y="ctxMenu.y"
      :items="ctxMenu.items"
      @update:visible="ctxMenu.visible = $event"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import type { Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  ImageIcon,
  Video,
  Music,
  FileText,
  FolderHeart,
  Plus,
  Trash2,
  Pencil,
  ChevronRight,
  Undo2,
  Download,
} from '@lucide/vue'
import { useCollectionStore } from '../stores/collectionStore'
import { useViewStore } from '../stores/viewStore'
import { useToastStore } from '../stores/toastStore'
import { useHistoryStore } from '../stores/historyStore'
import { useConfirm } from '../composables/useConfirm'
import { useExportStore } from '../stores/exportStore'
import { useExportEntries } from '../composables/useExportEntries'
import ContextMenu, { type ContextMenuItem } from '../components/common/ContextMenu.vue'
import type { Collection } from '../types/media'

const store = useCollectionStore()
const viewStore = useViewStore()
const toast = useToastStore()
const history = useHistoryStore()
const router = useRouter()
const { confirm } = useConfirm()
const { t } = useI18n()
const exportStore = useExportStore()
const exportEntries = useExportEntries()

// 系统夹按类型映射图标；用户夹统一用「收藏文件夹」图标。
const SYS_ICON: Record<string, Component> = {
  image: ImageIcon,
  video: Video,
  audio: Music,
  document: FileText,
}
function iconFor(c: Collection) {
  if (c.kind === 'system' && c.mediaTypeFilter) return SYS_ICON[c.mediaTypeFilter] ?? FolderHeart
  return FolderHeart
}

function onCardClick(c: Collection) {
  if (editingId.value === c.id) return // 编辑中不导航
  openCollection(c)
}

function openCollection(c: Collection) {
  viewStore.setActiveCollection(c)
  router.push(`/collections/${c.id}`)
}

// ── 右键菜单:导出相册(U-2 方案 A ①)── 最小菜单,只这一项,不发明其它命令。
const ctxMenu = ref({
  visible: false,
  x: 0,
  y: 0,
  items: [] as ContextMenuItem[],
})

function startExportAlbum(c: Collection) {
  const payload = exportEntries.buildAlbumExportPayload(c)
  if (!payload) return // 系统夹被拒(见 useExportEntries 防御注释);此路径实际不可达,onCardContextMenu 已早退
  const { selection, source } = payload
  // ViewStale 重试回调(P3):按 albumId 重组装,而非借用当前选区(严禁回退借用选区)。
  exportStore.openExportDialog(
    selection,
    source,
    () => exportEntries.buildAlbumExportPayload(c)?.selection ?? null,
  )
}

// 菜单钳制视口(建议项):ContextMenu.vue 本身不钳边界,MediaGrid.vue 既有右键菜单(onContextMenu)
// 同样直接取 e.clientX/clientY、未钳——本处不改上游共用组件姿态,只在本菜单落地时按视口夹取。
// 尺寸按 ContextMenu.vue 的 CSS 估算(min-width:160px;单项 padding 6px×2 + 文字行高约 20px ≈ 32px,
// 容器 padding 4px×2):固定菜单只此一项,估算足够,不必额外量 DOM。
const MENU_WIDTH_ESTIMATE = 180
const MENU_HEIGHT_ESTIMATE = 48
const MENU_VIEWPORT_MARGIN = 8

function clampMenuPos(x: number, y: number): { x: number; y: number } {
  const maxX = window.innerWidth - MENU_WIDTH_ESTIMATE - MENU_VIEWPORT_MARGIN
  const maxY = window.innerHeight - MENU_HEIGHT_ESTIMATE - MENU_VIEWPORT_MARGIN
  return {
    x: Math.max(MENU_VIEWPORT_MARGIN, Math.min(x, maxX)),
    y: Math.max(MENU_VIEWPORT_MARGIN, Math.min(y, maxY)),
  }
}

function onCardContextMenu(c: Collection, e: MouseEvent) {
  // 系统夹早退(仿 startRename/onDelete 门控):系统夹不承载 album_items,scope=collection 的
  // 相册导出对它会错集——系统夹的导出路径是打开后用工具栏「导出当前视图」。
  if (c.kind !== 'user') return
  const pos = clampMenuPos(e.clientX, e.clientY)
  ctxMenu.value.x = pos.x
  ctxMenu.value.y = pos.y
  ctxMenu.value.items = [
    {
      id: 'collections.contextExport',
      label: t('selection.export'),
      icon: Download,
      action: () => startExportAlbum(c),
    },
  ]
  ctxMenu.value.visible = true
}

// ── 重命名（用户夹）─────────────────────────────────────────────────────────
const editingId = ref<number | null>(null)
const editName = ref('')
const renameInput = ref<HTMLInputElement | HTMLInputElement[] | null>(null)
// 防 blur 与 enter 重复提交（同 create 流程）。
let renameSubmitted = false

function startRename(c: Collection) {
  if (c.kind !== 'user') return // 系统夹只读
  editingId.value = c.id
  editName.value = c.name
  renameSubmitted = false
  nextTick(() => {
    const el = Array.isArray(renameInput.value) ? renameInput.value[0] : renameInput.value
    el?.focus()
    el?.select()
  })
}

async function submitRename(c: Collection) {
  if (renameSubmitted) return
  renameSubmitted = true
  const name = editName.value.trim()
  editingId.value = null
  if (name && name !== c.name) await store.rename(c.id, name)
}

function cancelRename() {
  renameSubmitted = true
  editingId.value = null
}

// ── 新建 ───────────────────────────────────────────────────────────────────
const creating = ref(false)
const newName = ref('')
const nameInput = ref<HTMLInputElement | null>(null)
// 防止 blur 与 enter 同时触发导致重复提交。
let submitted = false

function startCreate() {
  creating.value = true
  newName.value = ''
  submitted = false
  nextTick(() => nameInput.value?.focus())
}

async function submitCreate() {
  if (submitted) return
  submitted = true
  const name = newName.value.trim()
  creating.value = false
  if (name) await store.create(name)
}

function cancelCreate() {
  submitted = true
  creating.value = false
}

// ── 删除（用户夹，软删除 + 即时 undo，镜像 Persons 隐藏/忽略范式） ────────────────
async function onDelete(c: Collection) {
  const { confirmed } = await confirm({
    title: t('collections.deleteCollection'),
    message: t('collections.deleteConfirmMsg', { name: c.name }),
    confirmText: t('selection.delete'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  await store.remove(c.id)
  // 软删除可撤销，**无时限**：撤销登记进 historyStore（会话内长存 + Ctrl+Z 可达），toast 只是即时入口。
  // 此前撤销回调寄生在 toast 对象上，5 秒后随 removeToast 一起蒸发——而后端 restore_collection 无时效
  // 校验、全库无 purge，数据本就永久可恢复（真机 round10 #7）。
  // 跨重启的捞回入口是本页下方的「已删除」区（2026-07-16）：本栈在内存里，重启即空。
  const undoId = history.pushUndoable({
    undo: () => store.restore(c.id),
    redo: () => store.remove(c.id),
    undoMessage: t('collections.restoredDone', { name: c.name }),
    redoMessage: t('collections.deletedDone', { name: c.name }),
  })
  toast.addToast('success', t('collections.deletedDone', { name: c.name }), 5000, [
    { label: t('common.undo'), onClick: () => history.undoIfTop(undoId) },
  ])
}

// ── 回收站（软删夹的持久读路径）──────────────────────────────────────────────
// 默认折叠:它是 escape hatch,不该与正列表争视觉权重;计数在标题里,想捞时一眼可见。
const trashOpen = ref(false)

async function onRestore(c: Collection) {
  await store.restore(c.id)
  // 不登记 undo:回收站里的恢复是「用户主动来捞」,反悔直接再删一次即可,压栈只会与删除路径的
  // undo 记录混叠(Ctrl+Z 撤销一次「恢复」= 又删掉,语义绕)。
  toast.addToast('success', t('collections.restoredDone', { name: c.name }), 3000)
}

onMounted(() => {
  store.load()
  // 与 load 并发:回收站计数决定「已删除」区是否出现,不能等正列表回来再串行拉一次。
  store.loadDeleted()
})
</script>

<style scoped>
.collections-view {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xl);
}
.collections-header {
  margin-bottom: var(--spacing-xl);
}
.collections-title {
  font-size: var(--font-size-xl);
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0 0 var(--spacing-xs);
}
.collections-subtitle {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0;
}
.collections-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: var(--spacing-sm);
}

/* ── 回收站(软删夹的持久读路径)────────────────────────────────────────────── */
.collections-trash {
  margin-top: var(--spacing-xl);
  padding-top: var(--spacing-md);
  border-top: 1px solid var(--color-divider);
}
.collections-trash__toggle {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  margin-left: calc(-1 * var(--spacing-sm)); /* 视觉左缘与下方卡片网格对齐(抵消 padding) */
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 500;
  cursor: pointer;
}
.collections-trash__toggle:hover {
  background: var(--color-sidebar-hover-bg);
  color: var(--color-text-primary);
}
.collections-trash__toggle:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}
.collections-trash__chevron {
  transition: transform var(--transition-fast);
}
.collections-trash__chevron.is-open {
  transform: rotate(90deg);
}
.collections-trash__hint {
  margin: var(--spacing-sm) 0 var(--spacing-md);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
/* 回收站卡片无 __primary 覆盖钮(整卡不可点,只有「恢复」可点),故撤掉正卡片的 pointer/悬浮抬起——
   那两者是「这张卡能点开」的可供性信号,留着就是骗手。 */
/* 不用 opacity 压暗表达「已删除」:那会连带压低文字对比度(本仓有 AA 对比度门禁),
   而「已删除」的语义由所在分区 + 恢复钮已经说清,不需要再借降低可读性来表达。 */
.collection-card--deleted {
  cursor: default;
}
.collection-card--deleted:hover {
  background: var(--color-bg-secondary);
  border-color: var(--color-border);
  transform: none;
}
.collection-card__restore {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-2xs) var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    border-color var(--transition-fast);
}
.collection-card__restore:hover {
  background: var(--color-sidebar-hover-bg);
  border-color: var(--color-accent);
}
.collection-card__restore:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}
.collection-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  aspect-ratio: 4 / 3;
  padding: var(--spacing-md);
  border-radius: var(--radius-lg);
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
  color: var(--color-text-primary);
  cursor: pointer;
  text-align: center;
  transition:
    background var(--transition-fast),
    border-color var(--transition-fast),
    transform var(--transition-fast);
}
.collection-card:hover {
  background: var(--color-bg-hover);
  border-color: var(--color-border-strong);
  transform: none;
}
.collection-card:focus-within {
  border-color: var(--color-accent);
  box-shadow: var(--control-focus-ring);
}
.collection-card__primary {
  position: absolute;
  inset: 0;
  z-index: 0;
  border: 0;
  border-radius: inherit;
  background: transparent;
  cursor: pointer;
}
.collection-card__primary:focus-visible {
  outline: none;
}
/* 卡面内容(图标/名字/计数/重命名输入)浮到覆盖式 primary 按钮(position:absolute; inset:0; z-index:0)
   之上, 并让点击穿透给它。
   **必须排除两个自带绝对定位的角标按钮**(真机 round10 #6):它们是 .collection-card 的直接子元素, 会被这条
   blanket 规则命中——而 `:not()` 把参数的特异性算进来 → 本条 (0,3,0) 高过 `.collection-card__del` 自身的
   (0,2,0), **与书写顺序无关**地把它们的 position:absolute 覆写成 relative → 角标掉回常规流, 成为 column
   flex 的第 4、5 项各占一行, 且 left/right:6px 在 relative 下变成推移使两者中轴错开 12px。
   `.collection-card__input` **不**排除:它是流内元素(width:100%), 正需要本条给的 position:relative 才能让
   下方 z-index:2 生效, 只需另行补回 pointer-events:auto。 */
.collection-card > :not(.collection-card__primary, .collection-card__del, .collection-card__edit) {
  position: relative;
  z-index: 1;
  pointer-events: none;
}
.collection-card--system .collection-card__icon {
  color: var(--color-accent);
}
.collection-card__icon {
  display: inline-flex;
  color: var(--color-text-secondary);
}
.collection-card__name {
  font-size: var(--font-size-sm);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}
.collection-card__count {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
.collection-card__del,
.collection-card__edit {
  position: absolute;
  top: var(--spacing-xs);
  display: inline-flex;
  padding: var(--spacing-xs);
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  opacity: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  transition:
    opacity var(--transition-fast),
    color var(--transition-fast),
    background var(--transition-fast);
}
.collection-card__del {
  right: var(--spacing-xs);
}
.collection-card__edit {
  left: var(--spacing-xs);
}
.collection-card:hover .collection-card__del,
.collection-card:hover .collection-card__edit,
.collection-card:focus-within .collection-card__del,
.collection-card:focus-within .collection-card__edit {
  opacity: 1;
}
/* 三者须高于卡面内容(z-index:1)与 primary(z-index:0)且自持点击。
   注:`.collection-card__input` 的 pointer-events:auto 是**必需**的(它仍被上面的 blanket 规则命中、拿到
   pointer-events:none);两个角标已从 blanket 排除, 这里对它们只剩 z-index 有效——保留 pointer-events 作
   防御(万一将来又被纳入 blanket)。此前本条只补了 z-index/pointer-events 却**漏了 position**, 正是 #6 的
   直接来源:补丁救了症状(按钮点不动), 没救根因(定位被覆写)。 */
.collection-card .collection-card__del,
.collection-card .collection-card__edit,
.collection-card .collection-card__input {
  z-index: 2;
  pointer-events: auto;
}
.collection-card__del:hover {
  color: var(--color-error);
  background: var(--color-bg-hover);
}
.collection-card__edit:hover {
  color: var(--color-accent);
  background: var(--color-bg-hover);
}
.collection-card--new {
  border-style: dashed;
  color: var(--color-text-secondary);
}
.collection-card--editing {
  cursor: default;
}
.collection-card__input {
  width: 100%;
  height: var(--control-size-default);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-input-border-focus);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-align: center;
  outline: none;
}
</style>
