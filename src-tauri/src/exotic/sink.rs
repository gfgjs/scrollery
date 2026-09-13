// src-tauri/src/exotic/sink.rs
//! 缩略图 Sink（v3 Part2 §4.4）。**只**实现首发 `image + thumbnail`。
//!
//! 落盘语义（严格顺序）：
//!   1. 计算最终路径 `thumb_path(cache_dir, tier, cache_key)`；
//!   2. 同目录建唯一 `.tmp`，写 blob、flush、sync_all；
//!   3. WebP 二次魔数校验后**原子 rename**；
//!   4. （由 Pipeline）DB 事务**条件**更新 task done + media_items（status=1 AND lease_owner）；
//!   5. 事务成功后同步 layout cache + 合并发事件。
//!
//! **绝不先写 task done 再落文件**。DB 事务失败时删除临时文件；最终文件已替换但 DB 失败时允许留下
//! 可回收孤儿（启动维护按 DB 引用清理）。thumbhash 由 **Host** 对已验证像素计算（不信任 Worker 声明）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{ensure_thumb_dir, thumb_db_path, thumb_path};
use crate::thumbnail::thumbhash::generate_thumbhash;

/// 临时文件名去重计数器（进程内单调，配合 pid + 纳秒避免碰撞）。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Sink 产物：最终相对 DB 路径 + Host 计算的 thumbhash。
pub struct SinkOutput {
    pub thumb_db_path: String,
    pub thumbhash: Vec<u8>,
}

/// 原子落盘已验证的 WebP，并由 Host 计算 thumbhash。**不**触碰 DB（DB 由 Pipeline 在条件事务内做）。
///
/// `blob` 必须是已通过 Host 验证的合法 WebP（[`super::worker::validate_thumbnail_output`]）。
pub fn write_thumbnail_atomic(
    cache_dir: &Path,
    tier: u32,
    cache_key: i64,
    blob: &[u8],
) -> Result<SinkOutput> {
    // 二次魔数校验（纵深防御；上游已验证）。
    if blob.len() < 12 || &blob[0..4] != b"RIFF" || &blob[8..12] != b"WEBP" {
        return Err(AppError::Internal("Sink: blob 非合法 WebP".into()));
    }

    let final_path = thumb_path(cache_dir, tier, cache_key);
    ensure_thumb_dir(cache_dir, tier, cache_key).map_err(|e| AppError::Internal(e.to_string()))?;

    // 同目录唯一 .tmp（同卷 → rename 原子）。
    let tmp_path = unique_tmp_path(&final_path);
    {
        let mut f =
            std::fs::File::create(&tmp_path).map_err(|e| AppError::Internal(e.to_string()))?;
        f.write_all(blob).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            AppError::Internal(e.to_string())
        })?;
        f.flush().map_err(|e| AppError::Internal(e.to_string()))?;
        // sync_all：确保数据落盘后再 rename（防崩溃留下空/半文件被当成有效缓存）。
        f.sync_all()
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    // 原子替换。
    if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::Internal(format!("rename 失败：{e}")));
    }

    // Host 计算 thumbhash（独立解码已验证 blob → RGBA）。
    let thumbhash = thumbhash_from_webp(blob)?;

    Ok(SinkOutput {
        thumb_db_path: thumb_db_path(tier, cache_key),
        thumbhash,
    })
}

/// 解码 WebP → RGBA → ThumbHash（Host 侧计算，不信任 Worker 声明）。
fn thumbhash_from_webp(blob: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory_with_format(blob, image::ImageFormat::WebP)
        .map_err(|e| AppError::Internal(format!("thumbhash 解码失败：{e}")))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let decoded = DecodedImage {
        pixels: rgba.into_raw(),
        width: w,
        height: h,
        icc: None, // 仅用于 ThumbHash 计算,ICC 与此无关
    };
    generate_thumbhash(&decoded)
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
    fn writes_atomically_and_computes_thumbhash() {
        let dir = std::env::temp_dir().join(format!("exotic-sink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 480x240 是**图像尺寸**（与档位无关，仅需一张能编码的图）。
        let webp = make_webp(480, 240);
        let out = write_thumbnail_atomic(&dir, TIER, 0x1234_5678, &webp).unwrap();

        // 最终文件存在、内容一致、无残留 .tmp。
        let final_path = thumb_path(&dir, TIER, 0x1234_5678);
        assert!(final_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), webp);
        assert!(out.thumb_db_path.starts_with(&format!("{TIER}/")));
        assert!(!out.thumbhash.is_empty());
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
        let r = write_thumbnail_atomic(&dir, TIER, 1, b"not webp");
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
