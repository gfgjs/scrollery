// src-tauri/src/video/worker_service/tests.rs
//! 视频 worker 服务单测(自 worker_service.rs 结构性拆分,tierB-1)。

use super::types::op_timeout;
use super::*;
use exotic_protocol::{FailureBody, SuccessBody, WorkerErrorCode};
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use crate::error::AppError;
use crate::exotic::catalog::Capability;
use crate::exotic::coordinator::VIDEO_PLUGIN_ID;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build current-thread runtime")
}

fn probe_info() -> VideoProbeInfo {
    VideoProbeInfo {
        container: "matroska,webm".into(),
        duration_ms: Some(1000),
        width: Some(1920),
        height: Some(1080),
        rotation: Some(0),
        fps: Some(24.0),
        bitrate: Some(1_000_000),
        video_codec: "h264".into(),
        video_profile: None,
        bit_depth: Some(8),
        pixel_format: Some("yuv420p".into()),
        audio_tracks: vec![],
        has_subtitles: false,
        has_hdr_metadata: false,
    }
}

fn success_for(op: &VideoOp) -> RawOutcome {
    match op.kind() {
        // ProbeVerify 从不由 `VideoOp::kind()` 产出(见其定义注释),此臂为穷尽匹配占位。
        VideoKind::Probe | VideoKind::ProbeVerify => RawOutcome::Success {
            body: SuccessBody {
                video_probe: Some(probe_info()),
                ..Default::default()
            },
            blob: Vec::new(),
        },
        VideoKind::Playable => RawOutcome::Success {
            body: SuccessBody {
                video_out: Some(VideoOutInfo {
                    out_bytes: 42,
                    out_duration_ms: 1000,
                    video_copied: true,
                    audio_copied: true,
                }),
                ..Default::default()
            },
            blob: Vec::new(),
        },
        VideoKind::Cover | VideoKind::Keyframes => RawOutcome::Success {
            body: SuccessBody {
                video_frames: Some(VideoFramesInfo {
                    cell_width: 160,
                    cell_height: 90,
                    n: 4,
                }),
                ..Default::default()
            },
            blob: vec![1, 2, 3],
        },
    }
}

/// mock runner:记录 op 运行序;背景 op(Cover/Keyframes)模拟「长活」并遵守 cancelled
/// (供抢占测试),交互 op 秒回成功。session 恒就绪(除非 `always_disconnect`)。
struct MockRunner {
    log: Arc<StdMutex<Vec<VideoKind>>>,
    /// 背景 op 的模拟工作预算(未被取消则跑满后成功)。
    bg_budget: Duration,
    /// 恒返回 Disconnected(注入 genuine worker 通路失败 → WorkerFailed,供 respawn 重试
    /// 测试;§2.4 深审 V4-6a)。
    always_disconnect: bool,
}

