---
status: snapshot
type: working-memory
line: queries模块拆分方案
created: 2026-07-16
---

# 进度日志:queries模块拆分方案

## 会话:2026-07-16

- 做了:只读盘点 `queries.rs` 的规模、公开面、测试占比、领域分布、39 个消费文件、近期 Git churn、CI 门和既有 R21 审查记录。
- 做了:形成 `queries.rs` 稳定 facade + `queries/*.rs` 14 个领域模块的目标架构，明确 API/行为/测试不变量、P0–P5 迁移顺序、风险、回滚与 Done 定义。
- 做了:建立 T 线正式设计件与 planning 三件套；未修改 Rust 业务代码。
- 验证:使用 `rg`/PowerShell 对当前源码取数；`file!`/`module_path!`/`include_*` 在 `queries.rs` 零命中；`node tools/check_docs.mjs` 与 `node tools/check_docs_index.mjs` 均 exit 0。
- 遗留:等待用户 Review 设计文档 D-001～D-007；获批前不开始 P0/P1 代码施工。

## 会话:2026-07-16（Review 复核与裁决回写）

- 做了:对设计全部量化声称做只读复核（行数/字节/公开面/测试/39 消费文件/churn/零路径宏逐值实证），并补结构性核查（mapper 调用面全同域、测试模块自包含、跨域测试只碰 pub 项、零 `macro_rules!`）。
- 做了:发现三个未显式归属/跨域符号（`duplicate_media_item_into_dir`、`delete_media_item_hard`、`reset_error_items_batched`）与两处流程缺口（P1 无全量测试门、layout 已知债需登记）。
- 做了:用户裁决"采纳全部"后，把 D-001～D-007 状态与 A1～A5 修订回写设计正文（§3.3/§4.1/新增 §4.3/§6/§9/§11），更新三件套与 `docs/todo.md` T 节。
- 验证:`node tools/check_docs.mjs` 与 `node tools/check_docs_index.mjs` exit 0（本次提交前复跑）；全程零 Rust 代码改动。
- 遗留:P0 起于新会话（用户明示本会话不开工）；施工时注意工作树现存并行修改（`src-tauri/src/tree/`、`lib.rs`），只 stage 本线显式路径。

## 会话:2026-07-16（二次核实建议采纳回写）

- 做了:核实首轮 Review 后的当前源码，确认 `f3498dd` 已使基线从 10804/211/39 漂移到 10885/213/41，并发现两个新增 FS 树 DAO 未分配 owner、S-P3 与 T-P1/P4 将修改同一 search/layout 区域。
- 做了:用户采纳全部建议后，将 D-008～D-011 回写设计正文：旧数字绑定 `33e5846` 快照、P0 改为完整 manifest、两个新增 DAO 归 `scan.rs`、S→T 串行硬门、test list 路径归一化与 symbol+commit 定位。
- 做了:同步 task_plan、findings 与 `docs/todo.md` T 节；未修改 Rust 代码。
- 验证:`cargo test -p scrollery --lib --locked -- --list` exit 0（588 tests，`db::queries` 125，末级名重复数 0）；`node tools/check_docs.mjs`、`node tools/check_docs_index.mjs`、`git diff --check` 均 exit 0。
- 遗留:等待 S 线 P3/P4 完成；届时才启动 T-P0，并按开工 HEAD 重新生成 public API/test/consumer manifest。

## 会话:2026-07-16（三次核实 D-012 裁决回写）

- 做了:U 线时效性核查发现 `push_in_predicate` 跨 layout/search 共用,推翻「search.rs 是叶子」前提;代码审查确认方案 a 可行。
- 做了:用户采纳 D-012 方案 a,已回写 T 设计 §6/§11、task_plan、findings 与 `docs/todo.md`;P1 保留 search.rs,P4 迁 layout owner 时顺改一处路径。
- 边界:本轮只修改文档,未启动 T-P0、未修改 Rust。
- 遗留:另起施工会话后按开工 HEAD 重建完整 manifest并冻结 `queries.rs` 写入窗口;S 真机验收若触发返修则停止并重建基线。

## 会话:2026-07-16(T-P0 基线冻结,施工会话)

