---
status: 施工中
type: 工作记忆
line: 侧栏动画与画廊重排修复
created: 2026-08-17
---

# 进度日志:侧栏动画与画廊重排修复

## 会话:2026-08-17
- 做了:定位 AppShell 侧栏 `margin-left` 动画与 MediaGrid ResizeObserver 的交互；撤回了“路由切换禁用动画”的临时方案；获用户确认后开始实施“动画期间冻结、结束后单次同步”。
- 新发现:上一版只冻结布局源的宽度输入，未冻结画廊 DOM 视口；KeepAlive 激活首帧及 Canvas/bucket 内部 ResizeObserver 仍可能造成短闪。下一版需锁住路由返回期间的实际视口宽度。
- 做了:为侧栏过渡增加 `manual`/`route-return` 类型；路由返回时锁定 `.media-grid` 外框宽度，侧栏动画结束后解除并同步最终宽度；保留 transitioncancel 与定时器兜底。
- 新发现:Canvas 未被 `dispose()` 清理，但失活期间 ResizeObserver/rAF 可能把 0×0 视口转成 1×1 backing store，隐式清空画布；已改为失活停观测/停绘、`fitCanvas` 拒绝 0×0、激活复用画布并补绘。
- 验证:`npx vue-tsc --noEmit` 通过；AppShell、MediaGrid、MediaGridCanvas、uiStore 定向 ESLint 通过；`git diff --check` 通过；`npx vitest run src/stores/uiStore.spec.ts` 9 tests passed。
- 遗留:需要真实 GUI 场景确认返回画廊无短闪；本轮按用户要求未跑完整测试门禁。

## 会话:2026-08-18（代次门控补强）
- 做了:侧栏运行时信号增加 generation；MediaGrid 监听 transitioning/kind/generation 三元组，覆盖 manual→route-return 在布尔值不变时的换代；每次 route-return 从当前 wrapper 重新取锁宽度。
- 做了:MediaGridCanvas 增加 resizeLocked，锁窗口断开 ResizeObserver、取消待绘 rAF，解锁后按最终 wrapper 几何重新测量并补一次绘制。
- 验证:侧栏/MediaGrid/Canvas 受影响 TypeScript 与 ESLint 通过；新增 helper 与 uiStore 代次回归通过。
- 遗留:真实 Tauri/WebView2 逐帧几何与无闪场景仍待人工 GUI，阶段 3 保持 pending。

## 会话:2026-08-18（最终锁与阻塞表面收口）
- 做了:Canvas 接收 `resizeLockGeneration` 并监听 `[resizeLocked, generation]`；每次 route-return 锁开始/换代/解锁都会让旧 rAF/RO 失效，锁窗中 `scheduleDraw`/`draw` 拒绝，解锁重新 measure、重连 RO、只补一次 draw。
- 做了:MediaGrid 根级 `media-grid-layout--route-blocked` 隐藏 MediaScrollbar、时间轴/minimap、浮动与 Teleport 辅助交互，blocked 网格 tabindex=-1；route-state 错误/Retry 保持可见可操作。
- 验证:定向 5 files/83 tests 通过；`npx vue-tsc --noEmit` 通过；受影响 ESLint 无错误；完整 Vitest/lint/build/diff-check 待本会话收尾运行。
- GUI:仍需真实 Tauri/WebView2 慢动作确认 manual→route-return 几何锁、Canvas backing store、transitioncancel/transitionend 缺失与 reduced-motion；阶段 3 继续 pending。

## 会话:2026-08-18（最终门禁收尾）
- 验证:完整 `npx vitest run --reporter=dot` 通过（141 files/1587 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过；构建入口 `695.44 kB / 708 kB`。
- 记录:`npm test -- --reporter=dot` 因 npm 转发层 `EUNKNOWNCONFIG Unknown cli flag --reporter` 失败，Vitest 直跑为等价通过路径。
- GUI:自动门禁未覆盖真实 Tauri/WebView2 逐帧宽度/backing-store/pointer/reduced-motion/transitionend；阶段 3 仍 pending，等待人工验收。

## 会话:2026-08-18（Luna 最终 P1 route surface 补强）
- 做了:route hydration pending/failed 与 KeepAlive 失活统一进入 gallery controls gate，根级 gallery-mode-btn、ContextMenu、SelectionToolbar、FolderTreeSelectorDialog、media-drag-ghost 均卸载；阻塞/失活时清 context/dialog、主动取消 pointer drag 并撤销 docked hostActive，避免状态栏/body Teleport 残留。
- 做了:AppToolbar 在 collection/person 水合未完成时退场，网格键盘/滚轮/指针入口同步拒绝；route-state 错误与 Retry 保留为唯一恢复面。
- 验证:gallery safety contract 与 Canvas proof 定向回归通过；`npx vue-tsc --noEmit`、受影响 ESLint 通过。完整 Vitest/lint/build/diff-check 待本会话收尾。
- GUI:真实 Tauri/WebView2 route-blocked pointer、Teleport、manual→route-return 几何/backing-store 及 reduced-motion 仍待人工验收，阶段 3 保持 pending。

