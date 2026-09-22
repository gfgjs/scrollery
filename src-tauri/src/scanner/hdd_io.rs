//! HDD 图片富化按实际访问调度：头读共享，补读独占；超时线程延长占用。

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, Result};

#[derive(Debug, Default)]
struct DiskState {
    headers: usize,
    supplemental: bool,
    waiting_supplemental: usize,
    timed_out: bool,
}

#[derive(Debug)]
pub(crate) struct DiskBudget {
    pub disk: u32,
    pub header_limit: usize,
    state: Mutex<DiskState>,
    available: Condvar,
}

impl DiskBudget {
    fn new(disk: u32, header_limit: usize) -> Self {
        Self {
            disk,
            header_limit,
            state: Mutex::default(),
            available: Condvar::new(),
        }
    }

    /// 只覆盖实际头读，必须在将头缓冲交给解析队列之前释放。
    pub(crate) fn acquire_header(
        self: &Arc<Self>,
        cancel: &CancellationToken,
    ) -> Result<HeaderPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            if state.timed_out {
                return Err(AppError::ImageReadTimeout);
            }
            // 补读优先，避免连续头读让寻道密集的补读一直排队。
            if !state.supplemental
                && state.waiting_supplemental == 0
                && state.headers < self.header_limit
            {
                state.headers += 1;
                return Ok(HeaderPermit(Arc::clone(self)));
            }
            state = self
                .available
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// 补读独占同物理盘的图片读取；共享守卫可转交 TIFF 超时线程。
    pub(crate) fn acquire(self: &Arc<Self>, cancel: &CancellationToken) -> Result<Arc<ReadPermit>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.waiting_supplemental += 1;
        loop {
            if cancel.is_cancelled() {
                state.waiting_supplemental -= 1;
                self.available.notify_all();
                return Err(AppError::Cancelled);
            }
            if state.timed_out {
                state.waiting_supplemental -= 1;
                self.available.notify_all();
                return Err(AppError::ImageReadTimeout);
            }
            if !state.supplemental && state.headers == 0 {
                state.waiting_supplemental -= 1;
                state.supplemental = true;
                return Ok(Arc::new(ReadPermit(Arc::clone(self))));
            }
            state = self
                .available
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

pub(crate) struct HeaderPermit(Arc<DiskBudget>);
impl Drop for HeaderPermit {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .headers -= 1;
        self.0.available.notify_all();
    }
}

#[derive(Debug)]
pub(crate) struct ReadPermit(Arc<DiskBudget>);
impl ReadPermit {
    /// 超时只唤醒等待者报错，不释放仍在实际读取的独占名额。
    pub(crate) fn mark_timed_out(&self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .timed_out = true;
        self.0.available.notify_all();
    }

    pub(crate) fn timed_out(&self) -> bool {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .timed_out
    }
}

impl Drop for ReadPermit {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.supplemental = false;
        state.timed_out = false;
        drop(state);
        self.0.available.notify_all();
    }
}

pub(crate) fn header_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|c| c.get().saturating_mul(2).clamp(4, 32))
        .unwrap_or(8)
}

fn shared_budget(disk: u32) -> Arc<DiskBudget> {
    static BUDGETS: OnceLock<Mutex<HashMap<u32, Weak<DiskBudget>>>> = OnceLock::new();
    let mut budgets = BUDGETS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(budget) = budgets.get(&disk).and_then(Weak::upgrade) {
        return budget;
    }
    budgets.retain(|_, budget| budget.strong_count() > 0);
    let budget = Arc::new(DiskBudget::new(disk, header_worker_count()));
    budgets.insert(disk, Arc::downgrade(&budget));
    budget
}

#[cfg(test)]
pub(crate) fn test_budget(header_limit: usize) -> Arc<DiskBudget> {
    Arc::new(DiskBudget::new(0, header_limit))
}

/// 只在系统确认存在寻道惩罚且能取得单个设备号时自动调度；未知设备保持既有路径。
pub(crate) fn for_root(path: &str) -> Option<Arc<DiskBudget>> {
    let disk = seek_penalty_disk(path)?;
    Some(shared_budget(disk))
}

#[cfg(not(windows))]
fn seek_penalty_disk(_path: &str) -> Option<u32> {
    None
}