- 做了:开工 HEAD `ae69565`(工作树干净)。重测 queries.rs=11,228 行(wc)/503,724 bytes,**与三次核实快照 `13a3192` 逐字节相同=零漂移**;214 pub fn/30 私有 fn/18 公开类型/138 `#[test]`/22 test mod/1 `#[ignore]`(canonical_plan_tests 内)。manifest 三件落 `p0-baseline/`:api_manifest.txt(232 项)、consumers.txt(42 文件,+`ai/runtime_config.rs` 系 U-P2 当日新增)、tests_full.txt(695)+tests_queries_lastnames.txt(138,末级名重复 0)。路径宏(`include_*`/`file!`/`module_path!`/`macro_rules!`)零命中复证。基线 `cargo test --workspace --locked` EXIT=0(主 lib 608/0/5)。`queries.rs` 写入窗口自此冻结。
- 做了:施工期跨域符号全量摸排(rg 逐 helper 调用面),7 项 §4.3 未覆盖归属按「谁的不变量被保护」判据裁定并回写设计 §4.3/§6:①`NOT_BLOCKED_BY_EXOTIC(_M)` 归 exotic.rs(与 `has_blocking_exotic_thumbnail_task` 同谓词孪生,横跨 thumbnail/derivations/ai/faces 四消费域,P2-4 统一顺改);②`SELECTION_BATCH_CHUNK` 归 layout.rs(media 两处过渡引用,P4-3 顺改);③`get_placeholder_item_path` 归 metadata.rs(0×0 占位尺寸提取);④`get_item_file_format` 归 documents.rs(唯一消费者 doc_subtype);⑤`x1_conditional_finish_tests` 归 faces.rs;⑥`r2_6_query_tests` 跨五域按测试函数拆归 owner(fixture 就地复制,U-P3 先例);⑦域内私有 helper(EXCLUDE_* 三 const/COVER_DERIV_KINDS/VERSION_COLS/EXOTIC_TASK_COLS/batch_update_set_value/search_join_needs 等)确认无跨域随迁。
- 验证:上述每条有命令级证据;三处「疑似跨域」排除=行 390/4585 是注释引用、intra-doc link 随迁自愈。
- 遗留:P1 起施工;错误账新增两行(Get-Content .Count 量具偏差、PS 5.1 管道污染退出码)。

## 会话:2026-07-16(T-P1 五叶子迁移)

- 做了:五 commit 依序落地——config(e95a9a3,建 queries/ 目录+facade 首组 mod/pub use)、collections(363b6c6)、documents(5537bd3,三段:replacements/versions/reader + document_meta + 测试,get_item_file_format 按 P0 裁决随迁)、storage(5516f2e,backend+卷+map_volume+volume_dao_tests)、search(39607ce,D-012 方案 a `super::push_in_predicate` 过渡引用 + r2_6 R19 测试随迁同名子 mod)。
- 施工发现:①volume_dao_tests 用 scan 域五 pub 符号(insert_scan_root 等),补过渡定向 import `use super::super::{…}`,P4-1 scan 落位顺改 owner 路径;②facade 头 `use crate::db::models::{…}` 随域迁出累积 unused import,**不能等 P5**——P2 末 clippy -D warnings 会拦,已改为每 commit 顺清(P1-4 清 6 项、P1-5 清 SearchResult),属 §7 允许的「必要局部 import 修改」。
- 验证(P1 出口门):`cargo test --workspace --locked` EXIT=0;`-- --list` 归一化对拍 P0 manifest:总量 695=695、queries 138=138、末级名 diff=0、非 queries 完整路径 diff=0(零测试静默消失);每 commit fmt+check+db::queries 138 测绿。全部本地非 CI。
- 遗留:P2 施工(thumbnail→metadata→derivations→exotic);NOT_BLOCKED 双 const 过渡引用与 P2-4 顺改照 P0 增补裁决。

## 会话:2026-07-16(T-P2 四流水线迁移)

- 做了:四 commit——thumbnail(717bfb4,EXCLUDE_* 两 const+COVER_DERIV_KINDS 域内随迁,cover_thumb_pipeline_tests 实测已无 backfill_derivations 依赖零过渡)、metadata(15301fb,get_placeholder_item_path 按 P0 裁决归本域)、derivations(cbac384,get_item_cache_key 留 facade 待 media)、exotic(7e7d3a2,**NOT_BLOCKED_BY_EXOTIC(_M) 双 const 落位 pub(in crate::db::queries)**,thumbnail/derivations 过渡 use 同 commit 顺改 super::exotic::,facade ai/faces 段裸名经 glob re-export 解析)。
- 施工发现:①derivations 首编译缺 Row import(E0425,cargo check 不编测试的老坑之外的新形态:主体代码就缺);②exotic_dao_tests 直调 thumbnail 四函数+ai/faces 两计数——前者 owner 已就位改直引,后者过渡引用 facade 待 P3 顺改;③facade 头 unused import 随每 commit 清(ExoticTaskRow/ExoticTaskStatus/AudioMeta)。
- 验证(P2 阶段门):`cargo clippy --workspace --locked -- -D warnings` EXIT=0;`cargo test --workspace --locked` EXIT=0;每 commit fmt+check+db::queries 138 测绿。全部本地非 CI。
- 遗留:P3(ai→faces);reset_error_items_batched 迁 ai 时升 pub(in) 供 facade faces 段裸名过渡。

