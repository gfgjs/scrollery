# 冷门格式插件子系统 v3.1 · Part 1 地基（P0–P2）

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 范围：技术探针、能力目录、数据库任务模型、扫描入库、主缩略图路由、跨流水线门控。
> 前置：阅读 `exotic_format_plugin_plan_v3.md`。
> 版本：v3.1（已并入 `exotic_format_plugin_plan_v3.1_addendum.md` 的 R1/R2/R3/R7/R9/R12/R13）。
> 本卷不启动 Worker、不实现下载。

## 本卷完成定义

1. 用固定样本矩阵实测 PSD 解码 crate；首发支持范围与结果一致。
2. 全新离线 App 能从内置 Catalog 识别 PSD，生成 thumbnail task，并返回 `AvailableUninstalled`。
3. 普通 JPG/MP4 不产生 exotic task；百万普通媒体不会污染 exotic 待处理索引。
4. PSD 的单项、批量、全库缩略图请求均在 generator 之前让路，无 `UnsupportedFormat`。
5. PSD thumbnail 完成前，CLIP/人脸不领取；Catalog 认领的冷门视频不进入主视频派生。
6. 文件变化、Catalog 更新、插件版本变化具备明确失效入口。

## Phase 0 — 事实核验与技术探针

### 0.1 先做探针，后冻结 manifest

新增独立实验程序或测试：`crates/exotic-workers/psd-probe`。锁定候选 crate 的**精确版本与 feature**，使用有授权的样本矩阵：

| 样本 | 必测结果 |
|---|---|
| RGB 8-bit PSD，含 merged image | 尺寸、RGBA、缩略图正确 |
| 无 merged image PSD | 明确支持图层合成或明确拒绝 |
| CMYK PSD | 颜色可接受或返回稳定错误码 |
| 16-bit PSD | 支持或明确拒绝 |
| 超大画布/大量图层 | 峰值内存与耗时 |
| 截断、随机字节、畸形 RLE | 不 panic、不越界、稳定失败 |
| PSB | 仅在实测通过后写入 formats |

探针报告写入 `plan-docs/exotic_format_plugin_plan/exotic-psd-probe-report.md`：依赖版本、许可证、样本 hash、结果、峰值内存、首发范围。若 PSB 未通过，Catalog 与 manifest 只能登记 `psd`。

候选 crate 预研（R12，缩短探针周期；最终以实测为准，不得据此预先宣称支持）：

| 候选 | 关注点（待 probe 验证） |
|---|---|
| `psd` crate | 纯 Rust、API 简；CMYK/16-bit/PSB 支持存疑，畸形 RLE 健壮性需压测 |
| `image` + 手解 | PSD 非 image 内建格式，须自处理图层/合成，成本高 |
| OS 委托（Windows WIC / macOS ImageIO） | 可能直出 PSD 合成图，但跨平台不一致，属四档「OS 委托」一档，须单列许可与可用性 |

### 0.2 迁移版本核验

施工第一步读取 `src-tauri/src/db/migration.rs` 当前最高版本。本卷使用最高版本 +1，并同步更新 schema 常量、迁移块、schema_version；文档中的 `V_NEXT` 不得原样进入代码。

## Phase 1 — Catalog 与任务 schema

### 1.1 文件

- 新增 `src-tauri/resources/exotic-catalog.json`
- 新增 `src-tauri/src/exotic/catalog.rs`
- 新增 `src-tauri/src/exotic/task.rs`
- 新增 `src-tauri/src/exotic/mod.rs`
- 修改 `src-tauri/src/db/schema.rs`
- 修改 `src-tauri/src/db/migration.rs`
- 修改 `src-tauri/src/db/queries.rs`

### 1.2 内置 Catalog

内置 Catalog 随应用签名发布；保证首次离线扫描也能识别可购买格式。远程 Catalog 在 Part3 加签名与合并逻辑。

```json
{
  "schema": 1,
  "sequence": 1,
  "offerings": [
    {
      "plugin_id": "exotic-image-psd",
      "name": "PSD 图像引擎",
      "media_kind": "image",
      "formats": ["psd"],
      "capabilities": ["thumbnail"],
      "license_tier": "paid",
      "platforms": ["x86_64-pc-windows-msvc", "aarch64-apple-darwin"],
      "min_host_version": "0.1.0",
      "override_common": false,
      "store_url": "https://example.invalid/plugins/psd"
    }
  ]
}
```

约束：format 小写、无点、满足 `[a-z0-9]{1,16}`；plugin_id 满足固定安全字符集；重复 format、重复 plugin_id、覆盖常见格式、未知 media_kind/capability 均拒绝整个 Catalog。

