//! 源盘准入：提交时分类，调度时尝试占位，收到 Decoded 后释放源读取许可。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// 系统确认的物理盘类型；无法确定单个设备时统一使用保守额度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceDevice {
    Ssd(u32),
    Hdd(u32),
    Unknown,
}

#[derive(Default)]
struct Occupancy {
    hdds: HashSet<u32>,
    unknown: bool,
}

/// 同一服务共享的分类缓存和源盘额度。
pub(crate) struct SourceBudget {
    devices: Mutex<HashMap<(u64, String), SourceDevice>>,
    occupied: Mutex<Occupancy>,
}

impl SourceBudget {
    /// 创建空缓存和空闲额度。
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::default(),
            occupied: Mutex::default(),
        })
    }

    /// 按 `(epoch, root)` 缓存分类，包括 Unknown。
    ///
    /// 首次探测执行同步系统调用，调用方应在提交阶段的非 UI 线程调用。
    pub(crate) fn classify(&self, epoch: u64, root: &str) -> SourceDevice {
        self.classify_with(epoch, root, probe_device)
    }

    fn classify_with(
        &self,
        epoch: u64,
        root: &str,
        probe: impl FnOnce(&str) -> SourceDevice,
    ) -> SourceDevice {
        let key = (epoch, root.to_owned());
        if let Some(device) = self
            .devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .copied()
        {
            return device;
        }
        // 探测可能阻塞；不占分类锁，也不占调度额度锁。
        let device = probe(root);
        // 同键并发探测时保留先发布的结果，使本 epoch 内分类一致。
        *self
            .devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_insert(device)
    }

    /// 仅访问内存，不等待空闲额度；失败时调度器应继续遍历有界队列。
    pub(crate) fn try_acquire(self: &Arc<Self>, device: SourceDevice) -> Option<SourcePermit> {
        match device {
            SourceDevice::Ssd(_) => {}
            SourceDevice::Hdd(disk) => {
                if !self
                    .occupied
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .hdds
                    .insert(disk)
                {
                    return None;
                }
            }
            SourceDevice::Unknown => {
                let mut occupied = self.occupied.lock().unwrap_or_else(|e| e.into_inner());
                if occupied.unknown {
                    return None;
                }
                occupied.unknown = true;
            }
        }
        Some(SourcePermit {
            budget: Arc::clone(self),
            device,
        })
    }
}

/// 源读取阶段许可；调用方应在 Decoded 事件时丢弃，异常退出亦由 Drop 归还。
pub(crate) struct SourcePermit {
    budget: Arc<SourceBudget>,
    device: SourceDevice,
}

impl Drop for SourcePermit {
    fn drop(&mut self) {
        match self.device {
            SourceDevice::Ssd(_) => {}
            SourceDevice::Hdd(disk) => {
                self.budget
                    .occupied
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .hdds
                    .remove(&disk);
            }
            SourceDevice::Unknown => {
                self.budget
                    .occupied
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .unknown = false;
            }
        }
    }
}

#[cfg(not(windows))]
fn probe_device(_root: &str) -> SourceDevice {
    SourceDevice::Unknown
}

#[cfg(windows)]
fn probe_device(root: &str) -> SourceDevice {
    match storage_device(root) {
        Some(StorageDevice {
            number,
            incurs_seek_penalty: true,
        }) => SourceDevice::Hdd(number),
        Some(StorageDevice {
            number,
            incurs_seek_penalty: false,
        }) => SourceDevice::Ssd(number),
        None => SourceDevice::Unknown,
    }
}

// 选择性复用 a9d5a51d 的 scanner/hdd_io.rs：卷 GUID、单设备号和寻道属性探测。
#[cfg(windows)]
struct StorageDevice {
    number: u32,
    incurs_seek_penalty: bool,
}

#[cfg(windows)]
fn storage_device(path: &str) -> Option<StorageDevice> {
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
