// src-tauri/src/video/playback_orchestrator.rs
//! 视频格式扩展 · 播放链路编排内核(design.md §5)。从 `ipc/video_commands.rs` 拆出的
//! 私有调用链(超长文件拆分方案 tierB-2):`resolve_impl` 判定 → `resolve_derived` 分流 →
//! `spawn_playable_job` 后台产出 → 事件/快照广播。命令壳(`#[tauri::command]`)与 DTO
//! 契约原样留在 `ipc/video_commands.rs`,只经 `pub(crate)` 接口调用本模块。
//!
//! ## 硬约束落实
//! - rusqlite 全走 `spawn_blocking`;不跨 `.await` 持 std Mutex。
//! - `PREPARING_OUTPUTS`/`PLAYBACK_PROGRESS` 两个 static 是命令壳(`cancel_video_playback`/
//!   `video_playback_progress_snapshot`)与本模块的唯一跨文件耦合点——只经下方 `pub(crate)`
//!   读写函数暴露,static 本身不出模块(拆分方案 tierB-2 §③最大风险)。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;

use crate::db::models::VideoMeta;
use crate::db::queries::{
    batch_finish_derivations, get_derivation_state, get_media_detail, upsert_and_claim_derivation,
    upsert_video_meta,
};
use crate::error::{AppError, Result};
use crate::ipc::video_commands::{
    read_video_cache_max_mb, VideoPlaybackProgressSnapshot, VideoPlaybackResolution,
};
use crate::state::AppState;
use crate::video::playable_cache::{
    enforce_pool_limit, estimate_output_bytes, exceeds_half_pool, playable_path, playable_tmp_path,
    pool_dir, touch_played,
};
use crate::video::playback_policy::{
    decide, is_direct_container, is_direct_video_codec, normalize_codec, selected_audio_index,
    AudioTrackFacts, PlaybackVerdict, PolicyInput,
};
use crate::video::worker_service::{VideoProgressSnapshot, VideoServiceError, VideoWorkerService};
use exotic_protocol::VideoProbeInfo;

/// 播放产物派生 kind(§5.2:remux 与 transcode 同 kind)。命令壳(`cancel_video_playback`)
/// 也要用同一个 kind 字符串,故 `pub(crate)`。
pub(crate) const KIND_PLAYABLE: &str = "video_playable";
/// 转码编码器阶梯(LGPL 构建无 libx264,§3.2):厂商硬编 → Media Foundation 软件回退。
const ENCODER_LADDER: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf", "h264_mf"];
/// 转码 CRF(质量优先,视觉近无损区间上沿)。
const TRANSCODE_CRF: u8 = 23;
/// 播放链路进度/状态事件名(前端订阅;缩略图进度事件先例)。
const VIDEO_PLAYBACK_PROGRESS_EVENT: &str = "video:playback_progress";

/// 在产/在播产物路径登记(§5.4 in_use 真实接线,§V6-4):`spawn_playable_job` rename 前登记本次
/// 产物终名、终态(ready/error/rename 失败)摘除。传给 [`enforce_pool_limit`] 使 LRU 驱逐**跳过**
/// 本次刚产出的产物——防并发 resolve(为别的 item 产出)的驱逐轮删掉正在交付的新文件。
/// 注:这只是「同进程内驱逐避让」的软提示;真正拦住「正在被 WebView2 播放的文件被删」的是
/// Windows 删占用文件本就失败(`enforce_pool_limit` 删失败即 `continue` 跳过,下轮再试),此登记
/// 集合是那层平台兜底之上的补充,不替代它。
static PREPARING_OUTPUTS: LazyLock<StdMutex<HashSet<PathBuf>>> =
    LazyLock::new(|| StdMutex::new(HashSet::new()));

/// per-item 最近播放准备进度快照(§5.3 快照命令 §V6-12):`spawn_progress_forwarder` 每收到一帧
/// 即写入(不受事件节流影响,始终是最新已知值)、job 终结(forwarder 因发送端 drop 退出)即摘除。
/// `video_playback_progress_snapshot` 命令读它——有 = 正在准备的 percent/stage,无 = job 不存在(None)。
static PLAYBACK_PROGRESS: LazyLock<StdMutex<HashMap<i64, VideoPlaybackProgressSnapshot>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn register_in_use(path: &Path) {
    PREPARING_OUTPUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf());
}
fn unregister_in_use(path: &Path) {
    PREPARING_OUTPUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
}
fn in_use_snapshot() -> HashSet<PathBuf> {
    PREPARING_OUTPUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
fn set_progress_snapshot(item_id: i64, snap: VideoPlaybackProgressSnapshot) {
    PLAYBACK_PROGRESS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(item_id, snap);
}
fn clear_progress_snapshot(item_id: i64) {
    PLAYBACK_PROGRESS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&item_id);
}
/// 命令壳 `video_playback_progress_snapshot` 的唯一读接口(见模块头 §跨文件耦合)。
pub(crate) fn progress_snapshot(item_id: i64) -> Option<VideoPlaybackProgressSnapshot> {
    PLAYBACK_PROGRESS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&item_id)
        .cloned()
}

