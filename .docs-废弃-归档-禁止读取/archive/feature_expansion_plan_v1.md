# 功能扩展设计 v1 | Feature Expansion Plan v1

> 🔴 **已废弃(2026-07-10 文档治理补标)**:已被 refactor_2026 整体取代(Part0 §0.3 降历史参考);其 §3.5 阅读体验部分由 designs/2026-07-07-阅读器完善方案.md 接管。

> 视频 / 文档 / 音频 / 收藏夹 / 网络盘 八项需求的正式设计文档。
> Formal design for eight features: video, documents, audio, collections, network storage.

---

## 0. 文档信息 | Meta

| 项 | 内容 |
|---|---|
| 版本 Version | v1（2026-06-15） |
| 状态 Status | 草案待评审 Draft / 部分决策待定 |
| 作者 Author | Picasa Next（设计协作） |
| 关联文档 Related | `implementation_plan_v1.2.md`（母设计）、`architecture_notes.md`（活文档）、`perf_hardening_plan_v2.md`（性能加固） |

### 0.1 对母设计的修订声明 | Revisions to the master plan

本文档**显式修订** `implementation_plan_v1.2.md` 的以下选型，原因见 §1「轻量优先」原则：

- **视频处理**：母设计为「FFmpeg Sidecar」（捆绑 ~数十 MB）。**本文改为以 Windows Media Foundation（MF）为主**（复用现有 `windows` crate，零捆绑），FFmpeg 降级为「检测系统是否存在 / 可选下载」的兜底。
- **文档渲染**：母设计为「mupdf / pdfium / resvg」（捆绑 native 库）。**本文改为前端 pdf.js / epub.js + Webview 离屏渲染**（无 native 二进制增量）。
- **分发形态**：上述「轻量」选型为 **Lite 变体（默认）**；`perf` 变体经 Cargo feature 启用 FFmpeg/pdfium 等原生后端。两个编译产物对外形成**三种分发包**（完整 / 轻量+组件 / 轻量），共用同一后端 trait 抽象、一键切换，详见 **§1.4**。

其余（`lofty` 音频、`trash`、4 张扩展表、按类型默认宽高 595×842 等）与母设计一致并细化。

---

## 1. 设计原则 | Guiding Principles

### 1.1 双变体：极致轻量 / 极致性能 | Two variants: Lite / Perf

产品提供**两种程序包**，共用一套代码与后端抽象（切换机制见 **§1.4**）：

- **Lite（极致轻量，默认）**：零大体积 native 捆绑，主打最小安装包——产品核心卖点。
- **Perf（极致性能）**：经 Cargo feature 启用捆绑型原生后端，追求最快处理与最广格式覆盖。

各能力在两变体下的后端选型：

| 能力 Capability | Lite（默认，零/极小体积） | Perf（性能变体，feature-gated） |
|---|---|---|
| 视频解码/取帧/元数据 | **Media Foundation**（`windows` crate 已有依赖；硬解 D3D11 优先、软解降级） | + **FFmpeg sidecar**（externalBin）：覆盖 MF 不支持的容器、批量更快 |
| 不支持的容器（mkv/webm/flv…） | 检测系统已装 ffmpeg；缺失则「无封面图标」 | FFmpeg 直接支持 |
| PDF/文档**缩略图** | 前端 Webview 离屏渲染（**pdf.js**） | **pdfium**（`load-dynamic`，并行、不占主窗口） |
| PDF/EPUB **阅读器** | pdf.js / epub.js（前端，自带文字层） | 同 Lite；超大文件可选 native 加速栅格化 |
| **文本**提取/搜索/替换 | 前端 JS（pdf.js 文字层 + JS regex） | **native Rust**（`aho-corasick` 并行，适合大文集） |
| EPUB 封面/元数据 | `zip` crate（纯 Rust，极小） | 同 Lite |
| SVG | Webview 原生渲染 / 离屏捕获 | 同 Lite（或 `resvg`） |
| 音频封面/标签/歌词 | `lofty`（纯 Rust，小） | 同 Lite |
| WebDAV / SMB | **依赖 OS 挂载**；WebDAV 可选 `reqwest_dav`（纯 Rust） | native 客户端 + 流式代理 |

> **变体边界 = 是否捆绑大体积 native 二进制**。FFmpeg、pdfium、libsmbclient 等（单个 10–80 MB）仅进 Perf。**纯 Rust crate**（lofty/zip/similar/aho-corasick/reqwest 等，1–2 MB 静态增量）两变体都可含——它们不构成「重」。若此边界与预期不符请指出。

### 1.2 复用既有管线模式 | Reuse existing pipeline patterns

新功能**不另起炉灶**。后台处理一律复用 `ai/pipeline.rs` 的成熟模式：

```
Producer → crossbeam channel → Consumer pool(rayon) → Writer
+ CancellationToken（暂停/停止）
+ should_yield_to_higher_priority()（让步）
+ 状态列（0待处理/1处理中/2完成/3错误）→ 断点续传 + 孤儿恢复
```

### 1.3 必须遵守的不变量 | Invariants to preserve

引自 `architecture_notes.md`，本设计的所有改动不得破坏：

1. `scrollHeight == totalHeight`（普通模式）。
2. 用户可见查询恒附加 `is_deleted = 0 AND companion_of IS NULL`。
3. `cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`；缩略图落 `cache/thumbnails/{size}/{2-hex}/{cache_key_hex}.webp`。
4. `is_favorited` 与 `thumb_status` 在 layout 缓存中按 id 原位 O(1) 同步；集合变更才触发 `compute_layout` 重排。
5. asset 协议在运行时按扫描根/缓存目录 `allow_directory` 授权（`add_scan_root` 时追加）。

### 1.4 构建变体与后端抽象 | Build variants & backend abstraction

**目标**：同一套代码产出 **Lite** 与 **Perf** 两种程序包，能力后端一键切换、运行时可降级。

**关键认知**：包体积由**编译期/打包期**决定（未使用的 native 依赖仍会增大二进制）→ **主开关必须是 Cargo feature**；运行期选择是其上的细化。同一套 trait 抽象同时支撑三种分发模型（§1.4.6）。

#### 1.4.1 能力 trait（与编译期无关的接口层）

`src-tauri/src/backend/`：每个「有 lite/perf 之分」的能力定义一个 trait：

```rust
pub trait VideoBackend: Send + Sync {
    fn name(&self) -> &'static str;             // "media-foundation" | "ffmpeg"
    fn can_handle(&self, ext: &str) -> bool;
    fn probe(&self, p: &Path) -> Result<VideoInfo>;          // 宽高/时长/rotation/fps
    fn cover(&self, p: &Path, t_ms: u64) -> Result<DecodedImage>;
    fn keyframes(&self, p: &Path, n: usize) -> Result<Vec<DecodedImage>>;
}
pub trait DocThumbBackend  { /* render_first_page / cover */ }
pub trait TextBackend      { /* extract / replace / search */ }
pub trait StorageBackend   { /* walk / stat / read_range */ }
```

