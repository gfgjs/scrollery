// src-tauri/src/enhance/service.rs
//! 影像增强 host 侧服务（降噪/超分子系统 design.md §A/§B/§E）。
//!
//! enhance **不进** exotic 任务化调度（D-OCR-7 同型豁免）：本服务直持 enhance-worker
//! 子进程 + 内存 job 队列，由 IPC `enhance_start` 显式驱动。P0 job 队列为**内存态**
//! （重启即丢，B 节裁决——交互任务，用户重发即可）。
//!
//! 执行链（每 item）：准入门 → 参数校验 → EnhanceSessionInit → claim `{stem}-enhanced.{ext}`
//! → EnhanceRun（worker 写 `{work_dir}/…tmp`）→ 容器级 EXIF 注入（不重编码）→ 同卷 rename
//! → 单文件 ingest（编辑线 `ingest_single_file` 同款）。
//!
//! GPU 令牌（D2 天条）：执行期先 `BackgroundHeavyLimiter` CPU permit 后 `GpuToken`，
//! 与 CLIP/人脸争同一令牌，自然串行不并发推理。
//!
//! 取消：`cancel(job_id)` 置该 job 的 `CancellationToken`——`run_request`/limiter.acquire
//! 每 CANCEL_POLL 醒来检查，命中即中止在途请求并由 supervisor kill worker（design.md §E
//! 「取消 = kill worker」）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use exotic_protocol::{
    capability, EnhanceStep, EnhanceTask, ModelDescriptor, ModelHandle, ModelRole, ProgressBody,
    RequestBody, SuccessBody, WorkerErrorCode, MAX_BLOB_LEN,
};
use scrollery_ai_core::enhance::plan_tiles;
use scrollery_ai_core::enhance_profile::find_enhance_profile;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::db::queries as q;
use crate::editing::{ingest, naming};
use crate::enhance::exif_inject::{build_source_exif, inject_exif_container};
use crate::error::AppError;
use crate::exotic::coordinator::op_timeouts;
use crate::exotic::supervisor::WorkerSupervisor;
use crate::exotic::worker::{RawOutcome, WorkerConfig, WorkerSpec};
use crate::state::AppState;

/// 批请求硬止损：同一请求至多尝试次数（首发 + 重建后重发一次，照 worker_client MAX_ATTEMPTS）。
const MAX_ATTEMPTS: usize = 2;
/// retryable 失败重发前退避（给 DirectML 瞬态错误一个恢复窗口）。
const RETRY_BACKOFF: Duration = Duration::from_millis(500);
/// 弃用实例时给 worker 的体面退出宽限。
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// 准入门：降噪/去伪影输入像素上限（与编辑线 D-008 同门，100MP）。
/// ⚠ J-6 草案，bench 后终钉。
const MAX_INPUT_PIXELS_DIRECT: u64 = 100_000_000;
/// 准入门：4x 超分输入像素上限（输出像素 = 输入×scale² 反推得草案 16MP）。
/// ⚠ J-6 草案，bench 后终钉。
const MAX_INPUT_PIXELS_UPSCALE_4X: u64 = 16_000_000;

/// 前端事件：队列/进度变化（前端据此 `get_enhance_queue` 拉全量，形态对齐 exotic:status-changed）。
const EVT_QUEUE_CHANGED: &str = "enhance:queue-changed";
/// per-tile 进度事件节流下限（深审 b）：Progress 帧每 ≥500ms 才推一次 queue-changed，
/// 避免高频 tile 心跳打爆前端事件通道。
const TILE_EMIT_THROTTLE: Duration = Duration::from_millis(500);
/// RAW 插件 id（单源：`resources/exotic-catalog.json` 的 `exotic-image-raw` offering；
/// 运行时经 catalog `resolve_format` 反查该 id，RAW 格式清单不在此手抄，深审 g）。
const RAW_PLUGIN_ID: &str = "exotic-image-raw";

// ── 参数 / DTO ────────────────────────────────────────────────────────────────

/// 输出格式选择（design.md §G `enhance_output_format`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormatChoice {
    FollowSource,
    Jpeg,
    Png,
}

/// 一次增强提交的参数：任务链（未排序，host 负责排序）+ 输出格式。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhanceParams {
    /// 执行链（每步 = 任务 + 模型档 + 可选强度）。host 排序为固定序（降噪→去伪影→超分）。
    pub steps: Vec<EnhanceStep>,
    pub output_format: OutputFormatChoice,
}

/// job 生命周期状态（P0 内存态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
}

/// 队列内单 job 的前端投影。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDto {
    pub id: u64,
    pub status: JobStatus,
    /// 本 job 的源 item 总数。
    pub total: usize,
    /// 已完成 item 数。
    pub done: usize,
    /// 本 job 累计已完成 tile 数（per-tile Progress 帧 live 计数，深审 b）。
    pub tile_done: usize,
    /// 预计总 tile 数（批 5.5：入队时按各 item 已知尺寸 + steps 链几何估算，与 worker
    /// `chain.rs` 同源公式 `plan_tiles(...).len()` 逐 step 累加、scale 逐 step 叠乘）。
    /// 0 = 无法估算（item 尺寸未知/尚未扫描）——前端据此回退为不定长进度态。
    pub tiles_total: u32,
    pub error_code: Option<String>,
}

// ── 内部簿记 ──────────────────────────────────────────────────────────────────

struct JobRecord {
    id: u64,
    status: JobStatus,
    total: usize,
    done: usize,
    /// 累计已完成 tile 数（per-tile Progress 心跳，深审 b）。
    tile_done: usize,
    /// 入队时估算的总 tile 数（批 5.5，不随执行更新——纯估算基线）。
    tiles_total: u32,
    error_code: Option<&'static str>,
    token: CancellationToken,
    item_ids: Vec<i64>,
    params: EnhanceParams,
}

struct JobBook {
    next_job_id: u64,
    jobs: Vec<JobRecord>,
}

/// worker 句柄 + 会话号发号器。独立 Mutex：一 job 执行期全程持此锁 → 天然串行（worker 单实例）。
struct WorkerHandle {
    sup: Option<WorkerSupervisor>,
    next_session_id: u64,
    /// 当前 worker 实例上是否已装载本 job 的 EnhanceSessionInit 会话。worker 一旦重建
    /// （drop_worker / ensure 重生）即失效——`send_request` 重发 EnhanceRun 前据此补发
    /// init（深审 h：worker 死亡后 session_id 落到无会话的新实例必败）。
    session_alive: bool,
}

