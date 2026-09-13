//! 派生任务域 DAO(media_derivations 状态机:pending/claim/finish/reset/backfill、
//! keyframe payload、doc thumb pending;T 线拆分自 queries.rs,SQL 与行为不变)。

use rusqlite::{params, Connection, OptionalExtension, Row};

// 「exotic 接管门控」谓词由 exotic 域持有(P0 增补裁决);format! 内联捕获须裸名,故 use 引入。
use super::exotic::NOT_BLOCKED_BY_EXOTIC;
// 隐藏根排除谓词(V21 生成侧,与缩略图/AI/人脸三流水线同款):排除只放**消费口**
// (get_pending_derivations / list_pending_doc_thumbs),backfill 入队与按状态计数**有意不排**——
// 已入队的 status=0 行躺表无害,unhide 后无需重新入队即被生产者直接领取,
// 复用本文件 exclude_kinds 开关已验证的「非破坏性暂停」语义。
use super::scan::EXCLUDE_HIDDEN_ROOTS_M;
// 文档格式权威读取(store_doc_thumbnail 守卫复用);documents 为 queries 的兄弟私有子模块。
use super::documents::get_item_file_format;
use crate::error::{AppError, Result};

// ── 派生任务（media_derivations） ─────────────────────────────────────────────
//
// 状态机与 AI 完全同构：0 待处理 / 1 处理中 / 2 完成 / 3 错误，支持断点续传 + 孤儿恢复。
// 与 AI（ai_status 列）不同，派生任务是独立表，每个 (item, kind) 一行，需显式入队（backfill）。
// 见 docs/archive/feature_expansion_plan_v1.md §2.2。

/// 解析给消费者的待处理派生任务：绝对源路径在此通过 JOIN 解析（仿 `get_pending_ai_items`），
/// 使每种 kind 的 `run` 可直接读取源文件。
///
/// `(item_id, kind, abs_path, file_format, media_type, source_revision, cache_key)`。
///
/// `source_revision` 与 `cache_key` 是从同一条 `media_items` 读取的生产快照；它们必须
/// 随任务一路传到认领与完成阶段，不能在 worker 返回时重新读取当前行来「补快照」。
pub type DerivationTask = (i64, String, String, String, String, i64, i64);

/// 派生任务认领所需的最小快照：`(item_id, kind, source_revision, cache_key)`。
pub type DerivationClaim = (i64, String, i64, i64);

/// 获取待处理派生任务（status=0），可选按一组 `kind` 过滤，并可排除一组 kind。`kind_filter`
/// 支持多 kind——视频控制卡需要封面+关键帧一次受限运行。`exclude_kinds` 使流水线尊重用户的
/// 「提取视频封面 / 关键帧」开关：某 kind 被关闭时，其已入队的待处理行在此直接跳过
/// （非破坏性 —— 开关重新打开后即续传）。
pub fn get_pending_derivations(
    conn: &Connection,
    limit: i64,
    kind_filter: Option<&[String]>,
    exclude_kinds: &[&str],
) -> Result<Vec<DerivationTask>> {
    // 真正的跨 kind 优先级由「入队哪些 kind」（backfill 顺序）在上游保证，
    // 这里按 (kind, item_id) 排序即可保证确定性批处理。
    // pdf/svg 文档缩略图是「前端驱动」（Lite 无 native 栅格化器）：后端无法生成，
    // 故在生产者查询里排除，使其保持待处理（status=0）留给前端 list_pending_doc_thumbs 领取。
    // epub 文档缩略图仍由后端处理（derive/doc.rs 取 OPF 封面）。详见 §3.4。
    // video_playable 是「按需交互 kind」（播放解析路径 §5.2）：仅由 upsert_and_claim_derivation
    // 即刻领取 + VideoWorkerService 交互优先级派活,绝不经背景流水线；同 pdf/svg 样式在此排除,
    // 使其误入队的行(不应发生)也留在 status=0,不被背景批领取/跑坏(与 pipeline.rs run 分支穷尽注释互证)。
    // 隐藏根排除(V21):隐藏根的待处理行留在 status=0 暂停,unhide 后直接续跑。
    let base = format!(
        "
        SELECT dv.item_id, dv.kind,
               CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                    ELSE r.path || '/' || d.rel_path || '/' || m.file_name
               END,
               m.file_format, m.media_type, m.source_revision, m.cache_key
        FROM media_derivations dv
        JOIN media_items m ON dv.item_id = m.id
        JOIN directories d ON m.directory_id = d.id
        JOIN scan_roots r ON d.root_id = r.id
        WHERE dv.status = 0 AND m.is_deleted = 0
          AND NOT (dv.kind = 'doc_thumb' AND m.file_format IN ('pdf','svg'))
          AND dv.kind != 'video_playable'
          {EXCLUDE_HIDDEN_ROOTS_M}"
    );

    let map_row = |row: &Row<'_>| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    };

    // 动态拼接 WHERE/ORDER/LIMIT 尾部，使可选 `kind` 过滤与变长 `exclude_kinds` 集合全部以
    // 绑定参数传入（绝不拼接值）。
    let mut sql = base;
    let mut sql_params: Vec<&dyn rusqlite::ToSql> = Vec::new();
    let mut idx = 1;

    // 过滤集合以借用调用方切片元素的方式压入 `sql_params`（引用在整个函数内有效）。
    // 空切片视同无过滤——绝不能生成 `IN ()` 非法 SQL。
    if let Some(kinds) = kind_filter.filter(|k| !k.is_empty()) {
        let placeholders: Vec<String> = (idx..idx + kinds.len()).map(|i| format!("?{i}")).collect();
        sql.push_str(&format!(" AND dv.kind IN ({})", placeholders.join(",")));
        for k in kinds {
            sql_params.push(k as &dyn rusqlite::ToSql);
        }
        idx += kinds.len();
    }

    if !exclude_kinds.is_empty() {
        let placeholders: Vec<String> = (idx..idx + exclude_kinds.len())
            .map(|i| format!("?{i}"))
            .collect();
        sql.push_str(&format!(" AND dv.kind NOT IN ({})", placeholders.join(",")));
        for k in exclude_kinds {
            sql_params.push(k as &dyn rusqlite::ToSql);
        }
        idx += exclude_kinds.len();
    }

    sql.push_str(&format!(" ORDER BY dv.kind, dv.item_id LIMIT ?{idx}"));
    sql_params.push(&limit as &dyn rusqlite::ToSql);

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(sql_params.as_slice(), map_row)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按生产快照认领一批派生任务（status=0 → 1），避免重复排队和旧路径继续执行。
///
/// 返回真正认领成功的任务。查询待处理项与此处写入之间若源文件被重新扫描，快照条件
/// 会使该项保持 pending，调用方不会把已经过期的路径送入 worker。
pub fn mark_derivations_processing(
    conn: &Connection,
    tasks: &[DerivationClaim],
) -> Result<Vec<DerivationClaim>> {
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.unchecked_transaction()?;
    let mut claimed = Vec::with_capacity(tasks.len());
    for (item_id, kind, source_revision, cache_key) in tasks {
        let changed = tx.execute(
            "UPDATE media_derivations SET status=1, updated_at=strftime('%s','now')
             WHERE item_id=?1 AND kind=?2 AND status=0
               AND EXISTS (
                   SELECT 1 FROM media_items m
                   WHERE m.id=?1 AND m.is_deleted=0
                     AND m.source_revision=?3 AND m.cache_key=?4
               )",
            params![item_id, kind, source_revision, cache_key],
        )?;
        if changed == 1 {
            claimed.push((*item_id, kind.clone(), *source_revision, *cache_key));
        }
    }
    tx.commit()?;
    Ok(claimed)
}

/// 单 `(item, kind)` 按需入队 + 立即 claim(播放交互路径,§5.2)。`INSERT OR IGNORE` 建待处理行
/// (若无),再 `UPDATE … status=1` 领取——使 `VideoWorkerService` 立刻跑,不等背景流水线 tick。
/// 幂等;行已在处理中(status=1)时重复调用无害。仅 `video_playable` 用(绝不 backfill)。
/// 后续经 `batch_finish_derivations` 转 2/3。
///
/// 返回**是否发生真实认领**(TOCTOU 去重,§V6-2):`UPDATE … WHERE status != 1` 只把 0/2/3 行
/// 转 1,`changes()` > 0 即本次调用完成 `→1` 转移、调用方应 spawn 派活;=0 说明行已是 status=1
/// (另一并发 resolve 抢先认领),调用方不再重复 spawn、直接回 preparing。原子事务内判定,
/// 杜绝「先 get_derivation_state 见非在途 → 再 claim」两步之间的竞态双 spawn。
pub fn upsert_and_claim_derivation(conn: &Connection, item_id: i64, kind: &str) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO media_derivations (item_id, kind, status) VALUES (?1, ?2, 0)",
        params![item_id, kind],
    )?;
    // status != 1 守卫:已在途行不被本次 UPDATE 命中(changed=0),即并发第二方不夺认领。
    let changed = tx.execute(
        "UPDATE media_derivations SET status=1, updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND status != 1",
        params![item_id, kind],
    )?;
    tx.commit()?;
    Ok(changed > 0)
}

