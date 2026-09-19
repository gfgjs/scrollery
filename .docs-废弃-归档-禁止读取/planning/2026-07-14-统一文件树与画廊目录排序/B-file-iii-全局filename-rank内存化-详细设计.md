---
id: 2026-07-14-B-file-iii-全局filename-rank内存化-详细设计
status: active
type: design
line: 统一文件树与画廊目录排序
created: 2026-07-14
# supersedes 无:B-file 家族新增实现路径,与 B-file-ii 持久列并列(原散文值删除,图不变量要求 id 或缺省)
relates:
  - 方案B-持久化排序键列-详细设计.md（§14.3.2 root cause 隔离实证 = 本设计的证据基座）
---

# B-file-iii：全局 filename rank 内存化 — filename 排序卡顿根治（详细设计）

> ⚠️ **本文件是待审核设计，尚未施工。** 施工须用户显式 go。B-file-ii（持久字节键列，D-013）仍按红线**独立待 go**，不因本设计而启动。
>
> 🔴 **2026-07-15 施工前核实 + 用户裁决：D-018 定为「方案 A′ 全面 UTC 桶」，本设计原「方案 A 全面本地桶」及 F-006 的时区判断被推翻。** 追代码坐实 `sort_datetime` 是**「EXIF 墙钟当 UTC 存」**（[`metadata.rs:328-333`](../../../src-tauri/src/scanner/metadata.rs) `parse_exif_datetime` 忽略时区、以 `Utc.with_ymd_and_hms` 存；[`enricher.rs:311-314`](../../../src-tauri/src/scanner/enricher.rs) `COALESCE(exif,mtime)` 后不再套偏移），故显示标签（`timestamp_to_date_label`/`timestamp_to_year_month` 均 `Utc`）与 date+datetime 分桶（`div_euclid(86400)` UTC）**都用 UTC 且正确**；反倒是 SQL date+filename 的 `date(…,'localtime')` **在墙钟上再叠一次本地时区 = 双重偏移的 latent bug**（UTC+8 机器上傍晚 EXIF 照片错分次日）。∴ 正确方向是**全库统一到 UTC 桶**：date+filename 内存派生用 `sort_datetime.div_euclid(86400)`（每项自带、零新列），并把 `push_order_by` 与 `enrichment_order_clause` 的 date+filename `ORDER BY` 去掉 `'localtime'`（改 UTC 日界）以维持 SelectAll 逐值等价。**run_layout / 标签 / date+datetime 一律不动**；唯一用户可感变化 = date+filename 傍晚照片分桶从错日回到正确日（与 date+datetime 一致），是修 bug 非位移。正文 §3.3 / §6 阶段3 / §7 D-018 的「本地桶」表述以本横幅为准，完整合并待收口。

## 0. 定位与命名（在 B-file 家族中的位置）

`sort_within_group = filename` 的排序性能治理，family 已有两条已定路径 + 一条本设计新增：

| 成员 | 做法 | 状态 |
|---|---|---|
| **B-file-i** | 运行时内存 filename 基准序 + 整数 `filename_rank`（**每 filter 一份**，`ensure_filename_rank_for_hit` 惰性建）；双键统一缓存（D-015）令 datetime↔filename 跨轴切换走内存派生 | ✅ landed（`779df5a` / `4ea7b68`） |
| **B-file-ii** | 持久 `media_items.file_name_sort_key BLOB` 字节键列，`ORDER BY` 走 BINARY memcmp，SQL 层消灭 collation | ⏸ 后手，须显式 go（D-013） |
| **B-file-iii（本设计）** | **全局 filter-invariant** 内存 `filename_rank`（**全库一份**，开机后台建一次）+ 取数基准恒为 datetime + date 轴亦内存派生 → **所有** filename 视图彻底不再跑 SQL collation 排序 | 📄 待审核 |

**B-file-iii 相对 B-file-i 的增量**（三点）：
1. **rank 从「每 filter 一份」升为「全库一份、filter-invariant」**：filename 次序是与 filter 无关的**全序**（A 的文件名 < B 的，在任何子集里都成立），故一份全局 rank 服务所有筛选子集。切 filter 不再重建 rank。
2. **取数基准恒为 datetime（canonical，便宜、走索引、无 collation FFI）**：MISS 一律 `query_layout_items_canonical` 取数，filename 序由全局 rank 内存派生——**彻底删除 `query_layout_items_filename_baseline` 这条全表 NATURAL_CMP 排序路径**。
3. **date+filename 亦可内存派生**：借「SQL 把日期桶作为一列带回」消灭 UTC/本地对齐风险（§3.3），使单槽缓存服务**全部** group×sort 组合、根治 folder↔date 切分组 thrash。

