// crates/exotic-workers/psd-worker/src/decode.rs
//! PSD → WebP 缩略图解码（v3 Part2 §3.5；范围以探针报告 §5 冻结）。
//!
//! 首发只交付 **RGB 8-bit + merged image（raw）**。CMYK/16-bit/PSB/无 merged 等返回稳定错误码
//! `unsupported_variant`（不强行出图）。畸形输入由顶层 `catch_unwind` 兜底（psd 0.3.5 的 `rgba()`
//! 无 `Result`、可能 panic）；raw 像素段截断由解码前 [`raw_image_data_intact`] 自验长度截获
//! （探针风险①，问题3：psd 对截断零填充、不报错），不依赖 psd 行为；Host 侧另用独立 WebP parser
//! 二次验证解码后实际尺寸。

use std::io::Cursor;

use exotic_protocol::WorkerErrorCode;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use psd::{ColorMode, Psd, PsdDepth};

/// 源画布像素上限（宽×高）。超过即 `resource_limit`，避免超大画布 OOM（Host 另有自己的上限兜底）。
/// 100 兆像素 ≈ 10000×10000，对缩略图用途足够宽松。
pub const MAX_SOURCE_PIXELS: u64 = 100_000_000;

/// 解码成功产物：WebP 字节 + 实际输出尺寸（Host 二次校验用）。
#[derive(Debug)]
pub struct DecodedThumb {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Worker 内部解码错误：携带稳定错误码 + 诊断信息（诊断**不**含绝对路径）。
#[derive(Debug)]
pub struct DecodeError {
    pub code: WorkerErrorCode,
    pub message: String,
}

impl DecodeError {
    fn new(code: WorkerErrorCode, message: impl Into<String>) -> Self {
        DecodeError {
            code,
            message: message.into(),
        }
    }
}

/// 把 PSD 字节解为目标长边 `target_long_edge` 的 WebP 缩略图。
///
/// `target_long_edge` 由 Host 传入**吸附后档位**（与指纹一致，R5）。长边 ≤ 档位时不放大、保持原尺寸。
pub fn decode_psd_to_webp(
    bytes: &[u8],
    target_long_edge: u32,
) -> Result<DecodedThumb, DecodeError> {
    // 1. 解析 header。psd 0.3.5 对 PSB(version=2)/错 magic/截断头直接返回 Err → malformed/unsupported。
    let psd = Psd::from_bytes(bytes).map_err(|e| {
        DecodeError::new(WorkerErrorCode::MalformedInput, format!("解析失败：{e:?}"))
    })?;

    // 2. 变体门控：只接受 probe 实测通过的 RGB 8-bit。CMYK/灰度/16-bit 等 → 稳定 unsupported_variant。
    let cm = psd.color_mode();
    if !matches!(cm, ColorMode::Rgb) {
        return Err(DecodeError::new(
            WorkerErrorCode::UnsupportedVariant,
            format!("不支持的色彩模式：{cm:?}（首发仅 RGB）"),
        ));
    }
    let depth = psd.depth();
    if !matches!(depth, PsdDepth::Eight) {
        return Err(DecodeError::new(
            WorkerErrorCode::UnsupportedVariant,
            format!("不支持的位深：{depth:?}（首发仅 8-bit）"),
        ));
    }

    // 3. 画布像素上限（checked，避免溢出与超大画布 OOM）。
    let w = psd.width();
    let h = psd.height();
    let src_pixels = (w as u64)
        .checked_mul(h as u64)
        .ok_or_else(|| DecodeError::new(WorkerErrorCode::ResourceLimit, "尺寸乘积溢出"))?;
    if src_pixels == 0 {
        return Err(DecodeError::new(
            WorkerErrorCode::MalformedInput,
            "退化尺寸（0 像素）",
        ));
    }
    if src_pixels > MAX_SOURCE_PIXELS {
        return Err(DecodeError::new(
            WorkerErrorCode::ResourceLimit,
            format!("画布过大：{w}x{h} > {MAX_SOURCE_PIXELS} 像素"),
        ));
    }

    // 3.5 raw 截断自验（探针风险①，问题3）：psd 0.3.5 对截断的 raw 像素段**零填充**、不报错，
    //      故 §5 的长度自洽校验（零填充后长度正好）也判不出截断 → 会出一张内容错误但结构合法的
    //      WebP，Host 的 dims 二次校验同样无法识别。这里在合成前自验输入字节是否足以覆盖 raw 段
    //      （channels×w×h，8-bit 已门控），不足即判 MalformedInput（落地 Part2 DoD#3）。
    //      仅对 raw 压缩成立；RLE/ZIP 段长不定，仍归已知边界（首发 composite 仅 raw）。
    if !raw_image_data_intact(bytes, w, h)? {
        return Err(DecodeError::new(
            WorkerErrorCode::MalformedInput,
            "raw image data 段截断（字节不足以覆盖 channels×w×h）",
        ));
    }

    // 4. 合成 RGBA。psd 0.3.5 的 rgba() 无 Result、对畸形像素段可能 panic → 包裹（最后防线）。
    let rgba = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| psd.rgba()))
        .map_err(|_| DecodeError::new(WorkerErrorCode::MalformedInput, "rgba() 合成 panic"))?;

    // 5. 长度自洽校验：psd 不严格校验像素段长度（探针风险①）→ 若长度不符直接拒绝，不出错图。
    let expect = src_pixels
        .checked_mul(4)
        .ok_or_else(|| DecodeError::new(WorkerErrorCode::ResourceLimit, "缓冲乘积溢出"))?;
    if rgba.len() as u64 != expect {
        return Err(DecodeError::new(
            WorkerErrorCode::MalformedInput,
            format!("像素长度不符：{} != {expect}", rgba.len()),
        ));
    }

    let img = RgbaImage::from_raw(w, h, rgba).ok_or_else(|| {
        DecodeError::new(WorkerErrorCode::InternalError, "RgbaImage::from_raw None")
    })?;

    // 6. 按长边缩放（保持比例；档位以下不放大）。
    let (nw, nh) = scaled_dims(w, h, target_long_edge);
    let resized = if (nw, nh) == (w, h) {
        img
    } else {
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
    };

    // 7. WebP 无损编码（与主程序同系 image 0.25）。
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut webp))
        .write_image(resized.as_raw(), nw, nh, ExtendedColorType::Rgba8)
        .map_err(|e| {
            DecodeError::new(
                WorkerErrorCode::InternalError,
                format!("WebP 编码失败：{e:?}"),
            )
        })?;

    Ok(DecodedThumb {
        webp,
        width: nw,
        height: nh,
    })
}

