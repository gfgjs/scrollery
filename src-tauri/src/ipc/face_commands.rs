//! 人脸识别流水线管理的 IPC 命令（F5）。
//!
//! 仿 `ai_commands`(开始/暂停/停止/重启/状态),但面向人脸流水线。模型加载发生在
//! ai-worker 的 SessionInit(T16:host 无进程内引擎);就绪判定 = 配置启用 + 激活轨
//! 双 onnx 在盘。CLIP 与人脸共用唯一 GPU 分析槽(`AppState::gpu_analysis_owner`)且
//! 互斥——这些命令以与 CLIP 命令相同的方式占用/释放它。

use std::sync::Arc;

use tauri::State;
use tracing::info;

use std::path::Path;

use super::blocking::write_blocking;
use crate::ai::face_pipeline::start_face_pipeline;
use crate::ai::face_profile::{
    face_profiles, find_face_profile, FaceProfile, DEFAULT_FACE_PROFILE_ID,
};
use crate::ai::runtime_config::models_dir;
use crate::db::models::{
    FaceBox, FaceModelInfo, FaceStatusSummary, LikelyMatchGroup, PersonSummary,
};
use crate::db::queries::{
    confirm_face_assignment, count_error_face_items, count_faces_for_model,
    count_pending_face_items, count_persons, count_processed_face_items, count_total_ai_items,
    create_person_from_faces, get_config, get_faces_for_item, list_ignored_persons,
    list_likely_matches, list_persons, merge_persons, reassign_face_to_person,
    reject_face_candidate, rename_person, reset_error_face_items, reset_face_data, set_config,
    set_person_hidden, set_person_ignored, unassign_face,
};
use crate::error::{AppError, Result};
use crate::ipc::ai_commands::ANALYSIS_BUSY_WAITING_KEY;
use crate::ipc::model_download::{download_assets, DownloadProgress};
use crate::state::{AppState, GPU_OWNER_FACE};

/// 共享 GPU 分析会话当前是否被**对端**（CLIP 语义分析）持有——本端未运行时的等待原因判定。
/// 等待原因键与语义分析侧共用同一个定义（见 `ai_commands::ANALYSIS_BUSY_WAITING_KEY`）。
fn other_analysis_holds_gpu_session(state: &AppState) -> bool {
    // 毒锁恢复(与全库 gpu_analysis_owner 访问点一致):门闩只是 Option<&'static str>.
    *state
        .gpu_analysis_owner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        == Some(crate::state::GPU_OWNER_AI)
}

/// 一条人脸轨的两个 onnx 文件均在盘,且——当轨的下载清单载有该文件期望 size 时——尺寸须一致
/// (Part4-T5 选项 B,2026-07-06 拍板)。
///
/// size 快检只多一次 stat(零启动税),专抓「手动放入 LFS pointer 文本/截断文件」的自伤面
/// (F5):官方下载路径落盘前已做 size+sha256 全校验,存在≈已校验,唯手动绕过时这里当场识破,
/// 而非等 worker 推理崩溃。全量 sha256 刻意不进判定路径(数十 MB 模型的常驻税,决策 brief §3
/// 选项 C 否决);清单无该文件条目(如 SCRFD 手动导入轨 assets 为空)→ 退回存在判定,诚实降级。
fn face_variant_installed(models_dir: &Path, profile: &FaceProfile) -> bool {
    [&profile.detect_file, &profile.embed_file].iter().all(|file| {
        let path = models_dir.join(file.as_str());
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        match profile.assets.iter().find(|a| &a.dest == *file) {
            Some(asset) if meta.len() != asset.size_bytes => {
                tracing::warn!(
                    "人脸模型文件尺寸与清单不符,按未安装处理(疑似 LFS pointer/截断,重新下载即修复) | {} 期望 {} 字节,实际 {} 字节",
                    path.display(),
                    asset.size_bytes,
                    meta.len()
                );
                false
            }
            _ => true,
        }
    })
}

