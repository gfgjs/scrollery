---
id: 2026-07-24-Spec02_扫描与画廊
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec02-扫描与画廊

> 一句话:本篇讲扫描引擎(文件系统 → DB)与布局引擎(DB → 前端画廊几何)的 as-built 机制;服务扩展扫描/布局逻辑的工程师与低能力编码代理。读前无需先读其它篇,但涉及 DB 全表结构请交叉看 [数据层](./Spec01_数据层.md)。

## 1. 概览

扫描引擎负责把磁盘上的文件变成 `media_items` 行,布局引擎负责把 `media_items` 行变成前端画廊要渲染的行几何(justified/grid 布局)。二者是画廊数据流的前后两段:**FS → 扫描(engine+scanner) → DB → 布局查询(layout) → 前端画廊**。

代码位置:

| 目录 | 职责 |
|---|---|
| `src-tauri/src/engine/` | 图像解码引擎注册表(`EngineArena`),按格式分派 image-rs / WIC(Windows) |
| `src-tauri/src/scanner/` | 扫描根遍历、快速入库、后台信息丰富(enrichment)、卷探测/监听、Live Photo 配对 |
| `src-tauri/src/layout/` | Justified/Grid 布局算法、布局行缓存(`LayoutCache`)、视图取数缓存(`ItemsCache`) |
| `src-tauri/src/formats/` | 已注册格式运行时并集(内置表 ∪ exotic Catalog),供 UI 格式弹层/facet 分组 |

在整机中的位置(ASCII 概览):

```
                       ┌────────────── engine::EngineArena ──────────────┐
                       │ image-rs(内置格式) / WicEngine(heic/avif,Win)   │  ← 缩略图/编辑消费,详见 Spec03
                       └───────────────────────────────────────────────┘

FS(walkdir)
  │
  ▼
scanner::walker::MediaWalker ──分类── formats::merged_formats / utils::format(常见格式)
  │  (common-first,catalog 兜底 exotic 格式,见 §3.1)
  ▼
scanner::fast_scan::run_fast_scan ──批量事务── DB media_items/directories(见 Spec01)
  │  (spawn_blocking,§3.4 线程模型)
  ▼
scanner::enricher::run_enrichment(后台,fire-and-forget)──写回── EXIF/尺寸/sort_datetime/live_photo
  │
  ▼
ipc::layout_commands::compute_layout ──读── db::queries(query_layout_items*)
  │                                       │
  │                                       ▼
  │                              layout::items_cache::ItemsCache(视图取数缓存,§3.2)
  ▼
layout::justified::compute_justified_layout / compute_grid_layout(几何计算)
  │
  ▼
layout::cache::LayoutCache(布局行缓存,常驻)
  │
  ▼
ipc::layout_commands::get_bucket_rows* ──出口拼装(hydrate)── 前端画廊(Spec11)
```

上游调用者:前端 `start_scan`/`compute_layout` 等 IPC 命令(用户添加扫描根、滚动/换轴/换筛选触发)。下游消费者:缩略图生成(Spec03,消费 `media_items`/`thumb_status`)、AI/人脸(Spec06,消费 enrichment 后的媒体行)。

## 2. 数据模型与状态

DB 全表定义见 [数据层](./Spec01_数据层.md);本节只摘录扫描/布局查询用到的关键列与内存投影。

### 2.1 DB 关键列(摘要,全表见 Spec01)

- `scan_roots`:`id`/`path`/`alias`/`volume_id`/`status`(扫描状态机)。
- `directories`:`id`/`root_id`/`parent_id`/`rel_path`/`name`/`depth`/`mtime`(T17a 增量剪枝基线,`src-tauri/src/scanner/fast_scan.rs:184-204`)/`media_count`(直接媒体计数基线,`fast_scan.rs:403-404,668-673`)。
- `media_items`:`id`/`directory_id`/`file_name`/`file_size`/`file_mtime`/`file_format`/`media_type`/`width`/`height`/`sort_datetime`/`cache_key`/`volume_id`/`availability`('online'|'offline'|'missing')/`is_deleted`。

### 2.2 内存投影(`src-tauri/src/db/models.rs`)

- `LayoutItem`(`models.rs:153-181`):布局算法输入,每项含 `id/width/height/file_size/sort_datetime/file_format/media_type/is_live_photo/duration_ms/thumb_status/thumb_path/thumbhash/is_favorited/rating/color_label/availability/dir_id/similarity`。**不含文件名/目录路径**(S2 消脂,大表 JOIN 成本占查询 2/3,已剥离,`models.rs:176-178`)。
- `DirLabel`(`models.rs:187-200`):`dir_id → {rel_path, display, name, root_created_at, root_id}`,布局分组边界处按需还原目录标签,量级 10³。
- `MediaFilter`(`models.rs:405-433`):视图筛选投影(media_types/file_formats/live_photo_only/favorited_only/min_rating/color_label/date_range/directory_id/album_id/person_id/search_query/ai_search/trashed_only/recent_only 等)。其 canonical JSON 序列化即 `ItemsCacheData::filter_key`。
- `MediaMeta`(`models.rs:206-`起):重型逐项元数据(文件名/GPS/EXIF),仅可视区按需拉取,不随布局行常驻。

