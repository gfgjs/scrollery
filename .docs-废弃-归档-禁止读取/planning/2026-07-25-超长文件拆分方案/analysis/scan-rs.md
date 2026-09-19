---
id: 2026-07-25-scan-rs
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# 拆分分析:src-tauri/src/db/queries/scan.rs

> 只读分析,未改任何代码。行号取自分析时刻的快照,并行注释精简会话可能已致漂移——
> 迁移执行时以符号名(函数名/类型名/mod 名)定位为准,行号仅作参考锚点。

## 0. 背景与结论先行

`scan.rs` 现状 1870 行(86KB),是 `docs/planning/.../超长文件拆分方案` 详案批次(阶段 3)10 件之一。
`src-tauri/src/db/queries.rs` 已是一个 facade:14 个领域子模块(`ai`/`collections`/`config`/
`derivations`/`documents`/`exotic`/`export`/`faces`/`layout`/`media`/`metadata`/`scan`/`search`/
`storage`/`thumbnail`)各一个 `.rs` 文件,`queries.rs` 用 `mod x; pub use x::*;` 逐个重导出,注释里
明确标注这是「T 线拆分」的既定模式(`queries.rs:7-8`)。

本方案是把这个模式**再下钻一层**:`scan.rs`(单文件)→ `scan/`(目录,`mod.rs` facade + 4 个域文件),
`queries.rs` 里的 `mod scan;` 一行不用动(Rust 对 `scan.rs` 与 `scan/mod.rs` 同等对待)。

## 1. 现状结构图

全文件按源码里已有的分区注释(`// ── XXX ──`)自然分三大域,外加两处独立辅助逻辑,尾部 5 个
`#[cfg(test)]` 模块占全文件近一半篇幅(约 806 / 1870 行,43%)。

