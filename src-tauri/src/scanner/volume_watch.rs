// src-tauri/src/scanner/volume_watch.rs
//! 卷插拔监听（poll 轮询，Part2 T2 / C5 Piece B）。
//!
//! 后台线程定期对账「已知卷」的在线态，实时维护 `volumes.is_online` 与
//! `media_items.availability`(online↔offline)，**不必等扫描**。拔盘 ≤15s 画廊变灰「离线」，
//! 插回 ≤15s 恢复。
//!
//! **正交铁律**：监听只切 availability(online↔offline)，**绝不碰 `is_deleted`、绝不动 `'missing'`**。
//! `offline` 是卷级（可自动恢复），`missing` 是扫描差集的文件级结论，二者不互转——离线卷重连后
//! 由扫描恢复 missing，监听只管 online/offline（Part1 §3.3c 硬规则「离线≠删除」的运行期落点）。
//!
//! 机制选择：plan §3.1.2-3 既定「15s 轮询兜底」即作 v1 主机制——跨平台、零原生 FFI、最简。
//! 未来可叠加 Windows `WM_DEVICECHANGE` 仅作「提前唤醒轮询」，对账逻辑不变。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::db::queries as q;
use crate::error::Result;
use crate::scanner::volume_probe::{PathProber, VolumeOnlineCheck};
use crate::state::AppState;

/// 轮询间隔（plan §3.1.2-3 既定 15s）。
const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// 冷启动后首轮对账的延迟：让初次扫描先行，避开冷启动锁竞争，同时仍能尽早对账掉线卷。
const STARTUP_DELAY: Duration = Duration::from_secs(5);

/// Tauri 事件名：有卷在线态变化时发出，前端据此刷新画廊（离线徽标显隐）。
pub const EVENT_VOLUMES_CHANGED: &str = "volumes:changed";

/// 一次对账中某卷的状态变化（用于 emit 与单测断言）。
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeChange {
    pub volume_id: i64,
    pub stable_id: String,
    pub now_online: bool,
}

/// 对账一次：比对每个「已知卷」当前在线态与 DB 记录，**仅在变化时**写库
/// （`set_volume_online` 翻转卷态 + `bulk_set_availability` 整盘 online↔offline），返回变化列表。
///
/// 纯逻辑 + 可注入 `VolumeOnlineCheck`，便于单测「拔盘→离线、插回→在线、未变→零写、missing 不动」。
pub fn run_once(
    conn: &Connection,
    checker: &dyn VolumeOnlineCheck,
    now: i64,
) -> Result<Vec<VolumeChange>> {
    let volumes = q::list_volumes(conn)?;
    let mut changes = Vec::new();

    for v in volumes {
        // 无挂载点的卷无法判定在线态 → 跳过（保守，不动其状态）。
        let Some(mount) = v.last_mount_path.as_deref() else {
            continue;
        };
        let online_now = checker.is_online(Path::new(mount));
        if online_now == v.is_online {
            continue; // 未变 → 零写、零事件
        }

        // 翻转 DB 卷态（mount_path=None：保留最后已知挂载点）+ 整盘 availability 切换。
        q::set_volume_online(conn, &v.stable_id, online_now, None, now)?;
        if online_now {
            // 重连：仅 'offline' → 'online'（'missing' 不在过滤内，天然不被触碰）。
            q::bulk_set_availability(conn, v.id, "offline", "online")?;
        } else {
            // 拔出：仅 'online' → 'offline'（同理不碰 'missing' / is_deleted）。
            q::bulk_set_availability(conn, v.id, "online", "offline")?;
        }

        changes.push(VolumeChange {
            volume_id: v.id,
            stable_id: v.stable_id,
            now_online: online_now,
        });
    }

    Ok(changes)
}

