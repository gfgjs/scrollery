// crates/exotic-workers/video-worker/src/remux.rs
//! VideoRemux:容器改封(design.md §2.3/§5.2)。`-c:v copy` + 音轨 copy 或转 aac,
//! 字幕一律 `-sn` 丢弃,输出 `-movflags +faststart` 的 MP4。产物 out_bytes/时长回执。
//!
//! 参数拼装(`build_remux_args`)是纯函数,单测覆盖 copy/transcode/无音轨各分支;
//! 输出路径经 work_dir 白名单 canonicalize 越界拒(与 enhance-worker 同型)。

use std::io::Write;
use std::path::Path;

use exotic_protocol::{Frame, SuccessBody, VideoOutInfo};

use crate::error::VideoError;
use crate::ffrun::CancelFlag;
use crate::session::VideoSessionState;

/// 拼 remux 的 ffmpeg 参数(纯函数)。`audio_track_index=Some(n)` → 映射 `0:a:n`;
/// `None` 时若源有音轨 → 映射 `0:a:0`,否则不映射音轨(design.md §9.9 无音轨容错)。
pub fn build_remux_args(
    source: &str,
    output: &str,
    audio_transcode: bool,
    audio_track_index: Option<u32>,
    source_has_audio: bool,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        source.into(),
        // 大写 V 排除 attached_pic(封面图)流,只取真正视频轨(probe.rs 同型判据)。
        "-map".into(),
        "0:V:0".into(),
    ];
    let audio_mapped = if let Some(idx) = audio_track_index {
        a.push("-map".into());
        a.push(format!("0:a:{idx}"));
        true
    } else if source_has_audio {
        a.push("-map".into());
        a.push("0:a:0".into());
        true
    } else {
        false
    };
    a.push("-c:v".into());
    a.push("copy".into());
    if audio_mapped {
        a.push("-c:a".into());
        a.push(if audio_transcode { "aac" } else { "copy" }.into());
    }
    a.push("-sn".into());
    a.push("-movflags".into());
    a.push("+faststart".into());
    a.push("-progress".into());
    a.push("pipe:1".into());
    a.push("-nostats".into());
    a.push(output.into());
    a
}

/// 处理一次 VideoRemux:白名单校验(拒即回失败,不落盘不清理)→ 探测源(时长/有无音轨)
/// → 拼参 → 流式跑(Progress stage="remux")→ 产物统计回执。运行期失败/取消清 tmp。
#[allow(clippy::too_many_arguments)]
pub fn handle_remux<W: Write>(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    output_tmp_path: &str,
    audio_transcode: bool,
    audio_track_index: Option<u32>,
    cancel: &CancelFlag,
    writer: &mut W,
) -> Frame {
    // 白名单拒绝分支绝不 cleanup:此刻还没有任何 tmp 产物由本次请求写出
    // (enhance-worker run.rs 先例同型)。cleanup 只对已通过校验的 canonical output 执行。
    let output = match crate::session::resolve_output_path(output_tmp_path, &state.work_dir) {
        Ok(p) => p,
        Err(e) => return e.to_failure_frame(request_id),
    };
    match remux_inner(
        state,
        request_id,
        source_path,
        &output,
        audio_transcode,
        audio_track_index,
        cancel,
        writer,
    ) {
        Ok(frame) => frame,
        Err(e) => {
            crate::cleanup_tmp(&output.to_string_lossy());
            e.to_failure_frame(request_id)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn remux_inner<W: Write>(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    output: &Path,
    audio_transcode: bool,
    audio_track_index: Option<u32>,
    cancel: &CancelFlag,
    writer: &mut W,
) -> Result<Frame, VideoError> {
    let output_str = output.to_string_lossy().into_owned();

    // 探测源:拿时长(算百分比)+ 是否有音轨(无音轨容错)。
    let probe = crate::probe::run_probe(&state.ffprobe_path, source_path)?;
    let source_has_audio = !probe.audio_tracks.is_empty();
    let audio_mapped = audio_track_index.is_some() || source_has_audio;

    let args = build_remux_args(
        source_path,
        &output_str,
        audio_transcode,
        audio_track_index,
        source_has_audio,
    );
    crate::run_streaming_op(
        &state.ffmpeg_path,
        &args,
        probe.duration_ms,
        "remux",
        output,
        request_id,
        cancel,
        writer,
    )?;

    let out_bytes = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    // 完工后对产物本身跑一次 ffprobe 回填**实测**时长(不再抄源时长);产物打不开/损坏
    // 一律按 MalformedInput 处理(remux 完工却产出坏文件属源侧问题,非 worker 内部错误)。
    let out_duration_ms = crate::probe_output_duration_ms(&state.ffprobe_path, output)?;
    let body = SuccessBody {
        video_out: Some(VideoOutInfo {
            out_bytes,
            out_duration_ms,
            video_copied: true,
            audio_copied: audio_mapped && !audio_transcode,
        }),
        ..Default::default()
    };
    Ok(Frame::control(exotic_protocol::FrameType::Success, request_id, &body).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(a: &[String]) -> String {
        a.join(" ")
    }

    #[test]
    fn copy_both_when_no_transcode() {
        let a = build_remux_args("in.mkv", "out.mp4.tmp", false, None, true);
        let s = joined(&a);
        assert!(s.contains("-c:v copy"));
        assert!(s.contains("-c:a copy"));
        assert!(s.contains("-map 0:V:0"));
        assert!(s.contains("-map 0:a:0"));
        assert!(s.contains("-sn"));
        assert!(s.contains("-movflags +faststart"));
        assert!(s.contains("-progress pipe:1"));
        assert!(s.ends_with("out.mp4.tmp"));
    }

    #[test]
    fn transcode_audio_only() {
        let a = build_remux_args("in.mkv", "o.mp4", true, None, true);
        let s = joined(&a);
        assert!(s.contains("-c:v copy"));
        assert!(s.contains("-c:a aac"));
    }

    #[test]
    fn explicit_audio_track_index_mapped() {
        let a = build_remux_args("in.mkv", "o.mp4", false, Some(2), true);
        assert!(joined(&a).contains("-map 0:a:2"));
    }

    #[test]
    fn no_audio_source_omits_audio_map_and_codec() {
        let a = build_remux_args("in.mkv", "o.mp4", false, None, false);
        let s = joined(&a);
        assert!(s.contains("-map 0:V:0"));
        assert!(!s.contains("0:a"), "无音轨源不得映射音轨:{s}");
        assert!(!s.contains("-c:a"), "无音轨源不得设音频编解码:{s}");
    }

    #[test]
    fn index_given_maps_even_if_probe_says_no_audio() {
        // host 显式给了 index → 以 host 为准映射(容错逻辑只对 None 生效)。
        let a = build_remux_args("in.mkv", "o.mp4", false, Some(0), false);
        assert!(joined(&a).contains("-map 0:a:0"));
    }
}
