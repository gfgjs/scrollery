---
status: 施工中
type: 工作记忆
line: 画廊与大图预览闪烁分析
created: 2026-08-17
---

# 发现与决策:画廊与大图预览闪烁分析

## 需求
- 关闭大图、重新显示画廊时中间有很短闪烁：先确定问题并提出优化方案。
- 大图页关闭当前直接消失：评估并设计过渡动画。
- 3000×4000 图片表面已显示完成后仍闪一下；关闭 ICC 色域切换仍存在；同图立即二次进入不闪，但浏览其他图片后重进会再次闪烁。
- 主会话只分析问题和出解决方案；其他排查工作使用 Luna（Max）子代理。

## 发现
- 工作区存在未提交的 `AppShell.vue`、`MediaGrid.vue`、`MediaGridCanvas.vue`、`uiStore.ts` 及测试改动；本轮只读并保留。
- `App.vue` 的路由出口没有过渡，`KeepAlive` 只保活 `MediaGrid`；`ContentViewer.closeViewer()` 直接 `router.back()/push('/')`，所以大图根节点会在路由切换时直接卸载。
- 返回画廊时，`MediaGrid.onActivated()` 还要恢复滚动、可见行和可能的延期布局；Canvas 分支激活后通过 rAF 补绘。旧查看器已经消失而新画廊尚未完成真实绘制，形成可见交接窗口。
- 画廊与查看器分别使用 `gallerySidebarVisible` / `viewerSidebarVisible`；默认前者开、后者关。返回时侧栏 `margin-left` 过渡逐帧改变 flex 主区宽度，网格 `ResizeObserver` 会看到中间宽度并触发布局/画布调整。
- 当前未提交改动已加入 route-return 过渡信号、宽度冻结和 Canvas KeepAlive 防清屏；Canvas 的 `0×0 -> 1×1` backing store 清空路径已被针对性阻断。
- 当前宽度锁只给 `.media-grid` 写 `width`，但该元素及父容器仍为 `flex: 1`；`flex-basis: 0%` 下单独 `width` 可能不成为实际主轴约束，必须在 WebView2 逐帧验证，必要时锁 wrapper 或 `flex: 0 0 <width>`。
- 默认开启的 bucket 分段引擎有独立 `ResizeObserver`；KeepAlive 摘离后若读到 `clientHeight=0`，`syncDesired()` 会清空愿望段和已发布行。当前在途改动尚未给这条 RO 加失活闸门，因此返回首帧仍可能先空、再异步取段；需在失活期断观测并保留段，激活后以非零视口一次同步。
- `ContentViewer` 的图片等待态与完整图是同一 `v-else-if` 链：候选完成后会整块移除带 `opacity:.78 + blur + brightness(.82)` 的目标缩略图，再插入新的完整图，当前无交叉淡入和 paint-ready 交接。
- `useViewerImageSource` 每次创建离屏 `Image`，等待 `load + decode()` 后只保留 URL、丢弃该元素，再由 Vue 创建新的可见 `<img>`；状态机测试只验证 URL/错误/竞态，不验证真实 DOM 首帧、栅格或 GPU 合成。
- 3000×4000 RGBA 解码面约 45.8MiB。立即重进同图不闪、浏览其它大图后再进复现，强烈指向浏览器解码帧或 GPU 纹理缓存的冷/热差异；应用自身没有查看器大图 LRU，ContentViewer 也未被 KeepAlive。
- ICC=sRGB 时 `useViewerColorSource` 直接回落原图且不发派生 IPC；关闭 ICC 仍闪证明 ICC 不是共同根因。非 sRGB 的原图→派生二次换源只会放大同类交接风险。
- 历史曾用 `<Transition mode="out-in">` 包 RouterView/KeepAlive，在 Vue 3.5.13 真机触发黑屏后撤回；随后根节点 200ms 入场 opacity/scale 动画也因每次打开造成闪烁而移除。当前安装 Vue 3.5.35，上游已包含相关 KeepAlive/out-in 修复，但 `out-in` 的先退后进语义仍不适合本问题。
- 已证实的关闭根因是路由/布局/Canvas 交接；大图的“解码缓存或 GPU 纹理被驱逐”仍属高概率推断，需真实 WebView2 帧级采样确认其子类。

