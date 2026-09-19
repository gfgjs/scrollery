---
status: 施工中
type: 工作记忆
line: 画廊与大图预览闪烁分析
created: 2026-08-17
---

# 进度日志:画廊与大图预览闪烁分析

## 会话:2026-08-17
- 做了:读取项目文档治理与 planning 规则，确认工作区在途改动，建立分析三件套；由 5 个 Luna（Max）子代理分别排查关闭时序、大图管线、动画、在途 diff 和验证设计；主会话交叉核对代码、Git 历史和规范资料。
- 验证:本轮未修改产品代码、未跑产品测试；静态取证确认关闭直卸载、route-return 侧栏宽度变化、Canvas 失活清屏旧路径、图片 placeholder→full 分支替换及 hidden Image→visible img 二次绑定。真实 GPU/paint 子类尚未用 WebView2 trace 实测。
- 遗留:实施前先做一次短时帧级诊断，确认当前宽度锁是否真实生效、bucket RO 是否在失活时清段，并区分大图亮度/清晰度突变与可见层 raster/GPU 空帧；再按 P0→P3 顺序实施。

## 会话:2026-08-17（画廊返回链路施工）
- 做了:保留既有 AppShell/MediaGrid/MediaGridCanvas/uiStore 在途改动；将 route-return 宽度锁从滚动节点补到 wrapper，使用 `flex: 0 0` + `min/max-width`，并加入两帧 `getBoundingClientRect` 稳定检查。
- 做了:bucket ResizeObserver 加 KeepAlive 激活代与非零视口闸门，失活不清 `desired/segments`；Canvas 失活断 RO/rAF、阻断 0×0 尺寸写回、激活复用 backing store；uiStore 增加四分片 gallery-ready 信号并接入真实恢复/绘制回调。
- 记录:大图可见层（ContentViewer/useViewerImageSource 相关）由并行代理完成，本代理未修改其产品文件；其真实 GUI/Tauri 仍未验。
- 验证:`npx vitest run src/composables/useBucketVirtualScroll.spec.ts src/stores/uiStore.spec.ts src/components/media/mediaGrid.helpers.spec.ts --reporter=dot`（3 files/100 tests passed）；`npx vitest run --reporter=dot`（134 files/1542 tests passed）；`npx vue-tsc --noEmit`、定向 ESLint、`git diff --check` 通过。`npm test -- --reporter=dot` 在本机 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`，已用等价 `npx vitest run --reporter=dot` 完成验证。
- GUI:未宣称 WebView2/Tauri 真机通过；需要用真实 3000×4000 素材逐帧确认锁定宽度、关闭返回无闪与 Canvas/GPU 交接。
- 遗留:父会话需审查共享工作树 diff，并决定后续关闭动画是否消费 `ui.galleryReady`；不修改 App.vue/ContentViewer/大图可见层。

## 回顾(收口时填)
- 亮点:把三个用户现象拆成“关闭交接”和“图片可见提交”两条链，避免把 ICC 或动画当共同根因。
- 教训:状态机测试里的 decode/URL 原子提交不能替代真实可见 DOM 首帧验收；flex 宽度锁也必须验证 computed geometry。
- 意外:历史 Transition 黑屏的 Vue 缺陷已在当前依赖版本上游修复，但 out-in 语义本身仍不适合需要新旧视图重叠交接的场景。

## 会话:2026-08-17（关闭大图过渡施工）
- 做了:新增 viewerClosing/原始目标/代际状态与 beginViewerClose、finishViewerClose；早期 timeout 伪 ready 路径随后在终审中移除。AppShell 路由守卫覆盖 ESC/按钮、命令、浏览器 back、侧栏直达和深链返回，关闭中拒绝非原始目标导航。
- 做了:App.vue RouterView 使用默认 simultaneous enter/leave；旧查看器离场根节点 absolute + inert + pointer-events:none，gallery-ready 后 opacity 160ms 淡出；支持 reduced-motion、transitionend、300ms 淡出兜底、600ms ready 兜底及 1s 路由闸门兜底；未恢复任何进入动画。
- 做了:ContentViewer/DocumentViewer/AudioPlayer 本地返回入口增加重复关闭闸门；新增 viewerCloseTransition 纯函数契约测试与 uiStore 关闭状态测试。
- 验证:npx vitest run src/stores/uiStore.spec.ts src/utils/viewerCloseTransition.spec.ts --reporter=dot（2 files/17 tests passed）；npx vue-tsc --noEmit、定向 ESLint、git diff --check 通过。npm 直接参数转发沿用既有事实：npm test -- --reporter=dot 会被 npm 转发层报 EUNKNOWNCONFIG，本轮用等价 npx vitest run。
- GUI:未宣称真实 Tauri/WebView2；需人工逐帧验收大图关闭时画廊首绘、pointer/inert、reduced-motion 与真实 3000×4000 素材。
- 遗留:阶段 7 仍需完成定向受影响测试/全量 Vitest 与前端 build，再由主会话审查共享工作树合并结果；三件套保持 status: 施工中，不归档。
- 追加验证:全量 npx vitest run --reporter=dot 通过（135 files/1549 tests）；npm run build 通过，入口包 682.29 kB / 708 kB；构建过程仅有既存动态/静态 import 分块提示。
- npm 实测: npm test -- --reporter=dot 仍以 exit 1 报 EUNKNOWNCONFIG Unknown cli flag --reporter；等价 npx vitest run --reporter=dot 为实际通过路径。

## 会话:2026-08-17（Luna 终审竞态修复）
- 做了:修复 AppShell afterEach 竞态，仅原始关闭目标的 aborted/cancelled failure 可收尾；新增 query/侧栏/重复导航 characterization，并保留快速连发目标不变。
- 做了:彻底移除 App.vue/AppShell `forceGalleryReady()` 伪 ready；超时只记录 `galleryReadyTimeoutGeneration`、保留旧查看器 inert/不透明 shield。`galleryReadyToken` 绑定 close generation + target identity，MediaGrid 再校验当前 route/view identity 与 layoutVersion，迟到旧 bucket/Canvas/失活回调不能污染本轮。
- 纠偏:MediaGrid session identity 改为 viewStore 维度 + filter key，不把 `/view/:id` ↔ gallery 的路由壳变化当成新 view；目标 fullPath 继续由 close token 独立校验，避免同视图无 compute 时永久等待 shield。
- 做了:空布局必须有已确认 layoutSummary 且 compute 结束；激活先 flush deferred/current view layout，再恢复滚动和可视行。Canvas 首绘只在当前可见 normal row 完成真实 draw 后发 token ack；图片 opacity 交接补 220ms 仅释放旧层的 transitionend 缺失兜底，reduced-motion 保持真实候选 ready。
- 验证:定向 `npx vitest run ... --reporter=dot`（5 files/144 tests）通过；最终全量 `npx vitest run --reporter=dot`（135 files/1555 tests）通过；`npm run typecheck`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 `683.05 kB / 708 kB`，相对此前 682.29 kB 增加约 0.76 kB，仍在预算内。
- npm 实测:`npm test -- --reporter=dot` 仍被 npm 转发层以 exit 1 报 `EUNKNOWNCONFIG Unknown cli flag: --reporter`；Vitest 直跑为等价通过路径。
- 人工缺口:尚未在真实 Tauri/WebView2、Windows/macOS/iOS/Android、3000×4000 原图上逐帧验证关闭 shield、reduced-motion、Canvas/GPU paint 与 transitionend 缺失；自动化未伪造该结论。

## 会话:2026-08-17（最终行为竞态修复）
- 做了:以共享工作树为事实重新核对 AppShell/MediaGrid/Canvas/mediaStore；新增同目标关闭 attempt FIFO 所有权，并用 `createMemoryHistory` 回归第一 pending→第二同目标取消→第二成功及旁路 guard 拒绝。
- 做了:mediaStore latest-request 状态机（request key、success epoch、失败/timeout 清旧 summary、30s race watchdog）；MediaGrid 只接受当前 ready status，并提供失败态重试入口。
- 做了:Canvas ack 绑定 close/view/layout/success epoch/rows/session identity，旧 draw 被父层拒绝后由 session/rows token 变化触发真实重绘重 ack；新增 lifecycle state-machine spec。
- 做了:激活恢复改为等待强制 `updateVisible(true)`、`nextTick`/rAF 与当前 DOM/bucket paint；ready timeout 改持久 shield fallback，旧 viewer 可离场且导航可恢复，四分片真实到齐才撤 shield。
- 验证:定向 Vitest 79 项通过；全量 `npx vitest run --reporter=dot` 通过（137 files/1563 tests）；`npm run typecheck`、`npm run lint`、`npm run build` 通过（入口 685.57 kB / 708 kB）；`git diff --check` 通过。`npm test -- --reporter=dot` 仍为 npm 转发层 `EUNKNOWNCONFIG`，已记录并以 Vitest 直跑验证。
- 人工缺口:仍未宣称真实 Tauri/WebView2、3000×4000 GPU/paint、reduced-motion 与 transitionend 缺失场景自动通过；这些需真实 GUI 手工验收。

## 会话:2026-08-18（activation/readiness barrier 最终修复）
- 做了:App route watcher 增加 collection/person 路由水合 token/status；MediaGrid 激活建立单一 generation barrier，严格等待 route hydration、当前成功 layout、scroll restore、当前 rows DOM/bucket/canvas paint，再开放四片 ready。每个 await 后复核 route/view/layout/activation 身份，旧 restore、fetch、bucket、Canvas callback 不能落地。
- 做了:useVirtualScroll fetch 绑定 layoutSuccessEpoch + viewIdentity + activationGeneration；bucket fetch 同样绑定 provenance，失活/激活换代，保留段只能视觉复用且必须在新 readyVersion 后才作为本轮证据。AppShell afterEach 只在 failure===undefined 且当前路由真正换目标时清理 fallback；aborted/cancelled 保留 token/barrier/shield。160ms fade 窗口内 view/filter/layout 换代会撤 ready、恢复 shield 并重启 barrier。
- 做了:新增实际 lifecycle/race 回归：useVirtualScroll old/new provenance 交错、bucket 旧 view 同段交错、viewStore collection→person hydration token、createMemoryHistory fallback aborted/成功换目标、uiStore fade-window ready 撤回；新增/修改 `mediaGridCanvasReady.ts` 公共注释为中文。
- 验证:定向 Vitest（5 files/94 tests）通过；全量 `npx vitest run --reporter=dot` 通过（138 files/1569 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 `686.53 kB / 708 kB`，仅有既存动态/静态 import 分块提示。
- npm 实测:`npm test -- --reporter=dot` 仍由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag: --reporter`；等价 `npx vitest run --reporter=dot` 已实际通过。
- 人工缺口:未在真实 Tauri/WebView2、Windows/macOS/iOS/Android、3000×4000 原图、GPU/paint、reduced-motion、transitionend 缺失场景逐帧验收；不宣称自动化覆盖。

