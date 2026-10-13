//! 一批设置变更的运行时影响:按**最终配置**合并应用,同一批里每种动作最多执行一次。
//!
//! # 为什么要「批」而不是逐键
//! 逐键应用会重复触发同一件事:一次提交里同时改缩略图尺寸与编码质量,逐键处理就是两轮
//! 「复位展示引用 + 失效布局缓存 + bump 数据版本」;而重置会一次改动几十个键,
//! 逐键更是灾难。故入口只接受一批差异,内部先归约成若干**动作**(图片/封面展示引用复位、
//! 缓存目录换址、日志级别重载、窗口材质切换),每个动作至多执行一次。
//!
//! # 与应用失败的关系
//! 返回值是「已落盘但运行时影响应用失败」的键。这类结果既不能报成完全成功,也不能报成保存失败,
//! 故由上层放进 SettingsChange.apply_failed 明确告知(方案 §5.4)。

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tracing_subscriber::{reload::Handle, EnvFilter, Registry};

use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::generator::ThumbConfig;

/// 日志级别热重载句柄(logging 模块在装配 subscriber 时经 config_commands 的再导出写入)。
pub static LOG_RELOAD: std::sync::OnceLock<Handle<EnvFilter, Registry>> =
    std::sync::OnceLock::new();

/// 本批设置变更的来源。唯一影响的是**派生流水线是否需要被重启**(见 `restart_derivation`):
/// 用户提交与外部编辑要保持既有功能(新参数要作用到存量待处理项),而恢复默认设置不得把用户
/// 已经停止或暂停的任务重新拉起来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOrigin {
    /// 用户提交或外部文件编辑:与既有行为一致。
    Commit,
    /// 恢复默认设置:保留任务启停意图。
    Reset,
}

/// 改这些键会改变派生流水线的输入参数或产出集合,故需要按最终配置重启一次流水线
/// (沿用既有清单:前端旧实现即在改动这些键后重启派生,后端接管后语义不变)。
const DERIVATION_RESTART_KEYS: &[&str] = &[
    "thumb_size",
    "thumb_webp_quality",
    "enable_video_cover",
    "enable_video_keyframes",
    "ai_hq_cache_enabled",
];

#[derive(Default)]
struct ThumbnailConfigChanges {
    image_reset_keys: Vec<String>,
    cover_reset_keys: Vec<String>,
    cache_dir: Option<(String, std::path::PathBuf)>,
}

// 先合并最终镜像，再用产物身份的同一策略比较决定失效，尺寸不改变普通图片策略。
fn update_thumbnail_config(
    cfg: &mut ThumbConfig,
    changed: &[(String, String)],
) -> ThumbnailConfigChanges {
    let before = cfg.clone();
    let mut effects = ThumbnailConfigChanges::default();
    for (key, value) in changed {
        match key.as_str() {
            "thumb_skip_max_kb" => {
                if let Ok(val) = value.parse::<u64>() {
                    cfg.skip_max_bytes = val * 1024;
                    effects.image_reset_keys.push(key.clone());
                }
            }
            "thumb_size" => {
                if let Ok(val) = value.parse::<u32>() {
                    cfg.size = val;
                    effects.cover_reset_keys.push(key.clone());
                }
            }
            "thumb_webp_quality" => {
                if let Ok(val) = value.parse::<u8>() {
                    cfg.webp_quality = val.clamp(1, 100);
                    effects.image_reset_keys.push(key.clone());
                    effects.cover_reset_keys.push(key.clone());
                }
            }
            "thumb_strategy" => {
                cfg.strategy = value.clone();
                effects.image_reset_keys.push(key.clone());
            }
            "gpu_engine" => {
                cfg.gpu_engine = value.clone();
                effects.image_reset_keys.push(key.clone());
            }
            "ai_hq_cache_enabled" => {
                cfg.ai_hq_cache = value == "true";
                effects.image_reset_keys.push(key.clone());
            }
            "ai_cache_short_edge" => {
                if let Ok(val) = value.parse::<u32>() {
                    cfg.ai_cache_short_edge = val;
                    effects.image_reset_keys.push(key.clone());
                }
            }
            "thumb_cache_dir" => {
                let path = std::path::PathBuf::from(value);
                cfg.cache_dir = path.clone();
                effects.cache_dir = Some((key.clone(), path));
                effects.image_reset_keys.push(key.clone());
            }
            _ => {}
        }
    }
    if cfg.same_policy(&before) {
        effects.image_reset_keys.clear();
    }
    if cfg.size == before.size && cfg.webp_quality == before.webp_quality {
        effects.cover_reset_keys.clear();
    }
    effects
}

