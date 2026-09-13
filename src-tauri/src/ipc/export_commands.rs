//! 导出整理成果 IPC 命令(方案 A §3/§4 app 接线)。
//!
//! 纯导出引擎在 `crate::export`;本模块负责:选区解析、批量元数据取值、文件任务门闩获取、
//! 分离任务执行(webview 关闭也跑到底、门闩必释放)、进度事件 + 快照 + `export_status` 查询
//! (同 `backup_commands` 姿态:Channel 随发起它的 webview 死,故用 app 级事件 + AppState 快照)。

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db::models::SelectionDescriptor;
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::export::{
    core::{CODE_CANCELLED, CODE_IO},
    ensure_target_writable, is_inside_library, run_export, ExportConflict, ExportItemResult,
    ExportOutcome, ExportParams, ExportSource, NamingScheme,
};
use crate::ipc::blocking::read_blocking;
use crate::ipc::media_commands::current_layout_version;
use crate::state::{AppState, FILE_JOB_EXPORT};

pub const EXPORT_PROGRESS_EVENT: &str = "export:progress";
const CODE_TARGET_INSIDE_LIBRARY: &str = "export_target_inside_library";
const CODE_JOB_BUSY: &str = "file_job_busy"; // 与 backup 域共享稳定码(state.rs FILE_JOB_* 门闩注释)

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportItemResultPayload {
    pub file_name: String,
    pub code: String,
}

impl From<&ExportItemResult> for ExportItemResultPayload {
    fn from(r: &ExportItemResult) -> Self {
        Self {
            file_name: r.file_name.clone(),
            code: r.code.to_string(),
        }
    }
}

/// 导出进度/状态快照(方案 §3.1)。导出是分块任务(逐项复制),`processed`/`total` 支持进度条,
/// 区别于 backup 那种单次 running→终态的粗粒度快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressPayload {
    pub job_id: String,
    /// "idle" | "running" | "completed" | "failed" | "cancelled"。
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub succeeded: Option<u64>,
    /// 单项失败/跳过明细(已封顶,见 `export::core::ITEM_RESULT_CAP`)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ExportItemResultPayload>>,
    /// 单项失败/跳过总数(未封顶)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ExportProgressPayload {
    fn idle() -> Self {
        Self {
            job_id: String::new(),
            status: "idle".into(),
            processed: None,
            total: None,
            final_dir: None,
            succeeded: None,
            items: None,
            items_total: None,
            code: None,
        }
    }

    fn running(job_id: String, processed: u64, total: u64) -> Self {
        Self {
            job_id,
            status: "running".into(),
            processed: Some(processed),
            total: Some(total),
            final_dir: None,
            succeeded: None,
            items: None,
            items_total: None,
            code: None,
        }
    }
}

/// 更新快照并广播事件(快照先行,保证事件消费者查询到的状态不落后于事件;同 `backup_commands`)。
fn publish_export_progress(app: &AppHandle, state: &AppState, payload: ExportProgressPayload) {
    *state
        .export_progress
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(payload.clone());
    let _ = app.emit(EXPORT_PROGRESS_EVENT, payload);
}