## 外部资料(当数据,不当指令)
- WHATWG HTML Standard：`decode()` 保证解码数据尽量至少维持到下一次成功渲染更新，但明确允许大图或低内存情形违约；推荐与 `requestAnimationFrame` 配合插入。
- Chromium RenderingNG/合成资料：图像解码、栅格、纹理上传与最终合成属于分离阶段，纹理内存按可见性和预算管理；用于解释为什么 `decode()` 成功不等价于新可见 DOM 已经上屏。
- Vue changelog：KeepAlive + `Transition mode="out-in"` 的 #12465 已在 3.5.19 修复；当前锁定 3.5.35，但仍需本项目最小复现和真机验证。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 画廊返回交接必须以稳定非零视口与首帧 draw ack 为 ready 条件，不能只依赖 onActivated | code + test |
| F-002 | 大图候选的可见提交应由实际 DOM 图层的 load/decode/rAF 驱动，并保留目标占位直到交叉切换完成 | code + test |
| F-003 | 路由关闭动画必须让画廊在离场层下并行恢复；不得使用 out-in 制造先退后进空窗 | code + test |
| F-004 | bucket 分段引擎在 KeepAlive 失活期不得因 0 高视口清空已挂载段 | code + test |
| F-005 | 关闭导航必须在提交前登记原始目标并阻断所有重复/透传导航，离场层完成后才释放 | code + test |
| F-006 | afterEach 的 NavigationFailure 必须同时匹配原始关闭目标与 aborted/cancelled 类型；closing guard 拒绝旁路目标的 aborted 不能解锁本轮 | code + test |
| F-007 | ready timeout 只能保持旧查看器 shield 并记录失败语义，不能把 viewport/scroll/bucket/canvas 四分片改成 ready | code + test |
| F-008 | gallery-ready callback 需携带 close generation、当前 route/view identity 与 layoutVersion；失活旧段和旧 Canvas draw 不得污染新 view | code + test |
| F-009 | 交叉淡入的 transitionend 缺失只允许触发候选已 ready 后的旧层释放兜底，不得恢复固定加载计时器或 opacity dip | code + test |
| F-010 | KeepAlive 失活/重新激活不能遗留 loading 段；必须撤销旧 fetch ownership、释放泵槽位并让当前代重取，旧响应不得覆盖新代 | code + test |
| F-011 | route hydration pending/failed 时所有 grid 命令入口共享 `galleryRouteBlocked` context gate；ContextualToolbar 隐藏，AppShell F11/undo/redo 只阻止旧命令及默认行为，Retry/Back/Tab/Esc 保持可用 | code + test |
| F-010 | Canvas route-return 锁不能只消费布尔值；连续锁代次必须断开旧 RO/取消 rAF，schedule/draw/迟到回调按当前代次拒绝，解锁仅最终几何补一次绘制 | code + test |
| F-011 | route hydration pending/failed 的安全表面包含外层 MediaScrollbar、时间轴/minimap 与 Teleport 辅助按钮；仅隐藏行会泄露旧 layout/pointer | code + test |
| F-012 | fallback shield 必须使用无 alpha 的全不透明黑；真实 ready 收尾时若焦点仍在 Retry/Back 或 body，应在当前 token/route 校验后经 DOM patch+rAF 恢复 `.media-grid[tabindex=0]` | code + test |
| F-013 | Canvas 非 bucket 以当前 identity draw ack + 新 rows 代次替代互斥 DOM layer 证明，空/旧 draw 不得打开 ready | code + test |
| F-014 | route-blocked gate 必须同时覆盖根级/Teleport 控件并取消失活 drag/menu 状态，不能只依赖 scoped CSS | code + test |
| F-015 | 目录滚动 pending 需以单调代次绑定请求所有权；stale finally 只清理 id+代次仍匹配的全局目标，同目录新请求不得被旧请求清除 | code + test |
| F-016 | pending 布局不得让 `totalRows` 同时切换 bucket/原生滚动条/轴侧栏等几何外壳；当前数据有效性与可视壳体必须分离，并以 ResizeObserver 集成回归守住 | code + test |
| F-017 | 普通 view/filter identity 换代应在同一渲染批次阻止旧行及旧 viewport metadata 表面；post-flush layout 请求不能留下可见一帧窗口 | code + test |

