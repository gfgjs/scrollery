// src-tauri/src/exotic/validate.rs
//! Worker 输出的纯校验器(U-P3,2026-07-16 从 worker.rs 拆出)。
//!
//! Host **不信任** Worker 返回值(§3.7):这些函数用独立解码/结构核对验证输出,
//! 与 [`super::worker::WorkerConn`] 状态机无共享状态——除缩略图校验被
//! `run_thumbnail` 内联调用外,主要消费者是 `ai::worker_client`、`ai::face_pipeline`
//! 与 `bin/worker_e2e`。旧路径 `exotic::worker::validate_*` 经 worker.rs 的
//! `pub use` 继续可用。

use exotic_protocol::{
    EmbedItem, EmbedResult, FaceItem, FaceItemResult, OcrItem, OcrItemResult, OcrLine, RequestBody,
    SuccessBody, WorkerErrorCode,
};

use super::outcome::{EmbedItemOutcome, FaceItemOutcome};
use super::worker::WorkerLimits;

/// 验证缩略图 Success（§3.7）：core/request 核对 + 独立解码器验真尺寸 + 上限。返回 (w,h,mime)。
///
/// 用 `image` crate 解码 WebP（独立于 Worker 声明）得到**真实**尺寸——既验证 WebP 自洽，
/// 又拿到与声明对照的实际宽高（Worker 声明不可信）。缩略图体积小，解码开销可忽略。
pub fn validate_thumbnail_output(
    req: &RequestBody,
    body: &SuccessBody,
    blob: &[u8],
    limits: &WorkerLimits,
) -> Result<(u32, u32, String), String> {
    // 核对 item / fingerprint（防错序串扰）。
    if body.item_id != req.item_id() {
        return Err(format!(
            "item_id 错配：{:?} != {:?}",
            body.item_id,
            req.item_id()
        ));
    }
    if body.input_fingerprint.as_deref() != req.input_fingerprint() {
        return Err("fingerprint 错配".into());
    }
    // mime 必须 image/webp。
    let mime = body.mime.clone().unwrap_or_default();
    if mime != "image/webp" {
        return Err(format!("mime 非 image/webp：{mime}"));
    }
    // blob 非空且不超上限。
    if blob.is_empty() {
        return Err("blob 为空".into());
    }
    if blob.len() as u64 > limits.max_blob_len as u64 {
        return Err(format!(
            "blob 超限：{} > {}",
            blob.len(),
            limits.max_blob_len
        ));
    }
    // WebP 魔数（RIFF....WEBP）。
    if blob.len() < 12 || &blob[0..4] != b"RIFF" || &blob[8..12] != b"WEBP" {
        return Err("WebP 魔数非法".into());
    }
    // 独立解码取真实尺寸（同时验证 WebP 自洽）。
    let img = image::load_from_memory_with_format(blob, image::ImageFormat::WebP)
        .map_err(|e| format!("WebP 独立解码失败：{e}"))?;
    use image::GenericImageView;
    let (aw, ah) = img.dimensions();
    // 声明尺寸（若有）必须与实际一致。
    if let Some(dw) = body.width {
        if dw != aw {
            return Err(format!("声明宽 {dw} != 实际 {aw}"));
        }
    }
    if let Some(dh) = body.height {
        if dh != ah {
            return Err(format!("声明高 {dh} != 实际 {ah}"));
        }
    }
    // 长边不超过请求档位 + 容差。
    if let RequestBody::Thumbnail {
        target_long_edge, ..
    } = req
    {
        let long = aw.max(ah);
        if long > target_long_edge.saturating_add(limits.long_edge_tolerance) {
            return Err(format!(
                "长边 {long} 超过档位 {target_long_edge}+容差 {}",
                limits.long_edge_tolerance
            ));
        }
    }
    // 总像素不超上限。
    let pixels = (aw as u64).saturating_mul(ah as u64);
    if pixels > limits.max_output_pixels {
        return Err(format!("像素 {pixels} 超上限 {}", limits.max_output_pixels));
    }
    Ok((aw, ah, mime))
}

/// 默认缩略图上限：64 MiB blob、4 兆像素（足够 960 档）、64px 长边容差。
pub fn default_thumbnail_limits() -> WorkerLimits {
    WorkerLimits {
        max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        max_output_pixels: 4_000_000,
        long_edge_tolerance: 64,
    }
}