> 并非凭空新增：现有 `engine_arena` + `get_gpu_engine`（WIC GPU 解码 / image-rs CPU + 运行时降级）已是同一模式的先例，本节将其推广到视频/文档/文本/存储。

#### 1.4.2 实现按 feature 编入

```rust
#[cfg(windows)]                pub struct MediaFoundationBackend; // OS 原生零捆绑 → 两变体都在
#[cfg(feature = "ffmpeg")]     pub struct FfmpegBackend;          // 仅 Perf：调 sidecar
pub struct FrontendDocThumb;                                      // Lite：委托 Webview 离屏渲染
#[cfg(feature = "native-doc")] pub struct PdfiumDocThumb;         // 仅 Perf：load-dynamic pdfium.dll
pub struct FrontendTextBackend;                                   // Lite：JS 处理
#[cfg(feature = "native-text")] pub struct NativeTextBackend;     // 仅 Perf：Rust 原生
```

```toml
[features]
default = ["lite"]
lite = []
perf  = ["ffmpeg", "native-doc", "native-text", "netfs"]
ffmpeg      = []                     # FFmpeg sidecar（externalBin，仅打包进 Perf）
native-doc  = ["dep:pdfium-render"]
native-text = []
netfs       = ["dep:reqwest_dav"]
```

#### 1.4.3 注册表 + 运行期选择

`backend/registry.rs`：启动时构建 `BackendRegistry`，收集「编译期编入 + 运行期探测可用」的后端，按 `(用户偏好, 硬解能力, priority)` 排序。

```
取某 ext 的后端：candidates = can_handle(ext) 者按序 → 依次 try，失败降级下一个
              → 全失败标记 unsupported（UI 提示「该格式需性能版 / 缺组件」）
```

- Lite + mkv → 无 FFmpeg、MF 不支持 → unsupported + 图标占位。
- Perf + mp4 → MF 硬解优先、FFmpeg 兜底。
- 用户可在设置里**强制**某后端（「强制 CPU」「优先 FFmpeg」），存 `app_config`。

#### 1.4.4 一键构建

```jsonc
// package.json scripts
"tauri:build:lite": "tauri build",
"tauri:build:perf": "tauri build --features perf -c src-tauri/tauri.perf.conf.json"
```

`tauri.perf.conf.json` 仅在 Perf 追加 `externalBin`(ffmpeg) 与 `resources`(pdfium.dll)；编译期常量 `BUILD_VARIANT`（"lite"/"perf"）供 UI 角标与埋点。

#### 1.4.5 前端适配

前端**不分叉逻辑**：启动调 `get_capabilities()` IPC 得到后端能力 → `native_doc=false`(Lite) 懒加载 pdf.js/epub.js 在 Webview 渲染；`=true`(Perf) 向后端请求渲染结果。可选 `VITE_VARIANT` define 用于在 Perf 构建里 tree-shake 掉 pdf.js（非必需，体积差异主要在 native 侧）。

#### 1.4.6 分发形态：2 个编译产物 → 3 个分发包

**两个编译产物**：
- `lite` 二进制：native 后端 glue 全部 `#[cfg]` 剔除，并按 §1.4.7 连 pure-Rust 重 crate 一起极致裁剪。
- `perf` 二进制：native 后端 glue 编入，但 pdfium 走 `load-dynamic`、ffmpeg 走 sidecar——**glue 不含 DLL/exe 本体，故二进制本身仍很小**（与现有 `ort` load-dynamic 同理）。

**三个分发包**（对应需求的三种模型）：

| 分发包 | 二进制 | 组件（pdfium.dll / ffmpeg.exe…） | 离线 | 体积 |
|---|---|---|---|---|
| **完整包 Full** | `perf` | **随安装包捆绑** | ✅ 开箱即完整 | 最大 |
| **轻量+组件包** | `perf` | **不捆绑**；另以「离线组件包」分发，放入 components 目录后 `load-dynamic` | ✅ 组件包亦离线安装 | 基础包小、按需增重 |
| **轻量包 Lite** | `lite` | 无（glue 未编入，无法事后添加） | ✅ | 最小（§1.4.7） |

- 「完整包」与「轻量+组件包」**共用同一个 `perf` 二进制**，仅区别于组件是否随包捆绑——一次编译，两种打包。
- 组件发现：`perf` 二进制启动时在 `<exe目录>/components` 与 `appData/components` 查找；「离线组件包」就是一个解压即用的目录（可选附应用内「检测/校验」UI，**不依赖联网**）。
- 「轻量包」是真正的极致最小（§1.4.7），代价是放弃事后加组件的能力。

#### 1.4.7 极致裁剪 Lite 的实现与效果 | Extreme-trim Lite: how & expected size

**目标**：连 pure-Rust 重 crate 也从 `lite` 二进制彻底移除（而非仅不调用）。

**实现手段**：

1. **依赖 `optional` + 代码 `cfg` 门控**（唯一能真正移除 crate 的办法）：
   ```toml
   lofty   = { version = "*", optional = true }
   similar = { version = "*", optional = true }
   reqwest = { version = "*", optional = true, default-features = false, features = ["rustls-tls"] }
   [features]
   audio-meta = ["dep:lofty"]     # 关掉 → 二进制内无 lofty
   doc-diff   = ["dep:similar"]
   netfs      = ["dep:reqwest_dav", "dep:reqwest"]
   ```
   所有用到该 crate 的代码加 `#[cfg(feature=...)]` 并给「能力缺失」降级分支。**代价：`lite` 丢这些能力**（如无 native 音频标签 / 无 WebDAV / 无版本 diff）→ 需明确划分「core 永远在」与「extreme-lite 可裁」。
2. **尺寸向 cargo profile**（新增，不动现有 release）：
   ```toml
   [profile.lite]
   inherits      = "release"
   opt-level     = "z"     # 体积优先（或 "s"）
   lto           = "fat"   # 比 thin 更彻底的死代码消除
   codegen-units = 1
   strip         = true
   # ⚠️ 不可用 panic = "abort"：现有 release 注释已说明依赖 catch_unwind
   #    拦截图像编解码器对坏图的 panic（thumbnail/generator.rs）。
   ```
