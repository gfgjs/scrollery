---
id: 2026-07-25-MediaGrid-vue
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# MediaGrid.vue 拆分方案分析

> 只读分析,零代码改动。目标文件:`src/components/media/MediaGrid.vue`(2865 行,~128KB)。
> 结论先行:该组件是**路由级页面组件**(`src/router/index.ts` 用 `component: () => import(...)` 挂载 8 处,均是画廊类路由的裸组件,**无任何 `defineProps`/`defineEmits`/`defineExpose`**)。这意味着对外契约是空的——拆分不涉及"改变父组件如何使用它",风险完全在**内部**:大量闭包共享同一批 `ref`/`computed`(`gridRef`/`bucketActive`/`activeRows()`/`currentLogicalY` 等),以及本文件的画廊轴/minimap 红线状态(`axisVisible`/`axis_mode`/time 轴线性映射,2026-07-24~25 刚定案)。
> 本文档只做结构移动设计,**不建议任何行为改动**;凡触及红线状态键的部分,移动方案要求"原样搬运、原名保留、原触发时机保留"。

## 0. 关键前提(影响全篇方案的判断)

- **零 props/emits 契约**:拆分子组件时不必担心"破坏父组件调用方式";但新增子组件之间、子组件与根组件之间要新建 props/emits 契约,这是本次拆分**唯一新增的接口面**,须谨慎设计(见 §2、§3)。
- **零组件级测试**:`Glob` 确认仓库只有 `src/components/media/mediaGrid.helpers.spec.ts`(纯函数单测),没有 `MediaGrid.spec.ts`。也就是说当前对 `onGridScroll`/reflow anchor/双引擎切换等"重函数"没有任何自动化回归网,只能靠 vue-tsc + eslint + 人工 GUI 走查兜底。这是拆分时最大的现实风险来源(见 §5)。
- **CSS 越界耦合**:`MediaGridRow.vue` 第 129 行注释明确写"`.media-card`/`.separator-content` 等内部类经宿主(host)scoped 样式命中"——即卡片/分隔符的视觉样式**故意**放在 `MediaGrid.vue` 的 `<style scoped>` 里,通过 Vue scoped 属性传递穿透到子组件渲染的 DOM 上。这意味着"把模板挪到子组件"和"把对应 CSS 挪到子组件"是两件独立的事,不能想当然一起搬(见 §2.3、§3)。

---

## 1. 现状结构图

### 1.1 Template 区块(第 1–435 行)

| 区块 | 行号 | 职责一句话 | 符号锚点 |
|---|---|---|---|
| 语义子视图返回栏 | 5–8 | 「某人物/某收藏夹」子视图的返回入口(点击/ESC) | `backBar`, `exitToOverview` |
| 画廊滚动容器根 | 12–25 | 承载 `gridRef`,统一转发 scroll/wheel/keydown/touchmove 给双引擎 | `gridRef`, `onGridScroll`, `onGridWheel`, `onGridKeydown`, `onGridTouchmove` |
| 空状态 | 30–43 | 零结果时的文案 + CTA(加目录/清筛选) | `emptyState`, `emptyTitle`, `onEmptyAction` |
| 首屏骨架屏 | 51–62 | compute_layout 慢首屏时的占位铺满 | `showSkeleton`, `skeletonCount` |
| Canvas 渲染分支 | 73–105 | `canvasMode` 命中时接管渲染(与下方两分支互斥) | `MediaGridCanvas`, `canvasRows`, `canvasMode` |
| bucket 分段渲染分支 | 107–155 | T16 方案B:等高算术分段 + `MediaGridRow` 复用 | `bucketActive`, `bucketSegments`, `bucketAnchorDelta` |
| 方案A 虚拟滚动分支 | 157–208 | 经典虚拟滚动(压缩坐标平移态) | `visibleRows`, `renderAnchor`, `spacerHeight`, `layerRef` |
| 自绘逻辑滚动条 | 216–225 | bucket 引擎专属,替代原生滚动条 | `MediaScrollbar`, `onScrollbarJump`, `axisScrubbing` |
| 悬浮滚动按钮 | 229–246 | 一键滚到顶/底 | `scrollGridToTop`, `scrollGridToBottom` |
| 时间轴/minimap 侧栏 | 250–300 | 真时间 scrubber / Canvas 原型 / minimap 三选一,`<Transition axis-slide>` 折叠动画 | `timelineVisible`, `minimapVisible`, `timelineCanvasRef`, `onMinimapJump` |
| 轴控制簇(Teleport 到底栏) | 302–385 | chevron 显隐钮 + 形态切换钮 + 渲染/坐标系调试钮,2026-07-24 迁 `#statusbar-axis-outlet` | `axisOpen`, `toggleAxis`, `switchAxisMode`, `isCanvasTimeline` |
| 画廊渲染调试钮 | 387–401 | dev-only DOM↔Canvas 切换药丸 | `showRenderModeDebug`, `toggleGalleryRenderMode` |
| 右键菜单 | 403–409 | 呈现 `ctxMenu` 状态 | `ContextMenu`, `onContextMenu` |
| 选择工具栏 | 411–412 | 数据驱动的批量操作条 | `SelectionToolbar`, `selectionCommands` |
| 移动/复制目标选择弹窗 | 414–419 | 文件夹树选择器 | `FolderTreeSelectorDialog`, `onMoveCopyConfirm` |
| 拖拽幽灵(Teleport 到 body) | 421–432 | 命令式定位的拖图跟随提示,避免逐帧重渲染 | `mediaGhostEl`, `useMediaDragToFolder` |