/// 单 `(item, kind)` 行的当前 `(status, payload_path)`,无行则 `None`。供播放解析区分
/// 已就绪(status=2+payload)/ 在途(status=1)/ 待产出(None 或 0/3)。
pub fn get_derivation_state(
    conn: &Connection,
    item_id: i64,
    kind: &str,
) -> Result<Option<(i64, Option<String>)>> {
    conn.query_row(
        "SELECT status, payload_path FROM media_derivations WHERE item_id=?1 AND kind=?2",
        params![item_id, kind],
        |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .map_err(AppError::from)
}

/// 单个派生的结果：`(item_id, kind, status, payload_path, error, thumbhash, page_count)`。
/// `thumbhash` 仅封面类 kind 为 `Some`（写入器回填 `media_items`）；`page_count` 仅 epub `doc_thumb`
/// 为 `Some`（写入器 upsert 进 `document_meta`，§3.8.2 / T10）。两者均不存入 `media_derivations`。
pub type DerivationResultRow = (
    i64,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
    Option<i64>,
);

/// 带源快照的后台派生结果：旧 worker 只能完成它实际读取的那一代源项。
///
/// 字段前 7 项与 [`DerivationResultRow`] 保持相同顺序，末尾追加
/// `(source_revision, cache_key)`，便于现有交互调用者继续使用旧 API，而通用后台流水线
/// 使用快照版完成函数。
pub type DerivationResultWithSnapshot = (
    i64,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
    Option<i64>,
    i64,
    i64,
);

/// 在单个事务中批量写入派生结果（状态 + 产物路径 + 错误）。
/// `thumbhash`（元组第 6 项）此处有意忽略 —— 封面另经 `update_thumb_result` 回填到 `media_items`。
pub fn batch_finish_derivations(conn: &Connection, results: &[DerivationResultRow]) -> Result<()> {
    if results.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for (item_id, kind, status, payload_path, error, _thumbhash, _page_count) in results {
        // `orphan_count=0`（2026-07-22 毒任务防线）：任务经正常流水线路径写下结果（无论成功
        // status=2 还是常规失败 status=3），说明它这次没有卡死/挂死——归零孤儿计数，防止「偶发
        // 崩溃累计三次」把一个大体健康、只是偶尔倒霉的任务误判为毒任务并永久转 error。
        tx.execute(
            "UPDATE media_derivations
             SET status=?3, payload_path=?4, error=?5, orphan_count=0, updated_at=strftime('%s','now')
             WHERE item_id=?1 AND kind=?2",
            params![item_id, kind, status, payload_path, error],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 在单个事务中完成带源快照的后台派生结果，并返回实际接受的结果。
///
/// 认领后源项可能已经被扫描为新代次，或者任务行可能已经被停止/重置。只有当派生行仍
/// 为 `status=1` 且关联 `media_items` 的 `(source_revision, cache_key)` 与 worker 快照同时
/// 匹配时，才会写入状态和 payload。封面回填、epub 文档元数据也在同一事务中受同一条件
/// 保护；调用方只能把返回的 accepted 结果应用到常驻缓存。
pub fn batch_finish_derivations_with_snapshot(
    conn: &Connection,
    results: &[DerivationResultWithSnapshot],
) -> Result<Vec<DerivationResultWithSnapshot>> {
    if results.is_empty() {
        return Ok(Vec::new());
    }

    let tx = conn.unchecked_transaction()?;
    let mut accepted = Vec::with_capacity(results.len());
    for (
        item_id,
        kind,
        status,
        payload_path,
        error,
        thumbhash,
        page_count,
        source_revision,
        cache_key,
    ) in results
    {
        // status=1 是认领代次的最小保护；source_revision/cache_key 是源内容保护。
        let changed = tx.execute(
            "UPDATE media_derivations
             SET status=?3, payload_path=?4, error=?5, orphan_count=0,
                 updated_at=strftime('%s','now')
             WHERE item_id=?1 AND kind=?2 AND status=1
               AND EXISTS (
                   SELECT 1 FROM media_items m
                   WHERE m.id=?1 AND m.is_deleted=0
                     AND m.source_revision=?6 AND m.cache_key=?7
               )",
            params![
                item_id,
                kind,
                status,
                payload_path,
                error,
                source_revision,
                cache_key
            ],
        )?;
        if changed != 1 {
            continue;
        }

        // 封面是派生状态的第二个落点。与上面的状态更新在同一事务中再次带快照条件，
        // 防止未来有人把两段 SQL 拆开后重新引入 stale cover 回填。
        if *status == 2 && produces_cover(kind) {
            tx.execute(
                "UPDATE media_items
                 SET thumb_status=1, thumb_path=?2, thumbhash=?3,
                     updated_at=strftime('%s','now')
                 WHERE id=?1 AND is_deleted=0
                   AND source_revision=?4 AND cache_key=?5",
                params![item_id, payload_path, thumbhash, source_revision, cache_key],
            )?;
        }

        // 后端 doc_thumb 只处理 epub；沿用原 writer 的 document_meta 写入语义，但把它
        // 放在受快照保护的同一事务里，避免旧 epub 任务覆盖新源的页数。
        if *status == 2 && kind == "doc_thumb" {
            if let Some(page_count) = page_count {
                super::documents::upsert_document_meta(
                    &tx,
                    *item_id,
                    Some(*page_count),
                    Some(crate::utils::format::doc_subtype("epub")),
                )?;
            }
        }

        accepted.push((
            *item_id,
            kind.clone(),
            *status,
            payload_path.clone(),
            error.clone(),
            thumbhash.clone(),
            *page_count,
            *source_revision,
            *cache_key,
        ));
    }
    tx.commit()?;
    Ok(accepted)
}

fn produces_cover(kind: &str) -> bool {
    matches!(kind, "video_cover" | "audio_cover" | "doc_thumb")
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, source_revision) VALUES
                 (1, 10, 'a.mp4', 1, 1, 'mp4', 'video', 0, 0, 100, 17, 1),
                 (2, 10, 'b.epub', 1, 1, 'epub', 'document', 0, 0, 200, 18, 1);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'video_cover', 0),
                 (2, 'doc_thumb', 1);",
        )
        .unwrap();
        c
    }

    #[test]
    fn pending_task_carries_the_media_source_snapshot() {
        let c = seeded();
        c.execute(
            "UPDATE media_items SET source_revision=7, cache_key=71 WHERE id=1",
            [],
        )
        .unwrap();

        let tasks = get_pending_derivations(&c, 10, None, &[]).unwrap();
        assert_eq!(
            tasks,
            vec![(
                1,
                "video_cover".to_string(),
                "/r/a.mp4".to_string(),
                "mp4".to_string(),
                "video".to_string(),
                7,
                71,
            )],
            "pending task must preserve source_revision and cache_key from the same row"
        );
    }

    #[test]
    fn claim_rejects_a_stale_source_snapshot_and_returns_real_claims() {
        let c = seeded();
        c.execute("UPDATE media_items SET source_revision=2 WHERE id=1", [])
            .unwrap();

        let stale = vec![(1, "video_cover".to_string(), 1, 17)];
        assert!(
            mark_derivations_processing(&c, &stale).unwrap().is_empty(),
            "a task queried from an older source revision must not be claimed"
        );
        assert_eq!(
            c.query_row(
                "SELECT status FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );

        let current = vec![(1, "video_cover".to_string(), 2, 17)];
        assert_eq!(
            mark_derivations_processing(&c, &current).unwrap(),
            current,
            "the current snapshot should be the only real claim"
        );
        assert_eq!(
            c.query_row(
                "SELECT status FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn stale_finish_cannot_update_derivation_cover_or_document_meta() {
        let c = seeded();
        assert_eq!(
            mark_derivations_processing(&c, &[(1, "video_cover".to_string(), 1, 17)]).unwrap(),
            vec![(1, "video_cover".to_string(), 1, 17)],
            "the stale-result fixture must start from a real claimed task"
        );
        c.execute("UPDATE media_items SET source_revision=3 WHERE id=1", [])
            .unwrap();

        let stale_cover = vec![(
            1,
            "video_cover".to_string(),
            2,
            Some("stale.webp".to_string()),
            None,
            Some(vec![1, 2, 3]),
            None,
            1,
            17,
        )];
        assert!(
            batch_finish_derivations_with_snapshot(&c, &stale_cover)
                .unwrap()
                .is_empty(),
            "old source_revision must be discarded"
        );
        assert_eq!(
            c.query_row(
                "SELECT status, payload_path FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (1, None),
            "stale completion must not alter the claimed row"
        );
        assert_eq!(
            c.query_row(
                "SELECT thumb_status, thumb_path, thumbhash FROM media_items WHERE id=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .unwrap(),
            (0, None, None),
            "stale completion must not mirror a cover"
        );

        // 即使代次相同，缓存键变化也会使旧产物失效；两部分快照必须同时匹配。
        c.execute("UPDATE media_items SET cache_key=18 WHERE id=1", [])
            .unwrap();
        let stale_cache_key = vec![(
            1,
            "video_cover".to_string(),
            2,
            Some("stale-cache-key.webp".to_string()),
            None,
            Some(vec![7, 8, 9]),
            None,
            3,
            17,
        )];
        assert!(
            batch_finish_derivations_with_snapshot(&c, &stale_cache_key)
                .unwrap()
                .is_empty(),
            "same source_revision with an old cache_key must also be discarded"
        );

        let current_cover = vec![(
            1,
            "video_cover".to_string(),
            2,
            Some("current.webp".to_string()),
            None,
            Some(vec![4, 5, 6]),
            None,
            3,
            18,
        )];
        assert_eq!(
            batch_finish_derivations_with_snapshot(&c, &current_cover)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT status, payload_path FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (2, Some("current.webp".to_string()))
        );
        assert_eq!(
            c.query_row(
                "SELECT thumb_status, thumb_path, thumbhash FROM media_items WHERE id=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .unwrap(),
            (1, Some("current.webp".to_string()), Some(vec![4, 5, 6]))
        );

        c.execute_batch(
            "INSERT INTO document_meta (item_id, page_count, doc_subtype) VALUES (2, 7, 'epub');
             UPDATE media_items SET source_revision=2, cache_key=19 WHERE id=2;",
        )
        .unwrap();
        let stale_doc = vec![(
            2,
            "doc_thumb".to_string(),
            2,
            Some("stale-epub.webp".to_string()),
            None,
            None,
            Some(41),
            1,
            18,
        )];
        assert!(
            batch_finish_derivations_with_snapshot(&c, &stale_doc)
                .unwrap()
                .is_empty(),
            "stale epub completion must not update document metadata"
        );
        assert_eq!(
            c.query_row(
                "SELECT status FROM media_derivations WHERE item_id=2 AND kind='doc_thumb'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT page_count, doc_subtype FROM document_meta WHERE item_id=2",
                [],
                |row| Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?
                )),
            )
            .unwrap(),
            (Some(7), Some("epub".to_string()))
        );

        let current_doc = vec![(
            2,
            "doc_thumb".to_string(),
            2,
            Some("epub.webp".to_string()),
            None,
            None,
            Some(42),
            2,
            19,
        )];
        assert_eq!(
            batch_finish_derivations_with_snapshot(&c, &current_doc)
                .unwrap()
                .len(),
            1,
            "accepted epub result should finish in the same snapshot-guarded path"
        );
        assert_eq!(
            c.query_row(
                "SELECT page_count, doc_subtype FROM document_meta WHERE item_id=2",
                [],
                |row| Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?
                )),
            )
            .unwrap(),
            (Some(42), Some("epub".to_string()))
        );
    }
}

/// 将孤儿派生任务（崩溃/暂停/停止遗留的 status=1）恢复为待处理（status=0），使下次运行续传。
/// 返回恢复的数量。
///
/// 毒任务防线（2026-07-22 故障复盘，V22 `orphan_count` 列）：某 (item,kind) 反复卡在
/// status=1（如引发 MF 硬解挂死的 mkv），此前无条件退回 status=0 会使其每次启动都被同一批
/// 复现领取、无限循环（本次故障 125 个孤儿循环 6+ 轮）。现先把已达阈值
/// （`orphan_count>=2`，第 3 次孤儿当轮转 error）的行直接判定为毒任务、转 status=3（error）而非
/// 再投入领取；其余孤儿正常退回 status=0 并令 `orphan_count+1`。error 文案不含内部路径。
/// 返回值签名不变（仍是本次复位为 pending 的数量），调用方（derive/pipeline.rs）无感知本次改动。
pub fn reset_processing_derivations(conn: &Connection) -> Result<usize> {
    // video_playable 是按需交互 kind(§5.2),从不经背景流水线,也无孤儿/毒任务语义——
    // 两条复位 UPDATE 均排除它,避免 boot/stop 的通用复位误动在途播放行(其复位归专用路径)。
    let poisoned = conn.execute(
        "UPDATE media_derivations
         SET status=3, error='repeatedly orphaned (poison guard) | 反复孤儿(毒任务防线)', updated_at=strftime('%s','now')
         WHERE status=1 AND orphan_count>=2 AND kind != 'video_playable'",
        [],
    )?;
    // 日志按仓内惯例应归调用方统一记录（derive/pipeline.rs 的流水线事件日志），但该文件另一
    // 批次正并行送审、本次施工按计划冻结不动该文件——此处内联 `tracing::warn!` 是有意的临时
    // 偏离，注明供 V22 收束时上提至调用方。
    if poisoned > 0 {
        tracing::warn!(
            count = poisoned,
            "派生任务反复孤儿超过重试上限,已转 error(毒任务防线) | derivation tasks exceeded orphan retry limit, marked error (poison guard)"
        );
    }
    conn.execute(
        "UPDATE media_derivations SET status=0, orphan_count=orphan_count+1 WHERE status=1 AND kind != 'video_playable'",
        [],
    )
    .map_err(AppError::from)
}

/// 优雅退回:用户主动停止流水线时，把当前仍在途（status=1）的行退回待处理（status=0），
/// **不**递增 `orphan_count`。返回退回的行数。
///
/// 语义分工（裁决 J10，2026-07-23）：主动 stop 是良性中断——worker 已在停止路径里全部静默
/// 退出，在途任务只是「来不及写完结果」，并非挂死/崩溃。此前 `reset_processing_derivations`
/// 对所有 status=1 一视同仁计孤儿数，导致用户短时间内多次 stop/start 同一批任务时，
/// 良性中断被反复计数、误触第 3 次孤儿转 error 的毒任务防线。现与「启动时恢复崩溃/
/// force-quit 遗留孤儿」（`reset_processing_derivations`，计数正当——那类挂死本就该占用
/// 有限重试预算）拆成两条独立路径：本函数只在优雅 stop 时调用，不吃孤儿预算。
pub fn requeue_in_flight_derivations(conn: &Connection) -> Result<usize> {
    // video_playable 排除(§V6-1):按需交互 kind 不属背景流水线 stop 的退回范围。
    conn.execute(
        "UPDATE media_derivations SET status=0 WHERE status=1 AND kind != 'video_playable'",
        [],
    )
    .map_err(AppError::from)
}

/// 把指定 kind 的**已完成(status=2)或失败(status=3)**派生行退回 pending(status=0)并清 payload_path。
/// 2026-07-06 审查 P1-4:清理缓存/GC 删掉派生产物文件后,若 derivation 行仍 status=2,则
/// backfill(INSERT OR IGNORE)不再入队、读取侧返回死路径 → 永不重建。删文件必须同步退状态。
/// 2026-07-07:并入 status=3(失败)。用户显式清缓存=「重新生成」意图,失败项理应一并重试——
/// 否则被瞬时/环境性失败(如 CSP 拦截 pdf.js 抓取)误标 status=3 的好文件将永久卡占位符、无自愈
/// 路径(唯一调用方是 clear_cache;真坏文件重试后再失败回 3,一次重试非死循环,安全)。
/// 2026-07-22(V22 毒任务防线复核裁定):显式清缓存重生成=用户人工点名重试,一并清
/// `orphan_count` 给全新的 3 次孤儿预算——显式意图覆盖自动防线(与 D-002 显式 kinds
/// 覆盖 enable_* 同哲学);否则曾判毒的行复位后一挂即被重新判毒,只得 1 次机会。
/// `kinds` 为空则不动;返回受影响行数。
pub fn reset_derivations_by_kinds(conn: &Connection, kinds: &[&str]) -> Result<usize> {
    if kinds.is_empty() {
        return Ok(0);
    }
    let placeholders: Vec<String> = (1..=kinds.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "UPDATE media_derivations SET status=0, payload_path=NULL, orphan_count=0 \
         WHERE status IN (2, 3) AND kind IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        kinds.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    conn.execute(&sql, params.as_slice())
        .map_err(AppError::from)
}

/// 单个 item 的某 kind 派生行退回 pending(读取侧发现产物文件缺失时的自愈,P1-4)。
pub fn reset_derivation_for_item(conn: &Connection, item_id: i64, kind: &str) -> Result<usize> {
    conn.execute(
        "UPDATE media_derivations SET status=0, payload_path=NULL WHERE item_id=?1 AND kind=?2",
        params![item_id, kind],
    )
    .map_err(AppError::from)
}

/// 取消播放准备时**只作废在途**(status=1)行退回 pending(§V6-7):已就绪(status=2)产物
/// 不退——用户取消的是「正在准备」的那次派生,不该殃及已缓存可播的产物行。返回受影响行数。
pub fn reset_in_flight_derivation_for_item(
    conn: &Connection,
    item_id: i64,
    kind: &str,
) -> Result<usize> {
    conn.execute(
        "UPDATE media_derivations SET status=0, payload_path=NULL WHERE item_id=?1 AND kind=?2 AND status=1",
        params![item_id, kind],
    )
    .map_err(AppError::from)
}

/// 启动复位(§V6-11):把某 kind 的**全部在途**(status=1)行退回 pending。boot 时进程内无任何
/// 真实在途 job(video_playable 的派活在 host 侧 Service 内存队列,随进程退出而灭),故 status=1
/// 必是上次退出遗留的假在途,退回后下次 resolve 按需重触发。返回受影响行数。
pub fn reset_in_flight_derivations_for_kind(conn: &Connection, kind: &str) -> Result<usize> {
    conn.execute(
        "UPDATE media_derivations SET status=0, payload_path=NULL WHERE kind=?1 AND status=1",
        params![kind],
    )
    .map_err(AppError::from)
}

/// 为 `media_type`（可选限定 `formats`）下所有缺少 `(item, kind)` 行的未删除项插入待处理派生行
/// （`INSERT OR IGNORE`）。返回插入的行数。这是独立派生表所需的显式「入队」步骤
/// （AI 通过 `ai_status` 列天然免费获得）。
pub fn backfill_derivations(
    conn: &Connection,
    kind: &str,
    media_type: &str,
    formats: Option<&[&str]>,
) -> Result<usize> {
    let inserted = if let Some(fmts) = formats {
        if fmts.is_empty() {
            return Ok(0);
        }
        // 将格式作为参数绑定（?3、?4…）—— 绝不拼接值。
        let placeholders: Vec<String> = (3..3 + fmts.len()).map(|i| format!("?{i}")).collect();
        // §6.3：exotic 已认领 thumbnail 的媒体不建主派生封面任务（NOT_BLOCKED_BY_EXOTIC）。
        let sql = format!(
            "INSERT OR IGNORE INTO media_derivations (item_id, kind, status)
             SELECT id, ?1, 0 FROM media_items
             WHERE media_type = ?2 AND is_deleted = 0 AND file_format IN ({}) {NOT_BLOCKED_BY_EXOTIC}",
            placeholders.join(",")
        );
        let mut sql_params: Vec<&dyn rusqlite::ToSql> = vec![
            &kind as &dyn rusqlite::ToSql,
            &media_type as &dyn rusqlite::ToSql,
        ];
        for f in fmts {
            sql_params.push(f as &dyn rusqlite::ToSql);
        }
        conn.execute(&sql, sql_params.as_slice())?
    } else {
        let sql = format!(
            "INSERT OR IGNORE INTO media_derivations (item_id, kind, status)
             SELECT id, ?1, 0 FROM media_items
             WHERE media_type = ?2 AND is_deleted = 0 {NOT_BLOCKED_BY_EXOTIC}"
        );
        conn.execute(&sql, params![kind, media_type])?
    };
    Ok(inserted)
}

/// 某视频的关键帧雪碧图产物路径（相对 `cache_dir`），若已生成（status=2）。用于悬停 scrub 降级（§3.1 / §3.3）。
pub fn get_keyframe_sprite_payload(conn: &Connection, item_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT payload_path FROM media_derivations
         WHERE item_id = ?1 AND kind = 'video_keyframes' AND status = 2",
        params![item_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|o| o.flatten())
    .map_err(AppError::from)
}

/// 前端离屏渲染文档缩略图的格式白名单——`store_doc_thumbnail` IPC 守卫与下方
/// `list_pending_doc_thumbs` 的 `file_format IN (...)` 共用**同一集合**(epub 走后端 zip,
/// 有意排除,不在此列)。该 SQL 的 IN 子句由本常量单源生成,消除「pump 入队某格式却被
/// 守卫拒收」的双向漂移。改动此集合前先读 §3.4 Lite 路径约定。
pub const FRONTEND_DOC_THUMB_FORMATS: &[&str] = &["pdf", "svg"];

/// `file_format` 是否属前端可渲染的文档缩略图格式(pdf/svg)。
pub fn is_frontend_doc_thumb_format(file_format: &str) -> bool {
    FRONTEND_DOC_THUMB_FORMATS.contains(&file_format)
}

/// `store_doc_thumbnail` IPC 守卫的可测决策核心:校验 `item_id` 指向的项确为前端可渲染
/// 文档(pdf/svg),命中则返回其 `file_format`。x-item-id 来自请求头**不可信**——拿非
/// doc-thumb 项(.txt/.jpg/epub/mp4 等)的 id 调用本命令会污染其 doc_thumb 派生态与
/// document_meta,故非白名单格式一律 `AppError::Internal` 早退;item 不存在返回 `MediaNotFound`。
pub fn validate_frontend_doc_thumb_item(conn: &Connection, item_id: i64) -> Result<String> {
    let fmt = get_item_file_format(conn, item_id)?.ok_or(AppError::MediaNotFound(item_id))?;
    if !is_frontend_doc_thumb_format(&fmt) {
        return Err(AppError::Internal(format!(
            "store_doc_thumbnail: item {item_id} file_format='{fmt}' 非前端文档缩略图格式(仅 pdf/svg),拒绝 | non-doc-thumb item rejected"
        )));
    }
    Ok(fmt)
}

/// 列出等待「前端渲染」缩略图的文档（pdf/svg，§3.4 Lite 路径）：后端无法栅格化的待处理
/// `doc_thumb` 行。返回 `(item_id, 绝对路径, 格式)`；前端逐个离屏渲染后经 `store_doc_thumbnail`
/// 回传字节。epub 有意排除（由后端处理）。
pub fn list_pending_doc_thumbs(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<(i64, String, String)>> {
    // 隐藏根排除(V21):pdf/svg 前端泵同样跳过隐藏根;unhide 后由下次泵触发
    // (app 启动首泵 / 可见性变化 / 任意 db:media_enriched)续跑。
    // IN 子句由 FRONTEND_DOC_THUMB_FORMATS 单源生成(编译期 ASCII 常量,非用户输入,无注入
    // 风险;与既有 {EXCLUDE_HIDDEN_ROOTS_M} 常量插值同法),与 store_doc_thumbnail 守卫共用白名单。
    let format_in = FRONTEND_DOC_THUMB_FORMATS
        .iter()
        .map(|f| format!("'{f}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "
        SELECT dv.item_id,
               CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                    ELSE r.path || '/' || d.rel_path || '/' || m.file_name
               END,
               m.file_format
        FROM media_derivations dv
        JOIN media_items m ON dv.item_id = m.id
        JOIN directories d ON m.directory_id = d.id
        JOIN scan_roots r ON d.root_id = r.id
        WHERE dv.kind = 'doc_thumb' AND dv.status = 0 AND m.is_deleted = 0
          AND m.file_format IN ({format_in})
          {EXCLUDE_HIDDEN_ROOTS_M}
        ORDER BY dv.item_id LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按状态聚合派生计数：`(待处理, 处理中, 完成, 错误)`。
///
/// 注(§V6-1):本无过滤聚合会**连带计入** video_playable 行(按需交互 kind,非背景流水线产物)。
/// 用户面视频进度计数走 [`count_derivations_by_status_for_kinds`] 传显式 kinds(cover/keyframes),
/// 天然把 video_playable 排除在外;此不带过滤的全量计数仅供整体诊断,语义不变、无需在此排除。
pub fn count_derivations_by_status(conn: &Connection) -> Result<(i64, i64, i64, i64)> {
    conn.query_row(
        "SELECT
            COALESCE(SUM(status=0),0),
            COALESCE(SUM(status=1),0),
            COALESCE(SUM(status=2),0),
            COALESCE(SUM(status=3),0)
         FROM media_derivations",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .map_err(AppError::from)
}

/// 同一聚合但限定一组 kind——视频控制卡的进度计数不能混入文档/音频/AI 派生。
/// `kinds` 为空退化为全量计数。
pub fn count_derivations_by_status_for_kinds(
    conn: &Connection,
    kinds: &[String],
) -> Result<(i64, i64, i64, i64)> {
    if kinds.is_empty() {
        return count_derivations_by_status(conn);
    }
    let placeholders: Vec<String> = (1..=kinds.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT
            COALESCE(SUM(status=0),0),
            COALESCE(SUM(status=1),0),
            COALESCE(SUM(status=2),0),
            COALESCE(SUM(status=3),0)
         FROM media_derivations WHERE kind IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        kinds.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    conn.query_row(&sql, params.as_slice(), |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .map_err(AppError::from)
}

#[cfg(test)]
mod reset_derivations_tests {
    use super::*;

    /// 建全量 schema + 5 个 item + 各类 (kind, status) 派生行,覆盖复位边界。
    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a', 1, 1, 'pdf', 'image', 0, 0, 100, 0),
                 (2, 10, 'b', 1, 1, 'pdf', 'image', 0, 0, 200, 0),
                 (3, 10, 'c', 1, 1, 'pdf', 'image', 0, 0, 300, 0),
                 (4, 10, 'd', 1, 1, 'pdf', 'image', 0, 0, 400, 0),
                 (5, 10, 'e', 1, 1, 'mp4', 'video', 0, 0, 500, 0);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'doc_thumb', 3),
                 (2, 'doc_thumb', 2),
                 (3, 'doc_thumb', 0),
                 (4, 'doc_thumb', 1),
                 (5, 'video_cover', 3);",
        )
        .unwrap();
        c
    }

    fn status_of(c: &Connection, item_id: i64, kind: &str) -> i64 {
        c.query_row(
            "SELECT status FROM media_derivations WHERE item_id=?1 AND kind=?2",
            params![item_id, kind],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 清缓存复位须同时覆盖 status=2(已完成)与 status=3(失败)——后者是 2026-07-07 修复:
    /// 被瞬时/环境性失败(如 CSP 拦 pdf.js 抓取)误标 status=3 的好文件须能经清缓存重试,
    /// 否则永久卡占位符、无自愈路径。status=0/1 与未列入 kinds 的其他 kind 一律不动。
    #[test]
    fn reset_covers_done_and_failed_but_not_pending_or_other_kind() {
        let c = seeded();
        let affected = reset_derivations_by_kinds(&c, &["doc_thumb"]).unwrap();
        assert_eq!(affected, 2, "仅 status=2/3 的 doc_thumb 两行受影响");
        assert_eq!(
            status_of(&c, 1, "doc_thumb"),
            0,
            "失败(3)→退回 0(本次修复核心)"
        );
        assert_eq!(status_of(&c, 2, "doc_thumb"), 0, "已完成(2)→退回 0");
        assert_eq!(status_of(&c, 3, "doc_thumb"), 0, "待处理(0)保持 0");
        assert_eq!(status_of(&c, 4, "doc_thumb"), 1, "处理中(1)不动");
        assert_eq!(
            status_of(&c, 5, "video_cover"),
            3,
            "未列入 kinds 的其他 kind 不动"
        );
    }
}

// ── 派生流水线毒任务防线(V22 orphan_count,2026-07-22 故障复盘)──────────────────
#[cfg(test)]
mod poison_guard_tests {
    use super::*;

    /// 建单个 item + 一行 status=1 的派生任务,用于逐轮驱动 `reset_processing_derivations`。
    fn one_processing_task() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'poison.mkv', 1, 1, 'mkv', 'video', 0, 0, 100, 0);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'video_cover', 1);",
        )
        .unwrap();
        c
    }

    fn status_and_orphan(c: &Connection) -> (i64, i64) {
        c.query_row(
            "SELECT status, orphan_count FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// ①同一毒任务反复孤儿(每轮领取后又挂死留在 status=1)达到第 3 次,应转 status=3(error)
    /// 而非再退回 status=0 重新领取;此后再调用 `reset_processing_derivations` 也不应把它复位。
    #[test]
    fn poisoned_task_converts_to_error_on_third_orphan_and_stays_there() {
        let c = one_processing_task();

        // 第 1 轮孤儿:orphan_count 0→1,退回 pending(未达阈值)。
        let n = reset_processing_derivations(&c).unwrap();
        assert_eq!(n, 1, "第 1 轮:1 行退回 pending");
        assert_eq!(
            status_and_orphan(&c),
            (0, 1),
            "第 1 轮:status=0, orphan_count=1"
        );

        // 模拟重新领取(mark processing)后再次挂死。
        c.execute(
            "UPDATE media_derivations SET status=1 WHERE item_id=1 AND kind='video_cover'",
            [],
        )
        .unwrap();
        let n = reset_processing_derivations(&c).unwrap();
        assert_eq!(n, 1, "第 2 轮:仍未达阈值,退回 pending");
        assert_eq!(
            status_and_orphan(&c),
            (0, 2),
            "第 2 轮:status=0, orphan_count=2"
        );

        // 第 3 次领取后再挂死:orphan_count 已是 2(>=2),本轮孤儿复位应判定为毒任务转 error。
        c.execute(
            "UPDATE media_derivations SET status=1 WHERE item_id=1 AND kind='video_cover'",
            [],
        )
        .unwrap();
        let n = reset_processing_derivations(&c).unwrap();
        assert_eq!(n, 0, "第 3 轮:未再有行退回 pending(已转 error 分支)");
        let (status, orphan) = status_and_orphan(&c);
        assert_eq!(status, 3, "第 3 次孤儿→转 status=3(error)");
        assert_eq!(orphan, 2, "转 error 分支不改 orphan_count");
        let err: String = c
            .query_row(
                "SELECT error FROM media_derivations WHERE item_id=1 AND kind='video_cover'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            err.contains("poison guard"),
            "error 文案应标明毒任务防线,实得:{err}"
        );

        // 已转 error 后,即使再次调用复位也不应被误复位为 pending(status!=1,两条 UPDATE 均不命中)。
        let n = reset_processing_derivations(&c).unwrap();
        assert_eq!(n, 0, "已 error 的行不应被再次复位");
        assert_eq!(status_and_orphan(&c).0, 3, "status 保持 error,不被复位覆盖");
    }

    /// ②孤儿复位后若任务经正常路径完成(`batch_finish_derivations` 写下 status=2/3),
    /// orphan_count 应归零——防止偶发崩溃累计的孤儿计数,把后续真正偶发的失败误判为毒任务。
    #[test]
    fn orphan_count_resets_to_zero_on_normal_finish() {
        let c = one_processing_task();

        // 先经历一轮孤儿复位,使 orphan_count 变为非零(模拟此前有过一次崩溃)。
        reset_processing_derivations(&c).unwrap();
        assert_eq!(
            status_and_orphan(&c),
            (0, 1),
            "预置:一轮孤儿后 orphan_count=1"
        );

        // 模拟重新领取后本次正常完成:写结果 status=2。
        c.execute(
            "UPDATE media_derivations SET status=1 WHERE item_id=1 AND kind='video_cover'",
            [],
        )
        .unwrap();
        batch_finish_derivations(
            &c,
            &[(
                1,
                "video_cover".to_string(),
                2,
                Some("cover.webp".to_string()),
                None,
                None,
                None,
            )],
        )
        .unwrap();

        let (status, orphan) = status_and_orphan(&c);
        assert_eq!(status, 2, "正常完成→status=2");
        assert_eq!(orphan, 0, "正常完成归零 orphan_count,防偶发崩溃累计误杀");
    }

    /// ③优雅 stop 路径(`requeue_in_flight_derivations`)三轮反复退回同一行,orphan_count
    /// 全程不涨、不应转 status=3——裁决 J10:主动 stop 是良性中断,不吃孤儿预算,防止用户
    /// 短时间多次 stop/start 把良性中断误判为毒任务。
    #[test]
    fn requeue_in_flight_does_not_increment_orphan_count_across_three_rounds() {
        let c = one_processing_task();

        for round in 1..=3 {
            // 每轮先确认行处于在途(status=1),再走优雅退回。
            c.execute(
                "UPDATE media_derivations SET status=1 WHERE item_id=1 AND kind='video_cover'",
                [],
            )
            .unwrap();
            let n = requeue_in_flight_derivations(&c).unwrap();
            assert_eq!(n, 1, "第 {round} 轮:1 行退回 pending");
            assert_eq!(
                status_and_orphan(&c),
                (0, 0),
                "第 {round} 轮:优雅退回后 status=0 且 orphan_count 全程为 0"
            );
        }
    }
}

// ── 多 kind 过滤 + 分 kind 计数(视频封面/关键帧独立控制线)────────────────────
#[cfg(test)]
mod kind_filter_tests {
    use super::*;

    /// 三 kind 混布:视频 1/2 各有 cover+keyframes 行(状态混合),音频 3 有 audio_cover 行。
    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.mp4', 1, 1, 'mp4', 'video', 0, 0, 100, 0),
                 (2, 10, 'b.mp4', 1, 1, 'mp4', 'video', 0, 0, 200, 0),
                 (3, 10, 'c.mp3', 1, 1, 'mp3', 'audio', 0, 0, 300, 0);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'video_cover', 0),
                 (1, 'video_keyframes', 0),
                 (2, 'video_cover', 2),
                 (2, 'video_keyframes', 3),
                 (3, 'audio_cover', 0);",
        )
        .unwrap();
        c
    }

    #[test]
    fn multi_kind_filter_returns_only_listed_kinds() {
        let c = seeded();
        let video_kinds = vec!["video_cover".to_string(), "video_keyframes".to_string()];
        let got: Vec<(i64, String)> = get_pending_derivations(&c, 100, Some(&video_kinds), &[])
            .unwrap()
            .into_iter()
            .map(|(id, kind, ..)| (id, kind))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, "video_cover".to_string()),
                (1, "video_keyframes".to_string())
            ],
            "只返回过滤集内的待处理行,audio_cover 与非 pending 行不出现"
        );
        // 空过滤集 = 无过滤(绝不能生成 IN ())。
        assert_eq!(
            get_pending_derivations(&c, 100, Some(&Vec::new()), &[])
                .unwrap()
                .len(),
            3,
            "空切片视同无过滤"
        );
    }

    #[test]
    fn kind_scoped_counts_exclude_other_kinds() {
        let c = seeded();
        let video_kinds = vec!["video_cover".to_string(), "video_keyframes".to_string()];
        assert_eq!(
            count_derivations_by_status_for_kinds(&c, &video_kinds).unwrap(),
            (2, 0, 1, 1),
            "视频两 kind:pending=2(item1 两行) done=1 error=1;audio_cover 不计入"
        );
        assert_eq!(
            count_derivations_by_status_for_kinds(&c, &[]).unwrap(),
            count_derivations_by_status(&c).unwrap(),
            "空 kinds 退化为全量计数"
        );
    }
}

// ── 按需 video_playable 状态机(播放交互路径,§5.2)────────────────────────────
#[cfg(test)]
mod video_playable_tests {
    use super::*;

    fn one_video() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.mkv', 1, 1, 'mkv', 'video', 0, 0, 100, 42);",
        )
        .unwrap();
        c
    }

    /// upsert+claim 从无行 → status=1;重复调用幂等(仍 1);finish → status=2 + payload;
    /// get_derivation_state 全程反映当前态。
    #[test]
    fn upsert_claim_finish_lifecycle() {
        let c = one_video();
        // 初始无行。
        assert_eq!(get_derivation_state(&c, 1, "video_playable").unwrap(), None);

        // 首次 upsert+claim:建行并领取。
        upsert_and_claim_derivation(&c, 1, "video_playable").unwrap();
        assert_eq!(
            get_derivation_state(&c, 1, "video_playable").unwrap(),
            Some((1, None)),
            "首次 → status=1(processing),无 payload"
        );

        // 幂等:再调不新建、保持 processing。
        upsert_and_claim_derivation(&c, 1, "video_playable").unwrap();
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM media_derivations WHERE item_id=1 AND kind='video_playable'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "幂等:不重复建行");
        assert_eq!(
            get_derivation_state(&c, 1, "video_playable").unwrap(),
            Some((1, None))
        );

        // 完成:status=2 + payload(复用通用 batch_finish)。
        batch_finish_derivations(
            &c,
            &[(
                1,
                "video_playable".to_string(),
                2,
                Some("video/42.mp4".to_string()),
                None,
                None,
                None,
            )],
        )
        .unwrap();
        assert_eq!(
            get_derivation_state(&c, 1, "video_playable").unwrap(),
            Some((2, Some("video/42.mp4".to_string()))),
            "完成 → status=2 + payload_path"
        );
    }

    /// TOCTOU 去重(§V6-2):真实 0/无→1 转移返回 true;已在途重复调返回 false(并发第二方
    /// 不夺认领、不重复 spawn);error(3)→1 可重领返回 true。
    #[test]
    fn upsert_claim_returns_true_only_on_real_transition() {
        let c = one_video();
        assert!(
            upsert_and_claim_derivation(&c, 1, "video_playable").unwrap(),
            "首次(无行→0→1)发生真实认领"
        );
        assert!(
            !upsert_and_claim_derivation(&c, 1, "video_playable").unwrap(),
            "已在途(status=1):并发第二方不再认领,返回 false"
        );
        // error(3)复位后应可重领。
        batch_finish_derivations(
            &c,
            &[(
                1,
                "video_playable".to_string(),
                3,
                None,
                Some("e".to_string()),
                None,
                None,
            )],
        )
        .unwrap();
        assert!(
            upsert_and_claim_derivation(&c, 1, "video_playable").unwrap(),
            "error(3)→1 可重领"
        );
    }

    /// 背景流水线排除 video_playable(§V6-1):get_pending 不领取;reset_processing /
    /// requeue_in_flight 均不动其在途行。
    #[test]
    fn background_pipeline_excludes_video_playable() {
        let c = one_video(); // item 1, mkv
        c.execute_batch(
            "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (2, 10, 'b.mkv', 1, 1, 'mkv', 'video', 0, 0, 200, 43);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'video_playable', 1),
                 (2, 'video_playable', 0);",
        )
        .unwrap();

        // 生产者查询不领取任何 video_playable(status=0 的 item2 也被排除)。
        let pending = get_pending_derivations(&c, 100, None, &[]).unwrap();
        assert!(
            pending.iter().all(|(_, kind, ..)| kind != "video_playable"),
            "get_pending 不应返回 video_playable 行:{pending:?}"
        );

        // 通用在途复位不动 video_playable(item1 保持 status=1)。
        reset_processing_derivations(&c).unwrap();
        assert_eq!(
            status_of(&c, 1, "video_playable"),
            1,
            "reset_processing 跳过 video_playable"
        );
        requeue_in_flight_derivations(&c).unwrap();
        assert_eq!(
            status_of(&c, 1, "video_playable"),
            1,
            "requeue_in_flight 跳过 video_playable"
        );
    }

    /// 取消只作废在途(§V6-7):status=1→0;status=2 就绪产物不退。
    #[test]
    fn cancel_resets_only_in_flight() {
        let c = one_video();
        upsert_and_claim_derivation(&c, 1, "video_playable").unwrap(); // →1
        assert_eq!(
            reset_in_flight_derivation_for_item(&c, 1, "video_playable").unwrap(),
            1
        );
        assert_eq!(status_of(&c, 1, "video_playable"), 0, "在途(1)→退回 0");
        // 置就绪(2)后再取消:不退。
        batch_finish_derivations(
            &c,
            &[(
                1,
                "video_playable".to_string(),
                2,
                Some("video/42.mp4".to_string()),
                None,
                None,
                None,
            )],
        )
        .unwrap();
        assert_eq!(
            reset_in_flight_derivation_for_item(&c, 1, "video_playable").unwrap(),
            0,
            "就绪(2)不受取消影响"
        );
        assert_eq!(status_of(&c, 1, "video_playable"), 2);
    }

    /// 启动复位(§V6-11):全部在途 video_playable 退回 pending。
    #[test]
    fn boot_reset_requeues_in_flight_playable() {
        let c = one_video();
        upsert_and_claim_derivation(&c, 1, "video_playable").unwrap(); // →1
        assert_eq!(
            reset_in_flight_derivations_for_kind(&c, "video_playable").unwrap(),
            1
        );
        assert_eq!(
            status_of(&c, 1, "video_playable"),
            0,
            "boot 复位:在途→pending"
        );
    }

    fn status_of(c: &Connection, item_id: i64, kind: &str) -> i64 {
        c.query_row(
            "SELECT status FROM media_derivations WHERE item_id=?1 AND kind=?2",
            params![item_id, kind],
            |r| r.get(0),
        )
        .unwrap()
    }
}