3. **依赖 feature 瘦身**：`tokio` 现为 `features=["full"]`（偏重）→ 裁到按需（`rt-multi-thread`/`macros`/`sync`/`time`/`fs`）；`tauri` 的 `devtools` 仅 dev 启用；`image`/`chrono` 已是 `default-features=false` 的范例，延续。
4. **测量驱动**：`cargo bloat --release` + `cargo tree -e features` 定位最大贡献者，按收益排序裁。

**最终效果（量级估算，需实测校准）**：

| 项 | 量级 | 说明 |
|---|---|---|
| Tauri 基础二进制（非 AI） | **~5 MB** | Cargo.toml 自述「不捆绑 ORT 时重回 5MB」 |
| 全裁 pure-Rust 重 crate | 省 **~3–5 MB** | reqwest+rustls 占大头 |
| `opt-level="z"`+fat LTO+CGU=1 | 再省 **~10–25%** 代码段 | 牺牲少量运行速度/编译时长 |
| **极致 Lite 落点** | **~4–7 MB** | 估算 |
| 对比·完整包额外捆绑 | ORT+DirectML **~20MB**、ffmpeg **~30–70MB**、pdfium **~3–10MB** | 完整包可达 **60–100MB+** |

> **诚实的优先级**：主导体积的是 **①是否捆绑大 native blob（已按包区分）** 与 **②WebView2 分发模式**——**Evergreen Bootstrapper**（假定系统已装，Win11 默认预装）几乎零增量，而 **Fixed-Version 捆绑 ~150MB**，这一项压倒一切 Rust 裁剪。逐个门控 1–2MB 的小 crate 收益有限且换来功能缺失；建议**只裁最大的几个（reqwest / tokio-full）+ 上尺寸 profile**，其余小 crate 保留，避免把 `lite` 做成「功能残废版」。

---

## 2. 跨需求地基 | Foundations（阶段 P0）

> 需求 2/3/4/6 全部依赖本节。**必须先做。**

### 2.1 修复：非图片项尺寸补全 + 按类型默认宽高 | Fix: dimensions for non-image items

**现存缺陷**：`enricher.rs:84`、`queries.rs:1274/1422` 的补全/AI 查询硬编码 `media_type='image'`，但 `query_layout_geometry`（`queries.rs:607`）对所有类型读取 `media_items.width/height`。导致**视频/音频/文档入库后宽高恒为 0×0 → Justified Layout 错乱**。

**方案**：

1. **入库即给「按类型默认宽高」**（避免 0×0 进布局）：

   | 类型 | 默认 W×H | 说明 |
   |---|---|---|
   | video | 16:9（如 1280×720） | 补全后回填真实值 + rotation 修正 |
   | audio | 400×400 | 方形封面位 |
   | document（pdf） | 595×842 | A4 |
   | document（其它） | 400×400 | — |

   落点：`scanner/fast_scan.rs` 写入时按 `classify_media_type` 给默认值。

2. **补全阶段改为 media_type 感知**：`enricher.rs` 的 image-only 查询拆分为「图片走 EXIF」「视频走 MF probe」「音频走 lofty」「文档走子类型探测」，统一回填真实宽高（视频含 rotation 交换）。

### 2.2 统一派生任务框架 | Derivation Framework

将「视频封面 / 视频关键帧 / 文档缩略图 / 音频封面 / 音频元数据」抽象为**派生任务（derivation）**，共享一套可续传、可让步、可取消的调度。

#### 数据表 | Table

```sql
-- media_derivations：每个 (item, kind) 一行派生任务的状态机
-- one row per (item, kind) derivation job
CREATE TABLE IF NOT EXISTS media_derivations (
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL,            -- 'video_cover'|'video_keyframes'|'doc_thumb'|'audio_cover'|'audio_meta'|...
    status       INTEGER NOT NULL DEFAULT 0,  -- 0待处理 1处理中 2完成 3错误（复用 ai_status 语义）
    payload_path TEXT,                         -- 产物相对路径（sprite/封面等），可空
    error        TEXT,
    updated_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (item_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_deriv_pending ON media_derivations(kind, status) WHERE status < 2;
```

#### 优先级调度 | Priority scheduling

扩展 `state.rs:should_yield_to_higher_priority()` 为分级：

```
scan  >  thumbnail  >  derivation(封面/缩略图/元数据)  >  AI 语义分析
```

派生任务在 `should_yield_to_higher_priority()` 为真时 sleep 让步（沿用 AI producer 的 500ms 让步逻辑）。

#### kind 注册 | Registry

每种派生只实现一个纯函数 `fn run(item, abs_path, cfg) -> Result<DerivationOutput>`，管线/续传/让步/取消/孤儿恢复全部复用框架。新增派生 = 注册一个 kind。

落点：新增 `src-tauri/src/derive/mod.rs`（框架）+ `derive/{video,doc,audio}.rs`（各 kind 实现）。

### 2.3 迁移策略 | Migration strategy

沿用 `migration.rs` 既有模式：每个阶段实现时，递增 `CURRENT_VERSION` 并追加 `if version < N { execute_batch(SCHEMA_VN) }` 块。本文 §4 的 DDL 按阶段映射为 `SCHEMA_V4`…`SCHEMA_V7`。

---

## 3. 逐需求设计 | Per-Feature Design

### 3.1 需求 1：悬停自动静音播放（视频 / 动态照片）

**目标**：鼠标移入视频/动态照片格子 → 自动静音循环播放预览。

**方案**：
- **共享 `<video>` 元素池**（2–3 个），悬停挂载、移出回收。**严禁每格一个 video**（百万级库内存/解码爆炸）。
- 属性：`muted + loop + playsinline + preload="metadata"`。
- **悬停防抖 150–250ms**：快速划过不触发解码。
- `compact`（<100px，见 `MediaThumb.vue`）下**禁用**悬停播放。
- **动态照片来源**（schema 已区分）：
  - Apple Live：`companion_of` → 同目录 MOV，直接 `convertFileSrc` 播放。
  - Google Motion Photo：`has_embedded_video=1`，视频内嵌 JPEG（XMP `MicroVideoOffset`）→ 新增后端命令 `extract_motion_video(item_id)`，按字节偏移抽出 MP4 至 cache 再播放。
  - 普通视频：直接播原文件。
- **设置开关**：全局「悬停自动播放」开/关；「仅本地文件」（网络盘默认不自动播，省流量，见 §3.8）。
- **网络盘/超大视频降级**：不解码视频，改用 §3.3 预生成的关键帧 sprite 做悬停 scrub。

**落点**：`MediaThumb.vue`（悬停事件）+ 新增 `composables/useHoverPreview.ts`（video 池 + 防抖）+ `media_commands.rs::extract_motion_video`。

---

### 3.2 需求 2：视频画面比例 + 封面（后台任务）

**完善点**：**比例**与**封面**拆开，优先级不同。