### 2.3 布局侧内存状态(`layout::cache` / `layout::items_cache`)

- `LayoutCacheData`(`layout/cache.rs:125-150`):常驻布局行(`Vec<LayoutRow>`)+ `total_height` + `layout_version`(全局原子计数器,`cache.rs:19`)+ `flat_ids`/`flat_rowcol`/`id_to_flat`(O(1) 相邻项/按 id 重锚定索引)+ `gen_key`(S3.1 幂等去重指纹)+ `separators`/`month_buckets`(时间轴用,与行同遍构建)。存于 `AppState` 的 `RwLock<Option<LayoutCacheData>>`(即 `LayoutCache` 类型别名,`cache.rs:154`)。
- `ItemsCacheData`(`layout/items_cache.rs:77-114`):视图取数缓存,`filter_key`+`order`(`CachedOrder` 枚举,基准序形态)+`data_version`+`items: Vec<LayoutItem>`+`id_to_idx`+`dir_labels`+`dir_rank`+`filter`+`reusable`(bool)+`median_aspect`(惰性)+`perm_memo`(folder 轴置换缓存)+`filename_rank`(惰性,B-file-iii)。存于 `AppState` 的 `RwLock<Option<ItemsCacheData>>`。
- 状态归属:两把缓存均为**进程内存**、随 Tauri 进程生命周期存在,重启即空(下次 `compute_layout` 触发 MISS 重建)。DB 是唯一持久真相源。

## 3. 关键流程与算法

### 3.1 增量扫描(fast scan)

主入口:`run_fast_scan`(`src-tauri/src/scanner/fast_scan.rs:325-690`)。步骤:

