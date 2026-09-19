//! AI 推理引擎管理和语义搜索的 IPC 命令。

use std::sync::Arc;

use tauri::State;
use tracing::{info, warn};

use crate::ai::pipeline::start_ai_pipeline;
use crate::ai::profile::{self};
use crate::ai::remote_registry::{self, BatchKind};
use crate::ai::runtime_config::{
    active_profile, active_profile_with, models_dir, persist_provider_echo, variant_installed,
};
use crate::db::models::AiStatusSummary;
use crate::db::queries::{
    count_embeddings_for_model, count_error_ai_items, count_total_ai_items, get_config,
    reset_ai_embeddings, reset_error_ai_items, set_config, sync_ai_status_for_model,
};
use crate::error::{AppError, Result};
use crate::ipc::model_download::{download_assets, DownloadProgress};
use crate::state::AppState;

// ── 辅助函数 ──────────────────────────────────────────────────────────────────
// 跨层共享的运行时配置解析(models_dir/active_profile*/active_face_profile*/
// variant_installed/persist_provider_echo)已下沉
// `ai::runtime_config`(U-P2-a):它们是 ai 核心层与多个 IPC 文件的共享底座,
// 留在单一命令文件构成层次倒挂。本文件只保留命令私有 helper。

/// tokio `spawn_blocking` 的 JoinError（后台阻塞任务 panic 或被取消）统一归为内部错误，
/// 使所有 AI 命令的「任务调度失败」走同一稳定 code（Internal），而非各自拼裸字符串丢给前端。
fn join_err(e: tokio::task::JoinError) -> AppError {
    AppError::internal("后台任务异常 | blocking task failed", e)
}

/// 破坏性重置嵌入(重启分析 / 重建嵌入)的收尾契约(P1-3)。
///
/// `reset_ai_embeddings` 是**分批** DELETE + UPDATE 而非一整笔事务(db/queries/ai.rs),
/// 后续批失败会留下「已删掉的向量 + 陈旧常驻快照 + 仍有效的旧在途票」。因此重置的结果先
/// 存下来、不做 `?` 提前返回:无论重置成败,吊销在途请求、作废常驻快照、代次守卫擦库都
/// 必须执行,最后才把重置的错误传播给调用方。这样崩溃/失败后重跑不会让旧向量空间的排名
/// 继续可见,也不会让迟到的旧查询把结果写回。
fn reset_embeddings_with_search_teardown(
    db_writer: &crate::db::DbWriter,
    search: &crate::ai::search_control::SearchControl,
    model_id: &str,
) -> Result<()> {
    let reset = reset_ai_embeddings(db_writer, model_id);

    let ticket = search.begin_clear();
    search.invalidate_cache();
    if let Err(e) = search.clear_if_current(&ticket, || {
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
        crate::ai::search::wipe_search_results(&conn)
    }) {
        warn!("Search results wipe failed on embedding reset (revocation applied) | 重置嵌入时擦除搜索结果失败(在途请求已吊销): {e}");
    }

    reset
}

/// 由图像变体文件名反查所属架构元数据。`.img.` 前缀 + fp16/fp32 标记唯一确定架构
/// （两个 B/16 同前缀 `vit-b-16`，靠 fp16/fp32 区分）。
fn arch_for_image_file(image_file: &str) -> Option<profile::ArchMeta> {
    let prefix = image_file.split(".img.").next();
    let fp16 = image_file.contains(".fp16.");
    profile::arch_metas()
        .into_iter()
        .find(|m| m.default_image_file.split(".img.").next() == prefix && m.fp16 == fp16)
}

/// 图像变体的固定 batch `k`（>1），动态/单批返回 `None` —— 用于「设置 batch 不得 < k」约束与自动 batch 兜底。
fn variant_fixed_batch(image_file: &str) -> Option<u32> {
    match remote_registry::parse_batch(image_file) {
        Some(BatchKind::Fixed(k)) if k > 1 => Some(k),
        _ => None,
    }
}

/// 变体文件未就位 → 稳定 code `AiModelNotLoaded` + 可操作的下载指引。供命令与单测共用。
fn require_variant_installed(
    models: &std::path::Path,
    image_file: &str,
    text_file: &str,
) -> Result<()> {
    if variant_installed(models, image_file, text_file) {
        return Ok(());
    }
    Err(AppError::AiModelNotLoaded(format!(
        "AI 模型尚未下载，请先在「设置 → AI → 模型库」下载 {} | AI model not downloaded, download it in Settings → AI → Model Library: {}",
        image_file, image_file
    )))
}

/// 激活 CLIP 模型的必需文件是否已就位（复用安装判定）。缺失即拒：流水线不启动、向量不重置、
/// GPU 分析槽不占用——此前缺失只在 worker 派发线程里退化成一条日志，前端静默无感。
fn require_active_clip_assets(state: &AppState) -> Result<()> {
    let profile = active_profile(state);
    require_variant_installed(&models_dir(state), &profile.image_file, &profile.text_file)
}

/// get_status 的「资源等待」原因键（P1-1）：本端未运行、仍有剩余，而共享 GPU 分析会话被对端
/// 持有。与前端 `useAnalysisController` 的 `ANALYSIS_BUSY_WAITING_KEY` 同值；两侧流水线
/// （语义分析 / 人脸）共用这一套等待原因词汇，故在此定义、face_commands 复用，避免字面量漂移。
pub const ANALYSIS_BUSY_WAITING_KEY: &str = "analysisBusy";

/// 共享 GPU 分析会话当前是否被**对端**（人脸）持有——本端未运行时的等待原因判定。
fn other_analysis_holds_gpu_session(state: &AppState) -> bool {
    // 毒锁恢复(与全库 gpu_analysis_owner 访问点一致):门闩只是 Option<&'static str>.
    *state
        .gpu_analysis_owner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        == Some(crate::state::GPU_OWNER_FACE)
}

// ── 命令 ──────────────────────────────────────────────────────────────────────

/// 返回最近一次 worker 会话回声的 provider/GPU(T16:探测发生在 worker SessionInit,
/// 本命令不再触发模型加载;冷启动未回声前为配置旧值/空)。
///
/// Returns: `{ provider: string, gpuName: string, clipLoaded: bool }`
#[tauri::command]
pub async fn detect_ai_provider(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let state = Arc::clone(&state);

    tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let conn = state.db_read_pool.get()?;
        let provider = get_config(&conn, "ai_provider")
            .unwrap_or_default()
            .unwrap_or_default();
        let gpu_name = get_config(&conn, "ai_gpu_name")
            .unwrap_or_default()
            .unwrap_or_default();
        drop(conn);
        let session_live = {
            let client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
            client.session().is_some()
        };
        Ok(serde_json::json!({
            "provider": provider,
            "gpuName":  gpu_name,
            "clipLoaded": session_live,
        }))
    })
    .await
    .map_err(join_err)?
}

