// crates/exotic-workers/video-worker/src/transcode.rs
//! VideoTranscode:一次性全转码 H.264/AAC MP4(design.md §2.3/§3.2)。
//!
//! encoder_ladder 由 host 下发(如 `["h264_nvenc","h264_qsv","h264_amf","h264_mf"]`),
//! worker 逐个试起、首个成功者用之;失败判定 = ffmpeg 非零退出(编码器不可用),重试
//! 下一枚;源侧终态(损坏/不可解)则立即中止阶梯(换编码器无济于事)。
//!
//! 率控按 encoder 择用:nvenc/qsv/amf 支持 CRF 类恒定质量参数;h264_mf(Media Foundation,
//! Windows 10+ 恒在,专利责任落 OS 层,§3.2)只吃固定码率 `-b:v`。

use std::io::Write;
use std::path::Path;

use exotic_protocol::{Frame, FrameType, SuccessBody, VideoOutInfo};

use crate::error::VideoError;
use crate::ffrun::CancelFlag;
use crate::session::VideoSessionState;

/// h264_mf 无 CRF 率控时的兜底码率(kbps)。
const MF_DEFAULT_BITRATE_KBPS: u32 = 6000;

/// 按 encoder 名选率控参数(纯函数)。crf/bitrate 二选一由 host 下发;h264_mf 恒用码率。
pub fn rate_control_args(encoder: &str, crf: Option<u8>, bitrate_kbps: Option<u32>) -> Vec<String> {
    let bv = |kbps: u32| vec!["-b:v".to_string(), format!("{kbps}k")];
    match encoder {
        "h264_nvenc" => match (crf, bitrate_kbps) {
            (Some(c), _) => vec!["-rc".into(), "vbr".into(), "-cq".into(), c.to_string()],
            (None, Some(k)) => bv(k),
            (None, None) => vec![],
        },
        "h264_qsv" => match (crf, bitrate_kbps) {
            (Some(c), _) => vec!["-global_quality".into(), c.to_string()],
            (None, Some(k)) => bv(k),
            (None, None) => vec![],
        },
        "h264_amf" => match (crf, bitrate_kbps) {
            (Some(c), _) => vec![
                "-rc".into(),
                "cqp".into(),
                "-qp_i".into(),
                c.to_string(),
                "-qp_p".into(),
                c.to_string(),
                "-qp_b".into(),
                c.to_string(),
            ],
            (None, Some(k)) => bv(k),
            (None, None) => vec![],
        },
        // h264_mf 不支持 CRF:恒用码率(bitrate 缺则兜底默认),专利责任落 OS 层。
        "h264_mf" => bv(bitrate_kbps.unwrap_or(MF_DEFAULT_BITRATE_KBPS)),
        // 未知/软件编码器(理论兜底):CRF 优先。
        _ => match (crf, bitrate_kbps) {
            (Some(c), _) => vec!["-crf".into(), c.to_string()],
            (None, Some(k)) => bv(k),
            (None, None) => vec![],
        },
    }
}

/// 长边限制的 scale 滤镜:缩到长边 ≤ ml、保持宽高比、偶数对齐,且不放大
/// (box=min(源边,ml) + force_original_aspect_ratio=decrease)。滤镜表达式内逗号以 `\,` 转义。
pub fn scale_filter(ml: u32) -> String {
    format!(
        "scale=w=min(iw\\,{ml}):h=min(ih\\,{ml}):force_original_aspect_ratio=decrease:force_divisible_by=2"
    )
}

/// 拼 transcode 的 ffmpeg 参数(纯函数)。音轨恒转 aac,用可选映射 `0:a:N?`(无音轨源不报错)。
#[allow(clippy::too_many_arguments)]
pub fn build_transcode_args(
    source: &str,
    output: &str,
    encoder: &str,
    crf: Option<u8>,
    bitrate_kbps: Option<u32>,
    max_long_edge: Option<u32>,
    audio_track_index: Option<u32>,
    hw_decode: bool,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-nostdin".into(), "-y".into()];
    if hw_decode {
        a.push("-hwaccel".into());
        a.push("auto".into());
    }
    a.push("-i".into());
    a.push(source.into());
    // 大写 V 排除 attached_pic(封面图)流,只取真正视频轨(probe.rs 同型判据)。
    a.push("-map".into());
    a.push("0:V:0".into());
    a.push("-map".into());
    a.push(format!("0:a:{}?", audio_track_index.unwrap_or(0)));
    a.push("-c:v".into());
    a.push(encoder.into());
    a.extend(rate_control_args(encoder, crf, bitrate_kbps));
    // max_long_edge=None 时仍恒挂偶数对齐 scale(奇数尺寸源在多数硬件编码器上会全阶梯
    // 空转失败——编码器要求宽高皆偶数,§9.9 型加固)。
    a.push("-vf".into());
    a.push(match max_long_edge {
        Some(ml) => scale_filter(ml),
        None => "scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string(),
    });
    a.push("-c:a".into());
    a.push("aac".into());
    a.push("-sn".into());
    a.push("-movflags".into());
    a.push("+faststart".into());
    a.push("-progress".into());
    a.push("pipe:1".into());
    a.push("-nostats".into());
    a.push(output.into());
    a
}