### 1.3 DDL

```sql
CREATE TABLE exotic_catalog_formats (
    format            TEXT PRIMARY KEY,
    plugin_id         TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    media_kind        TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    license_tier      TEXT NOT NULL,
    platforms_json    TEXT NOT NULL,
    min_host_version  TEXT NOT NULL,
    store_url         TEXT,
    catalog_sequence  INTEGER NOT NULL,
    source            TEXT NOT NULL
);

CREATE TABLE exotic_plugins (
    plugin_id          TEXT PRIMARY KEY,
    version            TEXT NOT NULL,
    manifest_hash      TEXT NOT NULL,
    package_sequence   INTEGER NOT NULL,
    install_state      TEXT NOT NULL,
    installed_at       INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

CREATE TABLE exotic_tasks (
    id                 INTEGER PRIMARY KEY,
    item_id            INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    plugin_id          TEXT NOT NULL,
    capability         TEXT NOT NULL,
    status             INTEGER NOT NULL DEFAULT 0,
    input_fingerprint  TEXT,
    attempts           INTEGER NOT NULL DEFAULT 0,
    next_retry_at      INTEGER,
    claimed_at         INTEGER,
    lease_owner        TEXT,
    last_error_code    TEXT,
    last_error_message TEXT,
    output_path        TEXT,
    worker_version     TEXT,
    created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, plugin_id, capability)
);

CREATE INDEX idx_exotic_tasks_ready
ON exotic_tasks(plugin_id, capability, status, next_retry_at);
CREATE INDEX idx_exotic_tasks_item
ON exotic_tasks(item_id, capability, status);
```

配置默认值：

```text
exotic_enabled=true
exotic_auto_process=true
exotic_paused=false
exotic_max_workers=0
```

禁止新增 `exotic_dev_mode` 数据库配置。

### 1.4 CatalogStore

```rust
pub struct CatalogStore {
    by_format: std::collections::HashMap<String, CatalogOffering>,
    by_plugin: std::collections::HashMap<String, CatalogPlugin>,
    sequence: u64,
}

impl CatalogStore {
    pub fn resolve_format(&self, format: &str) -> Option<&CatalogOffering>;
    pub fn media_kind(&self, format: &str) -> Option<MediaKind>;
    pub fn claims_capability(&self, format: &str, capability: Capability) -> bool;
}
```

运行时只读快照使用 `ArcSwap` 或 `RwLock<Arc<CatalogSnapshot>>`；热路径一次读锁/原子 load，不查 DB。刷新流程先完整解析到新快照，校验通过后一次替换，禁止半更新。

Catalog 启动顺序：加载内置 → 尝试加载已验签远程缓存 → 按 sequence 合并 → 投影到 `exotic_catalog_formats` → backfill 新能力任务。

### 1.5 扫描生命周期

保留 `utils::format::classify_media_type` 作为常见格式真相。在 scanner 增加正交入口：

```rust
fn classify_scanned_file(ext: &str, catalog: &CatalogSnapshot) -> Option<MediaType> {
    classify_media_type(ext).or_else(|| catalog.media_kind(ext).map(Into::into))
}
```

常见格式优先；Catalog 不可覆盖现有分类。**exotic 识别只依赖文件扩展名**（Catalog `by_format`=ext），扫描事务内即可判定，不依赖 enrichment 富化阶段（富化产出宽高/时长，与 exotic 分类无关）；`classify_scanned_file` 仅做扩展名匹配，零额外 IO（R13）。扫描事务中完成：

1. upsert `media_items`；
2. 若 Catalog 命中，按 capabilities `INSERT OR IGNORE exotic_tasks`；
3. 事务提交后向 Coordinator 发一次 `ScanCommitted`，不能逐文件发事件。

现有 `upsert_fast_scan_item` 返回值扩为：

```rust
pub enum UpsertOutcome { Unchanged(i64), Inserted(i64), SourceChanged(i64) }
```

`SourceChanged` 必须同时：重置 `thumb_status/thumb_path/thumbhash`；把该 item 的 exotic tasks 置 pending、清空输出与旧指纹；删除或覆盖旧缓存产物。不能只依赖列默认值。

Catalog 更新后执行集合式 backfill：只为 Catalog 已登记格式创建缺失任务。远程 Catalog 删除 offering 时不删除历史媒体与输出；任务转为不可领取并在 UI 显示 `NoOffering/InvalidInstallation`。

### 1.6 任务 DAO

新增：

