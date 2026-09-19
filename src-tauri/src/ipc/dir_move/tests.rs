//! 目录移动的表征回归（审查 §7.1-A/B + P0-1 第 8 节完成标准 + 主审复核 1–9）。
//!
//! 覆盖：同卷/跨根（跨卷）移动的身份与卷重绑、跨根同相对路径（cache_key 不变）、旧卷下线不误伤、
//! DB 注入失败后的可恢复状态与重试幂等、目标冲突不覆盖、暂存目录只认自己的标记、落扫描根下不写
//! NULL 父指针、取闸后的计划复核，以及**恢复边界**：无凭据不认领外部目标、目标缺失不假收尾、
//! 删源必须逐文件证明内容、启动档不做大拷贝、卷离线（NotFound 伪装）不作废恢复线索，外加
//! 「复制完成→凭据已落→崩溃在发布前/发布后」两个崩溃窗的续跑。
//!
//! 全部在临时目录 + 内存库里跑，绝不触碰用户真实媒体。

use super::*;
use rusqlite::params;
use std::sync::Mutex;
use tempfile::TempDir;

// ── 夹具 ────────────────────────────────────────────────────────────────────

struct Fixture {
    db: Mutex<Connection>,
    tmp: TempDir,
    root_a: PathBuf,
    root_b: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root_a = tmp.path().join("volA");
        let root_b = tmp.path().join("volB");
        std::fs::create_dir_all(&root_a).unwrap();
        std::fs::create_dir_all(&root_b).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        // 与既有 DB 测试同姿态：关外键，直接手工造目录/媒体行。
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // 两个已登记卷：A 为「源卷」（可离线），B 为在线目标卷；volume_subpath 非空，
        // 卷内相对路径断言才有意义。
        conn.execute_batch(
            "INSERT INTO volumes (id, stable_id, label, kind, last_mount_path, is_online) VALUES
                 (1, 'vol-guid-A', '源卷', 'removable', '/mnt/A', 1),
                 (2, 'vol-guid-B', '目标卷', 'local', '/mnt/B', 1);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias, volume_id, volume_subpath) VALUES
                 (1, ?1, 'A', 1, 'volA'), (2, ?2, 'B', 2, 'volB')",
            params![
                root_a.to_string_lossy().to_string(),
                root_b.to_string_lossy().to_string()
            ],
        )
        .unwrap();
        Self {
            db: Mutex::new(conn),
            tmp,
            root_a,
            root_b,
        }
    }

    fn db(&self) -> MoveDb<'_> {
        MoveDb::Writer(&self.db)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn read(&self, root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    fn exists(&self, root: &Path, rel: &str) -> bool {
        root.join(rel).exists()
    }

    fn dir(&self, id: i64, root_id: i64, parent: Option<i64>, rel: &str, name: &str, depth: i64) {
        self.conn()
            .execute(
                "INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, root_id, parent, rel, name, depth],
            )
            .unwrap();
    }

    /// 插一条媒体行，返回它的 cache_key（按移动前的 rel 计算）。
    #[allow(clippy::too_many_arguments)]
    fn media(
        &self,
        id: i64,
        dir_id: i64,
        file_name: &str,
        rel: &str,
        volume_id: i64,
        thumb_status: i64,
        thumb_path: Option<&str>,
    ) -> i64 {
        let key = compute_cache_key_with_mtime_ns(rel, file_name, 1_700_000_000, 0);
        self.conn()
            .execute(
                "INSERT INTO media_items
                    (id, directory_id, file_name, file_size, file_mtime, file_mtime_ns, file_format,
                     media_type, width, height, sort_datetime, cache_key, thumb_status, thumb_path,
                     volume_id, volume_relative_path, availability, is_favorited, rating)
                 VALUES (?1,?2,?3,10,1700000000,0,'jpg','image',10,10,1700000000,?4,?5,?6,?7,?8,'online',1,4)",
                params![
                    id,
                    dir_id,
                    file_name,
                    key,
                    thumb_status,
                    thumb_path,
                    volume_id,
                    Option::<String>::None
                ],
            )
            .unwrap();
        key
    }

    fn cache_dir(&self) -> PathBuf {
        self.tmp.path().join("cache")
    }

    fn plan(&self, source: i64, target: i64) -> MovePlan {
        load_plan(&self.conn(), source, target).unwrap()
    }

    /// 手工登记一条阶段日志（模拟「上一次会话留下的半完成移动」）。
    #[allow(clippy::too_many_arguments)]
    fn journal(
        &self,
        stage: &str,
        source_dir_id: i64,
        staging: Option<&Path>,
        payload: Option<(&str, i64)>,
    ) -> i64 {
        let id = q::insert_intent(
            &self.conn(),
            source_dir_id,
            1,
            "Photos/Move",
            &self.root_a.join("Photos/Move").to_string_lossy(),
            2,
            "Dest/Move",
            &self.root_b.join("Dest/Move").to_string_lossy(),
            20,
            staging.map(|p| p.to_string_lossy().to_string()).as_deref(),
        )
        .unwrap();
        if let Some((digest, files)) = payload {
            q::set_payload(&self.conn(), id, Some(digest), Some(files)).unwrap();
        }
        if stage != q::STAGE_INTENT {
            q::set_stage(&self.conn(), id, stage).unwrap();
        }
        id
    }
}