/// 获取 UI 状态栏所需的综合 AI 状态。
#[tauri::command]
pub async fn get_ai_status(state: State<'_, Arc<AppState>>) -> Result<AiStatusSummary> {
    let state = Arc::clone(&state);

    tokio::task::spawn_blocking(move || -> Result<AiStatusSummary> {
        let conn = state.db_read_pool.get()?;

        let provider = get_config(&conn, "ai_provider")
            .unwrap_or_default()
            .unwrap_or_default();
        let gpu_name = get_config(&conn, "ai_gpu_name")
            .unwrap_or_default()
            .unwrap_or_default();

        // B-1:全程只用头上这一个 conn(active_profile/resolve_batch_size 曾各自再取池,
        // 状态轮询在读池紧张时自体饿死——它本该是诊断饥饿的观测面)。
        // A2:两键均已迁往 config.toml,改传 `&state.config`(不再需要 conn)。
        let active_prof = active_profile_with(&state.config);
        let active_model = active_prof.id.clone();
        // 当前图像变体若是固定 batch（k>1），向前端暴露 k 以驱动「设置 batch 不得 < k」约束。
        let active_fixed_batch = variant_fixed_batch(&active_prof.image_file);
        let total_items = count_total_ai_items(&conn).unwrap_or(0);
        // 搜索只依赖 ai_embeddings；Error 状态没有向量，不能算“可搜索的已分析”。
        let analyzed_items = count_embeddings_for_model(&conn, &active_model).unwrap_or(0);
        // A11:Error 项从 pending 拆出——此前被计入 pending,进度永远到不了 100% 且无解释。
        let error_items = count_error_ai_items(&conn).unwrap_or(0);
        let pending_items = total_items
            .saturating_sub(analyzed_items)
            .saturating_sub(error_items);

        let clip_loaded = {
            // T16:进程内引擎已删;「已加载」= worker 在载会话(SessionInit 后为真)。
            let client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
            client.session().is_some()
        };

        let is_analyzing = state.ai_analysis_token.is_running();

        // 让步阻塞源快照(可观测性三修 #2):仅当流水线正在跑时才有意义,否则空——
        // 避免「未运行」态下残留上一轮的阻塞源误导前端。
        // P1-1:未运行但「仍有剩余 + 共享 GPU 分析会话被对端持有」时也报等待原因——前端据此把
        // 「资源等待(本地已排队,对端释放即自动续跑)」与「用户手动暂停(不自动唤醒)」分开显示。
        let waiting_on: Vec<String> = if is_analyzing {
            state
                .ai_yield_blockers()
                .into_iter()
                .map(str::to_string)
                .collect()
        } else if pending_items > 0 && other_analysis_holds_gpu_session(&state) {
            vec![ANALYSIS_BUSY_WAITING_KEY.to_string()]
        } else {
            Vec::new()
        };

        // 跨运行/重启持久化的「期望运行」标志：开始/续传/暂停时置位，停止或自然完成时清除。
        // 驱动续传与自动续传（问题7）。
        let analysis_active = get_config(&conn, "ai_analysis_active")
            .unwrap_or_default()
            .map(|v| v == "1")
            .unwrap_or(false);

        let vram_bytes = crate::ai::provider::detect_vram_bytes();
        let vram_gb = vram_bytes.map(|b| (b / (1024 * 1024 * 1024)) as i64);

        // 有效 batch 统一走 resolve_batch_size(2026-07-10 审查 A2):此处曾用另一套 VRAM
        // 阶梯(≥8GB→64)算默认值并 set_config 落库——状态轮询必先于任何分析执行,自动档
        // (≥12GB→256/≥8GB→128)从此被钉死 64,批吞吐砍 3/4。读命令不得有写副作用;
        // 「0/缺省=自动」语义留在配置键里,本字段只报告解析后的有效值(含 256 上限与
        // 固定 batch 模型抬到 ≥k,均在 resolve_batch_size 内)。
        let batch_size =
            crate::ai::pipeline::resolve_batch_size_with(&state.config, &active_prof) as i64;

        Ok(AiStatusSummary {
            provider,
            gpu_name,
            vram_gb,
            batch_size,
            active_fixed_batch: active_fixed_batch.map(|k| k as i64),
            clip_loaded,
            total_items,
            analyzed_items,
            pending_items,
            error_items,
            is_analyzing,
            analysis_active,
            waiting_on,
        })
    })
    .await
    .map_err(join_err)?
}

