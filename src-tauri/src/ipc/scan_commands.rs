//! 用于扫描管理的 Tauri IPC 命令（§ 6.1 — 扫描管理）。

use std::sync::Arc;

use serde::Serialize;

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::info;

use crate::db::models::{NewVolume, ScanRoot};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::scanner::fast_scan::{run_fast_scan_with_generation_and_state, ScanChannelPayload};
use crate::scanner::volume_probe::{PlatformVolumeResolver, VolumeResolver};
use crate::state::AppState;
use crate::utils::path::normalize_root_path;

const CODE_SCAN_DATABASE_CLEARING: &str = "scan_database_clearing";

fn scan_database_clearing() -> AppError {
    AppError::Scan {
        code: CODE_SCAN_DATABASE_CLEARING,
        message: "数据库正在清理，请稍后重试 | Database is being cleared; retry shortly".into(),
    }
}

fn require_scan_lifecycle<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(scan_database_clearing)
}

/// 添加新的扫描根目录。
#[tauri::command]
pub async fn add_scan_root(
    app: AppHandle,
    path: String,
    alias: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<ScanRoot> {
    // span 埋点(W1,D-312 info 档:任务型低频、await 覆盖 DB+卷探测真实工作)。
    let _span = crate::logging::SpanTimer::info("ipc:add_scan_root");
    // 8A：用 root 专用规范化（保留 UNC `//server/share` 前缀），使网络盘/挂载盘可作扫描根（§3.8）。
    let norm = normalize_root_path(&path);
    let norm_for_scope = norm.clone();

    // R1-3：查重（读池）→ 落库（写锁）→ 卷探测（原生 syscall）全是同步阻塞，整段下沉 blocking；
    // 读写混排 + 中途 early-return，不拆套 read/write_blocking 助手（见 blocking.rs 头注释豁免口）。
    let state_arc = Arc::clone(&*state);
    let root = tokio::task::spawn_blocking(move || {
        state_arc.with_scan_lifecycle_read(|| -> Result<ScanRoot> {
            // 检查根目录是否已存在
            {
                let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
                let roots = q::list_scan_roots(&pool)?;
                if let Some(existing) = roots.into_iter().find(|r| r.path == norm) {
                    info!(
                        "Scan root already exists: id={} path={} | 扫描根目录已存在: id={} path={}",
                        existing.id, norm, existing.id, norm
                    );
                    return Ok(existing);
                }
            }

            // 为新根解析并登记卷，绑定 volume_id（C5 Piece2）：缺失检测守门1 的「在线卷集」依赖它；
            // 未绑卷 → 该根媒体 volume_id 恒 NULL → 缺失检测对此根休眠（见 C5 Piece1）。
            // Windows 用原生卷 GUID（抗盘符重映射），失败/非 Windows 回退路径派生（C5 Piece A）。
            // 该原生探测在 writer 锁外完成，遵守「文件/平台 IO 不持有 SQLite writer」约束。
            let resolved = PlatformVolumeResolver.resolve(&norm);
            let new_vol = NewVolume {
                stable_id: resolved.stable_id,
                label: None,
                kind: resolved.kind,
                last_mount_path: Some(resolved.mount_path),
                last_seen: None,
                is_online: true, // 刚添加 = 在场
            };

            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let id = q::insert_scan_root(&conn, &norm, alias.as_deref())?;
            {
                let volume_id = q::upsert_volume(&conn, &new_vol)?;
                q::set_scan_root_volume(&conn, id, Some(volume_id))?;
            }

            // 立即创建顶级目录记录，以便前端可以立刻加载和选中
            let dir_name = alias.clone().unwrap_or_else(|| {
                std::path::Path::new(&norm)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string()
            });
            q::upsert_directory(&conn, id, None, "", &dir_name, 0, None)?;

            let root = q::get_scan_root(&conn, id)?;
            info!("Scan root added: id={id} path={norm} | 已添加扫描根目录: id={id} path={norm}");
            // S1：新根/顶级目录已入库（目录映射与成员变化）→ bump。
            state_arc.bump_data_version();
            Ok(root)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .ok_or_else(scan_database_clearing)??;

    // Grant asset-protocol read access to this root so its images load via convertFileSrc
    // (the static config no longer opens whole drives). See E1 in docs/archive/perf_hardening_plan_v2.md.
    // 落库成功后才授 asset 协议读权限(审查 F-05 同款时序:授权面=进程生命周期,失败路径
    // 不得留下不属于媒体库的 WebView 可读面)。已存在根的 early-return 也走到这——重复授权幂等。
    // 失败只告警不阻断:仅影响图片显示,根已成立。
    if let Err(e) = app
        .asset_protocol_scope()
        .allow_directory(&norm_for_scope, true)
    {
        tracing::warn!(
            "Failed to allow scan root {} in asset scope | 扫描根授权失败: {}",
            norm_for_scope,
            e
        );
    }

    Ok(root)
}

/// 移除扫描根目录及其所有数据 (CASCADE)。
#[tauri::command]
pub async fn remove_scan_root(id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    // span 埋点(W1,D-312 info 档:级联删除,await 覆盖真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:remove_scan_root");
    info!(
        "User action: Removing scan root ID: {} | 用户操作：正在移除扫描根目录 ID: {}",
        id, id
    );
    // 取消、级联删除和派生状态失效必须在同一根级 gate 内完成；否则 restart 可能在
    // 「已取消但尚未删库」的窗口安装新轮，随后旧根删除会与新轮并发。
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_scan_root_exclusive(id, || -> Result<()> {
            state_arc.cancel_scan_under_root_gate(id);
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            q::delete_scan_root(&conn, id)?;
            // S1：整根级联删除（成员变化）→ bump。
            state_arc.bump_data_version();
            // 树快照随根一并丢弃（R-12）。死快照不可达（scan_roots id AUTOINCREMENT 不复用 +
            // get_scan_root 先拦），纯内存卫生——至多 20 万条目滞留至 LRU 逐出。
            state_arc.tree_snapshots.invalidate_root(id);
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .ok_or_else(scan_database_clearing)??;
    info!("Scan root removed: id={id} | 已移除扫描根目录: id={id}");
    Ok(())
}

/// 设置扫描根的显隐（V21，设置页库级显隐）。隐藏后该根媒体从画廊「全部」/时间轴/搜索/统计/
/// 侧栏文件树/全选全部排除，取消即恢复。
#[tauri::command]
pub async fn set_scan_root_hidden(
    id: i64,
    hidden: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    info!(
        "User action: set scan root {} hidden={} | 用户操作：设置扫描根 {} 显隐 hidden={}",
        id, hidden, id, hidden
    );
    // 仅改一行标志（短写），但必须与 clear_database 共享扫描生命周期读闸门。
    // 否则 clear 可能删库后再收到旧 hidden 写入，产生幽灵 data_version/wake 副作用。
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_scan_lifecycle_read(|| -> Result<()> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            q::set_scan_root_hidden(&conn, id, hidden)?;
            // S1 失效契约：显隐改变了可见集合，必须与该写入处于同一生命周期读区。
            state_arc.bump_data_version();
            if !hidden {
                // 取消隐藏：隐藏期被 claim 排除的 exotic 任务仍停 pending，踢一次醒来重领。
                state_arc.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .ok_or_else(scan_database_clearing)??;
    Ok(())
}

/// 带选项删除扫描根的结果
#[derive(Serialize)]
pub struct RemoveRootResult {
    /// 计划清理的缩略图文件数
    pub cleared_count: usize,
}

/// 带缩略图清理选项删除扫描根。
#[tauri::command]
pub async fn remove_scan_root_with_options(
    state: State<'_, Arc<AppState>>,
    id: i64,
    clear_thumbnails: bool,
) -> Result<RemoveRootResult> {
    // span 埋点(W1,D-312 info 档:抽样查询+级联删除,await 覆盖真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:remove_scan_root_with_options");
    // 查询待清理缩略图、取消扫描、级联删除和缩略图文件删除共享扫描生命周期写锁，
    // 避免旧 cache key 的文件删除与同一目录立即重建的缩略图撞车。文件 IO 仍在 DB
    // writer 锁释放后执行。
    let state_arc = Arc::clone(&*state);
    let cleared_count = tokio::task::spawn_blocking(move || {
        let cache_dir = state_arc
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone();
        state_arc.with_scan_root_and_lifecycle_exclusive(id, || -> Result<usize> {
            state_arc.cancel_scan_under_root_gate(id);
            let cache_keys = if clear_thumbnails {
                let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
                let mut stmt = pool.prepare(
                    "SELECT DISTINCT m.cache_key
                     FROM media_items m
                     JOIN directories d ON m.directory_id = d.id
                     WHERE d.root_id = ?1 AND m.thumb_status = 1
                       AND NOT EXISTS (
                           SELECT 1
                           FROM media_items shared
                           JOIN directories shared_d ON shared.directory_id = shared_d.id
                           WHERE shared.cache_key = m.cache_key
                             AND shared_d.root_id <> ?1
                       )",
                )?;
                let keys = stmt
                    .query_map([id], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                keys
            } else {
                vec![]
            };
            {
                let conn = state_arc
                    .db_writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                q::delete_scan_root(&conn, id)?;
            }
            state_arc.bump_data_version();
            state_arc.tree_snapshots.invalidate_root(id);

            // DB writer 已释放；生命周期写锁仍阻止 add/start/relink 在清理完成前重建同一
            // cache key。查询阶段已排除仍被其它根引用的 key，删除失败只代表文件本来已
            // 不存在或外部占用，不影响根删除结果。
            let mut deleted = 0usize;
            if clear_thumbnails {
                for key in &cache_keys {
                    for size in crate::thumbnail::generator::THUMB_TIERS {
                        let full = crate::thumbnail::cache::thumb_path(&cache_dir, size, *key);
                        if std::fs::remove_file(&full).is_ok() {
                            deleted += 1;
                        }
                    }
                }
                tracing::info!(
                    "Cleaned {} thumbnails for root id={} | 为根 id={} 清理了 {} 个缩略图",
                    deleted,
                    id,
                    id,
                    deleted
                );
            }
            Ok(deleted)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    Ok(RemoveRootResult { cleared_count })
}

/// relink 抽样校验的样本数上限。
///
/// 100 个足以把「随便挑了个不相干目录」按下去（错树几乎必然大面积 miss），
/// 而 100 次 `fs::metadata` 在冷盘上也就百毫秒级——用户显式动作，付得起。
const RELINK_SAMPLE_SIZE: i64 = 100;

/// relink 放行所需的最低命中率（百分比）的回退默认值(config.toml 缺省时用;真实生效值以
/// `config::schema::SETTING_DEFS` 的 `relink_match_threshold_pct` 为准,批次C接线)。
///
/// 不取 100%：整根迁移后用户很可能已顺手改过几张图（mtime 变），或有同步盘占位落地
/// 造成的 mtime 抖动（见 `UpsertOutcome::SuspectChanged` 的注释）。留 5% 容差既能吸收
/// 这些正常抖动，又远严于「错树」的命中率（错树通常个位数百分比甚至 0）。
const DEFAULT_RELINK_MATCH_THRESHOLD_PCT: u32 = 95;

/// 判定抽样结果是否放行。
///
/// 抽样为 0（空根）时放行：没有任何证据能证伪，而空根 relink 本就无害（无派生产物可废）。
fn relink_sample_passes(matched: usize, sampled: usize, threshold_pct: u32) -> bool {
    if sampled == 0 {
        return true;
    }
    (matched as u64) * 100 >= (sampled as u64) * (threshold_pct as u64)
}

/// 比对单个抽样项：新根下的实际文件是否与 DB 记录的体征一致。
///
/// **size + mtime 双比**，故意严于 `upsert_fast_scan_item` 的 Unchanged 判据（那里只比 mtime，
/// scan.rs 内）。两者职责不同：upsert 问「这个文件变了吗」，mtime 够用；relink 问「这是同一棵
/// 树吗」，要的是尽可能强的指纹——而 size 在同一次 `fs::metadata` 里是白送的，双比零额外 IO。
fn relink_item_matches(new_root: &str, item: &q::RootSampleItem) -> bool {
    let abs = crate::utils::path::resolve_media_path(new_root, &item.rel_path, &item.file_name);
    let Ok(meta) = std::fs::metadata(&abs) else {
        return false; // 文件不在新路径下 → 不匹配（缺失也是一种证据）
    };
    if meta.len() as i64 != item.file_size {
        return false;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    mtime == item.file_mtime
}

/// 为根路径重叠判断提供规范化值和组件边界斜杠。
///
/// 根路径不能走相对路径的裁剪规则：/ 和 C:/ 都必须保留其根标记，且规范化后与
/// add_scan_root/relink_scan_root 使用同一把尺子，避免尾斜杠把同一路径误判为重叠。
fn normalize_root_for_overlap(path: &str) -> (String, String) {
    let normalized = normalize_root_path(path);
    let with_sep = if normalized.ends_with('/') {
        normalized.clone()
    } else {
        format!("{normalized}/")
    };
    (normalized, with_sep)
}

/// 重链接扫描根到新路径的结果。
#[derive(Serialize)]
pub struct RelinkResult {
    /// 更新后的扫描根（前端据此刷新列表）。
    pub root: ScanRoot,
    /// 实际抽样数（可能小于 `RELINK_SAMPLE_SIZE`——根内项数不足时）。
    pub sampled: usize,
    /// 抽样中体征一致的数量。
    pub matched: usize,
}

/// 文件夹整体迁移后（如 D 盘 → C 盘），把扫描根重链接到新路径。
///
/// #7 方案A。**不重扫、不重生成缩略图**：缩略图 cache_key = xxh3(rel_path/file_name|mtime)
/// 不含盘符（`utils::hash`），DB 里目录/文件也全是相对根的——整根迁移后这些全部不变，
/// 唯一失真的就是 `scan_roots.path` 这一行（外加卷绑定要重探）。
///
/// 抽样校验是**安全闸**：随机取 N 项在新路径下 stat 比对 size+mtime，命中率不足即拒绝，
/// 避免用户选错目录后把整个根的身份指向一棵无关的树（那会让所有派生产物张冠李戴）。
///
/// 兜底重扫由**前端**在本命令成功后补发 `start_scan`：`start_scan` 签名要一个
/// `Channel<ScanChannelPayload>` 进度通道，只有前端造得出，后端无法自调。
#[tauri::command]
pub async fn relink_scan_root(
    app: AppHandle,
    root_id: i64,
    new_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<RelinkResult> {
    // span 埋点(W1,D-312 info 档:抽样 stat + 事务落库,await 覆盖真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:relink_scan_root");
    // 与 add_scan_root 同一把规范化尺子（保留 UNC 前缀），否则存库形态会与既有根不一致。
    let norm = normalize_root_path(&new_path);
    let norm_for_scope = norm.clone();

    let state_arc = Arc::clone(&*state);
    let result = tokio::task::spawn_blocking(move || -> Result<RelinkResult> {
        // 重链接与活动扫描共享同一根级闸门：先取消当前轮，再把抽样、路径切换和卷
        // 绑定放在独占区内。旧轮若已在文件 IO 中，会在下一次 generation 写入处被拒绝；
        // 新轮只能在路径切换完成后安装，不能看到半旧半新的 root。
        require_scan_lifecycle(state_arc.with_scan_root_exclusive(root_id, || -> Result<RelinkResult> {
            state_arc.cancel_scan_under_root_gate(root_id);

            // ① 新路径必须真实存在且是目录——先于一切校验，给最直白的错误。
            if !std::path::Path::new(&norm).is_dir() {
                return Err(AppError::Relink {
                    code: "relink_not_a_dir",
                    message: "target path is not an existing directory".into(),
                });
            }

        // ② 查重 + 抽样（读池，不占写锁——抽样含 N 次文件 stat，绝不能拿着写锁做 IO）。
        let sample = {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            // 目标根必须存在（ScanRootNotFound 由 get_scan_root 抛出），也顺带确认 root_id 合法。
            q::get_scan_root(&pool, root_id)?;
            // path 有 UNIQUE：别的根已占该路径时先给可读错误，而非让 UPDATE 撞出裸 Db 错。
            if let Some(other) = q::list_scan_roots(&pool)?
                .into_iter()
                .find(|r| r.path == norm && r.id != root_id)
            {
                tracing::info!(
                    "Relink rejected: path already used by root id={} | 重链接被拒：路径已被根 id={} 占用",
                    other.id,
                    other.id
                );
                return Err(AppError::Relink {
                    code: "relink_path_taken",
                    message: format!("path already bound to scan root id={}", other.id),
                });
            }
            q::sample_root_items(&pool, root_id, RELINK_SAMPLE_SIZE)?
        };

        // ③ 抽样比对（纯文件 IO，读池连接已归还）。
        // 批次C:低频调用点(用户手动触发的一次性 IPC),直接经 ConfigManager 每次读一次(内存读零 IO)。
        let threshold_pct: u32 = state_arc
            .config
            .get("relink_match_threshold_pct")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_RELINK_MATCH_THRESHOLD_PCT);
        let sampled = sample.len();
        let matched = sample.iter().filter(|it| relink_item_matches(&norm, it)).count();
        if !relink_sample_passes(matched, sampled, threshold_pct) {
            tracing::info!(
                "Relink rejected: sample mismatch {}/{} | 重链接被拒：抽样命中 {}/{}",
                matched,
                sampled,
                matched,
                sampled
            );
            return Err(AppError::Relink {
                code: "relink_mismatch",
                // 只带统计数：绝对路径不进 message（泄漏面），前端自己有用户刚选的路径。
                message: format!("sample match {matched}/{sampled} below threshold"),
            });
        }

        // ④ 落库：改根路径 + 重探卷绑定（同 add_scan_root:74-86）。
        // 迁移很可能换了物理卷（D→C 即是），卷绑定必须跟着重探：不重绑则 volume_id 仍指向
        // 旧卷，旧卷一旦离线，缺失检测会把这批本地文件全判成 missing（C5 Piece1）。
        // 卷探测是原生 syscall——在写锁外先行,不拿着写锁做 IO。
        let resolved = PlatformVolumeResolver.resolve(&norm);
        let new_vol = NewVolume {
            stable_id: resolved.stable_id,
            label: None,
            kind: resolved.kind,
            last_mount_path: Some(resolved.mount_path),
            last_seen: None,
            is_online: true, // 刚校验过文件可 stat = 在场
        };

        // 单事务原子落库(审查 F-03):旧实现 autocommit 逐条提交,upsert_volume /
        // set_scan_root_volume 任一失败时根路径已永久改掉、错误却返回给前端(前后端认知
        // 分裂),且失败路径跳过下方缓存失效。事务中途失败整体回滚,根保持迁移前状态。
        let mut conn = state_arc
            .db_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(AppError::Db)?;
        q::update_scan_root_path(&tx, root_id, &norm)?;
        let volume_id = q::upsert_volume(&tx, &new_vol)?;
        q::set_scan_root_volume(&tx, root_id, Some(volume_id))?;
        let root = q::get_scan_root(&tx, root_id)?;
        tx.commit().map_err(AppError::Db)?;
        drop(conn);

        info!(
            "Scan root relinked: id={root_id} sample={matched}/{sampled} | \
             已重链接扫描根: id={root_id} 抽样命中={matched}/{sampled}"
        );
        // 根路径变了 → 目录映射的绝对路径全变 → 树快照与数据版本都须失效。
        state_arc.bump_data_version();
        state_arc.tree_snapshots.invalidate_root(root_id);

            Ok(RelinkResult {
                root,
                sampled,
                matched,
            })
        }))?
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // 校验+落库全部成功后才授 asset 协议读权限(审查 F-05):授权面=进程生命周期,先授后验
    // 会让 relink_not_a_dir / relink_path_taken / relink_mismatch 的拒绝路径留下不属于媒体库
    // 的 WebView 可读面。授权失败只告警不阻断(同 add_scan_root:38 姿态):仅影响图片显示,
    // 重链接本身已成立。旧根路径的既有授权不撤销:asset scope 无逐条撤销 API,且多根可能共享
    // 父目录前缀,误撤会连坐其它根;进程重启后按当前根重授自然收敛(lib.rs setup)。
    if let Err(e) = app
        .asset_protocol_scope()
        .allow_directory(&norm_for_scope, true)
    {
        tracing::warn!(
            "Failed to allow relinked root {} in asset scope | 重链接根授权失败: {}",
            norm_for_scope,
            e
        );
    }

    Ok(result)
}

/// 重叠扫描根的信息
#[derive(Serialize, Clone)]
pub struct OverlapInfo {
    pub id: i64,
    pub path: String,
    pub alias: Option<String>,
}

/// 文件夹重叠检查结果
#[derive(Serialize)]
pub struct FolderOverlapResult {
    /// 新路径包含的已有根（新路径是父级）
    pub children: Vec<OverlapInfo>,
    /// 包含新路径的已有根（新路径是子级）
    pub parents: Vec<OverlapInfo>,
}

/// 检查新文件夹路径是否与现有扫描根重叠。
#[tauri::command]
pub async fn check_folder_overlap(
    state: State<'_, Arc<AppState>>,
    new_path: String,
) -> Result<FolderOverlapResult> {
    // 与 add/relink_scan_root 使用同一根路径规范化尺子，再按组件边界补斜杠比较。
    let (normalized, normalized_with_sep) = normalize_root_for_overlap(&new_path);

    // R1-3：读池查询走 blocking；同时与 clear_database 共享生命周期读闸门，避免返回
    // 清库前的根快照。后面的重叠判定是小数据纯 CPU，留在 async 侧。
    let state_arc = Arc::clone(&*state);
    let roots: Vec<(i64, String, Option<String>)> = tokio::task::spawn_blocking(move || {
        state_arc.with_scan_lifecycle_read(|| -> Result<Vec<(i64, String, Option<String>)>> {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            let mut stmt =
                pool.prepare("SELECT id, path, alias FROM scan_roots WHERE is_active = 1")?;
            let roots = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(roots)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .ok_or_else(scan_database_clearing)??;

    let mut children = vec![];
    let mut parents = vec![];

    for (id, path, alias) in &roots {
        let (root_normalized, root_with_sep) = normalize_root_for_overlap(path);

        if root_with_sep.starts_with(&normalized_with_sep) && root_normalized != normalized {
            // 已有根是新路径的子级（新路径是父级）
            children.push(OverlapInfo {
                id: *id,
                path: path.clone(),
                alias: alias.clone(),
            });
        } else if normalized_with_sep.starts_with(&root_with_sep) && root_normalized != normalized {
            // 已有根是新路径的父级（新路径是子级）
            parents.push(OverlapInfo {
                id: *id,
                path: path.clone(),
                alias: alias.clone(),
            });
        }
    }

    Ok(FolderOverlapResult { children, parents })
}

/// 列出所有扫描根目录。
#[tauri::command]
pub async fn list_scan_roots(state: State<'_, Arc<AppState>>) -> Result<Vec<ScanRoot>> {
    // 与 clear_database 共享生命周期读闸门，避免返回清库前的旧快照。
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_scan_lifecycle_read(|| {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            q::list_scan_roots(&pool)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .ok_or_else(scan_database_clearing)?
}

/// 启动根目录扫描（包括快速扫描和后台内容丰富）。
///
/// 此命令在快速扫描完成时返回（UI 准备就绪）。
/// 后台内容丰富继续进行并发出 Tauri 事件。
// Tauri 命令参数与前端 invoke 字段一一对应，塞进 struct 会改 IPC 形状（且需前端同步改）；
// 故沿用本仓库既有约定（如 ai_commands.rs）直接 allow。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn start_scan(
    root_id: i64,
    run_id: String,
    on_progress: Channel<ScanChannelPayload>,
    group_by: Option<String>,
    sort_within_group: Option<String>,
    sort_order: Option<String>,
    // T17b：opt-in「快速扫描」。None/false → 全量逐文件（默认）；true → mtime 未变目录跳过 per-file
    // 工作（仅漏就地编辑，全量扫描兜底）。前端增量重扫时传 true 提速。
    quick: Option<bool>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let pipeline_started = std::time::Instant::now();
    // 该 span 必须随后台 enrichment 一起存活；start_scan 返回只代表快扫完成，
    // 不能让默认的 IPC span 把后续元数据解析排除在整条流水线之外。
    let pipeline_span =
        crate::logging::SpanTimer::info("scan:pipeline").with_operation_id(Some(run_id.clone()));
    // span 埋点(W1,D-312 info 档:命令自身 await 覆盖 fast scan 主链,后台 enrichment 是
    // fire-and-forget、不计入本 span)。
    let _span = crate::logging::SpanTimer::info("ipc:start_scan");
    info!(
        "User action: Starting scan for root ID: {} run_id={} | 用户操作：开始扫描根目录 ID: {} run_id={}",
        root_id, run_id, root_id, run_id
    );
    // 在同一锁区取消旧轮并安装新轮，避免旧收尾插入 stop→restart 的空窗；同时捕获数据库
    // 生命周期 epoch，清库开始后这次启动不得继续向旧库写回。
    let start_epoch = state
        .current_database_epoch()
        .ok_or_else(scan_database_clearing)?;
    let (generation, cancel) = state
        .try_replace_scan_run_token_with_id(root_id, Some(&run_id))
        .ok_or_else(scan_database_clearing)?;

    // 用于首屏优先级排序的视图顺序（默认值与 UI 默认一致）。
    let group_by = group_by.unwrap_or_else(|| "date".to_string());
    let sort_within_group = sort_within_group.unwrap_or_else(|| "datetime".to_string());
    let sort_order = sort_order.unwrap_or_else(|| "desc".to_string());
    let quick = quick.unwrap_or(false);

    // 获取根目录路径（R1-3：读池查询走 blocking），读查询也必须绑定本次 epoch，不能
    // 在清库切换期间取得一份旧库路径后启动扫描。
    let state_for_root = Arc::clone(&*state);
    let root_path_result = tokio::task::spawn_blocking(move || {
        state_for_root.with_database_lifecycle_read(start_epoch, || {
            let pool = state_for_root.db_read_pool.get().map_err(AppError::from)?;
            Ok(q::get_scan_root(&pool, root_id)?.path)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    let root_path = match root_path_result {
        Some(Ok(path)) => path,
        Some(Err(e)) => {
            // 根路径读取失败时也只清理本轮 token，避免留下永久的 scan blocker。
            state.cancel_scan_generation(root_id, generation);
            return Err(e);
        }
        None => {
            state.cancel_scan_generation(root_id, generation);
            return Err(scan_database_clearing());
        }
    };

    if !state.is_database_epoch_current(start_epoch) {
        state.cancel_scan_generation(root_id, generation);
        return Err(scan_database_clearing());
    }

    info!("start_scan: root_id={root_id} path={root_path} | 开始扫描: root_id={root_id} path={root_path}");

    // 克隆 Arc 以便闭包拥有独立的引用（不需要 unsafe）
    let state_arc = Arc::clone(&*state);
    let cancel_fast = cancel.clone();
    let run_id_fast = run_id.clone();
    let root_path_clone = root_path.clone();
    let generation_fast = generation;
    // 取当前 exotic Catalog 快照，移入扫描闭包（walker 据此 common-first 分类、seed 任务）。
    let catalog_snap = state_arc.exotic_catalog.snapshot();
    // 同一视图顺序也驱动后台 enrichment 的补全顺序。
    let (group_by_e, sort_within_e, sort_order_e) = (
        group_by.clone(),
        sort_within_group.clone(),
        sort_order.clone(),
    );

    // 运行快速扫描（使用 spawn_blocking，因此我们不会阻塞异步运行时）
    let fast_scan_started = std::time::Instant::now();
    let fast_scan_result = tokio::task::spawn_blocking(move || {
        // T12：fast_scan 改流式入库、视图序让渡布局层，故不再需要 group_by/sort_* 入参；
        // 同名参数仍单独驱动后台 enrichment 的补全顺序（见上方 `*_e` 克隆）。
        run_fast_scan_with_generation_and_state(
            &state_arc,
            root_id,
            &run_id_fast,
            &root_path_clone,
            &catalog_snap,
            &on_progress,
            &cancel_fast,
            // S1：每批入库提交后 bump——扫描进行中前端逐批重排，items 取数缓存须逐批失效。
            &|| state_arc.bump_data_version(),
            quick,
            generation_fast,
        )
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    let fast_scan_elapsed_ms = fast_scan_started.elapsed().as_millis() as u64;

    if let Err(e) = fast_scan_result {
        // 快速扫描阶段还没进入 enrichment 的统一收尾区，必须在这里清理 token；
        // 但只能清理本轮，不能在 stop→restart 窗口取消新轮。
        let _ = state.cancel_scan_generation(root_id, generation);
        let total_elapsed_ms = pipeline_started.elapsed().as_millis() as u64;
        tracing::warn!(
            "Scan pipeline stopped during fast scan: root_id={root_id} run_id={run_id} fast_scan_ms={fast_scan_elapsed_ms} total_ms={total_elapsed_ms} error={e} | 扫描+解析流水线在快速扫描阶段终止"
        );
        return Err(e);
    }

    // 快扫返回后的缓存失效、树快照失效和 exotic 唤醒也必须与 generation 检查同处根级
    // 闸门；否则新轮可插入在检查之后，旧轮仍会刷新全局/根级状态或启动旧富化。
    if state
        .with_scan_generation_write(root_id, generation, || {
            state.bump_data_version();
            state.tree_snapshots.invalidate_root(root_id);
            state.wake_exotic(crate::exotic::coordinator::WakeReason::ScanCommitted);
        })
        .is_none()
    {
        info!(
            "Ignoring stale fast-scan completion: root_id={root_id} generation={generation} | 忽略过时代次的快速扫描收尾"
        );
        return Ok(());
    }

    info!(
        "Scan pipeline stage complete: root_id={root_id} run_id={run_id} stage=fast_scan elapsed_ms={fast_scan_elapsed_ms} total_elapsed_ms={} | 扫描+解析阶段完成: 快速扫描耗时={fast_scan_elapsed_ms}ms",
        pipeline_started.elapsed().as_millis()
    );

    // Spawn background enrichment (fire-and-forget, emits events)
    // After enrichment completes, remove the scan token so the AI pipeline
    // can start without waiting forever (was the root cause of infinite yielding).
    // 生成后台内容丰富任务（触发后不管，发出事件）
    // Enrichment 完成后移除 scan token，AI pipeline 才能正常启动（否则会无限让步）。
    {
        let state_arc2 = Arc::clone(&*state);
        let app_clone = app.clone();
        let cancel_enrich = cancel.clone();
        let run_id_enrich = run_id.clone();
        let generation_enrich = generation;
        let pipeline_started_enrich = pipeline_started;
        let pipeline_span_enrich = pipeline_span;
        tokio::task::spawn_blocking(move || {
            // 持有到本闭包结束，span_close 才覆盖「快扫 + 后台富化」全周期。
            let _pipeline_span = pipeline_span_enrich;
            if !state_arc2.is_scan_generation_current(root_id, generation_enrich) {
                tracing::info!(
                    "Skipping stale scan enrichment: root_id={root_id} generation={generation_enrich} | 跳过过时代次的扫描富化"
                );
                return;
            }
            let enrichment_started = std::time::Instant::now();
            let enrichment_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::scanner::enricher::run_enrichment_with_generation(
                    &app_clone,
                    &state_arc2,
                    root_id,
                    &run_id_enrich,
                    &group_by_e,
                    &sort_within_e,
                    &sort_order_e,
                    &cancel_enrich,
                    generation_enrich,
                )
            }));

            let (outcome, terminal_payload) = match enrichment_result {
                // 正常路径的完成事件由 run_enrichment 自身发出；token 仍须由本轮收尾 claim。
                Ok(Ok(())) => ("completed", None),
                Ok(Err(e)) => {
                    // 取消属正常终态（用户主动停止），不算失败 → 仍发「正常完成」让 UI 停转圈；
                    // 真错误才带 error_code，前端据此弹 warning（区分「取消」与「失败」）。
                    let is_cancel = matches!(e, AppError::Cancelled);
                    if is_cancel {
                        tracing::info!(
                            "Enrichment cancelled for root_id={root_id} | 增量补全已取消"
                        );
                    } else {
                        tracing::error!("Enrichment error for root_id={root_id}: {e}");
                    }
                    // On cancel/error, run_enrichment doesn't emit its completion event.
                    // Emit a terminal signal anyway so the frontend progress UI stops.
                    // 取消/出错时 run_enrichment 不会发出完成事件，这里补发终止信号，
                    // 以便前端进度 UI 能够停止。
                    let payload = if is_cancel {
                        crate::scanner::enricher::EnrichmentCompletedPayload::ok(
                            root_id,
                            &run_id_enrich,
                            0,
                        )
                    } else {
                        crate::scanner::enricher::EnrichmentCompletedPayload::failed(
                            root_id,
                            &run_id_enrich,
                            "enrich_failed",
                        )
                    };
                    if is_cancel {
                        ("cancelled", Some(payload))
                    } else {
                        ("failed", Some(payload))
                    }
                }
                Err(_) => {
                    tracing::error!("Enrichment panicked for root_id={root_id} | 增量补全崩溃");
                    (
                        "panicked",
                        Some(
                            crate::scanner::enricher::EnrichmentCompletedPayload::failed(
                                root_id,
                                &run_id_enrich,
                                "enrich_panicked",
                            ),
                        ),
                    )
                }
            };
            let enrichment_elapsed_ms = enrichment_started.elapsed().as_millis() as u64;
            let total_elapsed_ms = pipeline_started_enrich.elapsed().as_millis() as u64;
            let orchestration_ms = total_elapsed_ms
                .saturating_sub(fast_scan_elapsed_ms)
                .saturating_sub(enrichment_elapsed_ms);

            // generation compare-and-clear 必须先于外层终态事件；旧轮失去 claim 后不再
            // 发布取消/失败状态，也不清理新轮 token。
            if !state_arc2.finish_scan_with_action(root_id, generation_enrich, || {
                if let Some(payload) = terminal_payload {
                    let _ = app_clone.emit("enrichment:completed", payload);
                }
            }) {
                tracing::info!(
                    "Stale scan enrichment finish ignored: root_id={root_id} generation={generation_enrich} | 忽略过时代次的扫描富化收尾"
                );
                return;
            }
            info!(
                "Scan pipeline complete: root_id={root_id} run_id={run_id_enrich} outcome={outcome} fast_scan_ms={fast_scan_elapsed_ms} enrichment_ms={enrichment_elapsed_ms} orchestration_ms={orchestration_ms} total_ms={total_elapsed_ms} | 扫描+解析流水线完成: 结果={outcome} 快速扫描={fast_scan_elapsed_ms}ms 元数据解析={enrichment_elapsed_ms}ms 编排={orchestration_ms}ms 总耗时={total_elapsed_ms}ms"
            );
            tracing::info!(
                "Scan token cleared for root_id={root_id} generation={generation_enrich} | 已清除扫描 token: root_id={root_id} generation={generation_enrich}"
            );
        });
    }

    Ok(())
}

/// 停止（取消）正在进行的扫描。
#[tauri::command]
pub async fn stop_scan(
    root_id: i64,
    run_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    info!(
        "User action: Stopping scan for root ID: {} run_id={} | 用户操作：停止扫描根目录 ID: {} run_id={}",
        root_id, run_id, root_id, run_id
    );
    let cancelled = state.cancel_scan_run(root_id, &run_id);
    info!(
        "stop_scan: root_id={root_id} run_id={run_id} cancelled={cancelled} | 停止扫描: root_id={root_id} run_id={run_id} cancelled={cancelled}"
    );
    Ok(())
}

#[tauri::command]
pub async fn clear_database(state: State<'_, Arc<AppState>>, app: AppHandle) -> Result<()> {
    // span 埋点(W1,D-312 info 档:全表 DELETE+VACUUM+缩略图目录递归删除,await 覆盖重负载)。
    let _span = crate::logging::SpanTimer::info("ipc:clear_database");
    info!("User action: Clearing database | 用户操作：正在清除数据库");

    let cache_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
        .join("cache")
        .join("thumbnails");

    // R1-3：全表 DELETE + VACUUM（可达秒级）+ 缩略图目录递归删除（可达数万文件）全是重阻塞，
    // 整段下沉 blocking；事务需要 `&mut Connection`，write_blocking 只给 `&`，故手写 spawn_blocking。
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        // 取消、删库、删缩略图和内存失效必须由同一生命周期写锁覆盖；否则新的
        // start_scan 可在取消与删库之间重新安装并向已清空的库写入。
        state_arc.with_all_scans_exclusive(|| -> Result<()> {
            state_arc.with_dedup_lifecycle_write(|| -> Result<()> {
                // 去重 worker 也可能持有一个迟到的读块；先撤销它的数据库代次，
                // 避免全库清空完成后旧任务重新写 sidecar。
                state_arc.dedup_task.stop().map_err(|error| {
                    AppError::internal("内部任务失败 | internal task failed", error)
                })?;
                state_arc.cancel_all_scans_under_exclusive();

                // 擦除所有数据库表
                {
                    let mut conn = state_arc
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let tx = conn.transaction()?;
                    // dedup_index 随 media CASCADE；这里显式删除也让全库清空语义不依赖外键配置。
                    // persons 无 media FK 不随 CASCADE 清(2026-07-10 审查 F7):库清空后名册若不清,
                    // 全部人物以旧计数挂墙成幽灵。faces 随 media_items CASCADE,故 persons 后删安全。
                    tx.execute_batch(
                        "DELETE FROM dedup_index;
                         DELETE FROM image_meta;
                         DELETE FROM media_items;
                         DELETE FROM persons;
                         DELETE FROM directories;
                         DELETE FROM scan_roots;",
                    )?;
                    tx.commit()?;

                    // VACUUM 必须在事务外运行
                    conn.execute("VACUUM", [])?;
                }

                // 删除缩略图缓存目录（此时 DB writer 锁已释放）。
                if cache_dir.exists() {
                    std::fs::remove_dir_all(&cache_dir).map_err(AppError::Io)?;
                }

                // 重置内存布局缓存与 S1 items 取数缓存，并 bump 数据版本。
                state_arc.clear_layout_caches();
                state_arc.dedup_folder_stats_cache.clear();
                state_arc.bump_data_version();
                state_arc.tree_snapshots.clear(); // 树快照同批清（R-12：根已全删，全部为死快照）
                Ok(())
            })
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    info!("clear_database: all media data wiped | 清除数据库：所有媒体数据已擦除");
    Ok(())
}

#[cfg(test)]
mod relink_tests {
    use super::*;
    use std::io::Write;

    /// 造一个真实文件并返回其 DB 侧应有的体征（size + 真实 mtime）。
    /// mtime 必须**从磁盘读回**而非自造：本函数的被测对象就是「DB 记录 vs 磁盘实况」的比对，
    /// 自造 mtime 等于把待测的两侧都由测试给定，测不出真东西。
    fn write_sample(
        root: &std::path::Path,
        rel_path: &str,
        name: &str,
        body: &[u8],
    ) -> q::RootSampleItem {
        let dir = if rel_path.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel_path)
        };
        std::fs::create_dir_all(&dir).unwrap();
        let full = dir.join(name);
        let mut f = std::fs::File::create(&full).unwrap();
        f.write_all(body).unwrap();
        drop(f);

        let meta = std::fs::metadata(&full).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        q::RootSampleItem {
            rel_path: rel_path.to_string(),
            file_name: name.to_string(),
            file_size: body.len() as i64,
            file_mtime: mtime,
        }
    }

    /// 放行判据：空样本放行（空根无从证伪且 relink 无害）；边界值 95/100 恰好放行；94/100 拒。
    #[test]
    fn sample_passes_respects_threshold_and_empty_root() {
        assert!(relink_sample_passes(0, 0, 95), "空根须放行");
        assert!(relink_sample_passes(95, 100, 95), "恰好达阈值须放行");
        assert!(!relink_sample_passes(94, 100, 95), "低于阈值须拒");
        assert!(relink_sample_passes(100, 100, 95));
        assert!(!relink_sample_passes(0, 100, 95), "全 miss（错树）须拒");
        // 小样本：3 项根，95% 意味着一个都不能错。
        assert!(relink_sample_passes(3, 3, 95));
        assert!(!relink_sample_passes(2, 3, 95));
    }

    #[test]
    fn overlap_normalization_preserves_absolute_root_markers() {
        let (posix_root, posix_boundary) = normalize_root_for_overlap("/");
        assert_eq!(posix_root, "/");
        assert_eq!(posix_boundary, "/");

        let (drive_root, drive_boundary) = normalize_root_for_overlap("C:\\");
        assert_eq!(drive_root, "C:/");
        assert_eq!(drive_boundary, "C:/");
    }

    /// 迁移语义主张：同一棵树整体搬到新根下 → 逐项体征一致 → 命中。
    /// 这正是 #7 免重扫的地基（rel_path/file_name/size/mtime 全部与根路径无关）。
    #[test]
    fn item_matches_after_whole_tree_move() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();

        // 顶级目录（rel_path 为空串）与嵌套子目录各一，覆盖 resolve_media_path 的两个分支。
        let top = write_sample(old.path(), "", "a.jpg", b"hello");
        let nested = write_sample(old.path(), "sub/deep", "b.jpg", b"world!!");

        // 在新根下重建同样的相对结构 = 模拟整根迁移。
        let top_new = write_sample(new.path(), "", "a.jpg", b"hello");
        let nested_new = write_sample(new.path(), "sub/deep", "b.jpg", b"world!!");

        let new_root = new.path().to_string_lossy().replace('\\', "/");
        // 用「旧根记录的体征」比对新根实况：size 必然一致；mtime 由新建文件决定，
        // 故这里用新根实测 mtime 覆盖，等价于迁移工具保留了 mtime（robocopy /COPY:DAT 等）。
        let moved_top = q::RootSampleItem {
            file_mtime: top_new.file_mtime,
            ..top.clone()
        };
        let moved_nested = q::RootSampleItem {
            file_mtime: nested_new.file_mtime,
            ..nested.clone()
        };
        assert!(relink_item_matches(&new_root, &moved_top), "顶级项须命中");
        assert!(
            relink_item_matches(&new_root, &moved_nested),
            "嵌套项须命中"
        );
    }

    /// 三种不匹配都必须判 false —— 这是「选错目录」的安全闸赖以成立的前提。
    #[test]
    fn item_mismatch_on_missing_size_or_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().replace('\\', "/");
        let good = write_sample(dir.path(), "", "a.jpg", b"hello");
        assert!(relink_item_matches(&root, &good), "对照：一致须命中");

        // ① 文件不存在（错树最常见形态）
        let missing = q::RootSampleItem {
            file_name: "nope.jpg".into(),
            ..good.clone()
        };
        assert!(!relink_item_matches(&root, &missing), "缺失须判不匹配");

        // ② size 不同（同名不同内容 —— mtime 单比会漏掉这种）
        let wrong_size = q::RootSampleItem {
            file_size: good.file_size + 1,
            ..good.clone()
        };
        assert!(
            !relink_item_matches(&root, &wrong_size),
            "size 不符须判不匹配"
        );

        // ③ mtime 不同
        let wrong_mtime = q::RootSampleItem {
            file_mtime: good.file_mtime + 3600,
            ..good.clone()
        };
        assert!(
            !relink_item_matches(&root, &wrong_mtime),
            "mtime 不符须判不匹配"
        );
    }
}
