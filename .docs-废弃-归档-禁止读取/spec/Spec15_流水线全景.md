---
id: 2026-08-07-Spec15_流水线全景
status: active
type: canon
line: asbuilt-spec
created: 2026-08-07
---

# Spec15 · 流水线全景(横切总览)

> 一句话:本篇是 spec 集的**横切总览**——不重复各子系统篇的细节,而是把全部「有输入 → 阶段 → 输出」的数据处理链并排成一张对照表,回答「整个 Scrollery 有哪些流水线、各自怎么触发、走什么链、用什么引擎、并发与现状如何」。是 [Spec00](./Spec00_产品与架构全景.md) §3.1「后端 26 模块一览」的**流水线视角**深化;各流水线的深机制、数据模型与不变量仍以对应子篇为权威,本篇只锚点指路。

## 1. 概览

- **§2 核心业务流水线(15 条)**:扫描 / 缩略图 / 派生 / 视频 / AI 语义 / AI 人脸 / OCR / 增强 / exotic / 图片编辑+ICC / 文档阅读 / 备份导出 / 布局渲染 / 音频播放 / 日志协议。§2.1 总表速查,§2.3 逐条**详细工作流**(触发/状态机/主链阶段/数据产物/前端消费链/并发取消错误)。
- **§3 跨流水线基础件(5 个)**:通用下载引擎、格式注册并集、存储抽象、解码引擎注册表、授权/安装校验。
- **§4 发布/CI 工程流水线(5 条)**:ci / sync-oss / oss-gate / drift-alarm / release。
- **§5 架构特征与已知遗留**:统一调度范式、推理恒子进程、发布阻断项。

> **as-built 纪律**:本篇描述代码现状(`src-tauri/src/` host + `crates/exotic-workers/` 子进程 worker + `.github/workflows/` + 前端消费侧),锚点可直跳;与计划正典冲突以代码为准并点明。行号锚点会随代码漂移,关键论断以锚点 + 通读对应文件双确认。

---

## 2. 核心业务流水线(15 条)

### 2.1 总表

| # | 流水线 | 定位 | 触发入口 | 处理链(输入 → 阶段 → 输出) | 引擎 / 技术 | 并发与性能特征 | 现状 |
|---|---|---|---|---|---|---|---|
| 1 | **目录扫描** | 媒体入库唯一入口:快扫先有行、富化补齐元数据 | `start_scan`(`ipc/scan_commands.rs:525`,用户「开始扫描」按钮,`quick` 增量开关) | 目录 → **fast_scan**(walker 流式遍历 → 头部尺寸/EXIF 方向 → 0×0 占位入库)→ **enricher**(EXIF/XMP/Motion Photo → 尺寸回填 → Live Photo 配对 → 视频/音频探测)→ **mark_missing**(四道闸差集标 missing) | walkdir + `image::image_dimensions` + kamadak-exif + rayon(eager 头读) | `spawn_blocking` 单线程遍历 + 每批 500 一事务 rayon 并行头读 + `Mutex<Connection>` 串行写;每批提交即 `bump_data_version` 喂前端逐批重排 | ✅ 完成。`fast_scan.rs:7` 注释过时(TIFF 实为 5s 非 50ms);增量 = 逐目录 mtime 快照剪枝。详见 [Spec02](./Spec02_扫描与画廊.md) |
| 2 | **缩略图生成** | 画廊/网格缩略图生产,视口批量 + 全库两条路径 | `batch_request_thumbnails`(视口)、`start_full/incremental_thumbnail_generation`(全库);懒自愈 `regenerate_missing_thumb` | 缓存命中查询 → `route_thumbnail`(现有>常见格式>冷门让路)→ decode(image-rs / WIC / EXIF 内嵌快路径)→ encode WebP → thumbhash → 回填 `thumb_status` → LRU(10GB)+孤儿 GC 治理 | image-rs + WIC(HEIC/HEIF/AVIF/ICO 兜接)+ fast_image_resize + thumbhash | 多阶段线程池(dispatcher/decode/deferred/encode 按核预算)+ QoS 前后台线程优先级;全库 Phase2 用 rayon | ✅ 完成。`THUMB_TIERS=[64,128,256,512,1024]` 唯一事实源(`generator.rs:27`);exif_thumb 大档位拒绝上采样。详见 [Spec03](./Spec03_缩略图与派生.md) |
| 3 | **派生数据**(derivations) | 封面类派生的总调度,kind 无关框架 | `start_derivation`/`pause`/`stop`(`ipc/derive_commands.rs:64/121/136`)+ 启动 3s 自动 kick + `db:media_enriched` 防抖 1.5s | **生产者**(status=0 批量领取)→ crossbeam 通道(512)→ **rayon 消费者池** `kind::run`(audio_cover/video_cover/doc_thumb/ai_thumb)→ **写入器**批量 flush → `update_thumb_result` 回填缩略图;backfill/孤儿恢复/毒任务防线 | kind 纯函数 + video MF/worker 桥 + lofty + foliate/zip | 3 独立 OS 线程 + `rayon::scope` 解码;`background_heavy_limiter` 公平 permit(与 exotic 共享);扫描/缩略图运行硬让步、交互涓流 | ✅ 完成。`audio_meta` 是桩(`is_implemented=false`);`video_playable` 不经流水线(播放路径直接 claim)。详见 [Spec03](./Spec03_缩略图与派生.md) |
| 4 | **视频关键帧/转码** | 视频封面、雪碧图关键帧、播放 remux/transcode 支撑 | 派生链 `backend_for`(MF 认的容器直解,mkv/webm/flv 走 worker 桥);播放 `resolve_impl` | probe → `playback_policy` 五态判定(DirectPlay/NeedsHevcExt/Remux/Transcode)→ MF 解码(XVP 内缩放 RGB32)/ ffmpeg 抽帧 → WebP 产物 → playable_cache(独立 20GB 池) | MediaFoundation(Windows)+ FFmpeg(video-worker 子进程)+ D3D11 硬解(HW_SLOTS=4) | MF **异步 ReadSample 回调 + 30s 超时** + COM 泄漏隔离(毒任务防线);worker 双优先级队列、背景任务被抢占 kill | ✅ 完成。D-003 旋转双钉(输出类型钉解码坐标 + MFT SetRotation 关 XVP 自动转正);MF 白名单排除 mkv/webm。详见 [Spec05](./Spec05_视频与音频.md) |
| 5 | **AI 语义搜索**(CLIP) | embedding 生成 + 常驻 f16 余弦搜索 | `start_ai_analysis`/`restart`/`pause`(`ipc/ai_commands.rs`);搜索 `semantic_search_cmd` | **Producer**(ai_status=0,批 512)→ **dispatch**(CPU permit → 缺 ai_cache 现场派生 → GPU 令牌 → `EmbedBatch`)→ **Writer**(满 512 或 3s 陈龄落库)→ `ai_search_results` TopN 回填 | ai-worker 子进程 ONNX(DirectML/CPU,**host 零 ort**)+ Chinese-CLIP(cn-clip-vit-b16 默认) | 3 线程 + `background_heavy_limiter` + GPU 分析令牌互斥(与 face/enhance 争同一槽);f16 向量常驻内存 + rayon 全量打分 | ✅ 完成。T16 去 host ort 收官;文本塔强制 CPU(BERT int64 DirectML 会算错)。详见 [Spec06](./Spec06_AI人脸OCR.md) |
| 6 | **AI 人脸** | 检测 → 嵌入 → 增量聚类 → 墙/审批 | `start_face_analysis`/`restart_face_analysis`(全量重置)/`pause`/`stop`(`ipc/face_commands.rs`) | **producer**(face_status=0,批 512)→ **dispatch**(三级定源:缩略图 / **face 缓存 640** / 原图 → `FaceDetectEmbed`)→ **writer**(条件写 + 增量最近质心聚类)→ 墙/审批 IPC | ai-worker ONNX(YuNet 检测 + SFace 嵌入,128d;SCRFD+ArcFace 非商用轨**物理不编译**) | 四阶段 3 线程 + rayon 预解码;RunTokenSlot generation compare-and-clear 纪律 | ✅ 完成。face 缓存短边 640(45× 提速为文档声明);增量贪心碎片化靠手动 recluster 修正。详见 [Spec06](./Spec06_AI人脸OCR.md) |
| 7 | **OCR**(PP-OCRv5) | 图片/视频帧文字提取,结果**不入库**直接回前端 | `ocr_extract_image`/`ocr_extract_frame`(右键 / 视频暂停);`download_ocr_models`(`ipc/ocr_commands.rs`) | `OcrSessionInit`(det/cls/rec 三模型 + dict 四件)→ `OcrBatch` → **det**(DB 文本检测+unclip)→ **cls**(方向判 180°)→ **rec**(CTC 解码)→ DTO 回传 `useOcr`/`OcrResultPanel` | ai-worker ONNX **恒 CPU EP**(SVTR transformer 系,防 DirectML 算错) | 交互单发、独立 `ocr_sess` 槽与 CLIP 双槽并存;`MAX_ATTEMPTS` 重试圈 | ✅ 完成。`swap_rb` 通道序**未定案**(待 golden 对拍);builtin 三面过滤防幽灵扩展名。详见 [Spec06](./Spec06_AI人脸OCR.md) |
| 8 | **图像增强**(enhance) | 降噪/超分/去 JPEG 伪影,对标 Topaz、付费插件 | `enhance_start`/`enhance_preview`/`enhance_cancel`/`get_enhance_queue`(`ipc/enhance_commands.rs:74-286`)+ enhanceStore | 准入门(直接 100MP / 4×超分 16MP)→ `EnhanceSessionInit` → `EnhanceRun`(worker 按 **512²+pad16 clamp-short** tiling)→ 容器级 EXIF 注入(不重编码)→ 同卷 rename → `ingest_single_file` 入库 | enhance-worker 子进程 ONNX(fp32/fp16 双件)+ `plan_tiles` 纯几何(`ai-core/enhance/tiling.rs`) | **内存态 job 队列**(P0 重启即丢,交互任务可重发);GPU 令牌与 CLIP/人脸互斥;`TILE_EMIT_THROTTLE` 500ms 事件节流 | 🔶 **部分完成**。模型清单 `PENDING_USER_REPO` 占位(`enhance/registry.rs:20`,fail-closed);队列无 FIFO(F-10);发布 F-01/F-02 阻断项。详见 [Spec06](./Spec06_AI人脸OCR.md) §增强 |
| 9 | **exotic 格式插件** | RAW/PSD 外部格式探测→解码→入库 + 插件商店分发 | 扫描播种 → **Coordinator** 幂等唤醒唯一 Pipeline(`exotic/coordinator.rs:6-12`)→ `run_exotic_pipeline_blocking` | probe(raw-probe / psd-probe)→ worker 解码(raw-worker LibRaw CDDL 嵌入预览 / psd-worker keyset 验签)→ sink 写缩略图缓存 → 回填 `exotic_tasks`(0/1/2/3/4 五态状态机) | 独立 worker 子进程 + exotic-protocol 帧 + `VerifyingKeyset` 验签 + keyring 授权链 | Coordinator 单调度器 + 有界事件通道(dirty 位防丢唤醒)+ 指数退避重试 + `background_heavy_limiter` | ✅ 完成。RAW 内测发行链闭合;psd 坏签名迁移后自愈;video-extended 恒零播种(D-444 单轨裁决)。详见 [Spec09](./Spec09_插件平台与exotic.md) |
| 10 | **图片编辑 + ICC** | 旋转/裁剪/拉直/调色(另存副本)+ 查看器色域映射 | `edit_*`(save_edited_image 链)+ `viewer_color` IPC(`editing/` + `viewer_color/`) | ingest 原图 → `apply_geometry`(裁剪 + 拉直 ±45°)→ `apply_adjust`(亮度/对比度/饱和度,**moxcms f32 sRGB 工作空间**)→ `write_edited_image`(.tmp→rename)→ `ingest_single_file` 注册新 item;查看器按 target 色域派生嵌入 ICC | moxcms + imageproc + fast_image_resize;`D-008` 内存预算准入门 | `spawn_blocking` 串行 + 内存预算门(100MP+45° 拉直解码前拒绝);E6 授权门(`edit_not_entitled` 稳定码) | ✅ 完成。D-412 像素域==嵌入 profile;缩略图 ICC→sRGB(A 线)与查看器 sRGB/P3/DCI-P3/自定义 ICC(B 线)。详见 [Spec04](./Spec04_图像与色彩管线.md) |
| 11 | **文档阅读** | PDF/EPUB/txt/md 解析→分章→渲染→阅读 + 远程校对 | `get_text_book_index`/`get_text_chapter`/`get_reader_book_prefs` 等(`ipc/doc_commands.rs`)+ 路由 `/doc/:id` | 编码 seam(chardetng+encoding_rs 修 GBK 三链路)→ 分章规则+两级分段+索引缓存 → **Book Model 单管线**(txt/md→SyntheticBook / epub→foliate-js vendored)→ 渲染(竖排 writing-mode 轴对换)→ 简繁转换/书签/进度 | foliate-js vendored + ferrous-opencc + 远程 LLM 校对(OpenAI 兼容 `/chat/completions`,key 存 keyring) | `spawn_blocking` 分片(24K 分片 + 256K 护栏,修 md 内存爆炸);loc1 定位器 | ✅ 完成。GBK/GB18030/Big5/SJIS/UTF-16 全绿;竖排竹简模式为品牌签名。详见 [Spec07](./Spec07_文档与阅读器.md) |
| 12 | **备份恢复 + 导出** | DB 一致快照备份/恢复 + 媒体导出整理 | `start_backup`/`restore_stage`/`restore_arm`(`ipc/backup_commands.rs:234/538/569`)+ `start_export`(`ipc/export_commands.rs:264`) | **备份**:preflight → `VACUUM INTO` 一致快照 → 文档一致性校验 → zip 流式打包 + 逐条 SHA-256 → `.tmp` 同卷 rename → 自动 retention;**恢复**:validate→stage→arm→restart→**swap** 五阶段;**导出**:staging 逐项 `.tmp`→rename → manifest 原子写 → 整目录 rename 落正式目录 | rusqlite VACUUM + zip + 纯函数引擎(时间戳/取消由调用方注入) | `file_job_owner` 门闩 + RunTokenSlot;`document_storage_guard` 写锁保证快照一致 | ✅ 完成。swap10 崩溃窗口不变量已锁;导出含命名档 original/sequence/date + 合规化 + 库内目标判定。详见 [Spec08](./Spec08_存储备份导出文件操作.md) |
| 13 | **布局/渲染** | 后端两端对齐布局 + O(1) 索引 + 前端虚拟滚动 | `compute_layout`/`get_bucket_rows`/`get_layout_rows_by_y`/`get_view_ids`/`get_item_y_by_id`/`get_subtree_scroll_target`(`ipc/layout_commands.rs:160/576/547`) | **取数**(items_cache:命中键 = `filter_key`+`data_version`+序形态,轴切换内存派生免 SQL)→ `compute_justified_layout`(行高/无缝分组/宫格 grid_pack)→ 物化 **O(1) id→flat 索引** → `hydrate_rows` 拼装载荷 → 前端 bucket 虚拟化(wishlist 单飞泵)+ 逻辑滚动条(<16.7M px 映射) | Rust 算法 + rayon scope 分块 + SQL 免 JOIN 规范序 | compute HIT 亚秒 / MISS 大库秒级(1M 项 canonical 取数 + 布局 + 物化);`get_adjacent_item`/`get_item_y_by_id` O(1) 无 DB 往返;双 RwLock(items 与 layout)不得同时持有 | ✅ 完成。sort_datetime DESC 基准 + filename 轴派生统一缓存(根治换轴 thrash)。详见 [Spec02](./Spec02_扫描与画廊.md) §布局 |
| 14 | **音频 + 播放** | 音频元数据/封面派生 + 播放支撑 | enricher 回填 `audio_meta`;derive `audio_cover`;`get_audio_detail`(`ipc/audio_commands.rs:25`) | lofty **单次解析**同取标签+封面 → 封面走 `encode_media_step` 写缩略图缓存 + thumbhash;播放器 `write_cover_to_cache` 全分辨率无损写 `<cache>/audio_covers/`;`get_audio_detail` 即时解析(未补全库也能用) | lofty(纯 Rust)+ 缩略图编码器复用 | 纯函数层无副作用;三处复用(enricher/derive/IPC);`lyrics_source` 只记来源 | ✅ 完成。`derive/audio.rs:60` run_meta 恒桩(audio_meta 归 enricher 管)。详见 [Spec05](./Spec05_视频与音频.md) |
| 15 | **日志 + worker 协议** | 跨进程日志与 worker 调度基础设施 | tracing 全链自动 + `SpanTimer` 手动埋点 + worker stderr 行协议转发 | 日志信封(ts/level/target/session_id/operation_id + error.chain 数组)→ JSONL 文件 + dev 控制台 + UI 环形缓冲三扇出;worker `WorkerLogLine{lvl,msg,fields}` → supervisor 行缓冲扫描 → 主 JSONL | tracing 三 Layer + exotic-protocol 帧 + `span_close` 事件(`scrollery::span`) | 稳态 ~1ns/调用点 callsite 缓存;64KiB 环形缓冲 + 崩溃摘尾;WARN/ERROR 重复压缩 | ✅ 完成。span TopN 前端聚合;worker 日志经 IPC 汇入主 JSONL。详见 [Spec12](./Spec12_配置状态日志.md) |

### 2.2 调度范式注

除扫描/导出外,上述流水线绝大多数共享同一骨架:**生产者 → 有界 crossbeam 通道 → 消费者池 → 写入器**,配 `CancellationToken` + 状态机(0/1/2/3)+ `RunTokenSlot` generation 纪律(AI/face/derive/thumb 四槽已统一),孤儿恢复 + `background_heavy_limiter` 公平 permit 池共享。详见 [Spec12](./Spec12_配置状态日志.md) §调度。

## 2.3 详细工作流(逐流水线)

> 本节把 §2.1 总表的 15 条流水线逐条展开为**端到端工作流**:触发入口(IPC/UI/事件)、状态机全态与迁移、主链各阶段(输入 → 关键函数 → 输出)、数据与产物、前端消费链、并发/取消/错误路径。深机制、数据模型与不变量仍以各子篇为权威,本节只锚点指路 + 写「工作流视角」;共享调度骨架原理见 §2.2 与 [Spec12](./Spec12_配置状态日志.md)。锚点经 30% 抽查交叉核实(schema 拆分后 `db/schema.rs` 已迁 `db/schema/{early,mid,late}.rs`,锚点一律指向现行文件)。

### 2.3.1 目录扫描

**一句话**:媒体入库唯一入口——`walkdir` 流式遍历 → 快扫先有行(首屏 eager 头读、其余类型占位/0×0)→ 后台 enrichment 补齐 EXIF/尺寸/配对/探测 → 四道闸缺失检测差集标 missing;增量 = 逐目录 FS mtime 快照剪枝(T17a/b)。已建成(`fast_scan.rs:7` 头注释「TIFF 50ms」过时,实测为 5s 硬超时,见 `metadata.rs:15`)。

**触发入口**
- `start_scan`(`ipc/scan_commands.rs:526`)。参数含 `quick` 增量开关(`scan_commands.rs:534`,默认 `false`=全量逐文件,`553`);当前前端未传 quick(`scanStore.ts:259` 无此参数),剪枝为后端可选项、UI 未接线。
- 调用方链:侧栏库管理 `toggleScan`(`ManagementSection.vue:141`)、relink 后兜底重扫(`useFolderRootActions.ts:138`)、添加文件夹后自动重扫(`useFolderRootActions.ts:212`)、目录复制入库(`historyStore.ts:127`)、编辑另存后重扫(`EditOverlay.vue:394`)。
- 前置建根:`add_scan_root`(`scan_commands.rs:22`)建根+卷绑定+顶级目录,随后前端发 start_scan;`relink_scan_root` 只改根路径、抽样闸放行后由**前端**补发 start_scan 收尾(仅前端造得出进度 Channel,`scan_commands.rs:307-308`)。
- 中止:`stop_scan`(`scan_commands.rs:695`)→ `state.cancel_scan`。

**状态机** — 两套并置:
- 后端 `scan_roots.scan_status`:`scanning`(快扫每批 `update_scan_root_status` 写,`db/queries/scan/roots.rs:199`)→ `idle`(`finish_scan_root`,`roots.rs:215`);未扫根为初始态。
- 前端 `progressMap`(`scanStore.ts:33`):`discovering`(启动置,`193`)→ `scanning`(Channel progress,`213`)→ `enriching`(completed 移交,`232`)→ 停转。`enrichmentDone` 终态账本(`scanStore.ts:114`)+ 10 分钟怠速看门狗(`116,125`)防「Channel completed 晚于 enrichment:completed 到达而复活运行态」。

**主链阶段**

阶段 0 · 启动编排(`start_scan`):先 `cancel_scan` 取消该根旧 token 再 `new_scan_token`(`scan_commands.rs:546-547`);读根路径(`556`);快照 exotic Catalog(`568`,walker 分类/播种统一口径)。快扫与 enrichment 均走 `spawn_blocking`——**不经 Spec12 共享骨架**(生产→crossbeam→消费者池→写入器),是骨架的例外(见「并发」节,骨架原理指路 [Spec12](./Spec12_配置状态日志.md) §调度)。

阶段 1 · fast_scan(`run_fast_scan`,`scanner/fast_scan.rs:275`):
1. 入口守门(第 1 闸):`PathProber::is_online` 判卷在线(`298`),离线 → 不动 DB、发 `Completed{total:0}` 即返回(`298-306`)。
2. 读本根 `volume_id`(`311`,缺失检测守门1「在线卷集」;None → 空集 → 不标)。
3. `quick` 时一次性加载全根 `rel_path→mtime` 快照(`343`;J1:基线只读启动时快照、不查活行,防 `ensure_dir_chain` 遍历期覆写污染剪枝判定),并初始化连接级 `_mm_seen` TEMP 表(阶段4)。
4. 流式遍历:`MediaWalker`(walkdir 单线程,隐藏目录剪枝+遍历/metadata 错误累积,`walker.rs:95,132`)攒满 `BATCH_SIZE=500`(`fast_scan.rs:45,369-376`)即处理一批,内存峰值 O(batch)(T12 根治百万级 ~600MB 峰值)。
5. 批内提尺寸:前 `EAGER_DIM_COUNT=500`(跨批累计,`50`)项 `par_iter` 并行真读文件头(`390`;非 TIFF 走 `read_header_buf_for_ext`+`read_image_dimensions_buf` 一次 open 解尺寸与 JPEG 方向,TIFF `run_with_timeout` 5s 超时 `metadata.rs:15`),其余 `cheap_phase2_dimensions` 给类型占位(视频 1280×720/音频 400×400/PDF 595×842,`68-89`);仅 eager 预算耗尽后的新图像以 0×0 占位、待 enrichment 回填。
6. 一事务入库(`416-528`):`ensure_dir_chain` 递归 upsert 目录链并记目录 mtime(`100`);`upsert_fast_scan_item`(`db/queries/scan/media_upsert.rs:202`,内部 prepare_cached)按 mtime 判 Unchanged / size 变→SourceChanged(全失效派生,`media_upsert.rs:121`) / mtime 变 size 同→SuspectChanged **零写**攒批(`250-252`);新项 `INSERT` 带卷 id(`259-278`);命中的 id(含 Unchanged)经 `insert_seen_id` 流式写 `_mm_seen`(`484`,否则 mark_missing 误判删);Catalog 命中的 exotic 格式按能力播种任务(`502-524`;`seed_gate_admits` 挡 builtin 合成标记与 video-extended,`655`)。
7. 提交即回调 `on_batch_committed` = `AppState::bump_data_version`(`528-530`,`state.rs:387`)→ 前端逐批重排。
8. 事务外指纹定案 SuspectChanged(`535-567`):写锁释放后读文件算 `content_fingerprint`,`resolve_suspect_change`(`media_upsert.rs:168`)三环定案 touch(抖动,派生全保留)/保守失效——hash IO 绝不进写事务。
9. 进度:Channel `Progress`(流式不预扫总数→`total=0`,`573`)+ `update_scan_root_status`(`583`)。
10. 收尾门闩:`walker.finish()` 得 `WalkOutcome.complete`(遍历/metadata 错误或取消→false,`walker.rs:110`)。
11. 缺失检测 `finalize_missing_detection_preloaded`(`318`)四道闸:完整门闩 → TOCTOU 卷复查(写删前再 `is_online`,`616`)→ 在线卷集 → `mark_missing_preloaded` 三重守门(本根子树∩在线卷∩¬seen,`mark_missing.rs:66`,seen 已流式在连接级 TEMP 表)。任一不过 → 0、**绝不写 is_deleted**。
12. 写回 `set_directory_media_count`(T17a 剪枝基线,`fast_scan.rs:624`,`directories.rs:284`)+ `finish_scan_root`(`628`)+ Channel `Completed` 带 `marked_missing` 计数(`635`,供前端 toast)。

阶段 2 · 扫描收尾(`scan_commands.rs`):bump 兜底(`603`)、`tree_snapshots.invalidate_root`(`609`,文件树枚举快照失效,与 data_version 两套失效)、`wake_exotic(ScanCommitted)`(`613`)幂等唤醒 exotic 流水线(让步机制保证其不抢占扫描期)。

阶段 3 · enrichment(fire-and-forget,`run_enrichment`,`scanner/enricher.rs:104`):
1. 图片段:按画廊视图序批选未富化项(`122`:单根、`is_deleted=0`、`media_type='image'`、`image_meta.item_id IS NULL`;默认 date+datetime 走 `(sort_datetime,id)` keyset)→ 单次 open 按格式阶梯读头(`read_header_buf_for_ext`,`metadata.rs`,JPEG 128KB/TIFF·HEIC 系 256KB/其它 64KB,EXIF/XMP/尺寸三消费者复用,根治每文件 3 次 open)→ 并行解析(`enricher.rs:188-256`)→ 一事务回填 `image_meta`(`284`,prepare_cached)+ 0×0 尺寸回填(`274`)+ `sort_datetime` 修正(EXIF 时间按 UTC 墙钟,`290`)+ Live/Motion 旗标(`310`)。
2. EXIF 解析失败/panic 写 minimal 行(`300-305`)防同项下轮重选——补全前进性的关键。
3. 批提交后 `bump_layout_data_version` + `db:media_enriched` 事件(`317,323`)。
4. Live Photo 配对:`pair_live_photos`(`live_photo.rs:52`):(directory_id, file_stem) 聚合;HEIC/HEIF/JPG+.mov 纯 stem 配对(T7);JPG+.mp4 仅静图带 motion 信号才配(T15 守门,`117`),防普通照片+同名 mp4 误吞。
5. 视频段 `enrich_videos`(`enricher.rs:375`):按 `m.id` keyset 批选(`idx_media_type_id`),`backend_for`(MF/worker 桥)探测真实宽高(rotation 交换)+时长+codec+fps+bitrate → `video_meta`(`427-450`);失败写 minimal 防重探。
6. 音频段 `enrich_audios`(`484`):同样按 `m.id` keyset 批选;lofty 标签/时长 + lrc 来源 → `audio_meta`(`533`);失败同样写行。
7. 终态 `enrichment:completed`(`364`);取消/异常时由 start_scan 侧补发终态(`scan_commands.rs:654-672`)。

