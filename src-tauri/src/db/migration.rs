//! 带版本控制的模式迁移。
//!
//! 策略：读取 `app_config.schema_version`，按顺序执行每个 `if version < N` 块。
//! 添加新迁移：递增 `CURRENT_VERSION` 并添加一个新块。
//! 重新运行是安全的：所有 DDL 都使用 `CREATE TABLE IF NOT EXISTS`。

use rusqlite::Connection;
use tracing::{info, warn};

use crate::db::schema::{
    SCHEMA_V1, SCHEMA_V10, SCHEMA_V11, SCHEMA_V12, SCHEMA_V13, SCHEMA_V14, SCHEMA_V15, SCHEMA_V16,
    SCHEMA_V17, SCHEMA_V18, SCHEMA_V19, SCHEMA_V2, SCHEMA_V20, SCHEMA_V21, SCHEMA_V22, SCHEMA_V23,
    SCHEMA_V24, SCHEMA_V25, SCHEMA_V26, SCHEMA_V27, SCHEMA_V28, SCHEMA_V29, SCHEMA_V3, SCHEMA_V30,
    SCHEMA_V31, SCHEMA_V32, SCHEMA_V33, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8,
    SCHEMA_V9,
};
use crate::error::Result;

/// 此二进制文件支持的最新模式版本。
const CURRENT_VERSION: u32 = 33;

/// 本二进制支持的最新 schema 版本(单一事实源)。数据备份/恢复(方案 B §6.1)据此判定
/// `restore_schema_too_new`——**勿在别处硬编码版本号**,升级迁移只改 `CURRENT_VERSION` 一处。
pub fn current_schema_version() -> u32 {
    CURRENT_VERSION
}

/// 读取某连接上 DB 的 schema 版本(表/键缺失=全新库返回 0)。恢复暂存库校验(方案 B §6.1)
/// 与迁移器共用同一读法,避免恢复侧另立一套版本读取逻辑而与迁移不一致。
pub fn read_schema_version(conn: &Connection) -> u32 {
    read_version(conn)
}

/// 从数据库读取当前的模式版本。
/// 如果表或键尚不存在，则返回 0（全新数据库）。
fn read_version(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT value FROM app_config WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<u32>().ok())
    .unwrap_or(0)
}

/// 写入当前的模式版本。
fn write_version(conn: &Connection, version: u32) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO app_config (key, value) VALUES ('schema_version', ?1)",
        rusqlite::params![version.to_string()],
    )?;
    Ok(())
}

/// 单个版本块：DDL + 版本号写入在**同一事务**内原子提交。
/// 失败整块回滚 → `schema_version` 不前进 → 下次启动安全重跑（杜绝半迁移 / `duplicate column` 死循环式启动失败）。
///
/// 用 `unchecked_transaction`（与 queries/scanner/enricher 全仓一致）而非把 `run_migrations`
/// 改 `&mut Connection`：迁移在启动期独占运行、无并发写，DEFERRED 事务足够，且免去波及 7 处
/// 调用方的破坏性签名变更。
fn migrate_step(conn: &Connection, version: u32, sql: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(sql)?; // 该版本全部 DDL
    write_version(&tx, version)?; // 版本号与 DDL 同事务（&Transaction 经 Deref 强转 &Connection）
    tx.commit()?; // 原子提交：失败整块回滚、版本号不前进
    Ok(())
}

