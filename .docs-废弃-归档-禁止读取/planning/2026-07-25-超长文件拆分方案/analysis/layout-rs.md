---
id: 2026-07-25-layout-rs
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# `src-tauri/src/db/queries/layout.rs` 拆分方案分析

> 本文只读代码、只写方案,未改任何代码。目标文件 137KB / 2736 行(与并行精简注释会话共享,
> 行号会漂移——本文锚点以**符号名**为主,行号区间标"约"仅供定位参考)。
>
> ⚠️ 命名提醒:仓库中存在两个同名但完全不同的 `layout`——`crate::db::queries::layout`(本文档
> 对象,DB 查询域)与 `crate::layout`(布局计算/`items_cache`/`cache` 等业务模块)。本文所有
> `layout` 均指前者,行文中不再逐次注明。

## 0. 度量口径

用 `wc -c` 对当前文件按符号边界切片实测(非估算),总字节 **136,842B**。生产代码约
**44.3KB(32%)**,五个 `#[cfg(test)] mod ..._tests` 测试体约 **89.1KB(68%)**——本文件体积主体
是测试代码而非生产逻辑,这一比例直接决定了第 2 节的拆分策略(见 §2.3)。

## 1. 现状结构图

`layout.rs` 是 `db/queries.rs`(facade)已完成的"域拆分"(T 线)产物之一——`queries.rs` 用
`mod layout; pub use layout::*;` 把 `layout` 与 `ai/collections/media/…` 等 12 个同级域文件
并列声明(`src-tauri/src/db/queries.rs:9-39`)。本次是**在这一个域内部再拆一层**。

### 1.1 生产代码(约 1-865 行,~44.3KB)

| 分组 | 符号锚点 | 约行区间 | 约字节 | 职责一句话 |
|---|---|---|---|---|
| 模块头 | 文件顶部 doc + `use` | 1-17 | 1.0KB | 模块级文档 + `models`/`error` 导入 |
| Row mapper | `map_layout_item` | 19-43 | 1.0KB | `Row` → `LayoutItem` 位置映射(18 列,与三处 SELECT 列表位置严格对齐) |
| Facet/标签查询(A) | `list_library_formats` | 72-94 | ~1.4KB | 库内实际存在的媒体格式 distinct 列表(细分格式弹层用),自带隐藏根裸拼排除 |
| 画廊主查询 | `query_layout_items` | 100-144 | | 取完整 `LayoutItem` 的画廊查询入口,调 `push_query_body` |
| S1 canonical SQL 组装 | `canonical_layout_sql` | 157-199 | | 组出「基准序、免 ORDER BY」SQL(unary `+` 压制 partial index 是全文件最脆弱的一段字符串契约) |
| S1 canonical 查询 | `query_layout_items_canonical` | 202-215 | | 调用上者 + `sort_canonical` 内存补序 |
| filename 基准查询 | `query_layout_items_filename_baseline` | 232-247 | | 复用 `canonical_layout_sql`,SQL 侧下发 `ORDER BY … NATURAL_CMP` |
| filename rank 数据源 | `query_item_ids_filename_order` | 259-283 | | 双键缓存的惰性 filename rank(只取 id) |
| 内存补序 | `sort_canonical` | 289-300 | | `(sort_datetime DESC, id DESC)` 装饰-排序-还原 |
| JOIN 需求判定 | `search_join_needs` | 305-317 | | 供 `canonical_layout_sql`/`query_item_ids_filename_order` 共用 |
| Facet/标签查询(B) | `query_dir_labels` | 321-352 | | 全库 `dir_id → DirLabel` 一次性映射 |
| **SQL builder 引擎** | `push_query_body` | 359-394 | ~19.5KB(整块) | FROM/JOIN/WHERE/ORDER 主体,`query_layout_items` 与 `view_to_sql` 的单一事实源 |
| | `push_in_predicate` | 408-426 | | `AND col IN (...)` 通用谓词,`pub(in crate::db::queries)`——**被 `search.rs` 跨域直接引用**(见 §3.2) |
| | `push_root_exclusion` | 439-458 | | V21 隐藏根排除谓词,`pub(in crate::db::queries)` |
| | `push_where_predicates` | 468-645 | | 全文件最长单函数(178 行),画廊 WHERE 全部谓词(含 5 种搜索 scope 分支) |
| | `push_order_by` | 653-721 | | 画廊 ORDER BY(folder/date/none × filename/similarity/默认) |
| View 编译 | `view_to_sql` | 731-753 | | `ViewDescriptor` → 只取 `m.id` 的 SQL,复用 `push_query_body` |
| **选择解析** | `SELECTION_EXPLICIT_MAX`/`SELECTION_BATCH_CHUNK` | 756/758 | ~5.3KB(整块) | 显式选择上限 / 批量分块大小(后者 `pub`,被 IPC 层消费) |
| | `resolve_selection` | 768-808 | | `SelectionDescriptor` → 实际 id 集合 |
| | `count_selection` | 814-865 | | 精确计数(`COUNT(*) − 交集`) |