**与 B-file-ii 的关系**：两者都消除热路径上的 NATURAL_CMP FFI，区别在**在哪里算自然序、算几次**——iii 每会话在内存算一次（整数 rank），ii 全库算一次并持久化为字节（跨重启免重建）。**iii 是低风险主攻，ii 是可选后续**（§9）。二者可叠加（ii 落地后可把 iii 的一次性 rank 构建从 NATURAL_CMP 排序改为读字节键 BINARY 排序，把构建成本 546ms→201ms），但**互不阻塞**。

## 1. 问题与证据（本设计的事实基座）

用户 2026-07-14 真机报「顶栏点筛选（视频/收藏/星级）、切分组模式等待过长」——dev >5s、release >1s。三条 `compute_layout MISS`（axis=`folder/filename` ↔ `date/filename`，全库 543,449 项）`sql` 分项 5227–5541ms（dev）。

根因已由 `sort_profile` **决定性隔离段**坐实（详见 [方案B 详细设计 §14.3.2](方案B-持久化排序键列-详细设计.md)，提交 `b203262`）：

| id-only,543k | dev NATURAL→BINARY | release NATURAL→BINARY | 纯 collation FFI 占比 |
|---|---|---|---|
| none/filename | 4670→341ms | 570→201ms | **93% / 65%** |
| date/filename | 4008→508ms | 581→308ms | 87% / 47% |
| datetime 对照 | 304ms | 158ms | — |

- **瓶颈 = 自研 `NATURAL_CMP` collation 的 SQLite→Rust FFI**：换成内建 `BINARY`（memcmp）后 filename 排序即跌到 datetime 对照水平；`EXPLAIN` 三者结构一致（皆 `USE TEMP B-TREE`）→ join / 18 列物化 / filesort 结构**都不是**瓶颈。
- **dev 放大唯一归因该 Rust FFI**：非 FFI 活儿 dev/release ≈1.8×，纯 FFI 差值 dev/release ≈12×。
- **两个叠加问题**：① filename 基准排序贵（folder/filename MISS 走 `filename_baseline` 全表 NATURAL_CMP = 891ms release / 5279ms dev）；② `date+filename` 不可内存派生 → 每次走 SQL，folder↔date 来回切打穿单槽缓存 = thrash。B-file-iii 一并根治①②。

## 2. 核心思路

**一句话**：把「每次 MISS 在 SQL 里用 NATURAL_CMP 给 filename 排序」替换为「全库只算一次自然序位次（内存整数 rank），此后所有视图从 datetime 基准 + 该 rank 内存派生」。

**三条支柱**：

1. **全局 filter-invariant rank（§3.1）**：`AppState` 持一份 `GlobalFilenameRank { data_version, id_to_rank: HashMap<i64,u32> }`，开机后台建一次（`query_item_ids_filename_order` **不带 filter**，全库 NATURAL_CMP → id→位次），`data_version` 变更（重扫/导入）时后台重建。~2–6MB，全局共享。

2. **取数基准恒 datetime + 内存派生一切（§3.2）**：`compute_layout` MISS 一律走 `query_layout_items_canonical`（datetime 序，无 collation，走 `idx_media_sort`），`CachedOrder::Canonical` 常驻；请求 filename 序时 `derive_order` 用全局 rank 派生。**删除 `filename_baseline` 取数分支**。单槽缓存对**任意** group×sort 组合皆 HIT（含跨 filter 后的首次填充）。

3. **date+filename 用 UTC 日桶内存派生（§3.3，🔴 D-018 A′ 已定，非原「借 SQL 本地桶列」）**：`derive_order` 的 date+filename 分支按 `(sort_datetime.div_euclid(86400), 全局rank, id)` 排序 —— UTC 日桶**每项自带 `sort_datetime` 内存直算**（与 run_layout 分组同算式），**无需新 SQL 列**。同步把 SQL `push_order_by` / `enrichment_order_clause` 的 date+filename `ORDER BY` 去掉 `'localtime'` 改 UTC 日界，两侧**构造性逐值等价**。∵ `sort_datetime` 是 EXIF 墙钟当 UTC 存（`metadata.rs::parse_exif_datetime`），UTC 桶才与显示标签/月桶/date+datetime 分组一致，原 `'localtime'` 是双重时区偏移 bug，本轮顺带修正。