### 1.2 `<script setup>` 逻辑域(第 437–2311 行,约 1875 行)

按功能聚类(非严格行序,标出主代表符号与大致跨度):

| # | 逻辑域 | 约行数 | 代表符号 |
|---|---|---|---|
| S1 | 导入 + store 初始化 + 拖拽幽灵 refs | 437–571 | `ui`/`config`/`media`/`selection`/`viewIds` 等 store 句柄 |
| S2 | 空状态决策 | 580–627 | `emptyState`, `emptyTitle`, `onEmptyAction` |
| S3 | 滚动闸门基础状态 + dev 渲染开关 + 轴模式状态 | 629–720 | `lastScrollTop`, `GATE_RELEASE_MS`, `useRenderMode`, `canShowScrubber`, `activeAxis`, `timelineVisible`, `switchAxisMode` |
| S4 | 程序化滚动守卫 | 731–757 | `beginProgrammaticScroll`, `endProgrammaticScroll` |
| S5 | 右键菜单构建 | 764–852 | `ctxMenu`, `onContextMenu`(经命令注册表生成 move/copy/export) |
| S6 | 逻辑坐标跳转 + 轴拖拽关闸 | 854–902 | `scrollToY`, `onScrollbarJump`, `onMinimapJump`, `axisScrubbing` |
| S7 | 双引擎构造(核心) | 904–1104 | `layoutSource`(useGalleryLayoutSource)、`bucketActive`、`useVirtualScroll`、`useBucketVirtualScroll`、`currentLogicalY`、`activeRows()`、canvas 相关 computed、引擎切换 watcher |
| S8 | 骨架屏可见性防抖 | 932–972 | `showSkeleton`, `skeletonCount`(嵌在 S7 区间内,声明顺序耦合但逻辑独立) |
| S9 | 重排锚点(Reflow anchor) | 1106–1186 | `captureReflowAnchor`, `restoreReflowAnchor`, `pendingAnchor` |
| S10 | 输入路由转发 + 滚动主处理函数(mega) | 1188–1275 | `onGridWheel/Keydown/Touchmove`, `onGridScroll`(速度采样/闸门/文件夹联动/settle) |
| S11 | 滚到顶/底 | 1277–1285 | `scrollGridToTop`, `scrollGridToBottom` |
| S12 | 挂载期布局初始化 | 1290–1363 | `onMounted`(ResizeObserver、初算、滚动恢复) |
| S13 | 缩略图请求/自愈 | 1365–1415 | `onRequestThumb`, `onCancelThumb`, `onRegenerateThumb` |
| S14 | 可视优先取尺寸 + perf probe 接线 | 1417–1446 | `useViewportDimPriority(...)`, `useGalleryPerfProbe(...)` |
| S15 | 乐观 UI 回写(收藏/评分/色标) | 1448–1525 | `handleFavorite`, `patchVisibleRating`, `patchVisibleFavorite`, `patchVisibleColorLabel`, `itemPatchSignal` watch |
| S16 | 选区批量操作公共出入口 | 1527–1563 | `selectionDescriptor`, `patchVisibleSelected`, `batchColor` |
| S17 | 卸载清理 #1 | 1565–1587 | `onBeforeUnmount`(perf/resize/anchor/闸门定时器) |
| S18 | 拖拽到文件夹接线(逻辑已抽出) | 1592–1602 | `useMediaDragToFolder(...)` |
| S19 | 卡片点击 + 无障碍标签 | 1604–1655 | `handleCardClick`, `cardAriaLabel` |
| S20 | 批量操作全集(最大单域) | 1657–1936 | `batchFavorite/Unfavorite`, `useGridFlipReflow`, `pendingDeleteIds`, `batchDelete/undoDelete/redoDelete/commitPendingReflow`, `startBatchMove/Copy`, `selectionCommands`, `onMoveCopyConfirm` |
| S21 | 返回栏 + ESC + 数字键评分 | 1939–2025 | `backBar`, `exitToOverview`, `onKeyDown` |
| S22 | KeepAlive 生命周期(核心胶水) | 2033–2101 | `onActivated`, `onDeactivated`(焦点回收/滚动恢复/性能面板/keydown 绑定) |
| S23 | Tauri 事件 + 布局脏标监听 | 2103–2217 | `MEDIA_ENRICHED`, `VOLUMES_CHANGED`, `layoutVersion`/`layoutDirty`/`totalItems` watch,viewport meta 懒取 watch |
| S24 | 侧栏点击滚到目录 | 2219–2310 | `scrollToDir`, `pendingScrollWhileComputing` |