### 1.2 测试代码(约 868-2736 行,~89.1KB,占文件 68%)

| `mod` 锚点 | 约行区间 | 约字节 | 主测对象 | 备注 |
|---|---|---|---|---|
| `canonical_plan_tests` | 868-1066 | 8.8KB | `canonical_layout_sql`(EXPLAIN QUERY PLAN 断言) | 唯一直接调用 items 簇私有 fn 的测试组之一 |
| `view_to_sql_tests` | 1069-1491 | 19.0KB | `view_to_sql`/`push_in_predicate`/`ViewDescriptor::to_media_filter` | **含 4 个跨域测试**(见下) |
| `selection_resolve_tests` | 1495-2481 | **50.1KB** | `resolve_selection`/`count_selection`/`query_layout_items` 列位/派生序对拍 | 单模块比全部生产代码总和还大,文件体积的真正大头 |
| `format_facet_tests` | 2485-2573 | 4.7KB | `list_library_formats` | |
| `hidden_root_exclusion_tests` | 2579-2736 | 6.5KB | V21 隐藏根排除,横跨 item_queries/query_builder/selection/facets 四域 | 天生跨子模块,见 §3.3 |

⚠️ 顺手发现(非本次任务范围,只报不修):`view_to_sql_tests`(1074-1078 行)与
`selection_resolve_tests`(1502-1505 行)各自显式 `use super::super::collections::{...}` /
`use super::super::faces::{...}` / `use super::super::media::{...}`,并附注释"跨域测试定向
import(§5 规则 2)"。即 `rename_collection_user_only`/`soft_delete_and_restore_collection`/
`list_deleted_collections_partitions_and_orders_by_deleted_at`/`list_ignored_persons_returns_only_ignored`
(collections/faces 域)与 `get_media_item_maps_rating_and_color_label`/
`batch_helpers_write_resolved_selection` 等(media 域)测试被历史性地放进了 `layout.rs`,与文件
名/模块职责不符——像是当年 T 线拆分时"就地挂靠"未回迁。本次分析按现状照搬迁移(不重排测试
归属,风险见 §3.3),仅在此指出供后续单独立项处理。

## 2. 拆分方案

### 2.1 依赖方向(先立后拆)

```
facets.rs         (无内部依赖,仅用 sibling `scan::hidden_root_ids`)
query_builder.rs   (无内部依赖,仅用 sibling `scan` 间接经调用方传入 hidden_roots: &[i64])
      ↑                              ↑
item_queries.rs ───────┘        selection.rs ───────┘
   (依赖 query_builder 的 push_query_body/push_where_predicates/push_root_exclusion)
                                  (依赖 query_builder 的 view_to_sql)
```

`query_builder.rs` 是唯一的公共下游依赖点,`item_queries.rs`/`selection.rs` 单向依赖它,
`facets.rs` 全域内零依赖——四者之间**无环**,可安全拆成 4 个生产文件 + 1 个 facade。

### 2.2 生产代码目标结构

