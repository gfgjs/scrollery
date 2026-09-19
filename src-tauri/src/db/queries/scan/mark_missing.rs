//! 缺失标记(mark_missing):三重守门差集,独立高密度契约注释块。
//! (D 线从 `scan.rs` 拆出,SQL/事务边界不变。)

use rusqlite::{params, Connection};
use std::collections::HashSet;

use crate::error::Result;

/// 缺失检测三重守门差集（Part2 §3.2.2）：把「在线卷上、本 scan_root 子树内、本次未出现」的项
/// 标 `availability='missing'`（**绝不写 is_deleted**，与用户回收站正交）。返回标记数。
///
/// 三重守门（缺一不可）：
///   - 守门1：仅 `online_volume_ids` 内的卷（离线卷的项绝不参与——离线≠删除）。
///   - 守门2：仅本 scan_root 子树（经 directories.root_id 关联；media_items 无 scan_root 列）。
///   - 守门3：本次扫描 `seen_ids` 之外（`seen` 须含 Unchanged，否则未变更文件被误标）。
///
/// `dry_run=true` 时只 `SELECT count(*)`（返回**将标记数**）、绝不 UPDATE——供灰度核对 / 可观测。
/// seen/online 用连接级 TEMP 表装载（百万级友好：`NOT IN` 改 TEMP 表子查询，避免巨型字面量列表；
/// 全程参数绑定）。生产扫描的表名还带本轮 generation，避免同根重扫互相清空；**调用前置**（由调用方
/// 保证）：扫描完整（WalkReport.complete）+ 卷在线已复查（TOCTOU）。
///
/// 生产扫描走 [`init_seen_table_for_run`]+[`SeenWriter::new_for_run`]+[`mark_missing_preloaded_for_run`]
/// 的流式路径:seen id 边扫边入带 generation 的 TEMP 表,收尾不再整体搬运 HashSet;本函数保留为测试/兼容入口。
pub fn mark_missing(
    conn: &Connection,
    root_id: i64,
    online_volume_ids: &[i64],
    seen_ids: &std::collections::HashSet<i64>,
    dry_run: bool,
) -> Result<usize> {
    init_seen_table(conn, root_id)?;
    let seen = SeenWriter::new(root_id);
    for id in seen_ids {
        seen.insert(conn, *id)?;
    }
    mark_missing_preloaded(conn, root_id, online_volume_ids, dry_run)
}

/// 本根 seen 集的连接级 TEMP 表名。
///
/// **按 root + generation 后缀隔离**(P0):`DbWriter` 是全局单写连接,不同根的扫描可并发、同根
/// 重扫也不等待旧轮退出。若共用一张表,新轮初始化会把旧轮已流式写入的 seen 抹掉,任一轮收尾
/// 都可能拿混合集合做差集。表名仅由内部的 i64 root_id/u64 generation 派生,format! 无注入面。
fn seen_table_name(root_id: i64) -> String {
    format!("_mm_seen_r{root_id}")
}

fn seen_table_name_for_run(root_id: i64, generation: u64) -> String {
    format!("_mm_seen_r{root_id}_g{generation}")
}

fn online_table_name_for_run(root_id: i64, generation: u64) -> String {
    format!("_mm_online_r{root_id}_g{generation}")
}

/// 初始化/清空本根的 seen TEMP 表。每次扫描开始调用:清空同时覆盖上一轮取消/出错残留的旧行,
/// 并保证同连接复用不携带上一轮 seen。
pub fn init_seen_table(conn: &Connection, root_id: i64) -> Result<()> {
    init_seen_table_named(conn, &seen_table_name(root_id))
}

/// 初始化指定扫描代次的 seen TEMP 表。只创建/清空本轮表,不会触碰同根旧轮或其它根。
pub fn init_seen_table_for_run(conn: &Connection, root_id: i64, generation: u64) -> Result<()> {
    init_seen_table_named(conn, &seen_table_name_for_run(root_id, generation))
}

fn init_seen_table_named(conn: &Connection, table: &str) -> Result<()> {
    conn.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table}(id INTEGER PRIMARY KEY);
         DELETE FROM {table};"
    ))?;
    Ok(())
}