impl WorkerHandle {
    /// 确保有存活 worker（死亡/缺失即重建）。重建即置 `session_alive=false`（新实例无会话）。
    fn ensure(&mut self) -> Result<&mut WorkerSupervisor, AppError> {
        if self.sup.as_ref().is_some_and(|s| s.is_alive()) {
            return Ok(self.sup.as_mut().expect("上方已判存活"));
        }
        if let Some(old) = self.sup.take() {
            old.shutdown(SHUTDOWN_GRACE);
        }
        self.session_alive = false; // 新实例无会话（深审 h）
        if !enhance_worker_ready() {
            return Err(eerr(
                "enhance_worker_missing",
                "增强组件缺失，请重新安装应用",
            ));
        }
        let sup = spawn_enhance_worker().map_err(|e| {
            // 深审 e：对外固定串，内部错误只进 tracing（不泄进程/路径细节）。
            tracing::warn!("增强 worker 启动失败:{e}");
            eerr("enhance_io", "增强 worker 启动失败")
        })?;
        self.sup = Some(sup);
        Ok(self.sup.as_mut().expect("上方已 set"))
    }

    fn drop_worker(&mut self) {
        if let Some(w) = self.sup.take() {
            w.shutdown(SHUTDOWN_GRACE);
        }
        self.session_alive = false; // 会话随实例消亡（深审 h）
    }
}

/// 前后对比预览产物路径（前端 convertFileSrc 消费；固定名覆盖写，不累积）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPaths {
    pub before_path: String,
    pub after_path: String,
}

/// 影像增强 host 服务（`AppState.enhance_service` 持有）。
pub struct EnhanceService {
    worker: Mutex<WorkerHandle>,
    book: Mutex<JobBook>,
    /// 启动后是否已清扫过 work_dir 残留（深审 f：进程首次用 work_dir 时清一次，重启即丢语义）。
    work_dir_swept: AtomicBool,
}

impl Default for EnhanceService {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhanceService {
    pub fn new() -> Self {
        EnhanceService {
            worker: Mutex::new(WorkerHandle {
                sup: None,
                next_session_id: 1,
                session_alive: false,
            }),
            book: Mutex::new(JobBook {
                next_job_id: 1,
                jobs: Vec::new(),
            }),
            work_dir_swept: AtomicBool::new(false),
        }
    }

