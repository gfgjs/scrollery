//! 媒体项域 DAO:读取/detail、批量与单项用户属性(收藏/评分/颜色)、companion、
//! 回收站、统计、路径信息与 cache_key、拖拽复制
//! (T 线拆分自 queries.rs,SQL 与行为不变;不吸收 layout SQL builder,§4.1)。

use rusqlite::{params, Connection, OptionalExtension, Row};

// 跨域定向引用(§4.3):选区分批口径由 selection owner(layout)定义。
use super::layout::SELECTION_BATCH_CHUNK;
use crate::db::models::{AppStats, ImageMeta, MediaDetail, MediaItem, MediaMeta, VideoMeta};
use crate::error::{AppError, Result};
use crate::utils::path::resolve_media_path;

fn map_media_item(row: &Row<'_>) -> rusqlite::Result<MediaItem> {
    Ok(MediaItem {
        id: row.get(0)?,
        directory_id: row.get(1)?,
        file_name: row.get(2)?,
        file_size: row.get(3)?,
        file_mtime: row.get(4)?,
        file_format: row.get(5)?,
        media_type: row.get(6)?,
        width: row.get(7)?,
        height: row.get(8)?,
        duration_ms: row.get(9)?,
        sort_datetime: row.get(10)?,
        cache_key: row.get(11)?,
        thumb_status: row.get(12)?,
        thumb_path: row.get(13)?,
        thumbhash: row.get(14)?,
        is_favorited: row.get::<_, i64>(15)? != 0,
        is_deleted: row.get::<_, i64>(16)? != 0,
        deleted_at: row.get(17)?,
        rating: row.get(18)?,
        is_live_photo: row.get::<_, i64>(19)? != 0,
        has_embedded_video: row.get::<_, i64>(20)? != 0,
        companion_of: row.get(21)?,
        content_hash: row.get(22)?,
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
        // color_label 追加在末列（索引 25）而非插中间——保既有列位全不动，仅新增一位，
        // 避免移动 rating 之后所有列引发静默串列。喂入的三处 SELECT 同样把它追加到末尾。
        color_label: row.get(25)?,
        // view_rotation 同理追加在 color_label 之后（索引 26，V20）；三处 SELECT 同步追加。
        view_rotation: row.get(26)?,
        // playback_position_ms 同理追加在 view_rotation 之后（索引 27，V23）；三处 SELECT 同步追加。
        playback_position_ms: row.get(27)?,
        // source_revision 追加在末列（索引 28），避免移动既有字段列位；三处 SELECT 同步追加。
        source_revision: row.get(28)?,
    })
}

/// 物理复制完成后从目标文件读取的大小与修改时间，用于建立副本的缓存身份。
pub struct MediaCopyFileStat {
    /// 目标文件的字节数。
    pub file_size: i64,
    /// 修改时间的 Unix 秒值。
    pub file_mtime: i64,
    /// 修改时间的绝对 Unix 纳秒值；0 表示调用方无法取得精度。
    pub file_mtime_ns: i64,
}

