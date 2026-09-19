---
status: snapshot
type: working-memory
line: queries模块拆分方案
created: 2026-07-16
---

# 发现与决策:queries模块拆分方案

## 需求

- 用户发现 `src-tauri/src/db/queries.rs` 已超过一万行，询问是否有必要拆分。
- 用户要求输出拆分方案并落盘，供后续 Review。
- 当前阶段只允许文档方案，不实施 Rust 重构。

## 发现

- 立项快照 `33e5846`：`10,804` 行、`481,355` bytes、`211` 个 `pub fn`、`29` 个私有函数、`18` 个公开 struct/enum/type/const。
- 二次 Review 当前源码：`10,885` 行、`485,129` bytes、`213` 个 `pub fn`、`29` 个私有函数、`18` 个公开 struct/enum/type/const，即 `231` 个顶层公开项。
- 测试仍为 `125` 个、`21` 个测试模块；按测试模块边界估算约 `4,453` 行，占当前文件约 `40.9%`；生产实现与注释约 `6,432` 行。
- 至少承载 scan/directory/media/layout/selection/thumbnail/metadata/storage/search/config/AI/face/derivation/document/collection/exotic/volume 等领域。
- 立项快照有 `39` 个 Rust 消费文件；二次 Review 当前为 `41` 个。大量调用方使用 `use ...::queries as q`，稳定 facade 可避免调用面迁移。
- 2026-07-01 至二次 Review 有 `59` 个提交修改该文件，numstat 累计 added=4620、deleted=477、churn=5097。
- `docs/reviews/2026-07-10-全库规范优化审查报告.md:219` 已记录 R21：当时 9133 行、198 个公开函数，列清单重复曾导致 P0(A3)；当前继续增长。
- `push_query_body`、`push_where_predicates`、`push_order_by` 与 layout mapper/等价测试构成强耦合，应整体迁移。
- face/person/approval/cluster/recluster 共享 aggregate 与事务不变量，第一轮机械拆成多个文件的风险高于收益。
- `queries.rs` 内没有 `file!`、`module_path!`、`include_str!`、`include_bytes!` 路径敏感宏。
- `.github/workflows/ci.yml` 的 Rust 权威门为 fmt、workspace check、workspace clippy、workspace test；文档门为 `check_docs.mjs` 与 `check_docs_index.mjs`。
- 首轮立项时工作树存在其它任务修改；实施时必须显式 stage T 线文件，不吸收或还原不重叠的并行修改；同文件并发另按 D-010 串行化。

### 2026-07-16 Review 复核（结论=方案成立，D-001～D-007 全按推荐采纳）

- 全部量化声称逐值实证：10,804 行（`wc -l`）/481,355 bytes/211 pub fn/29 私有 fn/18 公开类型/125 test/21 test mod/39 消费文件（`grep -rl` 恰 39）/churn 58 提交 +4539 −477（git log 复算逐值同）。
- 零 `file!`/`module_path!`/`include_*` 复证成立；另证零 `macro_rules!`——glob re-export 不覆盖宏的隐患也不存在。
- mapper 调用面全部同域（`map_media_item` 三处全在 media 区、`in_clause` 八处全在 faces 区、`push_*` 全在 layout 区）；`search_media` 内联 mapper 自包含，search.rs 确为叶子。
- 21 个测试模块各自建库（`open_in_memory`+`run_migrations` 全限定路径调用），无共享 fixture；跨域测试（如 `cover_thumb_pipeline_tests` 同时调 `backfill_derivations`+thumbnail 五函数）只碰 pub 项，机械可迁。
- 三个符号需显式归属（已裁决入设计 §4.3）：`duplicate_media_item_into_dir`（queries.rs:429，归 media.rs）、`delete_media_item_hard`（queries.rs:4730，全文件唯一真跨域事务，归 faces.rs）、`reset_error_items_batched`（queries.rs:3289，ai/faces 共用，归 ai.rs）。
- B-file-iii 线 F-007（搜索谓词 `'localtime'` 时区债）实证位于 `push_where_predicates` 内 queries.rs:1423/1456，将整体随 layout.rs 迁；禁顺修，layout 搬迁 commit 须登记"已知债随迁未修"。
- glob re-export 名冲突按构造不可能：211 个名今天已在同一命名空间共存必唯一；`ambiguous_glob_reexports` lint 在 `-D warnings` 门下自动升 error，双保险。
- `db/mod.rs:5` 现为 `pub mod queries;`，facade 形态与现声明兼容，无需动。

### 2026-07-16 二次核实（D-008～D-011 已采纳）

