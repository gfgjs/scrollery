---
id: 2026-07-25-faces-rs
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# 拆分方案:src-tauri/src/db/queries/faces.rs

> 目标文件 2480 行 / 112KB(2026-07-25 快照,并行注释精简会话在同步瘦身,行号会漂移)。
> 本文档只出方案,不改代码。锚点以**符号名**为主、行号区间为辅(辅助定位,漂移后以符号名重新定位)。
> `faces.rs` 本身就是更早一轮"T 线拆分"的产物——`src-tauri/src/db/queries.rs` 是 facade,
> 声明 `mod ai; mod faces; ...` + `pub use faces::*;` 等,外部调用方一律走
> `crate::db::queries::<symbol>`(已核实,见下文§3 调用点)。本次拆分是同一模式再套一层。

## 1. 现状结构图

按文件内 `// ── ... ──` 分节注释(作者已手工分区,与实际职责边界完全吻合)与
`#[cfg(test)] mod` 边界统计:

| # | 职责群 | 约行数 | 起始符号锚点 | 结束符号锚点 |
|---|--------|--------|--------------|--------------|
| A | Face 检测队列状态机(F3):待检测项查询、状态位批量更新/复位、按模型 reset/sync | ~250 | `count_error_face_items`(L15) | `sync_face_status_batched`(L224‑250) |
| B | 人物墙 + 人物管理(F6):人物列表(含隐藏根排除 V21)、单图人脸框、命名/隐藏/忽略/合并 | ~370 | `list_persons`(L259) | `merge_persons`(L518‑621) |
| C | 批量审批命令(Part4 T3 / §3.5.1):确认/改派/移出/拒绝/建人 + likely-match 列表 + face 写入(insert/finish/guarded) | ~730(生产)+ ~250(其自身测试,物理位置在文件尾部) | `recompute_person_aggregates`(L646) | `batch_update_face_status_guarded`(L1186‑1205);测试见下 |
| D | 增量聚类(F4,仅增量)+ 派生字段对账 | ~483 | `PersonRow`(L1360) | `apply_face_clusters`(L1536‑1602) |
| E | 全量重聚类(显式命令,非增量) | ~164 | `PersonReclusterRow`(L1845) | `rebuild_person_clusters`(L1935‑1999) |

测试模块(`#[cfg(test)] mod`,共 5 个,均嵌在生产代码之间或文件尾部):

| 测试模块 | 锚点行 | 覆盖对象 | 物理位置 vs 所属职责群 |
|----------|--------|----------|------------------------|
| `hidden_root_person_wall_tests` | L374‑431 | `list_persons`(隐藏根排除 V21) | 紧跟 B 群,位置吻合 |
| `x1_conditional_finish_tests` | L1208‑1351 | `batch_finish_face_items`/`batch_update_face_status_guarded`(+ 借用 `ai::batch_finish_ai_items` 对照) | 紧跟 C 群,位置吻合 |
| `f2_f3_cluster_concurrency_tests` | L1605‑1835 | `apply_face_clusters` 并发/FK 边界 | 紧跟 D 群,位置吻合 |
| `face_approval_tests` | L2002‑2251 | C 群命令(confirm/reassign/unassign/reject/create/list_likely_matches)**+ B 群的 `merge_persons`** | ⚠️ 物理位置在 E 群**之后**、文件尾部,与其覆盖的 C(+B)群不相邻 |
| `r2_6_query_tests` | L2256‑2480 | A 群的 `reset_face_data_batched`/`sync_face_status_batched` **+ D 群的 `apply_face_clusters` + E 群的 `rebuild_person_clusters`**(Part4‑T6 模型隔离跨群集成测试) | ⚠️ 物理位置在文件最末,跨 A/D/E 三群,且顶部残留一条不匹配的模块级 doc 注释(见§6) |