/// 将媒体项的用户和内容属性复制到 `target_dir_id`，以目标文件信息建立独立缓存身份。
/// 返回新 id，供调用方精确撤销复制；副本的缩略图由其独立生成流程建立。
/// 源行在复制期间消失时返回 `MediaNotFound`，不生成可撤销的副本记录。
/// 调用方须持有本事务至副本文件发布成功后再提交；本函数不创建嵌套事务或自行提交。
pub fn duplicate_media_item_into_dir(
    tx: &rusqlite::Transaction<'_>,
    src_id: i64,
    target_dir_id: i64,
    stat: &MediaCopyFileStat,
) -> Result<i64> {
    // 计算身份与复制属性共用事务，避免另一连接改名/移动目录后落入不匹配的缓存键。
    let file_name: String = tx
        .query_row(
            "SELECT file_name FROM media_items WHERE id=?1",
            [src_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(AppError::MediaNotFound(src_id))?;
    let rel_path: String = tx.query_row(
        "SELECT rel_path FROM directories WHERE id=?1",
        [target_dir_id],
        |row| row.get(0),
    )?;
    let cache_key = crate::utils::hash::compute_cache_key_with_mtime_ns(
        target_dir_id,
        &rel_path,
        &file_name,
        stat.file_mtime,
        stat.file_mtime_ns,
    );
    // 列清单取舍(2026-07-10 审查 A3):内容/用户属性(color_label 用户资产、content_identifier
    // Live Photo 配对键——字节级复制保留内容内在标识)必须随行复制;而 volume_id /
    // volume_relative_path / availability 是**位置属性**——副本物理落在目标目录,照抄源值会指向
    // 源位置(缺失检测误报/重链接错键),故有意不复制,留 NULL/DEFAULT 由下次扫描 COALESCE 治愈。
    // 2026-07-23 裁决 J11 补齐:view_rotation(V20 看图台展示旋转)与 playback_position_ms
    // (V23 播放进度记忆)同为用户属性,按 A3 原则随行——先前遗漏致副本「转回去」,与本注释矛盾。
    // RETURNING 只返回本次真正插入的行；零行时 last_insert_rowid 仍会指向之前的无关资产。
    // 副本即使与源字节相同也须拥有独立缓存；源路径以后可能被同时间的新文件重新占用。
    let new_id = tx
        .query_row(
            "INSERT INTO media_items
            (directory_id, file_name, file_size, file_mtime, file_mtime_ns, file_format, media_type,
             width, height, duration_ms, sort_datetime, cache_key, thumb_status, thumb_path,
             thumbhash, is_favorited, is_deleted, deleted_at, rating, is_live_photo,
             has_embedded_video, companion_of, content_hash, color_label, content_identifier,
             view_rotation, playback_position_ms, created_at, updated_at)
         SELECT ?1, file_name, ?3, ?4, ?5, file_format, media_type,
             width, height, duration_ms, sort_datetime, ?6, 0, NULL,
             NULL, is_favorited, is_deleted, deleted_at, rating, is_live_photo,
             has_embedded_video, companion_of, content_hash, color_label, content_identifier,
             view_rotation, playback_position_ms, strftime('%s','now'), strftime('%s','now')
         FROM media_items WHERE id = ?2
         RETURNING id",
            params![
                target_dir_id,
                src_id,
                stat.file_size,
                stat.file_mtime,
                stat.file_mtime_ns,
                cache_key
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(AppError::MediaNotFound(src_id))?;
    Ok(new_id)
}

#[cfg(test)]
mod duplicate_media_item_tests {
    //! 2026-07-10 审查 A3 回归钉:复制须带走用户/内容属性,且**不得**带走位置属性。
    use super::*;

    /// 源行在物理复制后消失，INSERT SELECT 不插入任何行；不得返回无关资产的旧 rowid。
    #[test]
    fn missing_copy_source_never_returns_unrelated_item_id() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, 'src', 's'), (11, 1, 'dst', 'd');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, sort_datetime, cache_key) VALUES
                 (1, 10, 'source.jpg', 5, 1, 'jpg', 1, 11),
                 (2, 10, 'untouched.jpg', 9, 2, 'jpg', 2, 22);
             DELETE FROM media_items WHERE id=1;",
        )
        .unwrap();
        assert_eq!(c.last_insert_rowid(), 2, "连接还保存着无关资产的旧 rowid");

        let tx = c.transaction().unwrap();
        let result = duplicate_media_item_into_dir(
            &tx,
            1,
            11,
            &MediaCopyFileStat {
                file_size: 5,
                file_mtime: 1,
                file_mtime_ns: 1_000_000_000,
            },
        );
        assert!(
            matches!(result, Err(AppError::MediaNotFound(1))),
            "缺失源必须报错，不能把旧 rowid 作为副本交给撤销逻辑: {result:?}"
        );
        tx.rollback().unwrap();
        let survivor: (i64, String, i64) = c
            .query_row(
                "SELECT directory_id, file_name, cache_key FROM media_items WHERE id=?1",
                params![2],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(survivor, (10, "untouched.jpg".to_string(), 22));
        assert_eq!(
            c.query_row("SELECT count(*) FROM media_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1,
            "没有创建任何副本，也不改变无关资产"
        );
    }

    #[test]
    // 8 元组仅为一次性读回断言,拆结构体反增噪——测试内豁免 type_complexity。
    #[allow(clippy::type_complexity)]
    fn copies_user_and_content_fields_but_resets_location_fields() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, 'src', 's'), (11, 1, 'dst', 'd');
             INSERT INTO volumes (id, stable_id, label) VALUES (7, 'vol-guid-7', 'V');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, width, height, sort_datetime, cache_key,
                 color_label, content_identifier, volume_id, volume_relative_path, availability,
                 view_rotation, playback_position_ms, thumb_status, thumb_path, thumbhash,
                 file_mtime_ns, source_revision)
             VALUES (1, 10, 'a.heic', 1, 1, 'heic', 'image', 10, 10, 100, 11,
                 'red', 'live-pair-1', 7, 'src/a.heic', 'offline', 90, 4321,
                 3, '/r/src/a.heic', X'0102', 1000000000, 8);",
        )
        .unwrap();

        let stat = MediaCopyFileStat {
            file_size: 246,
            file_mtime: 1_700_000_000,
            file_mtime_ns: 1_700_000_000_123_456_700,
        };
        let tx = c.transaction().unwrap();
        let new_id = duplicate_media_item_into_dir(&tx, 1, 11, &stat).unwrap();
        tx.commit().unwrap();

        let (dir, label, cid, vol, vrp, avail, rot, pos): (
            i64,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            String,
            Option<i64>,
            Option<i64>,
        ) = c
            .query_row(
                "SELECT directory_id, color_label, content_identifier,
                        volume_id, volume_relative_path, availability,
                        view_rotation, playback_position_ms
                 FROM media_items WHERE id=?1",
                params![new_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(dir, 11);
        // 用户/内容属性随行(color_label 用户资产、content_identifier Live Photo 配对键)。
        assert_eq!(label.as_deref(), Some("red"));
        assert_eq!(cid.as_deref(), Some("live-pair-1"));
        // 用户属性随行补齐(2026-07-23 裁决 J11):看图台旋转与播放进度记忆均随副本。
        assert_eq!(rot, Some(90));
        assert_eq!(pos, Some(4321));
        // 位置属性复位:副本在目标目录,照抄源 volume 三元组会指向源位置。
        assert_eq!(vol, None);
        assert_eq!(vrp, None);
        assert_eq!(avail, "online");

        let expected_key = crate::utils::hash::compute_cache_key_with_mtime_ns(
            11,
            "dst",
            "a.heic",
            stat.file_mtime,
            stat.file_mtime_ns,
        );
        let (key, size, mtime, ns, revision, thumb_status, thumb_path, thumbhash): (
            i64,
            i64,
            i64,
            Option<i64>,
            i64,
            i64,
            Option<String>,
            Option<Vec<u8>>,
        ) = c
            .query_row(
                "SELECT cache_key,file_size,file_mtime,file_mtime_ns,source_revision,
                thumb_status,thumb_path,thumbhash FROM media_items WHERE id=?1",
                [new_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(key, expected_key);
        assert_ne!(key, 11, "副本不能沿用源目录的缓存身份");
        assert_eq!(
            (size, mtime, ns),
            (stat.file_size, stat.file_mtime, Some(stat.file_mtime_ns))
        );
        assert_eq!(revision, 1);
        assert_eq!(
            (thumb_status, thumb_path, thumbhash),
            (0, None, None),
            "不得继承指向源文件的直显路径"
        );
        let scanned = super::super::FastScanItem {
            directory_id: 11,
            file_name: "a.heic".into(),
            file_size: stat.file_size,
            file_mtime: stat.file_mtime,
            file_mtime_ns: stat.file_mtime_ns,
            file_format: "heic".into(),
            media_type: "image".into(),
            width: 10,
            height: 10,
            sort_datetime: stat.file_mtime,
            cache_key: expected_key,
        };
        assert_eq!(
            super::super::upsert_fast_scan_item(&c, &scanned, None).unwrap(),
            super::super::UpsertOutcome::Unchanged(new_id),
            "刚复制的文件重扫不应因遗漏纳秒时间进入可疑变更流程",
        );
        assert_eq!(
            c.query_row(
                "SELECT cache_key,source_revision,thumb_path FROM media_items WHERE id=1",
                [],
                |r| Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?
                ))
            )
            .unwrap(),
            (11, 8, "/r/src/a.heic".into()),
            "副本落库和重扫不能修改源项",
        );
    }
}

