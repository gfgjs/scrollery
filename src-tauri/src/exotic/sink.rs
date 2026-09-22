//! Exotic 缩略图的准备与发布。
//! 写入/同步临时文件在 DB 锁外；调用方取得 owner/source 条件写事务后才能 publish。
//! 文件发布成功后再提交 DB，未发布的临时文件由 Drop 清理。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, Result};
use crate::thumbnail::cache::{ensure_thumb_dir, thumb_db_path, thumb_path};

/// 临时文件名去重计数器（进程内单调，配合 pid + 纳秒避免碰撞）。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 已写入并同步的临时产物；尚未替换正在使用的缩略图。
pub struct PreparedThumbnail {
    pub thumb_db_path: String,
    tmp_path: PathBuf,
    final_path: PathBuf,
}

impl PreparedThumbnail {
    /// 调用方须持有通过 owner/source 条件更新取得的 DB 写事务，防旧结果覆盖新文件。
    pub fn publish(&self) -> Result<()> {
        std::fs::rename(&self.tmp_path, &self.final_path)
            .map_err(|e| AppError::Internal(format!("rename 失败：{e}")))
    }
}

impl Drop for PreparedThumbnail {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.tmp_path);
    }
}

/// 准备已通过宿主验证的 WebP；仅写唯一临时文件，失败/放弃均清理临时文件。
pub fn prepare_thumbnail(
    cache_dir: &Path,
    tier: u32,
    cache_key: i64,
    blob: &[u8],
) -> Result<PreparedThumbnail> {
    if blob.len() < 12 || &blob[0..4] != b"RIFF" || &blob[8..12] != b"WEBP" {
        return Err(AppError::Internal("Sink: blob 非合法 WebP".into()));
    }
    let final_path = thumb_path(cache_dir, tier, cache_key);
    ensure_thumb_dir(cache_dir, tier, cache_key).map_err(|e| AppError::Internal(e.to_string()))?;
    let tmp_path = unique_tmp_path(&final_path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let prepared = PreparedThumbnail {
        thumb_db_path: thumb_db_path(tier, cache_key),
        tmp_path,
        final_path,
    };
    let written = file
        .write_all(blob)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(prepared)
}

/// 同目录唯一临时文件名：`{final_stem}.{pid}.{nanos}.{seq}.tmp`。
fn unique_tmp_path(final_path: &Path) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let stem = final_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    let dir = final_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{stem}.{pid}.{nanos}.{seq}.tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([90, 140, 200, 255]));
        let mut buf = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut buf))
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    /// 测试用档位：绑 THUMB_TIERS 事实源。thumb_path 会断言档位合法，硬编码数值在
    /// b554aa5「档位重定」后即失效（详见 exotic/fingerprint.rs 测试模块的同名常量注释）。
    const TIER: u32 = crate::thumbnail::generator::THUMB_TIERS[3];

    #[test]
    fn prepares_then_publishes_atomically() {
        let dir = std::env::temp_dir().join(format!("exotic-sink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 480x240 是**图像尺寸**（与档位无关，仅需一张能编码的图）。
        let webp = make_webp(480, 240);
        let out = prepare_thumbnail(&dir, TIER, 0x1234_5678, &webp).unwrap();
        assert!(!thumb_path(&dir, TIER, 0x1234_5678).exists());
        out.publish().unwrap();

        // 最终文件存在、内容一致、无残留 .tmp。
        let final_path = thumb_path(&dir, TIER, 0x1234_5678);
        assert!(final_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), webp);
        assert!(out.thumb_db_path.starts_with(&format!("{TIER}/")));
        let leftover: Vec<_> = walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tmp").unwrap_or(false))
            .collect();
        assert!(leftover.is_empty(), "不应残留 .tmp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_webp_blob() {
        let dir = std::env::temp_dir().join(format!("exotic-sink-bad-{}", std::process::id()));
        let r = prepare_thumbnail(&dir, TIER, 1, b"not webp");
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
