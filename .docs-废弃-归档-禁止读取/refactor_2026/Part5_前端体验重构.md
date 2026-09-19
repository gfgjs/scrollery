---
id: 2026-06-26-Part5_前端体验重构
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 5 · 前端体验重构

> 依赖：[Part0](Part0_总纲与产品定稿.md)（§4 功能矩阵、§11 波次、§12 约定）、[Part2](Part2_扫描与画廊流水线.md)（消费其布局数据 + 时间直方图 + 离线态字段）、[Part3](Part3_缩略图派生与GPU引擎.md)（`asset://` 缩略图 + thumbhash 占位 + 缓存管理 IPC + 阅读器 document_meta）、[Part4](Part4_AI与人脸插件化.md)（语义搜索/人脸审批命令 + 插件 gate）、[Part6](Part6_插件平台与exotic收尾.md)（插件平台 IPC + EntitlementProvider 前端 gate）。
> 状态：定稿待执行（已并入 terminal review；正文即权威）。执行前必读 Part0 §13 + 各依赖 Part §7 + 本文。**旧 docs/记忆不可轻信，以代码实测为准。**
>
> 🔴 **「契约冻结」立论已被取代（2026-06-30，用户约束「开发期不冻结契约」）**：本文下方所有「交互契约冻结 / 一旦发布即冻结 / 信任红线 / 契约冻结红线」措辞,**一律读作「当前临时实现,开发测试阶段可随实测/市场验证演进」**,绝非「发布即不可改」。T7 交付物相应从「冻结契约」改为「文档化当前协议(标注临时可改,作团队对齐 + 回归基线)」。详见 `2026-06-30-Part5-选区契约与可插拔多模式设计.md` 与记忆 `no-contract-freeze-during-dev`。Google Photos 的教训是「**无序乱改**」,不等于「禁止迭代」。

---

## §1 目标与范围

### 1.1 本 Part 解决什么

前端是「百万级流畅 + 不破坏用户信任的交互契约」的最终呈现层。栈 Vue3（Composition + `<script setup>`）+ Pinia + TS strict。本 Part 交付：

1. **三层虚拟化画廊**（Section 年月 / 行 / 行内 justified，Part0 §2 卖点）：消费 Part2 后端布局缓存 + 坐标平移（bug 修复在 Part2，前端配合渲染层钉位）。
2. 🔴 **稳定多选**（框选 / Shift / Ctrl）+ 批量操作——交互契约**开发期保持临时可改、用实测/反馈有序演进**（取代旧「发布即冻结」立论;Part0 §4 Google Photos 的教训是「无序乱改」,不是「禁止迭代」）。
3. **星级 + 颜色标签**（消费级留存功能）。
4. **虚拟相册强化**（智能相册 / 集合）。
5. **时间轴滚动条**（右侧年月拖拽 + 密度热力图）：消费 Part2 §3.8 `get_date_histogram` / `month_buckets`。
6. **首启向导**（3 步 onboarding）。
7. **亮/暗主题**（`:data-theme` 绑定修正，Part0 §11 速赢）。
8. 🔴 **IPC 错误统一**（Part0 §1 矛盾 #10：~72 命令返 `String` 非 `AppError`，前端无法区分 Cancelled/NotFound/Db）：后端存量命令分批 AppError 化（ai/face/doc 起）+ 前端统一错误展示/toast。
9. **组件拆分**（MediaGrid 等巨组件解耦）。
10. **插件商店 UI**（安装 / 激活 / 购买引导 / 下载进度）+ 插件 **gate**（未授权能力的引导）：消费 Part4/Part6 IPC。
11. **语义搜索 UI + 人脸批量审批 UI**（grouped likely matches，消费 Part4 命令）。
12. **离线态 UX**（卷离线灰显 + `CloudOff` 角标 + `VOLUME_OFFLINE` 弹窗）：消费 Part2 卷可用性字段。
13. **速赢**（Part0 §11 line 509/515）：`showToast→ui.addToast`（8+ 处批量操作 toast）、`:data-theme` 绑定、幽灵 IPC 常量清理、`ScanChannelPayload` 类型修正。

### 1.2 不在本 Part（归属其它 Part）

- 后端布局/扫描/缩略图/AI/插件平台**逻辑** → Part2/3/4/6（本 Part 只消费 IPC + 渲染）。
- IPC 命令的**后端业务实现** → 各 Part（新命令已约定返 `AppError`）；本 Part 负责**存量命令 AppError 化的契约统一 + 前端错误处理**（前后端协同，主体触及前端契约 → 归 Part5）。
- **阅读器内核**（pdf.js/epub 渲染引擎、`usePager`）已实现（Part0 §4）→ 本 Part 只整合/修缺，不重写内核。
- **插件平台后端**（Registry/install/激活命令）→ Part6（本 Part 消费 IPC 做商店/gate UI）。
- 坐标平移**滚动 bug 根因修复** → Part2 §3.6（本 Part 配合渲染层，不重复）。

### 1.3 全局约定（继承 Part0 §12）

🔴 **交互契约开发期不冻结**：多选/拖拽手势协议临时可改、用实测/反馈有序演进（取代旧「发布即冻结」立论,见 `no-contract-freeze-during-dev`）；缩略图经 `asset://` 直通（绕 WebView2 IPC 字节）；画廊筛选来自 `uiStore`（`activeSmartAlbum/activeDirectoryId/activeCollection`）**非 vue-router**（memory `gallery-view-not-router-driven`）；侧栏 accordion 的 **fragment-sticky 不变量**（绝不用 div 包裹 section，memory `sidebar-accordion-architecture`）；Tauri 原生拖放阻断 HTML5 DnD → 用 pointer 事件（memory `gotcha-tauri-dragdrop`）；Vue3 Composition + TS strict + Pinia；中英双语注释；改后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（代码取证，文件:行）

> workflow `we4hi9jlk`（4 agent 测绘画廊交互/状态IPC层/框架主题向导/插件AI人脸离线UI）回传，逐项 file:line。**与旧 plan-docs 冲突处以本节为准。**

### 2.1 画廊交互（多选/拖放/星级/布局/虚拟滚动/toast）

**多选**（[useSelection.ts](../../src/composables/useSelection.ts) 模块级单例 ref，不经 Pinia，MediaGrid + SelectionToolbar 共享）：

- Ctrl/Cmd+Click → `toggleSelect`；Shift+Click → `selectRange(last, id, getAllVisibleItemIds())`。
- 框选（lasso）：`onPointerDown` → `pointermove` `elementFromPoint`+`querySelectorAll('[data-item-id]')`，`applyDragRange` 每帧重建 newSet。
- Ctrl+A → `selectAll(getAllVisibleIds())`。
- 🔴 **三症状一病因：选区只覆盖虚拟滚动可视窗口的 DOM**（useSelection.ts:55-77/106-222、MediaGrid.vue:870-915）：① Shift+Click 跨视口 → `orderedIds` 仅可视项、`indexOf=-1` → 整段 range 静默无效；② Ctrl+A 10 万库只选 ~30-50 行（一屏）；③ 框选只选已渲染节点。
- 🔴 **竞态**：`hasDragMoved` 在 `onPointerUpGlobal` 未显式复位 false（useSelection.ts:224-236）→ `wasDrag()` 余晖误判下次快速单击为拖拽（单击打不开详情）；两套 drag-flag 并行（`mediaWasDrag`@MediaGrid + `hasDragMoved`@useSelection）逻辑割裂；多面板共享 selectedIds 单例 → 跨面板选区混合风险。
- 框选性能：`applyDragRange` 每 `pointermove` 全量 `querySelectorAll` DOM（百格以上每帧全扫，性能隐患）。

