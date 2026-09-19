//! 元数据域 DAO:image/video/audio enrichment 写读、尺寸回填、待 enrichment 查询、
//! 占位尺寸项取路径(T 线拆分自 queries.rs,SQL 与行为不变)。

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::models::{AudioMeta, ImageMeta};
use crate::error::{AppError, Result};
use crate::utils::path::resolve_media_path;

/// 一个扫描根目录当前待解析元数据的工作量快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetadataWorkload {
    pub files: i64,
    pub bytes: i64,
}

/// 统计 root 下图片、视频、音频待解析候选的文件数和逻辑文件体积。
///
/// 该口径与三条富化队列的 LEFT JOIN 条件保持一致；已存在对应 meta 行的项目不会重复计入。
pub fn get_metadata_workload(conn: &Connection, root_id: i64) -> Result<MetadataWorkload> {
    get_metadata_workload_with_video_retry(conn, root_id, false)
}

/// 统计待解析工作量；`retry_video_minimal=true` 时，把之前探测失败后写入最小行的视频重新
/// 纳入队列，供当前已就绪的替代后端进行一次回退重试。
pub fn get_metadata_workload_with_video_retry(
    conn: &Connection,
    root_id: i64,
    retry_video_minimal: bool,
) -> Result<MetadataWorkload> {
    let video_predicate = if retry_video_minimal {
        "(m.media_type='video' AND (vm.item_id IS NULL OR
          (vm.video_codec IS NULL AND vm.fps IS NULL AND vm.bitrate IS NULL AND
           vm.rotation=0 AND vm.has_audio=0)))"
    } else {
        "(m.media_type='video' AND vm.item_id IS NULL)"
    };
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(m.file_size), 0)
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         LEFT JOIN image_meta im ON im.item_id = m.id
         LEFT JOIN video_meta vm ON vm.item_id = m.id
         LEFT JOIN audio_meta am ON am.item_id = m.id
         WHERE d.root_id=?1 AND m.is_deleted=0 AND (
             (m.media_type='image' AND im.item_id IS NULL) OR
             {video_predicate} OR
             (m.media_type='audio' AND am.item_id IS NULL)
         )"
    );
    let (files, bytes) = conn.query_row(&sql, params![root_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    Ok(MetadataWorkload { files, bytes })
}

// ── 图像元数据更新插入 ────────────────────────────────────────────────────────

pub fn upsert_image_meta(conn: &Connection, meta: &ImageMeta) -> Result<()> {
    // enrichment 热路径:500 项/批,复用连接级缓存语句,避免逐行 SQL 编译。
    conn.prepare_cached(
        "INSERT INTO image_meta
             (item_id, orientation, exif_datetime, exif_make, exif_model, exif_lens,
              exif_focal_length, exif_aperture, exif_shutter, exif_iso,
              exif_gps_lat, exif_gps_lng)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(item_id) DO UPDATE SET
             orientation=excluded.orientation,
             exif_datetime=excluded.exif_datetime,
             exif_make=excluded.exif_make,
             exif_model=excluded.exif_model,
             exif_lens=excluded.exif_lens,
             exif_focal_length=excluded.exif_focal_length,
             exif_aperture=excluded.exif_aperture,
             exif_shutter=excluded.exif_shutter,
             exif_iso=excluded.exif_iso,
             exif_gps_lat=excluded.exif_gps_lat,
             exif_gps_lng=excluded.exif_gps_lng",
    )?
    .execute(params![
        meta.item_id,
        meta.orientation,
        meta.exif_datetime,
        meta.exif_make,
        meta.exif_model,
        meta.exif_lens,
        meta.exif_focal_length,
        meta.exif_aperture,
        meta.exif_shutter,
        meta.exif_iso,
        meta.exif_gps_lat,
        meta.exif_gps_lng
    ])?;
    Ok(())
}

pub fn update_sort_datetime(conn: &Connection, item_id: i64, dt: i64) -> Result<()> {
    conn.prepare_cached(
        "UPDATE media_items SET sort_datetime=?1, updated_at=strftime('%s','now') WHERE id=?2",
    )?
    .execute(params![dt, item_id])?;
    Ok(())
}

/// 为快速扫描时以 0×0 占位插入的项补全真实像素尺寸。以 `width=0 OR height=0`
/// 作为条件守卫，绝不重写（也绝不双重翻转）已在即时路径得到真实、方向校正尺寸的项。
pub fn update_media_dimensions(
    conn: &Connection,
    item_id: i64,
    width: i64,
    height: i64,
) -> Result<()> {
    conn.prepare_cached(
        "UPDATE media_items SET width=?1, height=?2, updated_at=strftime('%s','now')
         WHERE id=?3 AND (width=0 OR height=0)",
    )?
    .execute(params![width, height, item_id])?;
    Ok(())
}

// ── 视频元数据（§2.1 / §3.2）──────────────────────────────────────────────────

/// 用 MF 探测得到的**显示**尺寸（已应用旋转）+ 时长覆盖视频的占位（16:9）尺寸。与
/// `update_media_dimensions` 不同，这里无 `0×0` 守卫：视频以 16:9 占位（非 0×0）入库，
/// 故探测后总是回填真实比例 —— 布局强依赖之（§3.2）。
pub fn update_video_dimensions(
    conn: &Connection,
    item_id: i64,
    width: i64,
    height: i64,
    duration_ms: Option<i64>,
) -> Result<()> {
    conn.prepare_cached(
        "UPDATE media_items SET width=?1, height=?2, duration_ms=?3, updated_at=strftime('%s','now')
         WHERE id=?4 AND media_type='video'",
    )?
    .execute(params![width, height, duration_ms, item_id])?;
    Ok(())
}

/// 更新插入一行 `video_meta`（编解码/帧率/比特率/旋转/是否含音频）。`cover_time_ms` 不动
/// （如需由封面派生路径后续设置）。
pub fn upsert_video_meta(
    conn: &Connection,
    item_id: i64,
    codec: Option<&str>,
    fps: Option<f64>,
    bitrate: Option<i64>,
    rotation: i64,
    has_audio: bool,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO video_meta (item_id, video_codec, fps, bitrate, rotation, has_audio)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(item_id) DO UPDATE SET
            video_codec = excluded.video_codec,
            fps         = excluded.fps,
            bitrate     = excluded.bitrate,
            rotation    = excluded.rotation,
            has_audio   = excluded.has_audio",
    )?
    .execute(params![
        item_id,
        codec,
        fps,
        bitrate,
        rotation,
        has_audio as i64
    ])?;
    Ok(())
}

