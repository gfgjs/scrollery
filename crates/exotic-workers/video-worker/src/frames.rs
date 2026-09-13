// crates/exotic-workers/video-worker/src/frames.rs
//! VideoFrames:缩略图后端取帧(design.md §2.3;契约对齐 src-tauri/src/video/mod.rs 的
//! VideoBackend trait 文档)。
//!
//! - Cover:时间戳 ≈ min(1s, 10%×时长) 抽一帧(黑帧规避:过暗则前跳重试),缩到长边
//!   ≤ max_long_edge,编 WebP 单帧回同帧 blob。
//! - Keyframes:n 等分时间轴抽帧,每帧缩到 cell_height(宽按显示比例、由 ffmpeg 自动派生),
//!   横向拼一张雪碧条 WebP + JSON `{cell_width, cell_height, n}`。
//!
//! ffmpeg 默认 `-autorotate` 开启:抽出的帧已按 display matrix 摆正(upright),无需手工旋转。

use std::io::Cursor;
use std::path::Path;

use exotic_protocol::{
    FailureBody, Frame, FrameType, SuccessBody, VideoFramesInfo, VideoFramesMode,
};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

use crate::error::VideoError;
use crate::ffrun::{self, CancelFlag};
use crate::session::VideoSessionState;

/// 平均亮度低于此阈值(0..=255)判为黑帧,前跳重试(简单启发式)。
const DARK_LUMA_THRESHOLD: u8 = 16;
/// Cover 黑帧规避最多尝试的时间戳数。
const COVER_MAX_ATTEMPTS: usize = 4;

/// Cover 首选时间戳(毫秒):min(1s, 10%×时长);时长未知/为 0 → 0(取首帧)。
pub fn cover_timestamp_ms(duration_ms: Option<u64>) -> u64 {
    match duration_ms {
        Some(d) if d > 0 => (d / 10).min(1000),
        _ => 0,
    }
}

/// Cover 黑帧规避的候选时间戳序列(首选 + 递增前跳,步长 = max(5%时长, 1s),不越界)。
/// 时长未知时从 0 起步、按 1s 递增(越界候选在抽帧失败时被 cover() 兜底跳过)。
pub fn cover_retry_timestamps(duration_ms: Option<u64>) -> Vec<u64> {
    let base = cover_timestamp_ms(duration_ms);
    let dur = duration_ms.unwrap_or(0);
    let step = if dur > 0 { (dur / 20).max(1000) } else { 1000 };
    let mut v = Vec::new();
    for i in 0..COVER_MAX_ATTEMPTS as u64 {
        let ts = base + step * i;
        if dur > 0 && ts >= dur {
            break;
        }
        v.push(ts);
    }
    if v.is_empty() {
        v.push(0);
    }
    v
}

/// n 等分时间轴的抽帧时间戳(毫秒):第 i 帧取 (i+0.5)/n × 时长(格中点,避开首尾黑场/片头)。
pub fn keyframe_timestamps_ms(duration_ms: u64, n: u32) -> Vec<u64> {
    if n == 0 {
        return vec![];
    }
    (0..n as u64)
        .map(|i| ((i * 2 + 1) * duration_ms) / (2 * n as u64))
        .collect()
}

/// 帧是否过暗(平均亮度 < 阈值)。Rec.601 亮度近似(整数权重,避免浮点)。
pub fn is_too_dark(img: &RgbaImage) -> bool {
    let (w, h) = (img.width() as u64, img.height() as u64);
    let px = w * h;
    if px == 0 {
        return true;
    }
    let mut sum: u64 = 0;
    for p in img.pixels() {
        let [r, g, b, _] = p.0;
        // luma ≈ 0.299R+0.587G+0.114B,用 /1000 整数权重。
        sum += (299 * r as u64 + 587 * g as u64 + 114 * b as u64) / 1000;
    }
    (sum / px) < DARK_LUMA_THRESHOLD as u64
}