### 1.3 `<style scoped>`(第 2313–2865 行,约 552 行)

| 子块 | 行号 | 职责 |
|---|---|---|
| 布局壳(layout/wrapper/skeleton) | 2314–2439 | 根容器、骨架屏、bucket loading 条纹 |
| 行/分隔符/卡片视觉(`:deep()` 穿透) | 2441–2494, 2694–2780 | **越界耦合**:实际渲染在 `MediaGridRow`/`MediaGridCanvas`/`MediaThumb`,样式定义在这里 |
| 轴侧栏与轴控制簇 | 2495–2692 | `timeline-sidebar*`、`axis-slide-*`、`timeline-toggle-btn`、`axis-mode-btn`、`timeline-mode-btn` |
| 画廊渲染调试药丸 | 2635–2667 | `.gallery-mode-btn` |
| 暂存删除态视觉 | 2716–2745 | `.media-card--pending-delete` 相关(同样是越界 `:deep()`) |
| 悬浮滚动按钮 | 2782–2823 | `.scroll-fab`, `.fab-btn` |
| 拖拽幽灵 | 2834–2864 | `.media-drag-ghost*` |

---

## 2. 拆分方案

设计原则:**composable 承载可测的纯状态/逻辑,子组件承载独立 DOM 子树(含其私有 CSS),根组件保留"胶水"**——凡是同时被 3 个以上逻辑域读写的响应式状态(`gridRef`、`bucketActive`、`activeRows()`、`compute`/`updateVisible`),不强行下沉,以免把"一个函数里能看到的耦合"变成"跨 5 个文件才能看到的隐式耦合"。所有 composable 均沿用仓库已有范式(`useVirtualScroll`/`useMediaDragToFolder`):依赖以 getter 函数或 Ref 注入,返回值原名导出,根组件按原变量名解构使用,模板 0 改动。

### 2.1 新增 composables(`src/composables/`)

