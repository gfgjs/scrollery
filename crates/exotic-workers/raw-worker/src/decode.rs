// crates/exotic-workers/raw-worker/src/decode.rs
//! RAW → WebP 缩略图解码（RAW 支持线 阶段 C）。
//!
//! **一期范围（D-430）**：仅提取厂商内嵌预览图（JPEG，由 `rsraw::RawImage::extract_thumbs`
//! 取出），转码/缩放为 WebP；不做完整 demosaic。无嵌入预览 / 提取失败 → 稳定错误码
//! `unsupported_variant`（不强行出图）。畸形输入由顶层 `catch_unwind` 兜底——`rsraw::open`
//! 底层是 vendored LibRaw C++，无法排除 FFI 边界内部真实 crash（见 raw-probe 探针结论）。

use std::io::Cursor;

use exotic_protocol::WorkerErrorCode;
use image::{ExtendedColorType, ImageEncoder};

/// 嵌入预览像素上限（宽×高）。超过即 `resource_limit`，避免异常大预览 OOM。
/// 100 兆像素 ≈ 10000×10000，对缩略图用途足够宽松（嵌入预览通常远小于此）。
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

/// 把 RAW 字节里的**嵌入预览**解为目标长边 `target_long_edge` 的 WebP 缩略图。
///
/// `target_long_edge` 由 Host 传入**吸附后档位**（与指纹一致）。长边 ≤ 档位时不放大、保持原尺寸。
pub fn decode_raw_to_webp(
    bytes: &[u8],
    target_long_edge: u32,
) -> Result<DecodedThumb, DecodeError> {
    // 1. 打开 RAW 容器。rsraw::Error 类型未公开导出（私有 err 模块，见 raw-probe 探针注记），
    //    不能具名标注该类型，故用 match 转 Debug 字符串携带，不走 `?` 串联。
    let mut img = match rsraw::RawImage::open(bytes) {
        Ok(img) => img,
        Err(e) => {
            return Err(DecodeError::new(
                WorkerErrorCode::MalformedInput,
                format!("RAW 打开失败：{e:?}"),
            ))
        }
    };

    // 2. 提取厂商内嵌预览（一期仅此，无完整 demosaic，D-430）。
    let thumbs = match img.extract_thumbs() {
        Ok(t) => t,
        Err(e) => {
            return Err(DecodeError::new(
                WorkerErrorCode::UnsupportedVariant,
                format!("无嵌入预览可提取：{e:?}"),
            ))
        }
    };
    if thumbs.is_empty() {
        return Err(DecodeError::new(
            WorkerErrorCode::UnsupportedVariant,
            "RAW 未携带嵌入预览（一期不做完整 demosaic）",
        ));
    }

    // 3. 取最大（像素面积最大）一张 **JPEG** 预览：非 JPEG 容器（Bitmap/Bitmap16/Layer/
    //    Rollei/H265）`image::load_from_memory` 判不出会必然失败，若不先过滤会因为它
    //    面积最大被选中，进而丢弃文件里其实可用的较小 JPEG 预览。
    let best = thumbs
        .iter()
        .filter(|t| t.format == rsraw::ThumbFormat::Jpeg)
        .max_by_key(|t| (t.width as u64).saturating_mul(t.height as u64))
        .ok_or_else(|| {
            DecodeError::new(
                WorkerErrorCode::UnsupportedVariant,
                "RAW 未携带 JPEG 嵌入预览（一期不做完整 demosaic）",
            )
        })?;

    // 4. 预览像素上限（checked，避免溢出与异常大预览 OOM）。
    let src_pixels = (best.width as u64)
        .checked_mul(best.height as u64)
        .ok_or_else(|| DecodeError::new(WorkerErrorCode::ResourceLimit, "预览尺寸乘积溢出"))?;
    if src_pixels == 0 {
        return Err(DecodeError::new(
            WorkerErrorCode::MalformedInput,
            "嵌入预览退化尺寸（0 像素）",
        ));
    }
    if src_pixels > MAX_SOURCE_PIXELS {
        return Err(DecodeError::new(
            WorkerErrorCode::ResourceLimit,
            format!(
                "嵌入预览过大：{}x{} > {MAX_SOURCE_PIXELS} 像素",
                best.width, best.height
            ),
        ));
    }

    // 5. 解码预览字节。第 4 步的像素门只 gate 了 LibRaw **上报**的 twidth/theight 元数据；
    //    畸形文件可能上报很小的尺寸而 JPEG SOF 头里实际尺寸巨大，`load_from_memory` 默认无
    //    像素上限，会先整张 `to_rgba8()` 物化再暴露问题——分配阶段即可能 OOM/abort，
    //    catch_unwind 拦不住。改走 `ImageReader` + `Limits`，让 decoder 用**实际解码尺寸**
    //    在分配前做强限（`set_limits` 内部即 `check_dimensions`），超限直接返回
    //    `ImageError::Limits` 而非物化后才失败。
    let side_limit = (MAX_SOURCE_PIXELS as f64).sqrt() as u32;
    let mut reader = image::ImageReader::new(Cursor::new(best.data.as_slice()))
        .with_guessed_format()
        .map_err(|e| {
            DecodeError::new(
                WorkerErrorCode::MalformedInput,
                format!("嵌入预览格式识别失败：{e}"),
            )
        })?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(side_limit);
    limits.max_image_height = Some(side_limit);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| {
        let code = if matches!(e, image::ImageError::Limits(_)) {
            WorkerErrorCode::ResourceLimit
        } else {
            WorkerErrorCode::MalformedInput
        };
        DecodeError::new(code, format!("嵌入预览解码失败：{e:?}"))
    })?;
    let rgba = decoded.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return Err(DecodeError::new(
            WorkerErrorCode::MalformedInput,
            "解码后预览退化尺寸（0 像素）",
        ));
    }

    // 6. 按长边缩放（保持比例；档位以下不放大）。
    let (nw, nh) = scaled_dims(w, h, target_long_edge);
    let resized = if (nw, nh) == (w, h) {
        rgba
    } else {
        image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Lanczos3)
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

    /// 无真实 RAW 样张仓内可用（RAW 无统一简单容器可手写合成，同 raw-probe 探针注记）；
    /// 单测只覆盖不依赖真实 RAW 解码路径的纯逻辑（畸形输入 / 缩放计算），真实解码路径
    /// 覆盖需真实授权样本，另见 raw-probe 的 RAW_PROBE_SAMPLES 环境目录机制。
    #[test]
    fn malformed_garbage_rejected_no_panic() {
        for bytes in [vec![], vec![0xABu8; 16], vec![0x5Au8; 4096]] {
            let err = decode_raw_to_webp(&bytes, 480).unwrap_err();
            assert!(matches!(
                err.code,
                WorkerErrorCode::MalformedInput | WorkerErrorCode::UnsupportedVariant
            ));
        }
    }

    #[test]
    fn scaled_dims_downscales_by_long_edge() {
        assert_eq!(scaled_dims(1000, 500, 480), (480, 240));
    }

    #[test]
    fn scaled_dims_no_upscale_below_target() {
        assert_eq!(scaled_dims(256, 192, 480), (256, 192));
    }
}