    /// 进程首次触及 work_dir 时清扫残留文件（深审 f）：重启即丢语义，安全——本进程尚无
    /// 在途产物，且 worker 锁全程串行（job 执行/预览互斥），不会误删在写文件。
    fn sweep_work_dir_once(&self, work_dir: &Path) {
        if self.work_dir_swept.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(work_dir) {
            for entry in rd.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// 队列全量快照（前端 `get_enhance_queue`）。
    pub fn queue_snapshot(&self) -> Vec<JobDto> {
        let book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        book.jobs
            .iter()
            .map(|j| JobDto {
                id: j.id,
                status: j.status,
                total: j.total,
                done: j.done,
                tile_done: j.tile_done,
                tiles_total: j.tiles_total,
                error_code: j.error_code.map(|c| c.to_string()),
            })
            .collect()
    }

    /// 入队一个 job（参数已在命令层校验 NaN/门控）：返回 job_id，状态 Queued。
    /// `tiles_total` 按各 item 的 DB 已知尺寸（未扫描出宽高则该 item 贡献 0）+ steps 链几何
    /// 估算——与 worker `chain.rs` 预算总瓦片数同款公式，供前端进度条换算百分比（批 5.5）。
    pub fn enqueue(&self, state: &AppState, item_ids: Vec<i64>, params: EnhanceParams) -> u64 {
        let sorted_steps = sort_steps(params.steps.clone());
        let dims = item_dims(state, &item_ids);
        let tiles_total = estimate_tiles_total(&sorted_steps, &dims);

        let mut book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        let id = book.next_job_id;
        book.next_job_id += 1;
        book.jobs.push(JobRecord {
            id,
            status: JobStatus::Queued,
            total: item_ids.len(),
            done: 0,
            tile_done: 0,
            tiles_total,
            error_code: None,
            token: CancellationToken::new(),
            item_ids,
            params,
        });
        id
    }

    /// 取消某 job：置其 token（在途 run_request/limiter.acquire 醒来即中止 → supervisor kill）。
    pub fn cancel(&self, job_id: u64) {
        let mut book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = book.jobs.iter_mut().find(|j| j.id == job_id) {
            j.token.cancel();
            if matches!(j.status, JobStatus::Queued) {
                j.status = JobStatus::Cancelled;
            }
        }
    }

    fn set_status(&self, job_id: u64, status: JobStatus, error_code: Option<&'static str>) {
        let mut book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = book.jobs.iter_mut().find(|j| j.id == job_id) {
            j.status = status;
            if error_code.is_some() {
                j.error_code = error_code;
            }
        }
    }

    fn bump_done(&self, job_id: u64) {
        let mut book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = book.jobs.iter_mut().find(|j| j.id == job_id) {
            j.done += 1;
        }
    }

    /// per-tile 心跳（深审 b）：worker 每完成一个 tile 累加一次。
    fn bump_tile(&self, job_id: u64) {
        let mut book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = book.jobs.iter_mut().find(|j| j.id == job_id) {
            j.tile_done += 1;
        }
    }

    /// 取 job 的执行输入快照（token/items/params）。
    fn job_inputs(&self, job_id: u64) -> Option<(CancellationToken, Vec<i64>, EnhanceParams)> {
        let book = self.book.lock().unwrap_or_else(|e| e.into_inner());
        book.jobs
            .iter()
            .find(|j| j.id == job_id)
            .map(|j| (j.token.clone(), j.item_ids.clone(), j.params.clone()))
    }

    /// 阻塞驱动一个 job（在 `spawn_blocking` 内调用）。持 worker 锁全程 → 与其它 job 串行。
    pub fn run_job_blocking(&self, app: &AppHandle, state: &AppState, job_id: u64) {
        let Some((token, item_ids, params)) = self.job_inputs(job_id) else {
            return;
        };
        if token.is_cancelled() {
            self.set_status(job_id, JobStatus::Cancelled, None);
            let _ = app.emit(EVT_QUEUE_CHANGED, ());
            return;
        }
        self.set_status(job_id, JobStatus::Running, None);
        let _ = app.emit(EVT_QUEUE_CHANGED, ());

        let mut handle = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        let result = self.execute(&mut handle, state, app, job_id, &token, &item_ids, &params);
        // 会话卸载 best-effort（worker 空闲 300s 自杀兜底；出错/取消时 worker 可能已被 kill）。
        let last_sid = handle.next_session_id.saturating_sub(1);
        if let Some(sup) = handle.sup.as_mut() {
            if sup.is_alive() {
                let _ = sup.run_request(
                    &RequestBody::EnhanceSessionClose {
                        session_id: last_sid,
                    },
                    op_timeouts::SESSION_CLOSE,
                    &|| false,
                );
            }
        }
        handle.session_alive = false;
        drop(handle);

        let final_status = match &result {
            Ok(()) => JobStatus::Done,
            Err(_) if token.is_cancelled() => JobStatus::Cancelled,
            Err(_) => JobStatus::Error,
        };
        let code = match &result {
            Err(AppError::Enhance { code, .. }) if !token.is_cancelled() => Some(*code),
            Err(_) if !token.is_cancelled() => Some("enhance_io"),
            _ => None,
        };
        self.set_status(job_id, final_status, code);
        let _ = app.emit(EVT_QUEUE_CHANGED, ());
    }

    /// job 执行主链（持 worker 锁）。
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        handle: &mut WorkerHandle,
        state: &AppState,
        app: &AppHandle,
        job_id: u64,
        token: &CancellationToken,
        item_ids: &[i64],
        params: &EnhanceParams,
    ) -> Result<(), AppError> {
        // 固定序（host 是排序契约，worker 不重排）+ 最大 scale（准入门用）。
        let steps = sort_steps(params.steps.clone());
        if steps.is_empty() {
            return Err(eerr("enhance_invalid_params", "未选择任何增强任务"));
        }
        let max_scale = steps
            .iter()
            .filter_map(|s| find_enhance_profile(&s.model_id).map(|p| p.scale))
            .max()
            .unwrap_or(1);

        let models_dir = crate::ai::runtime_config::models_dir(state);
        // 涉及的模型档均须就位（fp32+fp16）。缺 → enhance_model_missing（引导设置页下载）。
        let model_ids: HashSet<&str> = steps.iter().map(|s| s.model_id.as_str()).collect();
        for id in &model_ids {
            if !super::registry::enhance_model_installed(&models_dir, id) {
                return Err(eerr(
                    "enhance_model_missing",
                    "增强模型尚未下载，请前往设置页下载",
                ));
            }
        }

        // work_dir = app cache 下 enhance_work/（worker 输出白名单前缀）。
        let work_dir = state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .join("enhance_work");
        std::fs::create_dir_all(&work_dir)
            .map_err(|_| eerr("enhance_io", "增强临时目录创建失败"))?;
        self.sweep_work_dir_once(&work_dir); // 深审 f：首次清扫上次运行残留 tmp。

        // D2 顺序天条：先 CPU permit 后 GPU 令牌（取消即退队返回 None）。
        let _cpu = state
            .background_heavy_limiter
            .acquire(token)
            .ok_or_else(|| eerr("enhance_io", "已取消"))?;
        let _gpu = state
            .gpu_token
            .acquire(token)
            .ok_or_else(|| eerr("enhance_io", "已取消"))?;

        // EnhanceSessionInit（本 job 一次；模型集 = 各 distinct model_id 的 fp16 档）。
        let session_id = {
            let s = handle.next_session_id;
            handle.next_session_id += 1;
            s
        };
        // GPU 且 fp16_safe 档走 fp16，否则 fp32（含 CPU 与 SCUNet 等不安全档，spike-D/task a）。
        let gpu = enhance_prefers_gpu(state);
        let mut models = Vec::with_capacity(model_ids.len());
        for id in &model_ids {
            models.push(enhance_model_descriptor(&models_dir, id, gpu)?);
        }
        let init = RequestBody::EnhanceSessionInit {
            session_id,
            models,
            models_root: models_dir.to_string_lossy().into_owned(),
            work_dir: work_dir.to_string_lossy().into_owned(),
        };
        send_request(
            handle,
            &init,
            op_timeouts::ENHANCE_SESSION_INIT,
            token,
            None,
            None,
        )?;
        handle.session_alive = true; // 深审 h：init 成功即标会话就绪。

        // 逐 item。
        for (idx, &item_id) in item_ids.iter().enumerate() {
            if token.is_cancelled() {
                return Err(eerr("enhance_io", "已取消"));
            }
            self.process_item(
                handle,
                state,
                app,
                &init,
                session_id,
                &work_dir,
                job_id,
                idx,
                item_id,
                &steps,
                max_scale,
                params.output_format,
                token,
            )?;
            self.bump_done(job_id);
            let _ = app.emit(EVT_QUEUE_CHANGED, ());
        }
        Ok(())
    }

    /// 单 item 处理：准入 → claim → EnhanceRun → EXIF 注入 → rename → ingest。
    #[allow(clippy::too_many_arguments)]
    fn process_item(
        &self,
        handle: &mut WorkerHandle,
        state: &AppState,
        app: &AppHandle,
        session_init: &RequestBody,
        session_id: u64,
        work_dir: &Path,
        job_id: u64,
        idx: usize,
        item_id: i64,
        steps: &[EnhanceStep],
        max_scale: u32,
        output_format: OutputFormatChoice,
        token: &CancellationToken,
    ) -> Result<(), AppError> {
        // 源信息（DB 读；错误脱敏为 enhance_io，不泄内部串）。
        let (abs_path, directory_id, file_name, rel_path, volume_id) = {
            let conn = state
                .db_read_pool
                .get()
                .map_err(|_| eerr("enhance_io", "数据库读取失败"))?;
            let detail =
                q::get_media_detail(&conn, item_id).map_err(|_| eerr("enhance_io", "源不存在"))?;
            if detail.item.is_deleted || detail.availability != "online" {
                return Err(eerr("enhance_input_unsupported", "源文件当前不可用"));
            }
            let (_, rel_path, _) = q::get_item_path_info(&conn, item_id)
                .map_err(|_| eerr("enhance_io", "路径解析失败"))?;
            let (_, volume_id) =
                q::get_root_and_volume_for_directory(&conn, detail.item.directory_id)
                    .map_err(|_| eerr("enhance_io", "卷解析失败"))?;
            (
                detail.abs_path.clone(),
                detail.item.directory_id,
                detail.item.file_name.clone(),
                rel_path,
                volume_id,
            )
        };

        // RAW 前置拒：已移至 `enhance_start` 同步准入（入队前拦，不产生死 job，批 5 复核）；
        // 此处仅保留解码/尺寸类判定（须解码才知，维持异步）。

        // 解码尺寸（RAW/不可解码 → enhance_input_unsupported）+ 准入门。
        let (in_w, in_h) = image::ImageReader::open(&abs_path)
            .ok()
            .and_then(|r| r.with_guessed_format().ok())
            .and_then(|r| r.into_dimensions().ok())
            .ok_or_else(|| {
                eerr(
                    "enhance_input_unsupported",
                    "无法解码源图（RAW/不支持格式）",
                )
            })?;
        admission_check(in_w, in_h, max_scale)?;

        // 输出格式跟随/指定。
        let src_ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (out_fmt_str, out_ext) = resolve_output_format(output_format, &src_ext);

        // claim `{stem}-enhanced.{ext}`（naming.rs 复用；Edit 错误脱敏为 enhance 码）。
        let dir = Path::new(&abs_path)
            .parent()
            .ok_or_else(|| eerr("enhance_io", "目标目录解析失败"))?;
        let src_stem = Path::new(&file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name);
        let stem = format!(
            "{}-enhanced",
            crate::export::naming::sanitize_component(src_stem)
        );
        let claimed = naming::claim_target_path(dir, &stem, out_ext).map_err(|e| match e {
            AppError::Edit { code, .. } if code == naming::CODE_TARGET_CONFLICT => {
                eerr("enhance_io", "目标命名冲突次数过多")
            }
            _ => eerr("enhance_io", "目标目录不可写"),
        })?;

        // worker 输出临时文件（work_dir 内，白名单前缀）。
        let out_tmp = work_dir.join(format!("job{job_id}-item{idx}.tmp"));
        let _ = std::fs::remove_file(&out_tmp);

        let source_path = std::path::Path::new(&abs_path)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs_path.clone());
        let run = RequestBody::EnhanceRun {
            session_id,
            source_path,
            output_tmp_path: out_tmp.to_string_lossy().into_owned(),
            output_format: out_fmt_str.to_string(),
            steps: steps.to_vec(),
        };
        let run_timeout = op_timeouts::ENHANCE_SILENCE;
        // per-tile 心跳（深审 b）：每 Progress 帧累加 tile 计数，节流推 queue-changed。
        let mut last_emit = Instant::now();
        let mut on_tile = |_p: &ProgressBody| {
            self.bump_tile(job_id);
            if last_emit.elapsed() >= TILE_EMIT_THROTTLE {
                let _ = app.emit(EVT_QUEUE_CHANGED, ());
                last_emit = Instant::now();
            }
        };
        // 深审 h：worker 死亡重建后本 EnhanceRun 前先补发 session_init。
        let body = send_request(
            handle,
            &run,
            run_timeout,
            token,
            Some(&mut on_tile as &mut dyn FnMut(&ProgressBody)),
            Some((session_init, op_timeouts::ENHANCE_SESSION_INIT)),
        )
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&claimed); // 释放占位
            let _ = std::fs::remove_file(&out_tmp);
        })?;
        let done = body.enhance.ok_or_else(|| {
            let _ = std::fs::remove_file(&claimed);
            let _ = std::fs::remove_file(&out_tmp); // 深审 f：缺回执路径也清 worker tmp。
            eerr("enhance_io", "增强产物缺回执")
        })?;

        // 容器级 EXIF 注入（不重编码）→ 同卷 rename 到认领路径。
        finalize_output(&out_tmp, &claimed, Path::new(&abs_path), out_ext).inspect_err(|_| {
            let _ = std::fs::remove_file(&out_tmp); // 深审 f：finalize 失败早退亦清 tmp。
        })?;
        let _ = std::fs::remove_file(&out_tmp);

        // 单文件 ingest（编辑线同款）。
        let stat =
            std::fs::metadata(&claimed).map_err(|_| eerr("enhance_io", "读取新文件属性失败"))?;
        let file_mtime = stat
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let file_mtime_ns = stat
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_nanos()).ok())
            .unwrap_or(0);
        let out_name = claimed
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let ingest_input = ingest::IngestInput {
            directory_id,
            rel_path_norm: &rel_path,
            file_name: &out_name,
            file_size: stat.len() as i64,
            file_mtime,
            file_mtime_ns,
            file_format: out_ext,
            width: i64::from(done.out_width),
            height: i64::from(done.out_height),
            volume_id,
        };
        {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            ingest::ingest_single_file(&conn, &ingest_input).map_err(|_| {
                // 回滚:产物已改名进媒体库(DB 无对应行),失败不删会在库目录留孤儿文件,
                // 重试又撞 claim 命名冲突逐次累积副本——与上方各错误路径的 remove_file 对称。
                let _ = std::fs::remove_file(&claimed);
                eerr("enhance_io", "增强结果已生成，但写入媒体库失败")
            })?;
        }
        Ok(())
    }

    /// 前后对比预览（task c）：裁中心（或点位为中心）512² 单 tile 跑一次增强，产物固定名
    /// 覆盖写至 app cache `enhance_preview/`（before.png / after.png，不累积）。**不入队列、
    /// 不 claim、不 ingest**。worker 正忙（job 执行中持 worker 锁，严格单请求）→ `enhance_busy`。
    ///
    /// `point`：归一化 [0,1] 画面坐标；`None` = 画面中心。
    pub fn preview(
        &self,
        state: &AppState,
        item_id: i64,
        point: Option<(f64, f64)>,
        params: &EnhanceParams,
    ) -> Result<PreviewPaths, AppError> {
        // 严格单请求：worker 正被某 job 执行（持锁）即忙 → 稳定码。
        let mut handle = match self.worker.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(eerr("enhance_busy", "增强任务进行中，请稍后再试"));
            }
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        };

        let steps = sort_steps(params.steps.clone());
        if steps.is_empty() {
            return Err(eerr("enhance_invalid_params", "未选择任何增强任务"));
        }
        validate_strengths(&steps)?;

        let models_dir = crate::ai::runtime_config::models_dir(state);
        let model_ids: HashSet<&str> = steps.iter().map(|s| s.model_id.as_str()).collect();
        for id in &model_ids {
            if !super::registry::enhance_model_installed(&models_dir, id) {
                return Err(eerr(
                    "enhance_model_missing",
                    "增强模型尚未下载，请前往设置页下载",
                ));
            }
        }

        // 源 abs_path。
        let abs_path = {
            let conn = state
                .db_read_pool
                .get()
                .map_err(|_| eerr("enhance_io", "数据库读取失败"))?;
            let detail =
                q::get_media_detail(&conn, item_id).map_err(|_| eerr("enhance_io", "源不存在"))?;
            if detail.item.is_deleted || detail.availability != "online" {
                return Err(eerr("enhance_input_unsupported", "源文件当前不可用"));
            }
            detail.abs_path.clone()
        };
        if is_raw_format(state, Path::new(&abs_path)) {
            return Err(eerr("enhance_input_unsupported", "RAW 源暂不支持增强"));
        }

        // 解码 + 裁剪中心/点位 512²（小于 512 的维度取全幅）。
        let img = image::ImageReader::open(&abs_path)
            .ok()
            .and_then(|r| r.with_guessed_format().ok())
            .and_then(|r| r.decode().ok())
            .ok_or_else(|| {
                eerr(
                    "enhance_input_unsupported",
                    "无法解码源图（RAW/不支持格式）",
                )
            })?;
        let (iw, ih) = (img.width(), img.height());
        const PV: u32 = 512;
        let cw = iw.min(PV);
        let ch = ih.min(PV);
        let (cx, cy) = match point {
            Some((fx, fy)) => (
                (fx.clamp(0.0, 1.0) * f64::from(iw)) as u32,
                (fy.clamp(0.0, 1.0) * f64::from(ih)) as u32,
            ),
            None => (iw / 2, ih / 2),
        };
        let x = cx.saturating_sub(cw / 2).min(iw - cw);
        let y = cy.saturating_sub(ch / 2).min(ih - ch);
        let crop = img.crop_imm(x, y, cw, ch);

        // 预览目录 + before.png（固定名，但仍须 tmp + 同卷 rename，避免中断留下半图）。
        let cache_dir = state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone();
        let preview_dir = cache_dir.join("enhance_preview");
        std::fs::create_dir_all(&preview_dir)
            .map_err(|_| eerr("enhance_io", "预览目录创建失败"))?;
        let before = preview_dir.join("before.png");
        let mut before_bytes = std::io::Cursor::new(Vec::new());
        crop.write_to(&mut before_bytes, image::ImageFormat::Png)
            .map_err(|_| eerr("enhance_io", "预览裁剪编码失败"))?;
        crate::thumbnail::generator::write_atomic(&before, before_bytes.get_ref())
            .map_err(|_| eerr("enhance_io", "预览裁剪写盘失败"))?;

        // work_dir（worker 输出白名单前缀）。
        let work_dir = cache_dir.join("enhance_work");
        std::fs::create_dir_all(&work_dir)
            .map_err(|_| eerr("enhance_io", "增强临时目录创建失败"))?;
        self.sweep_work_dir_once(&work_dir);

        // D2 天条：与 CLIP/face 争同一 GPU 令牌，避免并发推理（预览无取消，用一次性 token）。
        let cancel = CancellationToken::new();
        let _cpu = state
            .background_heavy_limiter
            .acquire(&cancel)
            .ok_or_else(|| eerr("enhance_io", "资源获取失败"))?;
        let _gpu = state
            .gpu_token
            .acquire(&cancel)
            .ok_or_else(|| eerr("enhance_io", "资源获取失败"))?;

        // EnhanceSessionInit。
        let session_id = {
            let s = handle.next_session_id;
            handle.next_session_id += 1;
            s
        };
        let gpu = enhance_prefers_gpu(state);
        let mut models = Vec::with_capacity(model_ids.len());
        for id in &model_ids {
            models.push(enhance_model_descriptor(&models_dir, id, gpu)?);
        }
        let init = RequestBody::EnhanceSessionInit {
            session_id,
            models,
            models_root: models_dir.to_string_lossy().into_owned(),
            work_dir: work_dir.to_string_lossy().into_owned(),
        };
        send_request(
            &mut handle,
            &init,
            op_timeouts::ENHANCE_SESSION_INIT,
            &cancel,
            None,
            None,
        )?;
        handle.session_alive = true;

        // EnhanceRun（单 tile 量级；预览产物恒 PNG）。
        let out_tmp = work_dir.join("preview.tmp");
        let _ = std::fs::remove_file(&out_tmp);
        let source_path = std::path::Path::new(&before)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| before.to_string_lossy().into_owned());
        let run = RequestBody::EnhanceRun {
            session_id,
            source_path,
            output_tmp_path: out_tmp.to_string_lossy().into_owned(),
            output_format: "png".to_string(),
            steps: steps.clone(),
        };
        let res = send_request(
            &mut handle,
            &run,
            op_timeouts::ENHANCE_SILENCE,
            &cancel,
            None,
            Some((&init, op_timeouts::ENHANCE_SESSION_INIT)),
        );

        // 会话卸载 best-effort。
        if let Some(sup) = handle.sup.as_mut() {
            if sup.is_alive() {
                let _ = sup.run_request(
                    &RequestBody::EnhanceSessionClose { session_id },
                    op_timeouts::SESSION_CLOSE,
                    &|| false,
                );
            }
        }
        handle.session_alive = false;

        res.inspect_err(|_| {
            let _ = std::fs::remove_file(&out_tmp);
        })?;

        // after.png（固定名覆盖；worker 产物无元数据，预览直接同卷 rename，跨卷兜底 copy）。
        let after = preview_dir.join("after.png");
        let _ = std::fs::remove_file(&after);
        if std::fs::rename(&out_tmp, &after).is_err() {
            let bytes =
                std::fs::read(&out_tmp).map_err(|_| eerr("enhance_io", "预览产物读取失败"))?;
            crate::thumbnail::generator::write_atomic(&after, &bytes)
                .map_err(|_| eerr("enhance_io", "预览产物落盘失败"))?;
            let _ = std::fs::remove_file(&out_tmp);
        }

        Ok(PreviewPaths {
            before_path: before.to_string_lossy().into_owned(),
            after_path: after.to_string_lossy().into_owned(),
        })
    }
}