/// 一条媒体行的关键身份列（移动前后对比用）。
#[derive(Debug, PartialEq)]
struct MediaRow {
    directory_id: i64,
    cache_key: i64,
    thumb_path: Option<String>,
    volume_id: Option<i64>,
    volume_relative_path: Option<String>,
    availability: String,
    is_favorited: i64,
    rating: i64,
}

fn media_row(conn: &Connection, id: i64) -> MediaRow {
    conn.query_row(
        "SELECT directory_id, cache_key, thumb_path, volume_id, volume_relative_path,
                availability, is_favorited, rating FROM media_items WHERE id=?1",
        params![id],
        |r| {
            Ok(MediaRow {
                directory_id: r.get(0)?,
                cache_key: r.get(1)?,
                thumb_path: r.get(2)?,
                volume_id: r.get(3)?,
                volume_relative_path: r.get(4)?,
                availability: r.get(5)?,
                is_favorited: r.get(6)?,
                rating: r.get(7)?,
            })
        },
    )
    .unwrap()
}

/// (root_id, parent_id, rel_path, depth)
fn dir_row(conn: &Connection, id: i64) -> (i64, Option<i64>, String, i64) {
    conn.query_row(
        "SELECT root_id, parent_id, rel_path, depth FROM directories WHERE id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

fn run_move(f: &Fixture, plan: &MovePlan) -> Result<MoveDirOutcome> {
    execute_move(f.db(), plan, &f.cache_dir())
}

/// 标准小型夹具：Photos/Move 子树（1 文件）+ 目标 Dest。
fn basic_fixture() -> Fixture {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(20, 2, None, "Dest", "Dest", 1);
    f.media(100, 10, "a.jpg", "Photos/Move", 1, 1, None);
    f
}

// ── 1. 同卷移动：身份与元数据 ────────────────────────────────────────────────

/// 同卷（同根）移动：物理就位 + 目录/媒体路径重写；收藏与评分（用户资产）不动。
#[test]
fn same_volume_move_rewrites_paths_and_keeps_user_assets() {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.write(&f.root_a, "Photos/Move/sub/b.jpg", "bbbb");
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(11, 1, Some(10), "Photos/Move/sub", "sub", 3);
    f.dir(20, 1, None, "Albums", "Albums", 1);
    let key_a = f.media(
        100,
        10,
        "a.jpg",
        "Photos/Move",
        1,
        1,
        Some("256/ab/abcdef.webp"),
    );
    f.media(
        101,
        11,
        "b.jpg",
        "Photos/Move/sub",
        1,
        2,
        Some("404/cd/cdefab.webp"),
    );

    let plan = f.plan(10, 20);
    let outcome = run_move(&f, &plan).unwrap();

    assert_eq!((outcome.affected_dirs, outcome.affected_media), (2, 2));
    assert!(outcome.source_leftover.is_none(), "同卷 rename 无残留");
    assert!(outcome.recovery_id.is_none());
    assert!(!f.exists(&f.root_a, "Photos/Move"));
    assert_eq!(f.read(&f.root_a, "Albums/Move/a.jpg"), "aaa");
    assert_eq!(f.read(&f.root_a, "Albums/Move/sub/b.jpg"), "bbbb");
    let conn = f.conn();
    // depth 约定与扫描器一致（path_depth = 段数）："Photos/Move" → 2，"Albums/Move" → 2。
    assert_eq!(
        dir_row(&conn, 10),
        (1, Some(20), "Albums/Move".to_string(), 2)
    );
    assert_eq!(dir_row(&conn, 11).2, "Albums/Move/sub");
    let after = media_row(&conn, 100);
    assert_ne!(after.cache_key, key_a, "rel 变了 cache_key 必须重算");
    assert_eq!(after.volume_id, Some(1), "同卷移动卷身份不变");
    assert_eq!(
        after.volume_relative_path.as_deref(),
        Some("volA/Albums/Move/a.jpg"),
        "卷内相对路径须随移动重写"
    );
    assert!(
        after.thumb_path.as_deref().unwrap().starts_with("256/"),
        "尺寸档位段保留"
    );
    assert_ne!(
        after.thumb_path.as_deref(),
        Some("256/ab/abcdef.webp"),
        "缓存路径按新 cache_key 重映射"
    );
    assert_eq!((after.is_favorited, after.rating), (1, 4));
    assert!(
        q::list_pending(&conn).unwrap().is_empty(),
        "阶段日志收尾干净"
    );
}

/// 落到扫描根下：父指针必须是该根 rel_path='' 的目录行，绝不能写 NULL（否则普通目录会变成
/// 树里的伪根节点）。
#[test]
fn move_into_scan_root_keeps_root_directory_row_as_parent() {
    let f = basic_fixture();
    // 5 = 根目录行（rel_path=''）；10 = Photos/Move，父是 Photos。
    f.dir(5, 1, None, "", "volA", 0);

    let plan = f.plan(10, 5); // 目标 = 扫描根目录行（根级）
    assert_eq!(plan.new_rel, "Move");
    let outcome = run_move(&f, &plan).unwrap();
    assert_eq!(outcome.affected_dirs, 1);

    let conn = f.conn();
    let (root_id, parent_id, rel, depth) = dir_row(&conn, 10);
    assert_eq!(root_id, 1);
    assert_eq!(parent_id, Some(5), "父指针必须是根目录行，不是 NULL");
    assert_eq!((rel.as_str(), depth), ("Move", 1));
    assert!(f.exists(&f.root_a, "Move/a.jpg"));
}

// ── 2. 跨根（跨卷）移动：卷身份重绑 + 旧卷不下线误伤 ──────────────────────────

/// 跨根移动后条目绑到目标卷；源卷随后下线不得把已在新卷上的条目标成 offline。
#[test]
fn cross_root_move_rebinds_volume_and_survives_source_volume_offline() {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.write(&f.root_a, "Photos/Move/sub/b.jpg", "bbbb");
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(11, 1, Some(10), "Photos/Move/sub", "sub", 3);
    f.dir(20, 2, None, "Dest", "Dest", 1);
    // thumb_status=3（直接使用源文件）的绝对路径缩略图：跨根必须一起更新。
    let src_thumb = f
        .root_a
        .join("Photos/Move/a.jpg")
        .to_string_lossy()
        .to_string();
    let key_a = f.media(100, 10, "a.jpg", "Photos/Move", 1, 3, Some(&src_thumb));
    f.media(101, 11, "b.jpg", "Photos/Move/sub", 1, 1, None);

    let plan = f.plan(10, 20);
    let outcome = run_move(&f, &plan).unwrap();
    assert_eq!((outcome.affected_dirs, outcome.affected_media), (2, 2));

    assert!(!f.exists(&f.root_a, "Photos/Move"));
    assert_eq!(f.read(&f.root_b, "Dest/Move/sub/b.jpg"), "bbbb");
    let conn = f.conn();
    assert_eq!(
        dir_row(&conn, 10),
        (2, Some(20), "Dest/Move".to_string(), 2)
    );
    let after = media_row(&conn, 100);
    assert_ne!(after.cache_key, key_a, "跨根 rel 变了 → cache_key 重算");
    assert_eq!(after.volume_id, Some(2), "跨根移动必须重绑目标卷");
    assert_eq!(
        after.volume_relative_path.as_deref(),
        Some("volB/Dest/Move/a.jpg"),
        "卷内相对路径按目标卷 subpath 重算"
    );
    assert_eq!(after.availability, "online");
    assert_eq!((after.is_favorited, after.rating), (1, 4));
    // source-direct 缩略图指向新位置。
    assert_eq!(
        after.thumb_path.as_deref(),
        Some(resolve_media_path(&f.root_b.to_string_lossy(), "Dest/Move", "a.jpg").as_str())
    );

    // 源卷（1）整盘下线：不得碰已经在新卷上的条目（审查 §7.1-A 的核心回归）。
    assert_eq!(
        q::bulk_set_availability(&conn, 1, "online", "offline").unwrap(),
        0,
        "源卷下线不得影响已搬到目标卷的条目"
    );
    assert_eq!(media_row(&conn, 100).availability, "online");
    assert_eq!(media_row(&conn, 101).availability, "online");
    assert!(q::get_item_volume_offline_label(&conn, 100)
        .unwrap()
        .is_none());
}

// ── 3. 跨根同相对路径：cache_key 不变也要更新身份与绝对缩略图 ────────────────

/// 审查 F-003：旧实现「cache_key 相同即 continue」会漏掉 source-direct 绝对路径与卷定位。
#[test]
fn cross_root_same_relative_path_updates_volume_and_source_direct_thumb() {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(20, 2, None, "Photos", "Photos", 1);
    let src_abs = f
        .root_a
        .join("Photos/Move/a.jpg")
        .to_string_lossy()
        .to_string();
    let key = f.media(100, 10, "a.jpg", "Photos/Move", 1, 3, Some(&src_abs));

    let plan = f.plan(10, 20);
    assert_eq!(
        plan.new_rel, plan.old_rel,
        "前置：跨根但相对路径相同（cache_key 不变）"
    );
    run_move(&f, &plan).unwrap();

    let conn = f.conn();
    let after = media_row(&conn, 100);
    assert_eq!(after.cache_key, key, "相对路径相同 → cache_key 不变");
    assert_eq!(after.volume_id, Some(2), "卷身份必须更新（不能被跳过）");
    assert_eq!(
        after.volume_relative_path.as_deref(),
        Some("volB/Photos/Move/a.jpg")
    );
    let expect_thumb = resolve_media_path(&f.root_b.to_string_lossy(), "Photos/Move", "a.jpg");
    assert_eq!(
        after.thumb_path.as_deref(),
        Some(expect_thumb.as_str()),
        "thumb_status=3 的绝对源路径必须指向新位置"
    );
    assert!(!f.exists(&f.root_a, "Photos/Move"));
}

// ── 4. DB 注入失败：可恢复状态 + 重试幂等 ────────────────────────────────────

/// 物理已发布、DB 事务失败：返回稳定码 + 日志 id + 目标绝对路径，重试后收敛且幂等。
#[test]
fn db_failure_after_publish_returns_recoverable_state_and_retries_idempotently() {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.write(&f.root_a, "Photos/Move/sub/b.jpg", "bbbb");
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(11, 1, Some(10), "Photos/Move/sub", "sub", 3);
    f.dir(20, 2, None, "Dest", "Dest", 1);
    let key_a = f.media(100, 10, "a.jpg", "Photos/Move", 1, 1, None);
    f.media(101, 11, "b.jpg", "Photos/Move/sub", 1, 1, None);

    let plan = f.plan(10, 20);
    // 真实 DB 故障注入：不引入测试专用生产分支，用触发器让媒体行写入必定失败。
    f.conn()
        .execute_batch(
            "CREATE TRIGGER fail_media_update BEFORE UPDATE ON media_items
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    let err = run_move(&f, &plan).unwrap_err();
    let (recovery_id, target_abs) = match &err {
        AppError::MoveRecovery {
            code,
            recovery_id,
            target_abs_path,
            ..
        } => {
            assert_eq!(*code, CODE_DB_PENDING);
            (*recovery_id, target_abs_path.clone())
        }
        other => panic!("应返回可恢复状态，实际: {other:?}"),
    };
    // IPC 载荷：稳定码 + 恢复定位（前端据此显示真实位置与重试入口）。
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["code"], CODE_DB_PENDING);
    assert_eq!(v["recoveryId"].as_i64(), Some(recovery_id));
    assert_eq!(v["targetAbsPath"].as_str(), Some(target_abs.as_str()));

    assert!(!f.exists(&f.root_a, "Photos/Move"));
    assert_eq!(f.read(&f.root_b, "Dest/Move/sub/b.jpg"), "bbbb");
    {
        let conn = f.conn();
        let pending = q::list_pending(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].stage, q::STAGE_PUBLISHED);
        assert_eq!(pending[0].id, recovery_id);
        assert_eq!(
            dir_row(&conn, 10).2,
            "Photos/Move",
            "库未改：目录仍在原路径"
        );
        assert_eq!(media_row(&conn, 100).volume_id, Some(1));
    }

    // 解除注入 → 重试：身份重写收敛，日志清空。
    f.conn()
        .execute_batch("DROP TRIGGER fail_media_update;")
        .unwrap();
    let report = retry_entry(f.db(), &f.cache_dir(), recovery_id)
        .unwrap()
        .expect("日志行仍在");
    assert!(!report.needs_retry, "重试后不应仍需重试: {report:?}");
    {
        let conn = f.conn();
        assert!(
            q::list_pending(&conn).unwrap().is_empty(),
            "收尾后日志行删除"
        );
        assert_eq!(
            dir_row(&conn, 10),
            (2, Some(20), "Dest/Move".to_string(), 2)
        );
        let after = media_row(&conn, 100);
        assert_eq!(after.volume_id, Some(2));
        assert_ne!(after.cache_key, key_a);
        assert_eq!((after.is_favorited, after.rating), (1, 4));
    }
    // 再重试：日志行已不存在 → None（幂等，不重复搬运/改写）。
    assert!(retry_entry(f.db(), &f.cache_dir(), recovery_id)
        .unwrap()
        .is_none());
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));
}