/// 从配置（`face_model_active`）解析当前激活的人脸模型 id，缺省回退默认。这是统计该模型人脸时用的
/// `faces.model_name` 向量空间键。
///
/// A2:`face_model_active` 是 schema 设置类键,唯一真源已切到 `ConfigManager`。
fn active_face_model_id(state: &AppState) -> String {
    active_face_model_id_with(&state.config)
}

/// A2 前称「连接已在手的变体」——已迁往 config.toml,改收 `&ConfigManager`。
fn active_face_model_id_with(config: &crate::config::ConfigManager) -> String {
    config
        .get("face_model_active")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_FACE_PROFILE_ID.to_string())
}

/// 人脸能力就绪判定:配置启用 + 激活轨双 onnx 在盘(T16:与 worker SessionInit
/// 「会加载」的判定同源——active_face_profile 含 face_enabled 门;真正的模型加载
/// 发生在 worker 端,host 不触引擎)。
fn face_loaded(state: &AppState) -> bool {
    face_loaded_with(&state.config, state)
}

/// A2 前称「连接已在手的变体」——已迁往 config.toml,改收 `&ConfigManager`。
fn face_loaded_with(config: &crate::config::ConfigManager, state: &AppState) -> bool {
    crate::ai::runtime_config::active_face_profile_with(config)
        .map(|p| face_variant_installed(&models_dir(state), &p))
        .unwrap_or(false)
}

/// 人脸分析的**完整**依赖预检(2026-07-11 加固批 A-4,会话解耦的便宜版):
/// 合并单 worker 下 face SessionInit 强制携带 CLIP 成对模型(worker 侧角色完备性校验),
/// CLIP 缺失时旧行为是流水线已启动、GPU 槽已占,才在 worker 侧以笼统错误失败。
/// 此处把跨轨依赖前置为指名报错;真正的 face-only 会话解耦待实际场景需要再做(todo P 节 g)。
fn face_dependencies_ready(state: &AppState) -> Result<()> {
    if !face_loaded(state) {
        return Err(AppError::AiModelNotLoaded("人脸模型未启用或未下载".into()));
    }
    let prof = crate::ai::runtime_config::active_profile(state);
    if !crate::ai::runtime_config::variant_installed(
        &models_dir(state),
        &prof.image_file,
        &prof.text_file,
    ) {
        return Err(AppError::AiModelNotLoaded(format!(
            "人脸分析经合并推理会话运行,需 CLIP 模型就位:激活变体 {} 未安装或不完整,请先在模型库下载/修复",
            prof.id
        )));
    }
    Ok(())
}

