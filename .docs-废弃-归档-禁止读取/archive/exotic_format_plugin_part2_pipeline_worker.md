# 冷门格式插件子系统 · Part 2 引擎 | Pipeline & Worker（P3–P4）

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic v2 分卷,被 v3.1 同名分卷取代(见同目录 exotic_format_plugin_plan/)。

> 本卷范围：**worker IPC 协议 + PSD worker 实现 + 独立流水线 + 产物回填 + 调度令牌/让步**。
> 落完即可：开发模式下（`exotic_dev_mode=1` + 手放 worker），一张 `.psd` 端到端产出缩略图并在画廊显示。
> **本卷是"引擎本体"，不依赖 Part3 的 license/下载。**
>
> 配套：总纲 `exotic_format_plugin_plan_v2.md`（§4 架构、§8 形态）+ `exotic_format_plugin_part1_foundation.md`（已完成）。

---

## 本卷前置依赖 | Prerequisites

- **Part1 全部完成**（schema、host 路由、`claims_format` 让路、`exotic_dev_mode`）。
- 核验 `ai/face_pipeline.rs` 现状（4 线程 + `std::thread::scope` + `crossbeam_channel::bounded` + `CancellationToken`），本卷照此骨架。
- 核验 `derive/pipeline.rs:480 write_results`、`layout/cache.rs:115 apply_thumb_results`、`db/queries.rs:995 update_thumb_result`、事件 `db:media_enriched`——回填复用它们。

## 本卷完成定义 | DoD

1. `crates/exotic-protocol` 编译通过，主程序与 worker 同版本引用。
2. `psd-worker` 可执行：读 stdin 任务、解 PSD → 编 WebP → 回 stdout 二进制帧；单文件失败回 `Failed` 不崩溃。
3. `exotic/pipeline.rs` 4 线程跑通：Producer 查 pending → Dispatcher 路由 → WorkerPool（长驻进程）→ Writer 回填。
4. `exotic_dev_mode=1` + 手放 `plugins/exotic-image-psd/`（含 worker）→ 扫描含 .psd → **画廊出现 PSD 缩略图**（前端零改动）。
5. worker 崩溃→自动重启+标该项 error；任务超时→杀进程+标 error。
6. 让步：scan/thumbnail/derivation 运行或用户交互时，Dispatcher 暂停派发新任务（在途自然跑完）。

---

## Phase 3 — Worker 协议 + PSD worker | Protocol & PSD worker

### 3.1 目标
定义主程序↔worker 的稳定协议（独立 crate），实现首个 worker（PSD，T1 纯 Rust）。

### 3.2 协议 crate | `crates/exotic-protocol`（O5）

独立 crate，主程序与所有 worker 共享，避免协议漂移。**stdio + 长度前缀帧**：控制/元数据走 JSON，二进制产物（缩略图字节）走 4 字节小端长度前缀 + 原始字节（避免 base64 膨胀）。

```rust
// crates/exotic-protocol/src/lib.rs
//! 冷门 worker IPC 协议 | Exotic worker IPC protocol.
//! 主程序与 worker 必须同 PROTOCOL_VERSION。stdio 帧：JSON 行 / 二进制长度前缀帧。
use serde::{Serialize, Deserialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// 主程序 → worker 请求。 | Host → worker request.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkerRequest {
    /// 握手：worker 回 Ready{protocol_version, capabilities}，对不上即拒绝启用。
    Hello { host_version: String },
    /// 生成缩略图：返回 WebP 字节（图/视/文封面）。
    Thumbnail { item_id: i64, path: String, target_long_edge: u32 },
    /// 提取元数据：返回结构化 JSON（音频标签/文档页数/视频时长等）。
    Metadata { item_id: i64, path: String },
    /// 优雅退出。
    Shutdown,
}

/// worker → 主程序应答。 | Worker → host response.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerResponse {
    Ready { protocol_version: u32, capabilities: Vec<String> },
    /// 缩略图就绪：JSON 头给字节长度与可选 thumbhash，随后跟二进制帧。
    ThumbnailReady { item_id: i64, byte_len: u32, thumbhash: Option<Vec<u8>> },
    /// 元数据就绪。
    MetadataReady { item_id: i64, fields: serde_json::Value },
    /// 单项失败（不致命，主程序标 exotic_status=3 继续）。
    Failed { item_id: i64, message: String },
}

/// 帧编解码助手（主程序与 worker 共用，保证两端一致）。
pub mod frame {
    use std::io::{Read, Write, Result};
    /// 写一条 JSON 控制帧（换行分隔）。
    pub fn write_json<T: serde::Serialize, W: Write>(w: &mut W, v: &T) -> Result<()> { todo!() }
    /// 读一条 JSON 控制帧。
    pub fn read_json<T: serde::de::DeserializeOwned, R: Read>(r: &mut R) -> Result<T> { todo!() }
    /// 写二进制帧（4 字节小端长度 + 字节）。
    pub fn write_blob<W: Write>(w: &mut W, bytes: &[u8]) -> Result<()> { todo!() }
    /// 读二进制帧。
    pub fn read_blob<R: Read>(r: &mut R, len: u32) -> Result<Vec<u8>> { todo!() }
}
```

