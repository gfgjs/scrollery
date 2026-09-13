// crates/exotic-workers/video-worker/src/ffrun.rs
//! ffmpeg/ffprobe 子进程执行层:spawn + Job Object 纳管 + stdout(-progress 流 / 二进制
//! 捕获)+ stderr 尾部收集 + 退出码归因(design.md §2.3 错误映射 / §9.10 / §11)。
//!
//! stderr 明细只进 worker 侧日志(WorkerLogLine),**不进** `FailureBody.message`(协议红线)。

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::VideoError;
use crate::job::assign_current_job;
use crate::progress::{FinalizeAction, FinalizeMonitor, ProgressParser};

/// 取消旗标(host kill / 取消轮询共用);置位后运行中的 ffmpeg 被 kill、tmp 清理。
pub type CancelFlag = Arc<AtomicBool>;

/// stderr 尾部保留行数(归因足够,不占内存)。
const STDERR_TAIL_LINES: usize = 64;

/// finalize 态轮询周期(design.md V4 方案 c):`-progress` 无新行时每拍检查子进程存活并
/// 驱动 [`FinalizeMonitor`](心跳 ≤2s 由其内部节流保证;取消检测延迟亦以本周期为上界)。
/// [`FinalizeMonitor`] 自身要求连续 2 拍(2×本周期)无 progress 才真正进入 finalize 态,
/// 防正常编码负载抖动(单拍 >1s 无新行)被误判成 finalize 而闪烁 stage。
const FINALIZE_POLL: Duration = Duration::from_secs(1);

/// [`run_ffmpeg_streaming`] 回报给调用方的事件。
pub enum StreamEvent {
    /// `-progress` 位置推进(毫秒)——正常编码阶段。
    Progress(u64),
    /// finalize 心跳:`-progress` 停更但 ffmpeg 仍在写 trailer(moov 搬移静默窗),
    /// `out_bytes` = 产物当前字节数(调用方发 `stage="finalize"` Progress 帧,detail 携之)。
    Finalize { out_bytes: u64 },
}

fn is_cancelled(cancel: &CancelFlag) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// 通用 spawn:stdin=null(`-nostdin` 再加一道保险)、stdout/stderr 按需 piped;spawn 后即
/// 纳入进程级 kill-on-close job(§9.10)。
fn spawn(ffmpeg: &Path, args: &[String], stdout: Stdio) -> Result<Child, VideoError> {
    let child = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| VideoError::Io(format!("ffmpeg spawn 失败:{}", e.kind())))?;
    assign_current_job(&child);
    Ok(child)
}

/// 起一个后台线程把 child.stderr 逐行读入尾部环形缓冲(返回句柄 + 共享缓冲)。
fn drain_stderr(child: &mut Child) -> (std::thread::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
    let buf = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr = child.stderr.take();
    let sink = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        if let Some(se) = stderr {
            let reader = BufReader::new(se);
            for line in reader.lines().map_while(Result::ok) {
                let mut g = sink.lock().unwrap();
                g.push(line);
                if g.len() > STDERR_TAIL_LINES {
                    let excess = g.len() - STDERR_TAIL_LINES;
                    g.drain(0..excess);
                }
            }
        }
    });
    (handle, buf)
}

fn join_tail(handle: std::thread::JoinHandle<()>, buf: Arc<Mutex<Vec<String>>>) -> String {
    let _ = handle.join();
    buf.lock().unwrap().join("\n")
}