## 终审竞态修复裁决 (2026-08-17)
- AppShell `afterEach` 现在只接受与 `viewerCloseTarget` 相同的 `to.fullPath` 且 failure type 为 aborted(4)/cancelled(8)；closing guard 拒绝 query、侧栏或重复旁路目标时保持本轮 `viewerClosing`、timer 与焦点/overlay 屏障。
- App.vue/AppShell 移除了 `forceGalleryReady()` 超时旁路。ready 超时只写入 `galleryReadyTimeoutGeneration`，旧查看器离场层继续 inert/不透明 shield，待真实四分片到齐后再淡出。
- `galleryReadyToken`（close generation + target identity）与 MediaGrid 本地 session（route/view identity + layoutVersion）双重校验所有 viewport/scroll/bucket/canvas ack；布局换代/视图切换会先清空分片证据，KeepAlive 失活旧 rows 不能成为新 view 证据。
- session 的 view identity 不含 `/view/:id` 与 gallery 壳之间的路由路径变化，只由 viewStore 维度 + filter key 构成；否则同一画廊返回会被误判为新 view、在没有 compute 的情况下永久等待 shield。返回目标 fullPath 仍由 close token 独立校验。
- 空结果仅在 `layoutSummary` 已存在且 `isComputingLayout=false` 时成立；激活恢复先 `flushIfDeferred()`/等待 compute，再恢复 scrollTop、可视行和 ready 分片。Canvas 首绘在真实当前可见 normal row 完成实际 draw 后才发 ack。
- 图片交接 fallback 仅在候选已完成 load/decode/两帧 paint-ready、opacity crossfade 已开始后计时；transitionend 丢失只释放旧层，reduced-motion 继续走真实 ready 后零时长交接。

## 关闭过渡施工补充(2026-08-17)
- AppShell 注册全局 beforeEach：仅查看器→非查看器登记 closing 与原始 to.fullPath；查看器间 replace（翻页/换类型）不触发淡出。closing 期间任何非原始目标导航（包括底层画廊因 pointer-events:none 透传出的点击）均拒绝。
- App.vue 的 RouterView 使用 Vue 默认 simultaneous enter/leave（未启用先退后进模式）。JS leave hook 将旧查看器根节点设为 absolute、inert、pointer-events:none，画廊 KeepAlive 在其下方并行激活；仅 gallery-ready 后把 opacity 从 1 过渡到 0，时长 160ms。
- 正常离场以 transitionend(opacity) 收尾，300ms 兜底；gallery-ready 600ms 超时强制释放分片后继续，路由守卫另有 1s 最后 blocker 释放。prefers-reduced-motion 仍等待 ready，随后立即收尾，不依赖 transitionend。
- ContentViewer、DocumentViewer、AudioPlayer 的本地返回入口在 closing 状态下丢弃重复触发；焦点在旧查看器卸载后仅于焦点落回 body 时收回 .media-grid[tabindex=0]，已有侧栏/工具栏焦点不抢。

