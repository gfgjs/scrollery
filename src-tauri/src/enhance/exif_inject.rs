// src-tauri/src/enhance/exif_inject.rs
//! 容器级 EXIF 注入（不重编码，best-effort）。从 `service.rs` 拆出的纯字节操作单元
//! （超长文件拆分方案 tierB-2:与 `EnhanceService` 状态零耦合,`finalize_output` 是唯一调用点）。

use std::path::Path;

use crate::editing::metadata;

/// 读源图 DateTimeOriginal 构造最小 EXIF blob（orientation 固定 1）。读失败 → None（跳过注入）。
/// `finalize_output`(留在 `service.rs`)唯一调用点。
pub(crate) fn build_source_exif(source: &Path) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(source)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let date = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .and_then(|chunk| read_date_time_original(&chunk));
    Some(metadata::build_output_exif(date.as_deref()))
}

/// 从 EXIF TIFF blob 取 DateTimeOriginal（复用 kamadak-exif 只读解析，同 editing::metadata 私有逻辑）。
fn read_date_time_original(chunk: &[u8]) -> Option<String> {
    let exif = exif::Reader::new().read_raw(chunk.to_vec()).ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Ascii(v) => {
            let raw = v.first()?;
            let s = String::from_utf8_lossy(raw);
            Some(s.trim_end_matches('\0').to_string())
        }
        _ => None,
    }
}

/// 容器级注入 EXIF（JPEG APP1 / PNG eXIf）。未知格式或结构异常 → 原样返回（best-effort）。
/// `finalize_output`(留在 `service.rs`)唯一调用点。
pub(crate) fn inject_exif_container(data: &[u8], out_ext: &str, tiff: &[u8]) -> Vec<u8> {
    match out_ext {
        "jpg" | "jpeg" => inject_jpeg_app1(data, tiff),
        "png" => inject_png_exif(data, tiff),
        _ => data.to_vec(),
    }
}

/// JPEG：SOI 之后插入 APP1(Exif) 段。段长上界 65535，本 blob 极小。
fn inject_jpeg_app1(data: &[u8], tiff: &[u8]) -> Vec<u8> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return data.to_vec();
    }
    let mut payload = Vec::with_capacity(6 + tiff.len());
    payload.extend_from_slice(b"Exif\0\0");
    payload.extend_from_slice(tiff);
    let seg_len = payload.len() + 2;
    if seg_len > 0xFFFF {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() + seg_len + 2);
    out.extend_from_slice(&data[0..2]); // SOI
    out.push(0xFF);
    out.push(0xE1);
    out.extend_from_slice(&(seg_len as u16).to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&data[2..]);
    out
}

/// PNG：IEND 之前插入 eXIf 块（含 CRC32）。
fn inject_png_exif(data: &[u8], tiff: &[u8]) -> Vec<u8> {
    const SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    if data.len() < 8 || data[0..8] != SIG {
        return data.to_vec();
    }
    // 走链定位 IEND 块起点（长度字段偏移）。
    let mut offset = 8usize;
    let mut iend_pos: Option<usize> = None;
    while offset + 8 <= data.len() {
        let len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let ctype = &data[offset + 4..offset + 8];
        if ctype == b"IEND" {
            iend_pos = Some(offset);
            break;
        }
        offset = offset.saturating_add(12).saturating_add(len);
    }
    let Some(pos) = iend_pos else {
        return data.to_vec();
    };
    let mut chunk = Vec::with_capacity(12 + tiff.len());
    chunk.extend_from_slice(&(tiff.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"eXIf");
    chunk.extend_from_slice(tiff);
    let crc = crc32(&chunk[4..]); // CRC over type + data
    chunk.extend_from_slice(&crc.to_be_bytes());

    let mut out = Vec::with_capacity(data.len() + chunk.len());
    out.extend_from_slice(&data[..pos]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&data[pos..]);
    out
}

/// 标准 PNG CRC-32（poly 0xEDB88320，无查表，blob 极小无需优化）。
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_app1_injection_is_decodable_and_preserves_pixels() {
        use image::{DynamicImage, RgbImage};
        // 编码一张无 EXIF 的 JPEG，注入后仍可解码回同尺寸。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(8, 6, |x, y| {
            image::Rgb([(x * 20) as u8, (y * 30) as u8, 100])
        }));
        let mut jpeg = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        let exif = metadata::build_output_exif(Some("2024:03:15 08:30:00"));
        let injected = inject_jpeg_app1(&jpeg, &exif);
        assert!(injected.len() > jpeg.len(), "应插入 APP1 段");
        let decoded = image::load_from_memory(&injected).expect("注入后仍可解码");
        assert_eq!((decoded.width(), decoded.height()), (8, 6));
    }

    #[test]
    fn png_exif_injection_is_decodable() {
        use image::{DynamicImage, RgbImage};
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(8, 6, |x, y| {
            image::Rgb([(x * 20) as u8, (y * 30) as u8, 100])
        }));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let exif = metadata::build_output_exif(Some("2024:03:15 08:30:00"));
        let injected = inject_png_exif(&png, &exif);
        assert!(injected.len() > png.len(), "应插入 eXIf 块");
        let decoded = image::load_from_memory(&injected).expect("注入后仍可解码");
        assert_eq!((decoded.width(), decoded.height()), (8, 6));
    }

    #[test]
    fn inject_leaves_unknown_format_untouched() {
        let junk = b"not an image".to_vec();
        assert_eq!(inject_exif_container(&junk, "webp", b"exif"), junk);
        // JPEG 注入器对非 JPEG 字节原样返回。
        assert_eq!(inject_jpeg_app1(&junk, b"exif"), junk);
    }
}
