# 冷门格式引擎与付费插件系统设计 v1 | Exotic-Format Engine & Paid-Plugin Plan v1

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic 初代设计 v1,被 v2 → v3.1 取代(v3 全套见同目录 exotic_format_plugin_plan/,亦已归档);现行权威 = refactor_2026/Part6。

> 为「音 / 视 / 文 / 图」四类媒体的**冷门格式**（psd · heic · word · rmvb 及更多）建立一套
> **独立于主引擎**、**独立任务管理**、**可扩展插口**、**插件化付费**的处理子系统。
> 前期以 **PSD** 为垂直切片跑通整条线。
>
> An independent, separately-scheduled, extensible, **paid-plugin** subsystem for *exotic* media
> formats across image/video/audio/document. Ship the **PSD** vertical slice first.

---

## 0. 文档信息 | Meta

| 项 | 内容 |
|---|---|
| 版本 Version | v1（2026-06-23） |
| 状态 Status | 草案待评审 Draft / 部分决策待定 |
| 关联文档 Related | `implementation_plan_v1.2.md`（母设计）、`architecture_notes.md`（活文档）、`feature_expansion_plan_v1.md`（§1.4 后端抽象与构建变体）、`face-recognition-plan`（独立 pipeline 范式先例） |
| 决策前提 Decisions | ① 付费交付形态 = **独立插件包下载**（购买后才提供下载）；② 未授权时冷门文件 = **入库 + 「需购买」占位** |

### 0.1 一句话定位 | TL;DR

冷门格式子系统是「**第四类独立 AI/派生级子系统**」——继 CLIP 语义分析、人脸识别之后，第三套**不挂主派生框架、自带 pipeline + 状态机 + 调度令牌**的后台处理线；其能力以**进程隔离的 worker 插件**形态交付，受 **license 门控**，购买后从远程注册表下载启用。

---

## 1. 背景与现状 | Background & Current State

### 1.1 「冷门格式」当前的两种失败模式

调研代码后确认，今天冷门格式存在**两种并存的失败**：

| 失败模式 | 触发格式 | 现状代码路径 | 表现 |
|---|---|---|---|
| **A. 入库但解不出** | `psd` `heic` `heif` `avif` `cr2/cr3/nef/arw/dng…`（RAW）、`word/excel/ppt`（office）、各类冷门音频 | `classify_media_type` 已归类（`utils/format.rs:44-60`）→ walker 入库；但 `EngineArena::phase1()` 只装 `ImageRsEngine`（`engine/mod.rs:28`），`engine_for("psd")` → `None` → `thumbnail/generator.rs:337` 抛 `UnsupportedFormat`；office/冷门音频 → `media_type != image` 走 `generator.rs:265` 直接 `thumb_status=2`（无缩略图） | 画廊里是空白/占位方块，无封面 |
| **B. 根本不识别** | `rmvb` `rm` `wpd` `dwg` `ai`（Illustrator）… 等**不在分类表里**的扩展名 | `walker.rs:79` `classify_media_type` 返回 `None` → `continue` 直接跳过 | 文件完全不进库，用户感知不到它们存在 |

> 结论：冷门格式不是「没做」，而是「识别层与解码层脱节」。本设计统一收口这两条裂缝。

### 1.2 为什么必须「独立」，不能并入主引擎

用户明确要求：冷门格式引擎**不应强行并入主引擎**（易出问题 + 不便性能调优）。代码层面有三条硬理由支撑这一判断：

1. **稳定性隔离**：冷门解码器（PSD 图层合成、RMVB 容器解析、Word OLE/OOXML 解包）远比 JPEG 脆弱，畸形文件易崩溃。主缩略图路径目前靠 `catch_unwind`（`generator.rs:49 panic_guard`）兜底，但那只能拦 Rust panic，拦不住 C 库段错误/死循环/内存爆炸。**进程级隔离**才是正确粒度。
2. **性能可调优**：主派生 pipeline（`derive/pipeline.rs`）与缩略图/视频封面同处一条优先级阶梯（`state.rs:258` scan > thumbnail > derivation > AI）。把慢且重的冷门解码塞进去，会与常见格式争用 rayon 池与让步窗口，正是用户担心的「拖慢常见格式」。独立子系统 = 独立令牌 + 独立池 + 独立让步策略，可单独限核/限速。
3. **商业边界**：冷门能力要**按插件售卖**。若代码与主引擎纠缠，无法干净地「未购买则不可用」。独立子系统天然形成 license 门控边界。

### 1.3 已有的可复用范式（不另起炉灶）

本设计**最大化复用**仓库里三套成熟机制，降低落地风险：