模块级 `use`(L6‑12,当前只有一份,拆分后需按调用者拆分/去重):
```rust
use rusqlite::{params, Connection};
use super::ai::reset_error_items_batched;                              // 仅 A 群
use super::exotic::{NOT_BLOCKED_BY_EXOTIC, NOT_BLOCKED_BY_EXOTIC_M};    // 仅 A 群
use super::scan::{EXCLUDE_HIDDEN_ROOTS, EXCLUDE_HIDDEN_ROOTS_M};        // EXCLUDE_HIDDEN_ROOTS 仅 A;
                                                                         // EXCLUDE_HIDDEN_ROOTS_M 同时被 A、B 用
use crate::error::{AppError, Result};                                   // 全部群都要
```
`super::scan::hidden_root_ids`(B 群,`list_persons_by_ignored` 内联全路径调用,未在顶部 `use`)、
`super::scan::set_scan_root_hidden`(仅测试内联调用)同理需要按新嵌套深度调整路径。

跨群共享的私有 helper(**不是** re-export 的 `pub` API,拆分的关键难点):

| Helper | 定义处 | 调用方(跨群) |
|--------|--------|--------------|
| `recompute_person_aggregates`(`pub(in crate::db::queries)`,L646) | C 群头部 | C 群自身(reassign/unassign/reject/create)、D 群(`delete_media_item_hard`/`reconcile_person_face_counts`)、E 群(`rebuild_person_clusters` 尾部对账) |
| `in_clause`(私有 fn,L739) | C 群头部 | C 群自身(confirm/reassign/unassign/reject/create)、**B 群的 `merge_persons`**(L534) |
| `persons_of_faces`(私有 fn,L719) | C 群头部 | 仅 C 群(reassign/unassign/create) |
| `cosine_from_le_bytes`(私有 fn,L1081) | C 群 | 仅 C 群(`list_likely_matches`) |
| `insert_face_row`(私有 fn,L1119) | C 群 | 仅 C 群(`batch_finish_face_items`) |

`recompute_person_aggregates` 的可见性已是 `pub(in crate::db::queries)`——这是 `crate::db::queries`
及其**全部后代模块**均可见,`faces::clustering`/`faces::recluster` 等新增子模块天然在此可见域内,
**迁移零改动**。`in_clause` 目前是模块私有 `fn`,若拆到不同文件则必须提升为跨子模块可见(见§2)。

## 2. 拆分方案

保持 `db/queries.rs` facade 不变(`mod faces; pub use faces::*;` 原样),把 `faces.rs` 单文件
换成目录 `db/queries/faces/`,内部再做一层同款 facade:

```
src-tauri/src/db/queries/
  faces.rs              ← 删除(内容迁出)
  faces/
    mod.rs               ← facade:mod 声明 + pub use 重导出 + 模块级 doc 注释 + 2 个跨群共享 helper
    status.rs             ← A 群:F3 检测队列状态机
    wall.rs                ← B 群:F6 人物墙 + 人物管理
    approval.rs             ← C 群:Part4 T3 批量审批 + face 写入
    clustering.rs             ← D 群:F4 增量聚类 + 派生字段对账
    recluster.rs               ← E 群:全量重聚类
```

`db/queries.rs` 顶部的 `mod faces; pub use faces::*;` 一字不改——Rust 对 `mod faces;` 既能解析
`faces.rs` 也能解析 `faces/mod.rs`,外部调用方(`crate::db::queries::<symbol>`,已核实的调用点
如 `src-tauri/src/ai/face_cluster.rs:591` 的 `crate::db::queries::rebuild_person_clusters(...)`)
**完全不受影响**。

### 2.1 `faces/mod.rs`(facade,预估 ~90 行)

```rust
// 模块级 doc 注释(原 L1-4 原样迁入)
mod status;
mod wall;
mod approval;
mod clustering;
mod recluster;

pub use status::*;
pub use wall::*;
pub use approval::*;
pub use clustering::*;
pub use recluster::*;

use rusqlite::{params, Connection};
use crate::error::{AppError, Result};

// in_clause + recompute_person_aggregates 原样迁入(跨群共享,详见 §1 表)
```
迁移符号:`in_clause`(L739‑746)、`recompute_person_aggregates`(L646‑714,含其 doc 注释)。
`in_clause` 需要从模块私有提升为 `pub(super)`。⚠️ 施工复核更正(2026-07-25):`pub(super)` 定义于
`faces/mod.rs` 时,可见域是其父模块 `crate::db::queries` 及其**全部**后代(不止 `faces` 的 5 个
子模块,`ai`/`media`/`collections` 等同级域理论上也可见),并非"仅 faces 目录内"——但目前无任何
域外符号实际引用 `in_clause`,施工已按原方案落地 `pub(super)`,对外部零影响,维持现状不改。
`recompute_person_aggregates` 已是 `pub(in crate::db::queries)`,原样迁入
即可,无需改可见性。