1. **入口守门(缺失检测第一道闸)**:`PathProber::is_online(root)` 判根路径可访问性(`scanner/volume_probe.rs:29-36`,取 `metadata` 而非 `exists`)。离线/不可访问 → **直接返回,不动 DB**(`fast_scan.rs:348-357`)。
2. **文件系统遍历**:`MediaWalker`(`scanner/walker.rs:94-219`)基于 `walkdir`,单线程**流式**产出 `WalkedFile`,不再全量收集 `Vec`(内存峰值从 O(N)~600MB@100 万降到 O(batch))。隐藏目录(`.` 前缀)整棵剪枝(`walker.rs:73-79,108`)。分类走 `classify_scanned_file`(`walker.rs:20-22`):common-first,即先查 `utils::format::classify_media_type`,未命中再查 exotic `CatalogSnapshot`。builtin offering(如标记式 `ocr`)不是真实扩展名,恒不入库(`walker.rs:180`, `formats/mod.rs:63-64`)。
3. **分块提尺寸 + 批量入库**:每 `BATCH_SIZE=500`(`fast_scan.rs:47`)项一批;批内前 `EAGER_DIM_COUNT=500`(`fast_scan.rs:57`,跨批累计)项经 rayon `par_iter` 并行做真实文件头读取(JPEG orientation、TIFF 超时保护见 §3.1.1),其余用 `cheap_phase2_dimensions` 给类型相关占位尺寸(`fast_scan.rs:115-136`,视频 1280×720/音频 400×400/PDF 595×842/其它文档 400×400),避免 0×0 拖垮布局(justified 按宽高比排版)。阶段5 起 eager 路径对非 TIFF 图像一次 `read_header_buf_for_ext` + `read_image_dimensions_buf` 共用尺寸与 orientation;TIFF 保持原路径。批一次性开写事务(`fast_scan.rs:467-576`),`ensure_dir_chain`(`fast_scan.rs:149-204`)递归 upsert 目录链并记录目录 FS mtime(T17a 剪枝基线);写热路径 DAO 用 `prepare_cached` 复用语句。
4. **T17b opt-in 快速扫描剪枝**(`decide_dir_pruned_at` + walker `DirPruner`,`fast_scan.rs:198`/`walker.rs:47`):`quick=true` 时,walker **进入目录时**即询问 `QuickDirPruner`;若该目录当前 FS mtime 与**扫描启动时一次性加载的快照**(`load_directory_mtime_snapshot`,`fast_scan.rs:393-398`)一致 → 跳过该目录直接文件的 `WalkedFile` 产出与 stat(剪枝命中时 stat 全免;未剪枝文件自 2026-08-23 起由单次 `symlink_metadata` 同时取类型/size/mtime),并把该目录全部未删媒体 id 回填进本根 seen TEMP 表(防误删)。子目录仍独立下降(逐目录、非递归)。**J1 修复**(`fast_scan.rs:215-226`):基线**只读快照**、不查活 DB 行 —— 若查活行,本轮扫描中 `ensure_dir_chain` 会持续覆写同一目录行的 mtime(walkdir 无序遍历可能先降到子目录、顺路 upsert 祖先),导致「已被本轮覆写的新值」被误当「上一轮终态」比对,从而误判剪枝、永续漏扫新增文件。已知残留窗(接受):quick 扫描中途取消/出错时,已提交批次可能已覆写某祖先活 mtime 而其新文件未入库,下一轮快照即含「被治愈」值,跨轮漏扫直到全量扫描兜底(`fast_scan.rs:389-392`,2026-07-23 复核裁定接受,勿重报)。**唯一且已知的漏检边界**:文件**就地编辑**(内容变、父目录 mtime 不变,如同大小 EXIF/评分写回)会被跳过,由全量扫描兜底。
5. **可疑变更(SuspectChanged)三环判定**:`upsert_fast_scan_item` 返回 `UpsertOutcome`(Inserted/Unchanged/SourceChanged/SuspectChanged,定义于 `db::queries`,详见 Spec01)。`SuspectChanged`(mtime 变但 size 同)项攒批(`fast_scan.rs:540-544`),**批事务提交、写锁释放后**才在事务外计算内容指纹(`content_fingerprint`,读文件 IO 绝不进写事务,`fast_scan.rs:580-614`),再逐项短写定案(`resolve_suspect_change`)为 touch(mtime 抖动,派生全保留)或 SourceChanged(元数据编辑,派生全失效)。
6. **exotic 任务播种/失效**:扫描事务内(`fast_scan.rs:546-573`)按 `catalog.resolve_format` 命中的非 builtin offering 播种/失效 exotic 处理任务(详见 Spec09),不等 enrichment。
7. **缺失检测收尾(四道闸)**:生产路径 `finalize_missing_detection_preloaded`(`fast_scan.rs:318`)。seen 集自阶段4 起为连接级 TEMP 表流式累积(每文件 upsert 后经 `SeenWriter` 写入),收尾不再搬 HashSet;2026-08-23 起表按根隔离为 `_mm_seen_r{root_id}`——单写连接上不同根扫描可并发,单表会被后启动者清空、致进行中扫描收尾误标(P1 已修);`mark_missing_preloaded`(`mark_missing.rs:66`)表缺失时保守返回 0 不标,真实差集完成后 DROP 本表(`temp_store=MEMORY` 下免多根常驻)。闸序:① 完整门闩 `walk_complete`(`MediaWalker::finish()` 的 `WalkOutcome.complete`,遍历/metadata 错误非空或被取消则 `false`,`walker.rs:119-135`)——不完整必不删;② TOCTOU 复查(写删前再 `prober.is_online(root)` 一次,防扫描中途拔盘,`fast_scan.rs:663`);③ 在线卷集(本根 `volume_id`,`None` → 空集 → 不标,`fast_scan.rs:301-302`);④ `mark_missing_preloaded` 内置的 seen 差集守门(本根子树 ∩ 在线卷 ∩ ¬seen)。四道闸任一不过 → 返回 0、**绝不删除**。
8. **收尾写回**:`set_directory_media_count`(T17a 剪枝基线,绝对覆盖非累加,`fast_scan.rs:668-673`)、`finish_scan_root`、经 `Channel<ScanChannelPayload>` 发 `Completed` 事件(含 `marked_missing` 计数供前端 toast)。

#### 3.1.1 JPEG/TIFF 尺寸读取

`scanner::metadata::read_image_dimensions`(`metadata.rs:150`)= `read_raw_dimensions` + JPEG 按 EXIF orientation(`5..=8`)交换宽高(`apply_orientation_swap`,`metadata.rs:132`)。fast_scan eager 与 enrichment 优先走 `read_image_dimensions_buf`/`read_raw_dimensions_buf`,从一次 open 的 `HeaderBuf` 解尺寸与 orientation。TIFF 解析可能读取大量字节且畸形文件可能无限阻塞,用 `run_with_timeout`(detached 线程 + `recv_timeout`,`metadata.rs:19-39`)包一层硬超时 `TIFF_DIMENSION_TIMEOUT=5s`(`metadata.rs:17`)。

### 3.2 后台信息丰富(enrichment)

主入口:`run_enrichment`(`scanner/enricher.rs:118-`起),扫描完成后由 `start_scan` 经 `tokio::task::spawn_blocking` + `std::panic::catch_unwind` 触发(fire-and-forget,`ipc/scan_commands.rs:653-716`)。