- **比例（宽高 + rotation）**：极廉价（MF 读元数据），但**布局强依赖** → 放入**补全阶段**（§2.1）尽早回填。**必须处理 rotation**：手机竖拍视频带旋转元数据，与图片 EXIF orientation 同理会交换宽高，不处理则全部躺倒。
- **封面**：抽一帧解码缩放为 WebP，较重 → 走**派生框架** `kind=video_cover`，复用缩略图缓存与 `cache_key`，生成后写 `thumb_status`/`thumb_path`，前端 `MediaThumb` **零改动**即可显示。
- **取帧策略**：避开第 0 帧（常为黑帧）。取 `min(1s, 10%处)`，做简单非黑帧检测后跳。

**`video_meta` 扩展**（现仅 `video_codec`，远不够）：

```sql
ALTER TABLE video_meta ADD COLUMN fps           REAL;
ALTER TABLE video_meta ADD COLUMN bitrate       INTEGER;
ALTER TABLE video_meta ADD COLUMN rotation      INTEGER DEFAULT 0;
ALTER TABLE video_meta ADD COLUMN has_audio     INTEGER DEFAULT 0;
ALTER TABLE video_meta ADD COLUMN cover_time_ms INTEGER;   -- 封面取自哪一帧
-- 宽高/时长继续放主表 media_items（与图片统一）
```

**视频后端抽象** `trait VideoBackend`：
- `MediaFoundationBackend`（Windows，现阶段唯一实现，硬解 D3D11 优先、软解降级）。
- 占位：`AVFoundationBackend`（macOS/iOS）、`FfmpegBackend`（兜底/跨平台），后续阶段补。
- 与现有 `engine/gpu/wic_engine.rs`「OS 原生影像 API」思路一致（当前 GPU 路径本就是 Windows-first）。

落点：新增 `src-tauri/src/video/mod.rs` + `video/media_foundation.rs`；`Cargo.toml` 的 `windows` 依赖追加 `Win32_Media_MediaFoundation` feature（仅启用现有 crate 的更多模块，**无新外部二进制**）。

---

### 3.3 需求 3：关键帧提取（GPU 优先，可降级 CPU）

**已定（决策 1）**：关键帧 = **多张采样帧，用于悬停/进度条 scrub 预览**（非 I-frame 解析）。

**方案**：
- 抽 N 帧（5–10 张，按时长百分位均匀采样），**拼为单张 sprite 雪碧图**：单文件、单次解码、scrub 时仅换 `background-position`，远优于 N 个小文件。
- **GPU 优先 / CPU 降级**：MF Source Reader 配 `MF_SOURCE_READER_D3D_MANAGER` 走硬解；失败则去掉 D3D manager 软解。埋点「硬解是否成功」。
- **优化判断**：单张封面（§3.2）软解已足够快；**GPU 收益主要在「批量多帧 / 长视频 / 高分辨率」**，故 sprite 批量抽帧才上 GPU。
- 走派生框架 `kind=video_keyframes`，优先级**低于**封面。
- sprite 路径与网格规格存 `media_derivations.payload_path`（或 `video_meta`）。

**落点**：`video/media_foundation.rs::extract_keyframe_sprite()`；前端 `useHoverPreview.ts` 在视频不宜直接播放时切换为 sprite scrub。

---

### 3.4 需求 4：文档类型缩略图（布局算完后的后台任务）

**完善点**：与图片同构——文档先用**默认比例**（§2.1）进布局，真实缩略图**后台补**（派生框架 `kind=doc_thumb`，低优先级）。

**Lite 变体：前端离屏渲染（决策 4）；Perf 变体：native pdfium（见 §1.4，`DocThumbBackend`）。**

以下描述 **Lite 路径**——文档缩略图由 **Webview 离屏渲染 + 截图**生成（Perf 路径走 pdfium 并行栅格化，产物与缓存路径一致，前端 `MediaThumb` 无感）：

```
doc_thumb 队列 → 通知前端「隐藏渲染器」组件 → 按子类型渲染首页/封面到离屏 canvas
            → canvas 截图为 bytes → 交后端写入缩略图缓存（复用 cache_key/路径） → 标记 status=2
```

| 子类型 | 渲染方式（前端，零 native 捆绑） |
|---|---|
| pdf | **pdf.js** 渲染首页 → canvas 截图 |
| epub | 后端 `zip` crate 取 OPF `cover` 图（无则前端渲染首章首屏） |
| svg | Webview 原生 `<img>` 离屏绘制截图 |
| txt/md | **不栅格化**，前端用纯 CSS「文本卡」（首行 + 扩展名角标），更清晰、零解码 |
| office（doc/xls/ppt） | **首期仅类型图标占位**；后期评估（系统默认程序 / headless 转换） |

> 权衡：前端驱动的缩略图需主窗口存活、需节流（单线程）。这是「轻量」路线的取舍——以一点架构复杂度换取零 native 体积。视频缩略图仍在后端（MF），文档缩略图在前端，各取所长。

**落点**：新增隐藏组件 `components/media/DocThumbRenderer.vue`（受 doc_thumb 队列驱动、节流）+ 后端 `derive/doc.rs`（epub 封面 + 接收前端截图落盘）；`thumbnail/generator.rs` 的 document 分支委托给该流程。

---

### 3.5 需求 5：文档浏览器

#### 5.1 浏览器 + 多翻页方式

**方案**：新增路由 `/doc/:id` + `DocumentViewer.vue`，内部按格式选渲染器；**翻页逻辑抽成与渲染器解耦的 `composables/usePager.ts`**。

| 格式 | 渲染器（轻量） |
|---|---|
| txt/md | 前端原生渲染（md 用轻量解析器） |
| epub | **epub.js**（章节/分页/CSS） |
| pdf | **pdf.js**（自带文字层 → 选中/搜索/查找，且配合 5.2 替换） |
| office/excel | 后期；首期「调用系统默认程序打开」兜底 |

**翻页模式**（可切换，存配置）——`usePager` 暴露统一接口：
1. `scroll`：网页式连续滚动；
2. `wheel-snap`：滚轮一格 = 翻一页（节流 + 吸附）；
3. `keyboard`：↑↓ 滚屏、←→ 翻页。
4. 预留扩展：双页 / 卷轴 / 自动滚动（仅在 `usePager` 增 mode）。

**阅读进度**：

```sql
CREATE TABLE IF NOT EXISTS reading_progress (
    item_id    INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    position   TEXT NOT NULL,        -- 页码 / CFI(epub) / 滚动比例
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
```

#### 5.2 人名替换 / 角色扮演（替换规则存库）

