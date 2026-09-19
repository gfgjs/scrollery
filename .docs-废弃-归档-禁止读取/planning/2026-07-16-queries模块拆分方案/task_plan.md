---
status: snapshot
type: working-memory
line: queries模块拆分方案
created: 2026-07-16
---

# 任务计划:queries模块拆分方案

## 目标
基于当前源码形成可 Review、可分阶段回滚的 `src-tauri/src/db/queries.rs` 拆分方案；用户确认前只修改文档，不实施 Rust 重构。

## 当前阶段
收口(2026-07-16):P0～P5 共 19 个 Rust commit 全部落地,最终门+manifest 终拍全绿;§8.4 运行时 smoke 属 GUI 真机验收未自动化;三件套按用户指示留 docs/planning/ 不归档

## 阶段

### 阶段 1:现状取证
- [x] 测量文件行数、字节、公开函数/类型、测试数量与测试占比
- [x] 盘点领域职责、调用文件、近期 churn 和既有审查结论
- [x] 核查路径敏感宏与现有模块调用形态
- **状态:** completed

### 阶段 2:目标架构设计
- [x] 定义稳定 facade、14 个领域 owner 与依赖方向
- [x] 定义 API/行为/测试不变量
- [x] 定义 mapper、跨域 helper 和文档引用归属
- **状态:** completed

### 阶段 3:迁移与验收设计
- [x] 设计 P0–P5 分阶段迁移顺序
- [x] 设计一领域一 commit、阶段门、最终门与回滚策略
- [x] 形成 Review 裁决表与 Done 定义
- **状态:** completed

### 阶段 4:用户 Review 与裁决
- [x] Review D-001～D-007（2026-07-16 复核:全部量化声称逐值实证,结论=方案成立,D-001～D-007 全按推荐采纳）
- [x] 将用户裁决回写设计正文，而非仅追加说明（设计 §3.3/§4.1/§4.3/§6/§9/§11 已改;A1～A5 修订同批落正文）
- [x] 用户明确批准前不修改 Rust 代码（全程只读取证 + 文档回写,零 Rust 改动）
- [x] 二次核实发现立项后基线漂移、两个新增 DAO 未归属、S/T 同文件冲突与 test list 路径噪声；用户采纳 D-008～D-011 并要求回写
- [x] 三次核实发现 `search.rs` 叶子前提失效;用户采纳 D-012 方案 a(P1 暂引 facade helper,P4 迁 owner 时顺改路径)
- **状态:** completed