/// 获取人脸识别综合状态（计数、人物、运行态）。
#[tauri::command]
pub async fn get_face_status(state: State<'_, Arc<AppState>>) -> Result<FaceStatusSummary> {
    let state = Arc::clone(&state);

    tokio::task::spawn_blocking(move || -> Result<FaceStatusSummary> {
        let conn = state.db_read_pool.get()?;

        // Provider/GPU come from the shared engine (same AiEnginePool as CLIP) — reuse the keys
        // CLIP persisted on init.
        // provider/GPU 来自共享引擎（与 CLIP 同一 AiEnginePool）——复用 CLIP 初始化时持久化的键。
        let provider = get_config(&conn, "ai_provider")
            .unwrap_or_default()
            .unwrap_or_default();
        let gpu_name = get_config(&conn, "ai_gpu_name")
            .unwrap_or_default()
            .unwrap_or_default();

        // B-1:全程只用头上这一个 conn(model_id/face_loaded 曾各自再取池)。
        // A2:model_id 已迁往 config.toml,改传 `&state.config`。
        let model_id = active_face_model_id_with(&state.config);
        let total_items = count_total_ai_items(&conn).unwrap_or(0);
        let processed_items = count_processed_face_items(&conn).unwrap_or(0);
        let pending_items = count_pending_face_items(&conn).unwrap_or(0);
        let person_count = count_persons(&conn, &model_id).unwrap_or(0);
        let face_count = count_faces_for_model(&conn, &model_id).unwrap_or(0);
        // A11/F9:失败面可见化(processed 刻意含失败使进度到 100%,error 单列不再被掩盖)。
        let error_items = count_error_face_items(&conn).unwrap_or(0);

        let face_loaded = face_loaded_with(&state.config, &state);
        let is_analyzing = state.face_analysis_token.is_running();
        let analysis_active = get_config(&conn, "face_analysis_active")
            .unwrap_or_default()
            .map(|v| v == "1")
            .unwrap_or(false);

        // 让步阻塞源快照(可观测性三修 #2):与 CLIP 共用同一 ai_yield_blockers(),
        // 仅当流水线正在跑时才有意义,否则空。
        // P1-1:未运行但仍有剩余、而共享 GPU 分析会话被 CLIP 持有时也报等待原因,供前端区分
        // 「资源等待」与「用户手动暂停」(理由同 ai_commands::get_ai_status)。
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

        Ok(FaceStatusSummary {
            provider,
            gpu_name,
            face_loaded,
            total_items,
            processed_items,
            pending_items,
            person_count,
            face_count,
            error_items,
            is_analyzing,
            analysis_active,
            waiting_on,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 持久化「人脸分析期望运行」标志并启动流水线。
fn launch_face_pipeline(state: &Arc<AppState>) {
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "face_analysis_active", "1");
    }
    let (generation, token) = state.new_face_analysis_token();
    start_face_pipeline(Arc::clone(state), generation, token);
}

/// 启动（或续传）人脸流水线，不重置已有人脸——已处理图像跳过（face_status≠0），只处理待处理/
/// 中断项。这是「开始/继续」动作，也是自动续传的入口。
#[tauri::command]
pub async fn start_face_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);

    // 就绪检查(在盘状态判定)含 DB 读,收进 blocking(rusqlite 硬化;模型加载在 worker SessionInit)。
    tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || -> Result<()> {
            // 人脸模型可能被禁用(face_enabled=0)或未下载(worker 后端按在盘状态判定);
            // A-4:连带 CLIP 依赖一并前置预检(合并会话强制 CLIP 成对,缺失即指名报错)。
            face_dependencies_ready(&s)?;
            // 命令层不做 sync:X2(start 路径 sync)因「faces 行=覆盖」误伤零脸图于
            // 2026-07-11 撤销;V17 face_coverage 落地后,自愈 sync 已下沉到流水线启动序列
            // (run_face_pipeline_worker_blocking,孤儿恢复之后)——零脸图有账不再误重扫,
            // 且与在途 writer 的竞态因「status+覆盖同事务」而无害化。此处只留就绪预检。
            Ok(())
        }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // F5 互斥：占用共享 GPU 分析槽（若 CLIP 持有则快速失败）。
    if !state_arc.try_acquire_gpu_analysis(GPU_OWNER_FACE) {
        // P1-1：会话被 CLIP 持有属**可等待状态**，用稳定 code 交前端按类型分流（排队等对端释放
        // 后自动续跑），不靠本地化文案识别（error.rs AnalysisBusy）。
        return Err(AppError::AnalysisBusy);
    }

    state_arc.cancel_face_analysis();
    info!(
        "Starting/resuming face analysis pipeline (no reset) | 启动/续传人脸分析流水线（不重置）"
    );
    // launch 内含 set_config 落库（R1-3）。
    let s = Arc::clone(&state_arc);
    tokio::task::spawn_blocking(move || launch_face_pipeline(&s))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;

    Ok(())
}

/// 非破坏重试失败项(2026-07-10 审查 F9):`face_status` Error → Pending 分批复位,返回复位数。
/// 与 restart(**销毁本模型全部命名/确认**后全量重跑)语义严格区分——一批图因临时原因
/// (卷离线/解码抽风)标 Error 时,这是唯一零破坏的补跑通道;复位后由前端走既有 start。
#[tauri::command]
pub async fn retry_failed_face_items(state: State<'_, Arc<AppState>>) -> Result<i64> {
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<i64> {
        let n = reset_error_face_items(&s.db_writer)?;
        if n > 0 {
            info!("Retry failed face items: {n} reset to pending | 重试失败项:{n} 项复位待处理");
        }
        Ok(n as i64)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 从零重新开始人脸分析:清空**当前激活模型**的人脸 + 人物(face_status → 0)后运行——其他
/// 模型名册保留(Part4-T6 隔离)。警告会销毁本模型的用户劳动(已命名人物、确认指派)——
/// 前端确认框须明示。
#[tauri::command]
pub async fn restart_face_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    let state_arc = Arc::clone(&state);

    // 同 start:就绪检查(含 A-4 CLIP 依赖预检)随 DB 读收进 blocking。
    tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || -> Result<()> { face_dependencies_ready(&s) }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // 在破坏性 reset 之前占用（使被拒时不会清空人脸）。reset 失败时释放槽以免泄漏。
    if !state_arc.try_acquire_gpu_analysis(GPU_OWNER_FACE) {
        // 同 start：稳定 code 交前端分流；restart 是破坏性重置，前端不排队重做。
        return Err(AppError::AnalysisBusy);
    }

    state_arc.cancel_face_analysis();

    let reset_res = tokio::task::spawn_blocking({
        let s = Arc::clone(&state_arc);
        move || {
            let model_id = active_face_model_id(&s);
            reset_face_data(&s.db_writer, &model_id)
        }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
    .and_then(|inner| inner);
    if let Err(e) = reset_res {
        state_arc.release_gpu_analysis(GPU_OWNER_FACE);
        return Err(e);
    }

    info!("Restarting face analysis pipeline (full reset) | 重新开始人脸分析流水线（全量重置）");
    // launch 内含 set_config 落库（R1-3）。
    let s = Arc::clone(&state_arc);
    tokio::task::spawn_blocking(move || launch_face_pipeline(&s))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;

    Ok(())
}

/// 暂停运行中的人脸分析：取消流水线但保留 active 标志以便续传。释放共享 GPU 槽，使 CLIP 可在
/// 人脸暂停期间运行。
#[tauri::command]
pub async fn pause_face_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("Pausing face analysis (keeps resume flag) | 暂停人脸分析（保留续传标志）");
    state.cancel_face_analysis();
    state.release_gpu_analysis(GPU_OWNER_FACE);
    write_blocking(&state, |c| {
        let _ = set_config(c, "face_analysis_active", "1");
        Ok(())
    })
    .await
}

/// 停止运行中的人脸分析并清除续传标志（不再自动续传）。人脸/人物保留，仅放弃「自动继续」意图。
#[tauri::command]
pub async fn stop_face_analysis(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!(
        "Stopping face analysis pipeline (clears resume flag) | 停止人脸分析流水线（清除续传标志）"
    );
    state.cancel_face_analysis();
    state.release_gpu_analysis(GPU_OWNER_FACE);
    write_blocking(&state, |c| {
        let _ = set_config(c, "face_analysis_active", "0");
        Ok(())
    })
    .await
}

// ── 人物墙 / 详情画框（F6）────────────────────────────────────────────────────

/// 列出人物墙的人物簇。
#[tauri::command]
pub async fn list_face_persons(state: State<'_, Arc<AppState>>) -> Result<Vec<PersonSummary>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<PersonSummary>> {
        // 人物墙只呈现当前激活模型的名册(Part4-T6 隔离;旧轨名册保留但不混显)。
        let model_id = active_face_model_id(&state);
        let conn = state.db_read_pool.get()?;
        list_persons(&conn, &model_id)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 列出「已忽略」(误检桶)人物簇,供误检桶管理视图查看/恢复(镜像隐藏人物切换)。
#[tauri::command]
pub async fn list_ignored_face_persons(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<PersonSummary>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<PersonSummary>> {
        // 与人物墙同口径:只呈现当前激活模型的误检桶(Part4-T6 隔离)。
        let model_id = active_face_model_id(&state);
        let conn = state.db_read_pool.get()?;
        list_ignored_persons(&conn, &model_id)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 获取一张图的所有检测人脸（详情查看器叠加框）。
#[tauri::command]
pub async fn get_item_faces(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<Vec<FaceBox>> {
    // span 埋点(W1,D-312 debug 档:详情查看器逐项查询,最接近「query_faces」候选的实际命令)。
    let _span = crate::logging::SpanTimer::debug("ipc:get_item_faces");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<FaceBox>> {
        let conn = state.db_read_pool.get()?;
        get_faces_for_item(&conn, item_id)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 给人物命名（空名 → 未命名）。
#[tauri::command]
pub async fn rename_face_person(
    person_id: i64,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| rename_person(c, person_id, &name)).await
}

/// 在人物墙上显示/隐藏某人物。
#[tauri::command]
pub async fn set_face_person_hidden(
    person_id: i64,
    hidden: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| set_person_hidden(c, person_id, hidden)).await
}

/// 标记/取消误检桶(2026-07-10 审查 G1,`is_ignored` 的首个写入口)。
/// 与 hidden 的区别:hidden=「不想看」(纯展示);ignored=「非人脸误检」——重建时按锚定
/// 保护,其成员脸永留桶内不再参与聚类。
#[tauri::command]
pub async fn set_face_person_ignored(
    person_id: i64,
    ignored: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| set_person_ignored(c, person_id, ignored)).await
}

/// 将 `srcIds` 人物簇并入 `dstId`（改派人脸、重算质心、删空簇）。
#[tauri::command]
pub async fn merge_face_persons(
    src_ids: Vec<i64>,
    dst_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| merge_persons(c, &src_ids, dst_id)).await?;
    // S1：人物归属变化改变 personId 视图成员 → bump。
    state.bump_data_version();
    Ok(())
}

/// 全量重新聚类：从零重建人物簇以修增量碎片化（同一人散成多个未命名簇），同时锁定用户劳动——
/// 已确认脸与已命名/忽略人物绝不被打散（见 `ai::face_cluster::recluster_all`）。分析运行中拒绝执行
///（会与人脸写入竞争）。纯 CPU 余弦计算——不碰 GPU 分析槽。
#[tauri::command]
pub async fn recluster_faces(state: State<'_, Arc<AppState>>) -> Result<()> {
    // 守卫：流水线写入中不重建。
    if state.face_analysis_token.is_running() {
        return Err(AppError::System(
            "人脸分析正在进行，请先暂停后再重新聚类".into(),
        ));
    }

    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let model_id = active_face_model_id(&state);
        // 阈值/最低质量取自当前人脸 profile（与流水线同一组旋钮）。
        let prof = find_face_profile(&model_id)
            .ok_or_else(|| AppError::System(format!("未知人脸模型 {model_id}")))?;
        // 与增量聚类同源:阈值取「运行期 override 或 profile 默认」(同一组 config 键,保持一致)。
        let (threshold, min_quality) = crate::ai::face_cluster::effective_thresholds(&state, &prof);
        crate::ai::face_cluster::recluster_all(&state, &model_id, threshold, min_quality);
        // S1：全量重聚类重排人物归属 → bump。
        state.bump_data_version();
        info!(
            "Face re-clustering done | 人脸重新聚类完成 (model={})",
            model_id
        );
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ── 人脸批量审批（Part4 T3 / §3.5.1）───────────────────────────────────────────
//
// 写命令统一经 `write_blocking`（super::blocking，AppError 错误契约，P1-8 收敛)下沉 blocking
// 线程（R1-3：原「同步持锁不跨 .await」写法已被 CLAUDE.md rusqlite 硬化条款取代）；DAO 内
// 自带事务，归属变更连带重算受影响 person。
// `list_likely_matches` 为只读 + spawn_blocking（解码全部未确认脸嵌入算余弦，较重）。

/// 确认（锁定）`faceIds` 的当前归属，使重聚类不再移动它们。
#[tauri::command]
pub async fn confirm_faces(face_ids: Vec<i64>, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| confirm_face_assignment(c, &face_ids)).await
}

/// 把 `faceIds` 改派给 `personId` 并锁定（用户纠正聚类错误）。拒绝跨模型改派；重算源与目标 person。
#[tauri::command]
pub async fn reassign_faces(
    face_ids: Vec<i64>,
    person_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        reassign_face_to_person(c, &face_ids, person_id)
    })
    .await?;
    // S1：personId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(())
}

/// 移出 `faceIds`（误检/归错）：清 person_id 与 is_confirmed；重算源 person。
#[tauri::command]
pub async fn unassign_faces(face_ids: Vec<i64>, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| unassign_face(c, &face_ids)).await?;
    // S1：personId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(())
}

/// 拒绝 `faceIds`「不是 `personId`」：记负样本 + 立即移出。后续全量重聚类查阅负样本以避免重新吸附。
#[tauri::command]
pub async fn reject_faces(
    face_ids: Vec<i64>,
    person_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        reject_face_candidate(c, &face_ids, person_id)
    })
    .await?;
    // S1：personId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(())
}