```
src-tauri/src/db/queries/layout/
├── mod.rs             facade:模块头 doc + `mod` 声明 + `pub use` 全量重导出(镜像 queries.rs 现有手法)
├── item_queries.rs     map_layout_item, query_layout_items, canonical_layout_sql,
│                       query_layout_items_canonical, query_layout_items_filename_baseline,
│                       query_item_ids_filename_order, sort_canonical, search_join_needs
├── query_builder.rs    push_query_body, push_in_predicate, push_root_exclusion,
│                       push_where_predicates, push_order_by, view_to_sql
├── selection.rs         SELECTION_EXPLICIT_MAX, SELECTION_BATCH_CHUNK,
│                        resolve_selection, count_selection
└── facets.rs            list_library_formats, query_dir_labels
```

`item_queries.rs` 不再进一步细分:`map_layout_item` 与产出其 SELECT 列表的三个查询函数
(`query_layout_items`/`canonical_layout_sql`/`query_layout_items_filename_baseline`)必须留在
同一文件——三处 SELECT 列序与 `map_layout_item` 的 `row.get(N)` 位置是裸约定(无编译期校验),
拆开会让"改列必须同改四处"这条不变量失去"同文件可见"的天然提醒(详见 §3.1)。

`mod.rs` 关键内容(伪代码,示意重导出层次,非最终文本):

```rust
//! (原 1-17 行模块头 doc 原样迁入)
mod facets;
mod item_queries;
mod query_builder;
mod selection;

pub use facets::*;
pub use item_queries::*;
pub use selection::*;
pub use query_builder::*;
// push_in_predicate / push_root_exclusion 定义处即为 `pub(in crate::db::queries)`,
// 但 wildcard `pub use` 不会自动把非 `pub` 项"提级"暴露——必须显式具名重导出,
// 且重导出可见性必须与原定义一致(不可放宽),保 search.rs 现有 `use super::layout::push_in_predicate;`
// 零改动可编译(§3.2 铁证)。
pub(in crate::db::queries) use query_builder::{push_in_predicate, push_root_exclusion};
```

`crate::db::queries::layout` 这一层路径本身不变(`queries.rs` 里 `mod layout;` 一行不用动,
Rust 对"文件"还是"目录+mod.rs"透明),`queries.rs` 现有 `pub use layout::*;` 同样零改动。

### 2.3 测试代码目标结构

鉴于测试占比 68%、且 `selection_resolve_tests` 单体 50KB(比全部生产代码总和还大),测试代码
应整体迁出为独立文件树,而非分散塞进 4 个生产文件末尾(否则最大的生产文件仍会因内嵌测试
显著超预期体积):

```
src-tauri/src/db/queries/layout/tests/
├── mod.rs                    仅 `#[cfg(test)]` + 5 个 `mod` 声明,无逻辑
├── canonical_plan.rs          原 canonical_plan_tests
├── view_to_sql.rs             原 view_to_sql_tests(含 §1.2 所述 4 个跨域测试,原样搬)
├── selection_resolve.rs       原 selection_resolve_tests(50KB,单体最大)
├── format_facet.rs            原 format_facet_tests
└── hidden_root_exclusion.rs   原 hidden_root_exclusion_tests
```

每个测试文件顶部 `use super::super::*;`(即 `crate::db::queries::layout::*`,吃到 mod.rs 的全
量 `pub use`)+ 各自原有的跨域定向 import(路径需从 `super::super::media::…` 改为
`super::super::super::media::…`——多了 `tests` 这一层,纯路径深度调整,非逻辑改动,`cargo check`
会立刻报错,零静默风险)。

`layout/mod.rs` 追加 `#[cfg(test)] mod tests;`。

### 2.4 必须的最小可见性提升(仍属"结构移动",非逻辑改动)

逐一核对测试对生产私有符号的**直接**调用(已用 grep 验证全文件范围,非全部私有 fn 都被测试
直接触达——多数只经 pub 包装函数间接覆盖):

| 符号 | 现可见性 | 现所在(拟) | 被谁直接调用 | 需要的新可见性 |
|---|---|---|---|---|
| `canonical_layout_sql` | 私有 `fn` | `item_queries.rs` | `canonical_plan_tests`(3 处)、`hidden_root_exclusion_tests`(2 处) | `pub(in crate::db::queries::layout)` |
| `sort_canonical` | 私有 `fn` | `item_queries.rs` | `canonical_plan_tests`(1 处) | `pub(in crate::db::queries::layout)` |
| `SELECTION_EXPLICIT_MAX` | 私有 `const` | `selection.rs` | `selection_resolve_tests::explicit_over_limit_errs` | `pub(in crate::db::queries::layout)` |

