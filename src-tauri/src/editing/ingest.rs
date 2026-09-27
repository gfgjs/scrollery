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
