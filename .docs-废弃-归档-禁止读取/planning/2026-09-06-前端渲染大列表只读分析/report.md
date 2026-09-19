---
status: 施工中
type: 分析报告
line: 前端渲染大列表只读分析
created: 2026-09-06
---

# 提示词6只读分析报告：前端渲染 / 大列表 / 事件消费流水线（2026-09-06）

> 核实对象：`docs/reviews/2026-09-05-性能流水线梳理与分线优化提示词.md` 提示词6（:261-298）中 B1–B6 六个静态可疑瓶颈。
> 核实方式：只读静态核实（4 路并行探索 + 主会话抽查复核），**未改任何代码、未跑构建、未做实测**。行号为 2026-09-06 当前工作区实况。
> 报告定位：给后续实施会话的「瓶颈点现状核实表 + 基准工具盘点 + 风险与前置条件清单」，不是优化方案终稿。

## 0. 核实环境与漂移基线

- HEAD = `7fea98e3`，与提示词快照基线**一致**；composables 层（虚拟滚动/缩略图/事件同步）相对快照**零漂移**。
- 工作区有未提交主题系改动（主题色浓度/文字浓度/玻璃模式三条并行线），触及 `MediaGrid.vue`（仅 +2 行 props 透传）、`MediaGridCanvas.vue`（+22）、`mediaGridCanvas.cellRenderer.ts`（+45）、`mediaGridCanvas.helpers.ts`（+36）、`mediaGridCanvas.palette.ts`（+20）、`useCanvasHoverCard.ts`（+35）、`uiStore.ts`（+158）。Canvas 侧行号整体 +16~20 漂移，MediaGrid.vue +2。
- **结论：实施会话面对的是工作区而非 HEAD**；本报告所有行号均按当前工作区给出，并标注与快照的差异。

## 1. 总判定表

| 编号 | 快照声称 | 判定 | 一句话结论 |
|---|---|---|---|
| B1 | DOM 分支深响应式大数组 + 深 watch | **部分成立（机制需改写）** | visibleRows 确为深 ref，但规模有界（几十行）；DOM 侧**无深 watch**；真实成本 = 深代理 props + 窗口越界整体替换 + 每格 ≥2 个 watcher 扇出 |
| B2 | db:media_enriched → 2s 防抖 → 全量 compute_layout | **成立（注释位置漂移）** | 链路属实且已有两层合并（防抖+离屏）；「全库量级 IPC」注记在 useJustifiedLayout/useGalleryVirtualEngine 文件头，不在 useGalleryTauriSync |
| B3 | 404 懒自愈逐格触发、前端请求侧已有 single-flight | **前端侧部分成立** | 自愈触发链属实；但 single-flight 仅在 batch 队列，regenerate 路径**无跨实例去重**；快照引用的「ipc.ts CANCEL 注」已悬空 |
| B4 | Canvas draw 每帧全窗扫描 + selAnim.sync | **成立且被低估** | selAnim.sync 每帧第二遍全可见项遍历 + 每帧新建 Map 核实；另发现多项快照未记录的每帧/每格开销（见 §3 N5–N7） |
| B5 | >10M px 平移模式非被动 wheel | **成立（措辞修正）** | 是**显式 `{ passive: false }`**（非「未设」）；preventDefault + scrollTop 手动回写补偿语义核实 |
| B6 | computeLayout 每次重算 JSON.stringify 语义键 + viewportMeta 清理 | **成立（行号漂移）** | stableSerialize/buildLayoutContentKey 与快照吻合；viewportMeta 清理实际在 mediaStore.ts:231-244，且 scope key stringify 每轮 IPC 往返都跑 |

## 2. 逐项核实

### B1【DOM 分支深响应式】部分成立——机制描述需要改写

**快照原话**：visibleRows 是 ref\<LayoutRow[]\> 深响应（useVirtualScroll.ts:76），bucket 段行是 reactive 内深代理；MediaGrid.vue DOM 路径每格组件吃深代理 props；Canvas 侧已用浅 watch+patchTick 修复，DOM 分支仍保留深响应路径。

**核实结果**：

