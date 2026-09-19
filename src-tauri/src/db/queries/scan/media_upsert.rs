//! 媒体项快速扫描 upsert 域:含跨域调用 `faces::recompute_person_aggregates`。
//! (D 线从 `scan.rs` 拆出,SQL/事务边界不变。)

use rusqlite::{params, Connection, OptionalExtension};

// 跨域定向引用(§4.3):person 聚合重算归 faces 域(invalidate 事务内连带重算)。
// 迁出 scan.rs 单文件后 `super` = `scan`(非 `queries`),故此处改绝对路径。
use crate::db::queries::faces::recompute_person_aggregates;
use crate::error::Result;

// ── 媒体项 ───────────────────────────────────────────────────────────────

/// 快速扫描的批量插入/更新辅助数据。
#[derive(Clone)]
pub struct FastScanItem {
    pub directory_id: i64,
    pub file_name: String,
    pub file_size: i64,
    pub file_mtime: i64,
    /// 最后修改时间的绝对 Unix 纳秒值；0 表示调用方无法取得精度。
    pub file_mtime_ns: i64,
    pub file_format: String,
    pub media_type: String,
    pub width: i64,
    pub height: i64,
    pub sort_datetime: i64,
    pub cache_key: i64,
}

/// 快速扫描 upsert 的结果（Part1 §1.5）。
///
/// `SourceChanged` 必须触发失效：主缩略图已在 SQL 内重置；调用方还须把该 item 的
/// exotic 任务退回 pending、清旧产物（见 `scanner::fast_scan`）。不能只依赖列默认值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// mtime 未变 → 跳过。
    Unchanged(i64),
    /// 全新插入。
    Inserted(i64),
    /// 已存在但源文件变化（mtime 或 size 不同）→ 缓存与任务须失效重做。
    SourceChanged(i64),
    /// 可疑变更(mtime 变但 size 同,Part2 §3.3.2):可能是同步盘占位落地等 mtime 抖动
    /// (内容未变),也可能是同大小的元数据编辑(内容变)。upsert **零写**返回本变体,
    /// 调用方在写事务之外算内容指纹、经 `resolve_suspect_change` 定案——hash IO 不进写锁。
    SuspectChanged(i64),
}

impl UpsertOutcome {
    pub fn id(&self) -> i64 {
        match self {
            UpsertOutcome::Unchanged(id)
            | UpsertOutcome::Inserted(id)
            | UpsertOutcome::SourceChanged(id)
            | UpsertOutcome::SuspectChanged(id) => *id,
        }
    }
}