// ── 纯 helper（可单测）─────────────────────────────────────────────────────────

/// 任务固定执行序：降噪(0) → 去伪影(1) → 超分(2)。host 排序是契约。
fn task_order(task: EnhanceTask) -> u8 {
    match task {
        EnhanceTask::Denoise => 0,
        EnhanceTask::DejpegArtifact => 1,
        EnhanceTask::Upscale => 2,
    }
}

/// 按固定序稳定排序（同任务多档保持相对次序）。
pub fn sort_steps(mut steps: Vec<EnhanceStep>) -> Vec<EnhanceStep> {
    steps.sort_by_key(|s| task_order(s.task));
    steps
}

/// strength 有限性校验：NaN/Inf → enhance_invalid_params。
pub fn validate_strengths(steps: &[EnhanceStep]) -> Result<(), AppError> {
    for s in steps {
        if let Some(v) = s.strength {
            if !v.is_finite() {
                return Err(eerr("enhance_invalid_params", "强度参数非法（NaN/Inf）"));
            }
        }
    }
    Ok(())
}

/// 准入门（J-6 草案数值，bench 后终钉）：含 4x 超分 → 输入 ≤16MP；否则 ≤100MP。
pub fn admission_check(width: u32, height: u32, max_scale: u32) -> Result<(), AppError> {
    let pixels = u64::from(width) * u64::from(height);
    let limit = if max_scale >= 4 {
        MAX_INPUT_PIXELS_UPSCALE_4X
    } else {
        MAX_INPUT_PIXELS_DIRECT
    };
    if pixels > limit {
        return Err(eerr("enhance_input_too_large", "图像超出增强尺寸上限"));
    }
    Ok(())
}