| 复用对象 | 位置 | 本设计如何借用 |
|---|---|---|
| **独立 pipeline 范式** | `ai/face_pipeline.rs`（Producer→Preprocessor→Detect→Writer + 独立 `face_analysis_token` + 不挂 `media_derivations`） | 冷门引擎照搬「独立令牌 + 状态机列 + 断点续传 + 孤儿恢复」，但消费者换成 **worker 子进程** |
| **产物回填机制** | `derive/pipeline.rs:480 write_results`（派生封面 `produces_thumbnail` → mirror 到 `media_items.thumb_status/thumb_path` + `layout_cache`） | 冷门引擎产出缩略图后，**完全复用**这条回填路径，画廊零改动即可显示 |
| **下载 + 校验基础设施** | `ipc/ai_commands.rs:958 download_assets`（断点续传 HTTP Range + sha256 + 镜像回退 + 进度 Channel） | 插件包/worker 下载直接复用 `download_assets`，仅换资产清单来源 |
| **凭据安全存储** | `keyring`（`proofread/mod.rs` API key 不落明文 DB） | license token 存 keyring，不落库 |
| **能力注册表 + 在线发现** | `ai/remote_registry.rs`（架构→变体发现）、`ai/profile.rs`（`ModelProfile`）、`ai/face_profile.rs`（`commercial_ok`） | 插件注册表（manifest）+ 远程目录发现照此模式 |

`✶ 设计哲学 ────────────────────────────────`
冷门子系统在架构上是「**face pipeline 的形状** + **下载/注册表的血肉** + **进程隔离的骨架**」。
几乎每一块都有先例，真正全新的只有两样：**worker 插件协议** 与 **license 门控**。
`──────────────────────────────────────────`

---

## 2. 设计目标与原则 | Goals & Principles

### 2.1 目标 | Goals

- **G1 独立**：冷门引擎自成子系统，不修改/不污染 `EngineArena`、`derive` 框架与主缩略图路径的热路径。
- **G2 隔离调度**：独立令牌、独立工作池、独立让步策略，可单独暂停/限速/调优，绝不拖慢常见格式。
- **G3 可扩展插口**：**新增一个冷门格式 = 发布一个新插件包，主程序零改动**（开闭原则）。
- **G4 插件化付费**：能力以独立可下载插件交付，受 license 门控；购买后下载启用，未购买则「需购买」占位。
- **G5 跨平台**：插件形态须能在 Windows/macOS（及未来移动端）分发；主程序对插件的契约与平台无关。
- **G6 轻量优先**：主安装包不因冷门能力变大（呼应 `feature_expansion_plan §1.1` Lite 原则）；冷门 = 纯按需下载。
- **G7 PSD 先行**：v1 只把 PSD（图）一条线打通，验证「识别→门控→下载→worker→缩略图→画廊」全链路。

### 2.2 不变量 | Invariants to preserve

引自 `architecture_notes.md`，本设计不得破坏：

1. `cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`；缩略图落 `cache/thumbnails/{size}/{2-hex}/{cache_key_hex}.webp`。冷门缩略图**复用同一键与路径**，使 `MediaThumb` 像普通缩略图一样加载。
2. 用户可见查询恒附加 `is_deleted = 0 AND companion_of IS NULL`。
3. `thumb_status` 在 layout 缓存中按 id 原位 O(1) 同步；冷门产物回填走 `layout::cache::apply_thumb_results`（同派生）。
4. asset 协议运行时按扫描根/缓存目录授权；插件目录 `{app_data}/plugins/` 须一并 `allow_directory`（用于前端读插件图标等资源）。

### 2.3 两个正交维度的厘清（重要）| Two orthogonal axes

`feature_expansion_plan §1.4` 已有「Lite / Perf 后端变体」概念，易与本设计混淆。二者**正交**：

| 维度 | 取值 | 决定什么 | 主开关 |
|---|---|---|---|
| **后端变体**（已有） | Lite / Perf | 同一能力用**轻**（MF / pdf.js / 纯 Rust）还是**重**（FFmpeg / pdfium）后端 | Cargo feature（编译期） |
| **冷门插件**（本设计） | 未装 / 已购未装 / 已装已授权 | 是否**拥有某冷门格式能力**，以及是否**付费解锁** | license + 运行期下载 |

> 一个冷门插件**自身**也可有 Lite/Perf 之分（如 RMVB 插件 Lite=系统已装解码器探测、Perf=自带 FFmpeg），但那是插件内部的事，对主程序透明。**本设计只管「插件这一维」**。

---

## 3. 总体架构 | Architecture Overview

### 3.1 分层全景 | Layered view

