---
id: 2026-07-25-FoldersSection-vue
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
target: src/components/sidebar/sections/FoldersSection.vue
size_snapshot: 83KB / 1839 行(2026-07-25,并行注释精简会使数字缓降;锚点以符号名为主、行号区间为辅)
---

# 拆分方案:FoldersSection.vue

## 0. 结论摘要
`FoldersSection.vue` 是侧栏「文件夹」区块的单体组件:虚拟化文件树渲染 + 键盘导航(WAI-ARIA tree) +
拖拽移动/复制(目录与文件两路)+ 分页自动加载 + 树的加载/刷新生命周期 + 根目录管理(新增/迁移/新建
子文件夹)全部塞在一个 `<script setup>` 里。纯逻辑(路径身份判定、拍平、窗口计算、粘性头链、键盘决策)
已经在此前 T1 系列里下沉到同目录 `folderTree.helpers.ts`(420 行,已有 `folderTree.helpers.spec.ts`
单测覆盖),但**有状态的编排代码**(ref/watch/生命周期/DOM 交互)仍全部留在组件本体,这是本文件真正
的体量来源。

建议按「关注点」拆成 **7 个 composable + 2 个下沉到既有 helpers 的纯函数**,组件本体只保留:store/
composable 装配、`displayRows`、场景菜单(三态显示)UI 状态、行激活的路由决策(`onNodeClick` /
`onFileClick` / `navigateToFolder` / `showAll`)。template 与 style 不建议拆(见 §2.5 的理由),只有
script 部分被拆分。

---

## 1. 现状结构图

### 1.1 script setup 逻辑域(按文件内出现顺序)

