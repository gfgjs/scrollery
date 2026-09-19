---
id: 2026-07-24-Spec03_缩略图与派生
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec03 · 缩略图与派生

> 一句话:本篇讲媒体项如何从「一个文件」变成「一张可显示的缩略图」——图像的直接生成路径(`src-tauri/src/thumbnail/`)与视频/文档/音频/AI 缓存的统一后台派生框架(`src-tauri/src/derive/`),以及两者共用的磁盘缓存治理。服务对象:要改动缩略图生成算法、加一种新的派生 kind,或要排查「封面/缩略图为什么不出/404/驱逐」的工程师或 LLM 代理。读前无需先读其余篇。

## 1. 概览

Scrollery 把「给媒体项一张缩略图」拆成两条互补的生产线:

1. **缩略图生成引擎**(`thumbnail/`):图像的**同步/请求式**路径——前端滚动到某行,IPC 命令直接解码目标图并编码为 WebP。也是所有派生 kind 复用的**编码器**(`encode_media_step`),不止服务图像。
2. **派生流水线**(`derive/`):视频封面/关键帧雪碧图、文档首页、音频封面、AI 分析缓存这些**重、需要独立后端库、可后台跑**的产物,由一套 kind 无关的通用调度框架驱动——生产者/消费者/写入器 + 断点续传 + 让步 + 孤儿恢复,与 AI/人脸流水线(`Spec06`)同构。

两者通过 `media_items.thumb_status/thumb_path/thumbhash`(缩略图/封面镜像列)与 `media_derivations`(派生任务状态机,每个 `(item_id, kind)` 一行)两张表衔接:派生流水线产出封面后,把结果**镜像**回 `media_items`,前端 `MediaThumb` 组件零改动即可显示(不区分「原生缩略图」还是「派生封面」)。

### 代码位置

| 路径 | 职责 |
|---|---|
| `src-tauri/src/thumbnail/generator.rs` | 统一生成入口:decode→encode 两阶段、EXIF 快速路径调度、一次解码两份产物(缩略图+AI 缓存) |
| `src-tauri/src/thumbnail/cache.rs` | 缓存路径方案(尺寸分桶)、LRU 驱逐、占用统计、分类清理、孤儿对账 GC |
| `src-tauri/src/thumbnail/router.rs` | 冷门格式让路判定(纯函数,不查 DB) |
| `src-tauri/src/thumbnail/qos.rs` | 缩略图 worker 线程数预算 + 前后台 QoS(Windows/macOS) |
| `src-tauri/src/thumbnail/thumbhash.rs` | ThumbHash 编解码(前端占位模糊图) + 占位平均色计算 |
| `src-tauri/src/thumbnail/exif_thumb.rs` | EXIF 内嵌缩略图快速路径 + WebP/JPEG 编码器封装 |
| `src-tauri/src/derive/kind.rs` | 派生 kind 注册表(唯一事实源:有哪些 kind、适用媒体、编译期是否有后端) |
| `src-tauri/src/derive/pipeline.rs` | 通用派生调度框架:生产者/消费者/写入器、续传、让步、孤儿恢复 |
| `src-tauri/src/derive/video.rs` | 视频封面(`run_cover`)+ 关键帧雪碧图(`run_keyframes`),经 `VideoBackend` trait(见 `Spec05`) |
| `src-tauri/src/derive/doc.rs` | epub 封面提取(zip+OPF 解析)+ spine 页数;pdf/svg 前端渲染(见下 §5) |
| `src-tauri/src/derive/audio.rs` | 音频内嵌封面提取(lofty) |
| `src-tauri/src/derive/image.rs` | AI/人脸分析缓存(短边缩放 WebP),供 CLIP/人脸流水线消费(权威在 `Spec06`) |
| `src-tauri/src/ipc/thumbnail_commands.rs` | 缩略图 IPC:批量请求、全量/增量生成、进度、懒自愈、清缓存 |
| `src-tauri/src/ipc/derive_commands.rs` | 派生 IPC:`start_derivation`/`pause_derivation`/`stop_derivation`/`derivation_status` |
| `src-tauri/src/db/queries/derivations.rs` | `media_derivations` 状态机 DAO(pending/claim/finish/reset/backfill) |
| `src-tauri/src/db/schema.rs:271-284,806-817` | `media_derivations` 表定义(V4)+ `orphan_count` 列(V22) |

### 整机中的位置

```
                      画廊滚动 / 全量生成按钮
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
   thumbnail_commands.rs               derive_commands.rs
   (batch_request_thumbnails /         (start_derivation:
    start_full_thumbnail_generation)    video_cover/keyframes/
              │                          doc_thumb/audio_cover/
              ▼                          audio_meta/ai_thumb)
   thumbnail::generator                          │
   (decode_media_step → encode_media_step) ◄──────┘ (video/doc/audio 的
              │                                      run() 复用同一 encode_media_step)
              ▼
   thumbnail::cache（尺寸分桶落盘）─── 镜像 ───► media_items.thumb_status/path/hash
              │                                      │
              ▼                                      ▼
   LRU 驱逐 / 对账 GC                         前端 MediaThumb 组件（零改动显示封面）
```

