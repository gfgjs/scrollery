// crates/scrollery-ai-core/src/face.rs
//! 人脸检测 + 对齐 + 嵌入（F2，默认商用轨 YuNet + SFace）。
//!
//! 三步：
//!   1. `detect_faces`：DecodedImage → letterbox → YuNet 推理 → 解码(priors/strides) + NMS
//!      → 反 letterbox 回原图坐标 → `Vec<DetectedFace>`（框 + 5 关键点 + score）。
//!   2. 对齐：5 关键点 → 相似变换(最小二乘，手写) → backward warp 双线性到 112×112。
//!   3. `embed_faces`：对齐图 → 按 `FaceNorm` 预处理 → SFace/ArcFace 推理 → L2 归一化 → 向量。
//!
//! # F8 对拍结论（2026-06-21，OpenCV 4.13 + lena；脚本 tools/face_crosscheck.py、tools/sface_probe.py）
//! 默认轨 YuNet+SFace 已逐项对 OpenCV 验正：
//! - **YuNet 输入**：BGR、0-255、无归一化；输入**固定** [1,3,640,640]。✓（检测框/score/关键点与 OpenCV 吻合）
//! - **YuNet 输出**：12 张量 `cls/obj/bbox/kps × stride{8,16,32}`，shape [1,N,C]（N=6400/1600/400）。✓ 命名精确吻合
//! - **YuNet 解码**：`cx=(col+dx)·s, cy=(row+dy)·s, w=exp(dw)·s, h=exp(dh)·s`；`score=√(cls·obj)`。✓
//! - **SFace 输入**：对齐后 **RGB**（非 BGR！）、0-255、无归一化、112×112。✓ 同关键点 cosine **0.9998**
//!   （cv2.dnn 实测：RGB→1.0 / BGR→0.93 / BGR-归一化→-0.01，即 feature() 内部 swapRB=true）
//! - **对齐**：5 点相似变换 → `arcface_dst` 模板 → 双线性 backward warp。✓ 同关键点逐位一致
//! - 端到端 cosine ~0.92：残差仅来自检测器输入缩放的亚像素差（letterbox vs OpenCV setInputSize），
//!   远超识别阈值 0.363，不影响聚类/检索功能。
//! - SCRFD + ArcFace（非商用轨）尚未对拍，待 F7 接入后用同法验（InsightFace 参考）。

use image::imageops::FilterType;
use ndarray::Array4;
use ort::value::Tensor;
use tracing::debug;

use crate::decoded::DecodedImage;
use crate::embedding::l2_normalize;
use crate::engine::{SessionGuard, SessionPool};
use crate::error::{AiError, Result};
use crate::face_profile::{DetectorKind, FaceNorm, FaceProfile};

/// YuNet 检测的 anchor strides（每个 stride 一组 cls/obj/bbox/kps 输出）。
const STRIDES: [u32; 3] = [8, 16, 32];
/// NMS 的 IoU 阈值。
const NMS_IOU: f32 = 0.3;

/// SCRFD NMS IoU（InsightFace 默认 0.4）。
#[cfg(feature = "face-noncommercial")]
const SCRFD_NMS_IOU: f32 = 0.4;

// 置信度阈值（YuNet/SCRFD）已迁入 `FaceProfile::det_score_thresh`，按轨取值（YuNet 0.9 / SCRFD 0.5）。
// 最小脸过滤阈值见 `FaceProfile::min_face_px`，两者均在 NMS 前生效。
/// SCRFD 每个 anchor 位置的 anchor 数（scrfd_*_bnkps 系为 2）。
#[cfg(feature = "face-noncommercial")]
const SCRFD_NUM_ANCHORS: usize = 2;

// 纯几何类型已外移至 crate::face_types(T16 准备:不在 inference 门内,host 删 ort 后
// worker 派发仍需);此处原位再导出保持既有 `face::DetectedFace` 引用路径不变。
pub use crate::face_types::DetectedFace;

// ── 检测 ────────────────────────────────────────────────────────────────────────