| # | 逻辑域 | 职责 | 约行数 | 符号锚点 |
|---|--------|------|--------|----------|
| 1 | 装配层 | imports、defineProps、6 个 store、`useFolderTree`/`useConfirm`/router/i18n 初始化 | ~80 | `defineProps<{ order`、`const folderTree = useFolderTree(` |
| 2 | 行拍平 | dirs/files 交错拍平为显示行(委托 `flattenFolderRows`) | ~12 | `const displayRows = computed(` |
| 3 | 树虚拟化核心 | ROW_H/TREE_INDENT/BUFFER 常量、`treeRef`/`scrollAreaEl`、窗口计算 `updateWindow`/`scheduleUpdate`、行高实测、粘性区块标题堆叠插位(`stackTopPx`/`stackBottomPx`,依赖 `useSidebarSections`)、`initScroll`/mount-unmount 绑定 | ~145 | `const ROW_H = 28`、`function updateWindow`、`function initScroll`、首个 `onBeforeUnmount` |
| 4 | 滚动到节点 | 结构轴(`nodeKey`)滚动定位,供粘性头点击/树同步使用 | ~20 | `async function scrollTreeToNodeKey` |
| 5 | 键盘导航 | `activeIndex`/`activeDescId`/`rowDomId`、`setActiveIndex`/`scrollRowIntoView`、`onTreeFocus`/`onTreeKeydown`(委托纯函数 `treeKeyTarget`) | ~75 | `const activeIndex = ref(-1)`、`function onTreeKeydown` |
| 6 | 文件图标/tooltip | `fileIcon`、`fileTitle`(近纯函数,仅需 `t` 与 `isMobilePlatform`) | ~30 | `function fileIcon`、`function fileTitle` |
| 7 | 文件行激活 | 单击选中/打开路由(`openMediaRoute`)、双击预览或 reveal(`REVEAL_TREE_ENTRY` IPC) | ~60 | `function onFileClick`、`function onFileDblClick` |
| 8 | 树↔画廊 目录同步 | `treeSyncTargetId` 计算、latest-wins 队列 `createFolderTreeSyncQueue`、`requestTreeSync` | ~20 | `const treeSyncTargetId = computed(`、`function requestTreeSync` |
| 9 | 树加载/重载生命周期 | `scan.visibleScanRoots` watch(唯一加载入口)、`pendingSelectRootId`、`reloadTreePreserveExpansion`、`folder-stats-changed`/`request-add-folder` 窗口事件 | ~85 | `watch(\n  () => scan.visibleScanRoots,`、`async function reloadTreePreserveExpansion`、`function onFolderStatsChanged` |
| 10 | 节点点击/导航 | `onNodeClick`(模式 A 滚动锚点 / 模式 B 可寻址路由)、`navigateToFolder`、`showAll` | ~45 | `function onNodeClick`、`function navigateToFolder`、`function showAll` |
| 11 | 显示范围三态菜单 | `SCOPE_OPTIONS`、`scopeMenuOpen`/`scopeAnchor`、`toggleScopeMenu`/`chooseScope` | ~30 | `const SCOPE_OPTIONS`、`function chooseScope` |
| 12 | 显示模式切换 + 刷新 | `watch(ui.treeDisplayMode)` 整树重载、`refreshTree`(失效缓存+重载) | ~30 | `watch(\n  () => ui.treeDisplayMode,`、`async function refreshTree` |
| 13 | 展开/折叠全部 | `anyExpanded`、`toggleExpandAll` | ~6 | `const anyExpanded`、`async function toggleExpandAll` |
| 14 | 「加载更多」自动接力分页 | `loadingMoreDirKey`/`autoLoadBlocked`、`loadMore`/`onLoadMore`、静止门 `maybeAutoLoadMore`/`scheduleSettleRetry`、滚动补偿 | ~75 | `const loadingMoreDirKey = ref`、`function maybeAutoLoadMore` |
| 15 | 拖拽索引 + 目录落点判定 | `nodesById`/`nodesByKey`(双身份索引)、`isDescendant`/`canDropOnId`、`recomputeDrop` | ~65 | `const nodesById = computed(`、`function canDropOnId` |
| 16 | 拖拽边缘自动滚动 | `EDGE_ZONE`/`EDGE_MAX_SPEED`、`autoScrollStep`/`updateAutoScroll`/`stopAutoScroll` | ~50 | `function autoScrollStep`、`function stopAutoScroll` |
| 17 | 目录拖拽手势 + 落地 | `onTreePointerDown`(阈值/ghost/suppressClick)、`performTreeDrop`(history.move/copy) | ~70 | `function onTreePointerDown`、`async function performTreeDrop` |
| 18 | 文件拖拽手势 + 落地 | `recomputeFileDrop`、`onFilePointerDown`、`performFileDrop`(history.moveMedia/copyMedia) | ~95 | `function recomputeFileDrop`、`async function performFileDrop` |
| 19 | 右键菜单 + 对话框状态 | `contextMenu`/`createDialog`/`filePreview`、`onNodeContextMenu` | ~35 | `const contextMenu = ref(`、`function onNodeContextMenu` |
| 20 | 重链接扫描根 | `RelinkResult`、`relinkRoot`(选路径→二次确认→IPC→按 code 分流→兜底重扫) | ~80 | `interface RelinkResult`、`async function relinkRoot` |
| 21 | 新建文件夹 | `createNewGlobalFolder`、`onFolderCreated` | ~10 | `function createNewGlobalFolder`、`function onFolderCreated` |
| 22 | 导入扫描根 | `OverlapInfo`/`FolderOverlapResult`、`addRoot`(重叠检测→添加→自动扫描→自动选中) | ~55 | `interface OverlapInfo`、`async function addRoot` |

### 1.2 template(约 291 行)

| 区块 | 职责 | 约行数 | 锚点 |
|------|------|--------|------|
| 头部操作插槽 | 全部/显示范围菜单钮/展开折叠全部/新建空白文件夹/添加文件夹 5 个控件 | ~38 | `<template #actions>` |
| 空态 | 无可见根时的占位文案 | ~6 | `class="empty"` |
| 虚拟化树容器 + 粘性头链 | `role="tree"` 容器、aria-activedescendant、粘性目录头覆盖层(T1-b) | ~35 | `class="tree"`、`class="tree-sticky"` |
| 树行(dir/file/more) | 三种行的按钮渲染,承载全部 class 绑定(active/drag-over/kb-active/is-hidden 等)与事件 | ~110 | `class="tree-layer"`、`row.kind === 'dir'` |
| 显示范围三态菜单 | `UiPopover` + `menuitemradio` 单选集 + 刷新 | ~30 | `<UiPopover v-model:open="scopeMenuOpen"` |
| 右键菜单 / 预览 / 新建对话框 | `ContextMenu`、`FilePreviewDialog`、Teleport 到 body 的 `FolderCreateDialog` | ~25 | `<ContextMenu`、`<FilePreviewDialog` |
| 拖拽浮动预览 | `pointer-events:none` 的 ghost 标签 | ~12 | `class="drag-ghost"` |

