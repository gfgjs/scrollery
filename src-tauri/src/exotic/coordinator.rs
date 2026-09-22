// src-tauri/src/exotic/coordinator.rs
//! 冷门格式插件 · Coordinator（v3 Part2 §4.1）。
//!
//! 单一调度器：接收扫描/安装/激活/配置/重试时钟等事件，**幂等**唤醒**唯一**一条 Pipeline。
//!
//! 设计：
//!   - 原子语义位 + 单槽通知：合并 wake 时保留用户请求与版本对账语义。
//!   - 串行循环：唯一 owner，commands 只发 wake → 天然「两个并发 start 只启动一条 Pipeline」。
//!   - 尾部竞态：运行期间新增任务保留 wake，结束后由下一次通知重新评估。
//!   - 重试时钟：独立 interval 周期发 `RetryDue`，使到期 retryable 任务被重新评估。
//!   - 门控（[`evaluate_run`]）：enabled/未暂停/可领取(授权+平台+能力)/有就绪任务/Worker 可用 → 才跑。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::db::queries as q;
use crate::exotic::catalog::Capability;
use crate::exotic::pipeline::{
    recover_stale_exotic_leases, run_exotic_pipeline_blocking, PipelineDeps, SupervisorFactory,
    VideoThumbnailFactory, WorkerFactory,
};
use crate::exotic::worker::{WorkerConfig, WorkerSpec};
use crate::exotic::ExoticHost;
use crate::state::AppState;

/// 首发唯一插件 + 能力（Part2）。
pub const PSD_PLUGIN_ID: &str = "exotic-image-psd";
/// RAW 图像缩略图解码插件（builtin 叠加豁免，见 catalog.rs CommonFormatConflict 例外）。
pub const RAW_PLUGIN_ID: &str = "exotic-image-raw";
/// 视频格式扩展插件（builtin+free，design.md §4.1）。**Service 型 worker**：remux/转码/
/// 缩略图均由 host 侧 `VideoWorkerService` 直持 supervisor（同 OCR/Enhance 先例，§2.1）。
///
/// D-444③ A4 裁决(单轨收敛):rmvb/vob 已入 `utils::format` 注册表(classify=video),其缩略图
/// 走**常规 video 派生链**(video_cover/video_keyframes → `backend_for` → V5 `WorkerVideoBackend`
/// ffmpeg 桥),不经 exotic 任务化——`fast_scan::seed_gate_admits` 掐掉 builtin+video 的 exotic
/// 播种,避免与派生链双写 `thumb_path`。故 video-extended 的 exotic 缩略图任务实际恒零。
///
/// 尽管如此,video-extended 仍以 **Thumbnail 能力**进 `plugin_descriptors`、并保留
/// [`VideoThumbnailFactory`]（派发前先 `VideoSessionInit` 建 ffmpeg 会话）——作为 offering
/// 声明 thumbnail 能力的**一致性面可达实现**(路径可达可测;若播种侧未来放开即刻生效),
/// 非当前缩略图的实际产出方。
pub const VIDEO_PLUGIN_ID: &str = "video-extended";
/// video-worker 握手期望的 ReadyBody.worker_id（design.md §2.2）。
pub const VIDEO_WORKER_ID: &str = "video-worker";
// T13 后调度路径全走注册表(descriptor),此常量仅测试作 shorthand。
#[cfg(test)]
const CAPABILITY: Capability = Capability::Thumbnail;

