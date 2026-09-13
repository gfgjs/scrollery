// crates/exotic-workers/video-worker/src/job.rs
//! Windows Job Object(kill-on-close,design.md §9.10 / §2.4)。
//!
//! 进程级单例 job:worker 生命周期内每 spawn 一个 ffmpeg/ffprobe 子进程即 assign 进来。
//! job handle 活在 worker 进程生命周期(OnceLock 静态,永不主动 drop)——worker 被
//! supervisor kill 时,OS 关闭其所有句柄触发 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! 自动收割所有在途 ffmpeg,杜绝孤儿继续烧 CPU 写文件。
//!
//! 非 Windows(macOS/iOS/Android 前瞻,§7)cfg 门控为空实现:进程组/spawn 细节各平台
//! 后续独立适配,协议与本 crate 的 op 逻辑不变。
//!
//! **已裁决接受的残留窗**:`ffrun.rs::spawn()` 先 `Command::spawn()` 起子进程、再调用
//! [`assign_current_job`] 纳管,两步之间存在毫秒级窗口——若 worker 恰在此窗口内被杀,
//! 该子进程尚未入 job、kill-on-close 收割不到它。这是已知且接受的残留窗(概率极低、
//! 窗口极短),kill-on-close 仍是覆盖绝大多数场景的主保险;不追加 spawn 前占位/双重
//! 纳管等复杂化手段。

use std::process::Child;

/// 把子进程纳入进程级 kill-on-close job。best-effort:job 创建/assign 失败只记不阻断
/// (功能仍可用,只是失掉孤儿收割保险);Windows 之外为 no-op。
pub fn assign_current_job(child: &Child) {
    #[cfg(windows)]
    imp::assign(child);
    #[cfg(not(windows))]
    {
        let _ = child;
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// 全局 job handle,以 isize 存(整数天然 Send+Sync,规避裸 HANDLE 的 !Sync);
    /// 0 = 创建失败(退化为无 job)。永不 drop:进程退出时 OS 关句柄触发 kill-on-close。
    static JOB: OnceLock<isize> = OnceLock::new();

    fn job_handle() -> isize {
        *JOB.get_or_init(|| create_job().unwrap_or(0))
    }

    /// 建一个 kill-on-close job object,返回其 handle(isize)。
    fn create_job() -> Option<isize> {
        // SAFETY:标准 Win32 job 创建三步(创建→设扩展限制→返回)。参数均为栈上有效对象,
        // lpname/安全属性传 null(匿名 job)。失败即返回 None。
        unsafe {
            let h: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                // OnceLock::get_or_init 只调用本函数一次,此处 log 天然不会刷屏。
                crate::log_warn(format!(
                    "创建 kill-on-close job 失败(GetLastError={})——孤儿收割保险失效,功能仍可用",
                    GetLastError()
                ));
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info) as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let code = GetLastError();
                CloseHandle(h);
                crate::log_warn(format!(
                    "设置 job kill-on-close 限制失败(GetLastError={code})——孤儿收割保险失效,功能仍可用"
                ));
                return None;
            }
            Some(h as isize)
        }
    }

    pub fn assign(child: &Child) {
        let h = job_handle();
        if h == 0 {
            return;
        }
        // SAFETY:h 为有效 job handle,child 存活(引用)故其进程 handle 有效。
        unsafe {
            let ok = AssignProcessToJobObject(h as HANDLE, child.as_raw_handle() as HANDLE);
            if ok == 0 {
                crate::log_warn(format!(
                    "AssignProcessToJobObject 失败(GetLastError={})——该子进程未纳入 kill-on-close job(孤儿收割保险对其失效)",
                    GetLastError()
                ));
            }
        }
    }
}