### 1.3 style scoped(约 340 行)

| 区块 | 锚点 |
|------|------|
| 头部操作窄侧栏适配 + show-all 胶囊 | `.folders-action`、`.show-all-btn` |
| 三态菜单视觉 | `.scope-menu`、`.scope-item` |
| 空态 | `.empty` |
| 树容器/虚拟化层/粘性头/目录行 | `.tree`、`.tree-layer`、`.tree-sticky`、`.tree-item` |
| 文件叶子行 | `.file-item` |
| 「加载更多」行 | `.more-item` |
| 行内构件(箭头/图标/标签/计数) | `.tree-arrow`、`.tree-chevron`、`.tree-count` |
| 拖拽浮动预览视觉 | `.drag-ghost` |

Style 内部耦合度低、按 class 前缀天然分区,不是体量主因(340 行本身不算超长),故不建议拆分——见 §2.5。

---

## 2. 拆分方案

### 2.1 新增 composable 清单(均放 `src/composables/`,与既有 `useFolderTree.ts` 同级)

| 文件 | 职责 | 迁移符号(来自现状表 §1.1) | 对外接口(输入 → 输出) |
|------|------|---------------------------|------------------------|
| `useFolderTreeIndices.ts` | 目录节点的双身份索引(D-013:实体轴 id / 结构轴 nodeKey)+ 祖先判定 | 域 15 的 `nodesById`/`nodesByKey`/`isDescendant`(`canDropOnId` 留在拖拽 composable,见下) | 入:`nodes: ComputedRef<DirNode[]>`。出:`{ nodesById, nodesByKey, isDescendant }` |
| `useFolderTreeVirtualization.ts` | 行窗口化、行高实测、粘性区块堆叠插位、粘性目录头链、结构轴滚动定位;是体量最大也是其余多数 composable 的基础设施 | 域 3、域 4 全部 | 入:`displayRows: ComputedRef<TreeRow[]>`, `sectionId: 'folders'`(或直接注入 `useSidebarSections()`)。出:`{ treeRef, spacerHeight, offsetY, visibleRows, startIndex, firstVisibleIndex, stickyRows, rowH, stackTopPx, stackBottomPx, treeOffsetTop, scrollAreaEl(shallowRef,供其余 composable 读/写 scrollTop), scheduleUpdate, updateWindow, scrollTreeToNodeKey, getLastScrollTs() }` |
| `useFolderTreeKeyboardNav.ts` | WAI-ARIA tree 键盘决策的副作用层(域 5) | `activeIndex`/`activeDescId`/`rowDomId`/`setActiveIndex`/`scrollRowIntoView`/`onTreeFocus`/`onTreeKeydown` | 入:`displayRows`, virtualization 的 `{ treeOffsetTop, scrollAreaEl, rowH, stackTopPx, stackBottomPx }`,行激活回调 `{ onNodeClick, onFileClick, onFileDblClick, onLoadMore, toggleNode: folderTree.toggleNode }`。出:`{ activeIndex, activeDescId, setActiveIndex, onTreeFocus, onTreeKeydown }` |
| `useFolderTreeAutoLoadMore.ts` | 「加载更多」自动接力 + 快滚静止门 + 滚动补偿(域 14) | `loadingMoreDirKey`/`autoLoadBlocked`/`loadMore`/`onLoadMore`/`maybeAutoLoadMore`/`scheduleSettleRetry` | 入:`displayRows`, `visibleRows`, `nodesByKey`, virtualization 的 `{ scrollAreaEl, rowH, firstVisibleIndex, updateWindow, getLastScrollTs }`, `folderTree.loadMoreFiles`。出:`{ loadingMoreDirKey, onLoadMore }` + 内部 `watch(visibleRows, maybeAutoLoadMore)` |
| `useFolderTreeDragDrop.ts` | 目录 + 文件两路指针拖拽、共用的 ghost/边缘自动滚动/落点高亮(域 16、17、18,以及域 15 的 `canDropOnId`/`recomputeDrop`) | `dragId`/`dropId`/`dragFileId`/`ghost`/`canDropOnId`/`recomputeDrop`/`recomputeFileDrop`/自动滚动全部/`onTreePointerDown`/`performTreeDrop`/`onFilePointerDown`/`performFileDrop` | 入:`nodesById`/`nodesByKey`(来自 indices), virtualization 的 `scrollAreaEl`, `history`/`toast`/`t`。出:`{ dragId, dropId, dragFileId, ghost, onTreePointerDown, onFilePointerDown, consumeSuppressClick(): boolean }`(`suppressClick` 内部化,见 §3 风险①) |
| `useFolderTreeSync.ts` | 树的加载/重载/模式切换/刷新/展开折叠全部生命周期(域 8、9、12、13) | `treeSyncTargetId`/`treeSyncQueue`/`requestTreeSync`/`pendingSelectRootId`/`reloadTreePreserveExpansion`/`onFolderStatsChanged`/`onRequestAddFolder`/两个 mount 监听/`watch(ui.treeDisplayMode)`/`refreshTree`/`anyExpanded`/`toggleExpandAll` | 入:`folderTree`, `scan`, `media`, `ui`, `viewStore`, 回调 `navigateToFolder`(由组件本体传入,见 §3 风险②), `addRoot` 触发的 `onRequestAddFolder → addRoot()` 需要反向回调(见下)。出:`{ anyExpanded, toggleExpandAll, refreshTree, reloadTreePreserveExpansion, scopeMenuOpen 相关不含 }` |
| `useFolderRootActions.ts` | 根目录管理:导入/迁移/新建子文件夹 + 右键菜单构建(域 19、20、21、22) | `contextMenu`/`createDialog`/`filePreview` 的 ref 声明可留组件本体(模板直接绑定,见下)、`onNodeContextMenu`/`relinkRoot`/`createNewGlobalFolder`/`onFolderCreated`/`addRoot` + 三个 interface | 入:`scan`/`media`/`toast`/`confirm`/`t`, `reloadTreePreserveExpansion`(来自 sync composable)。出:`{ contextMenu, createDialog, onNodeContextMenu, relinkRoot, createNewGlobalFolder, onFolderCreated, addRoot }` |

