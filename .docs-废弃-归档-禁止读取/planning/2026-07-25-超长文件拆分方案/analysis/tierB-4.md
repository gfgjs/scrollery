---
id: 2026-07-25-tierB-4
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# TierB-4 简案:items_cache.rs / models.rs / justified.rs / face_pipeline.rs

范围:50–70KB 简案组第 4 组,四文件均只读分析,不改代码。锚点以符号名为主(并行注释精简线在跑,行号会漂)。

## src-tauri/src/layout/items_cache.rs

**① 缓** — 本文件是画廊「视图取数缓存」的单一事实源,读路径(`derive_order`/`is_hit_valid`)与 `CachedOrder`/`ItemsCacheData` 高度耦合,注释里明写多处"改一处必改另一处"(如 `can_derive_axis` 与 compute_layout 的 HIT 判据、`is_hit_valid` 与 HIT 守卫)。真正可摘的两块价值有限,先缓,不急拆。

**② 若拆,主刀口:**
- `items_cache.rs` → `items_cache/global_rank.rs`:迁 `GlobalFilenameRank`/`build_global_filename_rank`/`new_global_rank_cell`(B-file-iii,自含结构体+impl,与 `derive_order` 零耦合,只经 `AppState::try_global_filename_ranks` 消费)
- `items_cache.rs` → `items_cache/patch.rs`:迁写路径群 `patch_or_degrade`/`apply_thumb_results`/`set_dimensions`/`set_favorite`/`set_rating`/`set_color_label`(与读路径只共享 `ItemsCacheData` 类型,无算法耦合)
- 核心留守:`CachedOrder`/`PermMemo`/`ItemsCacheData`/`can_derive_axis`/`is_hit_valid`/`derive_order`/`hydrate_rows` 须同文件(语义强耦合,拆开即破坏"同判据"契约的同屏可见性)

**③ 最大风险:** `can_derive_axis`/`is_hit_valid` 两处"单一事实源"注释若跨文件,后续改一侧漏改另一侧的概率上升——是维护契约风险,非性能风险。

## src-tauri/src/db/models.rs

**① 拆** — 纯数据结构 + serde derive,零算法耦合;17 个调用点(`ipc/*_commands.rs`、`db/queries/*.rs`)已天然按域取子集导入(如 `queries/scan.rs` 只要 `{DirFile,DirNode,Directory,ScanRoot}`,`queries/faces.rs` 只要 `{FaceThumb,LikelyMatchGroup}`),按域分文件是本组四件中最省力、摩擦最低的一项。

**② 主刀口:**
- `models.rs` → `models/{scan.rs, media.rs, view.rs, ai_face.rs, storage.rs, derived.rs}` + `models/mod.rs` 用 `pub use` 全量再导出,17 处 `use crate::db::models::{...}` 零改动
- 域锚点:scan=`{ScanRoot,Directory,DirNode,DirFile}`;media=`{MediaItem,LayoutItem,DirLabel,MediaMeta,ImageMeta,AudioMeta,AudioDetail,MediaDetail,VideoMeta}`;view=`{MediaFilter,DateRange,SortSpec,ViewScope,GalleryFilter,ViewDescriptor,SelectionDescriptor}`;ai_face=`{AiStatus,FaceStatus,AiEmbedding,SemanticSearchResult,AiStatusSummary,FaceStatusSummary,DerivationStatusSummary,PersonSummary,FaceModelInfo,FaceBox,FaceThumb,LikelyMatchGroup}`
- 唯一跨域方法 `ViewDescriptor::to_media_filter`(view.rs 引用 media.rs 的 `MediaFilter`)随 view.rs 同迁,注意可见性

**③ 最大风险:** 纯移动本身低风险,唯一须警惕的是 serde `rename_all`/字段属性在搬运中被手误改动——一个字符差即 wire 格式静默漂移,前端零感知。

## src-tauri/src/layout/justified.rs

**① 拆** — `compute_justified_layout`/`compute_grid_layout` 两套打包算法与共享骨架(`layout_groups_parallel`/`group_mark`/`group_label`/输出类型)边界清晰;`horizontal.rs` 已跨文件复用本文件 `pub(crate) aspect_ratio`/`median_measured_aspect` 的先例证明拆分摩擦低。

**② 主刀口:**
- `justified.rs` → `layout/geometry.rs`:迁 `LayoutRow`/`SlimRowItem`/`HydratedRow`/`LayoutRowItem` 类型 + `hydrate_item`/`placeholder_item` + `group_mark`/`group_label` + `layout_groups_parallel` + `aspect_ratio`/`median_measured_aspect`(共享骨架,`pack_group: Fn(&[I]) -> (Vec<LayoutRow>, f64)` 签名不变)
- `justified.rs` 留 `compute_justified_layout`(含 `commit_row` 闭包);→ `layout/grid_pack.rs` 迁 `compute_grid_layout`(含 `commit_grid_row` 闭包)
- 跨算法特征化测试(如 `seamless_justified_matches_none_packing` 同时断言两算法一致)归属待定,建议落 `geometry.rs` 或独立 tests 聚合文件,避免又用 `use super::*` 把刚拆开的两个文件重新打通

**③ 最大风险:** `layout_groups_parallel` 的泛型签名 `I: Borrow<LayoutItem> + Sync` 与传入闭包必须逐字跨文件保持一致;任何"为兼容拆分"把闭包改成 `Box<dyn Fn>`/trait object 都会在百万项热路径引入间接调用与堆分配,必须点名禁止。

## src-tauri/src/ai/face_pipeline.rs

**① 拆** — 模块头已自述四阶段管线(Producer → 解码源三级定源 → worker 派发 → Writer),各阶段函数群天然独立、测试占比极小(仅约 30 行),按管线阶段分文件是本组价值最高、风险最低的一项。

**② 主刀口:**
- `face_pipeline.rs` → `face_pipeline/producer.rs`:迁 `produce_face_tasks`(`FaceTask` 定义留 mod.rs 供跨阶段共享)
- `face_pipeline.rs` → `face_pipeline/decode_source.rs`:迁 `resolve_face_decode_source` + `FaceDecodeSource`/`FaceSourceKind` + `face_cache_applies` + `faces_to_records` + `WORKER_DECODABLE_FORMATS`
- `face_pipeline.rs` → `face_pipeline/dispatch.rs`:迁 `face_dispatch_loop`/`dispatch_face_batch` + `FacePlan`/`FaceDispatchStats` + `FACE_DISPATCH_BATCH`/`face_dispatch_cap`;→ `face_pipeline/writer.rs`:迁 `write_face_results` + `flush_face_rows`/`flush_cluster`/`flush_face_failed`
- `mod.rs` 留 `start_face_pipeline`/`run_face_pipeline_blocking`/`run_face_pipeline_worker_blocking`/`recover_orphaned_face_items`/`reconcile_unclustered_faces` + `FaceResult` + 常量,`rayon::scope` 编排三阶段跨文件 fn 调用

**③ 最大风险:** `is_cancelled` 门控散落 5 处(`produce_face_tasks`/`write_face_results`/`face_dispatch_loop`/`dispatch_face_batch` 内两次/`run_face_pipeline_worker_blocking` 终态判定),纯移动务必逐点核对取消语义未被"顺手"合并或简化——尤其 `start_face_pipeline` 结尾 `token_outer.is_cancelled()` 判定 GPU 槽释放的分支,不得随文件搬迁误挂到别的 token 克隆上。

## 顺手发现

无。