- `src/composables/useVirtualScroll.ts:76` `const visibleRows = ref<LayoutRow[]>([])` —— 深响应 ref **属实**（主会话抽查确认）。写入点 `:321`（fetch 落地整体替换）、`:263`（清空）。
- **但「大数组」措辞不实**：可见窗口 + 自适应缓冲（`:288-293`，缓冲高 240–1200px）通常只有**几十行**；且 `:296-303` 有包围盒 skip 守卫（窗口未移出上次获取框则不取数）。
- **DOM 侧无深 watch**：全代码库唯一 `deep: true` watch 在 `player/usePlayerPrefs.ts:57`，与网格无关；对 rows 的 watch（`useViewportDimPriority.ts:80`、`useGalleryTauriSync.ts:103-127`）均为浅 watch。快照 B1 若被理解为「DOM 分支要照抄 Canvas 的深 watch→浅 watch 改造」，则改造对象不存在。
- **深代理 props 属实**：`MediaGrid.vue:212-239` `v-for="row in visibleRows"` → 每行一个 `MediaGridRow`（`MediaGridRow.vue:85-176`）→ 每格一个 `.media-card` 内嵌 `MediaThumbCompact`/`MediaThumb`；行对象与 item 对象都是深代理整对象传递。bucket 段行同型（`useBucketVirtualScroll.ts:260-267` `reactive({...})`）。
- **~1000 格出处**：`MediaGrid.vue:624-627`（快照 :622，+2 漂移）`compactCells = computed(() => ui.gridRowHeight < 100)` + 注释「极小单元尺寸下视口可容纳约 1000 个单元」——是注释估计值，无公式；`MediaGrid.styles.css:528` `content-visibility: auto` 仍在用。
- **窗口越界 = 窗口级全量重渲染**：一旦越过包围盒，`visibleRows.value = rows`（`:321`）整体替换 → 窗口内每行/每格拿到全新深代理 props → 全量 re-render。这是深响应路径的真实量化形态（快照未写明）。
- Canvas 侧修复的现状核实：`MediaGridCanvas.vue:539-545` rows **浅 watch** + `:547` patchTick watch；`:535-538` 原注释完整保留（「#15:原为 {deep:true}…同步开销集中在边界那一帧 = 周期性卡顿主因」）。

**实施会话注意**：B1 的可改造点不是「去深 watch」，而是①整窗替换导致的窗口级重渲染、②每格 watcher 扇出（见 N3）、③是否强制极密场景走 Canvas（`canvasMode` 判据在 `MediaGrid.vue:819-821` + `useGalleryVirtualEngine.ts:109-112`，canvasCapable = 非 iOS 且 totalHeight ≤ SAFE_MAX——**极密但总高 ≤10M px 时仍可能走 DOM**）。

### B2【enriched 事件 → 全量重算】成立

- `src/composables/useGalleryTauriSync.ts:48-57`：`useTauriListen(EVENTS.MEDIA_ENRICHED)` → `clearTimeout` + `setTimeout(..., 2000)` → `deps.requestCompute()`。与快照一致，零漂移。
- **注释位置漂移**：useGalleryTauriSync 文件头（:1-4）是「画廊 Tauri 事件 + 布局脏标/版本联动」；「compute_layout 是全库量级 IPC」注记实际在 `useJustifiedLayout.ts:24-25` 与 `useGalleryVirtualEngine.ts:26-27`。
- **已有两层合并**（实施会话评估增量方案时不可忽略）：①2s 防抖；②离屏合并——`useJustifiedLayout.ts:193-200` 离屏只记 dirty 不发 IPC，激活时 `flushIfDeferred`（:208-215）把失活期累积请求合并为一次。
- 触发源比快照扩容：`useJustifiedLayout.ts:219-254` 的布局触发 watch 源已扩到 **15 个**（含 `filter.apiFilterKey` :229、`ui.searchQuery` :234），`flush: 'post'`，无防抖节流（键击/开关即触发；快照记载上游搜索提交路径有 500ms 防抖，本次未复核）。
- **扇出新发现**：MEDIA_ENRICHED 实际有 **4 个前端监听器**（主会话 grep 确认）：useGalleryTauriSync.ts:48（2s 防抖重算）、scanStore.ts:310（enrichment 进度入 progressMap，10 分钟看门狗）、DocThumbRenderer.vue:175（pumpDebounced）、useDerivationAutoStart.ts:65（kickDebounced）。后端每 500 项一次的事件同时驱动四条路径。
- 后端侧事实：enrichment 每批 500-1000 条 emit 一次（与快照一致）；`get_rows_by_y` 走后端内存行缓存零 DB（`layout_commands.rs:922-971` + `layout/cache.rs:407-430` RwLock + binary_search）——「增量补行替代全量 compute_layout」的后端前提确实存在。

