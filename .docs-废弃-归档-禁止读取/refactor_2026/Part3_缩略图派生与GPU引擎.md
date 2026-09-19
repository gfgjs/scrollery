---
id: 2026-06-26-Part3_缩略图派生与GPU引擎
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 3 · 缩略图派生与 GPU 引擎

> 依赖：[Part0](Part0_总纲与产品定稿.md)（§4 功能矩阵、§11 波次、§12 约定）、[Part1 数据层](Part1_数据层.md)（`cache_key`/schema/`document_meta` DAO 补全）、[Part2 扫描与画廊流水线](Part2_扫描与画廊流水线.md)（消费其布局缓存 `apply_thumb_results` 的 O(batch) 回填口 + SourceChanged 失效钩子）。
> 状态：定稿待执行（已并入 terminal review；正文即权威）。执行前必读 Part0 §13 + Part1 §7 + Part2 §7 + 本文。**旧 docs/记忆不可轻信，以代码实测为准。**

---

## §1 目标与范围

### 1.1 本 Part 解决什么

缩略图与派生是「百万级流畅浏览 + 选片速度」的视觉引擎，也是「<10MB 核心 + 系统原生 API」的主战场。本 Part 交付：

1. 🔴 **CPU 解码瓶颈修复**（Part0 §1 矛盾 #7/#9、memory `gotcha-ai-analysis-cpu-decode`）：解码时**降采样直出目标档位**（WIC `IWICBitmapScaler` / 内嵌缩略图 / `image` crate `thumbnail`），而非解全分辨率再缩；档位吸附 `[120/240/480/960]`。这是「分析 CPU 99% / GPU 45%」与缩略图生成慢的共同根因。
2. **WicEngine 注册进 EngineArena**（Part0 §11 line 513）：把 Windows 原生 WIC 解码路径接入引擎仲裁，解锁 HEIC/HEIF 等现代格式（现代 iPhone 默认），并为「系统原生 = 轻量核心」铺路。
3. **缩略图缓存治理**：缓存金字塔（`cache/thumbnails/{size}/{2-hex}/{key_hex}.webp`）、`cache_key` 免疫盘符（Part1）、GC / 孤儿清理 / 磁盘占用上限。
4. 🔴 **派生产物清理 + SourceChanged 失效对接**（接 Part2 §3.3）：源文件变更/删除时，缩略图与各类派生（封面/关键帧/ai_thumb）随之失效或清理，不留孤儿、不显示旧图。
5. **派生让步模型加固**（继承 Part0 §12、memory `gotcha-background-yield-model`）：派生/AI 对交互让步（`note_interaction`），enrichment 绝不让步；厘清派生协调线程（`std::thread::scope`）与 `reserved_core_pool` 的边界。
6. **ai_thumb 派生 + `ai_hq_cache_enabled` 默认值引导**（Part0 §1 矛盾 #9）：短边 336 的 AI 专用派生喂 CLIP（避免解原图饿死 GPU）；解决「优化默认关闭形同虚设」。
7. **mac 原生媒体层（objc2 桥）**（Part0 §11 line 226/513，Win+mac 并重的直接代价）：Image I/O（图像/HEIC/RAW）+ AVFoundation（视频帧）+ PDFKit/QuickLook（文档）绑定，工作量约 Windows 路径 2–3 倍。
8. **各媒体派生强化**：视频封面/关键帧（Media Foundation 取帧约定）、文档（pdf/svg/epub）、音频封面（lofty）。
9. **零散正确性修复**：epub 扩展名漏登记（[format.rs](../../src-tauri/src/utils/format.rs) 1 行，致 epub 整链静默死）；TIFF 解析超时（与 Part2 §3.9.2 协同，避免损坏 TIFF 挂死 rayon worker）。

### 1.2 不在本 Part（归属其它 Part）

- `cache_key` 定义 / 缩略图相关 schema 列 / `document_meta` DAO 补全 → **Part1**（本 Part 消费）。
- 布局缓存 `apply_thumb_results` 的**缓存侧实现** → **Part2**（本 Part 只调其回填接口 + 触发失效）。
- **AI 推理本身**（CLIP ONNX 前向、向量写入）→ **Part4**（本 Part 只产 `ai_thumb` 喂给它）；人脸 → Part4。
- **exotic 冷门格式解码**（psd/raw 专有等经 sidecar worker）→ **Part6**（本 Part 走标准/原生派生；exotic 走 worker 协议）。
- **前端查看器/阅读器/画廊渲染**（缩略图展示、离线灰显、阅读器 UI）→ **Part5**（本 Part 提供派生产物 + `asset://` URL + thumbhash 占位）。

### 1.3 全局约定（继承 Part0 §12）

派生/AI 对交互让步（`note_interaction` → `should_yield_derivation`），**enrichment 绝不让步**；`cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`（免疫盘符，缓存跨卷重挂 100% 命中）；缩略图经 `asset://` 协议直通（绕 WebView2 IPC 大字节瓶颈）；rayon 内永不 `.await`；CPU 密集走 `spawn_blocking`/rayon；新增 DAO/命令返回 `AppError`；中英双语注释；改后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（代码取证，文件:行）

> workflow `w8rjfak3c`（4 agent 并行测绘引擎/缩略图/派生/缓存+mac+零散）回传，逐项 file:line 核实。**与旧 docs/记忆冲突处，以本节为准。**

### 2.1 解码引擎层与 CPU 解码瓶颈

**架构**（[engine/mod.rs](../../src-tauri/src/engine/mod.rs):28-38、[traits.rs](../../src-tauri/src/engine/traits.rs):35-61）：

- `ImageEngine` trait：`name`/`supported_formats`/`can_handle`/`decode`/`extract_embedded_thumb`。
- `EngineArena` 持 `Vec<Arc<dyn ImageEngine>>`，`engine_for()` 线性扫描首个 `can_handle`（字符串包含、大小写敏感）。
- 🔴 **`phase1()` 只注册 `ImageRsEngine`**（jpg/jpeg/png/webp/bmp/gif/tif/tiff，8 扩展名）。**`WicEngine` 不在 arena**，由独立工厂 `get_gpu_engine("wic")` 按需构造。⚠️ **实测 `WicEngine.supported_formats` = jpg/jpeg/png/bmp/tif/tiff/webp/gif + heic/heif/avif/ico（wic_engine.rs:22-27），几乎全格式、非仅现代格式**（对抗验证修正取证初稿，直接影响 §3.2 注册顺序）。

**解码路径派发**（[generator.rs](../../src-tauri/src/thumbnail/generator.rs):239-364）：

- `strategy=="gpu"` → `try_gpu_decode` → `get_gpu_engine("wic")` 成功用之；失败 `DeferredToCpu` → `try_cpu_decode` → `arena.engine_for(format)`。
- `strategy!="gpu"` → 直接 `try_cpu_decode` → `arena.engine_for`。
- 🔴 **arena 只有 ImageRsEngine、不认 heic** → 非 gpu 策略下 HEIC/HEIF/AVIF 直接 `UnsupportedFormat`，**无 image-rs fallback**。

🔴 **CPU 解码瓶颈（核心，Part0 §1 矛盾 #7/#9）**：

| 路径 | 解码行为 | 证据 |
|------|---------|------|
| **GPU（WicEngine）** | `try_gpu_decode` 传 `ResizeHint::LongEdge` → `IWICBitmapScaler`（Cubic 懒求值）→ `CopyPixels` 时按需缩，**真降采样**，不解全图 | wic_engine.rs:77-125 |
| **CPU（ImageRsEngine）** | `try_cpu_decode` 先试 `try_exif_thumb`（仅 JPEG 内嵌）；失败 → `engine.decode(abs, **None**)` **解全分辨率** → `encode_media_step` 再 `fast_image_resize` 二次缩 | generator.rs:355-364 |

- ⚡ **关键**：`ImageRsEngine` **已实现 `ResizeHint` 支持**（image_rs.rs:54-91，`resize_exact(CatmullRom)`），但 `try_cpu_decode` 调用时传 `None` → 能力闲置 + 多一次全图解码 + 一次额外缩放。**改传目标档位即修复**。
- 档位吸附 `THUMB_TIERS=[120,240,480,960]`（generator.rs:27），`snap_to_tier()` 取最近档；`generate_thumbnail`/`process_deferred_cpu` 入口均先 snap。
- `ai_hq_cache=true` 且宽幅时 `decode_long_edge` 让 GPU 解码长边略放大，一份 buffer 出「缩略图 + AI 缓存」两份，免二次解码。

**GPU 名实不符**（wic_engine.rs:6-9 imports 仅 `windows::Win32::Graphics::Imaging`）：WIC 全链系统内存，**无任何 D3D/DXGI/DirectML/纹理绑定**。注释「hardware acceleration」无源码依据（至多 OS codec 触发 CPU 固定功能 JPEG 解码，Rust 侧不可控不可验）。**memory「WIC『GPU 解码』实为 CPU」属实**；`gpu/` 模块名是「高性能」概念标签，非 GPU compute。

### 2.2 缩略图生成流水线

