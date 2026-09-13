// src-tauri/src/scanner/fast_scan/payload.rs
//! 扫描 IPC 载荷 DTO(自 fast_scan.rs 结构性拆分,tierB-1)。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressPayload {
    pub root_id: i64,
    pub run_id: String,
    pub scanned: u64,
    pub total: u64,
    pub processed_bytes: u64,
    /// 单遍流式扫描期间总量未知；收尾前为 `None`。
    pub total_bytes: Option<u64>,
    pub current_dir: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCompletedPayload {
    pub root_id: i64,
    pub run_id: String,
    pub total_items: u64,
    pub total_bytes: u64,
    pub elapsed_ms: u64,
    /// 本次缺失检测标记为 `availability='missing'` 的项数（四道闸通过才 >0）。前端可据此 toast 提示。
    pub marked_missing: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanErrorPayload {
    pub root_id: i64,
    pub run_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScanChannelPayload {
    Progress(ScanProgressPayload),
    Completed(ScanCompletedPayload),
    Error(ScanErrorPayload),
}
