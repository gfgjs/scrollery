---
status: 快照
type: 工作记忆
line: 审查应用配置重构
created: 2026-07-23
---

# 进度日志:审查应用配置重构

## 会话:2026-07-23
- 做了:读取项目文档治理规则、`planning` 与 `caveman-review` skill；核定 `b216dcc`/`5b0b925`/`036d444` 与后续 `eef2412`；逐层审查 schema、文件、迁移、watcher、IPC、副作用、派生与前端事件链；落盘审查报告。
- 验证:`cargo test --lib config:: -- --nocapture` → 32 passed/0 failed；`npx vitest run src/composables/useHoverPreview.spec.ts` → 11 passed；检索确认无 `useConfigFile` 事件测试；报告路径/行号与当前文件复核完成；`git diff --check` 通过；旧 `check_docs` 已移交外部 worklog-kit，仓内无可调用脚本。
- 遗留:产品代码仍有 4 项 P1、2 项 P2，等待用户决定是否进入修复施工；GUI 双实例/外部编辑/缓存目录真机矩阵未执行。

## 回顾(收口时填)
- 亮点:以原任务提交边界和当前实现交叉核对，分开记录已修历史 panic 与仍存问题，避免把旧缺陷重复报成当前问题。
- 教训:配置 schema 的“键单源”不等于“有效值与副作用单源”；类型、范围、热应用副作用和跨进程写协调都必须进入同一契约。
- 意外:仓库已有注释明确应用允许双实例，但本重构的写锁与固定 tmp 仅按单 manager 设计；现有配置测试全部通过仍未覆盖这一关键运行模型。
