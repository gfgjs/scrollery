use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tracing::info;

use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::ipc::dir_move;
use crate::ipc::scan_commands::add_scan_root;
use crate::state::{AppState, FILE_JOB_DIR_MOVE};
use crate::utils::path::{resolve_media_path, resolve_within_root};

/// Drop 守卫：把「布局失效 + bump 数据版本」放到 early-return 跳不过的位置。
///
/// 2026-07-06 审查 P1-6：本文件四个批量命令（move/relocate/copy_db/remove_hard）逐条自动提交，
/// 第 N 项失败时前 N-1 项的删行/改行**已生效**，而原实现把失效放在循环之后——中途 `?` 返回即
/// 跳过 bump，items 取数缓存继续按旧 data_version 命中，画廊持续端出已删/已移项（点击报
/// MediaNotFound），直到下次任意写路径 bump 才恢复。守卫在 Drop 时按 `dirty` 决定失效，
/// 成功与报错路径共用同一收尾。
struct InvalidateOnWrite {
    state: Arc<AppState>,
    /// 至少一条写入已生效（调用方在每次 DB 写成功后置 true）。
    dirty: bool,
}

impl InvalidateOnWrite {
    fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::clone(state),
            dirty: false,
        }
    }
}

impl Drop for InvalidateOnWrite {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }
        // Drop 内不可 panic：锁中毒用 into_inner 恢复（保护的是可整体置 None 的纯数据）。
        *self
            .state
            .layout_cache
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.state.bump_data_version();
        // 文件树目录快照一并失效（S 线审查 R-05）：本守卫覆盖的批量命令多数真实改写扫描根内
        // 磁盘，「所有文件」模式的快照会继续端出旧页直到手动刷新。全清而非逐根失效：批量项可
        // 跨多根，从每条 DB 行反查 root_id 只为省几份快照不值当——缓存双维度封顶（≤8 快照），
        // 重建成本 = 下次展开时一次 read_dir+sort。copy_media_items_db 是纯 DB 写、会被顺带
        // 多清一次，无害（快照只存 FS 事实，实体关联在切页时现查）。
        self.state.tree_snapshots.clear();
    }
}

#[tauri::command]
pub async fn create_physical_folder(
    app: AppHandle,
    base_path: String,
    folder_name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    // span 埋点(W1,D-312 info 档:目录创建+扫描根登记,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:create_physical_folder");
    let target_path = if base_path.is_empty() {
        PathBuf::from(&folder_name)
    } else {
        PathBuf::from(&base_path).join(&folder_name)
    };

    let path_str = target_path.to_string_lossy().to_string();
    info!("create_physical_folder: path={}", path_str);

    let created = tokio::fs::create_dir_all(&target_path).await;
    // 失效树快照(R-05):尝试即清,不等成功——create_dir_all 失败前可能已建出中间层目录。
    // 全清成本 ≤8 份快照,见 InvalidateOnWrite::drop 的注。
    state.tree_snapshots.clear();
    created.map_err(|e| AppError::CreateFolder(e.to_string()))?;

    let norm = crate::utils::path::normalize_db_path(&path_str);

    // R1-3：读池查询走 read_blocking。
    let is_within_existing = super::blocking::read_blocking(&state, move |conn| {
        let roots = q::list_scan_roots(conn)?;
        let target_norm = format!("{norm}/");
        Ok(roots.into_iter().any(|r| {
            let r_norm = format!("{}/", r.path);
            target_norm.starts_with(&r_norm)
        }))
    })
    .await?;

    if !is_within_existing {
        // 自动纳入 Scan Roots
        add_scan_root(app.clone(), path_str.clone(), None, state.clone()).await?;
    }

    Ok(path_str)
}

