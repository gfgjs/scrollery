//! 文件和目录的同卷原子拒绝覆盖重命名。

#[cfg(windows)]
pub(crate) fn rename_no_replace(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS};

    let to_wide = |path: &std::path::Path| -> std::io::Result<Vec<u16>> {
        let name = path.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "移动路径缺少文件名")
        })?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        // 父目录规范化取得长路径前缀；末段保留目录项身份，不跟随源文件链接搬走其目标。
        let absolute = parent.canonicalize()?.join(name);
        let mut value: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "移动路径包含空字符",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = to_wide(source)?;
    let target = to_wide(target)?;
    // 不启用 REPLACE_EXISTING 或 COPY_ALLOWED：原子拒绝抢占目标，跨卷仍按原语义失败。
    // SAFETY: 两个缓冲区均以 NUL 结尾，并在同步 Win32 调用期间保持有效。
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(std::io::Error::other)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn rename_no_replace(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let to_c_string = |path: &std::path::Path| {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "移动路径包含空字符")
        })
    };
    let source = to_c_string(source)?;
    let target = to_c_string(target)?;
    // 直接系统调用避免提高 glibc 动态符号最低版本；不支持的内核或文件系统安全失败。
    #[cfg(target_os = "linux")]
    // SAFETY: C 字符串存活且以 NUL 结尾；目录 fd 和拒绝覆盖标志均为系统定义值。
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    // SAFETY: 同上；RENAME_EXCL 将目标冲突检查与重命名合为一个系统操作。
    let result = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn rename_no_replace(
    _source: &std::path::Path,
    _target: &std::path::Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "当前平台不支持原子拒绝覆盖移动",
    ))
}
