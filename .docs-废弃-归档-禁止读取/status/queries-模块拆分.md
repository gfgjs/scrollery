---
id: 2026-07-17-status-queries-模块拆分
status: active
type: rolling-status
line: queries-模块拆分
created: 2026-07-17
---

# queries-模块拆分 · 滚动状态

> 状态板 2026-08-24 自 docs/todo.md「`db::queries` 模块拆分」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## 权威文档
> 已批准设计:[2026-07-16-queries模块拆分方案.md](../designs/2026-07-16-queries模块拆分方案.md)；工作记忆:[planning/2026-07-16-queries模块拆分方案/](../planning/2026-07-16-queries模块拆分方案/task_plan.md)。性质=纯结构重构，保持 `crate::db::queries::<symbol>` 公开路径、SQL 语义、事务边界与测试行为不变。

## 已收官指针
> 📦 本节 2026-08-23 整节收官搬迁:状态板 5 行全 ✅(P0 冻结+四批施工+三 manifest 零 diff),交付史已迁 → [completed.md](../completed.md) ▸ Q 节。**⏸ 余**:§8.4 运行时 smoke(真机:启动/扫描/画廊排序筛选/favorite 往返/缩略图/文档合集/AI 人脸只读/卷与插件列表)。