/// 流式运行(remux/transcode):读 stdout 的 `-progress` 行,位置推进即回
/// [`StreamEvent::Progress`];`-progress` 停更而进程仍存活(av_write_trailer / moov 搬移
/// 静默窗)则进入 finalize 态,每 ≤2s 回 [`StreamEvent::Finalize`] 心跳,直至流恢复、进程
/// 退出、或 finalize 自持预算耗尽(kill + InternalError,V4 裁决=方案 c)。取消命中即 kill,
/// 清理由调用方负责。成功 → Ok;非零 → 归因错误。
///
/// `output`:产物路径(可选);finalize 心跳的 `out_bytes` 与预算据其 metadata 现算。
/// `-progress` 逐行解析下沉到独立读取线程 → 主循环据 `recv_timeout` 心跳/存活/取消三判,
/// 不再阻塞在 `lines()` 上被 host 静默限时误杀(方案 c 的核心)。
pub fn run_ffmpeg_streaming(
    ffmpeg: &Path,
    args: &[String],
    output: Option<&Path>,
    cancel: &CancelFlag,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<(), VideoError> {
    let mut child = spawn(ffmpeg, args, Stdio::piped())?;
    let (stderr_handle, stderr_buf) = drain_stderr(&mut child);

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| VideoError::Internal("无法获取 ffmpeg stdout".into()))?;
    // 读取线程:逐行解析 `-progress`,位置推进即经 channel 送主循环;EOF/管道断即 drop tx
    // → 主循环 recv 得 Disconnected。位置不做去重(重复位置无害,节流在调用方)。
    let (pos_tx, pos_rx) = mpsc::channel::<u64>();
    let reader = std::thread::spawn(move || {
        let mut parser = ProgressParser::default();
        let r = BufReader::new(stdout);
        for line in r.lines() {
            let Ok(line) = line else { break };
            if let Some(pos) = parser.feed_line(&line) {
                if pos_tx.send(pos).is_err() {
                    break; // 主循环已退
                }
            }
        }
    });

    let mut finalize = FinalizeMonitor::default();
    loop {
        if is_cancelled(cancel) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = join_tail(stderr_handle, stderr_buf);
            return Err(VideoError::Cancelled);
        }
        match pos_rx.recv_timeout(FINALIZE_POLL) {
            Ok(pos) => {
                // 收到真实位置:退出 finalize 态(正常编码进行中)并回报进度。
                finalize.reset();
                on_event(StreamEvent::Progress(pos));
            }
            // stdout EOF(reader 线程结束)→ 跳出,统一 wait 归因。
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => match child.try_wait() {
                // 进程已退但 reader 尚未 EOF:跳出统一 wait 归因(reader 随即 EOF)。
                Ok(Some(_)) => break,
                // 存活但无新 progress:trailer/moov 静默窗 → finalize 心跳 / 越预算 kill。
                Ok(None) => {
                    let out_bytes = output
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len())
                        .unwrap_or(0);
                    match finalize.on_tick(Instant::now(), out_bytes) {
                        FinalizeAction::Heartbeat => on_event(StreamEvent::Finalize { out_bytes }),
                        FinalizeAction::Wait => {}
                        FinalizeAction::Kill => {
                            let _ = child.kill();
                            let _ = child.wait();
                            let _ = reader.join();
                            let _ = join_tail(stderr_handle, stderr_buf);
                            return Err(VideoError::Internal(
                                "finalize 超预算(trailer 静默窗)".into(),
                            ));
                        }
                    }
                }
                Err(_) => break,
            },
        }
    }

    let _ = reader.join();
    let status = child
        .wait()
        .map_err(|e| VideoError::Io(format!("ffmpeg wait 失败:{}", e.kind())))?;
    let tail = join_tail(stderr_handle, stderr_buf);

    if is_cancelled(cancel) {
        return Err(VideoError::Cancelled);
    }
    finish(status, &tail)
}

/// 捕获式运行(frames 取帧):把 stdout 整段读为二进制(PNG 字节),成功返回;非零归因。
/// 取帧命令短,取消只在读毕后判一次(不在 blocking read 中断)。
pub fn run_ffmpeg_capture(
    ffmpeg: &Path,
    args: &[String],
    cancel: &CancelFlag,
) -> Result<Vec<u8>, VideoError> {
    let mut child = spawn(ffmpeg, args, Stdio::piped())?;
    let (stderr_handle, stderr_buf) = drain_stderr(&mut child);

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| VideoError::Internal("无法获取 ffmpeg stdout".into()))?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|e| VideoError::Io(format!("读取 ffmpeg 输出失败:{}", e.kind())))?;

    let status = child
        .wait()
        .map_err(|e| VideoError::Io(format!("ffmpeg wait 失败:{}", e.kind())))?;
    let tail = join_tail(stderr_handle, stderr_buf);

    if is_cancelled(cancel) {
        return Err(VideoError::Cancelled);
    }
    finish(status, &tail)?;
    Ok(bytes)
}