```sql
CREATE TABLE IF NOT EXISTS doc_replacements (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_kind TEXT NOT NULL,        -- 'item' | 'group' | 'global'
    scope_id   INTEGER,              -- item_id 或书籍系列 id；global 为 NULL
    find       TEXT NOT NULL,
    replace    TEXT NOT NULL,
    is_regex   INTEGER DEFAULT 0,
    enabled    INTEGER DEFAULT 1,
    sort_order INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_repl_scope ON doc_replacements(scope_kind, scope_id) WHERE enabled = 1;
```

- **完善点**：规则可绑 `item` / `group`（同系列丛书共享人名映射）/ `global`。
- **应用时机**：文本进入渲染器**之前**替换（txt/epub 纯文本，易；pdf 走 pdf.js 文字层，受限，**首期仅 txt/epub**）。
- **性能**：多规则用 `aho-corasick` 建自动机一次扫描；中文无词边界，按「最长匹配优先」。
- **纯展示层**，不改源文件（与 5.3 严格区分）。

#### 5.3 编辑 / 校对 + 版本管理（类 git）

**已定（决策 2）**：版本**不进画廊**；「直接覆盖源文件」**默认禁用，仅高级开关 + 二次确认 + 自动先备份**。

**模型**（借鉴 git，但不引入真实 git 依赖）：源文件不可变为基线；版本独立成文件 + 元数据表。

```sql
CREATE TABLE IF NOT EXISTS document_versions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE, -- 原始件
    parent_id    INTEGER REFERENCES document_versions(id) ON DELETE SET NULL,   -- 父版本（成树）
    label        TEXT,                 -- "AI校对稿" / "我的修订"
    storage      TEXT NOT NULL,        -- 'appdata' | 'external'
    abs_path     TEXT NOT NULL,
    source       TEXT NOT NULL,        -- 'user' | 'ai-local' | 'ai-remote'
    note         TEXT,
    content_hash TEXT,
    is_current   INTEGER DEFAULT 0,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_docver_item ON document_versions(item_id);
```

**三种保存目标**（对应需求描述）：
1. **直接改源文件**：高级开关，禁用为默认；启用时先自动备份为一个 version。
2. **另存为新版本**（默认）：存 `appdata/documents/<content_hash>/<version_id>.<ext>`，**不进画廊**（避免污染）。
3. **存到用户指定目录 + 保存后自动导入**：走现有 scan/import 成为新 media_item，通过 `content_hash` + `version_of` 关联回原件。

**类 git 体验**：
- txt/epub **每版存全量快照**（便宜），用 `similar` crate **按需算 diff** 做对比/高亮、restore、从任意版本派生（`parent_id` 成树）。
- 版本时间线 UI：列版本、双版本 diff、设为当前、删除。
- **不引入真实 git**——单文档版本管理用「全量快照 + 按需 diff」更简单、无外部依赖、跨平台稳。

#### 5.4 AI 校对（远程 / 本地）

**已定（决策 5）**：**本地 AI 机制先搁置**（待用户调研）。本期完整设计**远程**路径，本地留**可插拔接口**占位。

- **文本 LLM 与 CLIP 是两套**，新增 `trait TextProofreader`：
  - `RemoteProofreader`（HTTP，OpenAI/Anthropic/兼容 API）；配置（base_url/model）存 `app_config`，**API key 存系统凭据库 `keyring`，不落明文 DB**。
  - `LocalProofreader`（占位，TBD：Ollama localhost / 进程内模型，待决策）。
- **流程**：文档分块 → 逐块发 LLM（校对 prompt：错别字/标点/语病）→ 返回修订 → 以 **track-changes 式建议**呈现 → 用户逐条接受/拒绝 → 接受后**生成新版本**（接 5.3，`source='ai-remote'`）。
- **新增依赖** `reqwest`（rustls，纯 Rust）——同时服务远程 AI、Ollama、§3.8 WebDAV。

---

### 3.6 需求 6：音频浏览器

**方案**（`lofty`，纯 Rust，符合轻量）：
- **封面/专辑图/元数据**：补全阶段 lofty 提取 → 封面走派生 `kind=audio_cover`（默认 400×400）→ 元数据回填 `audio_meta`。
- **歌词**：
  - 内嵌：ID3 `USLT`/`SYLT`、FLAC/Ogg vorbis comment `LYRICS` → lofty 读。
  - 同目录 `.lrc`：同名匹配 → 解析带时间轴 LRC → **同步滚动高亮**。
- **播放器 UI**：路由 `/audio/:id` + `AudioPlayer.vue`（封面 + 控件 + 同步歌词 + 元数据面板）；波形后期。

**`audio_meta` 扩展**（现有 artist/album/track）：

```sql
ALTER TABLE audio_meta ADD COLUMN track_no       INTEGER;
ALTER TABLE audio_meta ADD COLUMN year           INTEGER;
ALTER TABLE audio_meta ADD COLUMN genre          TEXT;
ALTER TABLE audio_meta ADD COLUMN lyrics_source  TEXT;    -- 'embedded'|'lrc'|'none'
ALTER TABLE audio_meta ADD COLUMN lyrics_path    TEXT;    -- 外部 .lrc 路径
```

---

### 3.7 需求 7：收藏分类（4 默认 + 自定义 + 收藏后提示）

**完善点**：**不造第三套机制**。现有 `is_favorited`（布尔，深度耦合 layout 缓存 `set_favorite_in_cache` + 索引 `idx_media_fav` + 过滤器）与 `albums/album_items`（通用多对多）组合即可。

- **保留 `is_favorited` 作快速收藏标志**（红心、索引、缓存快路径全不动）。
- **用 `albums` 承载收藏夹**：

```sql
ALTER TABLE albums ADD COLUMN kind              TEXT DEFAULT 'user';  -- 'system' | 'user'
ALTER TABLE albums ADD COLUMN media_type_filter TEXT;                 -- 系统夹：image/video/audio/document
ALTER TABLE albums ADD COLUMN icon              TEXT;
-- 启动播种 4 个 system 收藏夹（图/视/音/文档）
```

- **4 默认收藏夹** = `kind='system'`，语义 ≈「该类型 + is_favorited」。点红心时 `is_favorited=1` 且自动加入对应类型的 system 夹（快路径与索引双受益）。
- **自定义收藏夹** = `kind='user'`，可跨类型混装。
- **收藏后右下角提示**：复用 `ToastContainer.vue`，toast 内含「加入收藏夹 ▾」快捷 chips（最近用的几个 + 新建）。
- 路由 `/favorites` 扩为「收藏夹列表 + 进入某夹」。
- ⚠️ 若支持「拖到收藏夹」：必须用 pointer events（`usePointerDrag.ts`），**禁用 HTML5 DnD**（见 `gotcha-tauri-dragdrop` 记录）。