pub fn get_media_item(conn: &Connection, id: i64) -> Result<MediaItem> {
    conn.query_row(
        "SELECT id, directory_id, file_name, file_size, file_mtime, file_format,
                media_type, width, height, duration_ms, sort_datetime, cache_key,
                thumb_status, thumb_path, thumbhash, is_favorited, is_deleted,
                deleted_at, rating, is_live_photo, has_embedded_video, companion_of,
                content_hash, created_at, updated_at, color_label, view_rotation,
                playback_position_ms, source_revision
         FROM media_items WHERE id=?1",
        params![id],
        map_media_item,
    )
    .map_err(|_| AppError::MediaNotFound(id))
}

pub fn get_media_detail(conn: &Connection, id: i64) -> Result<MediaDetail> {
    let item = get_media_item(conn, id)?;

    // 通过关联目录和扫描根目录解析绝对路径
    let (rel_path, root_path): (String, String) = conn
        .query_row(
            "SELECT d.rel_path, r.path
             FROM directories d JOIN scan_roots r ON d.root_id = r.id
             WHERE d.id=?1",
            params![item.directory_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| AppError::PathResolution(e.to_string()))?;

    let abs_path = resolve_media_path(&root_path, &rel_path, &item.file_name);

    // 系统可用态（缺失检测 Part2 §3.2）：供查看器对「卷离线/文件缺失」明确提示。
    let availability: String = conn
        .query_row(
            "SELECT availability FROM media_items WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "online".to_string());

    // 图像元数据（可选 — 可能尚不存在）
    let image_meta = conn
        .query_row(
            "SELECT item_id, orientation, exif_datetime, exif_make, exif_model, exif_lens,
                    exif_focal_length, exif_aperture, exif_shutter, exif_iso,
                    exif_gps_lat, exif_gps_lng,
                    dominant_hue, dominant_sat, dominant_lum, dominant_hex, is_monochrome
             FROM image_meta WHERE item_id=?1",
            params![id],
            |row| {
                Ok(ImageMeta {
                    item_id: row.get(0)?,
                    orientation: row.get(1)?,
                    exif_datetime: row.get(2)?,
                    exif_make: row.get(3)?,
                    exif_model: row.get(4)?,
                    exif_lens: row.get(5)?,
                    exif_focal_length: row.get(6)?,
                    exif_aperture: row.get(7)?,
                    exif_shutter: row.get(8)?,
                    exif_iso: row.get(9)?,
                    exif_gps_lat: row.get(10)?,
                    exif_gps_lng: row.get(11)?,
                    dominant_hue: row.get(12)?,
                    dominant_sat: row.get(13)?,
                    dominant_lum: row.get(14)?,
                    dominant_hex: row.get(15)?,
                    is_monochrome: row.get::<_, i64>(16)? != 0,
                })
            },
        )
        .ok();

    // 视频元数据(播放器线):LEFT JOIN 语义——无 video_meta 行(非视频 / 尚未 enrichment)时 None。
    let video_meta = conn
        .query_row(
            "SELECT video_codec, fps, bitrate, rotation, has_audio FROM video_meta WHERE item_id=?1",
            params![id],
            |row| {
                Ok(VideoMeta {
                    video_codec: row.get(0)?,
                    fps: row.get(1)?,
                    bitrate: row.get(2)?,
                    // GA-fix #7:改经 Option<i64> 兜底 0,抗未来写入路径把 rotation/has_audio
                    // 写成 NULL(当前 schema/写入路径均非 NULL,此处是前瞻性防御而非回归修复)。
                    rotation: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    has_audio: row.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
                })
            },
        )
        .ok();

    Ok(MediaDetail {
        item,
        abs_path,
        image_meta,
        availability,
        video_meta,
    })
}

/// R1-2（S4 消费）：对 id 集分块执行「单值 SET」批量 UPDATE，整体包在一个事务内。
///
/// 分块（[`SELECTION_BATCH_CHUNK`]）的原因：SelectAll 解析出的 id 可达百万级，拼进单条
/// IN 会超 SQLite 绑定变量上限；分块后事务仍保证整体原子。`set_expr` 为 SET 片段
/// （其值绑定为 ?1，id 占位从 ?2 起逐块生成）——仅拼接**编译期常量片段与占位符**，
/// 值一律参数绑定（项目 SQL 红线）。返回受影响总行数。
fn batch_update_set_value(
    conn: &Connection,
    set_expr: &str,
    value: rusqlite::types::Value,
    ids: &[i64],
) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut affected: u64 = 0;
    for chunk in ids.chunks(SELECTION_BATCH_CHUNK) {
        let placeholders: Vec<String> = (0..chunk.len()).map(|i| format!("?{}", i + 2)).collect();
        let sql = format!(
            "UPDATE media_items SET {set_expr} WHERE id IN ({}) AND is_deleted = 0",
            placeholders.join(",")
        );
        let mut params: Vec<rusqlite::types::Value> = vec![value.clone()];
        for id in chunk {
            params.push(rusqlite::types::Value::Integer(*id));
        }
        affected += tx.execute(&sql, rusqlite::params_from_iter(params.iter()))? as u64;
    }
    tx.commit()?;
    Ok(affected)
}

/// 批量设收藏（R1-2：IPC 命令解析 SelectionDescriptor 后落到此，SQL 收拢回 db 层）。
pub fn batch_set_favorite(conn: &Connection, ids: &[i64], value: bool) -> Result<u64> {
    batch_update_set_value(
        conn,
        "is_favorited = ?1",
        rusqlite::types::Value::Integer(i64::from(value)),
        ids,
    )
}

/// 批量设评分（0-5，越界钳制）。镜像 [`batch_set_favorite`]。
pub fn batch_set_rating(conn: &Connection, ids: &[i64], rating: i64) -> Result<u64> {
    batch_update_set_value(
        conn,
        "rating = ?1, updated_at = strftime('%s','now')",
        rusqlite::types::Value::Integer(rating.clamp(0, 5)),
        ids,
    )
}

/// 批量设颜色标签（0=清除 / 1-7 色档，越界钳制）。镜像 [`batch_set_favorite`]。
pub fn batch_set_color_label(conn: &Connection, ids: &[i64], color_label: i64) -> Result<u64> {
    batch_update_set_value(
        conn,
        "color_label = ?1, updated_at = strftime('%s','now')",
        rusqlite::types::Value::Integer(color_label.clamp(0, 7)),
        ids,
    )
}

/// Fetch heavy per-item metadata (file name, dir path, EXIF, GPS) for a set of ids.
/// Backs `get_meta_for_viewport`, which lazily populates only the visible window.
///
/// 为一组 id 批量获取重型逐项元数据（文件名、目录路径、EXIF、GPS）。
/// 支撑 `get_meta_for_viewport` —— 仅懒填充可视窗口。
pub fn get_media_meta_batch(conn: &Connection, ids: &[i64]) -> Result<Vec<MediaMeta>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    // id 为 i64 — 内联安全；规避大窗口下 SQLite 的绑定参数数量上限。
    let in_clause = ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT m.id,
                m.file_name,
                CASE WHEN d.rel_path = '' THEN r.path ELSE r.path || '/' || d.rel_path END AS dir_path,
                im.exif_gps_lat, im.exif_gps_lng,
                im.exif_make, im.exif_model, im.exif_lens,
                im.exif_focal_length, im.exif_aperture, im.exif_shutter, im.exif_iso
         FROM media_items m
         JOIN directories d ON m.directory_id = d.id
         JOIN scan_roots r ON d.root_id = r.id
         LEFT JOIN image_meta im ON m.id = im.item_id
         WHERE m.id IN ({in_clause})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(MediaMeta {
            id: row.get(0)?,
            file_name: row.get(1)?,
            dir_path: row.get(2)?,
            gps_lat: row.get(3)?,
            gps_lng: row.get(4)?,
            exif_make: row.get(5)?,
            exif_model: row.get(6)?,
            exif_lens: row.get(7)?,
            exif_focal_length: row.get(8)?,
            exif_aperture: row.get(9)?,
            exif_shutter: row.get(10)?,
            exif_iso: row.get(11)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn update_live_photo_flags(
    conn: &Connection,
    item_id: i64,
    is_live: bool,
    has_embedded: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET is_live_photo=?1, has_embedded_video=?2,
                 updated_at=strftime('%s','now')
         WHERE id=?3",
        params![is_live as i64, has_embedded as i64, item_id],
    )?;
    Ok(())
}

pub fn set_companion_of(conn: &Connection, companion_id: i64, main_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET companion_of=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![main_id, companion_id],
    )?;
    Ok(())
}

// ── 收藏 / 评分 / 软删除 ────────────────────────────────────────

pub fn toggle_favorite(conn: &Connection, item_id: i64) -> Result<bool> {
    conn.execute(
        "UPDATE media_items SET is_favorited = NOT is_favorited,
                 updated_at=strftime('%s','now')
         WHERE id=?1",
        params![item_id],
    )?;
    let new_val: i64 = conn.query_row(
        "SELECT is_favorited FROM media_items WHERE id=?1",
        params![item_id],
        |row| row.get(0),
    )?;
    Ok(new_val != 0)
}

pub fn set_rating(conn: &Connection, item_id: i64, rating: i64) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET rating=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![rating, item_id],
    )?;
    Ok(())
}

