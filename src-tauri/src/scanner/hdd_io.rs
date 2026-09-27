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

/// 只在系统确认存在寻道惩罚且能取得单个设备号时自动调度；未知设备保持既有路径。
pub(crate) fn for_root(path: &str) -> Option<Arc<DiskBudget>> {
    let device = storage_device(path, None)?;
    device
        .incurs_seek_penalty
        .then(|| shared_budget(device.number))
}

/// 系统确认的单设备身份和寻道属性；未知、多盘卷保持 None。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StorageDevice {
    pub number: u32,
    pub incurs_seek_penalty: bool,
}

#[cfg(not(windows))]
pub(crate) fn storage_device(_path: &str, _expected_volume: Option<&str>) -> Option<StorageDevice> {
    None
}

#[cfg(windows)]
pub(crate) fn storage_device(path: &str, expected_volume: Option<&str>) -> Option<StorageDevice> {
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
    // 可移动盘复用盘符时，旧卷记录不能借用新设备的并行额度。
    if expected_volume.is_some_and(|expected| {
        !expected.eq_ignore_ascii_case(&format!("win:{}", name.trim_end_matches('\\')))
    }) {
        return None;
    }
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
    if ok == 0 || returned < 9 || descriptor[0] < 12 || descriptor[1] < 12 {
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
    (ok != 0 && returned >= 12 && device[0] == 7).then_some(StorageDevice {
        number: device[1],
        incurs_seek_penalty: descriptor[2] & 0xff != 0,
    })
}