`push_where_predicates`/`push_order_by`/`push_query_body`/`search_join_needs`
**均未被任何测试直接按名调用**(grep 全文件确认零命中),全部经各自的 `pub fn` 包装间接覆盖,
故**不需要**跟着放宽可见性——保持私有,是本次拆分对"暴露面越小越好"原则的天然收益。

⚠️ 施工复核更正(2026-07-25):`map_layout_item` 实际**被** `canonical_plan_tests` 直接按名调用
(`.query_map([], map_layout_item)`),上表遗漏此项——拆分后该测试迁入 `tests/canonical_plan.rs`
而 `map_layout_item` 留在 `item_queries.rs`,跨文件需同样提升为 `pub(in crate::db::queries::layout)`。

## 3. 风险与不变量

### 3.1 跨文件契约(核心风险)

- **列位置裸约定**:`map_layout_item` 的 18 个 `row.get(N)` 与 `query_layout_items`/
  `canonical_layout_sql`/`query_layout_items_filename_baseline` 三处 SELECT 列表顺序必须逐位
  对齐,无编译期保障,唯一防线是 `query_layout_items_maps_scalar_columns_by_position` 测试
  (现 selection_resolve_tests 内、拟迁 `tests/selection_resolve.rs`)。拆分后四者分处两个文件
  (mapper+3 查询在 `item_queries.rs`,测试在 `tests/selection_resolve.rs`)——**迁移时不得重排
  列顺序或"顺手"格式化这几段 SQL 字符串**,纯剪切粘贴。
- **`push_where_predicates`/`push_query_body` 单一事实源**(T18 §4):`query_layout_items`
  (`item_queries.rs`)与 `view_to_sql`(`query_builder.rs`)共享同一套 WHERE/FROM 构造,严禁在
  拆分中"顺手"给某一侧加影子实现——保持 `item_queries.rs → query_builder.rs` 单向依赖是维系
  这条契约的结构性保障,不能反向依赖或复制。
- **`canonical_layout_sql` 的 unary `+` 压制字符串契约**:`BROAD_BASE` 常量与 `sql.ends_with()`
  精确匹配 `push_where_predicates` 拼出的字面量(`"WHERE m.is_deleted=0 AND m.companion_of IS
  NULL"`),两处分处 `item_queries.rs`/`query_builder.rs` 两个文件后,**任何一侧改动措辞都会让
  `ends_with` 静默失配、回退到索引序随机回表**(1M 库实测 6.6s 退化,无编译错误、无运行时报错,
  只影响性能)。`canonical_default_view_scans_table_not_sort_index` 测试是唯一防线,拆分中必须
  保持通过。
- **V21 隐藏根排除的三套并行实现**:`push_root_exclusion`(`query_builder.rs`,`m` 别名参数化)、
  `list_library_formats` 内联版本(`facets.rs`,无别名)、`query_dir_labels` 不涉及排除——三处
  故意不合一(注释已注明原因是别名不同),拆分后分居三个文件,`hidden_root_exclusion_tests`
  横跨全部四个生产文件断言这条不变量,是拆分后"广度最大"的单个测试模块,回归时应视为集成
  测试整体跑一遍。
- **`push_in_predicate`/`push_root_exclusion` 的 `pub(in crate::db::queries)` 跨域复用**:
  已用 grep 实证 `src-tauri/src/db/queries/search.rs:8` 有 `use super::layout::push_in_predicate;`
  且在 `search.rs:53/58` 直接调用——这是本次拆分**唯一的外部(非 layout 内部)硬依赖**,前移到
  `query_builder.rs` 后若不在 `layout/mod.rs` 做具名重导出(§2.2),`search.rs` 会编译失败。
  `push_root_exclusion` 目前查无外部直接调用点,但其 `pub(in crate::db::queries)` 可见性本身
  即表明设计上预留跨域复用,建议同等重导出以保持对称、免未来复用时二次改动。

### 3.2 rusqlite 红线