#[tauri::command]
pub async fn move_media_items(
    media_ids: Vec<i64>,
    target_dir: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<i64>> {
    // span 埋点(W1,D-312 info 档:逐条 fs rename + DB 删行,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:move_media_items");
    // R1-3：逐条「读路径(SQL) → rename(fs) → 删行(SQL)」串行交织，且尾部 trash::delete 是
    // 回收站 syscall，整段下沉一个 blocking 任务（tokio::fs 内部本就逐调用 spawn_blocking，
    // 合并后反而少跳线程）；顺序与失败语义不变（中途失败即返回，已移动项的删行已生效）。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let mut guard = InvalidateOnWrite::new(&state);
        let mut moved_ids = vec![];
        let mut dirs_to_check = std::collections::HashSet::new();

        for id in media_ids {
            let src_path = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                let (root, rel, name) = q::get_item_path_info(&pool, id)?;
                resolve_media_path(&root, &rel, &name)
            };

            let src_file_name = PathBuf::from(&src_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            if src_file_name.is_empty() {
                continue;
            }

            let target_path = PathBuf::from(&target_dir).join(&src_file_name);

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| AppError::MoveFile(e.to_string()))?;
            }

            std::fs::rename(&src_path, &target_path)
                .map_err(|e| AppError::MoveFile(e.to_string()))?;

            if let Some(parent) = PathBuf::from(&src_path).parent() {
                dirs_to_check.insert(parent.to_path_buf());
            }

            {
                // 硬删走对账封装(2026-07-10 审查 F7):CASCADE 删脸后同事务重算受影响
                // person 的派生字段,防计数虚高/封面悬挂/幽灵人物。
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                q::delete_media_item_hard(&conn, id)?;
            }
            guard.dirty = true; // 删行已生效——即使后续项失败也必须 bump（P1-6）
            moved_ids.push(id);
        }

        // 如果源文件夹为空，则移入系统回收站
        for dir in dirs_to_check {
            if let Ok(mut entries) = std::fs::read_dir(&dir) {
                // 与原 tokio 版语义一致：仅首个 entry 成功读取才算非空（读取出错视作空）。
                let is_empty = !matches!(entries.next(), Some(Ok(_)));
                if is_empty {
                    if let Err(e) = trash::delete(&dir) {
                        tracing::warn!("Failed to move empty folder to trash: {}", e);
                    } else {
                        tracing::info!("Moved empty folder to trash: {:?}", dir);
                    }
                }
            }
        }

        // S1：移动已逐条删行（DB 成员变化）→ 失效由 guard 在 Drop 统一执行（含报错路径）。
        Ok(moved_ids)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[tauri::command]
pub async fn copy_media_items(
    media_ids: Vec<i64>,
    target_dir: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<i64>> {
    // span 埋点(W1,D-312 info 档:逐条 fs copy(可达 GB 级),真实 IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:copy_media_items");
    // R1-3：同 move_media_items——SQL 与 fs 复制（可达 GB 级）交织，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        // 内层闭包收拢逐条 `?`:无论成功还是中途失败,已复制的文件都落了盘,树快照都要清(R-05)。
        let result = (|| -> Result<Vec<i64>> {
            let mut copied_ids = vec![];

            for id in media_ids {
                let src_path = {
                    let pool = state.db_read_pool.get().map_err(AppError::from)?;
                    let (root, rel, name) = q::get_item_path_info(&pool, id)?;
                    resolve_media_path(&root, &rel, &name)
                };

                let src_file_name = PathBuf::from(&src_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                if src_file_name.is_empty() {
                    continue;
                }

                let target_path = PathBuf::from(&target_dir).join(&src_file_name);

                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| AppError::CopyFile(e.to_string()))?;
                }

                std::fs::copy(&src_path, &target_path)
                    .map_err(|e| AppError::CopyFile(e.to_string()))?;
                copied_ids.push(id);
            }

            Ok(copied_ids)
        })();
        state.tree_snapshots.clear(); // R-05:成功/中途失败都清(部分文件可能已落盘)
        result
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 一次重定位请求：将媒体项 `id` 移动到目录 `target_dir_id`。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelocation {
    pub id: i64,
    pub target_dir_id: i64,
}

/// 重定位结果 — 带上原目录，使调用方能构造精确的逆操作以供撤销/重做。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelocationResult {
    pub id: i64,
    pub from_dir_id: i64,
    pub target_dir_id: i64,
}