/// 当前 Unix 秒（卷态 last_seen 用）。系统时钟异常时回退 0（不影响在线判定，仅影响时间戳）。
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 起卷监听后台任务：冷启动延迟后首轮对账，随后每 15s 一轮；有变化即 emit [`EVENT_VOLUMES_CHANGED`]。
///
/// 锁纪律：每轮「取写锁 → 对账 → 立即释放」，`sleep` 在锁外（std::sync::Mutex 绝不跨 `.await`）。
pub fn spawn(app: AppHandle, state: Arc<AppState>) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        // 冷启动让步：避开初次扫描的锁竞争，但仍尽早对账（捕获「关机期间掉线的卷」）。
        // PathProber 是零大小无状态类型,每轮在 blocking 闭包内就地构造(见 R7 下沉)。
        tokio::time::sleep(STARTUP_DELAY).await;

        loop {
            // 取写锁 → 对账 → 锁随该块结束即释放（result 仅持 Vec，不持 guard）。
            // 2026-07-06 审查 R7:对账含整卷 UPDATE(bulk_set_availability,可达百万行)且 lock()
            // 会等在途扫描批写释放——整段下沉 spawn_blocking,不占 runtime 线程(rusqlite 硬化条款)。
            let state_c = Arc::clone(&state);
            let result = tokio::task::spawn_blocking(move || match state_c.db_writer.lock() {
                Ok(conn) => run_once(&conn, &PathProber, now_unix()),
                Err(_) => {
                    tracing::warn!("volume_watch: db_writer 锁毒化，跳过本轮");
                    Ok(Vec::new())
                }
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("volume_watch: blocking 任务异常: {e}");
                Ok(Vec::new())
            });

            match result {
                Ok(changes) if !changes.is_empty() => {
                    tracing::info!(
                        "volume_watch: {} 个卷在线态变化 | {} volume(s) availability flipped",
                        changes.len(),
                        changes.len()
                    );
                    // S1：availability 随布局行下发（离线置灰徽标）→ bump 使重排取到新态。
                    state.bump_data_version();
                    // 前端据此刷新画廊（离线徽标显隐）。失败仅记日志（无监听者时属正常）。
                    let _ = app.emit(EVENT_VOLUMES_CHANGED, changes.len());
                }
                Ok(_) => {} // 无变化 → 静默
                Err(e) => tracing::warn!("volume_watch run_once 失败: {e}"),
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::collections::HashSet;

    /// 可注入的在线判定：online 集内的挂载点视为在线。
    struct MockChecker {
        online: HashSet<String>,
    }
    impl VolumeOnlineCheck for MockChecker {
        fn is_online(&self, p: &Path) -> bool {
            self.online.contains(p.to_str().unwrap_or(""))
        }
    }

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c
    }

    /// 建一个卷（指定挂载点 / 在线态）+ 一条挂在该卷上的 media（指定 availability）。
    fn seed_volume_with_item(
        c: &Connection,
        vol_id: i64,
        stable: &str,
        mount: &str,
        is_online: bool,
        item_id: i64,
        avail: &str,
    ) {
        c.execute(
            "INSERT INTO volumes (id, stable_id, label, kind, last_mount_path, is_online)
             VALUES (?1, ?2, NULL, 'removable', ?3, ?4)",
            params![vol_id, stable, mount, is_online as i64],
        )
        .unwrap();
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id, availability)
             VALUES (?1, 1, ?2, 0,0,'jpg','image',0,0,0,0, ?3, ?4)",
            params![item_id, format!("{item_id}.jpg"), vol_id, avail],
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
    fn vol_online(c: &Connection, id: i64) -> bool {
        c.query_row(
            "SELECT is_online FROM volumes WHERE id=?1",
            params![id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            != 0
    }

    /// 卷由在线变离线：media 'online'→'offline'，卷 is_online→0，记一条变化。
    #[test]
    fn online_to_offline_flips_availability() {
        let c = mem_db();
        seed_volume_with_item(&c, 1, "vol-A", "E:\\", true, 100, "online");
        let checker = MockChecker {
            online: HashSet::new(),
        }; // E:\ 不在线
        let changes = run_once(&c, &checker, 1000).unwrap();

        assert_eq!(changes.len(), 1);
        assert!(!changes[0].now_online);
        assert_eq!(avail(&c, 100), "offline");
        assert!(!vol_online(&c, 1));
    }

    /// 卷由离线变在线：media 'offline'→'online'，卷 is_online→1。
    #[test]
    fn offline_to_online_restores_availability() {
        let c = mem_db();
        seed_volume_with_item(&c, 1, "vol-A", "E:\\", false, 100, "offline");
        let checker = MockChecker {
            online: HashSet::from(["E:\\".to_string()]),
        };
        let changes = run_once(&c, &checker, 1000).unwrap();

        assert_eq!(changes.len(), 1);
        assert!(changes[0].now_online);
        assert_eq!(avail(&c, 100), "online");
        assert!(vol_online(&c, 1));
    }

    /// 状态未变（在线卷仍在线）：零变化、零写。
    #[test]
    fn no_change_when_state_matches() {
        let c = mem_db();
        seed_volume_with_item(&c, 1, "vol-A", "E:\\", true, 100, "online");
        let checker = MockChecker {
            online: HashSet::from(["E:\\".to_string()]),
        };
        let changes = run_once(&c, &checker, 1000).unwrap();
        assert!(changes.is_empty());
        assert_eq!(avail(&c, 100), "online");
    }

    /// 正交铁律：卷掉线时，'missing' 项**绝不**被改成 'offline'（只 online→offline）。
    #[test]
    fn missing_items_untouched_on_offline() {
        let c = mem_db();
        seed_volume_with_item(&c, 1, "vol-A", "E:\\", true, 100, "missing");
        // 同卷再加一个 online 项，确认它会翻而 missing 不翻。
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id, availability)
             VALUES (101, 1, '101.jpg', 0,0,'jpg','image',0,0,0,0, 1, 'online')",
            [],
        )
        .unwrap();
        let checker = MockChecker {
            online: HashSet::new(),
        };
        run_once(&c, &checker, 1000).unwrap();

        assert_eq!(avail(&c, 100), "missing", "missing 项不得被卷监听改动");
        assert_eq!(avail(&c, 101), "offline", "同卷 online 项应翻为 offline");
    }

    /// 无挂载点的卷：跳过、不动其状态（保守）。
    #[test]
    fn volume_without_mount_path_skipped() {
        let c = mem_db();
        c.execute(
            "INSERT INTO volumes (id, stable_id, label, kind, last_mount_path, is_online)
             VALUES (1, 'vol-A', NULL, 'removable', NULL, 1)",
            [],
        )
        .unwrap();
        let checker = MockChecker {
            online: HashSet::new(),
        };
        let changes = run_once(&c, &checker, 1000).unwrap();
        assert!(changes.is_empty(), "无挂载点卷应被跳过");
        assert!(vol_online(&c, 1), "其在线态不应被改动");
    }
}
