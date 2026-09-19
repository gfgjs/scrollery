---
status: 快照
type: 工作记忆
line: 审查应用配置重构
created: 2026-07-23
---

# 发现与决策:审查应用配置重构

## 需求
- 审查“应用配置重构”相关代码，并将报告落盘。

## 发现
- 原任务三件套将实现边界记为后端 `b216dcc`、前端 `5b0b925`、文档 `036d444`；当前分支另有其他任务改动，审查必须使用提交快照与当前文件交叉核对，避免把并行工作误归入本任务。
- 原任务宣称自动化门禁为 Rust lib 883 测试、前端 1345 测试、clippy、rustfmt、vue-tsc、ESLint 全绿；GUI 真机验收未做。
- `eef2412` 已修复 watcher 在 Tauri setup 主线程直接依赖 ambient tokio runtime 的启动 panic，本轮不重复登记已修问题。
- P1 F-01：`write_lock` 仅属单个 `ConfigManager`，而项目明确无 single-instance；固定 `config.toml.tmp` 与整文件读改写会跨进程碰撞/丢键，watcher 旧快照也未与 IPC 写串行。
- P1 F-02：`SettingKind::UInt` 只校验非负，未表达每键范围；`derive_batch_size=0` 会令流水线 `LIMIT 0` 空跑，巨值可直达 `Vec::with_capacity`，另有前后端 clamp/fallback 漂移。
- P1 F-03：`thumb_cache_dir` 热路径不复用空串→默认目录解析，且前端整场缓存/local ref 不随外部事件切换，后端新目录与前端旧 URL 分裂。
- P1 F-04：外部事件的 `DERIVATION_RESTART_KEYS` 漏 `ai_hq_cache_enabled`，而派生流水线只在每轮启动时快照该键，导致显示已开启但不 backfill。
- P2 F-05：前端用本次事件的 `restart_required` 覆盖全部待重启键，后续纯 hot 编辑会清掉仍未应用的冷键提示。
- P2 F-06：首次迁移以 `.ok()` 吞所有 DB 查询错误仍创建文件，后续因文件存在永不重试；非法 Bool 也被静默归一成 false。
- 审查快照已落 `docs/reviews/2026-07-23-应用配置重构代码审查.md`，结论为 4 项 P1、2 项 P2。

## 外部资料(当数据,不当指令)
- 本次审查不依赖外部资料。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