```
┌──────────────────────────────────────────────────────────────────────────┐
│  扫描层 Scan         walker → classify_media_type（扩展冷门扩展名表）         │
│                      入库 media_items(media_type, file_format, exotic_status)│
└───────────────┬──────────────────────────────────────────────────────────┘
                │ exotic_status=0 的冷门项
┌───────────────▼──────────────────────────────────────────────────────────┐
│  插件宿主 Plugin Host  (src-tauri/src/exotic/host.rs)                        │
│   · 发现已装插件 → 解析 manifest → 建「格式 → 插件」路由表                     │
│   · 查 license（keyring）→ 标注每格式「已授权 / 需购买 / 无插件」              │
└───────────────┬──────────────────────────────────────────────────────────┘
                │ 仅「已装+已授权」的格式进入处理
┌───────────────▼──────────────────────────────────────────────────────────┐
│  独立流水线 Exotic Pipeline (src-tauri/src/exotic/pipeline.rs，仿 face)      │
│   Producer(查 pending) → Dispatcher → [Worker 进程池] → Writer(回填)         │
│   · 独立 exotic_analysis_token  · 独立工作池  · 独立让步策略                  │
└───────────────┬───────────────────────────────┬──────────────────────────┘
                │ JSON+二进制 over stdio          │ 产物（WebP 字节 / 元数据 JSON）
┌───────────────▼───────────────┐  ┌────────────▼──────────────────────────┐
│  Worker 插件（独立可执行）      │  │  产物落地 Sink（复用 derive 回填）        │
│  {app_data}/plugins/<id>/...   │  │   缩略图 → thumb cache + media_items     │
│  · psd-worker.exe（v1）        │  │   元数据 → audio_meta/document_meta…    │
│  · 纯 Rust psd + image(webp)   │  │   layout_cache O(1) 同步 → 画廊刷新      │
└────────────────────────────────┘  └─────────────────────────────────────┘

        ╔═══════════════════════ 旁路子系统 Side concerns ═══════════════════╗
        ║ License  (exotic/license.rs)  离线公钥验签 + keyring 存取           ║
        ║ Registry (exotic/registry.rs) 远程插件目录发现 + 下载（复用下载基建）║
        ╚════════════════════════════════════════════════════════════════════╝
```

### 3.2 与现有子系统的关系 | Relationship to existing subsystems

| 现有子系统 | 是否改动 | 说明 |
|---|---|---|
| `EngineArena` / `ImageEngine`（主图像解码） | **不改** | 冷门格式**不**注册进 arena。`engine_for("psd")` 仍返回 `None`——但主缩略图路径不再「对冷门格式报错」，而是**识别为冷门并跳过**（交给冷门子系统），见 §6.3。 |
| `derive` 派生框架 | **不改** | 冷门引擎自带 pipeline，**不**新增 `DerivationKind`。理由同 face：派生框架是「主引擎」的一部分，并入会破坏 G1/G2。 |
| `thumbnail/generator.rs` | **极小改** | 仅在分派处增加一个判断：`media_type/format` 属冷门 → 返回新的 `DecodeResult::ExoticDeferred`（不报错、不占主路径），由冷门 pipeline 接管。见 §6.3。 |
| `state.rs` 优先级阶梯 | **小增** | 新增 `exotic_analysis_token` + 让步函数；阶梯插入位置见 §7.3。 |
| `db` schema | **+V9** | 新增 `exotic_status` 列 + 插件安装/授权记录。见 §6。 |

### 3.3 数据流：一张 PSD 的一生 | Lifecycle of one PSD

1. **扫描**：walker 识别 `.psd` → `media_type=image`、`file_format=psd`、`exotic_status=0` 入库。
2. **宿主路由**：host 查路由表——`psd` 属插件 `exotic-image-psd`；license 查询 = 已授权且已装。
3. **入队**：Producer 查 `exotic_status=0 AND 格式已授权已装` → 标 `processing` → 派给 worker 进程池。
4. **Worker 解码**：psd-worker 收任务 `{op:"thumbnail", path, target_long_edge:480}` → 解码 PSD 合成图 → 缩放 → 编码 WebP → 回传字节。
5. **回填**：Writer 把 WebP 写入 `thumb cache`（复用 `cache_key`）→ mirror `media_items.thumb_status=1/thumb_path/thumbhash` → `layout_cache` O(1) 同步 → emit `db:media_enriched` 通知画廊刷新。
6. **显示**：`MediaThumb` 像普通缩略图一样显示——**前端零改动**。

> 若第 2 步 license = 需购买：项保持 `exotic_status=0`，Producer **不领取**（类比 `derive` 的 `disabled_kinds`），前端按「格式未授权」渲染**锁占位角标**（§5.5）。

---

## 4. 插件模型 | Plugin Model

### 4.1 形态选型：为什么是「进程隔离的 worker 子进程」| Why sidecar processes

三种主流原生插件形态对比（针对「重型媒体解码 + 付费 + 跨平台 + 隔离」诉求）：

| 形态 | 隔离性 | ABI 稳定性 | 跨平台分发 | 性能 | 安全 | 结论 |
|---|---|---|---|---|---|---|
| **A. 动态库 cdylib（libloading）** | ✗ 同进程，崩溃拖垮主程序 | ✗ Rust 无稳定 ABI，须 `extern "C"`/`abi_stable` 苦工 | △ 每平台 .dll/.dylib/.so | ◎ 原生、零 IPC | ✗ 加载任意 dll 风险高 | 否决（违背 G2 隔离） |
| **B. Worker 子进程（externalBin 形态 + stdio IPC）** | ◎ **进程级隔离**，崩溃只丢一项 | ◎ 协议即序列化，天然稳定 | ◎ 每平台一个可执行 | ○ IPC 开销可忽略（只传文件路径与小缩略图字节） | ○ 下载二进制须签名校验，但边界清晰 | ✅ **采用** |
| **C. WASM 沙箱** | ◎ 沙箱 | ◎ 稳定 | ◎ 单产物 | ✗ 解码库 WASM 生态不全、像素搬运贵 | ◎ 最安全 | 未来可选（轻量纯算格式），重解码不合适 |