// ── 5. 复制中断的两个崩溃窗（凭据先落盘）────────────────────────────────────

/// 「复制完成、发布前」崩溃：日志已有内容凭据，重启收尾保守留待重试，显式重试直接复用暂存
/// （不重复拷贝）并完成发布 + 索引重写。
#[test]
fn crash_before_publish_is_recovered_by_persisted_payload() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    // 手工模拟：已复制到我们自己的暂存并算好凭据（不发布）。
    let payload = prepare_staging(&plan).unwrap();
    let journal = f.journal(
        q::STAGE_INTENT,
        10,
        Some(&plan.staging_abs),
        Some((&payload.digest, payload.files as i64)),
    );

    // 启动档：源在、目标不在 → 留待重试，且不碰暂存（省下重拷）。
    let startup = reconcile_at_startup(f.db(), &f.cache_dir()).unwrap();
    assert_eq!(startup[0].detail, "staging_busy");
    assert!(plan.staging_abs.exists(), "启动档不碰暂存（重试时可复用）");
    assert!(!f.exists(&f.root_b, "Dest/Move"), "尚未发布");

    // 显式重试：暂存凭据一致 → 直接发布 + 改写索引 + 清理源。
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(!report.needs_retry, "{report:?}");
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));
    assert!(!f.exists(&f.root_a, "Photos/Move"), "证明一致后源被清理");
    assert!(!plan.staging_abs.exists(), "暂存已发布，不留残留");
    {
        let conn = f.conn();
        assert!(q::list_pending(&conn).unwrap().is_empty());
        assert_eq!(
            dir_row(&conn, 10),
            (2, Some(20), "Dest/Move".to_string(), 2)
        );
        assert_eq!(media_row(&conn, 100).volume_id, Some(2));
    }
}