补充下沉(不新建文件,并入既有纯函数文件):

| 目标文件 | 迁移符号 | 理由 |
|----------|----------|------|
| `folderTree.helpers.ts` | `fileIcon`、`fileTitle`(域 6) | 已是本组件纯逻辑的归宿,`fileTitle` 只需把 `t`/`isMobilePlatform` 判断结果作为参数传入即可去状态化,与文件里其余纯函数同质 |

组件本体(`FoldersSection.vue`)保留的 script 内容:装配层(域 1)、`displayRows`(域 2)、文件行激活路由决策
`onFileClick`/`onFileDblClick`/`onNodeClick`/`navigateToFolder`/`showAll`(域 7、10 —— 这两域涉及
router/viewStore 的路由裁决,是组件对外行为契约的核心,建议留在本体而非再拆一层,风险收益比不高)、显示范围
三态菜单 UI 状态(域 11,体量小且只服务本组件模板)、七个 composable 的装配调用。

### 2.2 依赖方向

```mermaid
flowchart TB
  FS["FoldersSection.vue<br/>(装配 + displayRows + 行激活路由 + 三态菜单)"]
  IDX["useFolderTreeIndices"]
  VIRT["useFolderTreeVirtualization"]
  KBD["useFolderTreeKeyboardNav"]
  LOAD["useFolderTreeAutoLoadMore"]
  DRAG["useFolderTreeDragDrop"]
  SYNC["useFolderTreeSync"]
  ROOT["useFolderRootActions"]
  HELP["folderTree.helpers.ts(既有,纯函数)"]
  FT["useFolderTree.ts(既有,树数据源)"]

  FS --> IDX
  FS --> VIRT
  FS --> KBD
  FS --> LOAD
  FS --> DRAG
  FS --> SYNC
  FS --> ROOT
  KBD --> VIRT
  LOAD --> VIRT
  LOAD --> IDX
  DRAG --> IDX
  DRAG --> VIRT
  ROOT --> SYNC
  VIRT --> HELP
  KBD --> HELP
  LOAD --> HELP
  DRAG --> HELP
  SYNC --> FT
  IDX --> FT
```