/// per-op 请求超时表(Part4 D3 §5/T13:常量集中一处,Supervisor 请求执行按 op 取值)。
/// 起步值,实测再调。SESSION_INIT/SESSION_CLOSE/EMBED_BATCH/ENCODE_TEXT 由
/// `ai::worker_client`(T17 派发)消费。
pub(crate) mod op_timeouts {
    use std::time::Duration;
    /// thumbnail 单请求(原 pipeline::TASK_TIMEOUT 同值收拢至此)。
    pub const THUMBNAIL: Duration = Duration::from_secs(30);
    /// SessionInit **静默限时**(2026-07-11 加固批 A-2 语义变更:worker 装载期发
    /// Progress 阶段帧+10s 心跳,收帧即重置本计时;总上界另见 worker.rs
    /// PROGRESS_TOTAL_CAP)。旧值 300s 是「猜冷加载总时长」——与 worker 单段 600s
    /// 后备预算倒挂,宿主恒先杀。现在 90s = 九拍心跳全丢才判死,只量「进程还活着吗」
    /// 而非「装载要多久」;进程握手仍 5s 不动——模型加载不在握手(D3 §2)。
    pub const SESSION_INIT: Duration = Duration::from_secs(90);
    /// SessionClose:健康 worker 卸载即 drop(毫秒级);上界只兜「驱动释放 VRAM 慢」,
    /// 超时即 kill 回收,会话随进程消亡(T17)。
    pub const SESSION_CLOSE: Duration = Duration::from_secs(30);
    /// EmbedBatch 一批(D3 §5 起步值)。
    pub const EMBED_BATCH: Duration = Duration::from_secs(120);
    /// FaceDetectEmbed 基础超时(2026-07-03 GUI 实测修订:原与 EmbedBatch 同档固定
    /// 120s,64 张全尺寸原图批在 dev 构建下必然超时 → supervisor 误杀正常 worker,
    /// 重试同批再超时 → 硬止损终止整轮)。
    pub const FACE_DETECT_EMBED_BASE: Duration = Duration::from_secs(60);
    /// FaceDetectEmbed 单项增量:人脸源可为全尺寸原图(解码+letterbox 秒级,慢盘/dev
    /// 构建更甚),超时按批内项数线性放宽。这是「假死检测器」而非性能指标——用户取消
    /// 走 cancelled 回调即时生效,不受本值影响,宁可宽松。
    pub const FACE_DETECT_EMBED_PER_ITEM: Duration = Duration::from_secs(6);
    /// FaceDetectEmbed 一批的实际超时 = 基础 + 单项增量 × 项数。
    pub fn face_detect_embed(items: usize) -> Duration {
        FACE_DETECT_EMBED_BASE + FACE_DETECT_EMBED_PER_ITEM * (items as u32)
    }
    /// EncodeText 一批(文本塔恒 CPU、查询通常单条,轻;30s 已是慢盘冷启余量)。
    pub const ENCODE_TEXT: Duration = Duration::from_secs(30);
    /// OcrSessionInit(T6/D-OCR-2:三模型 CPU EP,worker 同步装载**不发 Progress 心跳**,
    /// 故不能按 SESSION_INIT 的「静默心跳」量纲缩到 90s——须容 server 档冷载全程;
    /// host 侧总兜底,越界即 kill 回收)。
    /// ⚠180s flat 系保守估计,无实测依据——ocr_bench 落位后回填冷载耗时;慢机 server 档
    /// (~190MB)若两 attempt 均撞冷载墙,按档位分预算(mobile/server 各钉各的)。
    pub const OCR_SESSION_INIT: Duration = Duration::from_secs(180);
    /// OcrBatch 一批:基础 60s + 单项 30s(交互恒单图;批口面向未来,det+cls+rec 全链
    /// CPU、大图经 limit_side_len 降采样后秒级,宽松即可——「假死检测器」非性能指标)。
    pub fn ocr_batch(items: usize) -> Duration {
        Duration::from_secs(60) + Duration::from_secs(30) * (items as u32)
    }
    /// EnhanceSessionInit **静默限时**(降噪/超分子系统 design.md §E):enhance-worker 装载期
    /// 发 Progress 阶段帧 + 10s 心跳(main.rs HEARTBEAT_INTERVAL),收帧即重置本计时——与
    /// CLIP `SESSION_INIT`(90s)同「静默心跳」量纲。增强模型仅数十 MB、加载秒级,90s 足够;
    /// 九拍心跳全丢才判死,只量「进程还活着吗」。
    pub const ENHANCE_SESSION_INIT: Duration = Duration::from_secs(90);
    /// EnhanceRun 的 per-tile **静默限时**(design.md §E):worker 每完成一个 tile 发 Progress,
    /// host 收帧即重置本计时。300s 容 CPU 兜底单 tile 慢跑(fp32、512² tile,慢机分钟级不误杀);
    /// ⚠ 数值系草案,dev 机 bench(单 tile 吞吐)落位后回填,与 design.md §E「未实测不给数字」一致。
    pub const ENHANCE_SILENCE: Duration = Duration::from_secs(300);
}

/// 单插件运行描述(Part6 §3.3 C1/T13):调度循环按注册表逐项评估与运行,不再写死 PSD。
pub(crate) struct PluginDescriptor {
    pub plugin_id: String,
    /// 握手校验的 ReadyBody.worker_id 期望值。
    pub worker_id: String,
    /// 逐能力评估/运行(exotic_tasks 队列按 capability 列分流)。
    pub capabilities: Vec<Capability>,
    /// 进程握手超时(per-plugin;模型加载不在握手,ai/face 也保持 5s,D3)。
    pub handshake_timeout: Duration,
    /// 是否占 GPU 令牌(AppState.gpu_token):psd=false;ai/face descriptor 随 T15
    /// 加入时=true(发批前先 CPU permit 后 GPU 令牌,D2 顺序天条)。
    pub uses_gpu: bool,
}

