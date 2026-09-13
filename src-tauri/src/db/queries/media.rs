//! 媒体项域 DAO:读取/detail、批量与单项用户属性(收藏/评分/颜色)、companion、
//! 回收站、统计、路径信息与 cache_key、拖拽复制
//! (T 线拆分自 queries.rs,SQL 与行为不变;不吸收 layout SQL builder,§4.1)。

use rusqlite::{params, Connection, OptionalExtension, Row};

// 跨域定向引用(§4.3):选区分批口径由 selection owner(layout)定义。
use super::layout::SELECTION_BATCH_CHUNK;
use super::scan::EXCLUDE_HIDDEN_ROOTS;
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

/// 将一条 media_items 行复制到 `target_dir_id`（新自增 id），复用源的 cache_key / 缩略图 /
/// 尺寸，使副本即时显示。返回新 id。用于拖拽复制；精确的新 id 使复制可干净撤销（问题2）。
pub fn duplicate_media_item_into_dir(
    conn: &Connection,
    src_id: i64,
    target_dir_id: i64,
) -> Result<i64> {
    // 列清单取舍(2026-07-10 审查 A3):内容/用户属性(color_label 用户资产、content_identifier
    // Live Photo 配对键——字节级复制保留内容内在标识)必须随行复制;而 volume_id /
    // volume_relative_path / availability 是**位置属性**——副本物理落在目标目录,照抄源值会指向
    // 源位置(缺失检测误报/重链接错键),故有意不复制,留 NULL/DEFAULT 由下次扫描 COALESCE 治愈。
    // 2026-07-23 裁决 J11 补齐:view_rotation(V20 看图台展示旋转)与 playback_position_ms
    // (V23 播放进度记忆)同为用户属性,按 A3 原则随行——先前遗漏致副本「转回去」,与本注释矛盾。
    conn.execute(
        "INSERT INTO media_items
            (directory_id, file_name, file_size, file_mtime, file_format, media_type,
             width, height, duration_ms, sort_datetime, cache_key, thumb_status, thumb_path,
             thumbhash, is_favorited, is_deleted, deleted_at, rating, is_live_photo,
             has_embedded_video, companion_of, content_hash, color_label, content_identifier,
             view_rotation, playback_position_ms, created_at, updated_at)
         SELECT ?1, file_name, file_size, file_mtime, file_format, media_type,
             width, height, duration_ms, sort_datetime, cache_key, thumb_status, thumb_path,
             thumbhash, is_favorited, is_deleted, deleted_at, rating, is_live_photo,
             has_embedded_video, companion_of, content_hash, color_label, content_identifier,
             view_rotation, playback_position_ms, strftime('%s','now'), strftime('%s','now')
         FROM media_items WHERE id = ?2",
        params![target_dir_id, src_id],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod duplicate_media_item_tests {
    //! 2026-07-10 审查 A3 回归钉:复制须带走用户/内容属性,且**不得**带走位置属性。
    use super::*;

    #[test]
    // 8 元组仅为一次性读回断言,拆结构体反增噪——测试内豁免 type_complexity。
    #[allow(clippy::type_complexity)]
    fn copies_user_and_content_fields_but_resets_location_fields() {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, 'src', 's'), (11, 1, 'dst', 'd');
             INSERT INTO volumes (id, stable_id, label) VALUES (7, 'vol-guid-7', 'V');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, width, height, sort_datetime, cache_key,
                 color_label, content_identifier, volume_id, volume_relative_path, availability,
                 view_rotation, playback_position_ms)
             VALUES (1, 10, 'a.heic', 1, 1, 'heic', 'image', 10, 10, 100, 11,
                 'red', 'live-pair-1', 7, 'src/a.heic', 'offline', 90, 4321);",
        )
        .unwrap();

        let new_id = duplicate_media_item_into_dir(&c, 1, 11).unwrap();

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

/// GA-fix:queries 层落库归属——DAO 本身不 clamp(调用方职责,同 doc 注明),这里只锚定
/// 「传入什么值就原样落库」的存储契约。负值 clamp→0 的行为归属 IPC 层
/// (见 `media_commands::set_playback_position` 的 `ms.max(0)`),测试见该文件同名模块。
#[cfg(test)]
mod playback_position_tests {
    use super::*;

    #[test]
    fn set_playback_position_roundtrip_stores_value_as_given() {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'd');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, width, height, sort_datetime, cache_key)
             VALUES (1, 10, 'a.mp4', 1, 1, 'mp4', 'video', 10, 10, 100, 11);",
        )
        .unwrap();

        set_playback_position(&c, 1, 4200).unwrap();
        let stored: i64 = c
            .query_row(
                "SELECT playback_position_ms FROM media_items WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 4200, "正 ms 原样落库(DAO 不 clamp)");
    }
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