| 文件 | 承接符号(原样搬运) | 对外接口 | 依赖方向 |
|---|---|---|---|
| `useGalleryEmptyState.ts` | `emptyState`,`emptyTitle`,`emptyDescription`,`emptyActionLabel`,`onEmptyAction` | 输入:无(内部读 `useUiStore`/`useFilterStore`/`useViewStore`/`useI18n`);输出同名五个值 | 叶子,零依赖其他新 composable |
| `useGallerySkeleton.ts` | `SKELETON_DELAY_MS`,`showSkeleton`,`skeletonTimer`,`skeletonCount` | 输入:`containerWidth: () => number`,`viewportHeight: () => number`;输出 `showSkeleton`,`skeletonCount` | 叶子 |
| `useGalleryAxisControls.ts` | S3 中轴相关部分(`canShowScrubber`…`switchAxisMode`)+ `showRenderModeDebug`/`useRenderMode` 接线 + `timelineCanvasRef`/`galleryViewActive` | 输入:无(内部读 store);输出:`axisOpen/toggleAxis/canShowScrubber/canShowMinimap/activeAxis/timelineVisible/minimapVisible/isCanvasTimeline/hasTimeBuckets/timelineAxisLabel/axisToggleTitle/axisModeToggleTitle/switchAxisMode/showRenderModeDebug/galleryRenderMode/timelineRenderMode/toggleGalleryRenderMode/toggleTimelineRenderMode/timelineCanvasRef/galleryViewActive` | 叶子;**红线**——`axisVisible`/`axis_mode` 读写点须逐行核对与原代码字节级一致 |
| `useGalleryVirtualEngine.ts` | S7 全部 + `activatePerformanceBenchmark`/`deactivatePerformanceBenchmark` + `scrollToY` + S12 中 ResizeObserver/onMounted 的"布局初始化"子集 | 输入:`gridRef: () => HTMLElement\|null`,`layerRef`,`bucketContentRef`(均为 getter),`onBeforeResize?: () => void`(用于挂 `captureReflowAnchor`);输出:`layoutSource`,`bucketActive`,`visibleRows`,`bucketSegments`,`spacerHeight`,`renderAnchor`,`currentLogicalY`,`activeRows()`,`canvasMode/canvasRows/canvasPatchTick/bumpCanvasPatchTick/canvasSpacerHeight/canvasSelectionVersion`,`compute/onResize`,`scrollToY`,`containerWidth` | **核心/最高风险**,见 §3.1。多个下游 composable(§2.1 其余项)反过来依赖它的输出(`activeRows`/`compute`/`updateVisible`/`bucketActive`),须最先落地、其余 composable 以参数注入的方式消费其返回值 |
| `useReflowAnchor.ts` | S9 全部 + 卸载清理里 `anchorClearTimer` 那一段 + S3 里"切视图清锚点"的 watch | 输入:`gridRef`,`activeRows: () => LayoutRow[]`,`currentLogicalY: () => number`,`getViewKey: () => string`,`bucketActive: () => boolean`,`scrollToLogicalY`,`logicalToPhysical`(均来自 virtual-engine 的返回值);输出:`captureReflowAnchor`,`restoreReflowAnchor` | 依赖 virtual-engine 的输出,不反向依赖 |
| `useGridItemPatching.ts` | `patchVisibleRating/Favorite/ColorLabel/Selected` | 输入:`activeRows: () => LayoutRow[]`,`bumpCanvasPatchTick: () => void`;输出四个 patch 函数 | 依赖 virtual-engine 的 `activeRows`/`bumpCanvasPatchTick` |
| `useGallerySelectionOps.ts` | S16 + S20 全部 + `handleFavorite`/`handleRate`/`itemPatchSignal` watch(S15 其余部分) | 输入:`compute`,`updateVisible`,`activeRows`,`bucketActive`,`bucketScroll.whenSettled`,`flipRootEl: () => HTMLElement\|null`(即 `bucketActive.value ? bucketContentRef.value : layerRef.value`),`patchVisible*`(来自上一条);输出:`selectionCommands`,`moveCopyDialog`,`onMoveCopyConfirm`,`handleFavorite`,`handleRate`,`batchColor`,`selectionDescriptor`,`pendingDeleteIds`,`isPendingDelete` 等 | 依赖 virtual-engine + item-patching 的输出;**次高风险**,见 §3.2 |
| `useGalleryContextMenu.ts` | S5 全部 | 输入:`activeRows`,`startBatchMove/Copy/Export`(来自上一条);输出:`ctxMenu`,`onContextMenu` | 依赖 selection-ops 的 `startBatchMove/Copy/startExportSelection` |
| `useGalleryKeyboard.ts` | S21 全部(`backBar/exitToOverview/onKeyDown`) | 输入:`selectionDescriptor`,`patchVisibleSelected`,`compute`,`updateVisible`;输出:`backBar`,`exitToOverview`,`onKeyDown`(供 KeepAlive 生命周期挂/卸监听) | 依赖 selection-ops + item-patching |
| `useGalleryScrollGate.ts` | S3 中滚动闸门基础状态 + S4 全部(程序化滚动守卫) | 输入:无(纯定时器/采样状态机);输出:`beginProgrammaticScroll/endProgrammaticScroll/isProgrammaticScroll`,`sampleVelocityGate(st, now, internalHop): boolean`(封装原 onGridScroll 里速度采样+`setDeferThumbLoad`那段判定,返回是否应关闸) | 叶子;**是否把 `onGridScroll` 整体搬入见 §3.3 的两种方案** |
| `useGalleryScrollToDir.ts` | S24 全部 | 输入:`gridRef`,`bucketActive`,`bucketScroll.scrollToLogicalY`,`logicalToPhysical`,`beginProgrammaticScroll/endProgrammaticScroll` | 依赖 virtual-engine + scroll-gate |
| `useGalleryTauriSync.ts` | S23 全部 | 输入:`compute`,`updateVisible`,`containerWidth`,`restoreReflowAnchor`,`viewIds.ensureFresh`,`activeRows` | 依赖 virtual-engine + reflow-anchor |

**不建议抽出、留在根组件的部分**(理由见 §3):
- S12 的 `onMounted` 里"滚动恢复 + `updateVisible()`"收尾片段、S17/S22 的 KeepAlive 生命周期(`onActivated`/`onDeactivated`)——这是把上面 10+ 个 composable 的初始化时序拼起来的**胶水**,本身不到 120 行,但改一行时序就可能复现"骨架闪帧"“滚动位丢失”这类已经踩过坑修复过的真机问题;强行再包一层 composable 只是把胶水从"根组件"搬到"另一个文件的根组件",不减少认知负担,只增加一次跳转。
- `onGridScroll` 主体(默认方案保留在根组件,仅把纯判定逻辑抽给 `useGalleryScrollGate.ts`,见 §3.3)。

### 2.2 新增子组件