/// `root_id` 下仍缺 `video_meta` 行的视频 —— MF 探测补全（宽高/旋转/时长）的工作队列。
/// 返回 `(id, abs_path, file_format)`，恒按 `m.id` 升序（keyset 友好）。
///
/// `after_id` 是上一批的最大 id（首批发 `i64::MIN`）。查询消费 `idx_media_type_id`
/// 的 `(media_type, id)` seek，避免每批从头跳过已探测前缀（原实现无 ORDER BY 的
/// `LIMIT ?2` 在批循环下近 O(N²/500) 的 rowid 前缀重扫）。
pub fn get_videos_needing_meta(
    conn: &Connection,
    root_id: i64,
    limit: i64,
    after_id: i64,
) -> Result<Vec<(i64, String, String, i64)>> {
    get_videos_needing_meta_with_retry(conn, root_id, limit, after_id, false)
}

/// 读取视频元数据工作队列；启用 `retry_minimal` 时纳入先前失败后留下的最小 `video_meta` 行。
pub fn get_videos_needing_meta_with_retry(
    conn: &Connection,
    root_id: i64,
    limit: i64,
    after_id: i64,
    retry_minimal: bool,
) -> Result<Vec<(i64, String, String, i64)>> {
    let video_meta_predicate = if retry_minimal {
        "(vm.item_id IS NULL OR
          (vm.video_codec IS NULL AND vm.fps IS NULL AND vm.bitrate IS NULL AND
           vm.rotation=0 AND vm.has_audio=0))"
    } else {
        "vm.item_id IS NULL"
    };
    let sql = format!(
        "SELECT m.id,
                CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                     ELSE r.path || '/' || d.rel_path || '/' || m.file_name
                END,
                m.file_format,
                m.file_size
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         JOIN scan_roots r ON r.id = d.root_id
         LEFT JOIN video_meta vm ON vm.item_id = m.id
         WHERE d.root_id = ?1 AND m.is_deleted = 0 AND m.media_type = 'video'
           AND {video_meta_predicate} AND m.id > ?3
         ORDER BY m.id ASC
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id, limit, after_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

// ── 音频元数据（§3.6）──────────────────────────────────────────────────────────

/// `root_id` 下仍缺 `audio_meta` 行的音频 —— lofty 标签/歌词补全的工作队列。
/// 返回 `(id, abs_path, file_format)`，恒按 `m.id` 升序（keyset 友好）。
///
/// `after_id` 是上一批的最大 id（首批发 `i64::MIN`）。与视频队列同型：
/// 消费 `idx_media_type_id` 的 seek，消除批循环下的前缀重扫。
pub fn get_audios_needing_meta(
    conn: &Connection,
    root_id: i64,
    limit: i64,
    after_id: i64,
) -> Result<Vec<(i64, String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT m.id,
                CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                     ELSE r.path || '/' || d.rel_path || '/' || m.file_name
                END,
                m.file_format,
                m.file_size
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         JOIN scan_roots r ON r.id = d.root_id
         LEFT JOIN audio_meta am ON am.item_id = m.id
         WHERE d.root_id = ?1 AND m.is_deleted = 0 AND m.media_type = 'audio'
           AND am.item_id IS NULL AND m.id > ?3
         ORDER BY m.id ASC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![root_id, limit, after_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 更新插入一行 `audio_meta`（编解码/艺术家/专辑/标题/音轨/年份/流派 + 歌词来源）。
#[allow(clippy::too_many_arguments)]
pub fn upsert_audio_meta(
    conn: &Connection,
    item_id: i64,
    codec: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
    title: Option<&str>,
    track_no: Option<i64>,
    year: Option<i64>,
    genre: Option<&str>,
    lyrics_source: Option<&str>,
    lyrics_path: Option<&str>,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO audio_meta
             (item_id, audio_codec, artist, album_title, track_title,
              track_no, year, genre, lyrics_source, lyrics_path)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(item_id) DO UPDATE SET
             audio_codec   = excluded.audio_codec,
             artist        = excluded.artist,
             album_title   = excluded.album_title,
             track_title   = excluded.track_title,
             track_no      = excluded.track_no,
             year          = excluded.year,
             genre         = excluded.genre,
             lyrics_source = excluded.lyrics_source,
             lyrics_path   = excluded.lyrics_path",
    )?
    .execute(params![
        item_id,
        codec,
        artist,
        album,
        title,
        track_no,
        year,
        genre,
        lyrics_source,
        lyrics_path
    ])?;
    Ok(())
}

/// 读取某项的 `audio_meta` 行（持久化的标签子集），若存在（§3.6）。
pub fn get_audio_meta(conn: &Connection, item_id: i64) -> Result<Option<AudioMeta>> {
    conn.query_row(
        "SELECT item_id, audio_codec, artist, album_title, track_title,
                track_no, year, genre, lyrics_source, lyrics_path
         FROM audio_meta WHERE item_id = ?1",
        params![item_id],
        |row| {
            Ok(AudioMeta {
                item_id: row.get(0)?,
                audio_codec: row.get(1)?,
                artist: row.get(2)?,
                album_title: row.get(3)?,
                track_title: row.get(4)?,
                track_no: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                lyrics_source: row.get(8)?,
                lyrics_path: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

/// 仍为占位(0×0)尺寸的项的绝对路径+扩展名；若不存在或已测量则为 `None`。
/// 支撑可视窗口优先取尺寸。
pub fn get_placeholder_item_path(conn: &Connection, id: i64) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT r.path, d.rel_path, m.file_name, m.file_format
         FROM media_items m
         JOIN directories d ON d.id = m.directory_id
         JOIN scan_roots  r ON r.id = d.root_id
         WHERE m.id = ?1 AND (m.width = 0 OR m.height = 0)",
        params![id],
        |row| {
            let root: String = row.get(0)?;
            let rel: String = row.get(1)?;
            let name: String = row.get(2)?;
            let ext: String = row.get(3)?;
            Ok((resolve_media_path(&root, &rel, &name), ext))
        },
    )
    .optional()
    .map_err(AppError::from)
}

// ── 丰富化辅助函数 ────────────────────────────────────────────────────────

/// 需要丰富化的项：没有 `image_meta` 行且 media_type='image' 的项。
pub fn get_unenriched_image_ids(conn: &Connection, limit: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT m.id FROM media_items m
         LEFT JOIN image_meta im ON im.item_id = m.id
         WHERE m.is_deleted=0 AND m.media_type='image' AND im.item_id IS NULL
         ORDER BY m.created_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| row.get(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');",
        )
        .unwrap();
        c
    }

    fn insert_media(c: &Connection, id: i64, media_type: &str) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 10, ?2, 0, 0, ?3, ?4, 0, 0, 0, 0)",
            params![id, format!("{id}.bin"), "mp4", media_type],
        )
        .unwrap();
    }

    #[test]
    fn video_and_audio_queues_page_by_id_keyset() {
        let c = mem_db();
        for id in [1, 2, 3, 4, 5] {
            insert_media(&c, id, "video");
        }
        for id in [10, 11, 12] {
            insert_media(&c, id, "audio");
        }

        let v1 = get_videos_needing_meta(&c, 1, 2, i64::MIN).unwrap();
        assert_eq!(
            v1.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let v2 = get_videos_needing_meta(&c, 1, 2, v1[1].0).unwrap();
        assert_eq!(
            v2.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![3, 4]
        );
        let v3 = get_videos_needing_meta(&c, 1, 2, v2[1].0).unwrap();
        assert_eq!(
            v3.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![5]
        );
        assert!(get_videos_needing_meta(&c, 1, 2, v3[0].0)
            .unwrap()
            .is_empty());

        let a1 = get_audios_needing_meta(&c, 1, 2, i64::MIN).unwrap();
        assert_eq!(
            a1.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![10, 11]
        );
        let a2 = get_audios_needing_meta(&c, 1, 2, a1[1].0).unwrap();
        assert_eq!(
            a2.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![12]
        );
    }

    #[test]
    fn processed_meta_rows_are_skipped_by_keyset_queues() {
        let c = mem_db();
        insert_media(&c, 1, "video");
        insert_media(&c, 2, "video");
        insert_media(&c, 3, "video");
        // 2 已探测过 → 队列只应返回 1 和 3(keyset 与 LEFT JOIN 过滤叠加)。
        c.execute("INSERT INTO video_meta (item_id) VALUES (2)", [])
            .unwrap();

        let batch = get_videos_needing_meta(&c, 1, 10, i64::MIN).unwrap();
        assert_eq!(
            batch.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn minimal_video_meta_rows_can_be_retried_when_fallback_is_ready() {
        let c = mem_db();
        insert_media(&c, 1, "video");
        insert_media(&c, 2, "video");
        c.execute("UPDATE media_items SET file_size=7 WHERE id=1", [])
            .unwrap();
        c.execute("INSERT INTO video_meta (item_id) VALUES (1)", [])
            .unwrap();
        c.execute(
            "INSERT INTO video_meta (item_id, video_codec, fps, bitrate, rotation, has_audio)
             VALUES (2, 'H264', 30.0, 1000, 0, 1)",
            [],
        )
        .unwrap();

        assert!(get_videos_needing_meta(&c, 1, 10, i64::MIN)
            .unwrap()
            .is_empty());
        let retry = get_videos_needing_meta_with_retry(&c, 1, 10, i64::MIN, true).unwrap();
        assert_eq!(
            retry.iter().map(|(id, _, _, _)| *id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            get_metadata_workload_with_video_retry(&c, 1, true).unwrap(),
            MetadataWorkload { files: 1, bytes: 7 }
        );
    }

    #[test]
    fn metadata_workload_sums_only_unprocessed_media() {
        let c = mem_db();
        insert_media(&c, 1, "image");
        insert_media(&c, 2, "video");
        insert_media(&c, 3, "audio");
        insert_media(&c, 4, "image");
        c.execute(
            "UPDATE media_items SET file_size=CASE id WHEN 1 THEN 10 WHEN 2 THEN 20 WHEN 3 THEN 30 WHEN 4 THEN 40 END",
            [],
        )
        .unwrap();
        c.execute("INSERT INTO image_meta (item_id) VALUES (4)", [])
            .unwrap();

        assert_eq!(
            get_metadata_workload(&c, 1).unwrap(),
            MetadataWorkload {
                files: 3,
                bytes: 60
            }
        );
    }
}