### B3【404 懒自愈】前端侧部分成立——single-flight 归属需澄清

- 触发链核实：`useThumbLoader.ts:75-80` `requestHealOnce`（仅 status===1 且 **per-loader 实例**守卫 `hasRequestedHeal` :68）+ `onError` 兜底（:165-169，内部仍判 status===1）→ `MediaGrid.vue:1295-1316` `onRegenerateThumb`：乐观 patch（`item.thumbStatus = 0; item.thumbPath = null; bumpCanvasPatchTick()`）→ `invokeIpc(IPC.REGENERATE_MISSING_THUMB, { id })`。另有独立路径 `useContentViewerPosterSource.ts:75`（视频 poster 404，`healedPosterIds` Set 去重）。
- **「请求侧去重已有 single-flight」需限定**：single-flight 仅存在于 batch 队列（`useRequestQueue.ts:213-230` `activeSlots` 按 id 复用 slot）；`regenerate_missing_thumb` 路径**无模块级/跨实例去重**——同 id 多卡（重复镜头等场景）会重复发 IPC，且每次自愈对 `activeRows()` 做线性 find（:1298-1301）。
- 乐观 patch 使死 URL 不再重试（useThumbLoader watcher :172-177 仅响应 0→1/3 迁移），后端复位后经 hydration 回 status=1 再重载——这个止滚设计核实存在。
- 后端每项 stat+复位+invalidate layout_cache（快照 thumbnail_commands.rs:754-759）**属提示词2线（缩略图流水线），本线不核实不处置**；前端侧合并空间即上述「同 id 去重」。
- 附带发现：`useRequestQueue.ts:22-23` 注释声称后端在途项 single-flight 注记在「ipc.ts CANCEL 注」，但当前 `src/utils/ipc.ts`（118 行）无任何相关注记，**引用悬空**。

### B4【Canvas draw 每帧全窗扫描 + selAnim.sync】成立，且快照低估

draw() 主体（工作区 `MediaGridCanvas.vue:392-453`；HEAD 版 :376-437，逐行吻合，+16~20 漂移）每帧流程核实：

1. `fitCanvas`（:234-265）：每帧 `cv.getContext('2d', {alpha})` + 读 `window.devicePixelRatio` + 两次 `setTransform` + **全 buffer clearRect/fillRect**（整窗重绘固有成本）+ 重设 imageSmoothing——快照未记录。
2. `visibleRowRange`（helpers.ts:220-246）两次二分界定可视行集。
3. `prioritizeVisibleThumbLoads`（:403；useCanvasThumbPipeline.ts:254-269，仅闸门关闭且 64 槽满时遍历）。
4. `selAnim.sync`（:404；**helpers.ts:519-531，主会话抽查确认**：每帧对全部可见项遍历 + 每项调 isSelected 谓词 + **每帧 `new Map()`** 整体替换快照）——即**每帧对可见项的全遍历实际发生两次**（draw 主循环 + selAnim.sync），快照只记了一次。
5. 主循环（:410-434）：遍历与视口相交的全部行×item，每格走 `drawCell`（cellRenderer.ts:78-253：drawImage/文本卡/描边/色条/覆盖层/角标/选中描边/checkbox 等）。
6. 收尾：prefetch 分片（:438-441，硬预算 1ms/16，useCanvasThumbPipeline.ts:455-460）；`selAnim.hasActive` 时连续 scheduleDraw（:442）。

