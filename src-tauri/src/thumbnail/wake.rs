//! 派发唤醒只合并状态变化；结果仍由各 ticket 的有界通道持有。

use crossbeam_channel::{bounded, Receiver, Sender};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Wake {
    sender: Sender<()>,
    receiver: Receiver<()>,
}

impl Default for Wake {
    fn default() -> Self {
        let (sender, receiver) = bounded(1);
        Self { sender, receiver }
    }
}

impl Wake {
    pub(crate) fn notify(&self) {
        let _ = self.sender.try_send(());
    }

    /// 必须在检查工作之前消费，检查与等待之间的通知由通道保留。
    pub(crate) fn clear(&self) {
        let _ = self.receiver.try_recv();
    }

    pub(crate) fn wait(&self) -> bool {
        // 外部取消令牌没有通道通知，保留有限复查；正常回包直接唤醒。
        self.receiver
            .recv_timeout(Duration::from_millis(100))
            .is_ok()
    }
}