## 会话:2026-08-18（P1 liveness、bucket/Canvas 与路由水合补强）
- 做了:新增 `mediaGridActivationBarrier`，MediaGrid 激活恢复失败保持安全 shield/错误态；layout/route hydration 成功重试同一 activation token，阶段未按 hydration→layout→scroll→paint 完成时不开放 readiness。补充旧代 retry、失败→retry→success 与 shield 状态回归。
- 做了:bucket 记录当前 fetch identity，失活/激活保留 rows 但清除当前 ready 证据；宿主 `refreshForActivation` 在更新 activation generation 后强制当前段重取。MediaGrid `hasCurrentRowsValidation` 要求新 readyVersion/current segment，Canvas first-draw ack 同样消费该证据。
- 做了:viewStore route hydration 增加 `pending|ready|failed`、path/target identity、错误与 retry 版本；App 使用实体解析器区分 unknown/load reject，失败不 `setActive(null)`，MediaGrid route-blocked 错误态遮住旧/全库行并提供 retry。新增 collection/person 解析回归。
- 做了:viewport ready 受 `sidebarTransitioning` 门控，route-return 宽度锁解除后一帧再采样；`mediaGridCanvasReady.ts` 公共注释保持中文。
- 验证:`npx vitest run --reporter=dot` 通过（140 files/1577 tests）；定向 activation/bucket/view/route specs 通过（含失败→retry→success 与 retained rows）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 通过。build 入口 690.62 kB / 708 kB；npm 参数转发的 `npm test -- --reporter=dot` 仍需记录 EUNKNOWNCONFIG，Vitest 直跑为等价路径。
- 人工缺口:未在真实 Tauri/WebView2、Windows/macOS/iOS/Android、3000×4000 原图、GPU/paint、侧栏 transitionend 缺失与 reduced-motion 逐帧验收；不宣称 GUI 自动通过。