## 施工补充(2026-08-17)
- route-return 锁由 `MediaGrid` wrapper 承载，写入 `width`、`min-width`、`max-width` 与 `flex: 0 0 <width>`；MediaGrid 在两次帧级 `getBoundingClientRect().width` 一致且符合锁定目标后才标记 viewport ready。此逻辑有 `buildViewportLockStyle` characterization test，但未宣称 WebView2 GUI 已验。
- `useBucketVirtualScroll` 失活时断开 ResizeObserver、拒绝 0 高度同步、保留 `desired/segments`；激活只在非零视口强同步一次，回调以 generation 丢弃过期 RO。`hasDrawableSegment + readyVersion` 为返回屏障提供当前目标段证据。
- `MediaGridCanvas` 失活时断开 RO、取消 rAF、增加绘制代际；`fitCanvas` 在任一视口尺寸为 0 时直接返回，避免写入 1×1 backing store；激活复用原 backing store 并补排有效 draw。
- `uiStore.galleryReady` 由 `viewport/scroll/bucket/canvas` 四分片合取。AppShell 仅在 route-return 起点清零；MediaGrid 在真实滚动恢复、bucket 可绘段与 Canvas 首次有效 draw 后回填。`npx vitest run --reporter=dot` 通过 134 files/1542 tests；真实 3000×4000 WebView2 及 GUI 无闪仍需人工验收。

## 最终行为竞态修复(2026-08-17)
- AppShell 关闭导航维护同 generation 的目标 attempt FIFO；Vue Router 在第一同目标导航被第二次替代后，旧 `cancelled(type=8)` 只消费队列头，队列仍有替代 attempt 时不得清 `viewerClosing`。旁路目标被 guard 拒绝时不消费原始 attempt。
- `mediaStore.computeLayout` 以 request key、latest sequence、`pending|ready|failed|timeout` 状态和 success epoch 管理布局；新请求立即清旧 summary，旧/迟到 IPC 结果不得提交，watchdog 也只进入 timeout 失败态。
- Canvas draw ack 身份包含 close generation、view/filter identity、layoutVersion、success epoch、rows generation、session epoch；父层拒绝旧 identity 后，当前 rows/session token 变化会安排真实重绘并重发 ack。
- `restoreGalleryAfterActivation` 现等待当前 compute、强制 `updateVisible(true)`、Vue patch 与一帧 paint；DOM/bucket ready 还需查询当前 `[data-item-id]` 或 ready segment，Canvas 分支由实际 normal row draw ack 提供证据。
- ready timeout 采用持久黑幕 shield fallback：旧 viewer leave 完成且关闭导航闸门释放，但未完成四分片仍可接受当前 token；全部 ready 后清 shield。换到其他目标时清旧 token，避免永久卡住。

## activation/readiness barrier 最终修复(2026-08-18)
- P1-A：MediaGrid 以 activation generation + barrier phase 统一门控 readiness；激活立即撤四片证据，旧 restore 在 route hydration、layout success、scroll restore、rows update、nextTick/rAF 任一 await 后失效。`scheduleGalleryPaintReadiness`、viewport check、DOM/bucket/Canvas ack 在 barrier 开启前均拒绝。
- P1-B：App route watcher 以 viewStore hydration token/status 包住 collection/person 异步加载与 setActive；MediaGrid 等待同一路径完成水合并再等 post-flush compute，旧 summary/rows 无法释放 shield。
- P1-C：方案 A 与 bucket fetch 均绑定 layoutSuccessEpoch、view identity、activation generation；bucket 失活保留段在激活时退回 idle，readyVersion 高于激活基线后才可作为当前证据。
- P1-D/P2：AppShell afterEach failure 非空只消费 attempt 账本，不清 token/barrier/shield；fallback 仅成功且 current route 已换目标才清理。uiStore 记录 160ms fade generation，MediaGrid 在 fade 内 view/filter/layout 换代撤 ready 并重启 barrier，App 旧 viewer 立即恢复不透明 shield。