要点:
- `useFolderTreeVirtualization` 是唯一的“基础设施层”,`KeyboardNav`/`AutoLoadMore`/`DragDrop` 都读它的
  `scrollAreaEl`/`rowH`/滚动定位函数,但**互相之间不直接依赖**——三者是虚拟化之上的并列关注点。
- `useFolderRootActions` 依赖 `useFolderTreeSync` 暴露的 `reloadTreePreserveExpansion`(迁移/新建后都要
  重载保留展开态);方向单向,`Sync` 不反向依赖 `RootActions`。
- 组件本体是唯一的横向装配点,不出现 composable 互相 import 组件本体的反向依赖。

### 2.3 需要的接口调整(结构性,非行为变更)

1. `scrollAreaEl` 现状是模块作用域的 `let` 裸变量(非响应式)。要跨 composable 共享读写(拖拽边缘自动
   滚动要写 `scrollTop`,键盘导航/自动加载都要读),需要在 `useFolderTreeVirtualization` 内改为
   `shallowRef<HTMLElement | null>` 导出。运行时行为不变,只是把闭包变量包了一层 ref 以便传递。
2. `suppressClick` 现状是组件作用域共享的 `let` 布尔:`onTreePointerDown`/`onFilePointerDown`(未来在
   `DragDrop` composable 内)置位,`onNodeClick`/`onFileClick`(留在组件本体)读并复位。拆分后建议由
   `DragDrop` composable 内部持有该状态,对外只暴露 `consumeSuppressClick(): boolean`(读取并复位,
   语义与现状完全一致),组件本体的点击处理器改调用这个方法而非直接摸变量。
3. `lastScrollTs`(静止门判据)现状在 virtualization 的 `onScroll` 里打时间戳、在自动加载模块里读。拆分后
   由 `useFolderTreeVirtualization` 暴露 `getLastScrollTs(): number` 取值函数,`AutoLoadMore` 每次探测
   时调用,而非共享裸变量。
4. `onFolderCreated`(新建文件夹后重载)、`onRequestAddFolder`(画廊空态转发的 `request-add-folder`
   事件 → 调用 `addRoot`)分别归属 `RootActions` 与 `Sync`——但 `request-add-folder` 监听目前挂在
   `onMounted` 里且与 `folder-stats-changed` 相邻(域 9)。拆分后建议:`Sync` composable 只保留
   `folder-stats-changed` 监听(它天然属于「重载生命周期」);`request-add-folder` 监听随 `addRoot` 一起
   下沉到 `RootActions` composable 自己的 `onMounted`/`onBeforeUnmount`,两个 composable 各自管好自己
   注册的事件,不共用一个 mount 钩子。

以上四点都是「结构性必需的重新包装」,不改变任何可观察行为,但是纯移动之外唯一需要**改写**(而非剪切
粘贴)的地方,施工阶段需要显式测这几处。

### 2.4 template / style 是否拆分

- **template**:不建议拆出子组件。三种行(dir/file/more)共享 `startIndex + i` 算出的 `activeIndex`
  比较、`row.node.depth`/`row.depth` 缩进计算、以及大量跨行为的 class 绑定(`kb-active`/`drag-over`/
  `is-hidden` 等),这些绑定来源分散在上面 7 个 composable 里。若拆成 `<TreeRow>` 子组件,单行需要
  透传 6-8 个 prop + 5-6 个 emit,虚拟化窗口每帧还要为每个可见行创建/销毁子组件实例,是从当前「函数式
  渲染」退化为「组件树」,对这个高频重渲染的虚拟列表是负收益。保持内联 `<template v-for>` 更贴合虚拟化
  场景的性能约束。