/// 检测一张图中的所有人脸。按 `profile.detector` 分派后处理。
pub fn detect_faces(
    detect_pool: &SessionPool,
    decoded: &DecodedImage,
    profile: &FaceProfile,
) -> Result<Vec<DetectedFace>> {
    match profile.detector {
        DetectorKind::YuNet => detect_yunet(detect_pool, decoded, profile),
        // Part4-T1:非商用轨仅 face-noncommercial build 编译(变体与函数同门控)。
        #[cfg(feature = "face-noncommercial")]
        DetectorKind::Scrfd => detect_scrfd(detect_pool, decoded, profile),
    }
}

/// letterbox 后的几何参数，用于把检测坐标反映射回原图。
struct Letterbox {
    /// 等比缩放因子（new = round(orig·scale)）。
    scale: f32,
    /// 居中填充的左/上偏移（像素，在 detect_size 画布内）。
    pad_x: f32,
    pad_y: f32,
}

/// 把 `DecodedImage`(RGBA) 等比缩放并居中填充到 `size×size` 的 **BGR f32 0-255** NCHW 张量。
/// YuNet 期望 BGR、无归一化（见模块头对拍说明）。返回张量 + 反映射几何。
fn letterbox_bgr(decoded: &DecodedImage, size: u32) -> Result<(Array4<f32>, Letterbox)> {
    let (w, h) = (decoded.width, decoded.height);
    if w == 0 || h == 0 {
        return Err(AiError::Internal("empty image for face detect".into()));
    }
    let scale = size as f32 / w.max(h) as f32;
    let new_w = ((w as f32 * scale).round() as u32).max(1).min(size);
    let new_h = ((h as f32 * scale).round() as u32).max(1).min(size);

    // 用 image crate 双线性缩放（检测无需高质量插值）。
    let src = image::RgbaImage::from_raw(w, h, decoded.pixels.clone())
        .ok_or_else(|| AiError::Internal("RGBA buffer size mismatch".into()))?;
    let resized = image::imageops::resize(&src, new_w, new_h, FilterType::Triangle);

    let pad_x = ((size - new_w) / 2) as f32;
    let pad_y = ((size - new_h) / 2) as f32;
    let s = size as usize;
    let (px, py) = (pad_x as u32, pad_y as u32);

    // 画布默认 0（黑边填充）。通道序 BGR：c0=B,c1=G,c2=R。
    let mut tensor = Array4::<f32>::zeros((1, 3, s, s));
    for y in 0..new_h {
        for x in 0..new_w {
            let p = resized.get_pixel(x, y).0; // [R,G,B,A]
            let (ty, tx) = ((y + py) as usize, (x + px) as usize);
            tensor[[0, 0, ty, tx]] = p[2] as f32; // B
            tensor[[0, 1, ty, tx]] = p[1] as f32; // G
            tensor[[0, 2, ty, tx]] = p[0] as f32; // R
        }
    }
    Ok((
        tensor,
        Letterbox {
            scale,
            pad_x,
            pad_y,
        },
    ))
}

/// 在 session 的输出元数据里按「关键词 + stride 后缀」找到对应输出索引。
/// F8 若实际 onnx 命名不符，改此匹配规则即可。
fn match_output(names: &[String], keyword: &str, stride: u32) -> Result<usize> {
    let sfx = stride.to_string();
    names
        .iter()
        .position(|n| {
            let l = n.to_lowercase();
            l.contains(keyword) && l.ends_with(&sfx)
        })
        .ok_or_else(|| {
            AiError::Internal(format!(
                "YuNet output not found: keyword='{keyword}' stride={stride}, outputs={names:?} | YuNet 输出未找到（F8 须核实命名）"
            ))
        })
}

