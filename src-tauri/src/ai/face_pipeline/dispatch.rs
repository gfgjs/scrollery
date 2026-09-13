// src-tauri/src/ai/face_pipeline/dispatch.rs
//! worker 派发路径(face 接线波;T16 起为唯一路径,进程内 ort 已删):攒批 →
//! 三级定源 → 缺缓存现场预解码 → GPU 令牌 → FaceDetectEmbed → 结果映射。

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ai::face::DetectedFace;
use crate::ai::face_profile::FaceProfile;
use crate::state::AppState;
use crate::thumbnail::cache::face_cache_path;

use super::decode_source::{
    face_cache_applies, faces_to_records, resolve_face_decode_source, FaceDecodeSource,
    FaceSourceKind, WORKER_DECODABLE_FORMATS,
};
use super::{FaceResult, FaceTask};

/// 攒批的空闲刷新周期(与 CLIP worker 派发同值)。
const WORKER_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(50);

/// face 单批派发上限(2026-07-03 GUI 实测拍板):worker 人脸推理逐图进行(YuNet 固定
/// 640 输入),协议批只摊薄毫秒级的 IPC 往返,吞吐与批大小无关;批越大,单请求超时
/// 敞口与取消/落库粒度越差。CLIP 的 VRAM 自适应 batch(可达 64)对 face 无意义,
/// 派发时收窄到本值(方案 A 后源恒为 640 级小图,16 已兼顾 IPC 摊薄与取消/落库粒度)。
const FACE_DISPATCH_BATCH: usize = 16;

/// face 有效派发批 = min(会话声明 batch, FACE_DISPATCH_BATCH),至少 1。
pub(super) fn face_dispatch_cap(session_batch: u32) -> usize {
    (session_batch as usize).clamp(1, FACE_DISPATCH_BATCH)
}

/// 派发批次累计诊断(2026-07-03 性能取证):随批输出墙钟与解码源构成,回答「慢在哪」。
/// 原图源占比高且单项耗时大 ⇒ worker 端全尺寸解码主导;worker 侧分段耗时(解码/检测/
/// 嵌入)见其 stderr 的「FaceDetectEmbed 批诊断」行,两侧日志对照即可定位瓶颈段。
#[derive(Default)]
struct FaceDispatchStats {
    /// 已派发并返回的项数(不含格式跳过项)。
    items: u64,
    /// face_detect_embed 往返墙钟累计(ms,含 IPC 与重试)。
    wall_ms: u128,
    /// 解码源构成:分档缩略图 / face 缓存(方案 A) / 原图直派。
    thumb: u64,
    cache: u64,
    orig: u64,
    /// 瞬态失败/防呆回退不可派项的跳过数(保持 Processing,下次运行恢复)。
    skipped: u64,
}

/// 单项派发计划(方案 A 三级定源的产物,与批内 tasks 同序)。
enum FacePlan {
    /// 源已可派(缩略图档位,或小原图直派)。
    Ready(FaceDecodeSource),
    /// 走 face 缓存;`predecode`=缓存缺失,须本批现场预解码。
    UseCache { predecode: bool },
    /// 不可派(detect_size 超缓存尺寸的防呆 + worker 不可解格式),保持 Processing。
    Skip,
}

