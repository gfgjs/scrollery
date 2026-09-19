---
status: 施工中
type: 分析报告
line: 缩略图派生流水线只读分析
created: 2026-09-06
---

# 提示词2 只读分析报告：缩略图/派生文件生成流水线（B1–B10 核实）

> 任务来源：`docs/reviews/2026-09-05-性能流水线梳理与分线优化提示词.md` 提示词2。
> 快照基线 dev@7fea98e3 = 当前 HEAD 7fea98e3（同一提交，**无行号漂移**，下文行号即当前行号）。
> 核实方式：4 路并行只读探索（按代码邻近性分组，文件+函数名重定位）+ 主会话对关键新结论抽查原文。**未做任何运行时测量、未改任何代码**；目标文件经 `git status` 核实均无未提交改动（工作区改动全在主题相关文件）。
> 本报告是给后续实施会话的「现状核实表 + 风险与前置条件」，不替代提示词2要求的实测基线。

## 1. 结论总览

| 瓶颈 | 判定 | 当前定位 | 与快照差异要点 |
|---|---|---|---|
| B1 全图解码后才缩放 | **成立** | `engine/image_rs.rs:45→64-79`；`engine/gpu/wic_engine.rs:61-63→109-120`；`engine/mod.rs:31-37` | 无实质差异；image-rs 新增 Limits/ICC 守卫（:36-44） |
| B2 WIC 失败二次解码 + COM 每次初始化 | **成立** | `thumbnail/generator.rs:314-330→404-453`；`wic_engine.rs:39-48` | 修正：JPEG 有合格 EXIF 内嵌图时可免二次全解（:415-434）；HEIC 回退仍是同一 WIC 引擎 |
| B3 dispatcher 逐项 N+1 查询 | **成立** | `ipc/thumbnail_commands.rs:417-430`；`ipc/thumbnail_full_gen.rs:321-323` | 批量 IN 参照实现已存在（:184-214、:269） |
| B4 每项取 database lifecycle 读锁 | **成立（范围更大）** | worker 6 处 + dispatcher/collector 2 处（§2.4） | 快照只列 2 处，实际 8 处取锁点 |
| B5 视频封面 5×30s seek；关键帧逐帧 seek | **成立** | `video/media_foundation.rs:131-143,411,165-177` | 常量无变化；最坏 150s 仅在 ReadSample 全部挂死时发生 |
| B6 LRU 全量 walkdir+排序 | **成立** | `thumbnail/cache.rs:208-297`；`tasks.rs:112-185` | 修正：四目录=thumbnails+ai_thumbs+face_thumbs+viewer_color；未超限路径**同样全量遍历** |
| B7 每张图双同步写盘 | **成立（归属修正）** | `generator.rs:66-82,421,529` | 快照把 create_dir_all 记到 exif_thumb.rs:421，实际在 generator.rs:421；exif_thumb.rs 纯内存无盘 IO |
| B8 结果收集单消费者串行 | **成立** | `thumbnail_full_gen.rs:460-527`；`thumbnail_commands.rs:611-622` | batch 收集器每条结果嵌套一次读锁（:613），串行点开销放大 |
| B9 派生让步 sleep 轮询 | **成立** | `derive/pipeline.rs:434-439,567-573` | 新增事实：派发前还有 R4 共享重活池 permit 限流（:580-585），不改变轮询本质 |
| B10 ffmpeg 回传 host 二次解码复核 | **成立** | `video/worker_backend.rs:212-224,250-277` | 重解码即信任边界（模块注释 §6），无校验和替代 |

## 2. 逐项核实

### B1 全分辨率解码后才缩放（最高优先）——成立

- **image-rs**：`engine/image_rs.rs:23` `ImageEngine::decode`；`:45` `image::DynamicImage::from_decoder(decoder)` 全图解出后，`:61-100` 才按 `ResizeHint` 缩放（LongEdge 分支 `:64-79` 用 `resize_exact` + CatmullRom）。`generator.rs:389-402` `decode_long_edge` 只是解码**后**缩放的目标长边，对 image-rs 不构成解码期降采样。24MP JPEG 全解到内存（~96MB 峰值）再缩小的论断成立。
- **WIC**：`wic_engine.rs:2-5` 文件头自认「全程 CPU 软件路径…实为 OS 原生 CPU 解码，真 GPU 引擎 nvjpeg/dxva 尚未实现」；`:61-63` `GetFrame(0)` 取全帧 → `:109-120` `CreateBitmapScaler`+`Initialize(&frame,…)` → `:147-149` `CopyPixels`。输出缓冲是缩放后尺寸（WIC scaler 管道），但解码处理仍是全帧。
- **引擎顺序**：`engine/mod.rs:31-37` `EngineArena::phase1` image-rs 在前、Windows 追加 WIC，`:52-58` 有测试 `arena_keeps_image_rs_first_for_phase1_formats` 锁定。
- **注意**：`:36-41` 已有 `image::Limits` max_alloc 守卫、`:42-44` 保留 ICC——B1 改造（如引入 turbojpeg/зune-jpeg 降采样）必须保持这两项语义。