pub fn soft_delete_items(conn: &Connection, item_ids: &[i64]) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    // D5：连带其 Live Photo companion，避免删静图留下孤儿伴随视频。
    let ids = expand_companions(conn, item_ids)?;
    let tx = conn.unchecked_transaction()?;
    for &id in &ids {
        tx.execute(
            "UPDATE media_items SET is_deleted=1, source_revision=source_revision+1,
                     deleted_at=strftime('%s','now'),
                     updated_at=strftime('%s','now')
             WHERE id=?1",
            params![id],
        )?;
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
    for &id in &ids {
        tx.execute(
            "UPDATE media_items SET is_deleted=0, source_revision=source_revision+1,
                     deleted_at=NULL,
                     updated_at=strftime('%s','now')
             WHERE id=?1",
            params![id],
        )?;
        tx.execute("DELETE FROM dedup_index WHERE item_id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get_trash(conn: &Connection, offset: i64, limit: i64) -> Result<Vec<MediaItem>> {
    // 隐藏根排除(V21):回收站画廊实际走 ViewScope::Trash(layout,已排除),本函数当前无前端
    // 调用方——但 IPC `get_trash` 仍注册,口径对齐防将来接线时泄漏(与 get_app_stats 的
    // total_deleted 同口径:隐藏根的回收站项不可见)。
    let mut stmt = conn.prepare(&format!(
        "SELECT id, directory_id, file_name, file_size, file_mtime, file_format,
                media_type, width, height, duration_ms, sort_datetime, cache_key,
                thumb_status, thumb_path, thumbhash, is_favorited, is_deleted,
                deleted_at, rating, is_live_photo, has_embedded_video, companion_of,
                content_hash, created_at, updated_at, color_label, view_rotation,
                playback_position_ms, source_revision
         FROM media_items WHERE is_deleted=1 {EXCLUDE_HIDDEN_ROOTS}
         ORDER BY deleted_at DESC, id DESC
         LIMIT ?1 OFFSET ?2"
    ))?;
    let rows = stmt.query_map(params![limit, offset], map_media_item)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 回收站 keyset seek 翻页（取代 OFFSET：百万行 `OFFSET 1e6` 要扫过百万行，keyset 恒定 <5ms）。
/// `cursor` = 上一页**最后一项**的 `(deleted_at, id)`，首页传 `None`；复合序 `(deleted_at DESC, id DESC)`，
/// 走 `idx_media_trash`。SQLite 行值比较 `(a,b) < (c,d)` 原生支持。
/// 注：回收站项的 `deleted_at` 在软删时即写入（非空），故 keyset 比较不需 COALESCE（保索引可用）。
pub fn get_trash_keyset(
    conn: &Connection,
    cursor: Option<(i64, i64)>,
    limit: i64,
) -> Result<Vec<MediaItem>> {
    // has_cursor=0 时 OR 短路放行全部（首页）；=1 时按行值游标 seek 下一页。
    let (cur_da, cur_id, has_cursor): (i64, i64, i64) = match cursor {
        Some((da, id)) => (da, id, 1),
        None => (0, 0, 0),
    };
    // 隐藏根排除(V21):同 get_trash(死路径口径对齐)。
    let mut stmt = conn.prepare(&format!(
        "SELECT id, directory_id, file_name, file_size, file_mtime, file_format,
                media_type, width, height, duration_ms, sort_datetime, cache_key,
                thumb_status, thumb_path, thumbhash, is_favorited, is_deleted,
                deleted_at, rating, is_live_photo, has_embedded_video, companion_of,
                content_hash, created_at, updated_at, color_label, view_rotation,
                playback_position_ms, source_revision
         FROM media_items
         WHERE is_deleted = 1 {EXCLUDE_HIDDEN_ROOTS}
           AND (?1 = 0 OR (deleted_at, id) < (?2, ?3))
         ORDER BY deleted_at DESC, id DESC
         LIMIT ?4"
    ))?;
    let rows = stmt.query_map(params![has_cursor, cur_da, cur_id, limit], map_media_item)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

#[cfg(test)]
mod source_revision_mapping_tests {
    use super::*;

    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items
                 (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                  width, height, sort_datetime, cache_key, source_revision, is_deleted, deleted_at)
             VALUES
                 (1, 10, 'live.jpg', 1, 1, 'jpg', 'image', 1, 1, 100, 11, 7, 0, NULL),
                 (2, 10, 'trash.jpg', 1, 1, 'jpg', 'image', 1, 1, 200, 22, 8, 1, 500);",
        )
        .unwrap();
        c
    }

    #[test]
    fn media_and_trash_mappings_keep_source_revision() {
        let c = seeded();

        assert_eq!(get_media_item(&c, 1).unwrap().source_revision, 7);
        assert_eq!(get_trash(&c, 0, 10).unwrap()[0].source_revision, 8);
        assert_eq!(
            get_trash_keyset(&c, None, 10).unwrap()[0].source_revision,
            8
        );
    }
}

// ── 隐藏根排除(V21 回收站死路径)──────────────────────────────────────────────
// get_trash/get_trash_keyset 当前无前端调用方(回收站画廊走 ViewScope::Trash 经 layout
// 排除),但 IPC 仍注册——口径对齐防将来接线泄漏,并与 get_app_stats.total_deleted 一致。
#[cfg(test)]
mod hidden_root_trash_tests {
    use super::super::scan::set_scan_root_hidden;
    use super::*;

    fn two_roots_trashed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, '', 'r1'), (20, 2, '', 'r2');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, is_deleted, deleted_at) VALUES
                 (1, 10, 'a.jpg', 1, 1, 'jpg', 'image', 0, 0, 100, 0, 1, 500),
                 (2, 20, 'b.jpg', 1, 1, 'jpg', 'image', 0, 0, 200, 0, 1, 600);",
        )
        .unwrap();
        c
    }

    #[test]
    fn trash_listings_exclude_hidden_root() {
        let c = two_roots_trashed();
        let ids = |v: Vec<MediaItem>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(get_trash(&c, 0, 10).unwrap()), vec![2, 1]);
        assert_eq!(ids(get_trash_keyset(&c, None, 10).unwrap()), vec![2, 1]);

        set_scan_root_hidden(&c, 2, true).unwrap();
        assert_eq!(
            ids(get_trash(&c, 0, 10).unwrap()),
            vec![1],
            "OFFSET 版排隐藏根"
        );
        assert_eq!(
            ids(get_trash_keyset(&c, None, 10).unwrap()),
            vec![1],
            "keyset 版排隐藏根"
        );

        set_scan_root_hidden(&c, 2, false).unwrap();
        assert_eq!(
            ids(get_trash_keyset(&c, None, 10).unwrap()),
            vec![2, 1],
            "unhide 恢复"
        );
    }
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
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND media_type='image'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND media_type='video'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND media_type='audio'),
            COUNT(*) FILTER (WHERE is_deleted=0 AND companion_of IS NULL AND media_type='document'),
            COUNT(*) FILTER (WHERE is_favorited=1 AND is_deleted=0),
            COUNT(*) FILTER (WHERE is_deleted=1),
            COUNT(*) FILTER (WHERE is_live_photo=1 AND is_deleted=0 AND companion_of IS NULL)
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