**两触发入口**（[thumbnail_commands.rs](../../src-tauri/src/ipc/thumbnail_commands.rs)）：

- `batch_request_thumbnails`（前端可见性按需）：查 DB → fast_results(status 1/3/2 回送 + `apply_thumb_results`) → needs_gen → exotic 路由过滤 → `spawn_blocking` 两阶段 → 写 DB → 回填。
- `start_full_thumbnail_generation`（全量重建）：先 `UPDATE thumb_status=0`，再走同一两阶段。
- ~~`USE_PIPELINE=true`（thumbnail_commands.rs:24，硬编码常量）→ 走多阶段；方案一（rayon 直线）残留为**死代码**~~ **🔴 本行已被 2026-07-10 裁决推翻**：开关已**运行时化**（`AppState::thumb_use_pipeline` AtomicBool，配置键 `thumb_use_pipeline` 默认 true=方案二，设置页「开发者工具」可切，`daab834`），方案一不再是死代码而是 **A/B 待实测分支**；两方案公平性缺陷已修（方案一进度风暴/zip 错位雷、方案二 Phase2 串行）。实测后裁决去留，见 [2026-07-10-缩略图流水线深审与优化.md](../designs/2026-07-10-缩略图流水线深审与优化.md) §3。

**两阶段并发**：decode threads = `cores×2`(最少4)、encode threads = `cores`（thumbnail_commands.rs:406/447/749/796）；full-gen chunk(50) 分批（:709），每攒 50 条 flush 回填（:867）。

🔑 **让步落点**：缩略图流水线**本身不调** `note_interaction`/`should_yield_derivation`；让步在**派生侧**（缩略图运行时派生让步，见 §2.3）。即「缩略图 > 派生」的优先级是派生主动退避实现的。

**cache_key 与缓存目录**（[hash.rs](../../src-tauri/src/utils/hash.rs):27-46、[cache.rs](../../src-tauri/src/thumbnail/cache.rs):7-75）：

- `cache_key = xxh3_64("{rel_path}/{file_name}|{file_mtime}") as i64`；文件名 `{:016x}`（16 位小写 hex）。
- 缩略图：`{cache_dir}/thumbnails/{size}/{hex[0..2]}/{hex}.webp`（size=snap 档位）；DB 存相对 `"{size}/{prefix}/{hex}.webp"`。
- AI 缓存：`{cache_dir}/ai_thumbs/{hex[0..2]}/{hex}.webp`（独立目录，不受 LRU）。
- 输出 WebP（默认 lossy），失败回退 JPEG q85（exif_thumb.rs:115）。

**router 路由**（[router.rs](../../src-tauri/src/thumbnail/router.rs):48-67，纯函数不查 DB）：① status 1/3 → `Existing`；② 常见格式 → `Common`（走主 generator）；③ exotic done+fingerprint 有效 → `Existing`；④/⑤ exotic 未完成 → `Exotic(resolution)`，绝不调 generator。**非 image 类型（视频/音频/文档）在 generator `decode_media_step` 直接返回 `thumb_status=2` stub**（generator.rs:271）——其真实缩略图走**派生流水线**（§2.3），非 generator。

**exif_thumb 快速通道**（[exif_thumb.rs](../../src-tauri/src/thumbnail/exif_thumb.rs):11-75）：`extract_embedded_thumb`（仅 JPEG 读 EXIF IFD1）→ 内嵌 JPEG `load_from_memory` → `max(w,h)>=120` 即用 → 读主图 orientation 旋转 → 写 WebP + thumbhash。⚠️ **质量权衡（取证修正，review w3mniao14）**：`max(w,h)<120` 时返回 None → generator.rs:358 **回退全量解码**（回退**存在**，初稿「无回退」有误）；但 `120≤内嵌<target` 时**有意 upscale**（160px 内嵌图 upscale 到 480/960 质量劣化）——劣化仅发生在「内嵌够 120 但不足大档位」区间、此区间无回退。

**thumbhash**（[thumbhash.rs](../../src-tauri/src/thumbnail/thumbhash.rs):17-55）：三路径生成（exif 快速 / 小文件 direct / encode 阶段），统一缩到 100×100 → `rgba_to_thumb_hash` → ~28 字节 → 存 `media_items.thumbhash` BLOB；前端 `thumbhashToDataURL` 作 `<img>` 占位。⚠️ `strategy=="direct"` 跳过 thumbhash（generator.rs:219）→ direct 下无占位。

**asset:// 直通**（tauri.conf.json:42-54 + [lib.rs](../../src-tauri/src/lib.rs):188-209）：静态 scope 白名单 + 运行时 `allow_directory(cache_dir + 各 scan root, recursive)`；前端 `convertFileSrc({cacheDir}/thumbnails/{thumbPath})` → `asset://`，绕 WebView2 IPC 字节。scope 外 403 防逃逸；`thumbPath` 来自 DB 非用户输入。

### 2.3 派生协调与让步模型

**DerivationKind**（[derive/kind.rs](../../src-tauri/src/derive/kind.rs):44-108）6 变体：`VideoCover`/`VideoKeyframes`/`DocThumb`/`AudioCover`/`AudioMeta`/`AiThumb`。`ALL` 按优先级高→低排（AudioMeta/AudioCover/VideoCover/DocThumb/AiThumb/VideoKeyframes），backfill 按序入队。`is_implemented` 门控：Video* 仅 `cfg!(windows)`；DocThumb/AudioCover/AiThumb=true；**AudioMeta=false**（enricher 处理，永不入队）。

**协调线程**（[derive/pipeline.rs](../../src-tauri/src/derive/pipeline.rs):255-291）：`tokio::spawn` → `spawn_blocking` → `std::thread::scope(|s|)` 开三 OS 专线程 **Producer / Consumer pool / Writer**；Consumer 内再 `rayon::scope` 派发并发解码。**刻意不用 rayon worker 承载 P/C/W**（长生命周期 + 多阻塞 channel，占 rayon 线程会拖慢真并行解码）。

**调度**（pipeline.rs:36-37/293-390）：`BATCH_SIZE=256`、`CHANNEL_CAPACITY=512`（crossbeam bounded）。Producer 取 256 pending → status=1 → 推 channel；Consumer rayon 并行 `kind::run`；Writer 批量更 status=2/3 + 回填封面 thumb。

🔑 **让步**（pipeline.rs:309-314/413-416）：`should_yield_derivation() = is_scan_or_thumb_running() || is_interactive()`，两处调用：Producer 顶部命中 → sleep 500ms continue（不取批）；Consumer dispatch → sleep 120ms（不派新重解码）。`is_interactive()` 基于 `AtomicI64 interactive_until_ms`，`note_interaction()` 在 layout_commands.rs:44-46/128/144 写 `now+1500ms`。**优先级阶梯：scan > thumbnail > derivation > AI。**

**池边界**（enricher.rs:40-50 / state.rs:209-215 / exotic/coordinator.rs:217）：
- `reserved_core_pool`（`available_parallelism()-1`）**仅 enrichment 用**（保留 1 核给前台 compute_layout）；**派生不用 reserved，用全局 rayon 池**。
- `BackgroundHeavyLimiter`（FIFO 公平 Semaphore，额度=`available_parallelism()`）：**派生与 exotic 共享同一实例**（pipeline.rs:423 acquire），同级不互相让步、靠公平额度防饿死。

### 2.4 各媒体派生

- **视频封面**（[derive/video.rs](../../src-tauri/src/derive/video.rs):26-57 + [media_foundation.rs](../../src-tauri/src/video/media_foundation.rs):100-383）：probe 时长 → 封面时间 `min(1s, 10%)`；`IMFSourceReader` + `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` 出 RGB32(BGRX) 到系统内存（无 D3D 回读）；DXVA 自动；读 `MF_MT_VIDEO_ROTATION` 归一化 + `MF_MT_DEFAULT_STRIDE` 负 stride bottom-up 处理；最多 5 次 +0.5s 跳暗帧（亮度<16）；`apply_rotation` 90/270 交换宽高 → encode WebP + thumbhash。
- **视频关键帧雪碧图**（video.rs:61-105 + media_foundation.rs:142-198）：`[5%,95%]` 均采 `KEYFRAME_COUNT=10` 帧 → 格高 200px、格宽按显示比例 → 水平拼接 → WebP 写 sprite dir；`thumbhash=None`。
- **Motion Photo companion**（[media_commands.rs](../../src-tauri/src/ipc/media_commands.rs):126-179）：Apple Live → DB 查配套 MOV → 绝对路径；Google/Samsung（`has_embedded_video=1`）→ `extract_embedded_mp4` 从 JPEG 尾部读 MP4 → 写缓存 → 返回。静止封面走正常 image enricher。
- **文档**（[derive/doc.rs](../../src-tauri/src/derive/doc.rs):29-196）：**epub** 后端 zip 解封面（container.xml → OPF rootfile → 按 EPUB3 cover-image/EPUB2 meta cover/启发式取 href → load → WebP）；**pdf/svg** 后端 `is_implemented=true` 建行但 `get_pending_derivations` SQL **排除** pdf/svg → 前端 `list_pending_doc_thumbs` 领取、`DocThumbRenderer.vue` 离屏渲染截图 → `store_doc_thumbnail` 回传落盘（与 epub 产物路径同构）。
- **音频封面**（[derive/audio.rs](../../src-tauri/src/derive/audio.rs):19-51）：`lofty read_cover` 读内嵌封面 → load → WebP；无封面 status=3（不重试，前端音符占位）。
- **ai_thumb**（[derive/image.rs](../../src-tauri/src/derive/image.rs):31-61 + pipeline.rs:140-149）：**opt-in 默认关**（`ai_hq_cache_enabled` 非 'true' 入 `disabled_kinds`，不 backfill 不领取）。`run_ai_thumb`：若缩略图流水线已顺带产出 ai_cache 则跳过；否则 `decode_short_edge`（WIC GPU 优先/image CPU 回退）短边 `AI_CACHE_SHORT_EDGE=336`（cache.rs:59）→ WebP 写 ai_cache_dir。`produces_thumbnail()=false`（不回填 thumb_status，非显示缩略图）。普通缩略图短边≥336 时顺带产 AI 缓存（generator.rs:309-326/477-504），避免 ai_thumb 重复解码。

