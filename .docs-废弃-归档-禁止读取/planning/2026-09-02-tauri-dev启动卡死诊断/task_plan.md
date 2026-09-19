---
status: 施工中
type: 工作记忆
line: tauri-dev启动卡死诊断
created: 2026-09-02
---

# 任务计划:tauri-dev启动卡死诊断

## 目标
定位 `npm run tauri dev` 在 Scrollery 启动期停在 Boot WAL checkpoint 后无窗口/无后续日志的真实原因，并给出可验证的处理方案；未经用户要求不修改应用逻辑。

## 当前阶段
阶段 3:修复与端到端验收

## 阶段

### 阶段 1:复现与启动链路定位
- [x] 获取当前工作树状态与相关启动入口
- [x] 复现并确认进程、窗口、数据库文件状态
- [x] 追踪 Boot WAL checkpoint 后的同步/异步调用
- **状态:** completed

### 阶段 2:证据收敛与验证
- [x] 排除数据库锁、前端 dev server、窗口创建等候选原因
- [x] 用最小安全验证确认阻塞点
- [x] 输出根因、临时规避与是否需要代码修复
- **状态:** completed

### 阶段 3:修复与端到端验收
- [x] 收窄启动自愈 writer guard 作用域，消除同一 Mutex 重入
- [x] 增加启动自愈回归测试
- [x] 运行 targeted test、Rust check，并验证 tauri dev 自动重启后主窗口可见且响应
- **状态:** completed

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 诊断结论为 `run_startup_reconciliation` 的同一 `std::sync::Mutex` 重入死锁；修复应只收窄首段 guard 作用域，不改变启动顺序 | 第 147 行 guard 覆盖函数尾部，第 191 行再次 `lock()`；进程隐藏且不响应，Vite/WAL/DB 均正常 | D-001 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