/// 使用 Chinese-CLIP 文本编码器执行语义搜索。
///
/// 返回 `None` = 本次请求已被更新的查询、清空或切模型取代(未写结果集)。前端据此保持
/// 当前视图与 loading,不得当作「成功 0 结果」——那会把画廊刷成旧查询的布局。
#[tauri::command]
pub async fn semantic_search_cmd(
    query: String,
    limit: Option<usize>,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<usize>> {
    let state = Arc::clone(&state);
    let top_k = limit.unwrap_or(50).min(1000);

    // 入口登记(P1-3):代次与模型身份在进入阻塞池之前定下。active_profile 已是 ConfigManager
    // 纯内存读,放异步入口不阻塞;若拖进 spawn_blocking,线程池调度会让「先发起的请求」后登记
    // ——用户看到的次序就被调度运气改写。身份解析在控制面闸门内现读,切模型的配置写入与吊销
    // 之间那扇窗也漏不出旧模型的登记。
    // profile 与代次同一次解析取回:身份(向量主键)、维度与下文编码用的 spec 出自同一份快照,
    // 不会出现「按新模型登记、拿旧模型的维度装载」。
    let (ticket, prof) = state.ai_search.begin_request_with(|| {
        let prof = active_profile(&state);
        (prof.id.clone(), prof)
    });
    let dim = prof.embed_dim;

    tokio::task::spawn_blocking(move || -> Result<Option<usize>> {
        // 已被取代的请求就地退出:worker 编码是串行的(单一 ai_worker 锁),让一个已经作废的
        // 查询占住编码槽,只会把当前查询排在它后面。过期判据与提交点同源(控制面闸门)。
        if !state.ai_search.is_current(&ticket) {
            info!("Semantic search superseded before encode | 语义搜索在编码前已被更新的请求取代");
            return Ok(None);
        }
        // T16 收束:查询向量恒经 ai-worker 的 EncodeText op 生成(host 零 ort/tokenizers)。
        // 首次搜索会触发 SessionInit(模型冷加载,与旧进程内懒加载语义一致)。
        let spec = crate::ai::worker_client::build_session_spec(&state, prof.clone(), None);
        let mut vecs = {
            let mut client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
            client.encode_text(&spec, std::slice::from_ref(&query), &|| false)?
        };
        persist_provider_echo(&state);
        let query_vec = vecs
            .pop()
            .ok_or_else(|| AppError::Internal("EncodeText 返回空向量集".into()))?;

        // 装载源:读池全量嵌入行(维度过滤与打包在控制面内的 EmbeddingCache::pack)。
        let outcome = crate::ai::search_control::run_search(
            &state.ai_search,
            &ticket,
            &query_vec,
            top_k,
            dim,
            || {
                let conn = state.db_read_pool.get().map_err(AppError::from)?;
                crate::db::queries::get_all_embeddings(&conn, ticket.model())
            },
            |scored| {
                // 提交闸门内的落库:检验「仍是最新请求」与整表事务同临界区(无 TOCTOU)。
                let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                crate::ai::search::replace_search_results(&mut conn, scored)
            },
        )?;

        match outcome {
            crate::ai::search_control::SearchCommit::Committed(n) => Ok(Some(n)),
            crate::ai::search_control::SearchCommit::Superseded => {
                info!("Semantic search superseded by a newer request/clear/model switch | 语义搜索已被更新的请求取代");
                Ok(None)
            }
        }
    })
    .await
    .map_err(join_err)?
}

/// 清空语义搜索结果集与在途请求。用户清空搜索或退出语义模式时调用。
///
/// 两段式(P1-3):**入口**当场登记清空代次并吊销在途请求——用户意图不排在磁盘动作后面;
/// 擦库在阻塞段,且只在期间没有更新的登记时才执行。这样「清空 IPC 先到、工作线程晚跑、
/// 期间用户又搜了一次」不会把新结果擦掉;而「查询先提交、清空后到」也照样被入口吊销拦下。
#[tauri::command]
pub async fn clear_semantic_search(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);
    let clear_ticket = state_arc.ai_search.begin_clear();
    tokio::task::spawn_blocking(move || -> Result<()> {
        state_arc.ai_search.clear_if_current(&clear_ticket, || {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            crate::ai::search::wipe_search_results(&conn)
        })?;
        Ok(())
    })
    .await
    .map_err(join_err)?
}

/// 持久化「期望运行」标志并启动流水线。
/// R1-3：标志位落库下沉 blocking（保留 into_inner 毒锁恢复）；`start_ai_pipeline` 内部要
/// `tokio::spawn`，须留在 async 上下文，故本函数整体改 async 而非塞进 spawn_blocking。
async fn launch_ai_pipeline(state: &Arc<AppState>) -> Result<()> {
    let s = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "ai_analysis_active", "1");
    })
    .await
    .map_err(join_err)?;
    let (generation, token) = state.new_ai_analysis_token();
    start_ai_pipeline(Arc::clone(state), generation, token);
    Ok(())
}

/// 启动（或续传）后台 AI 分析流水线，且不重置已有嵌入向量——已分析的图片会跳过，只处理
/// 待处理 / 被中断的项。这是「开始 / 继续」动作，也是自动续传调用的入口（问题7）。
#[tauri::command]
pub async fn start_ai_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);

    // R1-3：active_profile（读池 SQL）+ sync_ai_status（写锁 SQL）一并下沉 blocking；
    // 保留 into_inner 毒锁恢复（AI 命令族契约：控制类写不因毒锁失效）。
    tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || {
            // 模型文件未就位就该在启动前拒绝（variant_installed 是文件系统 IO，故并进本 blocking
            // 段）：缺失时前端按稳定 code 弹下载引导，而不是让流水线跑到派发线程再静默失败。
            require_active_clip_assets(&s)?;
            // ai_status 是全局列，历史失败（例如旧输入名导致的批量 Error）可能没有当前模型向量。
            // 启动前按真实向量覆盖重同步，确保“开始/继续”会补跑缺失项，而不是被 Error 永久跳过。
            let model_id = active_profile(&s).id;
            sync_ai_status_for_model(&s.db_writer, &model_id)
        }
    })
    .await
    .map_err(join_err)??;

    // F5 mutual exclusion: claim the shared GPU-analysis slot. Fails fast if face analysis holds
    // it (CLIP & face can't run together — VRAM contention). Re-entrant when CLIP already owns it
    // (resume/start while running). Placed AFTER the idempotent sync above but BEFORE cancel, so a
    // rejection leaves no half-applied state.
    // F5 互斥：占用共享 GPU 分析槽。若人脸分析持有则快速失败（CLIP 与人脸不能同跑——显存竞争）。
    // CLIP 已持有时可重入（运行中续传/开始）。放在上面幂等 sync 之后、cancel 之前，使被拒时不留半应用状态。
    if !state_arc.try_acquire_gpu_analysis(crate::state::GPU_OWNER_AI) {
        // F5/P1-1：共享 GPU 分析槽被人脸分析占用。这是**可等待状态**，用稳定 code 交前端按类型
        // 分流（排队等对端释放后自动续跑），不靠本地化文案识别（error.rs AnalysisBusy）。
        return Err(AppError::AnalysisBusy);
    }

    // 取消任何现有运行，然后续传（孤儿恢复在流水线内部完成）。
    state_arc.cancel_ai_analysis();
    info!("Starting/resuming AI analysis pipeline (no reset) | 启动/续传 AI 分析流水线（不重置）");
    // launch 失败（仅 blocking join panic 路径）须释放 GPU 槽，避免泄漏（与 restart 的 reset 失败同规）。
    if let Err(e) = launch_ai_pipeline(&state_arc).await {
        state_arc.release_gpu_analysis(crate::state::GPU_OWNER_AI);
        return Err(e);
    }

    Ok(())
}