// ── 隐藏根排除(V21 生成侧,派生流水线)──────────────────────────────────────
// 锁两消费口:后端生产者 get_pending_derivations 与前端 pdf/svg 泵
// list_pending_doc_thumbs 都跳过隐藏根;unhide 后 status=0 行原地续跑(非破坏暂停)。
#[cfg(test)]
mod hidden_root_derivation_tests {
    use super::super::scan::set_scan_root_hidden;
    use super::*;

    /// 两根:root1(视频 1)+root2(视频 2 + pdf 文档 3),全 status=0 待处理。
    fn two_roots_pending() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, '', 'r1'), (20, 2, '', 'r2');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.mp4', 1, 1, 'mp4', 'video', 0, 0, 100, 0),
                 (2, 20, 'b.mp4', 1, 1, 'mp4', 'video', 0, 0, 200, 0),
                 (3, 20, 'c.pdf', 1, 1, 'pdf', 'document', 0, 0, 300, 0);
             INSERT INTO media_derivations (item_id, kind, status) VALUES
                 (1, 'video_cover', 0),
                 (2, 'video_cover', 0),
                 (3, 'doc_thumb', 0);",
        )
        .unwrap();
        c
    }

    fn pending_ids(c: &Connection) -> Vec<i64> {
        get_pending_derivations(c, 100, None, &[])
            .unwrap()
            .into_iter()
            .map(|(id, ..)| id)
            .collect()
    }

    fn doc_thumb_ids(c: &Connection) -> Vec<i64> {
        list_pending_doc_thumbs(c, 100)
            .unwrap()
            .into_iter()
            .map(|(id, ..)| id)
            .collect()
    }

    #[test]
    fn both_consumers_exclude_hidden_root_and_unhide_restores() {
        let c = two_roots_pending();
        // 初始:生产者见两视频封面(pdf doc_thumb 本就走前端泵不在此列),泵见 pdf。
        assert_eq!(pending_ids(&c), vec![1, 2]);
        assert_eq!(doc_thumb_ids(&c), vec![3]);

        set_scan_root_hidden(&c, 2, true).unwrap();
        assert_eq!(pending_ids(&c), vec![1], "隐藏根 2 的视频封面任务被跳过");
        assert!(doc_thumb_ids(&c).is_empty(), "隐藏根 2 的 pdf 泵任务被跳过");

        set_scan_root_hidden(&c, 2, false).unwrap();
        assert_eq!(pending_ids(&c), vec![1, 2], "unhide 后 status=0 行原地续跑");
        assert_eq!(doc_thumb_ids(&c), vec![3]);
    }
}