/// 编码器阶梯驱动(纯逻辑,mock 可测):逐枚试 `try_one`,首个 Ok 即返回(用之);
/// 源侧终态错误立即中止;否则记为「本枚失败」试下一枚,全败返回最后一个错误。
pub fn run_encoder_ladder<T, F>(
    ladder: &[String],
    mut try_one: F,
) -> Result<(String, T), VideoError>
where
    F: FnMut(&str) -> Result<T, VideoError>,
{
    if ladder.is_empty() {
        return Err(VideoError::Internal("编码器阶梯为空".into()));
    }
    let mut last: Option<VideoError> = None;
    for enc in ladder {
        match try_one(enc) {
            Ok(v) => return Ok((enc.clone(), v)),
            Err(e) if e.is_source_terminal() => return Err(e),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| VideoError::Internal("所有编码器均失败".into())))
}

/// 处理一次 VideoTranscode:白名单校验(拒即回失败,不落盘不清理)→ 探测源(时长)
/// → 逐枚编码器试转(Progress stage="transcode")→ 首成功者产物统计回执。运行期失败/
/// 取消清 tmp。
#[allow(clippy::too_many_arguments)]
pub fn handle_transcode<W: Write>(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    output_tmp_path: &str,
    encoder_ladder: &[String],
    crf: Option<u8>,
    bitrate_kbps: Option<u32>,
    max_long_edge: Option<u32>,
    audio_track_index: Option<u32>,
    hw_decode: bool,
    cancel: &CancelFlag,
    writer: &mut W,
) -> Frame {
    // 白名单拒绝分支绝不 cleanup:此刻还没有任何 tmp 产物由本次请求写出
    // (enhance-worker run.rs 先例同型)。cleanup 只对已通过校验的 canonical output 执行。
    let output = match crate::session::resolve_output_path(output_tmp_path, &state.work_dir) {
        Ok(p) => p,
        Err(e) => return e.to_failure_frame(request_id),
    };
    match transcode_inner(
        state,
        request_id,
        source_path,
        &output,
        encoder_ladder,
        crf,
        bitrate_kbps,
        max_long_edge,
        audio_track_index,
        hw_decode,
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
fn transcode_inner<W: Write>(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    output: &Path,
    encoder_ladder: &[String],
    crf: Option<u8>,
    bitrate_kbps: Option<u32>,
    max_long_edge: Option<u32>,
    audio_track_index: Option<u32>,
    hw_decode: bool,
    cancel: &CancelFlag,
    writer: &mut W,
) -> Result<Frame, VideoError> {
    let output_str = output.to_string_lossy().into_owned();
    let probe = crate::probe::run_probe(&state.ffprobe_path, source_path)?;
    let duration_ms = probe.duration_ms;

    let (encoder, _) = run_encoder_ladder(encoder_ladder, |enc| {
        crate::log_info(format!("VideoTranscode 试编码器:{enc}"));
        let args = build_transcode_args(
            source_path,
            &output_str,
            enc,
            crf,
            bitrate_kbps,
            max_long_edge,
            audio_track_index,
            hw_decode,
        );
        crate::run_streaming_op(
            &state.ffmpeg_path,
            &args,
            duration_ms,
            "transcode",
            output,
            request_id,
            cancel,
            writer,
        )
    })?;
    crate::log_info(format!("VideoTranscode 采用编码器:{encoder}"));

    let out_bytes = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    // 完工后对产物本身跑一次 ffprobe 回填**实测**时长(不再抄源时长);产物打不开/损坏
    // 一律按 MalformedInput 处理(转码完工却产出坏文件属源侧问题,非 worker 内部错误)。
    let out_duration_ms = crate::probe_output_duration_ms(&state.ffprobe_path, output)?;
    let body = SuccessBody {
        video_out: Some(VideoOutInfo {
            out_bytes,
            out_duration_ms,
            video_copied: false,
            audio_copied: false,
        }),
        ..Default::default()
    };
    Ok(Frame::control(FrameType::Success, request_id, &body).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvenc_crf_uses_cq() {
        assert_eq!(
            rate_control_args("h264_nvenc", Some(23), None),
            vec!["-rc", "vbr", "-cq", "23"]
        );
    }

    #[test]
    fn qsv_crf_uses_global_quality() {
        assert_eq!(
            rate_control_args("h264_qsv", Some(24), None),
            vec!["-global_quality", "24"]
        );
    }

    #[test]
    fn amf_crf_uses_cqp() {
        assert_eq!(
            rate_control_args("h264_amf", Some(22), None),
            vec!["-rc", "cqp", "-qp_i", "22", "-qp_p", "22", "-qp_b", "22"]
        );
    }

    #[test]
    fn mf_always_bitrate_even_with_crf() {
        // MF 不吃 CRF:给了 crf 也回落码率(bitrate 缺→默认)。
        assert_eq!(
            rate_control_args("h264_mf", Some(23), None),
            vec!["-b:v".to_string(), format!("{MF_DEFAULT_BITRATE_KBPS}k")]
        );
        assert_eq!(
            rate_control_args("h264_mf", None, Some(4000)),
            vec!["-b:v", "4000k"]
        );
    }

    #[test]
    fn bitrate_path_when_no_crf() {
        assert_eq!(
            rate_control_args("h264_nvenc", None, Some(5000)),
            vec!["-b:v", "5000k"]
        );
    }

    #[test]
    fn transcode_args_shape() {
        let a = build_transcode_args(
            "in.mkv",
            "o.mp4",
            "h264_nvenc",
            Some(23),
            None,
            Some(1280),
            None,
            false,
        );
        let s = a.join(" ");
        assert!(s.contains("-c:v h264_nvenc"));
        assert!(s.contains("-cq 23"));
        assert!(s.contains("-map 0:a:0?"));
        assert!(s.contains("-vf scale=w=min(iw"));
        assert!(s.contains("-c:a aac"));
        assert!(s.contains("-movflags +faststart"));
        assert!(!s.contains("-hwaccel"));
    }

    #[test]
    fn transcode_args_hwdecode_and_fallback_even_scale() {
        // max_long_edge=None 时仍恒挂偶数对齐 scale(奇数尺寸源防全阶梯空转失败,D-fallback)。
        let a = build_transcode_args(
            "in.mkv",
            "o.mp4",
            "h264_mf",
            None,
            None,
            None,
            Some(1),
            true,
        );
        let s = a.join(" ");
        assert!(s.contains("-hwaccel auto"));
        assert!(s.contains("-map 0:a:1?"));
        assert!(
            s.contains("-vf scale=trunc(iw/2)*2:trunc(ih/2)*2"),
            "无 max_long_edge 仍应有偶数对齐兜底 scale:{s}"
        );
    }

    #[test]
    fn ladder_picks_first_success() {
        let ladder = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut tried = Vec::new();
        let r = run_encoder_ladder(&ladder, |e| {
            tried.push(e.to_string());
            if e == "c" {
                Ok(())
            } else {
                Err(VideoError::Internal("编码器不可用".into()))
            }
        });
        assert_eq!(r.unwrap().0, "c");
        assert_eq!(tried, vec!["a", "b", "c"]);
    }

    #[test]
    fn ladder_aborts_on_source_terminal() {
        let ladder = vec!["a".to_string(), "b".to_string()];
        let mut tried = Vec::new();
        let r = run_encoder_ladder(&ladder, |e| {
            tried.push(e.to_string());
            Err::<(), _>(VideoError::Malformed("坏源".into()))
        });
        assert!(matches!(r, Err(VideoError::Malformed(_))));
        assert_eq!(tried, vec!["a"], "源侧终态应止于首枚,不试后续");
    }

    #[test]
    fn ladder_all_fail_returns_last() {
        let ladder = vec!["a".to_string(), "b".to_string()];
        let r = run_encoder_ladder(&ladder, |_| Err::<(), _>(VideoError::Internal("x".into())));
        assert!(matches!(r, Err(VideoError::Internal(_))));
    }

    #[test]
    fn empty_ladder_errors() {
        let r = run_encoder_ladder::<(), _>(&[], |_| Ok(()));
        assert!(matches!(r, Err(VideoError::Internal(_))));
    }
}