/// 「发布完成、删源前」崩溃：日志已带凭据 → 双端存在不再被判成外部冲突，重试完成收尾。
#[test]
fn crash_after_publish_before_delete_source_converges() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    let payload = prepare_staging(&plan).unwrap();
    let journal = f.journal(
        q::STAGE_INTENT,
        10,
        Some(&plan.staging_abs),
        Some((&payload.digest, payload.files as i64)),
    );
    // 发布（rename）已发生，删源尚未发生。
    std::fs::rename(&plan.staging_abs, &plan.dst_abs).unwrap();

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(
        !report.needs_retry,
        "双端存在 + 凭据一致 → 可认领: {report:?}"
    );
    {
        let conn = f.conn();
        assert_eq!(
            dir_row(&conn, 10),
            (2, Some(20), "Dest/Move".to_string(), 2)
        );
        assert_eq!(media_row(&conn, 100).volume_id, Some(2));
        assert!(q::list_pending(&conn).unwrap().is_empty());
    }
    assert!(!f.exists(&f.root_a, "Photos/Move"));
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));
}

/// 若凭据是在发布**之后**才可能落盘（旧序），这组物理状态就永远收不了尾——本测试锁住
/// 「有凭据才认领」的判据：无凭据的双端存在必须保持冲突。
#[test]
fn both_present_without_payload_stays_conflict() {
    let f = basic_fixture();
    f.write(&f.root_b, "Dest/Move/a.jpg", "aaa"); // 目标已有同内容目录
    let journal = f.journal(q::STAGE_INTENT, 10, None, None);

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert_eq!(report.detail, "target_conflict");
    assert!(report.needs_retry);
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaa", "源不动");
    assert_eq!(f.read(&f.root_b, "Dest/Move/a.jpg"), "aaa", "目标不动");
    let conn = f.conn();
    assert_eq!(dir_row(&conn, 10).2, "Photos/Move", "库不动");
    assert_eq!(q::list_pending(&conn).unwrap().len(), 1, "日志保留");
}