> 本需求独立、便宜、价值高 → 路线图安排最先做。

---

### 3.8 需求 8：SMB / WebDAV 连接

**已定（决策 3）**：**先做 8A（OS 挂载，快速可用），8B（原生 VFS）后置。**

#### 阶段 8A — OS 挂载盘当扫描根（快速，~1–2 天）

- Windows：SMB 直接用 UNC `\\server\share`；WebDAV 用映射盘符。
- 改动极小：`add_scan_root` 时把该路径加入运行时 asset scope（`allow_directory` 已有）。
- 局限：依赖 OS 挂载（WebDAV 在 Windows redirector 偶有不稳）。

#### 阶段 8B — 原生客户端 + 存储后端抽象（大工程，后置）

- `trait StorageBackend { walk(); stat(); read_range(); … }`，实现 `LocalFs` / `WebDav`（`reqwest_dav`，纯 Rust）/ `Smb`（**SMB 优先继续依赖 OS 挂载**；纯 Rust SMB 生态不成熟，捆绑 libsmbclient 违反轻量原则 → 暂不原生实现）。
- scanner 改为走 trait（当前全是 `walkdir`/`std::fs`，是主要改造量）。
- **显示**：远程文件 asset 协议读不了 → 自定义 Tauri URI 协议做**流式代理**（支持 Range，边下边播）；缩略图/封面落本地 cache，**cache 即显示源**，原图按需流式拉。
- **凭据**：

```sql
CREATE TABLE IF NOT EXISTS storage_backends (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,        -- 'local'|'smb'|'webdav'
    name       TEXT NOT NULL,
    host       TEXT,                 -- 或 base_url
    base_path  TEXT,
    username   TEXT,
    cred_ref   TEXT,                 -- keyring 引用，密码不落库
    options    TEXT,                 -- JSON
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
ALTER TABLE scan_roots ADD COLUMN backend_id INTEGER REFERENCES storage_backends(id);  -- NULL=本地
```

---

## 4. 数据库 DDL 汇总 | Schema Deltas

> 实现各阶段时按 `migration.rs` 模式追加为 `SCHEMA_V4`…`SCHEMA_V7`，并递增 `CURRENT_VERSION`。

- **V4（P0 + 视频 + 音频 + 文档缩略图）**：`media_derivations`、`video_meta` 扩列、`audio_meta` 扩列、`reading_progress`。
- **V5（收藏夹）**：`albums` 扩列 + 播种 4 个 system 夹。
- **V6（文档浏览器/编辑）**：`doc_replacements`、`document_versions`。
- **V7（网络盘 8B）**：`storage_backends`、`scan_roots.backend_id`。

（各表完整 DDL 见 §2.2 / §3.2 / §3.5 / §3.6 / §3.7 / §3.8。）

---

## 5. IPC 命令清单 | IPC Commands（新增）

| 模块 | 命令 | 用途 |
|---|---|---|
| media | `extract_motion_video(item_id)` | 抽出 Google Motion Photo 内嵌 MP4 |
| derive | `start_derivation(kind?)` / `pause_derivation` / `derivation_status` | 派生任务控制（仿 AI 三按钮） |
| video | （内部）封面/关键帧由派生框架调度，无需独立命令 | — |
| doc | `store_doc_thumbnail(item_id, png_bytes)` | 前端离屏渲染回传缩略图 |
| doc | `get_replacements(scope)` / `upsert_replacement` / `delete_replacement` | 替换规则 CRUD |
| doc | `list_versions(item_id)` / `save_version(item_id, content, target)` / `set_current_version` / `diff_versions(a,b)` / `restore_version` | 版本管理 |
| doc | `proofread_chunk(text, provider)` | AI 校对（远程） |
| doc | `get_reading_progress(item_id)` / `set_reading_progress` | 阅读进度 |
| audio | `get_audio_detail(item_id)` | 封面 + 元数据 + 歌词 |
| collections | `list_collections` / `create_collection` / `add_to_collection` / `remove_from_collection` / `recent_collections` | 收藏夹 |
| storage | `list_backends` / `add_backend` / `test_backend` / `remove_backend` | 网络盘（8B） |

---

## 6. 前端路由与组件 | Frontend

**新增路由**：`/doc/:id`、`/audio/:id`、`/video/:id`（全屏播放器，可选）、`/collections`、`/collection/:id`。

**新增组件 / composable**：
- `composables/useHoverPreview.ts`（video 池 + 防抖 + sprite scrub 降级）
- `components/media/DocThumbRenderer.vue`（隐藏离屏渲染器，受 doc_thumb 队列驱动）
- `views/DocumentViewer.vue` + `composables/usePager.ts`（三种翻页）+ 子渲染器（pdf.js / epub.js / text）
- `views/AudioPlayer.vue`（封面 + 控件 + 同步歌词）
- `views/CollectionsView.vue` + 收藏夹卡片
- 文档版本时间线 + diff 视图组件

**前端依赖**（JS，非 native）：`pdfjs-dist`、`epubjs`。

---

## 7. 依赖清单 | Dependencies

### Rust（后端）

| crate / 组件 | 用途 | 变体 / feature |
|---|---|---|
| `windows`（追加 `Win32_Media_MediaFoundation`） | 视频解码/取帧/元数据 | 两者；已有依赖仅启用更多模块 |
| `lofty` | 音频封面/标签/歌词 | 两者（纯 Rust，小） |
| `zip` | EPUB 解析 | 两者（纯 Rust，小） |
| `reqwest`（rustls） | 远程 AI / Ollama / WebDAV | 两者（纯 Rust） |
| `keyring` | 凭据安全存储 | 两者（调 OS API） |
| `similar` | 文档版本 diff | 两者（纯 Rust） |
| `aho-corasick` | 多规则替换 / native 文本 | 两者（纯 Rust） |
| `reqwest_dav` | WebDAV 客户端（8B） | feature `netfs` |
| **FFmpeg sidecar** | 视频取帧（MF 兜底/扩展格式） | **Perf**：feature `ffmpeg` + externalBin |
| **pdfium**（`pdfium-render`，load-dynamic） | PDF 缩略图/栅格化 | **Perf**：feature `native-doc` |
| ~~libsmbclient~~ | SMB | 不采用：依赖 OS 挂载 |

### 前端

`pdfjs-dist`、`epubjs`（JS 体积，非 native 二进制增量）。

---

## 8. 路线图 | Roadmap

按「依赖 + 性价比」排序（非按需求编号）：

