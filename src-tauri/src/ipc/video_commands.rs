//! 视频格式扩展 · 播放链路 IPC(视频格式扩展子系统 design.md §5)。
//!
//! `resolve_video_playback` 是前端播放入口的唯一后端契约:据判定表([`playback_policy`])返回
//! 六态(direct / derived / preparing / needs_component / needs_hevc_ext / needs_confirm)。
//! remux/transcode 走**按需**派生:upsert+claim `video_playable` 行 → 派 [`VideoWorkerService`]
//! (交互优先级)→ 产物 `*.tmp` 同卷 rename 落独立视频池 → finish 回 derived。进度经事件广播
//! (Channel→事件+快照的缩略图进度先例)。
//!
//! 命令壳(本文件,契约面)+ 编排内核([`crate::video::playback_orchestrator`],resolve/派生/
//! 进度私有调用链)两层结构(超长文件拆分方案 tierB-2):`#[tauri::command]` 属性、函数名、
//! `Result<T, AppError>` 错误契约原样在本文件不动,内部改为调用编排内核的函数。
//!
//! ## 硬约束落实
//! - rusqlite 全走 `spawn_blocking`(`db_read_pool` / `db_writer`);不跨 `.await` 持 std Mutex。
//! - 错误全走 [`AppError`] 稳定 code(`VideoServiceError` → `AppError::Exotic`);ffmpeg stderr /
//!   worker 内部串**不进** IPC 载荷(只进 tracing,worker_service 侧已隔离)。
//! - (item, kind) 并发去重靠 [`VideoWorkerService`] 既有 `(item, VideoKind)` 去重。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::db::queries::{get_config, reset_in_flight_derivation_for_item};
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::video::playable_cache::{pool_stat, DEFAULT_VIDEO_CACHE_MAX_MB};
use crate::video::playback_orchestrator::{
    emit_progress, progress_snapshot, resolve_impl, KIND_PLAYABLE,
};
use crate::video::worker_service::VideoKind;

/// 查询某 item 当前播放准备进度快照(§V6-12,照缩略图 `full_thumb_gen_status` 先例):
/// 有在途准备 job → `Some({percent, stage})`(来源=host 侧转发器维护的最新 watch 值);
/// job 不存在(未准备/已完成/已失败)→ `None`。前端 webview 刷新后据此恢复进度条。
#[tauri::command]
pub async fn video_playback_progress_snapshot(
    item_id: i64,
) -> Result<Option<VideoPlaybackProgressSnapshot>> {
    Ok(progress_snapshot(item_id))
}

/// 播放准备进度快照(§V6-12):`video_playback_progress_snapshot` 命令的返回契约。
/// `percent` 仅 remux/transcode 能解出时携带;`stage` 为 worker 当前阶段串。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlaybackProgressSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u32>,
    pub stage: String,
}

/// `resolve_video_playback` 返回契约(§5.1/§5.3)。`mode` 为稳定字符串枚举。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlaybackResolution {
    /// direct | derived | preparing | needsComponent | needsHevcExt | needsConfirm。
    pub mode: &'static str,
    /// 可播源绝对路径(direct=原文件;derived=派生 mp4)。前端经 convertFileSrc 转 asset URL。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// needsConfirm 时的单文件产物字节预估(前端弹确认框展示,§5.4)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_bytes: Option<u64>,
    /// 诊断用:命中的判定分支(direct/needs_hevc_ext/remux/remux_audio_transcode/transcode)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<&'static str>,
    /// needsConfirm 态回显(V7 项6):本次 needsConfirm 是否由 `force_transcode` 覆写触发——前端
    /// 据此在下次 `confirm_video_playback` 复调时决定要不要继续携带 `force_transcode`(粘性,
    /// 不因 mode 已从 needsHevcExt 变成 needsConfirm 就丢失该意图,否则会被打回 needsHevcExt)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_transcode: Option<bool>,
}