**用户可感收益**：filename 轴的点筛选 / 切分组从 dev 5.5s / release ~900ms 降到 **~16–27ms 内存派生**；一次性全局 rank 构建 546ms（release）/ 4651ms（dev）藏于开机后台，用户无感。

## 3. 机制详解

### 3.1 全局 filename rank：结构与生命周期

**结构**（`AppState` 新增字段，`state.rs`）：

```rust
/// 全库 filename 自然序位次（filter-invariant）。id → 该 id 在「全部可见媒体项
/// 按 file_name COLLATE NATURAL_CMP ASC」中的稠密位次 [0, N)。
/// filter 无关：任意筛选子集的 filename 序 = 子集按此位次整数排序（自然序是全序）。
pub struct GlobalFilenameRank {
    pub data_version: u64,          // 构建时的数据代；与当前不符即失效
    pub base_scope: BaseScope,      // 覆盖的基集（默认可见集 = is_deleted=0 AND companion_of IS NULL）
    pub id_to_rank: HashMap<i64, u32>,
}
pub type GlobalRankCell = RwLock<Option<GlobalFilenameRank>>;
// AppState { …, pub global_filename_rank: GlobalRankCell }
```

**构建**（`build_global_filename_rank`，`spawn_blocking` 内）：
- 单次 `query_item_ids_filename_order(&conn, &MediaFilter::default())`（**不带 filter**，全库 id-only NATURAL_CMP，真机 546ms release / 4651ms dev）→ `Vec<i64>`（按自然序）。
- 枚举下标即位次：`for (rank, id) in ids.enumerate() { id_to_rank.insert(id, rank as u32) }`。
- 写入 `RwLock`，记 `data_version`。

**触发时机**：
1. **开机后台**：`startup` 后台任务（不阻塞首帧）预建，使用户第一次切 filename 时 rank 已就绪。
2. **`data_version` 变更后台重建**：重扫 / 导入 / 移动令 `data_version` bump（现有 `invalidate` 点：`scan_commands.rs:525`、`config_commands.rs` 三处）→ 触发后台重建；重建期间旧 rank 标记 stale（`data_version` 不符）。
3. **惰性兜底**：若某次 filename MISS 时 rank 未就绪 / stale，则该次**退化**为现有 B-file-i 路径（`filename_baseline` 或 per-filter `ensure_filename_rank_for_hit`），不阻塞、不坏序（§3.5 回退）。

**为何 filter-invariant 成立**（正确性论证）：filename 自然序是全部可见项上的**全序**（`natural_cmp` 是全序关系）。对任意筛选子集 S，「S 按 filename 排序」= 「S 按各项全局位次整数排序」——限制全序到子集保持相对序。∴ 一份全局位次服务所有 filter 子集，无需按 filter 重算。**契约测试** `global_rank_restricts_to_filtered_order`（§4）锁定。

**成本**：`HashMap<i64,u32>` over 543k ≈ 6.5MB（全局一份，非每 filter）。构建一次性 546ms release，藏后台。相比 B-file-i 每次 filter 变都跑一次 `ensure_filename_rank_for_hit`（真机 546ms），iii 把 N 次筛选的 N 次 rank 构建降为 1 次。

### 3.2 取数基准恒 datetime + 内存派生一切

**现状**（`layout_commands.rs:302-317`）：MISS 按请求轴分三路取数——datetime 走 canonical、filename+none/folder 走 `filename_baseline`（891ms release 全表 NATURAL_CMP）、其余走通用 `query_layout_items`。

**改造**：MISS **一律** `query_layout_items_canonical`（datetime 序，无 collation），`CachedOrder::Canonical` 常驻。filename 序由 `derive_order` 用全局 rank 派生（§3.1 就绪时）：

```rust
// compute_layout MISS 取数（简化）：
let items = query_layout_items_canonical(&pool, &filter)?;   // 便宜、走索引、无 FFI
let order = CachedOrder::Canonical;                            // 恒一
// filename 序在 derive_order 内经全局 rank 派生（不再单独取 filename 基准）
```