/// 针对 **写入** 连接运行所有挂起的迁移。
/// 这必须在启动时在任何其他数据库操作之前调用。
pub fn run_migrations(conn: &Connection) -> Result<()> {
    // V19 回填 SQL 依赖 `TREE_SORT_KEY` 标量函数。此处自注册使 run_migrations **自足** —— 生产
    // 写连接虽已在 create_write_connection 注册，但众多测试在裸 `Connection::open_in_memory()`
    // 上直接调本函数（若不自注册，V19 的 `UPDATE ... TREE_SORT_KEY(rel_path)` 会 `no such
    // function` 全线红）。幂等：同名 create_scalar_function/create_collation 即替换，写连接二次
    // 注册无害；顺带让「调用方须先注册」的隐式顺序契约消失。
    crate::db::register_custom_collations(conn)?;

    let version = read_version(conn);
    info!(
        "DB schema version = {}, target = {} | 数据库结构版本 = {}, 目标版本 = {}",
        version, CURRENT_VERSION, version, CURRENT_VERSION
    );

    // 版本块顺序表：每块「DDL + 写版本号」由 `migrate_step` 在**单一事务**内原子提交。
    // 第三元素为日志描述（保留原有中英双语上下文）。
    const STEPS: &[(u32, &str, &str)] = &[
        (1, SCHEMA_V1, "v1 base | 基础结构"),
        (2, SCHEMA_V2, "v2 (AI embeddings) | v2（AI 嵌入向量）"),
        (3, SCHEMA_V3, "v3 (AI search results) | v3（AI 搜索结果）"),
        (
            4,
            SCHEMA_V4,
            "v4 (derivations + meta columns + reading progress) | v4（派生任务 + 元数据扩列 + 阅读进度）",
        ),
        (5, SCHEMA_V5, "v5 (collections / favorites) | v5（收藏夹）"),
        (
            6,
            SCHEMA_V6,
            "v6 (doc replacements + document versions) | v6（文档替换规则 + 版本管理）",
        ),
        (
            7,
            SCHEMA_V7,
            "v7 (storage backends / network drives) | v7（存储后端 / 网络盘）",
        ),
        (
            8,
            SCHEMA_V8,
            "v8 (face recognition: persons + faces + face_status) | v8（人脸识别：人物 + 人脸 + 检测状态）",
        ),
        (
            9,
            SCHEMA_V9,
            "v9 (exotic format plugin: catalog + plugins + tasks) | v9（冷门格式插件：能力目录 + 已装插件 + 任务表）",
        ),
        (
            10,
            SCHEMA_V10,
            "v10 (volume availability: volumes + volume_id/availability + color_label + content_identifier + face_rejections + backfill) | v10（卷可用性：卷表 + 卷id/可用性 + 颜色标签 + Live Photo 标识 + 人脸负样本 + 回填）",
        ),
        (
            11,
            SCHEMA_V11,
            "v11 (keyset pagination: composite idx_media_sort + idx_media_trash) | v11（keyset 分页：复合排序索引 + 回收站 seek 索引）",
        ),
        (
            12,
            SCHEMA_V12,
            "v12 (multi-channel: exotic_plugins.entitlement_source) | v12（多渠道预留：安装来源渠道列）",
        ),
        (
            13,
            SCHEMA_V13,
            "v13 (reader: text_book_index cache) | v13（阅读器：txt 章节索引缓存表）",
        ),
        (
            14,
            SCHEMA_V14,
            "v14 (reader: reader_book_prefs) | v14（阅读器：每书阅读偏好表，先承载手动编码覆盖）",
        ),
        (
            15,
            SCHEMA_V15,
            "v15 (reader: reader_bookmarks) | v15（阅读器：书签表，一书多书签 + 位置串 + 全书进度）",
        ),
        (
            16,
            SCHEMA_V16,
            "v16 (faces.is_unassigned: user-unassign marker for orphan reconcile) | v16（人脸「用户移出」判别位，孤儿对账用）",
        ),
        (
            17,
            SCHEMA_V17,
            "v17 (face_coverage: per item×face-model scan ledger + backfill) | v17（图×人脸模型覆盖记录+回填，零脸图不再被切轨/续传误重扫）",
        ),
        (
            18,
            SCHEMA_V18,
            "v18 (albums.deleted_at: soft-delete for undoable collection delete) | v18（收藏夹软删除时间戳，删除可撤销）",
        ),
        (
            19,
            SCHEMA_V19,
            "v19 (directories.tree_sort_key: persisted pre-order DFS sort key + backfill) | v19（目录持久化前序 DFS 排序键列 + 回填，folder 目录序改读列免逐行 FFI）",
        ),
        (
            20,
            SCHEMA_V20,
            "v20 (media_items.view_rotation: persisted viewer display rotation 0/90/180/270) | v20（看图台用户展示旋转持久列，0/90/180/270）",
        ),
        (
            21,
            SCHEMA_V21,
            "v21 (scan_roots.is_hidden: per-root visibility toggle, library-wide exclusion) | v21（根目录显隐位，库级排除）",
        ),
        (
            22,
            SCHEMA_V22,
            "v22 (media_derivations.orphan_count: poison-task guard for orphan reset) | v22（派生任务孤儿计数列，毒任务防线）",
        ),
        (
            23,
            SCHEMA_V23,
            "v23 (media_items.playback_position_ms: persisted playback resume position) | v23（媒体项播放位置记忆列，ms）",
        ),
        (
            24,
            SCHEMA_V24,
            "v24 (enrichment keyset indexes: media_type+sort/id, drop redundant idx_media_type) | v24（富化 keyset 索引：media_type+排序/id，删除冗余 media_type 索引）",
        ),
        (
            25,
            SCHEMA_V25,
            "v25 (drop redundant idx_media_directory: UNIQUE(directory_id,file_name) covers its prefix) | v25（删除冗余目录索引：唯一约束隐式索引已覆盖）",
        ),
        (
            26,
            SCHEMA_V26,
            "v26 (exact duplicate sidecar index, source revisions, nanosecond mtime) | v26（精确去重旁路索引、源修订号、纳秒修改时间）",
        ),
        (
            27,
            SCHEMA_V27,
            "v27 (reserved) | v27（保留空槽）",
        ),
        (
            28,
            SCHEMA_V28,
            "v28 (reserved) | v28（保留空槽）",
        ),
        (
            29,
            SCHEMA_V29,
            "v29 (reserved) | v29（保留空槽）",
        ),
        (
            30,
            SCHEMA_V30,
            "v30 (legacy volume classification fix) | v30（历史卷类型修正）",
        ),
        (
            31,
            SCHEMA_V31,
            "v31 (repair dedup quick index) | v31（修复去重 quick 候选索引）",
        ),
        (
            32,
            SCHEMA_V32,
            "v32 (dedup unpublished working index) | v32（去重未发布工作索引）",
        ),
        (
            33,
            SCHEMA_V33,
            "v33 (directory move journal) | v33（目录移动阶段日志）",
        ),
    ];
    for &(v, sql, desc) in STEPS {
        if version < v {
            info!(
                "Applying migration → {} | 正在应用数据库迁移 → {}",
                desc, desc
            );
            migrate_step(conn, v, sql)?; // DDL + 版本号同事务原子提交（杜绝半迁移）
            info!("Migration v{} complete | v{} 数据库迁移完成", v, v);
        }
    }

    let final_version = read_version(conn);
    if final_version == CURRENT_VERSION {
        info!(
            "DB schema is up-to-date (v{}) | 数据库结构已是最新 (v{})",
            CURRENT_VERSION, CURRENT_VERSION
        );
    } else {
        warn!("Post-migration version check: expected {CURRENT_VERSION}, got {final_version}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全新内存库跑全部迁移，验证版本号到 CURRENT_VERSION 且 exotic 三表 + 索引 + 配置就绪（端到端到最新版本）。
    #[test]
    fn migrates_fresh_db_to_current_version_with_exotic_tables() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");

        assert_eq!(read_version(&conn), CURRENT_VERSION);
        // v33: 目录移动阶段日志就位（物理搬运先于 DB 事务时的可恢复账）。
        let journal_table: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='directory_move_journal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(journal_table, 1, "directory_move_journal 表缺失");

        // v26: 扫描源修订与精确去重旁路索引就位；精确摘要不污染 media_items 的位置语义。
        for column in ["file_mtime_ns", "source_revision"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('media_items') WHERE name=?1",
                    [column],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "media_items.{column} 列缺失");
        }
        let dedup_table: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='dedup_index'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dedup_table, 1, "dedup_index 表缺失");
        let working_table: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='dedup_index_working'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(working_table, 1, "dedup_index_working 表缺失");
        for idx in [
            "idx_dedup_exact_candidate",
            "idx_dedup_unit_group",
            "idx_dedup_quick_candidate",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }
        // v24:enrichment keyset 索引就位,旧单列 idx_media_type 已被复合索引取代(2026-08-21 性能线)。
        for idx in ["idx_media_type_id", "idx_media_type_sort"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }
        let old_type_idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_media_type'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_type_idx, 0, "idx_media_type 应被 V24 删除");

        // v25:idx_media_directory 冗余前缀索引删除,唯一约束隐式索引已覆盖同一查询面。
        let old_dir_idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_media_directory'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_dir_idx, 0, "idx_media_directory 应被 V25 删除");
        let unique_dir_idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_index_list('media_items') WHERE name='sqlite_autoindex_media_items_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            unique_dir_idx, 1,
            "UNIQUE(directory_id,file_name) 隐式索引缺失"
        );

        // v23:media_items.playback_position_ms 播放位置记忆列就位(2026-07-22 播放器线)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('media_items') WHERE name='playback_position_ms'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "media_items.playback_position_ms 列缺失");

        // v22:media_derivations.orphan_count 孤儿计数列就位(2026-07-22 毒任务防线)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('media_derivations') WHERE name='orphan_count'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "media_derivations.orphan_count 列缺失");

        // v21:scan_roots.is_hidden 根显隐位就位(2026-07-18 设置页需求)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('scan_roots') WHERE name='is_hidden'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "scan_roots.is_hidden 列缺失");

        // v20:media_items.view_rotation 看图台用户旋转持久列就位(2026-07-18 内容页需求)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('media_items') WHERE name='view_rotation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "media_items.view_rotation 列缺失");

        // v19:directories.tree_sort_key 持久前序 DFS 排序键列就位(目录排序统一 方案 B)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('directories') WHERE name='tree_sort_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "directories.tree_sort_key 列缺失");

        // v18:albums.deleted_at 软删除时间戳列就位(收藏夹可撤销删除，S5 阶段 11)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('albums') WHERE name='deleted_at'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "albums.deleted_at 列缺失");

        // v16:faces.is_unassigned 判别位就位(2026-07-10 审查 F3)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('faces') WHERE name='is_unassigned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "faces.is_unassigned 列缺失");

        // v17:face_coverage 记账表 + 模型索引就位(2026-07-11 加固批 B-3)。
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='face_coverage'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "face_coverage 表缺失");

        // 三表存在。
        for table in ["exotic_catalog_formats", "exotic_plugins", "exotic_tasks"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺表 {table}");
        }

        // 领取/门控索引存在。
        for idx in ["idx_exotic_tasks_ready", "idx_exotic_tasks_item"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }

        // 配置默认值就绪（含 max_workers），且**不存在** exotic_dev_mode。
        for (k, v) in [
            ("exotic_enabled", "true"),
            ("exotic_auto_process", "true"),
            ("exotic_paused", "false"),
            ("exotic_max_workers", "0"),
        ] {
            let got: String = conn
                .query_row("SELECT value FROM app_config WHERE key=?1", [k], |r| {
                    r.get(0)
                })
                .unwrap_or_else(|_| panic!("缺配置 {k}"));
            assert_eq!(got, v, "配置 {k} 值不符");
        }
        let dev: i64 = conn
            .query_row(
                "SELECT count(*) FROM app_config WHERE key='exotic_dev_mode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dev, 0, "Release 不得有 exotic_dev_mode 授权旁路");

        // 唯一约束：同 (item, plugin, capability) 不可重复。
        // 迁移启用了 FK；本子测试只验 UNIQUE，临时关 FK 免去构造完整 media_items 行。
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        conn.execute(
            "INSERT INTO exotic_tasks (item_id, plugin_id, capability) VALUES (1, 'exotic-image-psd', 'thumbnail')",
            [],
        )
        .expect("first task insert");
        let dup = conn.execute(
            "INSERT INTO exotic_tasks (item_id, plugin_id, capability) VALUES (1, 'exotic-image-psd', 'thumbnail')",
            [],
        );
        assert!(
            dup.is_err(),
            "UNIQUE(item_id,plugin_id,capability) 应拒绝重复"
        );
    }

    /// 事务化迁移的幂等性：已迁移库再跑一次必须是 no-op（不得 `duplicate column` / 不得改版本）。
    /// 锁住「半迁移死循环式启动失败」的根治效果——这是开机 panic 的来源之一。
    #[test]
    fn migrations_are_idempotent_on_rerun() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("first run");
        // 版本已是 CURRENT_VERSION，第二次全部跳过；若有未事务化/非幂等块，这里会 duplicate column 报错。
        run_migrations(&conn).expect("second run must be a safe no-op");
        assert_eq!(read_version(&conn), CURRENT_VERSION);
    }

    /// V30 修正 V10 时代的路径派生卷：`pending:`/`path:` 只说明路径来源，
    /// 不能证明是固定本地卷；升级后等待重新添加/重链接触发原生卷探测。
    #[test]
    fn v30_marks_legacy_path_derived_volumes_unknown() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("initial run");
        conn.execute_batch(
            "INSERT INTO volumes (id, stable_id, kind) VALUES
                 (101, 'pending:1', 'local'),
                 (102, 'path:C:', 'local'),
                 (103, 'win:{fixed-volume}', 'local');",
        )
        .expect("construct v29 legacy volume state");
        write_version(&conn, 29).expect("rewind to v29");

        run_migrations(&conn).expect("v29 to v30");

        let kind = |stable_id: &str| -> String {
            conn.query_row(
                "SELECT kind FROM volumes WHERE stable_id=?1",
                [stable_id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(kind("pending:1"), "unknown");
        assert_eq!(kind("path:C:"), "unknown");
        assert_eq!(kind("win:{fixed-volume}"), "local");
    }

    /// V31：旧 V30 库即使缺少 quick 候选索引，也能幂等补齐访问路径。
    #[test]
    fn v31_repairs_quick_candidate_index_for_existing_db() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("initial run");
        conn.execute_batch(
            "DROP INDEX idx_dedup_quick_candidate;
             UPDATE app_config SET value='30' WHERE key='schema_version';",
        )
        .expect("construct old v30 schema");

        run_migrations(&conn).expect("v30 to v31");
        assert_eq!(read_version(&conn), CURRENT_VERSION);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_dedup_quick_candidate'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "v31 应补建 quick 候选索引");
    }

    /// v17 回填双通道(2026-07-11 加固批 B-3):①有脸项按 faces 行逐模型回填;
    /// ②零脸 Done 项归当前激活模型。手法=迁到顶后删表回拨版本,构造「已有人脸
    /// 数据的 v16 老库」再重跑迁移,只让 v17 步重放。
    #[test]
    fn v17_backfills_coverage_from_faces_and_zero_face_done() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("initial run");
        conn.execute_batch("DROP TABLE face_coverage;").unwrap();
        // 回拨到 16 会重放所有 version<16 之后的步；v18 的 `ALTER TABLE albums ADD COLUMN
        // deleted_at` 与 v19 的 `ALTER TABLE directories ADD COLUMN tree_sort_key` 均非幂等，
        // 列已存在会 duplicate column 报错。故回拨前撤销这两列，使「回拨到 N」真正等价于「库
        // 处于版本 N 状态」（凡回拨重放式测试都须撤销所跨越的后续非幂等 DDL）。
        conn.execute_batch("ALTER TABLE albums DROP COLUMN deleted_at;")
            .unwrap();
        conn.execute_batch("ALTER TABLE directories DROP COLUMN tree_sort_key;")
            .unwrap();
        // v20 的 `ALTER TABLE media_items ADD COLUMN view_rotation` 同为非幂等：回拨到 16 会重放它，
        // 列已存在会 duplicate column，故一并撤销（凡回拨重放式测试都须撤销所跨越的后续非幂等 DDL）。
        conn.execute_batch("ALTER TABLE media_items DROP COLUMN view_rotation;")
            .unwrap();
        // v21 的 `ALTER TABLE scan_roots ADD COLUMN is_hidden` 同为非幂等：回拨到 16 会重放它，一并撤销。
        conn.execute_batch("ALTER TABLE scan_roots DROP COLUMN is_hidden;")
            .unwrap();
        // v22 的 `ALTER TABLE media_derivations ADD COLUMN orphan_count` 同为非幂等：回拨到 16 会重放它，一并撤销。
        conn.execute_batch("ALTER TABLE media_derivations DROP COLUMN orphan_count;")
            .unwrap();
        // v23 的 `ALTER TABLE media_items ADD COLUMN playback_position_ms` 同为非幂等：回拨到 16 会重放它，一并撤销。
        conn.execute_batch("ALTER TABLE media_items DROP COLUMN playback_position_ms;")
            .unwrap();
        // v26/v27 的列、旁路表在回拨到 v16 时也必须撤销；否则重放 v26 会遇到
        // duplicate column，无法验证真实的旧库升级路径。
        conn.execute_batch(
            "DROP TABLE dedup_index;
             ALTER TABLE media_items DROP COLUMN file_mtime_ns;
             ALTER TABLE media_items DROP COLUMN source_revision;",
        )
        .unwrap();
        write_version(&conn, 16).unwrap();

        // v16 现场:item 1=模型 mA 检出一脸;2=零脸 Done(无 faces 行);3=Pending;
        // 4=Done 但激活模型缺省(app_config 无 face_model_active 时回退 'yunet-sface',
        // 与 4 同库不可能——本测试显式设 mB 为激活模型,4 与 2 同归 mB)。
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        conn.execute_batch(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES ('face_model_active', 'mB');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, face_status) VALUES
                 (1, 10, 'a.jpg', 1, 1, 'jpg', 'image', 10, 10, 100, 11, 2),
                 (2, 10, 'b.jpg', 1, 2, 'jpg', 'image', 10, 10, 200, 22, 2),
                 (3, 10, 'c.jpg', 1, 3, 'jpg', 'image', 10, 10, 300, 33, 0),
                 (4, 10, 'd.jpg', 1, 4, 'jpg', 'image', 10, 10, 400, 44, 2);
             INSERT INTO faces (item_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h, det_score, quality, embedding) VALUES
                 (1, 'mA', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, x'00');",
        )
        .unwrap();

        run_migrations(&conn).expect("v17 replay");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        let cov = |id: i64, model: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM face_coverage WHERE item_id=?1 AND model_name=?2",
                rusqlite::params![id, model],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(cov(1, "mA"), 1, "回填①:有脸项按 faces 行归其模型");
        assert_eq!(cov(2, "mB"), 1, "回填②:零脸 Done 归激活模型");
        assert_eq!(cov(4, "mB"), 1, "回填②:零脸 Done 归激活模型(第二例)");
        assert_eq!(cov(3, "mB") + cov(3, "mA"), 0, "Pending 项不回填");
        // 注:item 1 Done+有 mA 行,回填②也会给它补一行 mB(status=2 全局列无从判归属);
        // 语义无害——切到 mB 时它显示已扫,与迁移前「有脸项不重扫」的体验一致。
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM face_coverage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 4, "1×mA + {{1,2,4}}×mB = 4 行");
    }

    /// v19 回填不变量(目录排序统一 方案 B):`ADD COLUMN tree_sort_key` + 回填 UPDATE 对**既有
    /// 目录行**正确落键。手法同 v17：迁到顶 → DROP 列回拨到 18（撤销 v19 的非幂等 ADD COLUMN，
    /// 否则重放 duplicate column）→ 构造「有目录数据的 v18 老库」→ 重放 v19。
    ///
    /// 断言：①非根目录(rel_path<>'')回填后键非空(X''=回填漏项=bug)；②根目录(rel_path='')保持
    /// X'' 合法空键；③回填值逐位 == `encode_tree_sort_key`；④BLOB 字节序复现**前序 DFS** 而非
    /// rel_path 字符串序（严格反例 `A < A/Z < A-`，字符串序会得 `A < A- < A/Z`）。
    #[test]
    fn v19_backfills_tree_sort_key_for_existing_directories() {
        use crate::utils::path::encode_tree_sort_key;

        let conn = Connection::open_in_memory().expect("open in-memory");
        // 先迁到 v19（注册 TREE_SORT_KEY + 加 tree_sort_key 列）。
        run_migrations(&conn).expect("initial run");
        // 回拨到 18：撤销 v19（tree_sort_key）与 v20（media_items.view_rotation）两个非幂等 ADD COLUMN，
        // 使重放真正等价于「v18 老库升 v19/v20」（回拨跨越的每个后续非幂等 DDL 都须先撤销，否则重放 duplicate column）。
        conn.execute_batch("ALTER TABLE directories DROP COLUMN tree_sort_key;")
            .unwrap();
        conn.execute_batch("ALTER TABLE media_items DROP COLUMN view_rotation;")
            .unwrap();
        // v21 的 `ALTER TABLE scan_roots ADD COLUMN is_hidden` 同为非幂等：回拨到 18 会重放它,一并撤销。
        conn.execute_batch("ALTER TABLE scan_roots DROP COLUMN is_hidden;")
            .unwrap();
        // v22 的 `ALTER TABLE media_derivations ADD COLUMN orphan_count` 同为非幂等：回拨到 18 会重放它,一并撤销。
        conn.execute_batch("ALTER TABLE media_derivations DROP COLUMN orphan_count;")
            .unwrap();
        // v23 的 `ALTER TABLE media_items ADD COLUMN playback_position_ms` 同为非幂等：回拨到 18 会重放它,一并撤销。
        conn.execute_batch("ALTER TABLE media_items DROP COLUMN playback_position_ms;")
            .unwrap();
        // v26 的列和旁路表在回拨到 v18 时同样撤销，候选索引随旁路表消失。
        conn.execute_batch(
            "DROP TABLE dedup_index;
             ALTER TABLE media_items DROP COLUMN file_mtime_ns;
             ALTER TABLE media_items DROP COLUMN source_revision;",
        )
        .unwrap();
        write_version(&conn, 18).unwrap();

        // v18 老库现场：根 + 多级 + 标点边界（覆盖 DFS 反例 A / A/Z / A-）。
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, '',    'r'),
                 (11, 1, 'A',   'A'),
                 (12, 1, 'A/Z', 'Z'),
                 (13, 1, 'A-',  'A-');",
        )
        .unwrap();

        run_migrations(&conn).expect("v19 replay"); // 重放 v19：ADD COLUMN + 回填 UPDATE。
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        // ① 非根目录键非空（回填无漏项）。
        let orphan: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM directories WHERE rel_path <> '' AND tree_sort_key = X''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphan, 0, "非根目录 tree_sort_key 回填漏项");

        let key = |id: i64| -> Vec<u8> {
            conn.query_row(
                "SELECT tree_sort_key FROM directories WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        // ② 根目录保持 X'' 合法空键。
        assert!(key(10).is_empty(), "根目录(rel_path='')键应为空 X''");
        // ③ 回填值逐位 == encode_tree_sort_key。
        assert_eq!(key(11), encode_tree_sort_key("A"));
        assert_eq!(key(12), encode_tree_sort_key("A/Z"));
        assert_eq!(key(13), encode_tree_sort_key("A-"));
        // ④ BLOB 字节序 = 前序 DFS：A < A/Z < A-（rel_path 字符串序会得 A < A- < A/Z）。
        assert!(
            key(11) < key(12) && key(12) < key(13),
            "tree_sort_key BLOB 序须复现前序 DFS 而非 rel_path 字符串序"
        );
    }

    /// 取某表的列名集合（pragma_table_info；表名为测试内字面量，format! 安全）。
    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let sql = format!("SELECT name FROM pragma_table_info('{table}')");
        let mut stmt = conn.prepare(&sql).unwrap();
        let cols = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        cols
    }

    /// v12(T13 多渠道):exotic_plugins.entitlement_source 列存在,旧式 INSERT 取默认 'direct'。
    #[test]
    fn v12_entitlement_source_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        assert!(
            table_columns(&conn, "exotic_plugins").contains(&"entitlement_source".to_string()),
            "缺列 entitlement_source"
        );
        // DEFAULT 'direct':不带该列的旧式 INSERT(v12 前的代码路径)取默认值。
        conn.execute(
            "INSERT INTO exotic_plugins
                (plugin_id, version, manifest_hash, package_sequence, install_state, installed_at, updated_at)
             VALUES ('p', '1', 'h', 1, 'installed', 0, 0)",
            [],
        )
        .unwrap();
        let src: String = conn
            .query_row(
                "SELECT entitlement_source FROM exotic_plugins WHERE plugin_id='p'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(src, "direct");
    }

    /// v13(阅读器 R1):text_book_index 缓存表建出,列齐备,item_id 为主键(每书一行)。
    #[test]
    fn v13_text_book_index_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='text_book_index'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "缺表 text_book_index");

        let cols = table_columns(&conn, "text_book_index");
        for col in [
            "item_id",
            "src_key",
            "encoding",
            "confidence",
            "chapters",
            "updated_at",
        ] {
            assert!(
                cols.contains(&col.to_string()),
                "text_book_index 缺列 {col}"
            );
        }
    }

    /// v14(阅读器 R1/R3):reader_book_prefs 表建出,列齐备(item_id 主键 + prefs JSON)。
    #[test]
    fn v14_reader_book_prefs_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='reader_book_prefs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "缺表 reader_book_prefs");

        let cols = table_columns(&conn, "reader_book_prefs");
        for col in ["item_id", "prefs", "updated_at"] {
            assert!(
                cols.contains(&col.to_string()),
                "reader_book_prefs 缺列 {col}"
            );
        }
    }

    /// v15(阅读器 R4):reader_bookmarks 表建出,列齐备(id 主键 + item_id + locator + label + fraction)。
    #[test]
    fn v15_reader_bookmarks_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='reader_bookmarks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "缺表 reader_bookmarks");

        let cols = table_columns(&conn, "reader_bookmarks");
        for col in [
            "id",
            "item_id",
            "locator",
            "label",
            "fraction",
            "created_at",
        ] {
            assert!(
                cols.contains(&col.to_string()),
                "reader_bookmarks 缺列 {col}"
            );
        }
    }

    /// 全新库到 V10：卷可用性模型的表 / 列 / 索引齐备（结构层端到端）。
    #[test]
    fn v10_volume_schema_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        // 新表存在。
        for table in ["volumes", "face_rejections"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺表 {table}");
        }

        // media_items 新列齐备。
        let mi = table_columns(&conn, "media_items");
        for col in [
            "volume_id",
            "volume_relative_path",
            "availability",
            "color_label",
            "content_identifier",
        ] {
            assert!(mi.contains(&col.to_string()), "media_items 缺列 {col}");
        }
        // scan_roots / persons 新列。
        let sr = table_columns(&conn, "scan_roots");
        assert!(
            sr.contains(&"volume_id".to_string()) && sr.contains(&"volume_subpath".to_string()),
            "scan_roots 缺卷列"
        );
        let p = table_columns(&conn, "persons");
        assert!(
            p.contains(&"model_name".to_string()),
            "persons 缺 model_name"
        );

        // 部分索引存在。
        for idx in [
            "idx_media_avail",
            "idx_media_volume",
            "idx_media_content_id",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }
    }

    /// V9→V10 升级 + 卷回填：模拟既有 V9 用户（有 scan_root/目录/媒体项但无卷列）。升级后必须：
    /// 建出 volumes 占位行、scan_roots 与 media_items 的 volume_id 经 directory→scan_root→volume
    /// 链正确回填、availability 默认 online。锁住「既有用户零数据丢失 + 正确联卷」（迁移最高危处）。
    #[test]
    fn v10_backfill_links_existing_data_to_volumes() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        // 1) 逐版本 DDL 搭一个 V9 库（停在 9，不含卷列）。
        for sql in [
            SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8,
            SCHEMA_V9,
        ] {
            conn.execute_batch(sql).unwrap();
        }
        write_version(&conn, 9).unwrap();

        // 2) 既有数据：scan_root → directory → media_item（media_item 列对齐 fast_scan INSERT）。
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/test/root', 'Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (1, 1, 'sub', 'sub')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (1, 1, 'a.jpg', 100, 0, 'jpg', 'image', 0, 0, 0, 0)",
            [],
        )
        .unwrap();

        // 3) 升级到 V10（version=9 → 只跑 V10 步：ALTER + 回填）。
        run_migrations(&conn).expect("V9→V10 migration");
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        // 4) volumes 占位行建出（stable_id='pending:1'，last_mount_path=根路径）。
        let vol_id: i64 = conn
            .query_row(
                "SELECT id FROM volumes WHERE stable_id='pending:1' AND last_mount_path='/test/root'",
                [],
                |r| r.get(0),
            )
            .expect("缺卷占位行");
        // 5) scan_roots.volume_id 回填到该卷。
        let sr_vol: i64 = conn
            .query_row("SELECT volume_id FROM scan_roots WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(sr_vol, vol_id, "scan_root 未联卷");
        // 6) media_items.volume_id 经 directory→scan_root→volume 链回填。
        let mi_vol: i64 = conn
            .query_row("SELECT volume_id FROM media_items WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(mi_vol, vol_id, "media_item 未经链回填联卷");
        // 7) availability 默认 online（零成本迁移：既有行自动在线）。
        let avail: String = conn
            .query_row("SELECT availability FROM media_items WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(avail, "online", "既有行 availability 应默认 online");
    }

    /// V11：idx_media_sort 升为复合键（2 列），idx_media_trash 新建。锁住 keyset 分页的索引地基。
    #[test]
    fn v11_keyset_indexes_present() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&conn).expect("run_migrations");
        // 断言迁移跑到底(对齐 v10 测试写法;具体版本随 CURRENT_VERSION 前进,勿硬编码)。
        assert_eq!(read_version(&conn), CURRENT_VERSION);

        // idx_media_sort 现为复合键：pragma_index_info 应有 2 列（sort_datetime + id）。
        let sort_cols: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_index_info('idx_media_sort')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sort_cols, 2, "idx_media_sort 应为 2 列复合键");

        // idx_media_trash 存在。
        let trash: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_media_trash'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(trash, 1, "缺 idx_media_trash");
    }
}