impl VideoJobRunner for MockRunner {
    fn ensure_session(&mut self) -> Result<(), VideoServiceError> {
        Ok(())
    }
    fn run_op(
        &mut self,
        op: &VideoOp,
        _timeout: Duration,
        _total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
        _on_progress: &mut dyn FnMut(&ProgressBody),
    ) -> RawOutcome {
        self.log.lock().unwrap().push(op.kind());
        if self.always_disconnect {
            return RawOutcome::Disconnected;
        }
        let is_bg = matches!(op.kind(), VideoKind::Cover | VideoKind::Keyframes);
        if is_bg {
            let start = Instant::now();
            while start.elapsed() < self.bg_budget {
                if cancelled() {
                    return RawOutcome::Disconnected; // 被抢占
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        success_for(op)
    }
    fn kill(&mut self) {}
}

fn mock_service(log: Arc<StdMutex<Vec<VideoKind>>>, bg_budget: Duration) -> VideoWorkerService {
    VideoWorkerService::with_runner(Box::new(MockRunner {
        log,
        bg_budget,
        always_disconnect: false,
    }))
}

/// 轮询 `log` 非空(消 40ms 固定 sleep 的 flake:driver 起跑时机不保证,§2.4 深审 V4-5),
/// 上限 2s 防真死锁下测试无限挂起。
fn wait_for_log_nonempty(log: &Arc<StdMutex<Vec<VideoKind>>>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while log.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "等待 op 起跑超时(2s)");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// map_outcome:静默超时 → Timeout;各 worker 错误码 → 对应 VideoServiceError;成功取对应应答体。
#[test]
fn map_outcome_covers_timeout_and_error_codes() {
    let probe = VideoOp::Probe {
        source_path: "a".into(),
        input_fingerprint: "f".into(),
    };
    assert_eq!(
        map_outcome(&probe, RawOutcome::TimedOut),
        Err(VideoServiceError::Timeout)
    );
    let fb = |c| FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: c,
        retryable: false,
        message: "内部串不外泄".into(),
    };
    assert_eq!(
        map_outcome(
            &probe,
            RawOutcome::Failure(fb(WorkerErrorCode::FfmpegUnavailable))
        ),
        Err(VideoServiceError::FfmpegUnavailable)
    );
    assert_eq!(
        map_outcome(
            &probe,
            RawOutcome::Failure(fb(WorkerErrorCode::UnsupportedVariant))
        ),
        Err(VideoServiceError::UnsupportedVariant)
    );
    assert_eq!(
        map_outcome(
            &probe,
            RawOutcome::Failure(fb(WorkerErrorCode::MalformedInput))
        ),
        Err(VideoServiceError::MalformedSource)
    );
    assert_eq!(
        map_outcome(&probe, RawOutcome::Disconnected),
        Err(VideoServiceError::WorkerFailed)
    );
    match map_outcome(&probe, success_for(&probe)) {
        Ok(VideoOutput::Probe(_)) => {}
        other => panic!("期望 Probe 成功,得 {other:?}"),
    }
    // 稳定 code + 无内部串泄漏(映射进 AppError::Exotic)。
    let app: AppError = VideoServiceError::NeedsComponent.into();
    let v = serde_json::to_value(&app).unwrap();
    assert_eq!(v["code"], "video_needs_component");
}

/// transcode 总上界:max(2h, 时长×6),上限 12h(§2.4)。
#[test]
fn transcode_total_cap_clamps() {
    assert_eq!(
        op_timeout::transcode_total_cap(None),
        op_timeout::TRANSCODE_CAP_MIN
    );
    // 短片(10 分钟)×6 = 1h < 2h 下限 → 取下限。
    assert_eq!(
        op_timeout::transcode_total_cap(Some(10 * 60 * 1000)),
        op_timeout::TRANSCODE_CAP_MIN
    );
    // 1h 影片 ×6 = 6h,落区间内。
    assert_eq!(
        op_timeout::transcode_total_cap(Some(3600 * 1000)),
        Duration::from_secs(6 * 3600)
    );
    // 3h 影片 ×6 = 18h > 12h 上限 → 取上限。
    assert_eq!(
        op_timeout::transcode_total_cap(Some(3 * 3600 * 1000)),
        op_timeout::TRANSCODE_CAP_MAX
    );
}

/// 优先级 + 抢占:背景任务在途时交互任务到达 → 背景被抢占(Disconnected)→ 交互先完成 →
/// 背景重回队列头续跑完成。运行序 = [背景, 交互, 背景]。
#[test]
fn interactive_preempts_background_then_resumes() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_millis(200));

    // 先入背景 Keyframes,给 driver 时间起跑。
    let rx_bg = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "bg".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Keyframes {
                    n: 4,
                    cell_height: 90,
                },
            },
            1,
            VideoPriority::Background,
        )
        .unwrap();
    wait_for_log_nonempty(&log);
    // 交互 Probe 到达 → 抢占在途背景。
    let rx_it = svc
        .enqueue(
            VideoOp::Probe {
                source_path: "it".into(),
                input_fingerprint: "f".into(),
            },
            2,
            VideoPriority::Interactive,
        )
        .unwrap();

    let (it, bg) = rt().block_on(async {
        (
            VideoWorkerService::await_output(rx_it).await,
            VideoWorkerService::await_output(rx_bg).await,
        )
    });
    assert!(matches!(it, Ok(VideoOutput::Probe(_))), "交互任务应成功");
    assert!(
        matches!(bg, Ok(VideoOutput::Frames { .. })),
        "背景任务应续跑成功"
    );

    let seq = log.lock().unwrap().clone();
    assert_eq!(
        seq,
        vec![VideoKind::Keyframes, VideoKind::Probe, VideoKind::Keyframes],
        "运行序应为 背景(被抢占)→ 交互 → 背景(续跑):{seq:?}"
    );
    drop(svc);
}