| 文件 | 承接模板 | Props(→ 根组件传入) | Emits(→ 根组件消费) | 依赖方向 |
|---|---|---|---|---|
| `GalleryAxisPanel.vue` | 模板 250–385(时间轴/minimap 侧栏 **+** Teleport 轴控制簇,两段绑定同一个 `timelineCanvasRef`,必须整体一起搬,否则要做跨组件 ref 转发) | `totalHeight`, `currentY`, `minThumb`, `monthBuckets`, `separators`, `gridViewportHeight`, `containerWidth`, `isScrolling`, `cacheDir`, `galleryViewActive` | `jump(y, smooth?)`(← 原 `@jump="scrollToY"`),`minimapJump(y)`(← 原 `@jump="onMinimapJump"`),`scrubbing(active)`(← 原 `axisScrubbing = $event`) | 组件内部直接 `useUiStore()`/`useMediaStore()` 读 `axisVisible`/`axisMode`/`groupBy`/`minimapRenderMode`(全局单例,无需 prop 传递),配合 `useGalleryAxisControls.ts` 承载逻辑;根组件保留 `axisScrubbing` ref(供同层的 `MediaScrollbar` 消费,不属于本组件) |
| `GalleryDragGhost.vue`(可选,低优先级) | 模板 421–432(Teleport 到 body 的拖拽幽灵) | 无 | 无(内部自行 `useMediaDragToFolder`,`defineExpose({ onCardPointerDown })`) | 根组件需通过模板 ref 拿子组件暴露的 `onCardPointerDown` 传给 `MediaGridRow`/`MediaGridCanvas`——引入一次 ref 转发,见 §3.4 |
| `GalleryScrollFab.vue`(可选,低优先级) | 模板 229–246(悬浮滚动按钮) | 无 | `scrollTop`,`scrollBottom`(根组件监听后调用现有 `scrollGridToTop/Bottom`,或直接把这两个函数当 props 传入,二选一,倾向 emits 保持单向数据流) | 叶子,不依赖其他新组件 |

**明确不拆的模板区域**(理由见 §3.1):
- `gridRef` 所在的滚动容器整体(模板 12–210,含空状态/骨架屏/Canvas 分支/bucket 分支/方案A 分支)。这块是 `activeRows()`/`onGridScroll`/`restoreReflowAnchor`/`scrollGridToTop`/KeepAlive 焦点恢复等**几十处闭包**共同持有的 `gridRef`,一旦挪进子组件,所有这些闭包都要改成 `() => viewportRef.value?.gridRef ?? null` 的二次转发。收益(减少的行数)远小于风险(一旦某处转发写漏,滚动/虚拟化在生产环境静默失效,且当前无组件级测试兜底)。**建议把"减少这块的可读负担"完全交给 §2.1 的 composable 拆分去做**,模板本身留在根组件。

### 2.3 CSS 处置

- 随 `GalleryAxisPanel.vue` 一起搬:`.timeline-sidebar-wrapper*`、`.axis-slide-*`、`.timeline-toggle-btn`、`.axis-mode-btn`、`.timeline-mode-btn`(2495–2692 区间,约 200 行)——这些选择器只命中该组件自己模板内的元素,无越界耦合,可以安全整体迁移为该组件自己的 `<style scoped>`。
- 随 `GalleryScrollFab.vue`(若做)一起搬:`.scroll-fab`/`.fab-btn`(2782–2823,约 42 行)。
- 随 `GalleryDragGhost.vue`(若做)一起搬:`.media-drag-ghost*`(2834–2864,约 30 行)。
- `.gallery-mode-btn`(2635–2667)绑定的按钮(模板 387–401)未被移入 `GalleryAxisPanel`(它是独立的"画廊引擎"调试钮,和轴控制簇是两回事,只是物理相邻),**留根组件**,对应 CSS 也留根组件。
- **不能搬的部分**:`:deep(.media-card*)`/`:deep(.separator-content)`/`:deep(.media-card--pending-delete)` 等(2441–2494、2694–2780、2716–2745)。这些选择器命中的 DOM 由 `MediaGridRow.vue`/`MediaGridCanvas.vue` 渲染,却由根组件的 scoped 属性"隔空穿透"命中——这是 Vue scoped CSS 的既有设计(`MediaGridRow.vue` 第 129 行注释已明确指出"经宿主 scoped 样式命中")。这部分**必须继续留在 `MediaGrid.vue` 的 `<style scoped>` 里**,不随任何模板搬迁而移动,否则卡片视觉(hover 放大、compact 降级、待删态置灰)会在子组件里失去宿主的 scoped attribute 而整体失效。这是本次拆分**唯一"模板搬了但 CSS 不能跟着搬"**的例外,必须在实施 PR 描述里显式标注,防止后续维护者顺手"归位"引入回归。

---

## 3. 风险与不变量