- `backfill_exotic_tasks_for_catalog`
- `seed_exotic_tasks_for_item`
- `invalidate_exotic_tasks_for_item`
- `invalidate_exotic_tasks_for_plugin_version`
- `claim_exotic_tasks`
- `finish_exotic_task`
- `fail_exotic_task`
- `recover_orphaned_exotic_tasks`
- `has_blocking_exotic_thumbnail_task`

领取在写事务中原子完成。若 SQLite 版本支持，使用带条件的 `UPDATE ... RETURNING`；否则 `BEGIN IMMEDIATE → SELECT ids LIMIT N → 条件 UPDATE → COMMIT`。更新条件必须包含 `status IN (0,3)`，防止两个启动请求重复领取。

孤儿恢复只重置**超出 lease 的** processing 任务；不能在第二个合法 App 实例仍工作时全量 `1→0`。

lease 为**必选**方案（R2；项目当前无单实例插件，不得以「单实例」兜底）：

- App 启动生成进程级 `instance_id`（随机，仅内存，不落库）；
- Claimer 领取时在同一写事务写 `status=1, claimed_at=now, lease_owner=instance_id`；
- 长任务由 Supervisor 周期续租 `UPDATE exotic_tasks SET claimed_at=now WHERE id=? AND lease_owner=?`，续租周期 << `lease_ttl`；
- 孤儿恢复只回收过期租约：

```sql
UPDATE exotic_tasks
SET status=0, claimed_at=NULL, lease_owner=NULL
WHERE status=1
  AND (claimed_at IS NULL OR claimed_at < (strftime('%s','now') - :lease_ttl));
```

`lease_ttl` 取 `≥ 3×task_timeout`。若未来引入单实例插件，单列任务并评估对扫描/AI/派生既有子系统的全局影响，不在本子系统内顺带启用。

## Phase 2 — 路由、门控与跨流水线隔离

### 2.1 Host 只组合状态，不创造能力

```rust
pub struct ExoticHost {
    pub catalog: Arc<CatalogService>,
    pub installed: Arc<InstalledPluginStore>,
    pub licenses: Arc<LicenseStore>,
}

impl ExoticHost {
    pub fn resolve_format(&self, format: &str) -> FormatResolution;
    pub fn is_task_runnable(&self, plugin_id: &str, capability: Capability) -> bool;
}
```

Part1 中 installed/license 使用只读桩：无安装记录即 `AvailableUninstalled`；测试 fixture 可注入 Authorized。Release 不提供全局跳过授权。

### 2.2 主缩略图 Router

新增 `src-tauri/src/thumbnail/router.rs`，导出纯函数 `route_thumbnail(file_format, thumb_status, &CatalogSnapshot) -> ThumbnailRoute`（与 generator 同为纯函数，不持 `AppState`）。

真实入口形态（R3）：`thumbnail_commands.rs` **无「单项」命令、无现成公共下层**——只有 `batch_request_thumbnails` 与 `start_full_thumbnail_generation` 两个命令，各自「查缓存 → `needs_gen` → `par_iter` 调 `generate_thumbnail`」。因此在**这两处**的 `needs_gen` 过滤点分别接入 `route_thumbnail`，不能塞进 `decode_media_step_inner`。`batch_request_thumbnails` 的缓存查询 SQL 须扩选 `file_format`（R7；`MediaItem` 已有该列），传入 Router，避免 Router 内再回查 DB。

规则：

1. 已有有效 `thumb_status=1/3`：`Existing`；
2. Catalog 不认领 thumbnail：`Common`；
3. Catalog 认领且 task done、指纹有效：`Existing`；
4. Catalog 认领且未完成：`Exotic(resolution)`，不调用 generator，发一次合并后的 wake；
5. 无插件/未授权/平台不支持：仍返回 `Exotic`，供前端显示准确占位。

batch、full、batch 内单元素、补偿重试四场景对同一 item 的路由必须经同一 `route_thumbnail` 判定并共用测试表。不得只修其中一个调用点。

### 2.3 前端查询命令

新增：

```text
list_exotic_format_resolutions()
get_exotic_item_state(item_id)
list_installed_exotic_plugins()
```

第一条返回 Catalog 中全部格式，因此未安装 PSD 也有 `AvailableUninstalled`。返回字段使用 camelCase 序列化，避免前后端手写字段转换漂移。

### 2.4 CLIP、人脸与主派生

CLIP/人脸待处理 SQL 增加：

```sql
AND NOT EXISTS (
    SELECT 1 FROM exotic_tasks et
    WHERE et.item_id = m.id
      AND et.capability = 'thumbnail'
      AND et.status <> 2
)
```

