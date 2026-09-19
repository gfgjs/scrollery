<template>
  <!-- 画廊行体组件(分隔符 + 卡片):选区/拖拽/FLIP 的标记(data-item-id 与全部 handlers)
       在此单源,避免多份逐字节对齐的模板漂移。本组件不带样式:行根类由宿主 scoped 样式
       直接命中(子组件根节点继承父作用域属性),内部类经宿主 :deep() 命中——样式保持单源。 -->
  <div
    :class="row.rowType === 'separator' ? 'date-separator' : 'media-grid__row'"
    :style="{
      position: 'absolute',
      top: 0,
      transform: `translate3d(0, ${row.y - offsetY}px, 0)`,
      left: 0,
      right: 0,
      height: row.height + 'px',
      gap: row.rowType === 'separator' ? undefined : gap + 'px',
    }"
  >
    <!-- 日期/文件夹分隔符(folder 分组时行内 sticky)。
         重复组头(§6.1/§11.2):separatorKind='duplicateGroup' 走独立分支——真实 h2 语义 +
         CopyCheck 图标(文本/间距/图标三通道表达组边界,不只靠颜色);sticky 跟随现有 folder
         分隔符机制(仅行内 sticky 定位,标题行不铺底色/不投影——2026-09-12 裁决与画廊底融合)。
         label 已含完整统计(「重复组 N · M 项 · …」),不再加 separatorCounts 徽标以免信息重复。
         文件夹头(§7.2/§11.2,P3):separatorKind='duplicateFolder' 走 h3 分支(层级低于组头 h2)
         ——簇首并入簇头行(parentGroupStart,避免两层 sticky)+ 路径行 + 三桶统计行;统计文案由
         宿主经 lensSeparator.formatLensFolderStats 组装,本组件保持 i18n 无关。 -->
    <template v-if="row.rowType === 'separator'">
      <h2
        v-if="row.separatorKind === 'duplicateGroup'"
        class="separator-content separator-content--dup"
        :style="{ position: 'sticky', top: 0, zIndex: 5 }"
      >
        <CopyCheck :size="16" class="separator-icon" />
        <span class="separator-text">{{ lensGroupLabel?.(row) ?? row.separatorLabel }}</span>
      </h2>
      <h3
        v-else-if="row.separatorKind === 'duplicateFolder'"
        class="separator-content separator-content--dupFolder"
        :style="{ position: 'sticky', top: 0, zIndex: 5 }"
      >
        <!-- 簇头行(§7.2):仅簇首文件夹头并入关联簇信息,Network 图标 + 文本双通道,非簇首不渲染。 -->
        <span v-if="isLensClusterStart(row)" class="dup-folder__cluster">
          <Network :size="12" class="dup-folder__cluster-icon" />
          <span class="dup-folder__cluster-text">{{ lensClusterLabel?.(row) }}</span>
        </span>
        <span class="dup-folder__path">
          <Folder :size="13" class="separator-icon" />
          <span class="separator-text">{{ row.separatorLabel }}</span>
        </span>
        <span v-if="lensFolderStatsText" class="dup-folder__stats">{{ lensFolderStatsText(row) }}</span>
      </h3>
      <div
        v-else
        class="separator-content"
        :style="{
          position: groupBy === 'folder' ? 'sticky' : 'static',
          top: 0,
          zIndex: 5,
        }"
      >
        <component
          :is="groupBy === 'folder' ? Folder : Calendar"
          :size="16"
          class="separator-icon"
        />
        <span class="separator-text">{{ row.separatorLabel }}</span>
        <span
          v-if="separatorCounts"
          v-show="separatorCounts.has(row.groupId ?? row.separatorLabel)"
          class="separator-count"
        >
          {{ separatorCounts.get(row.groupId ?? row.separatorLabel) }}
        </span>
      </div>
    </template>

    <!-- 正常行卡片 -->
    <template v-else>
      <!-- 有意不用 v-memo(R2-3 删除):嵌套 v-for 下 memo 缓存槽按模板位置分配、被外层各行共享,
           Vue 官方明示其在 v-for 内不生效;且原 deps 不含 item.id,grid 模式同尺寸未出图卡片
           deps 全等时会错误复用他项 vnode(串位隐患)。MediaThumb 子组件 props 浅比较已提供等效跳渲。 -->
      <div
        v-for="item in row.items"
        :key="item.id"
        class="media-card"
        :data-item-id="item.id"
        :class="{
          'media-card--selection-mode': !compactCells && selectionMode,
          'media-card--compact': compactCells,
          'media-card--pending-delete': isPendingDelete(item.id),
        }"
        :style="{ width: item.w + 'px', height: item.h + 'px' }"

        tabindex="0"
        @click="onCardClick(item, $event)"
        @keydown.enter.self.prevent="onCardClick(item, $event)"
        @keydown.space.self.prevent="onCardClick(item, $event)"
        @pointerdown="onCardPointerDown(item.id, $event)"
        @contextmenu.prevent="onCardContextMenu($event, item.id)"
      >
        <!-- 暂存删除标记:置灰 + 「待删除」角标,退出选择模式时统一移除(撤销可恢复)。 -->
        <div v-if="isPendingDelete(item.id)" class="media-card__pending-badge">
          {{ pendingDeleteLabel }}
        </div>
        <!-- 重复镜头卡片徽标(§6.2/§7.3):右上角与左下可用态角标对角错开;
             groups 模式 = 组内位次 M/N;folders 模式 = 重复卡片「组 N」文本徽标
             (组员跨目录无组内序,§16)/尚未确认卡片问号图标(不只靠灰色);独有无徽标。
             徽标选择经宿主注入的 lensCardBadgeText(lensSeparator.resolveLensCardBadge),
             问号以图标渲染、其余为文本;仅镜头态渲染,普通画廊行零开销。 -->
        <span
          v-if="lensCardBadgeText && lensCardBadgeText(item)"
          class="media-card__lens-badge"
        >
          <CircleHelp
            v-if="item.duplicateBucket === 'unconfirmed'"
            :size="12"
            class="media-card__lens-badge-icon"
          />
          <template v-else>{{ lensCardBadgeText(item) }}</template>
        </span>
        <!-- 极密(compact)走轻量卡片 MediaThumbCompact(T1 §6):砍 hover/徽章/评分/收藏 +
             不接 isSelectionMode → 进选择态该子组件 props 不变、不重渲染(消 §5.6 残余)。
             非 compact 保留全功能 MediaThumb。两分支挂在同一 .media-card(data-item-id + 全 handlers),
             故框选/拖拽/键盘无差异。§8.1 browse-only 经 browse-only 透传给 MediaThumb:
             镜头态隐藏收藏/评分快捷动作与拖拽手柄。 -->
        <MediaThumbCompact
          v-if="compactCells"
          :id="item.id"
          :w="item.w"
          :h="item.h"
          :media-type="item.mediaType"
          :thumb-status="item.thumbStatus"
          :thumb-path="item.thumbPath"
          :placeholder-color="item.placeholderColor"
          :file-format="item.fileFormat"
          :color-label="item.colorLabel"
          :availability="item.availability"
          :is-selected="isSelected(item.id)"
          :cache-dir="cacheDir"
          @request-thumb="onRequestThumb"
          @cancel-thumb="onCancelThumb"
          @regenerate-thumb="onRegenerateThumb"
        />
        <MediaThumb
          v-else
          :id="item.id"
          :item="item"
          :w="item.w"
          :h="item.h"
          :media-type="item.mediaType"
          :is-live-photo="item.isLivePhoto"
          :duration-ms="item.durationMs"
          :thumb-status="item.thumbStatus"
          :thumb-path="item.thumbPath"
          :placeholder-color="item.placeholderColor"
          :file-format="item.fileFormat"
          :file-size="item.fileSize"
          :similarity="item.similarity"
          :is-favorited="item.isFavorited"
          :rating="item.rating"
          :color-label="item.colorLabel"
          :is-selected="isSelected(item.id)"
          :is-selection-mode="selectionMode"
          :browse-only="!!lensActive"
          :cache-dir="cacheDir"
          @request-thumb="onRequestThumb"
          @cancel-thumb="onCancelThumb"
          @regenerate-thumb="onRegenerateThumb"
          @favorite="onFavorite"
          @rate="onRate"
          @select="onSelect(item.id)"
        />
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
// 行为直通:宿主 handler 以**函数 props** 传入(而非 emits 逐层转发)——签名零重复、
// 无中转样板;`onXxx` 名已声明为 props,Vue 不会误作 listener fallthrough。
// 本组件刻意**不带任何样式**:.media-grid__row/.date-separator(行根,继承父作用域
// 属性)由宿主 scoped 样式直接命中;.media-card/.separator-content 等内部类经宿主
// :deep() 命中——样式单源,与抽取前视觉零差异。
import MediaThumb from './MediaThumb.vue'
import MediaThumbCompact from './MediaThumbCompact.vue'
import { Folder, Calendar, CopyCheck, CircleHelp, Network } from '@lucide/vue'
import { isLensClusterStart } from './lensSeparator'
import type { LayoutRow, LayoutRowItem, LayoutRowSeparator } from '../../types/layout'