> **协议三铁律**：① worker 启动必先回 `Ready`，`protocol_version` 不匹配主程序拒绝启用（R7）；② 单文件失败回 `Failed` 而非崩溃；③ worker 只写 stdout/显式传入的临时路径，不碰主程序目录外任何位置。

### 3.3 PSD worker | `crates/exotic-workers/psd-worker`

T1 档：纯 Rust，静态链接，许可最干净（§7.2）。

```rust
// crates/exotic-workers/psd-worker/src/main.rs
//! PSD worker：解码 PSD 合成图 → 缩放 → 编码 WebP。纯 Rust（psd + image），T1 档。
//! 长驻进程：循环从 stdin 读 WorkerRequest，处理后向 stdout 写 WorkerResponse(+二进制帧)。
use exotic_protocol::{WorkerRequest, WorkerResponse, PROTOCOL_VERSION, frame};

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut r = stdin.lock();
    let mut w = stdout.lock();
    loop {
        let req: WorkerRequest = match frame::read_json(&mut r) { Ok(v) => v, Err(_) => break };
        match req {
            WorkerRequest::Hello { .. } => {
                let _ = frame::write_json(&mut w, &WorkerResponse::Ready {
                    protocol_version: PROTOCOL_VERSION,
                    capabilities: vec!["thumbnail".into(), "metadata".into()],
                });
            }
            WorkerRequest::Thumbnail { item_id, path, target_long_edge } => {
                match render_psd_thumb(&path, target_long_edge) {
                    // 成功：先写 JSON 头（含 byte_len），紧跟二进制帧。
                    Ok(webp) => {
                        let _ = frame::write_json(&mut w, &WorkerResponse::ThumbnailReady {
                            item_id, byte_len: webp.len() as u32, thumbhash: None,
                        });
                        let _ = frame::write_blob(&mut w, &webp);
                    }
                    Err(e) => { let _ = frame::write_json(&mut w, &WorkerResponse::Failed {
                        item_id, message: e.to_string() }); }
                }
            }
            WorkerRequest::Metadata { item_id, path } => { /* psd 尺寸/图层数 → MetadataReady */ }
            WorkerRequest::Shutdown => break,
        }
        use std::io::Write; let _ = w.flush();
    }
}

/// 解 PSD → RGBA → 缩放到 target_long_edge → 编码 WebP 字节。
/// PSD 通常内嵌合成图（merged image data），优先取它，避免重算图层合成（省内存，R10）。
fn render_psd_thumb(path: &str, target_long_edge: u32) -> Result<Vec<u8>, anyhow::Error> {
    // 1) psd crate 读文件 → 取合成 RGBA（Psd::from_bytes → rgba()）
    // 2) image::imageops 缩放（保持比例，长边=target_long_edge，三角/Lanczos）
    // 3) 编码 WebP（image 的 webp 编码或 webp crate）→ 返回字节
    todo!()
}
```

依赖（worker 自己的 Cargo.toml，与主程序解耦）：`psd`（MIT）、`image`（webp 编码）、`anyhow`、`exotic-protocol`。

> **缩略图档位对齐**：`target_long_edge` 由主程序传入，应吸附主缩略图档位（见 [[gotcha-ai-analysis-cpu-decode]] 档位 [120/240/480/960]，默认 480），使产物落 `thumb cache` 同档目录、`MediaThumb` 无缝加载。

### 3.4 开发期手放 worker（验证用）
`exotic_dev_mode=1` 时，把构建好的 `psd-worker` 放 `{app_data}/plugins/exotic-image-psd/bin/<triple>/`，配 §Part1-2.2 的 manifest。host.refresh 发现它、门控判 Authorized（dev_mode），即可端到端联调，**无需 Part3 的签名/下载**。

