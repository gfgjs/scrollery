//! 缩略图域 DAO:pending 调度/封面对账/淘汰复位/exotic 路由信息/结果回写
//! (T 线拆分自 queries.rs,SQL 与行为不变;写边界防降级测试与实现同迁)。

use rusqlite::{params, Connection};

// 「exotic 接管门控」谓词由 exotic 域持有(P0 增补裁决);format! 内联捕获须裸名,故 use 引入。
use super::exotic::NOT_BLOCKED_BY_EXOTIC;
// 「隐藏根排除」谓词由 scan 域持有(V21);缩略图 pending 三查询共用,跳过隐藏根的媒体。
use super::scan::EXCLUDE_HIDDEN_ROOTS;
use crate::error::{AppError, Result};
use crate::exotic::task::ExoticTaskStatus;

/// 图像缩略图调度器（generator.rs 主 generator）的待处理集必须排除 **pdf/svg 文档**：这两类
/// 由前端 `DocThumbRenderer` 离屏渲染并经 `store_doc_thumbnail` 独占回填 thumb_status（成功→1、
/// 失败→2 两路全包）。若不排除，调度器会把它们领进批次、命中 UNSUPPORTED_TYPE 分支
/// （generator.rs）回写 `thumb_status=2/thumb_path=NULL`，**竞写冲掉** DocThumbRenderer 刚写好的
/// 封面（P1-4）—— 派生仍是 status=2 但画廊拿不到路径，永久裂图。与 `get_pending_derivations`
/// 里对称的 `NOT (kind='doc_thumb' AND format IN ('pdf','svg'))` 排除同源同理。
/// 注：`media_type='document'` 守卫确保绝不误伤真实图像；txt/docx 等无渲染器的文档**不**排除，
/// 仍由本调度器标 thumb_status=2 得到占位（其行为不变，安全）。
const EXCLUDE_FRONTEND_DOC_THUMB: &str =
    "AND NOT (media_type='document' AND file_format IN ('pdf','svg'))";

/// 图像调度器还必须排除 **video / audio**:它们的封面由派生流水线(video_cover / audio_cover)
/// 产出并回填 thumb_status,主 generator 的 `decode_media_step` 对非图像一律判 UNSUPPORTED_TYPE →
/// 回写 `thumb_status=2`(灰卡)。封面派生尚未完成时(thumb_status=0)若被本调度器领走标 2,会
/// **抢先冲掉**其真实封面路径、且派生行仍 done 永不重做 —— 与 pdf/svg 被排除同源同理(那是前端
/// DocThumbRenderer 独占,这是派生流水线独占)。
/// (2026-07-13 定位:全量生成的 dispatcher 缺此排除,把 2414 视频 + 31 音频误标 status=2 灰卡;
/// `start_full_thumbnail_generation` 的复位面早已限 `media_type='image'`,派发面却漏了对称排除。)
const EXCLUDE_DERIVATION_COVER_MEDIA: &str = "AND media_type NOT IN ('video','audio')";

