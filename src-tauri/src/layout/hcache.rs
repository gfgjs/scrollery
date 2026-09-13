//! H-Lab(横向画廊实验)独立布局缓存——与生产 `layout_cache` 互不可见。
//!
//! 契约(docs/designs/2026-07-02-horizontal-gallery-lab.md §2):
//!   - `compute_h_layout` 存入,`get_h_blocks_by_x` 按 bbox 相交取块;
//!   - 版本号独立计数,**锁内递增**(承接生产缓存 R0-3 写序倒置教训);
//!   - 取块为线性过滤:块数 ≈ 项数/3,数十万项也只是微秒级,实验期不建索引;
//!   - **有意不接**缩略图批量回写(不触碰 8 处生产回写点):缓存中 thumb_status=0 的项
//!     滚回可视区后由前端重发请求,命中后端「已生成快路径」立即返回新状态并就地 patch,
//!     以少量 IPC 往返换取与生产管线的零耦合。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::layout::horizontal::HBlock;

static H_LAYOUT_VERSION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct HLayoutCacheData {
    pub blocks: Vec<HBlock>,
    pub total_width: f64,
    pub layout_version: u64,
    pub total_items: usize,
}

pub type HLayoutCache = RwLock<Option<HLayoutCacheData>>;

pub fn new_h_layout_cache() -> HLayoutCache {
    RwLock::new(None)
}

/// 存入新布局,返回版本号。版本在**写锁内**递增:保证「版本单调序 == 实际写入序」,
/// 避免并发计算下缓存留旧块集配小版本号、前端握大版本号后恒不匹配(生产 R0-3 同款守护)。
pub fn store_h_layout(cache: &HLayoutCache, blocks: Vec<HBlock>, total_width: f64) -> u64 {
    let total_items = blocks.iter().map(|b| b.items.len()).sum();
    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    let version = H_LAYOUT_VERSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    *guard = Some(HLayoutCacheData {
        blocks,
        total_width,
        layout_version: version,
        total_items,
    });
    version
}

/// 取与 [left_x, right_x] 相交的块(bbox 相交;lanes 模式相邻块 bbox 可重叠,故不能只按
/// 左缘二分,线性过滤最稳)。缓存为空或版本不符 → None(命令层抛 LayoutNotReady)。
pub fn get_h_blocks_by_x(
    cache: &HLayoutCache,
    left_x: f64,
    right_x: f64,
    expected_version: Option<u64>,
) -> Option<Vec<HBlock>> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;

    if let Some(ver) = expected_version {
        if data.layout_version != ver {
            return None;
        }
    }

    Some(
        data.blocks
            .iter()
            .filter(|b| b.x <= right_x && b.x + b.width >= left_x)
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::horizontal::{HBlock, HItem};

    fn mk_block(x: f64, width: f64, ids: &[i64]) -> HBlock {
        HBlock {
            x,
            width,
            items: ids
                .iter()
                .map(|&id| HItem {
                    id,
                    x,
                    y: 0.0,
                    w: width,
                    h: 100.0,
                    media_type: "image".into(),
                    file_format: "jpg".into(),
                    file_size: 0,
                    is_live_photo: false,
                    duration_ms: None,
                    thumb_status: 0,
                    thumb_path: None,
                    thumbhash: None,
                })
                .collect(),
        }
    }

    #[test]
    fn store_then_get_by_x_intersects_bbox() {
        let cache = new_h_layout_cache();
        let blocks = vec![
            mk_block(0.0, 200.0, &[1, 2]),
            mk_block(204.0, 200.0, &[3]),
            mk_block(408.0, 200.0, &[4, 5]),
        ];
        let version = store_h_layout(&cache, blocks, 608.0);

        // 窗口 [150, 250]:跨块 0 右缘与块 1 左缘 → 两块都返回。
        let got = get_h_blocks_by_x(&cache, 150.0, 250.0, Some(version)).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].x, got[1].x), (0.0, 204.0));

        // 边界相切(right = 块 2 左缘)也算相交。
        let got = get_h_blocks_by_x(&cache, 0.0, 408.0, None).unwrap();
        assert_eq!(got.len(), 3);

        // 窗口在末块右侧之外 → 空集(而非 None)。
        let got = get_h_blocks_by_x(&cache, 5000.0, 6000.0, Some(version)).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn version_guard_and_empty_cache() {
        let cache = new_h_layout_cache();
        assert!(
            get_h_blocks_by_x(&cache, 0.0, 100.0, None).is_none(),
            "无布局 → None(命令层抛 LayoutNotReady)"
        );
        let version = store_h_layout(&cache, vec![mk_block(0.0, 100.0, &[1])], 100.0);
        assert!(get_h_blocks_by_x(&cache, 0.0, 100.0, Some(version)).is_some());
        assert!(
            get_h_blocks_by_x(&cache, 0.0, 100.0, Some(version + 99)).is_none(),
            "版本不符 → None"
        );
        // total_items 由块内项数聚合。
        assert_eq!(cache.read().unwrap().as_ref().unwrap().total_items, 1);
    }
}
