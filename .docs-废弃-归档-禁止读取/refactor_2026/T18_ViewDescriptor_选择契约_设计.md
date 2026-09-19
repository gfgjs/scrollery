---
id: 2026-06-29-T18_ViewDescriptor_选择契约_设计
status: active
type: design
line: 选择契约 T18
created: 2026-06-29
---

# T18 / T14.5 ViewDescriptor + 百万级选择契约 设计文档

> **状态：🟢 决策已采纳为当前默认（**开发期可改,非定死**），施工中（2026-06-30）。** 本文把 Part2 §3.10 的设计草图，结合**真实代码现状**，落成一份可直接实施的完整设计。落地属 P0（Part5 全选/批量的硬前置，见 Part5:317 T4 ← Part2 T14.5）。决策裁定见文末 §10 后「决策（当前默认）」小节。
> 🔴 **「拍板/冻结」措辞已取代（2026-06-30,用户约束「开发期不冻结契约」）**:D1–D6 是**当前默认选择,未经实测比较/市场验证**,开发期可随数据演进;其中 **D3(精确计数)/D5(companion 展开)** 是未验证的策略默认,**D4** 阈值已自带「可调」。见记忆 `no-contract-freeze-during-dev`。
> **目标**：用一个不可变 `ViewDescriptor` 确定性复现「当前画廊视图全集 + 序」，并以 `SelectionDescriptor` 表达选择，使**百万级库的全选/批量操作既不把 id 灌进前端内存、也不经 IPC 整包传 id**，由后端按当前 filter 在 SQL 层解析。
> **本文不改代码**，仅设计 + 决策点。文末「决策点」需你拍板后方可开工。

---

## 1. 现状盘点（带 `文件:行号` 锚点，设计须扎根于此）

### 1.1 视图过滤已有事实源：`MediaFilter`
`src-tauri/src/db/models.rs:335` 已有 `MediaFilter`（即计划里 `GalleryFilter` 的雏形），字段：
```rust
pub struct MediaFilter {
    pub media_types: Option<Vec<String>>,   // image/video/audio/...
    pub live_photo_only: Option<bool>,
    pub favorited_only: Option<bool>,
    pub min_rating: Option<i64>,
    pub date_range: Option<DateRange>,       // { from:i64, to:i64 }
    pub directory_id: Option<i64>,           // 递归子树（见下 CTE）
    pub album_id: Option<i64>,               // 用户收藏夹（album_items 成员）
    pub person_id: Option<i64>,              // 人脸簇（F6 人物墙）
    pub search_query: Option<String>,
    pub search_scope: Option<String>,        // filename/device/location/global
    pub ai_search: Option<bool>,
    pub ai_threshold: Option<f64>,
    pub trashed_only: Option<bool>,          // is_deleted=1 视图
    pub recent_only: Option<bool>,
}
```

### 1.2 `view_to_sql` 其实已隐式存在：`query_layout_items`
`src-tauri/src/db/queries.rs:780` 的 `query_layout_items(conn, &MediaFilter, group_by, sort_within, sort_order, _include_meta)` **已经是一个 filter→SQL 编译器**：
- FROM/JOIN（queries.rs:804）：`media_items m JOIN directories d ON m.directory_id=d.id JOIN scan_roots r ON d.root_id=r.id`，按需 `LEFT JOIN image_meta`（搜索 device/location/global 时）/ `JOIN ai_search_results`（ai_search 时）。
- 基础谓词（queries.rs:826-830）：`trashed_only` → `WHERE m.is_deleted=1 AND m.companion_of IS NULL`，否则 `m.is_deleted=0 AND m.companion_of IS NULL`。
- 目录子树（queries.rs:846-860）：`directory_id` 经 `WITH RECURSIVE dir_tree` 取整棵子树 id —— **递归子树语义已实现，可直接复用**。
- 其余 WHERE 片段：media_types `IN`、favorited、album_id（`m.id IN (SELECT item_id FROM album_items WHERE album_id=?)`）、ai 阈值等，全部**参数绑定**（符合「SQL 必参数绑定」铁律）。
- ORDER BY 由 group_by/sort_within/sort_order 决定（layout 层 `compute_justified_layout` 再据此分组）。

