//! 视口普通图片收集器；沿用共享服务与批量发布，取消通知只用于及时归还订阅。

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Select;

use crate::db::models::ThumbResult;
use crate::error::Result;
use crate::state::AppState;

use super::generator::ThumbConfig;
use super::service::{ImageService, ImageSubscription, Priority};
use super::viewport::{pending_result, ViewportRequest};

pub(crate) fn run_images(
    state: &Arc<AppState>,
    epoch: u64,
    item_ids: &[i64],
    config: &ThumbConfig,
    request: &ViewportRequest,
    mut send: impl FnMut(ThumbResult) -> bool,
) -> Result<()> {
    const REQUEST_WINDOW: usize = 256;
    const FLUSH_LIMIT: usize = 50;
    const FLUSH_TICK: Duration = Duration::from_millis(100);
    let service = ImageService::for_state(state);
    let mut pages = item_ids.chunks(REQUEST_WINDOW);
    let mut requests = VecDeque::new();
    let mut live: Vec<ImageSubscription> = Vec::new();
    let mut completed: Vec<ImageSubscription> = Vec::new();
    let mut exhausted = false;
    let mut disconnected = false;
    let mut last_publish = Instant::now();

    'request: while !disconnected
        && !request.cancellation.is_cancelled()
        && state.is_database_epoch_current(epoch)
    {
        live.retain(|sub| request.token(sub.item_id()).is_some());
        completed.retain(|sub| request.token(sub.item_id()).is_some());
        while live.len() < REQUEST_WINDOW && !exhausted {
            if request.cancellation.is_cancelled() || !state.is_database_epoch_current(epoch) {
                break 'request;
            }
            if requests.is_empty() {
                let Some(ids) = pages.next() else {
                    exhausted = true;
                    break;
                };
                let sources = {
                    let conn = state.db_read_pool.get()?;
                    crate::db::queries::get_image_sources_batch(&conn, ids)?
                };
                let accepted: HashSet<_> = sources.iter().map(|source| source.item.id).collect();
                for id in ids.iter().filter(|id| !accepted.contains(id)) {
                    if state.with_database_lifecycle_read(epoch, || send(pending_result(*id)))
                        != Some(true)
                    {
                        disconnected = true;
                        break 'request;
                    }
                }
                let sources = sources
                    .into_iter()
                    .filter(|source| request.token(source.item.id).is_some())
                    .collect();
                requests = super::intake::prepare_sources(
                    state,
                    sources,
                    config,
                    epoch,
                    Priority::Viewport,
                    &request.cancellation,
                    false,
                )?
                .into();
            }
            if let Some(mut image_request) = requests.pop_front() {
                if let Some(token) = request.token(image_request.item.id) {
                    image_request.cancellation = token;
                    live.push(service.submit(image_request));
                }
            }
        }
        if live.is_empty() {
            break;
        }
        let collected = {
            let mut selection = Select::new();
            selection.recv(&request.changed);
            for subscription in &live {
                selection.recv(subscription.receiver());
            }
            match selection.select_timeout(FLUSH_TICK) {
                Ok(operation) if operation.index() == 0 => {
                    let _ = operation.recv(&request.changed);
                    None
                }
                Ok(operation) => {
                    let index = operation.index() - 1;
                    let completion = operation.recv(live[index].receiver()).ok();
                    Some((index, completion))
                }
                Err(_) => None,
            }
        };
        if let Some((index, completion)) = collected {
            let subscription = live.swap_remove(index);
            if completion.is_some_and(|completion| completion.result.is_some()) {
                completed.push(subscription);
            } else if state.with_database_lifecycle_read(epoch, || {
                send(pending_result(subscription.item_id()))
            }) != Some(true)
            {
                disconnected = true;
            }
        }
        if !completed.is_empty()
            && (completed.len() >= FLUSH_LIMIT || last_publish.elapsed() >= FLUSH_TICK)
        {
            super::publication::publish_batch(state, epoch, &completed, |result| {
                if !send(result.clone()) {
                    disconnected = true;
                }
            })?;
            completed.clear();
            last_publish = Instant::now();
        }
    }
    if !completed.is_empty() && !disconnected && !request.cancellation.is_cancelled() {
        super::publication::publish_batch(state, epoch, &completed, |result| {
            if !send(result.clone()) {
                disconnected = true;
            }
        })?;
    }
    Ok(())
}