/// YuNet 推理 + 解码 + NMS + 反 letterbox。
fn detect_yunet(
    detect_pool: &SessionPool,
    decoded: &DecodedImage,
    profile: &FaceProfile,
) -> Result<Vec<DetectedFace>> {
    let size = profile.detect_size;
    let (tensor, lb) = letterbox_bgr(decoded, size)?;
    let s = size as i64;

    let mut guard = detect_pool
        .get()
        .ok_or_else(|| AiError::Internal("face detect pool disconnected".into()))?;

    // run 之前先取输入名与输出名（run 借用 guard 后无法再访问元数据）。
    let input_name = guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Internal("YuNet has no input".into()))?;
    let out_names: Vec<String> = guard
        .outputs()
        .iter()
        .map(|o| o.name().to_string())
        .collect();

    let (flat, _off) = tensor.into_raw_vec_and_offset();
    let input = Tensor::from_array(([1i64, 3, s, s], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), input)])
        .map_err(AiError::Ort)?;

    // 把每个所需输出提取为 owned Vec（后处理需同时持有多张量）。
    let extract = |idx: usize| -> Result<Vec<f32>> {
        let (_shape, slice) = outputs[idx]
            .try_extract_tensor::<f32>()
            .map_err(AiError::Ort)?;
        Ok(slice.to_vec())
    };

    let mut faces: Vec<DetectedFace> = Vec::new();
    for &stride in STRIDES.iter() {
        let cls = extract(match_output(&out_names, "cls", stride)?)?;
        let obj = extract(match_output(&out_names, "obj", stride)?)?;
        let bbox = extract(match_output(&out_names, "bbox", stride)?)?;
        let kps = extract(match_output(&out_names, "kps", stride)?)?;

        let fw = (size / stride) as usize; // feature map 宽=高（方形输入）
        let fh = fw;
        let st = stride as f32;

        for row in 0..fh {
            for col in 0..fw {
                let idx = row * fw + col;
                if idx >= cls.len() || idx >= obj.len() {
                    continue;
                }
                // score = √(cls·obj)（YuNet 约定）。
                let score = (cls[idx].max(0.0) * obj[idx].max(0.0)).sqrt();
                if score < profile.det_score_thresh {
                    continue;
                }
                let b = idx * 4;
                let k = idx * 10;
                if b + 3 >= bbox.len() || k + 9 >= kps.len() {
                    continue;
                }
                // 解码框（letterbox 画布坐标系）：中心+offset，宽高 exp。
                let cx = (col as f32 + bbox[b]) * st;
                let cy = (row as f32 + bbox[b + 1]) * st;
                let bw = bbox[b + 2].exp() * st;
                let bh = bbox[b + 3].exp() * st;
                let x1 = cx - bw / 2.0;
                let y1 = cy - bh / 2.0;

                // 解码 5 关键点（letterbox 画布坐标系）。
                let mut lms = [[0f32; 2]; 5];
                for j in 0..5 {
                    lms[j][0] = (col as f32 + kps[k + 2 * j]) * st;
                    lms[j][1] = (row as f32 + kps[k + 2 * j + 1]) * st;
                }

                // 反 letterbox → 原 DecodedImage 坐标。
                let inv = |vx: f32, vy: f32| -> [f32; 2] {
                    [(vx - lb.pad_x) / lb.scale, (vy - lb.pad_y) / lb.scale]
                };
                let tl = inv(x1, y1);
                let mut landmarks = [[0f32; 2]; 5];
                for j in 0..5 {
                    landmarks[j] = inv(lms[j][0], lms[j][1]);
                }
                // 反 letterbox 后框宽高（原图像素）。短边 < min_face_px 丢弃，剔除远景小框噪点。
                let (ow, oh) = (bw / lb.scale, bh / lb.scale);
                if ow.min(oh) < profile.min_face_px as f32 {
                    continue;
                }
                faces.push(DetectedFace {
                    bbox: [tl[0], tl[1], ow, oh],
                    landmarks,
                    score,
                });
            }
        }
    }

    drop(outputs);
    drop(guard);

    let kept = nms(faces, NMS_IOU);
    debug!(
        "YuNet detected {} face(s) after NMS | YuNet 检测到 {} 张人脸",
        kept.len(),
        kept.len()
    );
    Ok(kept)
}

