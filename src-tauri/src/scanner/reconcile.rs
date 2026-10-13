//! 外部变化与手动扫描共享路径对账；文件 IO 在 SQLite 写锁之外。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::queries as q;
use crate::db::queries::{FastScanItem, UpsertOutcome};
use crate::error::Result;
use crate::exotic::catalog::CatalogSnapshot;
use crate::utils::file_identity::read_physical_key;
use crate::utils::path::{encode_tree_sort_key, normalize_db_path, path_depth};

/// 读锁中取得的候选；旧路径确实消失且数据库位置未变时才允许迁移。
#[derive(Clone)]
pub(crate) struct MoveCandidate {
    pub id: i64,
    pub old_rel: String,
    pub old_name: Option<String>,
}

pub(crate) fn candidate(
    conn: &Connection,
    root_id: i64,
    key: Option<&[u8]>,
    directory: bool,
) -> Result<Option<MoveCandidate>> {
    let Some(key) = key else { return Ok(None) };
    let sql = if directory {
        "SELECT id,rel_path,NULL FROM directories WHERE root_id=?1 AND physical_key=?2"
    } else {
        "SELECT m.id,d.rel_path,m.file_name FROM media_items m JOIN directories d ON d.id=m.directory_id
         WHERE d.root_id=?1 AND m.physical_key=?2 AND m.is_deleted=0"
    };
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query(params![root_id, key])?;
    let first = rows
        .next()?
        .map(|row| {
            Ok::<_, rusqlite::Error>(MoveCandidate {
                id: row.get(0)?,
                old_rel: row.get(1)?,
                old_name: row.get(2)?,
            })
        })
        .transpose()?;
    // 硬链接的多位置不能凭物理键猜测是哪一个被移动。
    Ok(if rows.next()?.is_some() { None } else { first })
}

pub(crate) fn absent_candidate(
    root: &Path,
    candidate: Option<MoveCandidate>,
) -> Option<MoveCandidate> {
    candidate.filter(|candidate| {
        let mut old = root.join(&candidate.old_rel);
        if let Some(name) = &candidate.old_name { old.push(name); }
        matches!(std::fs::symlink_metadata(old), Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    })
}

pub(crate) fn ensure_directory(conn: &Connection, root_id: i64, rel: &str) -> Result<i64> {
    let parent = if rel.is_empty() {
        None
    } else {
        let parent = Path::new(rel)
            .parent()
            .map(|p| normalize_db_path(&p.to_string_lossy()))
            .unwrap_or_default();
        Some(ensure_directory(conn, root_id, &parent)?)
    };
    let name = Path::new(rel)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("");
    // 不覆写手动扫描的目录 mtime 基线，局部存在性不等于完整扫描基线。
    conn.execute(
        "INSERT INTO directories(root_id,parent_id,rel_path,name,depth,tree_sort_key)
         VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(root_id,rel_path) DO UPDATE SET is_missing=0",
        params![
            root_id,
            parent,
            rel,
            name,
            path_depth(rel),
            encode_tree_sort_key(rel)
        ],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM directories WHERE root_id=?1 AND rel_path=?2",
        params![root_id, rel],
        |r| r.get(0),
    )?)
}

fn invalidate_location(
    conn: &Connection,
    id: i64,
    directory_id: i64,
    name: &str,
    key: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET directory_id=?2,file_name=?3,cache_key=?4,
         source_revision=source_revision+1,thumb_status=0,thumb_path=NULL,thumbhash=NULL,
         volume_relative_path=(SELECT CASE WHEN r.volume_subpath IS NULL OR r.volume_subpath='' THEN '' ELSE r.volume_subpath||'/' END || CASE WHEN d.rel_path='' THEN '' ELSE d.rel_path||'/' END || ?3 FROM directories d JOIN scan_roots r ON r.id=d.root_id WHERE d.id=?2),
         ai_status=CASE WHEN ai_status=1 THEN 0 ELSE ai_status END,
         face_status=CASE WHEN face_status=1 THEN 0 ELSE face_status END,
         updated_at=strftime('%s','now') WHERE id=?1",
        params![id, directory_id, name, key],
    )?;
    conn.execute("DELETE FROM image_artifacts WHERE item_id=?1", [id])?;
    // 音频的歌词可能保存绝对旁车路径；移动后重新发现旁车，内容元数据由补全恢复。
    conn.execute("DELETE FROM audio_meta WHERE item_id=?1", [id])?;
    conn.execute("DELETE FROM dedup_index WHERE item_id=?1", [id])?;
    conn.execute("DELETE FROM dedup_index_working WHERE item_id=?1", [id])?;
    conn.execute(
        "UPDATE media_derivations SET status=0,payload_path=NULL,error=NULL WHERE item_id=?1",
        [id],
    )?;
    q::invalidate_exotic_tasks_for_item(conn, id)?;
    Ok(())
}

