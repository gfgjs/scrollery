//! 后台派生流水线的 IPC 命令（§5）：开始 / 暂停 / 停止 / 状态。
//! 与 AI 三按钮控制面板（开始/暂停/停止）及其续传语义同构。

use std::sync::Arc;

use tauri::{AppHandle, State};
use tracing::info;

use crate::db::models::DerivationStatusSummary;
use crate::db::queries::{
    count_derivations_by_status_for_kinds, get_config, reset_derivations_by_kinds, set_config,
};
use crate::derive::derivation_counts;
use crate::derive::{start_derivation_pipeline, DerivationKind};
use crate::error::{AppError, Result};
use crate::state::AppState;

/// 持久化「期望运行」标志并启动流水线。
fn launch_derivation_pipeline(
    app: AppHandle,
    state: &Arc<AppState>,
    kind_filter: Option<Vec<DerivationKind>>,
    operation_id: Option<String>,
) {
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "derivation_active", "1");
    }
    let (generation, token) = state.new_derivation_token();
    start_derivation_pipeline(
        app,
        Arc::clone(state),
        generation,
        token,
        kind_filter,
        operation_id,
    );
}

/// 把可选 kind 字符串列表解析为类型化 kind(空列表 ⇒ 无过滤)。
fn parse_kind_filter(kinds: Option<Vec<String>>) -> Result<Option<Vec<DerivationKind>>> {
    match kinds {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|s| {
                // 未知 kind = 不受支持的派生类型 → 稳定 code=UnsupportedFormat(P1-8)。
                DerivationKind::from_str(s).ok_or_else(|| {
                    AppError::UnsupportedFormat(format!(
                        "Unknown derivation kind: {s} | 未知派生 kind"
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        _ => Ok(None),
    }
}

/// 启动（或续传）派生流水线。`kinds` 可选地限定一组 kind（如 `["video_cover","video_keyframes"]`）；
/// 省略则处理所有。`reset=true` 先把所列 kind 的已完成/失败行退回待处理（全量重做;必须带 `kinds`,
/// 防误调把所有 kind 进度清空）。其余情况已完成项跳过，仅运行待处理 / 被中断任务（孤儿恢复 +
/// backfill 在流水线内部完成）。带 `kinds` 的手动运行对所列 kind 覆盖 `enable_*` 后台开关（显式意图优先）。
#[tauri::command]
pub async fn start_derivation(
    app: AppHandle,
    kinds: Option<Vec<String>>,
    reset: Option<bool>,
    // 前端 invoke 封装显式生成并透传(方案 §3.3/S2 operation_id):span 不跨 spawn_blocking(D-304),
    // 只能靠字符串参数一路带到下面 tokio::spawn 里的完成/失败汇总日志。非全量调用点都带(如自动
    // 续传/配置变更触发的启动)——省略时下游日志的 operation_id 字段就是 null,不强求(方案 §9.4 S2)。
    operation_id: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = Arc::clone(&state);
    let kind_filter = parse_kind_filter(kinds)?;
    let reset = reset.unwrap_or(false);
    if reset && kind_filter.is_none() {
        return Err(AppError::UnsupportedFormat(
            "reset requires an explicit kinds list | 全量重做必须显式指定 kinds".to_string(),
        ));
    }

    // 取消任何现有运行，然后重新开始（续传会接续待处理/被中断项）。
    state.cancel_derivation();
    info!(
        operation_id = operation_id.as_deref(),
        "Starting/resuming derivation pipeline (filter={:?}, reset={}) | 启动/续传派生流水线",
        kind_filter
            .as_ref()
            .map(|ks| ks.iter().map(|k| k.as_str()).collect::<Vec<_>>()),
        reset
    );
    // launch 内含 set_config 落库（R1-3：rusqlite 离开 tokio worker）。
    tokio::task::spawn_blocking(move || -> Result<()> {
        // 全量重做:先把所列 kind 的 done/error 行退回 pending(status 2/3→0,清 payload)。
        // 不动 media_items 的封面镜像——新封面落地时 writer 同路径覆盖回填,过渡期旧图仍可显示。
        // 复位失败向 IPC 传播且不启动流水线(审查 F-06):旧实现 warn 后照常 launch 并返 Ok,
        // 用户请求重做已完成项,实际只跑了原有 pending 项还得到「成功」。
        if reset {
            if let Some(filter) = &kind_filter {
                let kind_strs: Vec<&str> = filter.iter().map(|k| k.as_str()).collect();
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                let n = reset_derivations_by_kinds(&conn, &kind_strs)?;
                info!(
                    "Full re-extract: {} derivation row(s) reset to pending | 全量重做:{} 行退回待处理",
                    n, n
                );
            }
        }
        launch_derivation_pipeline(app, &state, kind_filter, operation_id);
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    Ok(())
}

/// 暂停运行中的流水线：取消但保留 active 标志，以便之后续传（含下次启动自动续传）。
/// 在途任务下次运行时恢复为待处理。
#[tauri::command]
pub async fn pause_derivation(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("Pausing derivation pipeline (keeps resume flag) | 暂停派生流水线（保留续传标志）");
    state.cancel_derivation();
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "derivation_active", "1");
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    Ok(())
}

/// 停止流水线并清除续传标志（不再自动续传）。进度保留（已完成派生不删），仅放弃「自动继续」意图。
#[tauri::command]
pub async fn stop_derivation(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("Stopping derivation pipeline (clears resume flag) | 停止派生流水线（清除续传标志）");
    state.cancel_derivation();
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "derivation_active", "0");
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    Ok(())
}

/// 派生 UI 的状态摘要：按状态计数 + 运行/期望标志。`kinds` 可选地限定计数范围
/// （视频控制卡只统计封面/关键帧行）;运行/期望标志始终描述全局唯一流水线。
#[tauri::command]
pub async fn derivation_status(
    kinds: Option<Vec<String>>,
    state: State<'_, Arc<AppState>>,
) -> Result<DerivationStatusSummary> {
    let state = Arc::clone(&state);
    // 复用 parse 做未知 kind 校验,再退回字符串给 SQL IN 过滤。
    let kind_strs: Option<Vec<String>> =
        parse_kind_filter(kinds)?.map(|ks| ks.iter().map(|k| k.as_str().to_string()).collect());
    tokio::task::spawn_blocking(move || -> Result<DerivationStatusSummary> {
        let (pending, processing, done, error) = match &kind_strs {
            Some(ks) => {
                let conn = state.db_read_pool.get()?;
                count_derivations_by_status_for_kinds(&conn, ks)?
            }
            None => derivation_counts(&state)?,
        };

        let is_running = state.is_derivation_running();
        let conn = state.db_read_pool.get()?;
        let active = get_config(&conn, "derivation_active")
            .unwrap_or_default()
            .map(|v| v == "1")
            .unwrap_or(false);

        Ok(DerivationStatusSummary {
            pending,
            processing,
            done,
            error,
            is_running,
            active,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}