- 本次是纯移动,不新增/修改任何 SQL 字符串或参数绑定顺序,`extras.push`/占位符编号算术
  (`push_in_predicate`/`push_root_exclusion`/`push_where_predicates` 内的 `param_idx` 手工递增)
  原样保留——这段"错位不报错、只静默筛错"的算术是全文件对参数绑定最敏感的部分,拆分时严禁
  用编辑器"重新格式化"触碰,只做整段剪切粘贴。
- `layout.rs`(含拆分后四个子文件)内没有任何 `async fn`,全部是同步 `Result<...>` 函数,
  接收 `&Connection`;"async 命令走 spawn_blocking"红线的落点在 IPC 层调用方(如
  `ipc/layout_commands.rs`),不受本次拆分影响,调用方式（函数签名、调用路径）零改动。

### 3.3 性能热路径

- `canonical_layout_sql`/`push_where_predicates`/`push_order_by` 涉及的 SQL 拼装是画廊冷启动
  最热的路径(注释里多处引用 1M 库实测数据:0ms facet 查询、6.6s→顺序扫的 partial-index
  压制、~100ms 的内存补序)。纯结构移动不改变生成的 SQL 字符串本身,故性能不变;唯一风险是
  §3.1 提到的 `ends_with` 契约在移动中被无意改动测量文字。

## 4. 收益与优先级

### 4.1 拆后文件预估大小(实测字节切片估算,四舍五入)

| 文件 | 预估大小 |
|---|---|
| `layout/mod.rs` | ~1-2KB(仅 doc + mod 声明 + 重导出) |
| `layout/item_queries.rs` | ~14KB |
| `layout/query_builder.rs` | ~19.5KB |
| `layout/selection.rs` | ~5.3KB |
| `layout/facets.rs` | ~4.3KB |
| `layout/tests/mod.rs` | <0.5KB |
| `layout/tests/canonical_plan.rs` | ~8.8KB |
| `layout/tests/view_to_sql.rs` | ~19.0KB |
| `layout/tests/selection_resolve.rs` | ~50.1KB(仍是最大单体,见 §4.3 后续优化建议) |
| `layout/tests/format_facet.rs` | ~4.7KB |
| `layout/tests/hidden_root_exclusion.rs` | ~6.5KB |

生产代码从 1 个 44.3KB 文件拆成 5 个(mod.rs + 4 域文件),单文件最大降至 ~19.5KB
(`query_builder.rs`,因含全文件最长单函数 `push_where_predicates` 178 行);测试代码从 1 个
89.1KB 尾巴拆成 6 个,单文件最大降至 ~50.1KB。整体从 1 个 137KB 文件变为 11 个文件,无一
超过 20KB(除 `selection_resolve.rs`)。

### 4.2 建议施工顺序(风险从低到高)

1. **`facets.rs` 先行**:零内部依赖,`list_library_formats`/`query_dir_labels` 互不相关,
   可最先验证"拆分 + mod.rs 重导出 + `cargo check`"这套流程本身是否走通,试错成本最低。
2. **`query_builder.rs`**:抽出即为下游依赖点,须同步落地 §2.2 的 `push_in_predicate`/
   `push_root_exclusion` 具名重导出,并立刻跑一次涉及 `search.rs` 的编译验证(§3.1 最高风险项)。
3. **`item_queries.rs`**:依赖已就位的 `query_builder.rs`,需同步做 §2.4 的 `canonical_layout_sql`/
   `sort_canonical` 可见性提升。
4. **`selection.rs`**:依赖 `query_builder.rs` 的 `view_to_sql`,需同步做 `SELECTION_EXPLICIT_MAX`
   可见性提升。
5. **测试整体搬迁到 `tests/`**:待 1-4 全部落地、`cargo check` 通过后一次性搬(测试互相之间
   无依赖,可并发核对,但建议整体一次提交,避免生产/测试分处两个中间态造成 `cargo test` 长时间
   跑不过)。
6.(可选、低优先级、非本次必做)`selection_resolve.rs` 50KB 仍是单体最大,若后续需要继续瘦身,
   可考虑按其内部天然分区(`canonical_derive_order_matches_sql_order` 等 4 个 derive_order 对拍 /
   `global_rank_*` 4 个 / `resolve_selection`+`count_selection` 行为测试 / 跨域 media 测试)再拆
   3-4 个文件——但这已超出"纯移动一个 `mod` 块"的范畴,需要真正编辑测试代码归属,风险与工作量
   都明显更高,建议单独立项、且需先解决 §1.2 提到的跨域测试归属问题。

