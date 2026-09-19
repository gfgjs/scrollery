---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-18
---

# 进度日志:Canvas缩略图渲染重做

## 会话:2026-08-18
- 做了:建立独立 Canvas 缩略图渲染重做计划，确认可用源码基线、备份补丁和已有 Canvas 工作线。
- 验证:当前分支为 `codex/recovery-stabilization` 的 `84c6629`，`src` 与 HEAD 一致；旧补丁中 Canvas 直接相关差异位置已定位。
- 做了:创建 `codex/canvas-thumbnail-rendering`，在隔离工作树应用备份补丁并逐文件对比后清理；确定首批范围为 Canvas KeepAlive 生命周期和缩略图异步所有权。
- 遗留:先补 pause/迟到解码回包的管线回归测试，再以最小组件生命周期接线实现；不接入路由 ready/resize lock 机制。

## 会话:2026-08-18（首批实现与自动化验证）
- 做了:新增 `CanvasRenderLifecycle`，在 `MediaGridCanvas` 的 KeepAlive 激活/失活期安装或移除 observer、取消旧 rAF，并避免 0×0 视口重置 backing store；缩略图管线新增 epoch 所有权与 `pause()`，迟到 decode 只释放位图。
- 验证:定向 Canvas 测试 66 项通过；全量 `npm test` 为 134 文件/1,537 项，`npm run typecheck`、`npm run lint`、`npm run build` 和 `git diff --check` 通过。本地 UI harness 查看器往返后的 4 次尺寸采样均为 1455×978。
- 遗留:等待真实 Windows WebView2 观察冷区加载、快速滚动及多次查看器往返；通过后才提交本批。

## 会话:2026-08-18（真实 GUI 验收）
- 做了:用户在 Windows WebView2 完成 1--4 项实测：冷区缩略图、快速滚动、缩放/侧栏变化，以及进入/返回查看器。
- 验证:用户反馈全部通过；这补足了本地 Vite harness 无法覆盖的原生 WebView2 验收。
- 遗留:按用户要求先将此 Canvas 专项保存到独立分支，再切回 `codex/recovery-stabilization` 的恢复基线，等待对比测试结果决定合入或收口。

## 会话:2026-08-18（侧栏返回布局换代诊断）
- 做了:用户在恢复基线确认闪烁后切回 Canvas 分支，并报告查看器左侧栏开启后返回画廊仍出现 `store_layout`。
- 验证:当前分支与恢复基线的源码差异仅限 5 个 Canvas 文件；后端 `gen_key` 包含容器宽度，当前 `MediaGrid` 的 ResizeObserver 与 `flushIfDeferred()` 可把失活/过渡期宽度事件回放为布局计算。备份补丁中存在 route-return 宽度冻结和最终单次同步的对应实现。
- 遗留:用户已采纳最小提取方案；先补“最终宽度相同为零重排、不同为一次重排”的契约测试，再实现，不恢复旧补丁的混合状态机。

## 会话:2026-08-18（route-return 几何守卫实现）
- 做了:在 `AppShell`、`uiStore`、`MediaGrid` 加入带代次的 route-return 过渡信号；查看器返回期间固定画廊 wrapper 宽度、暂停布局调度，失活时拆除 ResizeObserver 并撤销旧 resize 防抖。
- 验证:新增 5 项回归测试（宽度锁/最终宽度判定、过渡代次、旧防抖取消）；定向 69 项、全量 `npm test` 135 文件/1,542 项、`npm run typecheck`、`npm run lint`、`npm run build` 与 `git diff --check` 通过。本地 UI harness 覆盖侧栏最终相同与不同的两种返回路径，Canvas 几何均稳定、无新增 warning/error。
- 遗留:等待用户在 Windows WebView2 复测同一路径并观察 `store_layout`：最终画廊宽度不变应不再换代；最终宽度改变时只允许一次换代。

## 会话:2026-08-18（视觉交接方向复核）
- 做了:根据用户“仍闪烁”的反馈，试作过旧查看器等待画廊最终首帧再淡出的交接；用户实测确认关闭大图存在明显迟滞。
- 验证:该试作没有保存；已用定向补丁撤回 `App.vue` Transition、查看器单根包装、Canvas paint ack 与 handoff store 状态，源码回到已验证的 Canvas + route-return 几何守卫集合。
- 遗留:改做无等待方案——保留 `/view/:id` 的地址/历史语义，但让 `MediaGrid` 在该路由期间持续留在 DOM，查看器作为同一内容区的绝对覆盖层；先完成现有生命周期/键盘/侧栏冻结面的取证和最小接线。

## 会话:2026-08-18（无等待路由覆盖层试作）
- 做了:新增 `GalleryRouteLayer` 保持同一个画廊 vnode；`/view/:id` 仍由路由提供 URL、历史和深链，但 `App.vue` 在内容区保留该画廊并将 `ContentViewer` 绝对覆盖。`MediaGrid` 在查看器期间冻结现有几何、暂停键盘/observer/性能采样，关闭时同步撤层并恢复原节点；没有等待首帧、淡出或路由 Transition。
- 验证:新增 `/view` 专用路由判据测试；定向 79 项、全量 `npm test` 为 135 文件/1,544 项，`npm run typecheck`、`npm run lint`、`npm run build` 和 `git diff --check` 均通过。入口包 678.37 kB，低于 708 kB 预算，`MediaGrid` 与 `ContentViewer` 仍是独立懒块。
- 遗留:等待用户在 Windows WebView2 测试：① 侧栏开/关下连续开关大图；② 快速 Esc/关闭按钮返回；③ 打开后的侧栏开关与方向键；④ 多次循环后是否仍有画廊闪烁或 `store_layout` 异常换代。通过后才保存本批。

## 会话:2026-08-18（真实 GUI 验收与收口）
- 做了:用户完成无等待查看器覆盖层的 Windows WebView2 测试并确认通过；保存本任务相关源码为 `3efdee6`。
- 验证:用户覆盖连续打开/关闭、侧栏开关、快捷键与多次往返，未再报告迟滞或闪烁；此前的 Canvas 生命周期提交为 `35c4cff`。
- 遗留:无；按用户授权归档工作记忆、蒸馏耐久决策并回写滚动状态。

## 回顾(收口时填)
- 亮点:先把 Canvas 生命周期与混合状态机拆开验证，再以“路由作地址、覆盖层作呈现”消除返回空档。
- 教训:视觉稳定不能靠延迟关闭伪装；几何守卫能抑制中间布局换代，却不能取代连续 DOM 帧。
- 意外:原有路由语义无需放弃，改变路由组件的呈现职责即可同时保留深链/历史与即时返回。