/// face 派发线程主循环:攒批 → 派发;通道关闭(生产者收尾)时刷余批后退出。
pub(super) fn face_dispatch_loop(
    state: &Arc<AppState>,
    spec: &crate::ai::worker_client::SessionSpec,
    profile: &FaceProfile,
    task_rx: Receiver<FaceTask>,
    result_tx: Sender<FaceResult>,
    token: &CancellationToken,
) -> std::result::Result<(), String> {
    let batch_cap = face_dispatch_cap(spec.batch_size);
    let mut buf: Vec<FaceTask> = Vec::with_capacity(batch_cap);
    // 性能取证(2026-07-03):批墙钟/解码源构成/跳过数累计,循环尾输出总结。
    let mut stats = FaceDispatchStats::default();

    loop {
        if token.is_cancelled() {
            info!("Face worker 派发已取消 | face dispatcher cancelled");
            break;
        }
        match task_rx.recv_timeout(WORKER_FLUSH_TIMEOUT) {
            Ok(task) => {
                buf.push(task);
                if buf.len() >= batch_cap {
                    dispatch_face_batch(
                        state, spec, profile, &mut buf, &result_tx, token, &mut stats,
                    )?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if !buf.is_empty() {
                    dispatch_face_batch(
                        state, spec, profile, &mut buf, &result_tx, token, &mut stats,
                    )?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if !buf.is_empty() {
                    dispatch_face_batch(
                        state, spec, profile, &mut buf, &result_tx, token, &mut stats,
                    )?;
                }
                break;
            }
        }
    }

    if stats.skipped > 0 {
        // 不静默截断(工作约定):跳过项保持 Processing,下次运行回 Pending。
        warn!(
            "Face worker 派发跳过 {} 项(瞬态失败,或 detect_size 超 face 缓存尺寸的防呆 \
             回退且源格式 worker 不可解;保持 Processing,下次运行恢复)",
            stats.skipped
        );
    }
    if stats.items > 0 {
        info!(
            "Face 派发总结:{} 项,批往返累计 {}ms,均 {}ms/项;解码源 缩略图 {} / face缓存 {} / 原图 {}",
            stats.items,
            stats.wall_ms,
            stats.wall_ms / stats.items as u128,
            stats.thumb,
            stats.cache,
            stats.orig
        );
    }
    info!("Face worker 派发完成 | face dispatcher finished");
    Ok(())
}

/// 派发一批:CPU permit → 三级定源(缩略图 → face 缓存 → 小原图)→ 缺缓存现场预解码
/// (方案 A,rayon 并行,镜像 CLIP 的 T18/T18.5)→ GPU 令牌 → FaceDetectEmbed →
/// 逐项映射落结果。返回 Err = 批级致命(终止本轮);取消返回 Ok 且清空 buf(在途项
/// 保持 Processing)。
fn dispatch_face_batch(
    state: &Arc<AppState>,
    spec: &crate::ai::worker_client::SessionSpec,
    profile: &FaceProfile,
    buf: &mut Vec<FaceTask>,
    result_tx: &Sender<FaceResult>,
    token: &CancellationToken,
    stats: &mut FaceDispatchStats,
) -> std::result::Result<(), String> {
    let tasks: Vec<FaceTask> = std::mem::take(buf);
    let thresh = profile.det_score_thresh;

    // D2 顺序天条:先 CPU permit(公平后台池)后 GPU 令牌;None = 已取消,直接收手
    // (本批项保持 Processing,下次运行恢复)。两者随作用域 Drop 释放。
    // permit 提前到定源/预解码之前(镜像 CLIP dispatch_batch):现场预解码是重 CPU 解码,
    // 必须在后台池配额内。
    let Some(_cpu_permit) = state.background_heavy_limiter.acquire(token) else {
        return Ok(());
    };

    let cache_dir = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .cache_dir
        .clone();

    // 阶段1:三级定源(纯内存决策 + face 缓存存在性 stat),与 tasks 同序。
    let plans: Vec<FacePlan> = tasks
        .iter()
        .map(|t| {
            // 与进程内同一决策:thumbnails 档位预测短边 ≥ detect_size 才用缩略图。
            let src = resolve_face_decode_source(t, state, profile.detect_size);
            if src.kind == FaceSourceKind::Thumb {
                return FacePlan::Ready(src);
            }
            let decodable =
                WORKER_DECODABLE_FORMATS.contains(&src.format.to_ascii_lowercase().as_str());
            if face_cache_applies(t.width, t.height, decodable, profile.detect_size) {
                FacePlan::UseCache {
                    predecode: !face_cache_path(&cache_dir, t.cache_key).exists(),
                }
            } else if decodable {
                // 小原图直派(短边 ≤ 缓存尺寸,预解码无收益),或未来 detect_size 超
                // 缓存尺寸的防呆回退(全尺寸解码,慢但正确)。
                FacePlan::Ready(src)
            } else {
                // 仅防呆分支会走到:detect_size 超缓存尺寸 + worker 不可解格式。
                stats.skipped += 1;
                FacePlan::Skip
            }
        })
        .collect();

    // 阶段2:缺缓存现场预解码(方案 A,镜像 CLIP 的 T18.5):rayon 全局池并行,WIC 优先/
    // image crate 回退,tmp→rename 原子落盘;CPU permit 保持「1 批=1 槽」记账。取消检查
    // 在每项开工前:已落盘项幂等可复用,未开工项随本批放弃(保持 Processing)。
    let predecode_failed: std::collections::HashSet<i64> = tasks
        .par_iter()
        .zip(plans.par_iter())
        .filter(|(_, p)| matches!(p, FacePlan::UseCache { predecode: true }))
        .filter_map(|(t, _)| {
            if token.is_cancelled() {
                return None;
            }
            // panic 拦截伞(与 derive/pipeline.rs kind::run 同款防线):worker 端 rayon
            // par_iter 内裸跑第三方解码,单个畸形文件 panic 会中止整批,需转为 Err
            // 落入既有 predecode_failed 失败路径(标 Error,不连坐整批)。
            crate::thumbnail::generator::panic_guard(
                &format!("face_worker:generate_face_cache item {}", t.item_id),
                || {
                    crate::derive::image::generate_face_cache(
                        &cache_dir,
                        t.cache_key,
                        &t.file_format,
                        &t.source_path,
                    )
                },
            )
            .err()
            .map(|e| {
                // 双引擎(WIC+image crate)都解不开 → 标 Error(镜像 CLIP T18 派生失败
                // 语义;worker 端同为 image crate,回退直派几无胜算),不连坐整批。
                warn!("item {} face 缓存现场预解码失败:{e}(标 Error)", t.item_id);
                t.item_id
            })
        })
        .collect();
    if token.is_cancelled() {
        return Ok(()); // 预解码中途取消:本批项保持 Processing,下次运行恢复。
    }

    // 阶段3:装配协议项。
    let mut items: Vec<exotic_protocol::FaceItem> = Vec::with_capacity(tasks.len());
    // (item_id, cache_key 快照):cache_key 随结果传给 Writer 做 X1 条件写。
    let mut item_keys: Vec<(i64, i64)> = Vec::with_capacity(tasks.len());
    // 本批解码源构成(缩略图 / face 缓存 / 原图):慢批定位的第一信号,与 worker 侧分段耗时对照。
    let (mut n_thumb, mut n_cache, mut n_orig) = (0u64, 0u64, 0u64);
    for (t, plan) in tasks.iter().zip(plans) {
        let src = match plan {
            FacePlan::Skip => continue,
            FacePlan::UseCache { .. } => {
                if predecode_failed.contains(&t.item_id) {
                    let _ = result_tx.send(FaceResult {
                        item_id: t.item_id,
                        cache_key: t.cache_key,
                        records: None,
                    });
                    continue;
                }
                FaceDecodeSource {
                    path: face_cache_path(&cache_dir, t.cache_key),
                    format: "webp".to_string(),
                    kind: FaceSourceKind::FaceCache,
                }
            }
            FacePlan::Ready(src) => src,
        };
        match src.kind {
            FaceSourceKind::Thumb => n_thumb += 1,
            FaceSourceKind::FaceCache => n_cache += 1,
            FaceSourceKind::Original => n_orig += 1,
        }
        items.push(exotic_protocol::FaceItem {
            item_id: t.item_id,
            cache_key: None,
            // 信任语义同 Thumbnail.source_path:host 给绝对路径(缩略图档位/face 缓存/原图)。
            source_path: Some(src.path.to_string_lossy().into_owned()),
            // 回声核对指纹:检测阈值是行为参数(同图不同阈值不同结果),纳入其中。
            fingerprint: format!("{}:{:.4}", t.item_id, thresh),
        });
        item_keys.push((t.item_id, t.cache_key));
    }
    if items.is_empty() {
        return Ok(());
    }

    let Some(_gpu_permit) = state.gpu_token.acquire(token) else {
        return Ok(());
    };

    let t0 = std::time::Instant::now();
    let outcomes = {
        let mut client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
        client.face_detect_embed(spec, &items, thresh, &|| token.is_cancelled())
    };
    let outcomes = match outcomes {
        Ok(o) => o,
        Err(e) => {
            // client 已做硬止损(重建重发一次);到这里即系统性失败,终止本轮。
            return Err(e.to_string());
        }
    };
    // 性能取证(2026-07-03):批墙钟含 IPC 往返与 worker 全链(解码/检测/嵌入);
    // 原图源=worker 端全尺寸解码,分段耗时见 worker stderr 的「FaceDetectEmbed 批诊断」。
    let wall_ms = t0.elapsed().as_millis();
    stats.items += items.len() as u64;
    stats.wall_ms += wall_ms;
    stats.thumb += n_thumb;
    stats.cache += n_cache;
    stats.orig += n_orig;
    info!(
        "Face 批:{} 项(缩略图源 {} / face缓存 {} / 原图源 {}) {}ms,均 {}ms/项;累计 {} 项,{:.2} 项/s",
        items.len(),
        n_thumb,
        n_cache,
        n_orig,
        wall_ms,
        wall_ms / items.len() as u128,
        stats.items,
        stats.items as f64 / (stats.wall_ms.max(1) as f64 / 1000.0)
    );

    for ((item_id, cache_key), outcome) in item_keys.into_iter().zip(outcomes) {
        match outcome {
            crate::exotic::worker::FaceItemOutcome::Ok {
                faces,
                embeddings,
                width,
                height,
            } => {
                // 协议 FaceDet 与 DetectedFace 字段同构,搬回后与进程内共用同一映射
                // (归一化按协议回报的实际解码尺寸,零脸也是 Ok → Done)。
                let det: Vec<DetectedFace> = faces
                    .iter()
                    .map(|f| DetectedFace {
                        bbox: f.bbox,
                        landmarks: f.landmarks,
                        score: f.score,
                    })
                    .collect();
                let records = faces_to_records(item_id, &det, &embeddings, width, height);
                let _ = result_tx.send(FaceResult {
                    item_id,
                    cache_key,
                    records: Some(records),
                });
            }
            crate::exotic::worker::FaceItemOutcome::Err(code) if code.default_retryable() => {
                // 瞬态(IoError 等):跳过,保持 Processing。
                stats.skipped += 1;
            }
            crate::exotic::worker::FaceItemOutcome::Err(code) => {
                // terminal(MalformedInput 等):标 Error,不再无限重查。
                warn!(
                    "item {item_id} 人脸检测/嵌入失败[{}](terminal)",
                    code.as_str()
                );
                let _ = result_tx.send(FaceResult {
                    item_id,
                    cache_key,
                    records: None,
                });
            }
        }
    }
    Ok(())
}