上游:画廊滚动触发 `batch_request_thumbnails`(按需),或用户点「生成全部缩略图」触发 `start_full_thumbnail_generation`(全量);派生侧由 `useDerivationAutoStart`(前端,详见 `Spec11`)自动 kick,或用户在派生控制卡手动 `start_derivation(kinds, reset)`。下游:两条线的产物都落 `cache/` 目录树,并镜像回 `media_items`,由前端 `MediaThumb` 统一消费(不感知产物来自哪条线)。视频/人脸解码细节交叉引用 `Spec05`;CLIP/人脸分析缓存消费方权威在 `Spec06`;色彩管理(ICC→sRGB 投影)权威在 `Spec04`。

## 2. 数据模型与状态

### 2.1 DB 表(全表定义权威见 `Spec01`;此处只列本篇消费的关键列)

**`media_items`**(`src-tauri/src/db/schema.rs:74-124`)缩略图相关列:
```sql
thumb_status    INTEGER NOT NULL DEFAULT 0,  -- 0待处理/1已生成/2跳过(不支持类型)/3小文件直显
thumb_path      TEXT,                        -- 相对 cache_dir/thumbnails 的路径，或直显时为源文件绝对路径
thumbhash       BLOB,                        -- ~28 字节，前端占位模糊图
cache_key       INTEGER NOT NULL             -- 派生产物的寻址键（见 §2.2 缓存布局）
```
`idx_media_thumb`(`schema.rs:118`)是部分索引,只覆盖 `thumb_status != 1`,使「找待生成项」扫描量与已完成量无关。

**`media_derivations`**(`schema.rs:271-284`,V4 引入;`orphan_count` 列 `schema.rs:806-817`,V22 引入):
```sql
CREATE TABLE media_derivations (
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL,      -- 'video_cover'|'video_keyframes'|'doc_thumb'|'audio_cover'|'audio_meta'|'ai_thumb'
    status       INTEGER NOT NULL DEFAULT 0,  -- 0待处理/1处理中/2完成/3错误（语义同 ai_status，见 Spec01/Spec06）
    payload_path TEXT,                   -- 产物相对路径（sprite/封面/AI 缓存等）
    error        TEXT,
    orphan_count INTEGER NOT NULL DEFAULT 0,  -- 毒任务防线计数(V22，见 §5.2)
    updated_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (item_id, kind)
);
```
`idx_deriv_pending`(`schema.rs:284`)部分索引只覆盖 `status<2`。每个 `(item_id, kind)` 一行,需显式入队(`backfill_derivations`,`db/queries/derivations.rs:269-306`)——与 AI 的 `ai_status` 单列驱动不同,是派生表独有的设计(理由:kind 是开放集合,需要显式登记「这个 item 适用哪些 kind」,而非借用一个整型状态列)。

**`document_meta.page_count`**(`schema.rs:174-178`)由 epub `doc_thumb` 派生顺带回填(spine `<itemref>` 计数近似页数,`derive/doc.rs:121-138`)。

### 2.2 缓存目录布局(`thumbnail/cache.rs`)

所有产物按 `cache_key`(i64,来自 `media_items.cache_key`,派生/寻址的单一事实源;权威定义见 `Spec01`)的 16 位小写 hex 命名,前 2 字符分桶避免单目录文件过多:

| 子目录 | 路径方案(相对 `cache_dir`) | 内容 | LRU 驱逐 | 权威函数 |
|---|---|---|---|---|
| `thumbnails/{size}/{xx}/{hex}.webp` | 5 档 `[64,128,256,512,1024]` | 显示缩略图 + 视频/音频/文档封面(封面复用此路径,§3.2) | 是 | `thumb_path`(`cache.rs:19`) |
| `ai_thumbs/{xx}/{hex}.webp` | 短边≥336 | CLIP 分析缓存 | 是(与 thumbnails 共预算) | `ai_cache_path`(`cache.rs:72`) |
| `face_thumbs/{xx}/{hex}.webp` | 短边 640 | 人脸检测分析缓存 | 是 | `face_cache_path`(`cache.rs:109`) |
| `sprites/{xx}/{hex}.webp` | N 帧横拼单条带 | 视频关键帧悬停/scrub | 否(绑定源媒体生命周期) | `keyframe_sprite_path`(`cache.rs:133`) |
| `motion_videos/{xx}/{hex}.mp4` | — | 动态视频缓存(按需生成,自愈) | 否 | `motion_video_cache_path`(`cache.rs:120`) |
| `audio_covers/{key:016x}.{ext}` | 5 种扩展名 | 音频内嵌封面 | 否 | `audio_cover_cache_path`(`cache.rs:482`) |
| `viewer_color/{target_id}/{xx}/{hex}.{ext}` | — | 查看器渲染色域派生(权威在 `Spec04`) | 是(与 thumbnails 共预算) | `viewer_color_path`(`cache.rs:158`) |