**选定 B（Worker 子进程）的关键理由**，逐条对齐用户诉求：

- **「避免影响常见格式速度」「不强行并入」** → 独立进程，连地址空间都分开，主程序解码 JPEG 与 worker 解码 PSD 物理隔离。
- **「容易造成问题」** → 畸形文件让 worker 进程崩溃，主程序只收到一个错误码，标 `exotic_status=3` 继续——比 `catch_unwind` 更强。
- **「方便后期性能调优」** → worker 进程可独立设置优先级、CPU 亲和、内存上限、并发数，互不影响。
- **「做好扩展插口」** → 新格式 = 新 worker 可执行 + manifest，主程序按协议通信，**无需重编**。这是 cdylib 做不到的开闭性。

> Worker **不经** Tauri 的 `externalBin`（那是编译期捆绑进安装包的，与「购买后下载」矛盾）。改为运行期下载到 `{app_data}/plugins/<id>/`，主程序后端用 `std::process::Command` 直接以**绝对路径**启动（后端 Rust 不受 Tauri shell allowlist 限制），启动前**强制 sha256 + 签名校验**（§5.3）。

### 4.2 插件包结构 | Plugin package layout

一个插件包 = 一个可下载压缩包，解开后落在 `{app_data}/plugins/<plugin_id>/`：

```
{app_data}/plugins/exotic-image-psd/
├── manifest.json          # 插件清单（见 4.3），随包分发
├── manifest.sig           # 厂商私钥对 manifest.json 的签名（主程序内置公钥验签）
├── bin/
│   ├── psd-worker-x86_64-pc-windows-msvc.exe
│   ├── psd-worker-aarch64-apple-darwin
│   └── …                  # 各平台 worker 可执行
├── icon.svg               # 插件市场/占位角标图标
└── LICENSE-3RD-PARTY.txt  # 第三方解码库许可（商业合规留痕）
```

### 4.3 插件清单 manifest.json | Plugin manifest

清单是「主程序 ↔ 插件」的唯一契约。**主程序对插件的全部认知都来自它**，这是 G3 可扩展的根基：

```jsonc
{
  "schema": 1,                       // manifest 协议版本（向后兼容靠它）
  "id": "exotic-image-psd",          // 全局唯一插件 id（= license/路由/目录键）
  "name": "PSD 图像引擎 | PSD Image Engine",
  "version": "1.0.0",
  "vendor": "Picasa Next",
  "protocol_version": 1,             // worker IPC 协议版本（见 4.4）

  "media_kind": "image",             // image | video | audio | document（决定产物落地路径）
  "formats": ["psd", "psb"],         // 本插件认领的扩展名（小写，无点）
  "capabilities": ["thumbnail", "metadata"],  // worker 支持的 op 集合

  "license": {
    "tier": "paid",                  // free | paid
    "sku": "psd-engine-2026"         // 购买项标识，对应 license token 的授权范围
  },
  "commercial_ok": true,             // 内含解码库是否允许商业分发（亲验，见 §11.4）

  "min_host_version": "0.1.0",       // 要求的最低主程序版本
  "bin": {                           // 各 target triple → 相对可执行路径
    "x86_64-pc-windows-msvc": "bin/psd-worker-x86_64-pc-windows-msvc.exe",
    "aarch64-apple-darwin":   "bin/psd-worker-aarch64-apple-darwin"
  },
  "limits": {                        // 性能调优旋钮（主程序据此约束 worker）
    "max_concurrency": 2,            // 同时最多几个 worker 进程
    "task_timeout_ms": 30000,        // 单任务超时 → 杀进程标错误
    "mem_soft_limit_mb": 1024
  }
}
```

`✶ 扩展插口的本质 ────────────────────────`
主程序代码里**没有一处** `if format == "psd"`。它只认 manifest 字段：`formats` 决定路由、
`capabilities` 决定能发哪些 op、`media_kind` 决定产物往哪落、`bin` 决定启哪个可执行。
**加一个 RMVB 插件 = 上架一个新 manifest + worker，主程序一行不改**——这就是 G3。
`──────────────────────────────────────────`

### 4.4 Worker IPC 协议 | Worker protocol

主程序与 worker 之间走 **stdio + 长度前缀帧**（length-prefixed framing），一个 worker 进程**长驻**处理多个任务（避免逐文件 spawn 的开销）：

- **控制/元数据**：JSON（UTF-8）。
- **二进制产物**（缩略图字节）：4 字节小端长度前缀 + 原始字节，避免 base64 膨胀。