- **线程模型**:CPU 密集且部分 IO(EXIF/XMP 读取)的探测跑在**保留一个前台核**的独立 rayon 池(`reserved_core_pool`,`enricher.rs:36-46`)——与派生流水线(用户交互即完全暂停)不同,enrichment 是用户正在等待的导入工作、且自身进度事件会自动触发重排,故不能"交互即让步"整体暂停;改为固定保留一核,兼顾 UI 流畅与导入不被拖慢。
- **批次**:`ENRICHMENT_BATCH=500`(`enricher.rs:34`)。
- **主要工作**(图片段 `par_iter` 并行,`enricher.rs:211`起;视频段 `enrich_videos` `enricher.rs:418-450`;音频段 `enrich_audios` `enricher.rs:538-570`):EXIF 解析回填真实宽高、`sort_datetime` 修正(EXIF 拍摄时间,`parse_exif_datetime` 按 UTC 墙钟存,见 §4)、XMP Motion Photo 信号探测(`detect_motion_photo_xmp*`)、`update_live_photo_flags`。图片段选批按视图序三模式(2026-08-23 定型):默认 date+datetime 序用 `(sort_datetime,id)` keyset + V24 `idx_media_type_sort`;不可 keyset 的序(folder 树序 / date+filename 日桶序)走一次性排序灌入的 TEMP 队列表(表名带调用代次 + RAII 收尾 DROP),按 seq 游标续取——全部视图序均脱离原「每批重排剩余候选集」的 O(N²/500) 行为;主源耗尽后补一轮全量 fallback 兜底并发插入与失败重试。视频/音频队列用 `m.id` keyset + `idx_media_type_id`。
- **进度/终态事件**:`db:media_enriched`(`MediaEnrichedPayload`)与 `enrichment:completed`(`EnrichmentCompletedPayload`,`enricher.rs:60-91`)。终态**必须**携带 `error_code`(`None`=正常,`Some("enrich_failed"/"enrich_panicked")`=异常)——避免失败伪装成正常完成、只进日志对用户不可见。取消(`AppError::Cancelled`)视为正常终态发 `ok`,真错误/panic 才带错误码(`ipc/scan_commands.rs:666-702`)。
- **批提交后 bump 数据版本**(`bump_layout_data_version`,`enricher.rs:111-116`):经 `AppHandle` 取 `AppState` 调 `bump_data_version()`,使 items 取数缓存失效——尺寸回写/EXIF 时间修正改变了布局的几何与排序输入。

### 3.3 Live Photo / Motion Photo 配对

`scanner::live_photo::pair_live_photos`(`live_photo.rs`起):按 `(directory_id, file_stem)` 聚合(`Group`,`live_photo.rs:37-47`)。两类:①Apple Live Photo(jpg/jpeg/heic/heif + `.mov`)纯 stem 配对;②分体式 Motion Photo(jpg + `.mp4`)**motion 守门**——仅当静图侧 `has_embedded_video=1`(enrichment 阶段 `detect_motion_photo_xmp` 置位)才配,避免普通照片与恰好同名 `.mp4` 被误吞为 companion(`live_photo.rs:16-19`)。

### 3.4 卷探测与插拔监听

- `scanner::volume_probe`:`VolumeOnlineCheck` trait(`volume_probe.rs:21-26`)抽象在线判定,生产实现 `PathProber`(取 `metadata().is_dir()`)。**范围声明**:当前只做路径级可访问性判定,原生卷稳定 ID(Win 卷 GUID / mac `DAVolumeUUID`)枚举是 Part2 T1/T2 的完整实现,尚**待落地**——本模块刻意只做路径兜底,标注见 `volume_probe.rs:15-19`。`VolumeResolver`/`PathVolumeResolver`(`volume_probe.rs:47-76`)以「卷挂载根」派生 `stable_id`,同盘多根并卷;**不抗盘符重映射**。
- `scanner::volume_watch::run_once`(`volume_watch.rs:44-`起):后台线程 `POLL_INTERVAL=15s`(`volume_watch.rs:25`)对账已知卷在线态,仅变化时写 `set_volume_online` + `bulk_set_availability`(online↔offline),经 `EVENT_VOLUMES_CHANGED` 事件通知前端。**正交铁律**:只切 `availability`(online↔offline),绝不碰 `is_deleted`、绝不动 `'missing'`(`volume_watch.rs:9-11`)——`offline` 是卷级可自动恢复态,`missing` 是扫描差集的文件级结论,离线卷重连后由**扫描**恢复 missing,监听只管 online/offline。冷启动延迟 `STARTUP_DELAY=5s`(`volume_watch.rs:28`)让首轮扫描先行,避开锁竞争。

### 3.5 图像解码引擎注册(`engine::EngineArena`)

