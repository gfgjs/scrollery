//! 编辑保存的元数据策略(方案 C §4,P0 spike)。
//!
//! **spike 结论**(详见 `docs/worklogs/2026-07-19-图片简单编辑施工/findings.md`):
//! - JPEG/PNG 的 ICC/EXIF 读写完全由 `image 0.25.10` 原生覆盖(`ImageDecoder::{icc_profile,
//!   exif_metadata,orientation}` + `ImageEncoder::{set_icc_profile,set_exif_metadata}`),两侧都不需要
//!   `img-parts`/`little_exif`。
//! - 有损 WebP(`webp` 0.3.1,libwebp 绑定)encoder 无任何 ICC/EXIF 钩子;image crate 自带的 WebP
//!   encoder 又只支持无损。方案 C-6 因此判定:v1 **不 admit WebP 输出**(不是用无损冒充有损,是老实
//!   不给这个选项),留 P2 用 RIFF 容器手工注入或新依赖重新评估。
//! - 输出侧**不转发原始 EXIF blob**:一是几何操作(orientation 之外还有本次 rotate/flip/crop)会让
//!   原始内嵌 EXIF 缩略图(IFD1)与新像素不一致,二是不想连带保留未验证的字段。改为构造一个全新的
//!   最小 EXIF:Orientation 固定写 1(旋转已烤入像素),DateTimeOriginal 若源文件存在且格式合法则透传。
//!   ICC 走独立通道,原样透传原始 profile 字节(不解析语义,只做不透明搬运)。

use image::metadata::Orientation;
use image::ImageDecoder;

/// 从源解码器读到的、决定后续处理的元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMetadata {
    pub orientation: Orientation,
    pub icc_profile: Option<Vec<u8>>,
    pub date_time_original: Option<String>,
}

/// 读取源图元数据。任何单项失败都视为该项缺失(不中断整体解码——这些字段本就是尽力而为)。
pub fn read_source_metadata(decoder: &mut impl ImageDecoder) -> SourceMetadata {
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let icc_profile = decoder.icc_profile().ok().flatten();
    let date_time_original = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .and_then(|chunk| read_date_time_original(&chunk));
    SourceMetadata {
        orientation,
        icc_profile,
        date_time_original,
    }
}

/// D-009(2026-07-19 裁决):编辑链「文件 EXIF orientation 只应用一次」**仅对 JPEG 生效**,
/// 与查看器实际行为对齐——`engine::image_rs::ImageRsEngine` 只在 jpg/jpeg 分支应用 orientation,
/// PNG/WebP/BMP/TIFF 一律显示原始像素。若编辑链单方面对非 JPEG 应用 orientation,用户已用
/// `view_rotation` 手动扶正过的 PNG 会被双重旋转,输出与所见不符。非 JPEG 带 orientation 的
/// 显示错位属 viewer 全格式 orientation 治理(P2),届时两侧同步放开并删除本函数。
///
/// `ext` 为源文件扩展名(不含点),大小写不敏感;调用方传 DB item 的 canonical 扩展名。
pub fn effective_source_orientation(ext: &str, decoded: Orientation) -> Orientation {
    if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
        decoded
    } else {
        Orientation::NoTransforms
    }
}

/// 从原始 EXIF TIFF blob(`ImageDecoder::exif_metadata` 返回值,已剥离容器专属前缀)中取
/// `DateTimeOriginal`。用项目既有的 `kamadak-exif`(`exif` crate,只读)解析——不是我们自己维护
/// 的解析逻辑,可信度高于自研 TIFF walker。
fn read_date_time_original(chunk: &[u8]) -> Option<String> {
    let exif = exif::Reader::new().read_raw(chunk.to_vec()).ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Ascii(v) => {
            let raw = v.first()?;
            let s = String::from_utf8_lossy(raw);
            let s = s.trim_end_matches('\0');
            is_valid_exif_datetime(s).then(|| s.to_string())
        }
        _ => None,
    }
}

/// `YYYY:MM:DD HH:MM:SS`(EXIF 2.3 §4.6.4)的最小格式校验,不做日历合法性检查(源文件已是相机
/// 写入的既成事实,过度校验只会平白丢字段)。
fn is_valid_exif_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 19
        && b[4] == b':'
        && b[7] == b':'
        && b[10] == b' '
        && b[13] == b':'
        && b[16] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, &c)| matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit())
}

/// 编辑保存输出用的最小 EXIF blob:Orientation 固定 1(几何已烤入像素),可选写回
/// `date_time_original`(须先通过 [`is_valid_exif_datetime`],调用方直接传 `read_source_metadata`
/// 读到的值即可,已经过校验)。
pub fn build_output_exif(date_time_original: Option<&str>) -> Vec<u8> {
    build_minimal_exif(
        Orientation::NoTransforms.to_exif() as u16,
        date_time_original,
    )
}