// ── SCRFD 检测（F7 非商用轨）────────────────────────────────────────────────────
//
// ⚠️ 【UNVERIFIED — 未对拍参考实现，切勿当作已验证】⚠️
// 本函数按 InsightFace SCRFD（scrfd_*_bnkps）的**公开标准后处理**推断写成，但：
//  - scrfd_10g_bnkps.onnx **尚未下载**到本地，本会话**无法**跑通、无法 Python/OpenCV 对拍；
//  - 下列每一项都是**推断**，必须对拍坐实后才可信（F2 前车之鉴：YuNet 当初同样"合理推断"，
//    结果 SFace 通道序 BGR/RGB 错了，端到端 cosine 0.93，唯有 F8 OpenCV 对拍才抓出）：
//    ① 输出顺序：假设 9 个输出按「score×3, bbox×3, kps×3」且各组按 stride{8,16,32} 升序排列
//       （部分导出按 stride 分组或用纯数字名 → 须核 onnx 实际 outputs() 名/序）；
//    ② 预处理：假设 RGB、(x-127.5)/128.0、letterbox **左上角** pad（非 YuNet 的居中）；
//    ③ anchor：假设 num_anchors=2、center=(col·s, row·s)、同位置 anchor 相邻排列；
//    ④ score 假设已 sigmoid（直接是概率，不再激活）；
//    ⑤ bbox 用 distance2bbox（中心 ± 距离·stride）、kps = center + 偏移·stride。
// 落地前务必：下载 scrfd onnx → 用 InsightFace Python 参考对同图比对框/关键点/score（仿 F8）。
// ArcFace 嵌入侧（FaceNorm::ArcFaceStd）已在 F2 写就，同样待对拍坐实。

/// 把 `DecodedImage`(RGBA) 等比缩放并**左上角**填充到 `size×size` 的 RGB f32 标准化 NCHW 张量。
/// SCRFD/InsightFace 预处理：RGB、(x-127.5)/128.0、右下黑边。返回张量 + 等比缩放因子 `det_scale`
/// （反映射用：orig = canvas / det_scale，无 pad 偏移，因 pad 在右下）。
#[cfg(feature = "face-noncommercial")]
fn letterbox_rgb_std(decoded: &DecodedImage, size: u32) -> Result<(Array4<f32>, f32)> {
    let (w, h) = (decoded.width, decoded.height);
    if w == 0 || h == 0 {
        return Err(AiError::Internal("empty image for SCRFD detect".into()));
    }
    // 等比：长边缩放到 size。det_scale = new/orig（宽高一致）。
    let det_scale = size as f32 / w.max(h) as f32;
    let new_w = ((w as f32 * det_scale).round() as u32).max(1).min(size);
    let new_h = ((h as f32 * det_scale).round() as u32).max(1).min(size);

    let src = image::RgbaImage::from_raw(w, h, decoded.pixels.clone())
        .ok_or_else(|| AiError::Internal("RGBA buffer size mismatch".into()))?;
    let resized = image::imageops::resize(&src, new_w, new_h, FilterType::Triangle);

    let s = size as usize;
    // 画布默认 0（右下黑边）。通道序 RGB；归一化 (x-127.5)/128。
    let mut tensor = Array4::<f32>::zeros((1, 3, s, s));
    for y in 0..new_h {
        for x in 0..new_w {
            let p = resized.get_pixel(x, y).0; // [R,G,B,A]
            let (ty, tx) = (y as usize, x as usize); // 左上角 pad → 原点对齐
            tensor[[0, 0, ty, tx]] = (p[0] as f32 - 127.5) / 128.0; // R
            tensor[[0, 1, ty, tx]] = (p[1] as f32 - 127.5) / 128.0; // G
            tensor[[0, 2, ty, tx]] = (p[2] as f32 - 127.5) / 128.0; // B
        }
    }
    Ok((tensor, det_scale))
}