/// 各 item 的已知宽高（DB `media_items.width/height`；best-effort，读失败或 0 尺寸的 item
/// 直接跳过——不让 tiles_total 估算失败拖累入队本身，只是该 item 不计入估算基线）。
fn item_dims(state: &AppState, item_ids: &[i64]) -> Vec<(u32, u32)> {
    let Ok(conn) = state.db_read_pool.get() else {
        return Vec::new();
    };
    let mut dims = Vec::with_capacity(item_ids.len());
    for &id in item_ids {
        if let Ok(detail) = q::get_media_detail(&conn, id) {
            let (w, h) = (detail.item.width, detail.item.height);
            if w > 0 && h > 0 {
                dims.push((w as u32, h as u32));
            }
        }
    }
    dims
}

/// 单 item 沿固定序 steps 链的瓦片数（几何模拟，不推理）：逐 step 累加 `plan_tiles(...).len()`，
/// 每步后按该档 `scale` 叠乘尺寸——与 `scrollery-ai-core::enhance::chain` 预算总瓦片数同源公式。
/// 未知模型档（理论不可达，档位在 UI/校验层已锁定）直接跳过该 step 的贡献。
fn tiles_for_item(sorted_steps: &[EnhanceStep], w: u32, h: u32) -> u32 {
    let (mut sw, mut sh) = (w, h);
    let mut total = 0u32;
    for step in sorted_steps {
        let Some(p) = find_enhance_profile(&step.model_id) else {
            continue;
        };
        let n = plan_tiles(sw, sh, p.tile, p.tile_pad, p.scale).len() as u32;
        total = total.saturating_add(n);
        sw = sw.saturating_mul(p.scale);
        sh = sh.saturating_mul(p.scale);
    }
    total
}