/// 失效一个媒体项的全部派生元数据（Part2 §3.3 + Part3 §3.4，SourceChanged 统一入口）。
/// 文件被替换/编辑（mtime 变）时调用：删三类 meta，使 enricher 的 `image_meta.item_id IS NULL`
/// 过滤重新命中、重算 EXIF/时长/编码（否则旧 EXIF/时长/编码永久停滞——这是修复前的真 bug）；
/// 并把 `media_derivations`（视频封面/关键帧、文档缩略图、音频封面…）退回 pending，使 Producer
/// 重新派生（Part3 Q5：源变后派生停留旧版的修复）。
///
/// **必须在 upsert 的同一事务内调用**（避免半失效）。范围：image/video/audio_meta + media_derivations。
/// - 主缩略图状态（`thumb_status/thumb_path/thumbhash`）由 upsert 的 UPDATE 同事务复位，不在此重复。
/// - exotic 任务失效已在 `scanner::fast_scan` 单独接（`invalidate_exotic_tasks_for_item`），此处不重复。
/// - 旧磁盘派生产物（sprites/封面）靠 `cache_key`（含 mtime）天然换 key 成孤儿，交缓存 GC（§3.3.2）
///   兜底回收——本函数只管 DB 状态，不碰文件系统。
/// - 🟢 AI 向量（`ai_status`）/ 人脸（`face_status`）失效已接（Part4 T4 / §3.12）：换内容后旧 CLIP
///   向量/人脸框是旧图的，复位状态重分析 + 删旧向量/faces 行 + 受影响 person 派生重算。
pub fn invalidate_derived_for_item(conn: &Connection, item_id: i64) -> Result<()> {
    conn.prepare_cached("DELETE FROM image_meta WHERE item_id=?1")?
        .execute(params![item_id])?;
    conn.prepare_cached("DELETE FROM video_meta WHERE item_id=?1")?
        .execute(params![item_id])?;
    conn.prepare_cached("DELETE FROM audio_meta WHERE item_id=?1")?
        .execute(params![item_id])?;
    // Part3 Q5：派生任务退回 pending（status=0），清空旧产物路径与错误，刷新时间戳。
    // Producer 的 `get_pending_derivations`（status=0）据此重新领取重派；`updated_at` 走部分索引
    // `idx_deriv_pending`（status<2）天然纳入。
    conn.prepare_cached(
        "UPDATE media_derivations
            SET status=0, payload_path=NULL, error=NULL, updated_at=strftime('%s','now')
          WHERE item_id=?1",
    )?
    .execute(params![item_id])?;

    // 🟢 Part4 T4（§3.12）：AI/人脸失效。换内容（mtime+size 变）后旧 CLIP 向量/人脸框属旧图，
    // 不失效则语义搜索/人脸命中错图。复位 ai_status/face_status=0（重分析），删旧向量/人脸行。
    // 🔑 删脸前先收集受影响 person（删后查不到）——删脸后这些 person 的质心/封面/计数陈旧，须
    // 连带重算（同事务），范本与 §3.5.1 审批一致（recompute_person_aggregates 含删空簇策略）。
    // ai_embeddings 有 ON DELETE CASCADE，但此处是“源变”非“删 item”，须显式删。
    let affected_persons: Vec<i64> = {
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT person_id FROM faces WHERE item_id=?1 AND person_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![item_id], |row| row.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    conn.prepare_cached(
        "UPDATE media_items SET ai_status=0, face_status=0, updated_at=strftime('%s','now') WHERE id=?1",
    )?
    .execute(params![item_id])?;
    conn.prepare_cached("DELETE FROM ai_embeddings WHERE item_id=?1")?
        .execute(params![item_id])?;
    conn.prepare_cached("DELETE FROM faces WHERE item_id=?1")?
        .execute(params![item_id])?;
    // V17:源变即销**全模型**覆盖账——不销则切轨/启动 sync 会把旧内容的扫描记录
    // 误当有效覆盖,把该项翻回 Done 跳过重扫(与上面显式删 faces 同理,CASCADE 只管硬删)。
    conn.prepare_cached("DELETE FROM face_coverage WHERE item_id=?1")?
        .execute(params![item_id])?;
    for pid in affected_persons {
        recompute_person_aggregates(conn, pid)?;
    }
    Ok(())
}

/// SourceChanged 落库(upsert「size 变」路径与 resolve「hash 变/无基线」路径共用):
/// 覆写文件字段 + 重置主缩略图 + 写/清 content_hash 基线,并在同一事务语境内全失效派生。
/// `content_hash=None` 表示无可信基线(size 变路径不算 hash;指纹计算失败)→ 置 NULL,
/// 使下次可疑变更走「NULL 兜底保守失效」而非拿陈旧基线错比。
fn apply_source_changed(
    conn: &Connection,
    id: i64,
    item: &FastScanItem,
    volume_id: Option<i64>,
    content_hash: Option<&str>,
) -> Result<()> {
    // 若发生变化的是 Live Photo companion，主项的逻辑单元也随之变化；只失效
    // companion 自身会留下旧的组合摘要，导致主项继续被错误分组。
    let companion_parent: Option<i64> = conn
        .query_row(
            "SELECT companion_of FROM media_items WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    // availability 经 CASE 顺带恢复(文件变 = 必在场);volume_id 经 COALESCE 治愈历史 NULL。
    conn.prepare_cached(
        "UPDATE media_items SET file_size=?1, file_mtime=?2, file_mtime_ns=?3,
                  file_format=?4, media_type=?5, width=?6, height=?7,
                  sort_datetime=?8, cache_key=?9, thumb_status=0,
                  thumb_path=NULL, thumbhash=NULL, content_hash=?10,
                  source_revision=source_revision+1,
                  volume_id=COALESCE(volume_id, ?11),
                  availability=CASE WHEN availability='missing' THEN 'online' ELSE availability END,
                  updated_at=strftime('%s','now')
         WHERE id=?12",
    )?
    .execute(params![
        item.file_size,
        item.file_mtime,
        item.file_mtime_ns,
        item.file_format,
        item.media_type,
        item.width,
        item.height,
        item.sort_datetime,
        item.cache_key,
        content_hash,
        volume_id,
        id
    ])?;
    // 精确摘要与物理身份键绑定旧 source_revision；源变后整行作废，避免旧摘要重新参与候选。
    conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
        .execute(params![id])?;
    if let Some(parent_id) = companion_parent {
        conn.prepare_cached(
            "UPDATE media_items SET source_revision=source_revision+1,
                    updated_at=strftime('%s','now')
              WHERE id=?1",
        )?
        .execute(params![parent_id])?;
        conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
            .execute(params![parent_id])?;
    }
    // 🔴 SourceChanged 全失效(Part2 §3.3):源文件变了,旧 EXIF/时长/编码必须作废,
    // 否则 enricher 不再重选该项、元数据永久停滞。与 UPDATE 同一事务语境,避免半失效。
    invalidate_derived_for_item(conn, id)
}

/// 「可疑变更」定案(Part2 §3.3.2 三环,P1-2/P1-4):比对存量 content_hash 基线与新指纹。
/// - 基线存在且相同 → **touch**:完整 `sha256:` 指纹只更新 file_mtime/file_mtime_ns（下次扫描回
///   Unchanged），但仍推进 source_revision 并删除精确摘要；普通派生可安全复用，因为该指纹已覆盖
///   全文件。抽样 `sha256s:` 指纹不能证明未采样区域未变，因此按 SourceChanged 处理，切换带纳秒
///   mtime 的 cache_key，并让普通派生/Exotic 一并重做，避免旧 worker 与旧缓存路径继续代表新源。
/// - 基线 NULL(历史行首遇可疑变更)→ 保守判 SourceChanged(无旧值无法证明内容未变,
///   宁可重派生),并写回新指纹建立基线,下次可疑变更即可精确比对。
/// - 基线不同 → SourceChanged + 基线更新。
/// - `current_hash=None`(指纹计算失败)→ 保守 SourceChanged,基线置 NULL。
///
/// 调用方须在**写事务之外**完成指纹计算(hash IO 不进写锁),本函数只做短写。
pub fn resolve_suspect_change(
    conn: &Connection,
    id: i64,
    item: &FastScanItem,
    volume_id: Option<i64>,
    current_hash: Option<&str>,
) -> Result<UpsertOutcome> {
    let (stored, companion_parent): (Option<String>, Option<i64>) = conn
        .prepare_cached("SELECT content_hash, companion_of FROM media_items WHERE id=?1")?
        .query_row(params![id], |row| {
            let content_hash = row.get(0)?;
            let companion_parent: Option<i64> = row.get(1)?;
            Ok((content_hash, companion_parent))
        })?;
    if let (Some(base), Some(cur)) = (stored.as_deref(), current_hash) {
        if base == cur {
            if cur.starts_with("sha256s:") {
                // 抽样指纹相同只证明窗口未变；未采样区域可能已经改写。按真实源变更处理，
                // 复用 apply_source_changed 的同一失效链，尤其让 cache_key 进入带纳秒 mtime
                // 的新路径，使迟到 worker 不会覆盖新源的最终缓存文件。
                apply_source_changed(conn, id, item, volume_id, current_hash)?;
                return Ok(UpsertOutcome::SourceChanged(id));
            }
            conn.prepare_cached(
                "UPDATE media_items SET file_mtime=?2, file_mtime_ns=?3,
                          volume_id=COALESCE(volume_id, ?4),
                          availability=CASE WHEN availability='missing' THEN 'online' ELSE availability END,
                          source_revision=source_revision + 1,
                          updated_at=strftime('%s','now')
                 WHERE id=?1",
            )?
            .execute(params![id, item.file_mtime, item.file_mtime_ns, volume_id])?;
            // 同 hash 只代表 change fingerprint 未变；抽样指纹可能漏掉大文件未抽样区域的改写。
            // 因此 touch 也必须切断旧 exact digest 的 source_revision 绑定，避免并发分析把旧摘要
            // 重新写回后继续参与重复组。
            conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
                .execute(params![id])?;
            if let Some(parent_id) = companion_parent {
                // Live Photo 的逻辑单元由主项与 companion 共同决定；companion touch 同样使主项
                // 的组合摘要过期，即便 companion 自身的 change fingerprint 没有变化。
                conn.prepare_cached(
                    "UPDATE media_items SET source_revision=source_revision+1,
                            updated_at=strftime('%s','now')
                      WHERE id=?1",
                )?
                .execute(params![parent_id])?;
                conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
                    .execute(params![parent_id])?;
            }
            return Ok(UpsertOutcome::Unchanged(id));
        }
    }
    apply_source_changed(conn, id, item, volume_id, current_hash)?;
    Ok(UpsertOutcome::SourceChanged(id))
}

/// 插入或更新来自快速扫描阶段的媒体项。
/// 返回 [`UpsertOutcome`]，供调用方据此播种/失效 exotic 任务。
/// 快速入库 upsert。`volume_id` 为本 scan_root 所属卷（扫描上下文常量，非 per-file 数据）——
/// 新项据此入库、历史 NULL 项经 `COALESCE` 顺带治愈，使其能参与缺失检测守门1（在线卷集）。
/// `None`（未识别卷/孤儿根）→ 新项 volume_id 留 NULL（守门把它天然排除，宁可不删）。
pub fn upsert_fast_scan_item(
    conn: &Connection,
    item: &FastScanItem,
    volume_id: Option<i64>,
) -> Result<UpsertOutcome> {
    // 检查是否存在具有相同 mtime 的项（无需更改）；同时取 availability + volume_id 以支撑
    // missing→online 自动恢复 与 历史 NULL volume_id 的定向治愈。
    // 热路径:每文件一次存在性 SELECT + 可能一次 INSERT。`prepare_cached` 让 SQLite
    // 复用同一连接上的已编译语句,消除逐文件 SQL 编译;查询错误仍按原语义视为「不存在」。
    #[allow(clippy::type_complexity)]
    let existing: Option<(i64, i64, Option<i64>, i64, String, Option<i64>, Option<i64>)> = conn
        .prepare_cached(
            "SELECT id, file_mtime, file_mtime_ns, file_size, availability, volume_id, companion_of
               FROM media_items WHERE directory_id=?1 AND file_name=?2",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_row(params![item.directory_id, item.file_name], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .optional()
            .ok()
            .flatten()
        });

    if let Some((id, mtime, mtime_ns, size, availability, existing_vol, companion_parent)) =
        existing
    {
        // V26 之前的行没有可用纳秒值（NULL）。调用方的 0 同样表示无法取得精度；两者
        // 都未知时仍可保留旧的秒级兼容行为，但任一侧有已知纳秒且数值不同必须进 suspect。
        let same_mtime = mtime == item.file_mtime
            && (mtime_ns == Some(item.file_mtime_ns)
                || (mtime_ns.is_none() && item.file_mtime_ns == 0));
        if same_mtime && size == item.file_size {
            // Unchanged — skip（但若曾被标 missing 的文件原样重现，需自动恢复 online；
            // 或历史插入遗留 volume_id=NULL，需补绑本根卷使其可参与缺失检测）。
            // 🔴 重现自动恢复（Part2 §3.2.4）+ 卷补绑：Unchanged 路径本不写库，仅当确有需要
            // （missing 复位 或 卷 NULL 待治愈）时做一次定向写 —— 普通未变更文件仍零写。
            // **不碰 is_deleted/offline**。COALESCE 保留既有卷、仅填 NULL。
            let needs_avail_fix = availability == "missing";
            let needs_vol_heal = existing_vol.is_none() && volume_id.is_some();
            if needs_avail_fix || needs_vol_heal {
                conn.prepare_cached(
                    "UPDATE media_items SET
                         availability = CASE WHEN availability='missing' THEN 'online' ELSE availability END,
                         volume_id    = COALESCE(volume_id, ?2),
                         source_revision = source_revision + CASE WHEN availability='missing' THEN 1 ELSE 0 END,
                         updated_at   = strftime('%s','now')
                     WHERE id=?1",
                )?
                .execute(params![id, volume_id])?;
            }
            if needs_avail_fix {
                // 文件从 missing 重现时，即使 size/mtime 恰好相同，也不能证明仍是同一
                // 物理内容；旧 digest 必须失效，等待下一次独立分析重新确证。
                conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
                    .execute(params![id])?;
                if let Some(parent_id) = companion_parent {
                    conn.prepare_cached(
                        "UPDATE media_items SET source_revision=source_revision+1,
                                updated_at=strftime('%s','now')
                          WHERE id=?1",
                    )?
                    .execute(params![parent_id])?;
                    conn.prepare_cached("DELETE FROM dedup_index WHERE item_id=?1")?
                        .execute(params![parent_id])?;
                }
            }
            return Ok(UpsertOutcome::Unchanged(id));
        }
        // 🔴 可疑变更(Part2 §3.3.2 / P1-4):mtime 变但 size 同——可能是同步盘占位落地等
        // mtime 抖动且 size 同(内容未变,无条件失效会白重派生全链),也可能是同大小元数据编辑
        // (内容变,仅 touch 会漏失效)。零写返回,由调用方在写事务外算指纹后经
        // `resolve_suspect_change` 定案。size 变 → 内容必变,直接 SourceChanged 不算 hash；
        // 即使 mtime/mtime_ns 未变，也必须走这里的 SourceChanged 路径。
        if size == item.file_size {
            return Ok(UpsertOutcome::SuspectChanged(id));
        }
        apply_source_changed(conn, id, item, volume_id, None)?;
        return Ok(UpsertOutcome::SourceChanged(id));
    }

    // New item — 据扫描上下文卷入库（修复:此前新项 volume_id 恒 NULL → 缺失检测对新数据休眠）。
    // 新项
    conn.prepare_cached(
        "INSERT INTO media_items
             (directory_id, file_name, file_size, file_mtime, file_mtime_ns,
               file_format, media_type, width, height, sort_datetime, cache_key, volume_id)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )?
    .execute(params![
        item.directory_id,
        item.file_name,
        item.file_size,
        item.file_mtime,
        item.file_mtime_ns,
        item.file_format,
        item.media_type,
        item.width,
        item.height,
        item.sort_datetime,
        item.cache_key,
        volume_id
    ])?;
    Ok(UpsertOutcome::Inserted(conn.last_insert_rowid()))
}

#[cfg(test)]
mod fast_scan_upsert_recovery_tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免构造 directory 链
        c
    }

    fn item(name: &str, mtime: i64) -> FastScanItem {
        FastScanItem {
            directory_id: 1,
            file_name: name.into(),
            file_size: 10,
            file_mtime: mtime,
            file_mtime_ns: 0,
            file_format: "jpg".into(),
            media_type: "image".into(),
            width: 0,
            height: 0,
            sort_datetime: mtime,
            cache_key: 0,
        }
    }

    /// 同 `item` 但可指定 file_size 与 cache_key(size 变 → 直接 SourceChanged;
    /// size 同 → SuspectChanged,见 content_hash 三环测试)。
    fn item_sized(name: &str, mtime: i64, size: i64, cache_key: i64) -> FastScanItem {
        FastScanItem {
            file_size: size,
            file_mtime: mtime,
            sort_datetime: mtime,
            cache_key,
            ..item(name, 0)
        }
    }

    fn item_with_ns(name: &str, mtime: i64, mtime_ns: i64) -> FastScanItem {
        FastScanItem {
            file_mtime_ns: mtime_ns,
            ..item(name, mtime)
        }
    }

    fn insert_missing(c: &Connection, id: i64, name: &str, mtime: i64) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, availability)
             VALUES (?1, 1, ?2, 10, ?3, 'jpg', 'image', 0, 0, ?3, 0, 'missing')",
            params![id, name, mtime],
        )
        .unwrap();
    }

    fn avail(c: &Connection, id: i64) -> String {
        c.query_row(
            "SELECT availability FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 原样重现（同 mtime）：Unchanged + availability 自动 missing→online；is_deleted 不动。
    #[test]
    fn unchanged_reappearance_recovers_missing() {
        let c = mem_db();
        insert_missing(&c, 1, "a.jpg", 100);
        let out = upsert_fast_scan_item(&c, &item("a.jpg", 100), None).unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(1));
        assert_eq!(avail(&c, 1), "online", "原样重现应自动恢复 online");
        let deleted: i64 = c
            .query_row("SELECT is_deleted FROM media_items WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(deleted, 0, "恢复不得碰 is_deleted");
    }

    /// 变更重现(mtime+size 变=内容确变):SourceChanged + availability 经 CASE 恢复。
    #[test]
    fn changed_reappearance_recovers_missing() {
        let c = mem_db();
        insert_missing(&c, 2, "b.jpg", 100);
        let out = upsert_fast_scan_item(&c, &item_sized("b.jpg", 200, 20, 0), None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(2));
        assert_eq!(avail(&c, 2), "online");
    }

    /// SourceChanged（mtime 变）：删除旧 image/video/audio_meta（enricher 据 IS NULL 重选重算）。
    /// Unchanged 不删（避免给未变更项白删 meta）。
    #[test]
    fn source_changed_invalidates_meta_but_unchanged_keeps() {
        let c = mem_db();
        // 既有项 + 三类 meta 各一行。
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (5, 1, 'd.jpg', 10, 100, 'jpg', 'image', 0, 0, 100, 0)",
            [],
        )
        .unwrap();
        c.execute("INSERT INTO image_meta (item_id) VALUES (5)", [])
            .unwrap();
        c.execute("INSERT INTO video_meta (item_id) VALUES (5)", [])
            .unwrap();
        c.execute("INSERT INTO audio_meta (item_id) VALUES (5)", [])
            .unwrap();
        // Part3 Q5：一条已完成派生（status=2，带旧产物路径 + 残留错误）——SourceChanged 须复位重派。
        c.execute(
            "INSERT INTO media_derivations (item_id, kind, status, payload_path, error)
             VALUES (5, 'video_cover', 2, 'old/cover.webp', 'stale')",
            [],
        )
        .unwrap();
        // Part4 T4：AI/人脸 fixtures。ai_status/face_status=2（done），item 5 一条向量；person 1
        // 两脸（item 5 一张、item 6 一张），用于验证源变后 item 5 的向量/脸删、状态复位、person 重算。
        // 嵌入/质心 = X'0000803F00000000' = [1.0, 0.0]（2×f32 LE）。
        c.execute(
            "UPDATE media_items SET ai_status=2, face_status=2 WHERE id=5",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO ai_embeddings (item_id, model_name, embedding) VALUES (5, 'clip', X'0000803F00000000')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO persons (id, name, model_name, centroid, face_count)
             VALUES (1, NULL, 'yunet-sface', X'0000803F00000000', 2)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO faces (id, item_id, person_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h,
                                det_score, quality, embedding, is_confirmed)
             VALUES (50, 5, 1, 'yunet-sface', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, X'0000803F00000000', 0),
                    (51, 6, 1, 'yunet-sface', 0.1, 0.1, 0.2, 0.2, 0.9, 0.6, X'0000803F00000000', 0)",
            [],
        )
        .unwrap();

        let ai_face_status = |c: &Connection| -> (i64, i64) {
            c.query_row(
                "SELECT ai_status, face_status FROM media_items WHERE id=5",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let count_where =
            |c: &Connection, sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };

        let meta_count = |c: &Connection| -> i64 {
            let q = |t: &str| -> i64 {
                c.query_row(
                    &format!("SELECT count(*) FROM {t} WHERE item_id=5"),
                    [],
                    |r| r.get(0),
                )
                .unwrap()
            };
            q("image_meta") + q("video_meta") + q("audio_meta")
        };
        // 派生行的 (status, payload_path 是否为空, error 是否为空)。
        let deriv_state = |c: &Connection| -> (i64, bool, bool) {
            c.query_row(
                "SELECT status, payload_path IS NULL, error IS NULL
                   FROM media_derivations WHERE item_id=5 AND kind='video_cover'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };

        // mtime 未变 → Unchanged → meta 保留 + 派生保持 done（不白删/不白重派）+ AI/人脸不动。
        let out0 = upsert_fast_scan_item(&c, &item("d.jpg", 100), None).unwrap();
        assert_eq!(out0, UpsertOutcome::Unchanged(5));
        assert_eq!(meta_count(&c), 3, "未变更不应删 meta");
        assert_eq!(
            deriv_state(&c),
            (2, false, false),
            "未变更不应复位派生（应仍为 done、保留产物路径）"
        );
        assert_eq!(ai_face_status(&c), (2, 2), "未变更不应复位 AI/人脸状态");
        assert_eq!(
            count_where(&c, "SELECT count(*) FROM ai_embeddings WHERE item_id=5"),
            1,
            "未变更不应删向量"
        );

        // mtime+size 变 → SourceChanged → meta 全删 + 派生退回 pending + AI/人脸失效(T4)。
        let out = upsert_fast_scan_item(&c, &item_sized("d.jpg", 200, 20, 0), None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(5));
        assert_eq!(meta_count(&c), 0, "SourceChanged 应删除全部旧 meta");
        assert_eq!(
            deriv_state(&c),
            (0, true, true),
            "SourceChanged 应把派生退回 pending(0) 并清空 payload_path/error"
        );
        // Part4 T4 断言：状态复位 0、item 5 的向量/脸删除、person 1 重算（仅剩 item 6 的脸 51）。
        assert_eq!(
            ai_face_status(&c),
            (0, 0),
            "SourceChanged 应复位 ai_status/face_status"
        );
        assert_eq!(
            count_where(&c, "SELECT count(*) FROM ai_embeddings WHERE item_id=5"),
            0,
            "应删 item 5 的旧 CLIP 向量"
        );
        assert_eq!(
            count_where(&c, "SELECT count(*) FROM faces WHERE item_id=5"),
            0,
            "应删 item 5 的旧人脸"
        );
        assert_eq!(
            count_where(&c, "SELECT count(*) FROM faces WHERE id=51"),
            1,
            "其它 item 的脸不受影响"
        );
        assert_eq!(
            count_where(&c, "SELECT face_count FROM persons WHERE id=1"),
            1,
            "受影响 person 应重算（2→1，仅剩 item 6 的脸）"
        );
    }

    /// 普通 online 未变更：Unchanged，availability 保持 online（CASE/条件均不误改）。
    #[test]
    fn unchanged_online_stays_online() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (3, 1, 'c.jpg', 10, 100, 'jpg', 'image', 0, 0, 100, 0)",
            [],
        )
        .unwrap();
        let out = upsert_fast_scan_item(&c, &item("c.jpg", 100), None).unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(3));
        assert_eq!(avail(&c, 3), "online");
    }

    fn vol_of(c: &Connection, id: i64) -> Option<i64> {
        c.query_row(
            "SELECT volume_id FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 新项据扫描上下文卷入库（修复:此前新项 volume_id 恒 NULL → 缺失检测对新数据休眠）。
    #[test]
    fn new_item_inherits_scan_volume() {
        let c = mem_db();
        let out = upsert_fast_scan_item(&c, &item("n.jpg", 100), Some(7)).unwrap();
        let id = match out {
            UpsertOutcome::Inserted(id) => id,
            other => panic!("应为 Inserted，实得 {other:?}"),
        };
        assert_eq!(vol_of(&c, id), Some(7), "新项应继承本根卷 id");
    }

    /// 历史遗留 volume_id=NULL 的项：再扫描时经 COALESCE 顺带治愈（Unchanged 定向写、SourceChanged 顺带）。
    #[test]
    fn legacy_null_volume_healed_on_rescan() {
        let c = mem_db();
        // 模拟修复前插入的项：volume_id 为 NULL。
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (9, 1, 'old.jpg', 10, 100, 'jpg', 'image', 0, 0, 100, 0)",
            [],
        )
        .unwrap();
        assert_eq!(vol_of(&c, 9), None, "前置：历史项 volume_id 应为 NULL");

        // 原样重扫（mtime 不变）→ Unchanged，但 NULL 卷应被定向治愈。
        let out = upsert_fast_scan_item(&c, &item("old.jpg", 100), Some(7)).unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(9));
        assert_eq!(vol_of(&c, 9), Some(7), "Unchanged 路径应治愈历史 NULL 卷");
    }

    /// COALESCE 不覆盖既有卷：已绑卷的项再扫描，volume_id 保持原值（即便传入不同卷）。
    #[test]
    fn existing_volume_not_overwritten() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id)
             VALUES (11, 1, 'bound.jpg', 10, 100, 'jpg', 'image', 0, 0, 100, 0, 3)",
            [],
        )
        .unwrap();
        // mtime+size 变 → SourceChanged,传入卷 9,但既有卷 3 应被 COALESCE 保留。
        let out = upsert_fast_scan_item(&c, &item_sized("bound.jpg", 200, 20, 0), Some(9)).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(11));
        assert_eq!(vol_of(&c, 11), Some(3), "既有卷不应被覆盖");
    }

    /// Part2 §3.3.2:可疑变更(mtime 变 size 同)→ upsert 零写返回 SuspectChanged,
    /// 失效不发生、DB mtime 未动(等待事务外指纹定案)。
    #[test]
    fn suspect_change_returns_without_writes() {
        let c = mem_db();
        insert_missing(&c, 30, "s.jpg", 100);
        c.execute("INSERT INTO image_meta (item_id) VALUES (30)", [])
            .unwrap();
        let out = upsert_fast_scan_item(&c, &item("s.jpg", 200), None).unwrap();
        assert_eq!(out, UpsertOutcome::SuspectChanged(30));
        let (mtime, meta): (i64, i64) = c
            .query_row(
                "SELECT file_mtime, (SELECT count(*) FROM image_meta WHERE item_id=30)
                 FROM media_items WHERE id=30",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(mtime, 100, "未定案不得写 mtime");
        assert_eq!(meta, 1, "未定案不得失效 meta");
    }

    /// 同一秒内仅纳秒变化也必须进入 suspect，不能被旧的秒级相等判断吞掉。
    #[test]
    fn same_second_nanosecond_change_is_suspect() {
        let c = mem_db();
        insert_missing(&c, 31, "nanos.jpg", 100);
        c.execute(
            "UPDATE media_items SET file_mtime_ns=100000000001, source_revision=7 WHERE id=31",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (31, 7, 1, 'done', 1)",
            [],
        )
        .unwrap();

        let out =
            upsert_fast_scan_item(&c, &item_with_ns("nanos.jpg", 100, 100000000002), None).unwrap();
        assert_eq!(out, UpsertOutcome::SuspectChanged(31));

        let (mtime_ns, revision, dedup_count): (i64, i64, i64) = c
            .query_row(
                "SELECT file_mtime_ns, source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=31)
                   FROM media_items WHERE id=31",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(mtime_ns, 100000000001, "suspect 阶段不得写入新纳秒 mtime");
        assert_eq!(revision, 7, "suspect 阶段不得提前推进源代次");
        assert_eq!(dedup_count, 1, "suspect 阶段不得提前删除精确摘要");
    }

    /// 新媒体项必须把 V26 纳秒 mtime 持久化，且从初始 source_revision=1 开始。
    #[test]
    fn new_item_persists_nanosecond_mtime() {
        let c = mem_db();
        let out =
            upsert_fast_scan_item(&c, &item_with_ns("new-nanos.jpg", 100, 100000000123), None)
                .unwrap();
        let id = out.id();
        let (mtime_ns, revision): (i64, i64) = c
            .query_row(
                "SELECT file_mtime_ns, source_revision FROM media_items WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(mtime_ns, 100000000123);
        assert_eq!(revision, 1);
    }

    /// 真正源变更推进 source_revision，并删除绑定旧代次的 dedup_index 行。
    #[test]
    fn source_change_advances_revision_and_clears_dedup_index() {
        let c = mem_db();
        insert_missing(&c, 32, "changed.jpg", 100);
        c.execute(
            "UPDATE media_items SET file_mtime_ns=100000000000, source_revision=4 WHERE id=32",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (32, 4, 1, 'done', 1)",
            [],
        )
        .unwrap();

        let mut changed = item_sized("changed.jpg", 200, 20, 999);
        changed.file_mtime_ns = 200000000001;
        let out = upsert_fast_scan_item(&c, &changed, None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(32));

        let (mtime_ns, revision, dedup_count): (i64, i64, i64) = c
            .query_row(
                "SELECT file_mtime_ns, source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=32)
                   FROM media_items WHERE id=32",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(mtime_ns, 200000000001);
        assert_eq!(revision, 5);
        assert_eq!(dedup_count, 0);
    }

    /// mtime 与 mtime_ns 都未变但 size 变时，仍必须视为源变更并失效 exact sidecar。
    #[test]
    fn same_mtime_with_size_change_advances_revision_and_clears_dedup_index() {
        let c = mem_db();
        insert_missing(&c, 36, "same-mtime.jpg", 100);
        c.execute(
            "UPDATE media_items SET file_mtime_ns=100000000001, source_revision=4 WHERE id=36",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (36, 4, 1, 'done', 1)",
            [],
        )
        .unwrap();

        let changed = FastScanItem {
            file_size: 20,
            file_mtime: 100,
            file_mtime_ns: 100000000001,
            cache_key: 999,
            ..item("same-mtime.jpg", 100)
        };
        let out = upsert_fast_scan_item(&c, &changed, None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(36));

        let (size, mtime_ns, revision, dedup_count): (i64, i64, i64, i64) = c
            .query_row(
                "SELECT file_size, file_mtime_ns, source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=36)
                   FROM media_items WHERE id=36",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(size, 20);
        assert_eq!(mtime_ns, 100000000001);
        assert_eq!(revision, 5, "size 变更即使 mtime_ns 不变也要推进源代次");
        assert_eq!(dedup_count, 0, "size 变更必须清除旧 exact sidecar");
    }

    /// 同 hash 定案只 touch 普通源字段；精确摘要仍必须失效并切换 source_revision。
    #[test]
    fn touch_updates_nanoseconds_and_invalidates_dedup_without_rederiving_metadata() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_mtime_ns,
                 file_format, media_type, width, height, sort_datetime, cache_key,
                 source_revision, content_hash)
             VALUES (33, 1, 'touch.jpg', 10, 100, 100000000000,
                     'jpg', 'image', 0, 0, 100, 777, 9, 'sha256:same')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (33, 9, 1, 'done', 1)",
            [],
        )
        .unwrap();

        let mut touched = item_sized("touch.jpg", 101, 10, 999);
        touched.file_mtime_ns = 101000000007;
        let out = resolve_suspect_change(&c, 33, &touched, None, Some("sha256:same")).unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(33));

        let (mtime, mtime_ns, revision, cache_key, dedup_count): (i64, i64, i64, i64, i64) = c
            .query_row(
                "SELECT file_mtime, file_mtime_ns, source_revision, cache_key,
                        (SELECT count(*) FROM dedup_index WHERE item_id=33)
                   FROM media_items WHERE id=33",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(mtime, 101);
        assert_eq!(mtime_ns, 101000000007);
        assert_eq!(
            revision, 10,
            "touch 也必须推进 source_revision 以拒绝旧 exact digest"
        );
        assert_eq!(cache_key, 777, "touch 不应改写缓存键");
        assert_eq!(dedup_count, 0, "抽样指纹相同也不能保留可能过期的精确摘要");

        // 同一项随后确认内容确实变化：resolve 路径也必须推进代次并清理旧索引。
        let mut changed = item_sized("touch.jpg", 102, 10, 1001);
        changed.file_mtime_ns = 102000000009;
        let out = resolve_suspect_change(&c, 33, &changed, None, Some("sha256:changed")).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(33));
        let (revision, dedup_count): (i64, i64) = c
            .query_row(
                "SELECT source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=33)
                   FROM media_items WHERE id=33",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(revision, 11, "确认源变更应推进 source_revision");
        assert_eq!(dedup_count, 0, "确认源变更应删除旧精确摘要");
    }

    #[test]
    fn sampled_touch_is_a_source_change_and_switches_cache_generation() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_mtime_ns,
                 file_format, media_type, width, height, sort_datetime, cache_key,
                 source_revision, content_hash, thumb_status, thumb_path)
             VALUES (35, 1, 'sampled.bin', 10, 100, 100000000000,
                     'bin', 'document', 0, 0, 100, 777, 9, 'sha256s:same', 1,
                     '480/aa/old.webp')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (35, 9, 1, 'ready', 1)",
            [],
        )
        .unwrap();
        let mut touched = item_sized("sampled.bin", 101, 10, 999);
        touched.file_mtime_ns = 101000000007;

        let out = resolve_suspect_change(&c, 35, &touched, None, Some("sha256s:same")).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(35));
        let (revision, cache_key, thumb_status, thumb_path, dedup_count): (
            i64,
            i64,
            i64,
            Option<String>,
            i64,
        ) = c
            .query_row(
                "SELECT source_revision, cache_key, thumb_status, thumb_path,
                        (SELECT count(*) FROM dedup_index WHERE item_id=35)
                   FROM media_items WHERE id=35",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(revision, 10);
        assert_eq!(cache_key, 999);
        assert_eq!(thumb_status, 0);
        assert_eq!(thumb_path, None);
        assert_eq!(dedup_count, 0);
    }

    /// Part2 §3.3.2 三环:NULL 基线保守失效+建基线;同 hash touch(保留 meta/cache_key 但清 exact digest);
    /// 变 hash 失效+基线更新;指纹失败保守失效+基线置 NULL;size 变直接失效+陈旧基线清 NULL。
    #[test]
    fn content_hash_three_rings() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (40, 1, 'r.jpg', 10, 100, 'jpg', 'image', 0, 0, 100, 777)",
            [],
        )
        .unwrap();
        let hash_of = |c: &Connection| -> Option<String> {
            c.query_row(
                "SELECT content_hash FROM media_items WHERE id=40",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let seed_meta = |c: &Connection| {
            c.execute("INSERT INTO image_meta (item_id) VALUES (40)", [])
                .unwrap();
        };
        let meta_count = |c: &Connection| -> i64 {
            c.query_row(
                "SELECT count(*) FROM image_meta WHERE item_id=40",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };

        // 环② NULL 兜底:无基线 → 保守 SourceChanged + 写回建立基线。
        seed_meta(&c);
        let out = resolve_suspect_change(
            &c,
            40,
            &item_sized("r.jpg", 200, 10, 888),
            None,
            Some("sha256:aaa"),
        )
        .unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(40));
        assert_eq!(hash_of(&c).as_deref(), Some("sha256:aaa"), "基线已建立");
        assert_eq!(meta_count(&c), 0, "保守失效应删 meta");

        // 环③ 同 hash → touch:mtime 更新、meta 与 cache_key 保留，但 exact digest 必须清除。
        seed_meta(&c);
        let out = resolve_suspect_change(
            &c,
            40,
            &item_sized("r.jpg", 300, 10, 999),
            None,
            Some("sha256:aaa"),
        )
        .unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(40));
        let (mtime, key): (i64, i64) = c
            .query_row(
                "SELECT file_mtime, cache_key FROM media_items WHERE id=40",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(mtime, 300, "touch 应更新 mtime(下次扫描回 Unchanged)");
        assert_eq!(key, 888, "touch 保留旧 cache_key,既有派生继续命中");
        assert_eq!(meta_count(&c), 1, "touch 不失效 meta");
        let (revision, dedup_count): (i64, i64) = c
            .query_row(
                "SELECT source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=40)
                   FROM media_items WHERE id=40",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            revision, 3,
            "同 hash touch 也要切换 exact digest 的 source_revision"
        );
        assert_eq!(dedup_count, 0, "同 hash touch 不得复用旧 exact digest");

        // 环① hash 变 → SourceChanged + 基线更新。
        let out = resolve_suspect_change(
            &c,
            40,
            &item_sized("r.jpg", 400, 10, 111),
            None,
            Some("sha256:bbb"),
        )
        .unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(40));
        assert_eq!(hash_of(&c).as_deref(), Some("sha256:bbb"), "基线随内容更新");
        assert_eq!(meta_count(&c), 0);

        // 指纹计算失败 → 保守 SourceChanged + 基线置 NULL(下次仍走兜底)。
        let out =
            resolve_suspect_change(&c, 40, &item_sized("r.jpg", 500, 10, 222), None, None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(40));
        assert_eq!(hash_of(&c), None, "失败不得留下可信假基线");

        // size 变 → upsert 直接 SourceChanged(不算 hash),陈旧基线清 NULL。
        c.execute(
            "UPDATE media_items SET content_hash='sha256:old' WHERE id=40",
            [],
        )
        .unwrap();
        let out = upsert_fast_scan_item(&c, &item_sized("r.jpg", 600, 20, 333), None).unwrap();
        assert_eq!(out, UpsertOutcome::SourceChanged(40));
        assert_eq!(hash_of(&c), None, "size 变后旧基线作废,须清 NULL");
    }

    /// companion 的 change fingerprint 相同时，也必须让主项的组合摘要失效。
    #[test]
    fn companion_touch_invalidates_parent_unit_digest() {
        let c = mem_db();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_mtime_ns,
                 file_format, media_type, width, height, sort_datetime, cache_key,
                 source_revision)
             VALUES (34, 1, 'live.heic', 10, 100, 100000000000,
                     'heic', 'image', 0, 0, 100, 1, 3),
                    (35, 1, 'live.mov', 10, 100, 100000000000,
                     'mov', 'video', 0, 0, 100, 2, 7)",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE media_items
                SET companion_of=34, content_hash='sha256:same'
              WHERE id=35",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
             VALUES (34, 3, 1, 'done', 1), (35, 7, 1, 'done', 1)",
            [],
        )
        .unwrap();

        let mut touched = item_sized("live.mov", 101, 10, 999);
        touched.file_mtime_ns = 101000000007;
        let out = resolve_suspect_change(&c, 35, &touched, None, Some("sha256:same")).unwrap();
        assert_eq!(out, UpsertOutcome::Unchanged(35));

        let (child_revision, parent_revision, child_dedup, parent_dedup): (i64, i64, i64, i64) = c
            .query_row(
                "SELECT child.source_revision, parent.source_revision,
                        (SELECT count(*) FROM dedup_index WHERE item_id=35),
                        (SELECT count(*) FROM dedup_index WHERE item_id=34)
                   FROM media_items AS child
                   JOIN media_items AS parent ON parent.id=child.companion_of
                  WHERE child.id=35",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(child_revision, 8);
        assert_eq!(parent_revision, 4);
        assert_eq!(child_dedup, 0);
        assert_eq!(parent_dedup, 0);
    }
}