/// SCRFD 推理 + distance2bbox 解码 + NMS + 反映射。见上方 UNVERIFIED 警告。
#[cfg(feature = "face-noncommercial")]
fn detect_scrfd(
    detect_pool: &SessionPool,
    decoded: &DecodedImage,
    profile: &FaceProfile,
) -> Result<Vec<DetectedFace>> {
    let size = profile.detect_size;
    let (tensor, det_scale) = letterbox_rgb_std(decoded, size)?;
    let s = size as i64;

    let mut guard = detect_pool
        .get()
        .ok_or_else(|| AiError::Internal("face detect pool disconnected".into()))?;
    let input_name = guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Internal("SCRFD has no input".into()))?;
    let n_outputs = guard.outputs().len();
    // 期望 9 输出（fmc=3，bnkps 含关键点）。数量不符直接报错，提示对拍核实导出。
    if n_outputs < 9 {
        return Err(AiError::Internal(format!(
            "SCRFD expected ≥9 outputs (score/bbox/kps × 3 strides), got {n_outputs} | SCRFD 输出数不符（须对拍核实）"
        )));
    }

    let (flat, _off) = tensor.into_raw_vec_and_offset();
    let input = Tensor::from_array(([1i64, 3, s, s], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), input)])
        .map_err(AiError::Ort)?;

    let extract = |idx: usize| -> Result<Vec<f32>> {
        let (_shape, slice) = outputs[idx]
            .try_extract_tensor::<f32>()
            .map_err(AiError::Ort)?;
        Ok(slice.to_vec())
    };

    let mut faces: Vec<DetectedFace> = Vec::new();
    // 输出索引假设：score=[0,1,2], bbox=[3,4,5], kps=[6,7,8]，各组按 stride{8,16,32}。
    for (gi, &stride) in STRIDES.iter().enumerate() {
        let score = extract(gi)?;
        let bbox = extract(3 + gi)?;
        let kps = extract(6 + gi)?;

        let fw = (size / stride) as usize;
        let fh = fw; // 方形输入
        let st = stride as f32;

        // anchor 顺序：每个位置 (row,col) 连续 num_anchors 个，center=(col·s,row·s)。
        for pos in 0..(fh * fw) {
            let row = pos / fw;
            let col = pos % fw;
            for a in 0..SCRFD_NUM_ANCHORS {
                let idx = pos * SCRFD_NUM_ANCHORS + a;
                if idx >= score.len() {
                    continue;
                }
                let sc = score[idx];
                if sc < profile.det_score_thresh {
                    continue;
                }
                let b = idx * 4;
                let k = idx * 10;
                if b + 3 >= bbox.len() || k + 9 >= kps.len() {
                    continue;
                }
                let cx = col as f32 * st;
                let cy = row as f32 * st;
                // distance2bbox：中心 ∓ 距离·stride。
                let x1 = cx - bbox[b] * st;
                let y1 = cy - bbox[b + 1] * st;
                let x2 = cx + bbox[b + 2] * st;
                let y2 = cy + bbox[b + 3] * st;

                let mut landmarks = [[0f32; 2]; 5];
                for j in 0..5 {
                    // kps = center + 偏移·stride；反映射除以 det_scale（左上角 pad 无偏移）。
                    let px = cx + kps[k + 2 * j] * st;
                    let py = cy + kps[k + 2 * j + 1] * st;
                    landmarks[j] = [px / det_scale, py / det_scale];
                }
                // 原图坐标系框宽高；短边 < min_face_px 丢弃。
                let (ow, oh) = ((x2 - x1) / det_scale, (y2 - y1) / det_scale);
                if ow.min(oh) < profile.min_face_px as f32 {
                    continue;
                }
                faces.push(DetectedFace {
                    bbox: [x1 / det_scale, y1 / det_scale, ow, oh],
                    landmarks,
                    score: sc,
                });
            }
        }
    }

    drop(outputs);
    drop(guard);

    let kept = nms(faces, SCRFD_NMS_IOU);
    debug!(
        "SCRFD detected {} face(s) after NMS [UNVERIFIED] | SCRFD 检测到 {} 张人脸（未对拍）",
        kept.len(),
        kept.len()
    );
    Ok(kept)
}