**拖放**（[usePointerDrag.ts](../../src/composables/usePointerDrag.ts):1-16，**全用 Pointer Events**，因 Tauri v2 `dragDropEnabled` 拦截 HTML5 DnD，memory `gotcha-tauri-dragdrop` 证实）：拖图到文件夹树（MediaGrid.vue:247-832，ghost 命令式 transform 不触发重渲染；左键移/Shift 复制/右键落点菜单）。⚠️ `onMoveCopyConfirm`(MediaGrid.vue:987) 直接 `invoke('move/copy_media_items')` **绕过 historyStore 撤销记录**，与 `performMediaDrop` 路径不一致。

**星级/颜色标签**：
- 星级 1-5 存 DB（`set_rating`），前端 `mediaStore.setRating`（mediaStore.rs:298）。🔴 **UI 入口仅 MediaDetailOverlay 详情侧栏**（MediaGrid/MediaThumb 无快捷评分）；`filterStore.minRating` 接通后端布局但**无 FilterPanel 暴露给用户**（功能盲区）。
- 🔴 **颜色标签完全不存在**（DB/IPC/store/UI 全无）。

**布局**：🔴 **无 Grid/Justified 模式切换**（始终后端 justified）；密度滑块 `gridRowHeight`（AppToolbar.vue:153，min60/max960，pre/post-flush watcher 顺序正确抓锚点）已有。

**虚拟滚动**（[useVirtualScroll.ts](../../src/composables/useVirtualScroll.ts)，与 Part2 同测）：`SAFE_MAX=10M`，平移模式 `renderAnchor` 钉位、命令式 transform 不触发重渲染；注释「平移模式已知滚动错位 bug 已搁置」（Part2 §3.6 修复）。

🔴 **toast 静默失效**（gallery + framework 双证）：MediaGrid 7 处 + MediaDetailOverlay 3 处用 `typeof (ui as any).showToast === 'function'`，而 uiStore **只有 `addToast`、无 `showToast`** → 条件**永假** → 批量收藏/取消/删除/移动复制/壁纸的反馈 toast **全部静默吞掉**（MediaGrid.vue:405/936/956/969/998/1032、MediaDetailOverlay.vue:330/632/668、uiStore.ts:198）。`as any` 同时绕过了类型检查。

**MediaGrid 巨组件**：1581 行（template 194 + script 1040 + style 347），承担虚拟滚动/行高锚定/缩略图队列/批量操作/拖图/右键菜单/Tauri 事件/时间轴边栏/键盘监听 7+ 职责。可拆：`useMediaFolderDrop`/`useGridContextMenu`/`useBatchOps`/`TimelineSidebar`/`useDimPriority`。

### 2.2 状态管理 + IPC 层

**10 个 Pinia store**（除 configStore 外均 Composition API）：

| store | 职责 | 问题 |
|-------|------|------|
| uiStore | 主题/语言/视图切换/排序/网格/toast/搜索/滚动 | 顶层直接 invoke(GET_APP_CONFIG ×7)非 action 封装 |
| mediaStore | layout/行缓存/可视区 meta 懒加载/详情/收藏/评分 | 内置 30s watchdog 防 compute_layout 卡死；`navigateDetail` 裸字符串 `invoke('get_adjacent_media')` |
| filterStore | 前端过滤(mediaTypes/livePhoto/favorited/minRating/dateRange) | 纯前端、`toApiFilter()`、**不持久化** |
| scanStore | 扫描根/进度/全量缩略图/enrichment 监听 | 🔴 ScanChannelPayload 类型错位（见下）；startScan error 分支**无 toast** |
| configStore | 应用配置持久化(14 项) | 🔴 **Options API（异类）**+裸字符串 `'get_app_config'`；与 uiStore 双持 thumbStrategy/gpuEngine |
| collectionStore | 收藏夹 CRUD | 用 IPC 常量、catch+console.error 返默认 |
| historyStore | 移动/复制 undo/redo（内存） | `refresh()` 用 `window.dispatchEvent` 绕 Pinia 响应式 |
| aiStore | AI 引擎/语义搜索/分析进度/模型库 | 🔴 ~15 裸字符串 invoke；直接写 `uiStore.searchQuery`(跨 store 副作用) |
| faceStore | 人脸分析控制（**镜像 aiStore**） | 🔴 裸字符串 invoke；与 aiStore **start/pause/stop/restart/2s 轮询完全重复**（无公共 composable） |
| personStore | 人物墙(list/rename/hide/merge/recluster) | 裸字符串 invoke；**缺 confirm/reassign/likely_matches** |

🔴 **IPC 层三大问题**：

1. **无统一 wrapper**：所有 store 直接 `invoke()`；~25 处裸字符串字面量（aiStore/faceStore/personStore/configStore/mediaStore）**绕过 `constants/ipc.ts`** → 重命名命令无静态保障。`useRequestQueue` 是缩略图专用队列、非通用 wrapper。
2. **无结构化 AppError 区分**（Part0 矛盾 #10）：后端 Err 序列化为 String，前端**无任何代码区分 Cancelled/NotFound/Db/Permission**；错误处理三类混乱：(a) console.error 返默认；(b) `addToast(String(e))`；(c) 静默 `catch(()=>{})`。
3. **幽灵常量**：`CLEAR_ALL_DATA`（constants/ipc.ts:127，**后端无此命令**、前端也不用）；`MOVE/COPY_MEDIA_ITEMS`、`LIST_AI_MODELS/IMPORT_AI_MODEL`（常量+后端在，但前端已改用别名、常量遗留）。