## 会话:2026-08-18（最终门禁复跑）
- 验证:完整 `npx vitest run --reporter=dot` 通过（141 files/1588 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 `695.62 kB / 708 kB`。
- 记录:`npm test -- --reporter=dot` 报 npm `EUNKNOWNCONFIG Unknown cli flag --reporter`；Vitest 直跑为等价通过路径。
- GUI:真实 Tauri/WebView2 route-blocked Teleport/pointer 与 manual→route-return geometry/backing-store/reduced-motion 仍待人工验收，三件套保持 `status: 施工中`，阶段 3 pending。

## 会话:2026-08-18（Luna 最终 bucket/命令 gate 修复）
- 做了:将 bucket active fetch 从段对象升级为独立 ownership token；失活/激活刷新使遗留 loading 失效并退回 idle，释放槽位，旧 resolve/reject/finally 不得覆盖新代，当前 desired 段可立即重取。
- 做了:新增 `galleryRouteBlocked` 路由快照判定与 `CommandContext.view='blocked'`；registry、ContextualToolbar、AppShell F11/undo/redo 与媒体上下文命令共用门禁，ready 后恢复 grid，fallback Retry/Back/Tab/Esc 与外部安全键不受阻塞。
- 验证:`npx vitest run src/composables/useBucketVirtualScroll.spec.ts src/commands/context.spec.ts src/commands/keybinding.spec.ts src/utils/routeHydration.spec.ts src/components/media/gallerySafety.contract.spec.ts --reporter=dot` → 5 files/67 tests passed；完整 `npx vitest run --reporter=dot` → 142 files/1596 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过，build 入口 696.61 kB / 708 kB。
- 记录:`npm test -- --reporter=dot` 由 npm 转发层报 `EUNKNOWNCONFIG Unknown cli flag --reporter`，Vitest 直跑为等价通过路径。
- GUI:真实 Tauri/WebView2 的 bucket 失活/激活帧级恢复、route-blocked F11/Retry/Back 焦点与侧栏/Canvas 几何仍未自动覆盖，阶段 3 继续 pending。

## 会话:2026-08-18（Luna 最终 P2 收紧修复）
- 做了:route-blocked gate 从输入入口延伸到后台调度；方案 A virtual、bucket fetch/RO/pump、Canvas draw/rAF/prefetch 均暂停而保留 rows/segments/cache，ready/retry 后按当前 activation identity 重取。
- 做了:显式 `useSelection.cancelPointerSession()` 清 document pointermove/up/cancel listener 与 lasso session；MediaGrid 在 route-blocked、切 view barrier、onDeactivated/onBeforeUnmount 调用；Canvas click/pointerdown 与 MediaGrid select/open route 均先过 gate。
- 做了:bucket `dispose()` 在 unmount 递增 generation、invalidate FetchOwnership、释放槽位并丢弃旧 resolve/reject；fallback Back 改为导航后由 afterEach/真实 ready 收尾，失败保持安全 dialog，补失败→成功回归。
- 验证:完整 `npx vitest run --reporter=dot` 通过（143 files/1603 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过；build 入口 `696.94 kB / 708 kB`。
- 记录:`npm test -- --reporter=dot` 报 npm `EUNKNOWNCONFIG Unknown cli flag --reporter`；以 `npx vitest run --reporter=dot` 等价路径通过。
- GUI:真实 Tauri/WebView2 逐帧 manual→route-return geometry/backing-store、route-blocked pointer/decode、bucket unmount 与 fallback 焦点仍待人工验收；三件套保持 `status: 施工中`，阶段 3 pending。

## 回顾(收口时填)
- 亮点:待填。
- 教训:路由宽度问题不能用取消动画解决，应把布局重算与视觉动画解耦。
- 意外:KeepAlive 激活时机与侧栏 flex 宽度动画叠加，是闪帧的直接触发条件。