- **style**:340 行本身不构成体量问题,且按 class 前缀天然分区、无跨区依赖,拆出去意义不大,反而增加
  `<style src>` 引入的 scoped-hash 协调风险(项目里未见此模式的先例)。不建议动。

### 2.5 拆分不改变的契约

- 组件对外 props(`order: number`)、对外可观察行为(渲染结果、路由跳转、IPC 调用时序)不变。
- `window` 事件契约(`folder-stats-changed` / `request-add-folder`,MediaGrid.vue 与 ManagementSection.vue
  经此解耦通信,已用 grep 核实)不变——两个事件仍以 `window.addEventListener`/`dispatchEvent` 形式存在,
  只是监听注册点分散到对应 composable。

---

## 3. 风险与不变量

1. **响应式边界跨 composable 传递**:`displayRows`/`visibleRows` 等必须以 `ComputedRef` 形式传参,不能
   在传递前 `.value` 解包成普通值——否则下游 composable 拿到的是拆分时刻的快照而非活的响应式引用。
   `scrollAreaEl` 从裸变量改 `shallowRef` 后,所有原来直接读 `scrollAreaEl`(非 `.value`)的代码点都要
   同步改成 `scrollAreaEl.value`(纯规模,但极易漏改,建议全局搜索校验)。
2. **WAI-ARIA tree 键盘契约**:容器持焦 + `aria-activedescendant` 伪焦点(2026-07-10 a11y 决策)——
   `activeIndex`/`activeDescId`/`rowDomId` 三者必须保持在同一个 composable 内闭环,`rowDomId` 生成的
   id 格式(`tree-row-d/f/m` 前缀 + nodeKey)不可变,模板里的 `:id`/`:aria-activedescendant` 绑定要对
   得上。
3. **D-013 双身份索引边界(§4.1)**:`nodesById`(实体轴,只索引有 DB id 的节点,服务拖拽/移动/复制)与
   `nodesByKey`(结构轴,索引全部节点,服务分页/滚动等结构操作)绝不可合并成一张表——FS-only 目录没有
   实体身份是「查不到即代表没有该能力」的显式设计,不是需要补的漏洞。拆分时两个索引各自的消费方名单
   (见 §2.1 表)要保持不变,不能因为「反正都在一个 composable 里」就互相替用。
4. **拖拽单飞与自动滚动生命周期**:`autoScrollRaf`/`autoScrollDir`/`autoScrollRecompute`/`lastDragPoint`
   四个状态必须整体搬进同一个 `DragDrop` composable(不可再切分目录拖拽与文件拖拽到两个文件),否则
   两路拖拽会各自起一个 rAF 循环,失去「贴边滚动全局单飞」的不变量。`stopAutoScroll()` 现在挂在组件级
   `onBeforeUnmount`(第一处,域 3 旁),拆分后必须确保 `DragDrop` composable 自己注册
   `onBeforeUnmount(stopAutoScroll)`,不能遗漏——否则卸载时残留悬空 rAF(T1-c 明确写过的坑)。
5. **快滚静止门 + 滚动补偿的时序耦合**:`maybeAutoLoadMore` 依赖「用户滚动时间戳」与「虚拟化窗口的
   `firstVisibleIndex`」两个值必须来自同一次 `updateWindow` 计算,`loadMore` 成功后的 scrollTop 补偿
   (`insertIdx <= firstVisibleIndex`)判据同理。拆分后这两个量都经 `getLastScrollTs()`/
   `firstVisibleIndex`(virtualization 导出)传入,不能在 `AutoLoadMore` composable 内自建一份影子状态。
6. **粘性头栈插位依赖侧栏区块顺序**:`stackTopPx`/`stackBottomPx` 依赖 `useSidebarSections().visibleIds`
   里 `'folders'` 的 index,这是跨组件的隐式契约(其他 accordion 区块的展开/折叠会改变这两个值)。
   迁入 `useFolderTreeVirtualization` 时要保留对 `useSidebarSections` 的直接依赖,不要改成由外部传入
   固定 index(那会让「折叠上方区块」这个动态场景失效)。