`THUMB_TIERS`(`thumbnail/generator.rs:29`)是**全后端唯一档位事实源**,必须与前端 `src/constants/defaults.ts` 的 `THUMB_SIZE_TIERS` 保持一致(改档位使全库缓存作废,属高代价变更,注释已标注未经多设备实测)。`snap_to_tier`(`generator.rs:33-39`)把任意请求尺寸就近吸附到 5 档之一。

### 2.3 内存态

| 状态 | 归属 | 生命周期 |
|---|---|---|
| `ThumbConfig`(`generator.rs:96-116`) | `AppState.thumb_config: RwLock<ThumbConfig>` | 快照式,流水线启动时读一次(避免逐任务读锁) |
| `thumb_gen_progress` / `thumb_gen_token` | `AppState`(具体字段名见 `thumbnail_commands.rs:487-517`) | 全量生成进度快照 + 可取消 token,支持 webview 重载后查询恢复(见 §3.2) |
| `derivation_token` | `AppState`(`derive/pipeline.rs:404`) | 代次守卫(generation-based),防「停止→立即重启」时新旧轮互相打断 |
| `cancelled_thumb_ids` | `AppState.cancelled_thumb_ids: Mutex<HashSet<i64>>` | 单次消费(`thumbnail_commands.rs:309`),防止旧取消标记误伤同 id 的新请求 |

## 3. 关键流程与算法

### 3.1 缩略图生成(图像,`thumbnail/generator.rs`)

`decode_media_step`(`generator.rs:212-361`)是核心判定函数,按序:

1. **缓存命中**:`thumb_status==1` 且磁盘文件 `exists()` → 直接返回(`generator.rs:236-261`)。**注意**:命中判定只查存在性不查内容,这是「原子落盘」不变量存在的原因(见 §4)。
2. **小文件直显**(`thumb_status=3`):web-safe 格式(`jpg/jpeg/png/webp/gif/svg/avif`)且 `strategy==direct` 或 `file_size<=skip_max_bytes` → 跳过生成,直接用源文件路径。仍会为 ≤500KB 的文件生成 ThumbHash 占位(`generator.rs:290-300`,Part3 Q14 裁决:占位图是体验底线不因 direct 策略牺牲)。
3. **按 media_type 分派**:仅 `image` 类型继续;非 image(`thumb_status=2`)留给派生流水线处理(`generator.rs:346-359`)。
4. **GPU/CPU 解码**:`strategy=="gpu"` 走 `try_gpu_decode`(WIC),失败降级 `DeferredToCpu`;否则直接 `try_cpu_decode`(`image` crate + EXIF 快速路径)。

**EXIF 快速路径**(`exif_thumb.rs:11-89`):优先复用 JPEG 内嵌缩略图,免全解码。分级验收(`embedded_thumb_acceptable`,`exif_thumb.rs:84-89`):大档位(≥512)严格要求内嵌图边长达标(拒绝数倍上采样劣化),小档位(≤256)宽松放行(容忍轻度放大,速度优先)。

**编码阶段**(`encode_media_step`,`generator.rs:472-540`)是所有 kind(视频/文档/音频封面)共用的终点:
```
resize_to_rgba(fast_image_resize 双线性)
  → ICC→sRGB 投影(project_rgba8_to_srgb,权威见 Spec04)
  → 生成 ThumbHash
  → WebP 编码(q<100 有损 / q=100 无损 VP8L)→ 失败回退 JPEG
  → write_atomic(tmp→rename)落盘
```

**一次解码两份产物**(`ai_hq_cache`,`generator.rs:388-411,576-618`):当 AI 高清缓存开启且图像「宽幅」(缩略图短边会低于 336px)时,GPU/CPU 解码阶段就按更大的长边取样,使**同一份解码缓冲**既产出显示缩略图又产出 AI 分析缓存(`maybe_write_ai_cache`),免去 AI 派生对同一图再解码一次。方图不触发(缩略图短边已够用,建 AI 缓存纯浪费盘)。

**GPU/CPU 长边计算**(`decode_long_edge`,`generator.rs:398-411`):默认等于 `config.size`;若 `ai_hq_cache` 开启且判定为宽幅图,放大到覆盖 AI 缓存短边所需的长边(绝不上采样,`image`/WIC 的 `LongEdge` 语义只降不升)。

### 3.2 全量/增量生成与进度追踪(`thumbnail_commands.rs`)

三条独立命令共用同一「多阶段流水线」引擎(解码池/编码池/deferred CPU 兜底池,`crossbeam_channel` 串联,2026-07-10 A/B 实测裁决为唯一实现,`thumbnail_commands.rs:24-30`):
- `batch_request_thumbnails`:画廊滚动触发,按 id 列表批量请求,经 `route_thumbnail`(§3.3)先过滤冷门格式让路项。
- `start_full_thumbnail_generation` / `start_incremental_thumbnail_generation`:全量先把 `media_type='image'` 的 `thumb_status` 整表复位为 0(`thumbnail_commands.rs:550`),增量只处理既有 `thumb_status=0` 项;其余流程共用 `run_thumbnail_generation`。