### P0 — 地基 Foundations
- [x] 后端 trait 抽象 + Cargo feature（lite/perf）+ 双包构建脚本（§1.4）—— 随 P2 首个 MF 视频
  后端一并落地：`video/` 模块的 `VideoBackend` trait + `backend_for` 运行期选择；Cargo
  `lite`/`perf`/`ffmpeg` feature + `[profile.lite]` 尺寸 profile + `BUILD_VARIANT` 常量；
  `tauri:build:lite/perf` 脚本 + `tauri.perf.conf.json` 占位。**待补**：`DocThumb`/`Text`/
  `Storage` trait 与 FFmpeg/pdfium 真实组件随 P4/P5 落地（届时补 perf 配置的 externalBin/resources）。
- [x] 修复非图片项尺寸补全 + 按类型默认宽高（§2.1）
- [x] 派生任务框架 `media_derivations` + 分级让步调度（§2.2）—— 框架/调度/续传/孤儿恢复/IPC 已就绪；各 kind 的 `run` 实现为桩，随 P2/P3/P4 后端落地（`is_implemented` 翻转即激活）。
- [x] migration V4 + 各 meta 表扩列

### P1 — 快速见效 Quick wins
- [x] 需求 7 收藏夹（4 默认 + 自定义 + toast 提示）—— 后端 V5（albums 扩列 + 播种系统夹）
  + 查询/IPC；前端 CollectionsView/侧栏入口/收藏夹过滤/收藏后 toast chips。系统夹虚拟
  （类型 + is_favorited），用户夹存 album_items。**待补增强**：拖到收藏夹（pointer-events）、
  卡片封面缩略图、批量/详情页收藏也触发 toast。
- [x] 需求 1 悬停播放（先本地视频 + Apple Live Photo）—— `useHoverPreview`（共享池容量1 +
  防抖200ms + 源缓存）+ MediaThumb `<video>` 叠加 + 设置开关。**P2 续做**：Google Motion
  Photo 内嵌抽帧（`extract_motion_video`）、网络盘/超大视频降级为关键帧 sprite scrub。

### P2 — 视频 Video
- [x] 需求 2 视频比例（含 rotation）+ 封面（MF）—— enricher 走 MF 探测回填真实显示尺寸
  （rotation 修正）+ duration + `video_meta`（codec/fps/bitrate/has_audio）；封面走派生
  `video_cover`，解码非黑帧 → 复用缩略图缓存/键编码 WebP，回填 `thumb_status/thumb_path/thumbhash`
  到 `media_items` + 布局缓存，`MediaThumb` 零改动显示。
- [x] 需求 3 关键帧 sprite —— 派生 `video_keyframes` 跨视频均匀采样 10 帧，正立旋转 + 统一缩放，
  拼为水平 sprite 落 `cache/sprites/`，路径存 `media_derivations.payload_path`。
  **说明**：当前为 MF 系统内存解码（MF 内部按需走硬件解码器）；未做显式 D3D 纹理回读 ——
  对「解码后回读 CPU 编码缩略图」用途 GPU 回读收益甚微（§3.3 注），如需 DXVA 纹理路径后续增强。
- [x] 需求 1 补全：Google Motion Photo 内嵌抽帧（后端 `get_companion_video_url` 早已支持
  内嵌 MP4 偏移抽出，前端 `isLivePhoto` 路径直接复用）+ 超大视频 sprite scrub 降级
  （>200MB 悬停切 sprite，鼠标横移 background-position 切帧）。**待 P5**：网络盘降级判定。

### P3 — 音频 Audio
- [x] 需求 6 lofty 封面/元数据 + 内嵌/.lrc 歌词 + AudioPlayer —— 新增 `lofty` 依赖（纯 Rust）+
  `audio/` 模块（`read_tags`/`read_cover`/`find_lrc`/`resolve_lyrics_text`，封装标签/封面/歌词提取）。
  元数据走 enricher 补全阶段：`enrich_audios` 用 lofty 回填 `audio_meta`（codec/artist/album/title/
  track_no/year/genre + 歌词来源 embedded/lrc/none + .lrc 路径）+ 时长（与视频探测同处理）。封面走派生
  `audio_cover`（lofty 内嵌封面 → 复用缩略图编码/缓存，`is_implemented` 翻 true；无封面→status=3 回落
  音符占位，与视频封面同路径）。`get_audio_detail` IPC 懒读标签+歌词（既有库无需重扫）、按需抽全分辨率
  封面到 `cache/audio_covers/`。前端：路由 `/audio/:id` + `AudioPlayer.vue`（封面 + 控件 + 同步歌词 +
  元数据面板）+ `utils/lrc.ts`（LRC `[mm:ss.xx]` 解析 + 二分定位高亮，同步滚动）+ MediaGrid 音频导航。
  **待补**：波形、播放列表、SYLT 逐字歌词、用户夹音频也触发收藏 toast。

### P4 — 文档 Documents
- [x] 需求 4 文档缩略图（前端离屏渲染 + 文本卡）—— epub 封面由后端 `zip` 解析 OPF 取图 → 复用
  缩略图缓存/编码（走派生框架）；pdf/svg 为「前端驱动」：`DocThumbRenderer.vue` 隐藏离屏渲染
  （pdf.js 渲首页 / svg 经 `<img>` 绘制）经 `ensure_doc_thumb_queue`+`list_pending_doc_thumbs`+
  `store_doc_thumbnail` 回环落盘（自播种，不依赖派生流水线启动），`get_pending_derivations` 排除
  pdf/svg；txt/md/office 用 `MediaThumb` CSS「文本卡」。**待补**：文本卡首行预览、Perf 路径 pdfium。
- [x] 需求 5.1 浏览器 + 三种翻页 + 阅读进度 —— 路由 `/doc/:id` + `DocumentViewer`（按格式分发
  pdf.js / epub.js / 文本渲染器，懒加载）；`usePager` 三种翻页模式（scroll/wheel-snap/keyboard，
  存 `doc_pager_mode`）；阅读进度存 `reading_progress`（pdf=page:N / epub=CFI / 文本=滚动比例）；
  office 等走「系统默认程序打开」。**待运行时验证**：epub（epubjs iframe + asset 取 .epub，CSP 已放开）。
  **待补**：epub 连续滚动流、md 首行预览、双页/自动滚动模式。
- [x] 需求 5.2 替换/角色扮演（txt/epub）—— migration V6（`doc_replacements` + `document_versions`）；
  规则 CRUD（item/global 作用域）后端 queries/IPC（`list_replacements`/`get_effective_replacements`/
  `upsert_replacement`/`delete_replacement`）；前端 `utils/replacements.ts`（字面量单次扫描·最长匹配
  优先 + 正则顺序应用，Lite JS 路径）；txt/md 经 `TextReader` transform 钩子、epub 经 epubjs content
  钩子就地替换文本节点；`ReplacementPanel.vue` 管理规则，变更即重渲染。**待补**：group（丛书）作用域、
  pdf 文字层替换、Perf aho-corasick。