`EngineArena::phase1()`(`engine/mod.rs:35-41`):注册顺序即分发优先级——`ImageRsEngine` 排首位(Phase 1 常见格式 jpg/png/webp/bmp/gif/tiff 保持既有引擎行为不变),`WicEngine`(仅 `cfg(windows)`)排后兜接 image-rs 不认的 heic/heif/avif/ico。`engine_for(format)` 取首个 `can_handle` 命中(`engine/mod.rs:45-47`)。WIC 对 heic/avif 依赖系统已装 HEIF/AV1 扩展,缺失时解码失败按既有链路记 `thumb_status=2`(运行时检测降级)。非 Windows 平台当前 heic 无引擎(`UnsupportedFormat`),mac Image I/O 引擎待 Part3 T9 落地(`engine/mod.rs:76-83`,测试锁定当前跨平台现实)。

### 3.6 布局引擎(justified/grid)

主算法:`compute_justified_layout`(`layout/justified.rs:358-`起)、`compute_grid_layout`(`justified.rs:576-`起)。输入为按视图序排好的 `&[LayoutItem]` 切片 + `LayoutParams`(`justified.rs:140-150`:容器宽/目标行高/间距/分组轴/组内排序/无缝分组标志)。

Justified 算法(Google Photos / Flickr 两端对齐布局,标准流式装箱):
```
for each item:
    aspect = width / height
    累加到当前行(按目标高度缩放)
    行宽达到 container_width ± tolerance → 提交本行(scale 到恰好贴边,
        行高上限裁到 target_h × MAX_ROW_HEIGHT_FACTOR(=2.0), 见 justified.rs:156-161)
    sort_datetime 跨日边界 → 先插入 Separator 行(除非 seamless=true)
```
最后一行短于 `LAST_ROW_JUSTIFY_THRESHOLD=0.6`(`justified.rs:155`)倍容器宽时保持原样不强制拉伸,否则两端对齐。

**几何/载荷分离(S3)**:布局行本身(`LayoutRow`,`justified.rs:38-70`)只存 `SlimRowItem{id,x,w,h}`(`justified.rs:74-79`,Copy、零堆载荷),不带文件名/缩略图路径等厚载荷——常驻内存降至约 32B/项。厚载荷(`LayoutRowItem`,`justified.rs:108-135`,含 file_format/is_live_photo/thumb_path/placeholder_color/rating 等全字段)只在**出口拼装**(`hydrate_item`,`justified.rs:169-193`;批量见 `items_cache::hydrate_rows`,`items_cache.rs:469-`起)时,针对**可视区**(10²级行)从 `ItemsCache` 现场拼装,serde 形状与 S3 之前完全一致(前端零改动)。占位平均色 `placeholder_color` 在此一次性算好(`average_color_hex`),替代原逐项过桥 thumbhash 数组 + 前端逐帧现算均色(§ experience 4)。

### 3.7 视图取数缓存(`items_cache`)与命中/派生

`compute_layout`(`ipc/layout_commands.rs:160-`起)把「取数」与「几何」拆开:滑块/窗宽/布局模式/分组轴等几何交互**不改变视图集合**(WHERE 子句不变),故不必每次都重跑 SQL。