- **删除** `is_filename_derivable ? filename_baseline` 这条取数分支（该分支正是 folder/filename MISS 的 891ms 来源）。
- `derive_order` 的 filename 派生：把 per-cache `data.filename_rank` 的来源从「per-filter 查询」改为「读全局 `id_to_rank` 映射」，构建与 items 平行的 `ranks: Vec<u32>`（`ranks[i] = global.id_to_rank[items[i].id]`）——即现 `ensure_filename_rank_for_hit` 的映射步骤，**但数据源换成全局 map、免 DB 查询**。
- **单槽缓存对任意 group×sort 皆 HIT**：基准恒 Canonical，`can_derive_axis` 放行全部组合（§3.3 补 date+filename）→ 换 group / 换 sort / 换方向都不换基准、不 evict、不重查 = **thrash 消灭**。

**换 filter 的路径**（用户「点筛选」）：filter 变 → `filter_key` 变 → 必 MISS（新 item 子集须查 DB）。但改造后：
- 取数走 canonical（datetime 序 + filter WHERE，走索引，无 collation）——比现 `filename_baseline`+filter 便宜一个量级。
- filename 序经全局 rank 内存派生（filter-invariant，无需为该 filter 重算 rank）。
- ∴ 换 filter 的 filename MISS 从「filtered NATURAL_CMP 排序」降为「便宜 datetime 取数 + 整数派生」。

### 3.3 date+filename 内存派生：全面 UTC 桶（🔴 D-018 A′ 已定并落地）

**时区根因（施工前核实，推翻原「本地桶正确」判断）**：`sort_datetime` 是**「EXIF 墙钟当 UTC 存」**——`scanner/metadata.rs::parse_exif_datetime` 把 `"YYYY:MM:DD HH:MM:SS"`（墙钟、无时区）以 `Utc.with_ymd_and_hms(...).timestamp()` 存（注释「Simple UTC timestamp (ignores timezone)」），`enricher.rs::311` `COALESCE(exif,mtime)` 后不再套偏移。∴：
- **UTC 桶才正确**：显示标签 `timestamp_to_date_label` / 月桶 `timestamp_to_year_month` 均 `Utc`（`justified.rs:687/702`，注释引 T9），date+datetime 分组用 `div_euclid(86400)`（`justified.rs:237`）亦 UTC——三者一致，给回墙钟拍摄日。
- **SQL date+filename 的 `date(...,'localtime')` 才是错项**：在已是墙钟的时间上再叠一次本地时区 = 双重偏移（UTC+8 机器傍晚 EXIF 照片错分次日）——是 date+filename 这一格的 **latent bug**，非「无法对齐」的对称困难。

**解法（A′ 全面 UTC 桶，零新列）**：
- `derive_order` date+filename 分支按 `(sort_datetime.div_euclid(86400) {方向}, 全局rank {方向}, id {方向})` 排序——UTC 日桶**每项自带 `sort_datetime` 内存直算**（与 run_layout 分组 `group_mark` 同算式），**无需 SQL 日期列、无 `LayoutItem` 新字段**。
- SQL `push_order_by`（`queries.rs`）+ `enrichment_order_clause`（`enricher.rs`）的 date+filename `ORDER BY` **去掉 `'localtime'`** 改 `date(m.sort_datetime,'unixepoch')` UTC 日界，与内存 `div_euclid` **构造性逐值等价**（SelectAll 刚性对拍锁定）。
- `can_derive_axis` 放行 `date + filename`；`is_filename_derivable` 扩到 `none|folder|date`。
- **run_layout 分组 / 标签 / date+datetime 一律不动**（已 UTC 且正确）。唯一用户可感变化 = date+filename 傍晚照片分桶从错日回到正确日（与 date+datetime 一致），是**修 bug 非位移**。

**落地实证**：新增 `date_filename_derive_matches_sql_order`（跨 3 UTC 日 fixture、双基准、手钉期望向量），SQL(去 localtime UTC) == 内存(`div_euclid`) 逐项一致，且对拍**与测试机时区无关**（旧 localtime 会机器相关——这正是旧设计把 date+filename 排除在内存派生外的历史原因）。

### 3.4 derive_order 泛化点（在 D-015 基础上）