**进度事件**(`THUMB_GEN_PROGRESS_EVENT = "thumb:gen_progress"`,`thumbnail_commands.rs:484-493`):原实现用 Tauri `Channel`,其生命周期绑定发起 invoke 的 webview,页面刷新后永久失联而后台任务照跑——现改为 **app 级事件广播 + `AppState` 快照**双轨:`publish_thumb_progress` 先更新快照再发事件(保证快照不落后于事件),`full_thumb_gen_status` 命令供 webview 重载瞬间查询回填。进度 IPC 按 100ms 节流(`PROGRESS_THROTTLE`,`thumbnail_commands.rs:612`),避免大库(8 万+ 条)逐张发送造成 IPC 风暴。

**代次守卫的终态发布**(`thumbnail_commands.rs:919-944`,同 `derive/pipeline.rs:141-153` 同款纪律):仅当本轮仍持有 token 槽(`finish(generation)` 返回 true)才清槽 + 发布终态事件,防止「停止→立即重启」时旧轮收尾覆盖新轮刚安装的状态。

### 3.3 冷门格式路由(`thumbnail/router.rs`)

`route_thumbnail`(`router.rs:48-67`)是纯判定函数(不查 DB、不持 `AppState`),按序:
1. `thumb_status` 已是 1/3(现成缩略图)→ `Existing`,即使源本是 exotic 格式。
2. 非 exotic(catalog 无 offering 或不认领 `Thumbnail` 能力)→ `Common`,交主 generator。
3. exotic 且任务 `Done` 且指纹有效 → `Existing`(一致性兜底,理论上 `thumb_status` 应已同步为 1)。
4. exotic 未完成(含未安装/未授权/平台不支持)→ `Exotic(resolution)`,**绝不**调用主 generator(会必然解码失败)。

exotic 插件体系权威见 `Spec09`。

### 3.4 派生流水线通用框架(`derive/pipeline.rs`)

复用 AI 流水线同款模式(`Producer → crossbeam channel → Consumer pool(rayon) → Writer`,`pipeline.rs:1-14`):

```
run_pipeline_blocking:
  1. reset_processing_derivations()          -- 孤儿恢复(status=1→0,毒任务防线,见 §5.2)
  2. 计算 disabled_kinds                     -- enable_* 开关 + kind_filter 覆盖(D-002,见 §4)
  3. backfill 各已实现 kind                  -- 按 DerivationKind::ALL 顺序入队缺行的 (item,kind)
  4. 若无 pending → 提前返回
  5. std::thread::scope 启动三个 OS 线程:
       produce_tasks   -- 批查 pending → mark processing(status=1) → 推入 channel
       consume_tasks   -- rayon::scope 内逐任务 panic_guard 包裹 kind::run(ctx) → 结果 channel
       write_results   -- 批量落库 + 封面镜像回 media_items + 通知画廊刷新
  6. 若 token 已取消(主动 stop)→ requeue_in_flight_derivations（优雅退回,不计孤儿数)
```

三个角色跑在**独立 OS 线程**(`std::thread::scope`)而非 rayon worker——它们多数时间阻塞在 channel 上,占用 rayon 线程会拖慢消费者内部真正的并行解码(`pipeline.rs:351-360` 注释)。

**让步与节流**(`consume_tasks`,`pipeline.rs:544-586`):扫描/缩略图运行中 → 硬暂停(高优先级层绝对优先);用户交互中 → 涓流(仅允许 1 个在途任务,而非全暂停——全暂停会让派生在持续浏览会话中彻底饿死)。派发前还需从**共享后台重活池**(`background_heavy_limiter`,与 exotic Worker 请求同一预算)取得 permit。

**用户开关 vs 显式 kind_filter(D-002 不变量)**:`enable_video_cover`/`enable_video_keyframes`/`ai_hq_cache_enabled` 三个后台开关决定 `disabled_kinds`,但**显式传入的 `kind_filter`(即 `start_derivation(kinds=...)`)会覆盖该屏蔽**(`pipeline.rs:219-223`)——用户在派生控制卡手动点名要跑的 kind,不受后台开关拦截(显式意图优先)。

**panic 隔离**(`consume_tasks`,`pipeline.rs:615-618`):`kind::run` 分发到大量第三方解码(epub `image::load_from_memory`/lofty/Media Foundation/WIC),裸跑时单个畸形文件 panic 会经 `rayon::scope→thread::scope→JoinError` 中止整条流水线;`panic_guard` 包裹后 panic → `status=3` error 行,仅废该项(与缩略图 `generator` 同款防线,`thumbnail/generator.rs:54-62`)。

