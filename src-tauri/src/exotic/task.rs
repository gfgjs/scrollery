// src-tauri/src/exotic/task.rs
//! 冷门格式插件 · 任务模型（v3 §5.3 / Part1 §1.3）。
//!
//! `exotic_tasks` 是「处理真相」：能力级任务、独立重试/失效，**不是** `media_items` 上的状态列。
//! 状态机：pending → processing →（done | retryable_error | terminal_error）。
//! 未安装/未授权/禁用**不**写任务状态——由 Scheduler 领取时经 `FormatResolution` 门控（v3 §5.3）。

use serde::{Deserialize, Serialize};

/// 任务状态码（与 DB `exotic_tasks.status` 整数列一一对应）。
/// 用显式整数值固定语义，避免后续重排枚举顺序导致旧库语义漂移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExoticTaskStatus {
    /// 0 — 待处理（含 SourceChanged 失效后重置）。
    Pending,
    /// 1 — 处理中（已原子领取，写 claimed_at + lease_owner）。
    Processing,
    /// 2 — 完成（产物已落盘、指纹有效）。
    Done,
    /// 3 — 可重试错误（崩溃/超时/暂时 IO）；按指数退避到 next_retry_at 再入队。
    RetryableError,
    /// 4 — 终态错误（不支持变体/输出非法）；等源文件或 Worker 版本变化才失效。
    TerminalError,
}

impl ExoticTaskStatus {
    /// 转为 DB 存储的整数。
    pub fn as_i64(self) -> i64 {
        match self {
            ExoticTaskStatus::Pending => 0,
            ExoticTaskStatus::Processing => 1,
            ExoticTaskStatus::Done => 2,
            ExoticTaskStatus::RetryableError => 3,
            ExoticTaskStatus::TerminalError => 4,
        }
    }

    /// 从 DB 整数解析；未知值视为 None（让调用方按损坏行处理，而非误判为某状态）。
    pub fn from_i64(v: i64) -> Option<Self> {
        Some(match v {
            0 => ExoticTaskStatus::Pending,
            1 => ExoticTaskStatus::Processing,
            2 => ExoticTaskStatus::Done,
            3 => ExoticTaskStatus::RetryableError,
            4 => ExoticTaskStatus::TerminalError,
            _ => return None,
        })
    }

    /// 是否已完成（done）——跨流水线门控用：done 才放行 CLIP/face。
    pub fn is_done(self) -> bool {
        matches!(self, ExoticTaskStatus::Done)
    }
}

/// `exotic_tasks` 行的内存表示（DAO 读出后用）。
#[derive(Debug, Clone)]
pub struct ExoticTaskRow {
    pub id: i64,
    pub item_id: i64,
    pub plugin_id: String,
    pub capability: String,
    pub status: ExoticTaskStatus,
    pub input_fingerprint: Option<String>,
    pub attempts: i64,
    pub next_retry_at: Option<i64>,
    pub claimed_at: Option<i64>,
    pub lease_owner: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub output_path: Option<String>,
    pub worker_version: Option<String>,
}