### 2.5 派生失效缺口（接 Part2 P1，更广）

🔴 **真 bug（媒体派生不随源失效）**（[queries.rs](../../src-tauri/src/db/queries.rs):510-573 + [fast_scan.rs](../../src-tauri/src/scanner/fast_scan.rs):395-412）：

- `UpsertOutcome::SourceChanged` 时**仅** `invalidate_exotic_tasks_for_item`（queries.rs:3512-3523）；`media_items` 重置 `thumb_status=0/thumb_path=NULL/thumbhash=NULL`（queries.rs:556-560）。
- **`media_derivations` 表无任何失效**（既不 DELETE 也不 status=0）。派生 backfill 走 `INSERT OR IGNORE`：若 `(item_id, kind)` 行已存在且 `status=2(done)` → 不重建 pending → **视频封面/关键帧/文档缩略图/ai_thumb 永久停留旧版**。
- 与 Part2 §2.2 P1（`image/video/audio_meta` 不失效）**同源**：SourceChanged 的失效面覆盖不全。**两 Part 须统一为一个 `invalidate_derived_for_item(id)`**（同时清 meta + media_derivations + 缩略图状态 + exotic，事务内）。

### 2.6 缓存治理（GC / 孤儿 / 上限）

**LRU**（[cache.rs](../../src-tauri/src/thumbnail/cache.rs):153-228 + [lib.rs](../../src-tauri/src/lib.rs):354-358/169-173）：

- `enforce_cache_limit(cache_dir, max_mb)` **启动后 60s 触发一次**（非周期循环），扫 `thumbnails/` + `ai_thumbs/`，按 mtime 升序淘汰至 80% 目标；默认上限 `thumb_cache_max_mb=1024`。
- 🔴 `sprites/`（关键帧雪碧图）+ `motion_videos/`（提取的 MP4）**有意排除 LRU**（cache.rs:162-167）→ **无上限、随时间无限增长**。
- 🔴 **无孤儿扫描**：DB 已删项的磁盘缓存永不被专门清理（只能等 LRU 按 mtime 裁）。
- 🔴 **无 IPC 暴露**：用户**不能查缓存占用、不能手动清理**；近似 LRU（不刷新被跳过文件 mtime）且 thumbnails 与 ai_thumbs 混排，可能优先驱逐 AI 缓存。

**删除不清缓存**（[file_ops_commands.rs](../../src-tauri/src/ipc/file_ops_commands.rs):386-415 + queries.rs:1586-1601）：软删（仅置 is_deleted）、硬删（送回收站 + DELETE DB 行）、目录级联删——**三条路径均不清理** thumbnails/ai_thumbs/sprites/motion_videos → 孤儿持续累积。

### 2.7 mac 平台现状（编译都过不了）

🔴 **mac 当前无法编译，更遑论功能**（[Cargo.toml](../../src-tauri/Cargo.toml):53 + grep 全仓）：

- **零 mac FFI**：无 objc/objc2/ImageIO/AVFoundation/PDFKit/CoreGraphics/QuickLook 依赖；无 `cfg(target_os="macos")` 块。
- 🔴 **`windows` crate(v0.58) 无 `cfg(windows)` 门控** → mac target 引入 Win32 FFI **直接编译失败**。
- 图像：`phase1()` 只注册 `ImageRsEngine`（纯 Rust 跨平台），但 GPU 路径 `get_gpu_engine("wic")` 调 `windows::Graphics::Imaging` → mac 编译即失败。
- 视频：[video/mod.rs](../../src-tauri/src/video/mod.rs):21-22/83-93 `#[cfg(windows)]` 门控 `media_foundation`，`backend_for()` 非 windows 返回 `None` → **mac 所有视频 unsupported**。
- 文档：epub（zip+image 纯 Rust）+ pdf/svg（前端 canvas）跨平台可用。
- **缺口**：① HEIC/HEIF/AVIF 解码（mac 应走 Image I/O）；② 视频取帧/封面/雪碧图（应走 AVFoundation）；③ RAW（两平台都未实现，engine/mod.rs:9-10 Phase 2 占位）。

### 2.8 零散矛盾复核（四条全证实）

| # | 旧述/矛盾 | 实测结论 | 证据 |
|---|----------|---------|------|
| ① | epub 整链静默死 | **证实**：`classify_media_type()` 无 epub 条目 → 返回 `None` → 扫描器忽略、不入 DB；`DOC_THUMB_FORMATS` 含 epub、doc.rs 后端完整，但上游登记缺失永远触达不到 | [format.rs](../../src-tauri/src/utils/format.rs):63-67 / kind.rs:126 |
| ② | TIFF「tokio 超时」 | **证伪旧 plan**：实为 `std::thread::scope + join()`，**无超时**（panic 隔离非限时）；超大/损坏 TIFF 仍无限阻塞（与 Part2 §3.9.2 同） | [metadata.rs](../../src-tauri/src/scanner/metadata.rs):52-60 |
| ③ | `ai_hq_cache_enabled` 默认 | **证实 false**：`INSERT ... VALUES('ai_hq_cache_enabled','false')` + 读取 `unwrap_or(false)` → CPU 解码优化默认关闭、形同虚设 | [schema.rs](../../src-tauri/src/db/schema.rs):33 / lib.rs:175-179 |
| ④ | document_meta 表无 DAO | **证实**：表 V1 起存在（item_id/page_count/doc_subtype）但全仓**零 DAO**（无 INSERT/SELECT/UPDATE）→ `page_count/doc_subtype` 永远 NULL | schema.rs:172-178 / format.rs:85-95 |

### 2.9 现状问题清单（编号 → §3 设计逐一对应）

| # | 问题 | 证据 | 对应设计 |
|---|------|------|---------|
| Q1 | CPU 解码全分辨率再缩（`try_cpu_decode` 传 `resize=None`，已有 ResizeHint 能力闲置） | generator.rs:359 / image_rs.rs:54-91 | §3.1 |
| Q2 | WicEngine 未注册 arena → 非 gpu 策略 HEIC/HEIF/AVIF 直接失败、无 fallback | engine/mod.rs:29-31 / generator.rs:330-364 | §3.2 |
| Q3 | `is_phase1_image` 不含 heic（⚠️ **非 scanner 漏扫**——heic 已被 `classify_media_type` 收录入库，format.rs:48-49）→ 实际后果：heic 在 fast_scan 走 `cheap_phase2_dimensions` 占位尺寸、不读真实宽高；叠加 WicEngine 未注册 arena → 非 gpu 策略下 heic 无法解码缩略图。能力集分裂须统一 | format.rs:129 / format.rs:48-49 / fast_scan.rs:112 | §3.2 |
| Q4 | exif_thumb 在 120≤内嵌<target 时 upscale 大档位致质量劣化（<120px 已回退全解，初稿「无回退」修正） | exif_thumb.rs:49/63 | §3.1 |
| Q5 | `media_derivations` 不随 SourceChanged 失效 → 视频/文档/ai 派生停留旧版 | queries.rs:510-573 / fast_scan.rs:400 | §3.4 |
| Q6 | LRU 仅启动一次、无周期；sprites/motion_videos 无上限无限增长 | lib.rs:354-358 / cache.rs:162-167 | §3.3 |
| Q7 | 删除媒体不清缓存 → 孤儿累积 | file_ops_commands.rs:386-415 | §3.3 §3.4 |
| Q8 | 无缓存占用查询/手动清理 IPC | grep（仅 enforce 一处） | §3.3 |
| Q9 | mac windows crate 无门控 → 编译失败；视频/HEIC mac 无后端 | Cargo.toml:53 / video/mod.rs:21-22 | §3.7 |
| Q10 | epub 漏登记 classify_media_type → 整链静默死 | format.rs:63-67 | §3.9 |
| Q11 | TIFF 无真超时 | metadata.rs:52-60 | §3.9（与 Part2 §3.9.2 协同） |
| Q12 | `ai_hq_cache_enabled` 默认 false，优化形同虚设 | schema.rs:33 | §3.6 |
| Q13 | document_meta 零 DAO，page_count/doc_subtype 永空 | schema.rs:172-178 | §3.8（DAO 在 Part1） |
| Q14 | direct 策略跳过 thumbhash → 无占位图 | generator.rs:219 | §3.1 |
| Q15 | 缩略图死代码（USE_PIPELINE 常量 true，方案一残留） | thumbnail_commands.rs:24 | §3.1（清理） |
| Q16 | sample_to_rgba 边界 `s+3 >= cur` 误判 break（⚠️ **精确修法是改 `> cur`、仍 break，非改 continue**——改 continue 会在 stride>宽×4 时令 x 继续推进、产生越界 unsafe 访问）→ 宽幅视频帧尾像素填黑 | media_foundation.rs:355-372 | §3.8 |