defineProps<{
  row: LayoutRow
  /** 行 transform 基准 = 所在段起点 seg.start。 */
  offsetY: number
  gap: number
  groupBy: string
  /** LayoutSummary 提供的分组计数;行载荷不重复携带统计字段。 */
  separatorCounts?: ReadonlyMap<string, number>
  compactCells: boolean
  selectionMode: boolean
  cacheDir: string
  /** 重复镜头激活(duplicateLensStore.mode 非空):普通画廊行无投影字段、徽标函数恒 null。 */
  lensActive?: boolean
  /** 文件夹头统计行文案(§7.2):宿主经 lensSeparator.formatLensFolderStats + t 组装后下发,
   *  本组件保持 i18n 无关(与 pendingDeleteLabel 同姿态);仅 duplicateFolder 行消费。 */
  lensFolderStatsText?: (row: LayoutRow) => string | null
  /** 重复组头文案(§6.1):宿主经结构化数值 + i18n 组装;路径分隔符不走此回调。 */
  lensGroupLabel?: (row: LayoutRowSeparator) => string
  /** 关联簇头文案(§7.1):宿主经结构化数值 + i18n 组装;路径分隔符不走此回调。 */
  lensClusterLabel?: (row: LayoutRowSeparator) => string | null
  /** 卡片镜头徽标文本(§6.2/§7.3):宿主经 lensSeparator.resolveLensCardBadge + t 组装;
   *  null = 无徽标(独有/普通画廊);尚未确认卡由模板按 duplicateBucket 换问号图标渲染。 */
  lensCardBadgeText?: (item: LayoutRowItem) => string | null
  pendingDeleteLabel: string
  isSelected: (id: number) => boolean
  isPendingDelete: (id: number) => boolean
  onCardClick: (item: LayoutRowItem, ev: MouseEvent | KeyboardEvent) => void
  onCardPointerDown: (id: number, ev: PointerEvent) => void
  onCardContextMenu: (ev: MouseEvent, id: number) => void
  onRequestThumb: (id: number) => void
  onCancelThumb: (id: number) => void
  onRegenerateThumb: (id: number) => void
  onFavorite: (id: number) => void
  onRate: (id: number, value: number) => void
  onSelect: (id: number) => void
}>()
</script>