/// Relocate media items into target directories by reassigning `directory_id` and moving
/// the file on disk. Unlike `move_media_items` (delete + re-ingest via rescan), this keeps
/// the SAME item id, so thumbnails and AI embeddings stay valid, and it is REVERSIBLE:
/// the returned `from_dir_id` lets the caller record an exact inverse for undo (问题5).
/// 通过重设 `directory_id` 并移动磁盘文件来重定位媒体项。不同于 move_media_items（删行 +
/// 重扫重导入），本命令保留同一 item id，缩略图与 AI 嵌入向量仍有效，且可逆：返回的
/// from_dir_id 让调用方记录精确逆操作以撤销（问题5）。传入不同目标目录即可驱动「拖到文件夹」
/// 与撤销/重做。
#[tauri::command]
pub async fn relocate_media_items(
    moves: Vec<MediaRelocation>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MediaRelocationResult>> {
    // span 埋点(W1,D-312 info 档:逐条 fs rename + DB 改行,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:relocate_media_items");
    // R1-3：同 move_media_items——逐条 SQL 与 fs rename 交织，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let mut guard = InvalidateOnWrite::new(&state);
        let mut results = Vec::new();

        for mv in moves {
            // 解析当前路径 + 当前目录，以及目标目录的绝对路径。
            let (cur_abs, from_dir_id, target_abs_dir) = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                let from_dir_id: i64 = pool
                    .query_row(
                        "SELECT directory_id FROM media_items WHERE id=?1",
                        rusqlite::params![mv.id],
                        |r| r.get(0),
                    )
                    .map_err(|_| AppError::MediaNotFound(mv.id))?;
                let (root, rel, name) = q::get_item_path_info(&pool, mv.id)?;
                let cur_abs = resolve_media_path(&root, &rel, &name);
                let target_abs_dir = q::get_directory_abs_path(&pool, mv.target_dir_id)?;
                (cur_abs, from_dir_id, target_abs_dir)
            };

            if mv.target_dir_id == from_dir_id {
                continue; // already there — no-op | 已在目标 — 跳过
            }

            let file_name = PathBuf::from(&cur_abs)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if file_name.is_empty() {
                continue;
            }

            let target_path = PathBuf::from(&target_abs_dir).join(&file_name);
            if target_path == cur_abs {
                continue;
            }
            if target_path.try_exists().unwrap_or(false) {
                return Err(AppError::MoveFile(format!(
                    "目标已存在同名文件: {}",
                    file_name
                )));
            }

            std::fs::create_dir_all(&target_abs_dir)
                .map_err(|e| AppError::MoveFile(e.to_string()))?;
            std::fs::rename(&cur_abs, &target_path)
                .map_err(|e| AppError::MoveFile(e.to_string()))?;

            {
                let conn = state
                    .db_writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                conn.execute(
                    "UPDATE media_items SET directory_id=?1, updated_at=strftime('%s','now') WHERE id=?2",
                    rusqlite::params![mv.target_dir_id, mv.id],
                )
                .map_err(AppError::Db)?;
            }
            guard.dirty = true; // 改行已生效——即使后续项失败也必须失效（P1-6）

            results.push(MediaRelocationResult {
                id: mv.id,
                from_dir_id,
                target_dir_id: mv.target_dir_id,
            });
        }

        // Invalidate the layout cache so the next compute_layout reflects the moved items.
        // Directory media_count is computed live by the tree query, so no count maintenance.
        // 布局失效 + bump 由 guard 在 Drop 统一执行（含中途报错路径，P1-6）。目录 media_count
        // 由树查询实时计算，无需维护计数列。
        Ok(results)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Result of a media copy — the new row id lets the caller record an exact inverse
/// (delete the copy) for undo/redo.
/// 媒体复制结果 — 新行 id 让调用方记录精确逆操作（删除副本）以撤销/重做。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCopyResult {
    pub src_id: i64,
    pub new_id: i64,
}

