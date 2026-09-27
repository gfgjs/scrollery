//! 数据库连接管理。
//! - 写入路径：`Mutex<Connection>` — 序列化所有写入操作。
//! - 读取路径：`r2d2::Pool<SqliteConnectionManager>` — WAL 模式下的并发读取。

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info};

use crate::error::{AppError, Result};

/// 为清晰起见定义类型别名。
pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbWriter = Mutex<Connection>;

/// 适用于每个连接（写入 + 每个读池连接）的 PRAGMA 语句。
/// journal_size_limit：checkpoint 后把 WAL 文件截回 64MiB 上限,防其高水位长期驻留(通用卫生封顶;
/// 注:「WAL 膨胀致热启动慢」假设已被启动探针数据证伪〔全日开机 WAL≤10.4MB〕,故此为卫生改而非该问题修复)。
const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA journal_size_limit = 67108864;
PRAGMA synchronous  = NORMAL;
PRAGMA cache_size   = -64000;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store   = MEMORY;
PRAGMA mmap_size    = 268435456;
";

/// 将性能相关的 PRAGMA 应用于连接。
/// `pub(crate)`:数据备份(方案 B §5.3)的独立 VACUUM INTO 连接须应用**相同** PRAGMA,复用此处单一事实源。
pub(crate) fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(PRAGMAS).map_err(AppError::from)
}

// ── 写入连接 ────────────────────────────────────────────────────────

/// 打开写入连接（读写，通过 `Mutex` 串行化访问）。
pub fn create_write_connection(db_path: &Path) -> Result<DbWriter> {
    info!(
        "Opening write connection at {:?} | 正在 {:?} 建立数据库写连接",
        db_path, db_path
    );
    let conn = Connection::open(db_path)?;
    apply_pragmas(&conn)?;
    crate::db::register_custom_collations(&conn).map_err(AppError::from)?;
    debug!("Write connection PRAGMAs applied");
    Ok(Mutex::new(conn))
}

/// 启动期 WAL 截断（S3.6，S3.7 修正调用时机）：退出钩子只覆盖正常退出——dev Ctrl+C/崩溃/
/// 强杀会让 WAL 带着整个会话的管线写量（缩略图/富化/AI）跨会话累积，拖慢后续所有读。
/// **调用方须在 tracing 订阅器就绪后调用**（原挂在 create_write_connection 内，先于日志
/// 初始化,info!/warn! 被静默丢弃——S3.6 首轮真机看不到日志的原因）。setup 内 tracing init
/// 之后、管线拉起之前调用：读池连接已归还、无并发读者，TRUNCATE 可完整回收；WAL 越大本步
/// 越久（一次性清偿，日志可见），失败仅告警不阻断启动。
pub(crate) fn checkpoint_wal_at_boot(conn: &Connection, db_path: &Path) {
    // SQLite WAL 命名 = 数据库路径直接追加 "-wal"（不是替换扩展名）。
    let wal_path = {
        let mut p = db_path.as_os_str().to_owned();
        p.push("-wal");
        std::path::PathBuf::from(p)
    };
    let size_mb = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| m.len() as f64 / 1_048_576.0)
            .unwrap_or(0.0)
    };
    let before = size_mb(&wal_path);
    // 返回 (busy, WAL 总页数, 已检查点页数)；非 WAL 库返回 (0, -1, -1)，无害。
    // 计时(2026-07-13 排查热启动变慢):WAL 膨胀是热启动变慢头号疑犯——被中断的全量生成会把
    // 海量写留在 WAL,此后每次启动都在此一次性截断,秒级阻塞。故记录耗时,慢/大于阈值升 warn
    // (warn 级也可见),下次复现即可坐实是否此步。
    let t0 = std::time::Instant::now();
    let result: rusqlite::Result<(i64, i64, i64)> =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    let elapsed_ms = t0.elapsed().as_millis();
    match result {
        Ok((busy, log, ckpt)) => {
            let after = size_mb(&wal_path);
            if elapsed_ms > 500 || before > 64.0 {
                tracing::warn!(
                    "Boot WAL checkpoint: {:.1}MB → {:.1}MB, {}ms (busy={}, pages {}/{})——耗时/体积偏高,疑热启动阻塞源 | 启动期 WAL 截断",
                    before, after, elapsed_ms, busy, ckpt, log
                );
            } else {
                info!(
                    "Boot WAL checkpoint: {:.1}MB → {:.1}MB, {}ms (busy={}, pages {}/{}) | 启动期 WAL 截断",
                    before, after, elapsed_ms, busy, ckpt, log
                );
            }
        }
        Err(e) => tracing::warn!("Boot WAL checkpoint failed | 启动期 WAL 截断失败: {}", e),
    }
}

// ── 读取池 ───────────────────────────────────────────────────────────────

/// 自定义每个读取池连接：只读标志 + PRAGMA。
#[derive(Debug)]
struct ReadPoolCustomiser;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for ReadPoolCustomiser {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(PRAGMAS)?;
        crate::db::register_custom_collations(conn)?;
        Ok(())
    }
}

/// 创建读取连接池。
///
/// `pool_size`：桌面端为 8，移动端为 2（由调用者决定）。
/// 桌面端 8 的理由见 `lib.rs` 建池处:前台布局 + 可视区元数据 + 缩略图批与后台派生/AI
/// 并发读,4 会让它们互相排队(布局被后台读饿死);WAL 下额外读连接开销很低（指锁竞争而非
/// 内存）。但每个连接持有各自私有的 page cache（`cache_size=-64000` ≈ 64 MiB，见 PRAGMAS），
/// 故并发打满时 pool_size=8 读池最多可占约 512 MiB page cache——这是瞬时占用（`min_idle(0)` +
/// r2d2 空闲回收,闲置连接会被收掉),不是泄漏。
pub fn create_read_pool(db_path: &Path, pool_size: u32) -> Result<DbPool> {
    info!(
        "Opening read pool at {:?} with max_size={} | 正在 {:?} 建立读连接池，最大连接数={}",
        db_path, pool_size, db_path, pool_size
    );

    let manager = SqliteConnectionManager::file(db_path).with_flags(
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    );

    let pool = Pool::builder()
        .max_size(pool_size)
        // 延迟连接创建到首次使用，避免在冷启动时阻塞 setup()（至多 `pool_size` 次 SQLite 打开 + PRAGMA 批次）
        .min_idle(Some(0))
        .connection_customizer(Box::new(ReadPoolCustomiser))
        .build(manager)
        .map_err(AppError::Pool)?;

    debug!("Read pool created successfully");
    Ok(pool)
}