🔴 **ScanChannelPayload 类型错位**（scanStore.ts:103-112 vs types/ipc.ts:29-33）：TS 类型声明为**邻接判别联合** `{type:'progress'; progress: {...}}`，但**后端实为内部标签** `#[serde(tag="type", rename_all="camelCase")]`（[fast_scan.rs:86](../../src-tauri/src/scanner/fast_scan.rs#L86)，newtype variant 扁平化）→ 实际线上形状是**扁平** `{type:'progress', scanned, total, currentDir, status}`。🔴 **第 7 轮终审核实后端 serde 标签后定论**：scanStore `msg as unknown as ScanProgressPayload` 读顶层 `p.scanned` **运行时正确**，**错的是 TS 类型**。**唯一正确修法 = 改 `types/ipc.ts` 为扁平内部标签联合**（`({type:'progress'} & ScanProgressPayload) | ({type:'completed'} & ScanCompletedPayload) | ({type:'error'} & ScanErrorPayload)`），并把 `as unknown as` 改 `msg.type` 收窄；**切勿**改 scanStore 去「读 `.progress`」——那是按错误的嵌套类型改、反而破坏正常运行时。

**uiStore 画廊筛选**（uiStore.ts:104-149，memory `gallery-view-not-router-driven` 证实）：四互斥视图态 `activeSmartAlbum/activeDirectoryId/activeCollection/activePersonId`，切任一清其余三 + `clearSelection()`，**纯 uiStore 驱动非 router**。`addToast(type, message, duration=3000, actions?)`（uiStore.ts:198）。

### 2.3 应用框架 / 主题 / 向导 / 组件结构

**骨架**（[AppShell.vue](../../src/components/layout/AppShell.vue):1-53）：顶层 flex 三列——sidebar(可拖宽 180-400px) + main(toolbar header + content + statusbar footer)，具名 slot 注入。App.vue 在 AppShell 外全局挂 MediaDetailOverlay / SettingsView(v-if) / DocThumbRenderer / ToastContainer / CloseConfirmDialog。

**路由**（[router/index.ts](../../src/router/index.ts)，`createWebHashHistory`，8 条）：`/` `/folder/:id` `/favorites` `/trash` **全映射同一 MediaGrid**（内容由 uiStore 决定，非路由）；`/collections` `/persons` `/doc/:id` `/audio/:id` 独立视图。

🔴 **主题 bug**（framework 证实 Part0 速赢）：两处绑定——main.ts:19 mount 前 `documentElement.setAttribute('data-theme')` 防 FOUC（写 **resolved** 值）；AppShell.vue:2 `:data-theme="ui.theme"` 写 **raw** 值。`ui.theme='system'` 时 AppShell 绑 `data-theme="system"`，**CSS 无匹配**（CSS 只认 dark/light）→ `.app-shell` 子元素无主题色。修：改 `:data-theme="ui.isDark ? 'dark' : 'light'"` 或移除（纯 documentElement 方案，uiStore.applyTheme 已写 resolved 到 documentElement）。

🔴 **首启向导不存在**（App.vue:92-171，grep onboarding/wizard/welcome 零命中）：冷启动直接进空画廊，无「加扫描目录/选主题/选语言」3 步引导。

**侧栏 accordion**（AccordionSection.vue:1-18，memory `sidebar-accordion-architecture` 证实）：**双根片段**（acc-header + acc-body 无包裹 div，注释「TWO-ROOT FRAGMENT — load-bearing, do not tidy into a single div」），sticky 跨区块堆叠正确。**fragment-sticky 不变量正确遵守**。

**设置页**（[SettingsView.vue](../../src/views/SettingsView.vue):835 行）：全屏 overlay（v-if `isSettingsOpen`），6 组 CollapsibleCard（外观/缩略图/视频/AI 模型 + ModelLibrary + FaceModelLibrary + NetworkStorageSection + debug），`DynamicSettingControl` 统一渲染。

🔴 **巨组件清单**（>500 行）：MediaGrid 1580 / MediaDetailOverlay 923 / SettingsView 835 / MediaThumb 761 / AppToolbar 693 / FoldersSection 642 / ToolsSection 512。

### 2.4 插件 / AI / 人脸 / 离线 / 文档 UI

**语义搜索 UI**（[SemanticSearchPanel.vue](../../src/components/media/SemanticSearchPanel.vue) **完整**）：provider 徽章 + 进度条 + 开始/停止/重建 + 相似度阈值滑块(0.1-0.5)；三态 `searchMode`(semantic/normal/mixed，aiStore.ts:31-272，mixed 自动检测查询类型切排序)；`runSemanticSearch → invoke('semantic_search_cmd')` 写 ai_search_results → invalidateLayout。⚠️ App.vue:16 `v-show` 常驻 DOM。

🔴 **人脸审批 UI「双缺」**（plugin 证实 Part4 F3）：
- [PersonsView.vue](../../src/views/PersonsView.vue) 有 rename/merge/hide/recluster（personStore：list/rename/setHidden/merge/getFacesForItem/recluster）。
- **缺**：`grouped likely matches` 批量审批工作流、`confirm_face_assignment`、`reassign_face`、`list_likely_matches` —— **前端 UI 无 + store action 无 + 后端 command 无（face_commands.rs 三缺）**。与 Part4 §3.5.1 命令补全**成对推进**。

🔴 **插件商店 / gate 现状**：
- AI/人脸模型库**有**：[ModelLibrary.vue](../../src/components/settings/ModelLibrary.vue)（架构→batch 分组、下载源选择、Channel 流式进度、安装/切换）；[FaceModelLibrary.vue](../../src/components/settings/FaceModelLibrary.vue)（两轨展示、**注释「切换暂未开放」**、SCRFD 仅手动导入）。**均无付费 gate/授权/购买引导。**
- 🔴 **exotic 插件商店前端完全为零**（SettingsView 仅 3 子组件，src/ 无任何 exotic/plugin_store/激活/购买 Vue/store）——**后端 P6.1-P6.4e 全完成、前端零入口**（Part6 前端 gate 落本 Part）。

🔴 **离线态 UX 缺失**（plugin 证实 Part2 卷）：[NetworkStorageSection.vue](../../src/components/settings/NetworkStorageSection.vue) 仅 WebDAV/SMB/本地后端 CRUD（密码存 keyring）；**无 `VOLUME_OFFLINE` 弹窗、无 `CloudOff` 角标、无卷灰显**——卷在线/离线变化前端零响应。

**文档阅读器（已实现，仅整合）**：[EpubReader.vue](../../src/components/doc/EpubReader.vue)（epub.js 分页 + CFI 恢复 + replacer hook）；[PdfReader.vue](../../src/components/doc/PdfReader.vue)（pdf.js + IntersectionObserver 懒渲染 + DPR canvas）。

🔴 **AI 关键词/自动标签 UI 不存在**（MediaDetailOverlay info 面板仅 EXIF，无 AI 标签字段）。

### 2.5 现状问题清单（编号 → §3 设计对应）

| # | 问题 | 证据 | 对应设计 |
|---|------|------|---------|
| G1 | 多选三症状一病因（选区只覆盖可视 DOM）：Shift 跨视口失效/Ctrl+A 只选一屏/框选只选渲染节点 | useSelection.ts:55-222 / MediaGrid.vue:870-915 | §3.1 |
| G2 | 多选竞态：hasDragMoved 余晖误判单击、两套 drag-flag、跨面板选区混合 | useSelection.ts:224-236 | §3.1 |
| G3 | toast 静默失效（10 处 `(ui as any).showToast` 永假） | MediaGrid/MediaDetailOverlay / uiStore.ts:198 | §3.9 速赢 |
| G4 | onMoveCopyConfirm 绕过 historyStore 撤销 | MediaGrid.vue:987 | §3.1 |
| G5 | 星级无快捷入口 + minRating 无 FilterPanel | MediaDetailOverlay / filterStore | §3.2 |
| G6 | 颜色标签完全不存在 | DB/IPC/store/UI 全无 | §3.2（需 Part1 加列） |
| G7 | 无 Grid/Justified 布局切换 | useJustifiedLayout | §3.11 |
| S1 | 无统一 IPC wrapper，~25 裸字符串 invoke 绕过常量 | aiStore/faceStore/personStore/configStore | §3.4 |
| S2 | 无结构化 AppError 区分（前端无法分 Cancelled/NotFound/Db） | 全 store | §3.4 |
| S3 | 幽灵常量 CLEAR_ALL_DATA（后端无命令）+ 遗留常量 | constants/ipc.ts:127 | §3.4 速赢 |
| S4 | ScanChannelPayload 类型错位（🔴 第7轮终审核实：后端 `#[serde(tag="type")]` 实为扁平、scanStore 运行时正确 → **改 TS 类型为扁平内部标签**，**非**改 scanStore 读 .progress） | scanStore.ts:103 / ipc.ts:29 / fast_scan.rs:86 | §3.4 速赢 |
| S5 | configStore Options API 异类 + 与 uiStore 双持 thumbStrategy/gpuEngine | configStore / uiStore:77 | §3.10 |
| S6 | aiStore/faceStore 分析控制完全重复（无公共 composable） | aiStore vs faceStore | §3.10 |
| S7 | scanStore.startScan error 无 toast | scanStore.ts:128 | §3.4 |
| F1 | 主题 AppShell :data-theme 绑 raw 值 system 无匹配 | AppShell.vue:2 | §3.9 速赢 |
| F2 | 首启向导不存在 | App.vue:92 | §3.8 |
| F3 | 巨组件 7 个 >500 行 | MediaGrid 等 | §3.10 |
| P1 | 人脸审批 UI+store+后端 三缺 | PersonsView / personStore / face_commands.rs | §3.6（配 Part4 §3.5.1） |
| P2 | exotic 插件商店前端零（后端全 done） | src/ 无 exotic UI | §3.5 |
| P3 | 离线态 UX 缺失（弹窗/角标/灰显） | NetworkStorageSection | §3.7 |
| P4 | AI 关键词/自动标签 UI 不存在 | MediaDetailOverlay | §3.6（后置/可选） |
| P5 | 模型库无付费 gate/购买引导 | ModelLibrary/FaceModelLibrary | §3.5 |

---

## §3 设计方案

> 原则：**速赢先行（toast/主题/幽灵常量/类型，零风险高回报）→ 稳定多选（契约文档化,交互稳定可用）→ 缺失功能补全（人脸审批/插件商店/离线 UX，多数配后端 Part4/Part6 成对）→ 重构债（组件拆分/store 去重）**。新组件 Vue3 Composition + TS strict，IPC 经统一 wrapper。

### 3.1 稳定多选 + 选区解耦（G1 + G2 + G4）

> 🔴 **核心：选区与虚拟化解耦**——G1 三症状同一病因（选区只覆盖可视 DOM）。选区必须基于**布局序 id**，非 DOM 节点。

**3.1.1 选区脱离 DOM**（消费 Part2 布局缓存 `flat_ids`，§2.6）：

- **Shift+Click 范围**：基于 `flat_ids` 的 index（anchor→target）算 range，**不依赖可视 DOM**。前端持有当前视图 `flat_ids`，**由 Part2 T14.5 新增的 `get_view_ids() -> Vec<i64>` 命令提供**（🔴 现 flat_ids 仅在后端内部 `LayoutCacheData`、IPC 零暴露 → 该命令是本任务硬前置，前端不自行拼凑）；随 `layout_version` 失效后重取。
- **Ctrl+A 全选**：**全选语义标记**（`selectAllMode=true` + 排除集），**不把百万 id 灌进内存**；批量操作时后端按当前 filter 解析全集。展示「已选全部 N 项」。
  - 🔴 **P0-9 后端 SelectionDescriptor 契约（第 6 轮独立核验，硬前置）**：现批量命令全收 `Vec<i64>`（`media_commands.rs:222/291/302` 等）→ `selectAllMode` 无后端落点。须新增统一 `SelectionDescriptor`（`Explicit(Vec<i64>)` | `SelectAll { filter: GalleryFilter, excluded_ids: Vec<i64> }`），**所有批量命令**（删除/恢复/收藏/评分/颜色/移动/复制/导出/批量人脸）改接受 `SelectionDescriptor`；后端 `SelectAll` 分支按 `filter` 在 **SQL 层**解析全集（复用 Part2 画廊查询 − `excluded_ids`），不经前端传百万 id。**owner**：描述符类型 + filter→ids 解析归 **Part2**（与 `get_view_ids` 同源）；各批量命令签名改造归命令所在 Part（media/face），并随之加入 `lib.rs` `generate_handler!`（IPC-1）。否则 Ctrl+A 仅停在 UI 展示层。
- **框选（lasso）**：保持可视操作（命中渲染节点），但选区**存稳定 id Set**；性能优化——`querySelectorAll` 结果缓存 + 仅 hover 变化时增量（替每帧全扫）。

**3.1.2 竞态消除**（G2）：

- **单一 drag-flag**：移除 `mediaWasDrag`(MediaGrid) / `hasDragMoved`(useSelection) 双轨 → 统一一个，**明确复位时机**（`pointerup`/`pointercancel` 同步复位 + 下次 `pointerdown` 兜底重置）。
- **多面板选区隔离**：selectedIds 按面板 scope（主画廊 / 语义结果各一），切面板清选区（已有 `clearSelection`，补 scope 维度）。

**3.1.3 撤销一致性**（G4）：`onMoveCopyConfirm` 改走 `historyStore.moveMedia/copyMedia`（与 `performMediaDrop` 同路径），纳入 undo/redo。

**3.1.4 🔴 交互契约文档化（非冻结）**：把当前多选/拖拽手势协议**写入文档作团队对齐 + 回归基线**,标注**开发期临时可改**——可随实测/市场验证演进（取代旧「发布即冻结」立论）。两层解耦（协议层 / 策略层,见选区设计稿）保证改动影响收敛;新增/调整模式不外溢到消费方与后端。

> **当前协议权威规格**见 `docs/refactor_2026/2026-06-30-Part5-选区契约与可插拔多模式设计.md` §5(手势→意图词汇表)。下表为 **T7 回写的「已实现基线」**(T4a–T6 交付后实测,commits 99eaf8e→17a52e1),作 T18 拆分前的回归对照。**全部临时可改**(no-contract-freeze-during-dev)。

| 物理手势 | 当前行为(已实现) | 落点 |
|---------|----------------|------|
| 单击项(普通模式) | 打开详情 / 文档·音频走专用路由 | handleCardClick |
| Ctrl/Cmd+单击 | toggle 翻转单项 | → `toggleSelect` → classic `toggle` |
| Shift+单击 | 从锚点(lastClickedId,缺失则回退**布局序首个已选项**)到点击项,布局序区间并入 | → `selectRange` → classic `range`(经 `useViewIds.rangeBetween`,跨视口稳定) |
| Ctrl/Cmd+A(选择模式中) | 全选语义,进 `all` 态、不物化百万 id;工具栏显「已选 N 项」 | → `selectAll` → classic `selectAll` |
| Esc | 清空退出选择 | → `clearSelection` → classic `clear` |
| checkbox 点击 | toggle 进入/退出选择,设锚点 | MediaThumb `@select` → `toggleSelect` |
| 框选拖拽(空白/未选项起手) | 在拖拽基线上叠加区间,区间经 `rangeBetween`(跨已滚动区间稳定);结束设锚点到终点项 | onPointerDown→applyDragRange |
| 左键拖已选项→文件夹 | 移动(可撤销) | → `historyStore.moveMedia` |
| Shift+左键拖 | 复制(可撤销) | → `historyStore.copyMedia` |
| 右键拖→文件夹 | 落点菜单(移动/复制) | showDropMenu |

> **实现 vs 计划的偏差(as-built,均属临时可演进)**:
> - **拖拽意图未独立建模**:设计稿 §5.1 曾列 `dragStart/dragOver/dragEnd` 意图;实现把框选折叠为「起手快照基线 + 逐帧 `range`」,纯 reducer 只需 6 个离散意图(replace/toggle/range/selectAll/clear/invert)——更简、更可测。
> - **单 drag-flag 复位时机**:统一 `hasDragMoved`(消 `mediaWasDrag` 双轨),复位在**下次 pointerdown**(`beginInteraction`)而非 pointerup——因尾随 click 须读到 flag 才能被抑制(详见 17a52e1/181b15a 推演)。
> - **§3.1.2 多面板 scope 隔离暂为空**:实测 `useSelection` 仅 MediaGrid + SelectionToolbar 消费,SemanticSearchPanel 未接,故当前无跨面板串扰;待语义面板接入再做。
> - **§3.1.1 框选性能**:已用 `rangeBetween`(布局序 O(range) 切片)取代每帧 `querySelectorAll` 全扫,设计稿的「querySelectorAll 缓存」方案随之 moot。
> - **回归基线**:策略层(classic + 访问器 + registry 可插拔)有 vitest 单测(32 项);useSelection/MediaGrid 含 DOM/IPC 副作用部分经**手动交互回归**通过,未覆盖自动化测试。

### 3.2 星级 + 颜色标签（G5 + G6）

- **3.2.1 星级快捷入口 + 筛选面板**：① MediaThumb hover 显 5 星快捷评分 + 键盘 `1-5`（选中项批量评分）；② 新 **FilterPanel** 组件暴露 `filterStore.minRating`（+ mediaTypes/livePhoto/favorited/dateRange，现全无 UI），接 AppToolbar 筛选芯片。
- **3.2.2 颜色标签**（G6）：列 `media_items.color_label`(INTEGER，0=无 / 1-7 色) **已入 Part1 V10 DDL**（Part1:163）。🔴 **IPC-2 owner 定稿（第 6 轮独立核验）**：`set_color_label(selection, color)` 命令**归本 Part**（落 `ipc/media_commands.rs`、随 T16 加入 `lib.rs` `generate_handler!`——**非 Part1**，Part1 只供列）；接受 §3.1.1 `SelectionDescriptor`（P0-9）。+ filterStore.colorLabel + UI（详情/右键设色 + FilterPanel 按色筛）。消费级留存功能（Lightroom/Bridge 标配）。

### 3.3 时间轴滚动条（消费 Part2 §3.8）

> 现 mini-timeline（MediaGrid 时间轴边栏）按逻辑高度均布圆点、无密度。升级为真时间 scrubber。

- 消费 Part2 §3.8 `get_date_histogram` / `LayoutSummary.month_buckets`（年/月密度 + 逻辑 y）。
- 右侧 scrubber：**按时间均布**（非高度比例）+ **密度热力条**（每月 tick 高=该月占比）+ **年份 label 浮层**（hover/固定）+ 拖拽跳转（`logicalToPhysical(y)` + smooth scroll）。
- 百万库**月/年聚合**（避免数千天节点密度失控，§2.8 Part2 缺陷）。
- 拆为独立 `TimelineScrubber` 子组件（从 MediaGrid 抽离，配合 §3.10）。

### 3.4 IPC 错误统一 + wrapper + 幽灵清理（S1 + S2 + S3 + S4 + S7）

🔴 **3.4.1 统一 IPC wrapper**（S1）：新 `src/utils/ipc.ts` `invokeIpc<T>(cmd: IpcCommand, args?)`：

```ts
// 统一封装：常量强制(类型层禁裸字符串) + 错误解析 + 可选 Channel
export async function invokeIpc<T>(cmd: IpcCommand, args?: Record<string, unknown>): Promise<T> {
  try { return await invoke<T>(cmd, args) }
  catch (e) { throw parseAppError(e) }  // String → 结构化 AppError
}
```

- 所有 store 的 ~25 处裸字符串 `invoke` 收敛到 `invokeIpc` + `constants/ipc.ts`（消除重命名无保障）。

🔴 **3.4.2 结构化 AppError**（S2，前后端协同，Part0 矛盾 #10）：

- **后端**：存量 ~72 命令分批（ai/face/doc 起）`String`→`AppError`；`AppError` 序列化为 `{ code: "Cancelled"|"NotFound"|"Db"|"Permission"|"VolumeOffline"|..., message }`（thiserror + serde）。
- **前端**：`parseAppError` 解析 code → 差异化处理：`Cancelled` 静默；`VolumeOffline` 弹「请插入设备」（§3.7）；`NotFound`/`Db`/`Permission` toast 错误。

**3.4.3 速赢清理**：
- **幽灵常量**（S3）：删 `CLEAR_ALL_DATA`（后端无）；遗留 `MOVE/COPY_MEDIA_ITEMS`、`LIST_AI_MODELS/IMPORT_AI_MODEL` 清理或对齐。
- **ScanChannelPayload 类型**（S4）：🔴 修正方向（第 8 轮核验）——Rust wire 实为**扁平 internally-tagged**（`#[serde(tag="type")]`，newtype variant 内层 struct 字段铺平到 `type` 旁，**非**嵌在 `progress` 键下）→ 正解是把 TS 类型 `ScanChannelPayload` 改为**扁平联合**（`{ type:'progress'; rootId; scanned; total; currentDir; status } | ...`）并删 scanStore 的 `as unknown as` 强制 cast；**勿**改成 `msg.progress.scanned`（该嵌套方向与 wire 相反，是文档原「二选一」的错误分支）。前后端定一致。
- **startScan error toast**（S7）：error 分支补 `ui.addToast('error', ...)`。

### 3.5 插件商店 + gate UI（P2 + P5，消费 Part6）

> 🔴 exotic 插件后端 P6.1-P6.4e 全完成、**前端零入口**；AI/人脸模型库有 UI 但无付费 gate。本节落「商店 + 授权 + 引导」前端。

**3.5.1 exotic 插件商店**（新 `PluginStoreView` + 组件）：消费 Part6 `fetch_exotic_registry` + install/uninstall/repair/activate 命令——

- 列出可用插件（Registry 快照：AI/人脸/exotic-formats），状态徽章（未安装/已安装/已授权/需更新）。
- 安装/卸载/修复/回滚 + **下载进度**（Channel 流式，复用 ModelLibrary 的进度模式）。
- 激活/移除授权（输入 license token → Part6 验签 → 状态刷新）。

**3.5.2 插件 gate + 购买引导**（消费 Part6 `EntitlementProvider` 前端态）：

- 未授权能力触点（点 AI 语义搜索 / 人脸 / exotic 格式缩略图）→ 检测授权 → 未授权弹 **gate**（功能说明 + 价格 + 「购买」跳官网 / 「已购买激活」入口）。
- ModelLibrary/FaceModelLibrary 接入授权态（已授权才可下载/启用对应模型；免费功能不 gate）。
- ⚠️ **开源边界**（Part0 §10）：gate UI 与授权判定均在公开源码内——判定走 Part6 `EntitlementProvider`（direct 为 `KeyringLicenseStore`，未授权 fail-closed 恒 `Unlicensed`）；前端不持验签逻辑。商业边界是私钥/签发凭证与付费载荷，非源码可见性。

### 3.6 语义搜索整合 + 人脸批量审批 UI（P1 + P4）

**3.6.1 语义搜索整合**（已完整，仅修缮）：① 评估 `SemanticSearchPanel` `v-show` 常驻 → 改 `v-if` 或确认常驻必要（watcher 开销）；② 双档 UI 对齐 Part4 模型切换（架构切换提示重分析）。

🔴 **3.6.2 人脸批量审批 UI**（P1，**配 Part4 §3.5.1 命令成对推进**）：

- **grouped likely matches 视图**：调 Part4 新 `list_likely_matches`（按候选 person 分组的未确认脸）→ 网格展示「这些可能是 X」→ 批量 **确认/拒绝/重分配**。
- personStore 加 action：`confirmAssignment`/`reassignFace`/`unassignFace`/`rejectCandidate`/`createPersonFromFaces`（调 Part4 命令）。
- PersonsView 加审批入口（现有 rename/merge/hide/recluster 之外）。
- ⚠️ **强依赖 Part4 §3.5.1 后端命令**（confirm/reassign/reject/list_likely_matches，当前后端三缺）；前后端**同一波次**交付，否则 UI 无命令可调。

**3.6.3 AI 关键词/自动标签 UI**（P4，后置/可选）：后端 caption 能力未定（Part4 未含），**后置**；预留 MediaDetailOverlay info 面板「AI 标签」区位，不在本 Part 强交付。

### 3.7 离线态 UX（P3，消费 Part2 卷 + §3.4 VolumeOffline）

> 🔴 卷在线/离线前端零响应。消费 Part2 卷可用性字段（`media_items.availability`）+ 监听事件。

- **灰显 + 角标**：MediaThumb 按 `availability='offline'` 灰显 + `CloudOff` 角标（缓存缩略图仍可见，Part0 §6 UX）。
- **VOLUME_OFFLINE 弹窗**：打开原图/视频若卷离线 → 后端返 `VolumeOffline`（§3.4 AppError code）→ 前端弹「请插入设备 `<label>`」（非破图）。
- **已知卷面板**（设置页）：列已知卷（在线/离线态）+ 重命名/删除/重扫；缺失文件手动重链接（`content_hash` 优先匹配，Part2）。
- **事件联动**：监听 Part2 `volume-availability-changed` → 刷新 availability + 重渲染角标。

### 3.8 首启向导（F2）

- 新 `OnboardingWizard` 组件，首次启动检测（config `first_launch` flag，无则显示）。
- **3 步**：① 添加扫描目录（核心，否则空画廊）；② 选主题（亮/暗/跟随系统）；③ 选语言（zh-CN/en-US）。
- 完成写 `first_launch=false`；可「跳过」（用默认）。
- 轻量 overlay（复用 SettingsView overlay 模式），不阻塞已配置用户。

### 3.9 主题 + toast 速赢（F1 + G3，零风险高回报，最先做）

- 🔴 **toast 修复**（G3）：MediaGrid 7 处 + MediaDetailOverlay 3 处 `(ui as any).showToast` → `ui.addToast(type, msg)`（去 `as any`，恢复批量收藏/删除/移动/壁纸的用户反馈）。**这是用户立即可感的修复。**
- 🔴 **主题修复**（F1）：AppShell.vue:2 `:data-theme="ui.theme"` → `:data-theme="ui.isDark ? 'dark' : 'light'"`（或移除，纯 documentElement 方案）；消除 system 模式 `.app-shell[data-theme='system']` 无匹配样式。
- 与 §3.4.3（幽灵常量 / ScanChannelPayload / startScan toast）同属 Part0 §11 速赢波，**优先级最高、改动最小、零依赖**。

### 3.10 组件拆分 + store 去重（F3 + S5 + S6）

**3.10.1 巨组件拆分**（7 个 >500 行）：

| 组件 | 行 | 拆分 |
|------|----|----|
| MediaGrid | 1580 | `useMediaFolderDrop` / `useGridContextMenu` / `useBatchOps` / `TimelineScrubber`(§3.3) / `useDimPriority` |
| MediaDetailOverlay | 923 | `MediaDetailMetaPanel` / `MediaDetailFacePanel` |
| MediaThumb | 761 | `ThumbHoverPreview` / `ThumbFaceOverlay` |
| AppToolbar | 693 | `ToolbarSearch` / `ToolbarFilters`(含 §3.2 FilterPanel) / `ToolbarSortGroup` |
| SettingsView | 835 | 各 CollapsibleCard 抽组件 + debug 函数归位 |
| FoldersSection | 642 | `FolderTreeNode` / `FolderContextMenu` |
| ToolsSection | 512 | `AiAnalysisPanel` / `FaceAnalysisPanel` |

> ⚠️ **拆分纪律**：纯结构重构、**零行为变更**（尤其多选/拖拽契约 §3.1.4 不可动）；逐组件拆 + 视觉/交互回归验证。

**3.10.2 store 去重**（S5 + S6）：
- **aiStore/faceStore 分析控制重复**（S6）：抽公共 `useAnalysisController(domain)`（start/pause/stop/restart/maybeAutoResume/2s 轮询），两 store 复用。
- **configStore 归一**（S5）：① Options API → Composition API（与其余 9 store 一致）；② 消除与 uiStore 双持 `thumbStrategy/gpuEngine`——**单一来源**（推荐 configStore 持有、uiStore 读派生，或反之，二选一去掉双写）；③ 裸字符串 `'get_app_config'` → `IPC.GET_APP_CONFIG`。

### 3.11 虚拟相册 + 布局切换（G7）

- **3.11.1 布局切换**（G7）：加 **Grid（固定宫格）** 模式 vs 现 **Justified（等高行）**——AppToolbar 切换按钮 + 密度滑块（gridRowHeight 已有）。Grid 模式可前端纯 CSS grid（固定列数/方图裁切）或后端 `group_by` 扩展；~~**优先前端 CSS grid**~~〔🔴 已被 T20 裁决推翻并落地（2026-07-02 回写）：实际采用**方案 (a) 后端 uniform-packing**——`compute_layout` 按 `layout_mode="grid"` 分支走 `compute_grid_layout`（layout/justified.rs），与 justified 共享取行+虚拟滚动+平移通路；「优先前端」未计入 SAFE_MAX/坐标平移与后端布局缓存的耦合。裁决见 [T20 合并设计](T20_T18-layout_布局策略接缝_合并设计.md) §4，落地见 todo.md Part5-T20 行〕。
- **3.11.2 虚拟相册强化**：智能相册（all/favorited/video/...）+ 用户集合（collectionStore）管理 UI——创建/重命名/删除/拖图入集；侧栏 LibrarySection 已有雏形，补集合 CRUD 完整性。

---

## §4 分步任务清单（优先级 + 依赖）

> 优先级：**P0 速赢（零依赖零风险）→ P1 稳定多选（契约文档化）+ IPC 基础设施 → P2 缺失功能（多数配后端 Part2/4/6 成对）→ P3 重构债**。配后端标【配 PartX】。

### P0 · 速赢（Part0 §11，最先，零依赖）

| ID | 任务 | 落点 | 验收锚点 |
|----|------|------|---------|
| **T1** 🔴 | toast 修复（10 处 `(ui as any).showToast`→`ui.addToast`） | MediaGrid / MediaDetailOverlay | §3.9 / 批量操作有反馈 |
| **T2** 🔴 | 主题修复（AppShell `:data-theme` raw→resolved 或移除） | AppShell.vue:2 | §3.9 / system 模式正确着色 |
| **T3** | 幽灵常量清理 + ScanChannelPayload 类型 + startScan toast | constants/ipc.ts / scanStore.ts | §3.4.3 |

### P1 · 稳定多选（契约文档化）+ IPC 基础设施

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T4** ✅🔴 | 选区脱离 DOM（Shift range / Ctrl+A 全库 / 框选稳定 id）—— 已交付 99eaf8e(T4a 模型层)+74c408e(T4b 灌入点)+回归基线 cd9ef64;手动交互回归通过 | useSelection.ts、MediaGrid、selection/、useViewIds.ts | get_view_ids（已就绪，Part2 T18 S1） | §3.1.1 |
| **T5** ✅🔴 | 竞态消除（单 drag-flag）—— 已交付 181b15a。多面板 scope 隔离暂为空(实测仅 MediaGrid+SelectionToolbar 消费,待语义面板接入) | useSelection.ts、MediaGrid | — | §3.1.2 |
| **T6** ✅ | 撤销一致性（onMoveCopyConfirm 走 historyStore）—— 已交付 181b15a(MediaGrid + MediaDetailOverlay 两入口) | MediaGrid.vue、MediaDetailOverlay.vue | — | §3.1.3 |
| **T7** ✅🔴 | 交互契约**文档化**（当前协议,标注临时可改,非冻结）—— 已回写 §3.1.4「已实现基线」表 + as-built 偏差 | 本文档 §3.1.4 | — | §3.1.4 |
| **S4(后端)** 🔴 | 暴露 `resolve_selection`/`count_selection` 为 IPC + 8 批量命令 `Vec<i64>`→`SelectionDescriptor` + lib.rs 注册;解除 all 态过渡物化(现 materializeIds 全库物化喂 IPC) | media_commands.rs / face_commands.rs / lib.rs | T4 SelectionState.toDescriptor(已就绪) | §3.1.1 P0-9 |
| **T4c** | 批量操作切 `toDescriptor()` 直传,过渡物化下线 | MediaGrid 批量 ops | ← S4 | §3.1.1 |
| **T8** | 统一 IPC wrapper `invokeIpc` + 收敛 ~25 裸字符串 | utils/ipc.ts、各 store | — | §3.4.1 |
| **T9** | 结构化 AppError（后端分批 ai/face/doc + 前端 parseAppError）；🔴 **显式新增 `AppError::VolumeOffline` 变体**（error.rs 现无，T13 离线 UX 硬依赖）+ Serialize 注册 `code='VolumeOffline'`，由打开原图/视频 IPC 在 `availability='offline'` 时返回 | 后端命令 + error.rs + utils/ipc.ts | 前后端协同 | §3.4.2 |

### P2 · 缺失功能补全（配后端成对）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T10** 🔴 | 人脸批量审批 UI（likely matches 分组 + 确认/拒绝/重分配） | PersonsView、personStore | **← Part4 §3.5.1 命令（成对）** | §3.6.2 |
| **T11** | exotic 插件商店 UI（列出/安装/激活/进度） | 新 PluginStoreView | ← Part6 命令 | §3.5.1 |
| **T12** | 插件 gate + 购买引导 | 触点组件 | ← Part6 EntitlementProvider | §3.5.2 |
| **T13** | 离线态 UX（灰显+CloudOff 角标+VOLUME_OFFLINE 弹窗+已知卷面板） | MediaThumb、设置页 | ← Part2 卷 + §3.4 VolumeOffline | §3.7 |
| **T14** | 时间轴滚动条（时间均布+密度热力+年份+拖拽） | 新 TimelineScrubber | ← Part2 §3.8 直方图 | §3.3 |
| **T15** | 星级快捷入口（hover 评分+键盘）+ FilterPanel | MediaThumb、AppToolbar | — | §3.2.1 |
| **T16** | 颜色标签（设色 UI + 筛选） | 详情/右键、FilterPanel | ← **Part1 加 `color_label` 列** | §3.2.2 |
| **T17** | 首启向导（3 步 onboarding） | 新 OnboardingWizard | — | §3.8 |

### P3 · 重构债

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T18** | 巨组件拆分（7 个 >500 行，零行为变更） | MediaGrid 等 | ← T7 契约文档化先 | §3.10.1 |
| **T19** | store 去重（useAnalysisController + configStore 归一） | aiStore/faceStore/configStore | — | §3.10.2 |
| **T20** ✅ **已实施（后端 uniform-packing）** | 布局切换 Grid/Justified〔原「前端 CSS grid」已被 T20 合并设计推翻，实际=后端 `compute_grid_layout` 分支+前端切换，2026-07-02 回写〕 | AppToolbar、MediaGrid、`layout/justified.rs`、`ipc/layout_commands.rs` | — | §3.11.1 / todo.md T20 行 |
| **T21** | 虚拟相册强化（集合 CRUD） | LibrarySection、collectionStore | — | §3.11.2 |

---

## §5 风险与回滚

| 风险 | 触发 | 缓解 / 回滚 |
|------|------|------------|
| 🔴 **多选契约 vs 组件拆分冲突** | 拆 MediaGrid 时误改多选/拖拽行为 | T7 契约文档化**先于** T18 拆分(先有对照基线再拆)；拆分纯结构、零行为变更；交互回归用例护栏 |
| 选区脱离 DOM 依赖 flat_ids | Part2 布局缓存未暴露 `flat_ids` 给前端 | 确认 Part2 接口（cache.rs 已有 flat_ids，§2.6）；未暴露则加只读命令 `get_view_ids` |
| 🔴 **人脸审批 UI 空转** | Part4 confirm/reassign/likely_matches 后端三缺 | T10 与 Part4 §3.5.1 **同波次**；后端先合入再做 UI；CI 断言命令存在 |
| 插件商店/gate 配 Part6 未就绪 | Part6 fetch_exotic_registry/install/Entitlement 未交付 | T11/T12 排 Part6 之后；先做不依赖 Part6 的 P0/P1 |
| 颜色标签依赖 Part1 加列 | Part1 无 `color_label` 列 | 回溯 Part1 §3.7 schema 债（类似 persons.model_name）；T16 排 Part1 加列后 |
| AppError 化 72 命令大改 | 一次全改易引回归 | **分批**（ai/face/doc 起，非一次）；前端 `invokeIpc` 先上、**兼容 String 和结构化**（渐进迁移） |
| Ctrl+A 全库选区内存 | 百万 id 灌内存 | 全选语义标记 + 排除集；批量操作后端按当前 filter 解析全集，不前端枚举 |
| 速赢 toast 改动面广 | 10 处分散 | 纯文本替换 `(ui as any).showToast`→`ui.addToast`，逐处验证有 toast 弹出 |

---

## §6 验收标准

**P0 速赢**：

1. **toast**：批量收藏/取消/删除/移动复制/壁纸操作均弹 toast（不再静默）。
2. **主题**：`system` 模式下 `.app-shell` 子元素正确着色（不再无匹配）。
3. **清理**：幽灵常量删除后 build 通过；扫描进度正确显示（🔴 P1-5：`ScanChannelPayload` 扁平联合，读顶层 `msg.scanned`，**不读** `msg.progress`，§3.4.3）；扫描失败弹 error toast。

**P1 多选 + IPC**：

4. **稳定多选**：Shift+Click 跨视口 range 正确（不再 -1 失效）；Ctrl+A 选全库（显示「已选 N 项」）；框选稳定；快速单击不被误判拖拽（详情可开）；主画廊与语义结果选区隔离；移动/复制可撤销。
5. **契约文档化**：多选/拖拽手势当前协议写入文档,标注**开发期临时可改**（非冻结,作团队对齐 + 回归基线）。
6. **IPC 统一**：store 无裸字符串 invoke（全经 `invokeIpc` + 常量）；`AppError` code 可区分（`Cancelled` 静默 / `VolumeOffline` 弹窗 / 其余 toast）。

**P2 缺失功能**：

7. **人脸审批**：likely matches 分组展示 → 批量确认/拒绝/重分配可用（配 Part4 命令）。
8. **插件商店**：列出/安装/激活/卸载/下载进度可用；未授权能力弹 gate 引导。
9. **离线 UX**：卷拔出 → 该卷项灰显 + CloudOff 角标；打开离线原图 → 「请插入设备 `<label>`」弹窗；重连自动恢复。
10. **时间轴**：scrubber 按时间均布 + 密度热力条 + 年份 label + 拖拽跳转。
11. **星级/颜色**：hover 快捷评分 + FilterPanel 星级筛选；颜色标签设置 + 按色筛选（配 Part1 列）。
12. **首启向导**：首次启动 3 步（目录/主题/语言），完成不再弹。

**P3 重构债**：

13. **拆分**：7 巨组件拆分后**交互/视觉回归零变化**（尤其多选契约）；store 去重（aiStore/faceStore 共用 controller、configStore 单一来源）。

---

## §7 执行提示词（新会话直接用）

```
任务：实施 Picasa Next 重构 Part5（前端体验重构）。

【先读】
1. docs/refactor_2026/Part0_总纲与产品定稿.md §4(功能矩阵)/§11(波次,Part5 速赢+Wave)/§12(约定)
2. docs/refactor_2026/Part2(布局数据 flat_ids + 时间直方图 §3.8 + 离线态字段)、Part3(asset://+thumbhash+缓存IPC)、
   Part4(语义搜索/人脸审批命令 §3.5.1 + 插件 gate)、Part6(插件平台 IPC + EntitlementProvider) 的 §7
3. docs/refactor_2026/Part5_前端体验重构.md 全文（§2 现状取证为基线，§3 设计，§4 任务表）

【铁律】
- 🔴 交互契约**开发期不冻结、临时可改**(取代旧「发布即冻结」;见 no-contract-freeze-during-dev)；T7=文档化当前协议作回归基线,**先于** T18 拆分(拆分前先有对照,防回归,非定死)。
- 稳定多选核心=选区脱离 DOM(基于布局序 flat_ids,非可视节点)；三症状(Shift 跨视口/Ctrl+A 一屏/框选渲染节点)同一病因。
- 画廊筛选来自 uiStore 非 router；侧栏 accordion fragment-sticky 不变量(绝不包 div)；拖放用 pointer 事件(Tauri 阻断 HTML5 DnD)。
- 人脸审批 UI(T10)/插件商店(T11)/gate(T12)/离线(T13) 配后端 Part4/Part6/Part2 成对,后端先合入再做 UI,否则空转。
- IPC AppError 化分批(ai/face/doc 起),前端 invokeIpc 先上兼容 String 和结构化(渐进)；裸字符串 invoke 全收敛常量。
- Vue3 Composition + TS strict + Pinia；中英双语注释；改后中文 commit；仅用户通知时 push；大文件小步 Edit。

【顺序】
P0 速赢(零依赖,最先): T1 toast → T2 主题 → T3 幽灵常量+类型+扫描 toast
P1 稳定多选+基建: T4 选区脱离DOM → T5 竞态消除 → T6 撤销一致 → T7 契约文档化(非冻结) → T8 IPC wrapper → T9 AppError
P2 缺失(配后端): T10 人脸审批(配 Part4) → T11 插件商店(配 Part6) → T12 gate(配 Part6) → T13 离线UX(配 Part2)
   → T14 时间轴(配 Part2 §3.8) → T15 星级+FilterPanel → T16 颜色标签(配 Part1 加列) → T17 首启向导
P3 重构债: T18 巨组件拆分(契约文档化后,零行为变更) → T19 store 去重 → T20 布局切换 → T21 虚拟相册

【验收】按 §6 十三条；P0 三条速赢 + T4/T5 稳定多选 + T7 契约文档化(非冻结)是核心门槛。
【关键认知】toast 静默失效(10 处 as any showToast 永假)是用户立即可感的速赢；多选稳定必须先解耦选区与虚拟化;
人脸审批/插件商店/离线 是「前端缺失但后端也需配套」的双缺,务必前后端同波次。
```

---

> **Part 5 正文完**。下游：Part6（插件平台框架 + exotic 收尾，提供本 Part 插件商店/gate 消费的 IPC + EntitlementProvider）、Part7（发布工程）、Part8（商业化分发，购买引导落地页 + 定价）。
> 回溯需求：**Part1 加 `media_items.color_label` 列**（T16 依赖，类似 persons.model_name 回溯）。
> 执行前必读：Part0 §13 + 各依赖 Part §7 + 本文 §7。