### B2 WIC 失败二次完整解码 + 每次解码重复 COM 初始化——成立（一处细节修正）

- 失败转投链：`generator.rs:314-330`（strategy=="gpu" 分支 `try_gpu_decode` 失败 → `DecodeResult::DeferredToCpu`）→ `ipc/thumbnail_commands.rs:492-508` 转投 deferred 小池（`:529-531` 池宽 `max(1, budget/2)`，非 budget）→ `generator.rs:158-189` `process_deferred_cpu` → `:404-453` `try_cpu_decode` → `:441-446` 二次解码。
- **修正**：`try_cpu_decode` 在二次全解**之前**先走 EXIF 内嵌缩略图快路径（`:415-434`）——JPEG 且内嵌图达档位时免二次全解，非无条件二次全解。**HEIC/HEIF/AVIF 回退时 `arena.engine_for` 仍命中 WIC**（image-rs 不认这些格式），即对 HEIC 是同一 WIC 引擎二次解码，不是引擎切换。
- COM：`wic_engine.rs:39-48` 每次 `decode` 都 `CoInitializeEx`（`:41`，幂等 no-op）+ `CoCreateInstance` 新建 factory（`:43-48`）；全仓无 thread_local/static 复用。

### B3 dispatcher 逐项 N+1 查询——成立

- 视口批：`ipc/thumbnail_commands.rs:416-453` dispatcher 循环 `:417-430` 对每个 `needs_gen` id 逐个 `get_media_item` + `get_item_path_info`（两次单行查询）。批量 IN 只存在于缓存预取段（`:184-214`）与 exotic 路由段（`:269` `exotic_thumbnail_route_info_for_items` 单条 IN）。
- 全量生成：`ipc/thumbnail_full_gen.rs:294-341` `run_thumbnail_generation`，`:321-323` 同样逐 id；`:304` `all_ids.chunks(50)` 只是取消粒度。
- **改造参照**：`:269` 的 exotic 批量查询就是现成范式。

### B4 每个 worker 每项取 database lifecycle 读锁——成立，且范围比快照大

`with_database_lifecycle_read`（`state.rs:1600-1614`）= `scan_lifecycle_gate` RwLock 读 + 两次原子 epoch 比对，单次成本轻但全部在每项循环体内：

| 路径 | 取锁点 |
|---|---|
| batch decode / encode / deferred worker | `thumbnail_commands.rs:473-475` / `:579-588` / `:542-548` |
| batch dispatcher 逐 id `is_database_epoch_current` | `:418`（实现在 `state.rs:1617-1619`，也是读锁） |
| batch 结果收集器逐条 | `:613` |
| 全量 decode / encode / Phase2 deferred | `thumbnail_full_gen.rs:363-365` / `:421-429` / `:568-575`（par_iter 内每项） |
| 全量 dispatcher 逐 id | `:317` |

共 8 处。B4 改造若只按快照的 2 处做会漏掉 dispatcher/collector 侧。

### B5 视频封面黑帧规避 + 关键帧逐帧 seek——成立

- `video/media_foundation.rs:114` `cover`：`:131` `for attempt in 0..5`，`:135` 黑帧判定 `is_too_dark`（`frame_post.rs:74`，每 64 像素采样），`:142` 每次失败 `t_100ns += 5_000_000`（+0.5s），全暗则 `:148` 回退第 0 帧。
- `:411` `READ_SAMPLE_TIMEOUT = 30s`，经 `:696` `wait_sample_or_timeout` 生效，超时 `:699-703` 弃用 reader 隔离泄漏；`:684` 注释明示 seek（`SetCurrentPosition`, `:688`）同步返回、**故意不设护栏**，超时只护 `ReadSample`（`:692-696`）。
- `:153` `keyframes`：`:165-177` 10 帧逐帧独立 seek+读，无预取。
- 入口：`derive/video.rs:37/:44`（cover）、`:73/:77`（keyframes）。
- 量级说明：最坏 5×30s=150s 仅当每次 ReadSample 都挂满超时；正常坏文件路径是毫秒级 seek×5。

### B6 LRU 全量 walkdir+排序——成立（构成修正 + 一处加重）