D-015 已把 `derive_order` 参数化为「按请求轴 `sort_within`」（同键反转 / 跨键实排）。B-file-iii 追加：
- filename 次键来源：per-cache `filename_rank`（其数据源在 §3.2 由「per-filter 查询」改为「优先全局 `id_to_rank` 映射」）。
- date 轴对 filename：新增独立 `(sort_datetime.div_euclid(86400) UTC 日桶, 全局rank, id)` keyed 排序分支（§3.3 A′，置于 folder 之后、none/date 通用分支之前——同键反转快捷只输出纯 filename 序、不含日分桶，对 date 轴无效）。
- `can_derive_axis`：`filename => group ∈ {none, folder, date}`（A′ 全纳）。

### 3.5 回退与安全（best-effort，绝不坏序）

- **rank 未就绪 / stale**：filename MISS 见全局 rank `data_version` 不符或 `None` → 该次退化为**现有 B-file-i 路径**（`filename_baseline` 或 per-filter `ensure_filename_rank_for_hit`），正确但慢一次，不阻塞、不坏序。与现 `order_ok` 见 rank 未就绪即退 MISS 同型。
- **id 不在全局 map**（data_version 竞态：新项已入库但 rank 未含）：`id_to_rank.get(id)` 缺失 → 该次整体退化 B-file-i 路径（**不**用 `u32::MAX` 兜，避免坏序），而非单项塞末尾。判据：派生前校验「本次 items 的 id 全在 map 内且 data_version 相符」，任一不满足即退化。
- **非默认基集视图**（回收站 `is_deleted=1` / 含隐藏项的特殊相册）：其基集 ≠ 全局 rank 的 `base_scope` → filename 序不可用全局 rank 派生 → 退化 B-file-i 路径。绝大多数 filter（视频/收藏/星级）是默认可见集的**子集**，全覆盖;特殊基集是少数、退化可接受。契约测试 `global_rank_scope_guard` 锁定退化触发。

## 4. 刚性契约与对拍（新增 / 扩展测试）

filename 内存派生序**必须与 SQL `push_order_by` 逐项一致**，否则 SelectAll（`view_to_sql`）与 flat_ids（内存派生）错位 → 选区漂移。沿用 D-015 已建的对拍范式（fixture 取 `img2 < img10 < img100` 锁 NATURAL_CMP 确被走到）。

| 测试 | 锁定 | 新增/扩展 |
|---|---|---|
| `global_rank_matches_filename_baseline` | 全局 rank（全库 id-only）赋出的位次序 == `query_layout_items_filename_baseline` 的 id 序 | 新增 |
| `global_rank_restricts_to_filtered_order` | 任取 filter 子集，按全局 rank 整数排序 == 该 filter 下 SQL filename 序（证 filter-invariance） | 新增 |
| `date_filename_derive_matches_sql_order` | date+filename 内存派生（`(local_epoch_day, 全局rank)`）== SQL `ORDER BY date(...,'localtime'), file_name NATURAL_CMP`（跨午夜 fixture：同一 UTC 日跨两本地日 / 同一本地日跨两 UTC 日各造样本） | 新增（方案 A） |
| `dual_key_cross_axis_derive_matches_sql_order` | 现有双向对拍（datetime↔filename，none/folder） | 扩至 date 轴（方案 A） |
| `global_rank_scope_guard` | 非默认基集 / data_version 竞态 / id 缺失 → 退化 B-file-i，不以坏序命中 | 新增 |
| `canonical_derive_order_matches_sql_order` | datetime 既有对拍不回归 | 保持 |

## 5. 诚实边界与风险

- **一次性构建不消除**：全库 rank 构建 546ms（release）/ 4651ms（dev）**仍要付一次**（每 data_version）。藏于开机后台使首次交互无感；但导入 / 重扫频繁的会话会反复重建（每次 ~546ms 后台，不阻塞前台但耗 CPU）。**未消除的是 filename 自然序的首次计算本身**——那与 B-file-i / B-file-ii 同样不可免（ii 把它挪到写时增量维护）。
- **date+filename 行为变化（若走方案 A）**：date 分隔符从 UTC 桶改本地桶，非 UTC 用户午夜前后的项归日相对现状位移。须用户确认接受（§3.3 决策 D-018）。走方案 B 则零行为变化但 thrash 部分残留。
- **内存**：全局 `HashMap<i64,u32>` ~6.5MB；per-cache filename ranks Vec ~2MB（现已有）。可接受。
- **data_version 竞态**：新项入库到 rank 重建完成之间的 filename MISS 退化 B-file-i（§3.5），慢一次不坏序。
- **不覆盖非默认基集**：回收站 / 含隐藏项视图退化 B-file-i（§3.5）。
- **profile 是纯后端**：真机手感还叠 IPC 序列化 + Vue 渲染；后端消除的是其中最大项（全表重排）。**真机 GUI 手感验收仍 pending**（CI/GUI 不覆盖）。