#[cfg(test)]
mod trash_keyset_tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免构造 directory 链
        c
    }

    fn add_trashed(c: &Connection, id: i64, deleted_at: i64) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, is_deleted, deleted_at)
             VALUES (?1, 1, ?2, 0, 0, 'jpg', 'image', 0, 0, 0, 0, 1, ?3)",
            params![id, format!("{id}.jpg"), deleted_at],
        )
        .unwrap();
    }

    /// keyset 逐页拼接 == 全量 (deleted_at DESC, id DESC) 序，无重叠无遗漏；同 deleted_at 时 id 次键生效。
    #[test]
    fn keyset_pages_match_full_order() {
        let c = mem_db();
        add_trashed(&c, 1, 100);
        add_trashed(&c, 2, 100); // 与 id=1 同 deleted_at → 靠 id 次键定序
        add_trashed(&c, 3, 200);
        add_trashed(&c, 4, 50);
        // 期望 (deleted_at DESC, id DESC)：(200,3),(100,2),(100,1),(50,4) → [3,2,1,4]
        let expected = vec![3i64, 2, 1, 4];

        // 基线：一页取全。
        let all = get_trash_keyset(&c, None, 100).unwrap();
        assert_eq!(
            all.iter().map(|m| m.id).collect::<Vec<_>>(),
            expected,
            "全量序错"
        );

        // size=2 keyset 翻页。
        let mut got = Vec::new();
        let mut cursor: Option<(i64, i64)> = None;
        loop {
            let page = get_trash_keyset(&c, cursor, 2).unwrap();
            if page.is_empty() {
                break;
            }
            let last = page.last().unwrap();
            cursor = Some((last.deleted_at.unwrap_or(0), last.id));
            got.extend(page.iter().map(|m| m.id));
        }
        assert_eq!(got, expected, "keyset 逐页拼接应等于全量序，无重叠无遗漏");
    }
}