### 2.2 `faces/status.rs`(A 群,预估 ~250 生产行 + 借入的 `r2_6_query_tests`)

迁移符号(原样搬运,函数体不动):
`count_error_face_items`(L15)、`reset_error_face_items`(L27)、`PendingFaceItem`(L39)、
`get_pending_face_items`(L52)、`count_pending_face_items`(L84)、`batch_update_face_status`(L95)、
`reset_processing_face_items`(L113)、`count_processed_face_items`(L126)、`count_persons`(L137)、
`count_faces_for_model`(L148)、`reset_face_data`(L169)、`reset_face_data_batched`(L173)、
`sync_face_status_for_model`(L217)、`sync_face_status_batched`(L224)。

顶部 `use` 改为:
```rust
use rusqlite::{params, Connection};
use crate::db::queries::ai::reset_error_items_batched;
use crate::db::queries::exotic::{NOT_BLOCKED_BY_EXOTIC, NOT_BLOCKED_BY_EXOTIC_M};
use crate::db::queries::scan::{EXCLUDE_HIDDEN_ROOTS, EXCLUDE_HIDDEN_ROOTS_M};
use crate::error::{AppError, Result};
```
(用 crate 绝对路径而非 `super::super::ai` 之类相对路径——嵌套深度多了一层 `faces/`,相对路径
的 `super` 计数容易在后续维护中数错;绝对路径对行号/嵌套漂移免疫,这是本次拆分**唯一**推荐的
非逐字搬运处,纯粹是路径写法而非行为改动。)

`r2_6_query_tests`(L2256‑2480)整体迁入本文件作为其 `#[cfg(test)] mod`。其内部对
`apply_face_clusters`/`rebuild_person_clusters` 的调用(现为 `super::apply_face_clusters(...)`)
改写为 `crate::db::queries::apply_face_clusters(...)` / `crate::db::queries::rebuild_person_clusters(...)`
(两者已是 `pub fn` 且经 facade 链路重导出,无需任何可见性提升);其内部对
`reset_face_data_batched`/`sync_face_status_batched` 的调用(`super::reset_face_data_batched`)
因测试模块与被测函数**同在** `status.rs` 内,`super::` 依旧原样成立,零改动。

### 2.3 `faces/wall.rs`(B 群,预估 ~370 行)

迁移符号:`list_persons`(L259)、`list_ignored_persons`(L270)、`list_persons_by_ignored`(L280,
含隐藏根排除 V21 双 SQL 分支)、`hidden_root_person_wall_tests`(L374‑431,随迁)、
`get_faces_for_item`(L435)、`rename_person`(L465)、`set_person_hidden`(L483)、
`set_person_ignored`(L495)、`merge_persons`(L518‑621)。

`merge_persons` 内的 `in_clause(&all_ids)` 调用改为 `use super::in_clause;`(从 `faces/mod.rs`
导入,`pub(super)` 可见性覆盖此处)。`list_persons_by_ignored` 内联调用的
`super::scan::hidden_root_ids` 改为 `crate::db::queries::scan::hidden_root_ids`。
`hidden_root_person_wall_tests` 内联调用的 `super::super::scan::set_scan_root_hidden` 改为
`crate::db::queries::scan::set_scan_root_hidden`(理由同 §2.2,多一层嵌套后绝对路径更稳)。

顶部 `use` 需要 `EXCLUDE_HIDDEN_ROOTS_M`(`crate::db::queries::scan::EXCLUDE_HIDDEN_ROOTS_M`,
不需要 `EXCLUDE_HIDDEN_ROOTS` 非 M 变体——该常量只在 A 群用)。

### 2.4 `faces/approval.rs`(C 群,预估 ~730 生产行 + 借入的 `x1_conditional_finish_tests` ~145 行 + 借入的 `face_approval_tests` ~250 行,合计约 1125 行——拆分后**单文件最大者**)