## 6. 施工阶段（每阶段独立可验，末尾各一提交）

> 施工须显式 go。每阶段先 `cargo test`（定向 + 一次全量）+ `rustfmt --check`（单文件，**不** `--all`）+ `cargo clippy -- -D warnings`；相同失败连续两次即停、重读源码。精确 pathspec 提交（并行前端会话在途）。

**阶段 1：全局 rank 结构 + 构建 + 生命周期**
- [ ] `state.rs`：`AppState` 加 `global_filename_rank: GlobalRankCell` + `GlobalFilenameRank` 结构。
- [ ] `items_cache.rs`（或新 `filename_rank.rs`）：`build_global_filename_rank(conn)` + `get_or_none(state, data_version)` + 失效钩子（挂现有 `invalidate` 点）。
- [ ] 开机后台预建任务（不阻塞首帧）；`data_version` bump 后台重建。
- [ ] 单测：`global_rank_matches_filename_baseline`、`global_rank_restricts_to_filtered_order`。
- **状态：** pending

**阶段 2：取数基准恒 datetime + derive_order 读全局 rank**
- [ ] `layout_commands.rs`：MISS 取数删 `filename_baseline` 分支，一律 `query_layout_items_canonical`；`CachedOrder` 恒 `Canonical`。
- [ ] `derive_order` filename 次键来源改读全局 `id_to_rank`（映射构建 = 现 `ensure_filename_rank_for_hit` 步骤，去掉 DB 查询）；rank 未就绪 / stale → 退化 B-file-i（§3.5）。
- [ ] `can_derive_axis` 维持 `{none, folder}`（date 轴留阶段 3）。
- [ ] 对拍不回归：`canonical_derive_order_matches_sql_order`、`filename_derive_order_matches_sql_order`、`dual_key_cross_axis_derive_matches_sql_order`。
- [ ] 新增 `global_rank_scope_guard`。
- **状态：** pending（此阶段已根治①与「换 filter / folder↔none/folder filename」thrash，date 轴除外）

**阶段 3 🔴（D-018 裁定 A′ 全面 UTC 桶——比原「本地桶」更简单：零新列、不改正确视图）：date+filename 内存派生**
> 施工时提前为「阶段 1」（thrash 根治，最高价值最低风险、自包含），先于全局 rank 基建落地。
- [ ] `items_cache.rs`：`can_derive_axis` 放行 `date+filename`；`derive_order` 加 date+filename 的 `(sort_datetime.div_euclid(86400) {方向}, filename 位次 {方向}, id {方向})` 分支（位次：CanonicalFilename 基准=下标、Canonical 基准=`filename_rank[i]`）；UTC 桶每项自带 `sort_datetime`，**无需新列/新字段**。
- [ ] `queries.rs::push_order_by` + `scanner/enricher.rs::enrichment_order_clause`：date+filename `ORDER BY date(m.sort_datetime,'unixepoch','localtime')` **去掉 `'localtime'`** 改 UTC 日界，与内存 `div_euclid` 构造性逐值等价。
- [ ] `layout_commands.rs`：`is_filename_derivable` 由 `(none||folder)` 扩到 `(none||folder||date)`，date+filename fresh MISS 走 filename_baseline→CanonicalFilename（可派生），切轴切分组即 HIT。
- [ ] `justified.rs` run_layout 分桶/标签 **不动**（已 UTC 且正确）。
- [ ] 新增 `date_filename_derive_matches_sql_order`（跨 UTC 午夜 fixture）；扩 `is_hit_valid` 断言 date+filename 可派生。
- **状态：** pending（A′ 已裁；顺带修正 SQL date+filename 的 localtime 双重时区 bug）

**阶段 4：真机验收 + 收口**
- [ ] 用户真机大库：点筛选 / 切分组 / 跨轴切换 filename↔datetime，实测降到 ~内存派生级；首次进入 filename 首帧无 rank 构建卡顿（后台预建生效）。
- [ ] 重扫 / 导入后 rank 重建期间不坏序（退化验证）。
- [ ] 归档三件套 / worklogs closeout。
- **状态：** pending（GUI/CI 不覆盖，须用户环境）

