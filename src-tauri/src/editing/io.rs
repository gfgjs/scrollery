//! 编辑保存先写同目录 `*.tmp`，同步并复读验证后，改名覆盖已认领的目标路径。
//! 失败时清理临时文件与占位文件，避免发布半成品。

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, ImageReader};

use crate::error::AppError;

use super::metadata;

pub const CODE_ENCODE_FAILED: &str = "edit_encode_failed";
pub const CODE_IO: &str = "edit_io";

/// 支持保留必要元数据的输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg,
    Png,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
        }
    }
}

fn encode_failed() -> AppError {
    AppError::Edit {
        code: CODE_ENCODE_FAILED,
        message: "图像编码或复读验证失败 | image encode or round-trip verification failed".into(),
    }
}

/// 编码 `img` 为 `format`(JPEG 用 `quality` 1..=100;PNG 忽略 `quality`),写入 ICC(若源图
/// 携带)与由 [`metadata::build_output_exif`] 构造的最小 EXIF，不转发原始 blob。
fn encode(
    img: &DynamicImage,
    format: OutputFormat,
    quality: u8,
    icc: Option<&[u8]>,
    date_time_original: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    let exif = metadata::build_output_exif(date_time_original);
    let mut out = Vec::new();
    let encode_result = match format {
        OutputFormat::Jpeg => {
            let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc.to_vec());
            }
            let _ = enc.set_exif_metadata(exif);
            img.write_with_encoder(enc)
        }
        OutputFormat::Png => {
            let mut enc = PngEncoder::new(&mut out);
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc.to_vec());
            }
            let _ = enc.set_exif_metadata(exif);
            img.write_with_encoder(enc)
        }
    };
    encode_result.map_err(|_| encode_failed())?;
    Ok(out)
}

/// 重新解码写出的字节，确认几何操作烤入像素后 orientation 已归一。
fn verify_round_trip(bytes: &[u8]) -> Result<(), AppError> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| encode_failed())?;
    let mut decoder = reader.into_decoder().map_err(|_| encode_failed())?;
    let meta = metadata::read_source_metadata(&mut decoder);
    if meta.orientation != image::metadata::Orientation::NoTransforms {
        return Err(encode_failed());
    }
    Ok(())
}

/// 落盘契约主入口。`claimed_path` 必须是 [`super::naming::claim_target_path`] 已独占创建的
/// 空占位文件——本函数编码 → 写同目录 `{claimed_path}.tmp` → 从磁盘复读验证(而非只验内存
/// 字节,以捕获实际写入期间的损坏)→ `sync_all` → `rename` 覆盖占位(同卷原子提交)。
/// 任一步失败:清理 tmp 与占位文件,返回错误,不留孤儿文件、不发布半成品。
pub fn write_edited_image(
    claimed_path: &Path,
    img: &DynamicImage,
    format: OutputFormat,
    quality: u8,
    icc: Option<&[u8]>,
    date_time_original: Option<&str>,
) -> Result<(), AppError> {
    let bytes = match encode(img, format, quality, icc, date_time_original) {
        Ok(b) => b,
        Err(e) => {
            let _ = fs::remove_file(claimed_path);
            return Err(e);
        }
    };

    let tmp_name = format!(
        "{}.tmp",
        claimed_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("edit_output")
    );
    let tmp_path = claimed_path.with_file_name(tmp_name);
    let _ = fs::remove_file(&tmp_path); // 清可能的异常退出残留

    let write_result: std::io::Result<()> = (|| {
        let mut f = File::create(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_file(claimed_path);
        return Err(AppError::Edit {
            code: CODE_IO,
            message: "临时文件写入失败 | tmp file write failed".into(),
        });
    }

    let verified = fs::read(&tmp_path)
        .ok()
        .map(|b| verify_round_trip(&b).is_ok())
        .unwrap_or(false);
    if !verified {
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_file(claimed_path);
        return Err(encode_failed());
    }

    if fs::rename(&tmp_path, claimed_path).is_err() {
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_file(claimed_path);
        return Err(AppError::Edit {
            code: CODE_IO,
            message: "改名落盘失败 | rename to final path failed".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("scrollery_edit_io_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_image() -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(4, 3, |x, y| {
            Rgb([(x * 60) as u8, (y * 80) as u8, 128])
        }))
    }

    #[test]
    fn write_edited_image_commits_jpeg_and_replaces_placeholder() {
        let dir = tmp_dir("jpeg_commit");
        let target = super::super::naming::claim_target_path(&dir, "photo-edit", "jpg").unwrap();
        write_edited_image(&target, &sample_image(), OutputFormat::Jpeg, 92, None, None).unwrap();

        assert!(target.exists());
        assert!(
            !target.with_file_name("photo-edit.jpg.tmp").exists(),
            "tmp 应已被 rename 消费"
        );
        let bytes = fs::read(&target).unwrap();
        assert!(!bytes.is_empty());
        // 落盘产物可正常解码回相同尺寸。
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4, 3));
        let _ = fs::remove_dir_all(&dir);
    }
}