/// 从零重新开始：清除所有嵌入向量（ai_status → 0）后运行。这是「重新开始」动作（问题7）。
#[tauri::command]
pub async fn restart_ai_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);

    // 同 start：模型缺失先拒，且必须在占槽 / cancel / 破坏性 reset **之前**——否则会白清空向量。
    tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || require_active_clip_assets(&s)
    })
    .await
    .map_err(join_err)??;

    // F5 mutual exclusion: claim the slot BEFORE the destructive reset below (so a rejection
    // doesn't wipe embeddings). Re-entrant when CLIP already owns it. If the reset then fails,
    // release the slot to avoid leaking it.
    // F5 互斥：在下面破坏性 reset 之前占用槽（使被拒时不会清空向量）。CLIP 已持有时可重入。
    // 若随后 reset 失败，释放槽以免泄漏。
    if !state_arc.try_acquire_gpu_analysis(crate::state::GPU_OWNER_AI) {
        // 同 start：稳定 code 交前端分流；但 restart 是破坏性重置，前端不排队重做（被拒即报错
        // 交用户决定何时再来）。
        return Err(AppError::AnalysisBusy);
    }

    state_arc.cancel_ai_analysis();

    // 清除之前的嵌入向量，保证全量重新分析。
    let reset_res = tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || {
            // 仅重置当前激活模型的嵌入向量（其它模型的向量保留）。
            let model_id = active_profile(&s).id;
            // 重置与「吊销 + 作废 + 守卫擦库」在同一个收尾函数内,重置失败也照样收尾。
            reset_embeddings_with_search_teardown(&s.db_writer, &s.ai_search, &model_id)
        }
    })
    .await
    .map_err(join_err)
    .and_then(|inner| inner);
    if let Err(e) = reset_res {
        state_arc.release_gpu_analysis(crate::state::GPU_OWNER_AI);
        return Err(e);
    }

    info!("Restarting AI analysis pipeline (full reset) | 重新开始 AI 分析流水线（全量重置）");
    // 同 start：launch 失败须释放 GPU 槽。
    if let Err(e) = launch_ai_pipeline(&state_arc).await {
        state_arc.release_gpu_analysis(crate::state::GPU_OWNER_AI);
        return Err(e);
    }

    Ok(())
}

/// 暂停运行中的分析：取消流水线但保留 active 标志，以便之后续传（含下次启动自动续传）。
/// 在途的「处理中」项会在下次运行时恢复为待处理（问题7）。
#[tauri::command]
pub async fn pause_ai_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("Pausing AI analysis (keeps resume flag) | 暂停 AI 分析（保留续传标志）");
    state.cancel_ai_analysis();
    // Release the shared GPU-analysis slot so face analysis can start while CLIP is paused
    // (F5 mutual exclusion). The completion handler only releases on natural completion, so
    // a pause/stop must release here.
    // 释放共享 GPU 分析槽，使 CLIP 暂停期间人脸分析可启动（F5 互斥）。完成回调仅在自然完成时
    // 释放，故暂停/停止须在此释放。
    state.release_gpu_analysis(crate::state::GPU_OWNER_AI);
    // R1-3：标志位落库下沉 blocking；保留 into_inner 毒锁恢复（暂停不能因毒锁失效）。
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "ai_analysis_active", "1");
    })
    .await
    .map_err(join_err)?;
    Ok(())
}

/// 停止运行中的 AI 分析流水线并清除续传标志（不再自动续传）。进度保留（嵌入向量不删），
/// 仅放弃「自动继续」的意图。
#[tauri::command]
pub async fn stop_ai_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!(
        "Stopping AI analysis pipeline (clears resume flag) | 停止 AI 分析流水线（清除续传标志）"
    );
    state.cancel_ai_analysis();
    // Release the shared GPU-analysis slot (F5 mutual exclusion) — see pause for why.
    // 释放共享 GPU 分析槽（F5 互斥）——理由见暂停。
    state.release_gpu_analysis(crate::state::GPU_OWNER_AI);
    // R1-3：同暂停——下沉 blocking + into_inner 毒锁恢复。
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "ai_analysis_active", "0");
    })
    .await
    .map_err(join_err)?;
    Ok(())
}

/// Reload the AI engine — T16 后语义:关闭 worker 在载会话,下次分析/搜索按当前
/// 配置重新 SessionInit(命令名保持前端兼容)。
#[tauri::command]
pub async fn reload_ai_engine(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("Reloading AI engine | 重新加载 AI 引擎(worker 会话重建语义)");
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        s.cancel_ai_analysis();
        // cancel 后必须释放 GPU 分析槽(2026-07-10 审查 A1):完成回调仅自然完成时释放,
        // 泄漏的槽因同 owner 可重入不困 CLIP 自身,却让人脸分析被「幽灵占用」永久拒绝。
        // release 带 owner 校验,未持有时空操作,安全。
        s.release_gpu_analysis(crate::state::GPU_OWNER_AI);
        s.ai_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .close_session();
        Ok(())
    })
    .await
    .map_err(join_err)?
}

/// 非破坏重试失败项(2026-07-10 审查 F9):`ai_status` Error → Pending 分批复位,返回复位数。
/// 不触碰任何已完成向量——与 rebuild(清空重来)/restart(全量重置)语义严格区分;
/// 复位后由前端走既有 start 复跑。
#[tauri::command]
pub async fn retry_failed_ai_items(state: State<'_, Arc<AppState>>) -> Result<i64> {
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<i64> {
        let n = reset_error_ai_items(&s.db_writer)?;
        if n > 0 {
            info!("Retry failed AI items: {n} reset to pending | 重试失败项:{n} 项复位待处理");
        }
        Ok(n as i64)
    })
    .await
    .map_err(join_err)?
}

/// 重置所有嵌入向量，将所有图像重新排入分析队列。
#[tauri::command]
pub async fn rebuild_embeddings(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);

    // 首先停止任何正在运行的流水线
    state_arc.cancel_ai_analysis();
    // cancel 后释放 GPU 分析槽(2026-07-10 审查 A1,理由见 reload_ai_engine)。
    state_arc.release_gpu_analysis(crate::state::GPU_OWNER_AI);

    tokio::task::spawn_blocking(move || -> Result<()> {
        let model_id = active_profile(&state_arc).id;
        // 同 restart:分批重置中途失败也必须先收尾(吊销在途票 + 作废快照 + 守卫擦库),
        // 再把重置错误原样报给用户——否则残留的旧排名会与「重建失败」的提示自相矛盾。
        reset_embeddings_with_search_teardown(&state_arc.db_writer, &state_arc.ai_search, &model_id)
    })
    .await
    .map_err(join_err)??;

    info!("Embeddings reset, re-queuing all images | 嵌入向量已重置，重新排队所有图像");
    Ok(())
}