/// 摘要对不上（目标是别人写了一半的同名目录）：不认领、不改库、不删源。
#[test]
fn mismatched_payload_never_claims_target() {
    let f = basic_fixture();
    f.write(&f.root_b, "Dest/Move/other.jpg", "different");
    let journal = f.journal(q::STAGE_INTENT, 10, None, Some(("deadbeef", 1)));

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert_eq!(report.detail, "target_conflict");
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaa");
    assert_eq!(f.read(&f.root_b, "Dest/Move/other.jpg"), "different");
    assert_eq!(dir_row(&f.conn(), 10).2, "Photos/Move");
}

// ── 6. 卷离线/根缺失：不丢恢复线索 ──────────────────────────────────────────

/// 凭据写入被拒 → **绝不发布**：目标未落盘、源完整、我们自己的暂存留着、日志可重试。
/// （内容凭据是发布的硬前置，不是 best-effort。）
#[test]
fn refused_payload_write_blocks_publish_and_keeps_source() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    let journal = f.journal(q::STAGE_INTENT, 10, Some(&plan.staging_abs), None);
    // 故障注入：日志行的任何更新都被拒（模拟 DB 暂不可用）。
    f.conn()
        .execute_batch(
            "CREATE TRIGGER refuse_journal_update BEFORE UPDATE ON directory_move_journal
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();

    let err = stage_and_persist(f.db(), &plan, journal).unwrap_err();
    match &err {
        AppError::MoveRecovery {
            code,
            recovery_id,
            target_abs_path,
            ..
        } => {
            assert_eq!(*code, CODE_DB_PENDING);
            assert_eq!(*recovery_id, journal);
            assert_eq!(target_abs_path, &plan.dst_abs.to_string_lossy().to_string());
        }
        other => panic!("应返回可重试的明确失败: {other:?}"),
    }

    assert!(!f.exists(&f.root_b, "Dest/Move"), "凭据没落盘就绝不发布");
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaa", "源完整");
    assert_eq!(dir_row(&f.conn(), 10).2, "Photos/Move", "索引不动");
    let pending = q::list_pending(&f.conn()).unwrap();
    assert_eq!(pending.len(), 1, "日志保留待重试");
    assert_eq!(pending[0].payload_digest, None, "凭据确实没写进去");

    // 解除注入 → 重试收敛（重试会先清掉无法证明的暂存再重新复制）。
    f.conn()
        .execute_batch("DROP TRIGGER refuse_journal_update;")
        .unwrap();
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(!report.needs_retry, "{report:?}");
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));
    assert!(!f.exists(&f.root_a, "Photos/Move"));
    assert!(q::list_pending(&f.conn()).unwrap().is_empty());
}

/// 重试路径同理：凭据写不进去 → 不发布、报 payload_pending、源与日志都留着。
#[test]
fn retry_without_payload_persistence_keeps_journal_and_source() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    let journal = f.journal(q::STAGE_INTENT, 10, Some(&plan.staging_abs), None);
    f.conn()
        .execute_batch(
            "CREATE TRIGGER refuse_journal_update BEFORE UPDATE ON directory_move_journal
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(report.needs_retry, "{report:?}");
    assert_eq!(report.detail, "payload_pending");
    assert!(!f.exists(&f.root_b, "Dest/Move"), "未发布");
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaa", "源完整");
    assert!(
        plan.staging_abs.exists(),
        "我们自己的暂存留着（下次重试可复用或重做）"
    );
    assert_eq!(q::list_pending(&f.conn()).unwrap().len(), 1, "日志保留");
}