| # | 职责 | 符号锚点(函数/类型/mod) | 约行数 | 源码区间(快照行号) |
|---|------|------------------------|--------|-------------------|
| 1 | 模块级私有 helper:行→结构体映射 | `map_scan_root`、`map_dir_node` | 36 | 13-48 |
| 2 | **扫描根(scan_roots)域**:CRUD + 后端绑定 + 隐藏根 + 卷绑定 + relink 改路径 + 抽样 | `insert_scan_root` / `set_scan_root_backend` / `set_scan_root_hidden` / `hidden_root_ids` / `EXCLUDE_HIDDEN_ROOTS[_M/_ITEMS]` 三常量 / `set_scan_root_volume` / `update_scan_root_path` / `RootSampleItem` / `sample_root_items` / `delete_scan_root` / `list_scan_roots` / `get_scan_root` / `update_scan_root_status` / `finish_scan_root` | ~198 | 50-248 |
| 3 | **目录(directories)域**:树/CRUD/祖先链/子树/移动辅助 | `upsert_directory` / `load_directory_mtime_snapshot` / `get_directory_tree` / `get_directory_children` / `list_directory_files` / `map_child_directory_ids` / `map_media_entities` / `set_directory_media_count` / `get_directory_ancestors` / `get_directory_abs_path` / `get_root_and_volume_for_directory` / `get_directory_descendant_ids` / `get_directory` / `dir_has_child_named` / `SubtreeDirRow` / `get_directory_subtree` / `SubtreeMediaRow` / `get_media_in_subtree` / `delete_directory_by_id` / `find_directory_id` | ~473 | 250-722 |
| 4 | **媒体项快速扫描 upsert 域**:含跨域调用 `faces::recompute_person_aggregates` | `FastScanItem` / `UpsertOutcome`(+`impl`) / `invalidate_derived_for_item` / `apply_source_changed`(私有) / `resolve_suspect_change` / `upsert_fast_scan_item` | ~272 | 727-998 |
| 5 | **缺失标记(mark_missing)**:三重守门差集,独立高密度契约注释块 | `mark_missing` | ~52 | 1000-1062 |
| 6 | 测试:mark_missing 六用例(三重守门矩阵/dry_run/多卷/复用连接/大集合/幂等) | `mod mark_missing_tests` | ~170 | 1064-1234 |
| 7 | 测试:fast-scan upsert 恢复/失效/可疑变更三环 | `mod fast_scan_upsert_recovery_tests` | ~425 | 1237-1661 |
| 8 | 测试:scan_root backend_id 绑定回读 | `mod scan_root_backend_tests` | ~38 | 1662-1699 |
| 9 | 测试:relink(#7 方案 A)改路径 + 抽样,**跨域**(同时用到 roots 与 directories 两组函数) | `mod relink_dao_tests` | ~90 | 1701-1790 |
| 10 | 测试:R2-6 目录树去相关化改写的行为锁 | `mod r2_6_query_tests` | ~78 | 1793-1870 |

结构性观察:
- 三大查询域(scan_roots / directories / media upsert)边界清晰,源码里已有分区注释佐证,零重叠字段。
- `mark_missing` 虽在源码里与 media upsert 同属 "Media items" 分区、无独立分区注释,但它是扫描收尾阶段
  单独调用的差集运算(`scanner::fast_scan.rs` 单独 `use`),契约注释(三重守门)与测试规模(170 行)都独立成套,
  值得单列一个小文件。
- 唯一跨域测试是 `relink_dao_tests`——它验证的是"relink 改路径"这个用户可见功能,天然需要
  `insert_scan_root`(roots)+ `upsert_directory`(directories)一起造 fixture,不是误分类。

## 2. 拆分方案

### 2.1 目标布局

```
src-tauri/src/db/queries/scan/
├── mod.rs            facade:mod 声明 + pub use 重导出 + 模块级 //! 文档注释迁入此处
├── roots.rs           scan_roots 域(含 scan_root_backend_tests + relink_dao_tests)
├── directories.rs      directories 域(含 r2_6_query_tests)
├── media_upsert.rs     FastScanItem/UpsertOutcome/upsert 链(含 fast_scan_upsert_recovery_tests)
└── mark_missing.rs    mark_missing(含 mark_missing_tests)
```

命名规避:仓库里已有同级兄弟文件 `queries/media.rs`(媒体项通用查询域),故子文件不用 `media.rs`,
改 `media_upsert.rs`,避免 `queries::media` 与 `queries::scan::media` 两个同名不同义模块混淆人眼。

预估行数(核心逻辑 + 随迁测试):

| 文件 | 核心逻辑 | 随迁测试 | 合计 |
|------|---------|---------|------|
| `mod.rs` | ~20(仅 mod+pub use+模块文档) | — | ~20 |
| `roots.rs` | ~198 | 38+90=128 | ~326 |
| `directories.rs` | ~473 | 78 | ~551 |
| `media_upsert.rs` | ~272 | 425 | ~697 |
| `mark_missing.rs` | ~52 | 170 | ~222 |
| 合计 | | | ~1816(≈原 1870,含分区注释/空行冗余) |

最大单文件从 1870 降到 ~697(media_upsert.rs),降幅 63%;四个域文件里最大与最小相差约 3×,
分布合理,均不逼近下一档"需要再拆"的阈值(对照仓内已有域文件 `derivations.rs` 1128 行、
`faces.rs` 2480 行、`layout.rs` 2736 行仍是单文件的现状,~700 行完全在可接受范围)。

### 2.2 迁移符号清单(按目标文件)

**`scan/mod.rs`**
- 模块级文档注释 `//! 扫描域 DAO:...`(原 1-4 行)整体迁入,作为目录模块的文档。
- `mod roots; mod directories; mod media_upsert; mod mark_missing;`
- `pub use roots::*; pub use directories::*; pub use media_upsert::*; pub use mark_missing::*;`
- 不放任何函数体——纯 facade,与 `queries.rs` 本身的角色对称。

**`scan/roots.rs`**
- `map_scan_root`(私有)、`insert_scan_root`、`set_scan_root_backend`、`set_scan_root_hidden`、
  `hidden_root_ids`、`EXCLUDE_HIDDEN_ROOTS`/`EXCLUDE_HIDDEN_ROOTS_M`/`EXCLUDE_HIDDEN_ROOT_ITEMS`
  三常量、`set_scan_root_volume`、`update_scan_root_path`、`RootSampleItem`、`sample_root_items`、
  `delete_scan_root`、`list_scan_roots`、`get_scan_root`、`update_scan_root_status`、`finish_scan_root`。
- 随迁测试:`mod scan_root_backend_tests`、`mod relink_dao_tests`。
- 本文件专属 `use`:`rusqlite::{params, Connection}`、`crate::db::models::ScanRoot`、
  `crate::error::{AppError, Result}`。`relink_dao_tests` 里用到的 `upsert_directory` **需要**显式
  跨文件 `use super::super::directories::upsert_directory;`——见 §2.3 施工复核更正。

**`scan/directories.rs`**
- `map_dir_node`(私有)及 §1 表格第 3 行全部符号。
- 随迁测试:`mod r2_6_query_tests`。
- 本文件专属 `use`:`rusqlite::{params, Connection, OptionalExtension, Row}`、
  `crate::db::models::{DirFile, DirNode, Directory}`、`crate::error::{AppError, Result}`,
  以及 `crate::tree::{node_key, parent_key, child_rel_path}`、`crate::utils::path::encode_tree_sort_key`
  (当前是内联全路径调用,非 `use` 导入,原样保留即可)。

**`scan/media_upsert.rs`**
- `FastScanItem`、`UpsertOutcome`(含 `impl UpsertOutcome`)、`invalidate_derived_for_item`、
  `apply_source_changed`(私有)、`resolve_suspect_change`、`upsert_fast_scan_item`。
- 随迁测试:`mod fast_scan_upsert_recovery_tests`。
- ⚠️ 唯一需要**手工改写**而非纯剪切的一行:原 `scan.rs:9` 的
  `use super::faces::recompute_person_aggregates;`(`super` 当时 = `queries`)。迁到
  `scan/media_upsert.rs` 后 `super` = `scan`,若原样保留会指向不存在的 `scan::faces`。
  须改写为绝对路径 `use crate::db::queries::faces::recompute_person_aggregates;`(比
  `super::super::faces::...` 更抗未来再嵌套)。**这是本次拆分唯一的行为无关但语法必改点**,
  施工时应第一时间 `cargo check` 验证。

**`scan/mark_missing.rs`**
- `mark_missing`。
- 随迁测试:`mod mark_missing_tests`。
- 本文件专属 `use`:`rusqlite::params`、`std::collections::HashSet`(仅测试用)、
  `crate::error::Result`。

### 2.3 `use super::*` 在新目录结构下为何不够——需补一条跨文件 `use`

⚠️ 施工复核更正(2026-07-25):本节原文断言"`relink_dao_tests` 里的 `use super::*;` 能直接拿到
`directories.rs` 的 `upsert_directory`,不需要额外手写跨文件 `use`",与落地代码矛盾,已实证有误,
正解如下。

`relink_dao_tests` 需要 `roots.rs` 的 `insert_scan_root`/`update_scan_root_path`/`sample_root_items`
**和** `directories.rs` 的 `upsert_directory`。测试模块写 `use super::*;`,其中 `super` 指测试模块的
直接父模块——测试模块嵌在 `roots.rs` 里,`roots.rs` 本身已是 `scan::roots` 子模块,故测试模块的
`super` = `scan::roots`,**不是** `scan`(`scan/mod.rs`)。`mod.rs` 的 glob 重导出
(`pub use directories::*` 等)只把符号注入 `scan` 这一层命名空间,不会穿透进 `scan::roots` 内部——
`super::*` 只能拿到 `roots.rs` 自身定义的符号,拿不到 `directories.rs` 的 `upsert_directory`。
施工已按正解在 `relink_dao_tests` 内补 `use super::super::directories::upsert_directory;`
(`super::super` = `scan`,兄弟子模块经此路径可达)并实证 `cargo check` 通过。

### 2.4 mod.rs 重导出保持 use 路径不变

`queries.rs` 现有的 `mod scan; pub use scan::*;`(两行,`queries.rs:20,36`)**一字不改**。外部调用方
(`scanner::fast_scan.rs`、`ipc::scan_commands.rs`、`editing::ingest.rs`、`db::queries::{faces,media,
thumbnail,layout,exotic,ai,collections,derivations}.rs` 等 9 个文件,已用 grep 核实)清一色写
`crate::db::queries::{一堆函数名}` 的扁平导入,没有任何地方写 `crate::db::queries::scan::xxx` 这种
深路径——即调用方 0 处需要改动,风险面仅限 `scan/` 目录内部四个新文件之间的相互引用。

## 3. 风险与不变量

- **rusqlite 红线**:全文件当前每条 SQL 已是逐参数 `params![...]` 绑定,无字符串拼接(仅
  `map_media_entities` 用动态生成 `?N` 占位符 + `Vec<&dyn ToSql>`,属于"参数数量随输入变化"的
  合规用法,非拼接用户输入)。纯移动不涉及任何 SQL 改写,此不变量天然保持。
- **`pub(in crate::db::queries)` 可见性域不得收窄**:`EXCLUDE_HIDDEN_ROOTS`/`_M`/`_ITEMS` 三常量当前
  标注为 `pub(in crate::db::queries)`,被 `media.rs`/`thumbnail.rs`/`layout.rs`/`exotic.rs`/
  `faces.rs`/`ai.rs`/`collections.rs`/`derivations.rs` 共 8 个同级域文件引用(已 grep 核实)。
  迁到 `scan/roots.rs` 后,**限定路径必须原样保留 `pub(in crate::db::queries)`**,不能写成
  `pub(in crate::db::queries::scan)` 或 `pub(in crate::db::queries::scan::roots)`——否则那 8 个
  文件会编译失败(私有性检查是按声明时写的绝对路径,不会因为物理挪到子目录就自动放宽或收紧)。
  `roots.rs` 物理位置是 `crate::db::queries` 的后代,原限定路径依然合法可达,只是"看起来隔了一层目录"
  容易被手误改窄——这是本次拆分**最大的一处编译期风险**,建议施工后专门 `grep -r
  "EXCLUDE_HIDDEN_ROOT"` 核对三处声明行未被改动限定符。
- **扫描管线契约(不可变)**:
  - `mark_missing` 三重守门(在线卷 ∩ 本 root 子树 ∩ 未 seen,且 `is_deleted=0`、非已 `missing`)——
    该逻辑与其 170 行测试整体搬迁,零改写,矩阵测试即回归锁。
  - `invalidate_derived_for_item` 必须在 upsert 同一事务内被调用(函数本身不开事务,由调用方
    `scanner::fast_scan.rs` 控制边界)——纯移动不改变函数体,此契约不受影响,但要注意
    `apply_source_changed`(私有 fn)与 `invalidate_derived_for_item`/`resolve_suspect_change`/
    `upsert_fast_scan_item` 必须留在**同一个文件**(`media_upsert.rs`),否则私有可见性会挡住调用。
  - `J1` 修复相关的 `load_directory_mtime_snapshot`("上一轮扫描终态"快照,不可读活行)随
    `directories.rs` 整体搬迁,注释与实现不可分离改写。
- **跨域调用点仅一处**:`invalidate_derived_for_item` 调用 `faces::recompute_person_aggregates`
  (`scan.rs:9,828`)。这是本次唯一必须手改的 `use` 行(见 §2.2),其余全部为剪切粘贴。
- **`relink_dao_tests` 的跨子模块依赖靠显式 `use`,非 mod.rs glob 重导出**(见 §2.3 施工复核更正);
  `use super::super::directories::upsert_directory;` 是硬依赖,若未来任何子模块调整目录结构,须
  同步检查该行路径深度是否仍正确。
- **未验证到的面**(超出本次只读分析范围,建议施工前补一次快速确认):`Row`/`OptionalExtension`
  等 `rusqlite` 子项在四个新文件里各自实际用量不同,`cargo check` 会精确报出每个文件缺失/多余的
  `use`,无需在方案阶段逐符号预判,施工时按报错增删即可。

## 4. 收益与优先级

**收益**:
- 单文件从 1870 行→最大 697 行,认知负载显著下降;四个域文件职责边界与源码既有分区注释完全对齐,
  阅读/定位成本降低,后续改动(如新增一种 scan_root 属性)只触达对应小文件,diff 更聚焦。
- 与仓库既有的 `queries.rs` facade 模式(14 域同构)完全一致,不引入新模式,后续开发者认知零额外成本。
- 测试随对应域一起搬迁,`cargo test` 的失败信息(文件路径)会更精确指向出问题的域。

**施工顺序建议**(风险从低到高,每步独立可编译可测试,符合"每步验证"约定):
1. 建 `scan/` 目录骨架:先只搬 `mark_missing.rs`(最小、自包含、零跨域依赖)+ 同步建 `mod.rs` facade,
   验证"目录模块替换单文件模块"这个机制本身可行(`cargo check` 通过即证明 Rust 侧无障碍)。
2. 搬 `roots.rs`(含 `scan_root_backend_tests` + `relink_dao_tests`),重点验证 §2.3 的
   `use super::*` 跨域可见性假设。
3. 搬 `directories.rs`(含 `r2_6_query_tests`)。
4. 搬 `media_upsert.rs`(含 `fast_scan_upsert_recovery_tests`),同步完成 §2.2 提到的
   `recompute_person_aggregates` 导入路径改写——这是全线唯一的语法性必改点,放最后做,
   前三步先把机制跑通再碰这个手改点,出错也容易定位。
5. 全部搬完后,`queries.rs` 的两行 `mod scan; pub use scan::*;` 保持不动,收尾跑一次全量校验(见 §5)。

## 5. 验证策略

- **编译**:`cd src-tauri && cargo check`(workspace 默认 target,覆盖 `scrollery_lib` 与 `scrollery`
  两个 crate)。每完成 §4 施工顺序中的一步就跑一次,不要攒到最后。
- **静态检查**:`cargo clippy --all-targets -- -D warnings`(或仓库既定 clippy 命令),重点关注
  拆分后每个新文件的未用 `use` 告警——最容易在"复制整段 use 块再各自精简"时留残留。
- **单测**(涉面即本文件五个 `#[cfg(test)]` 模块,零新增/零删减,纯搬迁回归锁):
  `cargo test --lib db::queries::scan::`(按实际 crate/包名调整前缀,若 workspace 下测试路径带
  `scrollery_lib::` 前缀则相应加上)应完整跑到:
  - `mark_missing_tests`(6 用例)
  - `fast_scan_upsert_recovery_tests`
  - `scan_root_backend_tests`
  - `relink_dao_tests`
  - `r2_6_query_tests`
  五个模块全绿,且用例数与拆分前一致(用 `cargo test ... -- --list` 或对比测试计数,防止重命名/
  漏挂 `#[cfg(test)] mod` 导致某组测试被静默排除在编译单元之外)。
- **rustfmt**:按项目约定对受影响 Rust 文件跑 `cargo fmt`(仅针对本次改动的文件/目录,不做仓库级
  `cargo fmt` 全量格式化,避免掺入无关 diff)。
- **不需要**:本次是纯结构移动、零 SQL/行为改写,不需要新增单元测试或跑前端/GUI/CI 全量门禁;
  若后续同批次"全仓深度 review"或"CI 全量"因其他并行改动触发,顺带覆盖即可,不必单独为本次拆分
  再起一轮全量 CI。