**每格每帧开销（快照未记录，均有代码证据）**：
- `getImage`（useCanvasThumbPipeline.ts:106-146）：每格 2 次模板串拼接（thumbStatus|thumbPath、#高度桶）+ 每格读 `devicePixelRatio` + 7 元素桶数组线性扫（helpers.ts:383-389）+ 2 次 Map 查 + syncSig。
- 临时对象：`coverRect` 每格新对象（cellRenderer.ts:143 / helpers.ts:363-371）；`starPoints` 每颗星新建 10 元组（painters.ts:230）；`visibleItems` 生成器每帧新建（MediaGridCanvas.vue:280-290）。
- **measureText 多处未缓存**：drawDuration（painters.ts:104）、drawAvailBadge（:131）、drawTextCard（:197）、drawLensBadge（:452）、drawSeparator（:417）、信息浮窗标题截断 truncateToWidth 内部多次 measureText（infoOverlay.ts:36-37/:96-99）。
- 视频格 formatDuration 字符串构建每帧执行（cellRenderer.ts:189 → format.ts:33-43）。

**调度事实**：currentY watch（:524-533）每 scroll 事件同步执行（含 clearHover/cancelPrefetchPlan/setScrollDirection，无节流）；合帧完全依赖 `CanvasRafScheduler` 同代次 rAF 合并（helpers.ts:71-96，:76-82 返回 'coalesced'）——每帧至多一次 draw，但每次 draw 的成本即上述全量。
**命中测试**：两级二分核实（rowIndexAtY helpers.ts:249-262 + hitTestCellWithRow :269-285，入口 useCanvasHitTest.ts:26-44）；DOM 仅悬停卡单例核实（MediaGridCanvas.vue:13-70）。

### B5【>10M px 平移模式非被动 wheel】成立（措辞修正）

- 阈值：`useVirtualScroll.ts:35` `SAFE_MAX_DEFAULT = 10_000_000`；`:260` `isTranslated = logicalTotal > SAFE_MAX`；`:113` physicalTotal 封顶。
- wheel：`:163-179` onWheel；注册 `:187` `addEventListener('wheel', onWheel, { passive: false })`——快照说「非被动 wheel 监听（passive 未设）」，实际是**显式 `{ passive: false }`**，语义相同（非被动）。
- 补偿语义核实：`:174` preventDefault → `:176` `container.scrollTop += dy / ratio`，`ratio = logMax/physMax`（:168，可达 2–4×）手动回写物理滚动位。
- 生命周期完备：`watch(isTranslated)`（:197）仅平移模式挂载，退出即移除（:190-193），onBeforeUnmount 兜底（:393）；无泄漏。容器模板另有 `@wheel.passive="onGridWheel"`（MediaGrid.vue:25）只转发 bucket 引擎，两者并存不冲突。
- syncTransform（:133-145）每帧写 `style.transform` 属滚动回调内命令式写入（刻意避开 Vue 渲染，:16 文件头注释自证），带 `lastAppliedOffset` 冗余写守卫（:93/:140）——机制健康，B5 的疑点仅在 wheel 处理本身是否构成输入延迟，需实测。

### B6【computeLayout stringify + viewportMeta 清理】成立（行号漂移）

- `mediaStore.ts:29-42` `stableSerialize`（filters 键排序递归 stringify）+ `:50-59` `buildLayoutContentKey`（`normal:d{目录}:f{...}`；lens 模式走常量键）——与快照 :29-59 吻合。
- 调用频率：`:190`（每次 computeLayout 入口、IPC 前）、`:289`（每次提交时）；computeLayout 触发源见 B2（15 源 watch + resize 防抖 + enriched 2s 防抖 + totalItems/layoutDirty/dedup 完成 watcher）。
- **viewportMeta 清理漂移**：实际在 `:231-244`（快照 :214-230 现为 watchdog 块）。`nextMetaScopeKey = JSON.stringify({directoryId, filters})`（:231-234）**每轮 while 迭代跑一次**（即每次 computeLayout IPC 往返，即使 scope 未变），且与语义键是两套序列化。scope 变化才清 Map + metaTimer + pendingMetaIds。
- 滚动取行路径核实：`mediaStore.ts:302-316/:320-332` fetchRowsByY/fetchBucketRows 是**纯 invokeIpc 透传**——快照把「rAF 节流+在途合并+陈旧丢弃」挂在 mediaStore.ts:302-332 名下属**位置错置**：这些机制实际在 useVirtualScroll.ts:223-237（rAF+pendingUpdate 合并+:312-319 陈旧丢弃）与 useBucketVirtualScroll.ts:378-394（pumpFetch 单飞 1-2+:348-372 落地三重复核）。