/// Scan the models dir for installed image-encoder variants of a given architecture (offline
/// fallback when discovery fails — the user can still switch among already-downloaded variants).
/// Matches `<prefix>.img.*.<fp16|fp32>.onnx` (excluding `.extra_file`); the fp16/fp32 marker keeps
/// the two B/16 architectures (same `vit-b-16` prefix) apart.
/// 扫描 models 目录里某架构已安装的图像变体（发现失败时的离线回退——用户仍可在已下载变体间切换）。
/// 匹配 `<prefix>.img.*.<fp16|fp32>.onnx`（排除 `.extra_file`）；fp16/fp32 标记区分同前缀的两个 B/16。
fn scan_installed_image_files(models: &std::path::Path, meta: &profile::ArchMeta) -> Vec<String> {
    let Some(prefix) = meta.default_image_file.split(".img.").next() else {
        return Vec::new();
    };
    let marker = format!("{prefix}.img.");
    let fp = if meta.fp16 { ".fp16." } else { ".fp32." };
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(models) {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with(&marker)
                    && name.contains(fp)
                    && name.ends_with(".onnx")
                    && !name.ends_with(".extra_file")
                {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// List the model library grouped by architecture → batch variants, with per-variant install /
/// active status. The static fp16 B/16 is always present; the rest are discovered live from
/// `gficcg/clip_cn_vit-onnx`. On discovery failure it falls back to static + installed-on-disk
/// variants (so switching still works offline) and reports `online: false`.
/// 按「架构 → batch 变体」分组列出模型库，含每个变体的安装/激活状态。静态 fp16 B/16 恒在；其余
/// 从 `gficcg/clip_cn_vit-onnx` 在线发现。发现失败时回退为「静态 + 磁盘已安装变体」（离线仍可切换）
/// 并返回 `online: false`。
#[tauri::command]
pub async fn list_model_registry(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    // A2:三键均为 schema 设置类,唯一真源已切到 ConfigManager(内存读,不必再借读池连接查 DB)。
    let active_arch = state
        .config
        .get("ai_active_model")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| profile::DEFAULT_PROFILE_ID.to_string());
    let active_image_cfg = state
        .config
        .get("ai_active_image_file")
        .filter(|s| !s.is_empty());
    let mirror_first = state.config.get("ai_download_source").as_deref() == Some("mirror");

    // 动态发现(async,置于 spawn_blocking 之外);带磁盘 L2(Part4-T8:冷启动免重复联网、
    // 断网用陈旧快照兜底)。语义微调:online=false 仅当「联网失败且无任何磁盘缓存」——
    // 有缓存时照常列出(离线可浏览,A5 的意义所在)。
    let disk_cache = models_dir(&state).join(remote_registry::DISK_CACHE_FILE);
    let discovered = remote_registry::discover(mirror_first, Some(&disk_cache)).await;
    let online = discovered.is_ok();
    let discovered = discovered.unwrap_or_default();

    let models = models_dir(&state);
    // R1-3：变体安装判定 = 每架构十余次 exists/read_dir（阻塞 IO），整段组装下沉 blocking。
    tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        // 当前激活变体文件名：配置缺省时取激活架构的缺省图像文件。
        let active_image = active_image_cfg.unwrap_or_else(|| {
            profile::arch_by_id(&active_arch)
                .map(|m| m.default_image_file.to_string())
                .unwrap_or_default()
        });

        let mut archs_json: Vec<serde_json::Value> = Vec::new();
        for meta in profile::arch_metas() {
            let text_file = meta.text_file.to_string();
            let arch_active = meta.id == active_arch;
            let mut variants_json: Vec<serde_json::Value> = Vec::new();

            // 追加一个变体条目（含安装/激活判定与 batch 分类）。
            let mut push_variant = |image_file: &str, batch: Option<BatchKind>, size_bytes: u64| {
                let installed = variant_installed(&models, image_file, &text_file);
                let active = arch_active && image_file == active_image;
                let (batch_kind, fixed_batch) = match batch {
                    None => ("single", serde_json::Value::Null),
                    Some(BatchKind::Dynamic) => ("dynamic", serde_json::Value::Null),
                    Some(BatchKind::Fixed(k)) => ("fixed", serde_json::json!(k)),
                };
                variants_json.push(serde_json::json!({
                    "imageFile": image_file,
                    "batchKind": batch_kind,
                    "fixedBatch": fixed_batch,
                    "sizeBytes": size_bytes,
                    "installed": installed,
                    "active": active,
                }));
            };

            match meta.folder {
                // 静态 fp16 B/16：单变体（eisneim），体积取已校验清单合计。
                None => {
                    let size: u64 = profile::static_fp16_b16_assets()
                        .iter()
                        .map(|a| a.size_bytes)
                        .sum();
                    push_variant(
                        meta.default_image_file,
                        remote_registry::parse_batch(meta.default_image_file),
                        size,
                    );
                }
                // 动态架构：优先用发现结果；发现不到则扫描磁盘已安装变体（离线回退）。
                Some(folder) => {
                    if let Some(arch) = discovered.iter().find(|a| a.folder == folder) {
                        let text_sz = arch.text_onnx.as_ref().map(|f| f.size_bytes).unwrap_or(0);
                        let text_extra_sz =
                            arch.text_extra.as_ref().map(|f| f.size_bytes).unwrap_or(0);
                        for v in &arch.variants {
                            let extra = v.extra.as_ref().map(|f| f.size_bytes).unwrap_or(0);
                            let size = v.onnx.size_bytes + extra + text_sz + text_extra_sz;
                            push_variant(&v.onnx.file, Some(v.batch), size);
                        }
                    } else {
                        for image_file in scan_installed_image_files(&models, &meta) {
                            let batch = remote_registry::parse_batch(&image_file);
                            push_variant(&image_file, batch, 0);
                        }
                    }
                }
            }

            // 无任何变体（如尚未导出且磁盘也没有的 h-14）→ 不展示该架构。
            if variants_json.is_empty() {
                continue;
            }
            archs_json.push(serde_json::json!({
                "id": meta.id,
                "displayName": meta.display_name,
                "description": meta.description,
                "imageSize": meta.image_size,
                "embedDim": meta.embed_dim,
                "sizeMb": meta.size_mb,
                "fp16": meta.fp16,
                "active": arch_active,
                "variants": variants_json,
            }));
        }

        Ok(serde_json::json!({
            "archs": archs_json,
            "activeArchId": active_arch,
            "activeImageFile": active_image,
            "online": online,
        }))
    })
    .await
    .map_err(join_err)?
}