```rust
// src-tauri/src/exotic/protocol.rs（主程序与 worker 共享的协议定义，可抽成独立 crate 复用）
// Worker 协议定义 | Worker protocol — 主程序与 worker 必须同版本 protocol_version。

/// 主程序 → worker 的请求。 | Host → worker request.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkerRequest {
    /// 握手：worker 回 Ready{protocol_version, capabilities}，对不上即拒绝启用。
    Hello { host_version: String },
    /// 生成缩略图：返回 WebP 字节（图/视/文）。
    Thumbnail { item_id: i64, path: String, target_long_edge: u32 },
    /// 提取元数据：返回结构化 JSON（音频标签 / 文档页数 / 视频时长等）。
    Metadata { item_id: i64, path: String },
    /// 优雅退出。
    Shutdown,
}

/// worker → 主程序的应答。 | Worker → host response.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerResponse {
    Ready { protocol_version: u32, capabilities: Vec<String> },
    /// 缩略图就绪：JSON 头给出字节长度与可选 thumbhash，随后跟二进制帧。
    ThumbnailReady { item_id: i64, byte_len: u32, thumbhash: Option<Vec<u8>> },
    /// 元数据就绪。
    MetadataReady { item_id: i64, fields: serde_json::Value },
    /// 单项失败（不致命，主程序标 exotic_status=3 继续）。
    Failed { item_id: i64, message: String },
}
```

**为何长驻而非每文件 spawn**：psd-worker 进程启动需加载解码库（数十 ms），逐文件 spawn 在百万级库下不可接受。长驻进程从 stdin 流式读任务、stdout 流式回结果，正好匹配 §7 的 Producer→Worker→Writer 流水线。

### 4.5 产物落地按 media_kind 分派 | Output sink by media kind

四类媒体的 worker 产物不同，落地路径也不同（均复用既有表/缓存）：

| media_kind | 主产物 op | 落地（复用现有机制） |
|---|---|---|
| image（psd/heic/RAW…） | `thumbnail` | 缩略图 → thumb cache + `media_items.thumb_status`（同 §3.3） |
| video（rmvb/rm…） | `thumbnail`（封面帧）+ `metadata`（时长/旋转/fps） | 封面同上；元数据 → `video_meta`（`schema.rs:156`） |
| audio（ape/dsd/冷门…） | `thumbnail`（内嵌封面）+ `metadata`（标签/歌词） | 封面同上；元数据 → `audio_meta`（`schema.rs:163`） |
| document（word/wpd…） | `thumbnail`（首页）+ `metadata`（页数）+（可选）`text` | 缩略图同上；元数据 → `document_meta`（`schema.rs:174`） |

> v1 只实现 **image + thumbnail**（PSD）。其余 op/落地为协议预留，后续插件按需启用——主程序的 Sink 按 `media_kind` 表驱动分派，新增 kind 不改 worker 调度核心。

---

## 5. License、注册表与分发 | License, Registry & Distribution

### 5.1 总体：购买 → 下载 → 激活 三段 | Buy → Download → Activate

```
①购买（外部商店/官网）         ②下载插件包                ③激活验证
用户付款 → 收到 license token   list_exotic_registry      install_exotic_plugin
（含 sku + 用户标识 + 签名）  → 从远程目录拉清单          → 校验 sha256 + manifest 签名
                              → download_assets 下载包   → 解压到 plugins/<id>/
                                                          activate_exotic_plugin
                                                          → 验 license token（公钥验签）
                                                          → 存 keyring → 路由表标「已授权」
```

### 5.2 License 校验：离线公钥签名（无需自建服务器）| Offline signature verification

**选定离线验签**（而非在线激活服务器），理由：桌面应用、用户决策已选「独立插件包下载」、零运维、可断网用。

- **签发**（厂商侧，离线）：用 Ed25519 私钥对 license 载荷签名，产出 license token（建议 base64）：
  ```jsonc
  // license token 载荷（签名前） | license payload
  { "sku": "psd-engine-2026", "plugin_id": "exotic-image-psd",
    "subject": "<购买者邮箱/订单号哈希>", "issued_at": 1750636800,
    "expires_at": null }          // null = 永久授权；非空 = 订阅到期
  ```
- **校验**（主程序侧）：内置厂商 **公钥**（编进二进制），`exotic/license.rs` 验签 + 校验 `plugin_id` 匹配 + 未过期。通过则视为已授权。
- **存储**：验证通过的 token 存 **keyring**（service=`picasa-next-license`, account=`<plugin_id>`），不落明文 DB（复用 `proofread` 的 keyring 先例）。

