// src-tauri/src/ai/face_pipeline/mod.rs
//! Background face-recognition pipeline (F3) — worker 派发架构(T16 收束)。
//! 后台人脸识别流水线:推理恒经 ai-worker 子进程派发。
//!
//!   Producer → 攒批 → CPU permit → 三级定源(缩略图档位 → face 缓存 640 → 小原图直派)
//!   → 缺缓存现场预解码(T16-R2 方案 A,镜像 CLIP T18)→ GPU 令牌(D2 顺序)
//!   → FaceDetectEmbed → faces_to_records 映射 → Writer
//!
//! 1. Producer:批量查询 face_status=0 的 media_items,标记 Processing,发 FaceTask。
//! 2. 派发线程:**worker 端只解小图**——缩略图档位(预测短边 ≥ detect_size)或 host 预解码
//!    的 face 缓存(短边 640 WebP,WIC 优先产出,exotic 原图也在覆盖内);仅短边本就 ≤640
//!    的小原图直派 worker 解码(白名单格式)。预解码失败(双引擎都解不开)标 Error,与
//!    CLIP T18 现场派生失败同语义。几何按协议回报的**实际解码尺寸**归一化
//!    (FaceItemResult::Ok.width/height),不用 host 预测尺寸。
//! 3. Writer:批量收集 FaceResult,成功项先删后插+置 Done(batch_finish_face_items,
//!    X1 条件写:cache_key 已换的失效项跳过),小批落库(问题2 进度平滑)、大批跑增量
//!    聚类;零脸图也是成功(Done 非 Error)。
//!
//! 与 CLIP 分析共用 F5 GPU 分析槽(互斥);让步复用 ai_yield_blockers()。
//! 进程内推理路径(engine 快照/rayon 预处理/detect+embed 线程)已随 T16 删除,
//! 历史实现见 git。
//!
//! # 解码源:不能复用 CLIP 的 ai_cache
//! CLIP 的 ai_cache 固定短边 336px,对 YuNet 的 640px 输入太小,会悄悄损害小脸召回率;
//! face 自持一份 `face_thumbs/`(短边 640,FACE_CACHE_SHORT_EDGE)。
//! resolve_face_decode_source 先在「常规分档缩略图」与「原图」间选择,原图回退项再由
//! 派发批升级为 face 缓存(决策核见 face_cache_applies,装配见 dispatch_face_batch)。
//!
//! # 模块拆分(tierB-4 简案)
//! 四阶段管线按文件切分:`producer`(Producer)/`decode_source`(三级定源)/
//! `dispatch`(worker 派发)/`writer`(Writer)。`FaceTask`/`FaceResult` 跨阶段共享,
//! 定义留本文件;编排(`rayon::scope` 派三线程)也留本文件。

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::bounded;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ai::face_profile::FaceProfile;
use crate::db::queries::{count_pending_face_items, reset_processing_face_items, NewFace};
use crate::error::AppError;
use crate::state::AppState;

mod decode_source;
mod dispatch;
mod producer;
mod writer;

/// 从数据库读取、及写入器聚类刷新的批次大小。
const BATCH_SIZE: i64 = 512;

/// 各阶段之间的通道容量。
const CHANNEL_CAPACITY: usize = 1024;

/// 从生产者发送到预处理器的任务项。
struct FaceTask {
    item_id: i64,
    source_path: PathBuf,
    file_format: String,
    thumb_status: i64,
    thumb_path: Option<String>,
    width: i64,
    height: i64,
    /// 缩略图/派生缓存键(xxh3(路径+mtime),兼陈旧防护)——face 缓存(方案 A)按此寻址。
    cache_key: i64,
}

/// 从检测+嵌入阶段（或预处理早期失败）发送到写入器的结果。
struct FaceResult {
    item_id: i64,
    /// 任务领取时的 `cache_key` 快照(X1 条件写:落库仅当行内当前值仍相等)。
    cache_key: i64,
    /// `Some(rows)` on success — possibly empty (zero-face image, still Done).
    /// `None` on decode/detect/embed failure → Error.
    /// 成功时为 `Some(rows)`——可能为空（零脸图，仍算 Done）。解码/检测/嵌入失败为 `None` → Error。
    records: Option<Vec<NewFace>>,
}

