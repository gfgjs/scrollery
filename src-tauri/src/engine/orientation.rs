//! 图片解码与扫描共享的已授权句柄 EXIF 方向读取。

use std::io::BufReader;

/// 从已授权句柄读取 EXIF 方向；也支持 EXIF 库可识别的 PNG/TIFF/WebP/HEIF 容器。
/// 缺失、无效或读取失败返回 1；会移动游标，解码前调用方须 rewind。
pub fn read_jpeg_orientation_file(file: &mut std::fs::File) -> u32 {
    use std::io::Seek;
    let read = || -> Option<u32> {
        file.rewind().ok()?;
        let mut reader = BufReader::new(file);
        let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
        let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
        field
            .value
            .get_uint(0)
            .filter(|value| (1..=8).contains(value))
    };
    read().unwrap_or(1)
}
