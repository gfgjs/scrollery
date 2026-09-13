// src-tauri/src/ai/face_pipeline/writer.rs
//! Writer 阶段:批量收集结果,先删后插 `faces` 行,更新 `face_status`,驱动增量聚类。

use std::sync::Arc;

use crossbeam_channel::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ai::face_profile::FaceProfile;
use crate::db::models::FaceStatus;
use crate::db::queries::{batch_finish_face_items, batch_update_face_status_guarded, NewFace};
use crate::state::AppState;

use super::{FaceResult, BATCH_SIZE};

/// 写入器「脸行 + face_status」的高频小批阈值（问题2 进度平滑）。
/// 脸行落库便宜且必须先于聚类（聚类按 item_id 从库读回脸），故脸+状态一起小批落库——
/// 让前端 `processedItems`（数 `face_status≠0`）平滑增长，且崩溃安全（Done 永远蕴含脸已写）；
/// 真正昂贵的聚类仍按 `BATCH_SIZE` 大批跑（见 `flush_face_rows` / `flush_cluster`）。
const STATUS_FLUSH_EVERY: usize = 16;

/// 写入器：批量收集结果，先删后插 `faces` 行，更新 `face_status`。
pub(super) fn write_face_results(
    state: &Arc<AppState>,
    result_rx: Receiver<FaceResult>,
    token: &CancellationToken,
    profile: &FaceProfile,
) {
    // 问题2 双节奏：
    // - 小批（STATUS_FLUSH_EVERY=16）：写脸行 + 置 face_status=Done，并把成功项累入 `cluster_pending`。
    //   `done_ids` 须含本批**所有**成功项（含零脸图），它驱动 `batch_finish_face_items` 的删除阶段。
    // - 大批（BATCH_SIZE=512）：对累积的 `cluster_pending` 跑一次昂贵的增量聚类。
    // 解耦后 `processedItems`（数 face_status≠0）平滑增长，而聚类仍低频跑。
    // (item_id, cache_key 快照):X1 条件写要求落库时逐项核对 cache_key 未变。
    let mut done_ids: Vec<(i64, i64)> = Vec::with_capacity(STATUS_FLUSH_EVERY);
    let mut rows: Vec<NewFace> = Vec::new();
    let mut failed_ids: Vec<(i64, i64)> = Vec::new();
    // 脸已落库、status 已 Done、但尚未聚类的 item_id。聚类按 item_id 从库读回脸，故此处累积安全。
    let mut cluster_pending: Vec<i64> = Vec::with_capacity(BATCH_SIZE as usize);
    let mut total_written: u64 = 0;

    // 时间驱动 flush(2026-07-11 加固批 B-2,与 AI 写入器同款):小批 16 已较细,时间上界
    // 只兜「尾巴 <16 项在慢速期长滞留」;**聚类刻意不进时间节拍**——cluster_new_faces 是
    // 昂贵的增量全扫,3s 一触会把它退化成逐项聚类,维持满 512/收尾两个触发点不变。
    let mut last_flush = std::time::Instant::now();
    loop {
        match result_rx.recv_timeout(crate::ai::pipeline::WRITE_FLUSH_INTERVAL) {
            Ok(result) => {
                if token.is_cancelled() {
                    break;
                }

                match result.records {
                    Some(mut recs) => {
                        done_ids.push((result.item_id, result.cache_key));
                        rows.append(&mut recs);
                    }
                    None => {
                        failed_ids.push((result.item_id, result.cache_key));
                    }
                }

                if done_ids.len() >= STATUS_FLUSH_EVERY {
                    flush_face_rows(
                        state,
                        &mut done_ids,
                        &mut rows,
                        profile,
                        &mut cluster_pending,
                        &mut total_written,
                    );
                    last_flush = std::time::Instant::now();
                }
                if failed_ids.len() >= STATUS_FLUSH_EVERY {
                    flush_face_failed(state, &mut failed_ids);
                    last_flush = std::time::Instant::now();
                }
                if cluster_pending.len() >= BATCH_SIZE as usize {
                    flush_cluster(state, &mut cluster_pending, profile);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if token.is_cancelled() {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        if last_flush.elapsed() >= crate::ai::pipeline::WRITE_FLUSH_INTERVAL
            && (!done_ids.is_empty() || !failed_ids.is_empty())
        {
            if !done_ids.is_empty() {
                flush_face_rows(
                    state,
                    &mut done_ids,
                    &mut rows,
                    profile,
                    &mut cluster_pending,
                    &mut total_written,
                );
            }
            if !failed_ids.is_empty() {
                flush_face_failed(state, &mut failed_ids);
            }
            last_flush = std::time::Instant::now();
        }
    }

    if !done_ids.is_empty() {
        flush_face_rows(
            state,
            &mut done_ids,
            &mut rows,
            profile,
            &mut cluster_pending,
            &mut total_written,
        );
    }
    if !failed_ids.is_empty() {
        flush_face_failed(state, &mut failed_ids);
    }
    // 收尾：把剩余未达大批阈值的项聚类掉。
    if !cluster_pending.is_empty() {
        flush_cluster(state, &mut cluster_pending, profile);
    }

    info!(
        "Face writer finished: {} images written | 人脸写入器已完成：写入 {} 张图像",
        total_written, total_written
    );
}

/// Flush a small batch of successfully-processed items: delete+insert their `faces` rows, mark
/// `face_status=Done`, then queue them for the (deferred, large-batch) clustering pass. On DB
/// failure, mark the batch `Error` instead (mirrors CLIP's `flush_batch`).
///
/// 刷新一小批处理成功的项：先删后插其 `faces` 行，标记 `face_status=Done`，再排入（延后的、
/// 大批的）聚类队列。DB 失败则改标 `Error`（镜像 CLIP 的 `flush_batch`）。脸与状态在此一起原子
/// 落库 → `Done` 永远蕴含脸已写，崩溃/取消时不会出现「Done 但零脸」的漏脸。
fn flush_face_rows(
    state: &Arc<AppState>,
    done_ids: &mut Vec<(i64, i64)>,
    rows: &mut Vec<NewFace>,
    profile: &FaceProfile,
    cluster_pending: &mut Vec<i64>,
    total_written: &mut u64,
) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    // X1 条件写:脸行+Done 在 batch_finish_face_items 内单事务完成,cache_key 已换的失效项
    // 整体跳过(其旧 faces 已被失效钩子删除,新结果按旧内容算出,一并丢弃)。
    match batch_finish_face_items(&conn, done_ids, &profile.id, rows) {
        Ok(fresh_ids) => {
            if fresh_ids.len() < done_ids.len() {
                info!(
                    "Face flush:{} 项中 {} 项因源变更失效被跳过(下轮按新内容重分析)",
                    done_ids.len(),
                    done_ids.len() - fresh_ids.len()
                );
            }
            *total_written += fresh_ids.len() as u64;
            drop(conn);
            // 脸已落库 → 排入聚类队列（聚类延后到大批，见 `flush_cluster`）。只聚新鲜项。
            cluster_pending.extend(fresh_ids);
        }
        Err(e) => {
            warn!("Batch face write failed | 批量写入人脸失败: {}", e);
            let _ = batch_update_face_status_guarded(&conn, done_ids, FaceStatus::Error.as_i64());
            drop(conn);
        }
    }
    done_ids.clear();
    rows.clear();
}

/// 对一大批**已落库**的项跑（昂贵的）增量聚类，然后清空队列。与 `flush_face_rows` 分离，使
/// 状态/进度可细粒度推进、聚类保持粗粒度。`cluster_new_faces` 自做短读+短写，不持外层写锁。
fn flush_cluster(state: &Arc<AppState>, cluster_pending: &mut Vec<i64>, profile: &FaceProfile) {
    // 阈值取「运行期 override 或 profile 默认」——可不重编译调参做实测比较（无 override 时同改动前）。
    let (threshold, min_quality) = crate::ai::face_cluster::effective_thresholds(state, profile);
    crate::ai::face_cluster::cluster_new_faces(
        state,
        cluster_pending,
        &profile.id,
        threshold,
        min_quality,
    );
    cluster_pending.clear();
}

/// 把一批失败项标记为 `face_status=Error`（镜像 CLIP 的 `flush_failed`;X1 条件写,失效项不标）。
fn flush_face_failed(state: &Arc<AppState>, failed_ids: &mut Vec<(i64, i64)>) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = batch_update_face_status_guarded(&conn, failed_ids, FaceStatus::Error.as_i64())
    {
        warn!(
            "Failed to mark face items as error | 标记人脸项为错误失败: {}",
            e
        );
    }
    failed_ids.clear();
}