/// 标准 NMS：按 score 降序，抑制与已留框 IoU > 阈值者。
fn nms(mut faces: Vec<DetectedFace>, iou_thresh: f32) -> Vec<DetectedFace> {
    faces.sort_unstable_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut keep: Vec<DetectedFace> = Vec::new();
    for cand in faces {
        if keep.iter().all(|k| iou(&cand.bbox, &k.bbox) <= iou_thresh) {
            keep.push(cand);
        }
    }
    keep
}

/// 两个 [x,y,w,h] 框的 IoU。
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (ax2, ay2) = (a[0] + a[2], a[1] + a[3]);
    let (bx2, by2) = (b[0] + b[2], b[1] + b[3]);
    let ix1 = a[0].max(b[0]);
    let iy1 = a[1].max(b[1]);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);
    let iw = (ix2 - ix1).max(0.0);
    let ih = (iy2 - iy1).max(0.0);
    let inter = iw * ih;
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

// ── 对齐 ────────────────────────────────────────────────────────────────────────

/// 估计「源点(landmarks) → 目标点(模板)」的相似变换参数 `(a, b, tx, ty)`，
/// 其中变换为 `u = a·x − b·y + tx, v = b·x + a·y + ty`（旋转+等比缩放+平移，4 自由度）。
///
/// 用线性最小二乘（5 点 → 10 方程 → 4 未知），手写法方程 + 4×4 高斯消元（无 nalgebra）。
/// 这与 OpenCV `estimateAffinePartial2D` 同解（相似变换最小二乘）。
fn estimate_similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> Option<[f64; 4]> {
    // 法方程 ATA·p = ATb，p=[a,b,tx,ty]。每点两行：
    //   [x, -y, 1, 0]·p = u ; [y, x, 0, 1]·p = v
    let mut ata = [[0f64; 4]; 4];
    let mut atb = [0f64; 4];
    for i in 0..5 {
        let (x, y) = (src[i][0] as f64, src[i][1] as f64);
        let (u, v) = (dst[i][0] as f64, dst[i][1] as f64);
        let rows = [[x, -y, 1.0, 0.0], [y, x, 0.0, 1.0]];
        let tgts = [u, v];
        for r in 0..2 {
            for j in 0..4 {
                for kk in 0..4 {
                    ata[j][kk] += rows[r][j] * rows[r][kk];
                }
                atb[j] += rows[r][j] * tgts[r];
            }
        }
    }
    solve4(ata, atb)
}

