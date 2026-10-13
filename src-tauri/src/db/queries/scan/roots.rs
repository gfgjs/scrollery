//! 扫描根(scan_roots)域:CRUD + 后端绑定 + 隐藏根 + 卷绑定 + relink 改路径 + 抽样。

use rusqlite::{params, Connection, Row};

use crate::db::models::{NewVolume, ScanRoot};
use crate::error::{AppError, Result};

fn map_scan_root(row: &Row<'_>) -> rusqlite::Result<ScanRoot> {
    Ok(ScanRoot {
        id: row.get(0)?,
        path: row.get(1)?,
        alias: row.get(2)?,
        scan_status: row.get(3)?,
        scan_progress: row.get(4)?,
        total_files: row.get(5)?,
        last_scan_at: row.get(6)?,
        is_active: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        is_hidden: row.get::<_, i64>(10)? != 0,
    })
}

// ── 扫描根目录 ───────────────────────────────────────────────────────────────

pub fn insert_scan_root(conn: &Connection, path: &str, alias: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO scan_roots (path, alias) VALUES (?1, ?2)",
        params![path, alias],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 待迁移子根及其相对新父根的路径；调用方须先 canonicalize 并验证真实包含关系。
pub struct ScanRootMergeSource {
    /// 预检时确认的现存子根 ID。
    pub root_id: i64,
    /// canonicalize 后的子根相对父根路径，使用正斜杠。
    pub relative_path: String,
}

fn merge_conflict() -> AppError {
    AppError::Scan {
        code: "scan_merge_conflict",
        message: "根目录重叠或目标已存在，未合并 | Conflicting scan roots; nothing merged".into(),
    }
}

/// 在同一事务中添加父根并迁移子根的目录归属，保留媒体 ID、用户状态和缓存键。
///
/// 调用方须持有扫描根重组闸门，保证输入根集合与文件系统预检期间不发生根管理操作。
/// 本函数不访问文件或取消扫描；成功返回后，调用方才可撤销旧根的扫描令牌。
pub fn merge_scan_root_children(
    conn: &mut Connection,
    parent_path: &str,
    alias: Option<&str>,
    volume: &NewVolume,
    sources: &[ScanRootMergeSource],
) -> Result<ScanRoot> {
    let tx = conn.transaction()?;
    let existing = list_scan_roots(&tx)?;
    if existing.iter().any(|root| root.path == parent_path) {
        return Err(merge_conflict());
    }
    let mut hidden = None;
    let mut prefixes = Vec::with_capacity(sources.len());
    for source in sources {
        let root = get_scan_root(&tx, source.root_id)?;
        // 跨卷移动的遗留日志靠旧根 ID 继续恢复；只要仍有日志，就不能删掉其恢复依据。
        let has_pending_move: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM directory_move_journal WHERE source_root_id=?1 OR target_root_id=?1)",
            [source.root_id], |row| row.get(0),
        )?;
        if has_pending_move {
            return Err(AppError::Scan {
                code: "scan_merge_pending_move",
                message: "文件夹仍有待恢复的移动操作，未合并 | Pending directory move must be recovered before merging".into(),
            });
        }
        if hidden.is_some_and(|previous| previous != root.is_hidden) {
            return Err(AppError::Scan {
                code: "scan_merge_visibility_conflict",
                message: "显示与隐藏的文件夹不能合并，原库未改动 | Visible and hidden roots cannot be merged".into(),
            });
        }
        hidden = Some(root.is_hidden);
        // 嵌套子根可能索引了同一实体；不能靠删除冲突行融合，否则会丢用户关联。
        let prefix = if cfg!(windows) {
            source.relative_path.to_lowercase()
        } else {
            source.relative_path.clone()
        };
        if prefix.is_empty()
            || prefixes.iter().any(|other: &String| {
                prefix == *other
                    || prefix.starts_with(&format!("{other}/"))
                    || other.starts_with(&format!("{prefix}/"))
            })
        {
            return Err(merge_conflict());
        }
        prefixes.push(prefix);
    }

    let parent_id = insert_scan_root(&tx, parent_path, alias)?;
    let volume_id = super::super::upsert_volume(&tx, volume)?;
    set_scan_root_volume(&tx, parent_id, Some(volume_id))?;
    set_scan_root_hidden(&tx, parent_id, hidden.unwrap_or(false))?;
    let parent_name = alias.unwrap_or_else(|| {
        std::path::Path::new(parent_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
    });
    let top_id = super::upsert_directory(&tx, parent_id, None, "", parent_name, 0, None)?;
    for source in sources {
        let directories = {
            let mut statement = tx.prepare(
                "SELECT id, rel_path FROM directories WHERE root_id=?1 ORDER BY length(rel_path), id",
            )?;
            let rows = statement
                .query_map([source.root_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        if !directories.iter().any(|(_, path)| path.is_empty()) {
            return Err(merge_conflict());
        }
        let parts = source.relative_path.split('/').collect::<Vec<_>>();
        let mut ancestor_id = top_id;
        let mut ancestor_path = String::new();
        for part in &parts[..parts.len() - 1] {
            if !ancestor_path.is_empty() {
                ancestor_path.push('/');
            }
            ancestor_path.push_str(part);
            ancestor_id = super::upsert_directory(
                &tx,
                parent_id,
                Some(ancestor_id),
                &ancestor_path,
                part,
                crate::utils::path::path_depth(&ancestor_path),
                None,
            )?;
        }
        for (id, old_path) in directories {
            let path = if old_path.is_empty() {
                source.relative_path.clone()
            } else {
                format!("{}/{}", source.relative_path, old_path)
            };
            let depth = crate::utils::path::path_depth(&path);
            let tree_key = crate::utils::path::encode_tree_sort_key(&path);
            if old_path.is_empty() {
                tx.execute(
                    "UPDATE directories SET root_id=?1,rel_path=?2,depth=?3,tree_sort_key=?4,parent_id=?5,name=?6 WHERE id=?7",
                    params![parent_id, path, depth, tree_key, ancestor_id, parts[parts.len() - 1], id],
                )?;
            } else {
                tx.execute(
                    "UPDATE directories SET root_id=?1,rel_path=?2,depth=?3,tree_sort_key=?4 WHERE id=?5",
                    params![parent_id, path, depth, tree_key, id],
                )?;
            }
        }
        // 所有目录已改归新根，删除空壳不会触发任何目录或媒体的级联删除。
        delete_scan_root(&tx, source.root_id)?;
    }
    let total: i64 = tx.query_row(
        "SELECT count(*) FROM media_items m JOIN directories d ON d.id=m.directory_id WHERE d.root_id=?1",
        [parent_id], |row| row.get(0),
    )?;
    update_scan_root_status(&tx, parent_id, "idle", 0, total)?;
    let root = get_scan_root(&tx, parent_id)?;
    tx.commit()?;
    Ok(root)
}

/// 设置扫描根的显隐（V21，设置页库级排除）。`hidden=true` 时该根媒体从画廊/时间轴/搜索/统计/
/// 树/全选全部排除。仅改本行标志——排除靠查询侧条件子查询（`hidden_root_ids` + `push_root_exclusion`），
/// 不反规范化到 media_items（避免 move/copy/rescan 多点同步）。调用方须随后 `bump_data_version` 使
/// S1 取数缓存失效（见 `ipc::scan_commands::set_scan_root_hidden`）。
pub fn set_scan_root_hidden(conn: &Connection, root_id: i64, hidden: bool) -> Result<()> {
    conn.execute(
        "UPDATE scan_roots SET is_hidden = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
        params![root_id, hidden as i64],
    )?;
    Ok(())
}

/// 当前被隐藏的扫描根 id 集（V21）。库内容列举查询据此拼排除谓词；空集（常态）→ 谓词整条省略，
/// 画廊 canonical 查询逐字节不变、免 JOIN 红线不触碰。目录/根量级 ~10^3，一次小查询。
pub fn hidden_root_ids(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM scan_roots WHERE is_hidden = 1")?;
    let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

// 「隐藏根排除」静态谓词。缩略图/AI/人脸三条**全表状态列排空**
// 流水线据此跳过被隐藏根下的媒体——用户隐藏一个根即不再为其花缩略图/CLIP/人脸算力。
//
// 与画廊侧(`push_root_exclusion`,layout.rs)走绑参不同,这里用**全静态不相关子查询**:
// 流水线枚举无 canonical `+` 抑制红线、无 param-index 不变量,静态形零 Rust 管线且天然正确——
// 无隐藏根时内层 `SELECT ... WHERE is_hidden=1` 空集 → `NOT IN (空)` 全通过(含 NULL 也通过,
// 但 media_items.directory_id 为 NOT NULL,无 NULL 陷阱)。子查询不相关、仅跑一次,量级 ~10^3,
// 相对已在做的全表 sweep 是零头。取消隐藏后这些项 `*_status` 仍停在 0,谓词一撤即被下一轮
// 枚举自然捞回(缩略图另经画廊可见格 on-demand 路径补跑,AI/人脸经 `maybeAutoResume`)。
//
// 两变体对应两种查询形态(与 exotic 域 NOT_BLOCKED_BY_EXOTIC / _M 同源):裸 `media_items`
// (缩略图三查询 + AI/人脸 count) 用 `EXCLUDE_HIDDEN_ROOTS`;带 `m` 别名(AI/人脸 get_pending
// 因拼绝对路径而 JOIN)用 `EXCLUDE_HIDDEN_ROOTS_M`。
pub(in crate::db::queries) const EXCLUDE_HIDDEN_ROOTS: &str = "AND directory_id NOT IN (
        SELECT id FROM directories WHERE root_id IN (
            SELECT id FROM scan_roots WHERE is_hidden=1))";

/// 同 [`EXCLUDE_HIDDEN_ROOTS`]，用于以 `m` 为 media_items 别名的查询（AI/人脸 get_pending）。
pub(in crate::db::queries) const EXCLUDE_HIDDEN_ROOTS_M: &str = "AND m.directory_id NOT IN (
        SELECT id FROM directories WHERE root_id IN (
            SELECT id FROM scan_roots WHERE is_hidden=1))";

/// 同族第三变体：按 `item_id` 排除隐藏根下媒体，用于**不 JOIN media_items** 的任务表查询
/// （exotic claim——第 5 条生成线,与前四条同口径:隐藏根不烧解码算力）。多一层
/// media_items 子查询,仍全静态不相关、空集全通过;media_items.directory_id 有索引,
/// 隐藏集非空时也只扫隐藏根子树。
pub(in crate::db::queries) const EXCLUDE_HIDDEN_ROOT_ITEMS: &str = "AND item_id NOT IN (
        SELECT id FROM media_items WHERE directory_id IN (
            SELECT id FROM directories WHERE root_id IN (
                SELECT id FROM scan_roots WHERE is_hidden=1)))";

/// 绑定 scan_root 所属卷。新根添加时调用，使其媒体可继承 volume_id 参与缺失检测守门1
/// （未绑卷的新根 → media volume_id 恒 NULL → 缺失检测休眠，见 C5 Piece1）。
pub fn set_scan_root_volume(conn: &Connection, root_id: i64, volume_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE scan_roots SET volume_id = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
        params![root_id, volume_id],
    )?;
    Ok(())
}

