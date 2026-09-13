// src-tauri/src/video/worker_service/runner.rs
//! 桥接真实 worker 进程的 [`VideoJobRunner`] 适配层(自 worker_service.rs 结构性拆分,tierB-1)。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use exotic_protocol::{ProgressBody, RequestBody, WorkerErrorCode};

use crate::exotic::catalog::Capability;
use crate::exotic::coordinator::{VIDEO_PLUGIN_ID, VIDEO_WORKER_ID};
use crate::exotic::supervisor::WorkerSupervisor;
use crate::exotic::worker::{RawOutcome, WorkerConfig, WorkerSpec};
use crate::state::AppState;

use super::types::op_timeout;
use super::{VideoJobRunner, VideoOp, VideoServiceError, HANDSHAKE_TIMEOUT};

/// 真实 runner:直持 [`WorkerSupervisor`],惰性起 worker + `VideoSessionInit`,崩溃后重建(§2.4)。
pub(super) struct WorkerVideoRunner {
    pub(super) state: Arc<AppState>,
    pub(super) supervisor: Option<WorkerSupervisor>,
    pub(super) session_id: u64,
}

impl WorkerVideoRunner {
    /// 输出白名单前缀 = `{cache_dir}/video/`(design.md §5.2:与派生产物同卷,`*.tmp` 同卷 rename)。
    fn work_dir(&self) -> PathBuf {
        let cache_dir = {
            let cfg = self
                .state
                .thumb_config
                .read()
                .unwrap_or_else(|e| e.into_inner());
            cfg.cache_dir.clone()
        };
        cache_dir.join("video")
    }
}

impl VideoJobRunner for WorkerVideoRunner {
    fn ensure_session(&mut self) -> Result<(), VideoServiceError> {
        // ① 总闸:授权(builtin+free 直放行既有语义,§2.1)。offering 声明 thumbnail 能力,
        //    is_task_runnable 折叠 availability == Authorized 即放行。
        let host = self.state.exotic_host();
        if !host.is_task_runnable(VIDEO_PLUGIN_ID, Capability::Thumbnail) {
            return Err(VideoServiceError::NotAuthorized);
        }

        // ② FFmpeg 组件就绪?未就绪 → needs_component(前端展示下载卡片,§3.3)。
        let ffmpeg_exe = match crate::exotic::tools::ffmpeg_tool_status(&self.state.app_data_dir) {
            crate::exotic::tools::ToolStatus::Ready { ffmpeg_exe, .. } => ffmpeg_exe,
            _ => return Err(VideoServiceError::NeedsComponent),
        };

        // ③ supervisor 活着即复用(会话续用)。
        if self
            .supervisor
            .as_ref()
            .map(|s| s.is_alive())
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.supervisor = None; // 丢弃死实例(Drop 兜底 kill)

        // ④ 定位 worker 二进制：dev 优先使用 EXOTIC_VIDEO_WORKER_PATH，默认找 target/debug
        // 同目录产物；prod 由 Tauri externalBin 随主程序同目录分发。
        let keyset =
            crate::exotic::trusted_keyset().map_err(|_| VideoServiceError::WorkerUnavailable)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let exe = crate::exotic::installer::resolve_worker_path(
            &self.state.exotic_install_dir(),
            VIDEO_PLUGIN_ID,
            &keyset,
            now,
        )
        .ok_or(VideoServiceError::WorkerUnavailable)?;

        let spec = WorkerSpec {
            exe_path: exe,
            expected_worker_id: VIDEO_WORKER_ID.to_string(),
            required_capabilities: vec![
                exotic_protocol::capability::VIDEO_PROBE.to_string(),
                exotic_protocol::capability::VIDEO_REMUX.to_string(),
                exotic_protocol::capability::VIDEO_TRANSCODE.to_string(),
                exotic_protocol::capability::VIDEO_FRAMES.to_string(),
            ],
        };
        let cfg = WorkerConfig {
            handshake_timeout: HANDSHAKE_TIMEOUT,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        };
        let mut sup = WorkerSupervisor::spawn(&spec, &cfg).map_err(|e| {
            tracing::warn!("video-worker spawn/握手失败:{e}");
            VideoServiceError::WorkerUnavailable
        })?;

        // ⑤ VideoSessionInit:ffmpeg 路径 + hash(防换包)+ work_dir 白名单前缀。
        self.session_id += 1;
        let work_dir = self.work_dir();
        if let Err(e) = std::fs::create_dir_all(&work_dir) {
            tracing::warn!("创建 video work_dir 失败:{e}");
            return Err(VideoServiceError::SessionInitFailed);
        }
        // hash:下发基准改为 tools.rs manifest 钉定常量(防换包闭环,§2.4 深审 V4-4)——本地对
        // ffmpeg.exe 现算保留,但降级为「本地损坏预检」:现算 ≠ manifest 即判定运行期被篡改/
        // 磁盘损坏,本地报错不 init(install 期 `.ready` 门本应保证一致,错误信息不含路径不入 IPC)。
        let local_sha256 = match crate::utils::hash::sha256_hex_of_file(&ffmpeg_exe) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("ffmpeg.exe 哈希失败:{e}");
                return Err(VideoServiceError::NeedsComponent);
            }
        };
        if !local_sha256.eq_ignore_ascii_case(crate::exotic::tools::FFMPEG_EXE_SHA256) {
            tracing::error!("ffmpeg.exe 现算 hash 与 manifest 不符(本地损坏预检未过),拒绝 init");
            return Err(VideoServiceError::NeedsComponent);
        }
        let req = RequestBody::VideoSessionInit {
            session_id: self.session_id,
            ffmpeg_exe_path: ffmpeg_exe.to_string_lossy().into_owned(),
            ffmpeg_sha256: crate::exotic::tools::FFMPEG_EXE_SHA256.to_string(),
            work_dir: work_dir.to_string_lossy().into_owned(),
        };
        match sup.run_request(&req, op_timeout::SESSION_INIT, &|| false) {
            RawOutcome::Success { body, .. } if body.video_session.is_some() => {
                self.supervisor = Some(sup);
                Ok(())
            }
            RawOutcome::Failure(fb) if fb.code == WorkerErrorCode::FfmpegUnavailable => {
                tracing::warn!("VideoSessionInit 报 FfmpegUnavailable(路径/hash/GPL 保险丝)");
                Err(VideoServiceError::FfmpegUnavailable)
            }
            other => {
                tracing::warn!("VideoSessionInit 未成功:{}", raw_outcome_label(&other));
                Err(VideoServiceError::SessionInitFailed)
            }
        }
    }

    fn run_op(
        &mut self,
        op: &VideoOp,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
        on_progress: &mut dyn FnMut(&ProgressBody),
    ) -> RawOutcome {
        let session_id = self.session_id;
        let Some(sup) = self.supervisor.as_mut() else {
            return RawOutcome::Disconnected;
        };
        sup.run_request_observed(
            &op.to_request(session_id),
            timeout,
            total_cap,
            cancelled,
            Some(on_progress),
        )
    }

    fn kill(&mut self) {
        self.supervisor = None; // Drop 兜底 kill + wait,不留孤儿
    }
}

fn raw_outcome_label(o: &RawOutcome) -> &'static str {
    match o {
        RawOutcome::Success { .. } => "success",
        RawOutcome::Failure(_) => "failure",
        RawOutcome::TimedOut => "timeout",
        RawOutcome::Disconnected => "disconnected",
        RawOutcome::Protocol(_) => "protocol_violation",
    }
}
