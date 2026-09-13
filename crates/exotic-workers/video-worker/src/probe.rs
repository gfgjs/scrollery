// crates/exotic-workers/video-worker/src/probe.rs
//! VideoProbe:ffprobe `-print_format json -show_format -show_streams` → [`VideoProbeInfo`]
//! 全字段(design.md §2.3)。worker **只报流事实,不做可播性判定**——判定表在 host。
//!
//! 解析(`parse_probe_json`)是纯函数,单测用 fixture JSON 字符串钉死各字段;运行
//! (`run_probe`)只负责起 ffprobe 进程 + 把 stdout 交给解析。

use std::path::Path;

use exotic_protocol::{VideoAudioTrack, VideoProbeInfo};
use serde::Deserialize;

use crate::error::VideoError;
use crate::ffrun;

/// ffprobe 顶层输出(只声明我们用到的字段;serde 忽略其余)。
#[derive(Debug, Deserialize)]
struct FfProbeOutput {
    #[serde(default)]
    streams: Vec<FfStream>,
    format: Option<FfFormat>,
}

#[derive(Debug, Deserialize)]
struct FfFormat {
    format_name: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    bits_per_raw_sample: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    bit_rate: Option<String>,
    channels: Option<u32>,
    color_transfer: Option<String>,
    #[serde(default)]
    tags: std::collections::BTreeMap<String, String>,
    disposition: Option<FfDisposition>,
    #[serde(default)]
    side_data_list: Vec<FfSideData>,
}

#[derive(Debug, Deserialize)]
struct FfDisposition {
    #[serde(default)]
    default: i32,
    /// 1 = 该视频流是容器内嵌的封面图(如 mp3/flac 的 attached picture),非真正画面轨。
    #[serde(default)]
    attached_pic: i32,
}

#[derive(Debug, Deserialize)]
struct FfSideData {
    side_data_type: Option<String>,
    /// Display Matrix 的旋转角(常为负,如竖屏 -90)。
    rotation: Option<f64>,
}

/// 起 ffprobe 探测源文件 → 解析流事实。非零退出走源侧归因(损坏/不可解)。
pub fn run_probe(ffprobe: &Path, source: &str) -> Result<VideoProbeInfo, VideoError> {
    let args = vec![
        "-v".into(),
        "error".into(),
        "-print_format".into(),
        "json".into(),
        "-show_format".into(),
        "-show_streams".into(),
        source.to_string(),
    ];
    let (ok, stdout, stderr) = ffrun::run_output(ffprobe, &args)?;
    if !ok {
        return Err(ffrun::classify_ffmpeg_error(None, &stderr));
    }
    parse_probe_json(&stdout)
}