/// 进度/状态事件载荷(前端据 `itemId` 匹配在准备中的播放)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackProgressPayload {
    item_id: i64,
    /// preparing | ready | error | cancelled。
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    /// error 时的稳定 code(无内部串)。
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    /// ffmpeg 细粒度进度百分比(§5.3,仅 preparing 态且已知时携带;remux/transcode 专属,
    /// probe/frames 不产出)。
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<u32>,
    /// worker 侧当前阶段(如 `remux`/`transcode`/`finalize`),`percent` 有值时同携带诊断用。
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
}

/// 判定分支 → 稳定诊断字符串。
fn verdict_label(v: &PlaybackVerdict) -> &'static str {
    match v {
        PlaybackVerdict::DirectPlay => "direct",
        PlaybackVerdict::NeedsHevcExt => "needs_hevc_ext",
        PlaybackVerdict::Remux => "remux",
        PlaybackVerdict::RemuxAudioTranscode { .. } => "remux_audio_transcode",
        PlaybackVerdict::Transcode => "transcode",
    }
}

/// `VideoProbeInfo`(流事实)→ 判定输入(§5.1)。`container_ext` 用**文件扩展名**(非 ffprobe
/// format_name),故由 host 侧传入而非取 `probe.container`。
fn probe_to_policy(container_ext: &str, probe: &VideoProbeInfo) -> PolicyInput {
    PolicyInput {
        container_ext: container_ext.to_string(),
        video_codec: probe.video_codec.clone(),
        video_profile: probe.video_profile.clone(),
        bit_depth: probe.bit_depth,
        audio_tracks: probe
            .audio_tracks
            .iter()
            .map(|t| AudioTrackFacts {
                index: t.index,
                codec: t.codec.clone(),
                is_default: t.is_default,
            })
            .collect(),
    }
}

/// 合并探测字段与既有 `video_meta`(§V6-6):探测某字段为 `None` 时**保留 DB 既有值**,避免
/// probe 回填把已知 rotation(D-003 红线:旋转双钉勿清零)/ fps / bitrate 覆写为 0/NULL。
/// 返回 `(fps, bitrate, rotation)`;探测有值(含 rotation=0)即以探测为准覆盖既有。
fn merge_probe_meta_fields(
    existing: Option<&VideoMeta>,
    probe: &VideoProbeInfo,
) -> (Option<f64>, Option<i64>, i64) {
    let fps = probe
        .fps
        .map(|f| f as f64)
        .or_else(|| existing.and_then(|m| m.fps));
    let bitrate = probe
        .bitrate
        .map(|b| b as i64)
        .or_else(|| existing.and_then(|m| m.bitrate));
    let rotation = probe
        .rotation
        .map(|r| r as i64)
        .or_else(|| existing.map(|m| m.rotation))
        .unwrap_or(0);
    (fps, bitrate, rotation)
}

/// 探测成功后回写 DB `video_meta` codec/fps/bitrate/rotation/has_audio(MF 探不了的容器补全)。
/// audio 轨列表不入 DB(仅本次判定用)。失败仅告警,不阻断播放解析。`existing` 为本次 resolve
/// 已读到的既有 meta,供 [`merge_probe_meta_fields`] 对 `None` 字段护值(§V6-6)。
async fn write_back_probe(
    state: &Arc<AppState>,
    item_id: i64,
    probe: &VideoProbeInfo,
    existing: Option<&VideoMeta>,
) {
    let codec = probe.video_codec.clone();
    let (fps, bitrate, rotation) = merge_probe_meta_fields(existing, probe);
    let has_audio = !probe.audio_tracks.is_empty();
    let s = Arc::clone(state);
    let _ = tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = upsert_video_meta(
            &conn,
            item_id,
            Some(&codec),
            fps,
            bitrate,
            rotation,
            has_audio,
        ) {
            tracing::warn!(item_id, error = %e, "回写 video_meta 失败(不阻断播放)");
        }
    })
    .await;
}