/// Copy media items INTO target directories (DB-aware): copy the file on disk and INSERT a
/// new media_items row duplicating the source (new directory_id, same cache_key so the
/// existing thumbnail is reused — no re-generation). Returns the new row ids so the caller
/// can undo precisely by deleting them. Same `moves` shape as relocate for symmetry (问题2).
/// 把媒体项复制到目标目录（DB 感知）：复制磁盘文件并 INSERT 一条复制自源的新 media_items 行
/// （新 directory_id、相同 cache_key 以复用现有缩略图，无需重新生成）。返回新行 id，使调用方
/// 通过删除它们精确撤销。与 relocate 相同的 moves 形状以对称（问题2）。
#[tauri::command]
pub async fn copy_media_items_db(
    moves: Vec<MediaRelocation>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MediaCopyResult>> {
    // span 埋点(W1,D-312 info 档:逐条 fs copy + DB 插行,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:copy_media_items_db");
    // R1-3：同 copy_media_items——逐条 SQL 与 fs copy（可达 GB 级）交织，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let mut guard = InvalidateOnWrite::new(&state);
        let mut results = Vec::new();

        for mv in moves {
            let (cur_abs, target_abs_dir) = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                let (root, rel, name) = q::get_item_path_info(&pool, mv.id)?;
                (
                    resolve_media_path(&root, &rel, &name),
                    q::get_directory_abs_path(&pool, mv.target_dir_id)?,
                )
            };

            let file_name = PathBuf::from(&cur_abs)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if file_name.is_empty() {
                continue;
            }

            let dest = PathBuf::from(&target_abs_dir).join(&file_name);
            if dest == cur_abs {
                continue; // copy onto self — skip | 复制到自身 — 跳过
            }
            if dest.try_exists().unwrap_or(false) {
                return Err(AppError::CopyFile(format!(
                    "目标已存在同名文件: {}",
                    file_name
                )));
            }

            std::fs::create_dir_all(&target_abs_dir)
                .map_err(|e| AppError::CopyFile(e.to_string()))?;
            std::fs::copy(&cur_abs, &dest).map_err(|e| AppError::CopyFile(e.to_string()))?;

            let new_id = {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                q::duplicate_media_item_into_dir(&conn, mv.id, mv.target_dir_id)?
            };
            guard.dirty = true; // 新行已插入——即使后续项失败也必须失效（P1-6）

            results.push(MediaCopyResult {
                src_id: mv.id,
                new_id,
            });
        }

        // S1：新行已插入（成员变化）→ 失效由 guard 在 Drop 统一执行（含报错路径）。
        Ok(results)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Hard-remove media items: move each file to the OS trash and delete its DB row. Used to
/// UNDO a drag-copy (问题2). Missing files / rows are tolerated. Invalidates the layout cache.
/// A trash failure aborts before the corresponding DB delete, so a failed physical move can
/// never silently discard the media row.
/// 硬删除媒体项：把每个文件移入系统回收站并删除其 DB 行。用于撤销拖拽复制（问题2）。
/// 容忍文件/行缺失。使布局缓存失效。
#[tauri::command]
pub async fn remove_media_items_hard(ids: Vec<i64>, state: State<'_, Arc<AppState>>) -> Result<()> {
    // span 埋点(W1,D-312 info 档:回收站 syscall+逐条 DB 删行+派生缓存清理,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:remove_media_items_hard");
    // R1-3：回收站 syscall + 逐条 SQL + 派生缓存文件删除全是阻塞，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let mut guard = InvalidateOnWrite::new(&state);
        // 缓存目录（删除后按 cache_key 清派生孤儿，§3.3.2 / Q7）。
        let cache_dir = state
            .thumb_config
            .read()
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
            .cache_dir
            .clone();
        for id in ids {
            // 删除前取路径 + cache_key（行还在；cache_key 决定其全部派生缓存路径）。
            let (abs, cache_key) = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                let abs = q::get_item_path_info(&pool, id)
                    .ok()
                    .map(|(root, rel, name)| resolve_media_path(&root, &rel, &name));
                let ck: Option<i64> = pool
                    .query_row(
                        "SELECT cache_key FROM media_items WHERE id=?1",
                        rusqlite::params![id],
                        |r| r.get(0),
                    )
                    .ok();
                (abs, ck)
            };
            if let Some(p) = abs {
                let pb = PathBuf::from(&p);
                if pb.exists() {
                    if let Err(e) = trash::delete(&pb) {
                        tracing::warn!("Failed to trash copied file {:?}: {}", pb, e);
                        // 物理回收失败时必须保留 DB 行，避免用户元数据与磁盘文件失配。
                        // 使用稳定 OS 错误码；底层错误仅写日志，不通过 IPC 泄漏。
                        return Err(AppError::os(
                            "无法移入系统回收站 | failed to move file to system trash",
                            e,
                        ));
                    }
                }
            }
            {
                // 硬删走对账封装(2026-07-10 审查 F7),理由同 move_media_items。
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                q::delete_media_item_hard(&conn, id)?;
            }
            // 删行已生效——即使后续项失败也必须失效（P1-6）
            guard.dirty = true;
            // 硬删后即时清理派生缓存孤儿（§3.3.2 / Q7）：按 cache_key 删 4 档缩略图 + ai_thumb +
            // sprite + motion。在 DB 锁之外、best-effort、失败不阻塞删除。软删（is_deleted）不走此路径
            // （保留缓存供恢复，与「离线≠删除」同理）。
            if let Some(ck) = cache_key {
                crate::thumbnail::cache::remove_cache_files_for_key(&cache_dir, ck);
            }
        }

        // S1：硬删（成员变化）→ 失效由 guard 在 Drop 统一执行（含报错路径）。
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ════════════════════════════════════════════════════════════════════════════
// Folder (directory) move / copy / undo-delete
// 文件夹（目录）移动 / 复制 / 撤销删除
// ════════════════════════════════════════════════════════════════════════════

/// 文件夹移动结果 — 足以让前端定位、记录撤销并在半完成时给出重试入口。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDirResult {
    pub dir_id: i64,
    pub root_id: i64,
    pub new_rel_path: String,
    pub affected_dirs: usize,
    pub affected_media: usize,
    /// 文件真实落点（绝对路径）。物理搬运成功时即使索引段失败，前端也能如实告知用户。
    pub target_abs_path: String,
    /// 源目录残留（跨卷删源未完成）：索引已更新，旧路径还有一份副本待清理。
    pub source_leftover: Option<String>,
    /// 未完成阶段日志 id（Some = 仍需重试收尾）。
    pub recovery_id: Option<i64>,
}