/// 启动后台人脸识别流水线。
///
/// 立即返回；所有工作在后台线程中完成。
pub fn start_face_pipeline(state: Arc<AppState>, generation: u64, token: CancellationToken) {
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        // span 埋点(W1,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入):覆盖整个 run
        // 的墙钟时间,块尾自然 Drop——正常完成/失败/panic 三条路径都会触达。既有的完成/失败/
        // panic 汇总日志(下方 info!/warn!)保留不动。
        let _span = crate::logging::SpanTimer::info("pipeline:face");
        let start_time = std::time::Instant::now();
        // 保留一个 token 句柄，使完成回调能区分自然完成与暂停/停止/重启取消（驱动下面 GPU 槽位释放决策）。
        let token_outer = token.clone();
        let result =
            tokio::task::spawn_blocking(move || run_face_pipeline_blocking(&state_clone, &token))
                .await;

        let elapsed_ms = start_time.elapsed().as_millis();
        match result {
            Ok(Ok(())) => info!(
                "Face pipeline completed: elapsed={}ms | 人脸流水线完成: 耗时={}ms",
                elapsed_ms, elapsed_ms
            ),
            Ok(Err(e)) => warn!("Face pipeline error | 人脸流水线错误: {}", e),
            Err(e) => warn!("Face pipeline task panicked | 人脸流水线任务崩溃: {}", e),
        }

        // 仅在自然完成时释放共享 GPU 分析槽（F5 互斥；同 pipeline.rs 的理由——取消由命令释放
        // 或 restart 须保持持有）。判定方式与 set_config 续传标志一致：未被取消即自然完成。
        if !token_outer.is_cancelled() {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = crate::db::queries::set_config(&conn, "face_analysis_active", "0");
            drop(conn);
            state.release_gpu_analysis(crate::state::GPU_OWNER_FACE);
        }

        // 不卸载共享的 ai-worker 子进程（与 CLIP 分析共用,空闲 300s 自杀兜底）。仅按代次清空
        // 本流水线自己的令牌(2026-07-10 审查 F10 compare-and-clear:旧轮迟退出不得误杀
        // restart 刚装的新一轮,理由同 pipeline.rs)。返回值刻意丢弃:终态副作用门控在上面的
        // `!token_outer.is_cancelled()`,不采用 thumb 的 finish-bool 门控姿态。
        let _ = state.finish_face_analysis(generation);
    });
}

/// Blocking pipeline runner:T16 起恒走 worker 派发(进程内 ort 路径已删)。
fn run_face_pipeline_blocking(
    state: &Arc<AppState>,
    token: &CancellationToken,
) -> crate::error::Result<()> {
    crate::ai::runtime_config::warn_legacy_ai_backend(state);
    run_face_pipeline_worker_blocking(state, token)
}

/// 续传支持(问题7):把上次运行已领取但未完成的项(face_status=Processing)放回 Pending——
/// 镜像 `reset_processing_ai_items`;进程内与 worker 派发两路径共用。
fn recover_orphaned_face_items(state: &Arc<AppState>) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    match reset_processing_face_items(&conn) {
        Ok(n) if n > 0 => info!(
            "Recovered {} orphaned face items (processing → pending) | 恢复 {} 个孤儿人脸项（处理中 → 待处理）",
            n, n
        ),
        Ok(_) => {}
        Err(e) => warn!(
            "Failed to recover orphaned face items | 恢复孤儿人脸项失败: {}",
            e
        ),
    }
}

/// F3 孤儿对账(2026-07-10 审查):把「face_status=Done 但脸未归属」的项补跑一次增量聚类。
/// 孤儿来源=硬崩溃丢 cluster_pending(脸行 16 项小批已落库、聚类 512 项大批未跑);增量
/// 聚类只查本轮 item_ids,这些脸此前**永不再被聚**,唯一救济是重排全部未确认归属的全量
/// recluster(杀鸡用牛刀)。判别位与负样本谓词保证不吸回「用户移出 / 被拒 / 低质量」脸
/// (见 get_orphan_cluster_item_ids / get_clusterable_faces),幂等:归簇后不再命中。
/// 对齐缩略图线 6ff2841「事件驱动复位 + 启动兜底」的精神(此处只需启动兜底半边)。
fn reconcile_unclustered_faces(state: &Arc<AppState>, profile: &FaceProfile) {
    let (threshold, min_quality) = crate::ai::face_cluster::effective_thresholds(state, profile);
    let ids = {
        let conn = match state.db_read_pool.get() {
            Ok(c) => c,
            Err(e) => {
                warn!("DB pool error in orphan reconcile | 孤儿对账 DB 池错误: {e}");
                return;
            }
        };
        match crate::db::queries::get_orphan_cluster_item_ids(&conn, &profile.id, min_quality) {
            Ok(v) => v,
            Err(e) => {
                warn!("Orphan face scan failed | 孤儿脸扫描失败: {e}");
                return;
            }
        }
    };
    if ids.is_empty() {
        return;
    }
    info!(
        "Face 孤儿对账:{} 项存在已分析未聚类脸,补跑增量聚类 | reconciling unclustered faces",
        ids.len()
    );
    for chunk in ids.chunks(BATCH_SIZE as usize) {
        crate::ai::face_cluster::cluster_new_faces(
            state,
            chunk,
            &profile.id,
            threshold,
            min_quality,
        );
    }
}