**写入器镜像封面**(`write_results`,`pipeline.rs:678-831`):对 `produces_thumbnail()==true` 的 kind(`VideoCover`/`AudioCover`/`DocThumb`),在同一写锁内把 `thumb_status=1/thumb_path/thumbhash` 回填到 `media_items`(`update_thumb_result`),再同步常驻布局缓存(`state.apply_thumb_results`),使封面在滚出再滚回时无需整表重算,最后 `emit("db:media_enriched", ...)` 通知画廊刷新——`MediaThumb` 组件零改动即可显示新派生的封面(不变量,见 §4)。epub 页数(`page_count`)同一写锁内 upsert 进 `document_meta`。

### 3.5 各 kind 的 `run` 实现

| kind | 后端 | 输出 | 编译期门控(`is_implemented`,`kind.rs:90-110`) |
|---|---|---|---|
| `VideoCover`/`VideoKeyframes` | `derive/video.rs`,经 `video::backend_for`(Media Foundation) | 封面(复用缩略图缓存)/雪碧图(独立 `sprites/`) | `cfg!(windows)`——非 Windows 平台暂无后端(AVFoundation/FFmpeg 待补) |
| `DocThumb` | `derive/doc.rs`(仅 epub,zip+OPF 解析) | 封面(复用缩略图缓存)+ `page_count` | `true`(backfill 覆盖 pdf/epub/svg,但后端只跑 epub;pdf/svg 由前端离屏渲染,§3.4 doc 命名空间的实际路由见 `get_pending_derivations` 排除) |
| `AudioCover` | `derive/audio.rs`(lofty 内嵌封面提取) | 封面(复用缩略图缓存) | `true` |
| `AudioMeta` | `derive/audio.rs::run_meta`(桩,`not_implemented`) | 无 | `false`——标签/歌词实际由 enricher(补全阶段)回填,不走本派生流水线 |
| `AiThumb` | `derive/image.rs::run_ai_thumb`(WIC 优先/CPU `image` crate 回退) | AI 分析缓存(`ai_thumbs/`) | `true`,但实际入队/处理受 `ai_hq_cache_enabled` 开关门控(opt-in,默认关) |

**视频封面**(`video.rs::run_cover`,`video.rs:37-65`):时间戳选择(`min(1s, 时长10%)`,黑帧规避)已内聚到 `VideoBackend::cover` 内部;长边直接按缩略图 tier 请求,`encode_media_step` 的 resize 步骤因此成为直通(Media Foundation 侧已用 XVP 缩好)。

**关键帧雪碧图**(`video.rs::run_keyframes`,`video.rs:69-121`):采样 N 帧(`keyframe_count`)拼成单条水平条带(`image::imageops::overlay` 逐格贴入),质量恒用 `DEFAULT_WEBP_QUALITY`(与显示质量设置解耦)。原子落盘同 `write_atomic`。

**epub 封面**(`doc.rs::run_thumb`,`doc.rs:30-72`):`container.xml`→OPF rootfile→封面 href(EPUB3 `properties="cover-image"` → EPUB2 `<meta name="cover">` → 启发式匹配含"cover"字样的 image manifest item,三级回退,`find_cover_href`,`doc.rs:176-225`)→读取 zip 条目字节→复用 `encode_media_step`。同一份 OPF 顺带数 `<spine><itemref>` 数量作为页数近似(`count_epub_spine`,`doc.rs:121-138`,epub 是 reflowable 无固定分页,故为近似值)。

**AI/人脸分析缓存**(`image.rs::generate_ai_cache`/`generate_face_cache`,`image.rs:55-92`):共用 `write_short_edge_webp` 落盘核心,幂等(文件已存在即跳过,免重复解码)。`face_thumbs`(640px)与 `ai_thumbs`(336px)故意分目录不共用——若 640px YuNet 输入吃 336px CLIP 缓存会损害小脸召回率(权威见 `Spec06`)。

### 3.6 线程/进程模型总表

| 阶段 | 归属 |
|---|---|
| 缩略图 decode/encode/deferred-CPU 三池 | 独立 `std::thread::spawn` 线程(非 rayon),池宽由 `thumb_cpu_budget()`(`qos.rs:24-29`)限流 |
| 派生 produce/consume/write 三角色 | `std::thread::scope` 独立 OS 线程,consumer 内部 `rayon::scope` 独享 rayon 池做实际解码 |
| Windows/macOS 前后台 QoS | worker 线程内联调用 `apply_thread_qos`(`qos.rs:69-123`),Windows 用 `SetThreadPriority`+EcoQoS,macOS 用 `pthread_set_qos_class_self_np` |
| DB 读写 | `spawn_blocking` 或专用 `db_writer`/`db_read_pool`,不跨 `.await` 持锁(项目硬约束) |

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令一览(错误码统一走 `AppError`;详情权威见 `Spec10`)

