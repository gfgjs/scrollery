//! 视口 IPC 的逐项订阅身份；独立于共享图片任务，不维护另一套执行队列。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crossbeam_channel::{bounded, Receiver, Sender};
use tokio_util::sync::CancellationToken;

use crate::db::models::ThumbResult;
use crate::error::{AppError, Result};

type Entries = Mutex<HashMap<String, Weak<ViewportRequest>>>;

#[derive(Default)]
pub(crate) struct ViewportRequests(Arc<Entries>);

impl ViewportRequests {
    pub fn register(&self, id: String, item_ids: &[i64]) -> Result<Arc<ViewportRequest>> {
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // 不在登记锁内临时升级 Arc：最后一个强引用析构也需要这把锁。
        if entries
            .get(&id)
            .is_some_and(|entry| entry.strong_count() > 0)
        {
            return Err(AppError::Internal("duplicate viewport request id".into()));
        }
        let cancellation = CancellationToken::new();
        let (wake, changed) = bounded(1);
        let request = Arc::new(ViewportRequest {
            id: id.clone(),
            active: Mutex::new(
                item_ids
                    .iter()
                    .map(|id| (*id, cancellation.child_token()))
                    .collect(),
            ),
            cancellation,
            wake,
            changed,
            registry: Arc::downgrade(&self.0),
        });
        entries.insert(id, Arc::downgrade(&request));
        Ok(request)
    }

    pub fn cancel(&self, id: &str, item_ids: &[i64]) {
        let request = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .and_then(Weak::upgrade);
        if let Some(request) = request {
            let mut active = request.active.lock().unwrap_or_else(|e| e.into_inner());
            for id in item_ids {
                if let Some(token) = active.remove(id) {
                    token.cancel();
                }
            }
            if active.is_empty() {
                request.cancellation.cancel();
            }
            let _ = request.wake.try_send(());
        }
    }
}

pub(crate) struct ViewportRequest {
    id: String,
    active: Mutex<HashMap<i64, CancellationToken>>,
    pub cancellation: CancellationToken,
    wake: Sender<()>,
    pub changed: Receiver<()>,
    registry: Weak<Entries>,
}

impl ViewportRequest {
    pub fn token(&self, item_id: i64) -> Option<CancellationToken> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&item_id)
            .cloned()
    }

    pub fn remaining(&self) -> Vec<i64> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect()
    }

    /// 与取消串行化；通道失败时撤销本请求剩余工作，但不影响其它订阅。
    pub fn deliver(&self, item_id: i64, send: impl FnOnce() -> bool) -> bool {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.remove(&item_id).is_none() || self.cancellation.is_cancelled() {
            return true;
        }
        if send() {
            true
        } else {
            self.cancellation.cancel();
            active.clear();
            let _ = self.wake.try_send(());
            false
        }
    }
}

impl Drop for ViewportRequest {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(registry) = self.registry.upgrade() {
            let mut entries = registry.lock().unwrap_or_else(|e| e.into_inner());
            if entries
                .get(&self.id)
                .is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self))
            {
                entries.remove(&self.id);
            }
        }
    }
}

/// Channel 的逐项回包；让路不重试，仅过期结果允许重新订阅。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportThumbResult {
    #[serde(flatten)]
    pub(crate) result: ThumbResult,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

impl From<ThumbResult> for ViewportThumbResult {
    fn from(result: ThumbResult) -> Self {
        Self {
            outcome: if result.thumb_status == 0 {
                "deferred"
            } else {
                "complete"
            },
            result,
            error: None,
        }
    }
}

impl ViewportThumbResult {
    pub(crate) fn retry(item_id: i64) -> Self {
        Self {
            outcome: "retry",
            ..pending_result(item_id).into()
        }
    }

    pub(crate) fn failed(item_id: i64, error: &AppError) -> Self {
        Self {
            outcome: "failed",
            // 复用 AppError 的稳定码和脱敏序列化，不把内部 SQL/路径放入回包。
            error: Some(serde_json::json!(error)),
            ..pending_result(item_id).into()
        }
    }
}

pub(crate) fn pending_result(item_id: i64) -> ThumbResult {
    ThumbResult {
        item_id,
        thumb_status: 0,
        thumb_path: None,
        thumbhash: None,
        source_revision: 0,
        cache_key: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_and_stale_results_have_distinct_retry_semantics() {
        let deferred = serde_json::to_value(ViewportThumbResult::from(pending_result(7))).unwrap();
        let stale = serde_json::to_value(ViewportThumbResult::retry(7)).unwrap();
        assert_eq!(deferred["outcome"], "deferred");
        assert_eq!(stale["outcome"], "retry");
        assert_eq!(deferred["thumbStatus"], 0);
        assert!(deferred.get("error").is_none());
    }

    #[test]
    fn failed_result_preserves_db_code_without_internal_sql() {
        let error = AppError::Db(rusqlite::Error::InvalidParameterName("private SQL".into()));
        let result = serde_json::to_value(ViewportThumbResult::failed(7, &error)).unwrap();
        assert_eq!(result["outcome"], "failed");
        assert_eq!(result["error"]["code"], "Db");
        assert!(!result.to_string().contains("private SQL"));
    }
}
