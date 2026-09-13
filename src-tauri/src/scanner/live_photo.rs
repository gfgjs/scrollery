// src-tauri/src/scanner/live_photo.rs
//! 实况照片 / 动态照片关联配对。
//!
//! 在快速扫描插入所有项目后，此模块将静图与其
//! 关联的视频文件配对（相同目录，相同文件主名）。两类：
//! - Apple 实况照片：JPEG/HEIC/HEIF + `.MOV` —— 纯 stem 配对（T7）。
//! - 分体式动态照片：JPG + `.MP4` —— stem 配对 **但带 motion 守门**（T15），
//!   仅当静图侧已带 XMP 动态照片信号（`has_embedded_video=1`，由
//!   enrichment 阶段的 `detect_motion_photo_xmp` 置位）时才配，
//!   避免把普通照片 + 恰好同名的普通 `.mp4` 误吞为 companion。

use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use tracing::{debug, info, warn};

use crate::error::Result;

/// 用于配对的媒体项目的轻量级记录。
#[derive(Debug)]
struct PairingRecord {
    id: i64,
    file_stem: String,
    directory_id: i64,
    extension: String,
    /// 静图侧的 motion 信号（`has_embedded_video`）。仅 mp4 分体式配对据此守门；
    /// 视频行恒为 false。
    is_motion: bool,
}

/// 每个 (目录, stem) 的聚合：静图 + 各候选视频。
#[derive(Default)]
struct Group {
    /// 静图（jpg/jpeg/heic/heif）id。
    image_id: Option<i64>,
    /// 同一目录、同一 stem 出现多个静图时，禁止猜测主文件。
    ambiguous_image_stem: bool,
    /// 静图是否带 XMP motion 信号（决定 mp4 是否可配）。
    image_is_motion: bool,
    /// Apple Live Photo 候选视频（.mov）。
    mov_id: Option<i64>,
    /// 分体式 Motion Photo 候选视频（.mp4）。
    mp4_id: Option<i64>,
}

