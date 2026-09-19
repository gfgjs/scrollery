//! Document IPC commands (P4, §3.4/§3.5). 文档相关 IPC 命令（P4）。
//!
//! 首批：文档缩略图的前端离屏渲染回环（§3.4 Lite 路径）。
//!  - `list_pending_doc_thumbs`：前端领取待渲染的 pdf/svg 文档。
//!  - `store_doc_thumbnail`：前端回传渲染好的 PNG 字节 → 落盘缩略图缓存 + 回填 media_items/布局缓存。
//!
//! 后续阶段（5.x）的替换/版本/校对/阅读进度命令也归入本模块。

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
/// 编码 seam(阅读器 R1,§5.1):读**字节**再经 `reader::encoding::decode_bytes` 解码为 UTF-8,
/// 取代原 `std::fs::read_to_string`(遇 GBK/Big5/Shift_JIS 等非 UTF-8 直接 InvalidData 报错)。
/// 一处修活四链路:阅读(get_document_text)、编辑、版本 diff(diff_versions/get_version_content)、
/// AI 校对(读生效文本)。R1 不接每书编码覆盖(reader_book_prefs 属 R3),故 override=None。
fn read_ref_text(state: &AppState, item_id: i64, r: Option<i64>) -> Result<String> {
    let path = match r {
        None => {
            let pool = state.db_read_pool.get()?;
            let (root, rel, name) = q::get_item_path_info(&pool, item_id)?;
            resolve_media_path(&root, &rel, &name)
        }
        Some(vid) => {
            let pool = state.db_read_pool.get()?;
            let v = q::get_version(&pool, vid)?.ok_or_else(|| {
                AppError::Internal(format!("version {vid} not found | 版本不存在"))
            })?;
            v.abs_path
        }
    };
    let bytes = std::fs::read(&path).map_err(AppError::from)?;
    Ok(crate::reader::encoding::decode_bytes(&bytes, None).text)
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

/// 单事务写一个 appdata 版本(方案 B §3.1,原为「插行→写文件→回填」两步非原子):
/// 插行(拿 id)→ 派生 `{id}.{ext}` 路径 → 写原子文件 → 回填路径 → commit。
///
/// **文件写在 commit 之前** → 已提交的行必有文件在盘;文件写失败则整事务回滚、行不落库,
/// 最多留可 GC 的孤儿 tmp(write_atomic 自身失败即清)。杜绝旧两步在①插行 commit 后、
/// ③写文件前崩溃残留的「committed 行 + 空 abs_path + 无文件」(备份据此产假完整包)。
/// 抽为纯函数以便单测崩溃语义;**须在持 `db_writer` 锁的 blocking 上下文调用**。
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
        // `write_version_tx` 纯函数以便单测崩溃语义。
        let write_version =
            |label: Option<&str>, parent: Option<i64>, src: &str, text: &str| -> Result<i64> {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                write_version_tx(&conn, &dir, item_id, parent, label, src, &ext, text)
            };

        if target == "overwrite" {
            // 编码 seam(R1):源文件可能是 GBK 等,读字节再解码(原 read_to_string 遇非 UTF-8 会吞成空备份,
            // 造成覆盖前备份丢失)。备份版本按项目版本模型统一存 UTF-8。
            // 读失败处置(2026-07-10 审查 B1):备份的意义正是覆盖前保底,保底失败仍覆盖会让用户
            // 唯一副本被无备份替换——除「源不存在」(空备份是准确语义)外,一律中止覆盖。
            let src_bytes = match std::fs::read(&src_path) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(AppError::from(e)),
            };
            let src_text = crate::reader::encoding::decode_bytes(&src_bytes, None).text;
            let backup_id = write_version(
                Some("覆盖前自动备份 | auto-backup"),
                None,
                "user",
                &src_text,
            )?;
            // 原子覆盖(2026-07-06 审查 R9):这是**用户源文件**,直写崩溃留半截比缓存更不可接受
            // (虽有先行备份可救,但不应依赖救援路径)。同卷 tmp→rename。
            crate::thumbnail::generator::write_atomic(
                std::path::Path::new(&src_path),
                content.as_bytes(),
            )
            .map_err(AppError::from)?;
            Ok(backup_id)
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

/// 生效文本引用:路径 + 源指纹(缓存键)。源文件用 mtime+size,当前版本用版本 id;附编码覆盖位。
struct EffectiveRef {
    path: String,
    src_key: String,
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
}

/// 解析 item 的「生效文本」引用(当前版本文件 或 源文件)+ 缓存指纹键。
/// 指纹随「当前版本 / 源文件 mtime+size / 编码覆盖」变化 → text_book_index 缓存自动失效重建。
fn effective_text_ref(
    state: &AppState,
    item_id: i64,
    override_label: Option<&str>,
) -> Result<EffectiveRef> {
    let cur = {
        let pool = state.db_read_pool.get()?;
        q::get_current_version(&pool, item_id)?
    };
    let ov = override_label.unwrap_or("");
    match cur {
        Some(v) => Ok(EffectiveRef {
            src_key: format!("ver:{}:{}", v.id, ov),
            path: v.abs_path,
        }),
        None => {
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
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Ok(EffectiveRef {
                src_key: format!("src:{}:{}:{}", mtime, meta.len(), ov),
                path,
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
    override_label: Option<&str>,
) -> Result<(String, String, Vec<crate::reader::text_index::ChapterMeta>)> {
    // 查缓存
    let cached = {
        let pool = state.db_read_pool.get()?;
        q::get_text_book_index(&pool, item_id)?
    };
    if let Some((src_key, encoding, confidence, chapters_json)) = cached {
        if src_key == eref.src_key {
            if let Ok(chapters) =
                serde_json::from_str::<Vec<crate::reader::text_index::ChapterMeta>>(&chapters_json)
            {
                return Ok((encoding, confidence, chapters));
            }
            // JSON 解析失败(格式漂移)→ 落到重建。
        }
    }
    // 重建:读全文 → 检测编码 + 分章。100MB 级一次顺序读 + 正则,在 blocking 线程可接受;结果入缓存。
    let bytes = std::fs::read(&eref.path).map_err(AppError::from)?;
    let index = crate::reader::text_index::build_index(&bytes, override_label);
    let chapters_json = serde_json::to_string(&index.chapters)
        .map_err(|e| AppError::Internal(format!("chapters serialize | 章节序列化失败: {e}")))?;
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        q::upsert_text_book_index(
            &conn,
            item_id,
            &eref.src_key,
            &index.encoding,
            &index.confidence,
            &chapters_json,
        )?;
    }
    Ok((index.encoding, index.confidence, index.chapters))
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
/// 字节偏移不出 Rust;前端只见「编码 / 置信度 / 章标题 + 字符数」。`encoding_override`:显式一次性
/// 覆盖(预览用);为 None 时回落每书持久化 prefs.encoding(§5.1 手动切换编码),再否则自动检测。
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
        let ov = resolve_encoding_override(&state, item_id, encoding_override.as_deref())?;
        let eref = effective_text_ref(&state, item_id, ov.as_deref())?;
        let (encoding, confidence, chapters) =
            ensure_text_index(&state, item_id, &eref, ov.as_deref())?;
        Ok(TextBookIndexDto {
            encoding,
            confidence,
            chapters: chapters
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
    tokio::task::spawn_blocking(move || -> Result<TextChapterContent> {
        let ov = resolve_encoding_override(&state, item_id, encoding_override.as_deref())?;
        let eref = effective_text_ref(&state, item_id, ov.as_deref())?;
        let (encoding, _confidence, chapters) =
            ensure_text_index(&state, item_id, &eref, ov.as_deref())?;
        let ch = chapters.get(chapter_index).ok_or_else(|| {
            AppError::Internal(format!(
                "chapter index {chapter_index} out of range (total {}) | 章节序号越界",
                chapters.len()
            ))
        })?;
        let enc =
            encoding_rs::Encoding::for_label(encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8);
        let slice = read_byte_range(&eref.path, ch.byte_start, ch.byte_end)?;
        let (text, _, _) = enc.decode(&slice);
        // 标题剥离前移到 segment 之前按行进行(2026-07-10 审查 B2):原「分段后比对首段」在
        // reflow 下必然失配——标题行无句末标点,重排会把它与下一行黏成一段,标题既重复渲染
        // 又污染正文首段。精确匹配语义与旧实现一致(截断标题不剥,见 strip_title_line 文档)。
        let body = crate::reader::paragraph::strip_title_line(&text, &ch.title);
        let paragraphs = crate::reader::paragraph::segment(body, reflow);
        Ok(TextChapterContent {
            title: ch.title.clone(),
            paragraphs,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
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
    use super::*;

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
    fn pending_doc_thumb_dto_carries_source_snapshot() {
        let dto = PendingDocThumbDto {
            item_id: 1,
            abs_path: "/r/doc.pdf".into(),
            file_format: "pdf".into(),
            source_revision: 7,
            cache_key: 42,
        };
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["sourceRevision"], 7);
        assert_eq!(value["cacheKey"], 42);
    }

    #[test]
    fn pending_doc_thumb_snapshot_does_not_consume_retryable_pending_state() {
        let conn = seeded_doc_thumb_db();
        let pending = list_pending_doc_thumbs_with_snapshot(&conn, 8).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!((pending[0].source_revision, pending[0].cache_key), (7, 42));
        assert_eq!(
            conn.query_row(
                "SELECT status FROM media_derivations
                 WHERE item_id=1 AND kind='doc_thumb'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "前端首败不回传时，pending 行必须仍可在下一轮领取"
        );
        assert_eq!(
            take_pending_doc_thumb(1, None).unwrap(),
            DocThumbSnapshot {
                source_revision: 7,
                cache_key: 42,
            }
        );
    }

    #[test]
    fn current_doc_thumb_success_writes_snapshot_guarded_state() {
        let conn = seeded_doc_thumb_db();
        let persisted = finish_doc_thumb_if_current(
            &conn,
            1,
            DocThumbSnapshot {
                source_revision: 7,
                cache_key: 42,
            },
            2,
            1,
            Some("256/aa/doc.webp"),
            None,
            Some(&[1, 2, 3]),
            Some(12),
            "pdf",
        )
        .unwrap()
        .expect("匹配当前快照的成功结果应被接受");

        let result = persisted.thumb_result.expect("成功写应返回 resident patch");
        assert_eq!(
            (result.source_revision, result.cache_key),
            (7, 42),
            "编码结果必须保留生产时源快照"
        );
        assert_eq!(result.thumb_path.as_deref(), Some("256/aa/doc.webp"));
        assert_eq!(result.thumbhash, Some(vec![1, 2, 3]));
        assert_eq!(
            conn.query_row(
                "SELECT status, payload_path FROM media_derivations
                 WHERE item_id=1 AND kind='doc_thumb'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (2, Some("256/aa/doc.webp".into()))
        );
        assert_eq!(
            conn.query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (1, Some("256/aa/doc.webp".into()))
        );
        assert_eq!(
            conn.query_row(
                "SELECT page_count, doc_subtype FROM document_meta WHERE item_id=1",
                [],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
            (Some(12), "pdf".to_string())
        );
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

    #[test]
    fn stale_doc_thumb_cache_key_mismatch_is_discarded() {
        let conn = seeded_doc_thumb_db();
        conn.execute("UPDATE media_items SET cache_key=43 WHERE id=1", [])
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
            Some("256/old/cache-key.webp"),
            None,
            Some(&[8]),
            Some(99),
            "pdf",
        )
        .unwrap();
        assert!(persisted.is_none(), "cache_key 失配的成功结果必须被丢弃");
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
            (7, 43, 0, None)
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
    }

    #[test]
    fn stale_doc_thumb_failure_cannot_downgrade_new_source() {
        let conn = seeded_doc_thumb_db();
        conn.execute(
            "UPDATE media_items
             SET source_revision=8, cache_key=43, thumb_status=1, thumb_path='256/new/current.webp'
             WHERE id=1",
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
            3,
            2,
            None,
            Some("frontend render failed | 前端渲染失败"),
            None,
            None,
            "pdf",
        )
        .unwrap();
        assert!(persisted.is_none(), "过期失败结果必须被丢弃");
        assert_eq!(
            conn.query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (1, Some("256/new/current.webp".into()))
        );
        assert_eq!(
            conn.query_row(
                "SELECT status FROM media_derivations
                 WHERE item_id=1 AND kind='doc_thumb'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "过期失败结果不得将新源派生标为 error"
        );
    }

    /// 方案 B §3.1 正常路径:单事务写版本后,行落库(storage=appdata / abs_path 非空指向存在文件 /
    /// content_hash 已填),文件内容正确。锁住「已提交行必有文件」不变量的正向面。
    #[test]
    fn write_version_tx_commits_row_and_file() {
        use rusqlite::Connection;
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免构造 media_items(仅测版本写事务)

        let dir = std::env::temp_dir().join(format!("scrollery_wvt_ok_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let id = super::write_version_tx(&c, &dir, 42, None, Some("稿"), "user", "txt", "正文内容")
            .expect("write_version_tx 应成功");

        let (storage, abs_path, hash): (String, String, Option<String>) = c
            .query_row(
                "SELECT storage, abs_path, content_hash FROM document_versions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(storage, "appdata");
        assert!(
            !abs_path.is_empty(),
            "abs_path 不得为空(旧两步 bug 正是此处残空)"
        );
        assert_eq!(std::fs::read_to_string(&abs_path).unwrap(), "正文内容");
        assert!(hash.is_some(), "content_hash 应已填");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn extracts_encoding_from_prefs() {
        assert_eq!(
            stored_encoding_override(r#"{"v":1,"encoding":"gb18030","vertical":true}"#),
            Some("gb18030".to_string())
        );
    }

    #[test]
    fn missing_encoding_is_none() {
        assert_eq!(stored_encoding_override(r#"{"v":1,"vertical":true}"#), None);
    }

    #[test]
    fn invalid_or_empty_json_is_none() {
        assert_eq!(stored_encoding_override("not json"), None);
        assert_eq!(stored_encoding_override(""), None);
    }

    #[test]
    fn non_string_encoding_is_none() {
        // encoding 字段类型错误(数字)→ 不误取,返回 None。
        assert_eq!(stored_encoding_override(r#"{"encoding":123}"#), None);
    }
}
