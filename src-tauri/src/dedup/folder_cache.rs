//! 文件夹去重统计的进程内只读快照。
//!
//! 动态聚合 SQL 适合低频重算，但在百万级媒体库上每次展开树节点都会重复付出全库
//! 聚合成本。这里把一次完整统计结果绑定到分析代次、数据代次、哈希版本和筛选器；
//! 任一写路径 bump 数据代次后，旧快照自然失效。

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::queries::{
    DuplicateFolderCandidateCursor, DuplicateFolderStatsRow, DuplicateFolderTreeCursor,
};
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    analysis_generation: u64,
    data_version: u64,
    hash_version: u32,
    include_offline: bool,
    include_zero_byte: bool,
}

#[derive(Debug)]
struct CacheEntry {
    key: CacheKey,
    rows: Arc<Vec<DuplicateFolderStatsRow>>,
}

/// 文件夹统计快照槽。构建期间持锁，避免多个并发首请求同时运行百万级聚合。
#[derive(Debug, Default)]
pub struct DedupFolderStatsCache {
    inner: Mutex<Option<CacheEntry>>,
}

impl DedupFolderStatsCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按完整视图身份命中或构建快照；调用方必须已在 `spawn_blocking` 中。
    pub fn get_or_build(
        &self,
        conn: &Connection,
        analysis_generation: u64,
        data_version: u64,
        hash_version: u32,
        include_offline: bool,
        include_zero_byte: bool,
    ) -> Result<Arc<Vec<DuplicateFolderStatsRow>>> {
        let key = CacheKey {
            analysis_generation,
            data_version,
            hash_version,
            include_offline,
            include_zero_byte,
        };
        let mut guard = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = guard.as_ref() {
            if entry.key == key {
                return Ok(Arc::clone(&entry.rows));
            }
        }

        let rows = Arc::new(crate::db::queries::list_all_duplicate_folder_stats(
            conn,
            include_zero_byte,
        )?);
        *guard = Some(CacheEntry {
            key,
            rows: Arc::clone(&rows),
        });
        Ok(rows)
    }

    /// 清空数据库时主动释放旧快照；普通写路径由 `data_version` 换代失效。
    pub fn clear(&self) {
        *self.inner.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}

/// 从快照中按目录 ID读取一行统计。
pub fn find_folder_stats(
    rows: &[DuplicateFolderStatsRow],
    folder_id: i64,
) -> Option<DuplicateFolderStatsRow> {
    rows.iter().find(|row| row.folder_id == folder_id).cloned()
}

/// 按候选优先级筛选并分页；候选命中的所有祖先会一并保留。
pub fn list_candidate_rows(
    rows: &[DuplicateFolderStatsRow],
    cursor: Option<&DuplicateFolderCandidateCursor>,
    limit: usize,
) -> Vec<DuplicateFolderStatsRow> {
    let candidate_ids = candidate_node_ids(rows);
    let mut matching = rows
        .iter()
        .filter(|row| candidate_ids.contains(&row.folder_id))
        .filter(|row| cursor.is_none_or(|value| candidate_after_cursor(row, value)))
        .collect::<Vec<_>>();
    matching.sort_unstable_by(|left, right| candidate_order(left, right));
    matching.into_iter().take(limit.max(1)).cloned().collect()
}

/// 按真实目录树顺序筛选一层；只返回候选命中节点或其祖先。
pub fn list_tree_rows(
    rows: &[DuplicateFolderStatsRow],
    parent_id: Option<i64>,
    cursor: Option<&DuplicateFolderTreeCursor>,
    limit: usize,
) -> Vec<DuplicateFolderStatsRow> {
    let candidate_ids = candidate_node_ids(rows);
    let mut matching = rows
        .iter()
        .filter(|row| candidate_ids.contains(&row.folder_id))
        .filter(|row| row.parent_id == parent_id)
        .filter(|row| cursor.is_none_or(|value| tree_after_cursor(row, value)))
        .collect::<Vec<_>>();
    matching.sort_unstable_by(|left, right| {
        sqlite_nocase_cmp(&left.name, &right.name)
            .then_with(|| left.folder_id.cmp(&right.folder_id))
    });
    matching.into_iter().take(limit.max(1)).cloned().collect()
}

fn candidate_node_ids(rows: &[DuplicateFolderStatsRow]) -> HashSet<i64> {
    let by_id = rows
        .iter()
        .map(|row| (row.folder_id, row))
        .collect::<HashMap<_, _>>();
    let mut ids = HashSet::new();
    for row in rows
        .iter()
        .filter(|row| row.duplicate_positions > 0 || row.unreviewed_positions > 0)
    {
        let mut current = Some(row.folder_id);
        while let Some(folder_id) = current {
            if !ids.insert(folder_id) {
                break;
            }
            current = by_id.get(&folder_id).and_then(|parent| parent.parent_id);
        }
    }
    ids
}

