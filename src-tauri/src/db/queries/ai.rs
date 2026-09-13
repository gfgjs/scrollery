//! AI(CLIP embedding)域 DAO:向量写读、ai_status 状态机计数/复位/同步、pending 队列
//! (T 线拆分自 queries.rs,SQL 与行为不变;与 face 状态机同构但有意不抽泛型,设计 §4.1)。
//! `reset_error_items_batched` 为 ai/faces 共用 helper,归本域并以
//! `pub(in crate::db::queries)` 供 faces 定向引用(设计 §4.3)。

use rusqlite::{params, Connection};

// 「exotic 接管门控」谓词由 exotic 域持有(P0 增补裁决);format! 内联捕获须裸名,故 use 引入。
use super::exotic::{NOT_BLOCKED_BY_EXOTIC, NOT_BLOCKED_BY_EXOTIC_M};
// 「隐藏根排除」谓词由 scan 域持有(V21):get_pending 走 JOIN 用 _M(m 别名)、count 裸表用无缀版。
use super::scan::{EXCLUDE_HIDDEN_ROOTS, EXCLUDE_HIDDEN_ROOTS_M};
use crate::error::{AppError, Result};

// ── AI 嵌入向量 ─────────────────────────────────────────────────────────────

/// 插入或更新媒体项的 AI 嵌入向量。
pub fn upsert_ai_embedding(
    conn: &Connection,
    item_id: i64,
    model_name: &str,
    embedding: &[u8],
    version: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ai_embeddings (item_id, model_name, embedding, version)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(item_id, model_name) DO UPDATE SET
             embedding=excluded.embedding,
             version=excluded.version,
             created_at=strftime('%s','now')",
        params![item_id, model_name, embedding, version],
    )?;
    Ok(())
}

/// AI 条件完成写(2026-07-10 审查 X1,对齐 `update_thumb_result` 的条件写范式):
/// 仅当 `media_items.cache_key` 仍等于任务**领取时的快照**才落 Done+向量。不符即该项已被
/// SourceChanged 失效(status→0、换 cache_key、删旧向量)——迟到的 Writer flush 整项落空,
/// 下轮重分析。worker 回声的 fingerprint 只防「worker 读错文件」,防不了 host 落库陈旧,
/// 这一步才是闭环(缩略图线 P1-4 同款病灶的既定修法,此前未复制到 AI/face)。
/// 返回实际完成数(len - 返回值 = 被失效跳过数)。
pub fn batch_finish_ai_items(
    conn: &Connection,
    rows: &[(i64, String, Vec<u8>, i64, i64)], // (item_id, model_name, embedding, version, cache_key 快照)
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut done = 0usize;
    for (item_id, model_name, embedding, version, cache_key) in rows {
        let fresh = tx.execute(
            "UPDATE media_items SET ai_status=2, updated_at=strftime('%s','now')
              WHERE id=?1 AND cache_key=?2",
            params![item_id, cache_key],
        )?;
        if fresh == 0 {
            // 已失效(或行已删):该向量由旧内容的缓存算出,必须丢弃。
            continue;
        }
        tx.execute(
            "INSERT INTO ai_embeddings (item_id, model_name, embedding, version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(item_id, model_name) DO UPDATE SET
                 embedding=excluded.embedding,
                 version=excluded.version,
                 created_at=strftime('%s','now')",
            params![item_id, model_name, embedding, version],
        )?;
        done += 1;
    }
    tx.commit()?;
    Ok(done)
}

/// X1 条件状态写(Error 等失败路径):仅 `cache_key` 未变的行生效——失效项不得被迟到的
/// Error 覆盖(它已回 Pending 等待按新内容重分析)。返回生效行数。
pub fn batch_update_ai_status_guarded(
    conn: &Connection,
    items: &[(i64, i64)], // (item_id, cache_key 快照)
    status: i64,
) -> Result<usize> {
    if items.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut n = 0usize;
    for (id, cache_key) in items {
        n += tx.execute(
            "UPDATE media_items SET ai_status=?1, updated_at=strftime('%s','now')
              WHERE id=?2 AND cache_key=?3",
            params![status, id, cache_key],
        )?;
    }
    tx.commit()?;
    Ok(n)
}