## 会话:2026-08-18（P1/P2 收口修复）
- 做了:fallback shield 改为可访问、不透明、可操作的恢复面板；Retry 通过当前 close generation 触发 MediaGrid barrier 的 token 校验重试，返回动作清理本轮交接并恢复历史导航。
- 做了:AppShell afterEach 接线 `isExpectedViewerCloseFailure` + FIFO replacement ownership；无替代原始 aborted/cancelled 进入 recover，App.vue 用 leave snapshot 恢复 inert/aria-hidden/inline style；补 router integration regression。
- 做了:侧栏 generation 与 Canvas resizeLocked 门控，manual→route-return 连续状态重新锁当前 wrapper，补几何 helper/状态回归。
- 验证:定向 Vitest（5 files/82 tests）通过；全量 `npx vitest run --reporter=dot` 通过（140 files/1582 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 694.79 kB / 708 kB；仅有既存动态/静态 import 分块提示。
- npm 实测:`npm test -- --reporter=dot` 由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`；以等价 `npx vitest run --reporter=dot` 完成实际测试。
- GUI:未在真实 Tauri/WebView2、Windows/macOS/iOS/Android、3000×4000 原图、GPU/paint、reduced-motion、transitionend 缺失与 manual→route-return 逐帧场景验收；自动化未伪造该结论。
- 遗留:阶段 10 源码与自动门禁完成，侧栏阶段 3/主线人工 GUI 验收仍待进行；三件套保持 status: 施工中，不归档。

## 会话:2026-08-18（Luna 最终残余修复）
- 做了:Canvas 新增 `resizeLockGeneration`，以锁布尔值+代次断开 ResizeObserver、取消待绘 rAF，并让迟到回调按绘制代次失效；锁窗期间 `scheduleDraw`/`draw` 均直接拒绝，解锁只重新 measure、重连 RO、补一次 rAF。新增 resize-lock lifecycle 回归覆盖锁窗拒绘、连续 lock generation 与解锁单次恢复。
- 做了:route hydration pending/failed 在 `.media-grid-layout--route-blocked` 根级契约上隐藏滚动条、时间轴/minimap、浮动按钮与 Teleport 辅助控制；`.media-grid__route-state` 保留错误/Retry 可操作，网格 tabindex 在 blocked 时降为 -1。
- 做了:fallback shield 背景改为 `rgb(0 0 0)` 完全不透明；真实四片 ready 后 finish 若焦点仍在 Retry/Back/body，则校验当前 close token/route，经 nextTick+rAF 恢复当前可交互 `.media-grid[tabindex="0"]`，不抢外部正常焦点。新增静态渲染契约回归。
- 验证:定向 `npx vitest run src/components/media/mediaGridCanvasReady.spec.ts src/components/media/gallerySafety.contract.spec.ts src/components/media/mediaGrid.helpers.spec.ts src/stores/uiStore.spec.ts src/utils/viewerCloseTransition.spec.ts --reporter=dot` 通过（5 files/83 tests）；`npx vue-tsc --noEmit` 通过；受影响 ESLint 无错误（CSS 文件按配置忽略）。完整门禁待本会话收尾运行。
- GUI:真实 Tauri/WebView2、3000×4000 GPU/paint、manual→route-return 逐帧 geometry、pointer/inert、reduced-motion 与 transitionend 缺失仍未自动覆盖，继续保持阶段 11 待人工验收。

## 会话:2026-08-18（最终门禁收尾）
- 验证:完整 `npx vitest run --reporter=dot` 通过（141 files/1587 tests）；`npx vue-tsc --noEmit` 通过；`npm run lint` 通过；`npm run build` 通过，入口 `695.44 kB / 708 kB`，仅报告既存动态/静态 import 分块提示；`git diff --check` 通过。
- 验证:`npm test -- --reporter=dot` 仍被 npm 参数转发层拒绝并报 `EUNKNOWNCONFIG Unknown cli flag --reporter`，以等价 `npx vitest run --reporter=dot` 完成实际测试。
- 遗留:源码与自动门禁完成；真实 Tauri/WebView2 GUI（manual→route-return 几何、Canvas backing store、route-blocked pointer、fallback Retry/Back 焦点、reduced-motion/transitionend 缺失）仍待人工验收，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终 P1 修复）
- 做了:非 bucket Canvas 不再查询互斥的 `layerRef` DOM；记录当前 close/view/layout/success/rows/session identity 的真实 draw，并要求 rowsGeneration 严格高于本次 activation baseline。旧 identity、空 rows、missing/empty 伪证与 retry 前旧 draw 均不能撤 shield；bucket 仍走 readyVersion/current segment。
- 做了:建立 `galleryRouteBlocked`/`galleryControlsBlocked` gate，卸载根级 gallery-mode-btn、ContextMenu、SelectionToolbar、FolderTreeSelectorDialog 与 body drag ghost；阻塞/失活 watcher 和生命周期主动清 menu/dialog、cancelMediaDrag、撤 hostActive，键盘/滚轮/指针入口拒绝旧画廊交互；AppToolbar 水合阻塞时同步隐藏。
- 做了:扩展 Canvas proof 状态机回归、route-blocked/Teleport/root/Retry 静态渲染合同回归，并让 beginPointerDrag 支持主动取消以清理迟到 pointer 会话。
- 验证:定向 Vitest 4 files/69 tests 通过；gallery safety 3 tests 通过；`npx vue-tsc --noEmit`、受影响 ESLint 通过。全量 Vitest/lint/build/diff-check 待本会话收尾运行。
- GUI:真实 Tauri/WebView2 的 Canvas frame/pointer、route-blocked Teleport、fallback Retry/Back 焦点与 reduced-motion/transitionend 仍未自动覆盖，阶段 11 继续 pending。

## 会话:2026-08-18（最终门禁复跑）
- 验证:完整 `npx vitest run --reporter=dot` 通过（141 files/1588 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 `695.62 kB / 708 kB`，仅有既存动态/静态 import 分块提示。
- 记录:`npm test -- --reporter=dot` 仍由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`；以 `npx vitest run --reporter=dot` 完成等价测试。
- GUI:真实 Tauri/WebView2 route-blocked Teleport/pointer、Canvas frame/backing-store、fallback Retry/Back 焦点、reduced-motion/transitionend 仍待人工验收，三件套保持 `status: 施工中`，阶段 11 pending。