**数据与产物**
- `media_items`:扫描主表。fast_scan 写 file_name/file_size/file_mtime/file_format/media_type/width/height/sort_datetime/cache_key/volume_id(`media_upsert.rs:259`);`availability` online/missing/offline 三态(缺失检测/卷监听各切其一,互不越权);`content_hash` 为 SuspectChanged 指纹基线。占位尺寸先于元数据落地,enrichment 回填。
- `directories`:`upsert_directory`(`directories.rs:32`)写 rel_path/name/depth/mtime/tree_sort_key;mtime + media_count 构成增量剪枝基线(`directories.rs:73,284`)。
- `scan_roots`:scan_status/scan_progress/total_files/last_scan_at(`roots.rs:199,215`)。
- `image_meta`(EXIF 单行)/`video_meta`/`audio_meta`:enrichment 写,行存在即不再被批选选中。
- `exotic_tasks`:扫描事务内播种(`exotic.rs:66`,INSERT OR IGNORE 幂等)。
- 无文件系统产物(缩略图/派生归 [Spec03](./Spec03_缩略图与派生.md));0×0 占位由布局按正方形渲染、enrichment 秒级回填。

**前端消费链**
- 组件 → `useScanStore().startScan`(`scanStore.ts:188`)→ `invokeIpc(IPC.START_SCAN)`(`259`,`src/utils/ipc.ts:86`;`IPC.START_SCAN='start_scan'`,`src/constants/ipc.ts:17`)。
- Channel 进度:`onmessage`(`scanStore.ts:209`)按 `msg.type` 窄化 progress/completed/error;progress 驱动 `media.loadStats()` 1s 节流重排(`220-225`);completed 移交 enriching + `markedMissing>0` toast(`241`)。注:`ScanErrorPayload` DTO 已定义(`payload.rs:28`)但 fast_scan 从不发 Error 变体——扫描失败走 `start_scan` invoke 抛错(scanStore catch,`266-272`)或 enrichment errorCode。
- 事件回显:全局监听 `db:media_enriched`(`scanStore.ts:156`;`EVENTS.MEDIA_ENRICHED`=`'db:media_enriched'`,`ipc.ts:413`)推进 enriching 进度;`enrichment:completed`(`scanStore.ts:169`;`ipc.ts:414`)停转 + `errorCode` 弹 warning(`178`)+ 终刷统计(`183`)。监听走应用级 `effectScope(true)` 惰性单次注册(`150-186`),防止被组件卸载连带拆掉。
- 消费方:状态栏 `AppStatusBar.vue:179`、侧栏库管理进度条(`ManagementSection.vue:120-133`)、统计页。

**并发 / 取消 / 错误**
- 并发模型:快扫 = `spawn_blocking` 单线程遍历(I/O 密集)+ 批内 rayon 并行头读 + `Mutex<Connection>` 串行写(`fast_scan.rs:416`);enrichment = `spawn_blocking` + 保留一核的独立 rayon 池(`reserved_core_pool`,`enricher.rs:37`)——**与派生/AI 的「交互即让步」不同**:enrichment 是用户等待中的导入、其进度事件自动触发重排,整体让步会饿死它,故只留一核保 UI 跟手。两者都不经 Spec12 共享骨架,扫描是骨架的例外(Spec15 §2.2)。
- scan token:按根存 `AppState.scan_tokens`(`state.rs:37`),`new_scan_token`/`cancel_scan`(`state.rs:879,889`);enrichment 结束后显式移除 token(`scan_commands.rs:679-683`)——否则 AI `should_yield_to_higher_priority()` 永久让步。
- 取消:同一 `CancellationToken` 贯穿快扫与 enrichment。快扫取消 → `Err(AppError::Cancelled)` 丢弃当前批、不进差集(`fast_scan.rs:379`);enrichment 取消视为正常终态、仍发 ok(`scan_commands.rs:642`)。
- 错误/panic:enrichment 双兜底——`panic_guard`(畸形文件 panic 降级为失败写 minimal,`enricher.rs:197`)+ `catch_unwind`(`scan_commands.rs:625`);walk 遍历错误 → 不完整不删(见不变量)。
- 关键不变量:①不完整扫描≠删除(`walker.rs:110`,错误非空或被取消即拦差集);②卷离线≠删除(入口守门+TOCTOU 双查);③卷未识别 → 空在线集不标缺失(宁可不删);④quick 剪枝基线只读快照、勿改回查活行(J1 红线);⑤missing 恢复仅切 availability、绝不碰 is_deleted/offline(`media_upsert.rs:232-244`);⑥SuspectChanged 指纹 IO 不进写事务;⑦builtin 合成标记(video-extended/ocr)不播种 exotic 任务(D-444 单轨裁决)。

---

### 2.3.2 缩略图生成

**一句话**:画廊/网格显示缩略图的生产链,「视口批量」与「全库」两入口共用同一**多阶段线程池引擎**(解码/编码/deferred CPU 三池 + crossbeam 通道,2026-07-10 真机 A/B 裁决唯一实现);冷门 exotic 格式入池前先经 `route_thumbnail` 让路给插件 Worker;产物按 `cache_key` 落 `thumbnails/{档位}/{xx}/{hex}.webp` + ~28B thumbhash 占位,LRU(默认 10GB)+ 孤儿对账 GC 治理。

#### 触发入口

| 入口 | 命令/函数 | 来源/场景 |
|---|---|---|
| 视口批量 | `batch_request_thumbnails`(`ipc/thumbnail_commands.rs:70`) | 画廊滚动:前端 `useRequestQueue` 收集可见区 id 批量请求(`composables/useRequestQueue.ts:147`),`target_size` 按网格行高向上取最小档位 ≥ 行高(`composables/useRequestQueue.ts:18-23` 的 `getOptimalThumbTier`) |
| 全库全量 | `start_full_thumbnail_generation`(`ipc/thumbnail_full_gen.rs:74`) | 侧栏工具按钮(`components/sidebar/sections/ToolsSection.vue:51`);先整表复位 `media_type='image'` 的 thumb_status 再重建 |
| 全库增量 | `start_incremental_thumbnail_generation`(`thumbnail_full_gen.rs:84`) | 侧栏工具按钮(`ToolsSection.vue:45`);只补 `thumb_status=0`(缺/被复位),不复位全表 |
| 懒自愈 | `regenerate_missing_thumb`(`thumbnail_commands.rs:507`) | 前端 `MediaThumb` 对 `thumb_status=1` 的封面 `img.onerror`=404 时按格触发(`useThumbLoader.ts:72 requestHealOnce`)——LRU 驱逐删文件不改状态,`route_thumbnail` 对 status=1 短路 → 永久 404,须复位待重生成 |
| 启动自愈 | `reconcile_cover_thumbs`(`db/boot.rs:143`) + 7 天一跑 stat 兜底 `reconcile_missing_cover_thumbs`(`boot.rs:192`) | 修复调度器竞写残留(P1-4)与手动删文件/异常退出的驱逐残留;LRU 事件驱动即时复位已取代 stat 兜底为主路径 |
| 缓存治理 | `tasks.rs:112` 周期任务(启动 5min 首跑 + 每 24h) | `reconcile_orphan_gc` 孤儿 GC → `enforce_cache_limit` LRU → `reset_thumbs_by_evicted_paths` 事件驱动复位(`tasks.rs:134/136/145`) |
| 手动清理 | `clear_all_thumbnails`(`thumbnail_commands.rs:584`) | 设置面板「清除所有缩略图」(`DynamicSettingControl.vue:472`) |

#### 状态机

- **`media_items.thumb_status`(主状态,四态)**:`0`=待生成 → `1`=已生成(WebP 落盘,`thumb_path` 为 DB 相对路径)/ `3`=小文件直显(`thumb_path` 即源文件绝对路径)/ `2`=跳过(不支持类型或生成失败终态,无产物)。迁移:0→1/3 由本流水线;0→2 由本流水线失败臂或 `media_type` 非 image 分派;1→0 由懒自愈/LRU 驱逐复位/全量复位;2→0 仅由 `clear_all_thumbnails`/全量复位。
- **全库生成运行态**:`idle/running/completed/cancelled`,载体为 `FullThumbProgressPayload.status`(`thumbnail_full_gen.rs:25-33`)+ 快照;持有性走共享 `RunTokenSlot`(见 [Spec12](./Spec12_配置状态日志.md)),终态发布代次守卫见下「取消」。视口批量路径**无独立运行态**(逐 invoke 火拼,取消走每 id 标记)。

#### 主链阶段

两条路径共用核心引擎,差异点标注。全部 IPC 经 `spawn_blocking` 执行(DB 读写不跨 `.await` 持锁)。

**视口批量路径**(`batch_request_thumbnails`):
1. **缓存命中查询**:`read_blocking` 内一条批量 SQL 取 `thumb_status/thumb_path/thumbhash/file_format/cache_key`(`thumbnail_commands.rs:97-143`);status∈{1,2,3} 入 `fast_results` 快路径,余者入 `needs_gen`。
2. **快速结果回送 + items 缓存同步**:命中项经 `on_result` Channel 逐条回送;`state.apply_thumb_results`(`thumbnail_commands.rs:146-161`)同步常驻 `items_cache`,防 `fetchRowsByY` 返回陈旧 status=0。
3. **冷门格式让路**(`route_thumbnail` 纯判定,`thumbnail/router.rs:47`):批内确有 catalog 已认领格式才走(`thumbnail_commands.rs:169-174`);`exotic_thumbnail_route_info_for_items`(`db/queries/thumbnail.rs:371`)批量取任务态 + `thumbnail_fingerprint` 比对 done 任务指纹(`thumbnail_commands.rs:221-229`)。分派:`Common`→kept 进主池;`Exotic`→gated,绝不调主 generator、绝不写 status=2;done 但指纹失效→`invalidate_exotic_tasks_for_item`(`db/queries/exotic.rs:99`)退回 pending;`Existing`→gated(一致性兜底)。让路项回送 status=0(平衡前端在途计数),合并 `wake_exotic(ConfigChanged)`(`thumbnail_commands.rs:275-292`)。
4. **dispatcher 装载**:逐 id 消费 `cancelled_thumb_ids`(单次消费,防旧取消标记误伤同 id 新请求,`thumbnail_commands.rs:326`);`get_media_item`+`get_item_path_info`+`resolve_media_path` 拼绝对路径推入 decode 通道;无法加载→回送 status=2(每 id 恰一结果不变量,`thumbnail_commands.rs:344-352`)。
5. **三池处理**:decode 池(`decode_media_step`,`generator.rs:186`)→ `DecodeResult::{Ready, ToEncode, DeferredToCpu, Err}`;CPU 密集回退转投**独立 deferred 池**(`process_deferred_cpu`,`generator.rs:154`,避免占住 decode 线程,`thumbnail_commands.rs:316-317/405-407`);encode 池(`encode_media_step`,`generator.rs:440`)。
6. **收集落库 + 回送**:结果收集器逐条 `on_result.send` + 批尾 `flush_thumb_results`(`thumbnail_commands.rs:49`,单事务逐条 `update_thumb_result`)+ `apply_thumb_results`(`thumbnail_commands.rs:458-470`)。

**全库路径差异**(`run_thumbnail_generation`,`thumbnail_full_gen.rs:91`):
- 全量先复位 `media_type='image'` 全表 + `reset_all_exotic_thumbnail_tasks`(`thumbnail_full_gen.rs:103-109`);增量跳过。
- 计数 `count_pending_thumb_items`(`thumbnail.rs:64`),`total==0` 直接发 completed 终态返回(`thumbnail_full_gen.rs:120-133`)。
- dispatcher 取 `get_all_pending_thumb_ids`(`thumbnail.rs:53`),该查询内嵌三谓词:**排除 pdf/svg**(前端 `DocThumbRenderer` 独占回填)与 **video/audio**(封面归派生流水线),防主 generator 误领标 status=2 灰卡。
- 收集器每 50 条 flush 一次 + 进度按 `PROGRESS_THROTTLE=100ms` 节流(`thumbnail_full_gen.rs:332-346`,大库 8 万+ 条避免 IPC 风暴);deferred 走 **Phase2 rayon 局部池**(`budget` 线程 + `apply_current_thread_qos` 逐线程钉 QoS,`thumbnail_full_gen.rs:390-462`);终态经 `finish(generation)` compare-and-clear(`thumbnail_full_gen.rs:480-500`)+ 清 layout_cache(`:504-510`)。

**解码/编码内部链**(`generator.rs:197-516`):缓存命中只查 `exists()`(不查内容,原子落盘的成因)→ 小文件直显(`strategy==direct` 或 ≤`skip_max_bytes`,web-safe 格式,≤500KB 仍算 thumbhash,`generator.rs:248-294`)→ `media_type='image'` 才解码,非 image→status=2(`:297-345`)→ `strategy=="gpu"` 走 WIC `try_gpu_decode`,失败降级 `DeferredToCpu`(`:348-371`);CPU 路径先试 EXIF 内嵌快路径(`try_exif_thumb`,`exif_thumb.rs:10`,大档位 ≥512 严格拒不足档位上采样,`embedded_thumb_acceptable`:`exif_thumb.rs:81`),失败全解码 `image` crate(`try_cpu_decode`,`generator.rs:392`)。encode:一次解码两份产物(`maybe_write_ai_cache`,`generator.rs:556`,宽幅图同一缓冲顺产 AI 分析缓存)→ `resize_to_rgba`(fast_image_resize 双线性)→ ICC→sRGB 投影(`generator.rs:481`)→ `generate_thumbhash`(`thumbhash.rs:15`)→ WebP 编码(libwebp,q<100 有损 / q=100 无损 VP8L,失败回退 JPEG,`encode_as_webp`:`exif_thumb.rs:107`)→ `write_atomic` tmp→rename(`generator.rs:66`)。

#### 数据与产物

- **DB**:`media_items.thumb_status/thumb_path/thumbhash/cache_key`(`db/schema/early.rs:84-88`);`idx_media_thumb` 部分索引只覆盖 `thumb_status != 1`(`db/schema/early.rs:113`),使「找待生成项」扫描量与完成量无关。回填时机:视口=批尾、全库=每 50 条,统一走 `update_thumb_result`(`thumbnail.rs:408`)。
- **缓存布局**:`thumbnails/{size}/{xx}/{hex}.webp`(`thumb_path`,`cache.rs:16`;DB 相对路径 `thumb_db_path`,`cache.rs:38`);5 档 `THUMB_TIERS=[64,128,256,512,1024]` 全后端唯一事实源(`generator.rs:27`),必须与前端 `THUMB_SIZE_TIERS`(`constants/defaults.ts:3`)一致;`ai_thumbs/{xx}/{hex}.webp` 短边≥336(`AI_CACHE_SHORT_EDGE`,`cache.rs:58`)一次解码两份产物。文件名=16 位小写 hex(`cache_key_to_hex`),前 2 字符分桶。
- **LRU**:`enforce_cache_limit`(`cache.rs:208`)默认预算 `DEFAULT_THUMB_CACHE_MAX_MB=10GB`(`cache.rs:307`),只遍历 thumbnails/ai_thumbs/face_thumbs/viewer_color;按 mtime 升序删(2026-07-19 裁决:排序键维持 mtime 生成序,不做访问追踪)。
- **孤儿 GC**:`reconcile_orphan_gc`(`cache.rs:550`)删 DB 无主产物;护栏=仅 16 位小写 hex stem + `mtime < 进程启动 epoch`(防会话内新产物未落行被误删)。
- **产物格式**:WebP(q<100 有损/100 无损 VP8L,`exif_thumb.rs:107`)+ thumbhash ~28B BLOB(`thumbhash.rs:15`),直显小图无 WebP 产物、`thumb_path` 为源绝对路径。

#### 前端消费链

- 画廊卡片:`MediaThumb.vue:347-357` / `MediaThumbCompact.vue:89-99`(canvas 路径 `canvasThumbState.ts:171` 同语义)→ `useThumbLoader`(status 1/3/0 解码 + 懒自愈,`loadThumb`:`useThumbLoader.ts:79`,`requestHealOnce`:`:72`)→ `emit('request-thumb'|'cancel-thumb'|'regenerate-thumb', id)` → 父级 MediaGrid → `useRequestQueue.request`(`useRequestQueue.ts:163`)→ Tauri `Channel<ThumbResult>` + `invokeIpc(BATCH_REQUEST_THUMBNAILS)`(`:127/147`)。
- URL 拼装:`buildThumbUrl`(`useThumbLoader.ts:37`)—status1 → `${cacheDir}/thumbnails/{path}`,status3 → 原文件绝对路径;`cacheDir` 来自 `get_thumb_cache_dir`(`ipc.ts:366`)。
- 全库进度:`scanStore.ensureThumbGenListener`(`scanStore.ts:350`)监听 `EVENTS.THUMB_GEN_PROGRESS`(`ipc.ts:422`)→ `applyThumbGenPayload`(完成/取消 → `invalidateLayout`,`scanStore.ts:341`);webview 重载经 `restoreThumbGenProgress` 查 `FULL_THUMB_GEN_STATUS`(`scanStore.ts:359`)回填(Channel 随 webview 死、事件对重建 webview 照送)。工具按钮 `ToolsSection.vue:45/51`,清理 `DynamicSettingControl.vue:472`。
- 无独立 IPC 契约层外的新命令:批内逐条结果走 `ThumbResult`(id/status/path/hash)经 Channel 回显,不做前端合并——`useRequestQueue` 按 batch slot 收尾(`:127-141`)保证「每 id 恰一次 resolve」。

#### 并发 / 取消 / 错误

- **并发模型**:独立 `std::thread::spawn` 三池(非 rayon),通道 `bounded(1024)`,池宽统一由 `thumb_cpu_budget`(`qos.rs:24`)= 逻辑核 − 保留(≤8 留 2、>8 留 4,`thumb_cpu_budget_for`:`qos.rs:12`);decode/encode 各 `budget`、deferred `max(1, budget/2)`(`thumbnail_commands.rs:364-435`)。worker 每项 `refresh_worker_qos`(`qos.rs:42`,前后台档位变化才 syscall):Windows `SetThreadPriority(BELOW_NORMAL)` + EcoQoS(`qos.rs:68`),macOS `pthread_set_qos_class_self_np`;全库 Phase2 用 rayon 局部池 + `start_handler` 钉 QoS。
- **取消**:视口路径每 id 标记 `cancel_thumbnail_request`(`thumbnail_commands.rs:487`,IPC 已随 P14 删除(前端本就按「在途保留单飞」设计不发起取消);dispatcher 单次消费仍为防御点);全库路径 `stop_full_thumbnail_generation`(`thumbnail_full_gen.rs:519`)→ `RunTokenSlot::cancel`(`state.rs:916-917`),dispatcher/池/收集器循环均检 `CancellationToken`,终态发布代次守卫防「停止→立即重启」旧轮盖新轮(`thumbnail_full_gen.rs:480-500`)。孤儿恢复(崩溃遗留 status 在途)走共享骨架(见 [Spec12](./Spec12_配置状态日志.md))。
- **错误**:`panic_guard` catch_unwind(`generator.rs:46`)使损坏/畸形文件的第三方解码 panic 仅废单项、不杀进程;decode/encode 失败→status=2 结果;GPU 失败→DeferredToCpu 兜底;「每 id 恰一结果」不变量(`thumbnail_commands.rs:329-330`)防止前端在途计数失衡;前端另有 30s 停滞看门狗释放卡死批(`useRequestQueue.ts:16,116-125`)。
- **关键不变量**:①产物原子落盘(tmp→rename,命中只查 `exists()`,直写崩溃会永久裂图);②exotic 让路项绝不进主 generator、绝不写 status=2;③LRU 驱逐事件驱动即时复位(`tasks.rs:142-177`)+ 启动 7 天 stat 兜底(`boot.rs:180-204`)+ 懒 404 自愈三道防线防「驱逐后永久 404」;④`THUMB_TIERS` 前端一致(改档位 = 全库缓存作废,发版前才可动);⑤`route_thumbnail`/`generator` 均为纯函数不持 `AppState`,数据层批量预取避免 N+1(`router.rs:1-11`)。

---

### 2.3.3 派生数据(derivations)

**一句话**:封面类重产物(视频封面/关键帧雪碧图、epub 封面、音频内嵌封面、AI 分析缓存)的 kind 无关后台总调度——生产者按状态领取 → crossbeam 通道 → rayon 消费者池跑各 kind 纯函数 → 写入器批量落库并把封面镜像回 `media_items`,配 backfill 显式入队、孤儿恢复、毒任务防线与分级让步;调度骨架复用 [Spec12](./Spec12_配置状态日志.md) §调度,本节只写本流水线特化与工作流。

**触发入口**(命令层 `ipc/derive_commands.rs`,全部命令见下):
- `start_derivation`(`derive_commands.rs:64`),三个来源共用此命令:
  1. **自动 kick**:`useDerivationAutoStart`(`src/composables/useDerivationAutoStart.ts:37`)——启动 3s 后延迟 kick 补全既有库封面(`useDerivationAutoStart.ts:70-73`,挂载点 `src/App.vue:272`);监听 `db:media_enriched` 防抖 1.5s 再 kick 覆盖新增(`useDerivationAutoStart.ts:57-65`)。kick 前先查 `derivation_status` 的 `isRunning`,在跑则跳过,避免 cancel+restart 抖动(线内注释 `useDerivationAutoStart.ts:41-44`)。
  2. **手动视频提取**:视频控制卡「增量/全量提取」(`src/components/sidebar/sections/ToolsSection.vue:99/105` → `derivationStore.startVideoIncremental/startVideoFull`,`src/stores/derivationStore.ts:90-94`)与设置页视频区(`src/views/SettingsView.vue:306/309`),带 `kinds:["video_cover",(勾选关键帧则 "video_keyframes")]`(`derivationStore.ts:52-55`)。
  3. **配置变更重启**:`configStore.restartDerivation`(`src/stores/configStore.ts:266`,设置 toggle 后无过滤重启)。
- 原 `pause_derivation`(`derive_commands.rs:121`,cancel 但保留续传标志)已随 P14 删除:暂停语义下线,派生只剩 `start_derivation`/`stop_derivation`(前端本就无 `PAUSE_DERIVATION` 调用点)。
- `stop_derivation`(`derive_commands.rs:136`):清续传标志、放弃自动续传意图;`derivationStore.stopVideoExtraction`(`derivationStore.ts:114`)。
- `derivation_status`(`derive_commands.rs:152`):前端 1s 轮询(`derivationStore.ts:57-81`,无推送事件)。

**状态机**:
- **流水线级**:代次守卫 `RunTokenSlot`(`state.rs:152`,`begin` 换新 token+generation;`new_derivation_token:703`/`cancel_derivation:708`/`finish_derivation:714`)+ 持久化配置 `derivation_active`(`derive_commands.rs:27/127/142`,经 `set_config`,`db/queries/config.rs:17`)驱动跨重启续传。`derivation_status` 的 `active` 字段即此标志、`is_running` 即 token 存在(`db/models/ai_face.rs:99-108`)。
- **任务级**(`media_derivations.status`,与 ai_status 同构):0 待处理 → 1 处理中(`mark_derivations_processing`,`db/queries/derivations.rs:113`)→ 2 完成 / 3 错误(`batch_finish_derivations`,derivations.rs:185);孤儿(崩溃遗留 1)复位退 0 并 `orphan_count+1`,`orphan_count>=2` 第 3 次直接转 3 毒任务(`reset_processing_derivations`,derivations.rs:214-237);优雅 stop 的在途行退 0 **不计数**(`requeue_in_flight_derivations`,derivations.rs:248)。任务级状态机权威在 [Spec03](./Spec03_缩略图与派生.md) §2.1/§5.2。

**主链阶段**(`run_pipeline_blocking`,`derive/pipeline.rs:152`):
1. **启动序**:孤儿恢复(`pipeline.rs:175`)→ 计算 `disabled_kinds`(`enable_video_cover`/`enable_video_keyframes` 默认开、`ai_hq_cache_enabled` 默认关 opt-in,`pipeline.rs:190-211`;显式 `kind_filter` 覆盖之 D-002,`pipeline.rs:214-216`)→ 按 `DerivationKind::ALL` 优先级序 backfill(封面/元数据先于关键帧雪碧图,`kind.rs:38`;`backfill_derivations` INSERT OR IGNORE,derivations.rs:320;`is_implemented=false` 的桩 kind 跳过,`pipeline.rs:233-235`)→ 无 pending 提前返回(`pipeline.rs:291-294`)。
2. **生产者**(`produce_tasks`,`pipeline.rs:417`):`get_pending_derivations` 批查 `status=0`(derivations.rs:33,LIMIT=`derive_batch_size` 默认 256,`pipeline.rs:38/325`)→ `mark_derivations_processing` 置 1(防重启重复领取)→ 逐个 push 通道(`CHANNEL_CAPACITY=512`,`pipeline.rs:41/332`);扫描/缩略图运行中让步 sleep 500ms(`pipeline.rs:433-439`);未知 kind 字符串保持 1 交给认识它的后续构建(`pipeline.rs:490-496`)。
3. **消费者池**(`consume_tasks`,`pipeline.rs:517`):`rayon::scope` 并行解码(`pipeline.rs:534`);派发前等**共享后台重活池** permit(与 exotic 共享同预算,`state.rs:165`,`pipeline.rs:568`);`panic_guard` 包裹 `kind::run`(单项 panic → status=3 不中止流水线,`pipeline.rs:605-608`;分发器 `kind.rs:178`)→ 产出 `DerivationResultRow(2/3, payload, error, thumbhash, page_count)` push 结果通道。
4. **写入器**(`write_results`,`pipeline.rs:674`):攒满 batch 或通道关闭 flush(`pipeline.rs:686-799`),同一写锁内 `batch_finish_derivations`(归零 orphan_count,derivations.rs:196)+ 封面类 kind 的 `update_thumb_result` 镜像回 `media_items`(`pipeline.rs:741`;`db/queries/thumbnail.rs:408`,status=2 写被守卫不覆盖已有产物行,防滞后失败冲掉封面)→ epub `page_count` upsert `document_meta`(`pipeline.rs:759`)→ `apply_thumb_results` 同步常驻布局缓存(`state.rs:472`)→ 有封面落地则 emit `db:media_enriched`(`pipeline.rs:811-819`)驱动画廊刷新——该事件也是 auto-kick 的输入,但 kick 有 `isRunning` 守卫且空跑不发事件,故无死循环。
5. **收尾**(`pipeline.rs:385-411`):`thread::scope` join 三 worker 后,若 token 已取消且仍持槽 → 优雅退回在途行;`finish_derivation` 代次守卫(`pipeline.rs:139`)仅自然完成且仍为当前轮才清 `derivation_active=0`(`pipeline.rs:144-147`)。整个 run 挂 `SpanTimer("pipeline:derive")` + 300s 看门狗 warn(`pipeline.rs:89-116`)。