### 4.3 收益总结

- 单文件不可读问题解除:并行注释精简会话与未来任何编辑都不必再面对 137KB/2736 行的单文件,
  IDE/搜索/diff 体验直接受益。
- 4 个生产域文件职责边界清晰(mapper+查询 / SQL builder / 选择解析 / facet),便于按 §2.1
  的依赖方向定位改动影响面。
- 测试单独成树后,与生产代码分处两处 diff,`git blame`/review 时"这次改动碰没碰生产逻辑"
  一眼可辨——目前 137KB 文件的 diff 里测试噪音常年淹没生产改动。

## 5. 验证策略

### 5.1 编译与静态检查

- `cargo check -p <crate>`(或 workspace 根 `cargo check`):拆分后第一道也是最基本的门禁,
  §3.1 提到的所有跨文件路径问题(`search.rs` 的 `push_in_predicate`、测试模块的
  `super::super::super::media` 路径深度)都会在此报错,不会静默通过。
- `cargo clippy`:确认拆分中 `use super::*;` 之类的 glob import 没有引入死代码警告
  (拆开后各文件的 `use` 收窄,可能暴露此前被 glob 掩盖的未用 import)。
- `rustfmt`(仅对新拆出的文件跑,不做仓库级 `cargo fmt` 全量——按项目约定,格式化改动与本次
  结构移动分批提交,避免 diff 噪音混入行为验证)。

### 5.2 单元测试

- `cargo test -p <crate> layout::`(或按新路径 `db::queries::layout::`):5 个测试模块整体跑一遍,
  重点关注:
  - `canonical_default_view_scans_table_not_sort_index`(§3.1 `ends_with` 契约的直接哨兵)
  - `query_layout_items_maps_scalar_columns_by_position`(§3.1 列位置契约)
  - `hidden_root_exclusion_tests` 全部 7 个用例(横跨四个生产文件的集成断言)
  - `canonical_derive_order_matches_sql_order`/`filename_derive_order_matches_sql_order`/
    `dual_key_cross_axis_derive_matches_sql_order`/`date_filename_derive_matches_sql_order`
    (SQL 序 ↔ 内存 `derive_order` 对拍,虽然 `derive_order` 本体在 `crate::layout::items_cache`
    不在本次拆分范围,但这几个测试是唯二验证两侧仍逐值等价的地方)
- 涉外调用面(§1 表中"被谁调用"列出的 `ipc/layout_commands.rs`、`ipc/media_commands.rs`、
  `ipc/export_commands.rs`、`ipc/hgallery_commands.rs`、`layout/items_cache.rs`、`layout/cache.rs`、
  `db/queries/media.rs`、`db/queries/export.rs`、`db/queries/faces.rs`、`db/schema.rs`、
  `db/models.rs`、`bin/sort_profile.rs`)不需要新增测试——它们只经 `crate::db::queries::*`
  扁平路径消费,拆分对其调用点是编译期透明的,`cargo check` 覆盖即为充分验证。

### 5.3 行为不变性验证

- 因为是纯结构移动、零 SQL/逻辑改动,行为不变性的证据链就是:(a) 移动前后 `cargo test` 涉及
  本域的用例全绿、且用例数量不变(可用 `cargo test -- --list` 对拆分前后测试计数做差异对比,
  确认没有测试在搬迁中被遗漏);(b)对 §3.1 列出的高风险契约(列位置、`ends_with` 压制字符串、
  隐藏根三套实现)逐条人工复核 diff,确认迁移后的字符串字面量与原文件逐字节一致(`git diff`
  应只体现"删除整块 + 在新文件新增整块",内容侧无改动)。
- 不需要新增性能基准:`bench_canonical_fat_table_1m`(`canonical_plan_tests` 内既有基准测试)
  随迁移一并搬入 `tests/canonical_plan.rs`,继续作为性能不回退的既有证据源,无需额外新建。