迁移符号:`persons_of_faces`(L719)、`confirm_face_assignment`(L753)、
`reassign_face_to_person`(L772)、`unassign_face`(L833)、`reject_face_candidate`(L863)、
`create_person_from_faces`(L904)、`list_likely_matches`(L972)、`cosine_from_le_bytes`(L1081)、
`NewFace`(L1104)、`insert_face_row`(L1119)、`batch_finish_face_items`(L1144)、
`batch_update_face_status_guarded`(L1186)、`x1_conditional_finish_tests`(L1208‑1351,随迁,
内部借用 `super::super::ai::{batch_finish_ai_items, batch_update_ai_status_guarded}` 需改绝对路径)。

本文件内对 `in_clause`/`recompute_person_aggregates` 的调用改为 `use super::{in_clause,
recompute_person_aggregates};`。

`face_approval_tests`(L2002‑2251)整体迁入本文件。其内部两处 `merge_persons(&c, ...)` 调用
(`merge_moves_faces_and_deletes_src`/`merge_cross_model_rejected`)需新增
`use crate::db::queries::merge_persons;`(该 fn 已是 `pub`,经 facade 重导出,零可见性改动,
只是补一条 import)。

> 本群体量最大,若日后仍嫌大,可再按"写入路径"(NewFace/insert_face_row/batch_finish_face_items/
> batch_update_face_status_guarded/x1 测试)与"人工纠错命令"(confirm/reassign/unassign/reject/
> create/list_likely_matches/face_approval_tests)二次细拆为 `approval/write.rs` +
> `approval/correction.rs`。本轮不作为必选项,列为§4"可选二级拆分"。

### 2.5 `faces/clustering.rs`(D 群,预估 ~483 生产行)

迁移符号:`PersonRow`(L1360)、`get_all_persons_for_clustering`(L1371)、
`ClusterableFaceRow`(L1396)、`get_clusterable_faces`(L1407)、`delete_media_item_hard`(L1449)、
`reconcile_person_face_counts`(L1469)、`get_orphan_cluster_item_ids`(L1498)、
`PersonClusterUpdate`(L1523)、`apply_face_clusters`(L1536‑1602)、
`f2_f3_cluster_concurrency_tests`(L1605‑1835,随迁)。

`use super::recompute_person_aggregates;`。`delete_media_item_hard`/`reconcile_person_face_counts`
从命名上更像"通用对账工具",但原作者把它们放进 F4 增量聚类分节内(紧邻质心/计数重算逻辑),
本方案遵循"纯结构移动"原则不重新归类,原样带过去。

### 2.6 `faces/recluster.rs`(E 群,预估 ~164 行)

迁移符号:`PersonReclusterRow`(L1845)、`get_persons_for_recluster`(L1854)、
`ReclusterFaceRow`(L1876)、`get_all_faces_for_recluster`(L1886)、`get_face_rejections`(L1912)、
`rebuild_person_clusters`(L1935‑1999)。`use super::recompute_person_aggregates;`。

### 2.7 预估结果一览

| 文件 | 预估行数 | 预估大小(按原文件 ~46B/行折算,仅供量级参考) |
|------|----------|----------------------------------------------|
| `mod.rs` | ~90 | ~4KB |
| `status.rs` | ~480(含借入测试) | ~22KB |
| `wall.rs` | ~370 | ~17KB |
| `approval.rs` | ~1125(含借入两个测试模块) | ~50KB |
| `clustering.rs` | ~483 | ~22KB |
| `recluster.rs` | ~164 | ~7.5KB |
| **合计** | **~2712** | **~123KB**(略高于原 112KB,因 `use`/mod 声明重复开销,属预期) |

## 3. 风险与不变量

- **rusqlite 参数绑定红线**:`reassign_face_to_person`/`create_person_from_faces`/
  `reject_face_candidate` 用手工占位符序号(如 `?{face_ids.len() + 1}`)拼 SQL,IN 子句先占
  `?1..?k`、追加绑定值放末位——这是刻意的顺序约定(见代码内注释:与 IN 占位冲突会触发
  `InvalidParameterCount`)。拆分要求函数体**整体**搬运、禁止在搬运途中重排参数或"顺手"改用
  `named_params!`/重新编号——纯移动不改行为的前提下这条自动满足,只需在 PR 自查时确认 diff
  是纯剪切粘贴而非重写。