/// **纯解析**:ffprobe JSON → VideoProbeInfo。无视频流 → Malformed(源不含画面/损坏)。
pub fn parse_probe_json(json: &str) -> Result<VideoProbeInfo, VideoError> {
    let out: FfProbeOutput = serde_json::from_str(json)
        .map_err(|e| VideoError::Malformed(format!("ffprobe JSON 解析失败:{e}")))?;

    let format = out.format.as_ref();
    let container = format
        .and_then(|f| f.format_name.clone())
        .unwrap_or_default();
    let duration_ms = format
        .and_then(|f| f.duration.as_deref())
        .and_then(parse_seconds_to_ms);
    let bitrate = format
        .and_then(|f| f.bit_rate.as_deref())
        .and_then(|s| s.parse::<u64>().ok());

    // 首个「真正」视频流:优先排除 attached_pic(封面图,如 mp3/flac 内嵌 cover)。
    // 若全部视频流都是 attached_pic(理论边界,如纯封面无画面轨),兜底取首个,
    // 保持「有视频流即不判 Malformed」的既有行为不变。
    let is_video = |s: &FfStream| s.codec_type.as_deref() == Some("video");
    let is_attached_pic = |s: &FfStream| {
        s.disposition
            .as_ref()
            .map(|d| d.attached_pic == 1)
            .unwrap_or(false)
    };
    let video = out
        .streams
        .iter()
        .find(|s| is_video(s) && !is_attached_pic(s))
        .or_else(|| out.streams.iter().find(|s| is_video(s)));
    let Some(v) = video else {
        return Err(VideoError::Malformed("源不含视频流".into()));
    };

    let rotation = extract_rotation(v);
    let fps = v
        .avg_frame_rate
        .as_deref()
        .and_then(parse_rational_fps)
        .or_else(|| v.r_frame_rate.as_deref().and_then(parse_rational_fps));
    let bit_depth = v
        .bits_per_raw_sample
        .as_deref()
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|d| *d > 0)
        .or_else(|| v.pix_fmt.as_deref().and_then(infer_bit_depth_from_pix_fmt));

    // 音轨:按音频流出现顺序赋 audio-relative index(与 remux `-map 0:a:N` 对齐)。
    let mut audio_tracks = Vec::new();
    let mut ai: u32 = 0;
    for s in &out.streams {
        if s.codec_type.as_deref() == Some("audio") {
            audio_tracks.push(VideoAudioTrack {
                index: ai,
                codec: s.codec_name.clone().unwrap_or_default(),
                channels: s.channels,
                language: s.tags.get("language").cloned(),
                is_default: s
                    .disposition
                    .as_ref()
                    .map(|d| d.default == 1)
                    .unwrap_or(false),
            });
            ai += 1;
        }
    }
    let has_subtitles = out
        .streams
        .iter()
        .any(|s| s.codec_type.as_deref() == Some("subtitle"));
    let has_hdr_metadata = detect_hdr(&out.streams);

    Ok(VideoProbeInfo {
        container,
        duration_ms,
        width: v.width,
        height: v.height,
        rotation,
        fps,
        bitrate: bitrate.or_else(|| v.bit_rate.as_deref().and_then(|s| s.parse::<u64>().ok())),
        video_codec: v.codec_name.clone().unwrap_or_default(),
        video_profile: v.profile.clone(),
        bit_depth,
        pixel_format: v.pix_fmt.clone(),
        audio_tracks,
        has_subtitles,
        has_hdr_metadata,
    })
}

/// 时长秒串("60.000000")→ 毫秒;`N/A`/畸形 → None。
fn parse_seconds_to_ms(s: &str) -> Option<u64> {
    let secs: f64 = s.trim().parse().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some((secs * 1000.0).round() as u64)
    } else {
        None
    }
}

/// ffprobe 分数帧率("30000/1001")→ fps;分母 0/畸形 → None。
fn parse_rational_fps(s: &str) -> Option<f32> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.trim().parse().ok()?;
    let den: f64 = den.trim().parse().ok()?;
    if den == 0.0 {
        return None;
    }
    let fps = num / den;
    if fps > 0.0 && fps.is_finite() {
        Some(fps as f32)
    } else {
        None
    }
}

/// 显示旋转角(0/90/180/270)。优先 Display Matrix side_data(display = -matrix 角,规整到
/// [0,360)),否则 `rotate` tag(直取,规整)。worker 只报事实,host 据此摆正。
fn extract_rotation(v: &FfStream) -> Option<i32> {
    if let Some(sd) = v
        .side_data_list
        .iter()
        .find(|d| d.rotation.is_some() && matches_display_matrix(d))
    {
        let raw = sd.rotation.unwrap().round() as i32;
        return Some(normalize_rotation(-raw));
    }
    if let Some(tag) = v.tags.get("rotate") {
        if let Ok(raw) = tag.trim().parse::<i32>() {
            return Some(normalize_rotation(raw));
        }
    }
    None
}

fn matches_display_matrix(d: &FfSideData) -> bool {
    d.side_data_type
        .as_deref()
        .map(|t| t.eq_ignore_ascii_case("Display Matrix"))
        .unwrap_or(false)
}