> **关键结论**：`view_to_sql` **不是新建编译器，而是把 `query_layout_items` 的 SELECT 主体抽出「只取 id」的孪生路径**，与现有「取完整 LayoutItem」路径共用同一套 FROM/JOIN/WHERE 构造 → 杜绝双套视图定义漂移（§3.10.2 的「单一事实源」诉求）。

### 1.3 布局缓存 + 版本：`LayoutCacheData`
`src-tauri/src/layout/cache.rs:68`：`rows / total_height / layout_version / total_items / flat_ids / flat_rowcol / id_to_flat`。
- `flat_ids`（cache.rs:76）= 按布局序排列的项 id（不含分隔符）—— **稳定选区/全选所需的「视图全集有序 id」已在内存**，但**当前零 IPC 暴露**（T14.5 要补 `get_view_ids`）。
- `layout_version`（cache.rs:19/98）= 全局 `AtomicU64`，每次 `store_layout` 自增 → 是「视图是否已变」的天然版本戳，`ViewStale` 守门即基于它。

### 1.4 批量命令现状：清一色 `Vec<i64>`
实测 `src-tauri/src/ipc/*_commands.rs`，所有批量操作都收**显式 id 列表**：

| 命令 | 文件:行 | 选择参数 | 落到 |
|------|---------|----------|------|
| `move_to_trash` | system_commands.rs:90 | `item_ids: Vec<i64>` | `soft_delete_items` |
| `add_to_collection` | collection_commands.rs:69 | `item_ids: Vec<i64>` | `add_to_collection` |
| `remove_from_collection` | collection_commands.rs:84 | `item_ids: Vec<i64>` | `remove_from_collection` |
| `set_favorited` | media_commands.rs:224 | `item_ids: Vec<i64>` | UPDATE…IN |
| `prioritize_dimensions` | media_commands.rs:28 | `item_ids: Vec<i64>` | — |
| `batch_request_thumbnails` | thumbnail_commands.rs:28 | `item_ids: Vec<i64>` | — |

### 1.5 前端选区现状：物化 `Set<number>`
- `src/stores/filterStore.ts`：`toApiFilter()` 产出 `MediaFilter` 形状（mediaTypes/livePhoto/favorited/minRating/dateRange）。
- `src/stores/uiStore.ts:174`：`groupBy: 'date'|'folder'|'none'` + `sortWithinGroup`；视图态散落在 `activeSmartAlbum`/`activeDirectoryId`/`activeCollection`（uiStore.ts:106-111）。
- `src/composables/useSelection.ts:11`：`selectedIds = ref(new Set<number>())`；`selectAll(allIds: number[])`（:74）需**先物化全部 id**。Part5:52/172 已点明：Ctrl+A 现状 `selectAll(getAllVisibleIds())` 只拿到**一屏可视 id** → 三症状一病因（选区只覆盖可视 DOM）。

---

## 2. 核心问题（为什么必须有这套契约）

面向 **>100 万项**库，当前两条路径都崩：
1. **前端物化崩**：`selectAll` 把百万 id 灌进 `Set<number>` → 内存/GC 压力 + 仍只拿到可视一屏（Part5 G1）。
2. **IPC 整包崩**：批量命令 `Vec<i64>` 在百万级要经 IPC 传 ~8MB+ JSON，且跨多命令（删除→刷新）时集合可能已漂移。

**解法**（§3.10）：选择不枚举 id，而是描述「哪个视图的全集，减去哪些排除项」，后端按 filter 在 SQL 层流式解析。前端只持有「全选标记 + 排除集（通常很小）」。

---

## 3. 类型设计

### 3.1 `ViewDescriptor`（不可变视图描述符）
确定性唯一复现「当前画廊视图全集 + 序」。落 `src-tauri/src/db/models.rs`（与 `MediaFilter` 同域），`#[derive(Serialize, Deserialize, Clone)]`，IPC 双向可传。

```rust
/// 不可变视图描述符：scope（FROM/JOIN）+ filter（WHERE）+ sort（ORDER BY）+ 版本戳。
/// 唯一确定「当前画廊视图的全集与序」，是 view_to_sql 的输入、全选解析的依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDescriptor {
    pub scope: ViewScope,          // 视图类型 + scope，决定 FROM/JOIN 与基础谓词
    pub filter: GalleryFilter,     // 附加筛选，决定 WHERE 增量
    pub sort: SortSpec,            // 决定 ORDER BY（与 layout 分组一致）
    /// 与 LayoutCache.layout_version 对齐；解析时不一致即拒（ViewStale，§5）。
    pub layout_version: u64,
}
```