| 分类 | 命令数 | 代表命令 |
|---|---|---|
| 缩略图(`thumbnail_commands.rs`) | 8 | `batch_request_thumbnails`、`start_full_thumbnail_generation`、`full_thumb_gen_status`、`regenerate_missing_thumb`、`clear_all_thumbnails` |
| 派生(`derive_commands.rs`) | 4 | `start_derivation`、`pause_derivation`、`stop_derivation`、`derivation_status` |

### 4.2 不变量清单

| 不变量 | 位置 | 为什么(违反会怎样) |
|---|---|---|
| 派生产物原子落盘(tmp→rename) | `thumbnail/generator.rs:64-94`(`write_atomic`),`derive/video.rs:114`、`derive/image.rs:120` 等全体调用点 | 命中判定普遍只查 `exists()`,直写崩溃会把半截文件永久当作有效缓存,UI 永久裂图(AGENTS.md 硬约束「派生产物一律原子落盘」) |
| `THUMB_TIERS` 5 档 `[64,128,256,512,1024]` 全后端唯一事实源 | `thumbnail/generator.rs:24-29` | 多处硬编码档位会漂移致 `thumb_path` 断言失败;必须与前端 `THUMB_SIZE_TIERS` 保持一致 |
| 显式 `kind_filter` 覆盖 `enable_*` 后台开关(D-002) | `derive/pipeline.rs:219-223` | 用户手动点名「全量/增量提取」的 kind 是显式意图,不该被静默拦截;无过滤的自动流水线仍完整尊重开关 |
| 关键帧勾选单源 `enable_video_keyframes` | `derive/pipeline.rs:204-206` | 避免设置项散落多处产生歧义来源 |
| 缓存单源 `thumbCacheDir`(前端)/ `ThumbConfig.cache_dir`(后端) | `thumbnail/generator.rs:97-116` | 路径计算集中一处,避免前后端/多命令各自推导路径产生偏差 |
| LRU 默认 10GB 单源 `DEFAULT_THUMB_CACHE_MAX_MB` | `thumbnail/cache.rs:313-317` | 2026-07-19 裁决:排序键维持 mtime(生成序非访问追踪),放宽预算降低驱逐触发频率,预算内 FIFO 与 LRU 无差别 |
| `orphan_count>=2` 转 `error`,毒任务防线 | `db/queries/derivations.rs:190-211` | 反复挂死的任务(如引发 MF 硬解死等的 mkv)若无限重投,每次启动都被同一批复现领取、无限循环(V22 引入,2026-07-22 故障复盘 125 个孤儿循环 6+ 轮) |
| `batch_finish_derivations` 正常完成即归零 `orphan_count` | `db/queries/derivations.rs:159-177` | 防偶发崩溃累计的孤儿计数,把后续真正偶发的失败误判为毒任务 |
| 优雅 stop 不计孤儿数(`requeue_in_flight_derivations`) | `db/queries/derivations.rs:213-225`,裁决 J10 | 用户主动 stop 是良性中断,不该占用毒任务防线的重试预算;与崩溃/force-quit 遗留孤儿(`reset_processing_derivations`,计数正当)拆两条独立路径 |
| 代次守卫:仅当前轮仍持槽才清 token/发终态 | `derive/pipeline.rs:141-153`、`thumbnail_commands.rs:919-944` | 无条件清槽会让「停止→立即重启」时旧轮收尾打断新轮刚安装的状态,与 AI 流水线 `finish_ai_analysis` 同款纪律 |
| 一次解码两份产物(AI 高清缓存)不上采样 | `thumbnail/generator.rs:398-411` | `LongEdge`/`ShortEdge` 缩放语义只降不升(WIC/`image` crate 原生保证);上采样会劣化分析精度且违反解码引擎契约 |

### 4.3 不可擅改项(标注出处)

- 生成引擎终局:多阶段流水线是唯一实现,不得回退到旧「Rayon 直线并发」方案(`thumbnail_commands.rs:24-30`,2026-07-10 真机 A/B 裁决:流水线 7.3s vs Rayon 直线 16-20s,慢 2.2~2.7 倍;旧分支已随 `daab834` 删除退役)。
- LRU 排序键维持 mtime(生成序),不做访问追踪(2026-07-19 裁决,权威见 `Spec14` 不变量总目录)。
- `THUMB_TIERS` 5 档改动须在发版前(缓存大面积失效,高代价变更)。

## 5. 边界与失败模式

### 5.1 LRU 驱逐产出「孤儿封面」——文件已删而状态短路自愈(experience §5)

**现象**:画廊滚动时 404 刷屏——LRU 驱逐(`enforce_cache_limit`)删掉了封面文件,但 `thumb_status=1` 使重建判断短路,自愈链被永久旁路。**根因**:「派生文件存在性」与「DB 状态记录」是两份真相,删文件一侧未同步复位另一侧。**as-built 现状**:`enforce_cache_limit`(`thumbnail/cache.rs:218-307`)已改为**事件驱动复位**——驱逐时同步计算被驱逐文件对应的 `media_items` DB 相对路径(`evicted_thumb_db_path`,`cache.rs:206-209`,`#[must_use]` 强制调用方处理返回值),调用方据此立即复位对应行的 `thumb_status`,而非等启动期全量 stat 扫描兜底。**红线**:任何删除派生文件的动作,必须在同一事务/同一调用序列里复位其状态行;自愈判据须「文件存在 ∧ 状态一致」双查。