- `thumbnail/cache.rs:208` `enforce_cache_limit`：扫描目录 `:221-226` 为**四个**——`thumbnails`/`ai_thumbs`/`face_thumbs`/`viewer_color`（sprites/motion_videos 有意排除 `:216-220`）；`:233-247` walkdir 逐文件 `metadata()` 累计 total_size 并收集 (path, mtime, size)；**`:249-258` 未超限早退发生在全量遍历之后**——常态路径同样付全目录遍历成本，只是免排序免删除；`:266` 按 mtime 全排序；`:273-274` 删至 `target = max×0.8`（`:210`）。
- 周期：`tasks.rs:113` 启动 5 分钟首跑、`:182` 每 24h、`:119` 在 `spawn_blocking` 内同步完成；上限默认 10GB（`db/schema` `thumb_cache_max_mb` default "10240"）。
- **快照未载**：驱逐已有事件驱动自愈——`tasks.rs:142-176` 驱逐后即时 `reset_thumbs_by_evicted_paths` + 广播 `db:media_enriched`，启动期 stat 扫描已降级为兜底（B6 优化勿重复建设）；`tasks.rs:106-107` 注释载明 T6 的「重建后即时触发 LRU」事件仍未接线。
- **陈旧注释**：`cache.rs:60-66` 仍称 LRU「只遍历 thumbnails/」，与四目录现状矛盾。

### B7 每张图双同步 IO——成立（归属修正）

- `generator.rs:66-82` `write_atomic`：`:78` `fs::write` + `:79` `fs::rename`，`:56-65` 注释明示这是防半截文件被 `exists()` 误判命中的**有意设计**（tmp+rename 不能去掉）。
- `ensure_thumb_dir`（`cache.rs:176-182`，`create_dir_all`）每张图一次：EXIF 快路径 `generator.rs:421`、全解路径 `:529`。**归属修正**：快照记的「exif_thumb.rs:421」不存在——`exif_thumb.rs` 全文 220 行纯内存编码（`:107-129` libwebp q1-99 有损/q100 无损 VP8L、`:132-140` JPEG），无任何盘 IO。
- **快照未载**：宽幅图 + `ai_hq_cache` 开启时 `encode_media_step_inner` 额外写一份 AI 缓存（`generator.rs:622-623`、`cache.rs:87-93`），每张图最多 2 文件 4 次写盘 syscall。

### B8 结果收集单消费者串行点——成立

- 全量：`thumbnail_full_gen.rs:460-527` 单消费者 `while let Ok(msg) = result_rx.recv()` 串行分拣，满 50 条落库（`:496-526`），残批 `:530-541`。
- batch：`thumbnail_commands.rs:611-622` 单消费者循环，每条结果在 `with_database_lifecycle_read` 内做 `on_result.send`+push（`:613`，读锁嵌套放大串行点），循环后统一 flush（`:624-628`）。

### B9 派生流水线 sleep 轮询——成立

- `derive/pipeline.rs:434-439` producer 遇扫描/缩略图运行 `sleep(500ms)` 轮询；`:567-573` consumer 硬暂停 `sleep(120ms)` + 交互期涓流（在途 ≥1 即让步，`InFlightGuard` `:542-547`）。无事件驱动唤醒。
- 结构确认：批 256（`:38`，可被 `derive_batch_size` 覆盖 `:326-328`）→ consumer `rayon::scope`（`:549`）→ writer 容量 256（`:727`）单事务 `batch_finish_derivations_with_snapshot`（`:742/:781`）。
- **快照未载**：派发前还需 `background_heavy_limiter.acquire`（`:580-585`，R4 与 exotic 共享 FIFO 重活池）——这是额度控制，不是唤醒机制，不改变轮询本质；B9 改事件驱动时须与它共存。

### B10 ffmpeg worker 桥 host 二次解码复核——成立

- `video/worker_backend.rs:212-213` `decode_cover_webp` 对 worker 回传 WebP `image::load_from_memory_with_format` **完整二次解码**；`:206-207` 容差 64px / 上限 16M 像素，`:224` 越界整批拒收；`:256-261`/`:277` 雪碧条声明几何不符拒收。模块注释 `:20-22` 明示「host 不信任 worker（§6）」——重解码本身就是信任边界，无校验和替代（`:134` DefaultHasher 只是 cache key）。
- `video/worker_service.rs:478` `driver_loop` 单线程阻塞串行（`:154` spawn、`:496-502` 交互优先出队、`:503` `cvar.wait` 事件唤醒队列到达、`:510/:582` run_job 阻塞到完成）；`:37` `MAX_ATTEMPTS=2`、`:34` 握手 5s、`:592-609` 背景任务被交互抢占时 kill 重派。

## 3. 快照未载的新发现（本轮新增可疑点）

