---
status: 施工中
type: 工作记忆
line: 启动界面优化
created: 2026-08-25
---

# 进度日志:启动界面优化

## 会话:2026-08-25
- 做了:只读审计 Tauri 双窗口配置、Rust 显隐命令、Vue 启动配置等待、现有窗口材质与主题首帧链路；形成“main 隐藏创建→恢复几何/修正装饰→setup 前段显示→静态同窗口启动层→Vue ready 淡出”的单窗口方案。
- 验证:`rg` 与定点源码读取确认当前路径为 `splashscreen visible → App mounted/config ready → close splash → show main`；`npx worklog-kit check --warn-only` 通过（761 个文档、11747 个代码/配置文件）；本轮未改功能代码、未跑应用构建。
- 遗留:等待用户确认推荐方案后再实施与测试。

## 会话:2026-08-25（施工）
- 做了:删除 `splashscreen` 原生窗口、独立 HTML、`close_splashscreen` IPC；main 在 setup 前段完成无框修正后立即显示；`index.html` 新增静态主题启动层，配置就绪后由 Vue/日志窗口淡出；Rust 在 config manager 就绪后提前应用原生 window material；补充启动层淡出幂等测试。
- 验证:`npm run lint` 通过；`npm run typecheck` 通过；`npm test` 通过（139 files / 1569 tests）；`npm run build` 通过（bundle budget 通过）；`cargo fmt --all -- --check` 通过；`cargo check -p scrollery` 通过；本地 UI harness 检查确认完整画廊正常挂载且启动层 ready 后移除；`npx worklog-kit check --warn-only` 与 `git diff --check` 通过，代码侧无残留 splash IPC 引用。
- 遗留:需在 Windows/macOS 真机手工验收冷启动、热启动、最大化、自定义尺寸恢复、Mica/Acrylic/none 与窗口关闭/托盘往返；完成后再收口归档三件套。

## 会话:2026-08-25（首帧闪烁优化）
- 做了:将 `index.html` 静态启动层前置为 `body` 第一节点；补齐 `html/body` 首帧背景；启动层背景、文字、logo 颜色和字体改用入口内联值，不再等待模块全局 CSS，减少原生窗口首次可见到 logo 层绘制之间的空背景重绘与文字重排。
- 验证:本地加载态 DOM 确认 `startup-layer` 为第一节点且保持 `loading`；截图确认 logo 层直接覆盖完整窗口；`npm run lint`、`npm run typecheck`、`npx vitest run src/utils/startupLayer.spec.ts`（2 tests）、`npm run build`、`git diff --check` 均通过。
- 遗留:仍需 Windows/macOS 真机观察冷启动是否无空背景闪烁；其余跨平台窗口状态与材质验收保持原计划。

## 会话:2026-08-25（回归修复：透明首帧与材质失效）
- 现象:窗口在 Rust setup 前段过早 `show()`，WebView 可能在 `AppState` 注入前请求启动配置；同时原生窗口材质通过异步主线程投递，首个可见帧可能先露出透明窗口。前端启动配置失败后 `html[data-glass]` 未水合，表现为全局毛玻璃消失、Acrylic 选项无效。
- 修复:移除 setup 前段的 `show()`；保留隐藏窗口的装饰修正，待 `app.manage(AppState)` 后同步挂载首帧材质再显示，并在 show 后异步重挂一次；材质投递失败增加结构化 warning。
- 验证:`cargo fmt --all -- --check`、`cargo check -p scrollery`、`npm run lint`、`npm run typecheck`、`npm test`（139 files / 1569 tests）、`npm run build` 均通过。
- 遗留:需在 Windows 真机确认冷启动不再出现透明框，并在设置页切换 `mica`/`acrylic`/`none` 后确认前端 `data-glass` 与原生 DWM 背板同步；macOS 仍需确认透明/材质 no-op 不受影响。

## 会话:2026-08-25（毛玻璃复核：根背景遮挡）
- 定位:启动层为避免首帧透明在 `index.html` 的 `html, body` 上保留启动底色；启动层淡出后该 `html` 背景仍然存在，之前只检查 `body` 透明而漏掉根节点，因此 DWM 背板被整页遮住。
- 修复:在 `glass.css` 增加 `html[data-glass] { background: transparent; }`，让 Mica/Acrylic 水合后同时清除根背景；补充 CSS 契约测试防止启动底色再次盖住原生背板。
- 验证:生产构建后重启实际 Windows WebView，确认 `data-glass=acrylic`、`html/body` 计算背景均为透明、DWM `DWMWA_SYSTEMBACKDROP_TYPE=3` 且配置仍为 Acrylic；实测 `none → acrylic` 热切换有差异；`npm test`（139 files / 1570 tests）、`npm run typecheck`、`npm run build`、`cargo fmt --all -- --check`、`git diff --check` 通过。
- 遗留:仍需用户侧确认自己的桌面壁纸/窗口位置下的观感，以及 macOS/跨平台手工验收；本轮未归档工作记忆。

## 回顾(收口时填)
- 亮点:待收口补充。
- 教训:待收口补充。
- 意外:待收口补充。
