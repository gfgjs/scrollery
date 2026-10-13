// src-tauri/src/ai/face_pipeline/decode_source.rs
//! 解码源选择：在「常规分档缩略图」「host 预解码 face 缓存」
//! 「原图直派」之间选择,及检测/嵌入结果向 DB 行的映射。

use std::path::PathBuf;

use crate::ai::clip::embedding_to_bytes;
use crate::ai::face::DetectedFace;
use crate::db::queries::{FaceDisplayArtifact, NewFace};
use crate::thumbnail::cache::FACE_CACHE_SHORT_EDGE;

use super::FaceTask;

#[path = "../../../../crates/shared/authorized_file.rs"]
mod authorized_file;

/// 已解析的解码源：解码哪个文件、按什么格式。
pub(super) struct FaceDecodeSource {
    pub(super) path: PathBuf,
    pub(super) format: String,
    pub(super) kind: FaceSourceKind,
}

/// 解码源三级构成：诊断计数随批输出；原图直派通常只剩短边 ≤640 的小图，
/// 若原图源占比高且单项耗时大,优先怀疑防呆回退或预解码异常。
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FaceSourceKind {
    /// 常规分档缩略图(预测短边 ≥ detect_size)。
    Thumb,
    /// host 预解码的 face 缓存(短边 640 WebP)。
    FaceCache,
    /// 原图直派 worker 解码(短边本就 ≤640 的小图,或 detect_size 超缓存尺寸的防呆回退)。
    Original,
}

/// worker 端可解码的源格式白名单(ai-worker 用纯 `image` crate 解码,无 WIC/exotic 引擎;
/// 与 ai-worker Cargo.toml 的 image features 对齐)。缩略图档位与 face 缓存源恒为 webp(可解);
/// 白名单外（heic/raw/psd 等）的原图回退项走 host 预解码的 face 缓存；
/// 若 detect_size 超过缓存尺寸且 worker 无法解码源格式，则跳过该项。
pub(super) const WORKER_DECODABLE_FORMATS: &[&str] =
    &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];

/// 选择短边仍满足 `detect_size`（640）的最廉价解码源。
///
/// 在同源、同策略的已提交显示产物中选择最小合格输入，不涉及 CLIP 的 336px 缓存。
pub(super) fn resolve_face_decode_source(
    task: &FaceTask,
    cache_dir: &std::path::Path,
    policy: &str,
    artifacts: &[FaceDisplayArtifact],
    detect_size: u32,
) -> FaceDecodeSource {
    let original = FaceDecodeSource {
        path: task.source_path.clone(),
        format: task.file_format.clone(),
        kind: FaceSourceKind::Original,
    };

    if task.width <= 0 || task.height <= 0 {
        return original;
    }
    let (w, h) = (task.width as u32, task.height as u32);
    let (long, short) = (w.max(h), w.min(h));
    let prefix = format!("{policy}:");
    let hex = format!("{:016x}", task.cache_key as u64);
    let mut best: Option<(u64, PathBuf)> = None;
    for a in artifacts {
        if a.item_id != task.item_id
            || a.cache_key != task.cache_key
            || a.source_revision != task.source_revision
        {
            continue;
        }
        let Some(tier) = a
            .fingerprint
            .strip_prefix(&prefix)
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let predicted_short = if long <= tier {
            short
        } else {
            (short as f64 * tier as f64 / long as f64).round() as u32
        };
        if predicted_short < detect_size {
            continue;
        }
        let expected = format!(
            "{}/{}/r{}-{}/{}.webp",
            tier,
            &hex[..2],
            task.source_revision,
            policy,
            hex
        );
        if a.path != expected {
            continue;
        }
        // 宿主和 worker 都从授权句柄读取，禁止路径穿越和检查后链接替换。
        let Ok(file) = authorized_file::open_relative(cache_dir, &format!("thumbnails/{}", a.path))
        else {
            continue;
        };
        let reader = image::ImageReader::with_format(
            std::io::BufReader::new(file),
            image::ImageFormat::WebP,
        );
        let Ok((actual_w, actual_h)) = reader.into_dimensions() else {
            continue;
        };
        if actual_w.min(actual_h) < detect_size {
            continue;
        }
        let pixels = u64::from(actual_w) * u64::from(actual_h);
        if best.as_ref().is_none_or(|(size, _)| pixels < *size) {
            best = Some((pixels, cache_dir.join("thumbnails").join(&a.path)));
        }
    }
    best.map(|(_, path)| FaceDecodeSource {
        path,
        format: "webp".into(),
        kind: FaceSourceKind::Thumb,
    })
    .unwrap_or(original)
}

/// 原图回退项是否应升级走 face 缓存(方案 A 决策核,纯函数可单测)。
/// - 防呆:未来 detect_size > 缓存短边(640)的模型不得吃偏小缓存(镜像 ai_cache 的
///   「336 绑定模型集」警示,但这里是运行期防护而非注释约定);
/// - 走缓存的条件:降采样有收益(原图短边 > 缓存短边,worker 解码量级级下降),或 worker
///   压根不可解(exotic 原图,host WIC 预解码是唯一通路);短边本就 ≤640 的小图直派更省
///   (预解码不缩尺寸,徒增一次编解码与盘占)。
pub(super) fn face_cache_applies(
    width: i64,
    height: i64,
    decodable: bool,
    detect_size: u32,
) -> bool {
    if detect_size > FACE_CACHE_SHORT_EDGE {
        return false;
    }
    let downscale_wins =
        width > 0 && height > 0 && (width.min(height) as u32) > FACE_CACHE_SHORT_EDGE;
    downscale_wins || !decodable
}

/// 把「检测几何 + 逐脸嵌入」映射为 DB 就绪行:bbox/关键点按解码图 `(img_w, img_h)` 归一化
/// 为 `[0,1]`,quality 同源派生。进程内(detect_and_embed_one)与 worker 派发(几何经协议
/// FaceDet 原样搬回 DetectedFace + 协议回报的实际解码尺寸)两路径共用,保证落库语义逐位一致。
pub(super) fn faces_to_records(
    item_id: i64,
    faces: &[DetectedFace],
    embeddings: &[Vec<f32>],
    img_w: u32,
    img_h: u32,
) -> Vec<NewFace> {
    let (w, h) = (img_w.max(1) as f32, img_h.max(1) as f32);
    faces
        .iter()
        .zip(embeddings)
        .map(|(face, emb)| {
            let quality = face.quality(img_w, img_h);
            let mut lm_flat = [0f32; 10];
            for i in 0..5 {
                lm_flat[i * 2] = (face.landmarks[i][0] / w).clamp(0.0, 1.0);
                lm_flat[i * 2 + 1] = (face.landmarks[i][1] / h).clamp(0.0, 1.0);
            }
            NewFace {
                item_id,
                bbox_x: (face.bbox[0] / w).clamp(0.0, 1.0),
                bbox_y: (face.bbox[1] / h).clamp(0.0, 1.0),
                bbox_w: (face.bbox[2] / w).clamp(0.0, 1.0),
                bbox_h: (face.bbox[3] / h).clamp(0.0, 1.0),
                landmarks: embedding_to_bytes(&lm_flat),
                det_score: face.score,
                quality,
                embedding: embedding_to_bytes(emb),
            }
        })
        .collect()
}