/// 运行注册表(Part6 §3.3):capabilities/worker_id/uses_gpu 全部取自 Catalog 的显式声明
/// (单一事实来源;P17 起无 Rust 侧硬编码 fallback)。新插件加入 = 在 Catalog 中声明
/// worker_id/uses_gpu，调度代码无需再改。
fn plugin_descriptors(snap: &crate::exotic::catalog::CatalogSnapshot) -> Vec<PluginDescriptor> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (_, off) in snap.iter_formats() {
        if !seen.insert(off.plugin_id.clone()) {
            continue;
        }
        let Some(worker_id) = off.worker_id.clone() else {
            // 未声明 worker_id 的 offering 不进入 exotic 调度:OCR/Enhance 等独立服务由 host 侧
            // 直持 supervisor,本就不走任务队列——属正常形态,非坏数据。
            continue;
        };
        out.push(PluginDescriptor {
            plugin_id: off.plugin_id.clone(),
            worker_id,
            capabilities: off.capabilities.clone(),
            handshake_timeout: HANDSHAKE_TIMEOUT,
            uses_gpu: off.uses_gpu,
        });
    }
    out.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    out
}

const WAKE_PENDING: u8 = 1;
const WAKE_USER: u8 = 2;
const WAKE_RECONCILE: u8 = 4;
/// 启动失败或熔断后暂停自动尝试，安装/用户请求可以提前复位。
const PLUGIN_COOLDOWN: Duration = Duration::from_secs(60);
/// 重试时钟周期。
const RETRY_TICK: Duration = Duration::from_secs(30);
/// 握手超时。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// 唤醒原因。多数仅 informational（调度按统一 `wake` 处理）；但 [`WakeReason::UserRequested`]
/// 额外携带「用户显式请求」语义——`exotic_auto_process=false` 时只有它能触发运行（绕过 auto 门控）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    Startup,
    ScanCommitted,
    CatalogBackfill,
    PluginInstalled,
    LicenseActivated,
    ConfigChanged,
    RetryDue,
    /// 用户显式要求处理（start_exotic_processing / retry 命令）。绕过 `exotic_auto_process` 门控。
    UserRequested,
}

/// Coordinator 句柄。`wake()` 只通知；实际调度在后台循环串行进行。
pub struct ExoticCoordinator {
    tx: mpsc::Sender<()>,
    pending: Arc<AtomicU8>,
}

impl ExoticCoordinator {
    /// 启动单一调度循环及重试时钟。
    pub fn start(app: AppHandle, state: Arc<AppState>, host: Arc<ExoticHost>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(1);
        let pending = Arc::new(AtomicU8::new(0));
        let handle = Arc::new(Self {
            tx,
            pending: Arc::clone(&pending),
        });
        tauri::async_runtime::spawn(run_loop(app, state, host, rx, pending));
        let timer = Arc::clone(&handle);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(RETRY_TICK);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if timer.tx.is_closed() {
                    break;
                }
                timer.wake(WakeReason::RetryDue);
            }
        });
        handle
    }

    /// 先保存语义，再发送通知；通知槽已满时语义仍保留到下一轮原子取走。
    pub fn wake(&self, reason: WakeReason) {
        let flags = WAKE_PENDING
            | if reason == WakeReason::UserRequested {
                WAKE_USER
            } else {
                0
            }
            | if needs_reconcile(reason) {
                WAKE_RECONCILE
            } else {
                0
            };
        self.pending.fetch_or(flags, Ordering::SeqCst);
        let _ = self.tx.try_send(());
    }
}

/// 每次通知只运行一轮。运行期间新增任务的 wake 保留在 pending，不依赖无进展重跑。
async fn run_loop(
    app: AppHandle,
    state: Arc<AppState>,
    host: Arc<ExoticHost>,
    mut rx: mpsc::Receiver<()>,
    pending: Arc<AtomicU8>,
) {
    let mut cooldowns = HashMap::new();
    while rx.recv().await.is_some() {
        let flags = pending.swap(0, Ordering::SeqCst);
        if flags == 0 {
            continue;
        }
        maybe_run_until_drained(
            &app,
            &state,
            &host,
            flags & WAKE_USER != 0,
            flags & WAKE_RECONCILE != 0,
            &mut cooldowns,
        )
        .await;
    }
}

/// 是否携带「worker 版本对账」语义(病历 #4,2026-07-05 真机):版本失效
/// (pipeline 步骤 2)只在 pipeline 启动后执行,而启动条件 has_ready 只认 pending/
/// retryable——纯升级/回滚后任务全是 done,失效永不可达,换了解码器旧缩略图仍陈旧。
/// PluginInstalled(安装/升级/回滚)与 Startup(装机换 app 构建、错过对账的兜底)
/// 额外允许 pipeline 免 has_ready 起一轮:探针拿到 worker_version 后步骤 2 对账,
/// 版本没变则本轮零领取即结束(代价 = 一次 worker spawn,毫秒级)。
fn needs_reconcile(reason: WakeReason) -> bool {
    // UserRequested 也入集:auto 关闭的用户升级后点「开始」是其唯一触发口,不对账则同卡死。
    // RetryDue/ScanCommitted 等周期/自动 wake 刻意排除——否则每 30s 时钟都白 spawn 一次探针。
    matches!(
        reason,
        WakeReason::PluginInstalled | WakeReason::Startup | WakeReason::UserRequested
    )
}