### 5.2 派生流水线毒任务防线(2026-07-22 故障复盘)

反复挂死的 `(item,kind)` 任务(如引发 Media Foundation 同步 `ReadSample` 死等的特定 mkv,权威见 `Spec05`/`Spec06` §5.1)若无限重投会导致启动即挂死循环。修复见 §4.2 `orphan_count` 不变量。测试见 `db/queries/derivations.rs:466-615`(`poison_guard_tests` 模块,逐轮驱动验证三次孤儿转 error、正常完成归零、优雅 stop 不计数三条路径)。

### 5.3 已知边界(逐条)

| 边界 | 处理 |
|---|---|
| 损坏/畸形输入触发第三方解码器 panic | `panic_guard` 捕获(`thumbnail/generator.rs:54-62`,`derive/pipeline.rs:615-618`),单项标 `status=3`/`thumb_status=2`,不中止整条流水线 |
| 未知派生 kind 字符串(高版本遗留) | 生产者跳过(`derive/pipeline.rs:499-505`),保持 `status=1`(处理中)由认识它的后续构建恢复处理 |
| 用户短时间多次 stop/start | 优雅退回(`requeue_in_flight_derivations`)不计孤儿数,避免误伤为毒任务(§4.2) |
| 清缓存后派生行仍 `status=2`(死路径) | `clear_cache`/`clear_all_thumbnails` 同步调用 `reset_derivations_by_kinds`(2026-07-06 审查 P1-4),failed(`status=3`)项一并纳入重试(2026-07-07 修复,防瞬时性失败永久卡占位符) |
| 显式清缓存重生成 vs 已判毒任务 | `reset_derivations_by_kinds` 同时清零 `orphan_count`(2026-07-22 复核裁定):显式意图覆盖自动防线,否则曾判毒的行复位后一挂即被重新判毒,只得 1 次机会 |
| 视频/文档/音频后端未编译(`is_implemented=false`) | 该 kind 不入队(`backfill` 跳过),不会被误标为错误(`derive/kind.rs:82-90`) |
| epub 找不到封面/无 spine | 返回 `Err`(封面)/`None`(页数,不写 0),不假造产物(`derive/doc.rs:104,138`) |
| 音频无内嵌封面 | 返回 `Err`,派生状态置 3(不重试),前端回落音符占位(`derive/audio.rs:1-10` 模块文档) |
| pdf/svg 文档缩略图 | 后端无 native 栅格化器,`get_pending_derivations` 显式排除(`db/queries/derivations.rs:66`),留 `status=0` 交前端离屏渲染队列(`list_pending_doc_thumbs`)+ `store_doc_thumbnail` 回传 |
| 冷门格式(exotic)缩略图 | `route_thumbnail` 让路,绝不调主 generator(必然解码失败),见 §3.3;exotic 授权/安装态权威见 `Spec09` |

## 6. 重建指引(从零实现)

### 6.1 依赖顺序

1. **`db/schema.rs` 的 `media_items` 缩略图三列 + `media_derivations` 表**(V4)——先有状态机骨架,`orphan_count`(V22)可后补迁移。
2. **`thumbnail/cache.rs` 的路径方案**(`thumb_path`/`ai_cache_path`/`keyframe_sprite_path` 等)——所有产物寻址的地基,`cache_key` 是唯一输入。
3. **`thumbnail/exif_thumb.rs` 的编码器**(`encode_as_webp`/`encode_as_jpeg`)——`generator.rs` 与全体 `derive/*.rs` 的共同底座,先实现它才能测通任何一条产物线。
4. **`thumbnail/generator.rs` 的 decode/encode 两阶段** + `write_atomic` 原子写——图像缩略图先跑通,同时是派生 kind 复用的编码终点。
5. **`derive/kind.rs` 注册表**——定义 kind 枚举、`for_media`、`is_implemented` 编译期门控,不含任何实现。
6. **`derive/pipeline.rs` 通用调度框架**——生产者/消费者/写入器 + 续传 + 让步。此时可用桩 kind(全 `is_implemented=false`)跑通空流水线。
7. **各 kind 的 `run` 实现**(`video.rs`/`doc.rs`/`audio.rs`/`image.rs`)——按需逐个补齐后端,翻转 `is_implemented`。
8. **IPC 命令层**(`thumbnail_commands.rs`/`derive_commands.rs`)——最后接前端。

### 6.2 外部 crate

