//! `canonical_layout_sql` 的 EXPLAIN QUERY PLAN 计划锁定 + 基准序内存补序测试。

use super::*;

/// EXPLAIN QUERY PLAN 的 detail 列拼串（参数以 NULL 占位绑定——EXPLAIN 不执行查询体）。
fn plan(c: &Connection, sql: &str) -> String {
    let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let nulls: Vec<rusqlite::types::Value> = (0..stmt.parameter_count())
        .map(|_| rusqlite::types::Value::Null)
        .collect();
    let rows: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows.join(" | ")
}

/// S3.7 计划锁定：默认全量视图必须顺序全表扫（unary + 压制 partial index 匹配）——
/// 索引序遍历逐行随机回表，1M 胖表冷启动实测 6.6s；选择性视图仍走各自索引。
/// 若 push_where_predicates 的基础谓词措辞改动导致 ends_with 失配，本测试即红。
#[test]
fn canonical_default_view_scans_table_not_sort_index() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();

    // 默认全量视图：全表扫，不得走 idx_media_sort。（&[] = 无隐藏根，V21）
    let (sql, _) = canonical_layout_sql(&MediaFilter::default(), &[]);
    assert!(
        !sql.contains("ORDER BY"),
        "canonical 查询不应再下发 ORDER BY: {sql}"
    );
    let p = plan(&c, &sql);
    assert!(
        !p.contains("idx_media_sort"),
        "默认视图不得走 idx_media_sort 随机回表: {p}"
    );

    // 目录视图：保持索引查找（unary + 压制不得波及选择性谓词）。
    // V25 起 idx_media_directory 已删,该查询面由 UNIQUE(directory_id,file_name) 的
    // 隐式索引 sqlite_autoindex_media_items_1 提供——计划锁定同步指向新索引名。
    let dir = MediaFilter {
        directory_id: Some(5),
        ..Default::default()
    };
    let (sql, _) = canonical_layout_sql(&dir, &[]);
    let p = plan(&c, &sql);
    assert!(
        p.contains("sqlite_autoindex_media_items_1"),
        "目录视图应由 (directory_id,file_name) 唯一索引查找: {p}"
    );
}

/// S3.7 对照基准(非门控,--release + --ignored 手动跑):1M **胖表**(thumb_path/
/// thumbhash 全填,sort_datetime 与 rowid 去相关=复现「索引序≠物理序」的随机回表
/// 形态)上,老计划(ORDER BY 吃 idx_media_sort)与新路径(顺序扫+内存置换排序)
/// 同库对跑。清库后的瘦表两者都快,无法区分——本基准是修复有效性的唯一本地证据。
/// cargo test --release --lib bench_canonical_fat_table_1m -- --ignored --nocapture
#[test]
#[ignore]
fn bench_canonical_fat_table_1m() {
    use std::time::Instant;
    const N: i64 = 1_000_000;
    let path = std::env::temp_dir().join(format!("scrollery_bench_fat_{}.db", std::process::id()));
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(p));
    }
    let c = Connection::open(&path).unwrap();
    // 页缓存对齐生产(64MB);sync OFF 仅加速灌数据,不影响读基准。
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=OFF; PRAGMA cache_size=-64000;")
        .unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    c.execute_batch("INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');")
        .unwrap();
    {
        let tx = c.unchecked_transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO directories (id, root_id, rel_path, name) VALUES (?1, 1, ?2, ?3)",
                )
                .unwrap();
            for d in 0..1000i64 {
                stmt.execute(params![d + 10, format!("dir/{d:04}"), format!("{d:04}")])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    {
        let tx = c.unchecked_transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO media_items
                        (id, directory_id, file_name, file_size, file_mtime, file_format,
                         media_type, width, height, sort_datetime, cache_key, availability,
                         thumb_status, thumb_path, thumbhash)
                     VALUES (?1, ?2, ?3, 2048000, 0, 'jpg', 'image', 1600, 1200, ?4, 0,
                             'online', 1, ?5, ?6)",
                )
                .unwrap();
            // LCG 伪随机时间戳:索引序与 rowid/物理序完全去相关(免 rand 依赖)。
            let mut seed: u64 = 0x243F_6A88_85A3_08D3;
            for i in 0..N {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let ts = ((seed >> 16) & 0x3FFF_FFFF) as i64;
                let thumb = format!(
                    "C:/Users/x/AppData/Local/scrollery/cache/thumbs/{:02x}/{:016x}.webp",
                    i & 0xff,
                    seed
                );
                stmt.execute(params![
                    i + 1,
                    (i % 1000) + 10,
                    format!("IMG_{i:07}.jpg"),
                    ts,
                    thumb,
                    vec![7u8; 25],
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let db_mb = std::fs::metadata(&path)
        .map(|m| m.len() / 1_048_576)
        .unwrap_or(0);
    println!("fat db: {db_mb}MB, {N} rows");

    // 老计划 SQL(仅作历史对照,非生产路径):ORDER BY 吃 idx_media_sort → 逐行随机回表。
    let old_sql = "SELECT m.id, m.width, m.height, m.file_size, m.sort_datetime, m.file_format, m.media_type, m.is_live_photo, m.duration_ms, m.thumb_status, m.thumb_path, m.thumbhash, m.is_favorited, m.directory_id as dir_id, m.availability, m.rating, m.color_label, NULL as similarity, m.cache_key FROM media_items m WHERE m.is_deleted=0 AND m.companion_of IS NULL ORDER BY m.sort_datetime DESC, m.id DESC";
    for round in 1..=2 {
        let t = Instant::now();
        let mut stmt = c.prepare(old_sql).unwrap();
        let n = stmt
            .query_map([], map_layout_item)
            .unwrap()
            .filter(|r| r.is_ok())
            .count();
        println!(
            "old(index-order) round{round}: {:.0}ms ({n} rows)",
            t.elapsed().as_secs_f64() * 1e3
        );
    }
    for round in 1..=2 {
        let t = Instant::now();
        let items = query_layout_items_canonical(&c, &MediaFilter::default()).unwrap();
        println!(
            "new(scan+sort) round{round}: {:.0}ms ({} rows)",
            t.elapsed().as_secs_f64() * 1e3,
            items.len()
        );
        // 序等价全查:严格 (ts DESC, id DESC)。
        assert!(items
            .windows(2)
            .all(|w| (w[1].sort_datetime, w[1].id) < (w[0].sort_datetime, w[0].id)));
    }
    drop(c);
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(p));
    }
}

/// S3.7:sort_canonical 输出序与被替换的 SQL ORDER 等价——(ts DESC, id DESC),
/// 同 ts 由 id DESC 裁决。
#[test]
fn sort_canonical_matches_sql_order_semantics() {
    let mk = |id: i64, ts: i64| crate::db::models::LayoutItem {
        id,
        width: 100,
        height: 100,
        file_size: 0,
        sort_datetime: ts,
        file_format: "jpg".into(),
        media_type: "image".into(),
        is_live_photo: false,
        duration_ms: None,
        thumb_status: 0,
        thumb_path: None,
        thumbhash: None,
        is_favorited: false,
        rating: 0,
        color_label: 0,
        availability: "online".into(),
        dir_id: None,
        similarity: None,
        cache_key: 0,
    };
    let items = vec![mk(1, 100), mk(3, 200), mk(2, 200), mk(4, 50)];
    let sorted = sort_canonical(items);
    let order: Vec<i64> = sorted.iter().map(|it| it.id).collect();
    assert_eq!(order, vec![3, 2, 1, 4], "(ts DESC, id DESC) 序");
}