/// 全库 cache_key 全集(**含软删行**——软删可恢复,其缓存不算孤儿),供对账 GC
/// (Part3 §3.3.2)判定「不在集即孤儿」。百万行 ≈ 十几 MB HashSet,内存可承受(复审 §467 ③)。
pub fn all_cache_keys(conn: &Connection) -> Result<std::collections::HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT cache_key FROM media_items WHERE cache_key IS NOT NULL")?;
    let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn get_pending_thumb_items(conn: &Connection, limit: i64) -> Result<Vec<(i64, i64)>> {
    let sql = format!(
        "SELECT id, cache_key FROM media_items
         WHERE thumb_status=0 AND is_deleted=0 {NOT_BLOCKED_BY_EXOTIC} {EXCLUDE_FRONTEND_DOC_THUMB} {EXCLUDE_DERIVATION_COVER_MEDIA} {EXCLUDE_HIDDEN_ROOTS}
         ORDER BY created_at DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn get_all_pending_thumb_ids(conn: &Connection) -> Result<Vec<i64>> {
    let sql = format!(
        "SELECT id FROM media_items
         WHERE thumb_status=0 AND is_deleted=0 {NOT_BLOCKED_BY_EXOTIC} {EXCLUDE_FRONTEND_DOC_THUMB} {EXCLUDE_DERIVATION_COVER_MEDIA} {EXCLUDE_HIDDEN_ROOTS}
         ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

pub fn count_pending_thumb_items(conn: &Connection) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM media_items WHERE thumb_status=0 AND is_deleted=0 {NOT_BLOCKED_BY_EXOTIC} {EXCLUDE_FRONTEND_DOC_THUMB} {EXCLUDE_DERIVATION_COVER_MEDIA} {EXCLUDE_HIDDEN_ROOTS}"
    );
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// 启动期自愈 P1-4 封面缩略图竞写残留：对「封面派生已完成（`status=2` 且 `payload_path` 非空）」
/// 但 `media_items` 分叉（`thumb_status<>1` 或 `thumb_path IS NULL`）的项，从**权威**派生产物
/// 重新回填 `thumb_status=1 / thumb_path`。冲突源于图像调度器与专门流水线（DocThumbRenderer /
/// 视频·音频封面）竞写、把新封面覆盖成 `thumb_status=2/NULL`。幂等（已收敛的库改 0 行）且严格
/// 非破坏：仅**提升**有真实产物的行，绝不制造裂图。覆盖 `produces_thumbnail()` 三类 kind
/// （video_cover / audio_cover / doc_thumb）。返回治愈行数。
///
/// 注：thumbhash 不从派生行恢复（doc_thumb 派生行未存 hash），保持原值 —— 它仅是加载前的
/// 模糊占位，缺失不影响真实缩略图（thumb_path）显示。
pub fn reconcile_cover_thumbs(conn: &Connection) -> Result<usize> {
    // 相关子查询取该项「已完成且有产物」的封面派生产物路径；WHERE 侧 EXISTS 同源门控，
    // 保证仅当产物确实存在时才提升，且只触碰分叉行（避免无谓写放大 idx_media_thumb）。
    let n = conn.execute(
        "UPDATE media_items
         SET thumb_status = 1,
             thumb_path = (
                 SELECT dv.payload_path FROM media_derivations dv
                 WHERE dv.item_id = media_items.id
                   AND dv.kind IN ('video_cover','audio_cover','doc_thumb')
                   AND dv.status = 2 AND dv.payload_path IS NOT NULL
                 LIMIT 1)
         WHERE is_deleted = 0
           AND (thumb_status <> 1 OR thumb_path IS NULL)
           AND EXISTS (
                 SELECT 1 FROM media_derivations dv
                 WHERE dv.item_id = media_items.id
                   AND dv.kind IN ('video_cover','audio_cover','doc_thumb')
                   AND dv.status = 2 AND dv.payload_path IS NOT NULL)",
        [],
    )?;
    Ok(n)
}

/// 封面类派生 kind（视频/音频封面、文档缩略图）—— 这三类的产物写入 `thumbnails/` 且经派生流水线
/// 回填 `media_items.thumb_status=1 / thumb_path`。自愈只针对它们（图像走主 generator 的缺文件
/// CACHE_MISS 自愈路径）。单一事实源，避免三处散落的 kind 列表漂移。
const COVER_DERIV_KINDS: [&str; 3] = ["video_cover", "audio_cover", "doc_thumb"];

/// 把一组 item 的封面派生复位为待重生成：`media_items` 退回 `thumb_status=0 / thumb_path=NULL`，
/// 封面派生行 `status 2→0 / payload_path=NULL`，交派生流水线（`derive::run_cover` 等）重跑写盘并
/// 回填 `status=1`。**必须同时复位派生行**：只退 `media_items` 会被主 generator 领走（视频非 image
/// → UNSUPPORTED_TYPE → 打回 `thumb_status=2`），而派生行仍是 done 不会重跑（生产者只领 status=0）。
/// 调用方须持写连接、在 `spawn_blocking` 内执行。返回复位的 item 数（`ids` 为空即 0）。
pub fn reset_cover_thumbs_for_regen(conn: &Connection, ids: &[i64]) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    // 显式编号占位符 `?2,?3,…`：`?1` 留给 item_id，避免 `?1` 与匿名 `?` 混用导致的编号歧义。
    let kinds_ph = (0..COVER_DERIV_KINDS.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(",");
    let deriv_sql = format!(
        "UPDATE media_derivations SET status = 0, payload_path = NULL
         WHERE item_id = ?1 AND status = 2 AND kind IN ({kinds_ph})"
    );
    let tx = conn.unchecked_transaction()?;
    for &id in ids {
        tx.execute(
            "UPDATE media_items SET thumb_status = 0, thumb_path = NULL WHERE id = ?1",
            params![id],
        )?;
        // 派生行按 (item, kind) 复位；`payload_path=NULL` 让 run_cover 重写后重新回填。
        let mut sql_params: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(id)];
        for k in COVER_DERIV_KINDS {
            sql_params.push(rusqlite::types::Value::Text(k.to_string()));
        }
        tx.execute(&deriv_sql, rusqlite::params_from_iter(sql_params.iter()))?;
    }
    tx.commit()?;
    Ok(ids.len())
}

/// 仅在懒自愈读取的缩略图源快照仍然匹配时复位一项封面。
///
/// 缓存文件检查发生在数据库锁外；调用方必须把这里的条件更新放在生命周期读区，
/// 让清库换代和源代次推进都能使过期自愈请求变成 no-op。返回实际复位的媒体行数。
pub fn reset_cover_thumb_for_regen_if_current(
    conn: &Connection,
    item_id: i64,
    expected_thumb_path: &str,
    expected_source_revision: i64,
    expected_cache_key: i64,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let affected = tx.execute(
        "UPDATE media_items
         SET thumb_status = 0, thumb_path = NULL, thumbhash = NULL
         WHERE id = ?1 AND is_deleted = 0 AND thumb_status = 1
           AND thumb_path = ?2 AND source_revision = ?3 AND cache_key = ?4",
        params![
            item_id,
            expected_thumb_path,
            expected_source_revision,
            expected_cache_key
        ],
    )?;
    if affected == 1 {
        let kinds_ph = (0..COVER_DERIV_KINDS.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let deriv_sql = format!(
            "UPDATE media_derivations SET status = 0, payload_path = NULL
             WHERE item_id = ?1 AND status = 2 AND kind IN ({kinds_ph})"
        );
        let mut sql_params: Vec<rusqlite::types::Value> =
            vec![rusqlite::types::Value::Integer(item_id)];
        for kind in COVER_DERIV_KINDS {
            sql_params.push(rusqlite::types::Value::Text(kind.to_string()));
        }
        tx.execute(&deriv_sql, rusqlite::params_from_iter(sql_params.iter()))?;
    }
    tx.commit()?;
    Ok(affected)
}

/// LRU 驱逐即时复位(深审 defer ⑥,事件驱动):按**被驱逐文件的 DB 相对路径**精确复位
/// 受影响项。`media_items.thumb_path` 与驱逐路径同构(`{size}/{xx}/{hex}.webp`),按路径
/// 匹配天然避免「同 key 另一档位文件被驱逐」误伤现行缩略图(按 cache_key 复位则会)。
/// 封面派生行**先于** media_items 复位(media_items 更新后 thumb_path 置 NULL,子查询将
/// 失配)。返回复位的 item id 集,调用方据此 patch 常驻 items 缓存并广播刷新。
/// 调用方须持写连接、在 `spawn_blocking` 内执行。幂等(路径已不匹配则改 0 行)。
pub fn reset_thumbs_by_evicted_paths(conn: &Connection, paths: &[String]) -> Result<Vec<i64>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.unchecked_transaction()?;
    let mut ids = Vec::new();
    // 分块防超 SQLite 变量上限(与 batch_update 惯例一致)。
    for chunk in paths.chunks(500) {
        let ph = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let deriv_sql = format!(
            "UPDATE media_derivations SET status = 0, payload_path = NULL
             WHERE status = 2 AND kind IN ('video_cover','audio_cover','doc_thumb')
               AND item_id IN (
                   SELECT id FROM media_items
                   WHERE thumb_status = 1 AND is_deleted = 0 AND thumb_path IN ({ph}))"
        );
        tx.execute(&deriv_sql, rusqlite::params_from_iter(chunk.iter()))?;
        let items_sql = format!(
            "UPDATE media_items SET thumb_status = 0, thumb_path = NULL
             WHERE thumb_status = 1 AND is_deleted = 0 AND thumb_path IN ({ph})
             RETURNING id"
        );
        let mut stmt = tx.prepare(&items_sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            r.get::<_, i64>(0)
        })?;
        for r in rows {
            ids.push(r?);
        }
        drop(stmt);
    }
    tx.commit()?;
    Ok(ids)
}

#[cfg(test)]
mod reset_by_evicted_paths_tests {
    //! ⑥ 事件驱动复位:按路径精确匹配 + 封面派生行同步复位 + 分块边界。
    use super::*;

    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             -- 1=被驱逐的视频封面;2=同类但路径未被驱逐;3=被驱逐的普通图像;4=软删的匹配路径
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, thumb_path, is_deleted) VALUES
                 (1, 10, 'a.mp4', 1, 1, 'mp4', 'video', 0, 0, 100, 11, 1, '480/aa/evicted1.webp', 0),
                 (2, 10, 'b.mp4', 1, 1, 'mp4', 'video', 0, 0, 200, 12, 1, '480/bb/alive.webp',    0),
                 (3, 10, 'c.jpg', 1, 1, 'jpg', 'image', 0, 0, 300, 13, 1, '480/cc/evicted2.webp', 0),
                 (4, 10, 'd.mp4', 1, 1, 'mp4', 'video', 0, 0, 400, 14, 1, '480/dd/evicted3.webp', 1);
             INSERT INTO media_derivations (item_id, kind, status, payload_path) VALUES
                 (1, 'video_cover', 2, '480/aa/evicted1.webp'),
                 (1, 'ai_thumb',    2, 'ai/aa.webp'),
                 (2, 'video_cover', 2, '480/bb/alive.webp');",
        )
        .unwrap();
        c
    }

    fn thumb_of(c: &Connection, id: i64) -> (i64, Option<String>) {
        c.query_row(
            "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    fn deriv_status(c: &Connection, id: i64, kind: &str) -> i64 {
        c.query_row(
            "SELECT status FROM media_derivations WHERE item_id=?1 AND kind=?2",
            params![id, kind],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 路径精确匹配:仅被驱逐路径的项复位;封面派生行同步退 pending,非封面派生(ai_thumb)
    /// 与未驱逐项不动;软删行不动(懒 404 自愈兜底)。返回 id 集与复位行一致。
    #[test]
    fn resets_only_evicted_paths_and_their_cover_derivations() {
        let c = seeded();
        let ids = reset_thumbs_by_evicted_paths(
            &c,
            &[
                "480/aa/evicted1.webp".into(),
                "480/cc/evicted2.webp".into(),
                "480/dd/evicted3.webp".into(), // 软删项路径:应被 is_deleted 过滤
                "480/zz/notindb.webp".into(),  // DB 无此路径:no-op
            ],
        )
        .unwrap();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 3], "仅活跃且路径命中的两项复位");
        assert_eq!(thumb_of(&c, 1), (0, None));
        assert_eq!(thumb_of(&c, 3), (0, None));
        assert_eq!(
            thumb_of(&c, 2),
            (1, Some("480/bb/alive.webp".into())),
            "未驱逐项不动"
        );
        assert_eq!(thumb_of(&c, 4).0, 1, "软删项不动");
        assert_eq!(
            deriv_status(&c, 1, "video_cover"),
            0,
            "封面派生行退 pending"
        );
        assert_eq!(deriv_status(&c, 1, "ai_thumb"), 2, "非封面派生不动");
        assert_eq!(deriv_status(&c, 2, "video_cover"), 2, "未驱逐项派生不动");
    }

    /// 分块边界:>500 条路径跨块执行不丢不错(与 batch_update 分块惯例同源)。
    #[test]
    fn chunks_across_boundary() {
        let c = seeded();
        // 599 条不存在的路径 + 2 条真实路径,故意分落两个块。
        let mut paths: Vec<String> = (0..599).map(|i| format!("480/xx/nope{i}.webp")).collect();
        paths.insert(3, "480/aa/evicted1.webp".into()); // 第一块
        paths.push("480/cc/evicted2.webp".into()); // 第二块
        let mut ids = reset_thumbs_by_evicted_paths(&c, &paths).unwrap();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 3]);
    }
}