/// 手工最小 TIFF/EXIF blob 构造器。`orientation` 对外只经 [`build_output_exif`](固定 1)暴露;
/// 保留可参数化版本供本文件内的 golden 测试构造「相机写入 orientation=6」等输入样本。
///
/// 布局(小端 `II` 字节序):
/// `TIFF header(8)` → `IFD0(count + entries + next=0)` → 有日期时再接
/// `Exif SubIFD(count + entry + next=0)` → `DateTimeOriginal 的 ASCII 数据区`。
/// 不写 IFD1(缩略图)、不写 GPS/XMP/IPTC——这些字段 v1 不承诺。
fn build_minimal_exif(orientation: u16, date_time_original: Option<&str>) -> Vec<u8> {
    const TIFF_HEADER_LEN: u32 = 8;
    const ORIENTATION_TAG: u16 = 0x0112;
    const EXIF_IFD_POINTER_TAG: u16 = 0x8769;
    const DATE_TIME_ORIGINAL_TAG: u16 = 0x9003;
    const TYPE_SHORT: u16 = 3;
    const TYPE_LONG: u16 = 4;
    const TYPE_ASCII: u16 = 2;

    let date = date_time_original.filter(|s| is_valid_exif_datetime(s));

    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&TIFF_HEADER_LEN.to_le_bytes());

    let ifd0_entry_count: u16 = if date.is_some() { 2 } else { 1 };
    let ifd0_len = 2 + 12 * u32::from(ifd0_entry_count) + 4;
    let exif_subifd_offset = TIFF_HEADER_LEN + ifd0_len;

    buf.extend_from_slice(&ifd0_entry_count.to_le_bytes());
    write_ifd_entry(
        &mut buf,
        ORIENTATION_TAG,
        TYPE_SHORT,
        1,
        &(u32::from(orientation)).to_le_bytes(),
    );
    if date.is_some() {
        write_ifd_entry(
            &mut buf,
            EXIF_IFD_POINTER_TAG,
            TYPE_LONG,
            1,
            &exif_subifd_offset.to_le_bytes(),
        );
    }
    buf.extend_from_slice(&0u32.to_le_bytes()); // IFD0 next-IFD offset:无
    debug_assert_eq!(buf.len() as u32, exif_subifd_offset);

    if let Some(date) = date {
        let mut ascii = date.as_bytes().to_vec();
        ascii.push(0);
        let data_len = ascii.len() as u32; // 20(19 字符 + NUL)
        let subifd_len = 2 + 12 + 4;
        let data_offset = exif_subifd_offset + subifd_len;

        buf.extend_from_slice(&1u16.to_le_bytes());
        write_ifd_entry(
            &mut buf,
            DATE_TIME_ORIGINAL_TAG,
            TYPE_ASCII,
            data_len,
            &data_offset.to_le_bytes(),
        );
        buf.extend_from_slice(&0u32.to_le_bytes()); // Exif SubIFD next-IFD offset:无
        debug_assert_eq!(buf.len() as u32, data_offset);
        buf.extend_from_slice(&ascii);
    }

    buf
}