- **事务边界**:所有 `unchecked_transaction()` 用法(`merge_persons`/`reassign_face_to_person`/
  `unassign_face`/`reject_face_candidate`/`create_person_from_faces`/`batch_finish_face_items`/
  `batch_update_face_status_guarded`/`apply_face_clusters`/`rebuild_person_clusters`/
  `delete_media_item_hard`/`reconcile_person_face_counts`)均在单个函数体内开启并 `commit()`,
  拆分只搬运整函数、不拆函数内部,因此事务边界不会跨文件断裂——零风险项,仅在此处明确记录
  以便实施者不误拆。
- **`std::sync::Mutex<Connection>` 锁粒度**:`reset_face_data_batched`/`sync_face_status_batched`
  在 `loop` 的**每次迭代内**重新 `db.lock()`(而非在循环外锁一次),这是刻意设计(允许其他线程
  在批次间穿插,避免长事务式独占)。搬运到 `status.rs` 时必须保持"每迭代一次 lock/drop"的写法,
  不得因为"顺手整理"把 lock 提到循环外。
- **同模型守卫(§3.5.1a / 2026-07-10 审查 F1)**:`merge_persons`/`reassign_face_to_person`/
  `create_person_from_faces` 均有"先查模型一致性、不一致即 `return Err` 回滚"的前置守卫,守卫
  查询与后续写入必须在同一个 `tx` 上顺序执行。函数整体搬运时此顺序天然保持,但如果日后有人
  想把守卫抽成共享 helper(本轮**不**建议这么做),必须保证守卫仍先于任何 mutation 执行。
- **`is_unassigned`/`is_confirmed` 判别位契约(F3)**:`unassign_face`/`reject_face_candidate` 写
  `is_unassigned=1`,`reassign_face_to_person`/`create_person_from_faces`/`apply_face_clusters`/
  `rebuild_person_clusters` 写 `is_unassigned=0`——这是"用户主动移出"与"崩溃丢聚类"的判别位,
  孤儿对账(`get_orphan_cluster_item_ids`)、增量聚类候选筛选(`get_clusterable_faces`)都依赖它。
  纯移动不改这些字面量,风险仅在"复核 diff 时看漏一处字面量被误改"。
- **可见性调整(本方案唯一需要的非纯搬运改动)**:`in_clause` 由模块私有 `fn` 提升为
  `pub(super)`(可见域实为父模块 `db::queries` 全域,非仅 `faces` 目录内——见 §2.1 施工复核更正;
  目前无域外符号引用,对外部调用方零实际影响)。除此之外**没有任何**其它可见性
  变更——`recompute_person_aggregates` 已是 `pub(in crate::db::queries)`,天然覆盖新增的
  `faces::{approval,clustering,recluster}` 三个子模块;`merge_persons`/`apply_face_clusters`/
  `rebuild_person_clusters` 等都已是 `pub fn`,测试模块跨文件引用只需补 `use` 路径,不涉及
  可见性放宽。
- **人脸流水线契约(跨 crate 边界,不在本文件内,但拆分时须留意)**:`faces.rs` 只含同步
  DB 查询函数体,不含 `spawn_blocking`/`async` 包装——那层在 `commands/` 与 `ai::face_pipeline`/
  `ai::face_cluster` 侧。拆分不触碰调用方,但施工者若顺手"整理"了调用点导入路径,须确认没有
  绕开 `spawn_blocking` 在异步上下文里直接调用这些同步 rusqlite 函数(项目红线,非本文件职责
  但属其消费方红线)。
- **性能热路径**:本文件内多数函数是幂等对账/低频人工命令(合并/改派/命名),真正的热路径是
  `get_pending_face_items`/`get_clusterable_faces`/`apply_face_clusters`(F3/F4 增量流水线每批
  调用)。拆分不改 SQL 与索引使用方式,预期零性能影响;验证时可关注 clippy 是否因内联/单态化
  边界变化给出新的性能相关 lint(不预期有,SQL 字符串与 prepare/query_map 结构不变)。

## 4. 收益与优先级

拆后最大单文件从 112KB(faces.rs 整体)降到约 50KB(`approval.rs`),其余四个域文件均在
5‑25KB 区间,符合本轮"≥70KB 详案"分层治理的目标(faces.rs 降到 tier 之下,无需再入下一轮
详案名单)。

建议施工顺序(风险从低到高,便于分阶段验证 + 逐阶段提交):