7. **`suppressClick`/`consumeSuppressClick` 语义对等**:改成方法调用后必须保证「读且复位」是原子的一步
   (现状 `if (suppressClick) { suppressClick = false; return }` 两行),不能拆成先读后写两次调用,否则
   并发点击(理论上不会,但作为契约仍需保证)可能读到脏值。
8. **右键菜单 / 迁移 / 新建对话框的三方状态共享**:`onNodeContextMenu` 写 `createDialog`(新建子文件夹)
   与 `contextMenu`(菜单本身),而 `relinkRoot` 只读 `node`、不碰这两个 ref;模板还直接绑定
   `createDialog.isOpen`/`contextMenu.visible` 做 Teleport 渲染开关。三者若被分别放进不同 composable,
   模板绑定的字段来源要在一次改动里对齐,避免「改了归属文件、忘了改模板里取值路径」。

---

## 4. 收益与优先级

### 4.1 预估拆分后大小

| 文件 | 预估行数 |
|------|----------|
| `FoldersSection.vue`(本体,script 大幅收缩,template/style 不变) | ~850–950(script 部分从 ~1204 行降到 ~230–280 行) |
| `useFolderTreeVirtualization.ts` | ~230 |
| `useFolderTreeDragDrop.ts` | ~300 |
| `useFolderTreeSync.ts` | ~200 |
| `useFolderRootActions.ts` | ~260 |
| `useFolderTreeKeyboardNav.ts` | ~90 |
| `useFolderTreeAutoLoadMore.ts` | ~90 |
| `useFolderTreeIndices.ts` | ~45 |
| `folderTree.helpers.ts`(+fileIcon/fileTitle) | 420 → ~460 |

主文件从 1839 行降到约 850–950 行(约 48%-54% 削减),且剩余体量主要是 template(291,虚拟化行渲染,
理由见 §2.4)与 style(340,天然分区),script 部分不再是体量瓶颈。

### 4.2 施工顺序建议(风险从低到高排列,每步独立可验证再进下一步)

0. **热身**:`fileIcon`/`fileTitle` 下沉到 `folderTree.helpers.ts`(纯函数,零状态耦合,风险最低)。
1. `useFolderTreeIndices.ts`——纯索引 + 一个递归查找函数,无 DOM/生命周期,第二低风险。
2. `useFolderTreeVirtualization.ts`——体量最大,但已有 §2.3-①③ 明确了必需的 ref 化改造;后续四个
   composable 都要读它的输出,应尽早稳定接口。
3. `useFolderTreeKeyboardNav.ts`——纯消费 virtualization 输出 + 几个回调,验证键盘导航（方向键/Home/End/
   Enter）不回归即过关。
4. `useFolderTreeAutoLoadMore.ts`——消费 virtualization + indices,验证「加载更多」自动接力与快滚静止门。
5. `useFolderTreeSync.ts`——加载/重载/模式切换独立性较高,可与步骤 3/4 并行施工(无共同状态)。
6. `useFolderTreeDragDrop.ts`——两路拖拽 + 边缘自动滚动整体搬迁,交互面最广、回归成本最高,放在
   virtualization/indices 都已验证稳定之后。
7. `useFolderRootActions.ts`——依赖步骤 5 的 `reloadTreePreserveExpansion`,放最后。

---

## 5. 验证策略

### 5.1 静态检查
- `npm run typecheck`(vue-tsc --noEmit):composable 间传递的 `ComputedRef`/`ShallowRef` 类型、
  `DirNode`/`DirFile`/`TreeRow` 等类型的跨文件 import 路径。strict 模式 + 禁 `any` 是硬约束,尤其是
  §2.3 提到的几处 ref 化改造容易引入隐式 `any`。
- `npm run lint`(eslint,风格问题用 `npm run lint:fix`,不跑仓库级 `npm run format`)。

### 5.2 单测(vitest)
- 现有 `folderTree.helpers.spec.ts`、`useFolderTree.spec.ts` 覆盖纯逻辑,拆分后应保持全绿(拆分不改
  这些函数的签名/实现,只改调用方位置)。