## 7. 关键决策（续 §16 D-011..D-015）

| 决策 | 理由 | ID |
|---|---|---|
| filename 排序卡顿走 **B-file-iii 全局内存 rank**（非直接上 B-file-ii 持久列） | 隔离实证证明瓶颈是 collation FFI、非 SQL 结构；全局 rank 以一次性 546ms + 零迁移 + 低风险（复用现成 `natural_cmp`、无字节键编码）拿到主要收益；ii 的持久化增益（免每会话重建）相对非阶跃、且背编码风险，作后续 | D-016 |
| rank **全局 filter-invariant**（全库一份），非 B-file-i 的每 filter 一份 | filename 序是与 filter 无关的全序 → 一份服务所有子集；把 N 次筛选的 N 次 rank 构建降为 1 次 | D-017 |
| ~~date+filename 派生须统一到 SQL 本地日期桶列~~ 🔴 **2026-07-15 裁定：方案 A′ 全面 UTC 桶**（前提反转，见顶部横幅） | 核实 `sort_datetime` = 墙钟当 UTC 存 → UTC 桶（现 date+datetime/标签/月桶）才正确，SQL date+filename 的 `localtime` 是双重时区偏移 bug。∴ date+filename 内存派生用 `sort_datetime.div_euclid(86400)`（零新列），`push_order_by`+`enrichment_order_clause` 去 `'localtime'` 保 SelectAll 等价；run_layout/标签/date+datetime 不动，唯一变化=date+filename 傍晚照片回到正确日 | D-018 |
| rank 未就绪 / stale / 非默认基集 → **退化 B-file-i,不以坏序命中** | best-effort:慢一次可接受,坏序（选区漂移）不可接受;与 D-015 `order_ok` 退 MISS 同型 | D-019 |
| 取数基准**恒 datetime canonical**、删 `filename_baseline` MISS 分支 | canonical 无 collation FFI、走索引（158-319ms）；filename 序全部内存派生 → 单槽缓存对任意 group×sort 皆 HIT，thrash 根治 | D-020 |

## 8. 验收标准

- filename 轴的点筛选 / 切分组 / 跨轴切换,后端时延从 SQL 全表 NATURAL_CMP（release ~900ms / dev ~5s）降到内存派生量级（release ~16–27ms）;埋点 / 日志 `compute_layout` 由 MISS 重查转为 HIT 派生。
- 全局 rank 构建藏后台,用户首次进入 filename 视图无可感构建卡顿(rank 已就绪);未就绪时退化正确(不坏序)。
- 所有对拍绿（§4）：filename / date+filename（若方案 A）内存派生 == SQL 精确序,逐项一致;datetime 既有对拍不回归。
- 换 filter（视频/收藏/星级）后 filename 序正确且快;重扫/导入后 rank 重建期间不坏序。
- fmt + clippy `-D warnings` + 全量 lib test 绿（本地,标注非 CI）；真机 GUI 手感由用户验收。

## 9. 与 B-file-ii（持久字节键列）的关系 —— 为何 iii 先、ii 仍后手

- **iii 拿到主要收益、风险低**：消除热路径 collation FFI（用户实际痛点),复用现成 `natural_cmp`,零迁移,无字节键编码风险。
- **ii 的独有增益是「免每会话重建」+「纯 filename 视图可建索引消 filesort」**:但——① 一次性构建 546ms 藏后台后本就低扰;② 隔离实证的 BINARY 列显示,分组配置(folder/date + filename)下 filesort 仍在(`USE TEMP B-TREE`),ii 只在**纯 filename 无分组**视图才可能消 filesort（阶跃），该视图是否热点未知(待埋点);③ 字节键编码 = [[lexicmp-natural-cmp-overflow]] 溢出坑近亲,须单独设计 + parity,+ 10⁶ 行 V20 迁移 + 每次 rename/move/重扫写触点。
- **叠加而非互斥**:ii 落地后,iii 的一次性 rank 构建可从「全库 NATURAL_CMP 排序 546ms」改为「读 `file_name_sort_key` BINARY 排序 ~201ms」——iii 的后台构建提速 2.7×。故 **iii 先行不挡 ii,ii 是 iii 之上的可选加速**。
- **裁决**:维持 D-013——**B-file-ii 仍须独立显式 go**,不因本设计启动。仅当 iii 落地后「一次性构建 / 重扫重建」被证明扰民,且埋点证明纯 filename 无分组为热点时,才评估 ii。

