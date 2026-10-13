use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

const MAX_MARKERS: usize = 4096;
const MAX_IFD_ENTRIES: u16 = 1024;

/// JPEG 主图的 EXIF 属性及可选的内嵌 JPEG 位置。
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct JpegMetadata {
    pub orientation: Option<u32>,
    pub color_space: Option<u32>,
    pub thumbnail: Option<EmbeddedJpeg>,
}

/// 位于同一 APP1 段内的内嵌 JPEG；方向缺失时由调用方决定继承策略。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct EmbeddedJpeg {
    pub offset: u64,
    pub length: u32,
    pub orientation: Option<u32>,
}

/// 有界读取已授权文件的 EXIF 元数据，不解码像素；调用后文件位置可能变化。
pub(super) fn inspect(file: &mut File) -> io::Result<JpegMetadata> {
    inspect_reader(file)
}

fn inspect_reader<R: Read + Seek>(reader: &mut R) -> io::Result<JpegMetadata> {
    let metadata = JpegMetadata::default();
    let file_end = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    if read_array::<2, _>(reader)? != Some([0xff, 0xd8]) {
        return Ok(metadata);
    }
    // 连续的 FF 填充字节也占用扫描额度，避免恶意输入拖长标记扫描。
    for _ in 0..MAX_MARKERS {
        let Some([prefix, marker]) = read_array::<2, _>(reader)? else {
            break;
        };
        if prefix != 0xff || marker == 0 || matches!(marker, 0xda | 0xd9) {
            break;
        }
        if marker == 0xff {
            reader.seek(SeekFrom::Current(-1))?;
            continue;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        let Some(length) = read_array::<2, _>(reader)? else {
            break;
        };
        let length = u16::from_be_bytes(length);
        if length < 2 {
            break;
        }
        let start = reader.stream_position()?;
        let Some(end) = start.checked_add(u64::from(length - 2)) else {
            break;
        };
        if end > file_end {
            break;
        }
        if marker == 0xe1 && length >= 8 && read_array::<6, _>(reader)? == Some(*b"Exif\0\0") {
            // 与主图 EXIF 读取保持一致，仅采用首个 EXIF 段的完整结果。
            return inspect_tiff(reader, start + 6, end);
        }
        reader.seek(SeekFrom::Start(end))?;
    }
    Ok(metadata)
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> io::Result<Option<[u8; N]>> {
    let mut bytes = [0; N];
    match reader.read_exact(&mut bytes) {
        Ok(()) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(error),
    }
}

struct Tiff {
    start: u64,
    end: u64,
    little: bool,
}

impl Tiff {
    fn u16(&self, bytes: [u8; 2]) -> u16 {
        if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }

    fn u32(&self, bytes: [u8; 4]) -> u32 {
        if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }

    fn read<const N: usize, R: Read + Seek>(
        &self,
        reader: &mut R,
        offset: u64,
    ) -> io::Result<Option<[u8; N]>> {
        let Some(start) = self.start.checked_add(offset) else {
            return Ok(None);
        };
        if start.checked_add(N as u64).is_none_or(|end| end > self.end) {
            return Ok(None);
        }
        reader.seek(SeekFrom::Start(start))?;
        read_array(reader)
    }

    fn scalar(&self, entry: &[u8; 12], kind: u16) -> Option<u32> {
        if self.u16([entry[2], entry[3]]) != kind
            || self.u32([entry[4], entry[5], entry[6], entry[7]]) != 1
        {
            return None;
        }
        match kind {
            3 => Some(u32::from(self.u16([entry[8], entry[9]]))),
            4 => Some(self.u32([entry[8], entry[9], entry[10], entry[11]])),
            _ => None,
        }
    }

    fn directory<R: Read + Seek>(
        &self,
        reader: &mut R,
        offset: u32,
    ) -> io::Result<Option<Directory>> {
        if offset < 8 {
            return Ok(None);
        }
        let offset = u64::from(offset);
        let Some(count) = self.read::<2, _>(reader, offset)? else {
            return Ok(None);
        };
        let count = self.u16(count);
        let next_offset = offset + 2 + u64::from(count) * 12;
        // 整张目录及 next 指针必须位于 APP1 内，不能读取后续段的字节。
        if count > MAX_IFD_ENTRIES
            || self
                .start
                .checked_add(next_offset + 4)
                .is_none_or(|end| end > self.end)
        {
            return Ok(None);
        }
        let mut directory = Directory::default();
        // 读取 count 后已位于表起点；整表边界已确认，后续顺序读取避免逐条 seek。
        for _ in 0..count {
            let Some(entry) = read_array::<12, _>(reader)? else {
                return Ok(None);
            };
            match self.u16([entry[0], entry[1]]) {
                0x0112 => {
                    let orientation = self
                        .scalar(&entry, 3)
                        .filter(|value| (1..=8).contains(value));
                    directory.invalid_orientation |= orientation.is_none();
                    directory.orientation = orientation;
                }
                0x0103 => directory.compression = self.scalar(&entry, 3),
                0x0201 => directory.jpeg_offset = self.scalar(&entry, 4),
                0x0202 => directory.jpeg_length = self.scalar(&entry, 4),
                0x8769 => directory.exif_ifd = self.scalar(&entry, 4),
                0xa001 if directory.color_space != Some(u32::MAX) => {
                    // 非法类型或数量不能当作字段缺失并回退 sRGB，后续重复字段也不能抹去拒绝标记。
                    directory.color_space = Some(self.scalar(&entry, 3).unwrap_or(u32::MAX));
                }
                _ => {}
            }
        }
        let Some(next) = read_array::<4, _>(reader)? else {
            return Ok(None);
        };
        directory.next = self.u32(next);
        Ok(Some(directory))
    }
}

#[derive(Default)]
struct Directory {
    orientation: Option<u32>,
    invalid_orientation: bool,
    compression: Option<u32>,
    jpeg_offset: Option<u32>,
    jpeg_length: Option<u32>,
    exif_ifd: Option<u32>,
    color_space: Option<u32>,
    next: u32,
}

fn inspect_tiff<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> io::Result<JpegMetadata> {
    let mut metadata = JpegMetadata::default();
    if end.saturating_sub(start) < 8 {
        return Ok(metadata);
    }
    let Some(header) = read_array::<8, _>(reader)? else {
        return Ok(metadata);
    };
    let little = match &header[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(metadata),
    };
    let tiff = Tiff { start, end, little };
    if tiff.u16([header[2], header[3]]) != 42 {
        return Ok(metadata);
    }
    let ifd0_offset = tiff.u32([header[4], header[5], header[6], header[7]]);
    let Some(ifd0) = tiff.directory(reader, ifd0_offset)? else {
        return Ok(metadata);
    };
    metadata.orientation = ifd0.orientation;
    if let Some(exif_offset) = ifd0.exif_ifd {
        if exif_offset != ifd0_offset && exif_offset != ifd0.next {
            if let Some(exif) = tiff.directory(reader, exif_offset)? {
                metadata.color_space = exif.color_space;
            }
        }
    }
    // 不追踪目录链，只读取 IFD0 指向的一张 IFD1，并拒绝已知的循环或别名。
    if ifd0.next == 0 || ifd0.next == ifd0_offset || Some(ifd0.next) == ifd0.exif_ifd {
        return Ok(metadata);
    }
    let Some(ifd1) = tiff.directory(reader, ifd0.next)? else {
        return Ok(metadata);
    };
    if ifd1.compression != Some(6)
        || ifd1.invalid_orientation
        || ifd1.next == ifd0_offset
        || ifd1.next == ifd0.next
    {
        return Ok(metadata);
    }
    let (Some(offset), Some(length)) = (ifd1.jpeg_offset, ifd1.jpeg_length) else {
        return Ok(metadata);
    };
    let offset = u64::from(offset);
    let Some(absolute_offset) = start.checked_add(offset) else {
        return Ok(metadata);
    };
    if offset < 8
        || length < 2
        || absolute_offset
            .checked_add(u64::from(length))
            .is_none_or(|limit| limit > end)
        || tiff.read::<2, _>(reader, offset)? != Some([0xff, 0xd8])
    {
        return Ok(metadata);
    }
    metadata.thumbnail = Some(EmbeddedJpeg {
        offset: absolute_offset,
        length,
        orientation: ifd1.orientation,
    });
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const IFD0: usize = 8;
    const EXIF_IFD: usize = 38;
    const IFD1: usize = 56;
    const JPEG_OFFSET: usize = 110;
    const TIFF_START: usize = 12;

    fn put16(bytes: &mut [u8], at: usize, value: u16, little: bool) {
        let encoded = if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[at..at + 2].copy_from_slice(&encoded);
    }

    fn put32(bytes: &mut [u8], at: usize, value: u32, little: bool) {
        let encoded = if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[at..at + 4].copy_from_slice(&encoded);
    }

    fn entry(bytes: &mut [u8], at: usize, tag: u16, kind: u16, value: u32, little: bool) {
        put16(bytes, at, tag, little);
        put16(bytes, at + 2, kind, little);
        put32(bytes, at + 4, 1, little);
        if kind == 3 {
            put16(bytes, at + 8, value as u16, little);
        } else {
            put32(bytes, at + 8, value, little);
        }
    }

    fn fixture(little: bool) -> Vec<u8> {
        let mut tiff = vec![0; JPEG_OFFSET + 4];
        tiff[..2].copy_from_slice(if little { b"II" } else { b"MM" });
        put16(&mut tiff, 2, 42, little);
        put32(&mut tiff, 4, IFD0 as u32, little);
        put16(&mut tiff, IFD0, 2, little);
        entry(&mut tiff, IFD0 + 2, 0x112, 3, 6, little);
        entry(&mut tiff, IFD0 + 14, 0x8769, 4, EXIF_IFD as u32, little);
        put32(&mut tiff, IFD0 + 26, IFD1 as u32, little);
        put16(&mut tiff, EXIF_IFD, 1, little);
        entry(&mut tiff, EXIF_IFD + 2, 0xa001, 3, 1, little);
        put16(&mut tiff, IFD1, 4, little);
        entry(&mut tiff, IFD1 + 2, 0x103, 3, 6, little);
        entry(&mut tiff, IFD1 + 14, 0x201, 4, JPEG_OFFSET as u32, little);
        entry(&mut tiff, IFD1 + 26, 0x202, 4, 4, little);
        entry(&mut tiff, IFD1 + 38, 0x112, 3, 3, little);
        tiff[JPEG_OFFSET..].copy_from_slice(&[0xff, 0xd8, 0xff, 0xd9]);
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xff, 0xda]);
        jpeg
    }

    fn parse(bytes: Vec<u8>) -> JpegMetadata {
        inspect_reader(&mut Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn app1_bounds_reject_truncated_tables_and_external_thumbnail_ranges() {
        let mut truncated = fixture(true);
        put32(&mut truncated, TIFF_START + IFD0 + 26, 112, true);
        let result = parse(truncated);
        assert_eq!(result.orientation, Some(6));
        assert_eq!(result.thumbnail, None);
        let mut external = fixture(true);
        put32(&mut external, TIFF_START + IFD1 + 34, 8, true);
        let result = parse(external);
        assert_eq!(result.orientation, Some(6));
        assert_eq!(result.thumbnail, None);
        let mut eof = fixture(true);
        eof.truncate(TIFF_START + IFD0 + 4);
        assert_eq!(parse(eof), JpegMetadata::default());
    }
}