### 3.2 `ViewScope`（决定 FROM/JOIN 与基础谓词）
把现在散落在 `MediaFilter`（directory_id/album_id/person_id/trashed_only）+ uiStore（activeSmartAlbum/...）里的「我在看哪种集合」收成一个**互斥**枚举——这是与 `GalleryFilter` 的关键分工：scope 决定**集合来源**，filter 决定**在该来源上再筛**。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ViewScope {
    /// 全库（is_deleted=0）。
    All,
    /// 某目录递归子树（复用 queries.rs:846 的 WITH RECURSIVE dir_tree）。
    Directory { directory_id: i64 },
    /// 用户收藏夹（album_items 成员，复用 queries.rs:884）。
    Collection { album_id: i64 },
    /// 人脸簇视图（faces JOIN + model_name 过滤；见 §4 注）。
    Person { person_id: i64 },
    /// 回收站（is_deleted=1）。
    Trash,
    /// CLIP 语义搜索：有序、非纯 SQL（见 §4「SemanticSearch 例外」）。
    SemanticSearch { query_embedding_id: i64, top_k: u32 },
}
```
> 「智能相册/系统夹」（全部照片/视频/收藏）不单列 scope——它们 = `All` + `GalleryFilter`（media_types/favorited），与现状 system collection 用 media_types+favorited 实现一致（models.rs:342 注释）。

### 3.3 `GalleryFilter`（决定 WHERE 增量）
**决策点 D1（见文末）**：是「`GalleryFilter` = 直接复用现有 `MediaFilter`」还是「新建瘦身版」。本文**推荐**：把 `MediaFilter` 中**属于 scope 的字段（directory_id/album_id/person_id/trashed_only）剥离**到 `ViewScope`，`GalleryFilter` 保留纯「附加筛选」字段，避免「同一语义两处可填、互相打架」：

```rust
/// 附加筛选（在 scope 选定的来源上再筛），决定 WHERE 增量。不含 scope 字段。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GalleryFilter {
    pub media_types: Option<Vec<String>>,
    pub live_photo_only: Option<bool>,
    pub favorited_only: Option<bool>,
    pub min_rating: Option<i64>,
    pub date_range: Option<DateRange>,
    pub search_query: Option<String>,
    pub search_scope: Option<String>,
    // ai_search/ai_threshold 归 SemanticSearch scope，不留在 filter（避免双路）。
}
```
> 迁移期可让 `GalleryFilter` 暂时 = `MediaFilter` 的别名以降风险（见 §7 阶段化），定稿再剥离 scope 字段。

### 3.4 `SortSpec`
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub group_by: String,         // "date" | "folder" | "none"（对齐 uiStore.groupBy）
    pub sort_within_group: String,// "datetime" | "name" | "size" ...
    pub sort_order: String,       // "asc" | "desc"
}
```

