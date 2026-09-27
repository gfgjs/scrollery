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
            let result = tokio::task::spawn_blocking(move || -> Result<Vec<VolumeChange>> {
                let Some(epoch) = state_c.current_database_epoch() else {
                    return Ok(Vec::new());
                };
                let (changes, volumes) = {
                    let conn = match state_c.db_writer.lock() {
                        Ok(conn) => conn,
                        Err(_) => {
                            tracing::warn!("volume_watch: db_writer 锁毒化，跳过本轮");
                            return Ok(Vec::new());
                        }
                    };
                    (
                        run_once(&conn, &PathProber, now_unix())?,
                        q::list_volumes(&conn)?,
                    )
                };
                // 设备查询可能等待系统 IO，不能持数据库锁或进入缩略图逐项热路径。
                let devices = volumes
                    .into_iter()
                    .filter(|volume| {
                        volume.is_online
                            && matches!(
                                volume.kind,
                                crate::db::models::VolumeKind::Local
                                    | crate::db::models::VolumeKind::Removable
                            )
                    })
                    .filter_map(|volume| {
                        super::hdd_io::storage_device(
                            volume.last_mount_path.as_deref()?,
                            Some(&volume.stable_id),
                        )
                        .map(|device| (volume.id, device))
                    })
                    .collect();
                state_c.with_database_lifecycle_read(epoch, || {
                    state_c
                        .background_volume_io_budget
                        .publish_devices(epoch, devices);
                });
                Ok(changes)
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