/// 去重(§9.12):同 (item, kind) 两请求挂载同一在途任务,worker 只跑一次,两 waiter 均得结果。
#[test]
fn same_item_kind_requests_are_deduped() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_millis(120));

    let mk = || VideoOp::Frames {
        source_path: "s".into(),
        input_fingerprint: "f".into(),
        mode: VideoFramesMode::Cover { max_long_edge: 512 },
    };
    let rx1 = svc.enqueue(mk(), 5, VideoPriority::Background).unwrap();
    let rx2 = svc.enqueue(mk(), 5, VideoPriority::Background).unwrap();

    let (r1, r2) = rt().block_on(async {
        (
            VideoWorkerService::await_output(rx1).await,
            VideoWorkerService::await_output(rx2).await,
        )
    });
    assert!(matches!(r1, Ok(VideoOutput::Frames { .. })));
    assert!(matches!(r2, Ok(VideoOutput::Frames { .. })));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        &[VideoKind::Cover],
        "同 (item,kind) 去重后 worker 只应跑一次"
    );
    drop(svc);
}

/// §V6-9 产物级复探去重隔离:同一 item_id 上「源 probe」(`VideoKind::Probe`)与「产物复探」
/// (`probe_verify` → `VideoKind::ProbeVerify`)去重键不同——两者应各跑一次(不互相去重挂靠),
/// 与 `same_item_kind_requests_are_deduped`(同键才去重)对照。
#[test]
fn probe_and_probe_verify_do_not_dedupe_across_kinds() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_millis(1));

    let (r1, r2) = rt().block_on(async {
        (
            svc.probe(7, "s".into(), "fp".into()).await,
            svc.probe_verify(7, "s".into(), "fp".into()).await,
        )
    });
    assert!(r1.is_ok(), "源 probe 应成功:{r1:?}");
    assert!(r2.is_ok(), "产物复探应成功:{r2:?}");
    // MockRunner 记录 `op.kind()`(恒 Probe,不因去重键而变——两者本是同一 VideoOp 变体),
    // 故这里断言的是**日志条数**:去重键若被误共用(同 `(item, VideoKind::Probe)`),第二次
    // 调用会挂靠第一个 job、worker 只跑一次、日志长度为 1;实际去重键不同 → 各自新建 job →
    // 日志长度为 2,证明两次调用未被去重合并。
    assert_eq!(
        log.lock().unwrap().as_slice(),
        &[VideoKind::Probe, VideoKind::Probe],
        "去重键不同(Probe vs ProbeVerify),两次调用应各自触发一次源 op 运行,不被去重合并成一次"
    );
    drop(svc);
}

/// WorkerFailed(genuine Disconnected)respawn 重试直至 `MAX_ATTEMPTS` 耗尽 → 报错;
/// 验证恰好跑 `MAX_ATTEMPTS` 次(respawn 重试预算不多不少,§2.4 深审 V4-6a)。
#[test]
fn worker_failed_retries_up_to_max_attempts_then_errors() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = VideoWorkerService::with_runner(Box::new(MockRunner {
        log: Arc::clone(&log),
        bg_budget: Duration::ZERO,
        always_disconnect: true,
    }));

    let rx = svc
        .enqueue(
            VideoOp::Probe {
                source_path: "a".into(),
                input_fingerprint: "f".into(),
            },
            1,
            VideoPriority::Interactive,
        )
        .unwrap();
    let res = rt().block_on(VideoWorkerService::await_output(rx));
    assert_eq!(res, Err(VideoServiceError::WorkerFailed));
    assert_eq!(
        log.lock().unwrap().len(),
        MAX_ATTEMPTS as usize,
        "respawn 重试预算耗尽前应恰好跑 MAX_ATTEMPTS 次"
    );
    drop(svc);
}

