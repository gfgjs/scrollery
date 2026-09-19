---
id: 2026-06-26-Part1_数据层
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 1 · 数据层

> 依赖：[Part0 总纲与产品定稿](Part0_总纲与产品定稿.md)（全局基准；尤其 §1 实测核实、§5 打包模型、§6 卷可用性、§11 推进波次、§12 全局约定）。
> 状态：**现行 canon**（2026-09-15 数据层消融后已按现行实现更新：逐版本迁移的实现与对应实施/验收指令已从本文删除，只保留设计动机、卷模型、DAO 与查询层内容）。当前建库规范：[Spec01 数据层](../spec/Spec01_数据层.md)——单一 `db/schema.rs` 的 `CURRENT_SCHEMA` 完整 DDL + `SCHEMA_VERSION`(34) 格式标识，**无 V1..V33 升级步骤、无按年代拆分的 DDL 常量文件、无回填 DML**。
> 执行前必读 [Spec01 数据层](../spec/Spec01_数据层.md) + 本文。**旧 docs/记忆不可轻信，schema 以代码实测为准。**

---

## §1 目标与范围

### 1.1 本 Part 解决什么

数据层是整个重构的地基（Part2/3/4 均依赖其 schema 与 DAO）。本 Part 交付：

1. **数据安全 P0**（Part0 §1.3 矛盾 #11 等）：① 迁移链事务化（每版本块单事务，杜绝半迁移静默损坏；实施用 `unchecked_transaction`/DEFERRED 而非原案 `BEGIN IMMEDIATE`，见 §3.1 横幅）；② `test_collation` 改内存 DB + `#[cfg(test)]`（消除工作区污染 + 生产构建不带破坏性删除）。
2. **卷可用性模型**（Part0 §6）：`volumes` 表 + `scan_roots` 扩列（`volume_id`/`volume_subpath`）+ `media_items` 扩列（`volume_id`/`volume_relative_path`/`availability`）+ 部分索引；为「移动盘/网络盘离线≠删除」提供数据基础（**扫描器行为在 Part2**）。该模型已在当前结构内（§3.2）。
3. **向量存储与 ANN 选型**：抽 `VectorStore` trait，将当前暴力 O(N) 余弦扫描（Part0 §1.3 矛盾 #5）演进为可热切换的 ANN（usearch/sqlite-vec 等），含启用阈值与迁移路径（**推理在 AI 插件 worker = Part4**；本 Part 只管存储/检索抽象与 schema）。
4. **DAO 补全**：`document_meta` 补 upsert/get + 文档 enrichment 写 `page_count`/`doc_subtype` 的查询（已完成）。原「`scan_roots.backend_id` 接线」一项已删除：该列与外键已从结构移除，卷身份由 `volumes` 承担（§3.2）。
5. **keyset 分页 + 复合索引**：以 `(sort_datetime DESC, id DESC)` 复合索引 + seek 分页替代 OFFSET（百万行 OFFSET 500ms+ → keyset 恒定 <5ms），为画廊/列表查询提速。〔🔴 第 8 轮核验：原写 `capture_time` 是**不存在的列名**（DB 真实列为 `sort_datetime`），已对齐 §3.5 权威 DDL 的 `(sort_datetime DESC, id DESC)`。〕
6. **建库与格式判别规范**（现行，见 §3.1）：`CURRENT_SCHEMA` 单事务建库 + `SCHEMA_VERSION` 格式判别；旧格式库一律拒绝，无升级 / 回填策略。

### 1.2 不在本 Part（归属其它 Part）

- 卷探测/插拔事件监听/扫描编排离线守门/删除检测/SourceChanged 重 enrich **行为逻辑** → **Part2**（本 Part 只提供其依赖的列与 DAO）。
- ANN 在 AI/人脸 **插件 worker 进程内的推理与编码** → **Part4**（本 Part 提供 `VectorStore` 抽象与 schema）。
- IPC 命令错误类型从 `String` 统一为 `AppError` → **Part5/IPC**（本 Part 的新 DAO 直接返回 `AppError`，不引入新 `String` 债）。
- 缩略图缓存目录治理、派生产物清理 → **Part3**。