### 3.5 `SelectionDescriptor`（选择 = 描述而非枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SelectionDescriptor {
    /// 显式 id 列表（手选少量项）。上限校验见 §6。
    Explicit { ids: Vec<i64> },
    /// 全选某视图 − 排除集（Ctrl+A）。excluded 通常远小于全集。
    SelectAll { view: ViewDescriptor, excluded_ids: Vec<i64> },
}
```

---

## 4. `view_to_sql`：单一编译器

新增 `src-tauri/src/db/queries.rs::view_to_sql(view: &ViewDescriptor) -> (String /*sql*/, Vec<Box<dyn ToSql>> /*params*/)`，**与 `query_layout_items` 共用同一套 FROM/JOIN/WHERE 构造**（重构：把 query_layout_items 现有的 WHERE 拼装抽成 `push_scope_and_filter(&mut sql, &mut params, scope, filter)`，两个调用方——「取 LayoutItem」与「只取 id」——都调它）。

映射规则（全部参数绑定）：
- **scope → FROM/JOIN + 基础谓词**：
  - `All` → `WHERE m.is_deleted=0 AND m.companion_of IS NULL`。
  - `Directory{id}` → 复用 `WITH RECURSIVE dir_tree`（queries.rs:846）。
  - `Collection{album_id}` → `m.id IN (SELECT item_id FROM album_items WHERE album_id=?)`（queries.rs:884）。
  - `Person{person_id}` → `JOIN faces f ON f.item_id=m.id AND f.person_id=? AND f.model_name=?`（见下注）。
  - `Trash` → `WHERE m.is_deleted=1 AND m.companion_of IS NULL`。
- **filter → WHERE 增量**：media_types `IN`、favorited、min_rating、date_range、search（按 scope 决定是否 `LEFT JOIN image_meta`）—— 全部沿用 query_layout_items 现成片段。
- **sort → ORDER BY**：与 layout 分组同源。
- **SemanticSearch 例外**：scope=SemanticSearch 时**不走纯 SQL**——先经 `VectorStore`（Part1）取向量 top_k 有序 id，再以 `m.id IN (...)` 回表过 `GalleryFilter`，序由向量相似度定（不被 ORDER BY 覆盖）。`view_to_sql` 对此 scope 返回「带 id 白名单 + filter」的 SQL，id 白名单由调用方先取。

> **Person.model_name 注**：人脸表按 `model_name` 区分嵌入模型（不同模型的簇 id 空间不可混）。scope 必须带 `model_name`（当前激活模型），否则跨模型误配。**决策点 D2**：model_name 由后端从「当前激活人脸模型」注入，还是 ViewDescriptor 显式带？推荐后端注入（前端无需知晓模型细节），但需确认激活模型的真相源。

---

## 5. `resolve_selection` / `count_selection` + ViewStale 守门

### 5.1 解析与计数
```rust
/// 把 SelectionDescriptor 展开为实际 id 集合。SelectAll 走 view_to_sql 流式游标。
pub fn resolve_selection(conn: &Connection, sel: &SelectionDescriptor) -> Result<Vec<i64>>;
/// 仅计数（UI「将操作 N 项」），SelectAll 走 COUNT(*) 不取全 id。
pub fn count_selection(conn: &Connection, sel: &SelectionDescriptor) -> Result<u64>;
```
- `Explicit{ids}` → 上限校验（§6）后直接返回 / `COUNT = ids.len()`。
- `SelectAll{view, excluded}` →
  1. **先守门**：`view.layout_version != 当前 LayoutCache.layout_version` → `Err(AppError::ViewStale)`（§5.2）。
  2. `view_to_sql(view)` 编译为单条 SQL，**流式游标**逐行取 id（百万级**不一次性 collect**，交批量命令分块消费；见 §6 分块）。
  3. 扣除 `excluded_ids`（小集合，HashSet 过滤）。
  4. `count_selection` 同路径但 `SELECT COUNT(*)`（再减 `excluded` 与全集的交集大小——或简化为 `COUNT(*) - excluded.len()`，**决策点 D3**：是否容忍 excluded 含已不在集合内的 id 造成的轻微计数偏差；推荐精确：`COUNT(*) WHERE id NOT IN (excluded)`，excluded 超阈值时并入 WHERE）。

### 5.2 ViewStale：版本失效（防「全选后视图已变、误操作新集合」）
- `AppError::ViewStale` 须 `serde::Serialize` + 带稳定 error code（符合「IPC 边界错误契约」铁律），前端据类型处理：**重取当前视图（重算 layout 拿新 layout_version）→ 重新发批量命令**。
- **为何安全**：解析在操作时**从 SQL 重新取全集**（不复用缓存 rows）；layout_version 仅用于确认「视图定义未变」。版本一致 ⇒ filter/scope/sort 未变 ⇒ 重解析得到的全集与用户全选时所见一致。
- **进阶（P2，v1 不做）**：服务端 snapshot token——`SelectAll` 先落一份 id 快照返回 token，批量命令引 token，避免大批量跨多命令（删一半视图变了）时集合漂移。v1 先 `layout_version` 守门即可（§3.10.4）。

### 5.3 `get_view_ids` 收敛（T14.5）
T14.5 的 `get_view_ids() -> Vec<i64>` **退化为** `resolve_selection(SelectAll{current_view, []})` 的特例 → 与全选**共用 view_to_sql**，视图定义单一事实源、杜绝漂移（§3.10 🔑）。`get_view_ids` 仍可保留为便捷 IPC（前端「稳定选区」需要按布局序的全集 id 做 Shift-range / 框选命中判定时用——见 §8）。

---

## 6. 批量命令迁移：`Vec<i64>` → `SelectionDescriptor`

§1.4 的全部批量命令签名迁移，入口统一先 `resolve_selection` 展开：
```rust
// 迁移前： pub async fn move_to_trash(item_ids: Vec<i64>, ...) 
// 迁移后： pub async fn move_to_trash(selection: SelectionDescriptor, ...) {
//             let ids = resolve_selection(&conn, &selection)?;  // ViewStale 在此抛
//             // 分块消费（百万级避免单条巨 SQL / 单事务过大）
//             for chunk in ids.chunks(BATCH_CHUNK) { soft_delete_items(&conn, chunk)?; }
//          }
```
迁移清单：`move_to_trash`/`restore_items`/move/copy/`set_rating`/`set_favorited`/`add_to_collection`/`remove_from_collection`/`set_color_label`/`batch_request_thumbnails`/`prioritize_dimensions`。

配套：
- **预览数量**：UI「将操作 N 项」→ `count_selection`（SelectAll 走 `COUNT(*)`，不取全 id）。
- **上限/排除集阈值（决策点 D4，建议初值）**：
  - `Explicit.ids` 上限 ~**100k**：超限提示「请用全选」（前端本就该走 SelectAll）。
  - `excluded_ids` 上限 ~**10k**(原设计:超限退化为反向 filter 避免巨 NOT IN)。🔴 **施工实况(2026-06-30 核验)**:`resolve_selection` 实际用**内存 HashSet 过滤**(queries.rs:1184,非 SQL NOT IN、与 excluded 规模无关),故「巨 NOT IN」风险不存在、10k 退化**未实现亦无必要**;该 10k 当前是**未强制的建议值**。
  - `BATCH_CHUNK`（分块大小）~**5k**/事务：平衡事务大小与往返次数。
- **`spawn_blocking`**：resolve_selection + 分块写均 rusqlite 同步，须在阻塞线程跑（不阻塞 tokio executor），符合后端铁律。

### 6.1 🔴 正确性红线：companion 展开
`view_to_sql` 沿用 `companion_of IS NULL`（Live Photo 的 mov/mp4 伴随项在画廊**不独立显示**）。但**删除/移动/恢复**一张「带 companion 的静图」时，**必须连带处理其 companion 视频**，否则伴随文件成孤儿（数据不一致）。
- **现状须复核**：`soft_delete_items` 是否已展开 companion？（迁移前必须确认；本设计**不假设**。）
- **方案**：在批量**写操作**侧（非 view_to_sql，因展示层正确）统一加 companion 展开——`resolve_selection` 返回的可见 id 集合，在删除/移动前 `UNION` 其 `companion_of=该 id` 的伴随项。建议封装 `expand_companions(conn, ids) -> ids`，仅对结构性写操作（删/移/恢复）调用；评分/收藏等元数据操作不展开（companion 不单独评分）。**决策点 D5**：确认展开策略（哪些操作展开）。

---

## 7. 前端契约对接（Part5 T4 解锁）

- **构建 ViewDescriptor**：`filterStore.toApiFilter()` + `uiStore`(groupBy/sort/active*) 合成 `ViewDescriptor{scope, filter, sort, layout_version}`；`layout_version` 取自最近 `compute_layout` 返回的 `LayoutSummary.layoutVersion`。建议新增 `useViewDescriptor()` composable 做单一合成点。
- **选区脱离 DOM**：`useSelection` 改为两态——
  - 普通多选：`selectedIds: Set<number>`（手选少量，照旧）。
  - **全选态**：`selectAllMode=true` + `excludedIds: Set<number>`（Ctrl+A 不灌全集；取消勾选某项 → 加入 excluded）。展示「已选全部 N 项」（N 来自 `count_selection`）。
  - 发批量命令时：全选态 → `SelectAll{view, excluded}`；否则 → `Explicit{[...selectedIds]}`。
- **三症状一病因修复**（Part5 G1）依赖 `get_view_ids` 提供**按布局序的全集 id**：Shift-range 跨视口、框选命中判定基于 flat_ids 序号而非可视 DOM。本设计的 `get_view_ids`（§5.3）正是其后端前置。
- **ViewStale 处理**：批量命令返回 `ViewStale` → 前端静默重算 layout 拿新版本 → 重发（或提示「视图已更新，请重试」）。

---

## 8. 风险与对策

| 风险 | 级别 | 对策 |
|------|------|------|
| companion 孤儿（删静图漏删伴随视频） | 🔴 高 | §6.1 写操作侧 `expand_companions`；迁移前先复核现状 |
| 全选后视图漂移、误操作新集合 | 高 | `layout_version` 守门（ViewStale）；P2 snapshot token |
| 双套视图定义漂移（view_to_sql vs query_layout_items） | 中高 | 抽 `push_scope_and_filter` 共用，二者单一事实源 |
| 巨 `NOT IN`（excluded 过大） | 中 | excluded 上限 → 退化反向 filter（§6 D4） |
| Person 跨模型误配 | 中 | scope 带 model_name（§4 D2） |
| 单事务过大（百万删） | 中 | `BATCH_CHUNK` 分块、流式游标不全量 collect |
| GalleryFilter 与 MediaFilter 双填打架 | 中 | scope 字段从 filter 剥离（§3.3 D1）；迁移期先别名、定稿再剥 |

---

## 9. 分阶段实施（可独立验、可回退）

- **S0（后端类型 + 编译器，~2–3 天）**：定义 4 类型；抽 `push_scope_and_filter` 让 `query_layout_items` 与新 `view_to_sql` 共用；`view_to_sql` 单测（各 scope/filter 组合 → 期望 SQL+params 快照）。**不碰命令、不碰前端。**
- **S1（resolve/count + ViewStale，~2 天）**：`resolve_selection`/`count_selection` + `AppError::ViewStale`（Serialize+code）；流式游标 + excluded + 上限退化；单测（Explicit 上限、SelectAll 解析、ViewStale 触发、excluded 退化）。
- **S2（get_view_ids 收敛，~0.5 天）**：`get_view_ids` 改走 `resolve_selection(SelectAll{view,[]})`；解锁 Part5 T4。
- **S3（批量命令迁移 + companion 展开，~2–3 天）**：逐命令 `Vec<i64>`→`SelectionDescriptor`，入口 resolve + 分块；`expand_companions` 接入写操作；每命令保留旧路径 feature-flag 灰度或逐个迁移 + 单测。
- **S4（前端对接，属 Part5 T4，~2–3 天）**：`useViewDescriptor` 合成、`useSelection` 全选态、ViewStale 处理、三症状修复。
- **合计后端 S0–S3 ~7 天**；S4 计入 Part5。每阶段 `cargo test` + `cargo clippy` 绿。

---

## 10. 决策点（须你拍板后开工）

- **D1 GalleryFilter 形态**：复用现有 `MediaFilter`（低风险、但 scope 字段双填）vs 剥离 scope 字段成瘦身版（更干净、需迁移调用点）。**推荐**：迁移期先别名复用，S0 末剥离。
- **D2 Person.model_name 来源**：后端从激活模型注入（推荐）vs ViewDescriptor 显式携带。需确认「当前激活人脸模型」的真相源在哪。
- **D3 count 精度**：`COUNT(*) - excluded.len()`（快、可能微偏）vs `COUNT(*) WHERE id NOT IN excluded`（精确、excluded 大时退化 filter）。**推荐**精确。
- **D4 上限初值**：Explicit ~100k / excluded ~10k / BATCH_CHUNK ~5k。需你确认或给出业务期望。
- **D5 companion 展开策略**：哪些写操作展开 companion（建议：删/移/恢复展开；评分/收藏不展开）。**且须先复核 `soft_delete_items` 现状是否已展开**。
- **D6 snapshot token**：v1 仅 layout_version 守门（推荐）vs 一上来就做服务端 snapshot。

### 决策（当前默认,未经实测验证,开发期可改）（2026-06-30）

| 决策 | 裁定 | 备注 |
|------|------|------|
| **D1** GalleryFilter 形态 | **迁移期别名复用 MediaFilter，S0 末剥离 scope 字段** | 前端 `GalleryFilter` 不含 scope 字段；后端 `view_to_sql` 经 `ViewDescriptor::to_media_filter()` lower 成现有 `MediaFilter` 复用同一套 SQL builder（单一事实源），不另起双套 WHERE。 |
| **D2** Person.model_name 来源 | **后端注入，读 config `active_face_model` 真相源** | 与 Part4 T6 `set_active_face_model` 同源。v1 单模型（默认 `yunet-sface` 轨），该 config 键由本契约与 T6 一并定义；ViewDescriptor 不带 model_name。 |
| **D3** count 精度 | **精确 `COUNT(*) WHERE id NOT IN (excluded)`**；excluded 超阈值并入 WHERE | 避免「已选 N 项」与实际不符。⚠️ 未实测:百万级精确 COUNT 的性价比未验证,估算快路径(`total−excluded.len()`)留作开发期可切的备选。 |
| **D4** 上限初值 | **Explicit 100k / excluded 10k / BATCH_CHUNK 5k** | 可调常量，后续按实测调。⚠️ 文档/代码漂移已对齐:excluded「10k 退化反向 filter」**未实现**——`resolve_selection` 用内存 HashSet 过滤(无 SQL NOT IN、无规模退化),10k 当前为未强制的建议值。 |
| **D5** companion 展开策略 | **删/移/恢复展开；评分/收藏不展开** | 已取证 `soft_delete_items`/`restore_items`（queries.rs:1739/1756）**当前不展开**，会留 Live Photo 孤儿；本决策顺带修此现存隐患。`expand_companions(conn, ids)` 仅对结构性写操作调用。⚠️ 未验证:「永远连带 companion」是否所有场景都符合用户预期未经验证,焊死无 override,开发期可改。 |
| **D6** snapshot token | **v1 仅 `layout_version` 守门（`AppError::ViewStale`）** | 服务端 snapshot 留 P2。 |

> 施工分阶段见 §9（S0–S3 后端，S4 计入 Part5 T4）。S0 采用 behavior-preserving 抽取：把 `query_layout_items` 的 FROM/JOIN/WHERE/ORDER 构造抽成私有 helper，新 `view_to_sql`（id-only SELECT）复用之，受现有布局测试 + 新增 view_to_sql 快照测试双重守护。

### 施工状态（2026-06-30，S0–S3 后端已交付）

| 阶段 | 状态 | commit | 要点 |
|------|------|--------|------|
| **S0** | ✅ | `4b12286` | 5 类型 + `to_media_filter()` lower + `push_query_body` 抽取 + `view_to_sql` + 8 快照单测。 |
| **S1** | ✅ | `7fe7a76` | `resolve_selection`/`count_selection`（`current_layout_version` 入参，DB 层纯函数）+ `AppError::ViewStale` + excluded 精确计数 + 7 单测。 |
| **S2** | ✅ | `84f05a7` | `get_view_ids` IPC。**性能偏移**：返回缓存内已物化 `flat_ids`（O(1)），与 `view_to_sql` 同源同序，不每次重查 DB；功能等价于 `resolve_selection(SelectAll{view,[]})`，单一事实源由 S3 批量写路径承担。+3 单测。 |
| **S3** | ✅（修订） | 本次 | **非破坏核心先行**：`expand_companions` + 接进 `soft_delete_items`/`restore_items`（修 Live Photo 孤儿 bug，D5）+ 4 单测。**8 命令签名迁移 `Vec<i64>`→`SelectionDescriptor` 并入 Part5/S4**——因其与现网前端 `MediaGrid.vue` 调用点强耦合，单独翻签名会破 app 或产生 Part5 即将重写的返工。`resolve_selection`/`expand_companions` 已 `pub`，作 S4 工具件。 |

> **真实命令名订正**（旧设计 §1.4/§6 不可轻信）：实测批量命令为 `soft_delete_items` / `restore_items` /
> `batch_toggle_favorite` / `prioritize_dimensions` / `add_to_collection` / `remove_from_collection` /
> `batch_request_thumbnails`，**非**设计文档假设的 `move_to_trash` / `set_favorited`。S4 迁移以实测名为准。

---

## 附：与既有计划的接缝
- 关闭 Part2 §3.10 的「契约悬空」（SelectionDescriptor/GalleryFilter/ViewDescriptor 此前全文零定义）。
- 解锁 Part5 T4「选区脱离 DOM」（Part5:317 明列 ← Part2 T14.5 硬前置）。
- 与 Part5 §3.1.4「交互契约文档化(非冻结)」对齐：Ctrl+A=全选语义标记 + 排除集（Part5:172），后端按 filter 解析全集、不前端枚举（Part5:358）。