/// shutdown 清账(§2.4 深审 V4-6b):service drop 时,在途任务(run_op 中途被 cancelled 命中
/// shutdown)与在队未起跑任务全部以 Cancelled 回收 waiter,不留挂起。
#[test]
fn shutdown_cancels_in_flight_and_queued_waiters() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_millis(300));

    let rx_running = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "a".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Cover { max_long_edge: 512 },
            },
            1,
            VideoPriority::Background,
        )
        .unwrap();
    wait_for_log_nonempty(&log); // 确保第一个 job 已起跑,进入 bg_budget 轮询。
    let rx_queued = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "b".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Cover { max_long_edge: 512 },
            },
            2,
            VideoPriority::Background,
        )
        .unwrap();

    drop(svc); // Drop 内 join 驱动线程,阻塞至收尾清账完成。

    let (r_running, r_queued) = rt().block_on(async {
        (
            VideoWorkerService::await_output(rx_running).await,
            VideoWorkerService::await_output(rx_queued).await,
        )
    });
    assert_eq!(
        r_running,
        Err(VideoServiceError::Cancelled),
        "在途任务应因 shutdown 回 Cancelled(非 WorkerFailed)"
    );
    assert_eq!(
        r_queued,
        Err(VideoServiceError::Cancelled),
        "在队未起跑任务应回 Cancelled"
    );
}

/// per-job 硬取消(§5.4):在队未起跑的 job 被 `cancel()` 直接出队,不占用 driver、立即回
/// Cancelled;driver 空闲后续跑其余(在途 job1)正常完成。
#[test]
fn cancel_removes_queued_job_without_running_it() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_millis(150));

    // job1 占住 driver(背景,跑 150ms)。
    let rx1 = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "a".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Cover { max_long_edge: 512 },
            },
            1,
            VideoPriority::Background,
        )
        .unwrap();
    wait_for_log_nonempty(&log); // job1 已起跑,driver 忙,job2 必落队列。
                                 // job2 入队(不同 item,同 kind 亦可——去重维度含 item_id)。
    let rx2 = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "b".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Cover { max_long_edge: 512 },
            },
            2,
            VideoPriority::Background,
        )
        .unwrap();
    svc.cancel(2, VideoKind::Cover); // 命中在队未起跑分支,立即出队。

    let (r1, r2) = rt().block_on(async {
        (
            VideoWorkerService::await_output(rx1).await,
            VideoWorkerService::await_output(rx2).await,
        )
    });
    assert!(
        matches!(r1, Ok(VideoOutput::Frames { .. })),
        "job1 不受影响,正常完成"
    );
    assert_eq!(r2, Err(VideoServiceError::Cancelled), "job2 应被出队取消");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        &[VideoKind::Cover],
        "job2 从未起跑,worker 只应跑 job1 一次"
    );
    drop(svc);
}

/// per-job 硬取消(§5.4):在途 job 被 `cancel()` 置位后,下一轮 cancelled() 轮询命中即
/// kill 回 Cancelled(非 respawn 重试的 WorkerFailed)。
#[test]
fn cancel_kills_in_flight_job_and_returns_cancelled() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let svc = mock_service(Arc::clone(&log), Duration::from_secs(2)); // 长跑,靠 cancel 提前打断

    let rx = svc
        .enqueue(
            VideoOp::Frames {
                source_path: "a".into(),
                input_fingerprint: "f".into(),
                mode: VideoFramesMode::Keyframes {
                    n: 4,
                    cell_height: 90,
                },
            },
            1,
            VideoPriority::Background,
        )
        .unwrap();
    wait_for_log_nonempty(&log); // 确保已在途(driver 已 pop,进入 bg_budget 轮询)。
    svc.cancel(1, VideoKind::Keyframes);

    let res = rt().block_on(VideoWorkerService::await_output(rx));
    assert_eq!(
        res,
        Err(VideoServiceError::Cancelled),
        "在途取消应回 Cancelled,不应 respawn 重试成 WorkerFailed"
    );
    drop(svc);
}

/// catalog offering 过校验(CommonFormatConflict 例外 + builtin/free):video-extended 声明
/// rmvb/vob(真 exotic 扩展名)、builtin、thumbnail、free、media_kind=video。
#[test]
fn video_offering_passes_catalog_validation() {
    use crate::exotic::catalog::{CatalogSnapshot, MediaKind};
    let snap = CatalogSnapshot::builtin().expect("内置 Catalog 含 video-extended 后仍须合法");
    for ext in ["rmvb", "vob"] {
        let off = snap
            .resolve_format(ext)
            .unwrap_or_else(|| panic!("{ext} 必须在内置 Catalog"));
        assert_eq!(off.plugin_id, VIDEO_PLUGIN_ID);
        assert!(off.builtin, "video-extended 须 builtin");
        assert_eq!(off.license_tier, "free");
        assert_eq!(off.media_kind, MediaKind::Video);
        assert!(off.claims_capability(Capability::Thumbnail));
    }
}