/// 整 job 的估算总瓦片数：各 item 独立跑链、求和。`dims` 为空（全 item 尺寸未知）→ 0，
/// 前端据此回退不定长进度态。
pub fn estimate_tiles_total(sorted_steps: &[EnhanceStep], dims: &[(u32, u32)]) -> u32 {
    dims.iter()
        .map(|&(w, h)| tiles_for_item(sorted_steps, w, h))
        .fold(0u32, |a, b| a.saturating_add(b))
}

fn resolve_output_format(
    choice: OutputFormatChoice,
    src_ext: &str,
) -> (&'static str, &'static str) {
    match choice {
        OutputFormatChoice::Jpeg => ("jpeg", "jpg"),
        OutputFormatChoice::Png => ("png", "png"),
        OutputFormatChoice::FollowSource => {
            if src_ext == "png" {
                ("png", "png")
            } else {
                ("jpeg", "jpg")
            }
        }
    }
}

/// 构造稳定 enhance 错误。
fn eerr(code: &'static str, msg: impl Into<String>) -> AppError {
    AppError::Enhance {
        code,
        message: msg.into(),
    }
}

/// 增强推理是否走 GPU：复用 AI 硬件策略键 `ai_provider_override`（auto→GPU、cpu→强制 CPU）。
/// GPU 与否决定 fp16/fp32 档选择（design.md §E；task a）。
fn enhance_prefers_gpu(state: &AppState) -> bool {
    state.config.get("ai_provider_override").as_deref() != Some("cpu")
}

/// 同步准入用（深审 g，批 5 复核）：某 item 是否 RAW（catalog 单源）。DB 读失败/源缺失
/// 一律按「非 RAW」放行——真正的缺失/不可用留异步链脱敏处理，本判据只挡 RAW 入队产生死 job。
pub fn item_is_raw(state: &AppState, item_id: i64) -> bool {
    let Ok(conn) = state.db_read_pool.get() else {
        return false;
    };
    let Ok(detail) = q::get_media_detail(&conn, item_id) else {
        return false;
    };
    is_raw_format(state, Path::new(&detail.item.file_name))
}

/// 该路径扩展名是否属 RAW（单源：catalog `exotic-image-raw` offering，经 `resolve_format`
/// 反查插件 id；RAW 格式清单不在此手抄，深审 g）。
fn is_raw_format(state: &AppState, path: &Path) -> bool {
    let Some(ext) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
    else {
        return false;
    };
    state
        .exotic_host()
        .resolve_format(&ext)
        .plugin_id
        .as_deref()
        == Some(RAW_PLUGIN_ID)
}

