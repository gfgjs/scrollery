//! 入口每页批量读取产物身份；文件校验在 DB 读连接释放后完成。

use super::generator::ThumbConfig;
use super::service::{ImageRequest, Priority};
use crate::db::models::ThumbResult;
use crate::db::queries::{self, ImageArtifact, ImageSource};
use crate::error::Result;
use crate::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) fn prepare_sources(
    state: &Arc<AppState>,
    sources: Vec<ImageSource>,
    config: &ThumbConfig,
    epoch: u64,
    priority: Priority,
    cancel: &CancellationToken,
    measured: bool,
) -> Result<Vec<ImageRequest>> {
    let ids: Vec<_> = sources.iter().map(|source| source.item.id).collect();
    let (display, ai) = {
        let conn = state.db_read_pool.get()?;
        let display =
            queries::find_image_artifacts(&conn, &ids, "display", &config.display_fingerprint())?;
        let ai = if config.ai_hq_cache {
            queries::find_image_artifacts(&conn, &ids, "ai", &config.ai_fingerprint())?
        } else {
            Vec::new()
        };
        (display, ai)
    };
    let mut display: HashMap<_, _> = display.into_iter().map(|a| (a.item_id, a)).collect();
    let mut ai: HashMap<_, _> = ai.into_iter().map(|a| (a.item_id, a)).collect();
    let mut missing = Vec::new();
    let mut requests = Vec::with_capacity(sources.len());
    for source in sources {
        let item = source.item;
        let matches = |a: &ImageArtifact| {
            a.source_revision == item.source_revision && a.cache_key == item.cache_key
        };
        let cached = display.remove(&item.id).filter(matches).filter(|a| {
            if a.status != 1 {
                return true;
            }
            let Some(path) = &a.path else { return false };
            if config.cache_dir.join("thumbnails").join(path).is_file() {
                true
            } else {
                missing.push(path.clone());
                false
            }
        });
        let cached_ai_path = ai
            .remove(&item.id)
            .filter(matches)
            .and_then(|a| a.path)
            .filter(|path| {
                if config.cache_dir.join(path).is_file() {
                    true
                } else {
                    missing.push(path.clone());
                    false
                }
            });
        let cached_result = cached.map(|a| ThumbResult {
            item_id: a.item_id,
            source_revision: a.source_revision,
            cache_key: a.cache_key,
            thumb_status: a.status,
            thumb_path: a.path,
            thumbhash: a.thumbhash,
        });
        let abs_path = crate::utils::path::resolve_media_path(
            &source.root_path,
            &source.relative_dir,
            &item.file_name,
        )
        .into();
        requests.push(ImageRequest {
            item,
            abs_path,
            root_path: source.root_path,
            relative_dir: source.relative_dir,
            cached_result,
            cached_ai_path,
            config: config.clone(),
            epoch,
            priority,
            cancellation: cancel.clone(),
            measured,
        });
    }
    if !missing.is_empty() {
        state
            .with_database_lifecycle_read(epoch, || {
                let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                queries::invalidate_image_artifacts_by_paths(&mut conn, &missing)
            })
            .transpose()?;
    }
    Ok(requests)
}