### 3.5 验收
- 单独跑 worker：手喂一行 `{"op":"hello","host_version":"0.1.0"}` → 回 `Ready`。
- 喂 `{"op":"thumbnail","item_id":1,"path":"<某.psd>","target_long_edge":480}` → 回 `ThumbnailReady` + WebP 字节，落盘可用看图器打开。
- 畸形 psd → 回 `Failed`，进程不退出（继续等下一任务）。

## Phase 4 — 独立流水线 + 回填 + 调度 | Pipeline, sink, scheduling

### 4.1 目标
`exotic/pipeline.rs` 仿 `ai/face_pipeline.rs` 起 4 线程，把"pending 项 → worker → 缩略图回填"串起来；加 `exotic_analysis_token` + `should_yield_exotic`。

### 4.2 流水线结构 | Pipeline（仿 face，消费端换 worker 进程池）

```
Producer ─► task channel ─► Dispatcher ─► WorkerPool(N 长驻子进程) ─► result channel ─► Writer
  │                            │                │                                         │
查 exotic_status=0 且        按 format 路由   每 worker：写 stdin 任务/读 stdout 结果      回填缩略图+元数据
format 已授权已装            到对应插件池     崩溃→重启+标该项 error；超时→杀进程+error    mirror media_items + layout_cache
标 processing(1)
```

四线程职责（用 `std::thread::scope`，**勿用 rayon::scope**——长生命周期阻塞在 channel 会拖慢解码，见 [[gotcha-background-yield-model]]）：

- **Producer**：批量查 `exotic_status=0` 且 `host.is_processable(format)` 的项；标 `processing(1)`（断点续传 + 孤儿恢复）。让步点：派发前 `if state.should_yield_exotic() { 节流/暂停 }`。
- **Dispatcher**：按 `format→plugin` 路由，投给对应插件的 worker 池。不同插件 worker 互不干扰。
- **WorkerPool**：每插件 ≤ `manifest.limits.max_concurrency` 个长驻 worker；崩溃自动重启（重启时在处理项标 error）；超时（`task_timeout_ms`）杀进程重启。
- **Writer**：按 `media_kind` 落地（§4.3），**复用** derive 回填路径。

```rust
// src-tauri/src/exotic/pipeline.rs（骨架，仿 ai/face_pipeline.rs:112 start_face_pipeline）
//! 冷门独立流水线 | Exotic pipeline：Producer→Dispatcher→WorkerPool→Writer。
//! 仿 ai/face_pipeline.rs，但消费端是 worker 子进程池（非 GPU 模型）。

pub fn start_exotic_pipeline(state: std::sync::Arc<crate::state::AppState>,
                             token: tokio_util::sync::CancellationToken) {
    tokio::spawn(async move {
        let token_outer = token.clone();
        let state_c = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            run_exotic_pipeline_blocking(&state_c, &token)
        }).await;
        // 自然完成（未取消）时清理令牌（仿 face）。
        if !token_outer.is_cancelled() { state.clear_exotic_analysis_token(); }
    });
}

fn run_exotic_pipeline_blocking(state: &crate::state::AppState,
                                token: &tokio_util::sync::CancellationToken) {
    let (task_tx, task_rx) = crossbeam_channel::bounded::<ExoticTask>(CHANNEL_CAPACITY);
    let (res_tx, res_rx)   = crossbeam_channel::bounded::<ExoticResult>(CHANNEL_CAPACITY);
    std::thread::scope(|s| {
        s.spawn(|| produce_exotic_tasks(state, task_tx, token));       // Producer
        s.spawn(|| dispatch_to_workers(state, task_rx, res_tx, token));// Dispatcher + WorkerPool
        s.spawn(|| write_exotic_results(state, res_rx, token));        // Writer
    });
}
```

### 4.3 WorkerPool：长驻进程管理 | `exotic/worker.rs`