/// 横向拼接雪碧条:canvas 宽 = cell_w × tiles.len、高 = cell_h;逐格覆写(尺寸不符者先缩放)。
pub fn compose_sprite(tiles: &[RgbaImage], cell_w: u32, cell_h: u32) -> RgbaImage {
    let n = tiles.len() as u32;
    let mut canvas = RgbaImage::new(cell_w.max(1) * n.max(1), cell_h.max(1));
    for (i, tile) in tiles.iter().enumerate() {
        let fitted;
        let src = if tile.width() == cell_w && tile.height() == cell_h {
            tile
        } else {
            fitted = image::imageops::resize(
                tile,
                cell_w.max(1),
                cell_h.max(1),
                image::imageops::FilterType::Triangle,
            );
            &fitted
        };
        image::imageops::overlay(&mut canvas, src, (i as u32 * cell_w) as i64, 0);
    }
    canvas
}

/// RGBA → 无损 WebP 字节(与 raw-worker 同系 image 0.25)。
pub fn encode_webp(img: &RgbaImage) -> Result<Vec<u8>, VideoError> {
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut webp))
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| VideoError::Internal(format!("WebP 编码失败:{e:?}")))?;
    Ok(webp)
}

/// 长边缩放目标尺寸(保持比例、不放大、至少 1px);对齐 raw-worker scaled_dims。
fn scaled_dims(w: u32, h: u32, max_long_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if max_long_edge == 0 || long <= max_long_edge {
        return (w.max(1), h.max(1));
    }
    let scale = max_long_edge as f64 / long as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

/// 抽单帧为 PNG 字节:`-ss` 前置快速定位 + `-frames:v 1` + 可选缩放滤镜 + image2pipe。
fn extract_frame_png(
    ffmpeg: &Path,
    source: &str,
    ts_ms: u64,
    vf: Option<&str>,
    cancel: &CancelFlag,
) -> Result<Vec<u8>, VideoError> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-ss".into(),
        format!("{:.3}", ts_ms as f64 / 1000.0),
        "-i".into(),
        source.into(),
        "-frames:v".into(),
        "1".into(),
    ];
    if let Some(f) = vf {
        args.push("-vf".into());
        args.push(f.to_string());
    }
    args.push("-f".into());
    args.push("image2pipe".into());
    args.push("-c:v".into());
    args.push("png".into());
    args.push("pipe:1".into());
    ffrun::run_ffmpeg_capture(ffmpeg, &args, cancel)
}

fn decode_png_rgba(bytes: &[u8]) -> Result<RgbaImage, VideoError> {
    if bytes.is_empty() {
        return Err(VideoError::Malformed("ffmpeg 未产出帧(空输出)".into()));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|e| VideoError::Internal(format!("帧解码失败:{e:?}")))?;
    Ok(img.to_rgba8())
}

/// 处理一次 VideoFrames。失败回 Failure 帧(取帧不落盘,无 tmp 清理)。
pub fn handle_frames(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    mode: VideoFramesMode,
    cancel: &CancelFlag,
) -> Frame {
    let r = match mode {
        VideoFramesMode::Cover { max_long_edge } => {
            cover(state, request_id, source_path, max_long_edge, cancel)
        }
        VideoFramesMode::Keyframes { n, cell_height } => {
            keyframes(state, request_id, source_path, n, cell_height, cancel)
        }
    };
    match r {
        Ok(frame) => frame,
        Err(e) => e.to_failure_frame(request_id),
    }
}

/// 抽封面帧并编 WebP(Cover 与任务化 Thumbnail 共用核心):probe → 黑帧规避抽帧 → 长边缩放
/// → WebP。返回 `(webp, width, height)`;响应体的组装由各调用方按 op 语义补齐(Cover 不填
/// item、Thumbnail 回填 item_id/input_fingerprint)。
fn cover_webp(
    state: &VideoSessionState,
    source_path: &str,
    max_long_edge: u32,
    cancel: &CancelFlag,
) -> Result<(Vec<u8>, u32, u32), VideoError> {
    let probe = crate::probe::run_probe(&state.ffprobe_path, source_path)?;
    let ts_list = cover_retry_timestamps(probe.duration_ms);
    let vf = (max_long_edge > 0).then(|| crate::transcode::scale_filter(max_long_edge));

    let mut chosen: Option<RgbaImage> = None;
    for (i, ts) in ts_list.iter().enumerate() {
        let png =
            match extract_frame_png(&state.ffmpeg_path, source_path, *ts, vf.as_deref(), cancel) {
                Ok(p) => p,
                // 首个时间戳抽帧失败即视为源问题;后续候选越界失败则用已有帧兜底。
                Err(e) if i == 0 => return Err(e),
                Err(_) => break,
            };
        let img = decode_png_rgba(&png)?;
        let dark = is_too_dark(&img);
        chosen = Some(img);
        if !dark {
            break;
        }
    }
    let img = chosen.ok_or_else(|| VideoError::Internal("未能抽出封面帧".into()))?;
    // ml>0 时 ffmpeg 已缩放,此处 scaled_dims 通常恒等;ml==0(原生)不放大。
    let (nw, nh) = scaled_dims(img.width(), img.height(), max_long_edge);
    let out = if (nw, nh) == (img.width(), img.height()) {
        img
    } else {
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
    };
    let (w, h) = (out.width(), out.height());
    let webp = encode_webp(&out)?;
    Ok((webp, w, h))
}