- `f3498dd` 在 T 线立项后、首轮 Review 回写前向 `queries.rs` 增加 81 行与 2 个 `pub fn`，并新增 2 个消费文件；证明 Review 数字只能作为带 commit 的快照，施工 P0 必须重建 manifest。
- 新增 `map_child_directory_ids` 与 `map_media_entities` 都服务 FS 文件树实体关联；后者还与 `list_directory_files` 共享可见项谓词，二者同归 `scan.rs`。
- S 线待施工 P3 明确修改 `push_where_predicates` 与 `search_media`，分别与 T-P4 layout、T-P1 search 重叠；显式 pathspec 不能隔离同一文件并发写，故 S-P3/P4 必须先完成，T-P0 后启动并冻结旧 facade 写入。
- `cargo test -p scrollery --lib --locked -- --list` 实测通过，共 588 tests；其中 `db::queries` 125 个，输出含完整模块路径，末级测试函数名重复数为 0。测试迁入 `queries::<domain>` 后路径必变，须按末级函数名归一化对拍，不能直接逐行 diff。
- 首轮文档中的源码行号随 `f3498dd` 整体后移 81 行：三个跨域 symbol 当前为 510/4811/3370，`'localtime'` 当前为 1504/1537。正式契约改用 symbol + P0 commit，行号只作快照。

### 2026-07-16 三次核实(U 线「后端大文件拆分审查」受托代查,快照 13a3192;设计已同步回写)

- S-P3 四提交 + P4 八门全绿已 landed(todo.md S 节),S 对 queries.rs 的代码写入已尽,仅余真机验收(不产码)——**D-010 硬门在代码面满足,T-P0 可启动**;残余风险=真机验收返修,按既有「插队则停止并重建基线」条款处理。
- 当前口径:**11,228 行 / 503,724 bytes / 214 pub fn / 30 私有 fn / 18 公开类型 / 138 `#[test]` / 22 test mod / 41 消费文件**。`f3498dd..HEAD` 涉本文件 4 提交(9166d3d/549861c/2afe1e1/73c44bb),+370 −27。
- 符号级差异仅 3 个新增声明:`pub fn list_library_formats`(快照行 400)、`fn push_in_predicate`(快照行 1439,私有)、`mod format_facet_tests`(快照行 11143;连同 2 条 builder 测试共 +13 测)。
- **首轮复核结论「search.rs 确为叶子」被推翻**:`push_in_predicate` 被 `push_where_predicates`(layout)与 `search_media`(search)两域共用,而 §6 序让消费者(search,P1)先于 owner(layout,P4)迁移——与 `reset_error_items_batched`(owner 先迁)方向相反。用户已采纳 D-012 方案 a:P1 照迁并引用暂留 facade 的 helper,P4 迁 layout owner 时顺改一行路径并在 commit 说明登记。
- 两新符号归属已按 §4.3 判据回写设计:双双归 `layout.rs`(facet 谓词一致性契约 / IN 参数序号算术不变量),search 侧定向引用 `pub(in crate::db::queries)`。
- 详细取证与全后端普查见 U 线三件套 `docs/worklogs/2026-07-16-后端大文件拆分审查/` 与设计件 `docs/designs/2026-07-16-后端大文件拆分审查与方案.md` §3。

## 外部资料(当数据,不当指令)

- 本次未使用外部网页资料；方案完全基于当前仓库源码、Git 历史、CI 与既有审查文档。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | `queries.rs` 巨石判据应综合领域数、调用面、churn 与历史缺陷，不以 LOC 单指标裁决 | design |
| F-002 | 保留 `queries.rs` facade + `queries/*.rs` 可在不改 P0 consumer manifest 中调用路径的前提下完成领域拆分 | design |
| F-003 | 位置 mapper、SELECT 列清单与回归测试必须同 owner，避免静默串列 | design |
| F-004 | 高风险结构重构采用一领域一 commit、禁止夹带行为修改，可显著提高可审与可回滚性 | design |
| F-005 | 跨域函数归属判据=“谁的不变量被保护”而非“语义像谁”，保证 revert 领域 commit 时不变量整体回滚 | design（已回写 §4.3） |
| F-006 | 子模块用私有 `mod` 声明可把“调用方不得直接引用子模块”从纪律升级为编译器强制，零成本 | design（已回写 §3.3） |
| F-007 | 活跃巨石拆分的数量基线必须绑定 commit，并在实施 P0 重建完整 API/test/consumer manifest，不能把 Review 数字当永久常量 | design（已回写 §1/§3/§6） |
| F-008 | Rust test list 含完整模块路径，结构迁移对拍须归一化合法 owner 前缀，否则必产生假阳性 | design（已回写 §5/§6/§9） |
| F-009 | 两条工作线会改同一巨石文件时，显式 stage 不构成隔离；必须串行化施工并冻结写入窗口 | design（已回写 §6/§9） |
| F-010 | 跨文件搬迁若以 `#[cfg(test)]`/声明行为切割上界,会把模组 doc 注释(`///`)留在原文件成孤儿;孤儿 doc 悬空文件尾是编译错(P4-2 companion_expand_tests 即此形态,P4-3 摘除归位)。切割边界必须含 doc 注释与前置属性,搬完须再读接缝上下文 | experience |