/// 应用一批已提交的变更;`changed` 只含**真正变了**的键与它们的最终值(规范文本)。
///
/// 返回值:值已落盘、但运行时影响没能应用上的键(空 = 全部应用成功)。
pub async fn apply_batch(
    app: &AppHandle,
    state: &Arc<AppState>,
    changed: &[(String, String)],
    origin: ApplyOrigin,
) -> Vec<String> {
    let mut failed: Vec<String> = Vec::new();

    // 本批触发的动作与触发它们的键(用于失败时回报具体是哪些键没应用上)。
    let mut log_level: Option<(String, String)> = None;
    let mut material: Option<(String, String)> = None;
    let mut derivation_touched: Option<String> = None;

    // 缩略图运行时配置镜像:一次性取锁写完,避免同批内反复加解锁。
    let thumbnail_changes = {
        let mut cfg = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        update_thumbnail_config(&mut cfg, changed)
    };

    for (key, value) in changed {
        match key.as_str() {
            "log_level" => log_level = Some((key.clone(), value.clone())),
            "window_material" => material = Some((key.clone(), value.clone())),
            _ => {}
        }
        if DERIVATION_RESTART_KEYS.contains(&key.as_str()) {
            // 同一批里改多项只安排一次重启:这里只记「谁触发的」,执行在下方统一做。
            derivation_touched.get_or_insert_with(|| key.clone());
        }
    }

    // 动作 1:按最终图片策略和封面参数，一次事务撤销展示引用；历史 artifact 保留。
    let reset_images = !thumbnail_changes.image_reset_keys.is_empty();
    let reset_covers = !thumbnail_changes.cover_reset_keys.is_empty();
    if reset_images || reset_covers {
        match reset_thumbnail_rows(state, reset_images, reset_covers).await {
            Ok((items, covers)) => {
                tracing::info!(
                    "[Config] 缩略图配置变更 → 复位 {items} 个展示引用与 {covers} 个封面派生待重生成"
                );
                invalidate_layout_caches(state);
            }
            Err(e) => {
                tracing::warn!("缩略图展示复位失败(设置已保存) | thumbnail reset failed: {e}");
                for key in thumbnail_changes
                    .image_reset_keys
                    .iter()
                    .chain(&thumbnail_changes.cover_reset_keys)
                {
                    if !failed.contains(key) {
                        failed.push(key.clone());
                    }
                }
            }
        }
    }

    // 动作 2:缓存目录换址(建目录 + 授权 assetProtocol 作用域)。
    if let Some((key, path)) = thumbnail_changes.cache_dir {
        // 建目录是阻塞 IO,下沉 spawn_blocking(硬约束:IO 不占 UI 线程)。
        let dir = path.clone();
        let prepared = tokio::task::spawn_blocking(move || std::fs::create_dir_all(&dir)).await;
        match prepared {
            Ok(Ok(())) => {
                if let Err(e) = app.asset_protocol_scope().allow_directory(&path, true) {
                    tracing::warn!(
                        "更新后的缓存目录授权失败(设置已保存) | allowing new cache dir in asset scope failed: {e}"
                    );
                    failed.push(key);
                }
            }
            Ok(Err(e)) => {
                // 目录建不出来 = 新缓存目录用不上:值已保存,但如实报告未生效(不吞错、不谎报成功)。
                tracing::warn!("缓存目录无法创建(设置已保存) | creating cache dir failed: {e}");
                failed.push(key);
            }
            Err(e) => {
                tracing::warn!("缓存目录准备任务失败(设置已保存) | cache dir task failed: {e}");
                failed.push(key);
            }
        }
    }

    // 动作 3:日志级别热重载。
    if let Some((key, value)) = log_level {
        let directive = crate::logging::build_env_filter_directive(&value);
        let mut ok = false;
        if let Some(handle) = LOG_RELOAD.get() {
            if let Ok(filter) = EnvFilter::try_new(&directive) {
                match handle.modify(|f| *f = filter) {
                    Ok(()) => {
                        ok = true;
                        tracing::info!("日志级别已热更新为 {value}");
                    }
                    Err(e) => tracing::warn!("日志级别重载失败 | log level reload failed: {e}"),
                }
            } else {
                tracing::warn!("日志级别指令非法,未应用 | invalid log directive: {directive}");
            }
        } else {
            // 还没装上 reload 句柄(极早启动阶段):值已落盘,下次启动按新级别装配。
            ok = true;
        }
        if !ok {
            failed.push(key);
        }
    }

    // 动作 4:窗口材质热切换(前端 uiStore 经同一事件翻转 data-glass,两侧同拍)。
    if let Some((key, value)) = material {
        // apply 内部已把失败按「排入主线程队列失败」记了 warn,并自带「窗口不存在则跳过」语义
        // (辅助窗口场景),故此处不再重复判错,也不把它算作应用失败。
        crate::window_material::apply(app, &value);
        let _ = key;
    }

    // 动作 5:按最终配置重启派生流水线(每批至多一次,故放在所有镜像更新之后)。
    if let Some(key) = derivation_touched {
        if let Err(e) = restart_derivation(app, state, origin).await {
            tracing::warn!("派生流水线重启失败(设置已保存) | restarting derivation failed: {e}");
            failed.push(key);
        }
    }

    failed
}

