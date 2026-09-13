// src-tauri/src/ai/face_pipeline/decode_source.rs
//! 解码源三级定源(T16-R2 方案 A):在「常规分档缩略图」「host 预解码 face 缓存」
//! 「原图直派」之间选择,及检测/嵌入结果向 DB 行的映射。

use std::path::PathBuf;

use crate::ai::clip::embedding_to_bytes;
use crate::ai::face::DetectedFace;
use crate::db::queries::NewFace;
use crate::state::AppState;
use crate::thumbnail::cache::FACE_CACHE_SHORT_EDGE;

use super::FaceTask;

/// 已解析的解码源：解码哪个文件、按什么格式。
pub(super) struct FaceDecodeSource {
    pub(super) path: PathBuf,
    pub(super) format: String,
    pub(super) kind: FaceSourceKind,
}

/// 解码源三级构成(T16-R2 方案 A):诊断计数随批输出;原图直派常态应只剩短边 ≤640 的小图,
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
/// 白名单外(heic/raw/psd 等)的原图回退项恒走 host 预解码的 face 缓存(方案 A,WIC 引擎可解
/// heic 等)——原「exotic 原图跳过」的过渡缺口就此收敛;仅 detect_size 超缓存尺寸的未来模型
/// 防呆分支仍会跳过(见 dispatch_face_batch 阶段1)。
pub(super) const WORKER_DECODABLE_FORMATS: &[&str] =
    &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];

/// 选择短边仍满足 `detect_size`（640）的最廉价解码源。
///
/// 故意不与 `pipeline.rs` 的 `resolve_decode_source` 共用（原因见模块头：那个函数优先级 1 的
/// AI 缓存捷径固定短边 336px，对 YuNet 的 640px 输入而言太小）。此版本只在「常规分档缩略图」
/// 与「原图」之间选择——不涉及 AI 缓存。
pub(super) fn resolve_face_decode_source(
    task: &FaceTask,
    state: &AppState,
    detect_size: u32,
) -> FaceDecodeSource {
    let original = FaceDecodeSource {
        path: task.source_path.clone(),
        format: task.file_format.clone(),
        kind: FaceSourceKind::Original,
    };

    if task.thumb_status != 1 || task.width <= 0 || task.height <= 0 {
        return original;
    }
    let Some(rel) = task.thumb_path.as_deref() else {
        return original;
    };

    // 从相对路径 "{档位}/{前缀}/{hex}.webp" 解析档位（长边）。
    let Some(tier) = rel.split('/').next().and_then(|s| s.parse::<u32>().ok()) else {
        return original;
    };

    // Thumbnail is LongEdge(tier) but never upscaled → long edge = min(tier, max(W,H)).
    // Predict short edge WITHOUT touching disk; use the thumbnail only if it's ≥ detect_size.
    // 缩略图按长边=tier 缩放但绝不放大 → 长边 = min(tier, max(W,H))；不读盘预测短边，
    // 仅当 ≥ detect_size 时采用。
    let (w, h) = (task.width as u32, task.height as u32);
    let (long, short) = (w.max(h), w.min(h));
    let thumb_short = if long <= tier {
        short
    } else {
        (short as f32 * tier as f32 / long as f32).round() as u32
    };
    if thumb_short < detect_size {
        return original;
    }

    let cache_dir = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .cache_dir
        .clone();
    let thumb_full = cache_dir.join("thumbnails").join(rel);
    if thumb_full.exists() {
        // 缩略图生成时已转正——用"webp"格式使 WIC 的旋转分支（仅 jpg/heic）不触发，避免二次转向。
        FaceDecodeSource {
            path: thumb_full,
            format: "webp".to_string(),
            kind: FaceSourceKind::Thumb,
        }
    } else {
        original
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 方案 A 决策核矩阵:防呆(detect_size 超缓存短边)恒 false;大图/不可解格式走缓存;
    /// 小图或尺寸未知的可解格式直派。
    #[test]
    fn face_cache_applies_matrix() {
        // 大图(短边 > 640):无论 worker 是否可解,都走缓存(降采样收益量级级)。
        assert!(face_cache_applies(4000, 3000, true, 640));
        assert!(face_cache_applies(4000, 3000, false, 640));
        // 小图(短边 ≤ 640):可解格式直派;不可解格式仍须缓存(host 预解码是唯一通路)。
        assert!(!face_cache_applies(800, 600, true, 640));
        assert!(face_cache_applies(800, 600, false, 640));
        // 宽幅全景:短边 600 ≤ 640 → 可解直派(长边虽大,沿用既有直派语义)。
        assert!(!face_cache_applies(6000, 600, true, 640));
        // 尺寸未知(0):可解直派(维持既有语义),不可解走缓存兜底。
        assert!(!face_cache_applies(0, 0, true, 640));
        assert!(face_cache_applies(0, 0, false, 640));
        // 防呆:detect_size 超缓存短边 → 一律不吃偏小缓存(可解回退全尺寸,不可解跳过)。
        assert!(!face_cache_applies(4000, 3000, true, 1024));
        assert!(!face_cache_applies(4000, 3000, false, 1024));
    }

    /// 边界:短边恰等于 640 → 缓存无降采样收益,可解格式直派;641 起走缓存。
    #[test]
    fn face_cache_applies_boundary_equal() {
        assert!(!face_cache_applies(640, 960, true, 640));
        assert!(face_cache_applies(641, 960, true, 640));
    }
}