- **命中键**:`filter_key`(`MediaFilter` 的 canonical JSON)+ `data_version`(`AppState` 全局数据版本,任何成员/几何/顺序写路径 bump)+ 序形态匹配(`CachedOrder`)。
- **双键统一缓存(D-015 + B-file-iii)**:`CachedOrder::Canonical`(datetime 基准,`sort_datetime DESC, id DESC`)与 `CachedOrder::CanonicalFilename`(filename 基准,`file_name COLLATE NATURAL_CMP ASC, id ASC`)**互为可派生**——任一基准都能在内存里派生另一轴的顺序(`derive_order`,`items_cache.rs:321-`起),而不必重新发 SQL。可派生性判据 `can_derive_axis`(`items_cache.rs:210-216`,**单一事实源**,`is_hit_valid` 与 `compute_layout` 的 HIT 判据都必须调它,不得各自维护副本):datetime 轴任意分组皆可派生;filename 轴 none/folder/**date** 三种分组皆可派生(date+filename 用 UTC 日桶 `sort_datetime.div_euclid(86400)` 内存分桶,与 SQL `push_order_by` 逐值等价)。
- **HIT 分支**(`layout_commands.rs:286-347`):items 读锁内 `derive_order` 得到派生序 + 调 `run_layout`(几何计算),**不发 SQL**;`median_aspect`(宽高比中位数,布局需要)惰性缓存于快照(`OnceLock`)。
- **MISS 分支**(`layout_commands.rs:349-`起):datetime 家族走基准序免 JOIN 查询(`query_layout_items_canonical`),取数与布局分离持锁——读连接仅在查询期间持有,CPU 密集的布局计算前即释放(否则会钉住 4 个池连接之一,拖慢滚动时并发的可视区读取)。查完回填 `ItemsCacheData` 存入 `ItemsCache`。
- **S3.1 幂等去重**:`gen_key`(filter+参数+`data_version` 拼接指纹)与 `dedup_summary`(`layout_commands.rs:211-243`)——前端可能以完全相同输入连发 `compute_layout`(挂载/统计返回/尺寸观察等多触发源),命中则直接复用现行 `LayoutSummary`、免全量重排、**版本不换代**。
- **filename_rank 惰性填充**:驻留命中源是 datetime 基准而请求 filename 序时,首次经 `ensure_filename_rank_for_hit`(`layout_commands.rs:84-151`)补一次 id-only NATURAL_CMP 查询(优先全局 filter-invariant rank,`AppState::try_global_filename_ranks`,免 DB;未就绪才退化 per-filter 查询)得到位次,写入 `ItemsCacheData::filename_rank`(`OnceLock`),此后 datetime↔filename 互切全命中。

### 3.8 并发/线程模型摘要

| 工作 | 线程归属 |
|---|---|
| `run_fast_scan` | `tokio::task::spawn_blocking`(`ipc/scan_commands.rs:606-620`) |
| `run_enrichment` | `tokio::task::spawn_blocking` + `catch_unwind`(`ipc/scan_commands.rs:653-716`),内部用保留一核的独立 rayon 池 |
| `compute_layout` 的取数+布局 | 同一个 `tokio::task::spawn_blocking` 闭包内完成(`layout_commands.rs:245-246`),二者均不阻塞 tokio 工作线程 |
| `volume_watch::run_once` | 独立后台线程,15s 轮询 |
| Justified/Grid 单次布局计算 | 单线程(`compute_layout` 闭包内),分组打包并行走 `layout_groups_parallel`(`justified.rs:297-`起,按 `group_by` 分组后 rayon 并行打包各组) |

## 4. 契约与不变量(施工红线)

IPC 命令(全量见 [Spec10](./Spec10_IPC与错误契约.md)),本篇仅列扫描/布局相关命令名:`add_scan_root`/`remove_scan_root(_with_options)`/`set_scan_root_hidden`/`relink_scan_root`/`check_folder_overlap`/`list_scan_roots`/`start_scan`/`stop_scan`/`clear_database`(`ipc/scan_commands.rs`);`compute_layout`/`get_view_ids`/`get_bucket_rows`/`get_layout_rows_by_y`/`get_item_y_by_id`/`get_subtree_scroll_target`(`ipc/layout_commands.rs`)。`get_layout_rows` 与 `get_separator_y_by_group_id` 已随 P14 消融删除。

不变量清单:

1. **不完整扫描 ≠ 删除**(最高优先级红线):`WalkOutcome.complete`(遍历/metadata 错误非空或取消 → `false`)为 `false` 时,`finalize_missing_detection` 必须直接返回 0、绝不调用差集删除(`fast_scan.rs:290-295`,`finalize_tests::incomplete_walk_blocks_deletion` 锁定)。为什么:遍历错误意味着 `seen` 集不完整,若仍按差集判"未出现即删除",会把**因权限/IO 而未能扫到、但实际仍在盘上**的文件误标为 missing。
2. **卷离线 ≠ 删除**:扫描入口守门 + 写删前 TOCTOU 复查双重把关(`fast_scan.rs:348-357,656-666`)。为什么:防止扫描中途拔盘导致的误删,以及离线卷本就不该被判定"文件消失"。
3. **卷未识别(`volume_id=None`)→ 空在线集 → 不标 missing**(`fast_scan.rs:301-302`,`finalize_tests::unidentified_volume_marks_nothing` 锁定)。为什么:宁可不删、不可误删(Part2 §5 不变量 4)。
4. **quick 剪枝基线只读扫描启动时的一次性快照,不查活 DB 行**(J1 修复,`fast_scan.rs:215-226`)。为什么:活行会被本轮 `ensure_dir_chain` 持续覆写,若拿"已被本轮覆写的新值"当基线比对,会恒等而误判剪枝、造成永续漏扫。**改动 `decide_dir_pruned` 前必须理解此契约,勿改回查活行。**
5. **序等价契约(刚性)**:`derive_order` 的内存派生输出必须与 `push_query_body` 的 SQL `ORDER BY` 逐项等价(`items_cache.rs:20-22`)。为什么:`get_view_ids`(走 `flat_ids`)与 `view_to_sql`(`SelectAll` 走 SQL 解析)分别源于两条路径,错位即前端选区(多选/范围选)漂移。改 SQL ORDER 必须同步改 `derive_order` 并跑对拍测试(experience §11)。
6. **布局缓存四契约**(正典在 `items_cache.rs`/`cache.rs` 模块头注释,`experience.md` §11 仅路标):①任何写路径提交后必须 `bump_data_version`;②改 SQL ORDER 须同步 `derive_order` 并过序对拍;③items 快照是布局行的唯一载荷源(出口拼装靠它,不得整体清空除非 `clear_database` 等硬失效场景);④改 HIT 守卫(`layout_commands.rs` 内联判据)必须同步改 `items_cache::is_hit_valid`——两处判据必须逐字一致,否则去重预检与真实 HIT 分支会不一致地放行/拒绝。
7. **锁纪律**:`layout_items_cache`(items 缓存)与 `layout_cache`(行缓存)是两把独立 `RwLock`,任何路径不得同时持有两把(`items_cache.rs:24-25`)——`compute_layout` 在 items 读锁内跑布局,出锁后才 `store_layout`。
8. **软删除不失效布局缓存**(有意,`layout/cache.rs` "失效契约"注释指针,见 experience §11):为暂存删除的 UX(用户删除后短暂仍可见于当前视图、便于撤销)服务,不 bump。
9. **`can_derive_axis` 是可派生性单一事实源**(`items_cache.rs:210`):`is_hit_valid` 与 `compute_layout` 内联 HIT 判据都必须调用它,不得各自维护判据副本(否则两处"能否派生"会漂移)。
10. **`sort_datetime` 按 UTC 墙钟存**(`scanner/metadata.rs::parse_exif_datetime`,`items_cache.rs:207-208` 注释指出原 SQL `'localtime'` 是双重时区偏移 bug、已修正为 UTC 日界):date 分组的月桶/分隔符标签与 filename 派生的 UTC 日桶必须共用同一时区基准,否则会出现"同一天"判定不一致。

## 5. 边界与失败模式

| 场景 | 处理 |
|---|---|
| 空扫描根(无媒体文件) | 正常完成,`inserted=0`,`Completed` 事件正常发出 |
| 扫描根路径不可访问(离线/网络盘断) | 入口守门直接返回 `Ok(0)`,不动 DB(`fast_scan.rs:348-357`) |
| 扫描中途卷离线(拔盘) | TOCTOU 复查拦截缺失检测差集,不误删(`fast_scan.rs:663`);扫描本身可能因 IO 报遍历错误,进而触发不变量 1 |
| 遍历权限错误 / symlink loop | 计入 `WalkError`,不致命,但令 `walk_complete=false`,拦截本轮缺失检测(`walker.rs:154-164`) |
| 文件在遍历后、metadata 读取前消失(竞态) | metadata 失败计入 `errors`(非静默丢弃),同样拦截缺失检测(`walker.rs:183-192`) |
| 畸形 TIFF 无限阻塞 | `run_with_timeout` 5s 硬超时兜底,超时返回失败尺寸(`metadata.rs:17,77-93`) |
| 可疑变更指纹计算失败(读失败/竞态删除/权限) | 保守判 `SourceChanged`(全失效),而非假定未变(`fast_scan.rs:584-592`) |
| 大目录(百万级文件) | 流式遍历 O(batch) 内存峰值(非 O(N));批量事务 500 行/批;首屏尺寸提取有预算上限(`EAGER_DIM_COUNT`),其余占位 + enrichment 回填 |
| 重复扫描(同根并发触发) | `start_scan` 先 `state.cancel_scan(root_id)` 取消旧 token 再发新 token(`ipc/scan_commands.rs:570-571`) |
| enrichment panic | `catch_unwind` 捕获,发 `enrich_panicked` 终态事件,清理 scan token(不使 AI 流水线永久让步)(`ipc/scan_commands.rs:693-716`) |
| Enrichment 被用户取消 | 视为正常终态(`Cancelled` 不算错误),仍发 `ok` 终态事件使前端进度 UI 停转(`ipc/scan_commands.rs:669-676`) |
| 布局请求 ai_search 视图 | `cacheable=false`,恒 MISS 全量重查(`ai_search_results` 随每次搜索整表重写,快照仍驻留作载荷源但 `reusable=false`)(`layout_commands.rs:256-258`) |
| items 缓存查无某 id(布局行与快照换代竞态窗口) | `placeholder_item` 占位(`justified.rs:198-`起):几何保留(不塌行高),载荷置空,`thumb_status=0` 前端按待生成骨架渲染,下次换代自愈 |
| 待核实 | 非 Windows(macOS/Linux)平台的 heic/avif 引擎缺失范围与后续 Image I/O 落地时间点,仅代码测试锁定"当前无",无独立设计文档交叉核实 |

## 6. 重建指引(从零实现)

依赖顺序(建构先后):

1. **格式识别**(`utils::format` 内置表,不在本篇范围但为前置依赖)→ 2. **exotic Catalog 投影**(`formats::merged_formats`,依赖 1)→ 3. **卷探测/身份**(`scanner::volume_probe`,独立,可与 1/2 并行)→ 4. **walker**(`scanner::walker`,依赖 1/2 的分类函数、依赖 3 的 `VolumeOnlineCheck` trait 仅在 fast_scan 层接入)→ 5. **fast_scan**(依赖 4,依赖 DB 层 `db::queries::scan` 见 Spec01)→ 6. **enricher**(依赖 5 已入库的行、依赖 `scanner::metadata`/`scanner::live_photo`)→ 7. **volume_watch**(独立后台线程,依赖 3)→ 8. **engine::EngineArena**(缩略图/编辑消费,依赖 1,可与 4-7 并行开发)→ 9. **layout**(依赖 DB 查询层 `db::queries::layout`,与扫描链路解耦,可独立开发/测试,只要求 `media_items` 表已有数据)。

外部 crate/npm 包:

| Crate | 用途 |
|---|---|
| `walkdir` | 递归目录遍历(`scanner::walker`) |
| `rayon` | 批内并行提尺寸(fast_scan)、enrichment 并行探测、布局分组并行打包(justified) |
| `tokio_util::sync::CancellationToken` | 扫描/enrichment 取消令牌 |
| `rustc-hash`(FxHashMap) | items/layout 缓存的 id 索引(百万级整数键) |
| `chrono` | 日期分组标签(justified) |
| `filetime` | 测试中 mtime 精确控制(J1 表征测试) |
| `image`(`image::image_dimensions`) | 无解码读取像素尺寸 |
| `kamadak-exif` | EXIF 解析(orientation、拍摄时间、GPS) |
| `windows`(`cfg(windows)`) | WIC 解码引擎(`engine::gpu::wic_engine`) |

坑与教训(链接 [experience.md](../experience.md)):

- §4(画廊极密网格快滚飞掠卡顿):布局占位色改为后端 hydrate 一次性 `#rrggbb`,替代逐项 thumbhash 数组过桥 + 前端逐帧现算——本篇 `hydrate_item`(`justified.rs:169-193`)即该修复的落点。
- §11(布局缓存四契约):改动 `items_cache.rs`/`cache.rs` 前必须先读模块头注释,契约详情见本篇 §4 第 6 条。
- §15(评审"补齐周期机制"提案先查编译期默认值):不直接适用本篇机制,但提示"新增布局/扫描周期性任务前先确认现有 spawn_blocking/rayon 池默认行为是否已覆盖"。
- §19(集合级契约证明不了"选得对"):`can_derive_axis`/`is_hit_valid`/HIT 内联判据三处必须逐字同判,门禁绿(编译通过、测试通过)不等于三处判据语义仍一致——改动后务必人工比对。

验收:

- 单测:`src-tauri/src/scanner/fast_scan.rs` 内 `exotic_seed_gate_tests`/`finalize_tests`/`dir_baseline_tests`/`quick_scan_tests`(含 J1 表征测试 `ancestor_chain_overwrite_does_not_corrupt_pruning_baseline`);`src-tauri/src/scanner/walker.rs` 内 walk 完整性测试;`src-tauri/src/formats/mod.rs` 内合并集契约测试;`src-tauri/src/engine/mod.rs` 内引擎分派测试。
- Gate 命令:`cargo test`(workspace 内 `src-tauri`),`cargo clippy`。布局/扫描无独立 e2e,真机 GUI 验收(百万级滚动、拔盘/断网等物理场景)需人工执行,标注为 not automated。

## 7. 关联

- 上游正典:[Part2 扫描与画廊流水线](../refactor_2026/Part2_扫描与画廊流水线.md)(2026-06-26 定稿,理由/决策见其 §1-§3)。**as-built 漂移**:Part2 §1.1-8 提出的「时间轴滚动条(后端支持:年/月密度直方图)」已在当前实现中落地为 `MonthBucket`(`layout/cache.rs:41-50`)+ `SeparatorInfo.epoch_day`(`cache.rs:23-34`),并在其上演化出更复杂的前端轴/minimap 交互(独立缓存 `HLayoutCache`/`layout/hcache.rs`、悬停显隐、无缝分组解耦等)——这些前端交互属 [Spec11 前端架构](./Spec11_前端架构.md),Part2 原文的"直方图"描述已不足以覆盖现状,以本篇 §3.6-3.7 与代码为准。
- 前端画廊 UI(BucketVirtualScroll/MediaGrid/时间轴 minimap 组件):[Spec11 前端架构](./Spec11_前端架构.md)。
- 缩略图与派生生成(`thumb_status`/`thumb_path` 回填、封面/关键帧):[Spec03 缩略图与派生](./Spec03_缩略图与派生.md)。
- DB 全表结构、`idx_media_sort` 复合索引、`tree_sort_key`/keyset 分页:[Spec01 数据层](./Spec01_数据层.md)。
- IPC 命令全量清单、错误码契约:[Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)。
- 相关设计:`docs/experience.md` §4(画廊快滚卡顿)、§11(布局缓存四契约)。
