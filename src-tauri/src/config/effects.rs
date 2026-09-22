//! 一批设置变更的运行时影响:按**最终配置**合并应用,同一批里每种动作最多执行一次。
//!
//! # 为什么要「批」而不是逐键
//! 逐键应用会重复触发同一件事:一次提交里同时改缩略图尺寸与编码质量,逐键处理就是两轮
//! 「复位全部已完成缩略图 + 失效布局缓存 + bump 数据版本」;而重置会一次改动几十个键,
//! 逐键更是灾难。故入口只接受一批差异,内部先归约成若干**动作**(全量缩略图复位、直接显示项复位、
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
    let mut full_reset_keys: Vec<String> = Vec::new();
    let mut direct_reset_keys: Vec<String> = Vec::new();
    let mut cache_dir: Option<(String, std::path::PathBuf)> = None;
    let mut log_level: Option<(String, String)> = None;
    let mut material: Option<(String, String)> = None;
    let mut derivation_touched: Option<String> = None;

    // 缩略图运行时配置镜像:一次性取锁写完,避免同批内反复加解锁。
    {
        let mut cfg = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        for (key, value) in changed {
            match key.as_str() {
                "thumb_skip_max_kb" => {
                    if let Ok(val) = value.parse::<u64>() {
                        cfg.skip_max_bytes = val * 1024;
                        direct_reset_keys.push(key.clone());
                    }
                }
                "thumb_size" => {
                    if let Ok(val) = value.parse::<u32>() {
                        cfg.size = val;
                        // 档位失效(2026-07-06 审查 R1):生成器 CACHE_HIT 只看 status=1+文件存在、
                        // 不含档位,不重置则存量图永远端出旧档缓存,设置项对存量库形同虚设。
                        full_reset_keys.push(key.clone());
                    }
                }
                "thumb_webp_quality" => {
                    if let Ok(val) = value.parse::<u8>() {
                        cfg.webp_quality = val.clamp(1, 100); // 1..=99 有损,100=无损
                        full_reset_keys.push(key.clone());
                    }
                }
                "thumb_strategy" => {
                    cfg.strategy = value.clone();
                    direct_reset_keys.push(key.clone());
                }
                "gpu_engine" => cfg.gpu_engine = value.clone(),
                "ai_hq_cache_enabled" => cfg.ai_hq_cache = value == "true",
                "ai_cache_short_edge" => {
                    // 同 thumb_webp_quality 惯例,运行时即时生效:下一次 AI 高清缓存路径的解码/
                    // 编码即用新短边。不触发存量缓存失效——旧短边缓存仍是合法缓存,分析侧本就按需
                    // 下采样,不会因短边变化而读出错误结果。
                    if let Ok(val) = value.parse::<u32>() {
                        cfg.ai_cache_short_edge = val;
                    }
                }
                "thumb_cache_dir" => {
                    let path = std::path::PathBuf::from(value);
                    cfg.cache_dir = path.clone();
                    cache_dir = Some((key.clone(), path));
                }
                _ => {}
            }
        }
    }

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

    // 动作 1:全量复位已生成的缩略图(档位/质量变更),每批至多一次。
    if !full_reset_keys.is_empty() {
        match reset_completed_thumbnail_rows(state).await {
            Ok((items, covers)) => {
                tracing::info!(
                    "[Config] 缩略图档位/质量变更 → 复位 {items} 个已完成项与 {covers} 个封面派生待重生成"
                );
                invalidate_layout_caches(state);
            }
            Err(e) => {
                tracing::warn!("缩略图全量复位失败(设置已保存) | full thumbnail reset failed: {e}");
                failed.append(&mut full_reset_keys.clone());
            }
        }
    }

    // 动作 2:复位「直接显示」项(跳过阈值/策略变更),每批至多一次。二者可同时命中。
    if !direct_reset_keys.is_empty() {
        match reset_direct_thumbnail_rows(state).await {
            Ok(affected) => {
                tracing::info!(
                    "[Config] 缩略图跳过阈值/策略变更 → 复位 {affected} 个直接显示项为待处理"
                );
                *state
                    .layout_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = None;
            }
            Err(e) => {
                tracing::warn!(
                    "缩略图直接显示项复位失败(设置已保存) | direct thumbnail reset failed: {e}"
                );
                failed.append(&mut direct_reset_keys.clone());
            }
        }
    }

    // 动作 3:缓存目录换址(建目录 + 授权 assetProtocol 作用域)。
    if let Some((key, path)) = cache_dir {
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

    // 动作 4:日志级别热重载。
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

    // 动作 5:窗口材质热切换(前端 uiStore 经同一事件翻转 data-glass,两侧同拍)。
    if let Some((key, value)) = material {
        // apply 内部已把失败按「排入主线程队列失败」记了 warn,并自带「窗口不存在则跳过」语义
        // (辅助窗口场景),故此处不再重复判错,也不把它算作应用失败。
        crate::window_material::apply(app, &value);
        let _ = key;
    }

    // 动作 6:按最终配置重启派生流水线(每批至多一次,故放在所有镜像更新之后)。
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

/// 复位已生成的缩略图:撤销在途 worker,并让复位与旧结果的最终写入共享同一 lifecycle 写区。
/// 封面派生行必须一并复位——只退 media_items 的话,video/audio/epub 封面会被主生成器打成
/// UNSUPPORTED_TYPE,而派生行仍 done 不重跑(2026-07-10 深审 defer ⑧)。
async fn reset_completed_thumbnail_rows(state: &Arc<AppState>) -> Result<(usize, usize)> {
    let state_arc = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_thumbnail_reset_exclusive(|| -> Result<(usize, usize)> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tx = conn.unchecked_transaction()?;
            let items = tx.execute(
                "UPDATE media_items SET thumb_status = 0, thumb_path = NULL \
                 WHERE thumb_status = 1 AND is_deleted = 0",
                [],
            )?;
            let covers = tx.execute(
                "UPDATE media_derivations SET status = 0, payload_path = NULL \
                 WHERE status = 2 AND kind IN ('video_cover','audio_cover','doc_thumb')",
                [],
            )?;
            tx.commit()?;
            Ok((items, covers))
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 跳过阈值/策略改变只需复位 direct-display 项,但同样必须撤销旧 worker,避免旧结果在新配置
/// 生效后重新写回。
async fn reset_direct_thumbnail_rows(state: &Arc<AppState>) -> Result<usize> {
    let state_arc = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        state_arc.with_thumbnail_reset_exclusive(|| -> Result<usize> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Ok(conn.execute(
                "UPDATE media_items SET thumb_status = 0, thumb_path = NULL, thumbhash = NULL \
                 WHERE thumb_status = 3 AND is_deleted = 0",
                [],
            )?)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}