## P1 补强施工证据（2026-08-18）
- `restoreGalleryAfterActivation` 在 route hydration/layout 失败或超时时把当前 activation 置为 failed，保持四分片与 shield 未就绪；`media.layoutRequestStatus=ready` 或 route hydration retry 成功后，以同 activation token 重新走 hydration→layout→scroll→paint→open，旧 generation 不能重开屏障。
- bucket `hasDrawableSegment` 现在同时要求当前 fetch identity（layoutSuccessEpoch/viewIdentity/activationGeneration）成功落地；失活保留 rows 只作候选，宿主换代后 `refreshForActivation()` 强制当前身份重取。Canvas ack 复用该当前段证据，不再以 canvasMode 绕过 readyVersion 基线。
- route hydration 明确保存 `pending|ready|failed`、path、target identity 与错误；collection/person load reject、未知实体保持旧 view 不变但 MediaGrid 隐藏旧行并显示可恢复错误态，retry 成功前不报告 hydrated。
- gallery viewport ready 额外等待 AppShell 侧栏 `sidebarTransitioning=false`，解除 route-return 宽度锁后的下一帧几何稳定；160ms viewer fade 不再单独构成解锁条件。

## P1/P2 收口修复（2026-08-18）
- 最终 P1：非 bucket Canvas 采用当前 identity draw ack + 新 rows 代次作为真实行 proof；空/旧 draw 继续拒绝 ready，bucket 保持 readyVersion/current segment。
- 最终 P1：route-blocked gate 同时卸载根级与 Teleport 控件，并在阻塞/失活时清 ContextMenu、FolderTree 对话框、drag ghost 与 SelectionToolbar hostActive；AppToolbar 也在水合期间退场。
- fallback shield 现在是全屏不透明 `alertdialog`，明确显示 route hydration/layout 失败或“仍在准备”状态，提供 Retry 与返回动作；Retry 只递增当前 close generation，由 MediaGrid 在 route/layout/activation token 仍匹配时重启 barrier，未完成四分片不会被伪造为 ready。
- AppShell `afterEach` 真实消费 `isExpectedViewerCloseFailure` 与 attempt FIFO：同目标替代 attempt 或旁路 guard 拒绝不提前清理；原始目标 aborted/cancelled 且没有替代时调用 recover，撤销 closing/token/shield 并恢复 viewer leave 的 inert/aria-hidden/inline style，关闭入口可重试。
- 侧栏过渡信号包含 `kind + generation`；manual→route-return 即使 `transitioning` 布尔值不变也会重新锁当前 wrapper。Canvas 在 route-return 锁窗口断 RO/取消待绘 rAF，解除后只按最终几何重新测量和绘制。

## 最终 P2 收紧补充（2026-08-18）
- route-blocked 输入入口必须在 handler 内检查当前 gate；MediaGrid、Canvas click/pointerdown、selection 入口统一拒绝合成/键盘迟到事件，`useSelection.cancelPointerSession()` 清理 document pointer listener/session 但保留既有选区。
- route hydration pending/failed 只暂停布局/virtual fetch、bucket pump、Canvas draw/prefetch/decode 调度；保留 rows/segments/cache 作为候选，恢复时以当前 activation/fetch identity 重取并要求新 proof。
- bucket `dispose()` 在 unmount 代次显式失效所有 FetchOwnership；旧 promise resolve/reject 只走丢弃路径，不得改写段状态或占用槽位。
- fallback Back 先尝试导航，afterEach 成功才 finish；失败保持不透明 dialog、token 与 Retry/Back，FIFO attempt 仍按 generation 消费。

## 目录定位请求代次收口（2026-08-18）
- `pendingScrollDirId` 仍只由 `FoldersSection` 发起写入，但通过 uiStore setter 同步递增单调 `pendingScrollDirRequestVersion`；同一目录重复点击也产生可区分的请求身份。
- `useGalleryScrollToDir` 捕获 `dirId + requestVersion + identity/viewKey`，在路由/身份/失活/blocked 变化、异步回包失效、finally、延迟 timer 与卸载路径均调用 ownership 检查；旧请求不能清掉 B 的同目录新请求，自己的残留会被清掉。
- watcher 同时观察目录 id 与请求代次，因此 stale 后回到原画廊再次点同目录不会被相同数值短路；定向 characterization 覆盖 stale 保留新代、回到 A 重触发及正常成功清理。