### 附：进度整体替换（快照 B2 段落附带声称）

- `scanStore.ts:397-406`：Channel 每消息**逐根条目**替换 `progressMap.value[rootId] = {...}`（快照说「整体替换 progressMap」不精确，Map 本体不换）；消费方 `isAnyScanRunning`（:54）与 `aggregateProgress`（:55-78）都是 `Object.values` 全表扫描 computed → **每条 Channel 消息触发全表重算**，侧栏/状态条（AppStatusBar、ScanProgressIndicator、ManagementSection）同步重渲染；写入端无节流（仅 loadStats 有 1s 节流 :407-413）。
- `scanStore.ts:578-585`：`thumbGenProgress.value = {...}` 每事件整对象替换，无节流；消费方 AppStatusBar/ToolsSection/SettingsView 三处；`completed/cancelled` 时连带 `media.invalidateLayout()`（:588-591）——全量生成收尾时一次布局失效属合理语义，但事件频率下三组件重渲染是持续成本。

## 3. 新发现清单（快照未记录）

| # | 发现 | 位置 | 量级评估 |
|---|---|---|---|
| N1 | visibleRows 规模有界（几十行），「深响应大数组」措辞不实 | useVirtualScroll.ts:288-303 | 修正快照定性 |
| N2 | `logicalScrollTop` 注释失实（:87 称「非热渲染绑定」）：实际经 `currentLogicalY` computed 被 MinimapAxis/MediaGridCanvas/自研滚动条 props 链消费，平移模式每滚动事件驱动该链更新 | useVirtualScroll.ts:87、useGalleryVirtualEngine.ts:95-97、MediaGrid.vue:96,250,296,308,318 | 子组件轻量，非全网格重渲染，但注释须改 |
| N3 | **每格 ≥2 个 watcher 扇出**：每卡经 useThumbLoader 注册 `watch([thumbPath, thumbStatus])` + `watch(loadGate)`；compact ~1000 格 ≈ **2000+ 活 watcher**；闸门翻转（setDeferThumbLoad）即全量齐发——`mediaGrid.helpers.ts:321-322` 注释自证（滞回带为此设计） | useThumbLoader.ts:172-184、useThumbLoadGate.ts:24-27 | 快照 B1 未记录；极密 DOM 场景的主要隐性成本 |
| N4 | 方案 A 窗口越界 = visibleRows 整体替换 → 窗口级全量重渲染（全新深代理 props） | useVirtualScroll.ts:321 | B1 真实形态 |
| N5 | 每帧可见项**双重全遍历**（draw 主循环 + selAnim.sync），selAnim.sync 每帧新建 Map | MediaGridCanvas.vue:404/:410-434、helpers.ts:519-531 | B4 低估点 |
| N6 | 每格每帧：2 次字符串签名拼接 + DPR 读取 + 7 桶线性扫 + 2 Map 查 + coverRect 新对象 + starPoints 新数组 + 视频格 formatDuration | useCanvasThumbPipeline.ts:106-146、helpers.ts:363-389、cellRenderer.ts:143/:189、painters.ts:230 | B4 低估点 |
| N7 | 每帧多处未缓存 measureText（时长/角标/文本卡/徽标/分隔行/浮窗截断）+ fitCanvas 每帧 getContext/DPR/全屏 clearRect | painters.ts:104,131,197,417,452、infoOverlay.ts:36-99、MediaGridCanvas.vue:234-265 | B4 低估点；量级需基准确认 |
| N8 | MEDIA_ENRICHED 扇出 4 监听器（快照只记 1 条） | 见 B2 | 事件窗口合并设计需覆盖全部扇出 |
| N9 | thumbGenProgress 每事件整对象替换无节流 + 3 组件消费；progressMap 每消息触发全表 computed 重算 | scanStore.ts:578-591/:54/:55-78 | 与后端事件节流（100ms）叠加后量级中等 |
| N10 | regenerate 自愈无跨实例去重 + activeRows 线性 find；useRequestQueue.ts:22-23 悬空注释引用（ipc.ts 无 CANCEL 注） | MediaGrid.vue:1295-1316、useRequestQueue.ts:22-23 | B3 前端侧合并空间 |
| N11 | 轻微：geometry() 每滚动事件新建对象（两引擎）、bucket 每事件拼接 range key（有 lastRangeKey no-op 守卫） | useVirtualScroll.ts:109-117、useBucketVirtualScroll.ts:230-241/:298-299 | 量级极小 |
| N12 | 快照「rAF 节流+在途合并+陈旧丢弃」位置错置（在 composables 不在 mediaStore）；媒体库滚动取行后端零 DB 核实成立 | mediaStore.ts:302-332、useVirtualScroll.ts:223-237、layout_commands.rs:922-971 | 修正快照定位 |

