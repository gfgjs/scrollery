//! 文档相关 IPC 命令。
//!
//! 文档缩略图由前端离屏渲染并回传：
//!  - `list_pending_doc_thumbs`：前端领取待渲染的 pdf/svg 文档。
//!  - `store_doc_thumbnail`：前端回传渲染好的 PNG 字节 → 落盘缩略图缓存 + 回填 media_items/布局缓存。
//!
//! 本模块还处理文档版本、编辑、校对与阅读进度。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter, State};

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use xxhash_rust::xxh3::xxh3_64;

use super::blocking::{read_blocking, write_blocking};
use crate::db::models::{DiffOp, DocumentVersion, ReplacementRule, ThumbResult};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::scanner::enricher::MediaEnrichedPayload;
use crate::state::AppState;
use crate::thumbnail::generator::{encode_media_step_with_snapshot, snap_to_tier, ThumbConfig};
use crate::utils::format::doc_subtype;
use crate::utils::path::resolve_media_path;

/// 已授权且已入库的本地文档链接；片段已单次 URL 解码，不向前端返回磁盘路径。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLinkTarget {
    pub item_id: i64,
    pub fragment: Option<String>,
}

/// 以原资产目录解析相对文档链接，仅打开同一扫描根内已入库的 TXT/MD。
#[tauri::command]
pub async fn resolve_document_link(
    item_id: i64,
    href: String,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentLinkTarget> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || resolve_document_link_sync(&state, item_id, &href))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

fn resolve_document_link_sync(
    state: &AppState,
    item_id: i64,
    href: &str,
) -> Result<DocumentLinkTarget> {
    let invalid = || {
        AppError::PathResolution(
            "文档链接无效或不可用 | document link invalid or unavailable".into(),
        )
    };
    let (path, fragment) = href
        .split_once('#')
        .map_or((href, None), |(path, hash)| (path, Some(hash)));
    let path = decode_document_link_part(path)?;
    let fragment = fragment
        .map(decode_document_link_part)
        .transpose()?
        .filter(|value| !value.is_empty());
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':', '?']) {
        return Err(invalid());
    }
    let conn = state.db_read_pool.get()?;
    let source = q::get_media_item(&conn, item_id)?;
    if source.is_deleted || source.companion_of.is_some() || source.media_type != "document" {
        return Err(AppError::MediaNotFound(item_id));
    }
    if !matches!(
        source.file_format.to_ascii_lowercase().as_str(),
        "txt" | "md"
    ) {
        return Err(AppError::UnsupportedFormat(
            "文档链接仅支持 TXT/MD | document links support TXT/MD only".into(),
        ));
    }
    let directory = q::get_directory(&conn, source.directory_id)?;
    let root = q::get_scan_root(&conn, directory.root_id)?;
    let source_rel = if directory.rel_path.is_empty() {
        source.file_name.clone()
    } else {
        format!("{}/{}", directory.rel_path, source.file_name)
    };
    crate::utils::path::resolve_within_root(&root.path, &source_rel)?;

    // URL 的父级段先在原资产目录内消解；越过扫描根即拒，现有路径守卫仍保持拒绝原始 .. 的契约。
    let mut parts: Vec<&str> = directory
        .rel_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(invalid());
                }
            }
            _ => {
                #[cfg(windows)]
                if part.ends_with(['.', ' ']) || part.contains(['<', '>', '"', '|', '*']) {
                    return Err(invalid());
                }
                parts.push(part);
            }
        }
    }
    let target = crate::utils::path::resolve_within_root(&root.path, &parts.join("/"))?;
    let root_canonical = std::fs::canonicalize(&root.path).map_err(|_| invalid())?;
    let relative = target
        .strip_prefix(&root_canonical)
        .map_err(|_| invalid())?;
    if !target.is_file() {
        return Err(invalid());
    }
    let file_name = relative
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid)?;
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "md" | "txt") {
        return Err(AppError::UnsupportedFormat(
            "文档链接仅支持 TXT/MD | document links support TXT/MD only".into(),
        ));
    }
    // canonical 路径还原真实大小写和根内链接目的地，按目录+文件名索引查找，不扫描整个媒体库。
    let parent = relative
        .parent()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid)?
        .replace('\\', "/");
    let directory_id = q::find_directory_id(&conn, root.id, &parent)?.ok_or_else(invalid)?;
    let mut media = q::map_media_entities(&conn, directory_id, &[file_name.to_owned()])?;
    let (target_id, media_type) = media.remove(file_name).ok_or_else(invalid)?;
    let target_item = q::get_media_item(&conn, target_id)?;
    if media_type != "document" || !target_item.file_format.eq_ignore_ascii_case(&extension) {
        return Err(AppError::UnsupportedFormat(
            "文档链接仅支持 TXT/MD | document links support TXT/MD only".into(),
        ));
    }
    Ok(DocumentLinkTarget {
        item_id: target_id,
        fragment,
    })
}

// 不借助宽松 URL 解码：畸形转义、非 UTF-8 与控制字符不能变成另一个磁盘目标；百分号只解一轮。
fn decode_document_link_part(value: &str) -> Result<String> {
    let invalid =
        || AppError::PathResolution("文档链接编码无效 | invalid document link encoding".into());
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|byte| char::from(*byte).to_digit(16))
                .ok_or_else(invalid)?;
            let low = bytes
                .get(index + 2)
                .and_then(|byte| char::from(*byte).to_digit(16))
                .ok_or_else(invalid)?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| invalid())?;
    if decoded.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(decoded)
}

/// 前端创建/更新替换规则的载荷（§5.2）。`id=None` → 插入。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementInput {
    pub id: Option<i64>,
    pub scope_kind: String,
    pub scope_id: Option<i64>,
    pub find: String,
    pub replace: String,
    pub is_regex: bool,
    pub enabled: bool,
    pub sort_order: i64,
}