### 3.1 `useGalleryVirtualEngine.ts` 是全篇最高风险项
它是 `gridRef`/`layerRef`/`bucketContentRef` 三个模板 ref 与两套虚拟滚动引擎(`useVirtualScroll` 方案A / `useBucketVirtualScroll` bucket 分段)之间的唯一交汇点,`bucketActive` 这一个 computed 决定了后续几乎所有函数的分支(`activeRows()`/`scrollToY`/`restoreReflowAnchor`/`onGridScroll`/KeepAlive 恢复滚动位……全部要判 `bucketActive.value ? A : B`)。拆分时的具体不变量:
- `bucketActive` 的判定条件(`ui.bucketSegmentedScroll && media.totalRows > 0`)与两个引擎各自的 `enabled: () =>` 互斥关系**不能变**——这是"方案A与bucket引擎互斥、开关即回退"的既有设计,拆分只是换文件,`enabled` 回调引用的仍必须是**同一个** `bucketActive` computed 实例(不能在两个文件里各建一份,否则可能出现一帧内两套引擎同时 enabled 的竞态)。
- 引擎切换时的滚动位保持 watcher(1094–1104,`watch(bucketActive, async (nowBucket) => …)`)读的是"离开方"的**当前**逻辑位、写的是"进入方"的物理/逻辑位——这段时序注释里写明了"方案 A 的重取由其 enabled watch 自触发……读到的已是回设后的 scrollTop",移动到 composable 内部时,`await nextTick()` 的相对位置**一个字符都不能挪**。
- `canvasCapable`(iOS 强制 DOM + `totalHeight <= resolveSafeMax()`)与画廊轴的 `9cd4bfb` time 坐标线性映射改动共享同一个"总高是否进映射态"的判据——若后续要改这一判据,必须先读 `docs/planning` 里 R-5 那条裁决,而不是在拆分时顺手"顺便统一"。

### 3.2 `useGallerySelectionOps.ts` 的 undo/redo 与 FLIP 动画时序
`batchDelete → stageDeleted → (退出选择态 watcher) → commitPendingReflow → fadeOutCells → flipReflow(async () => { compute(); updateVisible(); await bucketScroll.whenSettled() })` 这条链的**执行顺序**是行为契约(用户反馈驱动的"删除不立即重排,退出选择才一次性重排"设计),抽成 composable 时函数体内部顺序原样保留即可零风险;真正的风险在**跨文件的隐式依赖**——`watch(() => selection.isSelectionMode.value, …)` 这个 watcher 若被拆到 composable 里,而 `pendingDeleteIds` 状态也在同一 composable,没有问题;但如果实现时图省事把这个 watcher 留在根组件、把 `commitPendingReflow` 抽出去,就会出现"根组件 watcher 调用另一文件函数"的额外一跳,建议整条链**连 watcher 一起搬**,不要拆开。

### 3.3 `onGridScroll` 的去留:两个方案二选一
- **方案 A(推荐,默认)**:`onGridScroll` 函数体整体留在根组件,只把"纯判定"部分(速度阈值滞回、程序化滚动守卫状态机)抽给 `useGalleryScrollGate.ts`。理由:此函数同时读 `bucketActive`/`bucketScroll.onScroll()`/`media.layoutSummary.separators`/`ui.groupBy`/`currentLogicalY`/`scrollCache`/`getViewKey()`,是天然的"多域汇合点";抽成 composable 需要注入 6+ 个依赖,注入列表本身比函数体还难读,不符合"降低认知负担"的拆分初衷。
- **方案 B(可选,后续再评估)**:连同 `onGridScroll` 一起注入依赖搬入 `useGalleryScrollGate.ts`。仅当 §5 的特征化测试补齐、且团队认为"根组件行数"比"依赖注入列表长度"更值得优化时才考虑。
- 两个方案都**不改变** `GATE_RELEASE_MS`/`INTERNAL_HOP_CHAIN_MS`/`SCROLLBAR_DRAG_CHAIN_MS` 等既有阈值常量与滞回逻辑(`shouldDeferThumbLoad` 已是 `mediaGrid.helpers.ts` 里的纯函数、已有单测,不受本次拆分影响)。

### 3.4 Ref 转发的通用陷阱
`GalleryDragGhost.vue`(若做)与 `GalleryAxisPanel.vue` 内的 `timelineCanvasRef` 都涉及"子组件内部 template ref → 根组件需要访问"的模式。仓库已有先例(`timelineCanvasRef` 目前就是这样通过 `InstanceType<typeof TimelineScrubberCanvas>` + `defineExpose` 工作的),风险可控,但每多一层转发就多一处"组件未挂载时该 ref 为 `null`"的判空点,newcomer 容易漏判导致运行时报错而非编译期报错(TS strict 下 `.value` 上的可选链能兜住,但逻辑上仍要保证判空分支不改变原有行为——例如 `timelineCanvasRef?.visualMode ?? 'bars'` 这类默认值,原样照抄即可,不需要也不应该"优化"判空写法)。

