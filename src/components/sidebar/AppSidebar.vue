<template>
  <nav class="sidebar">
    <!-- 顶栏重构 Phase G: 品牌 logo 已移入自绘标题栏(TitlebarBrand), 侧栏顶部不再置 logo -->

    <!-- 可滚动的手风琴区域。区块标题（经由 AccordionSection 的两根片段）是直接子
         元素，因此能在整个滚动范围内粘顶+粘底并堆叠——见 AccordionSection.vue。 -->
    <div class="sidebar__scroll-area">
      <LibrarySection :order="0" />
      <ToolsSection :order="1" />
      <FoldersSection :order="2" />
      <ManagementSection :order="3" />
      <!--
        新增菜单只需在此再放一个区块，例如 <AlbumsSection :order="4" />。展开态持久化
        与粘性标题堆叠会通过 provideSidebarSections() 自动接入。
      -->
    </div>

    <!-- 固定的设置/主题页脚（滚动区下方） -->
    <SidebarFooter />

    <!-- 共享的、基于 Promise 的确认对话框，为所有区块仅挂载一次。 -->
    <ConfirmDialog />
  </nav>
</template>

<script setup lang="ts">
import { onMounted } from 'vue'
import SidebarFooter from './SidebarFooter.vue'
import LibrarySection from './sections/LibrarySection.vue'
import ToolsSection from './sections/ToolsSection.vue'
import FoldersSection from './sections/FoldersSection.vue'
import ManagementSection from './sections/ManagementSection.vue'
import ConfirmDialog from '../common/ConfirmDialog.vue'
import { provideSidebarSections } from '../../composables/useSidebarSections'
import { useScanStore } from '../../stores/scanStore'
import { useMediaStore } from '../../stores/mediaStore'

// 向所有区块提供手风琴控制器（展开状态 + 粘性计算）。
provideSidebarSections()

const scan = useScanStore()
const media = useMediaStore()

// 数据初始化：加载扫描根目录 + 媒体统计。文件夹树由 FoldersSection 自行加载
//（其对 scan.scanRoots 的 watch 是树加载的唯一来源）。
onMounted(async () => {
  await scan.loadScanRoots()
  await media.loadStats()
})
</script>

<style scoped>
.sidebar {
  /* Single source of truth for sticky-header height + stacking math.
     Inherited by every AccordionSection header (CSS vars pierce scoped styles).
     粘性标题高度 + 堆叠计算的唯一真值来源。被每个 AccordionSection 标题继承
     （CSS 变量可穿透 scoped styles）。 */
  --sidebar-header-h: 32px;
  /* 侧栏对齐网格(排版重构):所有区块共用同一左轨,子菜单内容左缘 = 区块标题文字起点,
     形成「标签在外、内容嵌套」的层级。
     --sidebar-rail   = 标题箭头列起点(标题行左内边距)
     --sidebar-indent = rail + 箭头 14px + gap 8px = 子菜单内容左缘
     消费方:AccordionSection 标题、LibrarySection 图标列、FoldersSection 树箭头列、
     ToolsSection 卡片左缘、ManagementSection 文本左缘。改动须五处同步对齐。 */
  --sidebar-rail: 8px;
  --sidebar-indent: 30px;
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  /* 容器查询锚点:侧栏可拖 180-400px,区块标题的操作钮按侧栏实宽增减
     (FoldersSection 窄态藏低频钮)。宽度由父级 .app-sidebar 显式指定,
     inline-size 包含不会反向影响自身尺寸。 */
  container-type: inline-size;
  container-name: sidebar;
}

.sidebar__scroll-area {
  flex: 1;
  /* `overlay` keeps the scrollbar from shifting layout; `stable` gutter is the
     modern fallback. Plain block flow (no flex) keeps `position: sticky` robust.
     `overlay` 使滚动条不挤压布局；`stable` 槽位是现代浏览器的回退。普通块级流
     （非 flex）让 `position: sticky` 更稳健。 */
  overflow-y: overlay;
  overflow-x: hidden;
  scrollbar-gutter: stable;
}

/* VSCode-style floating scrollbar — invisible until the area is hovered. */
/* VSCode 风格悬浮滚动条——hover 滚动区前不可见。 */
.sidebar__scroll-area::-webkit-scrollbar {
  width: var(--scrollbar-width, 8px);
  background: transparent;
}
.sidebar__scroll-area::-webkit-scrollbar-track {
  background: transparent;
}
.sidebar__scroll-area::-webkit-scrollbar-thumb {
  background: transparent;
  border-radius: var(--radius-full);
}
.sidebar__scroll-area:hover::-webkit-scrollbar-thumb {
  background: var(--color-scrollbar-thumb);
}
.sidebar__scroll-area::-webkit-scrollbar-thumb:hover {
  background: var(--color-scrollbar-thumb-hover);
}
</style>