---

## §3 设计方案

> 原则：**瓶颈/卖点（CPU 解码、HEIC、缓存治理）+ 数据正确性（派生失效）先落 → mac 平台地基（大工程）→ 零散修复**。多数修复是「能力已在、没接通」（Q1/Q5/Q10），改动小、收益大，先做。所有新增 DAO/命令返回 `AppError`，继承让步阶梯。

### 3.1 CPU 解码降采样 + 档位 + 占位 + 死代码（Q1 + Q4 + Q14 + Q15）

🔴 **3.1.1 CPU 路径接通 ResizeHint（Q1，一行级核心修复）**：`try_cpu_decode` 把 `engine.decode(abs, None)` 改为传目标档位的 `ResizeHint`：

```rust
// 现: engine.decode(abs_path, None)  // 解全分辨率 → encode 再 fast_image_resize 二次缩
// 改: 传 snap 后的目标档位，让 ImageRsEngine 在 decode 内 resize（能力已在 image_rs.rs:54-91）
let hint = ResizeHint::LongEdge(snap_to_tier(config.size));
engine.decode(abs_path, Some(hint))
```

- 收益：省一次全图驻留 + 一次额外缩放；与 GPU 路径（WIC scaler）语义对齐。
- ⚠️ **保真权衡**：`image` crate 的 `resize_exact(CatmullRom)` 是全解后缩、**非解码期降采样**（image-rs 无 JPEG DCT scaled decode）→ 内存峰值仍是全图，但省掉第二次 `fast_image_resize`。**真正的「解码期降采样」只有 WIC/Image I/O 原生路径能做**（§3.2/§3.7）；CPU 纯 Rust 路径只能优化到「解一次 + 缩一次」。文档须诚实标注此边界，不夸大。
- 进一步（可选）：对 JPEG 用 `jpeg-decoder` 的 `scale_denom`（1/2/4/8 DCT 降采样）在解码期直接出小图——但需评估 `image` crate 是否暴露该能力，列 §4 P2 可选。

**3.1.2 exif_thumb 质量守门（Q4）**：内嵌缩略图 `max(w,h) < target_size` 时**不 upscale**，回退全解码：

```rust
// 内嵌图够大才用；不足目标档位则回退 full decode（避免 160→960 劣化）
if embedded_max_edge >= snap_to_tier(target_size) { use_embedded } else { fall_through_to_full_decode }
```

- 例外：极小档位（120/240）内嵌图通常够用，仍走快速通道（选片速度优先）；大档位（480/960）才严格。

**3.1.3 direct 策略补 thumbhash（Q14）**：`strategy=="direct"` 也生成 thumbhash（占位图是体验底线，不应因策略丢失）。改 generator.rs:219 条件，去掉 `strategy != "direct"` 短路。

**3.1.4 ~~死代码清理~~ 生成引擎 A/B 裁决（Q15，🔴 2026-07-10 修订）**：原「`USE_PIPELINE` 恒 true → 删方案一残留」定性已推翻——「方案一在多核上性能最好」系未经实测的历史直觉，未经实测不定死（开发期不冻结契约）。过程方案：开关运行时化（`daab834`，开发者工具可切、每次生成命令读一次）+ 两方案 A/B 公平性修复（方案一进度 IPC 零节流 / `filter_map`+`zip` 结果错位雷 / 方案二全库 Phase 2 单线程）。**✅ 2026-07-10 用户真机 A/B 定案（同库同标准全量生成）：方案二（多阶段流水线）稳定 7.3s vs 方案一（Rayon 直线）16-20s（慢 2.2~2.7 倍）——「方案一多核最好」直觉证伪** → 方案二成**唯一生成引擎**，方案一分支 ~300 行×2 命令 + 运行时开关 `thumb_use_pipeline` 全链删除退役（`cfe63d8`）。协议、实测数与退役细节见 [2026-07-10-缩略图流水线深审与优化.md](../designs/2026-07-10-缩略图流水线深审与优化.md) §3.4/§3.5 + `cfe63d8` 文件头注。

### 3.2 WicEngine 注册 + 格式集统一（Q2 + Q3）

🔴 **3.2.1 WicEngine 注册进 EngineArena（顺序关键，对抗验证修正）**：在 `cfg!(windows)` 下把 `WicEngine` **追加到 ImageRsEngine 之后**，使 `engine_for("heic")` 由 WIC 接住，HEIC/HEIF/AVIF/ICO 走 CPU fallback 链也能解（不再依赖 `strategy=="gpu"`）：

```rust
// engine/mod.rs phase1
let mut engines: Vec<Arc<dyn ImageEngine>> = vec![Arc::new(ImageRsEngine)]; // 🔴 单元结构体无 ::new()，直接 Arc::new(ImageRsEngine)（见 engine/mod.rs:30）；先认领 jpg/png/...
#[cfg(windows)]
// get_gpu_engine 返回 Option<Box<dyn ImageEngine>>，须 if-let（直接 push 编译失败）；按元素类型转 Arc
if let Some(wic) = get_gpu_engine("wic") { engines.push(wic.into()); } // 追加在后：兜底 image-rs 不认的格式
```

- ⚠️ **顺序必须 ImageRsEngine 在前**：实测 `WicEngine.supported_formats` **含 jpg/jpeg/png/bmp/tif/tiff/webp/gif**（wic_engine.rs:22-27，非仅现代格式）。若 WIC 在前，`engine_for` 的「首个 `can_handle` 命中」会把**所有常见格式也导向 WIC 系统内存解码**，偏离「常见格式走 image-rs + §3.1 ResizeHint」的设计。ImageRsEngine 在前 → 它认领的 8 格式仍走 image-rs，WIC 只兜底它不认的 heic/heif/avif/ico。
- 备选（若要 jpg 也用 WIC 解码期降采样、更快）：**不靠 arena 顺序**，在 gpu 策略路径（`try_gpu_decode`）显式选 WIC；arena 仅作 CPU fallback，保持常见格式 image-rs。
- mac：`cfg(windows)` 排除 WIC；mac HEIC 走 §3.7 `ImageIoEngine` 注入同一 arena 末位槽。

**3.2.2 格式集单一事实源（Q3）**：`is_phase1_image`（format.rs:129）与 `WicEngine.supported_formats` 分裂 → 抽**统一格式能力表**：scanner 的「可扫描图像格式」= 各已注册引擎 `supported_formats` 的并集（运行时聚合或编译期常量），消除「WIC 能解但 scanner 不扫」。

### 3.3 缓存治理：GC + 孤儿清理 + 上限 + IPC（Q6 + Q7 + Q8）

🔴 **3.3.1 周期化 + 全目录纳管**（Q6）：

> 🔴 **本节三条命运各异，已按 T6 实际裁决回写（2026-07-02；裁决原文见 §4 T6 行「纠偏 §3.3.1」 与 §8.3）**：条1 未推翻但未做（T6 🔵 余项待排期）；条2 **被推翻**——sprites/motion **有意排除 LRU**（绑定源媒体生命周期而非浏览近期性，LRU 误逐会致昂贵关键帧重派，cache.rs:164-168 注释明示），孤儿清理兜底；条3 未采纳——现行 thumbnails+ai_thumbs **有意共用单一预算**单次淘汰（cache.rs 注释），重启需另行决策。

- `enforce_cache_limit` 从「启动一次」改为**周期触发**（后台定时 + 关键事件后，如全量重建/大批导入完成）。〔✅ 已实施(2026-07-02 `21fa53a`):lib.rs 周期任务=启动后 5min 首跑(错开 3min 的 PRAGMA optimize)→ 对账 GC → LRU 收敛,此后每 24h;「关键事件后触发」(全量重建/大批导入)仍未接事件,留余项〕
- `sprites/` + `motion_videos/` **纳入上限统计**（可设独立子上限，避免雪碧图/视频挤占缩略图额度，但绝不能无上限）。〔🔴 已被 T6 推翻：有意排除出 LRU 纳管，见节首横幅〕
- LRU 加权：thumbnails 与 ai_thumbs **分别核算**或加权，避免优先驱逐 AI 缓存（其 mtime=生成时间偏早）。〔未采纳：现行有意共用预算，见节首横幅〕

**3.3.2 孤儿清理（Q7）**：两条路径互补——