/// 校验 raw(uncompressed) image data 段未被截断（问题3）。
///
/// 重新解析 PSD v1 header 的三个 length-prefixed section（color mode data / image resources /
/// layer & mask）定位 image data 起点与 compression；仅当 compression=0(raw) 时要求剩余字节
/// ≥ `channels×w×h`（8-bit）。非 raw 或头部本身不完整 → 返回 `Ok(true)`（交后续 rgba/长度校验，
/// 不在此误判）。`Err` 仅用于尺寸乘积溢出。
fn raw_image_data_intact(bytes: &[u8], w: u32, h: u32) -> Result<bool, DecodeError> {
    const HEADER: usize = 26; // PSD v1 固定头长
    if bytes.len() < HEADER {
        return Ok(true); // 头不全：psd 已成功解析才到此，留给后续校验，不在此截获
    }
    // channels @ offset 12（u16 BE）。
    let channels = u16::from_be_bytes([bytes[12], bytes[13]]) as u64;
    // 跳过三个 length-prefixed section。
    let mut off = HEADER;
    for _ in 0..3 {
        if bytes.len() < off + 4 {
            return Ok(true); // section 长度字段都不全 → 不在此判定
        }
        let len = u32::from_be_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
            as usize;
        off = off.saturating_add(4).saturating_add(len);
    }
    // compression @ off（u16 BE）。
    if bytes.len() < off + 2 {
        return Ok(true);
    }
    let compression = u16::from_be_bytes([bytes[off], bytes[off + 1]]);
    off += 2;
    if compression != 0 {
        return Ok(true); // 非 raw（RLE/ZIP）：段长不定，已知边界
    }
    // raw 8-bit：需要 channels×w×h 字节。
    let need = channels
        .checked_mul(w as u64)
        .and_then(|v| v.checked_mul(h as u64))
        .ok_or_else(|| DecodeError::new(WorkerErrorCode::ResourceLimit, "raw 段尺寸乘积溢出"))?;
    let avail = (bytes.len() - off) as u64;
    Ok(avail >= need)
}