/// 按最终配置重启派生流水线:取消当前运行并按最新参数重新开始,续传待处理/被中断项
/// (与前端既有的「改动相关设置后重启派生」行为一致,不重置任何已完成行)。
///
/// **重置来源要保留任务启停意图**:恢复默认设置会把 enable_video_cover / enable_video_keyframes
/// 等键改回默认值,但那不等于「用户想让任务跑起来」。故 Reset 来源下只在流水线原本就处于「期望
/// 运行」状态(derivation_active=1,即用户启动过且未停止)时才重启;已停止/已暂停的任务保持停止,
/// 参数改动会在用户下次显式启动时自然生效。
async fn restart_derivation(
    app: &AppHandle,
    state: &Arc<AppState>,
    origin: ApplyOrigin,
) -> Result<()> {
    let state_arc = Arc::clone(state);
    let expected_running = tokio::task::spawn_blocking(move || {
        let conn = state_arc
            .db_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        matches!(
            crate::db::queries::get_config(&conn, "derivation_active"),
            Ok(Some(v)) if v == "1"
        )
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;

    if origin == ApplyOrigin::Reset && !expected_running {
        tracing::info!("[Config] 恢复默认设置:派生流水线未处于期望运行状态,保持停止不自动启动");
        return Ok(());
    }

    let app = app.clone();
    let state_arc = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        crate::ipc::derive_commands::restart_derivation_after_config_change(&app, &state_arc);
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    Ok(())
}

/// 失效布局与 S1 取数缓存并 bump 数据版本(缩略图复位后视图必须重算)。
fn invalidate_layout_caches(state: &Arc<AppState>) {
    state.clear_layout_caches();
    state.bump_data_version();
}

/// 撤销在途 worker，并让展示引用复位与旧结果最终写入共享同一 lifecycle 写区。
async fn reset_thumbnail_rows(
    state: &Arc<AppState>,
    reset_images: bool,
    reset_covers: bool,
) -> Result<(usize, usize)> {
    let state_arc = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_thumbnail_reset_exclusive(|| -> Result<(usize, usize)> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            crate::db::queries::reset_thumbnail_config_references(&conn, reset_images, reset_covers)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[cfg(test)]
mod thumbnail_invalidation_tests {
    use crate::db::queries::reset_thumbnail_config_references;
    use rusqlite::Connection;

    fn seeded() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots(id,path) VALUES(1,'C:/photos');
             INSERT INTO directories(id,root_id,rel_path,name) VALUES(1,1,'','photos');
             INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,
                 file_format,media_type,sort_datetime,cache_key,thumb_status,thumb_path,thumbhash,is_deleted) VALUES
                 (1,1,'a.jpg',1,1,'jpg','image',1,11,1,'display',x'07',0),
                 (2,1,'b.jpg',1,1,'jpg','image',1,12,3,'original',x'07',0),
                 (3,1,'c.jpg',1,1,'jpg','image',1,13,2,NULL,NULL,0),
                 (4,1,'d.jpg',1,1,'jpg','image',1,14,1,'deleted',x'07',1),
                 (10,1,'v.mp4',1,1,'mp4','video',1,20,1,'video',x'07',0),
                 (11,1,'a.mp3',1,1,'mp3','audio',1,21,1,'audio',x'07',0),
                 (12,1,'d.pdf',1,1,'pdf','document',1,22,1,'document',x'07',0),
                 (13,1,'e.psd',1,1,'psd','image',1,23,1,'exotic',x'07',0),
                 (14,1,'f.raw',1,1,'raw','image',1,24,1,'catalog',x'07',0);
             INSERT INTO media_derivations(item_id,kind,status,payload_path) VALUES
                 (10,'video_cover',2,'video'),(11,'audio_cover',2,'audio'),(12,'doc_thumb',2,'document');
             INSERT INTO exotic_tasks(item_id,plugin_id,capability,status) VALUES(13,'plugin','thumbnail',2);
             INSERT INTO exotic_catalog_formats(format,plugin_id,display_name,media_kind,
                 capabilities_json,license_tier,platforms_json,min_host_version,catalog_sequence,source)
             VALUES('raw','plugin','RAW','image','[\"thumbnail\"]','free','[]','1',1,'builtin');
             INSERT INTO image_artifacts(item_id,source_revision,cache_key,kind,fingerprint,status,path) VALUES
                 (1,1,11,'display','policy:64',1,'display'),
                 (1,1,11,'display','policy:128',1,'large'),
                 (1,1,11,'display','old:64',1,'old'),
                 (1,1,11,'ai','ai:336',1,'ai_thumbs/input');",
        )
        .unwrap();
        conn
    }

    type DisplayReference = (i64, i64, Option<String>, Option<Vec<u8>>);

    fn references(conn: &Connection) -> Vec<DisplayReference> {
        conn.prepare("SELECT id,thumb_status,thumb_path,thumbhash FROM media_items ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn artifacts(conn: &Connection) -> Vec<(String, String, String)> {
        conn.prepare("SELECT kind,fingerprint,path FROM image_artifacts ORDER BY kind,fingerprint")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn cover_reset_failure_rolls_back_image_references() {
        let conn = seeded();
        let old_refs = references(&conn);
        let old_artifacts = artifacts(&conn);
        conn.execute_batch(
            "CREATE TRIGGER fail_cover BEFORE UPDATE ON media_derivations
             BEGIN SELECT RAISE(ABORT,'forced'); END;",
        )
        .unwrap();
        assert!(reset_thumbnail_config_references(&conn, true, true).is_err());
        assert_eq!(references(&conn), old_refs);
        assert_eq!(artifacts(&conn), old_artifacts);
    }
}