- **删除时即时清理**：硬删/目录删媒体时，按 `cache_key` 推算其 thumbnails/ai_thumbs/sprites/motion_videos 路径并删除（事务后异步执行，失败不阻塞删除）。软删（is_deleted）**不删缓存**（保留以便恢复，离线≠删除同理）。
- **后台对账 GC**：周期扫缓存目录，对 DB 中已无对应 `cache_key`（或 is_deleted 超 N 天）的文件清理。兜底「即时清理」的遗漏。

**3.3.3 缓存管理 IPC（Q8）**：新增命令 `get_cache_stats()`（各子目录大小 + 总占用 + 上限）+ `clear_cache(kind)`（thumbnails/ai/sprites/all，供设置面板「清理缓存」按钮）。返回 `AppError`。Part5 UI 消费。

### 3.4 派生失效统一（Q5 + Q7，接 Part2 §3.3）

🔴 **统一失效入口 `invalidate_derived_for_item(conn, id)`**——Part2 与 Part3 的失效需求合并为**一个事务内函数**，SourceChanged 与删除路径都调它：

```rust
/// 源文件变更/替换时，失效该项的全部派生与元数据，使各流水线重新生成。
/// 统一 Part2(meta) + Part3(media_derivations) + 缩略图 + exotic 的失效面。
pub fn invalidate_derived_for_item(conn: &Connection, id: i64) -> Result<()> {
    // 1) 缩略图状态（已有）：thumb_status=0/thumb_path=NULL/thumbhash=NULL
    // 2) Part2：DELETE image_meta/video_meta/audio_meta（IS NULL 过滤驱动 enricher 重跑）
    // 3) Part3：media_derivations 退回 pending —— 关键修复 Q5
    //    UPDATE media_derivations SET status=0, payload_path=NULL WHERE item_id=?1
    //    （或 DELETE，让 backfill 的 INSERT OR IGNORE 重建；二选一，UPDATE 更省一次 backfill 扫描）
    // 4) exotic（已有）：invalidate_exotic_tasks_for_item
    // 5) AI/face（Part4 对接）：ai_status/face_status 复位
    Ok(())
}
```

- 🔑 **为何 UPDATE status=0 而非 DELETE**：`media_derivations` 行带 `(item_id, kind)` 唯一约束 + 历史；`status=0` 让 Producer 的 `get_pending_derivations` 自然重新领取（与现有 backfill 路径一致），比 DELETE + INSERT OR IGNORE 少一次重建扫描。
- 🔑 **同时清磁盘旧产物**：失效时一并删对应 sprites/缩略图旧文件（或靠 cache_key 因 mtime 变而天然换 key——`cache_key` 含 mtime，源变则 key 变，旧文件成孤儿交 §3.3.2 GC）。**优先靠 cache_key 自然换新**（最省），孤儿交 GC 兜底。
- 调用点：`fast_scan.rs:400` SourceChanged 分支（替换当前只调 exotic 的代码）；删除路径（§3.3.2）。
- ⚠️ **与 Part2 协同**：Part2 §3.3 已规划 DELETE 三 meta，此处合并——**Part2/Part3 共用此函数，避免两处各写一半**。文档交叉引用，落地时一个函数。

### 3.5 让步模型复核（现状基本正确，两处加固）

> §2.3 实测：派生让步阶梯（scan>thumbnail>derivation>AI）+ `reserved_core_pool`（enrichment 保 1 核）+ `BackgroundHeavyLimiter`（派生/exotic 公平池）已是**良好设计**，**不推翻**。仅两处加固：

- **3.5.1 缩略图 deferred CPU 不阻塞 decode worker**（issue thumbnail_commands.rs:428）：`DeferredToCpu` 在 decode worker 线程 inline 调 `process_deferred_cpu`，CPU 密集解码会占住 decode channel worker、反让 GPU decode 线程空等。改：deferred CPU 单独入一个低优先 CPU 阶段（或独立小线程池），与 GPU decode worker 解耦。
- **3.5.2 limiter 低核机下限保护**（issue pipeline.rs:420-433）：`BackgroundHeavyLimiter` 额度=`available_parallelism()`，双核机 permits 过小时 dispatch 线程阻塞、rayon worker 空等。设 permits 下限（如 `max(2, cores)`）或解耦 dispatch 与 acquire（acquire 移入 rayon 闭包内）。
- 缩略图侧**不加主动让步**（现状合理）：派生退避已实现「缩略图优先」，缩略图自身是用户等待的前台工作，无须再退避。

### 3.6 ai_thumb 默认值引导（Q12）

> `ai_hq_cache_enabled` 默认 false 本身**不是 bug**——无 AI 插件时开启纯属浪费（多解码 + 多磁盘）。真问题：**装了 AI 插件却没开 → CPU 解码优化形同虚设**。

设计 = **与 AI 插件生命周期联动**，而非粗暴改默认值：

- **安装 AI 插件时自动开启** `ai_hq_cache_enabled`（插件 install 编排里写 app_config），并在设置面板可见可关。
- **卸载 AI 插件时**询问是否保留 AI 缓存（默认保留，重装免重算）。
- 首次 AI 分析若检测到未开 → 一次性提示「开启高质量缓存可加速分析」（Part5 UX）。
- ⚠️ **诚实边界**：ai_thumb 优化的收益是「AI 分析时复用缩略图解码、不再解原图喂 CLIP」（memory `gotcha-ai-analysis-cpu-decode`：解原图饿死 GPU）。这与「缩略图生成速度」正交——别在文档里把两者混为一谈。

### 3.7 mac 平台地基：门控 → trait → objc2 桥（Q9，本 Part 最大工程）

> Win+mac 并重的直接代价。分**三步走**，第一步是「让 mac 能编译」，不是「写 mac 功能」。

**3.7.1 平台门控（先决，让 mac 编译通过）**：

- `Cargo.toml` 把 `windows` crate 改为 **target-specific 依赖**：`[target.'cfg(windows)'.dependencies] windows = ...`。
- 🔴 **平台相关代码 `#[cfg(windows)]` 全门控（P3-F1 回写，第 6 轮独立核验）**：WIC / `windows::Win32` / `get_gpu_engine` 调用实测散布 **≥12 文件 66 处**，远超初稿所列「`engine/gpu/` + `video/media_foundation`」——重灾 `ai/engine.rs`（~25）、`ai/provider.rs`（~19）、`ipc/system_commands.rs`（~5），另含 `ai/pipeline.rs` / `ai/face_pipeline.rs` / `derive/image.rs`（:16 无条件 `use ...get_gpu_engine`）/ `thumbnail/generator.rs` / `video/media_foundation.rs` / `engine/gpu/{mod,wic_engine}.rs`（当前 `engine/gpu` 模块零 `#[cfg(windows)]`）。须逐文件门控并为非 windows 提供 stub/平台实现，否则 mac `cargo check` 仍失败。完整落点见 T8（已据实重列）。
- 引擎/视频后端工厂在非 windows 返回平台对应实现或显式 stub（暂时 `UnsupportedFormat` 而非编译失败）。
- **验收**：`cargo check --target x86_64-apple-darwin`（或 mac 上 `cargo check`）通过。**这是 mac 一切的前提**。

**3.7.2 平台无关 trait 抽象**：`ImageEngine`（已有）+ `VideoBackend`（已有 `backend_for()` 雏形）即抽象点。mac 实现注入同一 arena/工厂槽位，上层流水线零改动。

**3.7.3 objc2 桥实现 mac 后端**（引 `objc2` + 子 crate，Part0 §11 line 226，~200KB）：

| mac 后端 | 框架 | 对应 Win | 优势 |
|---------|------|---------|------|
| `ImageIoEngine` | Image I/O（`CGImageSource`） | WicEngine | `CGImageSourceCreateThumbnailAtIndex` **解码期降采样**（真降采样，胜过 CPU 全解再缩）；原生 HEIC/HEIF + **多数 RAW**（mac 反比 Win 易拿 RAW） |
| `AvFoundationBackend` | AVFoundation（`AVAssetImageGenerator`） | MediaFoundationBackend | 视频封面/关键帧；旋转/色彩由框架处理 |
| `PdfKitDoc` | PDFKit（`PDFDocument`/`PDFPage`） | 前端 canvas | PDF 缩略图后端化（比前端离屏渲染快、省前端负担） |
| （兜底）`QuickLookThumb` | QuickLook（`QLThumbnailGenerator`） | — | 通用格式兜底（系统级，覆盖面广） |

- ⚠️ **CoreML/GPU entitlement**：mac GPU 加速（含未来 AI）需平台签名 + entitlement，建议 **P6.5 平台签名后再开**（Part0 §11 line 228），本 Part 先 CPU/系统路径。
- 工作量约 Win 路径 2–3 倍，列 §4 独立大任务块。

### 3.8 视频帧边界 bug + document_meta 回填（Q16 + Q13）