/// 按稳定顺序评估各插件；每个插件本轮至多一次 Pipeline，期间新增 wake 留给下一轮。
async fn maybe_run_until_drained(
    app: &AppHandle,
    state: &Arc<AppState>,
    host: &Arc<ExoticHost>,
    bypass_auto: bool,
    force_reconcile: bool,
    cooldowns: &mut HashMap<String, Instant>,
) {
    // 病历 #2(2026-07-05):wake 评估前先清扫过期租约。恢复不能只挂在 pipeline 步骤 0——
    // 「是否启动 pipeline」恰由 has_ready(只认 pending/retryable)决定,硬杀遗留的 stale
    // processing 行会让二者互锁,进度永久停摆(详见 pipeline::recover_stale_exotic_leases)。
    // 运行中跳过:本轮启动时已清扫,活租约由 renew_loop 维持。有恢复即广播,前端进度即时刷新。
    if !state.is_exotic_running() {
        let recovery_state = Arc::clone(state);
        let recovered = tokio::task::spawn_blocking(move || {
            recover_stale_exotic_leases(&recovery_state.db_writer)
        })
        .await
        .unwrap_or(0);
        if recovered > 0 {
            let _ = app.emit("exotic:status-changed", ());
        }
    }
    for desc in plugin_descriptors(&state.exotic_catalog.snapshot()) {
        for &capability in &desc.capabilities {
            run_capability_until_drained(
                app,
                state,
                host,
                &desc,
                capability,
                bypass_auto,
                force_reconcile,
                cooldowns,
            )
            .await;
        }
    }
}