/// resolve 主逻辑(`confirmed` 跳过单文件护栏;`force_transcode`/`force_transcode_confirmed`
/// 见命令壳 `confirm_video_playback`)。命令壳 `resolve_video_playback`/`confirm_video_playback`
/// 的唯一入口,`pub(crate)`。
pub(crate) async fn resolve_impl(
    app: AppHandle,
    state: Arc<AppState>,
    item_id: i64,
    confirmed: bool,
    force_transcode: bool,
    force_transcode_confirmed: bool,
) -> Result<VideoPlaybackResolution> {
    // ── 1. 读 item 详情(rusqlite → spawn_blocking)────────────────────────
    let s = Arc::clone(&state);
    let detail = tokio::task::spawn_blocking(move || {
        let conn = s.db_read_pool.get()?;
        get_media_detail(&conn, item_id)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    if detail.item.media_type != "video" {
        return Err(AppError::UnsupportedFormat(format!(
            "item {item_id} 非视频,不可解析播放 | not a video item"
        )));
    }
    let abs_path = detail.abs_path.clone();
    let file_format = detail.item.file_format.to_ascii_lowercase();
    let cache_key = detail.item.cache_key;
    let file_size = detail.item.file_size.max(0) as u64;
    let fingerprint = format!("{}:{}", detail.item.file_mtime, detail.item.file_size);
    let cache_dir = {
        state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone()
    };

    // ── 2. 缓存命中检查(提到 probe 之前):status=2 + 产物文件真在 → 直接回 derived,
    //    不为已缓存的重复播放再付一次 ffprobe + write_back_probe 代价。仅缓存未命中才走
    //    probe → 判定表(§3 之后)。就绪判据须文件真在(GC/清缓存删了产物但行仍 2 的自愈:
    //    视作待产出,继续走 probe → resolve_derived 的在途/派生流程)。
    let playable = playable_path(&cache_dir, cache_key);
    let s = Arc::clone(&state);
    let playable_check = playable.clone();
    let cache_hit = tokio::task::spawn_blocking(move || {
        let conn = s.db_read_pool.get()?;
        let st = get_derivation_state(&conn, item_id, KIND_PLAYABLE)?;
        Ok::<_, AppError>(matches!(&st, Some((2, Some(_)))) && playable_check.exists())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    if cache_hit {
        touch_played(&playable);
        return Ok(VideoPlaybackResolution::derived(
            playable.to_string_lossy().into_owned(),
            "cached",
        ));
    }

    // ── 3. 取判定输入:worker 可用则探测;否则 DB 兜底 / needs_component ─────
    let svc = state.video_worker_service.get().cloned();
    let (policy, probe_duration_ms) = match &svc {
        Some(svc) => match svc
            .probe(item_id, abs_path.clone(), fingerprint.clone())
            .await
        {
            Ok(probe) => {
                write_back_probe(&state, item_id, &probe, detail.video_meta.as_ref()).await;
                let policy = probe_to_policy(&file_format, &probe);
                (policy, probe.duration_ms)
            }
            // 组件未就绪/未授权:退 DB 兜底(直播容器仍可乐观直播;否则 needs_component)。
            Err(VideoServiceError::NeedsComponent) | Err(VideoServiceError::NotAuthorized) => {
                return Ok(db_fallback(&file_format, &abs_path, &detail));
            }
            Err(e) => return Err(e.into()),
        },
        None => return Ok(db_fallback(&file_format, &abs_path, &detail)),
    };

    // ── 4. 判定 → 分流 ─────────────────────────────────────────────────────
    let original_verdict = decide(&policy);
    // D-444 ④「本地转码作可选」(V7 补遗):用户已在前端点「改用本地转码」并复调 confirm——
    // 判定表本身语义不变(NeedsHevcExt 仍是 decide() 的原始结果),仅把**本次 resolve 结果**
    // 覆写为 Transcode,走既有 remux/transcode 产出管线,不再裸交原文件等前端二次实测。
    // 覆写是否真的发生(仅 NeedsHevcExt + force_transcode 才生效)单独记下,供下方护栏判定用——
    // 与 verdict 是否等于 Transcode 不同:后者也可能是判定表本就给出的 Transcode(无覆写)。
    let force_transcode_applied =
        force_transcode && matches!(original_verdict, PlaybackVerdict::NeedsHevcExt);
    let verdict = apply_force_transcode(original_verdict, force_transcode);
    let label = verdict_label(&verdict);
    // V7 项6:force_transcode 覆写出的 Transcode 不得因为「这是 confirm_video_playback 调用
    // (即 confirmed 恒 true)」就绕过超池 50% 护栏——那只表示走的是确认通道,不代表用户已经
    // 看过并接受了这次(可能远超普通判定预期的)产物体积。真正的「已确认体积」信号是
    // `force_transcode_confirmed`(前端在收到 needsConfirm 回显后的二次调用才会传 true)。
    let effective_confirmed = confirmed && (!force_transcode_applied || force_transcode_confirmed);
    match verdict {
        PlaybackVerdict::DirectPlay => Ok(VideoPlaybackResolution::direct(abs_path)),
        PlaybackVerdict::NeedsHevcExt => Ok(VideoPlaybackResolution::needs_hevc_ext(abs_path)),
        PlaybackVerdict::Remux
        | PlaybackVerdict::RemuxAudioTranscode { .. }
        | PlaybackVerdict::Transcode => {
            resolve_derived(
                app,
                state,
                svc,
                item_id,
                cache_key,
                file_size,
                effective_confirmed,
                force_transcode_applied,
                abs_path,
                fingerprint,
                &cache_dir,
                verdict,
                label,
                probe_duration_ms,
                selected_audio_index(&policy),
            )
            .await
        }
    }
}

/// force_transcode 覆写(D-444 ④,纯函数便于单测,§V7 项12):`NeedsHevcExt` + `force=true` →
/// `Transcode`;其余判定分支原样透传(force_transcode 只对「系统缺 HEVC 解码器」这一支生效,
/// 不影响 DirectPlay/Remux/RemuxAudioTranscode 等既有分流)。
fn apply_force_transcode(verdict: PlaybackVerdict, force: bool) -> PlaybackVerdict {
    if force && matches!(verdict, PlaybackVerdict::NeedsHevcExt) {
        PlaybackVerdict::Transcode
    } else {
        verdict
    }
}

/// 无探测时的 DB 兜底(§5.1):直播容器 + DB 可解 codec → 乐观直播(音轨未知);HEVC → 引导;
/// 否则 needs_component(须装 ffmpeg 才能探测/改封/转码)。
fn db_fallback(
    file_format: &str,
    abs_path: &str,
    detail: &crate::db::models::MediaDetail,
) -> VideoPlaybackResolution {
    let vc_norm = detail
        .video_meta
        .as_ref()
        .and_then(|m| m.video_codec.as_ref())
        .map(|c| normalize_codec(c));

    if is_direct_container(file_format) {
        match vc_norm.as_deref() {
            Some("hevc") => VideoPlaybackResolution::needs_hevc_ext(abs_path.to_string()),
            // 可解视频 codec,或无 codec 信息但直播容器 → 乐观直播(前端播放失败再回退,V7)。
            Some(v) if is_direct_video_codec(v) => {
                VideoPlaybackResolution::direct(abs_path.to_string())
            }
            None => VideoPlaybackResolution::direct(abs_path.to_string()),
            // 视频不可解 → 须转码 → 装组件。
            Some(_) => VideoPlaybackResolution::needs_component(),
        }
    } else {
        // 非直播容器须 remux/transcode → 须 ffmpeg。
        VideoPlaybackResolution::needs_component()
    }
}

/// 派生分流(remux/半转码/全转码):在途 / 护栏 / 派任务(缓存命中已由调用方 `resolve_impl`
/// 在 probe 之前短路,不会走到这里)。
#[allow(clippy::too_many_arguments)]
async fn resolve_derived(
    app: AppHandle,
    state: Arc<AppState>,
    svc: Option<Arc<VideoWorkerService>>,
    item_id: i64,
    cache_key: i64,
    file_size: u64,
    confirmed: bool,
    // 本次 Transcode 是否由 force_transcode 覆写产生(V7 项6):供落入 needsConfirm 时回显
    // `force_transcode` 字段,前端据此在下次复调时保持 forceTranscode 粘性。
    force_transcode_applied: bool,
    abs_path: String,
    // resolve 时拼的源指纹(`mtime:size`,§9.1):rename 前复核用,防派生耗时窗口内源文件被
    // 替换/编辑却仍让旧内容产出顶上终态。
    fingerprint: String,
    cache_dir: &std::path::Path,
    verdict: PlaybackVerdict,
    label: &'static str,
    probe_duration_ms: Option<u64>,
    audio_track_index: Option<u32>,
) -> Result<VideoPlaybackResolution> {
    let playable = playable_path(cache_dir, cache_key);
    let playable_rel = format!("video/{cache_key}.mp4");

    // ① 缓存命中检查已在 `resolve_impl` 提到 probe 之前做过(本函数只在未命中时才被调用);
    //    这里只需重取当前派生状态,判在途(status=1)→ preparing(去重:worker_service 侧
    //    (item,kind) 已合并)。
    let s = Arc::clone(&state);
    let existing_state = tokio::task::spawn_blocking(move || {
        let conn = s.db_read_pool.get()?;
        get_derivation_state(&conn, item_id, KIND_PLAYABLE)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // ② 在途(status=1)→ preparing。
    if matches!(existing_state, Some((1, _))) {
        return Ok(VideoPlaybackResolution::preparing(label));
    }

    // ③ 单文件护栏(§5.4):预估 > 池 50% 且未确认 → needs_confirm。
    let estimate = estimate_output_bytes(file_size);
    if !confirmed {
        let s = Arc::clone(&state);
        let max_mb = tokio::task::spawn_blocking(move || read_video_cache_max_mb(&s))
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
        if exceeds_half_pool(estimate, max_mb) {
            return Ok(VideoPlaybackResolution::needs_confirm(
                estimate,
                label,
                force_transcode_applied.then_some(true),
            ));
        }
    }

    // ④ 须产出:worker 必须可用(能走到这里说明 probe 已成功 → svc Some;稳妥再核)。
    let Some(svc) = svc else {
        return Ok(VideoPlaybackResolution::needs_component());
    };

    // ⑤ upsert + claim `video_playable` 行(按需入队,不经背景流水线)。原子事务返回是否发生
    //    真实 0/3→1 转移(§V6-2 TOCTOU):并发第二个 resolve 抢不到认领(changed=0)→ 不重复
    //    spawn,直接回 preparing(在途 job 完成后经事件广播,两方前端都收得到)。
    let s = Arc::clone(&state);
    let claimed = tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        upsert_and_claim_derivation(&conn, item_id, KIND_PLAYABLE)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    if !claimed {
        // 并发去重:另一次 resolve 已认领并派活,本次不重复派 job,直接回 preparing。
        return Ok(VideoPlaybackResolution::preparing(label));
    }

    // ⑥ 派后台任务(交互优先级),立即回 preparing;完成/失败经事件广播。
    spawn_playable_job(
        app,
        Arc::clone(&state),
        svc,
        item_id,
        cache_key,
        abs_path,
        fingerprint,
        cache_dir.to_path_buf(),
        playable,
        playable_rel,
        verdict,
        label,
        probe_duration_ms,
        audio_track_index,
    );
    Ok(VideoPlaybackResolution::preparing(label))
}

/// 后台产出任务:派 worker(remux/半转码/全转码)→ 验收 → `*.tmp` 同卷 rename → finish → 广播。
#[allow(clippy::too_many_arguments)]
fn spawn_playable_job(
    app: AppHandle,
    state: Arc<AppState>,
    svc: Arc<VideoWorkerService>,
    item_id: i64,
    cache_key: i64,
    abs_path: String,
    // resolve 时拼的源指纹(`mtime:size`),rename 前与源文件现值复核(§9.1)。
    fingerprint: String,
    cache_dir: std::path::PathBuf,
    playable: std::path::PathBuf,
    playable_rel: String,
    verdict: PlaybackVerdict,
    label: &'static str,
    probe_duration_ms: Option<u64>,
    audio_track_index: Option<u32>,
) {
    emit_progress(&app, item_id, "preparing", None, None);
    tokio::spawn(async move {
        let tmp = playable_tmp_path(&cache_dir, cache_key);
        // 确保池目录存在(work_dir 白名单前缀;worker 写 tmp 前 host 先建目录)。
        if let Err(e) = tokio::fs::create_dir_all(pool_dir(&cache_dir)).await {
            tracing::warn!(item_id, error = %e, "创建视频池目录失败");
            finish_and_emit_error(&app, &state, item_id, "video_resource_limit").await;
            return;
        }
        let tmp_str = tmp.to_string_lossy().into_owned();
        // §9.1 复核前置:产出耗时窗口(remux/transcode 可长达数十分钟)内源文件可能被替换/编辑,
        // rename 前须用**当前**源文件重新取 mtime+size 与 resolve 时的 `fingerprint` 比对——
        // `abs_path` 随后按 verdict 分支被移进 svc.remux/transcode(按值传参),这里先克隆一份
        // 供验收阶段读取,不影响下方派活。
        let abs_path_for_verify = abs_path.clone();

        // per-job 进度(§5.3):watch 发送端注册进 worker_service job、接收端由后台转发任务订阅。
        // 新订阅者 `borrow()`/首次 `changed()` 即取到当前值(watch 内建快照语义,同缩略图
        // 进度先例的「新订阅者可取当前值」诉求)。job 完成/取消后发送端随 Job 从 map 摘除被
        // drop,转发任务的 `changed()` 报错自然退出。
        let (progress_tx, progress_rx) = watch::channel(VideoProgressSnapshot::default());
        spawn_progress_forwarder(app.clone(), item_id, progress_rx);

        let result = match verdict {
            PlaybackVerdict::Remux => {
                svc.remux(
                    item_id,
                    abs_path,
                    tmp_str,
                    false,
                    None,
                    probe_duration_ms,
                    Some(progress_tx),
                )
                .await
            }
            PlaybackVerdict::RemuxAudioTranscode { audio_track_index } => {
                svc.remux(
                    item_id,
                    abs_path,
                    tmp_str,
                    true,
                    Some(audio_track_index),
                    probe_duration_ms,
                    Some(progress_tx),
                )
                .await
            }
            PlaybackVerdict::Transcode => {
                svc.transcode(
                    item_id,
                    abs_path,
                    tmp_str,
                    ENCODER_LADDER.iter().map(|s| s.to_string()).collect(),
                    Some(TRANSCODE_CRF),
                    None,
                    None,
                    audio_track_index,
                    true,
                    probe_duration_ms,
                    Some(progress_tx),
                )
                .await
            }
            // DirectPlay/NeedsHevcExt 不会派 job。
            _ => Err(VideoServiceError::WorkerFailed),
        };

        match result {
            Ok(out) => {
                // 验收①:worker 报的产物时长/字节须 > 0(基本完整性)。
                if out.out_duration_ms == 0 || out.out_bytes == 0 {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    tracing::warn!(item_id, "视频产物验收失败(时长/字节为 0)");
                    finish_and_emit_error(&app, &state, item_id, "video_malformed_source").await;
                    return;
                }
                // 验收②(§V6-9 产物级深检):rename 前经 service.probe 复探产物时长,与源探测时长
                // ±5% 容差比对——超差=产物损坏(转码丢帧/截断)。probe 签名无优先级参数,沿用现状。
                // 源时长未知(probe_duration_ms=None)时跳过(无基准可比)。faststart 产物级检测
                // (moov 是否真前置)不做:worker 侧 `-movflags +faststart` 参数已由 worker 单测钉定,
                // 产物级 moov 前置检测归 V8 遗留。
                if let Some(src_ms) = probe_duration_ms {
                    if src_ms > 0 {
                        let verify_path = tmp.to_string_lossy().into_owned();
                        // 产物复探走独立去重维度 `VideoKind::ProbeVerify`(`probe_verify`),不与
                        // 源 probe 共 `(item_id, VideoKind::Probe)` 键——见 worker_service.rs
                        // `VideoKind::ProbeVerify` 注释。`pv` 只用于下方比对,**不得**回填
                        // `video_meta`(write_back_probe 只在 `resolve_impl` 的源 probe 分支调用,
                        // 此处从未传入,隔离已现状成立,注释钉死)。
                        match svc
                            .probe_verify(
                                item_id,
                                verify_path,
                                format!("playable-verify:{cache_key}"),
                            )
                            .await
                        {
                            Ok(pv) => {
                                if let Some(out_ms) = pv.duration_ms {
                                    // |out-src|/src > 5%  ⇔  |out-src|*20 > src。
                                    let diff = (out_ms as i64 - src_ms as i64).unsigned_abs();
                                    if diff.saturating_mul(20) > src_ms {
                                        let _ = tokio::fs::remove_file(&tmp).await;
                                        tracing::warn!(
                                            item_id,
                                            src_ms,
                                            out_ms,
                                            "产物时长偏离源 >5%,判损坏"
                                        );
                                        finish_and_emit_error(
                                            &app,
                                            &state,
                                            item_id,
                                            "video_malformed_source",
                                        )
                                        .await;
                                        return;
                                    }
                                }
                                // 产物无时长信息:不阻断(验收①已保证 worker 报时长 > 0)。
                            }
                            Err(e) => {
                                let _ = tokio::fs::remove_file(&tmp).await;
                                tracing::warn!(item_id, code = e.code(), "产物复探失败,判损坏");
                                finish_and_emit_error(
                                    &app,
                                    &state,
                                    item_id,
                                    "video_malformed_source",
                                )
                                .await;
                                return;
                            }
                        }
                    }
                }
                // 验收③(§9.1 rename 前源指纹复核):产出耗时窗口内源文件若被替换/编辑(mtime/size
                // 变化),tmp 已基于**旧内容**产出——不得让它顶上终态。取源文件现值 mtime+size 与
                // resolve 时拼的 `fingerprint` 比对;取不到源文件元数据(已删/权限)同样判不通过。
                let source_meta_ok = match tokio::fs::metadata(&abs_path_for_verify).await {
                    Ok(meta) => {
                        let mtime_secs = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        source_fingerprint_matches(mtime_secs, meta.len() as i64, &fingerprint)
                    }
                    Err(_) => false,
                };
                if !source_meta_ok {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    tracing::warn!(
                        item_id,
                        "源文件指纹与 resolve 时不符(已变更/不可读),判损坏,不 rename"
                    );
                    finish_and_emit_error(&app, &state, item_id, "video_malformed_source").await;
                    return;
                }
                // §V6-4 in_use:rename 前登记产物终名,使随后(及并发)的 LRU 驱逐轮跳过本新产物。
                register_in_use(&playable);
                // *.tmp 同卷 rename 为终名。
                if let Err(e) = tokio::fs::rename(&tmp, &playable).await {
                    unregister_in_use(&playable);
                    let _ = tokio::fs::remove_file(&tmp).await;
                    tracing::warn!(item_id, error = %e, "视频产物 rename 失败");
                    finish_and_emit_error(&app, &state, item_id, "video_resource_limit").await;
                    return;
                }
                // finish → status=2 + payload;随后 LRU 驱逐(新产物在 in_use 集合内,不自伤)。
                let s = Arc::clone(&state);
                let rel = playable_rel.clone();
                let cache_dir2 = cache_dir.clone();
                let finish_res = tokio::task::spawn_blocking(move || {
                    let db_res = {
                        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        batch_finish_derivations(
                            &conn,
                            &[(
                                item_id,
                                KIND_PLAYABLE.to_string(),
                                2,
                                Some(rel),
                                None,
                                None,
                                None,
                            )],
                        )
                    };
                    let max_mb = read_video_cache_max_mb(&s);
                    enforce_pool_limit(&cache_dir2, max_mb, &in_use_snapshot());
                    db_res
                })
                .await;
                unregister_in_use(&playable);
                let _ = label; // 诊断标签仅用于同步返回;事件用统一 status
                               // §V6-10 finish 双静默显式化:DB 写失败 / 任务 join 失败均 warn(item_id),且**不发
                               // ready**——改发 error(产物在盘但 DB 未记 status=2,靠下次启动 boot 复位或 error 重触自愈)。
                match finish_res {
                    Ok(Ok(())) => {
                        let src = playable.to_string_lossy().into_owned();
                        emit_progress(&app, item_id, "ready", Some(src), None);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(item_id, error = %e, "video_playable finish 写 DB 失败,不发 ready");
                        emit_progress(&app, item_id, "error", None, Some("video_worker_failed"));
                    }
                    Err(e) => {
                        tracing::warn!(item_id, error = %e, "video_playable finish 任务 join 失败,不发 ready");
                        emit_progress(&app, item_id, "error", None, Some("video_worker_failed"));
                    }
                }
            }
            // 取消(§5.4):`cancel_video_playback` 命令已经把 `video_playable` 行回退 pending 并
            // 广播 cancelled 事件——这里只清 tmp,绝不能再 finish(status=3)+广播 error,否则会
            // 覆盖掉 cancel 命令刚写的 pending 态、并把用户主动取消误报成失败。
            Err(e) if should_suppress_finish_on_cancel(&e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                tracing::debug!(
                    item_id,
                    "视频派生任务已取消(DB/事件已由 cancel_video_playback 处理)"
                );
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                let code = e.code();
                tracing::warn!(item_id, code, "视频产物生成失败");
                finish_and_emit_error(&app, &state, item_id, code).await;
            }
        }
    });
}

/// 后台产出任务失败收尾判定(§5.4/§V6-7 衔接):`Cancelled` 已由 `cancel_video_playback` 命令
/// 自行处理 DB 回退 + cancelled 事件,`spawn_playable_job` 侧只须清 tmp,不得重复 finish/emit
/// error——否则与该命令的语义打架(取消会被误写成 status=3 失败态)。抽成纯函数便于单测,
/// 不依赖 tokio 任务/DB。
fn should_suppress_finish_on_cancel(e: &VideoServiceError) -> bool {
    matches!(e, VideoServiceError::Cancelled)
}

/// §9.1 rename 前源指纹复核:纯比对逻辑抽函数(不碰真实 IO,不依赖 tokio/`std::fs::Metadata`)。
/// 与 `resolve_impl` 拼 `fingerprint` 同格式(`"{mtime_secs}:{file_size}"`)——`spawn_playable_job`
/// 在 rename 前重新 `stat` 源文件、把现值喂进本函数与 resolve 时的指纹比对,不符即判损坏
/// (源文件在派生耗时窗口内被替换/编辑)。
fn source_fingerprint_matches(mtime_secs: i64, size: i64, expected_fingerprint: &str) -> bool {
    format!("{mtime_secs}:{size}") == expected_fingerprint
}

/// 失败收尾:finish → status=3 + 稳定 code(无内部串),广播 error 事件。
async fn finish_and_emit_error(
    app: &AppHandle,
    state: &Arc<AppState>,
    item_id: i64,
    code: &'static str,
) {
    let s = Arc::clone(state);
    let code_owned = code.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = batch_finish_derivations(
            &conn,
            &[(
                item_id,
                KIND_PLAYABLE.to_string(),
                3,
                None,
                Some(code_owned),
                None,
                None,
            )],
        );
    })
    .await;
    emit_progress(app, item_id, "error", None, Some(code));
}

/// 广播播放准备进度/状态事件(前端据 itemId 匹配)。emit 失败(无窗口)仅忽略。命令壳
/// `cancel_video_playback` 也要广播 cancelled 事件,故 `pub(crate)`。
pub(crate) fn emit_progress(
    app: &AppHandle,
    item_id: i64,
    status: &'static str,
    src: Option<String>,
    code: Option<&'static str>,
) {
    let _ = app.emit(
        VIDEO_PLAYBACK_PROGRESS_EVENT,
        PlaybackProgressPayload {
            item_id,
            status,
            src,
            code,
            percent: None,
            stage: None,
        },
    );
}

/// 广播 ffmpeg 细粒度进度(§5.3):`status` 恒 "preparing",附 percent/stage。
fn emit_progress_percent(app: &AppHandle, item_id: i64, snap: &VideoProgressSnapshot) {
    let _ = app.emit(
        VIDEO_PLAYBACK_PROGRESS_EVENT,
        PlaybackProgressPayload {
            item_id,
            status: "preparing",
            src: None,
            code: None,
            percent: snap.percent,
            stage: Some(snap.stage.clone()),
        },
    );
}

/// Progress 广播节流间隔(§5.3:≤2Hz;video-worker 侧 Progress 帧本已 ≤1/2s,这里再兜底一层
/// 防未来上游节流参数变化)。
const PROGRESS_EMIT_MIN_INTERVAL: Duration = Duration::from_millis(500);

/// 订阅 per-job 进度 watch、节流(≤2Hz)转发 [`emit_progress_percent`],直至发送端(job 完成/
/// 取消后 `Job` 从 map 摘除)关闭——`changed()` 返回错误即自然退出,不留悬挂任务(§5.3)。
fn spawn_progress_forwarder(
    app: AppHandle,
    item_id: i64,
    mut rx: watch::Receiver<VideoProgressSnapshot>,
) {
    tokio::spawn(async move {
        let mut last_emit: Option<Instant> = None;
        loop {
            if rx.changed().await.is_err() {
                clear_progress_snapshot(item_id); // 发送端已丢弃(job 完成/取消)→ 摘除 host 快照。
                return; // 订阅自然结束。
            }
            let snap = rx.borrow_and_update().clone();
            // §V6-12:先无条件更新 host 侧快照(供 video_playback_progress_snapshot 现读,不受下方
            // 事件节流影响,始终反映最新已知 percent/stage);再按 ≤2Hz 节流广播事件。
            set_progress_snapshot(
                item_id,
                VideoPlaybackProgressSnapshot {
                    percent: snap.percent,
                    stage: snap.stage.clone(),
                },
            );
            let now = Instant::now();
            if last_emit.is_some_and(|t| now.duration_since(t) < PROGRESS_EMIT_MIN_INTERVAL) {
                continue; // 节流:丢弃过密更新,watch 语义下一次 changed() 会拿到最新值。
            }
            last_emit = Some(now);
            emit_progress_percent(&app, item_id, &snap);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use exotic_protocol::VideoAudioTrack;

    fn probe(container: &str, vcodec: &str, tracks: Vec<VideoAudioTrack>) -> VideoProbeInfo {
        VideoProbeInfo {
            container: container.into(),
            duration_ms: Some(1000),
            width: Some(1920),
            height: Some(1080),
            rotation: Some(0),
            fps: Some(24.0),
            bitrate: Some(1_000_000),
            video_codec: vcodec.into(),
            video_profile: None,
            bit_depth: Some(8),
            pixel_format: Some("yuv420p".into()),
            audio_tracks: tracks,
            has_subtitles: false,
            has_hdr_metadata: false,
        }
    }

    fn atrack(index: u32, codec: &str, is_default: bool) -> VideoAudioTrack {
        VideoAudioTrack {
            index,
            codec: codec.into(),
            channels: Some(2),
            language: None,
            is_default,
        }
    }

    /// probe_to_policy 用**文件扩展名**作 container_ext(非 probe.container),并透传音轨。
    #[test]
    fn probe_to_policy_uses_file_extension_not_format_name() {
        let p = probe("matroska,webm", "h264", vec![atrack(0, "ac3", true)]);
        let policy = probe_to_policy("mkv", &p);
        assert_eq!(
            policy.container_ext, "mkv",
            "用扩展名不用 ffprobe format_name"
        );
        assert_eq!(policy.audio_tracks.len(), 1);
        // 端到端:mkv/h264/ac3 → 半转码(轨 0)。
        assert_eq!(
            decide(&policy),
            PlaybackVerdict::RemuxAudioTranscode {
                audio_track_index: 0
            }
        );
    }

    /// verdict_label 稳定串覆盖五态。
    #[test]
    fn verdict_labels_are_stable() {
        assert_eq!(verdict_label(&PlaybackVerdict::DirectPlay), "direct");
        assert_eq!(
            verdict_label(&PlaybackVerdict::NeedsHevcExt),
            "needs_hevc_ext"
        );
        assert_eq!(verdict_label(&PlaybackVerdict::Remux), "remux");
        assert_eq!(
            verdict_label(&PlaybackVerdict::RemuxAudioTranscode {
                audio_track_index: 0
            }),
            "remux_audio_transcode"
        );
        assert_eq!(verdict_label(&PlaybackVerdict::Transcode), "transcode");
    }

    /// apply_force_transcode(V7 项12,纯函数):NeedsHevcExt+force=true → Transcode;
    /// DirectPlay+force=true → 原样透传(force_transcode 只对 NeedsHevcExt 生效)。
    #[test]
    fn apply_force_transcode_overrides_needs_hevc_ext_only() {
        assert_eq!(
            apply_force_transcode(PlaybackVerdict::NeedsHevcExt, true),
            PlaybackVerdict::Transcode,
            "NeedsHevcExt + force=true 应覆写为 Transcode"
        );
        assert_eq!(
            apply_force_transcode(PlaybackVerdict::DirectPlay, true),
            PlaybackVerdict::DirectPlay,
            "DirectPlay + force=true 不应被覆写"
        );
        assert_eq!(
            apply_force_transcode(PlaybackVerdict::NeedsHevcExt, false),
            PlaybackVerdict::NeedsHevcExt,
            "force=false 时原样透传,不覆写"
        );
    }

    /// probe 回填护值(§V6-6):探测字段为 None 时保留既有 video_meta——尤其 rotation(D-003
    /// 红线不清零);探测有值(含 rotation=0)即覆盖既有。
    #[test]
    fn probe_backfill_preserves_existing_on_none() {
        let existing = VideoMeta {
            video_codec: Some("hevc".into()),
            fps: Some(30.0),
            bitrate: Some(5000),
            rotation: 90,
            has_audio: true,
        };
        // 探测三字段全 None → 保留既有(rotation 保 90)。
        let mut p = probe("mp4", "hevc", vec![]);
        p.rotation = None;
        p.fps = None;
        p.bitrate = None;
        let (fps, bitrate, rotation) = merge_probe_meta_fields(Some(&existing), &p);
        assert_eq!(rotation, 90, "probe rotation=None → 保留既有 90(D-003)");
        assert_eq!(fps, Some(30.0), "probe fps=None → 保留既有");
        assert_eq!(bitrate, Some(5000), "probe bitrate=None → 保留既有");
        // 无既有 + probe None → (None, None, 0)。
        assert_eq!(merge_probe_meta_fields(None, &p), (None, None, 0));
        // probe 有值(rotation=0)→ 覆盖既有 90。
        let mut p2 = probe("mp4", "hevc", vec![]);
        p2.rotation = Some(0);
        assert_eq!(
            merge_probe_meta_fields(Some(&existing), &p2).2,
            0,
            "probe 有值(0)应覆盖既有 90"
        );
    }

    /// 后台产出任务失败收尾判定(项 1):`Cancelled` 应抑制 finish/emit error(已由
    /// `cancel_video_playback` 命令处理 DB+事件),其余错误码一律不抑制、须走 finish_and_emit_error。
    #[test]
    fn cancel_error_suppresses_finish_others_do_not() {
        assert!(
            should_suppress_finish_on_cancel(&VideoServiceError::Cancelled),
            "Cancelled 应抑制 finish/emit error"
        );
        for e in [
            VideoServiceError::NotAuthorized,
            VideoServiceError::NeedsComponent,
            VideoServiceError::WorkerUnavailable,
            VideoServiceError::SessionInitFailed,
            VideoServiceError::UnsupportedVariant,
            VideoServiceError::MalformedSource,
            VideoServiceError::ResourceLimit,
            VideoServiceError::Timeout,
            VideoServiceError::WorkerFailed,
            VideoServiceError::FfmpegUnavailable,
        ] {
            assert!(
                !should_suppress_finish_on_cancel(&e),
                "{e:?} 不应抑制 finish/emit error"
            );
        }
    }

    /// §9.1 rename 前源指纹复核(项 1):现值 mtime:size 与 resolve 时拼的 `fingerprint` 一致才
    /// 放行;mtime 变、size 变、或两者都变(源文件在派生耗时窗口内被替换/编辑)均判不通过。
    #[test]
    fn source_fingerprint_matches_detects_source_file_drift() {
        let original = format!("{}:{}", 1_700_000_000_i64, 12345_i64);
        assert!(
            source_fingerprint_matches(1_700_000_000, 12345, &original),
            "mtime/size 与 resolve 时一致 → 应放行"
        );
        assert!(
            !source_fingerprint_matches(1_700_000_001, 12345, &original),
            "mtime 变化(内容被改)→ 应判不符"
        );
        assert!(
            !source_fingerprint_matches(1_700_000_000, 99999, &original),
            "size 变化(内容被替换)→ 应判不符"
        );
        assert!(
            !source_fingerprint_matches(1_700_000_001, 99999, &original),
            "mtime+size 都变 → 应判不符"
        );
    }
}
