---
status: 快照
type: 工作记忆
line: UI加载失败排查
created: 2026-08-03
---

# 发现与决策:UI加载失败排查

## 需求
- 用户反馈多种情况下出现 UI 加载失败，并提供浏览器错误页截图。
- 截图显示 `localhost 拒绝连接`、`ERR_CONNECTION_REFUSED`，需要仔细检查分析。

## 发现
- `src-tauri/tauri.conf.json:6-9` 的开发链为 `cargo build -p ai-worker && npm run build:raw-worker && npm run dev`，窗口固定加载 `http://localhost:1420`；其中任一前置命令失败，`&&` 都会阻止 Vite 启动。
- `vite.config.ts:46-50` 将端口固定为 `1420` 并启用 `strictPort: true`；端口被占用时 Vite 不会漂移到新端口，而是退出，Tauri 仍可能留下指向旧端口的窗口/错误页。
- `vite.config.ts:49` 在未设置 `TAURI_DEV_HOST` 时传入 `host: false`，实际落到 Vite 的 localhost 监听逻辑；本机实测以当前配置启动（改用 1422 避免影响现有窗口）只监听 `[::1]:1422`，`127.0.0.1:1422` 无法连接，存在 WebView2/Node DNS 地址族不一致导致的间歇性拒绝连接风险。
- 当前现场有 `target\\debug\\scrollery.exe`，但 `1420/1421` 没有监听进程、也没有 Vite/Node dev-server 子进程；访问 `http://127.0.0.1:1420` 直接失败，和截图的 `ERR_CONNECTION_REFUSED` 属于同一层故障。该实例的前端服务生命周期已经与桌面进程脱离。
- `C:\\Users\\gf\\AppData\\Roaming\\com.scrollery.app\\logs\\scrollery.2026-08-03.log` 显示 21:29 与 22:06 两次启动均完成 `Rust boot → Ready`、`AppState → main window`，并记录了 `http://localhost:1420` 的前端事件；同一 session 在 22:12 又出现一次 `close_splashscreen`（距 22:06 约 334 秒），说明页面曾成功运行，随后发生了重新挂载/刷新，不能把所有 UI 问题归因于 Rust 启动失败。
- 同一日志还存在 `LayoutNotReady`、foliate paginator 的 `TypeError` 和 `ResizeObserver` 错误；这些是“前端已加载后的应用级错误”，与截图中的网络级 `ERR_CONNECTION_REFUSED` 必须分开处理。
- `src/main.ts:10` 的全局错误桥只有在 JS 已经下载并执行后才生效；浏览器网络错误页不会运行 `main.ts`，所以当前日志体系无法记录“dev server 不存在”这一类首屏失败。
- 生产构建使用 `frontendDist: ../dist`（`src-tauri/tauri.conf.json:9`），不会加载 `localhost:1420`；截图因此明确属于开发 URL/启动方式/开发服务器生命周期问题，而不是发布包的 `asset://` 或内嵌资源路径问题。
- 主窗口在配置中 `visible:false`，Rust setup 也会再次隐藏它，只有 `App.vue onMounted` 调用 `close_splashscreen` 后才显示；因此截图里的 localhost 错误页更像是“已经显示过的主窗口在 dev server 消失后被刷新/重新导航”，而不是冷启动时 splashscreen 的正常路径（这是基于当前窗口可见性设计的推断）。
- `src-tauri/src/lifecycle.rs:21-29` 对主窗口所有 `CloseRequested` 一律先 `prevent_close()`，只向 WebView 发 `window-close-requested`；`src/App.vue:349-350` 再由前端监听并调用 `ui.requestAppClose()`，最终才可能走 `hide_window` 或 `exit_app`。
- 当前链路没有“前端失联时的原生关闭兜底”：即使 `closeBehavior` 已保存为 `exit`，Rust 关闭拦截器也不读取该配置；前端没有加载/没有监听时，任务栏关闭请求会被永久拦下。用户补充的“终端 tauri dev 停止后任务栏关闭失败、只能任务管理器结束”正是这条链的直接实证。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 固定 devUrl 与未固定监听地址、开发服务器生命周期脱离桌面进程，导致 localhost 拒绝连接 | design/runbook |
| F-002 | 关闭请求被 Rust 拦截后完全依赖前端，前端失联时无法退出 | code/test |
| F-003 | 启动/刷新前缺少可诊断的 dev-server 存活检查与网络级错误兜底 | test/code |