/// 列出某作用域的替换规则（规则编辑器）。`scope_id=None` → 全局规则（§5.2）。
#[tauri::command]
pub async fn list_replacements(
    scope_kind: String,
    scope_id: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ReplacementRule>> {
    read_blocking(&state, move |c| {
        q::list_replacements(c, &scope_kind, scope_id)
    })
    .await
}

/// 对某项实际生效的规则 = 启用的 global + item 作用域，按序（§5.2）。
#[tauri::command]
pub async fn get_effective_replacements(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ReplacementRule>> {
    read_blocking(&state, move |c| q::get_effective_replacements(c, item_id)).await
}

/// 插入/更新一条替换规则（§5.2）。返回行 id。
#[tauri::command]
pub async fn upsert_replacement(
    rule: ReplacementInput,
    state: State<'_, Arc<AppState>>,
) -> Result<i64> {
    write_blocking(&state, move |c| {
        q::upsert_replacement(
            c,
            rule.id,
            &rule.scope_kind,
            rule.scope_id,
            &rule.find,
            &rule.replace,
            rule.is_regex,
            rule.enabled,
            rule.sort_order,
        )
    })
    .await
}

/// 按 id 删除替换规则（§5.2）。
#[tauri::command]
pub async fn delete_replacement(id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| q::delete_replacement(c, id)).await
}

// ── Document versions (§5.3) ──────────────────────────────────────────────────

/// 某项的版本存储目录：`<appData>/documents/<item_id>/`。
fn documents_dir(state: &AppState, item_id: i64) -> PathBuf {
    // 从 app_data_dir 真值派生,不得用 log_dir.parent() 反推(2026-07-10 审查 A1,理由同 models_dir)。
    state
        .app_data_dir
        .join("documents")
        .join(item_id.to_string())
}

/// 读取某版本引用的文本：`None` = 源文件基线；`Some(id)` = 已存版本。
///
/// 全文编辑、版本 diff 与阅读章节共用来源编码：源文件尊重每书覆盖，内部版本固定 UTF-8。
fn read_ref_text(state: &AppState, item_id: i64, r: Option<i64>) -> Result<String> {
    let version = match r {
        None => None,
        Some(vid) => {
            let pool = state.db_read_pool.get()?;
            Some(q::get_version(&pool, vid)?.ok_or_else(|| {
                AppError::Internal(format!("version {vid} not found | 版本不存在"))
            })?)
        }
    };
    let reference = resolve_text_ref(state, item_id, version, None)?;
    let bytes = std::fs::read(&reference.path).map_err(AppError::from)?;
    Ok(crate::reader::encoding::decode_bytes(&bytes, reference.encoding_override.as_deref()).text)
}

/// 列出文档的所有版本，最旧在前（§5.3）。
#[tauri::command]
pub async fn list_versions(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DocumentVersion>> {
    read_blocking(&state, move |c| q::list_versions(c, item_id)).await
}

/// 文档当前版本（若有，§5.3）。`None` → 以源文件为当前。
#[tauri::command]
pub async fn get_current_version(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<DocumentVersion>> {
    read_blocking(&state, move |c| q::get_current_version(c, item_id)).await
}

/// Effective document text = current version's content if set, else the source file (§5.3).
/// Used by the viewer/editor so a "set current" version is what's read/edited.
/// 文档生效文本 = 已设当前版本的内容，否则源文件（§5.3）。供查看器/编辑器读取与编辑。
#[tauri::command]
pub async fn get_document_text(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<String> {
    // span 埋点(W1,D-312 info 档:load_document 类,读文件+解码,真实 IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:get_document_text");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<String> {
        let cur = {
            let pool = state.db_read_pool.get()?;
            q::get_current_version(&pool, item_id)?
        };
        read_ref_text(&state, item_id, cur.map(|v| v.id))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 单事务写一个 appdata 版本:
/// 插行(拿 id)→ 派生 `{id}.{ext}` 路径 → 写原子文件 → 回填路径 → commit。
///
/// **文件写在 commit 之前** → 已提交的行必有文件在盘;文件写失败则整事务回滚、行不落库,
/// 最多留可 GC 的孤儿 tmp(write_atomic 自身失败即清)。避免提交行指向空路径或缺失文件。
/// **须在持 `db_writer` 锁的 blocking 上下文调用**。
/// `&tx` 经 `Transaction: Deref<Target=Connection>` 自动强转喂给收 `&Connection` 的 DAO。
#[allow(clippy::too_many_arguments)]
fn write_version_tx(
    conn: &rusqlite::Connection,
    dir: &Path,
    item_id: i64,
    parent: Option<i64>,
    label: Option<&str>,
    source: &str,
    ext: &str,
    text: &str,
) -> Result<i64> {
    let hash = format!("{:016x}", xxh3_64(text.as_bytes()));
    let tx = conn.unchecked_transaction()?;
    let id = q::insert_version(
        &tx,
        item_id,
        parent,
        label,
        "appdata",
        "",
        source,
        Some(&hash),
    )?;
    let path = dir.join(format!("{id}.{ext}"));
    // 原子落盘(2026-07-06 审查 R9):版本文件是可恢复性依据,不留半截。
    crate::thumbnail::generator::write_atomic(&path, text.as_bytes()).map_err(AppError::from)?;
    q::update_version_path(&tx, id, &path.to_string_lossy())?;
    tx.commit()?;
    Ok(id)
}

fn overwrite_document_source(
    conn: &rusqlite::Connection,
    dir: &Path,
    item_id: i64,
    src_path: &Path,
    ext: &str,
    content: &str,
) -> Result<i64> {
    // 源文件可能是旧编码；备份版本按项目版本模型统一存 UTF-8。
    // 除源不存在外，读取失败必须中止，不能在没有旧源备份时覆盖。
    let src_bytes = match std::fs::read(src_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(AppError::from(e)),
    };
    let source_prefs = q::get_reader_book_prefs(conn, item_id)?;
    let source_encoding = source_prefs.as_deref().and_then(stored_encoding_override);
    let src_text =
        crate::reader::encoding::decode_bytes(&src_bytes, source_encoding.as_deref()).text;
    let backup_id = write_version_tx(
        conn,
        dir,
        item_id,
        None,
        Some("覆盖前自动备份 | auto-backup"),
        "user",
        ext,
        &src_text,
    )?;
    // 覆盖后源文件就是生效稿，不能继续让旧的 is_current 版本遮住新内容。
    // 备份先独立提交；文件替换失败时回滚指针与编码偏好，旧源与备份都仍可用。
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE document_versions SET is_current=0 WHERE item_id=?1",
        params![item_id],
    )?;
    // 保存契约统一输出 UTF-8，新字符可能超出旧编码；只更新编码，保留其余每书偏好。
    // 先写事务内状态再替换文件，JSON/数据库写入失败时不触碰源文件。
    tx.execute(
        "INSERT INTO reader_book_prefs (item_id, prefs, updated_at)
         VALUES (?1, json_object('encoding', 'UTF-8'), strftime('%s','now'))
         ON CONFLICT(item_id) DO UPDATE SET
             prefs=json_set(reader_book_prefs.prefs, '$.encoding', 'UTF-8'),
             updated_at=strftime('%s','now')",
        params![item_id],
    )?;
    // 自写必须显式失效索引，兼顾低精度文件系统或保留时间戳的等长修改；替换失败随事务回滚。
    tx.execute(
        "DELETE FROM text_book_index WHERE item_id=?1",
        params![item_id],
    )?;
    crate::thumbnail::generator::write_atomic(src_path, content.as_bytes())
        .map_err(AppError::from)?;
    // 文件替换后若 commit 失败则报错：新源与已提交旧源备份均保留。
    // 这不是跨文件/DB 原子事务；不能盲目回写原文，避免覆盖期间发生的外部编辑。
    tx.commit()?;
    Ok(backup_id)
}

/// Save edited text. `target`:
///   - `"version"` (default) → new snapshot in appData (does NOT enter the gallery, §5.3 D2).
///   - `"overwrite"` → overwrite the SOURCE file, after auto-backing-up the old source as a version
///     (advanced; the frontend gates this behind a confirm). Returns the new/backup version id.
/// 保存编辑后的文本。`target`：`"version"`（默认，appData 新快照，不进画廊）；
/// `"overwrite"`（覆盖源文件，先把旧源自动备份为一个版本；高级，前端二次确认）。返回版本 id。
#[tauri::command]
pub async fn save_version(
    item_id: i64,
    content: String,
    label: Option<String>,
    parent_id: Option<i64>,
    target: String,
    // 版本来源：'user'（默认）| 'ai-remote' | 'ai-local'（§5.4 AI 校对接受后存为新版本）。
    source: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<i64> {
    // span 埋点(W1,D-312 info 档:版本写入/覆盖源文件,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:save_version");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<i64> {
        // 文档存储一致性门(方案 B §3.1):持 read guard 覆盖整个 DB+文件写序,与数据备份的
        // write guard 互斥——备份不会捕获本次保存的中间态(插了行但文件未落)。仅在本 blocking
        // 闭包内持有,绝不跨 .await(项目硬约束)。
        let _doc_guard = state
            .document_storage_guard
            .read()
            .unwrap_or_else(|e| e.into_inner());

        let (root, rel, name) = {
            let pool = state.db_read_pool.get()?;
            q::get_item_path_info(&pool, item_id)?
        };
        let src_path = resolve_media_path(&root, &rel, &name);
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_string();
        let dir = documents_dir(&state, item_id);
        std::fs::create_dir_all(&dir).map_err(AppError::from)?;

        // 单事务写版本(方案 B §3.1):db_writer 写锁在小体量文档文件落盘期间被持有
        // (文档快照通常 KB~MB 级,可接受),换取「已提交行必有文件」的一致性。逻辑抽到
        // `write_version_tx` 负责事务与文件写入的一致性。
        let write_version =
            |label: Option<&str>, parent: Option<i64>, src: &str, text: &str| -> Result<i64> {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                write_version_tx(&conn, &dir, item_id, parent, label, src, &ext, text)
            };

        if target == "overwrite" {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            overwrite_document_source(&conn, &dir, item_id, Path::new(&src_path), &ext, &content)
        } else {
            write_version(
                label.as_deref(),
                parent_id,
                source.as_deref().unwrap_or("user"),
                &content,
            )
        }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 将某版本设为当前（传 `None` 回到源文件基线）（§5.3）。
#[tauri::command]
pub async fn set_current_version(
    item_id: i64,
    version_id: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        q::set_current_version(c, item_id, version_id)
    })
    .await
}

/// 删除一个版本（行 + 文件）（§5.3）。
#[tauri::command]
pub async fn delete_version(version_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    let state = Arc::clone(&state);
    // DB 删行 + 文件删除同段下沉（文件删除也是阻塞 IO）。原走 write_blocking 封装,现自建
    // spawn_blocking 块以在外层持文档一致性门 read guard(方案 B §3.1:与备份写快照窗互斥)。
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _doc_guard = state
            .document_storage_guard
            .read()
            .unwrap_or_else(|e| e.into_inner());
        // 删行先于删文件(安全序:崩溃只留可 GC 的孤儿文件,DB 永不引用缺失文件)。
        let path = {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            q::delete_version(&conn, version_id)?
        };
        if let Some(p) = path {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 两个版本引用间的行级 diff（`None` = 源文件基线）（§5.3）。
#[tauri::command]
pub async fn diff_versions(
    item_id: i64,
    a: Option<i64>,
    b: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DiffOp>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<DiffOp>> {
        let ta = read_ref_text(&state, item_id, a)?;
        let tb = read_ref_text(&state, item_id, b)?;
        let diff = TextDiff::from_lines(&ta, &tb);
        let ops = diff
            .iter_all_changes()
            .map(|c| {
                let tag = match c.tag() {
                    ChangeTag::Equal => "equal",
                    ChangeTag::Insert => "insert",
                    ChangeTag::Delete => "delete",
                };
                DiffOp {
                    tag: tag.to_string(),
                    value: c.value().trim_end_matches('\n').to_string(),
                }
            })
            .collect();
        Ok(ops)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Line-level diff between two arbitrary texts (§5.4 track-changes preview): original vs the
/// AI-corrected text, before the user accepts. Pure compute (no DB / files).
/// 两段任意文本的行级 diff（§5.4 track-changes 预览）：原文 vs AI 修订文，接受前预览。纯计算。
#[tauri::command]
pub async fn diff_texts(a: String, b: String) -> Result<Vec<DiffOp>> {
    tokio::task::spawn_blocking(move || {
        TextDiff::from_lines(&a, &b)
            .iter_all_changes()
            .map(|c| {
                let tag = match c.tag() {
                    ChangeTag::Equal => "equal",
                    ChangeTag::Insert => "insert",
                    ChangeTag::Delete => "delete",
                };
                DiffOp {
                    tag: tag.to_string(),
                    value: c.value().trim_end_matches('\n').to_string(),
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
}

/// 获取文档已保存的阅读位置（§5.1）。从未打开过则返回 `None`。
#[tauri::command]
pub async fn get_reading_progress(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    read_blocking(&state, move |c| q::get_reading_progress(c, item_id)).await
}

/// 持久化文档阅读位置（§5.1）。由查看器去抖调用。
#[tauri::command]
pub async fn set_reading_progress(
    item_id: i64,
    position: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        q::set_reading_progress(c, item_id, &position)
    })
    .await
}

// ── txt 分章/分段(阅读器 R1,§5.2/§6.3)────────────────────────────────────────

/// 生效文本引用：路径、实际解码编码与缓存指纹。源文件用纳秒 mtime+size，内部版本用版本 id。
struct EffectiveRef {
    path: String,
    src_key: String,
    encoding_override: Option<String>,
}

/// 前端可见的一章元数据(字节偏移不出 Rust)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextChapterDto {
    title: String,
    char_len: usize,
}

/// get_text_book_index 返回:编码/置信度 + 章节列表(仅标题 + 字符数)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBookIndexDto {
    /// encoding_rs 规范名(如 "GBK"/"UTF-8")。
    encoding: String,
    /// 'bom' | 'detected' | 'manual' | 'lossy'(替换率超阈,UI 据此提示「编码可能误判」)。
    confidence: String,
    chapters: Vec<TextChapterDto>,
}

/// get_text_chapter 返回:章标题 + 段落数组(已剥离作为标题的首行,避免与标题重复渲染)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextChapterContent {
    title: String,
    paragraphs: Vec<String>,
    /// 与 paragraphs 一一对应的绝对零基源行号，按原文 LF 计数。
    paragraph_source_lines: Vec<usize>,
    /// 本章首个非空源行；空章没有可定位标题。
    title_source_line: Option<usize>,
    /// 解码原文的 UTF-8 SHA-256；换行也必须相同，才能复用 LF 行号。
    source_hash: String,
}

/// 行号与全文身份必须随同一份章节索引缓存，不能把新章节和旧文本摘要混用。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextIndexSnapshot {
    chapters: Vec<crate::reader::text_index::ChapterMeta>,
    source_hash: String,
}

/// 解析 item 的「生效文本」引用(当前版本文件 或 源文件)+ 缓存指纹键。
/// 指纹随当前版本、源文件纳秒 mtime+size 或实际编码变化，令 text_book_index 缓存自动失效重建。
fn effective_text_ref(
    state: &AppState,
    item_id: i64,
    override_label: Option<&str>,
) -> Result<EffectiveRef> {
    let cur = {
        let pool = state.db_read_pool.get()?;
        q::get_current_version(&pool, item_id)?
    };
    resolve_text_ref(state, item_id, cur, override_label)
}

/// 路径、缓存指纹与解码编码必须来自同一来源，避免将源文件的 GBK 偏好套在 UTF-8 版本上。
fn resolve_text_ref(
    state: &AppState,
    item_id: i64,
    version: Option<DocumentVersion>,
    explicit_encoding: Option<&str>,
) -> Result<EffectiveRef> {
    match version {
        Some(v) => Ok(EffectiveRef {
            src_key: format!("ver:{}:UTF-8", v.id),
            path: v.abs_path,
            encoding_override: Some("UTF-8".into()),
        }),
        None => {
            let encoding_override = resolve_encoding_override(state, item_id, explicit_encoding)?;
            let (root, rel, name) = {
                let pool = state.db_read_pool.get()?;
                q::get_item_path_info(&pool, item_id)?
            };
            let path = resolve_media_path(&root, &rel, &name);
            let meta = std::fs::metadata(&path).map_err(AppError::from)?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                // 保留文件系统提供的亚秒精度，避免同一秒内等长编辑复用旧标题与字节边界。
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Ok(EffectiveRef {
                src_key: format!(
                    "src:{}:{}:{}",
                    mtime,
                    meta.len(),
                    encoding_override.as_deref().unwrap_or("")
                ),
                path,
                encoding_override,
            })
        }
    }
}

/// 从每书 prefs JSON 提取手动编码覆盖(`prefs.encoding` 字段)。缺失/非法 JSON → None。
fn stored_encoding_override(prefs_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(prefs_json)
        .ok()
        .and_then(|v| {
            v.get("encoding")
                .and_then(|e| e.as_str())
                .map(str::to_string)
        })
}

/// 解析生效的编码覆盖(§5.1「手动切换编码」):**显式参数(一次性预览)优先 → 每书持久化
/// prefs.encoding → None(自动检测)**。在 blocking 上下文调用(读 reader_book_prefs)。
fn resolve_encoding_override(
    state: &AppState,
    item_id: i64,
    explicit: Option<&str>,
) -> Result<Option<String>> {
    if let Some(e) = explicit {
        return Ok(Some(e.to_string()));
    }
    let prefs = {
        let pool = state.db_read_pool.get()?;
        q::get_reader_book_prefs(&pool, item_id)?
    };
    Ok(prefs.as_deref().and_then(stored_encoding_override))
}

/// 确保章节索引就绪:缓存命中(src_key 一致 + JSON 可解析)直接返回;否则读全文重建并落缓存。
/// 返回 (encoding, confidence, chapters)。**须在 blocking 上下文调用**(内部读文件 + DB 读写)。
fn ensure_text_index(
    state: &AppState,
    item_id: i64,
    eref: &EffectiveRef,
) -> Result<(String, String, TextIndexSnapshot)> {
    // v3 给既有章节边界补原文行号和摘要；旧数组不能提供可靠编辑定位，必须重建。
    let cache_key = format!("index-v3:{}", eref.src_key);
    // 查缓存
    let cached = {
        let pool = state.db_read_pool.get()?;
        q::get_text_book_index(&pool, item_id)?
    };
    if let Some((src_key, encoding, confidence, chapters_json)) = cached {
        if src_key == cache_key {
            if let Ok(snapshot) = serde_json::from_str::<TextIndexSnapshot>(&chapters_json) {
                return Ok((encoding, confidence, snapshot));
            }
            // JSON 解析失败(格式漂移)→ 落到重建。
        }
    }
    // 重建:读全文 → 检测编码 + 分章。100MB 级一次顺序读 + 正则,在 blocking 线程可接受;结果入缓存。
    let bytes = std::fs::read(&eref.path).map_err(AppError::from)?;
    let index = crate::reader::text_index::build_index(&bytes, eref.encoding_override.as_deref());
    let snapshot = TextIndexSnapshot {
        chapters: index.chapters,
        source_hash: index.source_hash,
    };
    let chapters_json = serde_json::to_string(&snapshot)
        .map_err(|e| AppError::Internal(format!("chapters serialize | 章节序列化失败: {e}")))?;
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        q::upsert_text_book_index(
            &conn,
            item_id,
            &cache_key,
            &index.encoding,
            &index.confidence,
            &chapters_json,
        )?;
    }
    Ok((index.encoding, index.confidence, snapshot))
}

/// 读取文件的字节区间 [start, end)(章节按需取,不整读大文件)。宽容短读(文件被截断不 panic)。
fn read_byte_range(path: &str, start: usize, end: usize) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(AppError::from)?;
    f.seek(SeekFrom::Start(start as u64))
        .map_err(AppError::from)?;
    let len = end.saturating_sub(start);
    let mut buf = Vec::with_capacity(len);
    f.take(len as u64)
        .read_to_end(&mut buf)
        .map_err(AppError::from)?;
    Ok(buf)
}

/// 取 txt 文档的章节索引(阅读器 R1,§6.3):首开检测编码 + 分章并落缓存;命中缓存直接返回。
/// 字节偏移不出 Rust；前端只见编码、置信度、章标题与字符数。内部版本固定按 UTF-8 读取。
/// 源文件的 `encoding_override` 用于一次性预览；缺省时使用每书 prefs.encoding，再否则自动检测。
#[tauri::command]
pub async fn get_text_book_index(
    item_id: i64,
    encoding_override: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<TextBookIndexDto> {
    // span 埋点(W1,D-312 info 档:load_document 类,首开检测编码+分章,真实 IO/CPU 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:get_text_book_index");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<TextBookIndexDto> {
        let eref = effective_text_ref(&state, item_id, encoding_override.as_deref())?;
        let (encoding, confidence, snapshot) = ensure_text_index(&state, item_id, &eref)?;
        Ok(TextBookIndexDto {
            encoding,
            confidence,
            chapters: snapshot
                .chapters
                .into_iter()
                .map(|c| TextChapterDto {
                    title: c.title,
                    char_len: c.char_len,
                })
                .collect(),
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 取某章内容(阅读器 R1,§6.3):seek 章节字节区间 → 按已定编码解码 → 分段(可选二级重排)。
/// 内存/IPC 载荷恒为单章级。首行若等于章标题则剥离(标题由 UI 单独渲染,避免重复)。
#[tauri::command]
pub async fn get_text_chapter(
    item_id: i64,
    chapter_index: usize,
    reflow: bool,
    encoding_override: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<TextChapterContent> {
    // span 埋点(W1,D-312 info 档:load_document 类,seek+解码+分段,真实 IO/CPU 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:get_text_chapter");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        load_text_chapter(
            &state,
            item_id,
            chapter_index,
            reflow,
            encoding_override.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

fn load_text_chapter(
    state: &AppState,
    item_id: i64,
    chapter_index: usize,
    reflow: bool,
    encoding_override: Option<&str>,
) -> Result<TextChapterContent> {
    let eref = effective_text_ref(state, item_id, encoding_override)?;
    let (encoding, _confidence, snapshot) = ensure_text_index(state, item_id, &eref)?;
    let ch = snapshot.chapters.get(chapter_index).ok_or_else(|| {
        AppError::Internal(format!(
            "chapter index {chapter_index} out of range (total {}) | 章节序号越界",
            snapshot.chapters.len()
        ))
    })?;
    let enc = encoding_rs::Encoding::for_label(encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let slice = read_byte_range(&eref.path, ch.byte_start, ch.byte_end)?;
    let (text, _, _) = enc.decode(&slice);
    let title_source_line = text
        .split('\n')
        .position(|line| !line.trim().is_empty())
        .map(|line| ch.start_line + line);
    // 先剥标题再分段，保持既有重排内容；被剥行数仅平移行号，不从渲染文本反查。
    let body = crate::reader::paragraph::strip_title_line(&text, &ch.title);
    let removed_lines = text[..text.len() - body.len()]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let segmented = crate::reader::paragraph::segment(body, reflow);
    Ok(TextChapterContent {
        title: ch.title.clone(),
        paragraphs: segmented.paragraphs,
        paragraph_source_lines: segmented
            .source_lines
            .into_iter()
            .map(|line| ch.start_line + removed_lines + line)
            .collect(),
        title_source_line,
        source_hash: snapshot.source_hash,
    })
}

/// 简繁转换(阅读器 §5.10/§6.3):按 config 批量转换文本(config 见 zh_convert::builtin_config,
/// 如 s2t/t2s/s2tw/s2twp)。前端在**应用替换规则之后**按章调用(保证 §5.13「替换→简繁→渲染」顺序);
/// txt/md/epub 三格式共用。纯计算(无 DB/文件),但词典转换可能大批量 → spawn_blocking 不阻塞执行器。
#[tauri::command]
pub async fn convert_chinese(texts: Vec<String>, config: String) -> Result<Vec<String>> {
    tokio::task::spawn_blocking(move || crate::reader::zh_convert::convert_batch(&texts, &config))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 取某书的每书阅读偏好 JSON(阅读器 §6.1)。从未设置返回 `None`。R1 承载手动编码覆盖
/// (`prefs.encoding`),R3 起扩充竖排/主题/字号等字段。
#[tauri::command]
pub async fn get_reader_book_prefs(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    read_blocking(&state, move |c| q::get_reader_book_prefs(c, item_id)).await
}

/// 写入/更新某书的每书阅读偏好 JSON(阅读器 §6.1)。`prefs` 为版本化 JSON diff。
/// 改动 `prefs.encoding` 会令 text_book_index 的 src_key 变化 → 下次取索引自动按新编码重建。
#[tauri::command]
pub async fn set_reader_book_prefs(
    item_id: i64,
    prefs: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        q::set_reader_book_prefs(c, item_id, &prefs)
    })
    .await
}

/// 列出某书的书签(阅读器 R4,§6.2),按全书进度升序。
#[tauri::command]
pub async fn list_reader_bookmarks(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::db::models::ReaderBookmark>> {
    read_blocking(&state, move |c| q::list_reader_bookmarks(c, item_id)).await
}

/// 新增书签(阅读器 R4)。`locator`=位置串("cfi:<epubcfi>"),`label`=章名,`fraction`=全书进度 0..1。
/// 同位置(item_id+locator)幂等:重复添加即刷新标签/进度/时间。
#[tauri::command]
pub async fn add_reader_bookmark(
    item_id: i64,
    locator: String,
    label: String,
    fraction: f64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    write_blocking(&state, move |c| {
        q::add_reader_bookmark(c, item_id, &locator, &label, fraction)
    })
    .await
}

/// 删除书签(阅读器 R4),按书签 id。
#[tauri::command]
pub async fn delete_reader_bookmark(id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    write_blocking(&state, move |c| q::delete_reader_bookmark(c, id)).await
}

/// 文档前端渲染任务在 pending 查询时捕获的源快照。
///
/// `media_derivations` 没有重复保存这两个字段，因此它们必须随 pending 任务在进程内
/// 传到最终写回；最终 SQL 仍以 `media_items` 的条件更新作为唯一接受边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DocThumbSnapshot {
    source_revision: i64,
    cache_key: i64,
}

/// 前端文档渲染 DTO。保留原有字段，并把源快照显式返回给较新的渲染器；旧渲染器会
/// 忽略额外字段，后端同时用进程内 lease 兼容旧客户端的回传协议。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDocThumbDto {
    pub item_id: i64,
    pub abs_path: String,
    pub file_format: String,
    pub source_revision: i64,
    pub cache_key: i64,
}

/// 当前前端 IPC 只回传 item id/page count。这个小 lease 表把 list 时的快照绑定到随后
/// 的 store 请求；显式携带快照的新版客户端仍须与 lease 一致。这样在不扩大本轮前端改动
/// 范围的情况下，扫描在渲染期间推进 source_revision 时，旧 PNG 不会被当成新源接受。
static PENDING_DOC_THUMB_LEASES: OnceLock<Mutex<HashMap<i64, DocThumbSnapshot>>> = OnceLock::new();

fn pending_doc_thumb_leases() -> &'static Mutex<HashMap<i64, DocThumbSnapshot>> {
    PENDING_DOC_THUMB_LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_pending_doc_thumb(item_id: i64, snapshot: DocThumbSnapshot) -> bool {
    let mut leases = pending_doc_thumb_leases()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match leases.get(&item_id) {
        Some(existing) => *existing == snapshot,
        None => {
            leases.insert(item_id, snapshot);
            true
        }
    }
}

fn take_pending_doc_thumb(
    item_id: i64,
    requested: Option<DocThumbSnapshot>,
) -> Result<DocThumbSnapshot> {
    let mut leases = pending_doc_thumb_leases()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let snapshot = leases.get(&item_id).copied().ok_or_else(|| {
        AppError::Internal(
            "store_doc_thumbnail: missing pending source snapshot | 缺少待处理源快照".into(),
        )
    })?;
    if requested.is_some_and(|requested| requested != snapshot) {
        return Err(AppError::Internal(
            "store_doc_thumbnail: source snapshot does not match pending task | 源快照与待处理任务不匹配".into(),
        ));
    }
    leases.remove(&item_id);
    Ok(snapshot)
}

fn read_doc_thumb_snapshot(conn: &rusqlite::Connection, item_id: i64) -> Result<DocThumbSnapshot> {
    conn.query_row(
        "SELECT source_revision, cache_key
         FROM media_items
         WHERE id=?1 AND is_deleted=0",
        params![item_id],
        |row| {
            Ok(DocThumbSnapshot {
                source_revision: row.get(0)?,
                cache_key: row.get(1)?,
            })
        },
    )
    .optional()?
    .ok_or(AppError::MediaNotFound(item_id))
}

#[derive(Debug)]
struct DocThumbPersisted {
    /// 仅当 media_items 的条件写实际命中时才返回，用于 patch resident cache。
    thumb_result: Option<ThumbResult>,
}

/// 在一个短事务内完成「派生状态 + media_items 缩略图 + document_meta」写回。
///
/// `source_revision/cache_key` 是 pending list 时捕获的源快照；派生行允许从 `status=0`
/// 或兼容旧流程的 `status=1` 完成。任一条件不满足都返回 `None`，不触碰三个持久化落点。
/// 失败结果的 media 状态仍经既有 `update_thumb_result_if_current`，所以不会把已有成功封面降级。
#[allow(clippy::too_many_arguments)]
fn finish_doc_thumb_if_current(
    conn: &rusqlite::Connection,
    item_id: i64,
    snapshot: DocThumbSnapshot,
    derivation_status: i64,
    thumb_status: i64,
    payload_path: Option<&str>,
    error: Option<&str>,
    thumbhash: Option<&[u8]>,
    page_count: Option<i64>,
    file_format: &str,
) -> Result<Option<DocThumbPersisted>> {
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE media_derivations
         SET status=?1, payload_path=?2, error=?3, orphan_count=0,
             updated_at=strftime('%s','now')
         WHERE item_id=?4 AND kind='doc_thumb' AND status IN (0, 1)
           AND EXISTS (
               SELECT 1 FROM media_items m
               WHERE m.id=?4 AND m.is_deleted=0
                 AND m.source_revision=?5 AND m.cache_key=?6
           )",
        params![
            derivation_status,
            payload_path,
            error,
            item_id,
            snapshot.source_revision,
            snapshot.cache_key,
        ],
    )?;
    if updated != 1 {
        tx.rollback()?;
        return Ok(None);
    }

    let thumb_changed = q::update_thumb_result_if_current(
        &tx,
        item_id,
        snapshot.source_revision,
        snapshot.cache_key,
        thumb_status,
        payload_path,
        thumbhash,
    )?;

    // PDF/SVG 的 subtype 必须使用当前守卫确认的格式；不能调用后台 epub 专用的通用
    // derivation writer，否则会把前端渲染结果暂记为 epub。
    if derivation_status == 2 {
        q::upsert_document_meta(&tx, item_id, page_count, Some(doc_subtype(file_format)))?;
    }
    tx.commit()?;

    Ok(Some(DocThumbPersisted {
        thumb_result: (thumb_changed == 1).then(|| ThumbResult {
            item_id,
            thumb_status,
            thumb_path: payload_path.map(str::to_owned),
            thumbhash: thumbhash.map(<[u8]>::to_vec),
            source_revision: snapshot.source_revision,
            cache_key: snapshot.cache_key,
        }),
    }))
}

/// 在生命周期读锁内串起最终 DB 事务和 resident cache patch。锁只覆盖短 SQL 与 O(1) 的
/// 内存 patch，不覆盖 PNG 解码/编码；生命周期切换或快照失配时返回 `None`。
#[allow(clippy::too_many_arguments)]
fn persist_doc_thumb_result(
    state: &AppState,
    database_epoch: u64,
    item_id: i64,
    snapshot: DocThumbSnapshot,
    derivation_status: i64,
    thumb_status: i64,
    payload_path: Option<&str>,
    error: Option<&str>,
    thumbhash: Option<&[u8]>,
    page_count: Option<i64>,
    file_format: &str,
) -> Result<Option<DocThumbPersisted>> {
    state
        .with_database_lifecycle_read(database_epoch, || {
            let persisted = {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                finish_doc_thumb_if_current(
                    &conn,
                    item_id,
                    snapshot,
                    derivation_status,
                    thumb_status,
                    payload_path,
                    error,
                    thumbhash,
                    page_count,
                    file_format,
                )?
            };
            if let Some(ref persisted) = persisted {
                if let Some(ref thumb) = persisted.thumb_result {
                    state.apply_thumb_results(std::slice::from_ref(thumb));
                }
            }
            Ok(persisted)
        })
        .transpose()
        .map(|persisted| persisted.flatten())
}

/// 在一个短事务中同时读取 pending 路径与源快照；文件渲染发生在此函数返回之后，
/// 因而不会持有 SQLite writer mutex。前端 renderer 首次失败时不会回传请求，故这里
/// 不把 `media_derivations` 改成 `status=1`；进程内 lease 仅用于把旧客户端的 item id
/// 回传绑定到这次快照，最终写回仍由 source CAS 决定。
fn list_pending_doc_thumbs_with_snapshot(
    conn: &rusqlite::Connection,
    limit: i64,
) -> Result<Vec<PendingDocThumbDto>> {
    let tx = conn.unchecked_transaction()?;
    let rows = q::list_pending_doc_thumbs(&tx, limit)?;
    let mut candidates = Vec::with_capacity(rows.len());
    for (item_id, abs_path, file_format) in rows {
        let snapshot = read_doc_thumb_snapshot(&tx, item_id)?;
        candidates.push(PendingDocThumbDto {
            item_id,
            abs_path,
            file_format,
            source_revision: snapshot.source_revision,
            cache_key: snapshot.cache_key,
        });
    }
    tx.commit()?;

    let mut result = Vec::with_capacity(candidates.len());
    for pending in candidates {
        if remember_pending_doc_thumb(
            pending.item_id,
            DocThumbSnapshot {
                source_revision: pending.source_revision,
                cache_key: pending.cache_key,
            },
        ) {
            result.push(pending);
        }
    }
    Ok(result)
}

/// 确保所有 pdf/svg 文档都有 `doc_thumb` 行（INSERT OR IGNORE 入队）。pdf/svg 缩略图是
/// 「前端驱动」，需在用户未启动后端派生流水线时也能工作，故常驻渲染器每轮自行调用本命令播种队列。
/// 幂等且廉价（仅扫描很小的 document 子集）。epub 有意留给后端流水线（与视频封面同路径）。
#[tauri::command]
pub async fn ensure_doc_thumb_queue(state: State<'_, Arc<AppState>>) -> Result<usize> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<usize> {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        q::backfill_derivations(&conn, "doc_thumb", "document", Some(&["pdf", "svg"]))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 列出等待前端渲染缩略图的 pdf/svg 文档（§3.4）。`limit` 限制渲染器每轮领取的批量（默认 8），
/// 以保持主窗口响应。
#[tauri::command]
pub async fn list_pending_doc_thumbs(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<PendingDocThumbDto>> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        // pending 路径与快照读取必须在 lifecycle 读区内完成，避免清库切换数据库代次时
        // 把已失效的文件地址交给 renderer。
        state
            .with_scan_lifecycle_read(|| {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                list_pending_doc_thumbs_with_snapshot(&conn, limit.unwrap_or(8))
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 接收前端渲染好的文档缩略图（PNG 字节）并像封面一样持久化：解码 → 复用缩略图编码器
/// （缩放 → WebP → 按 `cache_key` 写缓存）→ 按 `source_revision + cache_key` 条件标记
/// `doc_thumb` 派生完成 → 回填 `thumb_status/path/hash` 到 `media_items` 与常驻布局缓存 →
/// 通知画廊刷新（不变量 §1.3.4）。
/// **空 body = 渲染失败** → 标记派生错误（status=3）避免无限重试，并置 `thumb_status=2`（无缩略图）。
///
/// IPC 载荷走 **raw body**(深审 defer ①):PNG 字节经 JSON 数字数组序列化时每字节膨胀成
/// ~4 字符(300KB PNG → 1-2MB JSON 串,前端 stringify + 后端 parse 双重开销),Tauri v2 的
/// raw `InvokeBody` 直传字节零膨胀。元数据(item_id / page_count)随 raw body 只能走
/// **request headers**(小写自定义头,见前端 invokeIpcRaw)。除 `x-item-id`/`x-page-count` 外，
/// 新版客户端可回传 `x-source-revision` + `x-cache-key`；后端会与 pending lease 严格比对。
#[tauri::command]
pub async fn store_doc_thumbnail(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:解码+编码+落盘,真实 CPU/IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:store_doc_thumbnail");
    // ── 解包(在 spawn_blocking 前取得 owned 数据)──────────────────────────
    let header_i64 = |name: &str| -> Option<i64> {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())
    };
    let item_id = header_i64("x-item-id").ok_or_else(|| {
        AppError::Internal("store_doc_thumbnail: missing/invalid x-item-id header".into())
    })?;
    let requested_snapshot = match (
        header_i64("x-source-revision"),
        header_i64("x-cache-key"),
    ) {
        (None, None) => None,
        (Some(source_revision), Some(cache_key)) => Some(DocThumbSnapshot {
            source_revision,
            cache_key,
        }),
        _ => {
            return Err(AppError::Internal(
                "store_doc_thumbnail: source snapshot headers must be paired | 源快照请求头必须成对提供".into(),
            ))
        }
    };
    // T10(§3.8.2):pdf 渲染时前端顺带取 pdf.js numPages 传入;svg 无页概念不带此头。
    let page_count = header_i64("x-page-count");
    let png_bytes: Vec<u8> = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b.clone(),
        tauri::ipc::InvokeBody::Json(_) => {
            return Err(AppError::Internal(
                "store_doc_thumbnail: expected raw body | 本命令只接受 raw 字节载荷".into(),
            ))
        }
    };

    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let Some(database_epoch) = state.current_database_epoch() else {
            return Ok(());
        };

        // ── 守卫(#3 加固):x-item-id 来自请求头不可信,先校验其确为前端可渲染文档(pdf/svg)
        //    再落任何状态——否则拿非 doc-thumb 项(.txt/.jpg/epub/mp4 等)的 id 调用会污染其
        //    doc_thumb 派生态与 document_meta。白名单单源见 queries::FRONTEND_DOC_THUMB_FORMATS,
        //    与 list_pending_doc_thumbs 泵过滤同集。空 body 失败分支亦置于守卫之后:误传的
        //    非文档项不应被标 doc_thumb 错误。──────────────────────────────────────
        let file_format = {
            let pool = state.db_read_pool.get()?;
            q::validate_frontend_doc_thumb_item(&pool, item_id)?
        };

        // source snapshot 必须来自 pending 领取时的 lease；在 store 时重新读取当前行会把
        // 旧 renderer 产出的 PNG 错当成新源，失去 CAS 的意义。
        let snapshot = take_pending_doc_thumb(item_id, requested_snapshot)?;

        let outcome = (|| -> Result<Option<DocThumbPersisted>> {
            // ── 失败分支：空字节 → 标错，停止重试 ───────────────────────────────
            if png_bytes.is_empty() {
                return persist_doc_thumb_result(
                    &state,
                    database_epoch,
                    item_id,
                    snapshot,
                    3,
                    2,
                    None,
                    Some("frontend render failed | 前端渲染失败"),
                    None,
                    None,
                    &file_format,
                );
            }

            // ── 成功分支：解码 PNG → 编码为缩略图（复用图像编码器/缓存键）────────────
            let (cache_dir, thumb_size, webp_quality) = {
                let cfg = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
                (cfg.cache_dir.clone(), cfg.size, cfg.webp_quality)
            };

            let dynimg = image::load_from_memory(&png_bytes).map_err(|e| {
                AppError::Internal(format!("doc thumb decode failed | 文档缩略图解码失败: {e}"))
            })?;
            let rgba = dynimg.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            let decoded = crate::engine::traits::DecodedImage {
                pixels: rgba.into_raw(),
                width: w,
                height: h,
                icc: None, // 前端栅格化的文档封面 PNG 无 ICC 来源,按 sRGB 假定
            };

            let cfg = ThumbConfig {
                cache_dir,
                size: snap_to_tier(thumb_size),
                skip_max_bytes: 0,
                strategy: String::new(),
                gpu_engine: String::new(),
                ai_hq_cache: false, // 文档缩略图（前端驱动栅格化）非 CLIP 分析对象，不产 AI 缓存
                webp_quality,
                ai_cache_short_edge: crate::thumbnail::cache::AI_CACHE_SHORT_EDGE, // 未用(ai_hq_cache=false)
            };
            let res = encode_media_step_with_snapshot(
                item_id,
                snapshot.source_revision,
                snapshot.cache_key,
                decoded,
                &cfg,
            )?;

            persist_doc_thumb_result(
                &state,
                database_epoch,
                item_id,
                snapshot,
                2,
                1,
                res.thumb_path.as_deref(),
                None,
                res.thumbhash.as_deref(),
                page_count,
                &file_format,
            )
        })();

        let persisted = outcome?;

        let Some(persisted) = persisted else {
            // 过时代次、清库或其它消费者已经接管时，旧结果是正常丢弃，不向前端暴露错误。
            // pending 行保持 status=0 时，下一轮 renderer 会自然重试；若已被其它路径接管，
            // 其状态/快照条件仍由该路径负责。
            return Ok(());
        };

        // DB CAS 拒绝的旧结果不会返回 thumb_result，因此不能污染 resident cache；接受的
        // 失败结果也会带完整快照进入同一 patch 路径，保持 DB 与内存状态一致。实际 patch
        // 已在 persist_doc_thumb_result 的生命周期读锁内完成。
        if persisted.thumb_result.is_some() {
            let _ = app.emit("db:media_enriched", MediaEnrichedPayload::refresh_signal());
        }

        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[cfg(test)]
mod tests {
    fn text_test_state(
        temp: &Path,
        source_bytes: &[u8],
        prefs: &str,
    ) -> (AppState, tracing_appender::non_blocking::WorkerGuard) {
        let db_path = temp.join("test.db");
        let writer = crate::db::create_write_connection(&db_path).unwrap();
        std::fs::write(temp.join("story.txt"), source_bytes).unwrap();
        {
            let conn = writer.lock().unwrap();
            crate::db::schema::initialize_schema(&conn).unwrap();
            conn.execute(
                "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'Root')",
                params![temp.to_string_lossy()],
            )
            .unwrap();
            conn.execute_batch(
                "INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'Root');
                 INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                     file_format, media_type, sort_datetime, cache_key)
                 VALUES (42, 10, 'story.txt', 30, 1, 'txt', 'document', 1, 42);",
            )
            .unwrap();
            q::set_reader_book_prefs(&conn, 42, prefs).unwrap();
        }
        let pool = crate::db::create_read_pool(&db_path, 1).unwrap();
        let (config, _) =
            crate::config::ConfigManager::load_or_init(temp.join("config.toml")).unwrap();
        let (log_writer, log_guard) = tracing_appender::non_blocking(std::io::sink());
        let state = AppState::new(
            writer,
            pool,
            Arc::new(config),
            temp.join("cache"),
            temp.join("logs"),
            crate::logging::LogRingBuffer::new(Arc::from("text-version-encoding-test"), 1),
            log_writer.error_counter(),
            temp.to_path_buf(),
            temp.join("exotic"),
            256,
            0,
            "balanced".into(),
            "cpu".into(),
            false,
            80,
            Arc::new(crate::exotic::catalog::CatalogStore::with_snapshot(
                crate::exotic::catalog::CatalogSnapshot::empty(),
            )),
        );
        (state, log_guard)
    }

    #[test]
    fn document_link_rejects_unsafe_unregistered_and_unavailable_targets() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (state, _guard) = text_test_state(temp.path(), b"source", "{}");
        std::fs::write(temp.path().join("unregistered.md"), b"unregistered").unwrap();
        std::fs::write(temp.path().join("manual.pdf"), b"not a text document").unwrap();
        std::fs::write(temp.path().join("deleted.md"), b"deleted").unwrap();
        std::fs::write(temp.path().join("companion.md"), b"companion").unwrap();
        std::fs::write(temp.path().join("valid.md"), b"valid").unwrap();
        std::fs::write(outside.path().join("outside.md"), b"outside").unwrap();
        {
            let conn = state.db_writer.lock().unwrap();
            conn.execute_batch("INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,file_format,media_type,sort_datetime,cache_key,is_deleted,companion_of) VALUES
                (60,10,'manual.pdf',6,1,'pdf','document',1,60,0,NULL),
                (61,10,'deleted.md',6,1,'md','document',1,61,1,NULL),
                (62,10,'companion.md',6,1,'md','document',1,62,0,42),
                (64,10,'valid.md',5,1,'md','document',1,64,0,NULL);").unwrap();
        }
        for href in [
            "../outside.md",
            "/story.txt",
            "//server/story.txt",
            "C:/story.txt",
            "file:story.txt",
            "https://example.invalid/a.md",
            "story.txt:stream",
            "story.txt?x",
            "bad%",
            "%ff.md",
            "%00.md",
            "#heading",
            "unregistered.md",
            "missing.md",
            "manual.pdf",
            "deleted.md",
            "companion.md",
        ] {
            let error = resolve_document_link_sync(&state, 42, href).unwrap_err();
            let serialized = serde_json::to_string(&error).unwrap();
            assert!(!serialized.contains(temp.path().to_str().unwrap()));
            assert!(!serialized.contains(outside.path().to_str().unwrap()));
        }
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink(
            outside.path().join("outside.md"),
            temp.path().join("escape.md"),
        );
        #[cfg(windows)]
        let link = std::os::windows::fs::symlink_file(
            outside.path().join("outside.md"),
            temp.path().join("escape.md"),
        );
        if link.is_ok() {
            state.db_writer.lock().unwrap().execute("INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,file_format,media_type,sort_datetime,cache_key) VALUES(63,10,'escape.md',7,1,'md','document',1,63)", []).unwrap();
            assert!(resolve_document_link_sync(&state, 42, "escape.md").is_err());
        } else {
            eprintln!("document link fixture: external symlink unavailable on this host");
        }
        assert_eq!(
            resolve_document_link_sync(&state, 42, "valid.md")
                .unwrap()
                .item_id,
            64
        );
        state
            .db_writer
            .lock()
            .unwrap()
            .execute("UPDATE media_items SET file_format='pdf' WHERE id=42", [])
            .unwrap();
        assert!(matches!(
            resolve_document_link_sync(&state, 42, "valid.md"),
            Err(AppError::UnsupportedFormat(_))
        ));
        state
            .db_writer
            .lock()
            .unwrap()
            .execute(
                "UPDATE media_items SET file_format='txt',is_deleted=1 WHERE id=42",
                [],
            )
            .unwrap();
        assert!(matches!(
            resolve_document_link_sync(&state, 42, "valid.md"),
            Err(AppError::MediaNotFound(42))
        ));
    }

    use super::*;

    fn seed_overwrite_document(
        temp: &std::path::Path,
    ) -> (
        rusqlite::Connection,
        std::path::PathBuf,
        std::path::PathBuf,
        i64,
    ) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'Root')",
            rusqlite::params![temp.to_string_lossy()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'Root');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, sort_datetime, cache_key)
             VALUES (42, 10, 'story.md', 15, 1, 'md', 'document', 1, 42);",
        )
        .unwrap();
        let source = temp.join("story.md");
        // 短 GBK 文本不能依赖自动检测；覆盖前备份也必须遵循用户已指定的源编码。
        let (source_bytes, _, had_errors) = encoding_rs::GBK.encode("中");
        assert!(!had_errors);
        std::fs::write(&source, &source_bytes).unwrap();
        crate::db::queries::set_reader_book_prefs(
            &conn,
            42,
            r#"{"v":1,"encoding":"GBK","reflow":true,"theme":"sepia","custom":{"retain":true}}"#,
        )
        .unwrap();
        let versions = temp.join("versions");
        std::fs::create_dir(&versions).unwrap();
        let selected = super::write_version_tx(
            &conn,
            &versions,
            42,
            None,
            Some("历史稿"),
            "user",
            "md",
            "# Selected old draft",
        )
        .unwrap();
        crate::db::queries::set_current_version(&conn, 42, Some(selected)).unwrap();
        q::upsert_text_book_index(&conn, 42, "retained-source-metadata", "GBK", "manual", "[]")
            .unwrap();
        (conn, source, versions, selected)
    }

    #[cfg(windows)]
    #[test]
    fn failed_overwrite_keeps_selected_version_source_and_prewrite_backup() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().unwrap();
        let (conn, source, versions, selected) = seed_overwrite_document(temp.path());
        let original_bytes = std::fs::read(&source).unwrap();
        let original_prefs = crate::db::queries::get_reader_book_prefs(&conn, 42).unwrap();
        let original_index = q::get_text_book_index(&conn, 42).unwrap();
        // 允许备份读取，禁止替换原文件，真实触发同卷 rename 的共享冲突。
        let _locked_source = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&source)
            .unwrap();
        let result = super::overwrite_document_source(
            &conn,
            &versions,
            42,
            &source,
            "md",
            "# Unsaved latest draft",
        );
        assert!(result.is_err());
        assert_eq!(q::get_text_book_index(&conn, 42).unwrap(), original_index);
        assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
        assert_eq!(
            crate::db::queries::get_reader_book_prefs(&conn, 42).unwrap(),
            original_prefs
        );
        assert_eq!(
            crate::db::queries::get_current_version(&conn, 42)
                .unwrap()
                .unwrap()
                .id,
            selected
        );
        let all = crate::db::queries::list_versions(&conn, 42).unwrap();
        let backup = all.iter().find(|version| version.id != selected).unwrap();
        assert_eq!(std::fs::read_to_string(&backup.abs_path).unwrap(), "中");
    }

    fn seeded_doc_thumb_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items
                 (id, directory_id, file_name, file_size, file_mtime, file_format,
                  media_type, width, height, sort_datetime, cache_key, source_revision)
             VALUES (1, 10, 'doc.pdf', 10, 100, 'pdf', 'document', 0, 0, 100, 42, 7);
             INSERT INTO media_derivations (item_id, kind, status)
             VALUES (1, 'doc_thumb', 0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn stale_doc_thumb_success_is_discarded_before_persistent_writes() {
        let conn = seeded_doc_thumb_db();
        conn.execute(
            "UPDATE media_items SET source_revision=8, cache_key=43 WHERE id=1",
            [],
        )
        .unwrap();

        let persisted = finish_doc_thumb_if_current(
            &conn,
            1,
            DocThumbSnapshot {
                source_revision: 7,
                cache_key: 42,
            },
            2,
            1,
            Some("256/old/stale.webp"),
            None,
            Some(&[9]),
            Some(99),
            "pdf",
        )
        .unwrap();
        assert!(persisted.is_none(), "过期成功结果必须被丢弃");

        assert_eq!(
            conn.query_row(
                "SELECT source_revision, cache_key, thumb_status, thumb_path
                 FROM media_items WHERE id=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .unwrap(),
            (8, 43, 0, None)
        );
        assert_eq!(
            conn.query_row(
                "SELECT status, payload_path FROM media_derivations
                 WHERE item_id=1 AND kind='doc_thumb'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (0, None)
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM document_meta WHERE item_id=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "过期成功结果不得写 document_meta"
        );
    }

    /// 方案 B §3.1 崩溃语义(核心):文件写失败 → 整事务回滚 → document_versions 零残留行。
    /// 用不存在的目录作 dir 使 write_atomic 的 std::fs::write 失败(父目录缺失即 NotFound)。
    /// 旧「插行→写文件」两步在此会残留 committed 行 + 空 abs_path + 无文件,备份据此产假完整包。
    #[test]
    fn write_version_tx_rollback_leaves_no_row_on_file_failure() {
        use rusqlite::Connection;
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();

        let missing = std::env::temp_dir()
            .join(format!("scrollery_wvt_missing_{}", std::process::id()))
            .join("does-not-exist");
        let r = super::write_version_tx(&c, &missing, 42, None, None, "user", "txt", "x");
        assert!(r.is_err(), "文件写失败应返回 Err");

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM document_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "文件写失败后不得残留任何 document_versions 行(旧两步会残留)"
        );
    }
}