/// 两侧路径都看不见（卷离线常表现为 NotFound）→ 绝不作废日志（审查复核 6）。
#[test]
fn offline_roots_do_not_discard_journal() {
    let f = Fixture::new();
    // 源那侧整棵不见（模拟源卷离线），目标那侧根也不在。
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(20, 2, None, "Dest", "Dest", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    let journal = f.journal(q::STAGE_INTENT, 10, None, None);
    // 目标根路径指向不存在的位置 → 根不可访问。
    f.conn()
        .execute(
            "UPDATE scan_roots SET path=?1 WHERE id=2",
            params![f
                .tmp
                .path()
                .join("missing-volB")
                .to_string_lossy()
                .to_string()],
        )
        .unwrap();

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(report.needs_retry, "{report:?}");
    assert_ne!(report.detail, "absent", "卷离线不得被当成「什么都没发生」");
    assert_eq!(
        q::list_pending(&f.conn()).unwrap().len(),
        1,
        "恢复线索必须保留"
    );
}

/// 目标根行确实不存在（清库/删根后的终态）才作废日志；查询失败一律保留。
#[test]
fn missing_root_row_only_drops_journal_when_row_is_really_gone() {
    let f = basic_fixture();
    let journal = f.journal(q::STAGE_INTENT, 10, None, None);
    // 行被删掉（模拟根被清库删除）→ 终态，日志可作废。
    f.conn()
        .execute("DELETE FROM scan_roots WHERE id=2", [])
        .unwrap();
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert_eq!(report.detail, "target_root_missing");
    assert!(q::list_pending(&f.conn()).unwrap().is_empty());
}

/// 目标父目录行被删（重链接/清理带走了它）→ 保留日志，不写 NULL 父指针。
#[test]
fn unresolvable_parent_keeps_journal_without_null_parent() {
    let f = basic_fixture();
    let journal = f.journal(q::STAGE_INTENT, 10, None, None);
    f.conn()
        .execute("DELETE FROM directories WHERE id=20", [])
        .unwrap();
    // 父 id 记在日志里，但行已不在，且按 rel 反查也找不到 → 计划重建失败。
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(report.needs_retry);
    assert_eq!(report.detail, "target_unresolved");
    let conn = f.conn();
    assert_eq!(q::list_pending(&conn).unwrap().len(), 1, "日志保留");
    assert_eq!(dir_row(&conn, 10).2, "Photos/Move", "库不动");
}

// ── 7. 删源证明：内容一致才删 ────────────────────────────────────────────────

/// 删源必须逐文件证明内容：同尺寸改写、源里多出的文件都算「源不是目标的子集」→ 绝不删；
/// 反过来目标多出文件（源被完整包含）→ 可安全收敛。
#[test]
fn source_removal_requires_content_proof() {
    // ① 同长不同内容。
    let f = basic_fixture();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaaa");
    f.write(&f.root_b, "Dest/Move/a.jpg", "bbbb");
    let journal = f.journal(q::STAGE_SOURCE_LEFTOVER, 10, None, None);
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(report.needs_retry);
    assert_eq!(
        report.detail, "source_changed",
        "同尺寸改写必须被内容比对拦下"
    );
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaaa", "源保留");
    assert_eq!(q::list_pending(&f.conn()).unwrap().len(), 1);

    // ② 源里多出文件（用户后放）→ 删源会丢掉它，必须保留。
    let f2 = basic_fixture();
    f2.write(&f2.root_a, "Photos/Move/a.jpg", "same");
    f2.write(&f2.root_a, "Photos/Move/extra.txt", "later");
    f2.write(&f2.root_b, "Dest/Move/a.jpg", "same");
    let journal2 = f2.journal(q::STAGE_SOURCE_LEFTOVER, 10, None, None);
    let report2 = retry_entry(f2.db(), &f2.cache_dir(), journal2)
        .unwrap()
        .unwrap();
    assert!(report2.needs_retry, "{report2:?}");
    assert_eq!(report2.detail, "source_changed");
    assert!(
        f2.exists(&f2.root_a, "Photos/Move/extra.txt"),
        "源里独有内容不得丢失"
    );

    // ③ 源的内容被目标完整包含（目标多出一个文件）→ 子集成立，可收敛。
    let f3 = basic_fixture();
    f3.write(&f3.root_a, "Photos/Move/a.jpg", "same");
    f3.write(&f3.root_b, "Dest/Move/a.jpg", "same");
    f3.write(&f3.root_b, "Dest/Move/target-only.txt", "kept");
    let journal3 = f3.journal(q::STAGE_SOURCE_LEFTOVER, 10, None, None);
    let report3 = retry_entry(f3.db(), &f3.cache_dir(), journal3)
        .unwrap()
        .unwrap();
    assert!(!report3.needs_retry, "{report3:?}");
    assert!(!f3.exists(&f3.root_a, "Photos/Move"), "可收敛，不留死结");
    assert!(f3.exists(&f3.root_b, "Dest/Move/target-only.txt"));
    assert!(q::list_pending(&f3.conn()).unwrap().is_empty());
}

/// 源是指向根外的符号链接 → 直接拒绝（不搬、不删根外的东西）。
#[cfg(unix)]
#[test]
fn symlink_escaping_root_is_rejected() {
    let f = basic_fixture();
    let outside = f.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.jpg"), b"secret").unwrap();
    // 基础夹具已创建普通目录，先移开，才能在同一路径构造逃逸符号链接。
    std::fs::rename(
        f.root_a.join("Photos/Move"),
        f.root_a.join("Photos/Original"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, f.root_a.join("Photos/Move")).unwrap();

    let err = load_plan(&f.conn(), 10, 20).unwrap_err();
    assert!(matches!(err, AppError::PathResolution(_)), "{err:?}");
    assert!(outside.join("secret.jpg").exists(), "根外内容不受影响");
}

// ── 8. 启动收尾：只做可证的短收敛 ────────────────────────────────────────────

/// published 阶段、源与目标都在（跨卷「发布完成、删源前」崩溃）：启动档重放索引，
/// 但**不动源**（整树比对留给显式重试）。
#[test]
fn startup_recovery_rewrites_index_without_touching_source() {
    let f = basic_fixture();
    // 跨卷形态：源还在、目标已有同一棵树的副本（我们复制过去的），阶段已推进为 published。
    f.write(&f.root_a, "Photos/Move/a.jpg", "same");
    f.write(&f.root_b, "Dest/Move/a.jpg", "same");
    let payload = tree_payload_digest(&f.root_b.join("Dest/Move")).unwrap();
    let journal = f.journal(
        q::STAGE_PUBLISHED,
        10,
        None,
        Some((&payload.digest, payload.files as i64)),
    );

    let reports = reconcile_at_startup(f.db(), &f.cache_dir()).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].needs_retry, "{reports:?}");
    assert_eq!(reports[0].detail, "source_leftover");
    {
        let conn = f.conn();
        assert_eq!(
            dir_row(&conn, 10),
            (2, Some(20), "Dest/Move".to_string(), 2)
        );
        assert_eq!(media_row(&conn, 100).volume_id, Some(2));
        let pending = q::list_pending(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, journal);
        assert_eq!(
            pending[0].stage,
            q::STAGE_SOURCE_LEFTOVER,
            "阶段推进待显式清理"
        );
    }
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));

    // 幂等：再跑一次结果稳定，不重复改写。
    let again = reconcile_at_startup(f.db(), &f.cache_dir()).unwrap();
    assert_eq!(again[0].detail, "source_leftover");
    assert_eq!(dir_row(&f.conn(), 10).2, "Dest/Move");
}

