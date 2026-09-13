//! 已知卷面板命令（T13 §3.7 离线 UX）。
//!
//! 「已知卷」= 应用登记过的物理卷（U盘/移动硬盘/网络盘/本机盘）。在线态由 volume_watch 后台
//! 每 15s 对账维护（`volumes.is_online`），本模块只**读**该真相 + 提供**重命名 / 忘记**两个用户操作。
//! DTO 以 camelCase 序列化，前端类型直接对齐。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::blocking::{read_blocking, write_blocking};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::state::AppState;

/// 卷标最大长度（防滥用；UI 亦应限制）。
const MAX_LABEL_LEN: usize = 100;

/// 「已知卷」面板行（后端 `Volume` + 未删除媒体数的投影）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub id: i64,
    pub stable_id: String,
    /// 卷标（用户可改；未命名为 null，前端回退 stable_id / 挂载点展示）。
    pub label: Option<String>,
    /// 卷类型：local / removable / network。
    pub kind: String,
    /// 最近挂载点 / 盘符（离线后仍可提示「上次在 X:」）。
    pub last_mount_path: Option<String>,
    /// 当前在线态（volume_watch 15s 对账维护）。
    pub is_online: bool,
    /// 最近在线 unix 秒（离线时长展示用）。
    pub last_seen: Option<i64>,
    /// 该卷上未删除的媒体数（回收站项不计）。
    pub item_count: i64,
}

/// 列出全部已知卷（含在线态 + 媒体数），供设置页「已知卷」面板。
#[tauri::command]
pub async fn list_volumes(state: State<'_, Arc<AppState>>) -> Result<Vec<VolumeInfo>> {
    let rows = read_blocking(&state, q::list_volumes_with_item_counts).await?;
    Ok(rows
        .into_iter()
        .map(|(v, item_count)| VolumeInfo {
            id: v.id,
            stable_id: v.stable_id,
            label: v.label,
            kind: v.kind.as_str().to_string(),
            last_mount_path: v.last_mount_path,
            is_online: v.is_online,
            last_seen: v.last_seen,
            item_count,
        })
        .collect())
}

/// 重命名卷标（「已知卷」面板改名）。空标签拒绝（防误清），过长截断守卫。
#[tauri::command]
pub async fn rename_volume(
    volume_id: i64,
    label: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(AppError::System(
            "卷名不能为空 | Volume label cannot be empty".into(),
        ));
    }
    // 按字符（非字节）截断，避免多字节 UTF-8 边界内切断。
    let label: String = trimmed.chars().take(MAX_LABEL_LEN).collect();
    write_blocking(&state, move |c| {
        q::rename_volume_label(c, volume_id, &label)
    })
    .await
}

/// 忘记卷登记（用户不再管理该盘）。经 FK `ON DELETE SET NULL`：其 scan_roots / media_items 的
/// `volume_id` 自动置 NULL——**媒体行本身保留**（离线≠删除），只是不再随该卷插拔联动可用态。
/// 若日后重新接入并扫描，`upsert_volume` 会按 stable_id 重新登记并绑定。
#[tauri::command]
pub async fn forget_volume(volume_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| q::delete_volume(c, volume_id)).await
}