### 1.3 全局约定（继承 Part0 §12）

`thiserror`；仅 `rusqlite`（写 `Mutex<Connection>` + 读 `r2d2` 池 WAL）；**所有 SQL 参数绑定**；锁序 `db_writer → layout_cache`；面向用户查询追加 `is_deleted = 0 AND companion_of IS NULL`；新增/改动 DAO **一律返回 `AppError` 不返回 `String`**；中英双语注释；改动后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（取证 workflow，精确到 文件:行）

### 2.1 schema 与连接模型

- **当前 `SCHEMA_VERSION = 34`**，**33 张表 + 37 个显式索引**（`src-tauri/src/db/schema.rs` 的 `CURRENT_SCHEMA`）；结构总览见 [Spec01](../spec/Spec01_数据层.md) §2。
- **连接模型**（[connection.rs](../../src-tauri/src/db/connection.rs)）：写 = `Mutex<Connection>`（单连接串行化所有写）；读 = `r2d2::Pool<SqliteConnectionManager>`（桌面 4 连接，`SQLITE_OPEN_READ_ONLY|NO_MUTEX|URI`，`min_idle=0`）。两类连接 `on_acquire/create` 时 apply 7 条 PRAGMA + 注册自定义排序 `NATURAL_CMP`。
- **PRAGMA**：`journal_mode=WAL` / `synchronous=NORMAL` / `cache_size=-64000`(64MB) / `foreign_keys=ON` / `busy_timeout=5000` / `temp_store=MEMORY` / `mmap_size=256MB`。
- **FK 已启用**（`foreign_keys=ON`）→ 新增 FK 真实生效，内存测试插含 FK 行需注意。

### 2.2 建库与格式判别（现状）

- 建库唯一入口是 `db::schema::initialize_schema`（[`schema.rs`](../../src-tauri/src/db/schema.rs)）；生产启动由 [`db/boot.rs`](../../src-tauri/src/db/boot.rs) 的 `init` 在恢复启动交换之后、创建读池之前调用。
- `CURRENT_SCHEMA` 就是当前格式的完整 DDL（33 表 + 37 显式索引）+ 建库种子；`SCHEMA_VERSION = 34` 只作格式判别：标识相等即放行（幂等），库内无用户表则单事务建库，其余一律 `AppError::SchemaIncompatible`（稳定码 `db_schema_incompatible`）。
- 已不存在 `migration.rs`、`schema/{early,mid,late}.rs`、`run_migrations`、`CURRENT_VERSION`、V1..V33 版本步骤与回填 DML（2026-09-15 消融中整体删除）。

### 2.3 关键表/列现状（与本 Part 直接相关）

〔**历史取证（2026-06 时点）**：下表「无 model_name / backend_id 半实现 / document_meta 死表」等描述现已全部改变——`persons.model_name`、`media_items.color_label`/`content_identifier` 已在结构中；`document_meta` 已有 DAO；`scan_roots.backend_id` 列与接线**已删除**（卷身份只由 `volumes` 承担）。当前结构见 [Spec01](../spec/Spec01_数据层.md) §2。〕