/// 清理由指定扫描代次创建的 TEMP 表。用于取消/错误路径；绝不清理其它轮次的表。
pub fn cleanup_scan_temp_tables_for_run(
    conn: &Connection,
    root_id: i64,
    generation: u64,
) -> Result<()> {
    let seen_table = seen_table_name_for_run(root_id, generation);
    let online_table = online_table_name_for_run(root_id, generation);
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS {seen_table};
         DROP TABLE IF EXISTS {online_table};"
    ))?;
    Ok(())
}

/// 本根 seen 的流式写入器:持有按根派生好的 INSERT SQL(经 prepare_cached 复用),
/// 扫描期逐 id 写入——热路径免每文件重建表名字符串,替代「内存 HashSet 收尾整表搬运」。
pub struct SeenWriter {
    sql: String,
}

impl SeenWriter {
    pub fn new(root_id: i64) -> Self {
        Self::with_table(seen_table_name(root_id))
    }

    /// 为指定扫描代次构造 seen 写入器；同根不同代次使用不同 TEMP 表。
    pub fn new_for_run(root_id: i64, generation: u64) -> Self {
        Self::with_table(seen_table_name_for_run(root_id, generation))
    }

    fn with_table(table: String) -> Self {
        Self {
            sql: format!("INSERT OR IGNORE INTO {}(id) VALUES (?1)", table),
        }
    }

    /// 写入单个 seen id(幂等,INSERT OR IGNORE)。生产快扫在批事务内/quick 回填时调用。
    pub fn insert(&self, conn: &Connection, id: i64) -> Result<()> {
        conn.prepare_cached(&self.sql)?.execute(params![id])?;
        Ok(())
    }
}

/// 假定本根 seen TEMP 表已由扫描期流式填充的缺失检测(生产路径)。只装载在线卷集并跑差集,
/// **不清空 seen**。表缺失(非 fast_scan 预装载路径误调)时保守返回 0——宁可少标,
/// 绝不拿空 seen 集把在线项误标 missing。真实差集(!dry_run)完成后 DROP 本根 seen 表:
/// `temp_store=MEMORY` 下避免多根各表在写连接生命周期内常驻累积内存(各根各表,DROP 仅释放本根)。
pub fn mark_missing_preloaded(
    conn: &Connection,
    root_id: i64,
    online_volume_ids: &[i64],
    dry_run: bool,
) -> Result<usize> {
    mark_missing_preloaded_with_tables(
        conn,
        root_id,
        &seen_table_name(root_id),
        "_mm_online",
        online_volume_ids,
        dry_run,
    )
}

/// 指定扫描代次的生产收尾。seen 与 online 均只使用本轮 TEMP 表；真实差集完成后只清理本轮表。
pub fn mark_missing_preloaded_for_run(
    conn: &Connection,
    root_id: i64,
    generation: u64,
    online_volume_ids: &[i64],
    dry_run: bool,
) -> Result<usize> {
    mark_missing_preloaded_with_tables(
        conn,
        root_id,
        &seen_table_name_for_run(root_id, generation),
        &online_table_name_for_run(root_id, generation),
        online_volume_ids,
        dry_run,
    )
}

fn mark_missing_preloaded_with_tables(
    conn: &Connection,
    root_id: i64,
    seen_table: &str,
    online_table: &str,
    online_volume_ids: &[i64],
    dry_run: bool,
) -> Result<usize> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name=?1",
        params![seen_table],
        |r| r.get(0),
    )?;
    if exists == 0 {
        tracing::warn!(
            "seen 表 {seen_table} 缺失(非 fast_scan 预装载路径)→ 跳过缺失检测(不标) | root_id={root_id}"
        );
        return Ok(0);
    }
    conn.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {online_table}(id INTEGER PRIMARY KEY);
         DELETE FROM {online_table};"
    ))?;
    {
        let insert_sql = format!("INSERT OR IGNORE INTO {online_table}(id) VALUES (?1)");
        let mut o = conn.prepare_cached(&insert_sql)?;
        for v in online_volume_ids {
            o.execute(params![v])?;
        }
    }
    let n = run_diff(conn, root_id, seen_table, online_table, dry_run)?;
    // online 只服务本次差集；dry_run 也要清理，避免连接级 TEMP 状态残留。
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {online_table};"))?;
    if !dry_run {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {seen_table};"))?;
    }
    Ok(n)
}