fn candidate_order(left: &DuplicateFolderStatsRow, right: &DuplicateFolderStatsRow) -> Ordering {
    let left_actionable = i64::from(left.recommended_positions > 0);
    let right_actionable = i64::from(right.recommended_positions > 0);
    right_actionable
        .cmp(&left_actionable)
        .then_with(|| {
            ratio_cmp(
                right.recommended_positions,
                right.total_positions,
                left.recommended_positions,
                left.total_positions,
            )
        })
        .then_with(|| {
            ratio_cmp(
                right.external_covered_positions,
                right.total_positions,
                left.external_covered_positions,
                left.total_positions,
            )
        })
        .then_with(|| {
            right
                .recommended_logical_bytes
                .cmp(&left.recommended_logical_bytes)
        })
        .then_with(|| left.retained_positions.cmp(&right.retained_positions))
        .then_with(|| left.depth.cmp(&right.depth))
        .then_with(|| left.folder_id.cmp(&right.folder_id))
}

fn candidate_after_cursor(
    row: &DuplicateFolderStatsRow,
    cursor: &DuplicateFolderCandidateCursor,
) -> bool {
    let actionable = i64::from(row.recommended_positions > 0);
    if actionable != cursor.actionable {
        return actionable < cursor.actionable;
    }
    let recommended = ratio_cmp(
        row.recommended_positions,
        row.total_positions,
        cursor.recommended_positions,
        cursor.total_positions,
    );
    if recommended != Ordering::Equal {
        return recommended == Ordering::Less;
    }
    let external = ratio_cmp(
        row.external_covered_positions,
        row.total_positions,
        cursor.external_covered_positions,
        cursor.total_positions,
    );
    if external != Ordering::Equal {
        return external == Ordering::Less;
    }
    if row.recommended_logical_bytes != cursor.recommended_logical_bytes {
        return row.recommended_logical_bytes < cursor.recommended_logical_bytes;
    }
    if row.retained_positions != cursor.retained_positions {
        return row.retained_positions > cursor.retained_positions;
    }
    if row.depth != cursor.depth {
        return row.depth > cursor.depth;
    }
    row.folder_id > cursor.folder_id
}

/// 比较两个非负整数比例；分母为零时模拟 SQLite `NULL`，在 DESC 排序中落到末尾。
fn ratio_cmp(
    left_numerator: i64,
    left_denominator: i64,
    right_numerator: i64,
    right_denominator: i64,
) -> Ordering {
    match (left_denominator > 0, right_denominator > 0) {
        (false, false) => Ordering::Equal,
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        (true, true) => (i128::from(left_numerator.max(0)) * i128::from(right_denominator))
            .cmp(&(i128::from(right_numerator.max(0)) * i128::from(left_denominator))),
    }
}

fn tree_after_cursor(row: &DuplicateFolderStatsRow, cursor: &DuplicateFolderTreeCursor) -> bool {
    match sqlite_nocase_cmp(&row.name, &cursor.name) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => row.folder_id > cursor.folder_id,
    }
}

fn sqlite_nocase_cmp(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(ascii_lower)
        .cmp(right.bytes().map(ascii_lower))
}

fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn row(
        folder_id: i64,
        parent_id: Option<i64>,
        name: &str,
        depth: i64,
        total_positions: i64,
        duplicate_positions: i64,
        unreviewed_positions: i64,
        recommended_positions: i64,
    ) -> DuplicateFolderStatsRow {
        DuplicateFolderStatsRow {
            folder_id,
            root_id: 1,
            parent_id,
            name: name.to_string(),
            rel_path: name.to_string(),
            depth,
            total_positions,
            analyzed_positions: total_positions.saturating_sub(unreviewed_positions),
            duplicate_positions,
            external_covered_positions: 0,
            internal_duplicate_positions: duplicate_positions,
            unreviewed_positions,
            protected_positions: 0,
            recommended_positions,
            recommended_logical_bytes: recommended_positions.saturating_mul(10),
            retained_positions: total_positions.saturating_sub(recommended_positions),
        }
    }

    #[test]
    fn candidates_keep_hit_ancestors_and_use_stable_priority_order() {
        let rows = vec![
            row(1, None, "root", 0, 10, 0, 0, 0),
            row(2, Some(1), "child", 1, 10, 2, 0, 2),
            row(3, Some(2), "leaf", 2, 10, 0, 1, 0),
            row(4, Some(1), "empty", 1, 0, 0, 0, 0),
        ];
        let result = list_candidate_rows(&rows, None, 10);
        assert_eq!(
            result.iter().map(|item| item.folder_id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn tree_cursor_and_ascii_nocase_match_directory_order() {
        let rows = vec![
            row(1, None, "root", 0, 1, 0, 0, 0),
            row(2, Some(1), "beta", 1, 1, 1, 0, 0),
            row(3, Some(1), "Alpha", 1, 1, 1, 0, 0),
        ];
        let roots = list_tree_rows(&rows, None, None, 10);
        assert_eq!(roots[0].folder_id, 1);
        let children = list_tree_rows(&rows, Some(1), None, 10);
        assert_eq!(
            children
                .iter()
                .map(|item| item.folder_id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        let cursor = DuplicateFolderTreeCursor {
            name: "alpha".to_string(),
            folder_id: 3,
        };
        let after = list_tree_rows(&rows, Some(1), Some(&cursor), 10);
        assert_eq!(
            after.iter().map(|item| item.folder_id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn cache_reuses_same_identity_and_rebuilds_on_version_change() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        let cache = DedupFolderStatsCache::new();
        let first = cache.get_or_build(&conn, 1, 1, 1, false, false).unwrap();
        let second = cache.get_or_build(&conn, 1, 1, 1, false, false).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let changed = cache.get_or_build(&conn, 1, 2, 1, false, false).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
    }
}