/// T18 S3：`expand_companions` + soft-delete/restore 的 Live Photo companion 连带（D5 孤儿 bug 修复）。
#[cfg(test)]
mod companion_expand_tests {
    use super::*;

    /// seed：id1 静图 + id2 其 companion(companion_of=1) + id3 独立项。FK OFF 免构造目录链。
    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
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
    fn expand_includes_companion() {
        let c = mem_db();
        let mut got = expand_companions(&c, &[1]).unwrap();
        got.sort();
        assert_eq!(got, vec![1, 2], "静图展开应含其 companion");
    }

    #[test]
    fn expand_standalone_and_empty() {
        let c = mem_db();
        assert_eq!(expand_companions(&c, &[3]).unwrap(), vec![3], "独立项不变");
        assert_eq!(
            expand_companions(&c, &[]).unwrap(),
            Vec::<i64>::new(),
            "空入参空出"
        );
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

#[cfg(test)]
mod r2_6_query_tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // root1 → A(顶层) → A/B(子);C(顶层,无子)。
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES
                 (10, 1, NULL, 'A', 'A', 0),
                 (11, 1, 10, 'A/B', 'B', 1),
                 (12, 1, NULL, 'C', 'C', 0);",
        )
        .unwrap();
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn add_item(
        c: &Connection,
        id: i64,
        dir: i64,
        mtype: &str,
        fav: i64,
        del: i64,
        live: i64,
        companion: Option<i64>,
    ) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                 width, height, sort_datetime, cache_key, is_favorited, is_deleted,
                 is_live_photo, companion_of)
             VALUES (?1, ?2, ?3, 0, 0, 'jpg', ?4, 0, 0, 0, 0, ?5, ?6, ?7, ?8)",
            params![
                id,
                dir,
                format!("{id}.jpg"),
                mtype,
                fav,
                del,
                live,
                companion
            ],
        )
        .unwrap();
    }
    /// stats 合一:三处口径差异逐字锁定(favorited 不排 companion、deleted 不加过滤、
    /// 其余排 companion+软删)。
    #[test]
    fn app_stats_buckets_keep_original_semantics() {
        let c = mem_db();
        add_item(&c, 1, 10, "image", 0, 0, 0, None);
        add_item(&c, 2, 10, "video", 0, 0, 0, None);
        add_item(&c, 3, 10, "audio", 0, 0, 0, None);
        add_item(&c, 4, 10, "document", 0, 0, 0, None);
        add_item(&c, 5, 10, "image", 1, 0, 0, None); // favorited 正常项
        add_item(&c, 6, 10, "video", 1, 0, 0, Some(5)); // favorited 伴随 → 仍计入 favorited
        add_item(&c, 7, 10, "image", 0, 1, 0, None); // 软删
        add_item(&c, 8, 10, "image", 0, 0, 1, None); // live photo

        let s = get_app_stats(&c).unwrap();
        assert_eq!(s.total_items, 6, "排伴随(6)与软删(7)");
        assert_eq!(s.total_images, 3); // 1,5,8
        assert_eq!(s.total_videos, 1); // 2(6 是伴随)
        assert_eq!(s.total_audios, 1);
        assert_eq!(s.total_documents, 1);
        assert_eq!(s.total_favorited, 2, "favorited 口径不排 companion");
        assert_eq!(s.total_deleted, 1);
        assert_eq!(s.total_live_photos, 1);
    }
}