/// 三重守门差集本体(legacy 与 preloaded 共用):在线卷集/seen TEMP 表名由调用方传入。
fn run_diff(
    conn: &Connection,
    root_id: i64,
    seen_table: &str,
    online_table: &str,
    dry_run: bool,
) -> Result<usize> {
    // 守门2(子树) + 守门1(在线卷 TEMP) + is_deleted=0 + 已是 missing 不重标 + 守门3(¬seen TEMP)。
    // 注：volume_id 为 NULL 的孤儿行 `NULL IN (...)`→非真→天然排除（不误标孤儿）。
    if dry_run {
        let sql = format!(
            "SELECT count(*) FROM media_items
              WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)
                AND volume_id IN (SELECT id FROM {online_table})
                AND is_deleted = 0
                AND availability != 'missing'
                AND id NOT IN (SELECT id FROM {seen_table})"
        );
        let n: i64 = conn.query_row(&sql, params![root_id], |r| r.get(0))?;
        Ok(n as usize)
    } else {
        // 先在同一事务内锁定本次新变为 missing 的 id。除了自身摘要，这些 id 可能是
        // Live Photo companion；其主项的逻辑单元代次也必须一起推进，不能让旧组合摘要
        // 在 companion 离线期间继续可见。
        let ids_sql = format!(
            "SELECT id FROM media_items
              WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)
                AND volume_id IN (SELECT id FROM {online_table})
                AND is_deleted = 0
                AND availability != 'missing'
                AND id NOT IN (SELECT id FROM {seen_table})"
        );
        let tx = conn.unchecked_transaction()?;
        let newly_missing: Vec<i64> = {
            let mut stmt = tx.prepare(&ids_sql)?;
            let rows = stmt.query_map(params![root_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let sql = format!(
            "UPDATE media_items SET availability = 'missing',
                       source_revision = source_revision + 1,
                       updated_at = strftime('%s','now')
              WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)
                AND volume_id IN (SELECT id FROM {online_table})
                AND is_deleted = 0
                AND availability != 'missing'
                AND id NOT IN (SELECT id FROM {seen_table})"
        );
        tx.execute(&sql, params![root_id])?;
        let newly_missing_set = newly_missing.iter().copied().collect::<HashSet<_>>();
        let mut affected_parents = HashSet::new();
        for item_id in &newly_missing {
            // Missing 是新的源状态：旧精确摘要不能在文件离线期间继续代表当前文件。
            tx.execute("DELETE FROM dedup_index WHERE item_id=?1", params![item_id])?;
            let parent: Option<i64> = tx.query_row(
                "SELECT companion_of FROM media_items WHERE id=?1",
                params![item_id],
                |row| row.get::<_, Option<i64>>(0),
            )?;
            if let Some(parent_id) = parent {
                // 若主项也在本轮变为 missing，它已经在上面的批量 UPDATE 中推进过；
                // 同一逻辑单元只推进一次，避免主项+companion 同时缺失时代次虚增。
                if !newly_missing_set.contains(&parent_id) {
                    affected_parents.insert(parent_id);
                }
            }
        }
        for parent_id in affected_parents {
            tx.execute(
                "UPDATE media_items SET source_revision=source_revision+1,
                        updated_at=strftime('%s','now')
                  WHERE id=?1",
                params![parent_id],
            )?;
            tx.execute(
                "DELETE FROM dedup_index WHERE item_id=?1",
                params![parent_id],
            )?;
        }
        tx.commit()?;
        Ok(newly_missing.len())
    }
}