impl VideoPlaybackResolution {
    pub(crate) fn direct(src: String) -> Self {
        Self {
            mode: "direct",
            src: Some(src),
            estimate_bytes: None,
            verdict: Some("direct"),
            force_transcode: None,
        }
    }
    pub(crate) fn needs_hevc_ext(src: String) -> Self {
        Self {
            mode: "needsHevcExt",
            src: Some(src),
            estimate_bytes: None,
            verdict: Some("needs_hevc_ext"),
            force_transcode: None,
        }
    }
    pub(crate) fn needs_component() -> Self {
        Self {
            mode: "needsComponent",
            src: None,
            estimate_bytes: None,
            verdict: None,
            force_transcode: None,
        }
    }
    pub(crate) fn needs_confirm(
        estimate: u64,
        verdict: &'static str,
        force_transcode: Option<bool>,
    ) -> Self {
        Self {
            mode: "needsConfirm",
            src: None,
            estimate_bytes: Some(estimate),
            verdict: Some(verdict),
            force_transcode,
        }
    }
    pub(crate) fn preparing(verdict: &'static str) -> Self {
        Self {
            mode: "preparing",
            src: None,
            estimate_bytes: None,
            verdict: Some(verdict),
            force_transcode: None,
        }
    }
    pub(crate) fn derived(src: String, verdict: &'static str) -> Self {
        Self {
            mode: "derived",
            src: Some(src),
            estimate_bytes: None,
            verdict: Some(verdict),
            force_transcode: None,
        }
    }
}

/// 读取 `video_cache_max_mb` 设置(缺省 [`DEFAULT_VIDEO_CACHE_MAX_MB`])。调用方在 spawn_blocking 内跑。
/// 命令壳 `video_cache_stats`(本文件)与编排内核(`playback_orchestrator::resolve_derived`/
/// `spawn_playable_job`)共用,故 `pub(crate)`。
pub(crate) fn read_video_cache_max_mb(state: &AppState) -> u64 {
    let conn = match state.db_read_pool.get() {
        Ok(c) => c,
        Err(_) => return DEFAULT_VIDEO_CACHE_MAX_MB,
    };
    get_config(&conn, "video_cache_max_mb")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_VIDEO_CACHE_MAX_MB)
}

/// 解析视频播放路径(§5)。见模块头。
#[tauri::command]
pub async fn resolve_video_playback(
    app: AppHandle,
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<VideoPlaybackResolution> {
    resolve_impl(app, Arc::clone(&state), item_id, false, false, false).await
}

/// 单文件超池 50% 护栏放行后由前端复调(`confirmed=true` 跳过护栏,§5.4)。
///
/// `force_transcode`(D-444 ④「本地转码作可选」,V7 补遗):`needsHevcExt` 态下用户点「改用本地
/// 转码」时前端携带 `true`——resolve 判定仍是 `NeedsHevcExt`(判定表本身不改),但本命令在
/// resolve 层把该次结果**覆写**为 `Transcode` 并走既有 `spawn_playable_job` 产出,不裸交原文件
/// 等前端再次实测硬解。缺省(`None`)按 `false` 处理,即既有单文件护栏复调语义不变。
///
/// `force_transcode_confirmed`(V7 项6,与 `force_transcode` 配套):本命令固定以 `confirmed=true`
/// 调 [`resolve_impl`](因为它*是*那条「确认」通道),但这不该让 force_transcode 触发的 Transcode
/// 覆写连超池 50% 的护栏也一并跳过——首次点「改用本地转码」时前端传 `false`,让护栏真实判定一次;
/// 若因此落入 `needsConfirm`,前端把响应回显的 `force_transcode` 记下(粘性),二次「继续生成」时
/// 传 `true`,此时才真正放行。缺省 `false`(即普通调用形态下,force_transcode 一律先过护栏)。
#[tauri::command]
pub async fn confirm_video_playback(
    app: AppHandle,
    item_id: i64,
    state: State<'_, Arc<AppState>>,
    force_transcode: Option<bool>,
    force_transcode_confirmed: Option<bool>,
) -> Result<VideoPlaybackResolution> {
    resolve_impl(
        app,
        Arc::clone(&state),
        item_id,
        true,
        force_transcode.unwrap_or(false),
        force_transcode_confirmed.unwrap_or(false),
    )
    .await
}

/// 视频可播产物独立池占用统计(设计 §5.4「CacheStats 扩容先例」,V7 补遗):字节数/文件数/
/// 池上限(MB),供设置面板「视频转码缓存池」行从静态展示升级为实时占用。遍历缓存目录是阻塞
/// IO,走 spawn_blocking(照 `get_cache_stats` 先例)。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoCacheStats {
    pub bytes: u64,
    pub files: u64,
    pub limit_mb: u64,
}