**数据与产物**:
- **表**:`media_derivations(item_id, kind, status, payload_path, error, orphan_count, updated_at)`,主键 `(item_id,kind)`,需显式入队(区别于 ai_status 列驱动);`media_items.thumb_status/thumb_path/thumbhash` 封面镜像;`document_meta.page_count`(epub spine 近似页数)。全表定义权威 [Spec01](./Spec01_数据层.md)/[Spec03](./Spec03_缩略图与派生.md) §2.1。
- **产物目录**(按 `cache_key` 16 位 hex 前 2 字符分桶,`thumbnail/cache.rs`):封面复用显示缩略图路径 `thumbnails/{tier}/{xx}/{hex}.webp`(`thumb_path`,`cache.rs:16`);关键帧雪碧图 `sprites/{xx}/{hex}.webp`(`keyframe_sprite_path:126`/`keyframe_sprite_db_path:139`,非封面、绑源媒体生命周期、不参与 LRU);AI 分析缓存 `ai_thumbs/{xx}/{hex}.webp`(`ai_cache_path:67`,短边≥336)。
- **各 kind 产物**:视频封面 = `backend_for` 解码 → `snap_to_tier` 长边直通 → `encode_media_step`(`derive/video.rs:37-64`,MF 机制权威 [Spec05](./Spec05_视频与音频.md));关键帧雪碧图 = N 帧水平拼接 + `write_atomic`(`video.rs:67-117`,质量恒 `DEFAULT_WEBP_QUALITY` 与显示设置解耦);epub 封面 = zip 容器三级回退找封面 + 顺带数 spine(`derive/doc.rs:76-138`,封面发现 `find_cover_href:173`);音频封面 = lofty 内嵌封面提取(`derive/audio.rs:18-54`);AI 缓存 = WIC 优先/CPU 回退短边 336、已存在幂等跳过(`derive/image.rs:34-71`)。**所有产物原子落盘** tmp→rename(`write_atomic`,`generator.rs:66`),命中判定只查 exists()。

**前端消费链**:视频控制卡 / 设置页 → `derivationStore`(`derivationStore.ts:29`)→ `invokeIpc`(`src/utils/ipc.ts`)→ 命令名 `src/constants/ipc.ts:113-116`;进度无推送事件,运行中 1s 轮询 `derivation_status`(kinds 限定使分母只含视频两 kind,`derivationStore.ts:57-81`);自动链走 `App.vue:272` 挂载的 `useDerivationAutoStart`,事件 `EVENTS.MEDIA_ENRICHED`(`ipc.ts:413`)。封面落地经 `thumb_status=1` 镜像后 `MediaThumb` 零改动显示;pdf/svg 文档缩略图**不走本流水线**——前端 `DocThumbRenderer` 离屏渲染 + `store_doc_thumbnail` 回传(独立链,权威 [Spec03](./Spec03_缩略图与派生.md) §3.4)。`pause` 无前端消费方。

**并发 / 取消 / 错误**:
- **并发模型**:走共享骨架(生产者→crossbeam→消费者池→写入器,[Spec12](./Spec12_配置状态日志.md) §调度),本流水线特化为三角色跑**独立 OS 线程**(`std::thread::scope`,`pipeline.rs:355`,而非 rayon worker——长阻塞不占解码池),消费者内部 `rayon::scope` 独享全 rayon 池;派发节流 = 扫描/缩略图硬暂停 + 用户交互涓流(≤1 在途,`pipeline.rs:548-561`);permit 与 exotic 公平共享(FIFO,`pipeline.rs:565-570`)。
- **取消路径**:start/pause/stop 均 `cancel_derivation`(`state.rs:708`)触发 token 取消,三个 worker 各自 `is_cancelled()` 退出;优雅 stop 走 requeue 不计孤儿数(裁决 J10);代次守卫防「停止→立即重启」旧轮打断新轮(`pipeline.rs:398-411`)。
- **错误处理**:`panic_guard` 单项失败废项不废流水线;毒任务防线 `orphan_count>=2` 转 error 文案不含内部路径(derivations.rs:219-221);显式清缓存 `reset_derivations_by_kinds` 把 status 2/3 退 0 **并清 orphan_count**(显式意图覆盖自动防线,derivations.rs:267-281)。
- **关键不变量**:派生产物一律原子落盘;kind_filter 覆盖 `enable_*` 后台开关;`get_pending_derivations` 显式排除 pdf/svg(前端驱动)与 `video_playable`(按需交互 kind,播放路径直接 `upsert_and_claim_derivation` 领取,derivations.rs:60-61/138),故这两类永不进入本流水线;隐藏根待处理行留在 status=0 暂停、unhide 续跑(derivations.rs:62)。`AudioMeta` 是桩(`is_implemented=false`,`kind.rs:101`;`run_meta` 恒 not_implemented,`audio.rs:60`)——标签/歌词由 enricher 回填,不经本流水线。

---

### 2.3.4 视频关键帧/转码

**一句话**:视频媒体两段产物链同源同判定骨架——①封面/雪碧图关键帧:派生链 `backend_for`(`video/mod.rs:115`)路由 MF 直解或 video-worker 桥,解码→WebP 落缩略图/雪碧图缓存;②播放可播产物:`resolve_impl`(`video/playback_orchestrator.rs:215`)→ probe → 判定五态 → remux/半转码/全转码经 `VideoWorkerService` 按需产出,落独立 20GB 可播池(`playable_cache.rs:15`)。前者背景批量、后者交互按需抢占,共享「probe→判定」骨架与 FFmpeg 组件门槛。深机制以 [Spec05](./Spec05_视频与音频.md) 为权威,本节只写工作流视图。

**触发入口**:

| 入口 | path:line | 触发来源/场景 |
|---|---|---|
| 封面/雪碧图批量 | `start_derivation`(`ipc/derive_commands.rs:64`) | 用户「派生」按钮 / 启动自动续传;kind=`video_cover`/`video_keyframes`(`derive/kind.rs:16-18`),经 backfill 入队(`derive/pipeline.rs:224` 段) |
| 封面/雪碧图开关 | `enable_video_cover`/`enable_video_keyframes`(`derive/pipeline.rs:194-199`) | 配置键关闭→不入队/不领取,非破坏性暂停续传 |
| 播放解析 | `resolve_video_playback`(`ipc/video_commands.rs:154`) | ContentViewer 挂载/切项(`ContentViewer.vue:639` `useVideoSource` watch) |
| 确认护栏/本地转码 | `confirm_video_playback`(`ipc/video_commands.rs:175`) | 单文件超池 50% 二次确认 / needsHevcExt「改用本地转码」(D-444④,`apply_force_transcode`:`playback_orchestrator.rs:342`) |
| 取消 | `cancel_video_playback`(`ipc/video_commands.rs:229`) | 准备 overlay 取消按钮 |
| 组件装取 | `download_video_component`(`ipc/video_commands.rs:270`;只读的 `video_component_status`(`:254`)已随 P14 删除) | needsComponent 引导下载 FFmpeg(tools.rs 下载引擎,len+sha256 校验) |
| 池统计/进度快照 | `video_cache_stats`(`ipc/video_commands.rs:205`)、`video_playback_progress_snapshot`(`ipc/video_commands.rs:37`) | 设置页缓存池实时占用 / webview 刷新后恢复进度条 |

**状态机**:
- `video_cover`/`video_keyframes` 行:status 0/1/2/3 + 毒任务防线,走共享骨架(见 [Spec12](./Spec12_配置状态日志.md) §调度),本流水线无独立差异。
- `video_playable` 行:0(pending)/1(在途)/2(ready)/3(error);`upsert_and_claim_derivation`(`playback_orchestrator.rs:450`)原子 0/3→1 防并发重复派活,claim 失败方直接回 preparing 不重复 spawn。
- 后端判定 `PlaybackVerdict` 五态(`playback_policy.rs:20-35`):`DirectPlay`/`NeedsHevcExt`/`Remux`/`RemuxAudioTranscode{audio_track_index}`/`Transcode`。**总表纠错**:总表「五态(DirectPlay/NeedsHevcExt/Remux/Transcode)」只列四名,漏 `RemuxAudioTranscode`(半转码,视频 copy 仅转音轨),实为五变体。
- 前端消费六态(`composables/player/useVideoSource.ts:22`):direct/derived/preparing/needsComponent/needsHevcExt/needsConfirm,补 idle/cancelled/error(`useVideoSource.ts:48-58`)。needsHevcExt 前端实测修正:可硬解→direct 播原文件,不可解→引导/本地转码(`useVideoSource.ts:61-81`)。

**主链阶段**:
播放链(交互,resolve→产出→回执):
1. 取 item 详情 + 拼源指纹 `mtime:size`(`playback_orchestrator.rs:225-241`)。
2. 缓存命中检查提到 probe 之前:status=2 且产物文件真在 → 直接 derived + `touch_played`(`playback_orchestrator.rs:255-272`;文件真在判据自愈「行 2 但产物被删」)。
3. probe:交互优先级 `svc.probe`(`worker_service.rs:262`)→ video-worker ffprobe(`video-worker/main.rs:248` VideoProbe 分支)→ 回填 `video_meta`(`write_back_probe`:`playback_orchestrator.rs:185`,rotation/fps/bitrate 探测 None 时护既有值 `merge_probe_meta_fields`:162,D-003 旋转不清零)。
4. 判定:`probe_to_policy`(`playback_orchestrator.rs:141`,container_ext 用**文件扩展名**非 format_name)→ `decide` 纯函数判定表(`playback_policy.rs:121`);`force_transcode` 仅对 NeedsHevcExt 覆写为 Transcode(`playback_orchestrator.rs:342`)。
5. 分流:DirectPlay→direct 裸交原文件;NeedsHevcExt→needs_hevc_ext;其余→`resolve_derived`(`playback_orchestrator.rs:383`):在途(1)→preparing → 单文件护栏 `estimate_output_bytes`/`exceeds_half_pool`(`playable_cache.rs:38/45`,预估>池 50% 且未确认→needs_confirm)→ `upsert_and_claim_derivation` → `spawn_playable_job`(:482)并立即回 preparing。
6. 产出(worker 侧):driver 阻塞线程取任务(交互优先,`worker_service.rs:478`)→ `ensure_session`(`worker_service/runner.rs:42`;含 `VideoSessionInit` 校验 ffmpeg sha256 + GPL 保险丝 + work_dir 白名单,`video-worker/session.rs:45/151`)→ remux `-c copy`+faststart(`video-worker/remux.rs:66`,音轨可 copy 或转 aac)/ transcode 编码器阶梯 `ENCODER_LADDER=[nvenc,qsv,amf,mf]` CRF=23(`playback_orchestrator.rs:47-49`,`video-worker/transcode.rs:143`)→ ffmpeg `-progress` 位置推进,Progress 帧节流 ≤2s(`video-worker/main.rs:418`)。
7. 验收三重:①worker 回执 out_duration_ms/out_bytes>0;②rename 前产物复探 `probe_verify`(`worker_service.rs:306`,独立去重键 ProbeVerify,不与源 probe 互挂)与源时长 ±5% 比对(`playback_orchestrator.rs:581-633`);③源指纹复核 `source_fingerprint_matches`(`playback_orchestrator.rs:746`)——派生耗时窗口内源被替换/编辑则拒 rename。
8. 落盘:rename 前登记 `PREPARING_OUTPUTS` in_use(`playback_orchestrator.rs:59-79`)→ `*.tmp` 同卷 rename 终名 `{cache_key}.mp4`(`playable_cache.rs:32/27`)→ `batch_finish_derivations` status=2 + 相对路径 → `enforce_pool_limit` LRU 驱逐(跳过在播,`playable_cache.rs:121`)→ 广播 ready(`playback_orchestrator.rs:661-712`)。

封面/雪碧图链(背景,派生流水线内):`kind::run`(`derive/kind.rs:180`)→ `run_cover`/`run_keyframes`(`derive/video.rs:37/67`)→ `backend_for` 路由:
- MF 直解(MF 白名单 `MF_VIDEO_EXTS`,`media_foundation.rs:47`,排除 mkv/webm/flv/ogv):`open_session`(:271,硬解槽优先)→ 单会话内选封面时间戳 min(1s, 时长10%)+黑帧规避(`cover`:112)/[5%,95%] 均匀抽帧(`keyframes`:151)→ **异步 ReadSample 回调 + 30s 超时**(`READ_SAMPLE_TIMEOUT`:`media_foundation.rs:409`)→ XVP 内缩放 RGB32(`media_foundation.rs:9-12`)+ 负 stride 翻行(`frame_post.rs:104`)→ CPU `apply_rotation` 转正(`frame_post.rs:11`;D-003 双钉另半是 `pin_no_xvp_rotation` 关 XVP 自动转正,`media_foundation.rs:339`)→ `encode_media_step` WebP 落缩略图缓存 + thumbhash(`run_cover`,`derive/video.rs:44-58`)。
- worker 桥(video/mod.rs:129-153 三重门控:授权+ffmpeg 就绪+AppState 绑定):`WorkerVideoBackend::cover/keyframes`(`video/worker_backend.rs:85/99`)→ Background 优先级 `frames` op(`worker_service.rs:399`)→ worker 侧 ffmpeg `-ss` 抽帧+黑帧规避(`video-worker/frames.rs:235/290`)→ 无损 WebP blob → host 独立解码复核(长边容差 64px/总像素上限 16MP/切格几何严格对拍,`worker_backend.rs:212/250`)→ 雪碧图原子写盘(`derive/video.rs:106-110`)。

**数据与产物**:
- DB:`media_derivations` 行 kind=`video_cover`/`video_keyframes`/`video_playable`,status 0/1/2/3 + payload(相对路径);`video_meta`(codec/fps/bitrate/rotation/has_audio,`upsert_video_meta`:`playback_orchestrator.rs:197`)。`video_playable` **不进背景 backfill**(`derive/kind.rs:92`,全库 GB 级转码=磁盘爆炸),按需经 IPC 直写 status/payload。
- 产物:封面 WebP `thumbnails/`(镜像 `media_items` thumb_status/thumb_path,MediaThumb 零改动,`derive/video.rs:34-36`)、雪碧图 `sprites/`(`thumbnail/cache.rs:425-427`)、可播池 `{cache_dir}/video/{cache_key}.mp4`(`playable_cache.rs:27`)。`.tmp` 中间态不参与占用统计/驱逐(`playable_cache.rs:96-101`)。
- 池预算:默认 20GB(`DEFAULT_VIDEO_CACHE_MAX_MB`:`playable_cache.rs:15`),设置键 `video_cache_max_mb`(`config/schema.rs:481`,禁 0 回退默认);LRU 按**最近播放 mtime** 驱逐,resolve 命中即 `touch_played`(`playable_cache.rs:52`)。
- 参数键:`video_keyframe_count`/`sprite_cell_height`(`derive/pipeline.rs:299-317`),前端经 `get_startup_config` 同源读取。
- 写入时机:封面经 encode_media_step 立即落盘;可播产物 rename 前启动清扫 `.tmp` 残留(`lib.rs:284` `sweep_tmp`,`playable_cache.rs:61`)、崩溃假在途 boot 复位(`db/boot.rs:159`),rename 后 finish 落 status=2,LRU 驱逐紧随同 spawn_blocking 块。

**前端消费链**:
- 封面/雪碧图:无独立 IPC 链——封面回填 `media_items` 缩略图字段直读缓存路径;雪碧图由悬停/scrub 预览组件读 `sprites/`(同读 `video_keyframe_count`)。
- 播放:`ContentViewer.vue:639` 挂 `useVideoSource`(getter 形式,随查看项切换 watch 复位)→ `invokeIpc`(`utils/ipc.ts:86`)调 `RESOLVE_VIDEO_PLAYBACK`/`CONFIRM_VIDEO_PLAYBACK`/`CANCEL_VIDEO_PLAYBACK`/`VIDEO_COMPONENT_STATUS`/`DOWNLOAD_VIDEO_COMPONENT`(`constants/ipc.ts:119-132`)→ `applyResolution`(`useVideoSource.ts:131`)驱动 direct/derived 喂 `<VideoPlayer>`、引导态挂 `<VideoPreparingOverlay>`;preparing 态订阅 `EVENTS.VIDEO_PLAYBACK_PROGRESS` 事件(`constants/ipc.ts:425`,`useVideoSource.ts:200`),挂载时先取一次 `VIDEO_PLAYBACK_PROGRESS_SNAPSHOT` 防刷新丢进度,ready 事件给 src / error 给稳定 code / cancelled 转「可重新准备」态。

**并发 / 取消 / 错误**:
- 并发:封面/雪碧图走共享骨架(生产者→crossbeam→rayon 消费者池→写入器、RunTokenSlot、background_heavy_limiter,见 [Spec12](./Spec12_配置状态日志.md) §调度),本流水线差异仅在 `kind::run` 内经 `backend_for` 分派 MF/worker 桥。播放链走 `VideoWorkerService` 专用 driver 线程 + 双优先队列:交互(probe/remux/transcode)恒抢占背景(frames),背景在途遇交互入队即置 preempted→kill worker→重回队头,`ensure_session` 下轮重建(`worker_service.rs:534-561`);(item,kind) 在途/在队共挂一 job 多 waiter fan-out(`enqueue_keyed`:201)。MF 硬解槽 HW_SLOTS=4(`d3d.rs:29`),`try_acquire` 不阻塞、拿不到回退软解(正确性不依赖 GPU)。
- 取消:`cancel_video_playback` 先硬取消 worker job(在队出队 fan-out Cancelled / 在途置 cancelled 标志待轮询,`worker_service.rs:428`),再 `reset_in_flight_derivation_for_item` **只退 status=1 行**(不动已缓存 status=2,`video_commands.rs:244`),广播 cancelled;`spawn_playable_job` 对 `Cancelled` 抑制 finish/error(只清 tmp,`should_suppress_finish_on_cancel`:`playback_orchestrator.rs:738`)。开机复位假在途(`db/boot.rs:159`)。
- 超时:probe 30s / frames 120s / remux、transcode 静默限时 60s(收 Progress 重置)、总上界 remux `max(1h,时长×6)`、transcode `max(2h,时长×6)` 上限 12h(`worker_service/types.rs:16-57`);MF 读帧 30s 超时后 `Drop for Session` 泄漏隔离僵死 reader(COM 泄漏隔离,`media_foundation.rs:240-266`,硬解槽随 reader 永久占用有 warn 可观测)。
- 错误:稳定 code `video_*` 前缀(`worker_service/types.rs:260`)→ `AppError::Exotic` 透 IPC;ffmpeg stderr / worker 内部串只进 tracing 不进载荷;仅 `WorkerFailed`(通路瞬时失败)respawn 重试一次(`types.rs:278`),needs_component/unauthorized/malformed/timeout 等终态不重试。
- 关键不变量:D-003 旋转双钉(输出类型钉解码坐标 + `pin_no_xvp_rotation`,`media_foundation.rs:339`,`frame_post.rs:11`);产物时长 ±5% 验收 + rename 前源指纹复核;在播文件(Windows 占用删失败)驱逐跳过不中断整轮;worker 产物经 host 独立解码复核(host 不信任 worker)。

---

### 2.3.5 AI 语义搜索(CLIP)

**一句话**:CLIP 图文嵌入的**后台索引流水线**与**交互语义搜索**双半部——前者按 `media_items.ai_status` 扫描待处理项、经 ai-worker 子进程批量嵌入后按模型落 `ai_embeddings`;后者把文本塔产出的查询向量对常驻 f16 全库余弦打分,TopN 写 `ai_search_results` 临时表回喂画廊。

**触发入口**
- 后台索引(embedding 生成):`start_ai_analysis`(`ipc/ai_commands.rs:229`,「开始/继续」,侧栏 AI 工具 `ToolsSection.vue:482` 经 `aiStore.startAnalysis` 发 IPC.START_AI_ANALYSIS);启动时自动续传 `maybeAutoResume`(`useAnalysisController.ts:191`,`App.vue:351` 调,判 `analysisActive && pending>0 && !isAnalyzing`);隐藏根 unhide 补跑(`scanStore.ts:86`)。
- `restart_ai_analysis`(`ipc/ai_commands.rs:274`,清空当前模型向量后全量重跑);`pause_ai_analysis`(:321,保留续传标志)/`stop_ai_analysis`(:344,清除标志)。
- 辅助:`retry_failed_ai_items`(:444,Error→Pending 复位再走 start)/`rebuild_embeddings`(:459)。
- 语义搜索:`semantic_search_cmd`(`ipc/ai_commands.rs:177`)——工具栏语义/混合搜索框防抖提交(语义框 `AppToolbar.vue:478-482` → `searchStore.commitSemantic`;混合框 `:474-477` → `triggerMixedSearch`(`:451-464`)→ `commitMixed` → `aiStore.runSemanticSearch`)。**纠错(相对总表)**:搜索与索引链正交,不经 Producer/dispatch/Writer;`semantic_search_cmd` 直达 worker EncodeText → 内存打分 → `ai_search_results`,「TopN 回填」属搜索而非 Writer。
- 引擎/模型控制(不触发嵌入):`set_active_model`(:643,切模型后按新模型向量重同步 `ai_status`)、`reload_ai_engine`(:421)、`download_model`(:720)、`detect_ai_provider`(:60)。

**状态机**
- 项级 `media_items.ai_status`(`db/models/ai_face.rs:10-19`;schema `early.rs:233-235`):0=待处理/1=处理中/2=已完成/3=错误。
  - 0→1:Producer 领取(`pipeline.rs:198`,`batch_update_ai_status`);崩溃/暂停遗留的 1 由孤儿恢复归 0(`recover_orphaned_ai_items`:`pipeline.rs:115` → `reset_processing_ai_items`:`db/queries/ai.rs:253`,启动期执行)。
  - 1→2:Writer 条件落库(`batch_finish_ai_items`:`db/queries/ai.rs:42`),仅当行内 `cache_key` 仍等于领取时快照(X1,防迟到写)。
  - 1→3:推理 terminal 失败/现场派生失败(`pipeline.rs:422`);瞬态失败保持 1,下轮恢复。
  - 模型切换:`sync_ai_status_for_model`(`db/queries/ai.rs:315`)按新模型向量覆盖重排(有向量→2、缺→0);`reset_ai_embeddings` 分批删向量+归 0(R2-6 批间释放写锁)。
- 流水线级:配置键 `ai_analysis_active`("1"=期望运行;命令置位,自然完成清除 `pipeline.rs:87-95`)+ `RunTokenSlot` generation(`state.rs:507-521`)。无独立空闲态:终态由 token 区分自然完成(清标志+释放 GPU 槽)与取消(保留标志)。走共享骨架(见 [Spec12](./Spec12_配置状态日志.md) §调度),此处只列差异。

**主链阶段**(后台索引,6 步)
1. 启动编排:命令置 `ai_analysis_active=1`(`launch_ai_pipeline`:`ai_commands.rs:213`)→ `new_ai_analysis_token` 换代 → `start_ai_pipeline`(`pipeline.rs:61`)。F5 互斥:先 `try_acquire_gpu_analysis(GPU_OWNER_AI)`(`state.rs:543`)抢与人脸共享的唯一 GPU 分析槽,被占快速失败返回可操作文案。
2. Producer(`produce_tasks`:`pipeline.rs:125`):查 `get_pending_ai_items(512)`(`db/queries/ai.rs:153`,谓词 `ai_status=0 AND media_type='image' AND is_deleted=0` + exotic 接管/隐藏根排除)→ 整批标 Processing → 逐个发 crossbeam 通道(`AiTask{item_id, source_path, file_format, cache_key}`:`pipeline.rs:39-46`)。让步:每轮先查 `ai_yield_blockers()`(`state.rs:657`,scan/thumbnail/derivation/exotic/interaction),非空 sleep 500ms 续让(`pipeline.rs:141-164`),解除后记 info。
3. Dispatch(`dispatch_loop`/`dispatch_batch`:`worker_pipeline.rs:119/169`):攒批(50ms 空闲刷新)→ **先 CPU permit**(`background_heavy_limiter.acquire`:`worker_pipeline.rs:182`)→ 缺 ai_cache 项现场派生(rayon `par_iter` 调 `generate_ai_cache`,解原图短边 336→WebP 原子写:`derive/image.rs:54`;panic_guard 防畸形文件炸批)→ **后 GPU 令牌**(`gpu_token.acquire`,D2 先 CPU 后 GPU 天条,:256)→ 组 `EmbedItem{cache_key 十六进制, fingerprint}`(:244-249)→ `embed_batch` 发 ai-worker。
4. worker 推理(`crates/exotic-workers/ai-worker/src/batch.rs`):`handle_embed`(:139)批上限校检 → 解码线程池按核数(:37)读 `{ai_cache_dir}/{key[..2]}/{key}.webp`(key 白名单纯 ASCII 防路径穿越,:110-116)→ `clip::preprocess_image`(短边→image_size)→ 与推理**流水重叠**(子批 ≤16 边收边推,T18.5b,:202-264)→ `clip::encode_image_batch`。逐项失败不连坐;Ok 项 f32 LE 按请求序排 blob 同帧回传(:271-305)。
5. Writer(`write_results`:`pipeline.rs:301`):成功项攒批、失败项另记;**满 512 或 3s 陈龄先到先落**(`WRITE_FLUSH_INTERVAL`:`pipeline.rs:36`,封顶进度冻结与崩溃丢失窗口)。`flush_batch`(:383)单事务条件写 → `invalidate_embedding_cache`(:407)。
6. 收尾:provider 回声落库 + `close_session` 卸会话释放 VRAM(`worker_pipeline.rs:101-107`);自然完成清 `ai_analysis_active`+释放 GPU 槽(:87-95);worker 进程留存,空闲 300s 自杀兜底(`worker_pipeline.rs:22` 头注)。

**搜索半部**(独立链路):查询文本 → `AiWorkerClient::encode_text`(`worker_client/dispatch.rs:127`,EncodeText op,文本塔**恒 CPU**——BERT int64 DirectML 会算错,`scrollery-ai-core/src/engine.rs:396`)→ `semantic_search_with_vector`(`search.rs:106`;维度守卫 :116-121)→ `ensure_cache`(:47)首次一次性从 `ai_embeddings` 载全库为 f16 常驻(`EmbeddingCache`:`search.rs:26-31`)→ rayon 全行点积(单位向量点积==余弦,:160-170)→ 降序取 Top-K(:180-181)→ 事务内 DELETE+INSERT 覆写 `ai_search_results`(:192-203)。缓存命中时零 DB 读。

**数据与产物**
- `ai_embeddings(item_id, model_name, embedding BLOB[f32 LE], version, created_at)`,PK `(item_id, model_name)`(`early.rs:221-228`);维度=模型 `embed_dim`(默认 cn-clip-vit-b16=512,`scrollery-ai-core/src/profile.rs:90/170`);写入时机=Writer flush(`batch_finish_ai_items`)。
- `media_items.ai_status`(0-3)+ 部分索引 `idx_media_ai`(`early.rs:235-236`)。
- `ai_search_results(file_id PK, similarity REAL)`(`early.rs:252-255`),每次搜索事务覆写;画廊经 `JOIN ai_search_results ai ON m.id=ai.file_id` 消费(`db/queries/layout/query_builder.rs:42`)。
- 磁盘 ai_cache:`{cache_dir}/ai_thumbs/{hex[..2]}/{hex}.webp`,短边 336 WebP(`thumbnail/cache.rs:58,67-74`);独立目录,缩略图 LRU 不驱逐;命中判定=存在性,派生/现场生成共用 `generate_ai_cache`。
- 模型文件:`{app_data_dir}/models/`(`runtime_config.rs:19-23`),图像塔 onnx+extra+文本塔+vocab 五件套;SessionInit 逐件 Path+归属+len+sha256 校验(`ai-worker/src/session.rs:138-182`)。
- 配置键:`ai_active_model`/`ai_active_image_file`/`ai_provider_override`(auto/directml/cpu)/`ai_batch_size`/`ai_download_source`/`ai_analysis_active`。