| crate | 版本 | 用途 | 归属 |
|---|---|---|---|
| `image` | 0.25 | JPEG/PNG/WebP/TIFF 解码,JPEG 编码兜底 | `thumbnail`/`derive` 全体 |
| `webp` | 0.3 | libwebp 有损/无损编码(经 `exif_thumb::encode_as_webp`) | 同上 |
| `fast_image_resize` | 4 | SIMD 双线性缩放(`resize_to_rgba`/`resize_short_edge_rgba`) | `thumbnail/generator.rs`/`derive/image.rs` |
| `thumbhash` | 0.1 | ThumbHash 编解码 + 占位平均色 | `thumbnail/thumbhash.rs` |
| `imageproc` | 0.27(权威见 `Spec04`) | 编辑/旋转(不在本篇路径内,交叉引用) | `editing/` |
| `quick-xml` | 0.36 | epub OPF/container.xml 解析 | `derive/doc.rs` |
| `zip` | 2 | epub 容器读取 | `derive/doc.rs` |
| `lofty` | 0.22 | 音频内嵌封面提取 | `derive/audio.rs` |
| `walkdir` | 2 | 缓存目录遍历(统计/GC/LRU) | `thumbnail/cache.rs` |
| `crossbeam-channel` | (workspace 版本待核实) | 生产者/消费者/写入器有界通道 | `derive/pipeline.rs`/`thumbnail_commands.rs` |
| `rayon` | (workspace 版本待核实) | 派生消费者内部并行解码 | `derive/pipeline.rs` |
| `windows` | 0.58 | `SetThreadPriority`/EcoQoS(前后台 QoS) | `thumbnail/qos.rs`(`cfg(windows)`) |

### 6.3 坑与教训(链 `docs/experience.md`)

- LRU 驱逐产出孤儿封面(experience §5)——事件驱动复位是根治方案,任何删产物动作必须同一序列复位状态行,见 §5.1。
- 派生流水线毒任务防线(V22,2026-07-22 故障复盘)——见 §5.2,改动 `reset_processing_derivations`/`batch_finish_derivations`/`requeue_in_flight_derivations` 前须理解三条路径各自的孤儿计数语义。
- 生成引擎 A/B 裁决(`docs/designs/2026-07-10-缩略图流水线深审与优化.md`)——流水线方案对 Rayon 直线并发的实测优势,改动生成引擎前先读该工作线记录,勿凭直觉回退。
- `panic_guard`/`write_atomic` 是缩略图与派生两条线共用的两道防线(`generator.rs:41-94`),新增 kind 的 `run` 实现应默认走这两道防线而非裸解码/裸写盘。

### 6.4 验收

| 验证面 | 命令/路径 |
|---|---|
| Rust 单测(缩略图/缓存/派生) | `cargo test -p scrollery`(涉及 `thumbnail::`/`derive::` 模块的单测,如 `generator.rs:659+`、`cache.rs:631+`、`router.rs:69+`、`qos.rs:125+`、`thumbhash.rs:71+`、`exif_thumb.rs:146+`、`kind.rs:224+`、`derivations.rs:404+`) |
| clippy | `cargo clippy --workspace`(项目硬约束) |
| GUI 真机验收 | ⏸ 未做(本篇仅覆盖代码 as-built,不含真机手测结论;历史真机验收记录见 `docs/designs/2026-07-10-缩略图流水线深审与优化.md` §5) |

## 7. 关联

- 上游正典:`../refactor_2026/Part3_缩略图派生与GPU引擎.md`(缩略图/派生的原始决策与权衡,理由链见其 §2-3)。**注意**:Part3 未覆盖色彩管理与视频硬解码细节——这两块分属 `./Spec04_图像与色彩管线.md`(ICC→sRGB 投影、查看器色域)与 `./Spec05_视频与音频.md`(Media Foundation/DXVA/XVP,本篇 §3.5 只写「关键帧 sprite 派生」这一层,解码机制权威在 `Spec05`)。Part3 的 mac 平台地基章节(§3.7)与本篇 as-built 现状(视频派生 `cfg!(windows)` 门控)存在落差,当前非 Windows 平台视频派生仍无后端,以代码为准。
- 相关设计:`docs/designs/2026-07-10-缩略图流水线深审与优化.md`(生成引擎 A/B 裁决、13 项深审发现);`docs/experience.md` §5(LRU 孤儿封面,本篇 §5.1 引用)。
- 相关规格篇:`Spec01`(数据层,`media_items`/`media_derivations` 全表定义权威处)、`Spec04`(图像与色彩管线,ICC 投影/编辑链权威处)、`Spec05`(视频与音频,Media Foundation 解码机制权威处)、`Spec06`(AI/人脸/OCR,AI 分析缓存消费方与 `ai_hq_cache_enabled` 开关权威处)、`Spec09`(插件平台与 exotic,冷门格式让路的授权/安装态权威处)、`Spec10`(IPC 契约,错误码全量清单权威处)、`Spec11`(前端架构,`MediaThumb`/派生控制卡/`useDerivationAutoStart` 权威处)、`Spec14`(不变量总目录)。
