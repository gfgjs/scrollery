---
status: 施工中
type: 工作记忆
line: tauri-dev启动卡死诊断
created: 2026-09-02
---

# 进度日志:tauri-dev启动卡死诊断

## 会话:2026-09-02
- 做了:读取项目文档治理与 planning skill；建立本次诊断工作记忆目录。
- 做了:检查工作树、tauri/Vite 配置、Rust setup 与 `db::boot::run_startup_reconciliation` 调用链；独立子代理已受托做同范围只读复核。
- 验证:复现 PID 7000 卡在启动；主 Tauri 窗口仍隐藏且不响应；WAL 为 0MB/0ms；SQLite `quick_check=ok`；Vite 1420 端口监听正常；只读数据库模拟显示核心查询与文件 stat 均在毫秒级。
- 结论:确定是 `src-tauri/src/db/boot.rs:147` 外层 writer Mutex guard 未释放，`:191` 再次 lock 同一不可重入 Mutex 的自死锁。最小修复是收窄 `:147-178` guard 作用域/显式 `drop(conn)`；本轮按“诊断请求”暂不改应用逻辑。
- 做了:用户确认后将首段两项自愈包进独立作用域，补 `startup_reconciliation_releases_writer_between_phases` 回归测试。
- 验证:`rustfmt --edition 2021 --check src-tauri/src/db/boot.rs` 通过；targeted cargo test 1 passed；`cargo check -p scrollery --no-default-features --features lite,channel-direct` 通过；现有 `npm run tauri dev` 自动重启后 PID 13728 `Responding=True`、主窗口标题为 Scrollery，日志出现 `AppState initialised` 与 `Rust boot → Ready 342ms`。
- 遗留:代码与本次 planning 目录尚未提交；工作树其余既有改动保持不动。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