### 3.5 与 `MediaGridCanvas`/`TimelineScrubberCanvas` 的既有契约不变
两个 Canvas 子组件当前接收的全部 props/emits(如 `MediaGridCanvas` 的 `rows`/`current-y`/`selection-version`/`patch-tick`,`TimelineScrubberCanvas` 的 `month-buckets`/`total-height`/`current-y` + `defineExpose` 的 `visualMode`/`coordApplies`/`effectiveCoord`/`cycleVisualMode`/`toggleCoordMode`)**本次拆分不触碰、不新增、不改名**。所有新 composable/新组件只是把"喂给这些 props 的值从哪里计算出来"这件事挪了文件,计算出的值本身、传递时机、字段名全部保持字节级一致。

### 3.6 响应式边界总览(一句话清单)
- `bucketActive`/`activeRows()`/`currentLogicalY` 三者必须来自同一个 `useGalleryVirtualEngine` 实例,不得在多个 composable 里各自 `computed` 出"看起来一样"的第二份。
- `gridRef`/`layerRef`/`bucketContentRef` 三个模板 ref 只能声明在根组件(或唯一拥有对应 DOM 的组件)里,其余消费方一律以 getter 函数注入,不做值拷贝。
- `canvasPatchTick`/`selectionEpoch`(→ `canvasSelectionVersion`)是"信号型" ref,消费方只关心"变没变",不得在拆分时改成传递具体 diff——这是 2026-07-10 审查 B10 定案的契约(注释已写明"count 别名……曾致 canvas 漏重绘")。

---

## 4. 收益与优先级

### 4.1 拆后预估文件大小

| 文件 | 预估行数 |
|---|---|
| `MediaGrid.vue`(根组件,拆完后) | ~950–1100(模板 ~300 + script 胶水 ~350–450 + 越界 CSS ~250) |
| `GalleryAxisPanel.vue` | ~500–550(模板 ~135 + script ~100 + style ~300) |
| `GalleryDragGhost.vue`(可选) | ~70 |
| `GalleryScrollFab.vue`(可选) | ~70 |
| `useGalleryVirtualEngine.ts` | ~320–380 |
| `useGallerySelectionOps.ts` | ~300–330 |
| `useGalleryAxisControls.ts` | ~150 |
| `useReflowAnchor.ts` | ~110 |
| `useGalleryContextMenu.ts` | ~100 |
| `useGalleryTauriSync.ts` | ~110–130 |
| `useGalleryKeyboard.ts` | ~90 |
| `useGalleryScrollToDir.ts` | ~95 |
| `useGalleryScrollGate.ts` | ~120–140(方案A)/ ~260(方案B,含 onGridScroll) |
| `useGridItemPatching.ts` | ~90 |
| `useGalleryEmptyState.ts` | ~55 |
| `useGallerySkeleton.ts` | ~50 |

根组件从 2865 行降到约 950–1100 行(降幅 ~62%),且剩余部分绝大多数是"胶水时序"(挂载/激活/停用/watcher 编排),而非可复用的独立算法——这正是页面级组件应有的形态。

### 4.2 建议施工顺序(批次)

1. **批次 1(低风险,可并行)**:`useGalleryEmptyState.ts`、`useGallerySkeleton.ts`、`useReflowAnchor.ts`、`useGridItemPatching.ts`。四者互不依赖(除 reflow-anchor 依赖 virtual-engine 的输出,需等批次 2 先落地其接口签名,但可以先写好函数体、用参数占位),风险极低,建议**先做**以验证"拆分-回归"这套 CI 流程本身是否顺畅。
2. **批次 2(核心,必须单独一轮、单独验证)**:`useGalleryVirtualEngine.ts`。这是所有下游的地基,建议独立一个 PR,跑完整 GUI 手测清单(见 §5)后再继续。
3. **批次 3(中风险,依赖批次 2)**:`useGalleryAxisControls.ts` + `GalleryAxisPanel.vue`(轴红线相关,建议找近期做过画廊轴线的人复核 diff)、`useGalleryContextMenu.ts`、`useGalleryKeyboard.ts`、`useGalleryScrollToDir.ts`、`useGalleryTauriSync.ts`。
4. **批次 4(中高风险)**:`useGallerySelectionOps.ts`(undo/redo + FLIP 时序,建议单独一轮、手测全部批量操作 + 撤销)。
5. **批次 5(可选/低优先级)**:`GalleryDragGhost.vue`、`GalleryScrollFab.vue`、`useGalleryScrollGate.ts` 方案B(若决定做)。这批收益小、ref 转发/依赖注入代价相对高,可以不做。