/// Switch the active model: validate it is installed, persist the choice, reload the engine,
/// re-sync `ai_status` to the new model's embedding coverage, and invalidate the resident
/// cache. Afterwards the user runs analysis to (re)embed items missing under the new model —
/// already-embedded items are skipped, and switching BACK to a previously-used model is free.
/// 切换激活模型：校验已安装 → 持久化选择 → 重载引擎 → 按新模型向量覆盖重同步 `ai_status` →
/// 失效常驻缓存。之后用户运行分析以（重新）嵌入新模型下缺失的项 —— 已嵌入项跳过，切回曾用
/// 模型零成本。
#[tauri::command]
pub async fn set_active_model(
    app: tauri::AppHandle,
    image_file: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 代次在**操作开始时**读取:下方含变体校验、GPU 槽释放与向量覆盖重同步等耗时工作,期间若发生
    // 「恢复默认设置」,本次切换携带的旧代次会被统一入口拒绝——绝不在耗时工作后回读新代次,把切换前
    // 的意图灌回已重置的配置。
    let generation = state.config.generation();
    // 由变体文件名反查架构，合成该变体的 profile（id=架构 = 向量主键，不随变体变化）。
    let meta = arch_for_image_file(&image_file).ok_or_else(|| {
        AppError::UnsupportedFormat(format!(
            "无法识别的模型文件 | unknown model file: {image_file}"
        ))
    })?;
    let prof = profile::resolve_profile(meta.id, Some(&image_file)).ok_or_else(|| {
        AppError::UnsupportedFormat(format!("无法解析架构 | cannot resolve arch: {}", meta.id))
    })?;
    let arch_id = prof.id.clone();
    let text_file = prof.text_file.clone();
    let display_name = prof.display_name.clone();

    let state_arc = Arc::clone(&state);

    // 阶段 1:拒绝切换到文件尚未就位的变体(请先下载)+ 取消在跑分析 + 释放 GPU 分析槽。
    // 顺序与改动前一致(校验在前,cancel/释放紧随)。未安装时直接返回、不做收尾——与改动前
    // 「闭包内提前 return」的行为逐字等价。
    {
        let prep_state = Arc::clone(&state_arc);
        let image_for_prep = image_file.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let models = models_dir(&prep_state);
            if !variant_installed(&models, &image_for_prep, &text_file) {
                // 「未安装」语义上等同模型未就位 → AiModelNotLoaded（消息直透，保留可操作的中文提示）。
                return Err(AppError::AiModelNotLoaded(format!(
                    "模型「{}」尚未安装，请先下载其模型文件 | variant not installed: {}",
                    display_name, image_for_prep
                )));
            }
            prep_state.cancel_ai_analysis();
            // cancel 后释放 GPU 分析槽(2026-07-10 审查 A1,理由见 reload_ai_engine;face 侧对称
            // 命令 set_active_face_model 一直有此释放,本处补齐对称性)。
            prep_state.release_gpu_analysis(crate::state::GPU_OWNER_AI);
            Ok(())
        })
        .await
        .map_err(join_err)??;
    }

    // 阶段 2:两键一次提交,走统一设置入口(串行门 + 一次批量写盘 + 统一广播),不再逐键直接落盘。
    // 架构 id = 向量主键;变体文件名仅决定加载哪份图像塔。两者必须一起生效。
    let patch = std::collections::BTreeMap::from([
        ("ai_active_model".to_string(), arch_id.clone()),
        ("ai_active_image_file".to_string(), image_file.clone()),
    ]);
    let config_result = crate::config::settings::submit_settings_patch(
        &app,
        &state_arc,
        patch,
        generation,
    )
    .await
    .map(|_| ());

    // 阶段 3:配置写入成功才重同步向量覆盖(与改动前 `set_and_persist(..)?` 的短路顺序一致:
    // 配置没写成就不同步)。
    //
    // P1-3:整段「改配置 + 同步状态」的结果先收进 switch_result,不就地 `?` 提前返回——配置一旦写入
    // 就可能已经生效,此时若直接返回,旧模型的在途查询仍持有效票,会把旧向量空间的结果写回结果集。
    let switch_result = match config_result {
        Ok(()) => {
            let sync_state = Arc::clone(&state_arc);
            let arch_for_sync = arch_id.clone();
            tokio::task::spawn_blocking(move || {
                // ai_status 是全局列(非按模型)→ 重新指向新架构的向量覆盖(分批,批间自行取锁,R2-6)。
                sync_ai_status_for_model(&sync_state.db_writer, &arch_for_sync)
            })
            .await
            .map_err(join_err)?
        }
        Err(e) => Err(e),
    };

    // 共同收尾:只要**尝试过**改模型配置就必须执行,与上面成败无关。吊销在控制面闸门内现读 profile:
    // 此刻若配置已变,读到的就是新模型——此前登记的请求(按旧模型解析)一律作废,此后登记的读到新模型;
    // 若配置其实没变(第一步就失败),这次收尾也只是把上一轮结果集清掉,无副作用地保守一次。
    // 擦除失败只记警告:切换本身已生效或已尝试,为「旧结果多留一会儿」报错会误导用户以为切换失败
    // (同 restart/rebuild 的姿态);吊销与缓存作废无条件生效。
    //
    // 仍在 spawn_blocking 内:擦除要拿 db_writer 的 std 锁(硬约束:IO/DB 不占 UI 线程)。
    tokio::task::spawn_blocking(move || -> Result<()> {
        if let Err(e) = state_arc.ai_search.model_switched(|| {
            let conn = state_arc.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            crate::ai::search::wipe_search_results(&conn)
        }) {
            warn!("Search results wipe failed on model switch (revocation applied) | 切换模型时擦除搜索结果失败(在途请求已吊销): {e}");
        }

        // 关闭 worker 在载会话:spec 含 profile,下次派发/搜索自动按新变体重 SessionInit。
        state_arc
            .ai_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .close_session();
        Ok(())
    })
    .await
    .map_err(join_err)??;

    // 收尾做完再传播首个错误(配置/同步失败仍要让调用方看见,前端据此报错给用户)。
    switch_result?;

    info!(
        "Active AI model switched to {} (variant {}) | 已切换 AI 模型: {}（变体 {}）",
        arch_id, image_file, arch_id, image_file
    );
    Ok(())
}