---

## 修订记录

- **v1.0（2026-07-14）** —— 初稿。用户 go「出方案 1 并落盘三件套备审计」。基于 `sort_profile` 决定性隔离实证（[方案B §14.3.2](方案B-持久化排序键列-详细设计.md)，提交 `b203262`：collation FFI 占 filename MISS 93% dev / 65% release）。核实到代码：`push_order_by` date+filename 用 SQL 本地日期桶（`queries.rs:1508/1516`）、date+datetime 仅按 `sort_datetime` 排序（`:1520`）；run_layout 日期分组用 UTC 桶 `div_euclid(86400)`（`justified.rs:219/237/277`）——**两者既有不一致**是「date+filename 无法内存对齐」的真正根因（副发现 F-新增）。设计三支柱：全局 filter-invariant rank（D-017）+ 取数恒 datetime 删 `filename_baseline`（D-020）+ date+filename 借 SQL 桶列消对齐风险（D-018 待裁 A/B）。**待用户审核后施工**；B-file-ii（D-013）仍独立待 go。

- **v2.0（2026-07-15）—— D-018 裁定 A′ + 三阶段施工落地（本地验证，非 CI）**。用户 go「选 A，开始施工」。**施工前核实推翻 v1.0 的时区前提**：追代码坐实 `sort_datetime` = EXIF 墙钟当 UTC 存（`metadata.rs:328-333` `parse_exif_datetime` 忽略时区 + `enricher.rs:311` 不套偏移）→ **UTC 桶正确、SQL date+filename 的 `localtime` 才是双重时区偏移 bug**（v1.0/F-006 判反）。故 D-018 由「(A) 本地桶 / (B) 暂留 SQL」改判 **A′ 全面 UTC 桶**：date+filename 内存派生 `sort_datetime.div_euclid(86400)`（**零新列**，比原方案 A 更简单、不改正确视图），SQL 去 `'localtime'`，顺带修 localtime bug。§2.3 / §3.3 / §3.4 / §6 阶段3 / §7 D-018 已改写为 A′。
  **三阶段全部 landed（提交 `d928271` / `55bd4a4` / `aeed4e4`，`cargo test --workspace` 522/0/5 + rustfmt + clippy `--workspace -D warnings` 全绿，本地非 CI）**：
  - 阶段1（`d928271`）= date+filename UTC 内存派生（`can_derive_axis`+`derive_order` 独立 date+filename 分支；`push_order_by`+`enrichment_order_clause` 去 `'localtime'`；`is_filename_derivable` 扩 date）+ 对拍 `date_filename_derive_matches_sql_order`（跨 3 UTC 日双基准）——根治换轴 thrash（Pain 2）。
  - 阶段2（`55bd4a4`）= 全局 filter-invariant rank 基建（`GlobalFilenameRank`{data_version,id_to_rank}+`build_global_filename_rank`+`AppState` 槽/`try_global_filename_ranks`/`spawn_global_filename_rank_build` 幂等后台构建 + 开机延迟 15s 预建）+ 对拍（rank==baseline / 限制到 favorited 子集==子集 SQL 序证 filter-invariance）。
  - 阶段3（`aeed4e4`）= MISS 取数恒 canonical（删 `filename_baseline` 取数分支，`CachedOrder` 恒 `Canonical`）+ `resolve_filename_ranks`（优先全局免 DB、兜底 per-filter 查询+触发后台构建）+ `ensure_filename_rank_for_hit` 全局命中快路径 + `GlobalFilenameRank::ranks_for` 纯方法+单测——根治 filename 筛选 891ms（Pain 1）。
  - **遗留**：阶段4 真机 GUI 手感验收（点筛选/切分组/跨轴切换 filename↔datetime、开机预建无卡顿、重扫重建不坏序）——GUI/CI 不覆盖，须用户环境。相邻发现 F-007（搜索谓词 `queries.rs:1422/1455` 亦用 `localtime` 格式化匹配日期，同源不一致，**未在本轮改**）见 findings。B-file-ii（D-013）仍独立待 go。


