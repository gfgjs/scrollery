//! 精确内容去重的 Tauri IPC：任务生命周期(启动/停止/状态)。
//!
//! 这里只做输入校验与 DTO 映射；文件摘要在后台任务中完成。所有发送给前端的错误都使用
//! 稳定 code,不泄露 SQL、路径或 OS 原文。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::dedup::task::{DedupPhase, DedupProgress, DedupStatus};
use crate::error::Result;
use crate::state::AppState;

/// 去重状态变更事件名。
///
/// 事件只携带 [`DedupStatusSnapshot`]，前端收到后应以事件快照刷新状态，不把事件当作
/// 全量组/成员数据通道。
pub const DEDUP_STATUS_CHANGED_EVENT: &str = "dedup:progress";

/// 去重分析运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DedupRunStatus {
    Idle,
    Running,
    Stopping,
    Completed,
    Failed,
    Cancelled,
}

/// 状态中按稳定码聚合的错误计数；不携带路径、SQL 或底层错误串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupErrorSummary {
    pub code: String,
    pub count: u64,
}

/// `dedup_status()` 和状态事件的最小稳定快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupStatusSnapshot {
    pub run_id: Option<String>,
    pub status: DedupRunStatus,
    pub phase: String,
    pub items_done: u64,
    pub items_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub groups_found: u64,
    pub potential_logical_bytes: u64,
    pub errors: Vec<DedupErrorSummary>,
    pub waiting_on: Vec<String>,
}

pub(crate) fn progress_to_snapshot(progress: DedupProgress) -> DedupStatusSnapshot {
    let status = match progress.status {
        DedupStatus::Idle => DedupRunStatus::Idle,
        DedupStatus::Running => DedupRunStatus::Running,
        DedupStatus::Stopped => DedupRunStatus::Cancelled,
        DedupStatus::Completed => DedupRunStatus::Completed,
        DedupStatus::Failed => DedupRunStatus::Failed,
    };
    let phase = match progress.phase {
        DedupPhase::Idle => "idle",
        DedupPhase::Quick => "quick",
        DedupPhase::Exact => "exact",
        DedupPhase::Unit => "unit",
    };
    DedupStatusSnapshot {
        run_id: (progress.run_id != 0).then(|| progress.run_id.to_string()),
        status,
        phase: phase.to_string(),
        items_done: progress.items_done,
        items_total: progress.items_total,
        bytes_done: progress.bytes_done,
        bytes_total: progress.bytes_total,
        groups_found: progress.groups_found,
        potential_logical_bytes: progress.potential_logical_bytes,
        errors: progress
            .errors
            .into_iter()
            .map(|error| DedupErrorSummary {
                code: error.code,
                count: error.count,
            })
            .collect(),
        waiting_on: progress.waiting_on,
    }
}

/// 启动（或续跑）精确去重分析。分析口径恒为全部可见、在线资料库,不接受根范围参数;
/// `reset=true` 丢弃上次未完成的运行代次从头开始,`false` 则续跑。
#[tauri::command]
pub async fn start_dedup_analysis(
    reset: bool,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DedupStatusSnapshot> {
    let snapshot = state.inner().with_dedup_lifecycle_read(|| {
        state
            .dedup_task
            .start_with_state_and_app(state.inner().clone(), app, reset)
    })?;
    Ok(progress_to_snapshot(snapshot))
}

/// 停止当前去重分析；停止不删除已经写入的有效 sidecar 结果。
///
/// 接线依赖：调用 `crate::dedup::task::stop`，由任务层 compare-and-clear 当前运行代次。
#[tauri::command]
pub async fn stop_dedup_analysis(state: State<'_, Arc<AppState>>) -> Result<DedupStatusSnapshot> {
    let snapshot = state
        .inner()
        .with_dedup_lifecycle_read(|| state.dedup_task.stop())?;
    Ok(progress_to_snapshot(snapshot))
}

/// 返回可恢复的去重分析状态快照。
///
/// 接线依赖：调用 `crate::dedup::task::status`；不要在 IPC 层从内存数组重建全库进度。
#[tauri::command]
pub fn dedup_status(state: State<'_, Arc<AppState>>) -> Result<DedupStatusSnapshot> {
    Ok(progress_to_snapshot(state.dedup_task.status()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状态快照的稳定 wire 名:前端 dedupStore 直接消费 runId/itemsDone/potentialLogicalBytes/
    /// waitingOn;改名等于打断现行去重进度展示(P13 保留 start/stop/status 三命令的行为表征)。
    #[test]
    fn status_payload_uses_stable_wire_names() {
        let status = DedupStatusSnapshot {
            run_id: Some("run-1".into()),
            status: DedupRunStatus::Running,
            phase: "hashing".into(),
            items_done: 2,
            items_total: 3,
            bytes_done: 10,
            bytes_total: 20,
            groups_found: 1,
            potential_logical_bytes: 100,
            errors: vec![DedupErrorSummary {
                code: "SOURCE_STALE".into(),
                count: 1,
            }],
            waiting_on: vec!["scan".into()],
        };

        let json = serde_json::to_value(status).expect("status serializes");
        assert_eq!(json["runId"], "run-1");
        assert_eq!(json["itemsDone"], 2);
        assert_eq!(json["potentialLogicalBytes"], 100);
        assert_eq!(json["waitingOn"][0], "scan");
    }
}