| 表 | 关键事实 |
|---|---|
| `media_items` | 25 列：核心含 `directory_id`/`cache_key`/`sort_datetime`/`thumb_status`/`is_deleted`/`deleted_at`/`rating`/`companion_of`/`content_hash`/`ai_status`(V2)/`face_status`(V8)。14 个索引含 `idx_media_sort = (sort_datetime DESC) WHERE is_deleted=0 AND companion_of IS NULL`（**仅单列、无 id 次键**） |
| `scan_roots` | 卷/网络盘绑定由 `volume_id`/`volume_subpath` 承担；历史上的 `backend_id` 半实现（列、外键与接线）**已于 2026-09-15 整体删除**，不再保留 |
| `document_meta` | 表存在（`item_id` PK / `page_count` / `doc_subtype`，[schema.rs:174](../../src-tauri/src/db/schema.rs#L174)）；`format.rs:85` 有 `doc_subtype()` 引用；但 **queries.rs 零 DAO**（无任何 SELECT/INSERT FROM document_meta）→ 死表 |
| `ai_embeddings` | `PRIMARY KEY (item_id, model_name)`，`embedding BLOB`=**raw f32 小端**（512×4=2048B for ViT-B/16）；`idx_embed_model` |
| `persons`/`faces` | V8；`faces.embedding`=f32 LE（SFace 128/ArcFace 512）；`persons.centroid`=f32 LE BLOB；**`persons` 无 `model_name` 列**（切模型维度冲突，Part4 处理）；`persons.cover_face_id` 未声明 FK（[schema.rs:412](../../src-tauri/src/db/schema.rs#L411)） |

### 2.4 向量检索现状（澄清一处取证矛盾）

- 存储：`ai_embeddings.embedding` = f32 LE BLOB（每图一条，`(item_id, model_name)` 主键）。
- 检索：`get_all_embeddings(model_name)`（[queries.rs:1874](../../src-tauri/src/db/queries.rs#L1874)）一次性 `SELECT item_id, embedding ... WHERE model_name=?` 全量读入；**`AppState.ai_embedding_cache` 持有常驻 f16 连续缓存**（见 `architecture_notes.md` AI 语义搜索节），搜索在缓存上用 **rayon 暴力 O(N) 余弦**，每次 embedding 批写/reset 时缓存失效。
  > ⚠️ 取证纠正：DAO agent（仅读 queries.rs）报「无常驻缓存」——错，f16 常驻缓存在 `ai/search.rs`+`AppState`，不在 queries.rs。以 architecture_notes + ANN agent 为准：**有 f16 常驻缓存 + 暴力扫描**，瓶颈是 O(N) 无 ANN（百万级 ≈1GB RAM + 秒级）。
- 人脸聚类同理：`get_all_persons_for_clustering` 全读 `centroid` 进内存做最近质心（[queries.rs:2505](../../src-tauri/src/db/queries.rs#L2505)）。

### 2.5 分页现状（2026-06 取证）

> 现状补充（2026-09-15）：回收站已用 keyset（`get_trash_keyset` seek 取代 OFFSET），`idx_media_sort` 已是 `(sort_datetime DESC, id DESC)` 复合键；主画廊仍为驻留缓存、非分页。

- **主画廊** `query_layout_items`（[queries.rs:675](../../src-tauri/src/db/queries.rs#L675)）：**无分页**，一次性查全部匹配 `LayoutItem`，ORDER BY 收尾 → 后端建驻留布局缓存、前端按 Y 取可见行（这是百万级机制本身，**坐标平移 bug 归 Part2**）。
- **回收站** `get_trash`：`LIMIT ?1 OFFSET ?2`（[queries.rs:1629](../../src-tauri/src/db/queries.rs#L1629)）传统 OFFSET。
- **搜索** `search_media`：`LIMIT` 截断、无翻页。
- 结论：**无 keyset 分页**；主画廊靠驻留缓存（非分页），回收站 OFFSET（百万行翻页慢）。

---

## §3 设计方案

### 3.1 schema 初始化与格式判别（现行实现）

数据层不再有逐版本迁移链：`db/schema.rs` 的 `CURRENT_SCHEMA` 就是**当前格式的完整 DDL**（33 表 + 37 显式索引 + 建库种子），`SCHEMA_VERSION` 常量只作格式判别、无升级语义。完整判据、事务与回滚语义、失败呈现见 [Spec01 §3.1](../spec/Spec01_数据层.md)；本篇不复制 DDL 正文。

```
initialize_schema(conn):                          -- 生产启动与所有建库夹具的唯一入口
  标识 == SCHEMA_VERSION            → 直接放行（幂等，不动任何数据）
  库内无任何用户表（sqlite_% 不计）  → 单事务：execute_batch(CURRENT_SCHEMA) + 同事务写标识；失败整块回滚
  其余（标识缺失 / 不符 / 未来值）    → AppError::SchemaIncompatible（db_schema_incompatible），不改数据、不叠建
```

**对早期设计的实质继承**：原 P0 是给 `run_migrations()` 加「每版本块单事务」，动机是杜绝「半套结构 + 标识已前进」的悬挂态与重跑 `duplicate column` 式启动失败。该引擎与 V1..V33 版本链已整体删除，**不要再搭迁移器、不要再新增 `SCHEMA_Vn` 常量、不要写「旧库缺列就补列」的兼容分支**；保留下来的只有那条安全不变式，现在由「结构与标识同事务提交」直接满足。结构变更方式：改 `CURRENT_SCHEMA` 正文 + 提升 `SCHEMA_VERSION`，旧格式库由调用方报错提示重置。

### 3.2 卷可用性模型（现行落点）

卷可用性模型仍在使用，但**没有 `SCHEMA_V10` 这类版本常量**：`volumes` 表、`scan_roots.volume_id`/`volume_subpath`、`media_items.volume_id`/`volume_relative_path`/`availability`、`persons.model_name`、`media_items.color_label`/`content_identifier`、`face_rejections` 及 `idx_media_avail`/`idx_media_volume`/`idx_media_content_id` 全部在 `CURRENT_SCHEMA` 内，建库即有。列级形状与约束语义见 [Spec01 §2.2](../spec/Spec01_数据层.md)；本篇不复制 DDL 正文。

**仍成立的设计约定**：

- 卷 / 存储后端分离：`volumes` 是媒体与扫描根的卷身份锚点（`stable_id` = Win 卷 GUID / mac UUID / 规范化 UNC）；`storage_backends` 只作连接管理，`scan_roots.backend_id` 列已删除。
- `availability` 三态（`online`/`offline`/`missing`）由卷驱动，与用户软删除 `is_deleted` 正交；扫描路径永不写 `is_deleted`。
- `media_items.volume_id` 与 `scan_roots.volume_id` 均 `ON DELETE SET NULL`：删卷只置空引用，不删媒体 / 扫描根行；`delete_volume` 的媒体处置（软删 vs 保留）是「已知卷」面板的 UX 决策，DAO 不擅自级联删。
- `media_items.volume_relative_path`（卷根起相对路径）是重新挂载后的重链接键；`scan_roots.path` 降级为「最后已知绝对路径」。

### 3.3 DAO 补全（全部返回 `AppError`，参数绑定）

**(a) `document_meta`（死表激活）**——新增 [queries.rs](../../src-tauri/src/db/queries.rs)：
```rust
/// 文档元数据 upsert(页数/子类型)。文档 enrichment 完成后写入。
pub fn upsert_document_meta(conn: &Connection, item_id: i64,
                            page_count: Option<i64>, doc_subtype: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO document_meta(item_id, page_count, doc_subtype) VALUES(?1, ?2, ?3)
         ON CONFLICT(item_id) DO UPDATE SET page_count=excluded.page_count,
                                            doc_subtype=excluded.doc_subtype",
        params![item_id, page_count, doc_subtype])?;
    Ok(())
}
pub fn get_document_meta(conn: &Connection, item_id: i64) -> Result<Option<DocumentMeta>> { /* SELECT */ }
```
配套 `DocumentMeta` struct（models.rs）。`page_count` 用于 PDF 阅读进度条 / epub 章节数（消费在 Part3 文档派生、Part5 阅读器）。

**(b) `scan_roots.backend_id` 接线 — 已删除（2026-09-15）**：该列及其指向 `storage_backends` 的外键已从结构删除，`ScanRoot` 不再持有该字段，网络 / 网络盘绑定改由 `volumes`（§3.2）承担。**不要恢复该列、该字段或 `set_scan_root_backend` 之类的 DAO。**


**(c) `volumes` 及卷可用性 DAO**（新增，供 Part2 卷探测/扫描编排调用）：
```rust
pub fn upsert_volume(conn: &Connection, v: &NewVolume) -> Result<i64>;          // stable_id 冲突则更新 label/kind/last_mount_path/last_seen/is_online
pub fn get_volume_by_stable_id(conn: &Connection, stable_id: &str) -> Result<Option<Volume>>;
pub fn list_volumes(conn: &Connection) -> Result<Vec<Volume>>;                  // 设置「已知卷」面板用
pub fn set_volume_online(conn: &Connection, stable_id: &str,
                         online: bool, mount_path: Option<&str>, now: i64) -> Result<()>;
pub fn rename_volume_label(conn: &Connection, volume_id: i64, label: &str) -> Result<()>;
pub fn delete_volume(conn: &Connection, volume_id: i64) -> Result<()>;          // 级联(scan_roots.volume_id SET NULL / 由调用方决定软删 media)
/// 批量切换整盘可用性(离线/重连)，单字段过滤、O(索引)。
pub fn bulk_set_availability(conn: &Connection, volume_id: i64,
                             from: &str, to: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE media_items SET availability=?3 WHERE volume_id=?1 AND availability=?2",
        params![volume_id, from, to])?)
}
```
配套 `Volume` / `NewVolume` / `VolumeKind` struct（models.rs，`VolumeKind` 非穷尽 enum 仿 `TokenizerKind`）。
> 🔴 **离线≠删除硬规则的数据侧落点**：`bulk_set_availability(vol, 'online'→'offline')`（卷拔出）与 `('offline'→'online')`（重连）是仅有的合法整盘状态切换；**任何 DAO 都不得在 `availability='offline'` 时写 `is_deleted=1`**——差集删除的 SQL 守门在 Part2，本 Part 不提供「绕过卷判断的批量删除」DAO。

### 3.4 当前语义检索

2026-09-15：删除没有生产消费者的 `VectorStore`、`BruteForceStore`、`DynVectorStore` 与 `ANN_THRESHOLD`。原两阶段 ANN、增量 store 接线及 AppState 多 store 预留方案取消，不留接口或联网预取任务。

当前实现直接使用 [search.rs](../../src-tauri/src/ai/search.rs) 的 `EmbeddingCache`；[search_control.rs](../../src-tauri/src/ai/search_control.rs) 管理搜索请求换代与结果提交。保留维度、模型身份及并发结果保护。人脸聚类保留当前实际流程，不引入向量存储接口。

### 3.5 keyset 分页 + 复合索引

**澄清定位**：主画廊的百万级机制是**后端驻留布局缓存**（`query_layout_items` 一次性查全集建缓存、前端按 Y 取可见行），**不是分页**——这是 Part0 §1 已肯定的强项，坐标平移 bug 归 Part2。本 Part 只做两件支撑：

**(a) 复合索引强化** `idx_media_sort`（[schema.rs](../../src-tauri/src/db/schema.rs)，随 V10 或独立 ALTER）：
现状 `(sort_datetime DESC) WHERE is_deleted=0 AND companion_of IS NULL` 仅单列、无稳定次键。改为 **`(sort_datetime DESC, id DESC)`** 同条件：① 给 `query_layout_items` 的 `ORDER BY sort_datetime DESC` 一个**确定性 tiebreaker**（同秒时间戳的稳定排序，避免布局抖动）；② 直接支撑下方 keyset seek。
> 🔴 **建索引 ≠ tiebreaker（第 7 轮终审补）**：复合索引只让排序**走索引**、**不改 `ORDER BY` 语义**——须**同时把 `query_layout_items` 各 `ORDER BY` 分支显式追加 `, m.id DESC`**（实测 [queries.rs](../../src-tauri/src/db/queries.rs) 现有多处 `ORDER BY m.sort_datetime {dir}` / `m.file_name COLLATE NATURAL_CMP` / `ai.similarity` 均**无次键**），否则同秒/同名/同相似度行仍非确定序、布局仍抖。**索引与 `ORDER BY` 两处都改，缺一不可**（现行为：idx_media_sort 已是复合键，各 ORDER BY 分支已追加 id 次键）。
```sql
DROP INDEX IF EXISTS idx_media_sort;
CREATE INDEX idx_media_sort ON media_items(sort_datetime DESC, id DESC)
    WHERE is_deleted = 0 AND companion_of IS NULL;
```

**(b) keyset seek 分页**（取代 OFFSET，供回收站/搜索/未来「加载更多」列表）：百万行 `OFFSET 1000000` 需扫过百万行（500ms+），keyset 用「上一页末尾游标」seek（恒定 <5ms）。
```rust
/// 回收站 keyset 翻页：游标 = 上一页最后一项的 (deleted_at, id)。
pub fn get_trash_keyset(conn: &Connection, cursor: Option<(i64, i64)>, limit: i64)
    -> Result<Vec<TrashItem>> {
    // 首页 cursor=None；后续传上页末项游标。复合排序 (deleted_at DESC, id DESC)。
    let sql = "SELECT ... FROM media_items WHERE is_deleted=1
               AND (?1 IS NULL OR (deleted_at, id) < (?2, ?3))
               ORDER BY deleted_at DESC, id DESC LIMIT ?4";
    // 注：SQLite 行值比较 (a,b)<(c,d) 支持；需 idx (deleted_at DESC, id DESC)
}
```
配套索引 `idx_media_del`（现为 `(is_deleted) WHERE is_deleted=1`）扩为 `(is_deleted, deleted_at DESC, id DESC)` 或新建 `idx_media_trash`。`search_media` 同法加可选游标参数。
> 主画廊**不改为分页**（保持驻留缓存）。keyset 仅用于真正翻页的列表。

### 3.6 `test_collation` 内存 DB 修复（P0 快修）

[connection.rs](../../src-tauri/src/db/connection.rs) 的排序校验把真实 `.db` 文件落盘（`test_collation.db` 已出现在 `git status`，污染工作区，且破坏性 `remove_file` 在 Release 也编译）。改为**内存 DB + `#[cfg(test)]`**：
```rust
#[cfg(test)]
fn verify_natural_collation() -> Result<()> {
    let conn = Connection::open_in_memory()?;     // 不落盘
    register_custom_collations(&conn)?;
    // ...断言 NATURAL_CMP 排序正确...
    Ok(())
}
```
非测试构建不含该逻辑（删除生产路径的 `remove_file`）。`.gitignore` 补 `*.db`（兜底）。

### 3.7 其它 schema 债（现行口径）

- **已解决 / 不再适用（勿再当债处理）**：`persons.model_name`、`media_items.color_label`、`media_items.content_identifier`、`face_rejections` 均已在当前结构内（建库即有，无需 ALTER）；`scan_roots.backend_id` 与 `exotic_plugins.entitlement_source` **两列已删除**（扫描根卷身份由 `volumes` 承担；渠道 / 权益不落库），不要再为其预留列、DAO 或结构体字段。
- **仍在的已知债（记录、按需处理）**：`persons.cover_face_id` 未声明 FK（悬空由查询侧过滤）；`exotic_tasks.id` 无 `AUTOINCREMENT`（rowid 自增实际可用，不改）；`ai_search_results` 无会话隔离（多窗口非近期目标）；`exotic_catalog_formats` 无 `updated_at`；`doc_replacements.replace` 与 SQLite 内置 `replace()` 同名（涉及该列的 SQL 须加引号，不改列名）；`storage_backends` / `scan_roots` 无 `updated_at`。

结构改动的现行做法：直接改 `db/schema.rs` 的 `CURRENT_SCHEMA` 正文并提升 `SCHEMA_VERSION`（见 §3.1 与 [Spec01 §4](../spec/Spec01_数据层.md)），不写增量脚本、不新增版本常量。

## §4 分步实施清单

> 2026-09-15 消融后只保留**仍然成立或已完成**的条目。涉及逐版本迁移链与 V10/V11 版本常量的 T1/T3/T4/T10、`scan_roots.backend_id` 接线的 T7、`entitlement_source` 预留的 T12 已随消融删除（任务行与实现都不再保留）；当前结构（`SCHEMA_VERSION = 34`，33 表 + 37 索引）见 [Spec01](../spec/Spec01_数据层.md)。

| # | 任务 | 状态 | 落点 |
|---|---|---|---|
| **T2** | `test_collation` 改共享缓存内存库（写连接 + 读池共享，零文件残留，补自然序断言） | ✅ 已完成 | `db/connection.rs` |
| **T5** | `Volume`/`NewVolume`/`VolumeKind` models + 卷 DAO（upsert / get / list / set_online / rename / delete / bulk_set_availability）；不提供绕过卷判断的批量删除（离线≠删除） | ✅ 已完成 | `db/models/`、`db/queries/storage.rs` |
| **T6** | `document_meta` DAO（upsert / get）+ `DocumentMeta` struct | ✅ 已完成 | `db/models/`、`db/queries/` 文档侧 |
| **T8/T9/T11** | 已退役（2026-09-15）：休眠向量存储及专属测试整删，simsimd 预取与 ANN 实装作废 | 已退役 | `ai/search.rs`、`ai/search_control.rs`（当前检索见 §3.4） |

**关键路径**：无未完成项。Part2 依赖的 `volumes`/`availability` 与复合索引均在建库即成的当前结构内（§3.2）。

## §5 风险与回滚

- **现状风险模型**：结构只在 `CURRENT_SCHEMA` 定义一次，变更 = 改正文 + 提升 `SCHEMA_VERSION`；不存在「旧库升级失败」「新旧列并存」这类回滚矩阵。建库失败（结构与标识同事务）整块回滚，库回到「无表无标识」，下次启动可安全重试（§3.1、[Spec01 §5](../spec/Spec01_数据层.md)）。
- **旧格式库**：一律 `db_schema_incompatible` 拒绝，不叠建、不清理用户数据与源资产。恢复场景的回滚分支（`rollback_restore_at_boot`：先 drop 连接释放 Windows 文件句柄，再逆向恢复原始库并重开）见 [Spec08](../spec/Spec08_存储备份导出文件操作.md) §3.2 与 `db/boot.rs`。
- **`volume_relative_path` 写入代价**：百万行拼接路径不宜压在启动路径上，故建库不回填；该列由卷探测 / 增量扫描（Part2）写入，占位期卷在线时正常路径仍可用。
- **FK 级联**：删卷时 `media_items.volume_id` 与 `scan_roots.volume_id` 置空（SET NULL），媒体与扫描根行保留，符合「删卷 = 变未知卷」语义。

## §6 验收标准

- **建库（现行唯一验收）**：`db/schema.rs` 的 `initialize_*` 测试覆盖代表表 / 索引就位、系统收藏夹 4 条种子、`scan_roots` 默认值语义、外键与 `integrity_check` 干净、当前库重复初始化幂等且不清库、缺标识旧库与未知标识被拒且原数据不变、建库中途失败整体回滚（见 [Spec01 §6](../spec/Spec01_数据层.md)）。
- **连接与排序**：`db/connection.rs`、`db/mod.rs` 的 collation 测试覆盖写连接与读池均注册 `NATURAL_CMP`（内存库，不落盘）。
- **DAO 层（不变）**：新增 / 改动 DAO 一律返回 `AppError`（不返回 `String`）、SQL 全参数绑定、`cargo clippy` 无新警告。
- Gate 命令：`cargo test --package scrollery --lib db::`；全量门禁见 `.github/workflows/ci.yml`（broad/release 场景才需全量跑）。

早期针对「V1→V11 全链路重放」「启动期把 V9 库升级到当前版本」「V10 回填比例」的验收项随迁移链退役，见 §3.1。

## §7 给执行会话的提示词

实现或修改数据层时，读 [Spec01 数据层](../spec/Spec01_数据层.md) 与本文；`db/schema.rs` 的 `CURRENT_SCHEMA` 是当前格式的唯一 DDL 源，`SCHEMA_VERSION` 只作格式判别。**不要搭迁移器、不要新增 `SCHEMA_Vn` 常量、不要把 DDL 正文复制进文档、不要为已删除的 `scan_roots.backend_id` / `exotic_plugins.entitlement_source` 预留结构。** 铁律：SQL 全参数绑定；新 DAO 返回 `AppError`；离线≠删除（`availability='offline'` 绝不写 `is_deleted`）；改动后补对应单测、保持 `cargo test --offline` 全绿；中文 commit，仅用户通知时 push。

---

*（Part 1 正文完。Part2 扫描与画廊流水线消费本 Part 的 `volumes`/`availability` DAO 与复合索引。）*

<!-- 哨兵: Part1 全文定稿(§1-§7)；依赖取证 workflow wz3chm0ne 已核实；待 Part2 衔接 -->
