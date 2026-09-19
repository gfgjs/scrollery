---
status: 快照
type: 工作记忆
line: UI加载失败排查
created: 2026-08-03
---

# 进度日志:UI加载失败排查

## 会话:2026-08-03
- 做了:读取项目文档治理规则和 planning skill；确认仓库存在既有未提交修改；创建本任务独立工作记忆。
- 做了:检查 Tauri/Vite 配置、Cargo feature、启动生命周期、开发脚本、CI、当前进程与应用日志；对照官方 Tauri/Vite 文档核对 devUrl、beforeDevCommand、localhost 监听和 strictPort 语义；根据用户补充继续追踪任务栏关闭路径。
- 验证:当前 `1420/1421` 无监听；`target\\debug\\scrollery.exe` 仍在运行且无 Vite 子进程；独立启动 Vite 到 1422 成功但只监听 `::1`，IPv4 访问失败；应用日志证明 21:29/22:06 曾正常完成 Rust Ready 与前端 IPC；源码确认 CloseRequested 被 `prevent_close()` 后依赖 WebView 事件处理。
- 做了:核对 `lifecycle.rs`、`App.vue`、`uiStore`、关闭确认弹窗和 `exit_app` IPC；确认任务栏关闭会先被 Rust `prevent_close()` 拦截，再依赖前端事件监听，前端失联时无原生退出兜底。
- 验证:用户补充“终端 tauri dev 停止后任务栏关闭失败，只能任务管理器结束”与源码链路完全吻合；任务管理器强杀还会绕过 `RunEvent::Exit` 中的 WAL checkpoint 与后台任务 abort/join。未修改业务代码。
- 做了:将 Tauri `devUrl`、Vite 默认监听地址和两个开发辅助脚本统一到 `127.0.0.1:1420`；保留 `TAURI_DEV_HOST` 覆盖能力，兼容需要远程访问的开发场景。
- 做了:新增前端存活心跳 IPC；把 `window-close-requested` 监听和首个心跳放到 `App.vue` 首个启动 IPC 之前；Rust 对 `exit` 直接原生关闭、对 `minimize_to_tray` 原生隐藏、对 `ask` 在心跳失联时放行原生关闭。
- 验证:Rust lifecycle 定向测试 5 passed、前端全量测试 1528 passed、`vue-tsc`、受影响文件 ESLint、`npm run build`、`cargo clippy -p scrollery --lib --locked -- -D warnings` 均通过；独立 Vite 验证确认修复后监听 `127.0.0.1` 且 HTTP 返回 200。
- 验证:完整 `npm run lint`、`cargo test -p scrollery --lib --locked`（1075 passed / 6 ignored）和 `cargo fmt --all -- --check` 均通过；此前的前端全量测试、`vue-tsc`、构建和 Clippy 也保持通过。
- 遗留:需要一次真实 `npm run tauri dev` 的 WebView 加载、停止 dev server 后任务栏关闭和托盘行为手测；这些 GUI/进程生命周期场景未被自动化测试覆盖。

## 回顾(收口时填)
- 亮点:用进程监听状态、Vite 独立端口验证、应用日志和 Rust/前端关闭调用链交叉证明，避免把网络层与 Vue 运行时错误混为一谈。
- 教训:开发 URL 是桌面端运行时依赖；停止终端不应留下仍指向 devUrl 的孤儿窗口，关闭路径也不能依赖已经失联的 WebView。
- 意外:日志显示后端 281ms Ready、前端曾成功运行，随后同一 session 再次 close_splashscreen；故障是开发 server/页面生命周期不稳定，不是单纯 Rust 启动慢。
