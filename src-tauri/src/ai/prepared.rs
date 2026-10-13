//! 图片与人脸分析的单批预准备交接；模型准备、推理和写回由各自流水线处理。

use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, SendTimeoutError, Sender};
use tokio_util::sync::CancellationToken;

const POLL: Duration = Duration::from_millis(50);

/// 等待下游时不占资源配额；取消或下游关闭后放弃尚未交接的值。
pub(super) fn send_cancellable<T>(tx: &Sender<T>, mut value: T, token: &CancellationToken) -> bool {
    while !token.is_cancelled() {
        match tx.send_timeout(value, POLL) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(pending)) => value = pending,
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
    false
}

/// 当前批推理时准备后继批，最多一批 Ready 等待交接。
/// 零积压通道使准备线程在完成该批后停下；调用方须在返回 Ready 前释放准备配额。
pub(super) fn run_prepared_batches<T: Send, R: Send>(
    tasks: Receiver<T>,
    batch_cap: usize,
    token: &CancellationToken,
    mut prepare: impl FnMut(Vec<T>, &CancellationToken) -> Result<Option<R>, String> + Send,
    mut infer: impl FnMut(R, u128, &CancellationToken) -> Result<(), String>,
) -> Result<(), String> {
    let local = token.child_token();
    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = bounded::<(R, Instant)>(0);
        let preparation_token = local.clone();
        let producer = scope.spawn(move || {
            let result = (|| {
                let mut buf = Vec::with_capacity(batch_cap.max(1));
                loop {
                    if preparation_token.is_cancelled() {
                        return Ok(());
                    }
                    let (received, disconnected) = match tasks.recv_timeout(POLL) {
                        Ok(task) => {
                            buf.push(task);
                            (true, false)
                        }
                        Err(RecvTimeoutError::Timeout) => (false, false),
                        Err(RecvTimeoutError::Disconnected) => (false, true),
                    };
                    // 未满批继续等；空闲一拍或输入关闭时刷尾批。
                    if buf.len() < batch_cap.max(1) && received {
                        continue;
                    }
                    if !buf.is_empty() {
                        let batch = std::mem::take(&mut buf);
                        if let Some(ready) = prepare(batch, &preparation_token)? {
                            if !send_cancellable(
                                &ready_tx,
                                (ready, Instant::now()),
                                &preparation_token,
                            ) {
                                return Ok(());
                            }
                        }
                    }
                    if disconnected {
                        return Ok(());
                    }
                }
            })();
            if result.is_err() {
                preparation_token.cancel();
            }
            result
        });
        let mut inference = Ok(());
        while !local.is_cancelled() {
            match ready_rx.recv_timeout(POLL) {
                Ok((ready, at)) => {
                    if let Err(error) = infer(ready, at.elapsed().as_millis(), &local) {
                        inference = Err(error);
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        local.cancel();
        drop(ready_rx);
        let preparation = producer
            .join()
            .map_err(|_| "AI preparation thread panicked".to_string())?;
        preparation.and(inference)
    })
}
