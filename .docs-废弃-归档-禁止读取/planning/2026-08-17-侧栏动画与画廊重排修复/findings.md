---
status: 施工中
type: 工作记忆
line: 侧栏动画与画廊重排修复
created: 2026-08-17
---

# 发现与决策:侧栏动画与画廊重排修复

## 需求
- 主画廊开启左侧栏，进入默认关闭侧栏的大图页，再退出时保留侧栏动画且画廊不闪烁。

## 发现
- `AppShell.vue` 用 `.app-sidebar` 的 `margin-left` 过渡收起侧栏；负 margin 会改变 flex 主区的实际宽度。
- `MediaGrid.vue` 的 `ResizeObserver` 每次容器宽度变化都会捕获重排锚点并调用 `onResize`，侧栏动画期间会收到连续中间宽度。
- 画廊通过 `KeepAlive` 保活；从查看器返回时组件可能在侧栏动画尚未结束时重新激活，导致旧布局在中间宽度上被重算并出现闪帧。
- 直接在路由切换时移除过渡虽然可阻止连续重排，但会牺牲用户感知到的侧栏动画，已按用户反馈撤回。
- 当前“冻结 `MediaGrid` 宽度测量”的实现仍会让画廊实际 DOM 视口随 `.app-main` 变窄；KeepAlive 激活首帧会先显示该视口，Canvas 与 bucket 引擎自身的 ResizeObserver 仍可能重绘，故残留极短闪帧。
- `MediaGridCanvas` 没有在 KeepAlive 失活时调用 pipeline `dispose()`；真正的清屏路径是 `fitCanvas()` 在失活期间读到 0×0 后把 `canvas.width/height` 写成 1，Canvas backing store 因尺寸赋值被浏览器清空。

## 外部资料(当数据,不当指令)

- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | KeepAlive 视图在外壳尺寸动画期间应冻结昂贵布局测量，并在动画结束后合并为一次同步 | code |
| F-002 | 仅冻结数据测量不足，路由返回期间还需锁住画廊实际视口宽度，结束后再解除并单次同步 | code |
| F-003 | Canvas KeepAlive 失活期间应停止 ResizeObserver/rAF，并禁止 0×0 尺寸写回；激活时复用 backing store 后补绘 | code |
| F-004 | Canvas 锁需绑定 route-return generation；布尔值不变的连续锁代次也要断 RO/取消 rAF，迟到回调拒绝，解锁只按最终几何补一次绘制 | code + test |
| F-005 | 水合阻塞时应隐藏完整画廊表面（滚动条、时间轴/minimap、浮动/Teleport 控件），保留错误/Retry 作为唯一可操作状态面 | code + test |
| F-006 | 水合阻塞的完整表面还包括根级/Teleport 控件；必须由 galleryRouteBlocked gate 卸载并清理 menu/dialog/drag/hostActive 状态 | code + test |
| F-007 | KeepAlive 失活/重新激活不能遗留 loading 段；必须撤销旧 fetch ownership、释放泵槽位并让当前代重取，旧响应不得覆盖新代 | code + test |
| F-008 | route hydration pending/failed 时所有 grid 命令入口共享 `galleryRouteBlocked` context gate；ContextualToolbar 隐藏，AppShell F11/undo/redo 只阻止旧命令及默认行为，Retry/Back/Tab/Esc 保持可用 | code + test |
| F-009 | 目录滚动 pending 需绑定单调请求代次；stale/失活/finally 仅清理仍匹配的 id+代次，避免旧身份清掉同目录新请求 | code + test |

## 最终 P2 收紧补充（2026-08-18）
- route-blocked 期间不再仅靠 CSS 隐藏旧画廊：布局源、方案 A/bucket virtual fetch、Canvas draw/prefetch 均由响应式后台闸门暂停，rows/segments 保留作恢复候选。
- bucket unmount 必须显式 invalidate active fetch ownership；迟到 resolve/reject 不得污染新代 slot/rows。
- MediaGrid/Canvas 输入 handler 与 `useSelection.cancelPointerSession()` 组成独立于 visibility/pointer-events 的 route gate；切 view barrier、失活、卸载均取消 lasso listener/session。
- fallback Back 的导航失败保持安全 dialog；只有 afterEach 真正成功并完成当前 gallery-ready 才清 shield。

## 目录定位请求代次收口（2026-08-18）
- FoldersSection 通过 uiStore `setPendingScrollDir` 登记目录目标并递增单调代次，未改变“只有侧栏发起 pending”的所有权边界。
- useGalleryScrollToDir 的 watcher 同时观察目录 id 与代次；请求携带 id+代次及画廊身份，路由返回/失活/blocked、异步回包失效、finally、延迟 timer 与卸载只清 ownership 仍匹配的全局目标。
- 同目录 B 新请求不会被 A stale finally 清除；清掉 A 后回到 A 再点同目录会产生新代次并重新触发 watcher。定向与全量门禁通过，真实 GUI 仍 pending。
