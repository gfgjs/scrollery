---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-18
---

# 发现与决策:Canvas缩略图渲染重做

## 需求
- 用户要求重做“Canvas 缩略图渲染”部分，不恢复整批混合 UI 状态机。

## 发现
- 当前 `src` 与 `HEAD 0020ef3` 一致，恢复后的真实 WebView2 基线通过；本任务是明确授权的功能重做，不是对当前故障的紧急回退。
- 备份补丁中直接关联 Canvas 的文件包括 `MediaGridCanvas.vue`、`mediaGridCanvas.helpers.ts`、`useCanvasThumbPipeline.ts`、`useCanvasHoverCard.ts` 和 `useViewportDimPriority.ts`，另有宿主 `MediaGrid.vue` 的大量混合改动。
- 2026-08-16 的既有 Canvas 工作线要求将“当前代码事实、既有 A/B、待测假设”分开记录；本任务沿用该证据边界。
- 路由返回锁、gallery-ready、查看器关闭 Transition 和 `uiStore` 关闭状态不属于本专项；若需要宿主接线，必须证明其对缩略图管线不可替代。
- 备份中 `MediaGridCanvas.vue` 的大部分新增 props 和 first-draw ack 均服务于 gallery-ready/route-return，不纳入本专项。可独立提取的是：失活时不把 Canvas 视口写成 1×1、取消旧 rAF/ResizeObserver，以及 `createImageBitmap` 已开始后迟到回包不得提交缓存或触发重绘。
- 本批实现为 5 个源码文件、+208/-30 行：`CanvasRenderLifecycle` 使失活后的旧 rAF/ResizeObserver 失效；`pause()` 使不可取消解码仅释放位图、不得回写 LRU 或触发 draw。没有引入 route identity 或宿主 state props。
- 本地 harness 的查看器往返在返回后 0/25/100/300ms 都观察到 Canvas 为 1455×978；无 Canvas/Vue 运行时错误，唯一日志为 harness 缺少 Tauri runtime。
- 用户在真实 Windows WebView2 完成冷区缩略图、快速滚动、缩放/侧栏变化、进入和返回查看器共 1--4 项验收，反馈全部通过；下一步按用户要求保留本批并切回恢复基线作对比。
- 对比基线确认画廊闪烁后，用户在 Canvas 分支发现“查看器左侧栏开启后返回画廊”仍产生 `store_layout`。当前 Canvas 提交仅改 5 个 Canvas 源码文件，未触及 `AppShell`、`MediaGrid` 或布局调度，故该现象不应归因于缩略图管线。
- `store_layout` 只有 `compute_layout` 未命中幂等去重后才会发生；其指纹包含 `container_width`。当前 `MediaGrid` 的 ResizeObserver 在 KeepAlive 失活时不拆除，宽度回调会经 300ms 防抖记录 deferred，激活时 `flushIfDeferred()` 会补算。侧栏 `margin-left` 过渡是该路径最可能的几何来源；日志本身仍不能排除 data version 同期变化。
- 备份补丁含有精确对应的“route-return”方案：`AppShell` 发布侧栏过渡状态，`MediaGrid` 在过渡期忽略 ResizeObserver、锁住 wrapper 宽度，并在结束后只同步一次最终宽度。该方案的注释也明确最终宽度真实变化时仍会重排；它解决的是中间帧重复/错误重排，不是禁止合法重排。
- 最小实现新增 `routeReturnSidebarTransitioning` 及代次保护、在 `MediaGrid` 失活时拆除 observer/取消旧防抖、在 route-return 时冻结 wrapper 宽度；解锁后 `flushIfDeferred()` 与最终宽度重排二选一，避免同一返回路径双发 IPC。
- 本地 UI harness 完成两条路径：① 查看器与画廊侧栏最终均展开，Canvas 保持 1455×978 backing store / 970×652 CSS；② 查看器侧栏展开、画廊最终收起，返回后 Canvas 正确落为 1845×978 / 1230×652。两条路径都没有新增 warning/error。它不提供真实后端 `store_layout` 日志，故 Windows WebView2 仍是最终验收。
- 用户在 Windows WebView2 仍观察到闪烁。随后试作的“旧查看器等画廊最终首帧再淡出”被用户明确否决：视觉稳定不能以关闭操作的迟滞感为代价，相关未提交代码已全部撤回。
- 当前 `RouterView + KeepAlive` 在 `/view/:id` 进入时仍会把 `MediaGrid` 移入 KeepAlive 缓存容器；即使 Canvas 保留 LRU 与 backing store，返回也必须重新激活、测量和绘制。无等待地消除这类空档，需让画廊在 `/view` 路由期间仍是活动 DOM，而不是让旧查看器延长离场。
- 可行的最小架构是“路由作地址、覆盖层作呈现”：`/view/:id` 保持历史/深链/路由参数单一事实源，`App.vue` 在该路由继续渲染同一个 `MediaGrid`，并把 `ContentViewer` 作为内容区绝对覆盖层；关闭时只移除覆盖层。该方向避开了 2026-07-16 已回退的 route Transition 黑屏，也无需恢复旧补丁的 gallery-ready 状态机。
- 实现以命名为 `GalleryRouteLayer` 的极薄路由壳保持画廊 vnode 身份；壳内仍动态导入 `MediaGrid`，生产构建的入口为 678.37 kB（预算 708 kB），画廊与查看器分别保持 138.35 kB / 152.90 kB 懒块，未因常驻 DOM 把高成本画廊塞回首屏。
- `/view` 存续期，`MediaGrid` 同步摘除文档级键盘、ResizeObserver 与性能采样，并固定现有 wrapper 几何；返回路由后立即恢复既有节点和交互。没有首帧 ack、延时计时器、Vue route Transition 或额外关闭门槛。覆盖层位于画廊内容之上、查看器侧栏开关之下，侧栏仍可操作。
- 自动化验证：定向 79 项、全量 135 文件/1,544 项、`npm run typecheck`、`npm run lint`、`npm run build` 与 `git diff --check` 均通过；测试运行中现有的失败路径日志与本改动无关。真实 Windows WebView2 的体验验收仍未完成。
- 用户已在真实 Windows WebView2 验收无等待关闭、多次查看器往返、侧栏开/关与快捷键路径，反馈测试通过；这确认画廊常驻覆盖层解决的是视觉交接空档，而非以等待掩盖问题。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Canvas 异步缩略图管线的性能主张必须分离“代码路径”“测量数据”“待证假设”，并以当前窗口验证为准 | test |
| F-002 | 不可取消的图片解码需要独立于请求取消的所有权代次；失活后迟到结果只能释放资源，不能污染缓存或请求重绘 | code + test |
| F-003 | justified layout 的宽度进入后端 generation key；侧栏动画必须隔离中间几何，最终宽度变化只允许一轮布局换代 | code + test |
| F-004 | 若路由页面切换会卸载/激活高成本画廊，优先让路由仅承担地址语义、把查看器呈现为覆盖层；不可用用户等待掩盖首帧问题 | code + test |