1. **`recluster.rs`(E 群)**——依赖最少(只需 `recompute_person_aggregates` 一个共享 helper),
   无跨群测试借入,验证面最窄,适合作为"验证本方案可行"的首个切片。
2. **`clustering.rs`(D 群)**——同样只依赖 `recompute_person_aggregates`,但自带
   `f2_f3_cluster_concurrency_tests`(需确认测试内部引用路径调整正确)。
3. **`status.rs`(A 群)**——需要承接 `r2_6_query_tests`(跨群集成测试,涉及对 D/E 群的
   facade 绝对路径引用),验证面比 1/2 略宽。
4. **`wall.rs`(B 群)**——首次消费 `faces/mod.rs` 的 `in_clause`(`pub(super)` 可见性变更点),
   建议在此步之后立刻跑一次全量 `cargo test`(见§5)确认可见性改动无副作用。
5. **`approval.rs`(C 群)**——体量最大、借入测试最多(`x1_conditional_finish_tests` +
   `face_approval_tests`),放最后,复用前四步已验证过的 facade 模式与路径写法习惯。
6. **`faces/mod.rs`**——facade 本身随第 1 步就需要建立骨架(mod 声明 + pub use),`in_clause`/
   `recompute_person_aggregates` 的实际迁入可以推迟到它们首次被跨文件引用时(第 1/4 步)。

每步都是独立可编译、可测试的提交单元,天然支持"改必跑门禁,分批提交"的项目执行纪律。

## 5. 验证策略

- **编译**:每步迁移后 `cargo check -p scrollery`(或等效的 `--manifest-path src-tauri/Cargo.toml`),
  确认 `db/queries.rs` facade 与所有跨域调用点(如 `src-tauri/src/ai/face_cluster.rs`)零改动
  下仍可解析 `crate::db::queries::<symbol>`。
- **静态检查**:`cargo clippy -p scrollery --all-targets`,重点关注拆分引入的
  `unused_imports`(顶部 `use` 按群拆分后最容易残留冗余项)与 `dead_code`(私有 helper 提升为
  `pub(super)` 后如果某群实际未消费需收紧回私有)。
- **单元测试(本文件既有测试即是最强回归网)**:
  `cargo test -p scrollery db::queries::faces::` 定向跑 5 个测试模块
  (`hidden_root_person_wall_tests`/`x1_conditional_finish_tests`/
  `f2_f3_cluster_concurrency_tests`/`face_approval_tests`/`r2_6_query_tests`),这些测试已覆盖
  本文档§3 列出的全部红线(跨模型守卫、`is_unassigned`/`is_confirmed` 判别位、V17 覆盖账、
  隐藏根排除、Part4‑T6 模型隔离等)——拆分后测试**逐字不改**、只改 `use`/调用路径,测试全绿
  即可判定"纯结构移动"目标达成。
- **涉面回归**:人脸批量审批/人物墙/聚类是 GUI 强相关特性(人物命名、合并、批量审批 UI),
  本次是纯代码结构调整、不改 SQL 与返回结构,理论上不需要新增 GUI 手测;若不放心,可在
  `approval.rs`(第 5 步,体量最大风险相对最高)完成后跑一次现有的人脸相关 GUI 手测清单
  (若三件套/记忆索引中有现成清单,复用即可,不需新建)。
- **不需要**新增测试——本方案不改变任何函数的输入输出契约,现有 5 个测试模块的断言覆盖面
  已经是"拆分是否破坏行为"的充分判据。

## 6. 顺手发现

- `src-tauri/src/db/queries/faces.rs:2253-2254` — `r2_6_query_tests` 模块前的 doc 注释写的是
  "T18 S0:`view_to_sql` 编译器快照测试"(与 `layout.rs`/`push_query_body` 相关),但该模块实际
  测试的是 `reset_face_data_batched`/`sync_face_status_batched`/`apply_face_clusters`/
  `rebuild_person_clusters`(Part4‑T6 模型隔离),与注释文本完全不匹配,疑似从别处复制遗留。
  建议修法:下次touches 此文件时把该行注释改为准确描述(如"Part4‑T6 模型隔离跨群回归:
  reset/sync/cluster/rebuild 均按 model_name 隔离"),不影响本次拆分方案,只是文档准确性问题。