- **3.8.1 视频帧尾像素填黑（Q16，一行修复）**（media_foundation.rs:355-372）：`sample_to_rgba` 的 `if s + 3 >= cur { break; }` 在 stride>宽×4（宽幅/奇数对齐）时提前 break、尾像素静默填黑。**精确修法（review 核验）**：守卫改 `if s + 3 > cur { break; }`（等价 `s+2 >= cur`——行内最大访问下标是 s+2 的 R 通道）；现行 `>= cur` 在 `s+3==cur`（最后一个合法像素）时即提前 break、丢该像素。加损坏帧 + 宽幅/奇数对齐/bottom-up/旋转 90·270 单测。
- **3.8.2 document_meta 回填（Q13）**：DAO（`upsert_document_meta`/`get_document_meta`）由 **Part1 §3.3 提供**；本 Part 负责**写入**——epub 派生（doc.rs）解出页数/子类型时、PDF 渲染拿到页数时，调 `upsert_document_meta(item_id, page_count, doc_subtype)`。使 `document_meta` 从「零 DAO 死表」变为活表，供阅读器/详情面板（Part5）。

### 3.9 epub 漏登记 + TIFF 超时（Q10 + Q11）

- 🔴 **3.9.1 epub 登记（Q10，1 行解锁整链）**（format.rs:63-67）：`classify_media_type()` 文档分支加 `"epub" => Some("document")`，`doc_subtype()` 加 `"epub"` case。**后端实现（doc.rs zip 解封面、kind.rs DOC_THUMB_FORMATS、阅读器）全已就绪**，仅缺这一行登记 → epub 整链（入库/缩略图/阅读器）立即可用。**Part0 §1 矛盾 #6 兑现。**
- **3.9.2 TIFF 真超时（Q11）**：与 **Part2 §3.9.2 同一修复**（`recv_timeout` 替 `thread::scope.join`，独立线程 + 限时放弃）。`metadata.rs` 同时服务扫描与派生维度提取 → **归属 Part2 主修**，Part3 知晓并复用；不重复实现。

---

## §4 分步任务清单（优先级 + 依赖）

> 优先级：**P0 高收益低成本（能力已在没接通）+ 数据正确性 → P1 卖点/瓶颈深化 → P2 mac 大工程 + 元数据 → P3 加固清理**。多数 P0 是 1 行～小改即解锁大功能。

### P0 · 一行级解锁 + 数据正确性

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T1** ✅ **已实施(2026-06-30)** | CPU 路径接通 ResizeHint：`try_cpu_decode` 全解 fallback 由 `decode(abs, None)` 改传 `Some(ResizeHint::LongEdge(decode_long_edge(config,item)))`。**核实纠偏**：复用 GPU 路径同一 `decode_long_edge`（非 plan 简化的 `snap_to_tier(config.size)`）——保「AI 高清缓存+宽幅图解码略大、一次解码两份产物」语义；`image_rs` LongEdge 仅下采样不上采样（小图不放大）；常见情形 `resize_to_rgba` 因 w/h<=target 短路省二次缩放，同 CatmullRom 输出等价。245 单测全绿、clippy/fmt 净。🔵 缩略图视觉质量等价（同滤波、仅缩放位置前移），按 DoD 标「未自动覆盖、真机可抽查」 | generator.rs:362（`try_cpu_decode`） | — | §3.1.1 / 单次 resize |
| **T2** ✅ **已实现(早于本轮，已核实)** | epub 登记 `classify_media_type` + `doc_subtype`。**核实**：format.rs 文档分支已含 `"epub"`（classify_media_type:65、doc_subtype:94），并有单测 `epub_is_registered_document`（format.rs:137）锁定。Part0 §1 矛盾 #6 已兑现 | format.rs:62-65/93-94 | — | §3.9.1 / epub 整链可用 |
| **T3** ✅ **已实施(2026-06-30)** | 派生失效统一：`invalidate_derived_for_item` 在原三 meta DELETE 基础上**补 `media_derivations` 退 pending**（`status=0, payload_path=NULL, error=NULL`，Producer 据 `get_pending_derivations(status=0)` 重派）。范围核实：缩略图状态由 upsert 同事务 UPDATE 复位（不重复）、exotic 由 fast_scan 单独接（不重复）；旧磁盘产物靠 cache_key 含 mtime 天然换 key 成孤儿交 GC。扩展现有单测 `source_changed_invalidates_meta_but_unchanged_keeps` 断言派生 done→pending、Unchanged 不复位。245 单测全绿、clippy/fmt 净 | queries.rs:589（调用点 666 已接，无需改 fast_scan） | **接 Part2 §3.3（共用函数）** | §3.4 / 替换后派生重做 |
| **T4** ✅ **已实施(2026-06-30)** | 视频帧尾像素填黑修复（`s+3 >= cur`→`s+3 > cur`，仍 break）。**核实纠偏**：行内最大访问下标是 `s+2`（R 通道），`s+3==cur` 时该像素合法却被旧 `>=` 早退丢弃。顺手把 `sample_to_rgba` 逐像素 `*data.add()` 三处 unsafe 抽成纯函数 `copy_bgr32_to_rgba(src: &[u8],…)`（单一 `from_raw_parts`、切片安全索引），使边界可单测。+4 单测（尾像素不丢/bottom-up/含填充 stride/截断不 panic）。245 全绿、clippy/fmt 净 | media_foundation.rs（`copy_bgr32_to_rgba`） | — | §3.8.1 / 宽幅视频无黑边 |

### P1 · 卖点 / 瓶颈深化

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T5** | WicEngine 注册 arena（cfg windows）+ 格式集单一事实源 | engine/mod.rs、format.rs:129 | — | §3.2 / 非 gpu 策略 HEIC 出图 |
| **T6** 🟡 **部分已实施(2026-06-30)** | 缓存治理。**已落地**：①**T6a §3.3.3 IPC**——`get_cache_stats`（四子目录字节+总量+上限，spawn_blocking）+ `clear_cache(kind)`（thumbnails/ai/sprites/motion/all，返回释放字节），cache.rs 纯助手 `compute_cache_stats`/`clear_cache_kind` + 注册 lib.rs；②**T6b 即时孤儿清理 §3.3.2**——`cache_files_for_key`/`remove_cache_files_for_key`（按 cache_key 枚举 4 档缩略图+ai+sprite+motion），接入 `remove_media_items_hard` 硬删后 DB 锁外清理（软删不清，保留供恢复）。+4 单测。252 全绿、clippy/fmt 净。**纠偏 §3.3.1**：sprites/motion **不纳入 LRU**（代码注释证实有意排除——绑定源生命周期、非浏览近期性，LRU 淘汰会致昂贵关键帧重派），改由孤儿清理兜底。🔵 **余项（待排期，需缓存清理触发节奏的决策）**：对账 GC（周期扫缓存目录删 DB 无对应 key 的文件）+ enforce 周期化（现仅启动一次）——属后台定时器/调度，建议独立一轮 | cache.rs、config_commands.rs、lib.rs、file_ops_commands.rs | — | §3.3 / 删档清缓存、可查可清 |
| **T7** ✅ **已实施(2026-06-30)** | exif_thumb 质量守门 + direct 补 thumbhash。**(a §3.1.2)** 抽纯函数 `embedded_thumb_acceptable(max_edge,target)`：大档位（480/960）严格——内嵌图不足档位即回退全解码（避免 160→960 数倍上采样劣化 Q4）；小档位（120/240）维持 120px 下限宽通道（选片速度优先）。`target_size` 经 `snap_to_tier` 归一判级。+3 单测。**(b §3.1.3)** 去掉 `strategy != "direct"` 短路：strategy=direct 的图也在 ≤500KB 时生成 thumbhash 占位（体验底线，Q14）。248 单测全绿、clippy/fmt 净 | exif_thumb.rs、generator.rs:224 | — | §3.1.2-3 / 大档位不劣化、direct 有占位 |