// ── v2 批量输出校验(T15,D3 §4①:「embed 批的输出校验 = 维度×数量一致性」)────────────
// Host 不信任 Worker(§3.7)在 v2 上的延伸:results 严格同序同长、逐项 item/fingerprint
// 核对、blob 长度精确等于 Ok 项载荷之和。任一不符 → Err(协议违例,调用方 kill 回收);
// 逐项 Err 是数据结果、不判违例。T17 派发器直接消费这两个纯函数。

/// 校验 EmbedBatch 的 Success 输出并切出各项嵌入。`embed_dim` 取自 SessionReady。
pub fn validate_embed_batch_output(
    items: &[EmbedItem],
    body: &SuccessBody,
    blob: &[u8],
    embed_dim: usize,
) -> Result<Vec<EmbedItemOutcome>, String> {
    let batch = body
        .embed
        .as_ref()
        .ok_or("EmbedBatch Success 缺 embed 应答体")?;
    if batch.results.len() != items.len() {
        return Err(format!(
            "results 长度错配：{} != items {}",
            batch.results.len(),
            items.len()
        ));
    }
    if embed_dim == 0 {
        return Err("embed_dim 为 0".into());
    }
    let item_bytes = embed_dim * 4;
    let ok_count = batch
        .results
        .iter()
        .filter(|r| matches!(r, EmbedResult::Ok { .. }))
        .count();
    if blob.len() != ok_count * item_bytes {
        return Err(format!(
            "blob 长度错配：{} != {}×{}",
            blob.len(),
            ok_count,
            item_bytes
        ));
    }

    let mut out = Vec::with_capacity(items.len());
    let mut off = 0usize;
    for (i, r) in batch.results.iter().enumerate() {
        let (rid, rfp) = match r {
            EmbedResult::Ok {
                item_id,
                fingerprint,
            }
            | EmbedResult::Err {
                item_id,
                fingerprint,
                ..
            } => (*item_id, fingerprint.as_str()),
        };
        // 同序核对:错序/陈旧结果即违例(延续单项 input_fingerprint 核对语义到批量)。
        if rid != items[i].item_id || rfp != items[i].fingerprint {
            return Err(format!("第 {i} 项 item/fingerprint 错配"));
        }
        match r {
            EmbedResult::Ok { .. } => {
                let emb: Vec<f32> = blob[off..off + item_bytes]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                off += item_bytes;
                out.push(EmbedItemOutcome::Ok(emb));
            }
            EmbedResult::Err { code, .. } => out.push(EmbedItemOutcome::Err(*code)),
        }
    }
    Ok(out)
}

/// 校验 FaceDetectEmbed 的 Success 输出并按「Ok 项序 × 项内脸序」切出嵌入。
/// `face_embed_dim` 取自 SessionReady(未载人脸角色时本函数不应被调用)。
pub fn validate_face_batch_output(
    items: &[FaceItem],
    body: &SuccessBody,
    blob: &[u8],
    face_embed_dim: usize,
) -> Result<Vec<FaceItemOutcome>, String> {
    let batch = body
        .face
        .as_ref()
        .ok_or("FaceDetectEmbed Success 缺 face 应答体")?;
    if batch.results.len() != items.len() {
        return Err(format!(
            "results 长度错配：{} != items {}",
            batch.results.len(),
            items.len()
        ));
    }
    if face_embed_dim == 0 {
        return Err("face_embed_dim 为 0".into());
    }
    let face_bytes = face_embed_dim * 4;
    let total_faces: usize = batch
        .results
        .iter()
        .map(|r| match r {
            FaceItemResult::Ok { faces, .. } => faces.len(),
            FaceItemResult::Err { .. } => 0,
        })
        .sum();
    if blob.len() != total_faces * face_bytes {
        return Err(format!(
            "blob 长度错配：{} != {}×{}",
            blob.len(),
            total_faces,
            face_bytes
        ));
    }

    let mut out = Vec::with_capacity(items.len());
    let mut off = 0usize;
    for (i, r) in batch.results.iter().enumerate() {
        let (rid, rfp) = match r {
            FaceItemResult::Ok {
                item_id,
                fingerprint,
                ..
            }
            | FaceItemResult::Err {
                item_id,
                fingerprint,
                ..
            } => (*item_id, fingerprint.as_str()),
        };
        if rid != items[i].item_id || rfp != items[i].fingerprint {
            return Err(format!("第 {i} 项 item/fingerprint 错配"));
        }
        match r {
            FaceItemResult::Ok {
                faces,
                width,
                height,
                ..
            } => {
                // 解码尺寸为 0 = 旧帧缺字段或 worker bug——归一化会除坏,按协议违例回收。
                if *width == 0 || *height == 0 {
                    return Err(format!("第 {i} 项解码尺寸为 0({width}×{height})"));
                }
                let mut embeddings = Vec::with_capacity(faces.len());
                for _ in 0..faces.len() {
                    let emb: Vec<f32> = blob[off..off + face_bytes]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    off += face_bytes;
                    embeddings.push(emb);
                }
                out.push(FaceItemOutcome::Ok {
                    faces: faces.clone(),
                    embeddings,
                    width: *width,
                    height: *height,
                });
            }
            FaceItemResult::Err { code, .. } => out.push(FaceItemOutcome::Err(*code)),
        }
    }
    Ok(out)
}