/// 单模型描述符：`fp16_safe && gpu → fp16` 否则 `fp32`（CPU 与 SCUNet 等不安全档走 fp32，
/// spike-D/task a）。role 恒 Enhance，model_id 必填（worker 靠它寻址具体模型）。
fn enhance_model_descriptor(
    models_dir: &Path,
    model_id: &str,
    gpu: bool,
) -> Result<ModelDescriptor, AppError> {
    let profile = find_enhance_profile(model_id)
        .ok_or_else(|| eerr("enhance_invalid_params", "未知增强模型档位"))?;
    let file = if gpu && profile.fp16_safe {
        &profile.file_fp16
    } else {
        &profile.file_fp32
    };
    let path = models_dir.join(file);
    if !super::registry::enhance_manifest_ready(model_id) {
        return Err(eerr("enhance_manifest_unready", "增强模型发行清单尚未就绪"));
    }
    let asset = super::registry::enhance_assets(model_id)
        .and_then(|assets| assets.into_iter().find(|asset| asset.dest == *file))
        .ok_or_else(|| eerr("enhance_manifest_unready", "增强模型发行清单尚未就绪"))?;
    // 期望值必须来自发行清单；对待校验文件现算哈希会使篡改后的文件也通过 worker 校验。
    let sha256 = asset
        .sha256
        .ok_or_else(|| eerr("enhance_manifest_unready", "增强模型发行清单尚未就绪"))?;
    Ok(ModelDescriptor {
        role: ModelRole::Enhance,
        handle: ModelHandle::Path(path.to_string_lossy().into_owned()),
        len: asset.size_bytes,
        sha256,
        model_id: Some(model_id.to_string()),
    })
}

/// 发一个 enhance 请求（硬止损重试一次）：进程级异常 → 弃实例重建重发；retryable Failure →
/// 退避重发；terminal Failure → 映射稳定码直返。
///
/// `on_progress`：per-tile 进度回调（深审 b；仅 EnhanceRun 传，其余传 None）。
/// `session_restore`：`Some((init_req, init_timeout))` 时——worker 若已重建致会话失效
/// （`!handle.session_alive`），重发本请求前先补发 `init_req` 重建会话（深审 h；仅绑定会话的
/// EnhanceRun 传，init 本身传 None 避免自递归）。
fn send_request(
    handle: &mut WorkerHandle,
    req: &RequestBody,
    timeout: Duration,
    token: &CancellationToken,
    mut on_progress: Option<&mut dyn FnMut(&ProgressBody)>,
    session_restore: Option<(&RequestBody, Duration)>,
) -> Result<SuccessBody, AppError> {
    let mut last: Option<String> = None;
    for _attempt in 1..=MAX_ATTEMPTS {
        // 深审 h：会话随 worker 重建而失效，绑定会话的请求重发前先补发 init。
        if let Some((init_req, init_to)) = session_restore {
            if !handle.session_alive {
                let sup = handle.ensure()?;
                let outcome = sup.run_request(init_req, init_to, &|| token.is_cancelled());
                match outcome {
                    RawOutcome::Success { .. } => handle.session_alive = true,
                    RawOutcome::Failure(fb) => return Err(map_worker_failure(&fb.code)),
                    RawOutcome::TimedOut | RawOutcome::Disconnected | RawOutcome::Protocol(_) => {
                        if token.is_cancelled() {
                            return Err(eerr("enhance_io", "已取消"));
                        }
                        last = Some("会话重建失败".into());
                        handle.drop_worker();
                        continue;
                    }
                }
            }
        }

        // 每轮独立 reborrow（`&mut dyn` 直接跨迭代传会被借用检查判为长借用）。
        let progress: Option<&mut dyn FnMut(&ProgressBody)> = match on_progress {
            Some(ref mut cb) => Some(&mut **cb),
            None => None,
        };
        let sup = handle.ensure()?;
        match sup.run_request_with_progress(req, timeout, &|| token.is_cancelled(), progress) {
            RawOutcome::Success { body, .. } => return Ok(body),
            RawOutcome::Failure(fb) if fb.retryable && _attempt < MAX_ATTEMPTS => {
                last = Some(fb.message);
                std::thread::sleep(RETRY_BACKOFF);
            }
            RawOutcome::Failure(fb) => return Err(map_worker_failure(&fb.code)),
            RawOutcome::TimedOut | RawOutcome::Disconnected | RawOutcome::Protocol(_) => {
                if token.is_cancelled() {
                    return Err(eerr("enhance_io", "已取消"));
                }
                last = Some("worker 进程异常".into());
                handle.drop_worker(); // run_request 已 kill；此处清句柄，下轮 ensure 重建
            }
        }
        if token.is_cancelled() {
            return Err(eerr("enhance_io", "已取消"));
        }
    }
    let _ = last;
    Err(eerr("enhance_io", "增强 worker 多次尝试均失败"))
}

/// worker terminal Failure 码 → 稳定 enhance 码。
fn map_worker_failure(code: &WorkerErrorCode) -> AppError {
    match code {
        WorkerErrorCode::ModelLoadFailed => {
            eerr("enhance_model_missing", "增强模型加载失败，请重新下载")
        }
        WorkerErrorCode::MalformedInput | WorkerErrorCode::IoError => {
            eerr("enhance_input_unsupported", "源图读取或解码失败")
        }
        WorkerErrorCode::ResourceLimit => {
            eerr("enhance_io", "显存/资源不足，建议改用 CPU 或缩小图像")
        }
        _ => eerr("enhance_io", "增强处理失败"),
    }
}

/// 解析 enhance-worker 可执行文件：`PICASA_ENHANCE_WORKER_PATH` 覆盖（dev/test），
/// 否则取主程序同目录（照 `ai::worker_client::ai_worker_exe` 姿态）。
fn enhance_worker_exe() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PICASA_ENHANCE_WORKER_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!(
            "PICASA_ENHANCE_WORKER_PATH 指向的文件不存在:{}",
            p.display()
        ));
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe 失败:{e}"))?;
    let dir = exe.parent().ok_or("current_exe 无父目录")?;
    let p = dir.join(format!("enhance-worker{}", std::env::consts::EXE_SUFFIX));
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("enhance-worker 可执行文件不存在:{}", p.display()))
    }
}

/// 状态查询与执行共用解析路径。仅表达组件文件存在，启动/握手失败另走执行错误。
pub fn enhance_worker_ready() -> bool {
    enhance_worker_exe().is_ok()
}

/// spawn enhance-worker（握手校验 worker_id/能力）。
fn spawn_enhance_worker() -> Result<WorkerSupervisor, String> {
    let spec = WorkerSpec {
        exe_path: enhance_worker_exe()?,
        expected_worker_id: "enhance-worker".to_string(),
        required_capabilities: vec![capability::ENHANCE.to_string()],
    };
    let cfg = WorkerConfig {
        handshake_timeout: Duration::from_secs(5),
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        max_blob_len: MAX_BLOB_LEN,
    };
    WorkerSupervisor::spawn(&spec, &cfg)
}