/// 文件夹复制结果 — 标识新创建的子树以供撤销，并明确「物理已复制、尚未入库」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyDirResult {
    pub created_root_id: i64,
    pub created_rel_path: String,
    pub created_abs_path: String,
    /// 本次落盘的普通文件数。
    pub copied_files: usize,
    /// 需要调用方触发目标根重扫把新文件入库（可重试；失败不代表复制失败）。
    pub needs_rescan: bool,
}

/// Move a folder (and its whole subtree) into another folder, **preserving metadata**.
/// In-place DB update: rewrites rel_path/parent_id/depth/root_id of every descendant
/// directory plus each affected media item's cache_key / thumb_path / **volume identity**
/// (volume_id + volume_relative_path, so an offline source volume can never mark the
/// already-moved item offline), and relocates cached thumbnail files.
/// Favorites / ratings / AI embeddings (keyed by item id) are kept.
///
/// 将文件夹（及其整个子树）移动到另一个文件夹，**保留元数据**。
/// 原地更新数据库：重写每个后代目录的 rel_path/parent_id/depth/root_id，以及每个受影响媒体项的
/// cache_key / thumb_path / **卷定位**（volume_id + volume_relative_path —— 源卷离线不得影响
/// 已搬到在线目标卷的条目），并重定位已缓存的缩略图文件。收藏 / 评分 / AI 嵌入（按 item id
/// 关联）均保留。
///
/// 执行顺序与失败语义（审查 §7.1-A/B，实现见 `ipc::dir_move`）：
///   1. 校验 + 计算新旧路径；
///   2. 源/目标两个扫描根的闸门按 root_id 升序独占（期间不允许扫描竞争）；
///   3. 落一行阶段日志 → 物理搬运（同卷 rename；跨卷走独占暂存 + 校验 + rename 发布 + 删源，
///      全程不覆盖目标既有路径，源原件不丢）→ 重写身份 → 收尾日志。
/// 物理成功而 DB 失败时返回 `AppError::MoveRecovery`（稳定码 `move_db_pending` + 日志 id +
/// 目标绝对路径），前端据此显示文件真实位置并可重试；重试与启动收尾共用同一段幂等逻辑。
#[tauri::command]
pub async fn move_directory(
    source_dir_id: i64,
    target_dir_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<MoveDirResult> {
    // span 埋点(W1,D-312 info 档:子树重写事务+缓存文件重定位,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:move_directory");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<MoveDirResult> {
        // ── 1. 计划与校验 ── 读元数据、全套校验、算新前缀与规范化绝对路径 ──
        // R1-3：读池查询在下沉 blocking 段内取连接（校验失败以 Err 穿出）。
        let plan = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            dir_move::load_plan(&pool, source_dir_id, target_dir_id)?
        };

        // ── 2. 文件任务门闩 ──
        // 与备份/恢复/导出/编辑共用「同一时刻只有一个重文件任务」的门闩；忙碌即拒绝，不抢占。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let outcome = (|| -> Result<MoveDirResult> {
            // ── 3. 两个根级闸门按固定顺序独占 ──
            // 跨根移动要同时挡住源根与目标根上的扫描：按 root_id 升序取闸（见
            // AppState::with_scan_roots_exclusive），持闸期间新的扫描轮次安装不进来；闸内再确认
            // 当前没有扫描在跑——「检查」与「动手」之间因此不存在扫描竞争者。
            let roots = [plan.source_root_id, plan.target_root_id];
            let mut guard = InvalidateOnWrite::new(&state);
            let outcome = state
                .with_scan_roots_exclusive(&roots, || -> Result<dir_move::MoveDirOutcome> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::InvalidMove(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    // 计划是在取闸之前算的：排队期间另一次移动/删根/重链接都可能让它过期，
                    // 动磁盘之前必须复核源与目标身份（不一致即让用户重试，不用过期计划）。
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        dir_move::verify_plan_current(&conn, &plan)?;
                    }
                    // 缩略图缓存目录（缓存文件重定位用）在闸内取一次快照。
                    let cache_dir = state
                        .thumb_config
                        .read()
                        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
                        .cache_dir
                        .clone();
                    // 磁盘一定会被改写（同卷 rename 或跨卷「暂存→发布→删源」）→ 成败都失效视图。
                    guard.dirty = true;
                    state.tree_snapshots.clear(); // R-05:跨卷中途失败也已改写磁盘
                    dir_move::execute_move(dir_move::MoveDb::State(&state), &plan, &cache_dir)
                })
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })??;

            info!(
                "move_directory: dir={source_dir_id} -> parent={target_dir_id} ({} dirs, {} media) | 文件夹已移动",
                outcome.affected_dirs, outcome.affected_media
            );

            Ok(MoveDirResult {
                dir_id: source_dir_id,
                root_id: plan.target_root_id,
                new_rel_path: plan.new_rel.clone(),
                affected_dirs: outcome.affected_dirs,
                affected_media: outcome.affected_media,
                target_abs_path: plan.dst_abs.to_string_lossy().to_string(),
                source_leftover: outcome.source_leftover,
                recovery_id: outcome.recovery_id,
            })
        })();
        state.release_file_job(FILE_JOB_DIR_MOVE);
        outcome
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Copy a folder (and its whole subtree) into another folder on disk. The new files
/// are ingested as fresh assets via a subsequent re-scan triggered by the frontend.
/// Returns the created subtree's location so an undo can remove exactly it, plus the
/// copied file count and an explicit `needs_rescan` flag (审查 §7.1-B：复制依赖后续扫描
/// 入库，扫描中止会留下「文件在、库未入」，调用方必须能如实显示并重试)。
///
/// 将文件夹（及其整个子树）复制到磁盘上的另一个文件夹。新文件经前端随后触发的
/// 重扫作为全新资产引入。返回新建子树的位置（以便撤销时精确移除）、落盘文件数与
/// `needsRescan` 标记——复制成功但尚未入库是一个**明确、可重试**的中间状态。
///
/// 物理复制走独占暂存 + rename 发布（`dir_move::copy_tree_published`）：目标位置要么不存在、
/// 要么是完整副本，中途失败只清我们自己的暂存目录。
#[tauri::command]
pub async fn copy_directory(
    source_dir_id: i64,
    target_dir_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<CopyDirResult> {
    // span 埋点(W1,D-312 info 档:递归复制整树,真实 IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:copy_directory");
    // R1-3：读池查询与整段复制（文件 IO）下沉同一 blocking 段。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<CopyDirResult> {
        // 校验口径与目录移动共用一份（不可复制扫描根 / 不可入自身 / 不可入自身子树 / 同名冲突）。
        let plan = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            dir_move::load_plan(&pool, source_dir_id, target_dir_id)?
        };
        // 文件任务门闩：与备份/恢复/导出/编辑互斥（同一时刻只允许一个重文件任务）。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let copied_files = (|| -> Result<usize> {
            let roots = [plan.source_root_id, plan.target_root_id];
            state
                .with_scan_roots_exclusive(&roots, || -> Result<usize> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::InvalidMove(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    // 计划在取闸前算出：排队期间可能过期，复制前复核源与目标身份。
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        dir_move::verify_plan_current(&conn, &plan)?;
                    }
                    // R-05:复制中途失败也已落盘一部分,成败都清（暂存不留垃圾）。
                    state.tree_snapshots.clear();
                    dir_move::copy_tree_published(&plan.src_abs, &plan.dst_abs, &plan.name)
                })
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })?
        })();
        state.release_file_job(FILE_JOB_DIR_MOVE);
        let copied_files = copied_files?;

        info!(
            "copy_directory: dir={source_dir_id} -> parent={target_dir_id} ({copied_files} files) | 文件夹已复制"
        );

        Ok(CopyDirResult {
            created_root_id: plan.target_root_id,
            created_rel_path: plan.new_rel.clone(),
            created_abs_path: plan.dst_abs.to_string_lossy().replace('\\', "/"),
            copied_files,
            // 入库由调用方随后触发的目标根重扫完成；未完成时前端据此显示「已复制、未入库」并可重试。
            needs_rescan: true,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 列出未完成的目录移动（审查 §7.1-B：重启后用户要能看到文件真实位置与重试入口）。
///
/// 只读清单，不做任何收尾：收尾可能包含大规模跨卷拷贝，必须由用户显式触发
/// （[retry_directory_move]）或启动期轻量收尾负责。
#[tauri::command]
pub async fn list_pending_directory_moves(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<dir_move::MoveRecoveryReport>> {
    let _span = crate::logging::SpanTimer::info("ipc:list_pending_directory_moves");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<dir_move::MoveRecoveryReport>> {
        let pending = {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            q::list_pending(&conn)?
        };
        Ok(pending.iter().map(dir_move::pending_report).collect())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 重试一条未完成的目录移动（幂等，允许重做物理搬运）。
///
/// 返回收尾后的报告；日志行已不存在时返回 `None`（说明这条已经收尾干净）。
#[tauri::command]
pub async fn retry_directory_move(
    recovery_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<dir_move::MoveRecoveryReport>> {
    let _span = crate::logging::SpanTimer::info("ipc:retry_directory_move");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Option<dir_move::MoveRecoveryReport>> {
        // 1. 读日志行拿两个根，据根取闸（固定顺序），避免与扫描竞争。
        let roots = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            match q::get(&pool, recovery_id)? {
                Some(entry) => [entry.source_root_id, entry.target_root_id],
                None => return Ok(None),
            }
        };
        // 2. 文件任务门闩：收尾可能整树拷贝/删源残留，同属重文件任务。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let report = (|| -> Result<Option<dir_move::MoveRecoveryReport>> {
            let mut guard = InvalidateOnWrite::new(&state);
            state
                .with_scan_roots_exclusive(
                    &roots,
                    || -> Result<Option<dir_move::MoveRecoveryReport>> {
                        if state.any_scan_running(&roots) {
                            return Err(AppError::InvalidMove(
                                "扫描进行中，请稍后再试 | a scan is running, try again later"
                                    .into(),
                            ));
                        }
                        let cache_dir = state
                            .thumb_config
                            .read()
                            .map_err(|e| {
                                AppError::internal("内部任务失败 | internal task failed", e)
                            })?
                            .cache_dir
                            .clone();
                        // 收尾可能改写身份（DB）或磁盘（删源残留）→ 视图与树快照一律失效。
                        guard.dirty = true;
                        state.tree_snapshots.clear();
                        dir_move::retry_entry(
                            dir_move::MoveDb::State(&state),
                            &cache_dir,
                            recovery_id,
                        )
                    },
                )
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })?
        })();
        state.release_file_job(FILE_JOB_DIR_MOVE);
        report
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Undo a folder copy: move the copied folder to the system trash and purge the
/// ingested DB rows (CASCADE). Cancels any running scan on the affected root first
/// to avoid a race with re-ingestion.
///
/// 撤销文件夹复制：将复制出的文件夹移入系统回收站，并清除已登记的数据库行
/// （CASCADE）。会先取消受影响根目录上正在运行的扫描，避免与重新引入竞争。
#[tauri::command]
pub async fn delete_directory_to_trash(
    abs_path: String,
    root_id: i64,
    rel_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:回收站 syscall(整树)+级联删除,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:delete_directory_to_trash");
    // 停止该根目录上进行中的扫描，避免它重新创建我们要删除的行。
    state.cancel_scan(root_id);

    // R1-3：回收站 syscall（整树入回收站，可达秒级）+ 级联删除 SQL 一并下沉 blocking。
    let state = Arc::clone(&state);
    let rel_path_c = rel_path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        // 1. 物理：将文件夹移入系统回收站（可恢复，比硬删除更安全）。
        //
        // 路径源改为权威的 root_id + rel_path（与下方 DB 级联删同源），不再信任前端传入的裸
        // abs_path —— 那绕过了越界校验，可被诱导回收根目录之外的路径。resolve_within_root 做
        // canonicalize + 边界拦截；解析失败（不存在 or 越界）一律跳过物理回收，不回退裸路径。
        let root_path = {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            q::get_scan_root(&conn, root_id)?.path
        };
        match resolve_within_root(&root_path, &rel_path_c) {
            Ok(canonical) => {
                trash::delete(&canonical).map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
                state.tree_snapshots.clear(); // R-05:目录已离开磁盘,树快照立刻失真
            }
            Err(e) => {
                // 目录已不在磁盘（正常）或越界（异常输入）：两种都不该 trash 裸 abs_path，
                // 统一跳过物理回收，DB 清理照常继续。
                tracing::warn!(
                    "delete_directory_to_trash: 跳过物理回收(root_id={root_id}, rel_path={rel_path_c}): {e}"
                );
            }
        }

        // 2. 数据库：删除目录行 → CASCADE 级联移除后代及其媒体。
        {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(dir_id) = q::find_directory_id(&conn, root_id, &rel_path)? {
                q::delete_directory_by_id(&conn, dir_id)?;
                // S1：目录级联删除（成员变化）→ bump。
                state.bump_data_version();
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    info!("delete_directory_to_trash: {abs_path} | 已将复制的文件夹移入回收站并清理数据库行");
    Ok(())
}