/// 规整任意角度到 {0,90,180,270}(四舍五入到最近 90°,再取模)。
fn normalize_rotation(deg: i32) -> i32 {
    let m = deg.rem_euclid(360);
    (((m + 45) / 90) * 90) % 360
}

/// 从像素格式推位深(bits_per_raw_sample 缺失时兜底)。**白名单/结尾模式**,不再用
/// 子串 `contains`(旧写法对 `nv12` 之类 8 位格式会被 "1"+"2" 误判;凡不落在已知
/// 10/12 位模式即归 8 位):
///   - `p010`/`p012` 前缀系列(NV12 打包高位深变体,如 `p010le`);
///   - `yuv*p10le`/`p10be`/`p12le`/`p12be` 结尾(平面高位深格式,如 `yuv420p10le`);
///   - 其余(含 `nv12`/`yuv420p` 等常见 8 位格式)一律 8。
fn infer_bit_depth_from_pix_fmt(pix_fmt: &str) -> Option<u8> {
    if pix_fmt.starts_with("p012") {
        Some(12)
    } else if pix_fmt.starts_with("p010") {
        Some(10)
    } else if pix_fmt.ends_with("p12le") || pix_fmt.ends_with("p12be") {
        Some(12)
    } else if pix_fmt.ends_with("p10le") || pix_fmt.ends_with("p10be") {
        Some(10)
    } else {
        Some(8)
    }
}