**前端消费链**
- 索引控制与进度:`ToolsSection.vue:482-500`(start/restart)→ aiStore 委托 `useAnalysisController`(`aiStore.ts:46-69`,5 命令+logTag 参数化)→ `get_ai_status`(`ai_commands.rs:88`)2s 轮询(`useAnalysisController.ts:106`,in-flight 合并)→ `analyzeProgress`/`providerLabel`/`isWaitingBlocked` 渲染;start/restart 被 GPU 槽拒绝走 toast(`aiStore.ts:57-65`)。
- 搜索:`AppToolbar.vue:478-482`(语义模式防抖)→ `searchStore.commitSemantic`(`searchStore.ts:67`)或 `apply`(:91,URL 回填)→ `aiStore.runSemanticSearch`(`aiStore.ts:130`;searchToken 代次守卫:164,旧应答整体丢弃)→ `invokeIpc(SEMANTIC_SEARCH_CMD)`(:160)→ 成功后 `mediaStore.invalidateLayout()`(:167)让布局按 `ai_search_results` 重载 → `SemanticSearchPanel.vue` 覆盖面板(进度徽章/提供商标识/启动入口 :258);语义模式 sort=similarity、groupBy=none 由 `setSearchMode` 协调(`aiStore.ts:200-228`)。
- 事件回显:无流式事件——进度靠 2s 轮询,搜索靠 IPC 返回 count+layout 失效;`IPC.SEMANTIC_SEARCH_CMD` 常量 `src/constants/ipc.ts:259`。

**并发 / 取消 / 错误**
- 并发:共享骨架(Producer→有界 crossbeam→派发→Writer)三条 rayon::scope 线程(`worker_pipeline.rs:79-97`);worker 端解码线程=核数、推理子批≤16 与解码流水重叠;`background_heavy_limiter`「1 批=1 槽」与 exotic/derive 公平共享;GPU 分析令牌与 face/enhance 互斥(`state.rs:543`)。
- 取消:`CancellationToken` 三处逐点检查(Producer 领批/现场派生每项/Dispatch 攒批);取消后本批在途项保持 Processing,下次运行孤儿恢复归 Pending;pause/stop 命令 cancel 后**立即释放 GPU 槽**(`ai_commands.rs:329/351`),自然完成才由完成回调释放(:94),restart 因取消后同轮重启保持持有。
- 错误分层:逐项 terminal(解码失败/MalformedInput)→ `ai_status=3` 不连坐;瞬态(retryable/IoError)→ 跳过保持 Processing;批级(worker 硬止损重建重发一次仍败)→ 终止本轮返回 Err,在途项下轮恢复(`worker_pipeline.rs:264-270`);维度红线 EmbedDimMismatch → 整批 Failure(系统性);`MAX_ATTEMPTS=2`+500ms 退避(`worker_client/mod.rs:61-64`),批超时 120s/文本编码 30s(`exotic/coordinator.rs:74/88`);进程级异常(超时/断开/协议违例)→ 弃实例重建重发。
- 关键不变量:有效批恒夹 ≤256(自动档按 VRAM 阶梯,固定 batch 变体抬到 ≥k:`pipeline.rs:246-298`);Producer/Writer 批=512、派发批=spec.batch_size(**总表「批 512」实指前者**,两粒度正交);f16 缓存按 model_name 键、`invalidate_embedding_cache` 后惰性重载(`search.rs:47-101`);文本塔 EP 恒 CPU;打分维度不符直接报错不越界。

---

### 2.3.6 AI 人脸

**一句话**:检测(YuNet)→嵌入(SFace 128d)→增量最近质心聚类→人物墙/审批,产出 `faces`/`persons` 行与 `face_thumbs/`(短边 640)缓存;推理恒经 `ai-worker` 子进程,与 CLIP 互斥共享唯一 GPU 分析槽。

**触发入口**:
- `start_face_analysis`(`ipc/face_commands.rs:196`)——「开始/继续」按钮 + 崩溃后前端 `maybeAutoResume` 自动续传(`composables/useAnalysisController.ts:191`):**不重置**,只处理 `face_status≠0` 之外。链:就绪预检 `face_dependencies_ready`(`face_commands.rs:98`,face 双 onnx 在盘 + 合并会话的 CLIP 依赖前置)→ `try_acquire_gpu_analysis(GPU_OWNER_FACE)`(`:217`,CLIP 持有时快速失败)→ `cancel_face_analysis` 旧轮(`:223`)→ `launch_face_pipeline`(`:184`,落 `face_analysis_active=1`)→ `start_face_pipeline`(`face_pipeline/mod.rs:84`)。
- `restart_face_analysis`(`face_commands.rs:257`)——「从零重跑」:同预检+先占 GPU 槽(`:269`,被拒不毁数据),`reset_face_data`(`:281`,按 model 删 faces+persons+face_coverage、face_status 归 0)后 launch。会销毁本轨命名/确认劳动,前端须确认。
- `pause_face_analysis`(`:305`)——cancel + 释放 GPU 槽 + `face_analysis_active` 保持 1(可续传)。
- `stop_face_analysis`(`:318`)——cancel + 释放 GPU + active 清 0(不再自动续传,数据保留)。
- `retry_failed_face_items`(`:240`)——`face_status` 3→0 分批复位,**零破坏**补跑失败项(与 restart 销毁语义严格区分)。
- 自然完成:worker 回调在 `!token.is_cancelled()` 时清 active + 释放 GPU 槽(`face_pipeline/mod.rs:110-115`)。
- 间接入口:`download_face_model`(`face_commands.rs:652`)装模型(原 gated `set_active_face_model`(`:593`)切轨 IPC 已随 P14 删除)——切轨取消在途流水线,新轨须显式 start。

**状态机**:
- 媒体项 `face_status`(0/1/2/3,同 `ai_status` 语义):0=pending(Producer 领取置 1,`face_pipeline/producer.rs:94`);2=done(Writer 条件写 `db/queries/faces/approval.rs:423`);3=error(失败批 `face_pipeline/writer.rs:193`);启动孤儿恢复 1→0(`mod.rs:136`);restart 全 0(`faces/status.rs:152`);换轨 sync 指向新轨覆盖(`status.rs:197`,有账→2 无账→0,V17)。
- 期望运行标志 `face_analysis_active`(配置键):launch 置 1 / pause 保 1 / stop 与自然完成清 0(`mod.rs:112`)。
- 运行令牌 `face_analysis_token`(RunTokenSlot,`state.rs:138,524`) + GPU 分析槽 `gpu_analysis_owner`(`state.rs:146`)互斥门闩。完成 vs 取消判定 = `!token.is_cancelled()`(`mod.rs:110`),不采用 thumb 的 finish-bool 姿态。

**主链阶段**(启动前置 → 1~6):
0. **启动前置**(`run_face_pipeline_worker_blocking`,`mod.rs:195`):`active_face_profile`(`ai/runtime_config.rs:104`,face_enabled 门 + face_model_active 解析)→ `build_session_spec`(`ai/worker_client/session.rs:256`,CLIP 双塔+face 成对的**合并会话**)→ 孤儿恢复 Processing→Pending(`mod.rs:136`)+ V17 覆盖自愈 sync(`mod.rs:218`)+ person 计数对账(`:225`)+ 已分析未聚类脸补聚(`reconcile_unclustered_faces`,`mod.rs:157`)→ 预计数 `count_pending_face_items`(`:234`)。
1. **Producer**(`producer.rs:18`):批查 `get_pending_face_items`(512 批,`faces/status.rs:49`)→ 让步检查 `ai_yield_blockers`(`producer.rs:36`,scan/thumb/derive/exotic 占先时 sleep 500ms 让路)→ 置 Processing(`producer.rs:94`)→ 发 `FaceTask` 入通道(容量 1024,`mod.rs:55`)。
2. **Dispatch 攒批 + 三级定源**(`dispatch.rs:65`):`recv_timeout` 50ms 刷新(`:83`),有效批 cap = min(session batch, **16**)(`FACE_DISPATCH_BATCH`,`dispatch.rs:30-35`,YuNet 逐图推理批大小不影响吞吐,只影响超时/落库粒度)。三级定源(`dispatch.rs:164-189`,决策核 `decode_source.rs`):
   - **Thumb**:`resolve_face_decode_source`(`decode_source.rs:47`)缩略图档位预测短边 ≥ 640;
   - **FaceCache**:`face_cache_applies`(`decode_source.rs:110`)原图短边 > 640 或 worker 不可解格式(heic/raw/psd)→ 走 `face_thumbs` 640 缓存,缺则本批 rayon 并行现场预解码 `generate_face_cache`(`derive/image.rs:77`,WIC 优先/CPU 回退,panic_guard 防单文件连坐,失败标 Error);
   - **Original**:短边 ≤ 640 可解小原图直派;防呆分支(detect_size > 缓存 640 且不可解)→ Skip 保持 Processing。
   CPU permit(`dispatch.rs:153`,**D2 先 CPU 后 GPU**)→ GPU 令牌(`:274`)→ `client.face_detect_embed`(`worker_client/dispatch.rs:148`,FaceDetectEmbed 帧,`op_timeouts::face_detect_embed`=60s+6s/项,`exotic/coordinator.rs:78-85`)。
3. **Worker 推理**(`ai-worker/batch.rs:408 handle_face`):分块全并行(块大小 = 探测核数,`batch.rs:37-43`),线程内跑完解码→letterbox→YuNet 检测→SFace 对齐嵌入整链,GPU session 由池锁自串行;几何走 JSON、嵌入走 blob(f32 LE,`batch.rs:497-522`);**0 脸也是 Ok**(零脸图);EmbedDimMismatch 整批 Failure;批尾一行分段诊断(解码/检测/嵌入均值,`batch.rs:540-551`)。
4. **Writer**(`writer.rs:24`):**双节奏**——小批 16 落库(`STATUS_FLUSH_EVERY`,`writer.rs:21`)、大批 512 跑聚类(`writer.rs:79`)。`flush_face_rows`(`:142`)经 `batch_finish_face_items`(`approval.rs:410`)单事务「先删后插 faces + face_status=2 + face_coverage 记账」(X1 条件写:cache_key 快照核对,失效项整体跳过;`Done 必有脸已写`、`Done 必有覆盖行`);失败项小批置 3(`flush_face_failed`,`writer.rs:193`);DB 失败改标 Error 不连坐整批(`:167`);零脸图 records=Some(空)=成功。
5. **增量聚类**(`face_cluster.rs:175 cluster_new_faces`):读名册+待聚类脸(`:197-204`,低于 `min_quality` 不参与,`person_id` 留 NULL)→ `assign_face`(`:114`)余弦最近质心 ≥ threshold 并入(质心滑动平均重归一 + 质量更高者升级封面),否则建占位 person(id<0)→ `apply_face_clusters` 落库(`:254`)。阈值取「config override(`face_same_threshold`/`face_min_quality`)或 profile 默认」(`effective_thresholds`,`:50`)。
6. **全量重聚类**(显式命令,`face_commands.rs:426`→`recluster_all`,`face_cluster.rs:476`):O(n²)但**护用户劳动**——`is_confirmed` 脸与 `is_ignored`/命名人物永不动(`plan_recluster`,`:315`),`face_rejections` 负样本防吸回被拒脸。分析运行中拒绝执行(令牌守卫)。

**数据与产物**:
- `faces`(db/schema/early.rs:410-431,`item_id` 非唯一=一图多脸):bbox 归一化 [0,1](按**协议回报的实际解码尺寸**,`decode_source.rs:127`)、5 关键点 landmarks BLOB、det_score、quality、embedding f32 LE(128d)、is_confirmed、`model_name`(=向量空间键)。
- `persons`(db/schema/early.rs:396-409):name、cover_face_id(质量最高脸)、centroid BLOB、face_count、is_named/is_hidden/is_ignored、model_name。
- `face_coverage` 记账表(V17):item+model 已分析账,**零脸图也有账**,与脸行/status 同事务写(`approval.rs:438-444`)。
- `face_rejections`(db/schema/mid.rs:61):负样本对,全量重聚类防吸回。
- 缓存 `cache/face_thumbs/{prefix}/{hex}.webp` 短边 640(`thumbnail/cache.rs:99,103`;与 CLIP 336 `ai_thumbs/` 不共用,与缩略图共用 10GB LRU 预算)。
- 模型:激活轨双 onnx(detect/embed)在 models 目录,size 快检防 LFS pointer 截断(`face_commands.rs:43`);默认轨 `yunet-sface`(detect_size 640 / embed_dim 128 / threshold 0.363 / verified,`scrollery-ai-core/face_profile.rs:139-185`)。
- 写入时机:Writer 小批(16)单事务落库 + 记账;聚类大批(512 或收尾)触发。

**前端消费链**:
- **控制面**:`faceStore`(`stores/faceStore.ts:16`)委托 `useAnalysisController`(`composables/useAnalysisController.ts:53`)——2s 轮询 `get_face_status`(in-flight 合并)、start/pause/restart/stop、崩溃续传 `maybeAutoResume`(`:191`,读 analysisActive && pendingItems>0);进度 = processedItems(**含失败**,失败条仍到 100%);start/restart 后端错误走 toast。
- **人物墙/审批**:`personStore`(`stores/personStore.ts:12`)——`load`(墙)/`loadIgnored`(误检桶)/`loadLikelyMatches`(审批建议,限 50);审批动作 confirm/reassign/unassign/reject/createPerson 乐观 `dropResolvedFaces`(`personStore.ts:140`);`recluster`(`:120`)。
- **详情叠加框**:`get_item_faces` → `useContentViewerFaces.ts:43`。
- **模型库**:`FaceModelLibrary.vue` + `listFaceModels`/`downloadFaceModel`(Channel 进度,`faceStore.ts:82-100`)。
- **IPC 契约**:`src/constants/ipc.ts:301-323`(25 条,含 `START_FACE_ANALYSIS`…`CREATE_PERSON`)→ `src/utils/ipc.ts` `invokeIpc`,命令名与 Rust `#[tauri::command]` 同名。
- **事件回显**:人物归属写命令后 `bump_data_version()`(`face_commands.rs:418/480/489/505/521`)驱动画廊 data_version 重排;人物墙无后端推送,进页显式 `load()`(`PersonsView.vue:224`)+ 审批关闭重载(`:235`)。

**并发 / 取消 / 错误**:
- **并发**:走共享骨架(producer→crossbeam 1024→消费者→writer,CancellationToken + RunTokenSlot + `background_heavy_limiter`,见 [Spec12](./Spec12_配置状态日志.md))。本流水线特例:`rayon::scope` 派 **3 独立 OS 线程**(producer/dispatch/writer,`mod.rs:254-280`);批 cap=16 不随 VRAM 涨(见主链 2);**GPU 分析槽与 CLIP 严格互斥**(单锁 check-and-claim,`state.rs:543`,占用失败快速报错不排队);worker 端多线程并行由 GPU session 池锁自串行(`batch.rs:443-449`)。
- **取消**:命令层 `cancel_face_analysis` 置 token → 三阶段各自检查(`producer.rs:28`/`dispatch.rs:79`/`writer.rs:50`);在途批/预解码中途项保持 Processing 下轮恢复(`dispatch.rs:225-227`);**终态门控 `!token.is_cancelled()`**(`mod.rs:110`)——取消路径由命令释放 GPU 槽,restart 须保持持有。
- **RunTokenSlot generation compare-and-clear**:`finish_face_analysis(generation)`(`state.rs:535`,`mod.rs:121`)——旧轮迟退出不得误杀 restart 刚装的新一轮;终态副作用不依赖其返回值。
- **错误分级**:retryable Failure→跳过保持 Processing(`dispatch.rs:335`);terminal→标 Error 不再无限重查(`:339`);预解码失败→标 Error(`:220`);DB 写失败→改标 Error(`writer.rs:167`);批级致命→fatal Mutex 带出 scope 终止整轮(`mod.rs:246,291-297`)。
- **worker 异常**:硬止损 **2 次**(`worker_client/mod.rs:61 MAX_ATTEMPTS`;进程级弃实例重建重发、retryable 退避 500ms、输出校验违例重建);`SessionExpired` 清快照由下轮 ensure 重建;空闲 300s 自杀(`ai-worker/main.rs:43`)由下轮 `ensure_session` 自动换代。
- **孤儿恢复**:崩溃残留 Processing→Pending(`mod.rs:136`);已 Done 未聚类的脸启动补聚(`reconcile_unclustered_faces`,`mod.rs:157`,硬崩溃丢 cluster_pending 的唯一救济)。
- **关键不变量**(与 [Spec06](./Spec06_AI人脸OCR.md) §4.2 一致):推理恒子进程(host 零 ort);D2 先 CPU permit 后 GPU 令牌;`face_thumbs` 640 不共用 CLIP 336 缓存(否则损害小脸召回);X1 条件写防迟到落库;`persons`/`faces` 按 `model_name` 双向隔离(切换轨不销毁他轨标注)。

---

### 2.3.7 OCR(PP-OCRv5)

**一句话**:OCR(PP-OCRv5)是「交互单发」的文字提取流水线——图片/视频帧经 det→cls→rec 三阶段 **CPU EP** 推理(rec 是 SVTR transformer 系,D-OCR-2 禁 DirectML),结果以 DTO 直接回前端内存态展示,**不入库、无任务队列、无持久化产物**;与 CLIP/人脸共享 ai-worker 子进程但会话槽独立(D-OCR-1)。

**触发入口**:
- `download_ocr_models`(下载前置,一次/档位):设置页 OCR 分节「下载」按钮(`src/components/settings/OcrModelSection.vue:129-149` → `src/constants/ipc.ts:281`)→ `ipc/ocr_commands.rs:391`。场景:首次用前预下载 mobile/server 两档模型;命令**有意不过授权门**(J8,`ocr_commands.rs:385-390`,允许「先下后购」)。
- `ocr_status`(门控+安装态):设置页分节刷新(`OcrModelSection.vue:126`)+ 前端 useOcr 判门(`useOcr.ts:49`)→ `ipc/ocr_commands.rs:219`。
- `ocr_extract_image`(画廊图片):看图器按钮(ContentViewer.vue:235-242 常显,`disabled=ocr.busy` → `onOcrImage` 876-879)→ `useOcr.extractFromImage`(`useOcr.ts:149`)→ `ipc/ocr_commands.rs:253`。
- `ocr_extract_frame`(视频当前帧):暂停态工具按钮(VideoPlayer.vue:285-288 `ocrFrame`)→ `useOcr.extractFromVideoFrame`(`useOcr.ts:173`,canvas 截帧 → `blobToBase64`)→ `ipc/ocr_commands.rs:288`。
- 四命令经 `invoke_handler` 注册于 `ipc/registry.rs:187-190`;`core:default` 权限覆盖,capabilities 无新增。

**状态机**:无 DB 持久状态机(识别结果不入库、无 task 表 0/1/2/3 态)。交互态全在前端 `useOcr` 模块单例(`useOcr.ts:66-77`):`busy`(提取互斥,门控判定前**同步**置位 `:151`、finally 清 `:164`)→ `panelOpen/result/sourceLabel`(applyResult `:135-143` 开面板;`closePanel` 清空并 `reqSeq++` `:238-243`)→ `statusCache` 60s TTL(`:23`,下载完成/激活后 `resetOcrStatusCache` `:37-42` 失效,`statusGen` 代际计数防迟到旧请求回填 `:28-34`)。worker 侧会话态:独立 `ocr_sess` 槽(`ai-worker/src/main.rs:240`),host 侧 `ocr_loaded: Option<String>` 簿记(`worker_client/mod.rs:143`),换代/自杀复位。无长任务 CancellationToken 态。

**主链阶段**:
1. **模型下载**(前置,走共享下载引擎原语,非调度骨架):选档 → `ocr_assets` 取该档 4 件清单(`ocr_registry.rs:98-118`,7 物理文件 pin 定于 `:37-88`,含 url/mirror_url/size/sha256 均本机实测)→ `download_assets`(`model_download.rs:35-42`):HTTP Range 断点续传 + 镜像回退(cls 无镜像)+ size+sha256 校验 → `.part` 原子改名 → 落 `{app_data_dir}/models/`(`runtime_config.rs:19-23`)。输出:四件模型文件就位。
2. **会话装配 OcrSessionInit**(host→worker):`build_ocr_session_spec`(`worker_client/session.rs:289-305`,活跃档位读 `ocr_active_tier` 配置,非法值 warn 回退默认 mobile)→ `build_ocr_session_init`(`:177-211`,四角色 OcrDet/OcrCls/OcrRec/OcrDict 逐件带 len+sha256,sha 经 mtime+len 备忘 `model_descriptor` `:215-248` 不重算)→ `ensure_ocr_session`(`:71-124`)零帧匹配命中即返回,否则发 `OcrSessionInit`。worker 端(`main.rs:599-667`):`preflight_ort_runtime`(`:607`,60s watchdog 快败死路径)→ `validate_ocr_init` 纯校验(角色集完备、descriptor 归属校验,`ocr.rs:64-112`)→ `OcrEngine::init`(`ocr/mod.rs:57-77`,三模型 CPU EP + 各池容量 1 + dict 装载)。**同步装载**(秒级 CPU 小模型,不走流式 Progress,`main.rs:548-549` 注);超时 180s(`coordinator.rs:94`)。输出:worker `ocr_sess` 就绪。
3. **批请求派发 OcrBatch**(host):图片入口查源路径(`get_item_path_info` → canonicalize + 存在校验,`ocr_commands.rs:263-269`);帧入口 base64 解码 + 落临时文件;组装 `OcrItem{item_id, cache_key:None, source_path, fingerprint}`(`:271-276` / `:343-348`)。`run_ocr_batch`(`ocr_commands.rs:156-211`)全程持 `ai_worker` Mutex(在 spawn_blocking 闭包内取,不跨 `.await` `:158-160`)→ `AiWorkerClient::ocr_batch`(`worker_client/dispatch.rs:184-283`,超时 60s+30n/项 `coordinator.rs:97-98`)。
4. **worker 推理链**(核心,ai-core,结果不入库):
   - **det**(DB 文本检测):`preprocess`(`det.rs:43-49`:RGB→CHW、`resize_dims` 长边 cap 960/1280、32 倍数、ImageNet norm)→ `detect`(`det.rs:300-342`)得概率图 → `boxes_from_prob`(`:204-259`):二值化 → `find_contours` → `min_area_rect` → `box_score` poly 掩膜均值过滤 → 确定性 `unclip` 外扩(`:100-140`,免 clipper)→ clamp → 除 ratio 回原图坐标 → `sort_reading_order`(`:262-297`,质心 y 分行/行内 x)。输出:原图坐标 quad 集。
   - 逐 quad:`get_rotate_crop_image` 透视摆正裁剪(`geometry.rs:18-56`,竖条 h/w≥1.5 转横)→ **cls** `classify_and_maybe_flip`(`cls.rs:45-84`,输入 80×160,双类概率 `cls_decision` `:17-19`,p1≥0.9 旋 180°)→ **rec** `recognize_text`(`rec.rs:62-110`:等比缩至 H=48、宽 cap 3200、`(px/255-0.5)/0.5`)→ `ctc_greedy_decode`(`:20-59`:逐 t argmax、折叠连续重复、丢 blank、`dict.char` 映射、conf=保留字符均值)。
   - 过滤:空文本或 conf<`rec_min_conf`(0.5)丢行;`max_lines_per_image`(1000)cap(`ocr/mod.rs:99-108`);dict/rec 维度契约自检(`dict.rs:60-74`,坑8 同型防线)。**swap_rb 通道序默认 true,定案仪式在 golden 对拍**([Spec06](./Spec06_AI人脸OCR.md) §3.3,`ocr_golden.rs:7-9`),未定案前不宣称可用。
5. **host 校验 + DTO 回传**:worker `handle_ocr_batch`(`ocr.rs:211-270`,逐项不连坐、终帧 JSON 双保险)→ host `validate_ocr_batch_output`(`exotic/validate.rs:289-362`,`不信任 worker`:results 同序同长、item_id+fingerprint 核对、conf∈[0,1]、quad 有限、**blob 恒空** D-OCR-4)→ `run_ocr_batch` 映射 `OcrResultDto{lines,width,height}`(`ocr_commands.rs:178-210`)→ IPC resolve 回前端。整链结束后无写入器阶段。

**数据与产物**:
- **DB**:零写入。识别结果、模型安装态均不落库(`ocr_status` 现查:`ocr_tier_installed` 只 stat 四文件非空,`ocr_registry.rs:122-137`)。配置侧仅 `ocr_active_tier` 档位键(设置页 `config.setOcrTier` `OcrModelSection.vue:151-161`)。
- **临时帧文件**:`{cache_dir}/ocr_frames/{random_hex(16)}.png`,`*.tmp` 写 + 同卷 rename(`ocr_commands.rs:329-341`),finally 语义成败都删(`:351-353`);崩溃残留由下次调用 `purge_stale_ocr_frames` 清扫 mtime>1h(`:362-378`);base64 解码前 ≈89MB/解码后 64MB 双重上限(`:38-41,298-315`)。
- **模型产物**:`{app_data_dir}/models/` 下 7 个物理文件(det/cls/rec 各两档互异 + dict 共用 74012 字节),sha256 均实测钉定(`ocr_registry.rs:37-88`)。
- **识别产物**:仅内存态 DTO,无缓存目录、无落盘格式。

**前端消费链**:ContentViewer.vue(`:596` 实例、`:233-242` 按钮、`:901-905` 切项 watch 收口面板)/ VideoPlayer.vue(`:113`、`:285-288`)→ `useOcr` 模块单例(`useOcr.ts:79-255`)→ `src/utils/ipc.ts` `invokeIpc` + `src/constants/ipc.ts:275-281` → 后端命令 → **无订阅事件**,结果经 IPC resolve 直接回填;下载进度走 `Channel<DownloadProgress>` 事件(`OcrModelSection.vue:134-139`)。消费方 `OcrResultPanel.vue`(浮层面板,行文本 user-select 可选中复制 `:29-41`,`copyAll` 仅用户点击写剪贴板 `useOcr.ts:223-232`)。门控三分支 ensureGate `useOcr.ts:85-109`(未授权→跳插件商店/模型未装→跳设置页)。