## 会话:2026-07-16(T-P3 ai/faces 迁移)

- 做了:ai(2ef51ac,reset_error_items_batched 升 pub(in) 供 faces 定向引用;r2_6 两 ai 测试随迁;exotic_dao_tests 的 ai 计数顺改 owner 直引)、faces(a4d5f9e,2,132 行最大单迁:face 状态机+persons+审批+聚类+重聚类+delete_media_item_hard;x1 归 faces 且 ai 半边 super::super::ai:: 直引;face_approval/f2_f3/r2_6 四面部测试随迁)。
- 施工发现:**invalidate_derived_for_item(scan 域,暂驻 facade)调用 faces 私有 recompute_person_aggregates**——P0 摸排漏网(rg 只查了预列 helper 清单,没有反向查「facade 剩余代码引用已迁私有符号」);按 §3.3 升 pub(in crate::db::queries),facade 裸名经 glob re-export 过渡解析,P4-1 scan 落位时顺改显式路径。
- 验证(P3 阶段门):clippy --workspace -D warnings EXIT=0;cargo test --workspace --locked EXIT=0;每 commit fmt+check+db::queries 138 测绿。本地非 CI。
- 遗留:P4(scan→media→layout);顺改台账=①storage volume_dao_tests 五 scan 符号(P4-1)②facade recompute 裸名(P4-1)③media SELECTION_BATCH_CHUNK(P4-3)④search push_in_predicate(P4-3,D-012)⑤exotic_dao_tests faces 计数(P3-2 已改?)——核:count_pending_face_items 已随 faces 落位,exotic_dao_tests 过渡引用须顺改。

## 会话:2026-07-16(T-P4-1 scan 迁移)

- 做了:scan(c2b89ad)——scan root/目录树/fast scan 写入/source-change 失效/mark_missing + map_scan_root/map_dir_node + 三测试模组(mark_missing/fast_scan_upsert_recovery/scan_root_backend)+ r2_6 目录树测试纯搬迁;`invalidate_derived_for_item` 留在 source-change 事务 owner(§4.1),其 `recompute_person_aggregates` 调用改 `super::faces::` 定向引用。`list_library_formats` 留 facade 待 P4-3 归 layout,`duplicate_media_item_*` 留待 P4-2 归 media。
- 顺改台账清结:storage `volume_dao_tests` 五 scan 符号改 `super::super::scan::`;exotic_dao_tests faces 计数改 `super::super::faces::`(P3-2 落位遗留)。facade 头清 unused import 四项。
- 施工插曲:r2_6 目录测试首刀切分范围因前批行号偏移误切 layout 对拍测试(`date_filename_derive_matches_sql_order`)29 行,即时原位回插并复核上下文后重切,未产生遗留问题(见错误账后续可补一行)。
- 验证:fmt+check+db::queries 138 测全过。
- 遗留:P4-2 media;P4-3 layout(D-012 与 SELECTION_BATCH_CHUNK 两处顺改路径)。

## 会话:2026-07-16(T-P4-2 media 迁移 + 主仓 D→C 盘迁移)

- 背景:本阶段施工期间宿主机 D 盘(项目原所在盘)NVMe 硬件不稳定掉盘,导致 media.rs 搬迁工作在提交前中断;后续会话(迁移会话)接续:先完成 media.rs 提交,随即将项目主仓整体迁移至 C 盘并做完整性核验。
- 做了:media(f55e48c)——媒体项域 DAO 迁出 `queries/media.rs`:读取/detail、批量与单项用户属性(收藏/评分/颜色)、companion、回收站、统计、路径信息与 cache_key、拖拽复制(`duplicate_media_item_into_dir` 按 P0 裁决归本域)。`SELECTION_BATCH_CHUNK` 过渡引用暂驻 facade,待 P4-3 layout 落位顺改。
- 做了:迁移会话验证 target/ 完整性时顺带修复两处遗漏(与本 commit 同批,不算夹带行为修改——均为搬迁本身缺失的编译前提):media.rs 缺 `ImageMeta` import(E0422);facade 头误删仅测试用的 `params` 宏 import(`#[cfg(test)]` 不参与 `cargo build`,故先前被误判 unused 而清除)。
- 做了:项目主仓从 D 盘迁移至 C 盘(fdead51,原因=D 盘 NVMe 硬件不稳定掉盘,与项目仓库内容无关):更新残留字面路径引用(开发者项目手册路径列、两个人工对拍脚本默认路径);全仓 D:/d: 硬编码复扫确认 git 远程/Cargo·npm·Tauri 身份字段不依赖目录路径,其余命中均为测试假数据或终端用户 UI 占位符文本。
- 验证:迁移后 `git fsck --full --strict` 无 corrupt/missing;前端 vitest 87 文件/1123 测试绿;Rust build+clippy -D warnings+fmt+test --workspace --locked 四门绿(主 lib 608/0/5)。media 域本身:cargo build/clippy/fmt/test 四门绿。均本地验证,非 CI。
- 遗留:**P4-3 layout(最后、最高风险块)+ P5 收尾**;task_plan/progress 两件套在本迁移会话中滞后未同步更新(本条为补记),已于下一会话核对 git log 补齐。