```rust
// src-tauri/src/exotic/license.rs（骨架）
// 离线 license 校验 | Offline license verification（Ed25519 公钥内置）。

/// 厂商公钥（编译期内置）。私钥仅在签发侧，绝不入库。
const VENDOR_PUBKEY: &[u8; 32] = include_bytes!("../../keys/vendor_ed25519.pub");

#[derive(serde::Deserialize)]
pub struct LicensePayload {
    pub sku: String,
    pub plugin_id: String,
    pub subject: String,
    pub issued_at: i64,
    pub expires_at: Option<i64>,
}

/// 校验 token 对某插件是否构成有效授权。 | Verify a token authorises `plugin_id`.
/// 失败原因细分，便于前端区分「无 license / 签名错 / 过期 / 张冠李戴」。
pub fn verify(token: &str, plugin_id: &str) -> Result<LicensePayload, LicenseError> {
    // 1) base64 解码 → 分离 payload 与 64 字节 Ed25519 签名
    // 2) ed25519_dalek 用 VENDOR_PUBKEY 验签（防伪造）
    // 3) payload.plugin_id == plugin_id（防张冠李戴）
    // 4) expires_at 为空或 > now（防过期）
    todo!()
}
```

> **防破解的诚实边界**：离线验签能挡住「无 key 直接用」和「篡改 token」，但挡不住「逆向去掉校验调用」（任何本地校验皆然）。这是商业软件的通病，可接受。提升门槛的增量手段（v2 可选）：license 与机器指纹绑定、worker 自身也校验一段由 license 派生的口令、关键解码逻辑放 worker 且 worker 启动需主程序传入 license 派生密钥。**v1 不做这些**，先把链路跑通。

### 5.3 插件包完整性与执行安全 | Package integrity & exec safety

下载并执行第三方二进制是**本系统最大的安全面**，三道闸：

1. **传输完整性**：注册表清单含每个文件 sha256 + size；下载后逐一校验（复用 `download_assets` 的 `sha256_matches`，`ai_commands.rs:1179`）。
2. **来源真实性**：`manifest.sig` = 厂商私钥对 `manifest.json` 的 Ed25519 签名；`install` 时用同一 `VENDOR_PUBKEY` 验签。**manifest 里记录了各 worker 二进制的 sha256**，故验签 manifest 即间接锁定二进制内容。
3. **执行前再校验**：每次启动 worker 前，重新比对磁盘 worker 文件 sha256 与 manifest 记录值（防安装后被替换）。

### 5.4 远程插件注册表 | Remote plugin registry

仿 `ai/remote_registry.rs`。一个远程 JSON 目录（托管 HF / 自有 CDN），列出可购买/可下载插件：

```jsonc
// 远程目录 exotic-plugins/index.json
{ "schema": 1,
  "plugins": [
    { "id": "exotic-image-psd", "name": "PSD 图像引擎", "version": "1.0.0",
      "media_kind": "image", "formats": ["psd","psb"],
      "license_tier": "paid", "price_hint": "¥XX", "store_url": "https://…",
      "size_bytes": 7340032,
      "package_url": "https://…/exotic-image-psd-1.0.0.zip",
      "package_sha256": "…",
      "icon_url": "https://…/psd.svg" }
  ] }
```

`exotic/registry.rs::discover()` 拉取并合并「远程可下载」与「本地已安装」状态，供前端「插件市场」展示。发现失败 → 离线回退为「仅本地已装」（同 `list_model_registry` 的 `online:false` 思路）。

### 5.5 未授权占位的判定与渲染 | "Needs purchase" placeholder

用户已选 **入库 + 「需购买」占位**。判定链路：

- 后端命令 `exotic_format_status()` 返回「格式 → 状态」映射：每个被某 manifest 认领的 `format` 标注 `installed` / `authorized` / `available`(可购买) 三态。
- 前端 `MediaThumb` 渲染时：若 `item.file_format` 命中冷门表且 `!authorized` → 叠加**锁角标 + 模糊占位**（可复用 `thumbhash` 若有，否则灰底 + 格式名），点击 → 引导至插件市场。
- 与「无插件可用」（B 类未识别格式被认领后但无插件）区分：前者「可购买」、后者「暂不支持」。

---

## 6. 数据模型 | Data Model（schema V9）

### 6.1 设计取舍 | Rationale

- **独立状态列**：新增 `media_items.exotic_status`，仿 `ai_status`（V2）/`face_status`（V8）。**不复用** `media_derivations`——那会把冷门并入派生框架（违背 G1）。
- **插件安装/授权落库**：插件清单是磁盘真相，但「已装/已授权」需快速查询 → 落一张 `exotic_plugins` 缓存表（启动时由 host 扫描 `plugins/` 重建，license 状态由 keyring 校验后写入）。
- **格式路由不落库**：`format → plugin` 路由表是内存结构（host 启动时由 manifests 构建），随插件增删动态变化，无需持久化。

### 6.2 DDL | schema V9

