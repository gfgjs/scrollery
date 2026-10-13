//! 收集线程内的产品进度摘要；只统计已提交结果，不增加逐图锁或数据库查询。

use crate::db::models::ThumbResult;

use super::benchmark::{MeasuredOutcome, Publication, WorkMetrics};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResultCounts {
    pub available: u64,
    pub newly_generated: u64,
    pub cache_hit: u64,
    pub direct: u64,
    pub failed: u64,
    pub temporarily_unavailable: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbExecutionCount {
    pub backend: Option<String>,
    pub count: u64,
}

#[derive(Default)]
pub(crate) struct PublishedSummary {
    results: ThumbResultCounts,
    pub executions: Vec<ThumbExecutionCount>,
}

impl PublishedSummary {
    pub fn record_batch(
        &mut self,
        results: &[ThumbResult],
        measurements: &[WorkMetrics],
        published: &[Publication],
    ) {
        for ((result, metrics), publication) in results.iter().zip(measurements).zip(published) {
            if matches!(publication, Publication::Stale) {
                continue;
            }
            match result.thumb_status {
                3 => self.results.direct += 1,
                2 => self.results.failed += 1,
                1 => {
                    self.results.available += 1;
                    if matches!(publication, Publication::Accepted)
                        && matches!(metrics.item.outcome, Some(MeasuredOutcome::Generated))
                    {
                        self.results.newly_generated += 1;
                        let backend = &metrics.item.backend;
                        if let Some(entry) =
                            self.executions.iter_mut().find(|v| &v.backend == backend)
                        {
                            entry.count += 1;
                        } else {
                            self.executions.push(ThumbExecutionCount {
                                backend: backend.clone(),
                                count: 1,
                            });
                        }
                    } else {
                        self.results.cache_hit += 1;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn counts(&self, processed: u64) -> ThumbResultCounts {
        let mut counts = self.results.clone();
        // processed 已包含候选失效、服务不可用和发布失效，不包含尚未处理的取消项。
        counts.temporarily_unavailable =
            processed.saturating_sub(counts.available + counts.direct + counts.failed);
        counts
    }
}