| # | 发现 | 定位 | 影响 |
|---|---|---|---|
| N1 | 直显小文件（≤500KB、strategy=direct）为生成 thumbhash 做**无 ResizeHint 的全量解码**（`decode(abs_path, None)`，连缩放都没有） | `generator.rs:283-292` | 直显路径的隐藏解码成本；可改带 LongEdge hint 解码 |
| N2 | 生命周期读锁获取点全清单 8 处（见 §2.4 表），dispatcher/collector 侧也逐项取锁 | §2.4 | B4 改造范围比快照大，批级取锁/快照注入要覆盖全部点 |
| N3 | LRU 未超限的常态路径也要完成四目录全量遍历+逐文件 metadata（total_size 在遍历中累计后才早退） | `cache.rs:233-258` | B6 的「未超限免遍历」不成立，启动首跑 + 每 24h 各一次全遍历 |
| N4 | HEIC/HEIF/AVIF 的 DeferredToCpu 回退仍命中同一 WIC 引擎（image-rs 不认），是同引擎二次解码 | `generator.rs:406-409` | B2 优化对 HEIC 的收益形态与 JPEG 不同 |
| N5 | 驱逐自愈已是事件驱动（`reset_thumbs_by_evicted_paths` + `db:media_enriched`），启动 stat 扫描为兜底；T6「重建后即时触发 LRU」未接线 | `tasks.rs:142-176,106-107` | B6 优化的现有基础与边界 |
| N6 | 陈旧注释两处：`cache.rs:60-66`（称 LRU 只遍历 thumbnails/）；`generator.rs:436-440`「解码期即降采样」对 image-rs 名不副实（实为解码后缩放） | 如左 | 误导后续实施者，建议随手修 |
| N7 | B3 改造有现成参照：`exotic_thumbnail_route_info_for_items` 单条 IN + 缓存预取段批量 IN | `thumbnail_commands.rs:269,184-214` | 降低 B3 实施风险 |
| N8 | image-rs 已有 `image::Limits` max_alloc 守卫 + ICC 保留 | `image_rs.rs:36-44` | B1 换解码器时必须保持的语义 |
| N9 | deferred 池容量：batch 为 `max(1, budget/2)`（非 budget），全库 Phase2 为 rayon 局部池（非 crossbeam 池） | `thumbnail_commands.rs:529-531`；`thumbnail_full_gen.rs:553-557` | 快照「三池容量=budget」不完全准确，吞吐评估时按实际池宽算 |
| N10 | batch 结果收集每条嵌套一次 lifecycle 读锁 | `thumbnail_commands.rs:613` | 与 B4/B8 交叉，批量化时一并处理 |

## 4. 流水线地图现状核对（快照地图仍准确的部分）

三个触发入口、路由（`router.rs:47-51` 对 thumb_status 1/3 纯 DB 判 Existing 不 stat——注意生成管线内 CACHE_HIT 分支 `generator.rs:229` 仍有 `full.exists()`）、小文件直显（`generator.rs:255-303`）、CAS 写（`db/queries/thumbnail.rs:462-502`，status==2 带 `AND thumb_status NOT IN (1,3)` 不降级语义完好）、QoS（`qos.rs:12-29` budget 计算与 `:68-99` BELOW_NORMAL+EcoQoS）、crossbeam bounded(1024)、全库 50 条/批与 100ms 节流（`thumbnail_full_gen.rs:496-526,:279`）、`regenerate_missing_thumb`（`thumbnail_commands.rs:664,:713 锁外 stat、:730-736 CAS 复位、:754-759 三件套失效`）、serve 选档（`serve.rs:20-26/:67` 按 cell×DPR 取最小满足档）、前端链（`useRequestQueue.ts` 50ms/批 24/30s 看门狗/0.5-1-2s 退避、`useThumbLoader.ts` decode+自愈、`useThumbLoadGate.ts` 飞掠闸门、`src/constants/ipc.ts:98-112` 命令名）均与快照一致，无契约漂移。

## 5. 对后续实施会话的建议

1. **实测基线先行**（提示词2 原要求，本报告未做）：万级混合库测首屏缩略图延迟与全量吞吐；B1/B2 改造前后必须同条件复测。
2. **分批顺序照旧**：先 B3（参照 N7 现成范式）/B4（按 N2 全清单）/B7（保留 tmp+rename，只做目录预建与 IO 重叠）/B9；再 B1/B2（注意 N4、N8）；再视频与 LRU（B5 先实测坏文件占比、B6 注意 N3/N5）。
3. **不得破坏的语义**（核实确认现状如此，改造时保持）：CAS 不降级（`update_thumb_result_if_current`）、write_atomic tmp+rename、事件与 IPC 契约、QoS 与取消机制、先 CPU permit 后 GPU 令牌。
4. **低成本顺手项**：N6 两处陈旧注释可在任一相关批次中一并修正。

## 6. 本报告未覆盖

- 一切运行时测量（耗时、吞吐、内存峰值）——需测试库与运行环境。
- 未提交工作区中主题相关改动与本文目标文件无交集（已核实），但后续实施前应重新 `git status` 确认基线。