/// 解 4×4 线性系统（高斯消元 + 部分主元）。奇异返回 None。
fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        // 选主元
        let mut piv = col;
        for r in (col + 1)..4 {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        // 消元
        for r in 0..4 {
            if r == col {
                continue;
            }
            let f = a[r][col] / a[col][col];
            // 高斯消元行运算：同一索引 c 同时寻址 a[r][c] 与 a[col][c]，显式下标更直观。
            #[allow(clippy::needless_range_loop)]
            for c in col..4 {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    Some([
        b[0] / a[0][0],
        b[1] / a[1][1],
        b[2] / a[2][2],
        b[3] / a[3][3],
    ])
}

/// 把一张脸对齐裁剪为 `size×size` 的 RGB f32（0-255，HWC 顺序），用 backward warp + 双线性。
/// 通道序统一产出 RGB，BGR 转换与归一化在 `build_embed_tensor` 按 `FaceNorm` 处理。
fn align_face(
    decoded: &DecodedImage,
    face: &DetectedFace,
    profile: &FaceProfile,
) -> Result<Vec<f32>> {
    let size = profile.embed_size as usize;
    // 求 src(landmarks) → dst(模板) 的相似变换 T=(a,b,tx,ty)。
    let p = estimate_similarity(&face.landmarks, &profile.align_template)
        .ok_or_else(|| AiError::Internal("similarity transform singular | 相似变换奇异".into()))?;
    let (a, b, tx, ty) = (p[0], p[1], p[2], p[3]);
    // 逆变换（dst→src）：R=[[a,-b],[b,a]] 的逆 = (1/det)[[a,b],[-b,a]]，det=a²+b²。
    let det = a * a + b * b;
    if det.abs() < 1e-12 {
        return Err(AiError::Internal("degenerate transform".into()));
    }
    let (w, h) = (decoded.width as i64, decoded.height as i64);
    let mut out = vec![0f32; size * size * 3];

    for v in 0..size {
        for u in 0..size {
            // 目标像素 (u,v) → 源坐标 (sx,sy)。
            let du = u as f64 - tx;
            let dv = v as f64 - ty;
            let sx = (a * du + b * dv) / det;
            let sy = (-b * du + a * dv) / det;
            let rgb = sample_bilinear_rgb(decoded, sx, sy, w, h);
            let o = (v * size + u) * 3;
            out[o] = rgb[0];
            out[o + 1] = rgb[1];
            out[o + 2] = rgb[2];
        }
    }
    Ok(out)
}

/// 双线性采样 `DecodedImage`(RGBA) 在浮点坐标 `(x,y)` 的 RGB（0-255）。越界返回黑。
fn sample_bilinear_rgb(decoded: &DecodedImage, x: f64, y: f64, w: i64, h: i64) -> [f32; 3] {
    if x < 0.0 || y < 0.0 || x > (w - 1) as f64 || y > (h - 1) as f64 {
        return [0.0, 0.0, 0.0];
    }
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = (x - x0 as f64) as f32;
    let fy = (y - y0 as f64) as f32;
    let px = |xi: i64, yi: i64, c: usize| -> f32 {
        let idx = ((yi * w + xi) as usize) * 4 + c;
        decoded.pixels.get(idx).copied().unwrap_or(0) as f32
    };
    let mut rgb = [0f32; 3];
    // 双线性插值：索引 c 既选通道又传入 px(...,c)，显式下标比迭代器更贴合像素数学。
    #[allow(clippy::needless_range_loop)]
    for c in 0..3 {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bot = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        rgb[c] = top * (1.0 - fy) + bot * fy;
    }
    rgb
}

// ── 嵌入 ────────────────────────────────────────────────────────────────────────

/// 把对齐后的 RGB(0-255, HWC) 图按 `FaceNorm` 追加为一个 [3,size,size] 的 CHW f32 块到 `out`。
/// 批量时多次调用即拼成 [N,3,size,size]。
///
/// - SFace：RGB、保持 0-255、不减均值（OpenCV FaceRecognizerSF，feature() 内部 swapRB=true）。
///   【F8 对拍坐实】cv2.dnn 喂同一对齐图：RGB→cosine 1.0，BGR→0.93，BGR/归一化→-0.01。
/// - ArcFaceStd：RGB、`(x-127.5)/127.5`。
///
/// 两种归一化的通道序都是 RGB 恒等，故平面 c 直接取 `rgb[o+c]`，无需 BGR 交换。
fn push_embed_chw(rgb: &[f32], size: usize, norm: FaceNorm, out: &mut Vec<f32>) {
    for c in 0..3 {
        for y in 0..size {
            for x in 0..size {
                let v = rgb[(y * size + x) * 3 + c];
                let nv = match norm {
                    FaceNorm::SFace => v,
                    #[cfg(feature = "face-noncommercial")]
                    FaceNorm::ArcFaceStd => (v - 127.5) / 127.5,
                };
                out.push(nv);
            }
        }
    }
}

/// 维度兜底校验（F8 排错线索）：实际维度与 profile 不符则 warn，但仍按实际维度归一化存储。
fn warn_embed_dim(actual: usize, profile: &FaceProfile) {
    if actual != profile.embed_dim {
        tracing::warn!(
            "face embedding dim {} != profile {} ({}) | 人脸嵌入维度不符（F8 须核实）",
            actual,
            profile.embed_dim,
            profile.id
        );
    }
}

/// 取 embed session 的首输入名（run 借用 guard 前先取）。
fn embed_input_name(guard: &SessionGuard) -> Result<String> {
    guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Internal("face embedder has no input".into()))
}

/// 逐脸嵌入一张对齐图（CHW 已由 `push_embed_chw` 约定）。批量回退路径与 N==1 走此。
fn embed_one(
    embed_pool: &SessionPool,
    rgb: &[f32],
    size: usize,
    profile: &FaceProfile,
) -> Result<Vec<f32>> {
    let mut flat = Vec::with_capacity(3 * size * size);
    push_embed_chw(rgb, size, profile.embed_norm, &mut flat);

    let mut guard = embed_pool
        .get()
        .ok_or_else(|| AiError::Internal("face embed pool disconnected".into()))?;
    let input_name = embed_input_name(&guard)?;
    let input =
        Tensor::from_array(([1i64, 3, size as i64, size as i64], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), input)])
        .map_err(AiError::Ort)?;
    let (_shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;
    let emb: Vec<f32> = slice.to_vec();
    drop(outputs);
    drop(guard);

    warn_embed_dim(emb.len(), profile);
    Ok(l2_normalize(emb))
}