/// 设置颜色标签（0=无，1-7 色档；调用方负责 clamp）。镜像 `set_rating`（T16）。
pub fn set_color_label(conn: &Connection, item_id: i64, color_label: i64) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET color_label=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![color_label, item_id],
    )?;
    Ok(())
}

/// 设置看图台展示旋转（归一化 0/90/180/270；调用方负责归一化）。镜像 `set_color_label`（V20）。
pub fn set_view_rotation(conn: &Connection, item_id: i64, view_rotation: i64) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET view_rotation=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![view_rotation, item_id],
    )?;
    Ok(())
}

/// 设置播放器播放位置记忆(ms;调用方负责 clamp ≥0)。镜像 `set_view_rotation`(V23)。
pub fn set_playback_position(conn: &Connection, item_id: i64, ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE media_items SET playback_position_ms=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![ms, item_id],
    )?;
    Ok(())
}

/// 把一组 id 扩展为「自身 ∪ 其 Live Photo companion」（T18 §6.1，结构性写操作——删/移/恢复——专用）。
///
/// Live Photo 的 mov/mp4 伴随项以 `companion_of` 指向静图、在画廊**不独立显示**；删/移/恢复静图时
/// 必须连带处理其伴随项，否则伴随文件成孤儿（D5 取证：`soft_delete_items` 此前不展开 → 现存 bug）。
/// 评分/收藏等元数据操作**不**调用本函数（companion 不单独评分）。分块查 companion 避免单条巨 IN。
pub fn expand_companions(conn: &Connection, ids: &[i64]) -> Result<Vec<i64>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut out: Vec<i64> = ids.to_vec();
    let mut seen: std::collections::HashSet<i64> = ids.iter().copied().collect();
    for chunk in ids.chunks(SELECTION_BATCH_CHUNK) {
        let placeholders: Vec<String> = (0..chunk.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT id FROM media_items WHERE companion_of IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| r.get::<_, i64>(0))?;
        for r in rows {
            let cid = r?;
            // seen 去重：companion 不会与输入 id 重复，但多个静图可能指向同一伴随项（防御）。
            if seen.insert(cid) {
                out.push(cid);
            }
        }
    }
    Ok(out)
}