- [x] 需求 5.3 编辑 + 版本管理（快照 + diff）—— `document_versions`（V6 已建）；后端 queries/IPC：
  list_versions / get_current_version / get_document_text（当前版本或源）/ get_version_content /
  save_version（target=version 存 `<appData>/documents/<item>/<id>.<ext>`，不进画廊；target=overwrite
  覆盖源文件前自动备份旧源为版本）/ set_current_version / delete_version / diff_versions（similar 行级
  diff，源基线=null）。前端：DocumentViewer 文本编辑态（textarea + 另存为新版本/覆盖源文件二次确认）；
  `VersionPanel.vue` 版本时间线（设为当前/删除/与原始 diff 红绿渲染）；TextReader 增 content prop 读当前
  版本。**待补**：存到用户指定目录 + 自动导入画廊（target=export）、版本树父子可视化、epub 编辑。
- [x] 需求 5.4 AI 校对（远程；本地待决策 5）—— 新增 reqwest(rustls)/keyring 依赖；`proofread` 模块
  `proofread_remote`（OpenAI 兼容 /chat/completions，中文校对 prompt，base_url 仅 http(s) 校验）；
  IPC：get/set_proofread_config（base_url/model 存 app_config）、set/clear_proofread_key（key 存 keyring，
  不落 DB）、proofread_chunk；新增 diff_texts（原文↔修订 track-changes 预览）+ save_version 增 source
  参数。前端 `ProofreadPanel.vue`：配置表单 → 分块逐块校对 → track-changes 红绿预览 → 接受存为新版本
  （source='ai-remote' + 设为当前，接 §5.3）。本地实现（Ollama/进程内）按 D5 留可插拔接口待调研。
  **待补**：逐条接受/拒绝（当前为全部接受）、SettingsView 集中配置入口、流式输出。

### P5 — 网络盘 Network
- [x] 需求 8A OS 挂载盘当扫描根 —— 新增 `normalize_root_path`（root 专用规范化，**保留 UNC `//server/share`
  前缀**，避免 `normalize_db_path` 把绝对 UNC 路径剥成无意义相对路径；盘符/本地路径行为不变，含单测）；
  `add_scan_root` 改用之。映射网络盘（`Z:\`）与 UNC 共享经原生文件夹对话框即可添加，扫描/asset 授权复用
  现有路径（`allow_directory` 已有）。这是 Windows 上**可用的**网络存储路径（D3：SMB 继续依赖 OS 挂载）。
- [x] 需求 8B 存储后端抽象 + WebDAV 原生 —— migration **V7**（`storage_backends` + `scan_roots.backend_id`，
  NULL=本地/8A）；`storage/` 模块：`trait StorageBackend`（kind/list_dir/stat/read_range/test）+ `LocalFs`
  （纯 std::fs，两变体，8A 即用它）+ `WebDavBackend`（feature `netfs`，`reqwest_dav` rustls；持 current-thread
  运行时桥接异步→同步 trait，支持 Range 读取）。CRUD/测试 IPC（`list/add/test/remove_backend`，密码存
  keyring 不落库，仅 `cred_ref`）；`build_backend` 运行期选择，Lite 选 webdav 时返回清晰「需性能版」降级提示。
  前端 `NetworkStorageSection.vue`（设置页：列出/测试/添加/移除 WebDAV·SMB·本地后端）。
  **待补（与 D3「8B 后置」一致，属大改造）**：把 scanner 改为走 `StorageBackend` trait 遍历远程（当前
  scanner 仍 walkdir/std::fs）+ 自定义 Tauri URI 协议的**流式代理**（Range 边下边播，远程原图本地 cache 即显示源）。

---

## 9. 风险与对策 | Risks

| 风险 | 对策 |
|---|---|
| MF 不支持的容器（mkv/webm/flv） | 检测系统 ffmpeg / 可选下载；缺失降级「无封面图标」 |
| 前端离屏渲染缩略图占用主窗口/单线程 | 严格节流、队列化、仅在窗口可见时跑、可暂停 |
| pdf.js/epub.js 体积膨胀 JS 包 | 路由级懒加载（router 已用动态 import），按需加载 |
| 「直接覆盖源文件」误操作 | 默认禁用 + 高级开关 + 二次确认 + 自动备份为 version |
| 远程 AI 的 key 泄露 / SSRF | key 入 keyring；base_url 白名单/校验 |
| 网络盘随机读慢、asset 协议读不了远程 | 8A 先依赖 OS 挂载；8B 走本地 cache + 流式代理 |
| 跨平台（MF 仅 Windows） | `trait VideoBackend` 抽象；macOS/iOS 用 AVFoundation 后补（当前 GPU 路径本就 Windows-first） |

---

## 10. 决策记录 | Decisions

### 已定 Decided
- **D1（需求3 关键帧）**：= 多张采样帧 sprite，用于悬停 scrub 预览。
- **D2（需求5.3 版本）**：版本不进画廊；「直接覆盖源文件」默认禁用，仅高级开关 + 二次确认 + 自动备份。
- **D3（需求8）**：先 8A（OS 挂载），8B 原生 VFS 后置。
- **D4（体积）**：轻量是核心卖点。**Lite 变体**禁大体积 native 捆绑（MF / pdf.js / OS 挂载）。
- **D6（分发形态）**：2 个编译产物（`lite`/`perf`）→ **3 个分发包：完整包 / 轻量+组件包 / 轻量包**（§1.4.6）。`perf` 二进制用 load-dynamic + sidecar，本体小，组件可捆绑或离线补装；`lite` 二进制极致裁剪（§1.4.7，但不可用 `panic=abort`）。共用 §1.4 后端 trait 抽象。
- **D7（轻量定义校准）**：**纯 Rust crate 的小体积增量可接受**（§1.1）。lofty/zip/similar/aho-corasick/reqwest(rustls)/keyring 等两变体都含，不构成「重」；「重」边界 = 是否捆绑大体积 native blob（FFmpeg/pdfium/ORT）。故 `lite` 不再为这些小 crate 上 `optional`+`cfg` 门控（§1.4.7 的极致裁剪仅针对 reqwest/tokio-full 等最大贡献者 + 尺寸 profile）。

### 待定 TBD
- **D5（需求5.4 本地 AI 机制）**：Ollama（外部进程）vs 进程内模型 —— **用户调研后再定**；本期仅实现远程 + 留可插拔接口。

---

*本文档为活文档，随实现推进更新。落地时同步更新 `architecture_notes.md` 的相关不变量与数据流。*