/// 批量嵌入：一张图的多张对齐脸堆成 [N,3,size,size] 一次 `run()`（问题6b 提速）。
/// 输出期望 [N, dim]（行主序）；按 N 等分。模型 batch 维固定为 1 时 `run()` 会报错，由
/// `embed_faces` 捕获并回退逐脸——故此处失败不致命。
fn embed_batch(
    embed_pool: &SessionPool,
    aligned: &[Vec<f32>],
    size: usize,
    profile: &FaceProfile,
) -> Result<Vec<Vec<f32>>> {
    let n = aligned.len();
    let mut flat = Vec::with_capacity(n * 3 * size * size);
    for rgb in aligned {
        push_embed_chw(rgb, size, profile.embed_norm, &mut flat);
    }

    let mut guard = embed_pool
        .get()
        .ok_or_else(|| AiError::Internal("face embed pool disconnected".into()))?;
    let input_name = embed_input_name(&guard)?;
    let input = Tensor::from_array(([n as i64, 3, size as i64, size as i64], flat))
        .map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), input)])
        .map_err(AiError::Ort)?;
    let (_shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;
    let total = slice.len();
    if n == 0 || total % n != 0 {
        return Err(AiError::Internal(format!(
            "batch face embed output {total} not divisible by N={n} | 批量嵌入输出维度异常"
        )));
    }
    let dim = total / n;
    warn_embed_dim(dim, profile);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(l2_normalize(slice[i * dim..(i + 1) * dim].to_vec()));
    }
    drop(outputs);
    drop(guard);
    Ok(out)
}

/// 对齐 + 嵌入一组人脸，返回各自的 L2 归一化向量（维度 = `profile.embed_dim`）。
/// N>1 先试批量一次 `run()`（提速）；失败（模型 batch 维固定）回退逐脸，保证不回归。
pub fn embed_faces(
    embed_pool: &SessionPool,
    decoded: &DecodedImage,
    faces: &[DetectedFace],
    profile: &FaceProfile,
) -> Result<Vec<Vec<f32>>> {
    if faces.is_empty() {
        return Ok(Vec::new());
    }
    let size = profile.embed_size as usize;
    // 对齐全部脸（CPU；align_face 内部已是双线性 warp，量小串行即可）。
    let aligned: Vec<Vec<f32>> = faces
        .iter()
        .map(|f| align_face(decoded, f, profile))
        .collect::<Result<_>>()?;

    if aligned.len() > 1 {
        match embed_batch(embed_pool, &aligned, size, profile) {
            Ok(v) => return Ok(v),
            Err(e) => {
                debug!(
                    "batch face embed failed, falling back to per-face | 批量人脸嵌入失败，回退逐脸: {}",
                    e
                );
            }
        }
    }

    let mut out = Vec::with_capacity(aligned.len());
    for rgb in &aligned {
        out.push(embed_one(embed_pool, rgb, size, profile)?);
    }
    Ok(out)
}