```rust
// src-tauri/src/exotic/worker.rs
//! 单个 worker 子进程的句柄：spawn、握手、发任务、收结果、超时/崩溃处理。
pub struct WorkerHandle {
    child: std::process::Child,      // BELOW_NORMAL 优先级启动（§调优）
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    plugin_id: String,
}

impl WorkerHandle {
    /// 启动 worker：std::process::Command 绝对路径（D6）+ 启动前 sha256 再校验（Part3 R2）
    /// + 设低优先级（Win BELOW_NORMAL_PRIORITY_CLASS）+ Hello/Ready 握手校验 protocol_version。
    pub fn spawn(manifest: &crate::exotic::manifest::Manifest, bin_abs: &std::path::Path)
        -> Result<Self, crate::error::AppError> { todo!() }

    /// 发一个任务并阻塞等结果（含二进制帧读取）。超时由调用方用 task_timeout_ms 包裹。
    pub fn run_task(&mut self, req: &exotic_protocol::WorkerRequest)
        -> Result<exotic_protocol::WorkerResponse, crate::error::AppError> { todo!() }

    pub fn shutdown(mut self) { /* 发 Shutdown，等退出，超时则 kill */ }
}
```

> **超时实现**：`run_task` 用一个监控线程 + `child.kill()`，或平台定时；超时 = 杀进程、该项标 error、池补一个新 worker。**绝不让一个死循环 worker 卡死整池。**

### 4.4 产物落地 Sink（复用 derive 回填）| Output sink

| media_kind | 主产物 op | 落地（复用现有机制） |
|---|---|---|
| image（psd…） | thumbnail | WebP → `thumb_path(cache_dir, size, cache_key)` 写盘 → `update_thumb_result`（queries.rs:995）mirror `media_items.thumb_status=1/thumb_path/thumbhash` → `apply_thumb_results`（layout/cache.rs:115）O(1) 同步 → emit `db:media_enriched` |
| video（rmvb…） | thumbnail(封面) + metadata | 封面同上；元数据 → `video_meta` |
| audio | thumbnail(内嵌封面) + metadata | 封面同上；元数据 → `audio_meta` |
| document | thumbnail(首页) + metadata + text | 缩略图同上；元数据 → `document_meta` |

> **v1 切片只实现 image + thumbnail**。Writer 按 `media_kind` 表驱动分派，新增 kind 不改调度核心。Writer 拿到 worker 回传的 WebP 字节后，**直接复用** `cache_key`（由 item 的 rel_path/file_name/mtime 算，与主缩略图同键）写入同档缓存目录，使画廊零改动显示。

```rust
// Writer 关键：成功项的缩略图落地（复用主缩略图键/路径/回填）
// 1) cache_key = 既有 hash::cache_key(rel_path, file_name, mtime)（与主路径同算法）
// 2) path = thumbnail::cache::thumb_path(cache_dir, size, cache_key)；写 webp 字节
// 3) db: queries::update_thumb_result(item_id, thumb_status=1, thumb_db_path, thumbhash)
//    同时 exotic_status=2
// 4) layout::cache::apply_thumb_results(&cache, &[ThumbResult{..}])  O(1) 原位同步
// 5) emit "db:media_enriched"  → MediaGrid 2s 防抖重排刷新
```

### 4.5 调度令牌 + 让步 | Token & yield（state.rs）

```rust
// state.rs 追加（仿 ai/face token；worker 是 CPU 进程 → 不碰 gpu_analysis_owner）
pub exotic_analysis_token: std::sync::Mutex<Option<tokio_util::sync::CancellationToken>>,

pub fn new_exotic_analysis_token(&self) -> CancellationToken { /* 仿 new_face_analysis_token */ }
pub fn clear_exotic_analysis_token(&self) { /* 仿 clear_face */ }

/// 让步：scan/缩略图/派生运行中或用户交互时让路。
/// 注意：worker 独立进程，让步只需 Dispatcher 暂停派发新任务（在途自然跑完），比 rayon 让步更干净。
pub fn should_yield_exotic(&self) -> bool {
    self.is_scan_or_thumb_running() || self.is_derivation_running() || self.is_interactive()
}
```

优先级阶梯插入位置（现 `scan > thumbnail > derivation > AI`）：

```
scan > thumbnail > derivation > 【exotic】 > AI(CLIP/face)
```

理由：低于 derivation（常见格式派生更普遍该先出）；高于 AI（冷门缩略图是用户在等的可见产物，优先于纯后台 CLIP/人脸）。**独立令牌**可单独 start/pause/stop；**与 GPU 门闩无关**（R5：worker 不争显存）。