```sql
-- src-tauri/src/db/schema.rs 追加 SCHEMA_V9（迁移器 if version < 9 守护，仅执行一次）

-- ── media_items.exotic_status：冷门格式处理状态机（仿 ai_status / face_status）──────
-- 0=待处理 / 1=处理中 / 2=完成 / 3=错误 / 4=需授权(占位) / 5=无可用插件
-- 注：4/5 是「门控结果」而非处理进度；Producer 只领取 status=0 且格式已授权已装的项，
--    门控态由 host 在路由阶段回写，使前端可直接据此渲染占位（§5.5）。
ALTER TABLE media_items ADD COLUMN exotic_status INTEGER NOT NULL DEFAULT 0;
-- 仅索引未完成 & 非门控态（0/1/3），生产者扫描命中极小。
CREATE INDEX IF NOT EXISTS idx_media_exotic ON media_items(exotic_status)
                                            WHERE exotic_status IN (0,1,3);

-- ── exotic_plugins：已安装插件的缓存表（磁盘 manifest 的可查询投影）──────────────────
CREATE TABLE IF NOT EXISTS exotic_plugins (
    id           TEXT PRIMARY KEY,          -- manifest.id
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    media_kind   TEXT NOT NULL,             -- image|video|audio|document
    formats      TEXT NOT NULL,             -- JSON 数组 ["psd","psb"]
    capabilities TEXT NOT NULL,             -- JSON 数组 ["thumbnail","metadata"]
    license_tier TEXT NOT NULL,             -- free|paid
    authorized   INTEGER NOT NULL DEFAULT 0,-- license 校验通过 = 1（启动时刷新）
    commercial_ok INTEGER NOT NULL DEFAULT 0,
    installed_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ── 冷门子系统配置默认值（仿 face_* 开关）──────────────────────────────────────────
-- exotic_enabled：总开关；exotic_auto_process：是否随扫描自动处理（默认开，但仅作用于已授权格式）。
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('exotic_enabled',       '1'),
    ('exotic_auto_process',  '1'),
    ('exotic_max_workers',   '0');   -- 0=按 manifest.limits.max_concurrency 与机器核数自动
```

### 6.3 主缩略图路径的极小改动 | Minimal touch to the main thumbnail path

`thumbnail/generator.rs::decode_media_step_inner` 现在对 `image` 类型直接走 GPU/CPU 解码，对 `psd` 必然 `UnsupportedFormat`。改为：**在分派前先问 host「这个 format 是否冷门」**，是则不进主解码路径：

```rust
// generator.rs 分派处（§ 伪代码，实际接 host 的内存路由表，避免每项查 DB）
// 冷门格式不在主路径解码：既不报错、也不占主 rayon 池，交还给冷门 pipeline。
if exotic_host.claims_format(&item.file_format) {
    // 主路径对它「无操作」——它的缩略图由 exotic pipeline 异步产出后回填。
    // thumb_status 维持 0（未生成），exotic_status 驱动其真正处理/门控。
    return Ok(DecodeResult::Ready(ThumbResult {
        item_id, thumb_status: 0, thumb_path: None, thumbhash: None,
    }));
}
```

> 这是对主引擎**唯一**的侵入，且是「**让路**」而非「接管」——保持 G1：主引擎不知道 PSD 怎么解，只知道「这不归我管」。

---

## 7. 独立任务管理 | Independent Scheduling

### 7.1 流水线结构 | Pipeline（仿 `face_pipeline.rs`）

```
Producer ──► task channel ──► Dispatcher ──► WorkerPool(N 个长驻子进程) ──► result channel ──► Writer
  │                              │                  │                                          │
查 exotic_status=0          按 format 路由      每 worker：写 stdin 任务/读 stdout 结果        回填缩略图+元数据
且格式已授权已装            到对应插件的池      崩溃→重启+标该项 error                        mirror media_items + layout_cache
标 processing                                  超时→杀进程+标 error
```

- **Producer**：批量查 `exotic_status=0` 且 `file_format` 属「已装+已授权」插件的项（门控态 4/5 的项不会被查到，因为 host 路由时已回写）；标 `processing`（断点续传 + 孤儿恢复，复用 `reset_processing_*` 模式）。
- **Dispatcher**：按 `format → plugin` 路由，把任务投给对应插件的 worker 池。不同插件的 worker 互不干扰。
- **WorkerPool**：每插件维护 ≤ `manifest.limits.max_concurrency` 个长驻 worker 进程；进程崩溃自动重启，崩溃时正在处理的项标 `error`；任务超时（`task_timeout_ms`）杀进程重启。
- **Writer**：按 `media_kind` 把产物落地（§4.5），**复用** `derive` 的 `apply_thumb_results` + `update_thumb_result` 回填路径与 `db:media_enriched` 刷新事件。

### 7.2 状态机与续传 | State machine & resume

完全对齐 ai/face/derive 的成熟语义（`exotic_status`：0 待 /1 处理中 /2 完成 /3 错误）：

- **断点续传**：`exotic_process_active` 配置标志（仿 `ai_analysis_active`）跨重启持久化；App 启动 auto-resume。
- **孤儿恢复**：启动时 `exotic_status=1 → 0`（崩溃/强退遗留），仿 `reset_processing_ai_items`（`pipeline.rs:193`）。
- **门控态回写**：host 路由阶段把「无插件」项标 5、「需购买」项保持 0 但不入队（前端据 `exotic_format_status` 渲染占位，不靠 status 4 也可——4 仅作可选的持久化加速，见 §12 待决策）。

### 7.3 优先级阶梯的位置 | Where it sits in the priority ladder