// ── store_doc_thumbnail 守卫(#3 加固,2026-07-24)──────────────────────────────
// x-item-id 来自请求头不可信,须校验其确为前端可渲染文档(pdf/svg)再落任何状态。
#[cfg(test)]
mod doc_thumb_guard_tests {
    use super::*;

    /// 5 个 item 覆盖白名单内外:pdf/svg(接受)、epub(后端渲染,拒)、txt/mp4(非文档缩略图,拒)。
    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.pdf',  1, 1, 'pdf',  'image', 0, 0, 100, 0),
                 (2, 10, 'b.svg',  1, 1, 'svg',  'image', 0, 0, 200, 0),
                 (3, 10, 'c.epub', 1, 1, 'epub', 'image', 0, 0, 300, 0),
                 (4, 10, 'd.txt',  1, 1, 'txt',  'text',  0, 0, 400, 0),
                 (5, 10, 'e.mp4',  1, 1, 'mp4',  'video', 0, 0, 500, 0);",
        )
        .unwrap();
        c
    }

    #[test]
    fn whitelist_is_pdf_svg_only() {
        assert!(is_frontend_doc_thumb_format("pdf"));
        assert!(is_frontend_doc_thumb_format("svg"));
        assert!(!is_frontend_doc_thumb_format("epub")); // 后端 zip 渲染,有意排除
        assert!(!is_frontend_doc_thumb_format("txt"));
        assert!(!is_frontend_doc_thumb_format("jpg"));
        assert!(!is_frontend_doc_thumb_format(""));
        // 集合钉定:与 list_pending_doc_thumbs 的 IN 子句单源同集(该 SQL 由本常量插值生成),
        // 任一侧误改此集合,本断言即红——防守卫/泵白名单静默漂移。
        assert_eq!(FRONTEND_DOC_THUMB_FORMATS, &["pdf", "svg"]);
    }

    #[test]
    fn guard_accepts_pdf_svg_rejects_others() {
        let c = seeded();
        assert_eq!(validate_frontend_doc_thumb_item(&c, 1).unwrap(), "pdf");
        assert_eq!(validate_frontend_doc_thumb_item(&c, 2).unwrap(), "svg");
        // epub(后端渲染)/ txt / mp4 均非前端 doc-thumb 项 → Internal 拒绝
        assert!(validate_frontend_doc_thumb_item(&c, 3).is_err(), "epub 拒");
        assert!(validate_frontend_doc_thumb_item(&c, 4).is_err(), "txt 拒");
        assert!(validate_frontend_doc_thumb_item(&c, 5).is_err(), "mp4 拒");
    }

    #[test]
    fn guard_missing_item_is_media_not_found() {
        let c = seeded();
        assert!(
            matches!(
                validate_frontend_doc_thumb_item(&c, 999),
                Err(AppError::MediaNotFound(999))
            ),
            "不存在的 item_id → MediaNotFound(而非 Internal)"
        );
    }
}