/// 运行特定扫描根目录的 Apple 实况照片关联配对。
///
/// Algorithm:
/// 算法：
/// 1. 查询根目录中所有 JPEG 图像或 MOV 视频的项目。
/// 2. 按 `(directory_id, file_stem)` 分组。
/// 3. 如果一个组同时包含 JPEG 和 MOV，则 MOV 是关联文件。
///    - JPEG: `is_live_photo = 1`
///    - MOV:  `companion_of = JPEG.id`
/// 4. 对账已有关系；关系变化时推进主项 `source_revision` 并清除其去重旁路。
pub fn pair_live_photos(conn: &Connection, root_id: i64) -> Result<u64> {
    info!("Pairing Live Photos for root_id={root_id} | 正在配对实况照片 root_id={root_id}");

    // 配对、解除配对、代次推进和旁路清理必须是一个短事务：去重任务不能看到
    // companion 已换绑但主项仍带着旧 logical unit digest 的中间态。
    let tx = conn.unchecked_transaction()?;

    // 获取候选项目（静图 JPEG/HEIC/HEIF，或 MOV/MP4 视频）。这里故意包含已有
    // companion 行，才能把旧关系与当前文件名/目录重新对账；只查 NULL 会永远漏掉解除配对。
    // HEIC/HEIF：现代 iPhone 默认拍 HEIC，其 Live Photo 为 HEIC + MOV（T7）。
    // mp4：分体式 Motion Photo 候选，仅在静图带 has_embedded_video 时才配（T15）。
    let records: Vec<PairingRecord> = {
        let mut stmt = tx.prepare(
            "SELECT m.id, m.file_name, m.directory_id, m.file_format, m.has_embedded_video
             FROM media_items m
             JOIN directories d ON d.id = m.directory_id
             WHERE d.root_id = ?1
               AND m.is_deleted = 0
               AND m.file_format IN ('jpg','jpeg','heic','heif','mov','mp4')
             ORDER BY m.directory_id, m.file_name",
        )?;

        let records = stmt
            .query_map(rusqlite::params![root_id], |row| {
                let file_name: String = row.get(1)?;
                let ext: String = row.get(3)?;
                // 派生文件主名：删除扩展名部分
                let stem = file_name
                    .rsplit_once('.')
                    .map(|(s, _)| s.to_lowercase())
                    .unwrap_or_else(|| file_name.to_lowercase());
                Ok(PairingRecord {
                    id: row.get(0)?,
                    file_stem: stem,
                    directory_id: row.get(2)?,
                    extension: ext,
                    is_motion: row.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        records
    };

    // 关系可能因为文件移动或另一主项换绑而跨 root 残留；只要关系的任一端属于本次
    // root，就纳入对账。软删除不视为解除关系，否则恢复操作会丢失 companion 连带关系。
    // 这样旧主项也会被失效，而不是只修复新主项。
    let current_relations: Vec<(i64, i64)> = {
        let mut stmt = tx.prepare(
            "SELECT c.id, c.companion_of
               FROM media_items c
               JOIN media_items p ON p.id = c.companion_of
               JOIN directories cd ON cd.id = c.directory_id
              JOIN directories pd ON pd.id = p.directory_id
              WHERE c.companion_of IS NOT NULL
                AND c.is_deleted = 0
                AND p.is_deleted = 0
                AND (cd.root_id = ?1 OR pd.root_id = ?1)",
        )?;
        let relations = stmt
            .query_map(rusqlite::params![root_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        relations
    };

    // Group by (directory_id, file_stem)
    // 按 (directory_id, file_stem) 分组
    let mut groups: HashMap<(i64, String), Group> = HashMap::new();

    for rec in &records {
        let g = groups
            .entry((rec.directory_id, rec.file_stem.clone()))
            .or_default();
        match rec.extension.as_str() {
            // 静图侧：JPEG 或 HEIC/HEIF（HEIC Live Photo，T7）。记录其 motion 信号供 mp4 守门。
            "jpg" | "jpeg" | "heic" | "heif" => {
                if g.image_id.is_some() {
                    // 同 stem 的多个静图无法可靠判断哪一张才是 Live/Motion Photo
                    // 主文件。保留首项只为维持结构完整，真正计算 desired 关系时整组拒绝。
                    g.ambiguous_image_stem = true;
                } else {
                    g.image_id = Some(rec.id);
                    g.image_is_motion = rec.is_motion;
                }
            }
            "mov" => g.mov_id = Some(rec.id),
            "mp4" => g.mp4_id = Some(rec.id),
            _ => {}
        }
    }

    // 当前关系按 companion 与主项分别建索引，关系迁移时可同时失效旧、新两个主项。
    let mut current_parent_by_companion = HashMap::with_capacity(current_relations.len());
    let mut current_companions_by_parent: HashMap<i64, HashSet<i64>> = HashMap::new();
    for (companion, parent) in current_relations {
        current_parent_by_companion.insert(companion, parent);
        current_companions_by_parent
            .entry(parent)
            .or_default()
            .insert(companion);
    }

    // 计算本次扫描认可的关系。只把有 companion 的主项放入 map，避免把“没有关系”
    // 当成一次变化；这样重复扫描保持幂等，不会反复推进 source_revision。
    let mut desired_parent_by_companion = HashMap::new();
    let mut desired_companions_by_parent: HashMap<i64, HashSet<i64>> = HashMap::new();
    for ((directory_id, file_stem), g) in groups {
        if g.ambiguous_image_stem {
            // 歧义组不自动配对，也不让排序结果（“后者覆盖前者”）成为隐式裁决。
            // 若之前已经存在关系，desired 集合为空，下面的短事务会把它当作失配拆除。
            warn!(
                directory_id,
                file_stem = %file_stem,
                "Ambiguous Live/Motion Photo stem: multiple still images; pairing skipped"
            );
            continue;
        }
        let Some(image) = g.image_id else { continue };

        // 收集本组要标为 companion 的视频：
        // - MOV：Apple Live Photo，纯 stem 配对（沿用 T7，不守门——mov 几乎是 iPhone 专属容器）。
        // - MP4：分体式 Motion Photo，**仅当静图带 has_embedded_video 时**才配（T15 守门），
        //   否则普通照片 + 同名普通 mp4 会被误吞。
        let mp4_companion = if g.image_is_motion { g.mp4_id } else { None };
        for vid in [g.mov_id, mp4_companion].into_iter().flatten() {
            desired_parent_by_companion.insert(vid, image);
            desired_companions_by_parent
                .entry(image)
                .or_default()
                .insert(vid);
        }
    }

    // 关系发生变化的所有主项：包括被解除关系的旧主项、接收换绑 companion 的新主项，
    // 以及仅因候选条件变化而失配的主项。HashSet 保证一次调用最多推进一次代次。
    let mut affected_parents = HashSet::new();
    for parent in current_companions_by_parent
        .keys()
        .chain(desired_companions_by_parent.keys())
        .copied()
    {
        let empty = HashSet::new();
        if current_companions_by_parent.get(&parent).unwrap_or(&empty)
            != desired_companions_by_parent.get(&parent).unwrap_or(&empty)
        {
            affected_parents.insert(parent);
        }
    }

    // 先拆掉失配旧关系。被换绑到新主项的 companion 也会先经过这里，再在下一段挂到
    // 新主项，整个过程仍处于同一短事务内。
    for (companion, old_parent) in &current_parent_by_companion {
        if desired_parent_by_companion.get(companion) != Some(old_parent) {
            tx.execute(
                "UPDATE media_items
                    SET companion_of=NULL, updated_at=strftime('%s','now')
                  WHERE id=?1 AND companion_of=?2",
                rusqlite::params![companion, old_parent],
            )?;
            debug!(
                "Unpaired stale LIVE/MOTION relation: companion_id={companion}, old_image_id={old_parent}"
            );
        }
    }

    let mut paired = 0u64;
    for (companion, parent) in &desired_parent_by_companion {
        if current_parent_by_companion.get(companion) == Some(parent) {
            continue;
        }

        // 将视频标记为关联文件（它将从网格中隐藏）。
        tx.execute(
            "UPDATE media_items
                SET companion_of=?1, updated_at=strftime('%s','now')
              WHERE id=?2 AND (companion_of IS NULL OR companion_of<>?1)",
            rusqlite::params![parent, companion],
        )?;
        debug!("Paired LIVE/MOTION: image_id={parent}, companion_video_id={companion}");
        paired += 1;
    }

    for parent in affected_parents {
        let has_companion = desired_companions_by_parent.contains_key(&parent);

        // source_revision 是逻辑单元代次，不是单纯主文件代次；companion 集合变化时
        // 必须推进它。has_embedded_video 代表主文件自身的动态照片信号，解除外部配对时
        // 保留该内嵌语义；外部配对则始终把 is_live_photo 置 1。
        tx.execute(
            "UPDATE media_items
                SET source_revision=source_revision+1,
                    is_live_photo=CASE
                        WHEN ?2 <> 0 THEN 1
                        WHEN has_embedded_video <> 0 THEN is_live_photo
                        ELSE 0
                    END,
                    updated_at=strftime('%s','now')
              WHERE id=?1",
            rusqlite::params![parent, has_companion as i64],
        )?;

        // 旧 unit_digest 可能仍与主文件 exact digest 相同，但已不再代表当前逻辑单元；
        // 缺行让后续去重任务重新计算，而不是复用陈旧组合摘要。
        tx.execute(
            "DELETE FROM dedup_index WHERE item_id=?1",
            rusqlite::params![parent],
        )?;
    }

    tx.commit()?;

    info!("Live/Motion Photo pairing complete: {paired} pairs found | 实况/动态照片配对完成：发现 {paired} 对");
    Ok(paired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, OptionalExtension};

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

    fn add(c: &Connection, id: i64, name: &str, fmt: &str, media_type: &str) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 10, ?2, 0, 0, ?3, ?4, 0, 0, 0, 0)",
            params![id, name, fmt, media_type],
        )
        .unwrap();
    }

    /// 插入一张已带 motion 信号的静图（模拟 enrichment 已对 XMP `GCamera:MotionPhoto=1`
    /// 置位 has_embedded_video=1），供分体式 Motion Photo 配对测试。
    fn add_motion_image(c: &Connection, id: i64, name: &str, fmt: &str) {
        add(c, id, name, fmt, "image");
        c.execute(
            "UPDATE media_items SET has_embedded_video=1 WHERE id=?1",
            params![id],
        )
        .unwrap();
    }

    fn is_live(c: &Connection, id: i64) -> bool {
        c.query_row(
            "SELECT is_live_photo FROM media_items WHERE id=?1",
            params![id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            != 0
    }
    fn companion_of(c: &Connection, id: i64) -> Option<i64> {
        c.query_row(
            "SELECT companion_of FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn source_revision(c: &Connection, id: i64) -> i64 {
        c.query_row(
            "SELECT source_revision FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn unit_digest(c: &Connection, id: i64) -> Option<Vec<u8>> {
        c.query_row(
            "SELECT unit_digest FROM dedup_index WHERE item_id=?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
    }

    fn add_unit_digest(c: &Connection, id: i64, digest: &[u8]) {
        c.execute(
            "INSERT INTO dedup_index
                (item_id, source_revision, hash_version, unit_digest, unit_size, status, checked_at)
             VALUES (?1, ?2, 1, ?3, 1, 'done', 1)",
            params![id, source_revision(c, id), digest],
        )
        .unwrap();
    }

    /// HEIC + 同名 MOV → 配对（HEIC 标 live、MOV 标 companion）。这是现代 iPhone 的 Live Photo（T7）。
    #[test]
    fn heic_mov_pairs() {
        let c = mem_db();
        add(&c, 1, "IMG_1.heic", "heic", "image");
        add(&c, 2, "IMG_1.mov", "mov", "video");
        let n = pair_live_photos(&c, 1).unwrap();
        assert_eq!(n, 1);
        assert!(is_live(&c, 1), "HEIC 应标 is_live_photo");
        assert_eq!(companion_of(&c, 2), Some(1), "MOV 应标 companion_of=HEIC");
    }

    /// HEIF 扩展名同样配对。
    #[test]
    fn heif_mov_pairs() {
        let c = mem_db();
        add(&c, 1, "IMG_2.heif", "heif", "image");
        add(&c, 2, "IMG_2.mov", "mov", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        assert!(is_live(&c, 1));
    }

    /// 回归：JPEG + MOV 仍配对（不因加 HEIC 而退化）。
    #[test]
    fn jpeg_mov_still_pairs() {
        let c = mem_db();
        add(&c, 1, "IMG_3.jpg", "jpg", "image");
        add(&c, 2, "IMG_3.mov", "mov", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        assert!(is_live(&c, 1));
        assert_eq!(companion_of(&c, 2), Some(1));
    }

    /// 同目录同 stem 的多张静图不能静默由排序靠后的项目覆盖前者；整组必须拒绝配对。
    #[test]
    fn ambiguous_still_image_stem_does_not_pair() {
        let c = mem_db();
        add(&c, 1, "IMG_AMBIGUOUS.jpg", "jpg", "image");
        add(&c, 2, "IMG_AMBIGUOUS.jpeg", "jpeg", "image");
        add(&c, 3, "IMG_AMBIGUOUS.mov", "mov", "video");

        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert!(!is_live(&c, 1));
        assert!(!is_live(&c, 2));
        assert_eq!(companion_of(&c, 3), None);
    }

    /// HEIC 无同名 MOV → 不配对（不误标 live）。
    #[test]
    fn heic_without_mov_not_paired() {
        let c = mem_db();
        add(&c, 1, "IMG_4.heic", "heic", "image");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert!(!is_live(&c, 1), "无 MOV 伴随的 HEIC 不应标 live");
    }

    /// T15：分体式 Motion Photo —— JPG（带 motion 信号）+ 同名 MP4 → 配对。
    #[test]
    fn motion_jpg_mp4_pairs() {
        let c = mem_db();
        add_motion_image(&c, 1, "PXL_5.jpg", "jpg");
        add(&c, 2, "PXL_5.mp4", "mp4", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        assert!(is_live(&c, 1), "motion JPG 应标 is_live_photo");
        assert_eq!(
            companion_of(&c, 2),
            Some(1),
            "同名 MP4 应标 companion_of=JPG"
        );
    }

    /// T15 守门核心：普通 JPG（无 motion 信号）+ 同名 MP4 → **不配对**，
    /// 否则普通照片 + 恰好同名的普通视频会被误吞、从画廊隐藏。
    #[test]
    fn plain_jpg_mp4_not_paired() {
        let c = mem_db();
        add(&c, 1, "VID_6.jpg", "jpg", "image"); // 无 has_embedded_video
        add(&c, 2, "VID_6.mp4", "mp4", "video");
        assert_eq!(
            pair_live_photos(&c, 1).unwrap(),
            0,
            "无 motion 信号不应配 mp4"
        );
        assert!(!is_live(&c, 1));
        assert_eq!(
            companion_of(&c, 2),
            None,
            "普通 mp4 不应被标 companion（仍可见）"
        );
    }

    /// T15 边界：MOV 配对不受 motion 守门影响（Apple Live Photo 纯 stem，沿用 T7）。
    /// 即便静图无 has_embedded_video，JPG + MOV 仍应配对。
    #[test]
    fn mov_pairing_not_motion_gated() {
        let c = mem_db();
        add(&c, 1, "IMG_7.jpg", "jpg", "image"); // 无 motion 信号
        add(&c, 2, "IMG_7.mov", "mov", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1, "MOV 不守门，应配对");
        assert!(is_live(&c, 1));
        assert_eq!(companion_of(&c, 2), Some(1));
    }

    /// T15 边界：motion JPG 同时有同名 MOV + MP4 → 两个视频都标 companion（都从画廊隐藏）。
    #[test]
    fn motion_jpg_both_mov_and_mp4_paired() {
        let c = mem_db();
        add_motion_image(&c, 1, "MIX_8.jpg", "jpg");
        add(&c, 2, "MIX_8.mov", "mov", "video");
        add(&c, 3, "MIX_8.mp4", "mp4", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 2, "MOV + MP4 各计一对");
        assert!(is_live(&c, 1));
        assert_eq!(companion_of(&c, 2), Some(1));
        assert_eq!(companion_of(&c, 3), Some(1));
    }

    /// T15 回归：孤立 MP4（无同名静图）不受影响，保持可见。
    #[test]
    fn lone_mp4_untouched() {
        let c = mem_db();
        add(&c, 1, "CLIP_9.mp4", "mp4", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert_eq!(companion_of(&c, 1), None);
    }

    /// 配对使逻辑单元发生变化时，旧 unit_digest 必须失效；再次扫描同一关系不应重复推进代次。
    #[test]
    fn pairing_invalidates_unit_digest_and_is_idempotent() {
        let c = mem_db();
        add(&c, 1, "IMG_10.jpg", "jpg", "image");
        add(&c, 2, "IMG_10.mov", "mov", "video");
        add_unit_digest(&c, 1, &[0x10, 0x20]);

        let before = source_revision(&c, 1);
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        assert_eq!(source_revision(&c, 1), before + 1);
        assert_eq!(
            unit_digest(&c, 1),
            None,
            "配对不得保留旧 logical unit digest"
        );

        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert_eq!(source_revision(&c, 1), before + 1, "相同关系重复扫描应幂等");

        add_unit_digest(&c, 1, &[0x30, 0x40]);
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert_eq!(unit_digest(&c, 1), Some(vec![0x30, 0x40]));
    }

    /// 主文件保持不变、companion 被替换时，旧组合摘要不能继续代表新的逻辑单元。
    #[test]
    fn changed_companion_invalidates_old_unit_digest() {
        let c = mem_db();
        add(&c, 1, "IMG_11.jpg", "jpg", "image");
        add(&c, 2, "IMG_11.mov", "mov", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        let paired_revision = source_revision(&c, 1);
        add_unit_digest(&c, 1, &[0xaa, 0xbb]);

        // 只在数据库中模拟 companion 被替换，主文件行和内容身份保持不变。
        c.execute(
            "UPDATE media_items SET file_name=?1 WHERE id=?2",
            params!["IMG_11_old.mov", 2],
        )
        .unwrap();
        add(&c, 3, "IMG_11.mov", "mov", "video");

        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        assert_eq!(companion_of(&c, 2), None, "旧 companion 应解除配对");
        assert_eq!(companion_of(&c, 3), Some(1), "新 companion 应接替配对");
        assert_eq!(source_revision(&c, 1), paired_revision + 1);
        assert_eq!(
            unit_digest(&c, 1),
            None,
            "不同 companion 不得复用旧 unit 结果"
        );
    }

    /// 软删除是可恢复的隐藏态，不应被对账当成解除配对；恢复后仍沿用原逻辑单元。
    #[test]
    fn soft_deleted_companion_relation_is_preserved() {
        let c = mem_db();
        add(&c, 1, "IMG_12.jpg", "jpg", "image");
        add(&c, 2, "IMG_12.mov", "mov", "video");
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 1);
        let paired_revision = source_revision(&c, 1);

        c.execute(
            "UPDATE media_items SET is_deleted=1 WHERE id=?1",
            params![2],
        )
        .unwrap();
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert_eq!(companion_of(&c, 2), Some(1));
        assert_eq!(source_revision(&c, 1), paired_revision);

        c.execute(
            "UPDATE media_items SET is_deleted=0 WHERE id=?1",
            params![2],
        )
        .unwrap();
        assert_eq!(pair_live_photos(&c, 1).unwrap(), 0);
        assert_eq!(companion_of(&c, 2), Some(1));
        assert_eq!(source_revision(&c, 1), paired_revision);
    }
}