/// OCR 批单项校验产物：Ok 携行集 + 解码尺寸;Err 携稳定错误码（逐项不连坐,
/// 同 [`FaceItemOutcome`] 语义）。定义就近于 [`validate_ocr_batch_output`]。
#[derive(Debug, Clone, PartialEq)]
pub enum OcrItemOutcome {
    Ok {
        lines: Vec<OcrLine>,
        width: u32,
        height: u32,
    },
    Err {
        code: WorkerErrorCode,
    },
}

/// 校验 OcrBatch 的 Success 输出（「不信任 worker」§3.7 在 OCR 上的延伸）:
/// `body.ocr` 必在、results 严格同序同长、逐项 item_id+fingerprint 与请求核对;Ok 项
/// width/height>0、quad 八坐标 finite 且 ∈[-1e4,1e5]、confidence∈[0,1];blob 恒空
/// （D-OCR-4,OCR 结果走 JSON)。任一不符 → Err(触发调用方弃实例重发);逐项 Err 是
/// 数据结果、不判违例。与 embed/face 批同纪律,blob 校验方向相反（此处必须为空）。
pub fn validate_ocr_batch_output(
    items: &[OcrItem],
    body: &SuccessBody,
    blob: &[u8],
) -> Result<Vec<OcrItemOutcome>, String> {
    let batch = body.ocr.as_ref().ok_or("OcrBatch Success 缺 ocr 应答体")?;
    if batch.results.len() != items.len() {
        return Err(format!(
            "results 长度错配：{} != items {}",
            batch.results.len(),
            items.len()
        ));
    }
    // OCR 结果结构化走 JSON,blob 恒空(D-OCR-4):非空即协议违例。
    if !blob.is_empty() {
        return Err(format!("OcrBatch blob 应为空,实得 {} 字节", blob.len()));
    }

    let mut out = Vec::with_capacity(items.len());
    for (i, r) in batch.results.iter().enumerate() {
        let (rid, rfp) = match r {
            OcrItemResult::Ok {
                item_id,
                fingerprint,
                ..
            }
            | OcrItemResult::Err {
                item_id,
                fingerprint,
                ..
            } => (*item_id, fingerprint.as_str()),
        };
        // 同序核对:错序/陈旧结果即违例(延续单项 fingerprint 核对语义到批量)。
        if rid != items[i].item_id || rfp != items[i].fingerprint {
            return Err(format!("第 {i} 项 item/fingerprint 错配"));
        }
        match r {
            OcrItemResult::Ok {
                lines,
                width,
                height,
                ..
            } => {
                // 解码尺寸为 0 = quad 坐标系失真,按协议违例回收。
                if *width == 0 || *height == 0 {
                    return Err(format!("第 {i} 项解码尺寸为 0({width}×{height})"));
                }
                for (j, line) in lines.iter().enumerate() {
                    if !(0.0..=1.0).contains(&line.confidence) {
                        return Err(format!(
                            "第 {i} 项第 {j} 行 confidence 越界:{}",
                            line.confidence
                        ));
                    }
                    // 区间检查已拒 NaN/±Inf(contains 对非有限值恒 false),is_finite 仅为显式意图声明。
                    for pt in &line.quad {
                        for &c in pt {
                            if !c.is_finite() || !(-1e4..=1e5).contains(&c) {
                                return Err(format!("第 {i} 项第 {j} 行 quad 坐标非法:{c}"));
                            }
                        }
                    }
                }
                out.push(OcrItemOutcome::Ok {
                    lines: lines.clone(),
                    width: *width,
                    height: *height,
                });
            }
            OcrItemResult::Err { code, .. } => out.push(OcrItemOutcome::Err { code: *code }),
        }
    }
    Ok(out)
}