### P2 · mac 平台地基 + 元数据

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T8** | mac 平台门控（windows crate target-specific + WIC/MF/`get_gpu_engine` `cfg(windows)` 全门控 + stub） | Cargo.toml、engine/gpu/*、video/*、**ai/{engine,provider,pipeline,face_pipeline}.rs、ipc/system_commands.rs、derive/image.rs、thumbnail/generator.rs**（实测 ≥12 文件 66 处，P3-F1） | — | §3.7.1 / **mac `cargo check` 通过**（验收须遍历全部落点，非仅 engine/gpu+video） |
| **T9** 🔴后置 | **fast-follow（Part0 §11.4.2：mac 实功能后置 v0.1 之后；T8 `cargo check` 双平台门控保留防回归）**：objc2 桥 mac 后端 ImageIoEngine + AvFoundationBackend + PdfKitDoc + QuickLook 兜底 | 新增 mac 模块，注入 arena/工厂 | ← T8、**Part0 §11.4.2** | §3.7.3 / mac HEIC/视频/PDF 出图 |
| **T10** 🟡 **epub 已实施(2026-06-30)，pdf 待前端** | document_meta 回填。**epub**：`doc.rs` 抽 `count_epub_spine`（数 OPF `<spine>` 的 `<itemref>` 作页数近似，同一份 OPF 顺带、不二次开包）→ `DerivationOutput` 增 `page_count` 字段 → 经 `DerivationResultRow`(6→7 元组，编译器强制全构造点对齐) → 流水线 writer 在同写锁内 `upsert_document_meta(item_id, page_count, "epub")`。+2 单测(spine 计数/无 spine→None)。254 全绿、clippy/fmt 净。🔵 **pdf/svg 待续**：走 `store_doc_thumbnail` 前端回传路径，page_count 需前端传 pdf.js `numPages`（IPC 签名 + 前端改动）→ 现置 None，已留注释 | doc.rs、kind.rs、queries.rs、pipeline.rs（+doc_commands.rs 占位） | ← Part1 DAO（§3.3） | §3.8.2 / page_count 非空 |
| **T11** | ai_thumb 默认值联动 AI 插件生命周期（装即开/卸询问/首用提示） | 插件 install 编排、config | ← Part6 install / Part4 | §3.6 / 装 AI 插件后自动开 |

### P3 · 让步加固 + 清理

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T12** 🟡 **limiter 下限已实施(2026-06-30)，deferred 解耦待续** | **(§3.5.2 已落地)** `BackgroundHeavyLimiter` 预算 `available_parallelism().unwrap_or(4)` 加 `.max(2)` 下限——单核/受限容器（parallelism==1）下避免派生 dispatch 取走唯一额度后阻塞、rayon worker 空等且 exotic 饿死。改 state.rs 构造点（不动 limiter `new()` 的 `max(1)`，否则破坏其 `new(1)` FIFO 单测）。clippy/fmt/254 测净。🔵 **(§3.5.1 待续)** deferred CPU 与 GPU decode worker 解耦——属 batch_request Scheme-2 **热路径并发重排**（加 deferred 通道 + 独立 CPU 池、重写 `DeferredToCpu` 分支），通道生命周期微妙（drop 排序）、perf 性质、无自动化测试、需真机验证，与 T13 同风险画像，建议专注一轮 | state.rs（已）；thumbnail_commands.rs:428（待） | — | §3.5 |
| **T13** 🔨 **改性 A/B 待裁决(2026-07-10,推翻 06-30「暂缓删除」)** | ~~死代码清理~~ → **生成引擎 A/B 实测裁决**。开关已运行时化(`daab834`:AtomicBool + 配置键 `thumb_use_pipeline` + 开发者工具 toggle)且两方案公平性缺陷已修(F1 方案一进度零节流/F2 `filter_map`+`zip` 错位雷+静默跳过/F3 方案二 Phase2 串行)——方案一不再是死代码。**剩余动作**=用户真机 A/B(清缓存→双引擎各全量生成取日志 elapsed)→ 据实删方案一(~300 行×2 命令+开关退役)或反向优化方案二。原「重缩进无安全网」评估对「删除」动作仍有效,届时专注一轮+diff 复审 | thumbnail_commands.rs | 用户在环实测 | §3.1.4(已修订)+ [工作线文档](../designs/2026-07-10-缩略图流水线深审与优化.md) §3 |
| **T14**（可选） | JPEG `scale_denom` 解码期降采样（CPU 路径真降采样） | image_rs.rs / 评估 image crate 能力 | ← T1 | §3.1.1 深化 |

---

> **本轮审查回写（2026-06-30，5 路 agent 取证）**：
> - **P0（T1–T4）100% 落地带单测**：CPU 路径 ResizeHint、epub 文档登记、失效统一钩子（已含 Part4 回填的
>   AI/face 段）、视频帧尾像素修复。P1 完成 T1/T7，**T6 部分**（`get_cache_stats`/`clear_cache` IPC + 硬删孤儿清理
>   已落;✅ 对账 GC + `enforce_cache_limit` 周期化已于 2026-07-02 补齐 `21fa53a`），**T5 未做**（WicEngine 存在但 phase1 未注册，格式集未统一）。
> - **thumbhash 真相（修/删待 Part5 拍板）**：后端已生成 thumbhash BLOB 落库，但 **IPC DTO 未透出字段**；前端
>   `src/utils/thumbhash.ts` 的 `thumbHashToRGBA` 解码链是**残缺 stub**（chroma/alpha AC 系数解出后从未用于渲染循环，
>   产出近似纯色块而非真模糊图，「9 处 unused」即在此坏链内），且全仓零消费。**线上占位实际走的是正确的**
>   `thumbhashToAverageColor`（O(1) 纯色，header DC 取色，逻辑正确，MediaThumb.vue:381 在用）。故待决项实质是：
>   ① 删整条坏死链（`thumbHashToRGBA`+`getThumbhashBgAsync`+`useThumbnail`，最省）vs ② 官方算法正确移植再接进 Part5
>   渐进占位。`tsconfig` 未开 `noUnusedLocals`，故不致 build 失败，纯属潜伏。
> - **对 Part5 的两处可见缺口（建议启动时评估）**：**T5**（非 GPU 策略下 HEIC/HEIF 缩略图缺失 → 现代 iPhone 图显占位）、
>   **T10 pdf/svg 页数**（后端留 `None` 占位，需 Part5 前端传 pdf.js `numPages` 回填；epub 路径已通）。
> - **可独立延后**：T6 余项（LRU 容量驱逐，磁盘无限增长隐患非阻塞）、T8（mac 平台门控，卡 mac 出码不卡前端）、
>   T9（objc2 mac 后端，已主动后置）、T11（ai_thumb 默认值联动 AI 插件生命周期，← Part6 install/Part4）、T12 deferred 解耦、T13 死代码清理。**无任何项硬阻塞 Part5。**
> 🔴 **2026-07-02 订正**：本行原写「T11（HDR，需真机）」——**与 §4 任务表 T11（ai_thumb 默认值联动 AI 插件生命周期）矛盾**，全文档/全仓 grep 均无 HDR 相关设计内容，取证认定为早期草稿的错误标签（源头见 [2026-06-30-Part1-4完成度审查](2026-06-30-Part1-4完成度审查.md) §五同款误标，已一并订正）。T11 的真实延后理由是待 Part6 插件安装编排/Part4 就绪，与真机测试无关。

## §5 风险与回滚

| 风险 | 触发 | 缓解 / 回滚 |
|------|------|------------|
| CPU ResizeHint 改视觉 | CatmullRom（decode 内）替代 fast_image_resize 双线性，缩略图细微差异 | A/B 对比验收；保留旧路径开关（config）一版灰度；纯视觉差异非功能回归 |
| WicEngine 抢 jpg/png | 实测 WIC `supported_formats` 含 jpg/png/bmp/tif/webp/gif（wic_engine.rs:22-27，非仅现代格式），若注册在前会抢走常见格式 CPU 解码 | **ImageRsEngine 注册在前、WIC 追加在后**（§3.2.1）：image-rs 先认领 8 格式，WIC 只接 heic/heif/avif/ico；不改 WIC 能力（gpu 策略路径仍可用其降采样） |
| 派生失效触发批量重生成 | 批量替换文件后大量 media_derivations 退 pending | 本就应重生成；让步阶梯保证不抢前台；可分批 |
| 🔴 mac 门控致 Win 回归 | `cfg(windows)` 写错、漏门控某调用点 | **CI 双平台 `cargo check`（win+mac）**为合并门槛；T8 单独提交、单独验证 |
| 孤儿即时清理误删 | cache_key 推算错、删了在用文件 | 仅**硬删/目录删**触发即时清理（软删不删）；删前确认路径属本 cache_key；失败不阻塞、留 GC 兜底 |
| objc2 桥内存/线程安全 | Objective-C FFI 生命周期/autoreleasepool 误用 | objc2 的 `Retained`/`autoreleasepool` 严格遵循；mac 后端先 CPU/系统路径、不碰 GPU entitlement（P6.5 后）；充分单测 + 真机验证 |
| epub 登记触发存量重扫 | 老库 epub 之前被忽略，登记后需重扫才入库 | 引导用户对含 epub 目录重扫；或迁移时标记 epub 目录待扫 |

---

## §6 验收标准

**P0（解锁 + 正确性）**：

1. **CPU 解码**：480 档缩略图生成，CPU 路径从**两次 resize 降为一次**（decode 内 CatmullRom + 取消 encode 的 fast_image_resize）；⚠️ **内存峰值不变**（image crate 全解后缩、非解码期降采样，真降采样只 WIC/Image I/O 或 T14 jpeg-decoder）；视觉无明显劣化。
2. **epub 整链**：放 epub 入库 → 画廊出现 + 封面缩略图 + 可打开阅读器（Part0 §1 矛盾 #6 闭环）。
3. **派生失效**：替换一个视频（mtime+size 变）→ 重扫 → `media_derivations` 该项 status 回 0 → 重生成封面/关键帧（新画面）。
4. **视频帧**：宽幅/奇数对齐视频封面**无右侧黑边**（损坏帧单测通过）。

**P1（深化）**：

5. **HEIC**：`strategy!="gpu"` 下 HEIC 文件经 arena 命中 WicEngine 出缩略图（不再 UnsupportedFormat）。
6. **缓存治理**：硬删媒体 → 对应 thumbnails/ai_thumbs/sprites/motion_videos 文件被清；`get_cache_stats` 返回各目录占用；`clear_cache` 可手动清；周期 GC 生效（sprites 不再无限增长）。
7. **画质/占位**：大档位（960）不再由 160px 内嵌图 upscale；`direct` 策略下有 thumbhash 占位。

**P2（mac + 元数据）**：

8. **mac 编译**：`cargo check`（mac target）**通过**（T8 先决门槛）。
9. **mac 功能**（T9 后）：mac 下 HEIC 图像、视频封面、PDF 缩略图均出图。
10. **document_meta**：epub/pdf 入库后 `page_count` 非空、`doc_subtype` 正确。

---

## §7 执行提示词（新会话直接用）

```
任务：实施 Picasa Next 重构 Part3（缩略图派生与 GPU 引擎）。

【先读】
1. docs/refactor_2026/Part0_总纲与产品定稿.md §4(功能矩阵)/§11(波次,Part3=CPU解码修复+WicEngine注册+缓存治理+派生清理+mac原生层)/§12(约定)
2. docs/refactor_2026/Part1_数据层.md §3.3(document_meta DAO、cache_key)
3. docs/refactor_2026/Part2_扫描与画廊流水线.md §3.3(SourceChanged 失效)——Part3 T3 与之共用 invalidate_derived_for_item
4. docs/refactor_2026/Part3_缩略图派生与GPU引擎.md 全文（§2 现状取证为基线，§3 设计，§4 任务表）

【铁律】
- 多数 P0 是「能力已在、没接通」：CPU 路径 ImageRsEngine 已实现 ResizeHint(image_rs.rs:54-91)只是传了 None；epub 后端全在只缺 1 行登记。先做这些高杠杆修复。
- 派生失效(T3)必须与 Part2 §3.3 合并为一个 invalidate_derived_for_item，覆盖 meta+media_derivations+缩略图+exotic，事务内；勿两处各写一半。
- WicEngine 实测 supported_formats 含 jpg/png 等常见格式(wic_engine.rs:22-27)：注册 arena 时 ImageRsEngine 必须在前、WIC 追加在后(push 非 insert(0))，否则 WIC 抢走常见格式 CPU 解码。
- mac 第一步是「让 mac 能编译」(T8 平台门控)非「写 mac 功能」；windows crate 改 target-specific，CI 双平台 cargo check 为门槛。
- CPU 纯 Rust 路径无解码期降采样(image crate 限制)，只能优化到「解一次+缩一次」；真降采样只有 WIC/Image I/O 原生路径——文档/承诺勿夸大。
- 派生/AI 让步(should_yield_derivation)，enrichment 不让步(reserved_core_pool)；缩略图经 asset:// 直通；rayon 内不 await；新增命令 AppError；中英双语注释；改后中文 commit；仅用户通知时 push；大文件小步 Edit。

【顺序】
P0: T1 CPU ResizeHint → T2 epub 登记 → T4 视频帧 bug → T3 派生失效统一(接 Part2)
P1: T5 WicEngine 注册 → T7 exif 质量守门+direct占位 → T6 缓存治理(GC+孤儿+IPC)
P2: T8 mac 门控(cargo check 通过) → T9 objc2 桥(ImageIO/AVFoundation/PDFKit/QuickLook) → T10 document_meta 回填 → T11 ai_thumb 联动
P3: T12 让步加固 → T13 死代码清理 → T14(可选) JPEG scale_denom

【验收】按 §6 十条；P0 四条 + T8 mac cargo check 是硬门槛。
【关键认知】WIC 'GPU' 实为 CPU 系统内存解码(无 D3D/DXGI)，模块名是概念标签；ai_hq_cache_enabled 默认 false 是对的(无 AI 插件时)，问题在装了不开→联动开启。
```

---

## §8 评审修正记录（对抗 review w3mniao14，2026-06-26）

> 4 reviewer（一致性/可行性/源码核验/完整性）对抗审查。本节记录全部 Part3 相关 issue 的分诊处置，**无省略**。

### 8.1 已改正文（事实/措辞，已实锤）

| issue | 处置 |
|------|------|
| exif_thumb「无回退」取证错误（实际 <120px 回退全解、120≤内嵌<target 才 upscale） | ✅ §2.2 / §2.9 Q4 已改 |
| video `sample_to_rgba` 边界（应 `s+3>cur`，非笼统 continue） | ✅ §3.8.1 已精化 |
| WicEngine 代码 `engines.push(Option)` 编译失败 | ✅ §3.2.1 改 if-let + 类型转换 |
| §6 验收「仅一次 resize」措辞（内存峰值不变需澄清） | ✅ §6.1 已澄清 |

### 8.2 补充设计（实施时连同正文生效）

1. **§3.3 GC 并发安全 + 高效实现**（major）：① GC 只删 mtime 早于「启动 epoch token」的文件，避开正在写的；② 即时清理删前校验路径前缀匹配 `{size}/{prefix}/{hex}.webp` + 断言；③ 高效对账：`SELECT cache_key` 全集入内存 HashSet（百万 ~10MB）→ 扫盘 hex 比对，不在集即孤儿；④ 后台低优先、带 yield、分批（≤30s）、频率 24h + 全量重建后。
2. **ai_thumb 短边统一**（cross major，与 Part4§3.8 冲突）：**采用方案①**——ai_cache 固定短边 336，ViT-B/16(224) 在 worker 内**先 `Resize(短边=224)` 再 `center_crop(224)`**（🔴 第 8 轮核验：原写「`center_crop` 到 224」漏 Resize 步——勿对短边 336 直接裁 224，否则丢约 33% 画面、改变 embedding；源码 clip.rs/pipeline.rs 经解码 `ShortEdge(224)` 已正确，本行措辞与之对齐，开销极小），**避免双 cache**；`AI_CACHE_SHORT_EDGE` 保持 336（详见 Part4 §8）。
3. **Win RAW 归属**（major，Part0§4 must）：Win RAW 走 **WIC 系统 Camera Raw Codec**（运行时检测），T5 补 cr2/cr3/nef/arw/raf/dng 扩展名 + 可用性检测，不支持的专有 RAW 归 Part6 exotic。消除悬空。
4. **测试任务**（critical，Part0§11 每 Part 同步补测）：§4 补——P0 `T_t1` video MF stride/rotation/bottom-up/旋转单测；P1 `T_t2` WicEngine arena 注册集成测 + 缓存 GC/孤儿单测；P2 `T_t3` epub 整链 DB 端到端测；§6 各条追加「+对应单测」。
5. **T14 JPEG scale_denom 重定性**（minor）：image crate **无** scale_denom 公开 API → 需直接引 `jpeg-decoder`（`DecodingOptions::scale_denom`）+ 新建 `JpegFastEngine` 注册 arena，**结构变动非微调**；先验 offline cargo 缓存（jpeg-decoder 大概率已在 image 依赖树，提升直接依赖须确认）。
6. **§3.7.3 mac 真降采样限定**（minor）：`CGImageSourceCreateThumbnailAtIndex` 对 **JPEG/HEIC** 真降采样，**PNG/TIFF/BMP 仍全解后缩**；统一由系统框架处理旋转/色彩仍优于 image-rs。
7. **§5 风险补**：Part1 document_meta DAO(T6) 未就绪 → Part3 T10 验收阻塞 → T10 排 Part1 T6 合入后，CI 断言 API 存在。

### 8.3 已知接受（非 bug / 已设计）

- sprites/motion_videos 排除 LRU 是**有意设计**（绑源媒体生命周期）；孤儿靠「删除即时清 + GC 对账」（§3.3.2）。
- direct 跳 thumbhash：§3.1.3 已设计补占位。
- §2.9 Q13 document_meta「对应设计」精确化：**DAO→Part1 §3.3，本 Part 写入→§3.8.2**。

### 8.4 跨 Part 协调（与 Part4 §8 一致）

🔑 **`invalidate_derived_for_item` 唯一 owner = Part3**（在 queries.rs）。签名固定 5 段：缩略图状态复位 + meta 三删（Part2 需求）+ `media_derivations SET status=0, payload_path=NULL`（Part3，**补 payload_path=NULL** 防 Producer 读旧产物）+ exotic 失效 + AI/face 钩子（Part4 填）。🔑 **事务边界**：SQL 段进 upsert `unchecked_transaction`，**VectorStore/ANN 内存操作在事务提交后执行**（内存结构无法随 SQL 回滚，否则 DB 回滚但缓存已改的不一致）。Part2 只调不重写。§3.4 伪码据此定稿（去掉「UPDATE 或 DELETE 二选一」，定 UPDATE）。

---

> **Part 3 正文完**。下游：Part4（AI+人脸插件化，消费 ai_thumb 缓存 + invalidate_derived_for_item 的 AI/face 失效钩子）、Part5（前端，消费 asset:// 缩略图 + thumbhash 占位 + 缓存管理 UI + 阅读器 document_meta）、Part6（exotic 收尾 + ai_thumb 联动 install）、Part7（mac 平台签名/公证，开 CoreML/GPU entitlement）。
> 执行前必读：Part0 §13 + Part1 §7 + Part2 §7 + 本文 §7。
