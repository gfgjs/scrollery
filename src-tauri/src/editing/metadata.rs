//! 编辑保存的元数据策略。
//!
//! JPEG/PNG 通过 `image` 读写 ICC 与 EXIF。有损 WebP 编码器无 ICC/EXIF 写入接口，
//! 因此编辑输出仅支持 JPEG/PNG。
//! 几何操作会使原 EXIF 缩略图与新像素不一致，输出重建最小 EXIF：Orientation 为 1，
//! 格式合法时保留 DateTimeOriginal；ICC 原字节单独透传。

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

/// 编辑链只对 JPEG 应用文件 EXIF orientation，
/// 与查看器实际行为对齐——`engine::image_rs::ImageRsEngine` 只在 jpg/jpeg 分支应用 orientation,
/// PNG/WebP/BMP/TIFF 一律显示原始像素。若编辑链单方面对非 JPEG 应用 orientation,用户已用
/// `view_rotation` 手动扶正过的 PNG 会被双重旋转,输出与所见不符。非 JPEG 带 orientation 的
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