## 会话:2026-08-18（Luna 最终 bucket/命令 gate 修复）
- 做了:将 bucket active fetch 从段对象升级为独立 ownership token；失活/激活刷新使遗留 loading 失效并退回 idle，释放槽位，旧 resolve/reject/finally 不得覆盖新代，当前 desired 段可立即重取。
- 做了:新增 `galleryRouteBlocked` 路由快照判定与 `CommandContext.view='blocked'`；registry、ContextualToolbar、AppShell F11/undo/redo 与媒体上下文命令共用门禁，ready 后恢复 grid，fallback Retry/Back/Tab/Esc 与外部安全键不受阻塞。
- 验证:`npx vitest run src/composables/useBucketVirtualScroll.spec.ts src/commands/context.spec.ts src/commands/keybinding.spec.ts src/utils/routeHydration.spec.ts src/components/media/gallerySafety.contract.spec.ts --reporter=dot` → 5 files/67 tests passed；完整 `npx vitest run --reporter=dot` → 142 files/1596 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过，build 入口 696.61 kB / 708 kB。
- 记录:`npm test -- --reporter=dot` 由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`，Vitest 直跑为等价通过路径。
- GUI:真实 Tauri/WebView2 的 bucket 失活/激活帧级恢复、route-blocked F11/Retry/Back 焦点与侧栏/Canvas 几何仍未自动覆盖，阶段 11 继续 pending。

## 会话:2026-08-18（Luna 最终 P2 收紧修复）
- 做了:MediaGrid/Canvas 所有 click/pointerdown/select 入口增加当前 `galleryControlsBlocked`/interaction gate；`useSelection` 增加显式 `cancelPointerSession`，route-blocked、切 view barrier、失活、卸载均清理 document pointer listener/session，不清正常选区。
- 做了:route hydration pending/failed 通过响应式 `galleryBackgroundActive` 暂停 layout/方案 A virtual fetch、bucket pump/RO、Canvas draw/rAF/prefetch 与可取消 thumbnail IO；保留 visibleRows/segments/cache，恢复时以当前 identity/activation 强制重取。bucket `dispose` 在 unmount 显式失效 ownership 并释放槽位。
- 做了:fallback Back 不再先 `finishViewerClose`；uiStore 保留 fallback shield 跨导航，AppShell afterEach 仅在目标成功且无需 gallery-ready 或真实 ready 后收尾；失败回到安全 dialog，补失败→成功集成回归。
- 验证:定向回归（selection、bucket、virtual、gallery safety、router、activation/ui）通过；完整 `npx vitest run --reporter=dot` 通过（143 files/1603 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过；构建入口 `696.94 kB / 708 kB`，仅既存动态/静态 import 分块提示。
- 记录:`npm test -- --reporter=dot` 仍由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`，以等价 Vitest 直跑完成测试。
- GUI:真实 Tauri/WebView2 的 route-blocked synthetic/keyboard pointer、Canvas decode/paint、bucket unmount、fallback Retry/Back 焦点与 reduced-motion/transitionend 仍待人工验收；三件套保持 `status: 施工中`，阶段 11 pending。