### 阶段 5:实施（批准后）
- [x] S 线 P3/P4 代码写入与自动门已完成;剩余真机验收若触发返修则停止 T 线并重建基线
- [x] P0 按开工 HEAD 重建 manifest(2026-07-16,HEAD `ae69565`):queries.rs 11,228 行(wc)/503,724 bytes 与快照 `13a3192` 逐字节相同=零漂移;API manifest 232 项(214 pub fn+18 公开类型)、测试 manifest workspace 695/queries 138/末级名重复 0、消费文件 42(+`ai/runtime_config.rs`,U-P2 当日新增,合法);基线 `cargo test --workspace --locked` EXIT=0;路径宏零命中;写入窗口冻结;施工期跨域符号增补裁决 7 项回写设计 §4.3/§6(NOT_BLOCKED 双 const 归 exotic、SELECTION_BATCH_CHUNK 归 layout、get_placeholder_item_path 归 metadata、get_item_file_format 归 documents、x1 测试归 faces、r2_6 测试五域拆分)
- [x] P1 低耦合模块(config/collections/documents/storage/search 各一 commit:e95a9a3/363b6c6/5537bd3/5516f2e/39607ce;search.rs 按 D-012 方案 a 暂引 facade helper;出口门=`--list` manifest 对拍 695=695、queries 138=138、末级名 diff=0)
- [x] P2 独立流水线模块(thumbnail/metadata/derivations/exotic 各一 commit:717bfb4/15301fb/cbac384/7e7d3a2;NOT_BLOCKED_BY_EXOTIC(_M) 双 const 落位 exotic.rs;阶段门 clippy -D warnings + 全量 test 绿)
- [x] P3 AI / 人脸(ai:2ef51ac,faces:a4d5f9e,2,132 行最大单迁;reset_error_items_batched 升 pub(in) 供 faces 引用;阶段门绿)
- [x] P4-1/P4-2 scan / media(scan:c2b89ad,含 invalidate_derived_for_item 事务 owner 留置 + recompute_person_aggregates 定向引用;media:f55e48c,含 D→C 迁移完整性核查中修复的两处遗漏——media.rs 缺 ImageMeta import、facade 头误删测试用 params 宏 import)
- [x] P4-3 layout(6ccea07:mapper+canonical SQL+三 builder+view_to_sql/selection+list_library_formats+四测试模组整体一次迁移;对 HEAD 原块机械 diff 仅五处登记改点=push_in_predicate 升 pub(in)×1/跨域测试定向 import×3/P4-2 遗留孤儿 doc 摘除×1;同 commit 顺改 D-012 search.rs 与 media.rs SELECTION_BATCH_CHUNK 两处过渡路径;'localtime' 债 F-007 登记随迁未修)
- [x] P5 文档、全门、smoke 与收口(8c7f7fb facade 终态 37 行=顶层文档+14 mod+14 pub use;三 manifest 终拍零 diff:API 232=232/consumers 42=42/tests 总量 695=695+queries 末级名 138=138+重复 0;最终 Rust 四门+docs 两门全绿;release 编译烟测 EXIT=0(3m16s,release/scrollery.exe 产出);运行时 smoke ⏸GUI 真机;现行文档更新=T 设计状态头+S 设计 F-007 指路+todo.md T 节;三件套留 planning 不归档)
- **状态:** completed(2026-07-16 收官;§8.4 运行时 smoke 待 GUI 真机,余项全清)

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 保留 `queries.rs` 为 facade，子模块放 `queries/*.rs` | 维持公开路径与文件入口，避免 P0 consumer manifest 中的调用文件同步迁移 | D-001 |
| 第一轮按 14 个 bounded context 拆 | 6 个粗模块仍会留下巨石，20+ 微模块会制造 helper 网络 | D-002 |
| 搬迁提交不夹带 SQL、签名、visibility 或行为修改 | 保证 diff 可审、失败可定位、领域 commit 可独立回滚 | D-003 |
| P0 完整 public API manifest 全部 re-export | 本轮只做结构拆分；函数与公开类型/常量都不得遗漏 | D-004 |
| 第一轮不二拆 face/layout | 先保持事务、aggregate 与 SQL builder 内聚，稳定后再按实际冲突评估 | D-005 |
| 现行/normative 文档更新新路径，历史快照冻结 | 保证现行指引可用且不破坏存证 | D-006 |
| 分 commit focused gate，阶段/full workspace gate | 在控制反馈成本的同时覆盖 P0 consumer manifest | D-007 |
| `duplicate_media_item_into_dir` 归 `media.rs` | 核心语义是 media_items 列级复制，A3 回归钉测试同迁 | A1 |
| `delete_media_item_hard` 归 `faces.rs` | 事务内调 `recompute_person_aggregates`，被保护不变量=person 聚合一致性，归此可保 helper 模块私有 | A1 |
| `reset_error_items_batched` 归 `ai.rs`，faces 定向引用 | ai/faces 共用 helper 归最强 owner，可见性 `pub(in crate::db::queries)` | A2 |
| P1 末加全量 workspace test + `--list` 清单对拍 | 首个全量 test 门原在 P2 末，P1 五个 commit 缺"测试静默消失"兜底 | A4 |
| 立项数字冻结为 `33e5846` 快照，P0 按开工 HEAD 重建完整 manifest | `f3498dd` 已证明设计 Review 到开工之间会发生真实漂移 | D-008 |
| `map_child_directory_ids`、`map_media_entities` 同归 `scan.rs` | 两者服务 FS 文件树实体关联，需与 directory/list 可见项谓词保持内聚 | D-009 |
| S-P3/P4 完成后再启 T-P0；T 实施期冻结旧 facade 写入 | S-P3 已明确会改 layout/search 同一区域，显式 stage 不能解决同文件并发 | D-010 |
| test list 使用 workspace locked 命令并归一化 queries 模块路径 | Rust harness 输出完整路径，直接 diff 会把合法 owner 前缀变化误报为增删 | D-011 |
| search.rs 保留 P1,先引用 facade helper;P4 迁 layout owner 时顺改一处路径 | 保留低耦合练手顺序,同时用显式过渡路径解决消费者先于 owner 迁移;改点符合 D-003 允许的必要局部路径修改 | D-012 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 首次 `git numstat` 汇总用 `Measure-Object` 输出未得到字段名 | 直接枚举属性，结果只显示空键 | 改用 PowerShell 累加器解析 numstat，得到 added=4539、deleted=477 |
| `todo.md` 读取输出过长被截断 | 一次性输出 headings + 末尾 160 行 | 后续只按 S 节和尾部插入点定向读取/修改 |
| PowerShell `Measure-Object -Line` 测得 9,174 行与文档 10,804 矛盾 | 直接引用该值会误判取证造假 | 该 cmdlet 只数非空行；总行数用 `wc -l`（=10,804，bytes 逐值吻合佐证）。复测须同口径 |
| Edit 全角标点老不匹配：Read 显示层把全角逗号渲染成半角 | 按 Read 输出照抄 old_string 失败 | 用 `node -e JSON.stringify(line)` 导出真实字节再拼 old_string；本会话实测工具管道不再转全角（printf 字节原样），失配纯因显示层误导 |
| PowerShell `(Get-Content).Count` 测 queries.rs 得 10,206 行,与 wc -l 的 11,228 矛盾(bytes 两边逐值同=同一文件) | 直接引用 .Count 会误报基线漂移 | 第二种量具偏差(前一行是 Measure-Object -Line 只数非空行);T 线行数统一 `wc -l` 口径,任何复测须同量具 |
| P0 `cargo test 2>&1` 接管道 Select-String 后 `$LASTEXITCODE=-1`,而所有 test result 行全 ok | 误判基线红 | PS 5.1 对 native exe 重定向 stderr 会污染状态(工具说明已警告);重跑 `cargo test -q *> log` 取 `EXIT=0` 干净证据 |
| P4-2 施工中 D 盘(项目原所在盘)NVMe 掉线,rustc/git 相继 `STATUS_IN_PAGE_ERROR`/`fsync error` | 停工等重启复原 | 重启后工作树未丢失(media.rs 搬迁内容仍在);验证时补出两处编译遗漏(ImageMeta import、误清的 params 宏 import)后与迁移一并提交;后续项目主仓整体迁至 C 盘(fdead51),`git fsck` 全绿 |
| 施工期两会话(P4-1 尾声与 P4-2/迁移)未同步更新 task_plan/progress 三件套 checkbox 与会话记录 | 三件套一度滞后于 git log 达两个 commit | 新会话开工前先 `git log --oneline` 核对实际落地 commit,再补记缺失会话段;本次已按此方法补齐(见上两条会话记录) |