## 冷启动无限重排回归审查（2026-08-18）
- `mediaStore.computeLayout()` 在工作树的新 pending/guard 重构中于请求开始清空 `layoutSummary`；HEAD 的同一路径只在成功后替换 summary。清空后 `media.totalRows` 变为 0。
- `useGalleryVirtualEngine` 将 `bucketActive` 定义为 `ui.bucketSegmentedScroll && media.totalRows > 0`；默认 bucket 开启。bucket 类隐藏原生滚动条，而普通 `.media-grid` 强制 `overflow-y: scroll`，Windows WebView2 会因此改变 `contentRect.width`。
- 轴控制簇也以 `media.totalRows > 0` 判断；pending 时 `timeline-sidebar-wrapper` 被卸载，主区宽度增加 44px（时间轴）或 96px（minimap），成功后重新挂载。这条链不依赖原生滚动条，overlay-scrollbar 平台也可能触发。
- `MediaGrid` 的 ResizeObserver 对上述宽度变化调用 `applyContainerWidth → onResize`；300ms 防抖后以新的宽度再次调用 `computeLayout`。结果是 `summary=null → 外壳扩张 → 重算 → summary ready → 外壳收缩 → 重算` 的自驱循环；回包慢于防抖时还会持续丢弃旧回包，白屏/骨架时间更长。
- `useViewportDimPriority` 的 resetKey 新含 `layoutVersion`/`layoutSuccessEpoch`；每次布局成功都会清尺寸请求去重并重排 200ms timer。若可视项仍为 0×0 且后端返回 measured>0，它可放大重算，但不是 P0 必现环。
- `useJustifiedLayout` 的 identity/currentness gate 与 `useGalleryTauriSync` 的 layoutVersion watcher 没有发现独立自激；扫描期 `totalItems` 触发重算属于次级放大器。
- 现有定向 Vitest（5 files/148 tests 与 4 files/23 tests）通过，但只覆盖状态/异步所有权；没有 `ResizeObserver + bucket/axis 几何 + pending summary` 的集成回归，因此未能阻止本次问题。

## 冷启动布局反馈修复（2026-08-18）
- `mediaStore` 新增仅含 `hasRows/axisMode` 的 `layoutShell`：pending 继续清空可消费 `layoutSummary`，但只在成功回包提交新的外壳形态，不保存行、项或坐标。
- 根 `.media-grid--bucket` 改由用户 bucket 偏好维持原生滚动条槽；引擎实际接管与行/段/自绘滚动条仍以当前 summary、shell 和 ready gate 判定，避免旧 rows 泄露。
- 时间轴/minimap wrapper 改由 shell 保留宽槽；pending 期内层组件均不挂载且 `aria-hidden`，用户在 pending 中主动切 timeline/minimap 时立即采用新意图，避免新宽→旧宽→新宽的反跳。
- `useViewportDimPriority` resetKey 改为 `galleryAsyncIdentity()`，不再因 layoutVersion/successEpoch 清空同一视图的尺寸去重集。
- 全量前端门禁通过；真实 Windows WebView2 尚未采样，不能把自动结果表述为 GUI 已验收。

## 非阻塞审查后续项（2026-08-18）
- 普通 view/filter identity 的主布局 watcher 使用 post flush；在该 watcher 发起新请求前，模板的 `totalRows` 条件可能短暂保留旧 `visibleRows`。同一 pending 窗口的 viewport metadata 调度也应进一步以 layout ready 约束。该项未构成冷启动 P0 几何环，留作 F-017 后续核证。