## 会话:2026-08-18（Luna 最终交互/调度状态门）
- 做了:中央 `galleryInteractionBlocked = galleryRouteBlocked || viewerCloseFallbackActive` 贯穿 CommandContext/registry、ContextualToolbar、AppShell 全局键盘与 App 工具栏；fallback alertdialog 仍保留 Retry/Back/Tab/Esc 与安全窗口键。
- 做了:Justified 布局 direct compute、ResizeObserver 防抖、Tauri enriched/volumes/totalItems/layoutDirty、方案 B layoutVersion watcher 均在 route blocked 期暂停并保留旧 layout/rows/segments；恢复按当前 identity 一次重算/重取，旧代回调拒绝。
- 做了:Canvas hover/preparation、cell action emit、MediaThumb late event 复核 interaction gate；fallback Back 成功落到另一 gallery 时重绑 token/barrier，非 gallery 安全路由仍直接收尾；back-bar/empty/layout retry/scroll/axis/folder stats 等入口统一 gate。
- 验证:`npx vitest run --reporter=dot` 通过（143 files/1608 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 通过；build 入口 697.63 kB / 708 kB。
- 记录:`npm test -- --reporter=dot` 仍会触发 npm `EUNKNOWNCONFIG Unknown cli flag --reporter`，Vitest 直跑为等价验证路径。
- GUI:真实 Tauri/WebView2 fallback 焦点/路由序列、blocked→ready 帧级调度、Canvas synthetic event 与侧栏几何仍未自动覆盖；阶段 11 继续 pending。