/// 一次性运行(ffmpeg -version / ffprobe):等进程结束,返回 (退出码成功?, stdout, stderr)。
/// 不做归因(调用方按自身语义解析),但仍纳入 job。
pub fn run_output(ffmpeg: &Path, args: &[String]) -> Result<(bool, String, String), VideoError> {
    let child = spawn(ffmpeg, args, Stdio::piped())?;
    let out = child
        .wait_with_output()
        .map_err(|e| VideoError::Io(format!("ffmpeg wait 失败:{}", e.kind())))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// 退出码 → 结果:成功 Ok;非零调 [`classify_ffmpeg_error`](并把 stderr 尾记 worker 日志)。
fn finish(status: ExitStatus, stderr_tail: &str) -> Result<(), VideoError> {
    if status.success() {
        return Ok(());
    }
    let err = classify_ffmpeg_error(status.code(), stderr_tail);
    // stderr 明细进 worker 日志行(不进 FailureBody.message)。
    crate::log_warn(format!(
        "ffmpeg 非零退出 code={:?} → {} | stderr_tail={}",
        status.code(),
        err.code().as_str(),
        stderr_tail.replace('\n', " ⏎ ")
    ));
    Err(err)
}

/// ffmpeg 非零退出的稳定归因(design.md §2.3):盘满→ResourceLimit、源损坏/参数解析不出→
/// Malformed、输入 codec 不可解→Unsupported(source terminal,阶梯不再试下一枚)、
/// 其余→Internal(retryable;含编码器不可用——阶梯据此 fallthrough 下一枚)。
///
/// **纯函数**(仅看 exit_code + stderr 文本),单测覆盖各分支。
pub fn classify_ffmpeg_error(exit_code: Option<i32>, stderr: &str) -> VideoError {
    let s = stderr.to_ascii_lowercase();
    let has = |needle: &str| s.contains(needle);

    // 1) 磁盘空间。
    if has("no space left") || has("enospc") || has("errno 28") {
        return VideoError::ResourceLimit("磁盘空间不足".into());
    }
    // 2) 源缺失/占用(IO,retryable)。
    if has("no such file or directory") || has("permission denied") {
        return VideoError::Io("源文件不可达".into());
    }
    // 3) 输入 codec 不可解(换编码器无济于事 → source terminal)。
    if (has("decoder") && has("not found"))
        || has("unsupported codec")
        || has("codec not currently supported")
    {
        return VideoError::Unsupported("输入编码不受支持(无可用解码器)".into());
    }
    // 4) 源损坏/畸形(参数解析不出/非法数据/容器头缺失)。
    if has("invalid data found")
        || has("moov atom not found")
        || has("could not find codec parameters")
        || has("error opening input")
        || has("invalid nal")
    {
        return VideoError::Malformed("源文件损坏或格式非法".into());
    }
    // 5) 兜底:非零无法归因(含编码器不可用/初始化失败)→ Internal(retryable)。
    VideoError::Internal(format!("ffmpeg 非零退出(code={exit_code:?})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use exotic_protocol::WorkerErrorCode;

    #[test]
    fn classify_disk_full() {
        assert_eq!(
            classify_ffmpeg_error(
                Some(1),
                "av_interleaved_write_frame(): No space left on device"
            )
            .code(),
            WorkerErrorCode::ResourceLimit
        );
    }

    #[test]
    fn classify_missing_source_is_io() {
        assert_eq!(
            classify_ffmpeg_error(Some(1), "x.mkv: No such file or directory").code(),
            WorkerErrorCode::IoError
        );
    }

    #[test]
    fn classify_undecodable_is_unsupported_and_source_terminal() {
        let e = classify_ffmpeg_error(Some(1), "Decoder (codec rv40) not found for input stream");
        assert_eq!(e.code(), WorkerErrorCode::UnsupportedVariant);
        assert!(e.is_source_terminal(), "输入不可解应中止编码器阶梯");
    }

    #[test]
    fn classify_corrupt_is_malformed_and_source_terminal() {
        let e = classify_ffmpeg_error(Some(1), "Invalid data found when processing input");
        assert_eq!(e.code(), WorkerErrorCode::MalformedInput);
        assert!(e.is_source_terminal());
    }

    #[test]
    fn classify_encoder_unavailable_falls_through_to_internal() {
        // 编码器不可用归 Internal(非 source terminal)→ 阶梯继续试下一枚。
        let e = classify_ffmpeg_error(Some(1), "Unknown encoder 'h264_nvenc'");
        assert_eq!(e.code(), WorkerErrorCode::InternalError);
        assert!(!e.is_source_terminal());
        let e2 = classify_ffmpeg_error(
            Some(1),
            "[h264_nvenc] Cannot load nvcuda.dll\nError initializing output stream",
        );
        assert_eq!(e2.code(), WorkerErrorCode::InternalError);
        assert!(!e2.is_source_terminal());
    }
}