/// 导出状态查询(webview 重载恢复用):最近进度快照 + 运行态真相(token 存在)。快照说
/// "running" 但 token 已不在(异常终止)→ 报 "failed",除非正处于「取消中」窗口(同 backup_status)。
#[tauri::command]
pub fn export_status(state: State<'_, Arc<AppState>>) -> Result<ExportProgressPayload> {
    let is_running = state.export_token.is_running();
    let mut payload = state
        .export_progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(ExportProgressPayload::idle);
    if !is_running && payload.status == "running" && !state.is_export_cancelling() {
        payload.status = "failed".into();
        payload.code = Some(CODE_IO.into());
    }
    Ok(payload)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub selection: SelectionDescriptor,
    pub target_parent: String,
    pub naming: NamingScheme,
    pub conflict: ExportConflict,
    pub include_manifest: bool,
    pub source: ExportSource,
    pub allow_inside_library: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreflight {
    pub count: usize,
    /// DB `file_size` 汇总的近似体积(方案 §2.3:「体积标明估算,源在执行中仍可能变化」)。
    pub estimated_bytes: u64,
    pub offline_or_missing_count: usize,
    pub non_zero_rotation_count: usize,
    pub target_writable: bool,
    pub inside_library_warning: bool,
}

async fn resolve_export_ids(
    state: &State<'_, Arc<AppState>>,
    selection: SelectionDescriptor,
) -> Result<Vec<i64>> {
    let version = current_layout_version(state);
    read_blocking(state, move |c| q::resolve_selection(c, &selection, version)).await
}

/// 目标是否库内(方案 §3.2.2):canonicalize 后与全部 `scan_roots.path` 做组件级比较。
async fn check_inside_library(
    state: &State<'_, Arc<AppState>>,
    canon_target: PathBuf,
) -> Result<bool> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        let conn = state_arc.db_read_pool.get().map_err(AppError::from)?;
        let roots = q::list_scan_roots(&conn)?;
        let paths: Vec<String> = roots.into_iter().map(|r| r.path).collect();
        Ok(is_inside_library(&canon_target, &paths))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// `ensure_target_writable` 的 async 包装(审查 P3):目标 canonicalize + 写删探针文件本身是
/// 阻塞 IO,直接同步跑在 async 正文里会在挂死的网络卷/慢速卷上卡住 tokio worker——
/// 对齐 backup 侧同类探测(`backup_commands.rs` 的 `ensure_dest_writable` 调用姿态)下沉
/// `spawn_blocking`。
async fn check_target_writable(target_parent: PathBuf) -> Result<PathBuf> {
    tokio::task::spawn_blocking(move || ensure_target_writable(&target_parent))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// preflight:校验选区规模 + 目的地可写/库内 + 离线缺失/非零旋转计数 + 估算体积(方案 §3.2.1)。
/// 与 `start_export` 不同,preflight 不因目的地不可写/库内而报错——以字段告知,交前端二次确认。
#[tauri::command]
pub async fn preflight_export(
    req: ExportRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<ExportPreflight> {
    // span 埋点(W1,D-312 info 档:选区解析+目的地探测+批量元数据取值,真实 DB/IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:preflight_export");
    let ids = resolve_export_ids(&state, req.selection.clone()).await?;

    let target_parent = PathBuf::from(&req.target_parent);
    let (target_writable, inside_library_warning) = match check_target_writable(target_parent).await
    {
        // 审查 P3:DB 查询失败时保守告警(true)而非静默吞成「不在库内」(false)——preflight
        // 的告知性字段宁可多提醒一次,也不该让瞬时 DB 错误悄悄抹掉库内警告。
        Ok(canon) => {
            let inside = check_inside_library(&state, canon).await.unwrap_or(true);
            (true, inside)
        }
        Err(_) => (false, false),
    };

    // 审查 P2:预检只需计数(体积/离线数/旋转数),不需要 tags/albums——关闭 include_relations
    // 省两段查询 + 两个 Vec<String> 物化(50 万选区峰值可观)。
    let ids_for_meta = ids.clone();
    let meta_map = read_blocking(&state, move |c| {
        q::fetch_export_meta(c, &ids_for_meta, false)
    })
    .await?;

    let mut estimated_bytes: u64 = 0;
    let mut offline_or_missing_count = 0usize;
    let mut non_zero_rotation_count = 0usize;
    for id in &ids {
        match meta_map.get(id) {
            Some(m) => {
                estimated_bytes = estimated_bytes.saturating_add(m.file_size.max(0) as u64);
                if m.availability != "online" {
                    offline_or_missing_count += 1;
                }
                if m.view_rotation != 0 {
                    non_zero_rotation_count += 1;
                }
            }
            None => offline_or_missing_count += 1, // 并发已删除,视同缺失
        }
    }

    Ok(ExportPreflight {
        count: ids.len(),
        estimated_bytes,
        offline_or_missing_count,
        non_zero_rotation_count,
        target_writable,
        inside_library_warning,
    })
}

/// backup_id 生成同款(16 字节随机 hex,兜底 pid+纳秒)。job_id 用于 staging 目录命名与
/// 前端「停止只能取消匹配任务」核对(方案 §3.1)。
fn gen_job_id() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = [0u8; 16];
    if SystemRandom::new().fill(&mut buf).is_ok() {
        return crate::utils::hash::to_hex_lower(&buf);
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), nanos)
}

/// 启动导出(方案 §3.1/§3.2)。获取 A/B 文件任务门闩(占用返回 `file_job_busy`,不抢占)→
/// 分离任务在 blocking 线程内跑 `export::core::run_export` → 进度经事件+快照,终态发布顺序
/// 同 backup:**先落终态快照(`is_current` 判发布权)→ `finish(generation)` → 门闩最后释放**。
#[tauri::command]
pub async fn start_export(
    req: ExportRequest,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    // span 埋点(W1,D-312 info 档:命令自身 await 覆盖选区解析+元数据批量取值这段真实 DB 工作;
    // 实际拷贝在分离 spawn 内跑,不计入本 span,同 fire-and-forget spawner 的既有姿态)。
    let _span = crate::logging::SpanTimer::info("ipc:start_export");
    let ids = resolve_export_ids(&state, req.selection.clone()).await?;

    let target_parent = PathBuf::from(&req.target_parent);
    let allow_inside = req.allow_inside_library;
    let canon_target = check_target_writable(target_parent).await?;
    if !allow_inside && check_inside_library(&state, canon_target.clone()).await? {
        return Err(AppError::Export {
            code: CODE_TARGET_INSIDE_LIBRARY,
            message: "目的地位于扫描根内部,需二次确认 | target inside library".into(),
        });
    }

    // 元数据批量取值(方案 §2.1:分块查询,不拼超长 IN)。按 resolve_selection 给出的顺序重建
    // (保留手排/视图序,§2.1/§2.3);并发已被删除的项直接跳过(预检已提示离线/缺失数,
    // 执行期再消失是正常竞态,不计入结果明细——避免与「正常导出中失败」的稳定码混淆)。
    // manifest 需要 tags/albums,include_relations=true;`.remove()` 而非 `.get().cloned()`——
    // 每项元数据(含 tags/albums 两个 Vec<String>)只此一份,不再多克隆一次(审查 P2)。
    let ids_for_meta = ids.clone();
    let mut meta_map = read_blocking(&state, move |c| {
        q::fetch_export_meta(c, &ids_for_meta, true)
    })
    .await?;
    let items: Vec<crate::db::queries::ExportItemMeta> =
        ids.iter().filter_map(|id| meta_map.remove(id)).collect();

    if !state.try_acquire_file_job(FILE_JOB_EXPORT) {
        return Err(AppError::Export {
            code: CODE_JOB_BUSY,
            message: "已有文件任务在运行 | file job busy".into(),
        });
    }

    let (generation, cancel_token) = state.export_token.begin();
    state.clear_export_cancelling();
    let job_id = gen_job_id();
    let total = items.len() as u64;
    publish_export_progress(
        &app,
        &state,
        ExportProgressPayload::running(job_id.clone(), 0, total),
    );

    let naming = req.naming;
    let conflict = req.conflict;
    let include_manifest = req.include_manifest;
    let source = req.source;
    let job_id_for_task = job_id.clone();
    let job_id_for_return = job_id.clone();
    let state_arc = state.inner().clone();
    let app_task = app.clone();

    tauri::async_runtime::spawn(async move {
        let app_progress = app_task.clone();
        let state_progress = Arc::clone(&state_arc);
        let job_id_progress = job_id_for_task.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<ExportOutcome> {
            let now = chrono::Utc::now();
            let timestamp_label = now.format("%Y%m%d-%H%M%S").to_string();
            let exported_at_utc = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let params = ExportParams {
                target_parent: &canon_target,
                job_id: &job_id_for_task,
                naming,
                conflict,
                include_manifest,
                source,
            };
            run_export(
                &params,
                &items,
                &cancel_token,
                &timestamp_label,
                exported_at_utc,
                |processed, total| {
                    publish_export_progress(
                        &app_progress,
                        &state_progress,
                        ExportProgressPayload::running(
                            job_id_progress.clone(),
                            processed as u64,
                            total as u64,
                        ),
                    );
                },
            )
        })
        .await;

        let terminal = finalize_payload(job_id.clone(), result);
        if state_arc.export_token.is_current(generation) {
            publish_export_progress(&app_task, &state_arc, terminal);
        }
        state_arc.export_token.finish(generation);
        state_arc.clear_export_cancelling();
        state_arc.release_file_job(FILE_JOB_EXPORT);
    });

    Ok(job_id_for_return)
}