- 建议在动手拆分前,针对本文件里**尚无任何自动化覆盖、又是高风险数学**的几处补characterization test
  (项目规则:改动未测过的关键行为前先补测),优先级:
  - `computeTreeWindow`/`treeScrollTopForIndex`(已在 helpers 里,若尚未覆盖粘性插位 `stackTopPx`/
    `stackBottomPx` 参与的场景应补上)。
  - `useFolderTreeAutoLoadMore` 的滚动补偿判据(`insertIdx <= firstVisibleIndex`)与静止门时序,可以
    对 composable 单独 mount 一个最小 harness 组件后用 vitest + fake timers 验证。
  - `useFolderTreeDragDrop` 的 `canDropOnId`(环检测)已是纯函数、可直接单测;`recomputeFileDrop`
    依赖 `document.elementFromPoint`,建议用 jsdom mock 或降级为手测项。
- 当前仓库**没有** `FoldersSection.vue` 的组件级测试(已用 grep 核实,15 个匹配文件中无 `.spec.ts` 直接
  测组件本体),拆分前后都属于「手测兜底」的既有缺口,不因本次拆分而新增责任,但如果顺手为纯逻辑较重的
  `AutoLoadMore`/`Indices` 补最小单测,属于低成本高收益的順手项。

### 5.3 GUI 手测清单(不可自动化的交互面)
1. 展开/折叠子目录(点击箭头/双击行/键盘 → 和 Enter)、方向键上下移动、Home/End 跳转首尾。
2. 大量文件的目录里快速拖动滚动条/触摸板惯性滚动:虚拟化窗口不闪烁、不出现空洞、粘性目录头链随滚动
   正确更新且点击可跳转。
3. 目录拖拽移动到另一目录 / 按住 Ctrl 复制;拖到自身子孙目录应被拒绝(环检测);拖拽时把目标滚出视口
   外再滚回来,边缘自动滚动应把目标滚回可见并保持可落。
4. 文件行拖拽移动/复制到目录(同上,但无环检测,只拒绝落回当前目录)。
5. 「加载更多」:滚动到底部自动追加下一页;快速飞掠多个大目录时应有静止门(不应对每个扫过的目录都发
   IPC);人为制造分页失败后手点重试应能解除拉黑。
6. 显示范围三态菜单(仅入库/全部文件/全部文件+隐藏)切换,树按新范围整树重载(不保展开态,是设计使然)。
7. 新建文件夹(全局 + 目录内子文件夹)、导入文件夹(触发子目录重叠「合并替换」分支、触发父目录重叠
   「仍要添加」分支)。
8. 文件夹迁移重链接:核对不足(`relink_mismatch`)、路径已被占用(`relink_path_taken`)、目标不是目录
   (`relink_not_a_dir`)三种错误话术分别触发一次;成功后应有兜底增量重扫。
9. 树内文本文件双击预览(桌面 + 模拟移动端 UA);不可预览的未入库文件双击应 reveal(桌面)或提示不支持
   (移动端)。
10. 画廊空态点击「添加文件夹」引导按钮,应联动触发本组件的 `addRoot` 完整流程(重叠检测/自动扫描/树
    选中),验证 `request-add-folder` 事件跨组件契约在拆分后仍然打通。

---

## 附注:CSS 外置(D-451,2026-07-25 补录)
- 本组件 `<style scoped>` 可整块外置为同目录 `FoldersSection.styles.css`,SFC 留 `<style scoped src="./FoldersSection.styles.css"></style>`;外置文件仍编译为宿主组件 style 块,scope id 归属不变,`:deep()` 穿透语义不变。
- 前提(2026-07-25 核实):全仓 .vue 样式零 `v-bind()`;本文件为单一 `<style scoped>` 块。
- 定位:可选先行批、全场风险最低的行数削减刀;不替代 script 拆分主刀。
- 施工顺序:首刀拿最小文件实测 Vite 构建链 + HMR,通过后铺开。
- 原案裁 style 不拆进子组件维持不变;本附注是另一维度(外置文件)。该组件 style 体量小、结构简单,适合当首刀实测件。

## 顺手发现
无
