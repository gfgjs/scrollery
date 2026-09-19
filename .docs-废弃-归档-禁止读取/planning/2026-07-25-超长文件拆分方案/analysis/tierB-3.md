---
id: 2026-07-25-tierB-3
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# tierB-3 简案:media_foundation.rs / schema.rs / thumbnail_commands.rs / derivations.rs

> 锚点以符号名为主、行号为辅(并行注释精简会使行号漂移)。纯结构移动立场,不建议行为改动。

## src-tauri/src/video/media_foundation.rs(56KB)

**① 拆(仅安全尾段;核心状态机不动)**——文件大头是 `Session`/`CallbackShared`/`ReaderCallback` 构成的 MF 异步回调强耦合 unsafe 核心,恰是 3 天前(2026-07-22)刚合入的挂死根治红线保护区,此时挪动收益(减行数)覆盖不了引入回归的风险;但文件尾部两段是零 unsafe 耦合的纯函数,可安全抽出。

**② 主刀口:**
- 迁出「属性/编解码器辅助」域:`attr_size` / `attr_ratio` / `read_duration_ms` / `codec_label` / `normalize_rotation` → 新文件(如 `mf_attrs.rs`);纯 GUID/PROPVARIANT 解析,与 `Session` 无引用关系。
- 迁出「帧后处理」域:`apply_rotation` / `resize_rgba` / `is_too_dark` / `copy_bgr32_to_rgba`(含其 4 个尾像素/bottom-up/padding/截断单测)→ 新文件(如 `frame_post.rs`);全程不碰 `IMFSourceReader`。
- `MediaFoundationBackend` 的 `VideoBackend` 实现(`probe`/`cover`/`keyframes`)、`Session`/`CallbackShared`/`ReaderCallback`/`open_reader`/`configure_rgb32`/`read_frame_at` 这一整块**原地不动**——这是红线保护的核心,本轮不碰。

**③ 最大风险:** `#[cfg(test)] mod tests` 同时经 `super::` 引用了核心区(`open_reader`/`configure_rgb32`/`pin_no_xvp_rotation`)与尾段(`copy_bgr32_to_rgba`/`request_size`/`sprite_cell`)两侧的私有符号,一旦尾段搬家改了可见性,测试模块的引用路径必须同步调整,漏一处即编译红。

---

## src-tauri/src/db/schema.rs(55KB)

**① 拆(机械化,四文件中风险最低)**——23 个 `SCHEMA_Vn` 都是自包含字符串常量,真正的迁移顺序由 `migration.rs` 内显式的 `STEPS` 数组(`(版本号, 常量, 描述)` 三元组)驱动,不依赖 `schema.rs` 内的物理排列顺序,天然适合按版本区间切文件。

**② 主刀口:**
- `schema.rs` → `schema/mod.rs`(壳,`pub use` 全量重导出)+ 按版本纪元分 2–3 个子文件,例如 `early.rs`(`SCHEMA_V1..V9`:基础表 + exotic Part1)、`mid.rs`(`SCHEMA_V10..V17`:卷可用性 + 阅读器)、`late.rs`(`SCHEMA_V18..V23`:近期小增量),常量原样搬、一字不改。
- `migration.rs` 顶部 `use crate::db::schema::{SCHEMA_V1, ..., SCHEMA_V23}` 这一条 flat import 不必动——`mod.rs` 重导出后外部路径 `crate::db::schema::SCHEMA_Vn` 保持不变。
- 拆分断点只能落在**版本号之间**,不得把同一 `SCHEMA_Vn` 常量与其头顶的大段设计动机注释(如 V10/V19 的「同事务 DML 回填」说明)劈成两半。

**③ 最大风险:** `STEPS` 数组 23 项与新增子文件常量是否一一对应(不漏不重)——`cargo check` 可兜底捕获缺失导入,但需人工核对一次计数。

---

## src-tauri/src/ipc/thumbnail_commands.rs(55KB)

**① 拆(按"批量视口路径" vs "全库生成路径" 两条流水线切分)**——文件事实上是两条几乎独立的多阶段流水线:`batch_request_thumbnails` 服务视口按需生成(含内联 exotic 路由),`run_thumbnail_generation` 服务全库生成 + Phase2 CPU 兜底;二者除共享 `flush_thumb_results` 外几乎不耦合。

**② 主刀口:**
- 迁出「全库生成」域:`run_thumbnail_generation` + `FullThumbProgressPayload` + `publish_thumb_progress` + `full_thumb_gen_status` + `start_full_thumbnail_generation` / `start_incremental_thumbnail_generation` + `stop_full_thumbnail_generation` → 新文件(如 `thumbnail_full_gen.rs`,约 450 行)。
- `batch_request_thumbnails`(含内联 exotic 路由 `spawn_blocking` 段)+ `cancel_thumbnail_request` 留在 `thumbnail_commands.rs` 作视口批量路径;`regenerate_missing_thumb` / `clear_all_thumbnails` 可选一并留下或归入第三个小文件。
- `flush_thumb_results` 作两侧共享助手,需提升可见性(`pub(crate)`)并放在两侧都能 `use` 的位置。

**③ 最大风险:** `ipc/registry.rs` 的 `generate_handler!` 按 `ipc::thumbnail_commands::X` 全路径注册 8 个命令,`state.rs` 也直接引用 `ipc::thumbnail_commands::FullThumbProgressPayload` 类型路径——若把函数/类型物理挪到新模块,这两处调用点必须同步改路径(或在 `thumbnail_commands.rs` 留 `pub use` 转发以保持外部路径不变),漏改即整仓编译失败。

---

## src-tauri/src/db/queries/derivations.rs(54KB)

**① 拆(测试代码物理分离,四文件中最干净的分离面)**——全文件 1129 行中生产代码仅约前 525 行,后 600 余行(53%)是 6 个已良好命名的 `#[cfg(test)] mod ..._tests` 块,纯测试、零生产逻辑耦合。已核实 `db/queries.rs` 顶部为 `mod derivations;`(单文件模块,非目录),转目录模块语法不受影响。

**② 主刀口:**
- 生产代码(`get_pending_derivations` … `count_derivations_by_status_for_kinds` 共 18 个函数/常量)留在原文件,`derivations.rs` → `derivations/mod.rs`。
- 6 个测试子模块(`reset_derivations_tests` / `poison_guard_tests` / `kind_filter_tests` / `video_playable_tests` / `hidden_root_derivation_tests` / `doc_thumb_guard_tests`)整体迁到同目录 `derivations/tests.rs`,`mod.rs` 内加一行 `#[cfg(test)] mod tests;`;因 `tests.rs` 仍是 `derivations` 模块的直接子模块,嵌套深度与现状一致,`use super::*` 与 `hidden_root_derivation_tests` 内的 `use super::super::scan::set_scan_root_hidden` 均无需改层数。
- `db/queries.rs` 的 `mod derivations;` 声明本身不用改。

**③ 最大风险:** 6 个测试子模块内部散落多份几乎相同的 `seeded()` / `one_video()` / `status_of()` 治具函数(各自私有、互不共享)——物理搬家时若图省事顺手去重合并,会越界成"行为改动"(触碰测试语义),本轮应逐块整体平移、不做治具去重。

---

## 顺手发现

无。
