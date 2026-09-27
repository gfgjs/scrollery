// src-tauri/src/tree/cache.rs
//! 目录快照缓存 —— 支撑「所有文件」模式的**稳定分页**（S 线 §4.2）。
//!
//! ## 为什么需要
//!
//! 分页每页 200 项。若每页都重跑 `read_dir + sort`，一个 10 万项的目录翻完要 500 次
//! O(n log n) 全量枚举 —— 平方级。更要命的是**不稳定**：两页之间目录若有增删，用户会看到
//! 重复或漏项。快照一次、按下标切页，两个问题一起解决。
//!
//! ## 为什么自己写而不用 `lru` crate
//!
//! 需要 [`invalidate_root`](DirSnapshotCache::invalidate_root) 这种「按 root 前缀批量失效」，
//! `lru::LruCache` 也得自己遍历收键再删，省不下多少；而容量只有个位数，线性扫描比
//! HashMap + 侵入链表更简单也更快。
//!
//! ## 内存边界
//!
//! **同时按两个维度封顶**：快照条数与总条目数。只封条数是不够的 —— 8 个 10 万项目录
//! ≈ 50MB 常驻，对一个侧栏面板而言不可接受。

use std::sync::{Arc, Mutex};

use super::FsEntry;

/// 最多缓存几个目录的快照。用户同时展开并翻页的目录数量级是个位数。
const MAX_SNAPSHOTS: usize = 8;

/// 全部快照的条目总数上限。超过即从最久未用的开始逐出（即便条数没超）。
/// 20 万条 × 约 64 字节 ≈ 13MB 量级 —— 侧栏面板可接受的常驻上界。
const MAX_TOTAL_ENTRIES: usize = 200_000;

/// 快照键。`include_hidden` 必须进键：切「所有文件」↔「所有文件 + 隐藏项」是**不同集合**，
/// 共用一个快照会让切换后仍显示旧集合（D-002 要求切模式后共有项顺序不变，但集合本身要变）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotKey {
    pub root_id: i64,
    pub rel_path: String,
    pub include_hidden: bool,
}

/// 一页结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub entries: Vec<FsEntry>,
    /// 下一页的游标；`None` = 已到末页。
    pub next_cursor: Option<usize>,
    /// 该目录的直接子项总数（前端显示「已加载 N / 共 M」用；**不是**递归媒体数）。
    pub total: usize,
}

/// 有界 LRU 目录快照缓存。
///
/// 锁语义：内部用 `std::sync::Mutex`，所有方法都是**同步**的且不跨 `.await`
/// （项目硬约束：不得跨 await 持 std Mutex guard）。调用方应在 `spawn_blocking` 内使用。
pub struct DirSnapshotCache {
    /// 尾部 = 最近使用。容量个位数，`Vec` 线性扫描足够。
    inner: Mutex<Vec<(SnapshotKey, Arc<Vec<FsEntry>>)>>,
}

impl Default for DirSnapshotCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DirSnapshotCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    /// 取快照；命中则提升为最近使用。
    pub fn get(&self, key: &SnapshotKey) -> Option<Arc<Vec<FsEntry>>> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let pos = g.iter().position(|(k, _)| k == key)?;
        let hit = g.remove(pos);
        let snap = Arc::clone(&hit.1);
        g.push(hit); // 移到尾部 = 最近使用
        Some(snap)
    }

    /// 存快照（同键覆盖），随后按两个维度逐出最久未用者。
    pub fn put(&self, key: SnapshotKey, snap: Arc<Vec<FsEntry>>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = g.iter().position(|(k, _)| *k == key) {
            g.remove(pos);
        }
        g.push((key, snap));

        // 先按条数封顶。
        while g.len() > MAX_SNAPSHOTS {
            g.remove(0);
        }
        // 再按总条目数封顶；保底留一个（否则刚存的巨目录会被自己挤掉，下一页必然 MISS，
        // 退化成「每页全量重扫」—— 正是本缓存要消除的病）。
        while g.len() > 1 && g.iter().map(|(_, v)| v.len()).sum::<usize>() > MAX_TOTAL_ENTRIES {
            g.remove(0);
        }
    }

    /// 失效某扫描根下的全部快照（该根重扫完成/卷状态变化时调用）。
    pub fn invalidate_root(&self, root_id: i64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.retain(|(k, _)| k.root_id != root_id);
    }

    /// 清空（显式刷新）。
    pub fn clear(&self) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// 从快照切出一页。
///
/// `cursor` 是**下标**（快照内偏移）。越界游标返回空页而非报错 —— 目录在翻页途中缩小是
/// 正常竞态，不该让前端吃一个错误弹窗。
pub fn slice_page(snap: &[FsEntry], cursor: usize, page_size: usize) -> Page {
    let total = snap.len();
    let start = cursor.min(total);
    let end = start.saturating_add(page_size).min(total);
    Page {
        entries: snap[start..end].to_vec(),
        next_cursor: if end < total { Some(end) } else { None },
        total,
    }
}