/// worker 派发路径主入口(由 run_face_pipeline_blocking 无条件调用;T16 后为唯一路径)。
/// Producer/Writer 与进程内共用;中段 = 单派发线程攒批 → FaceDetectEmbed(解码在 worker)。
fn run_face_pipeline_worker_blocking(
    state: &Arc<AppState>,
    token: &CancellationToken,
) -> crate::error::Result<()> {
    // profile 纯由配置解析(零进程内引擎):enabled 门与激活轨语义与引擎加载完全同源。
    let Some(face_profile) = crate::ai::runtime_config::active_face_profile(state) else {
        return Err(AppError::Internal(
            "人脸功能未启用(face_enabled=0)或无激活轨".into(),
        ));
    };
    let clip_profile = crate::ai::runtime_config::active_profile(state);
    let spec = crate::ai::worker_client::build_session_spec(
        state,
        clip_profile,
        Some(face_profile.clone()),
    );
    let profile = Arc::new(face_profile);

    recover_orphaned_face_items(state);
    // V17 启动自愈(2026-07-11 加固批 B-3,X2 动机的正版承接):以 face_coverage 记账
    // 对齐全局 face_status——有账无标(A3/F11 竞态误置)补 Done,有标无账(误标/换轨
    // 残留)归 0 重扫。零脸图**有账**,不复 X2 时代「每次启动全量重扫无脸图」的病灶;
    // 分批零写幂等,放孤儿恢复(Processing→Pending)之后使两类修正互不遮蔽。
    if let Err(e) = crate::db::queries::sync_face_status_for_model(&state.db_writer, &profile.id) {
        warn!("face 覆盖自愈 sync 失败(不阻断启动): {e}");
    }
    // F7 兜底:凡 face_count 与实际引用不符的 person 按库内真相补算(直接删除路径已即时
    // 对账,此处幂等捕获任何绕过封装的历史/旁路删除)。
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        match crate::db::queries::reconcile_person_face_counts(&conn) {
            Ok(0) => {}
            Ok(n) => info!("Person 计数对账:修复 {n} 个陈旧 person 派生字段"),
            Err(e) => warn!("Person 计数对账失败: {e}"),
        }
    }
    reconcile_unclustered_faces(state, &profile);

    let read_conn = state.db_read_pool.get()?;
    let total = count_pending_face_items(&read_conn)?;
    drop(read_conn);
    info!(
        "Face worker 流水线启动:待分析 {total} 张(backend=worker, face={}, batch={})",
        profile.id,
        dispatch::face_dispatch_cap(spec.batch_size)
    );

    let (task_tx, task_rx) = bounded::<FaceTask>(CHANNEL_CAPACITY);
    let (result_tx, result_rx) = bounded::<FaceResult>(CHANNEL_CAPACITY);

    // 派发线程的批级致命错误经此带出 scope(同 CLIP worker 派发的手法)。
    let fatal: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

    let token_prod = token.clone();
    let state_prod = Arc::clone(state);
    let token_writer = token.clone();
    let state_writer = Arc::clone(state);
    let profile_writer = Arc::clone(&profile);

    rayon::scope(|s| {
        s.spawn(|_| {
            producer::produce_face_tasks(&state_prod, task_tx, &token_prod);
        });

        let state_disp = Arc::clone(state);
        let token_disp = token.clone();
        let fatal_ref = &fatal;
        let spec_ref = &spec;
        let profile_disp = Arc::clone(&profile);
        s.spawn(move |_| {
            if let Err(e) = dispatch::face_dispatch_loop(
                &state_disp,
                spec_ref,
                &profile_disp,
                task_rx,
                result_tx,
                &token_disp,
            ) {
                *fatal_ref.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
            }
        });

        s.spawn(move |_| {
            writer::write_face_results(&state_writer, result_rx, &token_writer, &profile_writer);
        });
    });

    // provider 回声落库(T16)须在 close_session 之前——快照随 close 清空。
    crate::ai::runtime_config::persist_provider_echo(state);
    // 结束即卸会话(自然完成/取消皆是;对齐 CLIP worker 派发「运行结束即 close_session」)。
    state
        .ai_worker
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .close_session();

    match fatal.into_inner().unwrap_or_else(|p| p.into_inner()) {
        Some(e) => Err(AppError::internal(
            "Face worker 派发终止 | dispatch failed",
            e,
        )),
        None => Ok(()),
    }
}