/// 按长边缩放计算目标尺寸（保持比例，至少 1px）。
fn scaled_dims(w: u32, h: u32, target_long_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= target_long_edge || target_long_edge == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = target_long_edge as f64 / long as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成最小合法 RGB 8-bit raw PSD（与探针 make_rgb_psd 同结构，全大端）。
    fn make_rgb_psd(w: u32, h: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"8BPS");
        b.extend_from_slice(&1u16.to_be_bytes()); // version 1
        b.extend_from_slice(&[0u8; 6]); // reserved
        b.extend_from_slice(&3u16.to_be_bytes()); // channels = 3
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&8u16.to_be_bytes()); // depth = 8
        b.extend_from_slice(&3u16.to_be_bytes()); // color mode = RGB
        b.extend_from_slice(&0u32.to_be_bytes()); // color mode data
        b.extend_from_slice(&0u32.to_be_bytes()); // image resources
        b.extend_from_slice(&0u32.to_be_bytes()); // layer & mask
        b.extend_from_slice(&0u16.to_be_bytes()); // compression = raw
        for ch in 0..3u32 {
            for y in 0..h {
                for x in 0..w {
                    b.push(match ch {
                        0 => {
                            if w > 1 {
                                (x * 255 / (w - 1)) as u8
                            } else {
                                200
                            }
                        }
                        1 => {
                            if h > 1 {
                                (y * 255 / (h - 1)) as u8
                            } else {
                                120
                            }
                        }
                        _ => 128,
                    });
                }
            }
        }
        b
    }

    fn webp_magic_ok(b: &[u8]) -> bool {
        b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP"
    }

    #[test]
    fn rgb8_merged_decodes() {
        let psd = make_rgb_psd(256, 192);
        let out = decode_psd_to_webp(&psd, 480).unwrap();
        // 256 长边 < 480 → 不放大。
        assert_eq!((out.width, out.height), (256, 192));
        assert!(webp_magic_ok(&out.webp));
    }

    #[test]
    fn truncated_raw_image_data_is_rejected() {
        // 探针风险①：psd 0.3.5 对截断的 raw 像素段**零填充**到完整长度 → 不 panic、长度自洽、
        // dims 正确，会出一张内容错误但结构合法的 WebP，Host dims 二次校验也判不出。
        // 修复（问题3）：Host 侧 raw 段长度自验（raw_image_data_intact）在合成前截获截断 →
        // MalformedInput，落地 Part2 DoD#3「畸形输入返回结构化错误」。
        let mut psd = make_rgb_psd(64, 64);
        psd.truncate(psd.len() - 50);
        let err = decode_psd_to_webp(&psd, 480).unwrap_err();
        assert_eq!(err.code, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn huge_dimensions_rejected() {
        let mut psd = make_rgb_psd(2, 2);
        psd[14..18].copy_from_slice(&60000u32.to_be_bytes()); // height
        psd[18..22].copy_from_slice(&60000u32.to_be_bytes()); // width
        let err = decode_psd_to_webp(&psd, 480).unwrap_err();
        // 60000×60000 = 3.6e9 > MAX_SOURCE_PIXELS → resource_limit；
        // 或 psd 在像素段长度不符时先报 malformed。两者都可接受（都不出图、不 OOM）。
        assert!(matches!(
            err.code,
            WorkerErrorCode::ResourceLimit | WorkerErrorCode::MalformedInput
        ));
    }
}