## 4. 未提交主题改动对热路径的影响（实施会话基线风险）

**结论：未触及核心热路径，唯逐格一次廉价新增。**

- `mediaGridCanvas.cellRenderer.ts:92`：每格新增 `deps.mirrorCellId() === item.id`（浅读 + 数字比较，悬停镜像场景至多 1 格命中反而停画多处 chrome）——每格路径唯一新增。
- `palette.ts` 改动只落在 `applyPalette`（挂载/激活/令牌 watch 时执行）；每帧引用的仍是同一缓存 palette 对象（MediaGridCanvas.vue:207-212）。
- draw() 主体、滚动 watch、rows 浅 watch、patchTick watch 均未被改动；`helpers.ts` 改动仅悬停卡弹卡路径；CSS 改动走单卡 DOM 合成路径（注释自证规避逐帧插值）。
- 风险在**基线时效**：主题三线收口前后行号与代码形态还会变，实施会话开工时应以届时工作区重跑本报告的抽查锚点（§2 各 file:line）。

## 5. 性能基准工具盘点（实施会话的量化入口，全部核实存在）

| 工具 | 位置 | 能力 |
|---|---|---|
| performanceRecorder | src/perf/performanceRecorder.ts | 帧间隔环形缓冲（FRAME_CAPACITY 36_000 ≈ 240Hz×150s，:96）；span 6 种（gallery.draw/gallery.bitmapLoad/gallery.imageFallback/timeline.*，:8-15）；long task（优先 long-animation-frame 回退 longtask，:204-226）；Event Timing（threshold 16ms，:227-244）；JS heap（:110-113）；counters/gauges（:182-195）；低成本轮询口 `currentCounters()`（:177-180，基准可安全高频调用）；`snapshot()` 带排序分配（:168-170，**基准中不得调用**）；start/stop 返回完整摘要（:141-165） |
| performanceStats | src/perf/performanceStats.ts | DistributionSummary（p50/p95/p99）、estimateFrameBudget（P20+12% 吸附刷新率，:82-99）、summarizeFrameHealth（jankFrames>1.5×budget、missedVsync，:102-125） |
| usePerformanceMonitor | src/composables/usePerformanceMonitor.ts | 校准（25 次 rAF，:47-55）；默认 10s 录制（:18，面板可选 5/10/20/30）；**「画廊三往返」自驱滚动基准 runGalleryRoundTrip**（:142-199：预热 1 趟不入样、3 趟正式、距离 max(3×clientHeight, 2400)、leg 900ms-2.5s、context='gallery-roundtrip'、结束恢复 scrollTop）；**`window.__scrolleryBench` DEV 桥**（:283-295，仅 `import.meta.env.DEV` 挂载） |
| __scrolleryBench API 面 | 同上 | `start(seconds=10)` / `stop()` / `roundTrip()`（phase==='idle' 才可）/ `phase()`（idle\|arming\|recording）/ `sessions()`（≤12 条 PerformanceSessionSummary）/ `sampleGauges()`（录制期低频轮询） |
| PerformancePanel | src/components/devtools/PerformancePanel.vue | Ctrl+Shift+P（:271-279）；展示 avgFps/p95/p99/missedVsync/jankRatio/span 表/longFrames/inputDelay/gauges（gallery.cacheBytes/cacheItems）/heapDelta；JSON 导出（:261-269） |
| 记录点（已盘点全部调用方） | — | gallery.drawRequests/drawCoalesced（MediaGridCanvas.vue:274-275）；**gallery.visibleCells/visibleColdCells/coldCellFrames/cacheItems/cacheBytes/inFlightThumbs/prefetchedCells**（:444-450）；bitmapLoadsCancelled/imageFallbackLoaded/Failed/prefetch 系列（useCanvasThumbPipeline.ts:245-437）；timeline.drawRequests/drawCoalesced（TimelineScrubberCanvas.vue:643-644） |
| 附加探针 | src/composables/useGalleryPerfProbe.ts | localStorage `scrollery.debug.perfProbe` 门控；`window.__scrolleryPerf.census()` 返回 {cards,totalNodes,visibleCards}+滚动 FPS+选择态耗时（:55-133）——DOM 分支节点数普查的现成入口 |

