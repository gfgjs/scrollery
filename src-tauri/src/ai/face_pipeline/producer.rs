// src-tauri/src/ai/face_pipeline/producer.rs
//! Producer 阶段:批量查询待处理项,标记 Processing,推送任务到通道。

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::db::models::FaceStatus;
use crate::db::queries::{batch_update_face_status, get_pending_face_items};
use crate::state::AppState;

use super::{FaceTask, BATCH_SIZE};

/// 生产者：批量查询待处理项，推送任务到通道。
pub(super) fn produce_face_tasks(
    state: &Arc<AppState>,
    task_tx: Sender<FaceTask>,
    token: &CancellationToken,
) {
    // 让步阻塞源变化追踪(可观测性三修 #1,与 AI 侧 produce_tasks 同款):集合与上次不同
    // (含首次非空/恢复为空)才 info,未变化保持 debug——避免刷屏,同时不再让阻塞源常驻
    // (如派生挂死)导致的永久让步在日志里悄无声息。
    let mut last_blockers: Option<Vec<&'static str>> = None;
    loop {
        if token.is_cancelled() {
            info!("Face producer cancelled | 人脸生产者已取消");
            break;
        }

        // 复用既有让步层（scan/缩略图/派生/exotic/交互）——与 CLIP 生产者调用同一函数。
        // v3.1 R9：阻塞源已含 exotic（冷门格式子进程解码优先于 AI/face），单点改动两边自动生效。
        // 人脸↔CLIP 互斥不在此实现，见模块头。
        let blockers = state.ai_yield_blockers();
        if !blockers.is_empty() {
            if last_blockers.as_ref() != Some(&blockers) {
                info!(
                    target: "scrollery::pipeline::face",
                    blockers = %blockers.join(","),
                    "Face producer yielding to higher priority task"
                );
                last_blockers = Some(blockers.clone());
            } else {
                debug!(
                    target: "scrollery::pipeline::face",
                    blockers = %blockers.join(","),
                    "Face producer yielding to higher priority task"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        } else if last_blockers.take().is_some() {
            info!(
                target: "scrollery::pipeline::face",
                "Face producer: yield blockers cleared, resuming | 让步解除，继续生产"
            );
        }

        // 读连接圈进块作用域,取完批立即归还(2026-07-11 加固批 B-1;
        // 与 AI 侧 produce_tasks 同病同修,详见彼处注释)。
        let batch = {
            let conn = match state.db_read_pool.get() {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "DB pool error in face producer | 人脸生产者 DB 池错误: {}",
                        e
                    );
                    break;
                }
            };
            match get_pending_face_items(&conn, BATCH_SIZE) {
                Ok(b) => b,
                Err(e) => {
                    warn!(
                        "Query pending face items failed | 查询待处理人脸项失败: {}",
                        e
                    );
                    break;
                }
            }
        };

        if batch.is_empty() {
            info!("Face producer: no more pending items | 人脸生产者：没有更多待处理项");
            break;
        }

        // 将项标记为"处理中"，避免重启时重新排队。
        let write_conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<i64> = batch.iter().map(|it| it.id).collect();
        if let Err(e) = batch_update_face_status(&write_conn, &ids, FaceStatus::Processing.as_i64())
        {
            warn!(
                "Failed to mark face items as processing | 标记人脸项为处理中失败: {}",
                e
            );
        }
        drop(write_conn);

        for item in batch {
            if token.is_cancelled() {
                break;
            }

            if task_tx
                .send(FaceTask {
                    item_id: item.id,
                    source_path: PathBuf::from(item.abs_path),
                    file_format: item.file_format,
                    thumb_status: item.thumb_status,
                    thumb_path: item.thumb_path,
                    width: item.width,
                    height: item.height,
                    cache_key: item.cache_key,
                })
                .is_err()
            {
                break;
            }
        }
    }

    info!("Face producer finished | 人脸生产者已完成");
}