#[cfg(windows)]
fn seek_penalty_disk(path: &str) -> Option<u32> {
    use std::ffi::c_void;
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };

    // Win32 winioctl.h 的固定 ABI；仅查询属性，打开卷时不请求数据读写权限。
    #[link(name = "kernel32")]
    extern "system" {
        fn DeviceIoControl(
            handle: *mut c_void,
            code: u32,
            input: *const c_void,
            input_len: u32,
            output: *mut c_void,
            output_len: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut mount = [0u16; 32768];
    let mut volume = [0u16; 64];
    unsafe {
        GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut mount).ok()?;
        GetVolumeNameForVolumeMountPointW(PCWSTR(mount.as_ptr()), &mut volume).ok()?;
    }
    let end = volume.iter().position(|&v| v == 0)?;
    let name = String::from_utf16(&volume[..end]).ok()?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .access_mode(0)
        .share_mode(3)
        .open(name.trim_end_matches('\\'))
        .ok()?;
    // STORAGE_PROPERTY_QUERY { StorageDeviceSeekPenaltyProperty=7, PropertyStandardQuery=0 }。
    let query = [7u32, 0, 0];
    let mut descriptor = [0u32; 3];
    let mut returned = 0;
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            2_954_240,
            query.as_ptr().cast(),
            12,
            descriptor.as_mut_ptr().cast(),
            12,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0
        || returned < 9
        || descriptor[0] < 12
        || descriptor[1] < 12
        || descriptor[2] & 0xff == 0
    {
        return None;
    }
    // STORAGE_DEVICE_NUMBER；跨盘卷不支持该请求时保持 Unknown，不按盘符猜物理盘。
    let mut device = [0u32; 3];
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            2_953_344,
            std::ptr::null(),
            0,
            device.as_mut_ptr().cast(),
            12,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    (ok != 0 && returned >= 12 && device[0] == 7).then_some(device[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_supplemental(budget: &DiskBudget) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while budget.state.lock().unwrap().waiting_supplemental == 0 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn timeout_wakes_waiters_rejects_restart_and_recovers_only_after_final_drop() {
        let budget = shared_budget(u32::MAX - 2);
        let cancel = CancellationToken::new();
        let permit = budget.acquire(&cancel).unwrap();
        let background = Arc::clone(&permit);
        let (done, results) = std::sync::mpsc::channel();
        let b = Arc::clone(&budget);
        let tx = done.clone();
        let header = std::thread::spawn(move || {
            tx.send(b.acquire_header(&CancellationToken::new()).map(|_| ()))
                .unwrap();
        });
        let b = Arc::clone(&budget);
        let supplemental = std::thread::spawn(move || {
            done.send(b.acquire(&CancellationToken::new()).map(|_| ()))
                .unwrap();
        });
        wait_for_supplemental(&budget);
        permit.mark_timed_out();
        assert!(matches!(
            results.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(AppError::ImageReadTimeout)
        ));
        assert!(matches!(
            results.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(AppError::ImageReadTimeout)
        ));
        header.join().unwrap();
        supplemental.join().unwrap();
        assert_eq!(budget.state.lock().unwrap().waiting_supplemental, 0);
        cancel.cancel();
        assert!(matches!(budget.acquire(&cancel), Err(AppError::Cancelled)));
        drop(permit);
        drop(budget);
        let restarted = shared_budget(u32::MAX - 2);
        assert!(matches!(
            restarted.acquire_header(&CancellationToken::new()),
            Err(AppError::ImageReadTimeout)
        ));
        assert!(shared_budget(u32::MAX - 3)
            .acquire(&CancellationToken::new())
            .is_ok());
        drop(background);
        assert!(restarted.acquire_header(&CancellationToken::new()).is_ok());
        assert!(restarted.acquire(&CancellationToken::new()).is_ok());
    }

    #[test]
    fn same_disk_shares_budget_and_timeout_clone_keeps_exclusion() {
        let a = shared_budget(u32::MAX);
        let b = shared_budget(u32::MAX);
        let other = shared_budget(u32::MAX - 1);
        assert!(Arc::ptr_eq(&a, &b));
        let token = CancellationToken::new();
        let lease = a.acquire(&token).unwrap();
        assert!(other.acquire(&token).is_ok());
        let child = Arc::clone(&lease);
        drop(lease);
        assert!(b.state.lock().unwrap().supplemental);
        drop(child);
        assert!(!b.state.lock().unwrap().supplemental);
        assert!(b.acquire_header(&token).is_ok());
    }

    #[test]
    fn supplemental_waits_for_headers_and_blocks_new_headers() {
        let budget = test_budget(2);
        let cancel = CancellationToken::new();
        let first = budget.acquire_header(&cancel).unwrap();
        let second = budget.acquire_header(&cancel).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let b = Arc::clone(&budget);
        let worker = std::thread::spawn(move || {
            tx.send(b.acquire(&CancellationToken::new()).unwrap())
                .unwrap();
        });
        wait_for_supplemental(&budget);
        drop(first);
        assert!(rx.try_recv().is_err());
        // 已有补读等待时，即便还有空闲头读名额也必须等候；取消可退出。
        let b = Arc::clone(&budget);
        let c = cancel.clone();
        let header = std::thread::spawn(move || b.acquire_header(&c));
        cancel.cancel();
        assert!(matches!(header.join().unwrap(), Err(AppError::Cancelled)));
        drop(second);
        let lease = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        assert!(budget.state.lock().unwrap().supplemental);
        drop(lease);
        assert!(budget.acquire_header(&CancellationToken::new()).is_ok());
    }

    #[test]
    fn cancelling_waiting_supplemental_reopens_headers() {
        let budget = test_budget(2);
        let token = CancellationToken::new();
        let _header = budget.acquire_header(&token).unwrap();
        let b = Arc::clone(&budget);
        let c = token.clone();
        let waiting = std::thread::spawn(move || b.acquire(&c));
        wait_for_supplemental(&budget);
        token.cancel();
        assert!(matches!(waiting.join().unwrap(), Err(AppError::Cancelled)));
        assert_eq!(budget.state.lock().unwrap().waiting_supplemental, 0);
        assert!(budget.acquire_header(&CancellationToken::new()).is_ok());
    }

    #[test]
    fn panic_releases_both_permit_kinds() {
        let budget = test_budget(2);
        for supplemental in [false, true] {
            let _ = std::panic::catch_unwind(|| {
                let token = CancellationToken::new();
                let _header = (!supplemental).then(|| budget.acquire_header(&token).unwrap());
                let _read = supplemental.then(|| budget.acquire(&token).unwrap());
                panic!("read failure");
            });
            let state = budget.state.lock().unwrap();
            assert_eq!(state.headers, 0);
            assert!(!state.supplemental);
        }
    }
}
