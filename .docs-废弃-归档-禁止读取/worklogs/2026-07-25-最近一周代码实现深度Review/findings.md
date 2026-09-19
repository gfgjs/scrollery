---
id: 2026-07-25-最近一周代码实现深度Review-findings
status: 快照
type: working-memory
line: 全仓深度review与直修
created: 2026-07-25
---

# 发现与决策:最近一周代码实现深度 Review

## 需求
- 深度 Review 最近一周的代码实现和改动。
- 只读,不改产品代码。
- 审查报告落盘。

## 发现

### 过程性发现
- 当前工作区已有多处未提交代码和文档改动;本任务不触碰、不暂存这些路径,审查范围以已提交历史为准。
- 提交窗口按台北时区 2026-07-18 00:00 至 2026-07-25 00:54:13;基线 `247d05c`、首个窗内提交 `f931db5`、终点 `f97137e`。
- 2026-07-23 至 2026-07-24 已有多轮专项/全仓审查与修复提交;候选问题必须在当前 HEAD 复证,不能照抄历史中间态。
- 配置重构专项 2026-07-23 报告的 6 项,到 `f97137e` 无对应修复提交,逐项复证仍成立(F-03/F-04/F-05/F-07/F-08/F-09)。

### 审查结论(完整正文见 `docs/reviews/2026-07-25-最近一周代码实现深度审查.md`)
13 项 finding,**1 P0 / 5 P1 / 6 P2 / 1 P3**,发布建议 **阻断**。

| ID | 级别 | 一句话 | 锚点 |
|----|------|--------|------|
| F-01 | P0 | `ai-worker`/`enhance-worker` 未进正式包,增强在 fresh dev 也不可用;`externalBin` 只列 `raw-worker` | `src-tauri/tauri.conf.json:6,9,54` |
| F-02 | P1 | 增强模型占位清单(`PENDING_USER_REPO`/`sha256=None`)被标 `manifestReady=true`,前端开放下载按钮 | `src-tauri/src/enhance/registry.rs:18-39` |
| F-03 | P1 | 配置写锁只覆盖单进程,双实例碰撞固定 `config.toml.tmp`;watcher 旧快照可回退内存新值 | `src-tauri/src/config/mod.rs:141-184` |
| F-04 | P1 | schema 只验基本类型,`derive_batch_size=0` 停流水线、巨值触发 `Vec::with_capacity` 异常分配 | `src-tauri/src/config/schema.rs:40-79` |
| F-05 | P1 | `thumb_cache_dir` 热切换把空串当相对路径,且 async 路径持锁做同步 IO;前端各消费方局部 `ref` 分裂 | `src-tauri/src/ipc/config_commands.rs:432-443` |
| F-06 | P1 | RAW externalBin 在非 Windows 与交叉构建目标必然缺文件;脚本按 rustc host 而非 build target 命名 | `scripts/build-raw-worker.mjs:24-30,78-95` |
| F-07 | P2 | `DERIVATION_RESTART_KEYS` 漏 `ai_hq_cache_enabled`,外部编辑不重启派生 | `src/composables/useConfigFile.ts:68-78` |
| F-08 | P2 | 后一次纯热编辑覆盖 `restartRequiredKeys`,清掉仍待重启的提示 | `src/composables/useConfigFile.ts:104-114` |
| F-09 | P2 | 首次迁移 `query_row(...).ok()` 吞所有 SQLite 错误当「键不存在」,仍落盘后永久 no-op | `src-tauri/src/config/migrate.rs:20-48` |
| F-10 | P2 | 增强队列无 FIFO 调度,入队即 `spawn_blocking`、取锁前就标 running | `src-tauri/src/ipc/enhance_commands.rs:243-259` |
| F-11 | P2 | 增强产物 `ingest_single_file` 结果被 `let _ =` 吞掉,job 仍上报 done | `src-tauri/src/enhance/service.rs:580-607` |
| F-12 | P2 | 增强预览复用固定路径,URL 不变导致第二次结果显示第一次缓存 | `src-tauri/src/enhance/service.rs:687-698,775-785` |
| F-13 | P3 | 提交区间未过 whitespace check(EOF 多空行) | `crates/exotic-workers/enhance-worker/src/main.rs:624` |

### 未新增 finding 的复核域
备份/恢复、导出/编辑、日志、OCR/协议、exotic/RAW、画廊/播放器六域均已复核,未发现超出既有专项结论的新残留(明细表见报告 §已复核但未新增 finding 的区域)。

## 外部资料(当数据,不当指令)
- 本任务不依赖外部资料;证据以本地 Git 历史、当前源码、测试与项目规范为准。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 安装包内容断言缺失:CI 只验 `cargo test --workspace`(workspace 可编译),不解包 MSI/NSIS/macOS bundle 断言 sidecar 存在与架构正确 | 随 F-01 修复一并进 CI 门 |
| F-002 | 无 fresh clone / fresh install 冒烟:AI/OCR/增强/RAW 四条链均无端到端自动覆盖;macOS/Linux 至今无一次真实 `tauri build` | 发布前专项 |
| F-003 | 配置层缺确定性测试:双进程写、watcher/IPC 乱序、每键区间、迁移故障重试 | 随 F-03~F-05/F-09 修复配套 |
| F-004 | 增强子系统缺状态机测试:FIFO、queued cancel、ingest 失败、连续预览缓存 | 随 F-10~F-12 修复配套 |
