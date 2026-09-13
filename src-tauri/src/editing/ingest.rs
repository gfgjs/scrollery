//! 单文件 ingest(方案 C §6 步骤 4):编辑保存落盘成功后,把新文件注册为新 `media_item`。
//!
//! 与目录级批量扫描(`scanner::fast_scan`)共用底层 upsert 原语,但跳过其目录链递归——
//! 编辑输出恒落在源 item 的同一目录,`directory_id` 调用方已知,无需 `ensure_dir_chain`。
//! 也不播种 exotic 任务:v1 编辑输出恒为 jpg/png,两者都不是 exotic catalog 格式
//! (`catalog.resolve_format` 对它们恒返回 `None`,播种分支等价于 no-op)。缩略图队列靠
//! `thumb_status` 列默认值(0=待生成)天然入队,无需额外触发。

use rusqlite::Connection;

use crate::db::queries::{upsert_fast_scan_item, FastScanItem};
use crate::error::Result;
use crate::utils::hash::compute_cache_key_with_mtime_ns;

/// [`ingest_single_file`] 的输入:全部字段调用方已从源 item / 编辑输出算好(stat 已完成),
/// 本函数只管 upsert。
pub struct IngestInput<'a> {
    pub directory_id: i64,
    /// 目录相对路径(与源 item 同目录,已 `normalize_db_path` 过)。
    pub rel_path_norm: &'a str,
    pub file_name: &'a str,
    pub file_size: i64,
    pub file_mtime: i64,
    /// 编辑输出的纳秒 mtime；无法取得时为 0，和扫描 upsert 保持同一精度契约。
    pub file_mtime_ns: i64,
    pub file_format: &'a str,
    pub width: i64,
    pub height: i64,
    pub volume_id: Option<i64>,
}

/// 单文件 ingest(方案 §6 步骤 4)。返回新建或复用的 `item_id`——`view_rotation` 依 schema
/// 默认值落地为 0(方案要求「新 item 明确以 `view_rotation=0` 入库」,`upsert_fast_scan_item`
/// 的 INSERT 分支不显式写该列,交 `ADD COLUMN ... DEFAULT 0` 自动满足)。
pub fn ingest_single_file(conn: &Connection, input: &IngestInput) -> Result<i64> {
    let cache_key = compute_cache_key_with_mtime_ns(
        input.rel_path_norm,
        input.file_name,
        input.file_mtime,
        input.file_mtime_ns,
    );
    let item = FastScanItem {
        directory_id: input.directory_id,
        file_name: input.file_name.to_string(),
        file_size: input.file_size,
        file_mtime: input.file_mtime,
        file_mtime_ns: input.file_mtime_ns,
        file_format: input.file_format.to_string(),
        media_type: "image".to_string(),
        width: input.width,
        height: input.height,
        sort_datetime: input.file_mtime,
        cache_key,
    };
    let outcome = upsert_fast_scan_item(conn, &item, input.volume_id)?;
    Ok(outcome.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');",
        )
        .unwrap();
        c
    }

    fn sample_input(file_name: &'static str, file_mtime: i64) -> IngestInput<'static> {
        IngestInput {
            directory_id: 10,
            rel_path_norm: "",
            file_name,
            file_size: 1234,
            file_mtime,
            file_mtime_ns: file_mtime.saturating_mul(1_000_000_000),
            file_format: "jpg",
            width: 100,
            height: 80,
            volume_id: None,
        }
    }

    #[test]
    fn inserts_new_item_with_default_view_rotation_zero() {
        let c = mem_db();
        let id = ingest_single_file(&c, &sample_input("photo-edit.jpg", 1_700_000_000)).unwrap();
        let (w, h, vr, mt): (i64, i64, i64, String) = c
            .query_row(
                "SELECT width, height, view_rotation, media_type FROM media_items WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((w, h, vr), (100, 80, 0));
        assert_eq!(mt, "image");
    }

    #[test]
    fn each_distinct_file_name_gets_its_own_item() {
        let c = mem_db();
        let id1 = ingest_single_file(&c, &sample_input("a-edit.jpg", 1000)).unwrap();
        let id2 = ingest_single_file(&c, &sample_input("a-edit-2.jpg", 1000)).unwrap();
        assert_ne!(id1, id2);
        let count: i64 = c
            .query_row("SELECT count(*) FROM media_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    /// 极端场景(理论不应发生,`naming::claim_target_path` 已用 `create_new` 保证磁盘上
    /// 该文件名此前不存在):同 directory_id+file_name+file_mtime 重复 ingest 一次,
    /// 函数须表现为「复用同一行」而非重复插入——`upsert_fast_scan_item` 既有的
    /// Unchanged 语义天然满足,这里锁住该行为不被意外改动。
    #[test]
    fn reingesting_identical_path_and_mtime_revives_rather_than_duplicates() {
        let c = mem_db();
        let input = sample_input("photo-edit.jpg", 1000);
        let id1 = ingest_single_file(&c, &input).unwrap();
        let id2 = ingest_single_file(&c, &input).unwrap();
        assert_eq!(id1, id2);
        let count: i64 = c
            .query_row("SELECT count(*) FROM media_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