// ── Model download (Layer B) ────────────────────────────────────────────────────
// ── 模型下载（Layer B）────────────────────────────────────────────────────────────
// 共享下载编排(DownloadProgress/download_assets/落盘名白名单)已迁兄弟模块
// `ipc::model_download`(U-P2-b):它同时服务本文件与 face_commands,不该寄居
// 单一命令文件。本文件只留 download_model 命令(资产清单构建 = 在线发现)。

/// Download all assets for a model into the models dir, with per-file resume (HTTP Range),
/// size + sha256 verification, mirror fallback, and progress streamed over `on_progress`.
/// Files are written to a `.part` sidecar then atomically renamed, so an interrupted download
/// never leaves a truncated model in place. Already-correct files (size + sha256) are skipped,
/// making this also a "repair" action.
/// 下载某模型的全部资产到 models 目录：逐文件断点续传（HTTP Range）、大小 + sha256 校验、镜像回退、
/// 进度经 `on_progress` 流式推送。先写 `.part` 旁路文件再原子改名，使中断不会留下截断的模型；已正确
/// 文件（大小 + sha256）跳过，故此命令同时是「修复」动作。
#[tauri::command]
pub async fn download_model(
    image_file: String,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 由变体文件名反查架构（决定走静态清单还是在线发现）。
    let meta = arch_for_image_file(&image_file).ok_or_else(|| {
        AppError::UnsupportedFormat(format!(
            "无法识别的模型文件 | unknown model file: {image_file}"
        ))
    })?;
    let display_name = meta.display_name.to_string();
    // 进度事件用变体文件名作 id（前端按变体键管理下载进度）。
    let download_id = image_file.clone();

    let models = models_dir(&state);
    std::fs::create_dir_all(&models)?;

    // 用户选择的首选下载源：`mirror`=国内镜像(hf-mirror.com)优先，其它(含默认/`official`)=官方
    // (HuggingFace)优先。两种模式都会在首选源失败时自动回退到另一源，保证健壮性。
    // Preferred download source: `mirror` puts the China mirror first; anything else (incl.
    // default / `official`) puts the official source first. Either way we fall back to the other.
    // A2:schema 设置类键,唯一真源已切到 ConfigManager(内存读)。
    let mirror_first = state.config.get("ai_download_source").as_deref() == Some("mirror");

    // 构造下载清单：静态 fp16 = 已校验固定清单；动态架构 = 由在线发现拼出
    // 图像 onnx + 其 extra + 共享文本塔 onnx + 其 extra + vocab（均带 size/sha256 校验）。
    let assets: Vec<profile::ModelAsset> = match meta.folder {
        None => profile::static_fp16_b16_assets(),
        Some(folder) => {
            // 在线发现失败=网络/远端问题（可操作）→ System 直透详情；arch/variant 缺失=配置指向不存在=格式问题。
            let disk_cache = models_dir(&state).join(remote_registry::DISK_CACHE_FILE);
            let archs = remote_registry::discover(mirror_first, Some(&disk_cache))
                .await
                .map_err(|e| AppError::internal("获取在线模型列表失败 | discovery failed", e))?;
            let arch = archs.iter().find(|a| a.folder == folder).ok_or_else(|| {
                AppError::UnsupportedFormat(format!(
                    "仓库中找不到架构 | arch not in repo: {folder}"
                ))
            })?;
            let variant = arch
                .variants
                .iter()
                .find(|v| v.onnx.file == image_file)
                .ok_or_else(|| {
                    AppError::UnsupportedFormat(format!(
                        "仓库中找不到该变体 | variant not in repo: {image_file}"
                    ))
                })?;

            let mut a = vec![remote_registry::remote_asset(&variant.onnx)];
            if let Some(extra) = &variant.extra {
                a.push(remote_registry::remote_asset(extra));
            }
            if let Some(t) = &arch.text_onnx {
                a.push(remote_registry::remote_asset(t));
            }
            if let Some(te) = &arch.text_extra {
                a.push(remote_registry::remote_asset(te));
            }
            a.push(profile::vocab_asset());
            a
        }
    };

    if assets.is_empty() {
        return Err(AppError::AiModelNotLoaded(format!(
            "模型「{display_name}」暂无可下载清单 | no download manifest"
        )));
    }

    // R10：用通用引擎的安全 client（HTTPS 强制 + 重定向加固 + 大文件不设整体超时，避免 ~GB 模型
    // 被整体超时误杀）。HF `resolve/` 会 302 跳到 HTTPS CDN —— 安全策略只拒「降级到非 HTTPS」的跳转，故兼容。
    let client = crate::download::secure_client(crate::download::TimeoutPolicy::LargeFile)
        .map_err(|e| AppError::internal("HTTP 客户端构建失败 | client build failed", e))?;

    // 逐资产下载循环已抽为共用函数 `download_assets`（人脸下载命令复用）。它仍返回 String
    // 以喂给 DownloadProgress.error（流式展示通道，非 IPC 契约）；命令边界把最终失败串包成
    // AppError::System（消息直透，保留「下载失败 <文件>: <原因>」这条可操作详情）。
    download_assets(
        &client,
        &models,
        &assets,
        mirror_first,
        &on_progress,
        &download_id,
    )
    .await
    .map_err(AppError::System)
}

// download_assets/is_safe_model_file_name/DownloadProgress 及其安全测试已迁
// `ipc::model_download`(U-P2-b),见该模块头注的层次边界说明。

#[cfg(test)]
mod p1_3_teardown_tests {
    use super::*;
    use crate::ai::search_control::{SearchCommit, SearchControl, SearchRequestId};
    use crate::db::DbWriter;
    use rusqlite::Connection;