// 删除/恢复只撤销普通图片的展示引用；插件产物仍由独立任务持有。
const ORDINARY_IMAGE_THUMBNAIL: &str = "media_type='image'
    AND NOT EXISTS (
        SELECT 1 FROM exotic_catalog_formats ec, json_each(ec.capabilities_json) capability
        WHERE ec.format=media_items.file_format AND capability.value='thumbnail')
    AND NOT EXISTS (
        SELECT 1 FROM exotic_tasks et
        WHERE et.item_id=media_items.id AND et.capability='thumbnail')";

pub fn soft_delete_items(conn: &Connection, item_ids: &[i64]) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    // D5：连带其 Live Photo companion，避免删静图留下孤儿伴随视频。
    let ids = expand_companions(conn, item_ids)?;
    let tx = conn.unchecked_transaction()?;
    let sql = format!(
        "UPDATE media_items SET is_deleted=1, source_revision=source_revision+1,
                 thumb_status=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN 0 ELSE thumb_status END,
                 thumb_path=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN NULL ELSE thumb_path END,
                 thumbhash=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN NULL ELSE thumbhash END,
                 deleted_at=strftime('%s','now'),
                 updated_at=strftime('%s','now')
         WHERE id=?1"
    );
    for &id in &ids {
        tx.execute(&sql, params![id])?;
        // 删除/恢复会改变去重候选的可见性；清掉旧 sidecar，避免恢复后在尚未重新
        // 分析时复用一份可能已过期的精确证据。
        tx.execute("DELETE FROM dedup_index WHERE item_id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn restore_items(conn: &Connection, item_ids: &[i64]) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    // D5：恢复静图时对称地连带恢复其 companion。
    let ids = expand_companions(conn, item_ids)?;
    let tx = conn.unchecked_transaction()?;
    let sql = format!(
        "UPDATE media_items SET is_deleted=0, source_revision=source_revision+1,
                 thumb_status=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN 0 ELSE thumb_status END,
                 thumb_path=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN NULL ELSE thumb_path END,
                 thumbhash=CASE WHEN {ORDINARY_IMAGE_THUMBNAIL} THEN NULL ELSE thumbhash END,
                 deleted_at=NULL,
                 updated_at=strftime('%s','now')
         WHERE id=?1"
    );
    for &id in &ids {
        tx.execute(&sql, params![id])?;
        tx.execute("DELETE FROM dedup_index WHERE item_id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

// ── 统计 ─────────────────────────────────────────────────────────────────────

pub fn get_app_stats(conn: &Connection) -> Result<AppStats> {
    // R2-6 合一:原 8 条独立 COUNT(8 次扫描,且并发写下互相可能不一致)→ 单次全表
    // 扫描 + FILTER 聚合(库内 list_volumes_with_item_counts 已有先例)。三处口径差异
    // 是既有语义,逐字保留:favorited 不排 companion、deleted 不加任何过滤、其余 6 项
    // 排 companion+软删。
    //
    // 隐藏根排除(V21):顶层 WHERE 收窄喂给所有 FILTER 的行——隐藏根的项(含其回收站项)不计入
    // 任何桶,与画廊/时间轴一致。空集(常态)→ 不加 WHERE,SQL 与今日逐字节一致。
    let hidden = super::scan::hidden_root_ids(conn)?;
    let mut sql = String::from(
        "SELECT
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND availability != 'missing'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND availability != 'missing' AND media_type='image'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND availability != 'missing' AND media_type='video'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND availability != 'missing' AND media_type='audio'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND availability != 'missing' AND media_type='document'),
            COUNT(*) FILTER (WHERE is_favorited=1 AND is_deleted=0 AND availability != 'missing'),
            COUNT(*) FILTER (WHERE is_deleted=1),
            COUNT(*) FILTER (WHERE is_live_photo=1 AND is_deleted=0 AND companion_of IS NULL AND availability != 'missing')
         FROM media_items",
    );
    if !hidden.is_empty() {
        let placeholders: Vec<String> = (0..hidden.len()).map(|i| format!("?{}", i + 1)).collect();
        sql.push_str(&format!(
            "\n         WHERE directory_id NOT IN (SELECT id FROM directories WHERE root_id IN ({}))",
            placeholders.join(",")
        ));
    }
    let refs: Vec<&dyn rusqlite::ToSql> =
        hidden.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
    Ok(conn.query_row(&sql, refs.as_slice(), |r| {
        Ok(AppStats {
            total_items: r.get(0)?,
            total_images: r.get(1)?,
            total_videos: r.get(2)?,
            total_audios: r.get(3)?,
            total_documents: r.get(4)?,
            total_favorited: r.get(5)?,
            total_deleted: r.get(6)?,
            total_live_photos: r.get(7)?,
        })
    })?)
}

/// 获取媒体项的完整路径信息。
pub fn get_item_path_info(conn: &Connection, item_id: i64) -> Result<(String, String, String)> {
    conn.query_row(
        "SELECT r.path, d.rel_path, m.file_name
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         JOIN scan_roots r ON r.id = d.root_id
         WHERE m.id=?1",
        params![item_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|_| AppError::MediaNotFound(item_id))
}

/// 获取实况照片的伴随视频 URL（Apple 风格：按文件主干）。
pub fn get_companion_item_id(conn: &Connection, item_id: i64) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM media_items WHERE companion_of=?1 LIMIT 1",
        params![item_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// 某未删除项的 `cache_key`，供按 id 编码封面/缩略图使用。
pub fn get_item_cache_key(conn: &Connection, item_id: i64) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT cache_key FROM media_items WHERE id = ?1 AND is_deleted = 0",
        params![item_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// T18 S3：`expand_companions` + soft-delete/restore 的 Live Photo companion 连带（D5 孤儿 bug 修复）。
#[cfg(test)]
mod companion_expand_tests {
    use super::*;

    /// seed：id1 静图 + id2 其 companion(companion_of=1) + id3 独立项。FK OFF 免构造目录链。
    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, companion_of)
                VALUES (1, 1, 'live.jpg', 1,1,'jpg','image',0,0,100,0, NULL),
                       (2, 1, 'live.mov', 1,1,'mov','video',0,0,100,0, 1),
                       (3, 1, 'solo.jpg', 1,1,'jpg','image',0,0,200,0, NULL);",
        )
        .unwrap();
        c
    }

    fn is_deleted(c: &Connection, id: i64) -> i64 {
        c.query_row(
            "SELECT is_deleted FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn soft_delete_cascades_to_companion() {
        let c = mem_db();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (1, 1, 1, 'ready', 1), (2, 1, 1, 'ready', 1)",
            [],
        )
        .unwrap();
        soft_delete_items(&c, &[1]).unwrap();
        assert_eq!(is_deleted(&c, 1), 1, "静图已删");
        assert_eq!(is_deleted(&c, 2), 1, "companion 连带删（修孤儿 bug）");
        assert_eq!(is_deleted(&c, 3), 0, "无关项不动");
        assert_eq!(
            c.query_row(
                "SELECT source_revision FROM media_items WHERE id=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2,
            "可见性改变推进 source_revision"
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM dedup_index", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "软删除清除旧去重证据"
        );
    }

    #[test]
    fn restore_cascades_to_companion() {
        let c = mem_db();
        soft_delete_items(&c, &[1]).unwrap();
        restore_items(&c, &[1]).unwrap();
        assert_eq!(is_deleted(&c, 1), 0, "静图已恢复");
        assert_eq!(is_deleted(&c, 2), 0, "companion 对称连带恢复");
        assert_eq!(
            c.query_row(
                "SELECT source_revision FROM media_items WHERE id=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            3,
            "恢复再次推进 source_revision"
        );
    }
}

/// 普通图片批量源读取与固定候选页共用的最大项数。
pub const IMAGE_SOURCE_PAGE_SIZE: usize = 256;

/// 普通图片的源授权数据；文件名位于 `item.file_name`，调用方仍须打开授权句柄。
#[derive(Debug, Clone)]
pub struct ImageSource {
    pub item: MediaItem,
    pub root_path: String,
    pub relative_dir: String,
}

// catalog 接管早于任务播种也必须让路；已完成的 exotic 任务同样不进入普通图片管线。
pub(super) const IMAGE_SOURCE_ELIGIBILITY: &str = "m.media_type='image' AND m.is_deleted=0
    AND r.is_hidden=0
    AND NOT EXISTS (
        SELECT 1 FROM exotic_catalog_formats ec, json_each(ec.capabilities_json) capability
        WHERE ec.format=m.file_format AND capability.value='thumbnail')
    AND NOT EXISTS (
        SELECT 1 FROM exotic_tasks et WHERE et.item_id=m.id AND et.capability='thumbnail')";

/// 批量读取普通图片和源路径组成部分，供视口与后台页共用（最多 256 项）。
///
/// 返回按 ID 排序的存在且可处理的项；已删除、隐藏、非图片、exotic 或缺失 ID 不返回。
/// 不执行磁盘授权；调用方用根路径、相对目录与文件名打开授权句柄。
pub fn get_image_sources_batch(conn: &Connection, ids: &[i64]) -> Result<Vec<ImageSource>> {
    if ids.len() > IMAGE_SOURCE_PAGE_SIZE {
        return Err(AppError::Internal("image source batch exceeds 256".into()));
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT m.id, m.directory_id, m.file_name, m.file_size, m.file_mtime, m.file_format,
                m.media_type, m.width, m.height, m.duration_ms, m.sort_datetime, m.cache_key,
                m.thumb_status, m.thumb_path, m.thumbhash, m.is_favorited, m.is_deleted,
                m.deleted_at, m.rating, m.is_live_photo, m.has_embedded_video, m.companion_of,
                m.content_hash, m.created_at, m.updated_at, m.color_label, m.view_rotation,
                m.playback_position_ms, m.source_revision, r.path, d.rel_path
         FROM media_items m JOIN directories d ON d.id=m.directory_id
         JOIN scan_roots r ON r.id=d.root_id
         WHERE m.id IN ({placeholders}) AND {IMAGE_SOURCE_ELIGIBILITY} ORDER BY m.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        Ok(ImageSource {
            item: map_media_item(row)?,
            root_path: row.get(29)?,
            relative_dir: row.get(30)?,
        })
    })?;
    rows.map(|row| row.map_err(AppError::from)).collect()
}