/// 校验 EncodeText 的 Success 输出并切出各文本向量(T17)。全批原子(无逐项结构),
/// 校验 = 应答体 count 与请求 texts 数一致 + blob 长度精确等于 count×embed_dim×4;
/// 任一不符即协议违例(调用方 kill 回收),与 embed/face 批的「不信任 worker」同纪律。
pub fn validate_encode_text_output(
    text_count: usize,
    body: &SuccessBody,
    blob: &[u8],
    embed_dim: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let te = body
        .text_embed
        .as_ref()
        .ok_or("EncodeText Success 缺 text_embed 应答体")?;
    if te.count as usize != text_count {
        return Err(format!("count 错配:{} != texts {}", te.count, text_count));
    }
    if embed_dim == 0 {
        return Err("embed_dim 为 0".into());
    }
    let item_bytes = embed_dim * 4;
    if blob.len() != text_count * item_bytes {
        return Err(format!(
            "blob 长度错配:{} != {}×{}",
            blob.len(),
            text_count,
            item_bytes
        ));
    }
    Ok(blob
        .chunks_exact(item_bytes)
        .map(|chunk| {
            chunk
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use exotic_protocol::{FaceDet, WorkerErrorCode};
    use std::io::Cursor;

    fn thumb_req(item_id: i64, fp: &str, tier: u32) -> RequestBody {
        RequestBody::Thumbnail {
            item_id,
            source_path: "x.psd".into(),
            target_long_edge: tier,
            input_fingerprint: fp.into(),
        }
    }

    /// 生成一张真实 WebP（用 image crate 编码一张纯色图）。
    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut buf = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut buf))
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    fn limits() -> WorkerLimits {
        default_thumbnail_limits()
    }

    #[test]
    fn validate_accepts_good_output() {
        let req = thumb_req(7, "fp", 480);
        let webp = make_webp(480, 240);
        let body = SuccessBody {
            item_id: Some(7),
            input_fingerprint: Some("fp".into()),
            mime: Some("image/webp".into()),
            width: Some(480),
            height: Some(240),
            ..Default::default()
        };
        let (w, h, mime) = validate_thumbnail_output(&req, &body, &webp, &limits()).unwrap();
        assert_eq!((w, h), (480, 240));
        assert_eq!(mime, "image/webp");
    }

    #[test]
    fn validate_rejects_item_id_mismatch() {
        let req = thumb_req(7, "fp", 480);
        let webp = make_webp(100, 100);
        let body = SuccessBody {
            item_id: Some(999),
            input_fingerprint: Some("fp".into()),
            mime: Some("image/webp".into()),
            width: Some(100),
            height: Some(100),
            ..Default::default()
        };
        assert!(validate_thumbnail_output(&req, &body, &webp, &limits()).is_err());
    }

    #[test]
    fn validate_rejects_declared_dims_mismatch() {
        let req = thumb_req(7, "fp", 480);
        let webp = make_webp(100, 100);
        let body = SuccessBody {
            item_id: Some(7),
            input_fingerprint: Some("fp".into()),
            mime: Some("image/webp".into()),
            width: Some(480), // 谎报
            height: Some(100),
            ..Default::default()
        };
        assert!(validate_thumbnail_output(&req, &body, &webp, &limits()).is_err());
    }

    #[test]
    fn validate_rejects_oversized_long_edge() {
        let req = thumb_req(7, "fp", 120);
        let webp = make_webp(960, 100); // 长边 960 >> 120+容差
        let body = SuccessBody {
            item_id: Some(7),
            input_fingerprint: Some("fp".into()),
            mime: Some("image/webp".into()),
            width: Some(960),
            height: Some(100),
            ..Default::default()
        };
        assert!(validate_thumbnail_output(&req, &body, &webp, &limits()).is_err());
    }

    #[test]
    fn validate_rejects_non_webp_blob() {
        let req = thumb_req(7, "fp", 480);
        let body = SuccessBody {
            item_id: Some(7),
            input_fingerprint: Some("fp".into()),
            mime: Some("image/webp".into()),
            width: None,
            height: None,
            ..Default::default()
        };
        assert!(validate_thumbnail_output(&req, &body, b"not a webp at all!!", &limits()).is_err());
    }

    // ── v2 批量输出校验(T15)────────────────────────────────────────────────────────

    fn embed_items(n: usize) -> Vec<EmbedItem> {
        (0..n)
            .map(|i| EmbedItem {
                item_id: i as i64 + 1,
                cache_key: format!("k{i}"),
                fingerprint: format!("fp{i}"),
            })
            .collect()
    }

    fn le_blob(embs: &[&[f32]]) -> Vec<u8> {
        let mut b = Vec::new();
        for e in embs {
            for f in e.iter() {
                b.extend_from_slice(&f.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn validate_embed_batch_happy_path_with_per_item_err() {
        let items = embed_items(3);
        let body = SuccessBody {
            embed: Some(exotic_protocol::EmbedBatchSuccess {
                results: vec![
                    EmbedResult::Ok {
                        item_id: 1,
                        fingerprint: "fp0".into(),
                    },
                    EmbedResult::Err {
                        item_id: 2,
                        fingerprint: "fp1".into(),
                        code: WorkerErrorCode::IoError,
                    },
                    EmbedResult::Ok {
                        item_id: 3,
                        fingerprint: "fp2".into(),
                    },
                ],
            }),
            ..Default::default()
        };
        // blob 只含两个 Ok 项(dim=2),按 Ok 项序连续。
        let blob = le_blob(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let out = validate_embed_batch_output(&items, &body, &blob, 2).unwrap();
        assert_eq!(out.len(), 3);
        assert!(matches!(&out[0], EmbedItemOutcome::Ok(v) if v == &vec![1.0, 2.0]));
        assert!(matches!(
            &out[1],
            EmbedItemOutcome::Err(WorkerErrorCode::IoError)
        ));
        assert!(matches!(&out[2], EmbedItemOutcome::Ok(v) if v == &vec![3.0, 4.0]));
    }

    #[test]
    fn validate_embed_batch_rejects_length_and_order_violations() {
        let items = embed_items(2);
        // ① results 少一项 → 违例。
        let short = SuccessBody {
            embed: Some(exotic_protocol::EmbedBatchSuccess {
                results: vec![EmbedResult::Ok {
                    item_id: 1,
                    fingerprint: "fp0".into(),
                }],
            }),
            ..Default::default()
        };
        assert!(validate_embed_batch_output(&items, &short, &le_blob(&[&[0.0, 0.0]]), 2).is_err());

        // ② 错序(item_id 对调)→ 违例(陈旧/错位防护)。
        let swapped = SuccessBody {
            embed: Some(exotic_protocol::EmbedBatchSuccess {
                results: vec![
                    EmbedResult::Ok {
                        item_id: 2,
                        fingerprint: "fp1".into(),
                    },
                    EmbedResult::Ok {
                        item_id: 1,
                        fingerprint: "fp0".into(),
                    },
                ],
            }),
            ..Default::default()
        };
        let blob = le_blob(&[&[0.0, 0.0], &[0.0, 0.0]]);
        assert!(validate_embed_batch_output(&items, &swapped, &blob, 2).is_err());

        // ③ blob 长度与 Ok 项数不符 → 违例。
        let good = SuccessBody {
            embed: Some(exotic_protocol::EmbedBatchSuccess {
                results: vec![
                    EmbedResult::Ok {
                        item_id: 1,
                        fingerprint: "fp0".into(),
                    },
                    EmbedResult::Ok {
                        item_id: 2,
                        fingerprint: "fp1".into(),
                    },
                ],
            }),
            ..Default::default()
        };
        assert!(validate_embed_batch_output(&items, &good, &le_blob(&[&[0.0, 0.0]]), 2).is_err());
        // ④ 缺 embed 应答体 → 违例。
        assert!(validate_embed_batch_output(&items, &SuccessBody::default(), &[], 2).is_err());
    }

    #[test]
    fn validate_encode_text_happy_path_and_violations() {
        // 合法:count=2、blob=2×dim×4,按顺序切出两个向量。
        let good = SuccessBody {
            text_embed: Some(exotic_protocol::TextEmbedSuccess { count: 2 }),
            ..Default::default()
        };
        let blob = le_blob(&[&[1.0, -2.0], &[0.5, 0.25]]);
        let out = validate_encode_text_output(2, &good, &blob, 2).unwrap();
        assert_eq!(out, vec![vec![1.0, -2.0], vec![0.5, 0.25]]);

        // ① count 与请求 texts 数不符 → 违例。
        assert!(validate_encode_text_output(1, &good, &blob, 2).is_err());
        // ② blob 长度错配 → 违例。
        assert!(validate_encode_text_output(2, &good, &le_blob(&[&[1.0, -2.0]]), 2).is_err());
        // ③ 缺 text_embed 应答体(op 错配)→ 违例。
        assert!(validate_encode_text_output(2, &SuccessBody::default(), &blob, 2).is_err());
        // ④ embed_dim=0 → 违例(除零/空契约防御)。
        assert!(validate_encode_text_output(2, &good, &blob, 0).is_err());
    }

    #[test]
    fn validate_face_batch_happy_path_zero_and_multi_faces() {
        let items = vec![
            FaceItem {
                item_id: 10,
                cache_key: Some("aaa".into()),
                source_path: None,
                fingerprint: "f10".into(),
            },
            FaceItem {
                item_id: 11,
                cache_key: None,
                source_path: Some("x.jpg".into()),
                fingerprint: "f11".into(),
            },
        ];
        let det = FaceDet {
            bbox: [1.0, 2.0, 3.0, 4.0],
            landmarks: [[0.0; 2]; 5],
            score: 0.95,
        };
        let body = SuccessBody {
            face: Some(exotic_protocol::FaceBatchSuccess {
                results: vec![
                    FaceItemResult::Ok {
                        item_id: 10,
                        fingerprint: "f10".into(),
                        faces: vec![det.clone(), det.clone()],
                        width: 640,
                        height: 480,
                    },
                    // 0 张脸也是 Ok(协议明文)。
                    FaceItemResult::Ok {
                        item_id: 11,
                        fingerprint: "f11".into(),
                        faces: vec![],
                        width: 320,
                        height: 240,
                    },
                ],
            }),
            ..Default::default()
        };
        let blob = le_blob(&[&[0.5, 0.6], &[0.7, 0.8]]); // 2 脸 × dim 2
        let out = validate_face_batch_output(&items, &body, &blob, 2).unwrap();
        assert_eq!(out.len(), 2);
        match &out[0] {
            FaceItemOutcome::Ok {
                faces,
                embeddings,
                width,
                height,
            } => {
                assert_eq!(faces.len(), 2);
                assert_eq!(embeddings, &vec![vec![0.5, 0.6], vec![0.7, 0.8]]);
                assert_eq!((*width, *height), (640, 480));
            }
            _ => panic!("期望 Ok"),
        }
        match &out[1] {
            FaceItemOutcome::Ok {
                faces, embeddings, ..
            } => {
                assert!(faces.is_empty() && embeddings.is_empty());
            }
            _ => panic!("期望 0 脸 Ok"),
        }
    }

    #[test]
    fn validate_face_batch_rejects_zero_dims() {
        // 旧帧缺 width/height 经 serde default 落 0——host 必须拒收(归一化会除坏),
        // 该测试锁死「additive 字段的缺省值不可被静默接受」的契约。
        let items = vec![FaceItem {
            item_id: 10,
            cache_key: Some("aaa".into()),
            source_path: None,
            fingerprint: "f10".into(),
        }];
        let body = SuccessBody {
            face: Some(exotic_protocol::FaceBatchSuccess {
                results: vec![FaceItemResult::Ok {
                    item_id: 10,
                    fingerprint: "f10".into(),
                    faces: vec![],
                    width: 0,
                    height: 0,
                }],
            }),
            ..Default::default()
        };
        assert!(validate_face_batch_output(&items, &body, &[], 2).is_err());
    }

    #[test]
    fn validate_face_batch_rejects_blob_mismatch() {
        let items = vec![FaceItem {
            item_id: 10,
            cache_key: Some("aaa".into()),
            source_path: None,
            fingerprint: "f10".into(),
        }];
        let body = SuccessBody {
            face: Some(exotic_protocol::FaceBatchSuccess {
                results: vec![FaceItemResult::Ok {
                    item_id: 10,
                    fingerprint: "f10".into(),
                    faces: vec![FaceDet {
                        bbox: [0.0; 4],
                        landmarks: [[0.0; 2]; 5],
                        score: 1.0,
                    }],
                    width: 640,
                    height: 480,
                }],
            }),
            ..Default::default()
        };
        // 1 脸 × dim 2 应为 8 字节,给 4 字节 → 违例。
        assert!(validate_face_batch_output(&items, &body, &le_blob(&[&[0.5]]), 2).is_err());
        // fingerprint 错配 → 违例。
        let bad_fp = SuccessBody {
            face: Some(exotic_protocol::FaceBatchSuccess {
                results: vec![FaceItemResult::Err {
                    item_id: 10,
                    fingerprint: "WRONG".into(),
                    code: WorkerErrorCode::IoError,
                }],
            }),
            ..Default::default()
        };
        assert!(validate_face_batch_output(&items, &bad_fp, &[], 2).is_err());
    }

    // ── OCR 批输出校验(T6)──────────────────────────────────────────────────────────

    fn ocr_items(n: usize) -> Vec<OcrItem> {
        (0..n)
            .map(|i| OcrItem {
                item_id: i as i64 + 1,
                cache_key: None,
                source_path: Some(format!("p{i}.png")),
                fingerprint: format!("fp{i}"),
            })
            .collect()
    }

    fn ocr_line() -> OcrLine {
        OcrLine {
            text: "hi".into(),
            quad: [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            confidence: 0.9,
        }
    }

    fn ocr_ok(item_id: i64, fp: &str, lines: Vec<OcrLine>, w: u32, h: u32) -> OcrItemResult {
        OcrItemResult::Ok {
            item_id,
            fingerprint: fp.into(),
            lines,
            width: w,
            height: h,
        }
    }

    fn ocr_body(results: Vec<OcrItemResult>) -> SuccessBody {
        SuccessBody {
            ocr: Some(exotic_protocol::OcrBatchSuccess { results }),
            ..Default::default()
        }
    }

    #[test]
    fn validate_ocr_batch_happy_and_per_item_err() {
        let items = ocr_items(2);
        let body = ocr_body(vec![
            ocr_ok(1, "fp0", vec![ocr_line()], 640, 480),
            OcrItemResult::Err {
                item_id: 2,
                fingerprint: "fp1".into(),
                code: WorkerErrorCode::IoError,
            },
        ]);
        let out = validate_ocr_batch_output(&items, &body, &[]).unwrap();
        assert_eq!(out.len(), 2);
        assert!(matches!(
            &out[0],
            OcrItemOutcome::Ok { lines, width, height }
                if lines.len() == 1 && *width == 640 && *height == 480
        ));
        assert!(matches!(
            &out[1],
            OcrItemOutcome::Err {
                code: WorkerErrorCode::IoError
            }
        ));
    }

    #[test]
    fn validate_ocr_batch_rejects_violations() {
        let items = ocr_items(1);
        // ① 缺 ocr 应答体 → 违例。
        assert!(validate_ocr_batch_output(&items, &SuccessBody::default(), &[]).is_err());
        // ② results 长度不符(0 项)→ 违例。
        assert!(validate_ocr_batch_output(&items, &ocr_body(vec![]), &[]).is_err());
        // ③ item_id 错位 → 违例。
        let bad_id = ocr_body(vec![ocr_ok(999, "fp0", vec![], 10, 10)]);
        assert!(validate_ocr_batch_output(&items, &bad_id, &[]).is_err());
        // ④ blob 非空 → 违例(OCR 走 JSON,blob 恒空)。
        let ok = ocr_body(vec![ocr_ok(1, "fp0", vec![ocr_line()], 10, 10)]);
        assert!(validate_ocr_batch_output(&items, &ok, b"x").is_err());
        // ⑤ 解码尺寸为 0 → 违例。
        let zero = ocr_body(vec![ocr_ok(1, "fp0", vec![], 0, 0)]);
        assert!(validate_ocr_batch_output(&items, &zero, &[]).is_err());
        // ⑥ NaN quad → 违例。
        let mut nan_line = ocr_line();
        nan_line.quad[0][0] = f32::NAN;
        let nan_body = ocr_body(vec![ocr_ok(1, "fp0", vec![nan_line], 10, 10)]);
        assert!(validate_ocr_batch_output(&items, &nan_body, &[]).is_err());
        // ⑦ conf 越界 → 违例。
        let mut bad_conf = ocr_line();
        bad_conf.confidence = 1.5;
        let conf_body = ocr_body(vec![ocr_ok(1, "fp0", vec![bad_conf], 10, 10)]);
        assert!(validate_ocr_batch_output(&items, &conf_body, &[]).is_err());
        // ⑧ 合法(单项 Ok)→ 通过。
        assert!(validate_ocr_batch_output(&items, &ok, &[]).is_ok());
    }
}