// ── 容器级 EXIF 注入（不重编码，best-effort）──────────────────────────────────

/// 读 worker 产物 → 注入源 EXIF（DateTimeOriginal + orientation=1）→ 写 `{claimed}.tmp` →
/// 同卷 rename 覆盖 `claimed` 占位。注入失败或格式未知 → 写 worker 原字节（best-effort，
/// 不因元数据缺失而失败整个增强）。
fn finalize_output(
    worker_tmp: &Path,
    claimed: &Path,
    source: &Path,
    out_ext: &str,
) -> Result<(), AppError> {
    let bytes = std::fs::read(worker_tmp).map_err(|_| {
        let _ = std::fs::remove_file(claimed);
        eerr("enhance_io", "读取增强产物失败")
    })?;
    let final_bytes = match build_source_exif(source) {
        Some(exif) => inject_exif_container(&bytes, out_ext, &exif),
        None => bytes,
    };

    let tmp_name = format!(
        "{}.tmp",
        claimed
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("enh")
    );
    let tmp = claimed.with_file_name(tmp_name);
    let _ = std::fs::remove_file(&tmp);
    let write: std::io::Result<()> = (|| {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&final_bytes)?;
        f.sync_all()
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(claimed);
        return Err(eerr("enhance_io", "增强产物落盘失败"));
    }
    if std::fs::rename(&tmp, claimed).is_err() {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(claimed);
        return Err(eerr("enhance_io", "增强产物改名落盘失败"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(task: EnhanceTask, model_id: &str, strength: Option<f32>) -> EnhanceStep {
        EnhanceStep {
            task,
            model_id: model_id.to_string(),
            strength,
        }
    }

    #[test]
    fn worker_handle_drop_marks_session_dead() {
        // 深审 h：worker 丢弃 = 会话失效标记（send_request 据此在重发前补发 init）。
        let mut h = WorkerHandle {
            sup: None,
            next_session_id: 1,
            session_alive: true,
        };
        h.drop_worker();
        assert!(!h.session_alive, "worker 丢弃后会话必标失效");
    }

    #[test]
    fn steps_sorted_to_fixed_order_denoise_dejpeg_upscale() {
        let unordered = vec![
            step(EnhanceTask::Upscale, "realesrgan-x4plus", None),
            step(EnhanceTask::Denoise, "scunet", None),
            step(EnhanceTask::DejpegArtifact, "fbcnn", None),
        ];
        let sorted = sort_steps(unordered);
        assert_eq!(sorted[0].task, EnhanceTask::Denoise);
        assert_eq!(sorted[1].task, EnhanceTask::DejpegArtifact);
        assert_eq!(sorted[2].task, EnhanceTask::Upscale);
    }

    #[test]
    fn strength_nan_and_inf_rejected() {
        assert!(
            validate_strengths(&[step(EnhanceTask::Denoise, "drunet", Some(f32::NAN))]).is_err()
        );
        assert!(
            validate_strengths(&[step(EnhanceTask::Denoise, "drunet", Some(f32::INFINITY))])
                .is_err()
        );
        assert!(validate_strengths(&[step(EnhanceTask::Denoise, "drunet", Some(25.0))]).is_ok());
        assert!(validate_strengths(&[step(EnhanceTask::Denoise, "drunet", None)]).is_ok());
    }

    #[test]
    fn admission_direct_100mp_boundary() {
        // 降噪/去伪影（max_scale=1）：≤100MP 放行，>100MP 拒。
        assert!(admission_check(10_000, 10_000, 1).is_ok()); // 100MP
        assert!(admission_check(10_001, 10_000, 1).is_err());
    }

    #[test]
    fn admission_upscale_4x_16mp_boundary() {
        // 4x 超分：输入 ≤16MP 放行，>16MP 拒。
        assert!(admission_check(4_000, 4_000, 4).is_ok()); // 16MP
        assert!(admission_check(4_001, 4_000, 4).is_err());
    }

    #[test]
    fn output_format_follow_source_maps_png_and_defaults_jpeg() {
        assert_eq!(
            resolve_output_format(OutputFormatChoice::FollowSource, "png"),
            ("png", "png")
        );
        assert_eq!(
            resolve_output_format(OutputFormatChoice::FollowSource, "jpg"),
            ("jpeg", "jpg")
        );
        assert_eq!(
            resolve_output_format(OutputFormatChoice::FollowSource, "tiff"),
            ("jpeg", "jpg")
        );
        assert_eq!(
            resolve_output_format(OutputFormatChoice::Png, "jpg"),
            ("png", "png")
        );
    }

    #[test]
    fn enhanced_stem_suffix() {
        // {stem}-enhanced 命名（naming.rs claim 复用；此处锁 stem 构造）。
        let src_stem = Path::new("photo.jpg")
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        let stem = format!(
            "{}-enhanced",
            crate::export::naming::sanitize_component(src_stem)
        );
        assert_eq!(stem, "photo-enhanced");
    }

    #[test]
    fn estimate_tiles_total_sums_across_items_and_chains_scale() {
        // 批 5.5：估算须与 worker chain.rs 同源公式——单 step 时即 plan_tiles(...).len() 之和；
        // 未知尺寸（空 dims）时估算为 0（前端回退不定长）。
        let steps = vec![step(EnhanceTask::Denoise, "scunet", None)];
        let one = tiles_for_item(&steps, 1000, 1000);
        assert!(one > 0, "已知尺寸应估出非零瓦片数");
        let two_items = estimate_tiles_total(&steps, &[(1000, 1000), (1000, 1000)]);
        assert_eq!(two_items, one * 2, "两 item 应为单 item 估算之和");
        assert_eq!(estimate_tiles_total(&steps, &[]), 0, "无已知尺寸 → 估算 0");
    }

    #[test]
    fn estimate_tiles_total_unknown_model_id_skipped_not_panicking() {
        // 未知档位（理论不可达，UI/校验层已锁定）直接跳过，不 panic、不计入。
        let steps = vec![step(EnhanceTask::Denoise, "no-such-model", None)];
        assert_eq!(estimate_tiles_total(&steps, &[(1000, 1000)]), 0);
    }
}