/// 获取给定模型的所有嵌入向量（用于内存余弦搜索）。
pub fn get_all_embeddings(conn: &Connection, model_name: &str) -> Result<Vec<(i64, Vec<u8>)>> {
    let mut stmt =
        conn.prepare("SELECT item_id, embedding FROM ai_embeddings WHERE model_name=?1")?;
    let rows = stmt.query_map(params![model_name], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 统计某个模型已经写入的向量数量；这是语义搜索真实可用的覆盖数。
pub fn count_embeddings_for_model(conn: &Connection, model_name: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM ai_embeddings WHERE model_name=?1",
        params![model_name],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 批量更新多个媒体项的 `ai_status`(Producer 标 Processing 等**领取路径**专用;完成/失败
/// 落库必须走 X1 条件写 `batch_finish_ai_items` / `batch_update_ai_status_guarded`)。
pub fn batch_update_ai_status(conn: &Connection, item_ids: &[i64], status: i64) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for &id in item_ids {
        tx.execute(
            "UPDATE media_items SET ai_status=?1, updated_at=strftime('%s','now') WHERE id=?2",
            params![status, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 获取 `ai_status=0`（待处理）的项，按最近顺序，最多 `limit` 条。
///
/// 一个待 CLIP 分析的图像项，携带流水线选择**最廉价且足够**解码源（AI 缓存 → 常规缩略图 → 原图）
/// 所需的全部信息。
pub struct PendingAiItem {
    pub id: i64,
    /// 源文件绝对路径(缺 ai_cache 时现场派生的解码源,T18)。
    pub abs_path: String,
    pub file_format: String,
    /// `cache_key` 定位磁盘上的 AI 分析缓存文件(`ai_cache_path(cache_dir, cache_key)`)。
    /// worker 端解码的唯一源;按**文件存在性**发现(而非 `media_derivations` 行),
    /// 故缩略图生成时顺带产出的缓存(一次解码两份产物)同样能被发现。
    pub cache_key: i64,
}

/// 取一批待 CLIP 分析的图像(T16:worker 端按 cache_key 读 ai_cache 解码,缩略图/尺寸
/// 提示字段已随进程内解码源决策退场)。
pub fn get_pending_ai_items(conn: &Connection, limit: i64) -> Result<Vec<PendingAiItem>> {
    let sql = format!(
        "SELECT m.id,
                CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                     ELSE r.path || '/' || d.rel_path || '/' || m.file_name
                END,
                m.file_format, m.cache_key
         FROM media_items m
         JOIN directories d ON m.directory_id = d.id
         JOIN scan_roots r ON d.root_id = r.id
         WHERE m.ai_status=0 AND m.is_deleted=0 AND m.media_type='image' {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ORDER BY m.created_at DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(PendingAiItem {
            id: row.get(0)?,
            abs_path: row.get(1)?,
            file_format: row.get(2)?,
            cache_key: row.get(3)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 统计待处理的 AI 项数量。
pub fn count_pending_ai_items(conn: &Connection) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM media_items
         WHERE ai_status=0 AND is_deleted=0 AND media_type='image' {NOT_BLOCKED_BY_EXOTIC} {EXCLUDE_HIDDEN_ROOTS}"
    );
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// 统计已分析的 AI 项数量（status=2 或 3）。
pub fn count_analyzed_ai_items(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_items WHERE ai_status IN (2, 3) AND is_deleted=0 AND media_type='image'",
        [],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 统计所有的 AI 项数量。
pub fn count_total_ai_items(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_items WHERE is_deleted=0 AND media_type='image'",
        [],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// `ai_status=3`(Error)项数(2026-07-10 审查 A11):谓词与 `count_total_ai_items` 对齐。
pub fn count_error_ai_items(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_items WHERE ai_status=3 AND is_deleted=0 AND media_type='image'",
        [],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 非破坏重试(2026-07-10 审查 F9):Error → Pending 分批复位,不触碰任何已完成向量。
/// 之后走既有 start 复跑失败项。分批理由同 `reset_ai_embeddings`(批间释放写锁)。
pub fn reset_error_ai_items(db: &std::sync::Mutex<Connection>) -> Result<usize> {
    reset_error_items_batched(db, "ai_status", 10_000)
}

pub(in crate::db::queries) fn reset_error_items_batched(
    db: &std::sync::Mutex<Connection>,
    column: &str,
    batch: i64,
) -> Result<usize> {
    // column 只来自 ai/faces 两域调用点的常量,非外部输入;断言防未来误用成注入面。
    assert!(matches!(column, "ai_status" | "face_status"));
    let mut total = 0usize;
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            &format!(
                "UPDATE media_items SET {column}=0, updated_at=strftime('%s','now')
                  WHERE rowid IN (SELECT rowid FROM media_items
                                  WHERE {column}=3 AND media_type='image' LIMIT ?1)"
            ),
            params![batch],
        )?;
        total += n;
        if (n as i64) < batch {
            break;
        }
    }
    Ok(total)
}

/// 释放 AI 流水线已领取（ai_status=Processing）但未完成的项——例如崩溃、强退、暂停或停止后。
/// 将其设回 Pending，使后续运行能续传而非永久搁置（问题7）。返回恢复的数量。
pub fn reset_processing_ai_items(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE media_items SET ai_status=0 WHERE ai_status=1 AND media_type='image'",
        [],
    )
    .map_err(AppError::from)
}

/// 重置所有 AI 嵌入向量 — 将 ai_status 设回 0 并删除嵌入向量。
///
/// R2-6 分批化:每批独立语句/事务、**批间释放 `db_writer`**——竞争在 Rust Mutex 层
/// (交互写如收藏/评分排队等的正是这把锁,WAL 只保护读),百万行全表 UPDATE 单事务会
/// 秒级独占写锁,故签名改收 `&Mutex<Connection>`。放弃单事务原子性是安全的:调用方
/// 已先取消流水线,且下次 start_ai_analysis 的 sync_ai_status_for_model 会按真实向量
/// 覆盖重新对账(自愈);中间态最多令个别项多重跑,不产生错数据。
pub fn reset_ai_embeddings(db: &std::sync::Mutex<Connection>, model_name: &str) -> Result<()> {
    reset_ai_embeddings_batched(db, model_name, 10_000)
}

fn reset_ai_embeddings_batched(
    db: &std::sync::Mutex<Connection>,
    model_name: &str,
    batch: i64,
) -> Result<()> {
    // 先删该模型向量(2KB 级 BLOB 大表,同样分批;单语句自成事务)。
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "DELETE FROM ai_embeddings WHERE rowid IN
               (SELECT rowid FROM ai_embeddings WHERE model_name=?1 LIMIT ?2)",
            params![model_name, batch],
        )?;
        if (n as i64) < batch {
            break;
        }
    }
    // 再分批清状态;`ai_status<>0` 谓词既跳过本就 Pending 的行(免白改 updated_at 与
    // 索引翻搅),也是循环的终止条件。
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "UPDATE media_items SET ai_status=0, updated_at=strftime('%s','now')
             WHERE rowid IN (SELECT rowid FROM media_items
                             WHERE media_type='image' AND ai_status<>0 LIMIT ?1)",
            params![batch],
        )?;
        if (n as i64) < batch {
            break;
        }
    }
    Ok(())
}

/// 将 `ai_status` 按某模型的向量覆盖重新同步：已有该 `model_name` 向量的项 → Done(2)，其余 →
/// Pending(0)。用于**切换激活模型**。ai_status 是单列全局状态（非按模型）且流水线只查 status=0，
/// 故切换后须据新模型覆盖重置 —— 已嵌入项跳过、缺失项重新分析。其它模型的向量保留（DB 以
/// `(item_id, model_name)` 为键），故切回某模型零成本。
///
/// R2-6:只改「现状 ≠ 目标」的行并分批(签名改 Mutex 的理由同 reset_ai_embeddings)。
/// 语义与原全表 CASE UPDATE 严格等价(任何现状≠目标的行都会被改),但常态「已同步」
/// 时零写——原实现在**每次** start_ai_analysis 与模型切换时把全表每行重写一遍
/// (白改 updated_at + 索引翻搅)。
pub fn sync_ai_status_for_model(db: &std::sync::Mutex<Connection>, model_name: &str) -> Result<()> {
    sync_ai_status_batched(db, model_name, 10_000)
}

fn sync_ai_status_batched(
    db: &std::sync::Mutex<Connection>,
    model_name: &str,
    batch: i64,
) -> Result<()> {
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "UPDATE media_items SET
                ai_status = CASE
                    WHEN id IN (SELECT item_id FROM ai_embeddings WHERE model_name=?1) THEN 2
                    ELSE 0 END,
                updated_at = strftime('%s','now')
             WHERE rowid IN (
                 SELECT rowid FROM media_items
                 WHERE media_type='image' AND is_deleted=0
                   AND ai_status <> (CASE WHEN id IN
                        (SELECT item_id FROM ai_embeddings WHERE model_name=?1) THEN 2 ELSE 0 END)
                 LIMIT ?2)",
            params![model_name, batch],
        )?;
        if (n as i64) < batch {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod r2_6_query_tests {
    use std::sync::Mutex;

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

    fn ai_status(c: &Connection, id: i64) -> i64 {
        c.query_row(
            "SELECT ai_status FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// reset 分批:batch=2 迫使多轮循环——目标模型向量删净、他模型保留、
    /// 全部 image ai_status 归 0、非 image 不动。
    #[test]
    fn reset_ai_embeddings_batched_clears_and_preserves() {
        let c = mem_db();
        for id in 1..=5 {
            add_item(&c, id, 10, "image", 0, 0, 0, None);
        }
        add_item(&c, 9, 10, "video", 0, 0, 0, None);
        c.execute_batch(
            "UPDATE media_items SET ai_status=2 WHERE id IN (1,2,3,4,5);
             UPDATE media_items SET ai_status=2 WHERE id=9;
             INSERT INTO ai_embeddings (item_id, model_name, embedding) VALUES
                 (1,'m1',x'00'),(2,'m1',x'00'),(3,'m1',x'00'),(1,'m2',x'00');",
        )
        .unwrap();

        let db = Mutex::new(c);
        super::reset_ai_embeddings_batched(&db, "m1", 2).unwrap();

        let c = db.lock().unwrap();
        let m1: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM ai_embeddings WHERE model_name='m1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let m2: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM ai_embeddings WHERE model_name='m2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(m1, 0, "目标模型向量删净");
        assert_eq!(m2, 1, "他模型向量保留");
        for id in 1..=5 {
            assert_eq!(ai_status(&c, id), 0, "image 全部归 0");
        }
        assert_eq!(ai_status(&c, 9), 2, "非 image 不动(既有 media_type 过滤)");
    }

    /// sync 分批 + 不一致谓词:错的行被纠正,已同步行零写(updated_at 不变)。
    #[test]
    fn sync_ai_status_batched_targets_only_mismatched() {
        let c = mem_db();
        add_item(&c, 1, 10, "image", 0, 0, 0, None); // 有向量但 ai=0 → 应改 2
        add_item(&c, 2, 10, "image", 0, 0, 0, None); // 无向量 ai=2 → 应改 0
        add_item(&c, 3, 10, "image", 0, 0, 0, None); // 有向量且 ai=2 → 已同步,不动
        add_item(&c, 4, 10, "image", 0, 1, 0, None); // 软删,有向量 ai=0 → is_deleted 过滤,不动
        c.execute_batch(
            "INSERT INTO ai_embeddings (item_id, model_name, embedding) VALUES
                 (1,'m1',x'00'),(3,'m1',x'00'),(4,'m1',x'00');
             UPDATE media_items SET ai_status=2 WHERE id IN (2,3);
             UPDATE media_items SET updated_at=111 WHERE id IN (1,2,3,4);",
        )
        .unwrap();

        let db = Mutex::new(c);
        super::sync_ai_status_batched(&db, "m1", 1).unwrap();

        let c = db.lock().unwrap();
        let upd = |id: i64| -> i64 {
            c.query_row(
                "SELECT updated_at FROM media_items WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(ai_status(&c, 1), 2);
        assert_eq!(ai_status(&c, 2), 0);
        assert_eq!(ai_status(&c, 3), 2);
        assert_eq!(ai_status(&c, 4), 0, "软删行不参与同步(保持原值)");
        assert_ne!(upd(1), 111, "被纠正的行 bump updated_at");
        assert_ne!(upd(2), 111);
        assert_eq!(upd(3), 111, "已同步行零写(不再翻搅 updated_at/索引)");
        assert_eq!(upd(4), 111);
    }
}