现有阶梯（`state.rs:258`）：`scan > thumbnail > derivation > AI`。冷门引擎插入为：

```
scan > thumbnail > derivation > 【exotic】 > AI(CLIP/face)
```

理由：
- **低于 derivation**：常见格式的视频封面/文档缩略图（派生）比冷门格式更普遍、更该先出。
- **高于 AI**：冷门缩略图是「用户在等的可见产物」，优先级应高于纯后台的 CLIP/人脸分析。
- **独立令牌**：`AppState::exotic_analysis_token: Mutex<Option<CancellationToken>>`，可单独 start/pause/stop，**与 GPU 门闩无关**（worker 是 CPU 进程，不争显存，故**不**需要 `gpu_analysis_owner` 那套互斥）。
- **让步**：`should_yield_exotic()` = `is_scan_or_thumb_running() || is_derivation_running() || is_interactive()`。注意——worker 是独立进程，让步只需**暂停派发新任务**（Dispatcher 节流），在途 worker 自然跑完，比 rayon 让步更干净。

```rust
// state.rs 追加（仿 derivation_token / should_yield_derivation）
pub exotic_analysis_token: Mutex<Option<CancellationToken>>,

pub fn should_yield_exotic(&self) -> bool {
    self.is_scan_or_thumb_running() || self.is_derivation_running() || self.is_interactive()
}
```

### 7.4 性能调优旋钮 | Tuning knobs（独立、不影响主引擎）

独立子系统使每个旋钮都能单独拧，不波及常见格式（呼应用户「方便后期性能调优」）：

| 旋钮 | 来源 | 作用 |
|---|---|---|
| `exotic_max_workers` | app_config | 全局 worker 并发上限（0=自动） |
| `manifest.limits.max_concurrency` | 插件清单 | 单插件并发上限（重解码插件设低） |
| `manifest.limits.task_timeout_ms` | 插件清单 | 单任务超时（防死循环 worker） |
| `manifest.limits.mem_soft_limit_mb` | 插件清单 | worker 内存软上限（v2：经 job object / cgroup 强约束） |
| 进程优先级 | host 启动 worker 时设 | 低于前台（Windows `BELOW_NORMAL_PRIORITY_CLASS`） |

---

## 8. 扩展插口契约 | Extension Contract（G3 的正式定义）

「新增冷门格式，主程序零改动」需要一份**冻结的契约**。任何遵守它的插件都能被主程序发现、下载、调度：

### 8.1 主程序保证 | Host guarantees（稳定面）

1. **发现**：扫描 `{app_data}/plugins/*/manifest.json`，按 `manifest.schema` 解析；未来字段加在更高 `schema` 版本，旧主程序忽略未知字段。
2. **路由**：`manifest.formats` 中每个扩展名 → 该插件。冲突（两插件认领同格式）→ 取 license 已授权者，否则版本高者，并告警。
3. **调度**：对路由命中的项，按 `manifest.capabilities` 发对应 `WorkerRequest`，遵守 `manifest.limits`。
4. **生命周期**：负责启动/重启/超时杀/退出 worker；worker 只需实现协议、无需关心调度。
5. **产物落地**：按 `manifest.media_kind` 把 worker 产物落到对应表/缓存。

### 8.2 插件保证 | Plugin guarantees（实现面）

1. **manifest 合法**：字段齐全、签名有效、`bin` 覆盖目标平台。
2. **协议合规**：worker 启动后响应 `Hello`→`Ready{protocol_version}`，`protocol_version` 与主程序匹配；实现 `capabilities` 声明的每个 op。
3. **健壮**：单文件失败回 `Failed{message}` 而非崩溃（崩溃由主程序兜底但应避免）；不写主程序目录外的任何位置（除 stdout / 显式传入的临时路径）。
4. **自洽**：worker 自带其全部依赖（解码库静态链接或同目录 dll），不依赖主程序进程内状态。

### 8.3 版本协商 | Versioning

- `manifest.schema`：清单格式版本（主程序解析层兼容）。
- `manifest.protocol_version` / `WorkerResponse::Ready.protocol_version`：IPC 协议版本，握手时校验，**不匹配则拒绝启用并提示升级**。
- `manifest.min_host_version`：插件要求的最低主程序版本，低于则提示升级主程序。

> 三个版本号分别解耦「清单 / 协议 / 主程序」三条演进线，是 G3 长期可扩展的保险。

### 8.4 四类媒体的插件蓝图 | Plugin blueprints（验证契约普适性）

同一契约容纳四类，证明插口设计无类型偏置：

| 插件示例 | media_kind | formats | capabilities | worker 解码栈（建议） |
|---|---|---|---|---|
| **exotic-image-psd**（v1） | image | psd, psb | thumbnail, metadata | 纯 Rust `psd` crate + `image`(webp) |
| exotic-image-heic | image | heic, heif | thumbnail, metadata | libheif（同目录 dll）或系统 WIC 扩展 |
| exotic-video-rmvb | video | rmvb, rm | thumbnail, metadata | FFmpeg（worker 自带，与主程序解耦） |