/// 单 (插件, 能力) 每轮至多启动一次；无进展返回协调器，避免重复探针。
#[allow(clippy::too_many_arguments)]
async fn run_capability_until_drained(
    app: &AppHandle,
    state: &Arc<AppState>,
    host: &Arc<ExoticHost>,
    desc: &PluginDescriptor,
    capability: Capability,
    bypass_auto: bool,
    force_reconcile: bool,
    cooldowns: &mut HashMap<String, Instant>,
) {
    if cooldown_active(cooldowns, &desc.plugin_id, force_reconcile, Instant::now()) {
        return;
    }
    // T15 接缝:目前仅 thumbnail 能力有 pipeline 实装;embedding/face_detect_embed 的
    // 批派发随推理核心迁移(T15)落地——届时在此按能力分派 EmbedWorker 管线,并按
    // desc.uses_gpu 走「先 CPU permit 后 GPU 令牌」双取(D2)。
    if capability != Capability::Thumbnail {
        debug!(
            "{} 能力 {} 的 pipeline 未实装(T15),跳过",
            desc.plugin_id,
            capability.as_str()
        );
        return;
    }

    // 先做便宜门控；只有真实任务/版本对账才读插件文件，阻塞操作全部在 blocking 内。
    let eval_state = Arc::clone(state);
    let eval_host = Arc::clone(host);
    let plugin_id = desc.plugin_id.clone();
    let worker_path = tokio::task::spawn_blocking(move || {
        let should = {
            let conn = eval_state
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            evaluate_run(
                &conn,
                &eval_state.config,
                &eval_host,
                &plugin_id,
                capability,
                true,
                bypass_auto,
                force_reconcile,
            )
        };
        if !should {
            return None;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let keys = crate::exotic::trusted_keyset().ok()?;
        let path = crate::exotic::installer::resolve_worker_path(
            &eval_state.exotic_install_dir(),
            &plugin_id,
            &keys,
            now,
        )?;
        if plugin_id == VIDEO_PLUGIN_ID
            && !matches!(
                crate::exotic::tools::ffmpeg_tool_status(&eval_state.app_data_dir),
                crate::exotic::tools::ToolStatus::Ready { .. }
            )
        {
            return None;
        }
        Some(path)
    })
    .await
    .ok()
    .flatten();
    let Some(exe_path) = worker_path else {
        return;
    };
    if state.is_exotic_running() {
        return;
    }

    // 启动唯一 Pipeline（token 入 AppState；stop 命令可取消）。
    info!(
        "启动 exotic Pipeline:{} cap={} uses_gpu={}",
        desc.plugin_id,
        capability.as_str(),
        desc.uses_gpu
    );
    let token = state.new_exotic_analysis_token();
    let _ = app.emit("exotic:status-changed", ());
    let app_run = app.clone();
    let state_run = Arc::clone(state);
    let exe_path = exe_path.clone();
    // §5.3 派发前授权复核：把 host 移入阻塞任务，供 Claimer 每批领取前调 is_task_runnable。
    let host_run = Arc::clone(host);
    let plugin_id = desc.plugin_id.clone();
    let worker_id = desc.worker_id.clone();
    let handshake_timeout = desc.handshake_timeout;

    let result = tokio::task::spawn_blocking(move || {
        let (cache_dir, requested_size) = {
            let cfg = state_run
                .thumb_config
                .read()
                .unwrap_or_else(|e| e.into_inner());
            (cfg.cache_dir.clone(), cfg.size)
        };
        let spec = WorkerSpec {
            exe_path,
            expected_worker_id: worker_id,
            required_capabilities: vec![capability.as_str().to_string()],
        };
        let cfg = WorkerConfig {
            handshake_timeout,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        };
        // video-extended:任务化 Thumbnail 无 session/ffmpeg 路径,用 VideoThumbnailFactory
        // 在派发前先建 ffmpeg 会话(D-444③)。ffmpeg 就绪已在 worker_available 门控,此处
        // 再取一次路径(idempotent 文件检查);竞态变不就绪则本轮不处理(任务留 pending)。
        let factory: Box<dyn WorkerFactory> = if plugin_id == VIDEO_PLUGIN_ID {
            match crate::exotic::tools::ffmpeg_tool_status(&state_run.app_data_dir) {
                crate::exotic::tools::ToolStatus::Ready { ffmpeg_exe, .. } => {
                    // 输出白名单前缀 = {cache_dir}/video(与 VideoWorkerService 同址,§5.2)。
                    let work_dir = cache_dir.join("video");
                    let _ = std::fs::create_dir_all(&work_dir);
                    let init = exotic_protocol::RequestBody::VideoSessionInit {
                        session_id: 1,
                        ffmpeg_exe_path: ffmpeg_exe.to_string_lossy().into_owned(),
                        ffmpeg_sha256: crate::exotic::tools::FFMPEG_EXE_SHA256.to_string(),
                        work_dir: work_dir.to_string_lossy().into_owned(),
                    };
                    Box::new(VideoThumbnailFactory { spec, cfg, init })
                }
                _ => return crate::exotic::pipeline::PipelineStats::default(),
            }
        } else {
            Box::new(SupervisorFactory { spec, cfg })
        };
        let app_evt = app_run.clone();
        let state_yield = Arc::clone(&state_run);
        let deps = PipelineDeps {
            writer: &state_run.db_writer,
            limiter: &state_run.background_heavy_limiter,
            token: &token,
            items_cache: &state_run.layout_items_cache,
            cache_dir,
            requested_size,
            plugin_id: plugin_id.clone(),
            on_progress: Arc::new(move || {
                // 合并发：画廊刷新（复用 enrichment 事件）+ 状态变化。
                let _ = app_evt.emit(
                    "db:media_enriched",
                    crate::scanner::enricher::MediaEnrichedPayload::refresh_signal(),
                );
                let _ = app_evt.emit("exotic:status-changed", ());
            }),
            should_yield: Arc::new(move || state_yield.should_yield_exotic()),
            is_runnable: Arc::new(move || host_run.is_task_runnable(&plugin_id, capability)),
        };
        run_exotic_pipeline_blocking(&deps, factory.as_ref())
    })
    .await;

    // 清理 token 槽（run 已结束）。
    state.cancel_exotic_analysis();

    match result {
        Ok(stats) => {
            info!(?stats, "exotic Pipeline 完成");
            let _ = app.emit("exotic:status-changed", ());
            if matches!(
                stats.stop,
                crate::exotic::pipeline::PipelineStop::Blocked
                    | crate::exotic::pipeline::PipelineStop::CircuitOpen
            ) {
                cooldowns.insert(desc.plugin_id.clone(), Instant::now() + PLUGIN_COOLDOWN);
            }
        }
        Err(e) => {
            warn!("exotic Pipeline 任务 panic：{e}");
            cooldowns.insert(desc.plugin_id.clone(), Instant::now() + PLUGIN_COOLDOWN);
        }
    }
}

fn cooldown_active(
    cooldowns: &mut HashMap<String, Instant>,
    plugin_id: &str,
    reset: bool,
    now: Instant,
) -> bool {
    if reset {
        cooldowns.remove(plugin_id);
    }
    cooldowns.get(plugin_id).is_some_and(|until| *until > now)
}

/// 门控判定（可单测）：是否应启动 Pipeline。
///
/// 顺序：Worker 可用 → 子系统启用 → 未暂停 → auto 门控 → 插件可领取(授权+平台+能力) → 有就绪任务。
/// `bypass_auto`：本批含用户显式请求（start/retry）时为 true，绕过 `exotic_auto_process` 门控；
/// 自动 wake（扫描/重试时钟/启动）为 false，`exotic_auto_process=false` 时不运行。
/// `force_run_once`(病历 #4):安装/升级/回滚/启动的版本对账——仅跳过末位 has_ready 检查
/// (其余门控全部照常),让 pipeline 起一轮做步骤 2 的 worker_version 失效比对。
/// A2:`config` 参数新增——`exotic_enabled`/`exotic_auto_process` 是 schema 设置类键,唯一
/// 真源已切到 config.toml;`exotic_paused` 仍是状态类键(用户点「暂停」的临时开关,非表单
/// 项),照旧走 `conn` 查 DB。
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_run(
    conn: &Connection,
    config: &crate::config::ConfigManager,
    host: &ExoticHost,
    plugin_id: &str,
    capability: Capability,
    worker_available: bool,
    bypass_auto: bool,
    force_run_once: bool,
) -> bool {
    if !worker_available {
        return false;
    }
    let enabled = config
        .get("exotic_enabled")
        .map(|v| v != "false")
        .unwrap_or(true);
    let paused = q::get_config(conn, "exotic_paused")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled || paused {
        return false;
    }
    // auto 门控：关闭自动处理后，仅用户显式请求（start/retry）可运行；自动 wake 一律不跑（P2）。
    let auto = config
        .get("exotic_auto_process")
        .map(|v| v != "false")
        .unwrap_or(true);
    if !auto && !bypass_auto {
        return false;
    }
    if !host.is_task_runnable(plugin_id, capability) {
        return false;
    }
    // 病历 #4:版本对账轮免查就绪任务(pipeline 步骤 2 需要 spawn 探针才能拿到
    // worker_version,此处无从预判「版本是否变了」,统一放行一轮,没变则零领取即收)。
    if force_run_once {
        return true;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 🔴 第 8 轮核验 P1-4：用传入的 `capability` 而非硬编码 CAPABILITY_STR("thumbnail")——
    // 否则 AI/face 等多能力插件会查错任务类型（has_ready_exotic_task 按 capability 列硬过滤）。
    // 现行调用方均传 Capability::Thumbnail，故 PSD 路径行为不变（capability.as_str()=="thumbnail"）。
    q::has_ready_exotic_task(conn, plugin_id, capability.as_str(), now).unwrap_or(false)
}

/// exotic（冷门格式插件）Coordinator 启动期装配（Part2 §4.1）。
///
/// 单一调度器：接扫描/安装/激活/配置/重试事件，幂等唤醒唯一 Pipeline。Part2 无真实
/// License → 默认门控为不可领取（除 dev fixture）；有 Worker + 授权时自动出图。
///
/// 自 `lib.rs::run()` 的 setup 段 p 迁出(D-450 纯结构移动,行为不变)。
/// 顺序不变量:必须在 `app.manage(app_state)` 之后调用。
pub fn bootstrap(app: AppHandle, state: Arc<AppState>) {
    // 运行期 Host：catalog + 只读连接池安装真相 + keyring 授权真相（Part3 §5）。
    let host = Arc::new(state.exotic_host());
    let coord = ExoticCoordinator::start(app, state.clone(), host);
    state.set_exotic_coordinator(coord);
    // 启动 wake：恢复上次遗留的就绪任务（孤儿恢复 + backfill 后的待处理）。
    state.wake_exotic(WakeReason::Startup);
    info!("exotic Coordinator 已启动 | exotic Coordinator started");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::catalog::CatalogStore;

    #[test]
    fn full_wake_slot_preserves_user_and_reconcile_semantics() {
        let (tx, mut rx) = mpsc::channel(1);
        let pending = Arc::new(AtomicU8::new(0));
        let coord = ExoticCoordinator {
            tx,
            pending: Arc::clone(&pending),
        };
        coord.wake(WakeReason::RetryDue);
        coord.wake(WakeReason::UserRequested);
        coord.wake(WakeReason::PluginInstalled);
        assert_eq!(rx.try_recv(), Ok(()));
        assert_eq!(
            pending.swap(0, Ordering::SeqCst),
            WAKE_PENDING | WAKE_USER | WAKE_RECONCILE
        );
        // 运行期间的新 wake 留给下一轮，不继承已经消费的 user 位。
        coord.wake(WakeReason::ScanCommitted);
        assert_eq!(rx.try_recv(), Ok(()));
        assert_eq!(pending.swap(0, Ordering::SeqCst), WAKE_PENDING);
    }

    #[test]
    fn cooldown_survives_wakes_until_due_or_explicit_reset() {
        let now = Instant::now();
        let mut cooldowns = HashMap::from([(PSD_PLUGIN_ID.to_string(), now + PLUGIN_COOLDOWN)]);
        assert!(cooldown_active(
            &mut cooldowns,
            PSD_PLUGIN_ID,
            false,
            now + RETRY_TICK
        ));
        assert!(!cooldown_active(&mut cooldowns, RAW_PLUGIN_ID, false, now));
        assert!(!cooldown_active(
            &mut cooldowns,
            PSD_PLUGIN_ID,
            false,
            now + PLUGIN_COOLDOWN
        ));
        assert!(!cooldown_active(&mut cooldowns, PSD_PLUGIN_ID, true, now));
        assert!(!cooldowns.contains_key(PSD_PLUGIN_ID));
    }

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c
    }

    /// A2:`evaluate_run` 的 `exotic_enabled`/`exotic_auto_process` 已迁往 config.toml——测试
    /// 需要一个独立于 `mem_db()` 的 `ConfigManager`(临时目录,泄漏而非清理,测试进程退出即回收,
    /// 与仓内其余 `tempdir()` 测试用法一致)。
    fn mem_config() -> crate::config::ConfigManager {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("config.toml");
        crate::config::ConfigManager::load_or_init(path).unwrap().0
    }

    /// 平台无关 Catalog 夹具(2026-07-05 Linux CI 面):本组测试验证 coordinator 的
    /// 运行条件逻辑(授权/任务/暂停/自动开关),**不验证平台门控**——内置 Catalog 的
    /// PSD 平台清单不含 Linux,在 ubuntu CI 上「应当运行」断言会因 UnsupportedPlatform
    /// 假红,负向断言则因错误的理由通过(测不到本要测的条件)。注入「支持当前平台」的
    /// 最小 Catalog 解耦;平台门控维度由 availability/catalog 自有测试覆盖。
    fn test_catalog() -> Arc<CatalogStore> {
        let json = format!(
            r#"{{"schema":1,"sequence":1,"offerings":[{{
                "plugin_id":"exotic-image-psd","name":"PSD 图像引擎","media_kind":"image",
                "formats":["psd"],"capabilities":["thumbnail"],"license_tier":"paid",
                "sku":"psd-engine-2026","platforms":["{}"],"min_host_version":"0.1.0",
                "override_common":false,"store_url":"https://example.invalid/plugins/psd",
                "worker_id":"psd-worker"}}]}}"#,
            crate::exotic::current_target_triple()
        );
        Arc::new(CatalogStore::with_snapshot(
            crate::exotic::catalog::CatalogSnapshot::parse(&json).unwrap(),
        ))
    }

    fn authorized_host() -> ExoticHost {
        ExoticHost::with_authorized_fixture(test_catalog(), PSD_PLUGIN_ID)
    }

    fn unauthorized_host() -> ExoticHost {
        ExoticHost::new(test_catalog())
    }

    #[test]
    fn plugin_registry_from_builtin_catalog() {
        // 注册表(T13):capabilities 来自内置 Catalog;PSD 不占 GPU、握手 5s。
        // RAW(builtin 叠加豁免,cr2..srw)接入后注册表含 PSD + RAW 两项。
        let store = CatalogStore::from_builtin().unwrap();
        let descs = plugin_descriptors(&store.snapshot());
        // PSD + RAW + video-extended(rmvb/vob 缩略图收编,D-444③)三项。
        assert_eq!(descs.len(), 3);
        assert!(
            !descs
                .iter()
                .any(|d| d.plugin_id == "exotic-ocr" || d.plugin_id == "exotic-enhance"),
            "未声明 worker_id 的 offering(OCR/Enhance 独立服务)不得生成 descriptor"
        );
        let d = descs
            .iter()
            .find(|d| d.plugin_id == PSD_PLUGIN_ID)
            .expect("PSD descriptor 应存在");
        assert_eq!(d.plugin_id, PSD_PLUGIN_ID);
        assert_eq!(d.worker_id, "psd-worker");
        assert_eq!(d.capabilities, vec![Capability::Thumbnail]);
        assert!(!d.uses_gpu);
        assert_eq!(d.handshake_timeout, HANDSHAKE_TIMEOUT);
        let raw = descs
            .iter()
            .find(|d| d.plugin_id == RAW_PLUGIN_ID)
            .expect("RAW descriptor 应存在");
        assert_eq!(raw.worker_id, "raw-worker");
        assert_eq!(raw.capabilities, vec![Capability::Thumbnail]);
        assert!(!raw.uses_gpu);
        assert_eq!(raw.handshake_timeout, HANDSHAKE_TIMEOUT);
        // video-extended:worker=video-worker,只 Thumbnail 能力(remux/transcode/frames 归
        // VideoWorkerService,不进任务化)。
        let video = descs
            .iter()
            .find(|d| d.plugin_id == VIDEO_PLUGIN_ID)
            .expect("video-extended descriptor 应存在");
        assert_eq!(video.worker_id, VIDEO_WORKER_ID);
        assert_eq!(video.capabilities, vec![Capability::Thumbnail]);
        assert!(!video.uses_gpu);
    }

    #[test]
    fn op_timeout_table_invariants() {
        use crate::exotic::worker::PROGRESS_TOTAL_CAP;
        // 表内不变量(D3,2026-07-11 加固批 A-2 修订):进程握手(5s,快失败)≪ thumbnail ≪ 批。
        assert!(HANDSHAKE_TIMEOUT < op_timeouts::THUMBNAIL);
        assert!(op_timeouts::THUMBNAIL < op_timeouts::EMBED_BATCH);
        // v3:SESSION_INIT 是**静默限时**(worker 装载期 10s 一拍心跳,收帧即重置),与各批
        // 处理的总限时不再同量纲——旧不变量「批 ≤ SessionInit(冷加载总上界)」随语义废止。
        // 新不变量:静默档 ≥ 数拍心跳(单拍抖动不误杀,worker HEARTBEAT_INTERVAL=10s),
        // 且 ≤ 批总限时(静默检测本应比任何总预算灵敏)。
        assert!(op_timeouts::SESSION_INIT >= std::time::Duration::from_secs(30));
        assert!(op_timeouts::SESSION_INIT <= op_timeouts::EMBED_BATCH);
        // face 批超时按项数缩放(2026-07-03):单项不低于 thumbnail 档。
        assert!(op_timeouts::face_detect_embed(1) >= op_timeouts::THUMBNAIL);
        // v3 总上界:一切 per-op 预算(含 16 项 face 批与静默档)必须落在 PROGRESS_TOTAL_CAP 之内
        // ——总上界是心跳在途时的最后防线,不得被任何单 op 预算越过。
        assert!(op_timeouts::face_detect_embed(16) < PROGRESS_TOTAL_CAP);
        assert!(op_timeouts::EMBED_BATCH < PROGRESS_TOTAL_CAP);
        assert!(op_timeouts::SESSION_INIT < PROGRESS_TOTAL_CAP);
        // T17 新档:文本编码(CPU 轻)不重于图像批;SessionClose 不重于文本编码。
        assert!(op_timeouts::ENCODE_TEXT <= op_timeouts::EMBED_BATCH);
        assert!(op_timeouts::SESSION_CLOSE <= op_timeouts::ENCODE_TEXT);
        // OCR 新档(T6):session_init 与 8 项批预算须落在总上界内(总上界是最后防线)。
        assert!(op_timeouts::ocr_batch(8) < PROGRESS_TOTAL_CAP);
        assert!(op_timeouts::OCR_SESSION_INIT < PROGRESS_TOTAL_CAP);
    }

    #[test]
    fn no_run_without_worker() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        // worker 不可用 → 不跑，即使其他条件满足。
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            false,
            false,
            false
        ));
    }

    #[test]
    fn no_run_when_unauthorized() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        // 未授权（无 fixture）→ 不跑（Part2：需 License/Part3）。
        assert!(!evaluate_run(
            &c,
            &cfg,
            &unauthorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn no_run_when_paused() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        q::set_config(&c, "exotic_paused", "true").unwrap();
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn no_run_when_no_ready_task() {
        let c = mem_db();
        let cfg = mem_config();
        // 无任务 → 不跑（避免空转）。
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn runs_when_all_conditions_met() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        assert!(evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn disabled_subsystem_blocks_run() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        cfg.set_and_persist("exotic_enabled", "false").unwrap();
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn auto_disabled_blocks_automatic_wake() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        cfg.set_and_persist("exotic_auto_process", "false").unwrap();
        // 自动 wake（bypass_auto=false）→ 不跑（P2）。
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            false
        ));
    }

    #[test]
    fn reconcile_wake_bypasses_ready_check_but_not_gates() {
        // 病历 #4(2026-07-05 真机):安装/升级/回滚/启动的版本对账轮——无就绪任务
        // (纯升级后任务全 done)也放行一轮,让 pipeline 步骤 2 拿探针 worker_version 做失效比对。
        let c = mem_db();
        let cfg = mem_config();
        // 零任务/零就绪:对账轮放行(版本是否变了只有 spawn 探针才知道)。
        assert!(evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            true
        ));
        // 其余门控不被对账绕过:worker 不可用照拦……
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            false,
            false,
            true
        ));
        // ……暂停也照拦。
        q::set_config(&c, "exotic_paused", "true").unwrap();
        assert!(!evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            false,
            true
        ));
    }

    #[test]
    fn needs_reconcile_maps_reasons() {
        // 对账集 = 安装/升级/回滚 + 启动兜底 + 用户显式开始;周期时钟刻意排除(防 30s 白 spawn)。
        assert!(needs_reconcile(WakeReason::PluginInstalled));
        assert!(needs_reconcile(WakeReason::Startup));
        assert!(needs_reconcile(WakeReason::UserRequested));
        assert!(!needs_reconcile(WakeReason::RetryDue));
        assert!(!needs_reconcile(WakeReason::ScanCommitted));
        assert!(!needs_reconcile(WakeReason::ConfigChanged));
    }

    #[test]
    fn auto_disabled_allows_user_request() {
        let c = mem_db();
        let cfg = mem_config();
        q::seed_exotic_tasks_for_item(&c, 1, PSD_PLUGIN_ID, &["thumbnail".into()]).unwrap();
        cfg.set_and_persist("exotic_auto_process", "false").unwrap();
        // 用户显式请求（start/retry → bypass_auto=true）→ 仍运行。
        assert!(evaluate_run(
            &c,
            &cfg,
            &authorized_host(),
            PSD_PLUGIN_ID,
            CAPABILITY,
            true,
            true,
            false
        ));
    }
}