## 会话:2026-08-18（Luna 最终异步 currentness 收口）
- 做了:补齐 Tauri/目录事件 blocked→ready 的 deferred dirty/recompute identity replay；尺寸优先与布局 IPC 加 scheduling、activation/view/layout currentness；键盘评分、ContextMenu、drag/drop 的 late action 在执行及 await 后复核中央 interaction gate，fallback gate 变化即时取消 pointer/lasso/menu/dialog/drag/hover。
- 验证:`npx vitest run --reporter=dot` → 145 files/1614 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 698.19 kB / 708 kB。
- 记录:vue-tsc/Vitest 的 npm wrapper 实际显示既存 `.npmrc` 警告 `ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`；build 仅既存 dynamic/static import 提示。
- GUI:真实 Tauri/WebView2、GPU/paint、reduced-motion、transitionend 缺失与 route-blocked synthetic action 仍待人工验收；阶段 11 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 普通 view identity P2 收口）
- 做了:普通 smart-album/目录/route identity 换代统一清理 lasso document listeners、ContextMenu、folder dialog、media drag；Canvas 接收 interaction identity，清 hover/prep、旧预取与 pointer resolver，持久选区不在此处强制清空。
- 做了:补 Tauri deferred stale-target guard、viewport priority in-flight reschedule + generation 去重、fallback Back 新 token/fullPath 直接启动 activation barrier；SelectionOps 所有 await 后副作用增加 interaction/view/layout currentness gate，compute/relocate 自推进 layout 可更新代次。
- 验证:定向 Tauri/viewport/selection/gallery safety/router/ui 回归通过；本轮完整门禁待主会话收尾复跑。GUI/Tauri/WebView2 逐帧场景仍未自动覆盖，阶段 11 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终 scroll/anchor/pipeline currentness 收口施工）
- 做了:普通画廊目录定位、重排锚点、Tauri layoutVersion 回滚、JustifiedLayout compute、键盘评分与 Canvas 缩略图解码统一补齐 view/route/activation identity 复核；MediaGrid 滚动缓存 key 现包含 collection/person/filter，避免 `album-all` 撞 key。
- 做了:Canvas pipeline 引入 epoch/ownership；pause/setIdentity 会令不可取消 decode/Image 回调失效，旧同 id/sig 只释放资源、不 commit 缓存或 scheduleDraw。
- 测试:`npx vitest run src/composables/useGalleryScrollToDir.spec.ts src/composables/useReflowAnchor.spec.ts src/composables/useGalleryKeyboard.spec.ts src/composables/useJustifiedLayout.spec.ts src/composables/useCanvasThumbPipeline.spec.ts src/composables/useGalleryTauriSync.spec.ts --reporter=dot` → 6 files/14 tests passed；新增 A→B late、B normal、running async epoch characterization。
- 验证:`npx vue-tsc --noEmit`、受影响 ESLint 已通过；全量门禁与 GUI 仍待本会话收尾，真实 Tauri/WebView2 仍为 GUI Pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终 late-event 门禁修复）
- 做了:ContentViewer 编辑授权 await 捕获 item id、route、viewer generation 与 request token；A→B/返回 A 的迟到 entitlement 不能打开当前 B，B 新请求正常放行 editor，补 identity 状态机回归。
- 做了:MediaGrid scroll settle 回调绑定当前 gallery identity/activation 与捕获的 scroll 位置；切 view、route、activation barrier、失活/卸载立即清理旧 timer，AppShell fallback 额外要求真实 gallery route 与 MediaGrid active signal。
- 验证:定向 3 files/68 tests；完整 `npx vitest run --reporter=dot` 通过（151 files/1635 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过，build 入口 `698.33 kB / 708 kB`。
- 记录:Vitest/vue-tsc 的 npx wrapper 报既存 `.npmrc` `ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`；build 仅既存 dynamic/static import 分块提示。
- GUI:真实 Tauri/WebView2 的 ContentViewer 授权竞态、route transition keyboard、scroll late-event、route-return 几何/backing-store 与 reduced-motion/transitionend 缺失仍待人工验收，阶段 11 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（目录定位 pending 代次收口）
- 做了:uiStore 增加单调 `pendingScrollDirRequestVersion` 与 `setPendingScrollDir`/`clearPendingScrollDir` ownership API；FoldersSection 不再直接写 ref，仍由其作为唯一发起方登记目录目标。
- 做了:useGalleryScrollToDir 捕获目录 id、请求代次、view/route identity；pending watcher 同时观察 id+代次，同目录重复点击可触发；身份/路由/失活/blocked、迟到 IPC/滚动回包、finally、延迟 timer 与卸载路径只清仍匹配的 ownership，避免 A stale 清 B 新请求并清掉 A 自身残留。
- 测试:`npx vitest run src/composables/useGalleryScrollToDir.spec.ts src/stores/uiStore.spec.ts --reporter=dot` → 2 files/24 tests passed；新增 A→B 同目录 stale、stale 后回 A 同目录重触发、正常成功清理与代次 API characterization。
- 验证:完整 `npx vitest run --reporter=dot` → 151 files/1638 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 `698.52 kB / 708 kB`，仅既存 dynamic/static import 分块提示。
- 记录:npx Vitest/vue-tsc 输出既存 npm `.npmrc` warning：`ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`。
- GUI:真实 Tauri/WebView2 的目录点击在 A→B/失活/回到 A 时序、滚动定位与 route-return 几何仍未自动覆盖；阶段 11 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（冷启动无限重排只读审查）
- 做了:按用户报告审查昨天至今的工作树；主会话只做分析与任务编排，四条只读 Luna（Max）审查交叉复核冷启动画廊闪烁。
- 结论:P0 为 `computeLayout` 新增的请求起点清 summary，令 `totalRows=0`；bucket 原生滚动条与 timeline/minimap flex 侧栏随之切换主区宽度，ResizeObserver 把该内部形态变化重新送入 `onResize → compute`，每约 300ms 循环一次。
- 证据:相对 HEAD，旧 store pending 期保留 summary；当前 `mediaStore.ts`、`useGalleryVirtualEngine.ts`、`useGalleryAxisControls.ts`、`MediaGrid.vue`/styles 构成完整闭环。P1 为尺寸优先 resetKey 的 success-epoch 自驱风险；P2 为扫描统计重算放大器。
- 验证:代理定向 Vitest 5 files/148 tests、4 files/23 tests 通过；它们不含 DOM ResizeObserver/scrollbar/axis 几何集成，不能证明 GUI 无回归。未修改产品代码、未暂存、未提交。
- 遗留:待用户授权后实施“当前数据状态与稳定外壳几何分离”或等效 RO 形态锁，并补冷启动/空视图/筛选切换/侧栏与轴切换回归；最终在 Windows WebView2 采样确认 5 秒内宽度与 compute 请求收敛。

## 会话:2026-08-18（冷启动布局反馈修复）
- 做了:用户授权后由 Luna（Max）实施 `layoutShell`；pending 保留 bucket/滚动条与轴槽的几何，当前 layout summary、行、交互与 ready 语义继续严格清空/门控。用户在 pending 中切换轴模式时保留新意图，不回弹旧宽度。
- 做了:尺寸优先 resetKey 收窄为 `galleryAsyncIdentity()`，移除 layoutVersion/layoutSuccessEpoch 自驱重排。
- 审查:独立 reviewer 确认 P0 geometry 环已切断；空/非空、非空→空、bucket on/off、route-blocked、轴形态与真实窗口 resize 均无 P0 blocker。另登记普通 view/filter 换代的一帧陈旧表面风险为后续 F-017，未混入本批。
- 验证:定向 8 files/165 tests 通过；全量 `npx vitest run --reporter=dot` 通过（151 files/1645 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 仅有既有 dynamic-import warnings，预算通过。
- GUI:尚未在真实 Windows WebView2 执行 5 秒 compute/ResizeObserver 宽度采样；阶段 12 保持 GUI pending。未暂存、未提交。