/// HDR 元数据存在性:PQ(smpte2084)/HLG(arib-std-b67)传递函数,或 mastering/content-light
/// side_data 出现即判有(只报存在性,不解析具体值)。
fn detect_hdr(streams: &[FfStream]) -> bool {
    streams.iter().any(|s| {
        let transfer_hdr = s
            .color_transfer
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("smpte2084") || t.eq_ignore_ascii_case("arib-std-b67"))
            .unwrap_or(false);
        let side_hdr = s.side_data_list.iter().any(|d| {
            d.side_data_type
                .as_deref()
                .map(|t| {
                    let t = t.to_ascii_lowercase();
                    t.contains("mastering display") || t.contains("content light")
                })
                .unwrap_or(false)
        });
        transfer_hdr || side_hdr
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MKV_H264_AAC: &str = r#"{
      "streams": [
        {"index":0,"codec_type":"video","codec_name":"h264","profile":"High",
         "width":1920,"height":1080,"pix_fmt":"yuv420p","bits_per_raw_sample":"8",
         "r_frame_rate":"30000/1001","avg_frame_rate":"30000/1001","bit_rate":"3800000"},
        {"index":1,"codec_type":"audio","codec_name":"aac","channels":2,
         "tags":{"language":"eng"},"disposition":{"default":1}}
      ],
      "format":{"format_name":"matroska,webm","duration":"60.000000","bit_rate":"4000000"}
    }"#;

    #[test]
    fn parses_full_facts() {
        let p = parse_probe_json(MKV_H264_AAC).unwrap();
        assert_eq!(p.container, "matroska,webm");
        assert_eq!(p.duration_ms, Some(60_000));
        assert_eq!((p.width, p.height), (Some(1920), Some(1080)));
        assert_eq!(p.video_codec, "h264");
        assert_eq!(p.video_profile.as_deref(), Some("High"));
        assert_eq!(p.bit_depth, Some(8));
        assert_eq!(p.pixel_format.as_deref(), Some("yuv420p"));
        assert_eq!(p.bitrate, Some(4_000_000));
        assert!((p.fps.unwrap() - 29.97).abs() < 0.01);
        assert_eq!(p.audio_tracks.len(), 1);
        assert_eq!(p.audio_tracks[0].index, 0);
        assert_eq!(p.audio_tracks[0].codec, "aac");
        assert_eq!(p.audio_tracks[0].channels, Some(2));
        assert_eq!(p.audio_tracks[0].language.as_deref(), Some("eng"));
        assert!(p.audio_tracks[0].is_default);
        assert!(!p.has_subtitles);
        assert!(!p.has_hdr_metadata);
        assert_eq!(p.rotation, None);
    }

    #[test]
    fn multi_audio_and_subtitle_and_rotation() {
        let json = r#"{
          "streams":[
            {"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,
             "pix_fmt":"yuv420p10le","color_transfer":"smpte2084",
             "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]},
            {"codec_type":"audio","codec_name":"ac3","channels":6},
            {"codec_type":"audio","codec_name":"aac","channels":2,"disposition":{"default":1}},
            {"codec_type":"subtitle","codec_name":"subrip"}
          ],
          "format":{"format_name":"matroska,webm","duration":"12.5"}
        }"#;
        let p = parse_probe_json(json).unwrap();
        assert_eq!(p.video_codec, "hevc");
        assert_eq!(p.bit_depth, Some(10)); // 从 pix_fmt 推
        assert_eq!(p.rotation, Some(90)); // display = -(-90) = 90
        assert!(p.has_hdr_metadata); // smpte2084 = PQ
        assert!(p.has_subtitles);
        assert_eq!(p.audio_tracks.len(), 2);
        assert_eq!(p.audio_tracks[0].index, 0);
        assert_eq!(p.audio_tracks[0].codec, "ac3");
        assert_eq!(p.audio_tracks[1].index, 1);
        assert!(p.audio_tracks[1].is_default);
        assert_eq!(p.duration_ms, Some(12_500));
    }

    #[test]
    fn no_video_stream_is_malformed() {
        let json = r#"{"streams":[{"codec_type":"audio","codec_name":"mp3"}],"format":{}}"#;
        let e = parse_probe_json(json).unwrap_err();
        assert!(matches!(e, VideoError::Malformed(_)));
    }

    #[test]
    fn rotation_normalization() {
        assert_eq!(normalize_rotation(-90), 270);
        assert_eq!(normalize_rotation(90), 90);
        assert_eq!(normalize_rotation(360), 0);
        assert_eq!(normalize_rotation(-270), 90);
        assert_eq!(normalize_rotation(179), 180);
    }

    #[test]
    fn garbage_json_rejected() {
        assert!(parse_probe_json("not json").is_err());
    }

    #[test]
    fn nv12_is_8bit_not_matched_by_substring() {
        // 回归:旧版 contains("1")/contains("2") 写法会把 "nv12" 误判为高位深;
        // 白名单/结尾模式下应稳定落 8 位。
        assert_eq!(infer_bit_depth_from_pix_fmt("nv12"), Some(8));
        assert_eq!(infer_bit_depth_from_pix_fmt("yuv420p"), Some(8));
        assert_eq!(infer_bit_depth_from_pix_fmt("yuv420p10le"), Some(10));
        assert_eq!(infer_bit_depth_from_pix_fmt("p010le"), Some(10));
        assert_eq!(infer_bit_depth_from_pix_fmt("p012le"), Some(12));
        assert_eq!(infer_bit_depth_from_pix_fmt("yuv420p12le"), Some(12));
    }

    #[test]
    fn attached_pic_stream_skipped_for_video_selection() {
        // 封面图(attached_pic=1)排在真正视频轨之前时,首视频流选取应跳过它。
        let json = r#"{
          "streams":[
            {"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,
             "disposition":{"attached_pic":1}},
            {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,
             "pix_fmt":"yuv420p","disposition":{"attached_pic":0}}
          ],
          "format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"10.0"}
        }"#;
        let p = parse_probe_json(json).unwrap();
        assert_eq!(
            p.video_codec, "h264",
            "应跳过 attached_pic 封面图选真正视频轨"
        );
        assert_eq!((p.width, p.height), (Some(1920), Some(1080)));
    }
}