/// 改写迁移后扫描根的绝对路径。
///
/// 整根迁移后目录结构与 mtime 不变，而 `directories.rel_path` / `media_items.file_name`
/// 全是相对根的，缩略图 cache_key 也不含盘符（`utils::hash::compute_cache_key`）——
/// 故**改这一行即完成迁移**，零重扫、零派生产物重生成。
///
/// ⚠️ `scan_roots.path` 有 UNIQUE 约束：新路径若已被别的根占用会返回 `AppError::Db`。
/// 调用方应先查重给出可读错误（见 `ipc::scan_commands::relink_scan_root`）。
pub fn update_scan_root_path(conn: &Connection, root_id: i64, new_path: &str) -> Result<()> {
    conn.execute(
        "UPDATE scan_roots SET path = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
        params![root_id, new_path],
    )?;
    Ok(())
}

/// 某扫描根内一个媒体项的「相对位置 + 体征」，供 relink 抽样校验比对新路径下的实际文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSampleItem {
    /// 所在目录相对根的路径（正斜杠；顶级目录为空串）。
    pub rel_path: String,
    pub file_name: String,
    pub file_size: i64,
    /// Unix 秒（与 `scanner::walker` 的取法一致，见 walker.rs 的 `as_secs()`）。
    pub file_mtime: i64,
}