---

## 5. 验证策略

### 5.1 静态检查(每批次必跑)
- `vue-tsc --noEmit`(或仓库既有的 `npm run type-check`):TS strict + no any,新增 composable 的入参/返回值类型必须显式标注(尤其 getter 函数签名),防止隐式 `any` 从 store 里"渗出"。
- ESLint + Prettier(`npm run lint:fix` 收敛格式,不跑仓库级 `npm run format`)。
- 编译期即可捕获的一类回归:props/emits 命名拼写错误、composable 返回值解构名对不上——这类错误在当前"零组件测试"现状下是**唯一**能在合并前拦住的信号,必须保证每批次 vue-tsc 全绿。

### 5.2 单元/特征化测试(建议随拆分新增)
- 沿用 `mediaGrid.helpers.spec.ts` 的既有范式,为新抽出的**纯函数**部分(如 `useGalleryScrollGate.ts` 里的速度判定包装、`useReflowAnchor.ts` 里不含 DOM 副作用的分支)补单测。
- 对 `useGallerySelectionOps.ts` 的 `batchDelete/undoDelete/redoDelete/commitPendingReflow` 这条链,建议在拆分**之前**先写一版特征化测试(mock `invokeIpc`/`historyStore`,断言调用顺序与参数),再做搬迁,搬完跑同一份测试验证零回归——这是项目规则"改动未测试的关键行为前先补特征化测试"的直接适用场景,即便本次任务只是分析,也应作为实施阶段的前置步骤写进计划。
- `useGalleryVirtualEngine.ts` 的双引擎切换 watcher、`useGalleryAxisControls.ts` 的 `axisVisible`/`axisMode` 分支,建议至少补"给定 store 状态组合 → 断言各 computed 输出"级别的浅层测试(不需要真实 DOM,只测计算逻辑),覆盖住画廊轴红线关心的几个状态位。

### 5.3 GUI 手测清单(不可自动化部分,每批次收尾各跑一遍相关子集)
1. 画廊 DOM/bucket/Canvas 三种渲染模式来回切换,滚动位置不丢、不跳变。
2. 拖动滚轮/触屏快滚(飞掠)时缩略图闸门表现不变(慢滚立即出图、快滚推迟、停稳补齐)。
3. 时间轴 ↔ minimap 切换(`switchAxisMode`)、显隐折叠动画(chevron)、Canvas 时间轴视觉形态/坐标系循环钮,均在底栏 `#statusbar-axis-outlet` 内正常呈现且状态跨会话持久(`axisVisible`/`axisMode` 写盘键不变)。
4. 拖拽图片到侧栏文件夹树(幽灵跟随、命中高亮、落点移动)。
5. 批量操作全套:收藏/取消收藏、评分(含数字键 0-5 快捷键)、色标设置/清除、删除(暂存置灰→退出选择态一次性重排→撤销 chip→再次重做)、移动/复制(弹窗选目录)、导出、加入收藏夹。
6. 右键菜单在多选态(已选中项 vs 未选中项)下 move/copy/export 的选区保留/替换语义。
7. ESC 在"人物/收藏夹子视图"下返回总览页 vs 多选态下优先清空选区。
8. 从画廊进入 `/view` 查看器再返回(KeepAlive 激活/失活):滚动位保持、键盘监听正确切换、性能面板正确注册/注销、失活期折叠文件夹变更后返回能补算一次。
9. 侧栏点击文件夹跳转(含"计算布局中点击"排队场景、跳到有媒体的子孙文件夹场景)。
10. 窗口宽度调整(拖拽分栏/resize):重排锚点生效,浏览项不因换行重排而跳走。

---

## 附注:CSS 外置(D-451,2026-07-25 补录)
- 本组件 `<style scoped>` 可整块外置为同目录 `MediaGrid.styles.css`,SFC 留 `<style scoped src="./MediaGrid.styles.css"></style>`;外置文件仍编译为宿主组件 style 块,scope id 归属不变,`:deep()` 穿透语义不变。
- 前提(2026-07-25 核实):全仓 .vue 样式零 `v-bind()`;本文件为单一 `<style scoped>` 块。
- 定位:可选先行批、全场风险最低的行数削减刀;不替代 script 拆分主刀。
- 施工顺序:首刀拿最小文件实测 Vite 构建链 + HMR,通过后铺开。
- §2.3 裁定"必须留宿主"的 `:deep(.media-card*)` 块(~250 行)同样适用外置——不能搬进子组件≠不能搬出文件;axis panel/fab/ghost 的 CSS 仍按 §2.3 随子组件走,外置只处理留守块。

---

## 顺手发现

无。(按范围约束仅做结构分析,发现的所有既有设计权衡已在正文标注为"既有契约/红线",未发现计划外的代码缺陷。)