## 6. 测试基线（实施会话验证命令的前置盘点，全部核实存在）

| spec | 用例数 | 覆盖面 |
|---|---|---|
| useVirtualScroll.spec.ts | 31 | 映射/SAFE_MAX 边界/缓冲窗口/fetch 去重/syncTransform 守卫/wheel 补偿（含 passive:false 挂载断言） |
| useBucketVirtualScroll.spec.ts | 39 | 段算术/取数管线（单飞/远跳丢弃/换代）/映射态（巨跳重锚/停稳偿债/scrollToLogicalY） |
| useGalleryVirtualEngine.spec.ts | 8 | bucketActive/canvasCapable 判据（含 iOS 伪装识别） |
| useJustifiedLayout.spec.ts | 1 | cancelPendingResize |
| mediaStore.spec.ts | 11 | 详情时序/连续 resize 取最新/同语义保留旧布局/内容键排序无关/watchdog 迟到丢弃；**不覆盖** fetchRowsByY/fetchBucketRows/viewportMeta 清理 |
| MediaGridCanvas.glass.spec.ts、MediaGridCanvas.hover.spec.ts | 存在（hover 有未提交改动 +75） | 玻璃背景/悬停卡 |
| useThumbLoader / useThumbLoadGate / useRequestQueue spec、performanceRecorder / performanceStats spec | 存在 | 快照要求的全集在位 |

## 7. 实施会话的前置条件与风险清单

1. **基线先行**：提示词6工作方式要求「优化前后同基准对比」。三往返基准 + `__scrolleryBench` 已可用，但仅 DEV 挂载；需构造大规模测试库（快照未给造库方案，实施会话需先解决），并注意基准跑在 DOM 分支还是 Canvas 分支（引擎选择受 `canvasCapable` 与 `bucketSegmentedScroll` 影响，极密+总高≤10M px 可能走 DOM 分支，恰是 B1 主场）。
2. **基线时效**：主题三线未提交改动在改 Canvas 侧文件；实施应等其收口或至少固定基线 commit 后开工，行号以届时工作区为准（本报告锚点可重跑抽查）。
3. **行为等价门槛**（快照原文要求，核实中确认了这些语义的承载点）：滚动位置（bucket 映射态偿债/scrollToLogicalY）、选择语义（selAnim 250ms 滚出滚回不跳变注释）、缩略图加载时序（sampleScrollGate 滞回 + 64ms 释放去抖 + 停稳放行）、键盘导航（patchTick 注释「按住方向键周期卡顿」正是上一次修复对象）。
4. **跨界边界**：B3 后端（thumbnail_commands.rs invalidate 语义）属提示词2线；共享资源协议（DB 单写/CPU permit/GPU 令牌）本线不涉及前端改动即可遵守。
5. **表征测试缺口**：fetchRowsByY/fetchBucketRows/viewportMeta 清理无直接单测；若 B2/B6 改动触及，先补表征测试（快照要求）。
6. **不可照抄快照的方向**：B1 若按快照「DOM 分支照抄 Canvas 浅 watch 改造」会打空——DOM 侧无深 watch；真实改造对象是 N3（watcher 扇出）、N4（整窗替换）、以及「极密强制 Canvas」的引擎判据评估。

## 8. 与姊妹线的关系

- 同日并行：`2026-09-06-缩略图派生流水线只读分析/`（提示词2线，B3 后端侧归它）、`2026-09-06-元数据扫描流水线优化/`（提示词1线，db:media_enriched 的生产侧归它——若该线改 emit 频率，本线 B2 的防抖参数需同步评估）。
- 本线全程只读，未动任何代码与协议。
