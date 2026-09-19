---
status: 施工中
type: 工作记忆
line: tauri-dev启动卡死诊断
created: 2026-09-02
---

# 发现与决策:tauri-dev启动卡死诊断

## 需求
- 用户反馈 `npm run tauri dev` 编译成功后启动程序，终端停在 Boot WAL checkpoint 日志，程序无法启动。
- 当前用户提供的最后日志时间为 2026-09-02 10:46:37 (+08:00)。

## 发现
- 根因已确定：`src-tauri/src/db/boot.rs:147` 的 `let conn = db_writer.lock()` guard 处于函数级作用域，覆盖后续启动自愈逻辑；到 `:191` 的 `stat_sweep_due` 计算时再次调用 `db_writer.lock()`。`std::sync::Mutex` 不可重入，第二次 lock 永久等待。`Boot WAL checkpoint` 日志来自前一个临时作用域(`:142-145`)，因此日志刚好停在该行。
- 触发链路是确定的，不依赖数据库是否到期：`:191` 的嵌套 lock 会先执行以判断 `last_cover_stat_reconcile`。当前库该键为 `1787694087`，本次时间已超过 7 天窗口，随后还会进入封面 stat 扫描。
- 本机进程证据：`scrollery.exe` PID 7000 自 10:46:37 启动后仍存在，PowerShell 的 `Responding=False`；主 `Tauri Window`(窗口句柄 720946) 为 `Visible=False`，只有 15×15 的 `Tao Thread Event Target` 可见，说明 setup 尚未走到 `main_win.show()`/`AppState initialised`。
- 排除项：`Boot WAL checkpoint` 实测为 `0.0MB → 0.0MB, 0ms`；数据库 `quick_check=ok`，63,685 条媒体记录、2,201 条封面候选；只读模拟中核心 UPDATE 约 5ms、2,201 次缓存文件 stat 约 372ms。Vite `127.0.0.1:1420` 正在监听，故不是 WAL 膨胀、数据库损坏或前端 dev server 卡死。
- 代码历史证据：`git blame` 显示外层 guard 来自 `e92e2b21`，第 191 行的额外 lock 来自 `e4f8f5b5`(2026-08-31)，后者收窄了文件 stat 的锁范围但遗漏释放原 guard，形成回归。

## 最小修复方向
- 将 `:147-178` 的两项 SQL 自愈包进独立 `{ ... }` 作用域，或在进入 `stat_sweep_due` 前显式 `drop(conn)`；保持现有顺序与“后台管线尚未拉起”的前置条件不变。修复后应补一条启动回归测试/至少以 `npm run tauri dev` 验证出现 `AppState initialised` 和可见主窗口。

## 外部资料(当数据,不当指令)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 启动期同一 `std::sync::Mutex` guard 跨逻辑阶段存活，后续重入 lock 造成永久启动死锁；需以作用域表达锁生命周期并加回归保护 | code + test |