#[cfg(test)]
mod mark_missing_tests {
    use super::*;
    use std::collections::HashSet;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // 两个 scan_root + 各一目录（root1→dir10，root2→dir20）。
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1'), (20, 2, '', 'r2');",
        )
        .unwrap();
        c
    }

    fn add(c: &Connection, id: i64, dir: i64, vol: Option<i64>, avail: &str, is_deleted: i64) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id, availability, is_deleted)
             VALUES (?1, ?2, ?3, 0, 0, 'jpg', 'image', 0, 0, 0, 0, ?4, ?5, ?6)",
            params![id, dir, format!("{id}.jpg"), vol, avail, is_deleted],
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

    fn source_revision(c: &Connection, id: i64) -> i64 {
        c.query_row(
            "SELECT source_revision FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 三重守门矩阵：只有「在线卷 + 本根子树 + 未 seen + 未删」的项被标 missing，其余四类各被一道闸拦住。
    #[test]
    fn three_gates_mark_only_genuinely_missing() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0); // ✓ 应标：在线卷5 + root1 + 不在 seen
        add(&c, 101, 10, Some(5), "online", 0); // ✗ 在 seen（守门3）
        add(&c, 102, 10, Some(6), "online", 0); // ✗ 卷6 不在线（守门1）
        add(&c, 103, 20, Some(5), "online", 0); // ✗ root2 子树（守门2）
        add(&c, 104, 10, Some(5), "online", 1); // ✗ is_deleted=1（用户回收站）
        add(&c, 105, 10, None, "online", 0); // ✗ 孤儿 volume_id=NULL（天然排除）

        let seen: HashSet<i64> = HashSet::from([101]);
        let n = mark_missing(&c, 1, &[5], &seen, false).unwrap();
        assert_eq!(n, 1, "仅 1 项真缺失");

        assert_eq!(avail(&c, 100), "missing", "真缺失项应标 missing");
        for id in [101, 102, 103, 104, 105] {
            assert_eq!(
                avail(&c, id),
                "online",
                "id={id} 应被某道闸拦住、保持 online"
            );
        }
        // is_deleted 项绝不被 availability 逻辑改动其删除态。
        let d: i64 = c
            .query_row("SELECT is_deleted FROM media_items WHERE id=104", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(d, 1);
    }

    /// dry_run：返回将标记数但 DB 零改动。
    #[test]
    fn dry_run_counts_without_writing() {
        let c = mem_db();
        add(&c, 200, 10, Some(5), "online", 0);
        add(&c, 201, 10, Some(5), "online", 0);
        let seen: HashSet<i64> = HashSet::new(); // 全部未出现

        let n = mark_missing(&c, 1, &[5], &seen, true).unwrap();
        assert_eq!(n, 2, "dry_run 应返回将标记数 2");
        // DB 未变。
        assert_eq!(avail(&c, 200), "online");
        assert_eq!(avail(&c, 201), "online");
    }

    /// 离线卷（online 集为空）：一项都不标——离线 ≠ 删除的第二层冗余防护。
    #[test]
    fn empty_online_set_marks_nothing() {
        let c = mem_db();
        add(&c, 300, 10, Some(5), "online", 0);
        let seen: HashSet<i64> = HashSet::new();
        let n = mark_missing(&c, 1, &[], &seen, false).unwrap();
        assert_eq!(n, 0, "无在线卷 → 一项不标");
        assert_eq!(avail(&c, 300), "online");
    }

    /// 多在线卷：online 集含多卷时，任一在线卷上的未 seen 项都被标；第三卷离线则不标。
    /// 压 `_mm_online` TEMP 表多行 + `volume_id IN (SELECT ...)` 子查询路径。
    #[test]
    fn multiple_online_volumes_all_covered() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0); // 在线卷5 → 标
        add(&c, 101, 10, Some(7), "online", 0); // 在线卷7 → 标
        add(&c, 102, 10, Some(9), "online", 0); // 卷9 离线（不在 online 集）→ 不标
        let seen: HashSet<i64> = HashSet::new();

        let n = mark_missing(&c, 1, &[5, 7], &seen, false).unwrap();
        assert_eq!(n, 2, "两在线卷上的未 seen 项均被标");
        assert_eq!(avail(&c, 100), "missing");
        assert_eq!(avail(&c, 101), "missing");
        assert_eq!(avail(&c, 102), "online", "离线卷9 上的项不受影响（守门1）");
    }

    /// 同连接复用：第二次调用必须先清空 TEMP 表，不被首次的 seen 集污染。
    /// 这是连接池复用下的真实数据安全点——清空逻辑若失效，二次扫描会用陈旧集合 → 误标/漏标。
    #[test]
    fn reuse_same_connection_clears_temp_state() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0);
        add(&c, 101, 10, Some(5), "online", 0);

        // 第一次：seen={100} → 仅 101 缺失。
        let seen1: HashSet<i64> = HashSet::from([100]);
        let n1 = mark_missing(&c, 1, &[5], &seen1, false).unwrap();
        assert_eq!(n1, 1);
        assert_eq!(avail(&c, 101), "missing");

        // 恢复 101，再以「相反」的 seen 集第二次调用（dry_run 纯验集合隔离、不改库）。
        c.execute(
            "UPDATE media_items SET availability='online' WHERE id=101",
            [],
        )
        .unwrap();
        // 第二次：seen={101} → 仅 100 应计。若 TEMP 未清空，残留 seen={100} 会污染 → 算成 0。
        let seen2: HashSet<i64> = HashSet::from([101]);
        let n2 = mark_missing(&c, 1, &[5], &seen2, true).unwrap();
        assert_eq!(n2, 1, "二次调用仅 100 缺失；若被首次 seen 污染则会误算成 0");
    }

    /// 查询本根 seen TEMP 表当前是否存在(收口 DROP 语义的观测口)。
    fn seen_table_exists(c: &Connection, root_id: i64) -> bool {
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name=?1",
                params![seen_table_name(root_id)],
                |r| r.get(0),
            )
            .unwrap();
        n > 0
    }

    fn temp_table_exists(c: &Connection, table: &str) -> bool {
        c.query_row(
            "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name=?1",
            params![table],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn temp_id_count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// 生产路径:`init_seen_table` + 扫描期 `SeenWriter` 流式写入 + `mark_missing_preloaded`
    /// 不经过 HashSet;真实差集完成后 DROP 本根表(内存即时释放,不复刻对照线的常驻缺陷);
    /// 复跑走「缺表保守跳过」;下一轮 `init_seen_table` 重建后差集口径回到空 seen。
    #[test]
    fn preloaded_streaming_path_drops_seen_after_real_diff() {
        let c = mem_db();
        for id in 1..=10i64 {
            add(&c, id, 10, Some(5), "online", 0);
        }
        init_seen_table(&c, 1).unwrap();
        let seen = SeenWriter::new(1);
        for id in 1..=5i64 {
            seen.insert(&c, id).unwrap();
        }
        let n = mark_missing_preloaded(&c, 1, &[5], false).unwrap();
        assert_eq!(n, 5, "id 6..10 未 seen → 应标 missing");
        for id in 6..=10i64 {
            assert_eq!(avail(&c, id), "missing");
        }
        assert!(
            !seen_table_exists(&c, 1),
            "真实差集完成后本根 seen 表应已 DROP(免 temp_store=MEMORY 常驻)"
        );

        // 复跑:表已销毁 → 缺表保守跳过,一项不多标。
        let n = mark_missing_preloaded(&c, 1, &[5], false).unwrap();
        assert_eq!(n, 0, "缺表必须保守返回 0");

        // 下一轮扫描:init 重建空表 → 已 missing 的 6..10 不重计,未 seen 且仍 online 的 1..5 重新纳入。
        init_seen_table(&c, 1).unwrap();
        let n = mark_missing_preloaded(&c, 1, &[5], true).unwrap();
        assert_eq!(n, 5, "重建后差集口径回到空 seen");
        // dry_run 不销毁表(不影响随后可能的真实收尾)。
        assert!(seen_table_exists(&c, 1));
    }

    /// 表缺失(非 fast_scan 路径误调 preloaded)→ 保守跳过,一项不标——绝不以空 seen 集误删。
    #[test]
    fn preloaded_missing_table_skips_safely() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0);
        let n = mark_missing_preloaded(&c, 1, &[5], false).unwrap();
        assert_eq!(n, 0, "seen 表缺失必须保守不标");
        assert_eq!(avail(&c, 100), "online");
    }

    /// 每根各表互不可见:root2 的 seen 表内容(甚至含 root1 的 id)不许影响 root1 的差集。
    /// 这是 P1 修复的核心契约——并发扫描不同根共用同一写连接,后启动扫描不得清掉他根 seen。
    #[test]
    fn per_root_seen_tables_are_isolated() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0); // root1 未 seen → 标
        add(&c, 103, 20, Some(5), "online", 0); // root2 子树,与 root1 差集无关
        init_seen_table(&c, 2).unwrap();
        let seen2 = SeenWriter::new(2);
        seen2.insert(&c, 100).unwrap(); // 故意塞 root1 的 id 进 root2 的表
        seen2.insert(&c, 103).unwrap();
        init_seen_table(&c, 1).unwrap(); // root1 启动:只清自己的表,root2 的 seen 必须原样保留

        assert!(
            seen_table_exists(&c, 2),
            "root1 的启动不得清掉 root2 的 seen 表"
        );
        let n = mark_missing_preloaded(&c, 1, &[5], false).unwrap();
        assert_eq!(n, 1, "root1 的差集只看 root1 的 seen 表(空)");
        assert_eq!(avail(&c, 100), "missing");
        assert_eq!(avail(&c, 103), "online", "root2 子树项不受 root1 扫描影响");
        assert!(!seen_table_exists(&c, 1), "root1 收尾只 DROP 自己的表");
        assert!(seen_table_exists(&c, 2), "root2 的表不受 root1 收尾影响");
    }

    /// 同一根目录的不同扫描代次必须各自持有 seen/online；旧轮清理不得碰新轮的表或数据。
    #[test]
    fn same_root_runs_are_isolated_by_generation() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0);
        add(&c, 101, 10, Some(5), "online", 0);
        let generation1 = 11;
        let generation2 = 12;

        init_seen_table_for_run(&c, 1, generation1).unwrap();
        SeenWriter::new_for_run(1, generation1)
            .insert(&c, 100)
            .unwrap();
        init_seen_table_for_run(&c, 1, generation2).unwrap();
        SeenWriter::new_for_run(1, generation2)
            .insert(&c, 101)
            .unwrap();

        for generation in [generation1, generation2] {
            let table = online_table_name_for_run(1, generation);
            c.execute_batch(&format!(
                "CREATE TEMP TABLE {table}(id INTEGER PRIMARY KEY); INSERT INTO {table}(id) VALUES (5);"
            ))
            .unwrap();
        }

        let marked1 = mark_missing_preloaded_for_run(&c, 1, generation1, &[5], false).unwrap();
        assert_eq!(marked1, 1, "generation1 只能看到自己的 seen=100");
        assert_eq!(avail(&c, 101), "missing");
        assert!(!temp_table_exists(
            &c,
            &seen_table_name_for_run(1, generation1)
        ));
        assert!(!temp_table_exists(
            &c,
            &online_table_name_for_run(1, generation1)
        ));
        assert!(temp_table_exists(
            &c,
            &seen_table_name_for_run(1, generation2)
        ));
        assert!(temp_table_exists(
            &c,
            &online_table_name_for_run(1, generation2)
        ));
        assert_eq!(
            temp_id_count(&c, &seen_table_name_for_run(1, generation2)),
            1,
            "旧轮收尾不得清空新轮 seen"
        );
        assert_eq!(
            temp_id_count(&c, &online_table_name_for_run(1, generation2)),
            1,
            "旧轮收尾不得清空新轮 online"
        );

        let marked2 = mark_missing_preloaded_for_run(&c, 1, generation2, &[5], false).unwrap();
        assert_eq!(marked2, 1, "generation2 仍按自己的 seen=101 做差集");
        assert!(!temp_table_exists(
            &c,
            &seen_table_name_for_run(1, generation2)
        ));
        assert!(!temp_table_exists(
            &c,
            &online_table_name_for_run(1, generation2)
        ));
    }

    /// 取消/错误兜底只 DROP 指定代次的 seen 与 online TEMP 表。
    #[test]
    fn cleanup_only_drops_requested_run_tables() {
        let c = mem_db();
        let generation1 = 21;
        let generation2 = 22;
        for generation in [generation1, generation2] {
            init_seen_table_for_run(&c, 1, generation).unwrap();
            let table = online_table_name_for_run(1, generation);
            c.execute_batch(&format!(
                "CREATE TEMP TABLE {table}(id INTEGER PRIMARY KEY); INSERT INTO {table}(id) VALUES (5);"
            ))
            .unwrap();
        }

        cleanup_scan_temp_tables_for_run(&c, 1, generation1).unwrap();
        assert!(!temp_table_exists(
            &c,
            &seen_table_name_for_run(1, generation1)
        ));
        assert!(!temp_table_exists(
            &c,
            &online_table_name_for_run(1, generation1)
        ));
        assert!(temp_table_exists(
            &c,
            &seen_table_name_for_run(1, generation2)
        ));
        assert!(temp_table_exists(
            &c,
            &online_table_name_for_run(1, generation2)
        ));
    }

    /// 大集合规模正确性：1000 项，奇数 id 全 seen、偶数 id 未 seen → 恰好 500 偶数项被标。
    /// 压 TEMP 表 join 在规模下的正确性（C1 「large-set temp-table path」加固）。
    #[test]
    fn large_set_marks_exactly_unseen() {
        let c = mem_db();
        let mut seen: HashSet<i64> = HashSet::new();
        for id in 1..=1000i64 {
            add(&c, id, 10, Some(5), "online", 0);
            if id % 2 == 1 {
                seen.insert(id); // 奇数已 seen
            }
        }
        let n = mark_missing(&c, 1, &[5], &seen, false).unwrap();
        assert_eq!(n, 500, "恰好 500 个偶数 id 未 seen → 被标");
        assert_eq!(avail(&c, 2), "missing", "偶数 id（未 seen）应标 missing");
        assert_eq!(avail(&c, 3), "online", "奇数 id（已 seen）应保持 online");
    }

    /// 已是 missing 的项不被重标/重计（`availability != 'missing'` 闸）——保证重复扫描幂等、计数不虚高。
    #[test]
    fn already_missing_not_remarked() {
        let c = mem_db();
        add(&c, 100, 10, Some(5), "online", 0); // online 未 seen → 标
        add(&c, 101, 10, Some(5), "missing", 0); // 已 missing 未 seen → 不再计
        let seen: HashSet<i64> = HashSet::new();

        let n = mark_missing(&c, 1, &[5], &seen, false).unwrap();
        assert_eq!(n, 1, "仅新缺失项计入，已 missing 不重复计");
        assert_eq!(avail(&c, 100), "missing");
        assert_eq!(avail(&c, 101), "missing");
    }

    /// companion 缺失时必须让主项的逻辑单元摘要失效；主项与 companion 同轮缺失时
    /// source_revision 只推进一次，避免一次扫描制造两个无意义代次。
    #[test]
    fn missing_companion_invalidates_parent_once() {
        let c = mem_db();
        add(&c, 200, 10, Some(5), "online", 0);
        add(&c, 201, 10, Some(5), "online", 0);
        c.execute("UPDATE media_items SET companion_of=200 WHERE id=201", [])
            .unwrap();
        c.execute(
            "INSERT INTO dedup_index
                (item_id, source_revision, hash_version, unit_digest, unit_size, status, checked_at)
             VALUES (200, 1, 1, X'01', 1, 'ready', 1),
                    (201, 1, 1, X'02', 1, 'ready', 1)",
            [],
        )
        .unwrap();

        let seen = HashSet::from([200]);
        assert_eq!(mark_missing(&c, 1, &[5], &seen, false).unwrap(), 1);
        assert_eq!(avail(&c, 201), "missing");
        assert_eq!(source_revision(&c, 200), 2);
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM dedup_index WHERE item_id=200",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "companion 缺失后主项逻辑摘要必须清除"
        );

        // 主项也在下一轮缺失时，不能因 companion 再额外推进一次。
        c.execute("UPDATE media_items SET availability='online'", [])
            .unwrap();
        let seen = HashSet::new();
        assert_eq!(mark_missing(&c, 1, &[5], &seen, false).unwrap(), 2);
        assert_eq!(source_revision(&c, 200), 3);
    }
}
