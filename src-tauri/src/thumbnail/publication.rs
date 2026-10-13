//! 普通图片的唯一结果发布出口：身份/配置/订阅校验、批量事务、缓存与通知。

use super::benchmark::Publication;
use super::service::ImageSubscription;
use crate::db::models::ThumbResult;
use crate::db::queries::{publish_image_artifacts, ImageArtifact};
use crate::error::Result;
use crate::state::AppState;

/// 调用方保持轮次闸门（后台）或本批订阅（视口）；返回值与订阅一一对应。
/// 文件 IO 已完成；本临界区保留生命周期、配置和执行身份，SQL 不占调度注册表锁。
pub(crate) fn publish_batch(
    state: &AppState,
    epoch: u64,
    subscriptions: &[ImageSubscription],
    mut notify: impl FnMut(&ThumbResult),
) -> Result<Option<Vec<Publication>>> {
    state
        .with_database_lifecycle_read(epoch, || {
            let current = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
            ImageSubscription::with_completed_batch(subscriptions, |completed| {
                let mut records = Vec::new();
                let mut indices = Vec::new();
                let mut outcomes = vec![Publication::Stale; subscriptions.len()];
                for (index, work) in completed.iter().enumerate() {
                    let Some((config, completion)) = work else {
                        continue;
                    };
                    if !config.same_policy(&current) {
                        continue;
                    }
                    let Some(result) = &completion.result else {
                        continue;
                    };
                    indices.push((index, records.len(), result));
                    records.push(ImageArtifact {
                        item_id: result.item_id,
                        source_revision: result.source_revision,
                        cache_key: result.cache_key,
                        kind: "display".into(),
                        fingerprint: config.display_fingerprint(),
                        status: result.thumb_status,
                        path: result.thumb_path.clone(),
                        thumbhash: result.thumbhash.clone(),
                    });
                    if let Some(path) = &completion.ai_path {
                        records.push(ImageArtifact {
                            item_id: result.item_id,
                            source_revision: result.source_revision,
                            cache_key: result.cache_key,
                            kind: "ai".into(),
                            fingerprint: config.ai_fingerprint(),
                            status: 1,
                            path: Some(path.clone()),
                            thumbhash: None,
                        });
                    }
                }
                let published = {
                    let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    publish_image_artifacts(&mut conn, &records, true)?
                };
                let mut accepted = Vec::new();
                for (index, record, result) in indices {
                    if published[record].accepted {
                        outcomes[index] = if published[record].inserted {
                            Publication::Accepted
                        } else {
                            Publication::Reused
                        };
                        accepted.push(result.clone());
                    }
                }
                // SQL 已全部提交且源/配置/订阅仍在保护范围内，才更新内存并发送成功。
                state.apply_thumb_results(&accepted);
                for result in &accepted {
                    notify(result);
                }
                Ok(outcomes)
            })
        })
        .transpose()
}