/// 从 `faceIds` 新建 person（一键「建人」），可选 `name`。返回新 person id。拒绝跨模型；重算源 person。
#[tauri::command]
pub async fn create_person(
    face_ids: Vec<i64>,
    name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<i64> {
    let pid = write_blocking(&state, move |c| {
        create_person_from_faces(c, &face_ids, name.as_deref())
    })
    .await?;
    // S1：personId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(pid)
}

/// 列出批量审批 UI 的 likely-match 组：未确认脸按候选 person 分组，各带人脸缩略图 + 匹配相似度。
/// 可选 `personId` / `limit` 过滤。
#[tauri::command]
pub async fn list_likely_face_matches(
    person_id: Option<i64>,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<LikelyMatchGroup>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<LikelyMatchGroup>> {
        let conn = state.db_read_pool.get()?;
        list_likely_matches(&conn, person_id, limit)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ── 人脸模型库（F7，只读）──────────────────────────────────────────────────────

/// 列出内置人脸模型轨 + 磁盘安装状态(F7)。下载走 `download_face_model`(仅有已校验清单的轨);
/// 切换走 **gated** `set_active_face_model`(Part4-T6)——仅 `verified`(已对拍)轨可激活,故
/// SCRFD/ArcFace 轨(`detect_scrfd` UNVERIFIED)即便 `installed=true` 也不会被启用。
/// `installed` 是诚实的磁盘状态,不代表「可用」;各轨名册按 `persons.model_name` 隔离,
/// 切换不销毁另一轨的标注。
#[tauri::command]
pub async fn list_face_model_registry(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<FaceModelInfo>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<FaceModelInfo>> {
        let models = models_dir(&state);
        let active_id = active_face_model_id(&state);
        let infos = face_profiles()
            .into_iter()
            .map(|p| {
                let installed = face_variant_installed(&models, &p);
                let active = p.id == active_id;
                FaceModelInfo {
                    active,
                    installed,
                    verified: p.verified,
                    // 有已校验下载清单才可一键下载（默认轨）；SCRFD 轨清单为空 → 仅手动导入。
                    downloadable: !p.assets.is_empty(),
                    detector: format!("{:?}", p.detector),
                    embedder: format!("{:?}", p.embedder),
                    embed_dim: p.embed_dim as i64,
                    size_mb: p.size_mb as i64,
                    id: p.id,
                    display_name: p.display_name,
                    description: p.description,
                    commercial_ok: p.commercial_ok,
                    license: p.license,
                }
            })
            .collect();
        Ok(infos)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 把某条人脸模型轨的 onnx 下载到 models 目录（size+sha256 校验、逐文件续传、`on_progress` 进度）。
/// 复用 CLIP 下载机制（`download_assets`）。仅有已校验清单的轨可下载（默认 YuNet+SFace）；SCRFD/
/// ArcFace 轨无已校验清单(非商用)→ 报错,仅手动导入。注意:下载可选轨文件**不会**启用它——
/// 启用走 gated `set_active_face_model`(未对拍轨拒绝激活,见 `list_face_model_registry`)。
#[tauri::command]
pub async fn download_face_model(
    profile_id: String,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let prof = find_face_profile(&profile_id)
        .ok_or_else(|| AppError::System(format!("未知人脸模型 {profile_id}")))?;
    if prof.assets.is_empty() {
        return Err(AppError::System(format!(
            "「{}」无可下载清单，仅支持手动导入",
            prof.display_name
        )));
    }

    let models = models_dir(&state);
    std::fs::create_dir_all(&models)?;

    // 首选下载源：mirror=国内镜像优先；其它=官方优先。两模式失败均自动回退另一源（复用 CLIP 约定）。
    // A2:schema 设置类键,唯一真源已切到 ConfigManager(内存读)。
    let mirror_first = state.config.get("ai_download_source").as_deref() == Some("mirror");

    // 用通用引擎安全 client(2026-07-10 审查 B9,对齐 CLIP 侧 download_model 的 R10 姿态):
    // HTTPS 强制 + 拒绝重定向降级 + 大文件不设整体超时。原裸 builder 无降级防护——
    // 镜像被劫持重定向到 http 不会被拒。
    let client = crate::download::secure_client(crate::download::TimeoutPolicy::LargeFile)
        .map_err(|e| AppError::internal("HTTP 客户端构建失败 | client build failed", e))?;
    // 进度事件以 profile id 作 download_id（前端按轨键管理下载进度）。
    // download_assets 返 String 错误契约(在 ipc::model_download,与 CLIP 共享)→ 收口为 System。
    download_assets(
        &client,
        &models,
        &prof.assets,
        mirror_first,
        &on_progress,
        &prof.id,
    )
    .await
    .map_err(AppError::System)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::profile::ModelAsset;
    use std::path::PathBuf;

    /// 仓例临时目录(std::env::temp_dir + 进程号,同 walker/generator 测试;不引 tempfile)。
    fn test_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("scrollery_face_inst_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 以默认轨为底、把清单期望 size 缩到测试字节数的 profile(避免造 38MB 真文件)。
    fn tiny_profile(detect_size: Option<u64>, embed_size: Option<u64>) -> FaceProfile {
        let mut p = face_profiles().into_iter().next().expect("默认轨必在");
        p.assets = [
            detect_size.map(|s| (p.detect_file.clone(), s)),
            embed_size.map(|s| (p.embed_file.clone(), s)),
        ]
        .into_iter()
        .flatten()
        .map(|(dest, size_bytes)| ModelAsset {
            url: String::new(),
            mirror_url: None,
            dest,
            size_bytes,
            sha256: None,
        })
        .collect();
        p
    }

    #[test]
    fn missing_file_not_installed() {
        let dir = test_dir("missing");
        assert!(!face_variant_installed(
            &dir,
            &tiny_profile(Some(3), Some(3))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn size_match_installed() {
        let dir = test_dir("match");
        let p = tiny_profile(Some(3), Some(5));
        std::fs::write(dir.join(&p.detect_file), b"det").unwrap();
        std::fs::write(dir.join(&p.embed_file), b"embed").unwrap();
        assert!(face_variant_installed(&dir, &p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F5 核心场景:手动放入 LFS pointer 文本/截断文件,size 与清单不符 → 按未安装处理。
    #[test]
    fn size_mismatch_treated_as_not_installed() {
        let dir = test_dir("mismatch");
        let p = tiny_profile(Some(3), Some(5));
        std::fs::write(dir.join(&p.detect_file), b"det").unwrap();
        std::fs::write(
            dir.join(&p.embed_file),
            b"version https://git-lfs.github.com/spec/v1",
        )
        .unwrap();
        assert!(!face_variant_installed(&dir, &p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 清单无该文件条目(如手动导入轨 assets 为空)→ 诚实降级为存在判定。
    #[test]
    fn no_manifest_entry_falls_back_to_existence() {
        let dir = test_dir("fallback");
        let p = tiny_profile(None, None);
        std::fs::write(dir.join(&p.detect_file), b"whatever").unwrap();
        std::fs::write(dir.join(&p.embed_file), b"x").unwrap();
        assert!(face_variant_installed(&dir, &p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