#[tauri::command]
pub async fn video_cache_stats(state: State<'_, Arc<AppState>>) -> Result<VideoCacheStats> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let cache_dir = state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone();
        let (bytes, files) = pool_stat(&cache_dir);
        let limit_mb = read_video_cache_max_mb(&state);
        Ok(VideoCacheStats {
            bytes,
            files,
            limit_mb,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 取消在途/待产出的播放准备:硬取消 worker 侧 job(在途 kill / 在队出队,§5.4),
/// 再把 `video_playable` 行退回 pending 并广播 cancelled 事件。
#[tauri::command]
pub async fn cancel_video_playback(
    app: AppHandle,
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 先硬取消 worker 侧 job(kill 在途 ffmpeg / 出队未起跑),再回退 DB——顺序不敏感:
    // worker 侧 fan-out 走独立 waiter,不依赖 DB 状态;DB 回退保证下次 resolve 重新触发。
    if let Some(svc) = state.video_worker_service.get() {
        svc.cancel(item_id, VideoKind::Playable);
    }
    let s = Arc::clone(&state);
    // 只作废在途(status=1)行(§V6-7):已就绪(status=2)的可播产物不退——取消的是「正在准备」
    // 那一次派生,不该殃及已缓存产物。
    tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        reset_in_flight_derivation_for_item(&conn, item_id, KIND_PLAYABLE)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    emit_progress(&app, item_id, "cancelled", None, None);
    Ok(())
}

/// 触发下载 + 安装 FFmpeg 视频扩展组件(§3.1/§3.3):走既有 tools.rs 下载引擎(len+sha256 校验、
/// `*.tmp` 同卷 rename、逐文件 sha256 复核)。幂等:已就绪直接返回。
#[tauri::command]
pub async fn download_video_component(state: State<'_, Arc<AppState>>) -> Result<()> {
    let app_data = state.app_data_dir.clone();
    // ① 下载 zip 整包(异步下载引擎)。
    let zip = crate::exotic::tools::download_ffmpeg_package(&app_data)
        .await
        .map_err(map_tools_err)?;
    // ② 解压安装(阻塞 IO,spawn_blocking)。
    let app_data2 = app_data.clone();
    tokio::task::spawn_blocking(move || {
        crate::exotic::tools::install_ffmpeg_package(&app_data2, &zip)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .map_err(map_tools_err)?;
    Ok(())
}

/// `ToolsError` → `AppError`(稳定 code,前缀 `video_component_`;不泄内部串,`code()` 已稳定)。
fn map_tools_err(e: crate::exotic::tools::ToolsError) -> AppError {
    // ToolsError.code() 为 &'static str;拼稳定前缀。message 走既有 Display(中文,无路径)。
    let code: &'static str = match e.code() {
        "download" => "video_component_download",
        "open_zip" => "video_component_open_zip",
        "unsafe_entry" => "video_component_unsafe_entry",
        "missing_binary" => "video_component_missing_binary",
        "hash_mismatch" => "video_component_hash_mismatch",
        _ => "video_component_io",
    };
    AppError::Exotic {
        code,
        message: "视频扩展组件下载/安装失败,请重试".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析结果构造器 → 稳定 mode 串 + serde camelCase 形状。
    #[test]
    fn resolution_modes_serialize_stably() {
        let cases = [
            (VideoPlaybackResolution::direct("/a.mp4".into()), "direct"),
            (
                VideoPlaybackResolution::needs_hevc_ext("/a.mp4".into()),
                "needsHevcExt",
            ),
            (VideoPlaybackResolution::needs_component(), "needsComponent"),
            (
                VideoPlaybackResolution::needs_confirm(999, "transcode", None),
                "needsConfirm",
            ),
            (VideoPlaybackResolution::preparing("remux"), "preparing"),
            (
                VideoPlaybackResolution::derived("/c.mp4".into(), "remux"),
                "derived",
            ),
        ];
        for (r, expect) in cases {
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(v["mode"], expect);
        }
        // needsConfirm 带 estimateBytes(camelCase)。
        let v = serde_json::to_value(VideoPlaybackResolution::needs_confirm(
            999,
            "transcode",
            None,
        ))
        .unwrap();
        assert_eq!(v["estimateBytes"], 999);
        // needsComponent 不带 src(skip_serializing_if)。
        let v = serde_json::to_value(VideoPlaybackResolution::needs_component()).unwrap();
        assert!(v.get("src").is_none());
    }

    /// map_tools_err:各 ToolsError code → 稳定 video_component_* 前缀,message 无内部串。
    #[test]
    fn tools_err_maps_to_stable_codes() {
        use crate::exotic::tools::ToolsError;
        let e = map_tools_err(ToolsError::MissingBinary);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "video_component_missing_binary");
        assert_eq!(v["message"], "视频扩展组件下载/安装失败,请重试");
        let e = map_tools_err(ToolsError::HashMismatch("x".into()));
        assert_eq!(
            serde_json::to_value(&e).unwrap()["code"],
            "video_component_hash_mismatch"
        );
    }
}