/// published 阶段但目标不在（卷离线/被删）→ 保留日志与旧索引状态，绝不假收尾。
#[test]
fn recovery_keeps_journal_when_published_target_missing() {
    let f = basic_fixture();
    let journal = f.journal(q::STAGE_PUBLISHED, 10, None, None);
    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(report.needs_retry);
    assert_eq!(report.detail, "target_missing");
    let conn = f.conn();
    assert_eq!(dir_row(&conn, 10).2, "Photos/Move", "旧索引状态保留");
    assert_eq!(media_row(&conn, 100).volume_id, Some(1));
    assert_eq!(q::list_pending(&conn).unwrap().len(), 1, "日志保留");
}

/// 两端都不在且两侧根都可见（真的什么都没发生）→ 日志行作废，不做任何改写。
#[test]
fn startup_recovery_drops_intent_when_nothing_happened() {
    let f = Fixture::new();
    f.dir(9, 1, None, "Photos", "Photos", 1);
    f.dir(10, 1, Some(9), "Photos/Move", "Move", 2);
    f.dir(20, 2, None, "Dest", "Dest", 1);
    f.media(100, 10, "a.jpg", "Photos/Move", 1, 1, None);
    let journal = f.journal(q::STAGE_INTENT, 10, None, None);

    let reports = reconcile_at_startup(f.db(), &f.cache_dir()).unwrap();
    assert_eq!(reports[0].detail, "absent");
    {
        let conn = f.conn();
        assert!(q::list_pending(&conn).unwrap().is_empty());
        assert_eq!(dir_row(&conn, 10).2, "Photos/Move", "库保持原状");
    }
    assert!(!f.exists(&f.root_b, "Dest/Move"));
    assert!(retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .is_none());
}

/// 启动档不做大拷贝：intent 且源仍在 → 保持待重试；用户显式重试才完成搬运与索引。
#[test]
fn startup_recovery_defers_physical_copy_until_retry() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    let journal = f.journal(q::STAGE_INTENT, 10, Some(&plan.staging_abs), None);

    let reports = reconcile_at_startup(f.db(), &f.cache_dir()).unwrap();
    assert!(reports[0].needs_retry);
    assert_eq!(reports[0].detail, "staging_busy");
    assert!(f.exists(&f.root_a, "Photos/Move/a.jpg"), "源原件不动");
    assert!(!f.exists(&f.root_b, "Dest/Move"), "启动档不重做拷贝");
    assert_eq!(q::list_pending(&f.conn()).unwrap().len(), 1);

    let report = retry_entry(f.db(), &f.cache_dir(), journal)
        .unwrap()
        .unwrap();
    assert!(!report.needs_retry, "{report:?}");
    assert!(f.exists(&f.root_b, "Dest/Move/a.jpg"));
    assert!(!f.exists(&f.root_a, "Photos/Move"));
    let conn = f.conn();
    assert!(q::list_pending(&conn).unwrap().is_empty());
    assert_eq!(media_row(&conn, 100).volume_id, Some(2));
}

// ── 9. 目标冲突、暂存独占、计划复核 ──────────────────────────────────────────

/// 目标物理存在但未索引：拒绝覆盖，源保持完整，日志不留残留行。
#[test]
fn existing_unindexed_target_is_never_overwritten() {
    let f = basic_fixture();
    f.write(&f.root_b, "Dest/Move/keep.txt", "user-data");

    let plan = f.plan(10, 20);
    let err = run_move(&f, &plan).unwrap_err();
    assert!(
        matches!(err, AppError::DirectoryExists(_)),
        "应拒绝覆盖目标: {err:?}"
    );
    assert_eq!(f.read(&f.root_b, "Dest/Move/keep.txt"), "user-data");
    assert_eq!(f.read(&f.root_a, "Photos/Move/a.jpg"), "aaa");
    let conn = f.conn();
    assert_eq!(dir_row(&conn, 10).2, "Photos/Move", "库不动");
    assert!(q::list_pending(&conn).unwrap().is_empty(), "日志不留行");
}