/// 启动期自愈 ②（与 `reconcile_cover_thumbs` 反向互补）：修复 **LRU 缓存驱逐残留**。
///
/// 病理：`enforce_cache_limit`（LRU 按 mtime 删 `thumbnails/` 文件）删封面时**不改**
/// `media_items.thumb_status`，而 `route_thumbnail` 对 `thumb_status=1` 直接短路（不 stat 文件、
/// 不重生成）→ 被驱逐的封面缩略图永久 404（视频封面尤甚：整批旧档 cohort 被驱逐）。主 generator
/// 的 `decode_media_step` 本有「缺文件 CACHE_MISS → 重生成」能力，但封面类根本走不到它（router
/// 短路 + 派生行仍 done 不会重排）。
///
/// 修法：扫「有封面派生（done）且 `media_items` 声称已生成（`thumb_status=1`）」的项，逐一 stat
/// `thumb_path`；文件缺失的交 `reset_cover_thumbs_for_regen` 复位（media_items + 派生行一并退回
/// pending）。仅扫封面派生项（数千，非全库五万），startup 廉价。幂等（无缺失则改 0 行）。
/// 需 `spawn_blocking`（含磁盘 stat）。返回自愈的 item 数。
///
/// 候选查询与文件检查拆开：调用方若持有 SQLite writer mutex，只能调用
/// [`list_cover_thumb_paths`]；`thumb_path.exists()` 必须在锁外执行。
pub fn list_cover_thumb_paths(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let kinds_ph = COVER_DERIV_KINDS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    // 候选：封面派生 done 且 media_items 标 status=1 有路径。DISTINCT —— 一个 item 可能有多条
    // 封面派生（理论上互斥，防御性去重）。
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT m.id, m.thumb_path
         FROM media_items m
         JOIN media_derivations dv ON dv.item_id = m.id
         WHERE m.is_deleted = 0
           AND m.thumb_status = 1
           AND m.thumb_path IS NOT NULL
           AND dv.status = 2
           AND dv.kind IN ({kinds_ph})"
    ))?;
    let kind_params: Vec<rusqlite::types::Value> = COVER_DERIV_KINDS
        .iter()
        .map(|k| rusqlite::types::Value::Text(k.to_string()))
        .collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(kind_params.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn reconcile_missing_cover_thumbs(
    conn: &Connection,
    cache_dir: &std::path::Path,
) -> Result<usize> {
    let candidates = list_cover_thumb_paths(conn)?;

    // 逐项 stat：仅文件确实缺失的才复位（避免对健康封面写放大）。
    // thumb_path 形如 "480/xx/hash.webp"（不含 thumbnails/ 前缀，前端/此处补），见 MediaThumb。
    let missing: Vec<i64> = candidates
        .into_iter()
        .filter(|(_, tp)| !cache_dir.join("thumbnails").join(tp).exists())
        .map(|(id, _)| id)
        .collect();

    reset_cover_thumbs_for_regen(conn, &missing)
}

/// 批量取一组 item 的 thumbnail exotic 任务状态（缩略图 Router 用，避免逐项查询 N+1，R7）。
/// 返回 item_id → 状态；无任务的 item 不在表中。
pub fn exotic_thumbnail_task_status_for_items(
    conn: &Connection,
    item_ids: &[i64],
) -> Result<std::collections::HashMap<i64, ExoticTaskStatus>> {
    let mut map = std::collections::HashMap::new();
    if item_ids.is_empty() {
        return Ok(map);
    }
    let placeholders = item_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT item_id, status FROM exotic_tasks
         WHERE capability='thumbnail' AND item_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    for r in rows.flatten() {
        if let Some(st) = ExoticTaskStatus::from_i64(r.1) {
            map.insert(r.0, st);
        }
    }
    Ok(map)
}

/// thumbnail 任务的路由信息（status + done 任务的指纹/worker 版本）。缩略图入口用它重算期望指纹、
/// 失效「指纹已变的 done」（R5/R6，问题4）——尤其用户改缩略图档位后，旧档 done 不再匹配新请求指纹。
pub struct ExoticThumbRouteInfo {
    pub status: ExoticTaskStatus,
    pub input_fingerprint: Option<String>,
    pub worker_version: Option<String>,
}

/// 批量取一组 item 的 thumbnail 任务路由信息（避免 N+1，R7）。无任务的 item 不在表中。
/// 比 `exotic_thumbnail_task_status_for_items` 多取指纹/worker 版本，供 Router 做指纹有效性判定。
pub fn exotic_thumbnail_route_info_for_items(
    conn: &Connection,
    item_ids: &[i64],
) -> Result<std::collections::HashMap<i64, ExoticThumbRouteInfo>> {
    let mut map = std::collections::HashMap::new();
    if item_ids.is_empty() {
        return Ok(map);
    }
    let placeholders = item_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT item_id, status, input_fingerprint, worker_version FROM exotic_tasks
         WHERE capability='thumbnail' AND item_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for (item_id, status, fp, wv) in rows.flatten() {
        if let Some(st) = ExoticTaskStatus::from_i64(status) {
            map.insert(
                item_id,
                ExoticThumbRouteInfo {
                    status: st,
                    input_fingerprint: fp,
                    worker_version: wv,
                },
            );
        }
    }
    Ok(map)
}

/// 按生产时的源快照条件回写缩略图结果，返回实际受影响的行数。
///
/// `source_revision` 与 `cache_key` 必须同时匹配当前 `media_items` 行；否则结果来自旧源，
/// 直接丢弃。失败状态（`2`）还保留既有写边界：不得把已成功产物（`1`/`3`）降级。
/// 缩略图 worker 的异步结果必须走此 API；返回 `0` 是正常的陈旧结果信号，不代表 SQL 失败。
pub fn update_thumb_result_if_current(
    conn: &Connection,
    item_id: i64,
    expected_source_revision: i64,
    expected_cache_key: i64,
    status: i64,
    path: Option<&str>,
    thumbhash: Option<&[u8]>,
) -> Result<usize> {
    let affected = if status == 2 {
        conn.execute(
            "UPDATE media_items SET thumb_status=?1, thumb_path=?2, thumbhash=?3,
                     updated_at=strftime('%s','now')
             WHERE id=?4 AND source_revision=?5 AND cache_key=?6
               AND thumb_status NOT IN (1, 3)",
            params![
                status,
                path,
                thumbhash,
                item_id,
                expected_source_revision,
                expected_cache_key
            ],
        )?
    } else {
        conn.execute(
            "UPDATE media_items SET thumb_status=?1, thumb_path=?2, thumbhash=?3,
                     updated_at=strftime('%s','now')
             WHERE id=?4 AND source_revision=?5 AND cache_key=?6",
            params![
                status,
                path,
                thumbhash,
                item_id,
                expected_source_revision,
                expected_cache_key
            ],
        )?
    };
    Ok(affected)
}

/// 兼容旧生产者的缩略图回写。
///
/// 此 API 仅保留既有的失败降级保护，不校验 `source_revision`/`cache_key`，因此不适用于
/// 读取源文件后异步返回的 worker 结果；这类调用必须使用 [`update_thumb_result_if_current`]。
pub fn update_thumb_result(
    conn: &Connection,
    item_id: i64,
    status: i64,
    path: Option<&str>,
    thumbhash: Option<&[u8]>,
) -> Result<()> {
    // 写边界防降级(P1-4 竞写残留的根治,2026-07-10):失败标记(status=2)不得覆盖已有产物行
    // (status=1 已生成 / status=3 直显)。五条流水线(主生成器/派生/exotic/doc 前后端)全汇聚
    // 于此函数,查询侧排除(EXCLUDE_FRONTEND_DOC_THUMB)只挡了 pdf/svg 的 FullThumbGen 路径,
    // 而视口 batch_request 仍可能让主生成器把 status=0 的 video/audio/epub 领走、在派生流水线
    // 已回填封面(=1)之后才 flush UNSUPPORTED_TYPE 失败(=2)——此守卫使滞后失败写落空,
    // 会话中期封面不再被冲掉(原先只能等下次启动 reconcile_cover_thumbs 自愈)。
    // 成功写(1/3)携带真实产物,恒无条件覆盖;所有合法的失败路径(FullThumbGen 重置/
    // regenerate_missing_thumb/clear_all)都先把行退回 0,不受影响。
    if status == 2 {
        conn.execute(
            "UPDATE media_items SET thumb_status=?1, thumb_path=?2, thumbhash=?3,
                     updated_at=strftime('%s','now')
             WHERE id=?4 AND thumb_status NOT IN (1, 3)",
            params![status, path, thumbhash, item_id],
        )?;
    } else {
        conn.execute(
            "UPDATE media_items SET thumb_status=?1, thumb_path=?2, thumbhash=?3,
                     updated_at=strftime('%s','now')
             WHERE id=?4",
            params![status, path, thumbhash, item_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod update_thumb_result_tests {
    //! 写边界防降级:失败写(status=2)不得覆盖已有产物行(1/3),成功写恒无条件。
    use super::*;

    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             -- 1=已生成封面(status=1),2=直显(status=3),3=待生成(status=0),4=已失败(status=2)
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, thumb_path) VALUES
                 (1, 10, 'a.mp4', 1, 1, 'mp4', 'video',    0, 0, 100, 11, 1, '480/aa/cover.webp'),
                 (2, 10, 'b.jpg', 1, 1, 'jpg', 'image',    0, 0, 200, 12, 3, '/r/b.jpg'),
                 (3, 10, 'c.jpg', 1, 1, 'jpg', 'image',    0, 0, 300, 13, 0, NULL),
                 (4, 10, 'd.txt', 1, 1, 'txt', 'document', 0, 0, 400, 14, 2, NULL);",
        )
        .unwrap();
        c
    }

    fn thumb_of(c: &Connection, id: i64) -> (i64, Option<String>) {
        c.query_row(
            "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// 竞写场景核心:派生流水线已回填封面(=1)后,主生成器滞后的 UNSUPPORTED_TYPE 失败写(=2)
    /// 必须落空——这是 video/audio/epub 同源竞写残留的根治点(原先要等下次启动 reconcile)。
    #[test]
    fn failure_write_cannot_downgrade_generated_cover() {
        let c = seeded();
        update_thumb_result(&c, 1, 2, None, None).unwrap();
        assert_eq!(
            thumb_of(&c, 1),
            (1, Some("480/aa/cover.webp".into())),
            "status=1 产物行不被失败写降级"
        );
    }

    /// 直显行(=3)同为「有产物」语义,失败写同样不得覆盖。
    #[test]
    fn failure_write_cannot_downgrade_direct_display() {
        let c = seeded();
        update_thumb_result(&c, 2, 2, None, None).unwrap();
        assert_eq!(thumb_of(&c, 2), (3, Some("/r/b.jpg".into())));
    }

    /// 合法失败路径不受影响:待生成(0)与已失败(2)行照常标记失败(幂等)。
    #[test]
    fn failure_write_applies_to_pending_and_failed_rows() {
        let c = seeded();
        update_thumb_result(&c, 3, 2, None, None).unwrap();
        assert_eq!(thumb_of(&c, 3), (2, None), "0→2 正常失败标记");
        update_thumb_result(&c, 4, 2, None, None).unwrap();
        assert_eq!(thumb_of(&c, 4), (2, None), "2→2 幂等");
    }

    /// 成功写(1/3)携带真实产物,恒无条件覆盖——含把失败行治愈为成功。
    #[test]
    fn success_write_is_unconditional() {
        let c = seeded();
        update_thumb_result(&c, 4, 1, Some("480/dd/doc.webp"), None).unwrap();
        assert_eq!(thumb_of(&c, 4), (1, Some("480/dd/doc.webp".into())));
        update_thumb_result(&c, 1, 1, Some("480/aa/new.webp"), None).unwrap();
        assert_eq!(
            thumb_of(&c, 1),
            (1, Some("480/aa/new.webp".into())),
            "成功写可替换旧产物(重生成)"
        );
    }

    #[test]
    fn guarded_write_requires_matching_source_snapshot_and_reports_affected_rows() {
        let c = seeded();

        // 默认源快照为 (source_revision=1, cache_key=13)。任一快照字段过期都必须是 no-op。
        assert_eq!(
            update_thumb_result_if_current(&c, 3, 2, 13, 1, Some("480/cc/new.webp"), None).unwrap(),
            0,
            "旧 source_revision 不得写入"
        );
        assert_eq!(
            update_thumb_result_if_current(&c, 3, 1, 99, 1, Some("480/cc/new.webp"), None).unwrap(),
            0,
            "旧 cache_key 不得写入"
        );
        assert_eq!(thumb_of(&c, 3), (0, None));

        assert_eq!(
            update_thumb_result_if_current(&c, 3, 1, 13, 1, Some("480/cc/new.webp"), None).unwrap(),
            1,
            "匹配当前快照时返回实际受影响行数"
        );
        assert_eq!(thumb_of(&c, 3), (1, Some("480/cc/new.webp".into())));
    }

    #[test]
    fn guarded_failure_preserves_success_and_rejects_stale_source() {
        let c = seeded();

        // 当前已有成功产物：失败结果即使快照匹配，也必须保留产物并返回 0。
        assert_eq!(
            update_thumb_result_if_current(&c, 1, 1, 11, 2, None, None).unwrap(),
            0,
            "失败写不得降级已有成功产物"
        );
        assert_eq!(thumb_of(&c, 1), (1, Some("480/aa/cover.webp".into())));

        // 模拟 scanner 在同一 database epoch 内推进源代次；旧 worker 的失败回写也必须丢弃。
        c.execute(
            "UPDATE media_items SET source_revision=2, thumb_status=0, thumb_path=NULL WHERE id=3",
            [],
        )
        .unwrap();
        assert_eq!(
            update_thumb_result_if_current(&c, 3, 1, 13, 2, None, None).unwrap(),
            0,
            "旧 worker 的失败结果不得标记新源"
        );
        assert_eq!(thumb_of(&c, 3), (0, None));
    }

    #[test]
    fn missing_cover_reset_is_snapshot_guarded_and_resets_cover_derivation() {
        let c = seeded();
        c.execute(
            "INSERT INTO media_derivations (item_id, kind, status, payload_path)
             VALUES (1, 'video_cover', 2, '480/aa/cover.webp')",
            [],
        )
        .unwrap();

        assert_eq!(
            reset_cover_thumb_for_regen_if_current(&c, 1, "480/aa/cover.webp", 1, 11,).unwrap(),
            1
        );
        assert_eq!(thumb_of(&c, 1), (0, None));
        assert_eq!(
            c.query_row(
                "SELECT status, payload_path FROM media_derivations
                 WHERE item_id=1 AND kind='video_cover'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
            (0, None)
        );

        c.execute(
            "UPDATE media_items SET thumb_status=1, thumb_path='480/aa/cover.webp', source_revision=2
             WHERE id=1",
            [],
        )
        .unwrap();
        assert_eq!(
            reset_cover_thumb_for_regen_if_current(&c, 1, "480/aa/cover.webp", 1, 11,).unwrap(),
            0,
            "旧自愈请求不得复位新源"
        );
        assert_eq!(thumb_of(&c, 1), (1, Some("480/aa/cover.webp".into())));
    }
}

#[cfg(test)]
mod cover_thumb_pipeline_tests {
    //! P1-4 缩略图流水线所有权边界:图像调度器排除前端驱动的 pdf/svg + 启动期自愈竞写残留。
    use super::*;

    /// 建 schema + 覆盖各流水线归属的 item/派生行。
    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             -- 待处理集(thumb_status=0):1=图像应入,2=pdf/3=svg 前端驱动应排除,4=txt 无渲染器应入
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, thumb_path) VALUES
                 (1, 10, 'img.jpg',  1, 1, 'jpg', 'image',    0, 0, 100, 11, 0, NULL),
                 (2, 10, 'a.pdf',    1, 1, 'pdf', 'document', 0, 0, 200, 12, 0, NULL),
                 (3, 10, 'b.svg',    1, 1, 'svg', 'document', 0, 0, 300, 13, 0, NULL),
                 (4, 10, 'c.txt',    1, 1, 'txt', 'document', 0, 0, 400, 14, 0, NULL),
                 -- 自愈场景:5=被冲的 pdf(派生已成有产物→应治愈),6=真失败 pdf(派生 status=3 无产物→不动)
                 (5, 10, 'd.pdf',    1, 1, 'pdf', 'document', 0, 0, 500, 15, 2, NULL),
                 (6, 10, 'e.pdf',    1, 1, 'pdf', 'document', 0, 0, 600, 16, 2, NULL),
                 -- 7=已收敛视频封面(thumb_status=1 且有 path→不动,验幂等)
                 (7, 10, 'f.mp4',    1, 1, 'mp4', 'video',    0, 0, 700, 17, 1, 'v/f.webp'),
                 -- 8=待封面视频 / 9=待封面音频(thumb_status=0):封面归派生流水线,主 generator 应排除
                 -- (否则被抢先标 status=2 灰卡,阻断真实封面——2026-07-13 修复核心)
                 (8, 10, 'g.mp4',    1, 1, 'mp4', 'video',    0, 0, 800, 18, 0, NULL),
                 (9, 10, 'h.mp3',    1, 1, 'mp3', 'audio',    0, 0, 900, 19, 0, NULL);
             INSERT INTO media_derivations (item_id, kind, status, payload_path) VALUES
                 (5, 'doc_thumb',   2, '480/55/deadbeef.webp'),
                 (6, 'doc_thumb',   3, NULL),
                 (7, 'video_cover', 2, 'v/f.webp');",
        )
        .unwrap();
        c
    }

    fn thumb_of(c: &Connection, id: i64) -> (i64, Option<String>) {
        c.query_row(
            "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// 图像调度器的待处理集必须排除 pdf/svg(前端 DocThumbRenderer 独占其 thumb_status),
    /// 但保留 txt(无渲染器,由本调度器标 2 占位)与真实图像。三个领取入口口径一致。
    #[test]
    fn image_dispatcher_excludes_frontend_docs_but_keeps_text_and_images() {
        let c = seeded();
        let ids: std::collections::HashSet<i64> =
            get_all_pending_thumb_ids(&c).unwrap().into_iter().collect();
        assert!(ids.contains(&1), "图像入待处理集");
        assert!(ids.contains(&4), "txt(无渲染器)入待处理集,仍得占位");
        assert!(!ids.contains(&2), "pdf 排除——归 DocThumbRenderer");
        assert!(!ids.contains(&3), "svg 排除——归 DocThumbRenderer");
        assert!(
            !ids.contains(&8),
            "待封面视频排除——归 video_cover 派生流水线,不得被主 generator 标灰"
        );
        assert!(
            !ids.contains(&9),
            "待封面音频排除——归 audio_cover 派生流水线,不得被主 generator 标灰"
        );

        let paged: std::collections::HashSet<i64> = get_pending_thumb_items(&c, 100)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(paged, ids, "分页领取与全量领取口径一致");
        assert_eq!(
            count_pending_thumb_items(&c).unwrap(),
            2,
            "计数仅含图像+txt(视频/音频/pdf/svg 均排除)"
        );
    }

    /// 启动自愈:派生已成有产物但 media_items 被冲成 thumb_status=2/NULL 的封面 → 从权威产物回填;
    /// 真失败(派生 status=3 无产物)与已收敛行不动;幂等。
    #[test]
    fn reconcile_heals_clobbered_covers_only() {
        let c = seeded();
        let healed = reconcile_cover_thumbs(&c).unwrap();
        assert_eq!(healed, 1, "仅 id=5(被冲的成功封面)被治愈");

        assert_eq!(
            thumb_of(&c, 5),
            (1, Some("480/55/deadbeef.webp".to_string())),
            "id=5:从派生产物回填 thumb_status=1 + thumb_path"
        );
        assert_eq!(
            thumb_of(&c, 6),
            (2, None),
            "id=6:真失败(派生 status=3 无产物)不动,保持占位"
        );
        assert_eq!(
            thumb_of(&c, 7),
            (1, Some("v/f.webp".to_string())),
            "id=7:已收敛封面不动"
        );

        assert_eq!(
            reconcile_cover_thumbs(&c).unwrap(),
            0,
            "幂等:再次运行治愈 0 行"
        );
    }

    /// 启动自愈 ②(反向):thumb_status=1 但封面文件已被 LRU 驱逐(磁盘缺失)→ 复位 media_items +
    /// 封面派生行待重生成;文件健在的封面不动;图像项(无封面派生)不在扫描域;幂等。
    #[test]
    fn reconcile_missing_covers_resets_only_evicted() {
        use std::io::Write;
        // 独立临时 cache 目录(按进程号隔离并行测试),返回前清空。
        let mut cache = std::env::temp_dir();
        cache.push(format!(
            "scrollery_reconcile_missing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&cache);
        let thumbs = cache.join("thumbnails");
        std::fs::create_dir_all(thumbs.join("48").join("aa")).unwrap();
        // 仅为 id=8 写真实封面文件(存在);id=9 的封面文件故意不写(模拟被 LRU 驱逐)。
        let present_rel = "48/aa/present01.webp";
        let missing_rel = "48/bb/missing02.webp";
        std::fs::File::create(thumbs.join("48").join("aa").join("present01.webp"))
            .unwrap()
            .write_all(b"webp")
            .unwrap();

        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(&format!(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, thumb_path) VALUES
                 (8,  10, 'ok.mp4',  1, 1, 'mp4', 'video', 0, 0, 100, 81, 1, '{present_rel}'),
                 (9,  10, 'ev.mp4',  1, 1, 'mp4', 'video', 0, 0, 200, 82, 1, '{missing_rel}'),
                 (12, 10, 'img.jpg', 1, 1, 'jpg', 'image', 0, 0, 300, 83, 1, '120/cc/img03.webp');
             INSERT INTO media_derivations (item_id, kind, status, payload_path) VALUES
                 (8, 'video_cover', 2, '{present_rel}'),
                 (9, 'video_cover', 2, '{missing_rel}');",
        ))
        .unwrap();

        let healed = reconcile_missing_cover_thumbs(&c, &cache).unwrap();
        assert_eq!(healed, 1, "仅 id=9(封面被驱逐)复位");

        assert_eq!(
            thumb_of(&c, 8),
            (1, Some(present_rel.to_string())),
            "id=8 封面健在,不动"
        );
        assert_eq!(thumb_of(&c, 9), (0, None), "id=9 复位为待生成");
        let dv_status = |id: i64| {
            c.query_row(
                "SELECT status FROM media_derivations WHERE item_id=?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
        };
        assert_eq!(dv_status(9), 0, "id=9 封面派生退回 pending(交流水线重跑)");
        assert_eq!(dv_status(8), 2, "id=8 封面派生不动");
        // 图像项无封面派生 → 不在本 reconcile 扫描域,保持不变(其缺文件走主 generator CACHE_MISS 自愈)。
        assert_eq!(
            thumb_of(&c, 12),
            (1, Some("120/cc/img03.webp".to_string())),
            "图像项不受本 reconcile 影响"
        );

        assert_eq!(
            reconcile_missing_cover_thumbs(&c, &cache).unwrap(),
            0,
            "幂等:再跑复位 0 行"
        );

        let _ = std::fs::remove_dir_all(&cache);
    }
}

#[cfg(test)]
mod hidden_root_pipeline_tests {
    //! 隐藏根排除(V21 派生流水线侧):缩略图 / AI / 人脸三条 pending 枚举**与配对 count**
    //! 均跳过被隐藏根下的媒体;取消隐藏即复原(status 仍 0,谓词一撤即被下一轮枚举捞回)。
    //! 跨域取 ai/face 计数与枚举(与 exotic_dao_tests 同一定向引用手法)。
    use super::super::ai::{count_pending_ai_items, get_pending_ai_items};
    use super::super::faces::{count_pending_face_items, get_pending_face_items};
    use super::super::scan::set_scan_root_hidden;
    use super::*;

    /// 两根各两张待处理图(thumb/ai/face 全 status=0、image、活行):
    /// root1(dir10)可见、root2(dir20)待隐。
    fn two_roots_pending() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1'), (20, 2, '', 'r2');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, ai_status, face_status, is_deleted) VALUES
                 (1, 10, 'a.jpg',  1, 1, 'jpg',  'image', 0, 0, 100, 11, 0, 0, 0, 0),
                 (2, 10, 'b.png',  1, 1, 'png',  'image', 0, 0, 200, 12, 0, 0, 0, 0),
                 (3, 20, 'c.gif',  1, 1, 'gif',  'image', 0, 0, 300, 13, 0, 0, 0, 0),
                 (4, 20, 'd.webp', 1, 1, 'webp', 'image', 0, 0, 400, 14, 0, 0, 0, 0);",
        )
        .unwrap();
        c
    }

    fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
        v.sort_unstable();
        v
    }
    fn ai_ids(c: &Connection) -> Vec<i64> {
        sorted(
            get_pending_ai_items(c, 100)
                .unwrap()
                .iter()
                .map(|it| it.id)
                .collect(),
        )
    }
    fn face_ids(c: &Connection) -> Vec<i64> {
        sorted(
            get_pending_face_items(c, 100)
                .unwrap()
                .iter()
                .map(|it| it.id)
                .collect(),
        )
    }

    /// 三条流水线一致:未隐藏 → 四项全待处理;隐藏 root2 → 其项从枚举+count 全消失;取消隐藏 → 复原。
    #[test]
    fn all_three_pipelines_exclude_hidden_root() {
        let c = two_roots_pending();

        // 未隐藏(常态):谓词内层空集 → 四项全在,与加 V21 前逐字节同结果。
        assert_eq!(
            sorted(get_all_pending_thumb_ids(&c).unwrap()),
            vec![1, 2, 3, 4]
        );
        assert_eq!(ai_ids(&c), vec![1, 2, 3, 4]);
        assert_eq!(face_ids(&c), vec![1, 2, 3, 4]);
        assert_eq!(count_pending_thumb_items(&c).unwrap(), 4);
        assert_eq!(count_pending_ai_items(&c).unwrap(), 4);
        assert_eq!(count_pending_face_items(&c).unwrap(), 4);

        // 隐藏 root2 → 其两项(3,4)从三条枚举 + 三个 count 全排除。
        set_scan_root_hidden(&c, 2, true).unwrap();
        assert_eq!(
            sorted(get_all_pending_thumb_ids(&c).unwrap()),
            vec![1, 2],
            "缩略图 sweep 排除隐藏根"
        );
        assert_eq!(ai_ids(&c), vec![1, 2], "AI 枚举排除隐藏根");
        assert_eq!(face_ids(&c), vec![1, 2], "人脸枚举排除隐藏根");
        assert_eq!(
            count_pending_thumb_items(&c).unwrap(),
            2,
            "缩略图 count 与枚举同口径"
        );
        assert_eq!(
            count_pending_ai_items(&c).unwrap(),
            2,
            "AI count 与枚举同口径"
        );
        assert_eq!(
            count_pending_face_items(&c).unwrap(),
            2,
            "人脸 count 与枚举同口径"
        );

        // 缩略图分页变体(get_pending_thumb_items)同样排除。
        let paged: Vec<i64> = get_pending_thumb_items(&c, 100)
            .unwrap()
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert!(
            paged.iter().all(|id| *id == 1 || *id == 2),
            "缩略图分页变体同样排除隐藏根"
        );

        // 取消隐藏 → 四项复原(被排除期间 status 仍 0,谓词一撤即被枚举捞回)。
        set_scan_root_hidden(&c, 2, false).unwrap();
        assert_eq!(
            sorted(get_all_pending_thumb_ids(&c).unwrap()),
            vec![1, 2, 3, 4],
            "取消隐藏:缩略图待处理集复原"
        );
        assert_eq!(ai_ids(&c), vec![1, 2, 3, 4], "取消隐藏:AI 待处理集复原");
        assert_eq!(face_ids(&c), vec![1, 2, 3, 4], "取消隐藏:人脸待处理集复原");
    }
}
