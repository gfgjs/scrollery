//! 布局请求登记、输入失效与发布共用的短临界区，不在其中查询数据库或计算几何。

use std::sync::Mutex;

/// 一次布局请求登记时的身份和输入代，不跨请求复用。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LayoutTicket {
    request: u64,
    inputs: u64,
}

#[derive(Default)]
struct Gate {
    request: u64,
    inputs: u64,
    writers: usize,
}

/// 只允许最后登记且输入仍有效的候选发布；失败不恢复旧请求资格。
#[derive(Default)]
pub struct LayoutPublication(Mutex<Gate>);

impl LayoutPublication {
    /// 在 IPC 入口登记，不能等阻塞线程开始执行后才领取身份。
    pub fn begin(&self) -> LayoutTicket {
        let mut gate = self.0.lock().unwrap_or_else(|e| e.into_inner());
        gate.request += 1;
        LayoutTicket {
            request: gate.request,
            inputs: gate.inputs,
        }
    }

    /// 校验与发布同锁，期间新请求和输入写入不能穿插。
    pub fn publish<T>(&self, ticket: LayoutTicket, write: impl FnOnce() -> T) -> Option<T> {
        let gate = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if gate.request != ticket.request || gate.inputs != ticket.inputs || gate.writers != 0 {
            return None;
        }
        Some(write())
    }

    /// 生产唯一发布入口：让取数槽与新布局持有相同的载荷句柄。
    pub fn install(
        &self,
        ticket: LayoutTicket,
        cache: &super::cache::LayoutCache,
        slot: &super::items_cache::ItemsCacheSlot,
        prepared: super::cache::PreparedLayout,
    ) -> Option<super::cache::LayoutSummary> {
        let source = prepared.data.source_items.as_ref()?.clone();
        // 缩略图 patch 可能正等待旧快照读锁；先等槽锁，再进发布闸门，避免阻塞新请求登记。
        let mut current = slot.write().unwrap_or_else(|e| e.into_inner());
        let committed = self.publish(ticket, || {
            let old = std::mem::replace(&mut *current, source);
            let summary = super::cache::publish_layout(cache, prepared);
            (summary, old)
        });
        drop(current);
        // 旧体在闸门之外释放；旧布局在缓存层另行后台释放。
        committed.map(|(summary, _old)| summary)
    }

    /// 写入登记与发布互斥；等待 items 写锁时释放闸门，不阻塞新 IPC 登记。
    /// 在途写入期间拒绝发布，完成后再换输入代；现行布局仍可用于过渡浏览。
    pub fn change<T>(&self, write: impl FnOnce() -> T) -> T {
        {
            let mut gate = self.0.lock().unwrap_or_else(|e| e.into_inner());
            gate.inputs += 1;
            gate.writers += 1;
        }
        struct Finish<'a>(&'a LayoutPublication);
        impl Drop for Finish<'_> {
            fn drop(&mut self) {
                let mut gate = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
                gate.inputs += 1;
                gate.writers -= 1;
            }
        }
        let _finish = Finish(self);
        write()
    }
}