任务 done 后仍应优先使用 `thumb_path`，避免再次尝试原始 PSD。若输出指纹失效，失效事务先把 task 置 pending，再阻止 AI/face。

派生 backfill 对 cover/doc thumbnail 增加等价 `NOT EXISTS`：Catalog 已由 exotic `thumbnail` 认领的媒体不建立主派生封面任务。未来 exotic metadata/text 与主派生冲突时使用显式 capability→DerivationKind 映射，不按 media_kind 粗暴屏蔽全部派生。

修改 `state.rs`（R1/R4/R9）：

- 新增 `exotic_analysis_token`（与 `derivation_token` 同构 `Mutex<Option<CancellationToken>>`）；
- 新增 `should_yield_exotic()` = `is_scan_or_thumb_running() || is_interactive()`。**用于 Claimer 派发新任务前的判断**——exotic 是子进程解码，主进程无法 sleep 让子进程让出 CPU，「让步」= 暂缓领新任务 + Worker 进程低优先级（见 Part2 §3.6）；在途解码不可中断让步，只能自然完成或超时 kill；
- `should_yield_exotic` **不含** derivation：exotic 与 derivation 视为同级后台重活，由 Host 全局并发池公平协调（R4）。若硬让步 derivation，大视频库下 derivation 长期运行会饿死 exotic、PSD 永不出图；同一 item 不会被两者同时处理（exotic 认领格式不进主派生），故同级不产生同 item 互等；
- `ai_yield_blockers()` 在 exotic token 存在时追加 `"exotic"`（使 CLIP/人脸让步给 exotic）。单点改动即覆盖二者——`ai/pipeline.rs:287` 与 `ai/face_pipeline.rs:286` 同调该函数；
- 同步更新 `ai/face_pipeline.rs:25` 注释（原写「不新增、不修改 `ai_yield_blockers`」，v3.1 已修改，R9）；
- 避免锁顺序反转；读取多个 token 时不同时持有多把 Mutex。

### 2.5 认领冲突与平台状态

- Catalog 同一 format 只能有一个默认 offering；发布工具阻止冲突。
- 安装 manifest 的 formats 必须是 Catalog formats 的子集。
- manifest 不得声明 `override_common=true`；该权限只存在于主程序内置审核表。
- Catalog 有产品但当前 target 无包：`UnsupportedPlatform`，主路径仍让路并显示平台说明。
- min_host_version 不满足：`IncompatibleHost`，不得尝试启动旧/新协议 Worker。

## 测试

### 单元测试

- 内置 Catalog 解析、重复 format、非法 plugin_id、常见格式覆盖拒绝；
- common-first 分类；大小写扩展名归一化；
- `UpsertOutcome` 三分支与 SourceChanged 失效；
- task 原子领取并发测试；
- FormatResolution 全状态表；
- Thumbnail Router 单项/批量/全库一致性；
- Catalog 更新 backfill 与删除 offering 行为。

### 集成与规模测试

- 全新离线库扫描 PSD：task 存在、Availability 正确、主 generator 未调用；
- PSD pending 时 CLIP/face 查询不返回该项；task done 后返回且读取 WebP；
- 冷门视频不进入主 VideoCover backfill；
- 100 万普通媒体 + 100 个 exotic item：`EXPLAIN QUERY PLAN` 命中 task 索引；待处理查询 p95 < 50 ms（发布测试机）；
- 无 exotic offering 命中时，固定 10 万常见媒体扫描/缩略图吞吐相对基线回退 ≤2%。

## 本卷产出清单

- [ ] PSD 技术探针与报告；Catalog 仅声明实测能力
- [ ] `exotic/catalog.rs`、内置 Catalog、完整校验
- [ ] `exotic_tasks`/Catalog/installed schema 与 DAO
- [ ] scanner common-first 分类、任务 seed、SourceChanged 失效
- [ ] thumbnail Router 覆盖全部入口
- [ ] FormatResolution 查询命令
- [ ] CLIP/face/derive 跨流水线门控
- [ ] state 优先级接线
- [ ] 单元、集成、百万库基准通过

## 续作提示词

```text
实施 plan-docs/exotic_format_plugin_plan/exotic_format_plugin_v3_part1_foundation.md。
先读总纲 v3；先完成 PSD probe，不得预先宣称 PSB/CMYK/16-bit 支持。
按 P0→P1→P2 顺序；每阶段独立测试、中文 commit。
Part1 DoD 全过后，进入 exotic_format_plugin_v3_part2_pipeline_worker.md。
```