**并发 / 取消 / 错误**:
- **并发模型**:**不适用**「生产者→crossbeam→消费者池→写入器」共享骨架(见 [Spec12](./Spec12_配置状态日志.md) §调度)——OCR 是交互单发同步通路,独占 `ai_worker` Mutex 于 spawn_blocking 全程(`ocr_commands.rs:158-160`),与 CLIP 批严格串行(OCR 最坏排在在途 EmbedBatch 后,前端有排队 toast `useOcr.ts:155`)。双槽:CLIP `sess` 与 OCR `ocr_sess` 在 worker 内独立并存、互不干扰(`main.rs:239-240`;边界 10 有 `#[ignore]` 双槽隔离测 `main.rs:752-806`)。
- **取消**:无 IPC 取消命令、无 CancellationToken——worker 内 `cancelled` 回调恒 `&|| false`(`ocr_commands.rs:159`)。前端侧取消 = `reqSeq` 单调取号令迟到结果作废(面板已关/被新提取取代不弹、不 toast `useOcr.ts:77,159,213`)+ `busy` 互斥拦重复点击。worker 空闲 300s 自杀兜底(`main.rs:43`)。
- **错误**:`MAX_ATTEMPTS=2` 硬止损重试圈(`worker_client/mod.rs:61`;`dispatch.rs:195-283` 与 CLIP 逐臂同构,仅 ensure 换 `ensure_ocr_session`):进程级(超时/断开/协议违例)→ 弃实例重建重发;`SessionExpired` → 清 `ocr_loaded` 由下轮重 init;retryable Failure → 500ms 退避原会话重发(`mod.rs:64`);`ModelLoadFailed` → 类型化 `ocr_model_missing`(深审#1,非字符串匹配 `session.rs:99-106`)。稳定码映射(`ocr_commands.rs:161-210`):门控四态(未激活/未授权/过期/不可用 `:65-84`)、单项 `IoError/MalformedInput`→`ocr_decode_failed`、`ResourceLimit/InternalError`→`ocr_engine_failed`、兜底→`ocr_worker_failed`(message 不透传 worker 内部串)。
- **关键不变量**:恒 CPU EP(D-OCR-2);worker 端像素级设防(`MAX_SOURCE_FILE_BYTES` 512MB stat 只拦压缩字节 → `MAX_SOURCE_PIXELS` 1e8 像素解头先判再解码,`ocr.rs:35-44,144-163`);单项文本 512KB/批 8 项/终帧 900KB 三重 cap(`ocr.rs:30-48`);dict/rec 维度错配拒绝带病服务(`dict.rs:60-74`);结果 blob 恒空走 JSON(D-OCR-4)。

---

### 2.3.8 图像增强(enhance)

**一句话**:影像增强(enhance)是付费门控的降噪/去 JPEG 伪影/超分推理流水线——前端多选入队 → **内存态 job 队列** → enhance-worker 子进程(ONNX)按 512²+pad16 clamp-short tiling 逐 tile 推理融合 → 产物经容器级 EXIF 注入(不重编码)、同卷 rename 后以**新 `media_item`** 入库,源文件不动;执行链固定序 降噪→去伪影→超分(host 排序契约)。调度不套 exotic 任务化骨架( `mod.rs:8`「D-OCR-7 同型豁免」),由 IPC 显式驱动;共享件(limiter/RunToken/CancellationToken/孤儿)见 [Spec12](./Spec12_配置状态日志.md) §调度。

**触发入口**
- `enhance_start` `ipc/enhance_commands.rs:210` — 查看器增强按钮(仅静态图,`ContentViewer.vue:244-250` → `openEnhance` `ContentViewer.vue:896`)→ `EnhanceDialog.vue` 提交 → `enhanceStore.start` `enhanceStore.ts:159`
- `enhance_preview` `enhance_commands.rs:184` — 对话框「生成对比」(`EnhanceDialog.vue:83`→`generatePreview` `:329`),单 tile 前后对比
- `enhance_cancel` `enhance_commands.rs:279` — 队列面板取消按钮(`EnhanceQueuePanel.vue:37-43`)
- `get_enhance_queue` `enhance_commands.rs:286` — 队列快照拉取(`enhanceStore.ts:143-151`)
- `enhance_status` `enhance_commands.rs:75` — 设置分节/对话框判门(`EnhanceSettingsSection.vue:155`、`EnhanceDialog.vue:317`)
- `download_enhance_model`/`delete_enhance_model` `enhance_commands.rs:108/154` — 设置分节模型卡下载/删除(`EnhanceSettingsSection.vue:123-153`)
- 七命令注册 `ipc/registry.rs:191-197`;capabilities 走 `core:default` 免新条目(`enhance_commands.rs:7-9`)

**状态机**
JobStatus 五态(`service.rs:88-96`),纯内存 `JobBook`(`service.rs:133-137`,P0 重启即丢):`Queued → Running → Done|Error|Cancelled`。
- `enqueue` 置 Queued(`service.rs:249-270`,含 tiles_total 估算)
- `run_job_blocking` 起跑置 Running(`service.rs:327`)
- `cancel`:`Queued→Cancelled` 立即(`service.rs:277-279`);`Running` 置 `CancellationToken` → 在途 run_request/limiter.acquire 醒来中止 → supervisor kill → 命中 token 返「已取消」→ 终态 Cancelled(`service.rs:348-358`)
- 全 item 完成→Done;任一步错误→Error(稳定码 `error_code`)
- 无 DB 持久化,进程重启队列丢、work_dir 首触清扫(`service.rs:216-227`)

**主链阶段**
命令层门(`enhance_commands.rs:217-231`):授权门 `ensure_enhance_authorized`(`:33-45`,过期/未激活→`enhance_unlicensed`)→ `validate_strengths`(NaN/Inf 拒,`service.rs:858`)→ **RAW 同步准入** `item_is_raw`(catalog 单源反查插件 id,`service.rs:960` / `:972`,入队前拦死 job)→ `enqueue` 估算 tiles_total(`service.rs:249-252`,几何模拟 `:904-925`,未知尺寸该 item 计 0)。

后台驱动 `run_job_blocking`(`service.rs:318`,spawn_blocking 内)持 worker Mutex 全程串行:
1. **准入预检**(`execute` `service.rs:364-416`):`sort_steps` 固定序(`:843-855`)→ steps 空拒→ max_scale → 模型就位检查 `enhance_model_installed`(`registry.rs:66`,缺→`enhance_model_missing`)→ work_dir=`<cache>/enhance_work/` + `sweep_work_dir_once` → **D2 天条**:先 `background_heavy_limiter.acquire` 后 `gpu_token.acquire`(`:408-416`,与 CLIP/人脸争同一 GPU 令牌)。
2. **EnhanceSessionInit**(`service.rs:418-444`):每 job 一次,distinct model_id 各装 fp16(GPU 且 `fp16_safe`)否则 fp32(`enhance_model_descriptor` `:990-1014`;`enhance_prefers_gpu` `:954`);90s 超时(`coordinator.rs:104`);worker 端流式装载 + 10s 心跳(`main.rs:348-520`)。
3. **逐 item**(`process_item` `service.rs:474-638`):
   a. DB 读源 `get_media_detail`+路径+卷(`:491-513`;deleted/离线→`enhance_input_unsupported`);
   b. 解码尺寸 + `admission_check`(`:519-529`;直接 ≤100MP / 4×超分 ≤16MP 准入门,`MAX_INPUT_PIXELS_*` `:53-56`、`admission_check` `:870`);
   c. 输出格式 `resolve_output_format`(`:927-942`,FollowSource:png→png 其余→jpeg);
   d. **claim** `{stem}-enhanced.{ext}` 占位(`naming.rs:40` `create_new` 独占;`service.rs:539-556`,冲突 9999 次→`enhance_io`);
   e. **EnhanceRun**(`service.rs:558-595`)→ worker `run.rs:129-208` → `run_enhance_chain`(`chain.rs:247`):解码→逐 step `plan_tiles`(512/16,步进 480 **clamp-short** 无缝无叠分区,`tiling.rs:46-104`)→ `sample_rgb_tile` reflect-101 越界补齐(`chain.rs:80/93`)→ `build_and_run_tile` 按 `aux_input` 分支(DRUNet=SigmaMap 第4通道 / FBCNN=QF 标量第二输入 / 其余单输入,`chain.rs:148-244`)→ `write_core` 只取中心 `pad·scale` 区拼接(`:119`)→ 全图 f32 planar 画布逐 step 叠乘 scale → 编码 JPEG q95 / PNG 无元数据(`encode_canvas` `:360`);per-tile `Progress` 帧(`run.rs:172-187`)→ host `bump_tile` + `TILE_EMIT_THROTTLE` 500ms 节流 emit(`service.rs:62/576-582`);
   f. **finalize_output**(`service.rs:602-605/1141-1182`):容器级 EXIF 注入不重编码(`exif_inject.rs:42` JPEG APP1/PNG eXIf,只带 DateTimeOriginal+orientation=1,`build_source_exif` `:11`)→ `{claimed}.tmp` 写盘+`sync_all` → 同卷 rename 覆盖占位;
   g. stat + `ingest_single_file` 入库(`db_writer` Mutex,`editing/ingest.rs:33`)→ `bump_done` + emit(`service.rs:466-467`)。
4. `EnhanceSessionClose` best-effort + `session_alive=false`(`:333-346`)→ 终态 `set_status` + emit(`:348-359`)。

预览(`preview` `service.rs:646-837`):`try_lock` 严格单请求(worker 忙→`enhance_busy`,`:654-660`)→ 解码裁中心/点位 512² → `before.png`(atomic 写)→ CPU+GPU token → SessionInit → 单 tile EnhanceRun → `after.png` 固定名覆盖(rename 失败兜底 copy)→ **不队列、不 claim、不 ingest**。

**数据与产物**
- 无新 DB 表:job 全内存(`JobRecord` `service.rs:119-137`);产物经 `ingest_single_file`(`ingest.rs:33`)upsert 新 `media_items` 行(`media_type="image"`、`width/height`=EnhanceDone、`view_rotation=0` 默认),缩略图靠 `thumb_status=0` 自动入队(`ingest.rs:6-7`);源 item 不动。
- 缓存目录:`<cache>/enhance_work/`(worker 输出白名单前缀,`run.rs:51-81` canonicalize 越界拒);`<cache>/enhance_preview/{before,after}.png` 固定名覆盖(`service.rs:729-836`)。
- 模型:`<models_dir>/{id}-fp32.onnx|{id}-fp16.onnx`,名单源 `enhance_profile.rs:69-123`(5 档 tile/pad 全 512/16);下载清单 `PENDING_USER_REPO` 占位 **fail-closed**(`registry.rs:20/31-48`,URL 含 PENDING_ 或零字节/空 sha256 前禁真实下载)。
- 产物格式:JPEG q95 / PNG(`chain.rs:377-387`),core 不写元数据,EXIF 全由 host 容器级注入。

**前端消费链**
`ContentViewer.vue:244-250`(Sparkles 入口,仅 image)→ `EnhanceDialog.vue`(任务勾选+模型档+σ/QF 滑杆+预览+输出格式)→ `enhanceStore.ts`(`start:159`/`preview:154`/`cancel:167`/`fetchQueue:143`)→ `invokeIpc`(IPC 契约 `src/utils/ipc.ts` + `constants/ipc.ts:285-297`)→ 回显事件 `enhance:queue-changed`(`ipc.ts:439`)→ `subscribeQueue`(`store:180-193`,订阅计数守卫)拉全量,`notifyTerminalTransitions` 弹终态 toast(`store:125-140`)。队列面板 `EnhanceQueuePanel.vue` 双挂载:对话框 + 设置分节(`EnhanceSettingsSection.vue:8`,常驻订阅);预览产物路径 `resolveAssetUrl`→`convertFileSrc` 喂 `BeforeAfterSlider`(`EnhanceDialog.vue:332-333`)。

**并发 / 取消 / 错误**
- **并发**:单 enhance-worker 进程惰性持有(`WorkerHandle` `service.rs:140-174`),job 执行全程持 worker Mutex 天然串行;队列**非 FIFO**(谁先抢到锁谁跑,F-10 已知);与 CLIP/人脸共用 GPU 令牌(`state.rs:171`)+ `background_heavy_limiter` CPU permit(`state.rs:165`);全链 `spawn_blocking` 离 UI 线程;worker 严格串行一帧一请求 + 顶层 `catch_unwind` panic 即退(`main.rs:9/258-310`);空闲 300s 自杀兜底(`main.rs:39`)。
- **取消**:`cancel(job_id)` 置 token(`service.rs:273-281`)→ 在途 `run_request` 按 CANCEL_POLL 轮询醒来(`worker.rs:296-297`)→ Disconnected → supervisor kill worker → `send_request` 命中 token 返「已取消」(`service.rs:1074`)→ 终态 Cancelled;claim 占位/worker tmp 由 `inspect_err` 清理(`:592-605`)。
- **错误**:全走 `AppError::Enhance` 稳定码不泄内部串;worker Failure→`map_worker_failure`(`service.rs:1083-1096`,ModelLoadFailed→`enhance_model_missing`、MalformedInput→`enhance_input_unsupported`、ResourceLimit→`enhance_io` 显存提示);chain 错误逐变体映射(`run.rs:85-101`);worker 死亡 `send_request` 重建 + 补发 session init(深审 h,`service.rs:583-591/1023-1080`,`MAX_ATTEMPTS=2` + 500ms 退避);会话未载→`SessionExpired` retryable(`run.rs:104`)。
- **关键不变量**:D-439 tile=512 与 ONNX 静态输入 shape 联动(改须重导,`tiling.rs:7-9`);clamp-short 无缝无叠分区、reflect 边缘(`tiling.rs:11-17`、`chain.rs:80`);host 排序是契约、chain 不重排(`enhance/mod.rs:9-10`);D2 CPU permit 先于 GPU token;输出路径白名单 `work_dir`。

**纠错注**:总表第 8 行「详见 [Spec06](./Spec06_AI人脸OCR.md) §增强」所指节**不存在**(Spec06 现无任何 enhance 内容,已 grep 核实);增强深机制权威源为 `docs/planning/2026-07-24-降噪超分子系统/design.md`,本节为工作流视角补充,建议总表该链接改指本节与 design.md。

---

### 2.3.9 exotic 格式插件

**一句话**:RAW/PSD 等主程序不内置解码的冷门格式,经独立 worker 子进程在后台解码为 WebP 缩略图并回填 `exotic_tasks` 五态;Coordinator 幂等唤醒唯一 Pipeline,验签/授权链只驻 host 侧,worker 进程只做解码——首发实装 psd-worker + raw-worker 两条解码器,格式并集经 `formats::merged_formats` 供扫描器/弹层消费。

> **对总表基线勘误(两处,以现码为准)**:①「probe(raw-probe/psd-probe)」是**开发期技术去险探针**(`raw-probe/main.rs:21`、`psd-probe/main.rs:27`,冻结解码支持范围用,不参与运行链);运行期「probe 阶段」指 `run_exotic_pipeline_blocking` 步骤 1 的**spawn Worker→握手→取 worker_version**(`pipeline.rs:188-196`)。②「psd-worker keyset 验签」不成立——验签/完整性命在 **host 侧** `installer::resolve_worker_path`(`installer.rs:350`,拉起前重验 manifest+hash),psd-worker/raw-worker 进程内零密钥材料;raw-worker 侧「CDDL」疑为笔误,实际是 **rsraw(vendored LibRaw)提取厂商内嵌 JPEG 预览**,一期不做 demosaic(D-430,`raw-worker/decode.rs:62`)。

**触发入口**(全部收敛到 `state.rs:850` `wake_exotic` → `ExoticCoordinator::wake`,`coordinator.rs:238`):

- **扫描播种(主入口)**:扫描事务内按扩展名命中 Catalog 即 `seed_exotic_tasks_for_item`(`fast_scan.rs:502-525` 主循环、`fast_scan.rs:553-564` suspect 定案分支;守卫 `seed_gate_admits` `fast_scan.rs:655`——掐掉合成标记 builtin 与 video 类 builtin);批提交后 `scan_commands.rs:613` 发 `WakeReason::ScanCommitted`。Catalog 更新另走 `backfill_exotic_tasks_for_format`(`db/queries/exotic.rs:83`)。
- **启动兜底**:`coordinator.rs:611` bootstrap 发 `Startup`(孤儿恢复 + worker 版本对账,`needs_reconcile` 仅认 Startup/PluginInstalled/UserRequested,`coordinator.rs:289`,防 30s 时钟白 spawn)。
- **用户显式**:`start_exotic_processing`/`retry_exotic_plugin_failures`(`exotic_commands.rs:162/330`;单任务重试 `retry_exotic_task`(`:315`)已随 P14 删除)发 `UserRequested`——唯一绕过 `exotic_auto_process` 门控的入口。
- **安装/回滚/升级/激活/配置**:`exotic_commands.rs:683/816` 发 `PluginInstalled`;`378` 发 `LicenseActivated`;`399` 与 `config_commands.rs:604`、`media_commands.rs:223`、`thumbnail_commands.rs:291/629` 发 `ConfigChanged`。
- **重试时钟**:Coordinator 独立 interval 每 30s 发 `RetryDue`(`coordinator.rs:172`、`222-231`),到期 retryable 任务重新被评估。

**状态机**(`exotic_tasks.status` 五态,`task.rs:13-24`,`db/schema/early.rs:484-506`):

```
pending(0) ──claim_exotic_tasks(单条 UPDATE...RETURNING,写 claimed_at+lease_owner)──> processing(1)
   ▲                                                                                    │
   │                                                              finish_exotic_task    │
   │                                                ┌──────────────┴───────────────┐
   │                                                ▼                               ▼
   │                                        done(2,写 output_path/指纹/版本)  retryable(3,next_retry_at 退避)/terminal(4)
   └──── SourceChanged / worker 升级 / 孤儿回收 / 取消清理 ── 回 0
```

- 领取原子性:`claim_exotic_tasks`(`db/queries/exotic.rs:159`)单条 `UPDATE...SELECT...LIMIT...RETURNING`,条件 `status IN(0,3)` 防双实例重复领;隐藏根排除与四条主流水线同口径(V21)。
- 成功/失败均为**条件写**:`finish_exotic_task`/`fail_exotic_task` 带 `status=1 AND lease_owner=?`(`db/queries/exotic.rs:209/231`),失去租约的迟到结果直接丢弃(计 lease_lost)。
- 回 0 的四种迁移:`invalidate_exotic_tasks_for_item`(源变,`exotic.rs:99`)、`invalidate_exotic_tasks_for_plugin_version`(worker 升级,`exotic.rs:112`)、`recover_orphaned_exotic_tasks`(超 LEASE_TTL=120s 的孤儿租约,`exotic.rs:365`)、`release_exotic_instance_leases`(取消/结束清理,`exotic.rs:380`)。

**主链阶段**(`run_exotic_pipeline_blocking`,`pipeline.rs:171`;阻塞,Coordinator 在 `spawn_blocking` 内调用,`coordinator.rs:440`;调度骨架原理见 [Spec12](./Spec12_配置状态日志.md) §调度):

- **步骤 0 孤儿恢复**:先回收过期租约 processing→pending(`pipeline.rs:179-186`);Coordinator 每次 wake 评估前**也**扫一次(`coordinator.rs:312`),打断「stale processing 使 has_ready 恒假→pipeline 不再启动」的死锁(病历 #2)。
- **步骤 1 Worker 探针**:`factory.spawn()` → 子进程握手(Hello→Ready,校验 worker_id/protocol/capabilities,`worker.rs:147`)→ 取 `worker_version`。spawn 失败则本轮跳过、任务留 pending(`pipeline.rs:188-196`)。
- **步骤 2 版本对账**:把该插件「done 但 worker_version 不同」的任务退回 pending(`invalidate_exotic_tasks_for_plugin_version`,`pipeline.rs:199-208`)——纯升级后旧 done 缩略图即失效。
- **步骤 3 通道与共享态**:有界 task/result 通道(容量 `pool_size * 2`,`pipeline.rs:212-213`)、种子 Worker 槽(复用探针实例免二次 spawn)、熔断闸(`pipeline.rs:210-221`)。
- **步骤 4 四线程并行**(`std::thread::scope`,`pipeline.rs:222-283`):
  1. **Claimer**(`pipeline.rs:301`):让步门控(`should_yield_exotic` `state.rs:728`,对 scan/thumb/interaction 让、**不对 derivation 让**→二者共享公平池同级)→ 派发前授权复核(`is_runnable`,`pipeline.rs:322`,补「运行期授权变化」窗口)→ `claim_exotic_tasks` → `exotic_item_source`(`exotic.rs:528`,JOIN 解绝对路径)→ `thumbnail_fingerprint`(`fingerprint.rs:59`,SHA-256 规范 JSON,复用 `snap_to_tier` `fingerprint.rs:65`)→ 组 `RequestBody::Thumbnail`(含吸附后 `target_long_edge`)→ 入队。空批即结束本轮。
  2. **Worker 池**(`pipeline.rs:406`,MAX_POOL=2):每任务先取 `background_heavy_limiter` permit(`pipeline.rs:420`,与派生共享票号队列)→ 确保活 Worker(种子→`factory.spawn`+500ms 崩溃退避)→ `run_thumbnail`(`supervisor.rs:314`,超时/断开/协议违例即 kill→wait 标死,`supervisor.rs:347`)。
  3. **Writer/Sink**(`pipeline.rs:495`):按 `TaskOutcome` 分派——Success→`finalize_success`;Failure→按 `code.default_retryable()` 分 retryable/terminal;TimedOut/Disconnected→retryable+strike;Protocol→terminal+strike;strike≥5 置熔断停领(`pipeline.rs:584-587`)。
  4. **续租线程**(`pipeline.rs:475`):每 40s 批量刷新本实例在途租约(`renew_all_exotic_leases`,`exotic.rs:200`),防排队中任务被孤儿回收误伤。
- **步骤 5 结束清理**:`release_exotic_instance_leases` 把本实例残留 processing 退 pending(`pipeline.rs:285-293`);返回 `PipelineStats`,`circuit_opened` 时 Coordinator 停止本轮(`coordinator.rs:523`)。

**数据与产物**:

- **DB**:`exotic_tasks`(db/schema/early.rs:484-506:item_id+plugin_id+capability 唯一;`status`/`input_fingerprint`/`attempts`/`next_retry_at`/`claimed_at`/`lease_owner`/`last_error_*`/`output_path`/`worker_version`;索引 `idx_exotic_tasks_ready` 供领取)。
- **成功路径两写同事务**(`finalize_success`,`pipeline.rs:597`):先文件(`write_thumbnail_atomic`,`sink.rs:35`——`.tmp` 同卷写→`sync_all`→原子 rename→Host 独立解码算 thumbhash `sink.rs:71`),后 DB 条件事务 finish task + `update_thumb_result` 回填 `media_items.thumb_status=1`/`thumb_path`/`thumbhash`(`pipeline.rs:608-629`)→ patch layout `items_cache`(`pipeline.rs:634-645`,滚出再滚回立即可见)。**绝不先写 task done 再落文件**。
- **缓存产物**:`{cache_dir}/thumbnails/{tier}/{2-hex前缀}/{cache_key_hex}.webp`(`cache.rs:4-30`),tier 取 `THUMB_TIERS` 吸附值;产物供画廊标准缩略图路由直接复用。
- **跨流水线门控**:`NOT_BLOCKED_BY_EXOTIC`(`exotic.rs:15-24`)让「有未完成 exotic thumbnail 任务」的 item 不进主缩略图 generator 与 CLIP/face 生产者(`db/queries/exotic.rs:888-950` 测试),exotic 完成后各链优先复用其 WebP。
- **Worker 二进制**:已装插件走 `<app_data>/exotic/plugins/<plugin_id>/`,worker 在 `<plugin_id>/bin/<worker>.exe`(host 验签复核;`plugin_install_dir` 只接受已验证 plugin_id,`install.rs:383-390`);RAW 系 builtin 分发,dev 走 `EXOTIC_RAW_WORKER_PATH` 旁路(仅 debug 编译,`installer.rs:358-390`)、Release 走 externalBin sidecar(`installer.rs:404` 起)。

**前端消费链**(纯商店/门控链,无独立画廊渲染链——产物进标准缩略图缓存):

- 页面 `PluginStoreView.vue`:处理进度区(进度条+四桶计数+start/pause/stop+详情分页,模板 `25-116`);`useTauriListen(EVENTS.EXOTIC_STATUS_CHANGED, …)`(`PluginStoreView.vue:484`)→ `refreshStatusThrottled` 500ms 尾随节流(`473-481`)→ `store.loadStatus()`。
- 数据层 `useExoticStore.ts`:`loadStatus`(`:130-132`)→ `get_exotic_processing_status`;控制 start/pause/stop/retry(`:192-211`);注册表/安装/内置能力加载 `loadAll`(`:143`)。
- 门控 `useExoticGate.ts`:`list_exotic_format_resolutions`(`:27`)+ 命中 exotic 格式再 `get_exotic_item_state`(`:84`,避免普通格式空跑);`PluginGate.vue` 按 `Availability` 渲染 purchase/activate/blocked/passthrough(纯展示、不持验签)。
- IPC 常量:`constants/ipc.ts:328-355`(查询/控制 17 条)、`:418-419`(`EXOTIC_STATUS_CHANGED='exotic:status-changed'`);后端事件在 `coordinator.rs:313/500/522` 发出。
- 现状注:进度/详情命令当前硬编码 PSD 插件(`exotic_commands.rs:207/272`),RAW 任务计数未单列。

**并发 / 取消 / 错误**:

- **并发模型**:Coordinator 单调度器 + 有界事件通道(容量 32)+ `dirty` 原子位防丢唤醒(`coordinator.rs:170/238-249`);串行循环天然保证并发 wake 只启动一条 Pipeline;单循环按注册表逐 (plugin, capability) 调度,插件间不互并发(`coordinator.rs:315-328`)。Worker 池子进程化(非 rayon 消费者池),低优先级+隐藏窗口创建(`worker.rs:85-102`)。
- **取消**:`stop_exotic_processing`(`exotic_commands.rs:177`)→ `state.cancel_exotic_analysis`(`state.rs:867`)取消 `CancellationToken`;Claimer 停领、在途 Worker 被 kill(`pipeline.rs:456-458` 丢弃结果不落库)、结束清理把已领未完成退 pending——**取消不烧退避、立即可重领**。
- **错误分级**:数据类错误(unsupported/malformed/resource_limit)terminal 不计 strike;io/internal 走 retryable。退避(`backoff_secs`,`pipeline.rs:679`):IoError `30s×2^attempts` 指数、进程级 1m/5m/30m;`MAX_ATTEMPTS=3`(`pipeline.rs:59`)后转 terminal。熔断:`STRIKE_THRESHOLD=5` 进程/协议级失败累计 → 本轮停领、等用户修复/升级(`coordinator.rs:523`)。
- **关键不变量**:先验签再拉进程(host 侧,`installer.rs:350`);原子领取+租约条件写;文件先于 DB;`is_task_runnable` 门控顺序(平台→Host 版本→安装→授权,`mod.rs:464`,拒领的不写任务行);未安装/未授权/禁用不播种不领取;`lease_owner` 跨实例互斥防踩;`background_heavy_limiter` 与派生共享公平池,exotic 与 derivation 互不让步(`state.rs:725-730` 注释)。

---

### 2.3.10 图片编辑 + ICC

**一句话**:图片简单编辑(几何+调色→同目录另存副本→单文件入库)+ 查看器渲染色域派生(按 target 色域 CMS 投影+嵌入 target ICC)两条**前台交互式**链,共用 `editing::color` 的 moxcms 变换原语与 100MP 级内存预算门;编辑是「前台单发」不走共享 job 骨架,查看器是「按需自愈缓存」。深机制(几何公式/调色公式/ICC 校验链/不变量)以 [Spec04](./Spec04_图像与色彩管线.md) 为权威,本节只写工作流视图。

**触发入口**(全前台交互,无后台 job 自动触发;总表「`edit_*` 命令族」实际为单命令 `save_edited_image` + preview/entitlement 共 5 条,另 viewer_color 4 条):
- 编辑链:
  - `get_editing_entitlement`(`ipc/edit_commands.rs:72`):ContentViewer 打开编辑覆层前查授权态(付费真门,前端 fail-closed,`useEditingEntitlement.ts:28`)。
  - `activate_editing_feature`(`edit_commands.rs:81`):设置页激活(撤销接口 `deactivate_editing_feature`(`:105`)已随 P14 删除),前端只交 token,plugin id/SKU 为后端常量(`editing/entitlement.rs:10-12`)。
  - `get_edit_preview`(`edit_commands.rs:126`):EditOverlay 挂载/打开时拉 E0 预览(orientation 烤入→ICC→sRGB→长边 2048 缩放→raw 二进制包,`editing/preview.rs:57-92`),非 JSON、固定 28B 包头(`preview.rs:31-54`)。
  - `save_edited_image`(`edit_commands.rs:150`):用户点「保存副本」——前台单发,直接跑完整链后返回终态(非 job_id+事件模型)。
- 查看器色域链:
  - `get_viewer_color_url`(`viewer_color_commands.rs:45`):ContentViewer 大图每次挂载/切 item/target 变化时(`useViewerColorSource.ts:40-65`)。
  - `import_icc_profile` / `list_icc_profiles` / `delete_icc_profile`(`viewer_color_commands.rs:145/160/172`):设置页与 `ViewerColorMenu.vue` 的自定义 ICC 管理。

**状态机**:无后端持久 job 状态机——编辑保存是前台单发、查看器派生是 `exists()` 自愈缓存。前端 `EditStatus`(`useImageEditor.ts:30`)是编辑链唯一状态机:`idle → editing`(open `:159`)`→ saving`(save `:294`)`→ done` / `partial`(savedNeedsIndex `:322-327`)/ `error`(`:330-336`);Esc 经 `requestDiscardEdits`(`useImageEditor.ts:388`)仅放弃会话,不发后端取消。

**主链阶段**:

A. 编辑保存链(`run_save_edited_image`,`edit_commands.rs:211`,单 spawn_blocking 内):
1. 授权门 + 参数校验:→ `entitlement::require_editing_entitlement`(`:219`;`entitlement.rs:46`,未授权拒 `edit_not_entitled`)→ `geometry::validate_ops`(`:221`;`geometry.rs:59`,旋转四态/拉直闭区间 ±45°/调色域,解码前拒绝)→ 通过。
2. DB 取源:→ `get_media_detail` + `get_item_path_info` + `get_root_and_volume_for_directory`(`:223-230`)→ detail/rel_path/root_id/volume_id;`is_deleted` 或非 online → `edit_source_unavailable`(`:232-237`);ext 不在 `[jpg,jpeg,png,webp,bmp,tiff,tif]` → `edit_format_unsupported`(`:238-244`)。
3. 解码 + 内存预算门(D-008):→ `ImageReader::open` → `decoder.dimensions` → `memory_budget::exceeds_memory_budget` / `exceeds_memory_budget_after_fine_rotate`(`memory_budget.rs:40/49`,12B/px×2 系数、上限 1.5GB;`edit_commands.rs:261-274`)→ 超限拒 `edit_image_too_large`;**任意大块分配之前**。
4. 源元数据读取:→ `metadata::read_source_metadata`(`metadata.rs:27`)→ orientation/ICC/DateTimeOriginal;`effective_source_orientation`(`metadata.rs:49`,D-009:orientation 仅 JPEG 生效)。
5. 几何链:解码图 → `geometry::apply_geometry`(`geometry.rs:134`)= orientation 一次 → `combine_rotation`(`:121`,合入 DB `view_rotation`+本次 rotate,取模落回四态)→ flip → fine rotate(仅非零进 imageproc `:168`,`maximum_aspect_inscribed_size` 取最大居中内接 `:100`)→ crop(clamp 后零面积拒 `edit_crop_empty`,`:233`)→ 变换后像素图。
6. E3 调色(链序最末,crop 后):→ `adjust::effective_adjust`(`adjust.rs:67`,D-106 全零跳过字节级直通)→ `adjust::apply_adjust`(`adjust.rs:156`):有 ICC 源经 moxcms **f32** 分条(`STRIP_ROWS=64`,`adjust.rs:471`)转 sRGB → 亮度→对比度→饱和度钉死公式 → 按源位深量化;输出嵌显式 sRGB profile(`srgb_profile_bytes`,`adjust.rs:145`),**不回写源 ICC**。
7. 编码 + 原子落盘:→ 格式/质量(Jpeg q 1-100 默认 92 / Png,`edit_commands.rs:297-313`)→ `naming::target_stem`(`:322`,`{stem}-edit`)→ `naming::claim_target_path`(`:323`;`naming.rs:40`,create_new 独占认领,TOCTOU 安全)→ `io::write_edited_image`(`io.rs:95`,编码→同目录 `*.tmp`→磁盘复读验证 `verify_round_trip`(`io.rs:79`)→sync_all→同卷 rename;任一步失败清理 tmp+占位不发布半成品)→ 落盘新文件。
8. 单文件 ingest:→ `ingest::ingest_single_file`(`ingest.rs:33`,跳过目录链递归与 exotic 播种;`upsert_fast_scan_item` 建新行,`view_rotation` 默认 0)→ `Saved { new_item_id }`(`edit_commands.rs:369`)。
9. 成功收尾:→ `bump_data_version`(`:185`)+ `tauri::async_runtime::spawn` fire-and-forget 单根 `scanner::enricher::run_enrichment`(`:188-203`,不阻塞命令返回)。

B. 查看器色域派生链(`get_viewer_color_url` → `viewer_color::render::ensure_derivative`,`render.rs:51`):
1. 平台/配置判定:移动端恒 `None`(`viewer_color_commands.rs:49-52`);`ViewerColorTarget::from_config`(`target.rs:44`,config 单源两键,前端不传 target)对 srgb/未选 custom 返 `None` = 直显原图零派生。
2. 并发去重:同 `(item_id, target_id)` 并发请求持 keyed tokio lock(`:66-83`;`state.rs:926-947`),只渲染一次。
3. 渲染主体(`render.rs:51-136`):卷离线守门 `get_item_volume_offline_label`(`:97-99`)→ 非 image/已删/不在线/格式非白名单优雅 `None`(`:103-112`)→ 命中检查 `hit_path`(`:61`,同 target_id+cache_key 的 jpg/png 任一存在即返)→ 动画 WebP 前置拒 `viewer_render_unsupported`(`:66-71`)→ `resolve_profile` 装配(CMS 对象, 嵌入字节)同源 D-412(`target.rs:69-109`)→ 解码+内存预算门(`:94-100`)→ orientation 烤入(`:107`,D-009)→ `color::to_target_rgba8` 投影(`editing/color.rs:40`,无 ICC 假定 sRGB 仍转换)→ 按 alpha 定格式(有→PNG 无损;无→JPEG q92,`render.rs:122-127`)嵌 target ICC → `write_atomic` 原子落盘(`:133`)→ 派生文件绝对路径。

**数据与产物**:
- DB `media_items`:编辑输出恒产生**新行**(`ingest.rs:33-49`),`media_type='image'`、`view_rotation=0`、`cache_key=xxh3(path,name,mtime)`、`width/height` 取输出尺寸、`sort_datetime=file_mtime`;与源 item 无 DB 级版本关联(编辑版本追踪不在数据库契约)。编辑链**不写 `image_meta`**(源元数据只读不落库)。
- 产物文件:编辑输出 `<源目录>/{stem}-edit.{ext}`(jpg/png,冲突 `-edit-2` 递增),tmp→rename 原子提交;ICC 策略=有调色嵌 sRGB profile、无调色透传源 ICC 字节;EXIF 为最小新构造(Orientation 固定 1 + 可选 DateTimeOriginal,`metadata.rs:92`)。
- 查看器派生缓存:`cache/viewer_color/{target_id}/{prefix}/{hex}.{ext}`(`thumbnail/cache.rs:151-168`),与缩略图共用 10GB 单源 LRU 记账;**不经 `media_derivations` 记行**,命中只查 `exists()`,mtime 变→cache_key 变→旧派生成孤儿,归 `reconcile_orphan_gc` 回收(`render.rs:6-7`)。
- 自定义 ICC:`{app_data}/config/icc/{16hex}.icc`(`target.rs:113-120`),id=原字节 xxh3(`:37`),≤16MB(`viewer_color/mod.rs:40`),同字节重复导入幂等。
- 配置键:`viewer_color_target`(enum srgb/display-p3/dci-p3/custom,默认 srgb,`config/schema.rs:442-448`)+ `viewer_color_custom_id`(`config/schema.rs:452-458`),均由设置页写、后端读。

**前端消费链**:
- 编辑:`ContentViewer.vue:589-594`(useImageEditor + useEditingEntitlement 单实例,`viewerStore.ts:60` 注册 `api.edit`)→ `EditOverlay.vue` 呈现(预览走 `useEditPreview.ts:91` 拉 `get_edit_preview` 二进制包解析 `parseEditPreviewPacket` `:25-60`,调色预览走 `useAdjustPreview.ts:83` canvas 逐像素双端同源公式)→ 保存 `useImageEditor.save`(`:293`)经 `utils/ipc.ts` `invokeIpc`(`:86`)调 `IPC.SAVE_EDITED_IMAGE`(`constants/ipc.ts:408`)→ 终态直接回填;`savedNeedsIndex` partial 态提示「文件已保存,索引失败」+ 立即扫描按钮(`:322-327`);错误码→i18n 分流(`messageForCode`,`useImageEditor.ts:62`)。保存成功由 `data_version` bump 经画廊重排回显(`edit_commands.rs:185`)。
- 查看器色域:`ContentViewer.vue:605-628`(`useViewerColorSource` → `absPath` 优先派生 URL,原图先显+完成后换源,`handleDisplayUrlError` 失败回落原图)→ `invokeIpc GET_VIEWER_COLOR_URL`(`constants/ipc.ts:68`);`SettingsView.vue:110` + `ViewerColorMenu.vue` 管理 target 档位与自定义 ICC(import/list/delete,`constants/ipc.ts:70-72`)。无独立事件回显——换源结果即 IPC 返回值。

**并发 / 取消 / 错误**:
- 本流水线**不走共享 producer→crossbeam→consumer→writer 骨架**(见 [Spec12](./Spec12_配置状态日志.md) §调度)——两条链都是前台交互单发:编辑保存单 `spawn_blocking`(`edit_commands.rs:171`)+ `FILE_JOB_EDIT` 门闩与 backup/restore/export 互斥(严格 claim-if-free,`state.rs:577-599`;冲突返共用码 `file_job_busy`),RAII `FileJobReleaseGuard` 保证 `?` 提前返回也释放(`:138-147`)。**无后端取消 API**:编码耗时至多秒级(方案 §5 基准),前端 Esc 只弃会话。查看器链同单发 `spawn_blocking`(`viewer_color_commands.rs:73`)+ keyed lock 去重;无取消(自愈缓存,命中即返)。
- 错误:编辑域 `AppError::Edit` 稳定码(edit_decode_failed / invalid_ops / crop_empty / image_too_large / encode_failed / io / target_conflict / source_unavailable / format_unsupported / not_entitled,`edit_commands.rs:19-27`);`edit_saved_needs_index` 是**成功**响应的负载字段不走错误路径(E6 授权门实现为 `require_editing_entitlement`,`entitlement.rs:46`,绕过前端 invoke 也无法使用)。色域域 `AppError::Color`(`viewer_color/mod.rs:24-37`),非致命判定一律优雅 `None` 回退直显原图、不落错误码。
- 关键不变量:D-008 内存预算门三处独立(保存/预览/查看器,`memory_budget.rs:24-59`)、D-009 orientation 仅 JPEG(`metadata.rs:49`)、D-106 全零调色跳过保字节直通、D-412 像素域==嵌入 profile、D-413 无 ICC 短路不经 CMS 数值管线、落盘 tmp→rename 原子、命名 create_new TOCTOU 安全、viewer_color 孤儿 GC 自愈。总表两处措辞勘误:触发入口为单命令 `save_edited_image`(另有 preview/entitlement 共 5 条),「ingest 原图」应为「取库内源 item」——ingest 只注册输出新文件,不注册源图。

---

### 2.3.11 文档阅读

**一句话**:点击文档项即开卷的按需读取链——txt/md 经编码 seam→分章/分片→foliate-js 统一渲染,epub 走 foliate 原生探测,pdf 独立 pdf.js 渲染,阅读产物(进度/书签/每书偏好/版本)全部落 DB,远程 LLM 校对为旁路;非批处理,不走共享调度骨架。

**触发入口**(全为单发 IPC,无后台 kick):
- 路由 `/doc/:id`(`src/router/index.ts:70`,路由级懒加载)→ `DocumentViewer.vue`;来源:画廊/收藏/搜索/收藏夹任一处点击文档项。
- txt 开卷:`BookReader.resolveOpenTarget`(`src/components/doc/BookReader.vue:453`)按 `textSource` 分支——txt 先 `GET_TEXT_BOOK_INDEX`(`BookReader.vue:482`)取章索引,再经 SyntheticBook 按章懒拉 `GET_TEXT_CHAPTER`(`BookReader.vue:485-490`)。
- md 开卷:`GET_DOCUMENT_TEXT` 取生效全文(`BookReader.vue:460`)。
- epub 开卷:`url`(convertFileSrc)直接交 foliate `makeBook` 自动探测(`BookReader.vue:501`)。
- pdf 开卷:`PdfReader.vue:103` 内 pdf.js `getDocument`。
- 每书偏好读/写:`GET/SET_READER_BOOK_PREFS`(装载 `DocumentViewer.vue:1016`;保存 `useReaderBookPrefs.ts:88`)。
- 阅读进度:relocate 事件去抖 1.2s 调 `SET_READING_PROGRESS`(`useReaderProgress.ts:25-29`);开卷 `GET_READING_PROGRESS`(`DocumentViewer.vue:993`)。
- 书签:`LIST/ADD/DELETE_READER_BOOKMARK`(`useReaderBookmarks.ts:27/51/68`)。
- 校对:`proofread_chunk`(`ProofreadPanel.vue:189`)、`get/set_proofread_config`、`set/clear_proofread_key`(`src-tauri/src/ipc/proofread_commands.rs`)。
- 文档缩略图回环(pdf/svg):`ensure_doc_thumb_queue`(`doc_commands.rs:729`)+ `store_doc_thumbnail`(`doc_commands.rs:770`);epub 封面+spine 走派生链 `derive/doc.rs:28`。

**状态机**:无独立状态机——本链全为按需无状态单发,`text_book_index` 只是指纹缓存。唯一有状态的部分是 doc_thumb 派生 0/1/2/3 三态(走共享骨架,见 [Spec03](./Spec03_缩略图与派生.md) 与 [Spec12](./Spec12_配置状态日志.md) §调度)。前端有一瞬态护栏位 `forcedScrolled`(`BookReader.vue:167`):检出超限单片置位 → 该书流切换禁用。

**主链阶段**:
1. **开卷装配**:`/doc/:id` → `DocumentViewer.load()`(`DocumentViewer.vue:969`)→ `GET_MEDIA_DETAIL` → `kind` 分流 pdf/epub/text/unsupported(`DocumentViewer.vue:793`)→ 按 `:key=readerKey` 挂载 BookReader/PdfReader(`DocumentViewer.vue:780`)。输入 item_id → 输出渲染器实例 + `initialPos`(已存进度)。
2. **txt 章索引**:`get_text_book_index`(`doc_commands.rs:589`)→ `resolve_encoding_override`(514:显式参数→每书 prefs.encoding→None)→ `effective_text_ref`(465:当前版本/源 + `src_key` 指纹)→ `ensure_text_index`(531:缓存命中即返;未命中整读文件 + `reader::text_index::build_index`(`text_index.rs:152`))。build_index:①`decode_bytes`(`encoding.rs:69`)四阶编码 seam(手动覆盖→BOM→UTF-16 零字节启发 `sniff_utf16_no_bom`:144→chardetng `sniff_chardetng`:126,`last=!truncated`);②`source_line_starts`(195)行首字节偏移;③`select_rule`(225)选规则 → `split_by_rule`(278)/巨章续切 `push_chapter_capped`(319,30K 上限)/`pseudo_chapters`(368,10K 伪章兜底)。输出 `TextBookIndexDto{encoding, confidence, chapters[{title, char_len}]}`,字节偏移不出 Rust,chapters 落 `text_book_index`。
3. **txt 单章内容**:`get_text_chapter`(`doc_commands.rs:621`)→ 复用索引 → `read_byte_range`(572)seek 章字节区间 → `enc.decode` → `paragraph::strip_title_line`(88,标题按行预剥离)→ `paragraph::segment(body, reflow)`(32,一级保守/二级重排)。输出 `TextChapterContent{title, paragraphs}`;IPC 载荷恒单章级,不整读大文件。
4. **md 链**(全在前端):`GET_DOCUMENT_TEXT`(`doc_commands.rs:151`,经 `read_ref_text`:110 读当前版本或源字节再解码)→ `renderMarkdownBlocks` → 含代码块则 shiki 懒载逐块高亮(`BookReader.vue:466-471`)→ `buildMarkdownSyntheticBook`(`syntheticBook.ts:265`)→ `groupMarkdownBlocks`(228,h1/h2 起新片 + `MD_SECTION_BUDGET_CHARS=24_000` 预算,单块超限独占一片不内切)。
5. **epub 链**:URL 交 foliate `makeBook` 自动探测(`BookReader.vue:501`);`denyScriptResources` 拒加载 blob 脚本(`BookReader.vue:562`);封面+spine 页数由派生链 `derive/doc.rs:28` 抽(container.xml→OPF→cover href 三级回退 + `<itemref>` 计数)。
6. **pdf 链**:pdf.js 独立渲染(`PdfReader.vue:103`,`isEvalSupported:false` 配合 CSP 删 unsafe-eval);按页懒渲染 `IntersectionObserver`(600px rootMargin,`PdfReader.vue:126-137`),`MAX_CANVAS_EDGE=2200`(38);位置格式 `"page:N"`。
7. **统一渲染**(BookReader,foliate-js vendored):`resolveOpenTarget`(453)→ `view.open` → 每章 `onLoad`(369):竖排 `writing-mode` 落到文档根(386-387,须在 foliate `getDirection` 读取前,轴对换全生效)→ 替换规则 `applyReplacerToDom`(388)→ 简繁 `applyZhConvertToDom`(399,经 `CONVERT_CHINESE`,渲染层变换)→ `onRelocate`(408)上报 `cfi:` 进度 + fraction。位置恢复 `restorePosition`(509):CFI 解析失败回退无位置初始化(517-521)。
8. **阅读产物写库**:进度 `set_reading_progress`(去抖 1.2s);书签 `add_reader_bookmark`(`UNIQUE(item_id,locator)` 幂等,`schema/mid.rs:159`);每书偏好 `set_reader_book_prefs`(JSON diff);编辑/校对接受后 `save_version`(`doc_commands.rs:228`,单事务 `write_version_tx`:191,文件写先于 commit)。
9. **远程校对旁路**:`ProofreadPanel` 分块 `chunkText`(125)→ 逐块 `proofread_chunk`(`proofread_commands.rs:104`,key 取 keyring)→ `proofread_remote`(`proofread/mod.rs:33`)POST `/chat/completions`(连接 15s/整体 300s 超时,61-67)→ `DIFF_TEXTS` track-changes 预览(`ProofreadPanel.vue:193`)→ 接受后 `SAVE_VERSION`(source=ai-remote)。

**数据与产物**:
- `text_book_index`(`schema/mid.rs:124`):item_id PK、src_key、encoding、confidence、chapters JSON `[{t,s,e,n}]`;写入时机=索引未命中重建时一次性落库,src_key(mtime+size/版本 id+编码覆盖)变化即失效重建。
- `reader_book_prefs`(`schema/mid.rs:139`):item_id PK + prefs 版本化 JSON diff(encoding/reflow/zhConvert/theme)。
- `reader_bookmarks`(`schema/mid.rs:151`):item_id+locator UNIQUE,同位置刷新标签/进度/时间不产生重复行。
- `reading_progress`(`schema/early.rs:293`):item_id PK + position(现 `cfi:`;旧 `scroll:`/`page:` 兼容恢复)。
- `document_versions`(`schema/early.rs:348`)、`doc_replacements`(`schema/early.rs:333`)、`document_meta`(`schema/early.rs:172`,page_count 由 epub spine / pdf numPages 回填):编辑/版本侧。
- 无独立缓存目录:章索引/单章/产物均落 DB;版本快照在 `<appData>/documents/<item_id>/`(`doc_commands.rs:96`);txt 章 XHTML 的 blob URL 是**运行时瞬态**(`syntheticBook.ts:112-127`,unload/destroy 即 revoke,不入盘)。

**前端消费链**:页面 `DocumentViewer.vue`(`/doc/:id`)→ **无独立 Pinia store**(局部 ref + 12 个 composable 装配;`viewerStore` 仅挂 `ViewerApi` 供顶栏命令)→ `src/utils/ipc.ts` `invokeIpc`(86)/`invokeIpcRaw`(101)常量强制 + `IpcError` 结构化(58)→ IPC 常量 `src/constants/ipc.ts:150-181` → 后端命令。事件回显:本链无进度事件流(命令同步返回);唯一事件是 `store_doc_thumbnail` 落盘后 emit `db:media_enriched` 触发画廊防抖刷新(`doc_commands.rs:903-910`);其余刷新靠前端变更后重拉(如 `useReaderBookmarks.loadBookmarks`:25)。

**并发 / 取消 / 错误**:
- **并发模型**:**不走共享调度骨架**(见 [Spec12](./Spec12_配置状态日志.md) §调度,仅 doc_thumb 缩略图一环例外)——每条命令独立 `spawn_blocking` 单任务(读文件+解码+正则,如 `doc_commands.rs:155/597/631`),无 crossbeam 池/无 RunTokenSlot/无 CancellationToken;`SpanTimer` info 档埋点(`doc_commands.rs:595/629`)。多章并发靠 foliate 懒加载天然滑窗,无后端池。
- **取消路径**:无后端取消;前端用 generation 计数器(`DocumentViewer.vue:963-967` documentLoadGeneration + isCurrentDocumentLoad)丢弃陈旧响应——换文档/退出时慢请求结果被忽略,`onBeforeUnmount` 先 `flushProgress()` 再拆监听(`DocumentViewer.vue:1128-1133`)。
- **错误处理**:统一 `AppError`/`IpcError` 稳定 code(`utils/ipc.ts:58`);损坏 CFI 回退首章不让整书打不开(`BookReader.vue:517-521`);编码误判(替换率>0.5%)confidence=lossy + UI 提示可手动覆盖(`encoding.rs:19,99-104`);校对 base_url 非 http(s)/未配 key 返 `AppError::System`(`proofread_commands.rs:110-118`);空 body 文档缩略图标派生 status=3 不再重试(`doc_commands.rs:811-828`)。
- **关键不变量**:①章界永在行首,字节偏移对齐(`text_index.rs:194-222`);②单章 seek 绝不整读全文(`doc_commands.rs:571-583`);③分片预算 24K + 护栏 256K(`syntheticBook.ts:177/187`),新格式接 foliate 必须分片;④简繁只作用渲染层不改 canonical(`zh_convert.rs` 模块头注);⑤`write_version_tx` 文件写先于 DB commit(`doc_commands.rs:191-218`);⑥`document_storage_guard` 只在 spawn_blocking 闭包内持有(`doc_commands.rs:256`);⑦chardetng 截断采样必须 `last=!truncated`(`encoding.rs:130-137`)。

**总表纠正**(对 §2.1 第 11 行):
- 编码 seam 实修**四**链路(阅读/编辑/版本 diff/AI 校对),权威表述 `doc_commands.rs:106-109`;`encoding.rs:6` 的「三链路」注释为陈旧说法。
- 并发列「loc1 定位器」**未接线**:`src/utils/readerLocator.ts` 的 loc1 编解码与三级恢复算法(章内偏移+上下文三元组 `t{b,h,a}`+4096 重锚窗)已落地并单测,但实时阅读路径仍用 `cfi:` 前缀(`BookReader.vue:412/446`),loc1 落地后方可换存 `loc1:<json>`(列已预留,`schema/mid.rs:148-149`)。

---

### 2.3.12 备份恢复 + 导出

**一句话**:备份恢复+导出是 Scrollery 的「数据出厂」双通道——备份把 catalog DB + `documents/**` 打成自校验一致快照包(手动/自动),恢复经 validate→stage→arm→restart→swap 五阶段把包换入活库;导出把选区/相册/视图的有序媒体集合原样复制到用户目录(可选 manifest)。三者共享 A/B 文件任务门闩 `file_job_owner` + 各自 `RunTokenSlot`,备份与 arm 全程持 `document_storage_guard` write guard。深机制详见 [Spec08](./Spec08_存储备份导出文件操作.md),本节只给工作流视角。

**触发入口**
- 备份(4 命令 + 1 调度器,`ipc/backup_commands.rs`):
  - `preflight_backup`(:203):设置页备份分节预检,返回目的地可写 / 同卷警告 / 文档一致性 / 估算体积(`estimate_backup_bytes`:153)。
  - `start_backup`(:234):手动「立即备份」按钮;经共享启动核 `begin_backup`(:254) 取得门闩+token、落 running 快照。
  - `stop_backup`(:463) / `backup_status`(:80):取消 / webview 重载恢复快照。
  - `list_backups`(:470):设置页备份包列表(逐包读 manifest,`read_backup_entry`:503)。
  - 自动备份:`start_auto_backup_scheduler`(:409) 启动 2min 首检 + 每小时复检,`maybe_run_auto_backup`(:353) 依 `should_run_auto_backup`(:336) 判据(开关+已设目录+距上次成功 ≥24h,`backup_last_success_at` 状态键走 DB)调 `begin_backup(kind=Auto, retention)`。
- 恢复(`ipc/backup_commands.rs`):`restore_stage`(:538) 用户选包→校验暂存(不动活库);`restore_arm`(:569) 双确认后生成回滚包+写 marker;`relaunch_app`(:639) 触发重启,启动期 `perform_swap_at_boot`(`db/boot.rs:45`) 完成交换。
- 导出(`ipc/export_commands.rs`):`preflight_export`(:189) / `start_export`(:264) / `stop_export`(:441) / `export_status`(:113);三入口(选区工具条、相册工具栏、视图工具栏)统一经 `exportStore.openExportDialog` 唤出。

**状态机**
- 任务终态机(idle → running → {completed | failed | cancelled}):备份是单次粗粒度(running→终态,`BackupProgressPayload`);导出分块,快照含 `processed/total` 支持进度条(`export_commands.rs:49-69`)。终态由 `finalize_payload` 映射,`cancelled` 仅当码==`CODE_CANCELLED`(`export_commands.rs:390-394`)。
- 恢复交换相位机(`swap.rs:42` `RestorePhase`,持久化于 `pending-restore.json`,`swap.rs:52`):`Prepared → CurrentMoved → Installed → Verified`。相位推进条件:`Prepared` 暂存库缺失→先 `restore_current_from_old` 再删 marker 中止(§#6);否则移活库入 old→写 CurrentMoved→装暂存→写 Installed。`CurrentMoved` 活库/暂存在位→**无条件**重跑 `install_staging_to_live`(补装 documents,§#2)→Installed;否则逆向从 old 恢复。`Installed` 待 verify;`Verified` 清理前崩溃则续清。换入库迁移失败走回滚分支(`db/boot.rs:61-104`,`rollback_restore_at_boot`:swap.rs:247)。

**主链阶段**
备份(输入:活库+documents → 输出:`Scrollery-{manual|auto}-{时间戳}.scrollerybackup`):
1. 门闩+guard:`try_acquire_file_job(FILE_JOB_BACKUP)`(`backup_commands.rs:262`)+ `document_storage_guard.write()`(:285-288,须 blocking 上下文、不跨 await),`tauri::async_runtime::spawn` 分离任务内 `spawn_blocking` 跑 `run_backup`。
2. preflight:`ensure_dest_writable`(`core.rs:223`)写-删探针。
3. VACUUM INTO 一致快照:`vacuum_into`(`core.rs:239`)独立连接+同生产 PRAGMA+自定义 collation 注册,`VACUUM INTO ?1` 参数绑定(:249-250);暂存写 **app_data_dir 本地卷**,仅最终 rename 须与正式包同卷(审查 #15)。
4. 快照校验:`integrity_ok`/`read_counts`/`read_roots`/`read_external_count`(`dbread.rs:13-53`),schema_version 运行时读。
5. 文档一致性校验:`validate_appdata_documents`(`core.rs:277`)逐行门(路径非空/文件存在/canonicalize 后位于 `documents/` 下),content_hash 比对**下沉**打包单遍读。
6. zip 流式打包:`write_package`(`core.rs:358`)db→documents→**manifest 最后写**;`add_file_streaming`(:414)单遍读同算 SHA-256(入 manifest)+xxh3(比对 content_hash,§#14 融合校验),`large_file(true)` ZIP64,收尾 fsync。
7. `.tmp` 同卷 rename(:205),此后视为已提交。
8. retention:自动包按 `apply_auto_retention`(`core.rs:460`)仅删 `Scrollery-auto-` 前缀+`verify_is_auto_package`(:486) 验证通过的包,留最新 keep 份;手动包/陌生文件不碰。
9. 终态发布(审查 #13 顺序):**先落终态快照(`is_current` 判发布权)→ `backup_token.finish` → 清 cancelling → 门闩最后释放**(:321-326);自动备份成功后回写 `backup_last_success_at`(:303-308)。

恢复(输入:不可信备份包 → 输出:活库换入 + 原始现场进 `restore-old/`):
1. validate→stage:`restore_stage`(`restore.rs:112`)读 manifest(64MiB 封顶,:170)→ `is_safe_backup_id`(:71 单段/拒穿越)→ `scan_central_directory`(:186 白名单+zip-slip/符号链接拒+checked 尺寸 4M 条目/1TiB+可用空间余量)→ `extract_and_verify`(:256 `safe_join`+`create_new`+边写边核 size/sha256)→ `validate_and_migrate_staged_db`(:339 quick_check/fk_check/schema 门,老版迁移,新于 runtime 拒)→ `rebase_appdata_documents`(:379 只取 basename 跨机、绑定参数 UPDATE `abs_path`=:429、与包内解压交叉核对)。
2. arm:`arm_restore`(`swap.rs:274`)先生成 pre-restore 回滚包(`run_backup` → `restore-rollback/`),**成功后才写** marker(Prepared);回滚包失败绝不 arm(`restore_rollback_failed`)。
3. restart:`relaunch_app`(`backup_commands.rs:639`)。
4. swap:`perform_swap_at_boot`(`db/boot.rs:45`,先于写连接/读池创建)按相位+文件存在性双判推进/逆向,幂等重入。
5. verify/收尾:交换完成后按当前格式打开并验证活库(`schema::initialize_schema`,当前 format 34;P23 裁决只接受当前格式快照,不再迁移升级旧库)→`finalize_restore_verified`(boot.rs:110;swap.rs:226)写 Verified+清 old/staging/marker,**回滚包保留 ≥7 天**。

导出(输入:选区/相册/视图 id 集 → 输出:正式目录):
1. 选区解析:`resolve_export_ids`(`export_commands.rs:152`)经内部 `queries::resolve_selection`+`current_layout_version`(IPC 壳已随 P14 删除,内部函数保留:export/media 仍用);`fetch_export_meta`(:290 分块查询,include_relations 取 tags/albums)。
2. 目的地:async 包 `ensure_target_writable`(:180;`core.rs:90` canonicalize+写删探针)、`is_inside_library`(:161;`core.rs:122` 组件级比较);库内需 `allow_inside_library` 二次确认否则 `export_target_inside_library`。
3. staging 逐项复制:`run_export`(`core.rs:168`)staging=`.scrollery-export-{job_id}.tmp`;每项 `build_file_name`(`naming.rs:76` 命名档 original/sequence/date,位宽 `max(3,total位数)`)+ `resolve_conflict`(:101 大小写不敏感,rename 在 stem/ext 间插 `-2`)→路径长>240 跳过→`fs::copy`→`filetime` 回写 DB `file_mtime`→rename。**tmp 名用循环下标脱钩**(:258,审查 P2);离线/缺失记 `source_missing` continue;`StorageFull` 任务级失败清 staging。
4. manifest 原子写:include_manifest 时 `Manifest::write_into`(`manifest.rs:69`).tmp→rename,只记成功项,不写绝对路径/凭据/人脸。
5. 整目录 rename:`core.rs:328` 终局目录名撞名追加 `-2/-3`(审查 P1)。

**数据与产物**
- 备份包:`Scrollery-{manual|auto}-{YYYYMMDD-HHmmss}.scrollerybackup`,zip(Deflated+ZIP64),包内 `db/scrollery.db` + `documents/{item_id}/{file}` + `manifest.json`(最后写)。manifest 逐条 payload 含未压缩 SHA-256 与 bytes;`format_version/schema_version/counts/roots/external_document_versions` 自描述;不写凭据。所有读 manifest 路径封顶 `MAX_MANIFEST_BYTES=64MiB`(`manifest.rs:18`)。
- 恢复中间态(均在 `<app_data>/`):`restore-staging/{backupId}/` 暂存、`restore-old/{backupId}/` 旧活库现场、`restore-rollback/` 回滚包、`pending-restore.json` marker;`backup_id` 恒过 `is_safe_backup_id`。
- 导出产物:`Scrollery-export-{timestamp}` 目录 + `manifest.scrollery.json`(`source` 结构化枚举 Selection|Album|View,`manifest.rs:18`)。
- DB:备份/导出**不新增专属表**;备份只读 `document_versions`(storage='appdata'),导出读 `media_items` 派生 `ExportItemMeta`;`backup_last_success_at` 状态类键走 DB,`backup_dir/auto_enabled/retention` 设置类键走 ConfigManager(`backup_commands.rs:170`)。

**前端消费链**
- 备份:设置页 `BackupSection.vue:161` + `RestoreWizard.vue:156` → `backupStore.ts`(`preflightBackup`:104 / `startBackup`:108 / `stopBackup`:113 / `listBackups`:118 / `restoreStage`:122 / `restoreArm`:126 / `relaunchApp`:133)→ `src/utils/ipc.ts` `invokeIpc`(:86,错误经 `parseAppError`:58 结构化)→ 命令常量 `constants/ipc.ts`(:380-396)→ 事件 `EVENTS.BACKUP_PROGRESS`(:429) 回显 `applyProgress`(`backupStore.ts:75`,快照禁止 terminal→running 回退);全局指示器 `BackgroundFileJobIndicator.vue:11`。
- 导出:`ExportDialog.vue:18`(全局单例,`App.vue:100` 挂载)+ 三入口 `useGallerySelectionOps.ts:59`(右键/选区工具条)、`GalleryViewControls.vue:155`(视图工具栏)、`CollectionsView.vue:182`(相册工具栏)→ `exportStore.openExportDialog`(:77,三入口各自持 rebuild 回调,ViewStale 重试不借用选区)→ `preflightExport`:140 / `startExport`:144 / `stopExport`:149 / `restoreExportProgress`:134→ `constants/ipc.ts`(:372-378)→ 事件 `EVENTS.EXPORT_PROGRESS`(:427) 回显 `applyProgress`(`exportStore.ts:110`,jobId 匹配+终态不可被迟到 running 快照回退)。`fileJobStore.ts:21-22` 聚合两者 running 态。

**并发 / 取消 / 错误**
- 并发模型:**不走** Spec12 生产者→crossbeam→消费者池共享骨架,备份/恢复/导出均为**单飞文件任务**:`file_job_owner` 严格 claim-if-free(`state.rs:577`,审查 #9 禁同名可重入),同刻仅允许 backup/restore/export/edit 之一,占用返 `file_job_busy` 不抢占不排队。执行体在分离任务(webview 关闭也跑到底),门闩在收尾 finally 式释放(`backup_commands.rs:280-327`);全部重 IO 下沉 `spawn_blocking`。文档写与备份/arm 互斥:文档写路径(`save_version`:doc_commands.rs:245 / `delete_version`:330)持 read guard,备份/arm 持 write guard(`state.rs:201`)。
- 取消:备份/导出走 `CancellationToken`,检查点各在关键节点(备份每文档、导出每项复制前后);`cancel_backup`/`cancel_export`(`state.rs:603,619`)先置「取消中」标志再 take 空 token,`backup_status`/`export_status` 据此不误报 failed(:91,:121,审查 #13)。取消清 tmp/工作目录/本 job staging,不碰已落名正式产物;恢复无显式取消,靠 swap 各相位幂等重入收敛。`stop_export` 仅 `job_id`+`status=="running"` 匹配才取消(`export_commands.rs:434`,审查 P1)。`RunTokenSlot` 采「finish 返回值门控终态发布」姿态(非 AI/face 的 !is_cancelled,详见 [Spec12](./Spec12_配置状态日志.md) §2.4)。
- 错误:稳定码 `backup_*`/`restore_*`/`export_*`/`file_job_busy` 透到 IPC `code`;message 固定文案,不携带绝对路径/SQL/内部串(`core.rs:34-50`)。备份失败/取消必清 `*.tmp` 不留半截包(:196-202);导出单项失败记 `item_*` 结果码(`ITEM_RESULT_CAP=100` 封顶,`export/core.rs:30`),不阻断整批。
- 关键不变量(权威清单见 [Spec08](./Spec08_存储备份导出文件操作.md) §4.2):`run_backup`/`arm_restore` 须在 write guard 下;`perform_swap_at_boot` 先于 DB 连接创建;`CurrentMoved` 无条件补装 documents;`Prepared` 暂存缺失先逆向再删 marker;manifest 全部文件落地后最后写 + tmp→rename;导出 tmp 名与最终名脱钩;恢复包当不可信输入(白名单+zip-slip+符号链接+checked 尺寸三层防线)。

---

### 2.3.13 布局/渲染

**一句话**:画廊布局流水线:把「当前视图集合 + 分组排序 + 几何参数」算成带绝对坐标的布局行(justified 等高 / grid 宫格),常驻内存 + O(1) 索引,供前端 bucket 虚拟滚动按段拉取渲染;取数与几何分离(双缓存),轴切换内存派生免 SQL,写路径全部在上游流水线。

**触发入口**(全部经 `ipc/registry.rs:33-40` 注册):
- `compute_layout`(`ipc/layout_commands.rs:162`)——前端几何/筛选/排序/模式/宽度任一变化的重算(watch 面见 `useJustifiedLayout.ts:182-213`),以及 `MEDIA_ENRICHED` 防抖 2s / `VOLUMES_CHANGED` / `totalItems`(扫描完成)/`layoutDirty` 等 Tauri 事件(`useGalleryTauriSync.ts:42/56/68/79`)。
- `get_bucket_rows`(`layout_commands.rs:576`)——bucket 段挂载取行,**主画廊唯一滚动取行引擎**;`get_layout_rows_by_y`(`:547`)——按 y 区间取行,由 MinimapAxis 分块缩略图消费(不作主画廊回退);`get_layout_rows`(`:530` 按行号切片)已随 P14 删除。
- `get_view_ids`(`:511`)——选区(Shift 跨视口范围/全选/框选)取布局序全集 id(`useViewIds.ts:36`)。
- `get_item_y_by_id`(`:594`)——整体重排后视口锚定回看项(`useReflowAnchor.ts:72`)。
- `get_subtree_scroll_target`(`:615`)——侧栏点文件夹滚动定位(空父文件夹落到首个有媒体后代,`useGalleryScrollToDir.ts:33`)。
- `get_adjacent_media`(`ipc/media_commands.rs:108`,内调 `get_adjacent_item`)——查看器上一张/下一张。
- `get_separator_y_by_group_id`(`:582`)——按 group_id 查分隔符 y(时间轴跳转契约位)已随 P14 删除,当前无对应 IPC 命令。

**状态机**:无独立状态机——本流水线**不走** Spec12 §调度的「生产者→crossbeam→消费者池→写入器」骨架,而是「按需计算 + 双缓存」交互模型(差异见并发节)。等价状态由两把缓存的驻留内容 + 两个版本号表达:`data_version`(`state.rs:392`,全局数据代,任何写路径 bump)与 `layout_version`(进程级原子计数器 `cache.rs:16`,每次换代 +1):「空(无布局)→ 可复用命中(reusable=true)→ 仅载荷源(reusable=false,视图敏感写/ai 视图)→ 数据代失配(下次 compute MISS 重建)」。迁移全靠前端 watcher + `layoutVersion` 换代驱动,无后台常驻。

**主链阶段**:

1. **取数(HIT/MISS 二分支)**。输入:`MediaFilter`(canonical JSON = `filter_key`)+ `data_version` + group_by/sort_within/sort_order。先做 S3.1 幂等去重预检(`layout_commands.rs:200-241`):`gen_key` = filter+参数+dv 指纹,与现行布局代相同则 `dedup_summary`(`cache.rs:396`)直接复用摘要、**免重排免换代**(前端挂载/统计返回/尺寸观察的重复触发,版本不变则 bucket 段表零虚假重建)。HIT 分支(`layout_commands.rs:286-345`):items 读锁内 `derive_order`(`items_cache.rs:320`)内存派生请求轴序 + `median_measured_aspect`(`geometry.rs:363`,OnceLock 惰性缓存)→ 免 SQL;判据 `is_hit_valid`(`items_cache.rs:220`)与 HIT 守卫**逐字同判**。MISS 分支(`:347`起):`spawn_blocking`(`:244`)内 `query_layout_items_canonical`(`item_queries.rs:146`,datetime 基准 `sort_datetime DESC,id DESC`,免 directories JOIN,广域视图 `+` 一元压制 partial index 改顺序全表扫,`item_queries.rs:132-137`)+ `query_dir_labels`(`db/queries/layout/facets.rs:61`);读连接仅在查询期间持有、布局计算前即释放(不钉死读池连接)。输出 `ItemsCacheData`(`items_cache.rs:76-113`)。
   - **filename 轴统一缓存**:datetime 基准派 filename 需惰性 `filename_rank`——`ensure_filename_rank_for_hit`(`layout_commands.rs:80`)优先用全局 filter-invariant rank(`try_global_filename_ranks`,`state.rs:401`,读锁内免 DB 映射);未就绪退化 per-filter id-only 查询(`query_item_ids_filename_order`,`item_queries.rs:203`)+ 触发后台全局构建(`spawn_global_filename_rank_build`,`state.rs:419`;开机 15s 预建 `tasks.rs:65-68`)。轴可派生性单一事实源 `can_derive_axis`(`items_cache.rs:209`,datetime 任意组、filename 仅 none/folder/date)。ai_search 视图每次搜索整表重写、`reusable=false` 不可命中(`layout_commands.rs:256`,走通用 `query_layout_items` 含 similarity 列)。
2. **布局计算(纯 CPU,几何)**:`run_layout`(`layout_commands.rs:487`)按 `layout_mode` 分派:`compute_justified_layout`(`justified.rs:41`,流式装箱,行宽达容器宽即提交、`MAX_ROW_HEIGHT_FACTOR=2.0` 防肖像图拉爆、`LAST_ROW_JUSTIFY_THRESHOLD=0.6` 末行短则保持原高不拉伸)或 `compute_grid_layout`(`grid_pack.rs:22`,固定列数方格宫格,cell 撑满容器宽);无缝分组(seamless)排序仍按 group_by 聚合、打包按 none 语义(`justified.rs:232-238`,行跨组连续)。组间并行 `layout_groups_parallel`(`geometry.rs:279`):按 `group_mark`(`:213`,date=UTC 日桶 `div_euclid(86400)`,folder=dir_id)切段 → rayon 并行打包(组内 y 从局部 0)→ 组高前缀和缝合绝对 y(位级一致);标签仅边界处构造 `group_label`(`:233`,date 产 `YYYY-MM` group_id + epoch_day)。输出 `Vec<LayoutRow>`,Normal 行仅存 `SlimRowItem{id,x,w,h}` 零载荷。
3. **物化 + 换代**:`store_layout`(`cache.rs:155`,又在 spawn_blocking 内 `layout_commands.rs:462`):单遍遍历行集建 `flat_ids`/`flat_rowcol`/`id_to_flat`(`IdToFlat` 密集直址/稀疏哈希双形态,`cache.rs:67-109`)与 `separators`/`month_buckets`(月密度桶,date 分组才非空,`cache.rs:175-224`);`layout_version` 在**写锁内**递增(`cache.rs:233-238`,防「后取号者先写」的假性 LayoutNotReady);旧代堆释放卸后台线程(`cache.rs:257-258`)。同时 `store_items`(`items_cache.rs:122`)无条件驻留 items 快照(ai 视图也驻留)作后续载荷源。输出 `LayoutSummary`(`cache.rs:51-60`:total_rows/total_height/layout_version/total_items/separators/month_buckets)。
4. **出口拼装(取行时,不常驻)**:`get_layout_rows_by_y`/`get_bucket_rows` 命中行切片 + `layout_version` 校验(`get_rows_by_y`,`cache.rs:326` 二分定位取视口相交;`get_bucket_rows`,`cache.rs:367` 半开区间 `[start_y,end_y)` 精确段归属)→ `hydrate_rows`(`items_cache.rs:468`)逐 id 经 `id_to_idx`(`items_cache.rs:251`)从 items 快照拼载荷(`hydrate_item`,`geometry.rs:147`,占位色 `placeholder_color` 在此现算一次),查无此 id(换代竞态窗)→ `placeholder_item`(`geometry.rs:176`)保留几何、载荷置空,不丢行不 panic。输出线上 `HydratedRow`(serde 形状 S3 前兼容,前端零改动)。

**数据与产物**:
- DB 只读、无本链写:写路径全在扫描/富化/编辑等上游(提交后 `bump_data_version`,`state.rs:387`,失效契约见 `items_cache.rs:507-513` 与 Spec02 §4 不变量 6)。本链读 `media_items`(canonical 免 JOIN 查询)、`directories`(标签映射)、`ai_search_results`(ai 视图)。
- 产物 = 两把**进程内存**缓存(ItemsCacheData + LayoutCacheData),无磁盘缓存/缓存目录,重启即空,DB 是唯一持久真相源(`Spec02_扫描与画廊.md` §2.3)。
- 失效分层:dv bump → 下次 compute MISS 重查;视图敏感写(收藏/评分/色标在过滤视图中改成员)只降 `reusable=false` 保留载荷源(`patch_or_degrade`,`items_cache.rs:513-535`);缩略图/尺寸回填就地 patch 不失效不 bump(`apply_thumb_results`/`set_dimensions`,`items_cache.rs:539/566`);软删除有意不 bump(`cache.rs:264-275`,暂存删除 UX)。

**前端消费链**:
- 几何源/行供给:`MediaGrid.vue` → `useGalleryVirtualEngine` → `useJustifiedLayout`(引擎直接消费,原 `useGalleryLayoutSource` 透传层已随 P18 删除;重算 watch + scan 期强制宫格 `effectiveLayoutMode` `useJustifiedLayout.ts:52`)→ `mediaStore.computeLayout`(`mediaStore.ts:117`,串行队列 + 30s 看门狗)→ `IPC.COMPUTE_LAYOUT`(`constants/ipc.ts:23`)→ 后端 compute_layout。行供给:`useBucketVirtualScroll`(`useBucketVirtualScroll.ts:168`)→ `fetchBucketRows`(`mediaStore.ts:208`)→ `IPC.GET_BUCKET_ROWS`(`ipc.ts:26`);`fetchRowsByY`(`mediaStore.ts:190`)→ `GET_LAYOUT_ROWS_BY_Y`(`ipc.ts:24`)由 MinimapAxis 分块缩略图消费(bucket 是主画廊唯一滚动取行引擎,不是回退)。
- 事件回显(重算触发):`MEDIA_ENRICHED` 防抖 2s / `VOLUMES_CHANGED` / `totalItems`(扫描完成)/`layoutDirty` / `layoutVersion` 换代(`useGalleryTauriSync.ts:42-111`,后二者带动锚点恢复与可见行刷新)。
- 摘要消费:`TimelineScrubberCanvas` 时间轴按 `monthBuckets`(y/groupId/count)均布跳转,`useGalleryAxisControls.ts:47-72`;侧栏文件夹 → `GET_SUBTREE_SCROLL_TARGET`;选区 → `useViewIds`(`useViewIds.ts:36`,refreshToken 最后发起者赢)↔ `GET_VIEW_IDS`(`ipc.ts:31`);查看器导航 → `GET_ADJACENT_MEDIA`(`ipc.ts:43`);重排锚定 → `GET_ITEM_Y_BY_ID`(`ipc.ts:28`)。

**并发 / 取消 / 错误**:
- 不走共享调度骨架(见 [Spec12](./Spec12_配置状态日志.md) §调度),差异:计算路径(取数+布局+索引物化)全部下沉 `tokio::task::spawn_blocking`(`layout_commands.rs:244/462`),不占 tokio worker;组间打包用 rayon 全局池(`geometry.rs:309-313`,调用方已在阻塞线程,不阻塞 tokio)。**无 CancellationToken**——「取消」= mediaStore 串行队列在飞合并只保留最新(`mediaStore.ts:123-184`)+ dv 换代使在途结果白算。
- 锁纪律:items 与 layout 两把独立 RwLock,任何路径不得同时持有(`items_cache.rs:24-25`);全局 rank 读锁是叶子锁不反向嵌套(`items_cache.rs:76-79`);布局换代旧代在写锁外释放。
- 交互节流:取行/滚动/重算命令 `note_interaction`(`layout_commands.rs:169/538/553/573`,`state.rs:376`)短窗内给后台派生/AI 让窗;取行热路径 span 走 debug 档不刷屏(`layout_commands.rs:167`)。
- 错误码:`LayoutNotReady`(无布局)/`ViewStale`(版本不符,前端重算重取)(`error.rs:44/49`,前端 `useViewIds.ts:47-54` 清空待重取);占位行兜底竞态窗不 panic。`compute_layout` 内部错误统一 `AppError::internal`。
- 关键不变量:①**序等价契约**——`derive_order` 内存派生与 SQL `ORDER BY` 逐项等价(`items_cache.rs:19-21`),`get_view_ids`(flat_ids)与 `view_to_sql`(SelectAll)两源错位即选区漂移,改 SQL 序必同步派生 + 对拍测试;②`can_derive_axis` 为可派生性单一事实源(`items_cache.rs:208`),`is_hit_valid` 与 HIT 守卫共用;③版本单调序 == 实际写入序(`cache.rs:233-238`);④`sort_datetime` 按 UTC 墙钟存,date 月桶/日分隔符/filename UTC 日桶共用同一时区基准(`items_cache.rs:204-208`)。

**总表纠错**(已并入上表):§2.1 行 13 补入主行供给 `get_bucket_rows`(bucket 引擎主路径)与选区/锚定命令 `get_view_ids`/`get_item_y_by_id`/`get_subtree_scroll_target`/`get_adjacent_media`;并移出已删除的 `get_layout_rows`;「compute 秒级 IPC」已改为 HIT 亚秒、MISS 大库秒级。

---

### 2.3.14 音频 + 播放

**一句话**:音频流水线把音频文件的「标签/封面/歌词」抽成三层独立产物:扫描补全期 enricher 回填 `audio_meta`(列表/索引用)、派生期 `audio_cover` 走缩略图编码器写 WebP 网格封面、播放器打开时 `get_audio_detail` **即时**懒加载全分辨率封面+歌词文本;播放本身由 WebView2 原生 `<audio>` 承担,后端不做任何解码/转码,只提供 `abs_path` 与封面路径。

#### **触发入口**

- **`audio_meta` 回填(扫描补全链)**:`start_scan`(`ipc/scan_commands.rs:526`)→ 后台 `spawn_blocking` 跑 `run_enrichment`(`scan_commands.rs:624-635`)→ `enrich_audios`(`scanner/enricher.rs:484`),队列 = `get_audios_needing_meta`(`db/queries/metadata.rs:148`,选 `audio_meta` 缺行且 `media_type='audio'` 的项)。音频比例固定 400×400 封面位(fast_scan 已设默认,`scanner/fast_scan.rs:77`),本链只补元数据(`enricher.rs:352`)。
- **`audio_cover` 派生(共享派生链)**:前端 `useDerivationAutoStart` 启动 3s 延迟 kick(`composables/useDerivationAutoStart.ts:73`)+ 监听 `db:media_enriched` 1.5s 防抖再踢(`:57-65`)→ `start_derivation`(`ipc/derive_commands.rs:64`,不带 kind 过滤)→ 后端 backfill 按 `for_media("audio",…)`(`derive/kind.rs:125`)入队 `audio_cover`(仅此一种,`AudioMeta::is_implemented=false` 被 `derive/pipeline.rs:233` 跳过)。
- **`get_audio_detail`(播放器即时路径)**:路由 `/audio/:id`(`router/index.ts:76`)打开即调(`ipc/audio_commands.rs:25`)。此链不依赖前两条是否完成——既有库无需重扫即可用(`audio_commands.rs:19-23` 模块注释)。

#### **状态机**

无独立状态机。`audio_cover` 走共享派生状态机 `media_derivations.status`:0 待处理 / 1 处理中 / 2 完成 / 3 错误(表 `db/schema/early.rs:266-274`,kind 注释含 `audio_cover`);迁移 = backfill `INSERT OR IGNORE` 插 0(`derivations.rs:320-355`)→ 消费者领 1 → `batch_finish_derivations` 写 2/3(`derivations.rs:185`)。`audio_meta` 由 enricher 一次性回填无持久状态(缺行即待补,`metadata.rs:162`);`get_audio_detail` 为纯即时调用无状态。孤儿恢复/毒任务防线/优雅退回见 [Spec12](./Spec12_配置状态日志.md) §调度 + `derivations.rs:214/248`。

#### **主链阶段**

1. **扫描补全 `audio_meta`**——输入:`(id, abs_path, file_format)` 批(`metadata.rs:148`,批大小 `ENRICHMENT_BATCH`)。处理:`reserved_core_pool` 保留核上 rayon 并行(`enricher.rs:493-527`),每项 `read_tags`(`audio/mod.rs:78`)单次 lofty 解析 → `find_lrc`(`audio/mod.rs:183`)+ `lyrics_source`(`audio/mod.rs:38`);解析失败 `unwrap_or_default` 静默写最小行(与视频探测同「失败也写行防重探」语义,`enricher.rs:482`)。输出:`upsert_audio_meta`(`metadata.rs:174`)写 `audio_meta`(codec/artist/album/title/track_no/year/genre/lyrics_source/lyrics_path);主表 `duration_ms` 顺手 UPDATE(`enricher.rs:547-552`);每批 `bump_layout_data_version` + emit `db:media_enriched`(`enricher.rs:557-567`)。**不产封面**(归阶段 2)。
2. **派生 `audio_cover`**——输入:`DerivationContext`(item_id/cache_key/abs_path/cache_dir/thumb_size/webp_quality,`derive/pipeline.rs:586-599`)。处理:`run_cover`(`derive/audio.rs:18`):`read_cover`(`audio/mod.rs:86`)取内嵌封面字节 → `image::load_from_memory` 解码为 RGBA(ICC 暂按 sRGB,`derive/audio.rs:34`)→ 复用缩略图编码器 `encode_media_step`(`thumbnail/generator.rs:440`):`snap_to_tier` 就近档位(`:30`)→ resize → WebP(`webp_quality`)→ `write_atomic` 写 `thumbnails/{tier}/{prefix}/{hex}.webp`(`thumbnail/cache.rs:16/38`)→ thumbhash(`generator.rs:489`)。输出:`DerivationOutput { payload_path: thumb_db_path, thumbhash }`(`derive/audio.rs:49-53`);无内嵌封面 → `AudioMetadata` 错误 → status=3 **不重试**,网格回落音符占位(`derive/audio.rs:19-20` 与模块注释 `:6`)。写入器:`batch_finish_derivations`(`derivations.rs:185`)+ 封面类镜像回填 `media_items.thumb_status/thumb_path/thumbhash`(`derive/pipeline.rs:703-753`,写边 `update_thumb_result` `db/queries/thumbnail.rs:408`)+ `state.apply_thumb_results` 同步常驻布局缓存(`pipeline.rs:777`)+ emit `db:media_enriched` 通知画廊刷新(`pipeline.rs:811`)。
3. **即时 `get_audio_detail`**——输入:`id`(`audio_commands.rs:25`)。处理:`get_media_detail` 取核心项+abs_path(`:28-31`)→ `audio::read_all` **单次解析同取标签+封面**(`audio/mod.rs:95`;避免同文件解析 3 次)→ `lyrics_source`/`lyrics_from_tags` 复用已读标签取歌词文本(`audio_commands.rs:39-56`;`audio/mod.rs:201`,先内嵌后 `.lrc`,含 `[mm:ss]` 判定 `is_synced_lrc` `audio/mod.rs:216`)→ `write_cover_to_cache`(`audio_commands.rs:79`):全分辨率原图经 `audio_cover_cache_path`(`thumbnail/cache.rs:472`)落盘,`exists()` 命中即复用,否则 `write_atomic` 原子写(`audio_commands.rs:96-104`)。输出:`AudioDetail { item, abs_path, meta, cover_path, lyrics, lyrics_synced }`(`db/models/media.rs:169-180`)。

#### **数据与产物**

- **DB**:`audio_meta` 表(`db/schema/early.rs:161-167` + SCHEMA_V4 扩列 `:285-290`);`media_items` 增 `duration_ms`;`media_derivations` 行 `kind='audio_cover'`,`payload_path` 存 `thumb_db_path`;封面镜像到 `media_items.thumb_status/thumb_path/thumbhash`。
- **缓存产物(两套并存)**:① 显示缩略图 `<cache>/thumbnails/{tier}/{prefix}/{hex}.webp`,与普通图片同键族 → `MediaThumb` 零改动显示(`derive/audio.rs:4-6` 模块注释);② 全分辨率原封面 `<cache>/audio_covers/{cache_key:016x}.{ext}`(`thumbnail/cache.rs:472-476`),ext ∈ `AUDIO_COVER_EXTS`(`:479`),**不重编码、无损**。
- **治理**:`audio_covers` 并入 CacheStats(`thumbnail/cache.rs:378`)与 `cache_files_for_key`/`remove_cache_files_for_key`(`:498-500/:506`,随媒体删除/清缓存联动);但**不进 LRU 淘汰**——`enforce_cache_limit` 只扫 thumbnails/ai_thumbs/face_thumbs/viewer_color(`cache.rs:221-226`),全分辨率封面绑定源媒体生命周期(同雪碧图姿态)。
- **`lyrics_source` 只记来源**:三态 `embedded|lrc|none` + `.lrc` 路径持久化(`metadata.rs:188-201`),**歌词文本不进库**,由播放器按来源现场读。

#### **前端消费链**

- **列表/网格**:音频项与其它媒体同流,封面走通用 `MediaThumb`(读 `media_items.thumb_path`)、时长徽标读 `duration_ms`;刷新靠 `db:media_enriched`(`constants/ipc.ts:413`)。
- **播放器** `/audio/:id`(`router/index.ts:76`)→ `AudioPlayer.vue`:load() 并行发 `get_media_detail`(底栏 rating/favorite 标量,`AudioPlayer.vue:313`)+ `get_audio_detail`(`:319`,命令名见 `constants/ipc.ts:47`)→ `<audio>` src = `convertFileSrc(absPath)`(`:216`)、封面 img = `convertFileSrc(coverPath)`(`:217`,路径在 asset scope 内)→ 歌词按 `lyricsSynced` 分流:带时间轴 `parseLrc` + `activeLineIndex` 随播放高亮/居中(`:283/:322`),否则整段 `<pre>` 文本(`:130`)。
- **派发触发**:派生 kick 由 `useDerivationAutoStart` 统一驱动(`useDerivationAutoStart.ts:37-62`),音频无独立前端触发链。IPC 契约层在 `src/utils/ipc.ts` + `src/constants/ipc.ts`。

#### **并发 / 取消 / 错误**

- **三条链互不依赖**:`audio_meta`(enricher)与 `audio_cover`(派生)可同时进行;`get_audio_detail` 不读 DB `audio_meta` 行,始终现场解析(故未补全也能用)。补全-派生-刷新闭环:`enrich_audios` emit `db:media_enriched`(`enricher.rs:560`)→ 前端防抖 kick `start_derivation` → 封面落地再 emit(`pipeline.rs:811`)→ 画廊刷新。
- **派生链**:走共享骨架(生产者 → crossbeam → rayon 消费者池 → 批量写入器),`background_heavy_limiter` 公平 permit(`derive/pipeline.rs:568`,与 exotic 共享),扫描/缩略图运行硬让步、交互涓流;`panic_guard`(`pipeline.rs:605` + `thumbnail/generator.rs:46`)把毒文件 panic → status=3;孤儿/毒任务防线(`derivations.rs:214`)——详见 [Spec12](./Spec12_配置状态日志.md)。
- **取消**:扫描 `CancellationToken` 传入 `enrich_audios`(`enricher.rs:489`),取消即返 `AppError::Cancelled`(`:498-500`),已写批不回滚;派生取消走共享 `CancellationToken` + 优雅 stop `requeue_in_flight_derivations`(`derivations.rs:248`)。
- **错误处理**:文件损坏/不支持 → `AppError::AudioMetadata`;enricher 与 `get_audio_detail` 均 `unwrap_or_default` 静默降级(空标签/无封面,**不阻断播放器打开**,`audio_commands.rs:39`);`audio_cover` 无内嵌封面 → status=3 不重试。`get_audio_detail` 的 `spawn_blocking` 任务 panic 包 `AppError::internal`(`audio_commands.rs:71`)。
- **关键不变量**:`cache_key = path|mtime` 的封面不可变 → `write_cover_to_cache` 仅当文件不存在才写,命中判定与写入分离(`audio_commands.rs:100-104`);封面与缩略图同 `cache_key` 键族(不变量 §1.3.3,`derive/audio.rs` 模块注释 `:5-6`)。

---

### 2.3.15 日志 + worker 协议

**一句话**:全仓日志与跨进程 worker 日志的统一汇聚底座——任意 `tracing` 调用点、`SpanTimer` 手动计时、worker stderr 单行 JSON、前端 `logger.ts` 四路来源汇入同一条「信封化 → 过滤 → 三扇出」链,产出 JSONL 文件(事实源)+ dev 控制台 + UI 环形缓冲实时流;WARN/ERROR 按 (调用点, 稳定 code) 30s 窗口重复压缩,全部调度原理共享骨架见 [Spec12](./Spec12_配置状态日志.md) §3.3,本节只写本流水线的特有实例化。

**触发入口**(被动汇聚型,入口 = 事件来源 4 路 + 控制命令 7 个):
- 任意 `tracing::info!/warn!/error!/debug!` 调用点(全仓),含 `error.rs:458` AppError Serialize 内 `tracing::error!`。
- `SpanTimer` 手动计时守卫 `logging.rs:409`,`info`/`debug` 两档(`logging.rs:418/428`),Drop 时发 `target:"scrollery::span"` 的 `"span_close"` 事件(`logging.rs:448-472`);埋点示例:`read_log_file_page` 内 `logging.rs:178`。
- worker 子进程 stderr 单行 JSON:`emit_stderr_log`(`crates/exotic-protocol/src/stderr_log.rs:37`),五个 worker(ai/psd/raw/video/enhance)的 `main.rs`/`batch.rs` 统一调用;宿主侧每 worker 一条排空线程 `spawn_stderr_drain`(`exotic/worker_log.rs:175`),在 `WorkerSupervisor::spawn` 拉起(`exotic/supervisor.rs:119`)。
- 前端 `logger.ts` 队列批量汇入 `log_frontend_events`(`ipc/system_commands.rs:231`):2s 定时/50 条阈值(`src/utils/logger.ts:26-27`)或 `window.onerror`/`unhandledrejection` 立即 flush(`logger.ts:104-127`)。
- 控制命令:启动装配 `init_subscriber`(`logging.rs:499`,由 `lib.rs:249` setup 段调用一次);运行期级别热切 `set_app_config` 的 `log_level` 分支(`config_commands.rs:469`)→ `LOG_RELOAD`(`config_commands.rs:21`)热替换 EnvFilter;日志窗口 `open_log_window`(`log_commands.rs:20`,设置页「日志窗口」项 `DynamicSettingControl.vue:490`);历史/分析 `list_log_files`(`log_commands.rs:59`)/`read_log_file_page`(`log_commands.rs:171`)/`get_log_diagnostics`(`log_commands.rs:211`)/`compute_log_histogram`(`log_commands.rs:313`)/`export_diagnostics_package`(`log_commands.rs:349`)/`clear_logs`(`system_commands.rs:148`)。

**状态机**:无独立业务状态机,也不走共享「生产者→通道→消费者池→写入器」骨架——本链被动事件驱动,无需 RunTokenSlot/CancellationToken 实例化。可观测状态:① 装配态——subscriber 三层(reload EnvFilter + 文件层 + 环形缓冲层)启动期一次性构建(`logging.rs:572-592`),构造后不可替换,故 `log_ring_buffer_capacity`/`max_log_dir_bytes` 只启动期读一次(hot=false,Spec12 §4.2);② 级别档——EnvFilter 热切(`logging.rs:546-549`),trace..off 六档,off=真正全关(`logging.rs:93-94`);③ 环形缓冲订阅态——二元 `subscribed` 原子标志(`logging.rs:338`),`open_log_window` 建窗置真、`WindowEvent::Destroyed` 置假(`log_commands.rs:37-42`),取消即清缓冲防陈旧批抢跑;④ worker 生死态(alive/死→池回收补新)属 supervisor 状态机,日志只作旁路消费。

**主链阶段**:
- **S0 启动装配**(`logging.rs:499-604`):`enforce_size_budget`(`logging.rs:45`,按 `max_log_dir_bytes` 删最旧 .log)→ `generate_session_id`(`logging.rs:24`,4 字节随机 `s-xxxx`)→ RollingFileAppender daily + max 14(`logging.rs:527-538`)→ `non_blocking`(`logging.rs:541`,guard 须经 `app.manage` 托管,`lib.rs:293-344` 传 `LoggingBoot`,局部持有即全丢)→ reload EnvFilter(`logging.rs:546-549`)→ 文件层 EnvelopeFormat(`logging.rs:552-557`)+ 环形缓冲层(`logging.rs:569-570`)+ dev 控制台层(`logging.rs:578-588`,release 不装)→ panic hook(`logging.rs:596`,panic 消息作最后一条 ERROR 落盘)。
- **S1 事件产生**:见触发入口四路。
- **S2 过滤**:reload `EnvFilter` 依当前 logLevel 档丢弃;trace/debug/info 档自动追加 `,scrollery::pipeline=warn,reqwest=warn` 后缀压住四条高频流水线(`build_env_filter_directive`,`logging.rs:97-111`;四条流水线调用点已改挂 `target:"scrollery::pipeline::{thumb,ai,face,video}"`)。
- **S3 信封化**:`build_envelope`(`logging.rs:271`)→ `FieldCollector`(`logging.rs:199-244`)——`message` 提为 `msg`、`operation_id` 提为信封顶层字段、`error_chain`/`error_code`/`frontend_context`/`worker_context` 四特判把调用点编码的 JSON 文本**解回真对象**落 `attributes`(防把数组/对象当转义字符串存);顶层六字段 `ts/level/target/session_id/operation_id/msg`(`EnvelopeFormat::format_event`,`logging.rs:298`)。
- **S4 三扇出**:① 文件层→`non_blocking` 后台写线程→daily JSONL;② dev 控制台文本层;③ `RingBufferLayer::on_event`(`logging.rs:375`)先查 `subscribed`,未订阅直接返回(不格式化不入队,零广播,测 `logging.rs:783-791`);入队超 `capacity` 按 FIFO 裁最旧(`logging.rs:380-384`);`LogRingBuffer::drain`(`logging.rs:351`)由 100ms 周期任务抽干(`tasks.rs:43-51`)emit `"log:batch"`(`tasks.rs:48`)。
- **S5 worker stderr 旁路**(`exotic/worker_log.rs`,注:Spec12 §3.3 旧锚点 `supervisor.rs:146-202` 已在超长拆分线 W4 迁出,现实际在 `worker_log.rs`):排空线程 4096 块读,两套机制并行互不合并(D-313 红线)——① 字节原样进 64KiB 环形缓冲(`STDERR_RING_CAP`,`supervisor.rs:59`;`stderr_tail_lossy` `supervisor.rs:340` 供崩溃摘尾);② `LineScanner`(`worker_log.rs:32-76`)按 `\n` 切行、残段累积、超 `LINE_RESIDUAL_CAP`(64KiB)强制整段冲出(`worker_log.rs:62`)→ `forward_stderr_line`(`worker_log.rs:139`)trim `\r`/跳空 → `parse_worker_log_line`(`worker_log.rs:86`,首字符 `{` 才尝试解析)→ 结构化则 `emit_worker_log_line`(`worker_log.rs:100`)按 `lvl` 五档 match 转 `target:"scrollery::worker"` tracing 事件,`fields` 经 `worker_context` 携带解回真对象;非 JSON 行按 WARN + `unparsed=true` 转发原文,未知 lvl 按 warn 附原 `lvl`(schema 漂移可见)→ 回 S2-S4 同链汇入主 JSONL + 环形缓冲。
- **S6 WARN/ERROR 重复压缩**:`error_log_dedup_check`(`error.rs:295`)——签名 (调用点 `module_path!()`, AppError 稳定 code),30s 窗口(`ERROR_DEDUP_WINDOW`,`error.rs:284`):首见落盘附 `repeat_count=1`(`error.rs:459`)、窗口内重复吞掉但降档 debug 留全量证据(`error.rs:481-487`,D-307,防泛化 code 误吞)、窗口切换首条落盘附 `suppressed=N`(`error.rs:471`)。仅 AppError 序列化路径经此;纯文本 warn/error 不压缩。
- **S7 历史/诊断消费**(独立于实时链,按需 IPC):`read_log_file_page` 整文件读入按行切片,用**绝对行号锚点**分页防浏览仍在写入的当日文件时跨页重叠(`slice_page_bounds`,`log_commands.rs:114`;锚点稳定性测 `log_commands.rs:467-478`);`compute_log_histogram` 开独立内存 SQLite 临时表 `GROUP BY strftime` 分桶(`compute_histogram_from_content`,`log_commands.rs:259`),裁时区后缀取朴素本地时间(`strip_tz_suffix`,`log_commands.rs:251`);`export_diagnostics_package` 取最新日志尾 2000 行(`DIAGNOSTICS_TAIL_LINES`,`log_commands.rs:344`)经 `redact_diagnostics_text`(`logging.rs:166`,对**解码后** JSON 值树递归脱敏用户名段)打包 zip。

**数据与产物**:
- 目录:`<app_data>/logs`(`state.log_dir`,启动期解析一次,`get_log_dir` `config_commands.rs:522`);文件 `scrollery.YYYY-MM-DD.log`,JSONL 每行一条信封。
- 格式:`{ts, level, target, session_id, operation_id, msg, attributes}`(`logging.rs:268-284`);`attributes` 承载 `error.chain`(数组)/`error.code`/`context`/`span_name`/`duration_ms`/`panicked`/`repeat_count`/`suppressed` 等。
- 写入时机:每事件经 non_blocking 后台线程异步落盘(每条无 fsync,`logging.rs:524-527`);进程退出 guard drop 时阻塞 flush(off 档特征化测试锁定 `logging.rs:617-670`)。
- 保留:`max_log_files=14`(RollingFileAppender)+ 启动期 `max_log_dir_bytes`(默认 512MB,`logging.rs:20`)按 mtime 最旧先删兜底(`logging.rs:45`,只清 `.log`,活跃文件 mtime 最新不误删,测 `logging.rs:697-742`)。
- 诊断包:`logs/diagnostics/scrollery-diagnostics-<毫秒stamp>.zip`,含 `system-info.json` + `log-tail.jsonl`(脱敏),`.tmp` 同卷 rename(`log_commands.rs:406-434`)。
- 无 DB 表:直方图仅用独立内存 SQLite 临时表,不占主库连接池。

**前端消费链**:
- 窗口挂载:`main.ts` 按 window label==='logs' 把 `LogWindowView.vue` 直接挂为应用根(跳过 AppShell/路由);`onMounted` 取 `GET_LOG_DIR` + `loadHistoryFiles`(`LogWindowView.vue:28-35`);开窗由设置页发 `OPEN_LOG_WINDOW`(`DynamicSettingControl.vue:490`)。
- 实时流:`logWindowStore.ts` 全生命周期 `useTauriListen(EVENTS.LOG_BATCH, onBatch)`(`logWindowStore.ts:58`)→ `onBatch`(`logWindowStore.ts:40`)tag 全局单调 `_seq` 入 `liveEntries`;暂停批走 `pendingEntries` 同口径 FIFO 裁剪(`logWindowStore.ts:43-51`);渲染保留行数 `renderCap` 默认 10k 为**前端本地**裁剪,后端安全阀为 `RING_BUFFER_CAPACITY` 20k(`logWindowStore.ts:15-17`/`logging.rs:315`)。
- 过滤/聚合:级别多选 + target 前缀 + 文本子串/正则(`logWindowStore.ts:115-130`,正则非法降级不误清视图);错误按 `attributes['error.code']` 分组(`aggregateErrorEntries`,`src/utils/logAggregation.ts:23`)、span 按 `attributes.span_name` 聚合次数/总/均/最大耗时(`aggregateSpanDurations`,`logWindowStore.ts:64`)。
- 历史/分析:分页锚点 `oldestLoadedLine` 原样带回(`logWindowStore.ts:144,174`);preset 存 localStorage 非后端(`logWindowStore.ts:193`);直方图 `histogramBucketUsed` 跟数据走不跟控件走(`logWindowStore.ts:271`)。
- IPC 契约层:`invokeIpc`(`src/utils/ipc.ts:86`,只收 `IpcCommand` 联合类型);命令常量 `src/constants/ipc.ts`:LOG_BATCH:432 / OPEN_LOG_WINDOW:208 / LIST_LOG_FILES:210 / READ_LOG_FILE_PAGE:212 / GET_LOG_DIAGNOSTICS:217 / COMPUTE_LOG_HISTOGRAM:219 / EXPORT_DIAGNOSTICS_PACKAGE:221 / CLEAR_LOGS:248 / LOG_FRONTEND_EVENTS:254 / GET_LOG_DIR:198。
- 事件回显:`"log:batch"` app 级广播(`tasks.rs:48`),仅日志窗口监听,其余窗口忽略零成本。
- 前端反向来源:`logger.ts` 队列→`LOG_FRONTEND_EVENTS`,信封落 `target:"scrollery::frontend"`、来源用 `source` 字段区分(tracing target 是编译期常量不能塞运行时字符串)。

**并发 / 取消 / 错误**:
- 并发:subscriber `on_event` 在**调用宏的那个线程**同步执行,环形缓冲层只做内存 push + 定长裁剪、std Mutex 短临界区(`logging.rs:360-386`);磁盘 IO 卸给 non_blocking 自建后台线程;worker stderr 排空每条 worker 一个独立 OS 线程(`worker_log.rs:181`)。不走共享调度骨架(见 [Spec12](./Spec12_配置状态日志.md) §调度)。
- 取消:日志自身无取消——off 档即过滤层静默;100ms 抽干任务入 `HandlesPool`,退出分支 abort + join(`tasks.rs:34-35,52`)。
- 错误:non_blocking lossy 丢弃计数 `error_counter`(`logging.rs:544`)经 `get_log_diagnostics` 可见(`log_commands.rs:211`);环形缓冲超容 FIFO;行扫描残段 64KiB 上限强冲;解析兜底(历史史前纯文本行→合成信封 `parse_log_line`,`log_commands.rs:148`;worker 非 JSON→Raw 兜底 `worker_log.rs:92`);路径穿越防护 `resolve_log_file`(`log_commands.rs:126`,canonicalize + starts_with 校验)。
- 关键不变量:`guard` 必须 `app.manage` 托管(局部持有即日志静默全丢,`logging.rs:482-489`);off=真正全关(追加后缀会让 pipeline target 在 off 态以 warn 放行);环形缓冲无订阅者零成本(不格式化不入队);stderr 双机制勿合并(D-313)、stdout 只走帧/stderr 只走日志(`stderr_log.rs:4-7`);信封 `operation_id` 提顶层不落 attributes(测 `logging.rs:1015-1029`);SpanTimer 用普通事件而非 `FmtSpan` 保证文件层 + 环形缓冲层同时免费收到(`logging.rs:403-408`)。

---

## 3. 跨流水线基础件(非独立业务流水线)

| 模块 | 定位 | 被谁复用 |
|---|---|---|
| `src-tauri/src/download/`(`download/mod.rs:1-8`) | 通用下载引擎:HTTPS 强制 + Range 断点续传 + 镜像回退 + sha256 校验(`.part` 原子改名) | exotic 包、AI/face 模型(`ipc/model_download`)、增强模型清单共用一套机制原语,各域保留领域编排 |
| `src-tauri/src/formats/`(`formats/mod.rs:1-16`) | 格式注册**运行时并集** = 内置表(66)∪ exotic Catalog,`FormatDescriptor` UI 投影(能力字段不下发) | facet 筛选链路、格式弹层、`media_kind()` 过滤(builtin 恒 None 防幽灵扩展名) |
| `src-tauri/src/storage/`(`storage/mod.rs:1-14`) | 存储后端抽象:`LocalFs`(OS 挂载盘/UNC)+ `WebDavBackend`(`netfs` 特性门控) | 扫描器走远程遍历仍待办(D3 后置);`backend_id IS NULL` 扫描根走本地。详见 [Spec08](./Spec08_存储备份导出文件操作.md) |
| `src-tauri/src/engine/` | 解码引擎注册表 `EngineArena`:**ImageRsEngine 在前、WicEngine 在后**(仅 Windows) | 缩略图/编辑/face 缓存/查看器解码统一入口;Phase1 格式走 image-rs,heic/heif/avif/ico 走 WIC |
| `src-tauri/src/exotic/{crypto,license,installer,package,validate}` | keyset 验签 + keyring 授权链 + 安装校验(hash/清单) | exotic/OCR/增强/编辑的授权与分发共用。详见 [Spec09](./Spec09_插件平台与exotic.md) |

---

## 4. 发布/CI 工程流水线(`.github/workflows/`)

| 流水线 | 作用 | 关键约束 |
|---|---|---|
| `ci.yml` | 私有 canonical 仓提交门控:rust(check+clippy+test)/ frontend(lint+typecheck+vitest)/ smoke | 自托管 runner(dev-box-win,`github.repository == 'gfgjs/scrollery-private'` guard);推理测试红线 J16(真加载 ORT 的测试必须 `#[ignore]`) |
| `sync-oss.yml` + `copy.bara.sky` | 本仓 → 公开镜像**单向**同步(Copybara) | 只做内部文件过滤(`docs/`/`.agents/`/AGENTS.md/同步配置自身);**代码与 `Cargo.lock` 原样投影**,公开树与私有树同构;两 workflow 均用 `git.origin` 只读已提交树 |
| `oss-gate.yml` | 公开树编译/测试把关(sync-staging 上跑,托管免费) | 公开 main 每次提升前的第二道防线;原锁 `cargo check/test --workspace --locked` + gitleaks 密钥扫描 |
| `drift-alarm.yml` / `release.yml` / `docs-governance.yml` | 同步漂移告警 / 发布(tag 触发 + 冒烟)/ docs 门 frontmatter 基线 | docs 索引新鲜度不变量门(`tools/check_docs_index.mjs`) |

---

## 5. 架构特征与已知遗留(跨流水线)

1. **统一调度范式**:见 §2.2。
2. **推理恒子进程**:T16 后 host 零 ort——CLIP/人脸/OCR/增强全部经 exotic-protocol 帧派发到独立 worker 子进程(ai-worker / enhance-worker / video-worker / raw-worker / psd-worker),宿主只做控制面。详见 [Spec06](./Spec06_AI人脸OCR.md) §1、[Spec13](./Spec13_构建发布商业化.md)。
3. **🔴 发布阻断(F-01/F-02)**:`tauri.conf.json` `externalBin` 仅含 raw-worker,ai/enhance/video worker 无生产 bundle/fresh-install 闭包;增强模型清单为 `PENDING_USER_REPO` 占位(`enhance/registry.rs:20`)。当前 dev 分支本地测试全绿但**未真打包验证**。跟踪于 [docs/completed.md](../completed.md) ▸ R-深审07-25(原 todo.md 2026-07-25 增补)与 todo.md「已收口索引」。详见 [Spec13](./Spec13_构建发布商业化.md)。

---

## 6. 关联

- **横切总览的对偶**:[Spec00](./Spec00_产品与架构全景.md) §3.1「后端 26 模块一览」是**按模块**组织,本篇是**按数据流链**组织;两表互补,本篇行内锚点指回各子篇。
- **上游正典**:`../refactor_2026/Part2_扫描与画廊流水线.md`、`../refactor_2026/Part3_缩略图与派生.md`、`../refactor_2026/Part4_AI与派生流水线.md`(计划期流水线设计理由)。
- **全局不变量入口**:[Spec14_不变量与约定](./Spec14_不变量与约定.md)。
- **相关规格篇**:各流水线明细见上表「现状」列内链接。