fn cover(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    max_long_edge: u32,
    cancel: &CancelFlag,
) -> Result<Frame, VideoError> {
    let (webp, w, h) = cover_webp(state, source_path, max_long_edge, cancel)?;
    let body = SuccessBody {
        mime: Some("image/webp".into()),
        width: Some(w),
        height: Some(h),
        ..Default::default()
    };
    Ok(Frame::with_blob(FrameType::Success, request_id, &body, webp).unwrap())
}

/// 任务化缩略图 op(exotic 任务化派发,rmvb/vob 收编 D-444③):语义等同 Cover(封面档吸附
/// = `target_long_edge`),但响应体回填 `item_id`/`input_fingerprint`——host `run_thumbnail`
/// 对 Success 与 Failure 均按二者核对,不符判 `invalid_worker_output`(raw-worker 同型)。
#[allow(clippy::too_many_arguments)]
pub fn handle_thumbnail(
    state: &VideoSessionState,
    request_id: u64,
    item_id: i64,
    source_path: &str,
    target_long_edge: u32,
    input_fingerprint: String,
    cancel: &CancelFlag,
) -> Frame {
    match cover_webp(state, source_path, target_long_edge, cancel) {
        Ok((webp, w, h)) => {
            let body = SuccessBody {
                item_id: Some(item_id),
                input_fingerprint: Some(input_fingerprint),
                mime: Some("image/webp".into()),
                width: Some(w),
                height: Some(h),
                ..Default::default()
            };
            Frame::with_blob(FrameType::Success, request_id, &body, webp).unwrap()
        }
        Err(e) => {
            let fail = FailureBody {
                item_id: Some(item_id),
                input_fingerprint: Some(input_fingerprint),
                code: e.code(),
                retryable: e.retryable(),
                message: e.to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
    }
}

fn keyframes(
    state: &VideoSessionState,
    request_id: u64,
    source_path: &str,
    n: u32,
    cell_height: u32,
    cancel: &CancelFlag,
) -> Result<Frame, VideoError> {
    if n == 0 || cell_height == 0 {
        return Err(VideoError::Malformed(
            "keyframes 的 n/cell_height 不得为 0".into(),
        ));
    }
    let probe = crate::probe::run_probe(&state.ffprobe_path, source_path)?;
    let dur = probe
        .duration_ms
        .ok_or_else(|| VideoError::Malformed("源无时长,无法均匀抽帧".into()))?;
    let ts_list = keyframe_timestamps_ms(dur, n);
    // 每帧缩到高度 = cell_height、宽度按比例偶数(-2),ffmpeg 派生显示比例。
    let vf = format!("scale=-2:{cell_height}");

    let mut tiles: Vec<Option<RgbaImage>> = Vec::with_capacity(n as usize);
    for ts in &ts_list {
        match extract_frame_png(&state.ffmpeg_path, source_path, *ts, Some(&vf), cancel) {
            Ok(png) => tiles.push(decode_png_rgba(&png).ok()),
            Err(VideoError::Cancelled) => return Err(VideoError::Cancelled),
            Err(_) => tiles.push(None), // 单帧失败:占黑格,保雪碧条几何
        }
    }
    // 格宽取首个成功帧的宽度(同源等宽);全失败 → 内部错误。
    let cell_w = tiles
        .iter()
        .flatten()
        .map(|t| t.width())
        .next()
        .ok_or_else(|| VideoError::Internal("全部关键帧抽取失败".into()))?;
    let filled: Vec<RgbaImage> = tiles
        .into_iter()
        .map(|t| t.unwrap_or_else(|| RgbaImage::new(cell_w, cell_height)))
        .collect();
    let sprite = compose_sprite(&filled, cell_w, cell_height);
    let webp = encode_webp(&sprite)?;

    // 雪碧条几何走结构化 SuccessBody.video_frames(host 解码后按此切格喂 sprite 组装);
    // 不再重复塞进 metadata(P1:两处曾各写一份,易生漂移)。
    let body = SuccessBody {
        mime: Some("image/webp".into()),
        width: Some(sprite.width()),
        height: Some(sprite.height()),
        video_frames: Some(VideoFramesInfo {
            cell_width: cell_w,
            cell_height,
            n: filled.len() as u32,
        }),
        ..Default::default()
    };
    Ok(Frame::with_blob(FrameType::Success, request_id, &body, webp).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_timestamp_rule() {
        assert_eq!(cover_timestamp_ms(Some(60_000)), 1000); // 10% = 6s → 钳 1s
        assert_eq!(cover_timestamp_ms(Some(5_000)), 500); // 10% = 500ms < 1s
        assert_eq!(cover_timestamp_ms(None), 0);
        assert_eq!(cover_timestamp_ms(Some(0)), 0);
    }

    #[test]
    fn cover_retry_within_duration() {
        let ts = cover_retry_timestamps(Some(60_000));
        assert_eq!(ts[0], 1000);
        assert!(ts.len() >= 2);
        assert!(ts.iter().all(|&t| t < 60_000), "候选不得越界");
        // 短片:候选被时长夹紧。
        let short = cover_retry_timestamps(Some(1_200));
        assert!(short.iter().all(|&t| t < 1_200));
        // 未知时长:从 0 起步、1s 递增(无上界钳制;越界候选由 cover() 抽帧失败兜底跳过)。
        assert_eq!(cover_retry_timestamps(None), vec![0, 1000, 2000, 3000]);
    }

    #[test]
    fn keyframe_timestamps_midpoints() {
        assert_eq!(keyframe_timestamps_ms(1000, 4), vec![125, 375, 625, 875]);
        assert_eq!(keyframe_timestamps_ms(1000, 1), vec![500]);
        assert!(keyframe_timestamps_ms(1000, 0).is_empty());
        assert_eq!(keyframe_timestamps_ms(0, 3), vec![0, 0, 0]);
    }

    #[test]
    fn dark_detection() {
        let black = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]));
        assert!(is_too_dark(&black));
        let white = RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        assert!(!is_too_dark(&white));
        let dim = RgbaImage::from_pixel(4, 4, image::Rgba([10, 10, 10, 255]));
        assert!(is_too_dark(&dim));
    }

    #[test]
    fn sprite_geometry() {
        let tiles = vec![
            RgbaImage::from_pixel(20, 10, image::Rgba([255, 0, 0, 255])),
            RgbaImage::from_pixel(20, 10, image::Rgba([0, 255, 0, 255])),
            RgbaImage::from_pixel(20, 10, image::Rgba([0, 0, 255, 255])),
        ];
        let sprite = compose_sprite(&tiles, 20, 10);
        assert_eq!((sprite.width(), sprite.height()), (60, 10));
        // 第 0 格红、第 1 格绿、第 2 格蓝(左上角像素抽样)。
        assert_eq!(sprite.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(sprite.get_pixel(20, 0).0, [0, 255, 0, 255]);
        assert_eq!(sprite.get_pixel(40, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn sprite_resizes_mismatched_tile() {
        let tiles = vec![RgbaImage::from_pixel(40, 20, image::Rgba([1, 2, 3, 255]))];
        let sprite = compose_sprite(&tiles, 20, 10);
        assert_eq!((sprite.width(), sprite.height()), (20, 10));
    }

    #[test]
    fn webp_roundtrips_dimensions() {
        let img = RgbaImage::from_pixel(8, 6, image::Rgba([100, 150, 200, 255]));
        let webp = encode_webp(&img).unwrap();
        assert!(!webp.is_empty());
        // WebP 魔数 RIFF....WEBP。
        assert_eq!(&webp[0..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
    }

    #[test]
    fn scaled_dims_no_upscale() {
        assert_eq!(scaled_dims(1000, 500, 480), (480, 240));
        assert_eq!(scaled_dims(256, 192, 480), (256, 192));
        assert_eq!(scaled_dims(256, 192, 0), (256, 192));
    }
}