    /// 造一个「已装载旧模型向量 + 已提交旧结果 + 有在途票」的现场:
    /// 嵌入行 + 一条 image 媒体项(ai_status 非 0,使重置的 UPDATE 阶段有事可做)
    /// + 结果表旧行 + 控制面常驻快照。
    fn scene() -> (DbWriter, SearchControl, i64) {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth)
                 VALUES (10, 1, NULL, 'A', 'A', 0);
             INSERT INTO media_items
                 (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                  width, height, sort_datetime, cache_key, is_favorited, is_deleted,
                  is_live_photo, companion_of, ai_status)
                 VALUES (1, 10, 'a.jpg', 0, 0, 'jpg', 'image', 0, 0, 0, 0, 0, 0, 0, NULL, 2);",
        )
        .unwrap();
        let blob: Vec<u8> = [1.0f32, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO ai_embeddings (item_id, model_name, embedding) VALUES (1, 'm1', ?1)",
            rusqlite::params![blob],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_search_results (file_id, similarity) VALUES (1, 0.9)",
            [],
        )
        .unwrap();

        let writer: DbWriter = std::sync::Mutex::new(conn);
        let search = SearchControl::new();
        let warm = search.begin_request("m1");
        search
            .snapshot_for(&warm, 2, || Ok(vec![(1, blob.clone())]))
            .unwrap();
        assert!(search.resident_identity().is_some(), "前置:快照已常驻");
        (writer, search, warm.seq() as i64)
    }

    /// 重置的第一阶段(删向量)成功、第二阶段(清 ai_status 的 UPDATE)失败:
    /// 已删掉的向量不会回来,但搜索侧必须已经收尾——吊销在途票、作废快照、擦掉旧结果。
    /// 这正是「分批非事务」留下的半完成态,不能让它带着旧排名继续可见。
    #[test]
    fn failed_reset_still_revokes_in_flight_search_and_drops_stale_cache() {
        let (writer, search, _) = scene();

        // 在途票:重置前登记(应被吊销)。
        let in_flight = search.begin_request("m1");
        let in_flight_id = in_flight.id().clone();
        search
            .snapshot_for(&in_flight, 2, || Ok(Vec::new()))
            .unwrap();

        // 注入失败:UPDATE media_items 触发 ABORT(模拟后续批失败)。
        {
            let conn = writer.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER p1_3_fail_status BEFORE UPDATE ON media_items
                 BEGIN SELECT RAISE(ABORT, 'injected status update failure'); END;",
            )
            .unwrap();
        }

        let err = reset_embeddings_with_search_teardown(&writer, &search, "m1").unwrap_err();
        assert!(
            matches!(err, AppError::Db(_)),
            "重置失败须原样传播给调用方,实际: {err:?}"
        );

        // 半完成态属实:向量确已删除(所以更不能再暴露旧排名)。
        {
            let conn = writer.lock().unwrap();
            let left: i64 = conn
                .query_row("SELECT count(*) FROM ai_embeddings", [], |r| r.get(0))
                .unwrap();
            assert_eq!(left, 0, "前置:第一阶段确已删掉向量(半完成态)");
        }

        // 收尾已生效:快照作废、在途票吊销、旧结果集擦净。
        assert!(
            search.resident_identity().is_none(),
            "重置失败也必须作废常驻快照"
        );
        assert!(search.latest().is_none(), "重置失败也必须吊销在途票");
        assert_eq!(
            search.commit(&in_flight, || Ok(1)).unwrap(),
            SearchCommit::Superseded,
            "重置前登记的请求不得在失败后仍能提交"
        );
        assert_eq!(search.committed(), None, "旧结果集身份须已清除(表已擦)");
        let conn = writer.lock().unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM ai_search_results", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "旧搜索结果须被擦掉");

        // 重置成功后登记的请求照常工作(新的向量空间)。
        drop(conn);
        {
            let conn = writer.lock().unwrap();
            conn.execute_batch("DROP TRIGGER p1_3_fail_status;")
                .unwrap();
        }
        let fresh = search.begin_request("m1");
        assert_eq!(
            search.commit(&fresh, || Ok(3)).unwrap(),
            SearchCommit::Committed(3),
            "重置后登记的请求正常提交"
        );
        let _ = in_flight_id;
    }

    /// 成功路径对照:收尾照样执行(擦净旧结果),且错误为空——证明上面的断言不是「总是失败」。
    #[test]
    fn successful_reset_also_clears_old_results() {
        let (writer, search, _) = scene();
        let in_flight = search.begin_request("m1");
        let _ = in_flight;

        reset_embeddings_with_search_teardown(&writer, &search, "m1").unwrap();

        assert!(search.resident_identity().is_none());
        assert!(search.latest().is_none(), "重置前的在途票须被吊销");
        assert_eq!(search.committed(), None);
        let conn = writer.lock().unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM ai_search_results", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        let status: i64 = conn
            .query_row("SELECT ai_status FROM media_items WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, 0, "成功路径须把 ai_status 复位");
        let _: Option<SearchRequestId> = None;
    }
}

/// 启动 / 重新开始分析的前置检查表征测试（首次使用未下载模型 → 同步可操作拒绝）。
///
/// 锁住三态：文件不齐即反 AiModelNotLoaded（前端按 code 分流成下载引导，不看文案）；
/// 只齐一半（共享文本塔 / 外置权重 / 词表任一缺）同样拒绝；五项齐 → 放行。
#[cfg(test)]
mod model_precheck_tests {
    use super::*;

    /// 与默认架构对应的图像/文本塔文件名（仅作样例，判定与具体架构无关）。
    const IMAGE: &str = "vit-b-16.img.fp16.onnx";
    const TEXT: &str = "vit-b-16.txt.fp16.onnx";

    fn write_asset(dir: &std::path::Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    fn all_assets(dir: &std::path::Path) {
        for name in [
            IMAGE.to_string(),
            format!("{IMAGE}.extra_file"),
            TEXT.to_string(),
            format!("{TEXT}.extra_file"),
            "vocab.txt".to_string(),
        ] {
            write_asset(dir, &name);
        }
    }

    #[test]
    fn missing_assets_rejected_with_download_guidance() {
        let dir = tempfile::tempdir().unwrap();

        let err = require_variant_installed(dir.path(), IMAGE, TEXT).unwrap_err();

        let AppError::AiModelNotLoaded(msg) = err else {
            panic!("未下载须反稳定 code AiModelNotLoaded（前端据此弹下载引导）");
        };
        assert!(msg.contains("模型库"), "消息须带可操作的下载指引: {msg}");
        assert!(msg.contains(IMAGE), "消息须指明缺哪个文件: {msg}");
    }

    #[test]
    fn partial_assets_rejected() {
        let dir = tempfile::tempdir().unwrap();
        write_asset(dir.path(), IMAGE);
        write_asset(dir.path(), TEXT);

        assert!(
            require_variant_installed(dir.path(), IMAGE, TEXT).is_err(),
            "外置权重 / 词表缺失仍视为未安装"
        );
    }

    #[test]
    fn complete_assets_pass() {
        let dir = tempfile::tempdir().unwrap();
        all_assets(dir.path());

        assert!(require_variant_installed(dir.path(), IMAGE, TEXT).is_ok());
    }
}