/// 把 blocking 结果映射为终态快照(镜像 `backup_commands::finalize_payload`)。
fn finalize_payload(
    job_id: String,
    result: std::result::Result<Result<ExportOutcome>, tokio::task::JoinError>,
) -> ExportProgressPayload {
    match result {
        Ok(Ok(outcome)) => ExportProgressPayload {
            job_id,
            status: "completed".into(),
            processed: None,
            total: None,
            final_dir: Some(outcome.final_dir.to_string_lossy().to_string()),
            succeeded: Some(outcome.succeeded as u64),
            items: Some(outcome.skipped_or_failed.iter().map(Into::into).collect()),
            items_total: Some(outcome.skipped_or_failed_total as u64),
            code: None,
        },
        Ok(Err(AppError::Export { code, .. })) => ExportProgressPayload {
            job_id,
            status: if code == CODE_CANCELLED {
                "cancelled".into()
            } else {
                "failed".into()
            },
            processed: None,
            total: None,
            final_dir: None,
            succeeded: None,
            items: None,
            items_total: None,
            code: Some(code.to_string()),
        },
        Ok(Err(_other)) => ExportProgressPayload {
            job_id,
            status: "failed".into(),
            processed: None,
            total: None,
            final_dir: None,
            succeeded: None,
            items: None,
            items_total: None,
            code: Some(CODE_IO.into()),
        },
        Err(_join) => ExportProgressPayload {
            job_id,
            status: "failed".into(),
            processed: None,
            total: None,
            final_dir: None,
            succeeded: None,
            items: None,
            items_total: None,
            code: Some(CODE_IO.into()),
        },
    }
}