fn write_ifd_entry(buf: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value_or_offset: &[u8; 4]) {
    buf.extend_from_slice(&tag.to_le_bytes());
    buf.extend_from_slice(&kind.to_le_bytes());
    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(value_or_offset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::{DynamicImage, ImageEncoder, ImageReader, RgbImage};
    use std::io::Cursor;

    const GOLDEN_ICC: &[u8] = b"golden-icc-profile-opaque-bytes-not-parsed";
    const GOLDEN_DATE: &str = "2024:03:15 08:30:00";

    fn golden_image() -> DynamicImage {
        // 4x4 足够验证 orientation 造成的宽高互换,不需要真实照片。
        DynamicImage::ImageRgb8(RgbImage::from_fn(4, 4, |x, y| {
            image::Rgb([x as u8 * 60, y as u8 * 60, 128])
        }))
    }

    fn encode_golden_jpeg(orientation_exif: u16, date: Option<&str>) -> Vec<u8> {
        let img = golden_image();
        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, 92);
        enc.set_icc_profile(GOLDEN_ICC.to_vec()).unwrap();
        enc.set_exif_metadata(build_minimal_exif(orientation_exif, date))
            .unwrap();
        img.write_with_encoder(enc).unwrap();
        out
    }

    fn encode_golden_png(orientation_exif: u16, date: Option<&str>) -> Vec<u8> {
        let img = golden_image();
        let mut out = Vec::new();
        let mut enc = PngEncoder::new(&mut out);
        enc.set_icc_profile(GOLDEN_ICC.to_vec()).unwrap();
        enc.set_exif_metadata(build_minimal_exif(orientation_exif, date))
            .unwrap();
        img.write_with_encoder(enc).unwrap();
        out
    }

    fn decode_bytes(bytes: &[u8]) -> (SourceMetadata, DynamicImage) {
        let reader = ImageReader::new(Cursor::new(bytes.to_vec()))
            .with_guessed_format()
            .unwrap();
        let mut decoder = reader.into_decoder().unwrap();
        let meta = read_source_metadata(&mut decoder);
        let img = DynamicImage::from_decoder(decoder).unwrap();
        (meta, img)
    }

    #[test]
    fn jpeg_golden_orientation_icc_date_round_trip() {
        // EXIF orientation=6 → image crate 映射为 Rotate90(方案 §4 验收项 1:「预览与输出只旋转一次」
        // 的前提是先要能正确读到「应该转几次」)。
        let golden = encode_golden_jpeg(6, Some(GOLDEN_DATE));
        let (meta, img) = decode_bytes(&golden);
        assert_eq!(meta.orientation, Orientation::Rotate90);
        assert_eq!(meta.icc_profile.as_deref(), Some(GOLDEN_ICC));
        assert_eq!(meta.date_time_original.as_deref(), Some(GOLDEN_DATE));
        assert_eq!((img.width(), img.height()), (4, 4)); // 应用 orientation 前仍是编码时的物理尺寸
    }

    #[test]
    fn png_golden_orientation_icc_date_round_trip() {
        // 证明「只应用一次 orientation」的读取路径对 PNG(eXIf chunk)与 JPEG 同构,不需要
        // 按格式分支特判——`image` crate 的默认 `orientation()` 实现本就是格式无关的。
        let golden = encode_golden_png(3, Some(GOLDEN_DATE));
        let (meta, img) = decode_bytes(&golden);
        assert_eq!(meta.orientation, Orientation::Rotate180);
        assert_eq!(meta.icc_profile.as_deref(), Some(GOLDEN_ICC));
        assert_eq!(meta.date_time_original.as_deref(), Some(GOLDEN_DATE));
        assert_eq!((img.width(), img.height()), (4, 4));
    }

    #[test]
    fn output_exif_normalizes_orientation_and_drops_thumbnail() {
        // 端到端:decode 原始(orientation=6)→ apply_orientation 烤入像素 → 用
        // `build_output_exif` 重新编码 → 再 decode,断言方案 §4 验收项:
        // 「输出 orientation 为 1」+「DateTimeOriginal 保留」+「不复制内嵌缩略图」(新 blob 本就没有 IFD1)。
        let golden = encode_golden_jpeg(6, Some(GOLDEN_DATE));
        let (meta, mut img) = decode_bytes(&golden);
        assert_eq!(meta.orientation, Orientation::Rotate90);
        img.apply_orientation(meta.orientation);
        assert_eq!((img.width(), img.height()), (4, 4)); // 4x4 旋转后仍方形,靠下面尺寸交换用例补证非方形场景

        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, 92);
        enc.set_icc_profile(meta.icc_profile.clone().unwrap())
            .unwrap();
        enc.set_exif_metadata(build_output_exif(meta.date_time_original.as_deref()))
            .unwrap();
        img.write_with_encoder(enc).unwrap();

        let (meta2, _) = decode_bytes(&out);
        assert_eq!(meta2.orientation, Orientation::NoTransforms);
        assert_eq!(meta2.icc_profile.as_deref(), Some(GOLDEN_ICC));
        assert_eq!(meta2.date_time_original.as_deref(), Some(GOLDEN_DATE));
    }

    #[test]
    fn output_exif_orientation_swap_is_visible_on_non_square_source() {
        // 非方形样本验证 orientation=6(Rotate90)确实交换了宽高,不是巧合方形导致的假阳性。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(6, 4, |x, y| {
            image::Rgb([x as u8 * 40, y as u8 * 60, 10])
        }));
        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, 92);
        enc.set_exif_metadata(build_minimal_exif(6, None)).unwrap();
        img.write_with_encoder(enc).unwrap();

        let (meta, mut decoded) = decode_bytes(&out);
        assert_eq!((decoded.width(), decoded.height()), (6, 4));
        decoded.apply_orientation(meta.orientation);
        assert_eq!((decoded.width(), decoded.height()), (4, 6));
    }

    #[test]
    fn orientation_only_effective_for_jpeg_matching_viewer() {
        // D-009:与 ImageRsEngine 的 jpg/jpeg-only orientation 行为对齐,防 PNG 双重旋转。
        assert_eq!(
            effective_source_orientation("jpg", Orientation::Rotate90),
            Orientation::Rotate90
        );
        assert_eq!(
            effective_source_orientation("JPEG", Orientation::Rotate270),
            Orientation::Rotate270
        );
        for ext in ["png", "webp", "bmp", "tiff", "PNG"] {
            assert_eq!(
                effective_source_orientation(ext, Orientation::Rotate90),
                Orientation::NoTransforms,
                "ext={ext}"
            );
        }
    }

    #[test]
    fn build_output_exif_without_date_has_no_exif_ifd_pointer() {
        // 无 DateTimeOriginal 时不应额外分配 Exif SubIFD——直接量长度断言,免得日后误引入死代码。
        let blob = build_output_exif(None);
        assert_eq!(blob.len(), 8 + 2 + 12 + 4); // header + IFD0(1 entry) + next=0
    }

    #[test]
    fn malformed_date_time_original_is_dropped_not_forwarded() {
        // 降级规则(方案 §4 验收项):无法验证的字段丢弃,不写入半成品数据。
        let blob = build_output_exif(Some("not-a-date"));
        assert_eq!(blob.len(), 8 + 2 + 12 + 4); // 与「无日期」分支长度一致,证明确被丢弃
    }
}