/// 暂存目录必须独占创建：已存在（不是我们的产物）→ 失败且原样保留；无标记目录不清理。
#[test]
fn staging_is_created_exclusively_and_foreign_paths_are_kept() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    std::fs::create_dir_all(f.root_b.join("Dest")).unwrap();
    std::fs::write(&plan.staging_abs, b"not-ours").unwrap();

    let err = prepare_staging(&plan).unwrap_err();
    assert!(matches!(err, AppError::MoveFile(_)), "{err:?}");
    assert!(plan.staging_abs.exists(), "不是我们的产物，绝不删除或复用");
    assert!(f.exists(&f.root_a, "Photos/Move/a.jpg"), "源完整");
    assert!(!f.exists(&f.root_b, "Dest/Move"), "未发布");

    let foreign_dir = f.root_b.join("Dest/.scrollery-move-Move-lookalike");
    std::fs::create_dir_all(&foreign_dir).unwrap();
    std::fs::write(foreign_dir.join("data.bin"), b"x").unwrap();
    cleanup_own_staging(&foreign_dir);
    assert!(foreign_dir.join("data.bin").exists(), "无兄弟标记不清理");
    // 只有兄弟标记存在时才是我们的暂存目录。
    std::fs::write(staging_marker_path(&foreign_dir), b"scrollery move staging").unwrap();
    cleanup_own_staging(&foreign_dir);
    assert!(!foreign_dir.exists(), "带标记的暂存目录可以清理");
    assert!(
        !staging_marker_path(&foreign_dir).exists(),
        "标记文件一并清掉"
    );
}

/// 取闸后的计划复核：源身份被别的操作改过 → 拒绝用过期计划动磁盘。
#[test]
fn stale_plan_is_rejected_after_identity_change() {
    let f = basic_fixture();
    let plan = f.plan(10, 20);
    {
        let conn = f.conn();
        assert!(verify_plan_current(&conn, &plan).is_ok());
        // 等待闸门期间，另一次操作把源搬走了。
        conn.execute(
            "UPDATE directories SET rel_path='Elsewhere' WHERE id=10",
            [],
        )
        .unwrap();
        assert!(
            verify_plan_current(&conn, &plan).is_err(),
            "必须拒绝过期计划"
        );
        // 目标下出现同名索引目录 → 也是过期。
        conn.execute(
            "UPDATE directories SET rel_path='Photos/Move' WHERE id=10",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth)
             VALUES (21, 2, 20, 'Dest/Move', 'Move', 1)",
            [],
        )
        .unwrap();
        assert!(verify_plan_current(&conn, &plan).is_err());
    }
    assert!(f.exists(&f.root_a, "Photos/Move/a.jpg"), "复核失败不动磁盘");
}

// ── 10. 复制 ─────────────────────────────────────────────────────────────────

/// 复制走独占暂存 + rename 发布：文件数正确、源不变、目标已存在一律拒绝。
#[test]
fn copy_publishes_complete_tree_and_counts_files() {
    let f = Fixture::new();
    f.write(&f.root_a, "Photos/Move/a.jpg", "aaa");
    f.write(&f.root_a, "Photos/Move/sub/b.jpg", "bbbb");
    f.write(&f.root_a, "Photos/Move/sub/c.jpg", "c");
    let src = f.root_a.join("Photos/Move");
    let dst = f.root_b.join("Dest/Move");
    std::fs::create_dir_all(f.root_b.join("Dest")).unwrap();

    let files = copy_tree_published(&src, &dst, "Move").unwrap();
    assert_eq!(files, 3);
    assert_eq!(std::fs::read_to_string(dst.join("sub/c.jpg")).unwrap(), "c");
    assert_eq!(std::fs::read_to_string(src.join("a.jpg")).unwrap(), "aaa");

    std::fs::write(dst.join("keep.txt"), b"user").unwrap();
    let err = copy_tree_published(&src, &dst, "Move").unwrap_err();
    assert!(matches!(err, AppError::DirectoryExists(_)), "{err:?}");
    assert_eq!(
        std::fs::read_to_string(dst.join("keep.txt")).unwrap(),
        "user"
    );
    assert_eq!(std::fs::read_to_string(src.join("a.jpg")).unwrap(), "aaa");
}

// ── 11. 小工具 ───────────────────────────────────────────────────────────────

/// 内容凭据对遍历顺序不敏感、对内容敏感。
#[test]
fn payload_digest_is_order_insensitive_but_content_sensitive() {
    let f = Fixture::new();
    f.write(&f.root_a, "x/a.jpg", "aaa");
    f.write(&f.root_a, "x/sub/b.jpg", "bbbb");
    std::fs::create_dir_all(f.root_b.join("y/sub")).unwrap();
    f.write(&f.root_b, "y/sub/b.jpg", "bbbb");
    f.write(&f.root_b, "y/a.jpg", "aaa");
    let left = tree_payload_digest(&f.root_a.join("x")).unwrap();
    let right = tree_payload_digest(&f.root_b.join("y")).unwrap();
    assert_eq!(left.digest, right.digest, "同内容同结构 → 同摘要");
    assert_eq!(left.files, 2);

    f.write(&f.root_b, "y/a.jpg", "aab");
    assert_ne!(
        tree_payload_digest(&f.root_b.join("y")).unwrap().digest,
        left.digest,
        "内容变了摘要必须变"
    );
}

/// 卷内相对路径：目标根未绑定卷 → NULL；绑定卷 → 卷 subpath + 目录 rel + 文件名。
#[test]
fn volume_relative_path_requires_bound_volume() {
    assert_eq!(volume_relative_path(None, Some("vol"), "a", "b.jpg"), None);
    assert_eq!(
        volume_relative_path(Some(2), Some("vol"), "a/b", "c.jpg"),
        Some("vol/a/b/c.jpg".to_string())
    );
    assert_eq!(
        volume_relative_path(Some(2), Some("/"), "", "c.jpg"),
        Some("c.jpg".to_string())
    );
}
