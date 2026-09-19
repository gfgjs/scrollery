---
id: 2026-08-30-精确内容去重施工-closeout
status: snapshot
type: closeout
line: 去重功能全局方案
created: 2026-08-30
---

# 收口处置:精确内容去重施工

> ⚠️ 已部分被推翻（2026-08-31）：本快照冻结于上一轮基线 `585341f1`。此后 P4 系统回收站交接因缺少可证明的句柄绑定原语而 fail closed，物理释放暂未开放，范围以 [状态片](../../status/去重功能全局方案.md) 为准。

本轮完成 P0–P4，并以实现基线 `9af0f155` 与修复验收批次 `585341f1` 冻结代码引用。修复批次补齐根路径语义、逐 item journal 收敛、V29 同卷 quarantine 交接与启动恢复、同 mtime/size 失效和前端成员请求隔离。Live Photo 物理清理因首版无法证明多组件原子性而明确禁用；应用回收站软删除仍可用。所有破坏性自动化测试仅使用测试临时目录。

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | code | repo:src-tauri/src/db/queries/dedup.rs@585341f1 | `write_dedup_index_if_generation_current` | new | — | yes |
| F-002 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §8.2 已落地：安全物理清理 | new | — | yes |
| F-003 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §7 文案与潜在逻辑大小 | new | — | yes |
| F-004 | code | repo:src-tauri/src/db/queries/scan/media_upsert.rs@585341f1 | `resolve_suspect_change` | new | — | yes |
| F-005 | code | repo:src-tauri/src/utils/path.rs@585341f1 | `normalize_root_path` 与绝对根回归 | new | — | yes |
| F-006 | code | repo:src-tauri/src/db/boot.rs@585341f1 | `retry_dedup_database_deletes` 混合状态恢复 | new | — | yes |
| F-007 | code | repo:src-tauri/src/dedup/cleanup.rs@585341f1 | quarantine 原子隔离、无覆盖恢复与测试 | new | — | yes |
| F-008 | code | repo:src-tauri/src/db/queries/scan/media_upsert.rs@585341f1 | 同 mtime/mtime_ns 不同 size 的 source revision 失效 | new | — | yes |
| F-009 | code | repo:src/stores/dedupStore.ts@585341f1 | 成员请求 generation 与切组迟到响应隔离 | new | — | yes |
| D-001 | code | repo:src-tauri/src/dedup/hash.rs@585341f1 | quick/exact 摘要边界与流式状态复核 | new | — | yes |
| D-002 | code | repo:src-tauri/src/db/schema/late.rs@585341f1 | V26 `dedup_index` migration | new | — | yes |
| D-003 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §8.2 journal 阶段与失败保留 | new | — | yes |
| D-004 | design | repo:docs/status/去重功能全局方案.md | 当前诚实边界 | new | — | yes |
| D-005 | code | repo:src-tauri/src/db/queries/scan/media_upsert.rs@585341f1 | `resolve_suspect_change` source revision 失效链 | new | — | yes |
| D-006 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §8.2 quarantine 与 writer 锁规则 | new | — | yes |
| D-007 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §8.2 逐 item journal 收敛 | new | — | yes |
| D-008 | design | repo:docs/designs/2026-08-30-去重功能全局实现方案.md | §9 根路径、symlink 与 physical identity 边界 | new | — | yes |