## 会话:2026-08-18（Luna 最终异步 currentness 收口）
- 做了:补齐 Tauri/目录事件在 blocked/失活期间的 deferred recompute intent（按 view/route/activation identity），ready 仅当前身份单次重放；MediaGrid folder-stats-changed 接入同一队列。尺寸优先 timer、在途 PRIORITIZE_DIMENSIONS 回包与布局 IPC success/catch/watchdog 均增加 scheduling/currentness 复核；键盘评分、菜单命令闭包、拖放落点 await 后拒绝旧 view。
- 验证:`npx vitest run --reporter=dot` → 145 files/1614 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过。build 入口 698.19 kB / 708 kB。
- 记录:vue-tsc/Vitest 的 npm wrapper 实际显示既存 `.npmrc` 警告 `ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`；build 仅既存 dynamic/static import 提示。
- GUI:真实 Tauri/WebView2 逐帧 route-blocked→ready、fallback pointer/hover、侧栏几何与 Canvas backing-store 仍待人工验收；阶段 3 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终状态门复核）
- 做了:补齐 fallback shield 与 route hydration 的中央交互门，命令/键盘/工具栏/Canvas/画廊辅助入口均不再把合成事件送入旧 gallery；Retry/Back 恢复入口保持可用。
- 做了:route blocked 调度暂停覆盖 Justified direct compute、bucket layoutVersion/ResizeObserver、Tauri enriched/volumes/dirty 及 Canvas hover/decode；旧段保留，ready 后只按当前身份恢复。
- 验证:`npx vitest run --reporter=dot` 143 files/1608 tests、`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 均通过；GUI 阶段仍待真实 Tauri/WebView2 人工验收，阶段 3 保持 pending。

## 会话:2026-08-18（Luna 普通 view identity P2 收口）
- 做了:普通 smart-album/目录/route identity 换代清理 lasso、菜单、目录对话框与媒体拖拽；Canvas 清 hover/prep、旧预取与 pointer resolver。fallback 新 token/fullPath 即使布局相同也会启动新 activation barrier。
- 做了:SelectionOps await 后复核 interaction/view/layout currentness；Tauri deferred intent 与 viewport priority 补 stale guard/尾随重排。侧栏 route-return 几何锁契约未改变。
- 验证:定向 Tauri/viewport/selection/gallery safety/router/ui 回归通过；完整门禁待主会话收尾复跑。真实 Tauri/WebView2 GUI 仍待人工验收，阶段 3 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终 scroll/anchor/pipeline currentness 收口施工）
- 做了:普通画廊目录定位、重排锚点、Tauri layoutVersion 回滚、JustifiedLayout compute、键盘评分与 Canvas 缩略图解码统一补齐 view/route/activation identity 复核；MediaGrid 滚动缓存 key 现包含 collection/person/filter，避免 `album-all` 撞 key。
- 做了:Canvas pipeline 引入 epoch/ownership；pause/setIdentity 会令不可取消 decode/Image 回调失效，旧同 id/sig 只释放资源、不 commit 缓存或 scheduleDraw。
- 测试:`npx vitest run src/composables/useGalleryScrollToDir.spec.ts src/composables/useReflowAnchor.spec.ts src/composables/useGalleryKeyboard.spec.ts src/composables/useJustifiedLayout.spec.ts src/composables/useCanvasThumbPipeline.spec.ts src/composables/useGalleryTauriSync.spec.ts --reporter=dot` → 6 files/14 tests passed；新增 A→B late、B normal、running async epoch characterization。
- 验证:`npx vue-tsc --noEmit`、受影响 ESLint 已通过；全量门禁与 GUI 仍待本会话收尾，真实 Tauri/WebView2 仍为 GUI Pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（Luna 最终 late-event 门禁修复）
- 做了:MediaGrid 150ms scroll settle 捕获 gallery async identity、view key 与实际目标 scroll position；回调执行前复核当前/活跃/未阻塞，view/route/activation barrier、失活与卸载立即清 timer，旧 A 不得写 B。
- 做了:AppShell 兜底键盘分发接入共享 MediaGrid galleryViewActive 信号与真实 gallery route 判定；/view、/doc、/audio 过渡期间不再把空 activeViewer 当 grid，alertdialog 与正常 gallery 键位保持可用。
- 验证:定向 3 files/68 tests；完整 `npx vitest run --reporter=dot` 通过（151 files/1635 tests）；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过，build 入口 `698.33 kB / 708 kB`。
- 记录:Vitest/vue-tsc 的 npx wrapper 报既存 `.npmrc` `ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`；build 仅既存 dynamic/static import 分块提示。
- GUI:真实 Tauri/WebView2 的 route-return 逐帧几何/backing-store、scroll late-event、route transition keyboard 与 reduced-motion/transitionend 缺失仍待人工验收，阶段 3 继续 pending，三件套保持 `status: 施工中`。

## 会话:2026-08-18（目录定位 pending 代次收口）
- 做了:FoldersSection 改用 uiStore `setPendingScrollDir` 登记目录目标；uiStore 维护单调 `pendingScrollDirRequestVersion` 与版本校验清理 API，同目录重复点击也形成新 owner。
- 做了:useGalleryScrollToDir watcher 同时消费目录 id+代次，并将 identity/viewKey 一并绑定；路由返回、失活、blocked、迟到 IPC/滚动回包、finally、延迟 timer 与卸载路径只清 ownership 仍匹配的目标，A stale 不会清 B 同目录新请求。
- 测试:`npx vitest run src/composables/useGalleryScrollToDir.spec.ts src/stores/uiStore.spec.ts --reporter=dot` → 2 files/24 tests passed；新增 stale 保留新代、回到 A 同目录重触发及正常成功清理 characterization。
- 验证:全量 `npx vitest run --reporter=dot` → 151 files/1638 tests passed；`npx vue-tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 均通过，build 入口 `698.52 kB / 708 kB`，仅既存 dynamic/static import 提示。
- 记录:npx Vitest/vue-tsc 输出既存 `.npmrc` warning：`ignoring unparseable entry "@deepseek-ai/dsh-subprocess-local koffi node-pty @google/genai protobufjs"`。
- GUI:真实 Tauri/WebView2 A→B/失活/回 A 目录定位、route-return 几何与 Canvas backing-store 仍待人工验收；阶段 3 继续 pending，三件套保持 `status: 施工中`。