## 会话:2026-07-16(T-P4-3 layout 迁移 + P5 收尾,T 线收官)

- 开工核对:git log 与三件套记录一致(P0～P4-2 共 16 commit + 2 docs 补记,零漂移)。
- 做了:P4-3(6ccea07)——facade 行 47–2503 整体一次迁 queries/layout.rs(map_layout_item+canonical SQL 族+三 builder+view_to_sql+selection 两函数两常量+list_library_formats+query_dir_labels+四测试模组)。对 git HEAD 原块与新文件身体做机械 diff 证纯搬迁,仅五处登记改点:①push_in_predicate 升 pub(in crate::db::queries)(§4.3,search 定向引用)②③④跨域测试定向 import(view_to_sql_tests→collections 六项+faces 两项;selection_resolve_tests→media 五项;format_facet_tests→search_media;§5 规则 2,裸名原经 facade glob 解析)⑤P4-2 遗留孤儿 doc 摘除。同 commit 顺改两处过渡引用:search.rs `super::push_in_predicate`→`super::layout::`(D-012 方案 a 收尾)、media.rs `super::SELECTION_BATCH_CHUNK`→`super::layout::`;`'localtime'` 时区债(B-file-iii F-007)在 commit 说明登记随迁未修。
- 施工发现:P4-2 迁 companion_expand_tests 时其模组 doc 注释(`/// T18 S3…`)留守 facade 成孤儿(切割边界从 `#[cfg(test)]` 起,漏了前置 doc)——不处置则 facet 模组迁出后孤儿悬空文件尾=编译错;已摘除归位 media.rs 该模组头(纯注释,commit 说明登记)。教训登记 findings F-010。
- 做了:P5——facade 重复文件头清理(8c7f7fb,终态 37 行,零业务 SQL);现行文档更新(T 设计状态头收官、S 设计 F-007 指路改 `db/queries/layout.rs`、todo.md T 节 ✅);三件套回写并按用户指示留 docs/planning/ 不归档。
- 验证:每步门全绿(P4-3 后 fmt --check/check/clippy -D warnings/test --workspace --locked;最终门复跑同绿;主 lib 608 过/5 ignore,db::queries 138=P0 口径,行数 wc -l)。manifest 终拍零 diff:API 232=232、consumers 42=42、tests 总量 695=695、queries 末级名 138=138 归一化对拍、末级名重复 0。release 编译烟测 EXIT=0(3m16s,release/scrollery.exe 产出)。docs 两门 exit 0。均本地非 CI。
- 遗留:§8.4 运行时 smoke=⏸GUI 真机(启动配置/扫描目录/画廊排序筛选 selection/favorite-rating-color 往返/缩略图派生回写/文档合集/AI 人脸只读/storage-exotic 列表);本线全部 commit 未 push(推送需用户批准)。

## 回顾(收口时填)

- 亮点:先把“行数过大”的直觉问题转成领域耦合、公开面、测试、churn 和历史缺陷五类可验证证据，再设计边界。
- 教训:测试占四成不能作为“不必拆”的理由；排除测试后仍有六千余行且多领域混杂。
- 意外:六天内文件从既有审查记录的 9133 行增长到 10804 行，说明该债正在持续放大，不是静态历史包袱。
- 收口补记:纯搬迁的可信度来自机械证明(HEAD 原块 vs 新文件 diff 仅登记改点)而非自述;manifest 三件终拍与 P0 逐值对拍是「无未解释差异」的唯一证据形态。孤儿 doc 一类的搬迁遗留,靠「搬完再读一遍接缝上下文」纪律兜底。