/// 从某扫描根**随机**抽样 N 个未删除媒体项的体征（relink 校验用）。
///
/// 用 `ORDER BY RANDOM()` 而非 `ORDER BY id LIMIT n`：后者只会取到最先扫到的那几个目录，
/// 抽样集中在一角，校验不出「新路径只是碰巧有个同名子目录」这类假阳性。随机抽样要对
/// 全根做一次扫描 + 临时排序（50 万行量级约百毫秒~秒级），但 relink 是用户显式发起的
/// 一次性动作，正确性优先于这点开销。
pub fn sample_root_items(
    conn: &Connection,
    root_id: i64,
    limit: i64,
) -> Result<Vec<RootSampleItem>> {
    let mut stmt = conn.prepare(
        "SELECT d.rel_path, m.file_name, m.file_size, m.file_mtime
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         WHERE d.root_id = ?1 AND m.is_deleted = 0
         ORDER BY RANDOM() LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![root_id, limit], |row| {
        Ok(RootSampleItem {
            rel_path: row.get(0)?,
            file_name: row.get(1)?,
            file_size: row.get(2)?,
            file_mtime: row.get(3)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn delete_scan_root(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM scan_roots WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_scan_roots(conn: &Connection) -> Result<Vec<ScanRoot>> {
    let mut stmt = conn.prepare(
        // id ASC 是必须的稳定 tiebreaker：同秒添加多根时 created_at 相等，顺序否则未定义；
        // 此序须与画廊 folder 目录序的 root 序 (created_at, id) 一致，否则文件树与画廊 root 顺序分歧。
        "SELECT id, path, alias, scan_status, scan_progress, total_files,
                last_scan_at, is_active, created_at, updated_at, is_hidden
         FROM scan_roots ORDER BY created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], map_scan_root)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn get_scan_root(conn: &Connection, id: i64) -> Result<ScanRoot> {
    conn.query_row(
        "SELECT id, path, alias, scan_status, scan_progress, total_files,
                last_scan_at, is_active, created_at, updated_at, is_hidden
         FROM scan_roots WHERE id = ?1",
        params![id],
        map_scan_root,
    )
    .map_err(|_| AppError::ScanRootNotFound(id))
}

pub fn update_scan_root_status(
    conn: &Connection,
    id: i64,
    status: &str,
    progress: i64,
    total: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE scan_roots SET scan_status=?1, scan_progress=?2, total_files=?3,
                 updated_at=strftime('%s','now')
         WHERE id=?4",
        params![status, progress, total, id],
    )?;
    Ok(())
}

pub fn finish_scan_root(conn: &Connection, id: i64, total: i64) -> Result<()> {
    conn.execute(
        "UPDATE scan_roots SET scan_status='idle', scan_progress=?1, total_files=?1,
                 last_scan_at=strftime('%s','now'), updated_at=strftime('%s','now')
         WHERE id=?2",
        params![total, id],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "root_merge_tests.rs"]
mod root_merge_tests;