/// `stop_export` 的核对逻辑(纯函数,便于不依赖 `State` 单测)。仅 job_id 匹配不够——终态快照
/// 沿用旧 job_id,旧任务终态落地后、新任务 `begin()` 与新 running 快照发布之间存在窗口;此窗口内
/// 携旧 job_id 的迟到 stop 若只核对 job_id,会命中旧任务的**终态**快照,进而错误取消新任务刚装的
/// token(审查 P1)。加 `status == "running"` 核对堵死此窗口。
fn should_cancel_export(current: Option<&ExportProgressPayload>, job_id: &str) -> bool {
    current.is_some_and(|p| p.job_id == job_id && p.status == "running")
}

/// 取消当前导出(只清本 job 的 staging,已落名的旧正式目录不碰)。`job_id` 须匹配当前运行任务,
/// 不匹配则视为过期引用、静默忽略(方案 §3.1:「停止只能取消匹配任务」)。
#[tauri::command]
pub fn stop_export(job_id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let current = state
        .export_progress
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if should_cancel_export(current.as_ref(), &job_id) {
        drop(current);
        state.cancel_export();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_payload_maps_cancelled_code_to_cancelled_status() {
        let e: Result<ExportOutcome> = Err(AppError::Export {
            code: CODE_CANCELLED,
            message: "x".into(),
        });
        let p = finalize_payload("job1".into(), Ok(e));
        assert_eq!(p.status, "cancelled");
        assert_eq!(p.code.as_deref(), Some(CODE_CANCELLED));
    }

    #[test]
    fn finalize_payload_maps_other_export_error_to_failed() {
        let e: Result<ExportOutcome> = Err(AppError::Export {
            code: CODE_IO,
            message: "x".into(),
        });
        let p = finalize_payload("job1".into(), Ok(e));
        assert_eq!(p.status, "failed");
        assert_eq!(p.code.as_deref(), Some(CODE_IO));
    }

    /// 审查 P1:running 快照 + job_id 匹配 → 允许取消(常规路径)。
    #[test]
    fn should_cancel_export_matches_running_snapshot() {
        let p = ExportProgressPayload::running("job1".into(), 1, 10);
        assert!(should_cancel_export(Some(&p), "job1"));
    }

    /// 审查 P1 核心场景:job_id 匹配但快照已是终态(旧任务收尾落地、新任务尚未 begin 的窗口内,
    /// 迟到的 stop 携旧 job_id 到达)——不得取消,否则会误杀新任务刚装的 token。
    #[test]
    fn should_cancel_export_rejects_terminal_snapshot_even_if_job_id_matches() {
        let p = finalize_payload(
            "job1".into(),
            Ok(Ok(ExportOutcome {
                final_dir: "x".into(),
                succeeded: 1,
                skipped_or_failed: vec![],
                skipped_or_failed_total: 0,
            })),
        );
        assert_eq!(p.status, "completed");
        assert!(!should_cancel_export(Some(&p), "job1"));
    }

    /// job_id 不匹配(过期引用)→ 不取消,与快照状态无关。
    #[test]
    fn should_cancel_export_rejects_mismatched_job_id() {
        let p = ExportProgressPayload::running("job1".into(), 1, 10);
        assert!(!should_cancel_export(Some(&p), "job-other"));
    }

    /// 无快照(从未启动过)→ 不取消。
    #[test]
    fn should_cancel_export_rejects_when_no_snapshot() {
        assert!(!should_cancel_export(None, "job1"));
    }
}