/// 新目录在同一事务内复用已确认移动的目录子树；目标冲突时保留两侧数据。
pub(crate) fn upsert_observed_directory(
    conn: &Connection,
    root_id: i64,
    rel: &str,
    key: Option<&[u8]>,
    moved: Option<&MoveCandidate>,
) -> Result<i64> {
    if let Some(moved) = moved.filter(|m| m.old_rel != rel && !m.old_rel.is_empty()) {
        let source_current: Option<String> = conn
            .query_row(
                "SELECT rel_path FROM directories WHERE id=?1 AND root_id=?2",
                params![moved.id, root_id],
                |r| r.get(0),
            )
            .optional()?;
        let target: Option<i64> = conn
            .query_row(
                "SELECT id FROM directories WHERE root_id=?1 AND rel_path=?2",
                params![root_id, rel],
                |r| r.get(0),
            )
            .optional()?;
        if source_current.as_deref() == Some(&moved.old_rel) && target.is_none() {
            let parent_rel = Path::new(rel)
                .parent()
                .map(|p| normalize_db_path(&p.to_string_lossy()))
                .unwrap_or_default();
            let parent = ensure_directory(conn, root_id, &parent_rel)?;
            let mut stmt = conn.prepare("SELECT id,rel_path FROM directories WHERE root_id=?1 AND (id=?2 OR substr(rel_path,1,length(?3)+1)=?3||'/') ORDER BY length(rel_path)")?;
            let dirs = stmt
                .query_map(params![root_id, moved.id, moved.old_rel], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (id, old) in dirs {
                let new = format!("{}{}", rel, &old[moved.old_rel.len()..]);
                let name = Path::new(&new)
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("");
                conn.execute("UPDATE directories SET rel_path=?2,name=?3,depth=?4,tree_sort_key=?5,is_missing=0,mtime=NULL,parent_id=CASE WHEN id=?6 THEN ?7 ELSE parent_id END WHERE id=?1",params![id,new,name,path_depth(&new),encode_tree_sort_key(&new),moved.id,parent])?;
                let mut media_stmt=conn.prepare("SELECT id,file_name,file_mtime,COALESCE(file_mtime_ns,0) FROM media_items WHERE directory_id=?1")?;
                let media = media_stmt
                    .query_map([id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                for (item, name, mtime, ns) in media {
                    let cache = crate::utils::hash::compute_cache_key_with_mtime_ns(
                        id, &new, &name, mtime, ns,
                    );
                    invalidate_location(conn, item, id, &name, cache)?;
                }
            }
        }
    }
    let id = ensure_directory(conn, root_id, rel)?;
    conn.execute(
        "UPDATE directories SET physical_key=COALESCE(?2,physical_key),is_missing=0 WHERE id=?1",
        params![id, key],
    )?;
    Ok(id)
}

/// 同路径更新和已确认移动共用原有内容变更判定；人工关联始终挂在原媒体 ID。
pub(crate) fn upsert_observed_item(
    conn: &Connection,
    item: &FastScanItem,
    volume: Option<i64>,
    key: Option<&[u8]>,
    moved: Option<&MoveCandidate>,
) -> Result<UpsertOutcome> {
    let target: Option<(i64, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT id,physical_key FROM media_items WHERE directory_id=?1 AND file_name=?2",
            params![item.directory_id, item.file_name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if target.is_none() {
        if let Some(m) = moved {
            let current:Option<(String,String)>=conn.query_row("SELECT d.rel_path,m.file_name FROM media_items m JOIN directories d ON d.id=m.directory_id WHERE m.id=?1 AND m.is_deleted=0",[m.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if current
                .as_ref()
                .is_some_and(|(rel, name)| rel == &m.old_rel && Some(name) == m.old_name.as_ref())
            {
                invalidate_location(
                    conn,
                    m.id,
                    item.directory_id,
                    &item.file_name,
                    item.cache_key,
                )?;
            }
        }
    }
    if let Some((id, Some(old_key))) = &target {
        if key.is_some_and(|key| key != old_key) {
            q::invalidate_derived_for_item(conn, *id)?;
            invalidate_location(
                conn,
                *id,
                item.directory_id,
                &item.file_name,
                item.cache_key,
            )?;
            conn.execute(
                "UPDATE media_items SET file_mtime=0,file_mtime_ns=NULL WHERE id=?1",
                [id],
            )?;
        }
    }
    let outcome = q::upsert_fast_scan_item(conn, item, volume)?;
    conn.execute("UPDATE media_items SET physical_key=COALESCE(?2,physical_key),volume_id=COALESCE(?3,volume_id),
        volume_relative_path=(SELECT CASE WHEN r.volume_subpath IS NULL OR r.volume_subpath='' THEN '' ELSE r.volume_subpath||'/' END || CASE WHEN d.rel_path='' THEN '' ELSE d.rel_path||'/' END || media_items.file_name FROM directories d JOIN scan_roots r ON r.id=d.root_id WHERE d.id=media_items.directory_id)
        WHERE id=?1 AND ((?2 IS NOT NULL AND physical_key IS NOT ?2) OR volume_relative_path IS NULL OR (?3 IS NOT NULL AND volume_id IS NOT ?3))",params![outcome.id(),key,volume])?;
    Ok(outcome)
}

pub(crate) fn mark_missing_ids(conn: &Connection, ids: &[i64]) -> Result<()> {
    let missing: HashSet<i64> = ids.iter().copied().collect();
    let mut parents = HashSet::new();
    for id in ids {
        let parent: Option<i64> = conn.query_row(
            "SELECT companion_of FROM media_items WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        if let Some(parent) = parent.filter(|p| !missing.contains(p)) {
            parents.insert(parent);
        }
        conn.execute("UPDATE media_items SET availability='missing',source_revision=source_revision+1,
            thumb_status=CASE WHEN media_type='image' THEN 0 ELSE thumb_status END,
            thumb_path=CASE WHEN media_type='image' THEN NULL ELSE thumb_path END,
            thumbhash=CASE WHEN media_type='image' THEN NULL ELSE thumbhash END WHERE id=?1 AND availability!='missing' AND is_deleted=0",[id])?;
        conn.execute("DELETE FROM dedup_index WHERE item_id=?1", [id])?;
    }
    for id in parents {
        conn.execute("UPDATE media_items SET source_revision=source_revision+1,thumb_status=CASE WHEN media_type='image' THEN 0 ELSE thumb_status END,thumb_path=CASE WHEN media_type='image' THEN NULL ELSE thumb_path END,thumbhash=CASE WHEN media_type='image' THEN NULL ELSE thumbhash END WHERE id=?1",[id])?;
        conn.execute("DELETE FROM dedup_index WHERE item_id=?1", [id])?;
    }
    Ok(())
}

/// 一个完整读到的父目录快照；读取失败不产生可用于缺失差集的快照。
pub(crate) struct DirectorySnapshot {
    pub rel: String,
    pub key: Option<Vec<u8>>,
    pub mtime_ns: u128,
    pub dirs: Vec<(String, Option<Vec<u8>>)>,
    pub files: Vec<(PathBuf, FastScanItem, Option<Vec<u8>>)>,
}

fn directory_version(path: &Path) -> std::io::Result<u128> {
    Ok(std::fs::metadata(path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos())
}

/// 提交前复查目录枚举期间是否再次增删改名；失败时保留脏范围重读。
pub(crate) fn snapshot_is_current(root: &Path, snapshot: &DirectorySnapshot) -> bool {
    let path = root.join(&snapshot.rel);
    directory_version(&path).ok() == Some(snapshot.mtime_ns)
        && read_physical_key(root, &path).ok() == Some(snapshot.key.clone())
}

pub(crate) fn read_directory(
    root: &Path,
    rel: &str,
    catalog: &CatalogSnapshot,
) -> Result<DirectorySnapshot> {
    let path = root.join(rel);
    let mtime_ns = directory_version(&path)?;
    let key = read_physical_key(root, &path)?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let physical = read_physical_key(root, &path)?;
            dirs.push((
                if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                },
                physical,
            ));
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_lowercase();
        let Some(kind) = super::walker::classify_scanned_file(&ext, catalog) else {
            continue;
        };
        let physical = read_physical_key(root, &path)?;
        let modified = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let (width, height) =
            super::fast_scan::cheap_phase2_dimensions(kind, &ext).unwrap_or((0, 0));
        files.push((
            path,
            FastScanItem {
                directory_id: 0,
                file_name: name,
                file_size: metadata.len() as i64,
                file_mtime: modified.as_secs() as i64,
                file_mtime_ns: i64::try_from(modified.as_nanos()).unwrap_or(0),
                file_format: ext,
                media_type: kind.as_str().to_string(),
                width,
                height,
                sort_datetime: modified.as_secs() as i64,
                cache_key: 0,
            },
            physical,
        ));
    }
    if directory_version(&path)? != mtime_ns || read_physical_key(root, &path)? != key {
        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into());
    }
    Ok(DirectorySnapshot {
        rel: rel.to_string(),
        key,
        mtime_ns,
        dirs,
        files,
    })
}