### 4.6 续传与孤儿恢复 | Resume & orphan recovery
- 断点续传：`exotic_process_active` 配置标志（仿 `ai_analysis_active`）跨重启持久化，App 启动 auto-resume。
- 孤儿恢复：启动时 `exotic_status=1 → 0`（崩溃/强退遗留），仿 `reset_processing_ai_items`（DAO 新增 `reset_processing_exotic_items`）。
- DAO（`db/queries.rs` 新增，镜像 ai_*）：`get_pending_exotic_items` / `count_pending_exotic_items` / `batch_update_exotic_status` / `reset_processing_exotic_items` / `update_exotic_thumb_result`。

### 4.7 命令：启动/暂停/停止 | `ipc/exotic_commands.rs` 追加
仿 `ipc/ai_commands.rs` 的 start/pause/stop（但**无 GPU acquire**）：`start_exotic_processing` / `pause_exotic_processing` / `stop_exotic_processing` / `get_exotic_status`（进度=已处理/总 pending）。App 启动 auto-resume（`exotic_enabled=1 && exotic_auto_process=1` 时）。

### 4.8 验收
- `exotic_dev_mode=1` + 手放 psd-worker + 扫描含 .psd → 数秒后**画廊出现 PSD 缩略图**；DB `exotic_status=2`、`thumb_status=1`。
- 浏览/滚动（触发 `note_interaction`）时 Dispatcher 暂停派发；停手后恢复。
- kill 掉 worker 进程 → 池自动重启、当前项标 error(3)、不影响其余项。
- 与常见格式导入并行时，jpg/mp4 处理速度**无可感知下降**（G2）。

---

## 新会话续作提示词 | Continuation prompt（Part 2）

```
任务：实施 plan-docs/exotic_format_plugin_part2_pipeline_worker.md（冷门子系统·引擎 P3–P4）。
先读：总纲 v2（§4/§8）+ Part1（确认已完成）+ 本卷全文。
前置依赖：Part1 全部完成（schema/host 路由/claims_format 让路/exotic_dev_mode）。
必看源码（照其骨架，勿另起炉灶）：
  - src-tauri/src/ai/face_pipeline.rs（4 线程 thread::scope + crossbeam + CancellationToken 范式）
  - src-tauri/src/derive/pipeline.rs:480 write_results（回填范式）
  - src-tauri/src/layout/cache.rs:115 apply_thumb_results
  - src-tauri/src/db/queries.rs:995 update_thumb_result（+ 镜像 ai_* DAO）
  - src-tauri/src/thumbnail/cache.rs（thumb_path/thumb_db_path，缩略图键）
  - src-tauri/src/state.rs（face_analysis_token / should_yield_derivation 范式）
  - src-tauri/src/ipc/ai_commands.rs（start/pause/stop 命令范式，但本卷无 GPU acquire）
施工顺序：P3 协议 crate → PSD worker（先单测 stdin/stdout）→ P4 pipeline 4 线程 → 回填 → 令牌/让步 → 命令。
DoD（本卷末）：dev_mode+手放 worker → 画廊出 PSD 缩略图；崩溃/超时恢复；让步生效；常见格式不降速。
约定：thiserror / rusqlite 参数绑定 / 中英双语注释 / 中文 commit / 改动及时 commit / 大文件分多次 Edit。
注意：worker 是 CPU 进程，绝不接 gpu_analysis_owner；让步只暂停派发新任务（在途跑完）。
完成后：回到 Part3（exotic_format_plugin_part3_license_distribution.md）。
```

## 本卷产出清单 | Deliverables checklist

- [ ] `crates/exotic-protocol`（WorkerRequest/Response + frame 编解码 + PROTOCOL_VERSION）
- [ ] `crates/exotic-workers/psd-worker`（长驻循环 + render_psd_thumb：psd→缩放→webp）
- [ ] `exotic/pipeline.rs`（4 线程 thread::scope：Producer/Dispatcher/WorkerPool/Writer）
- [ ] `exotic/worker.rs`（WorkerHandle：spawn/握手/run_task/超时/崩溃重启）
- [ ] Writer 回填（复用 cache_key + thumb_path + update_thumb_result + apply_thumb_results + media_enriched）
- [ ] `state.rs`：`exotic_analysis_token` + `should_yield_exotic` + 阶梯插入
- [ ] `db/queries.rs`：5 个镜像 ai_* 的 exotic DAO
- [ ] `ipc/exotic_commands.rs`：start/pause/stop/get_exotic_status + auto-resume
- [ ] 验收：dev_mode 端到端出 PSD 缩略图、容错、让步、不降速

