//! IPC commands for collections / favorites (需求7, §3.7).
//! 收藏夹 IPC 命令（需求7, §3.7）。
//!
//! Collections are backed by `albums`/`album_items` — no new mechanism. System folders
//! (kind='system') are virtual (type + is_favorited); user folders store membership in
//! `album_items`. The red-heart favorite path (`toggle_favorite`) is unchanged.
//! 收藏夹由 `albums`/`album_items` 承载，不另造机制。系统夹（kind='system'）虚拟（类型 +
//! is_favorited）；用户夹成员存 `album_items`。红心收藏路径（`toggle_favorite`）保持不变。

use std::sync::Arc;

use tauri::State;

use super::blocking::{read_blocking, write_blocking};
use crate::db::models::Collection;
use crate::db::queries as q;
use crate::error::Result;
use crate::state::AppState;

/// 列出所有收藏夹：4 个系统类型夹在前，用户夹在后。
#[tauri::command]
pub async fn list_collections(state: State<'_, Arc<AppState>>) -> Result<Vec<Collection>> {
    read_blocking(&state, q::list_collections).await
}

/// 列出已软删除的用户收藏夹（最近删除在前），供收藏夹页的「已删除」捞回入口；恢复走 `restore_collection`。
#[tauri::command]
pub async fn list_deleted_collections(state: State<'_, Arc<AppState>>) -> Result<Vec<Collection>> {
    read_blocking(&state, q::list_deleted_collections).await
}

/// Recently-used user collections (for the "加入收藏夹" toast chips). Defaults to 5.
/// 最近使用的用户收藏夹（用于「加入收藏夹」toast 快捷 chips）。默认 5 个。
#[tauri::command]
pub async fn recent_collections(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Collection>> {
    read_blocking(&state, move |c| {
        q::recent_collections(c, limit.unwrap_or(5))
    })
    .await
}

/// 新建一个用户收藏夹。返回其新 id。
#[tauri::command]
pub async fn create_collection(
    name: String,
    icon: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<i64> {
    write_blocking(&state, move |c| {
        q::create_collection(c, &name, icon.as_deref())
    })
    .await
}

/// Soft-delete a user collection (system folders are protected by the query). Undoable via
/// `restore_collection`; the row and its `album_items` survive with `deleted_at` set.
/// 软删除一个用户收藏夹（系统夹由查询层保护）。可经 `restore_collection` 撤销，行与成员保留。
#[tauri::command]
pub async fn delete_collection(album_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| q::delete_collection(c, album_id)).await?;
    // S1：albumId 视图成员随收藏夹删除而变 → bump。
    state.bump_data_version();
    Ok(())
}

/// Restore a soft-deleted user collection (承接删除 undo)。清 `deleted_at`，夹重新出现。
/// 系统夹由查询层保护、为空操作。
#[tauri::command]
pub async fn restore_collection(album_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| q::restore_collection(c, album_id)).await?;
    // S1：albumId 视图成员随收藏夹恢复而变 → bump。
    state.bump_data_version();
    Ok(())
}

/// 重命名一个用户收藏夹（系统夹由查询层保护）。
#[tauri::command]
pub async fn rename_collection(
    album_id: i64,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| q::rename_collection(c, album_id, &name)).await
}

/// 向用户收藏夹添加项。返回插入行数（已去重）。
#[tauri::command]
pub async fn add_to_collection(
    album_id: i64,
    item_ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<usize> {
    let n = write_blocking(&state, move |c| {
        q::add_to_collection(c, album_id, &item_ids)
    })
    .await?;
    // S1：albumId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(n)
}

/// 从收藏夹移除项。返回删除行数。
#[tauri::command]
pub async fn remove_from_collection(
    album_id: i64,
    item_ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<usize> {
    let n = write_blocking(&state, move |c| {
        q::remove_from_collection(c, album_id, &item_ids)
    })
    .await?;
    // S1：albumId 视图成员变化 → bump。
    state.bump_data_version();
    Ok(n)
}
