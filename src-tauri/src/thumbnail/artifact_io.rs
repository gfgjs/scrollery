//! 缓存产物的原子发布；父目录一经打开，后续操作固定于已授权对象。

use std::io;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

fn denied() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "invalid artifact path")
}

fn components(relative: &str) -> io::Result<Vec<&str>> {
    let parts: Vec<_> = relative.split(['/', '\\']).collect();
    for part in &parts {
        // 不让 Path::components 的规范化吞掉空组件或中间的 `.`，并拒绝 ADS。
        if part.is_empty()
            || part.contains(['\0', ':'])
            || !matches!(
                Path::new(part).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err(denied());
        }
        #[cfg(windows)]
        {
            // Win32 别名不能成为另一条写入设备或其它目录项的入口。
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if part.ends_with(['.', ' '])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || part.chars().any(|c| c.is_control())
                || matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(
                        &stem[3..],
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
            {
                return Err(denied());
            }
        }
    }
    Ok(parts)
}

fn temporary_name() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        ".artifact-{}-{time:x}-{:x}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// 在已有的用户缓存根下原子写入或替换宿主生成的相对产物路径。
///
/// 缓存根先 canonicalize；相对路径仅接受非空 Normal 组件，不接受 `.`、`..`、
/// 绝对路径或 Windows ADS。缺失的中间目录按组件创建，已有链接目录一律拒绝。
/// Windows 固定根的全部祖先和产物父目录，临时文件按句柄发布/清理；Unix 使用
/// no-follow 目录 fd 与 *at 操作，不重新解析已经打开的父路径。最终 rename 替换
/// 目录项，不写入既有产物内容，不承诺断电持久化。调用方仅在返回 Ok 后记录 DB 成功。
///
/// Unix 的目录 fd 固定对象而非目录的命名空间位置；其它进程仍可移动已打开的目录。
/// 临时内容始终写入独占创建时持有的 fd；renameat 后以 O_NOFOLLOW 打开的目标 fd
/// 核对原临时文件的设备号/inode，不匹配或无法核验均返回错误。renameat 按名称操作，
/// 后验只能拒绝错对象，不是按源 fd 原子 rename，也不阻止同 uid 在核验后继续篡改缓存。
/// Unix 错误分支不 unlink 任何名称，可能留下唯一 tmp，须交后续受控清理；本模块不实现
/// 该清理流程。失败时目标也可能已被 rename 替换，不能据 Err 假定旧目标仍在。
pub(crate) fn write_cached_artifact(
    cache_dir: &Path,
    relative: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let parts = components(relative)?;
    let root = std::fs::canonicalize(cache_dir)?;
    let (name, parents) = parts.split_last().ok_or_else(denied)?;
    platform::Parent::open(&root, parents)?.write(name, bytes)
}

/// 删除一个已提交缓存文件；不跟随最终链接，不创建缺失父目录。
/// 根及已有父目录先规范化并核验边界，再按平台固定目录对象，避免路径替换越界。
pub(crate) fn remove_cached_artifact(cache_dir: &Path, relative: &str) -> io::Result<()> {
    let parts = components(relative)?;
    let (name, parents) = parts.split_last().ok_or_else(denied)?;
    let root = std::fs::canonicalize(cache_dir)?;
    let mut parent = root.clone();
    parent.extend(parents);
    let parent = std::fs::canonicalize(parent)?;
    if !parent.starts_with(&root) {
        return Err(denied());
    }
    platform::Parent::open(&parent, &[])?.remove(name)
}

/// 仅在库生命周期读区内写入；生命周期写者在删除或换址前调用 `reset`。
/// 根句柄不会交给排队任务，重用期间仍固定全部祖先，避免逐图重复授权。
#[derive(Default)]
pub(crate) struct ArtifactWriter {
    #[cfg(windows)]
    root: std::sync::Mutex<Option<CachedRoot>>,
}

#[cfg(windows)]
struct CachedRoot {
    configured: std::path::PathBuf,
    directory: std::sync::Arc<platform::Parent>,
}

impl ArtifactWriter {
    pub(crate) fn write(&self, cache_dir: &Path, relative: &str, bytes: &[u8]) -> io::Result<()> {
        #[cfg(windows)]
        {
            let parts = components(relative)?;
            let (name, parents) = parts.split_last().ok_or_else(denied)?;
            let root = {
                let mut cached = self.root.lock().unwrap_or_else(|e| e.into_inner());
                if cached
                    .as_ref()
                    .is_none_or(|root| root.configured != cache_dir)
                {
                    let canonical = std::fs::canonicalize(cache_dir)?;
                    *cached = Some(CachedRoot {
                        configured: cache_dir.to_path_buf(),
                        directory: std::sync::Arc::new(platform::Parent::open(&canonical, &[])?),
                    });
                }
                std::sync::Arc::clone(&cached.as_ref().expect("authorized root").directory)
            };
            // 根只借用到此次写入结束；慢 IO 不持根缓存的互斥锁。
            root.write_relative(parents, name, bytes)
        }
        #[cfg(not(windows))]
        write_cached_artifact(cache_dir, relative, bytes)
    }

    pub(crate) fn reset(&self) {
        #[cfg(windows)]
        self.root.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::OsString;
    use std::fs::{File, OpenOptions};
    use std::io::Write as _;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
    use std::path::PathBuf;
    use windows::Win32::Foundation::{
        RtlNtStatusToDosError, BOOLEAN, HANDLE, NTSTATUS, UNICODE_STRING,
    };
    use windows::Win32::Storage::FileSystem::{
        FileDispositionInfo, GetFinalPathNameByHandleW, SetFileInformationByHandle, DELETE,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
        VOLUME_NAME_DOS,
    };

    // Win32 的 SetFileInformationByHandle 在当前 Windows 上拒绝非空 RootDirectory。
    // 原生调用可以相对已固定父目录打开/rename，父路径不再被解析。
    // 只绑定这两个入口，复用 windows 的句柄/状态/rename 结构，不新增 Cargo 特性。
    #[link(name = "ntdll")]
    extern "system" {
        fn NtCreateFile(
            file: *mut HANDLE,
            access: u32,
            attributes: *const ObjectAttributes,
            io_status: *mut usize,
            allocation_size: *const i64,
            file_attributes: u32,
            sharing: u32,
            disposition: u32,
            options: u32,
            ea: *const std::ffi::c_void,
            ea_length: u32,
        ) -> NTSTATUS;
        fn NtSetInformationFile(
            file: HANDLE,
            io_status: *mut usize,
            information: *const std::ffi::c_void,
            length: u32,
            class: i32,
        ) -> NTSTATUS;
    }

    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root: HANDLE,
        name: *const UNICODE_STRING,
        attributes: u32,
        security_descriptor: *const std::ffi::c_void,
        security_qos: *const std::ffi::c_void,
    }

    fn nt_result(status: NTSTATUS) -> io::Result<()> {
        if status.0 < 0 {
            // SAFETY: 将 NTSTATUS 转成 io::Error 所需的 Win32 错误码。
            Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ))
        } else {
            Ok(())
        }
    }

    fn open_child(
        parent: &File,
        name: &std::ffi::OsStr,
        directory: bool,
        create: bool,
    ) -> io::Result<File> {
        let mut wide: Vec<u16> = name.encode_wide().collect();
        let length = u16::try_from(wide.len() * 2).map_err(|_| denied())?;
        let name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: windows::core::PWSTR(wide.as_mut_ptr()),
        };
        let attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: HANDLE(parent.as_raw_handle()),
            name: &name,
            attributes: 0x40, // OBJ_CASE_INSENSITIVE，与 Win32 路径契约一致。
            security_descriptor: std::ptr::null(),
            security_qos: std::ptr::null(),
        };
        let mut handle = HANDLE::default();
        let mut io_status = [0usize; 2];
        let (access, sharing, disposition, kind) = if directory {
            (
                FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0,
                FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0,
                if create { 3 } else { 1 },
                1,
            )
        } else {
            (FILE_GENERIC_WRITE.0 | DELETE.0, 0, 2, 0x40)
        };
        // SAFETY: 名称是单个组件，缓冲区和父句柄在调用期间有效；
        // FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT 保证 no-follow 和同步返回。
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                io_status.as_mut_ptr(),
                std::ptr::null(),
                0,
                sharing,
                disposition,
                kind | 0x20 | 0x0020_0000,
                std::ptr::null(),
                0,
            )
        };
        nt_result(status)?;
        // SAFETY: 成功返回的新句柄唯一移交 File。
        Ok(unsafe { File::from_raw_handle(handle.0) })
    }

    pub(super) struct Parent {
        path: PathBuf,
        // 持有所有祖先；仅锁目标父目录无法阻止更上层目录被重命名/替换。
        _directories: Vec<File>,
    }

    fn actual_path(file: &File) -> io::Result<PathBuf> {
        let handle = HANDLE(file.as_raw_handle());
        // SAFETY: File 持有句柄；第一次仅查询缓冲区长度。
        let needed = unsafe { GetFinalPathNameByHandleW(handle, &mut [], VOLUME_NAME_DOS) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0; needed as usize];
        // SAFETY: 缓冲区具有 API 要求的容量，句柄仍有效。
        let length = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_DOS) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length as usize >= buffer.len() {
            return Err(denied());
        }
        Ok(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            // 单独 FILE_READ_ATTRIBUTES 不参与共享访问检查，不能阻止目录重命名。
            .access_mode(FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0)
            // 排除 DELETE 共享固定目录名；子项打开/rename 使用句柄，不跟随父路径。
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        validate_directory(&file, path)?;
        Ok(file)
    }

    fn validate_directory(file: &File, path: &Path) -> io::Result<()> {
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || actual_path(file)? != path
        {
            return Err(denied());
        }
        Ok(())
    }

    impl Parent {
        pub(super) fn write_relative(
            &self,
            parents: &[&str],
            name: &str,
            bytes: &[u8],
        ) -> io::Result<()> {
            if parents.is_empty() {
                return self.write(name, bytes);
            }
            let mut path = self.path.clone();
            let mut directories = Vec::with_capacity(parents.len());
            for part in parents {
                path.push(part);
                let directory = open_child(
                    directories
                        .last()
                        .or_else(|| self._directories.last())
                        .ok_or_else(denied)?,
                    std::ffi::OsStr::new(part),
                    true,
                    true,
                )?;
                validate_directory(&directory, &path)?;
                directories.push(directory);
            }
            // self 的根链仍被调用栈持有，子目录句柄只保留到这一张图发布结束。
            Self {
                path,
                _directories: directories,
            }
            .write(name, bytes)
        }

        pub(super) fn open(root: &Path, parents: &[&str]) -> io::Result<Self> {
            let mut directories = Vec::new();
            for ancestor in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
                let directory = match directories.last() {
                    None => open_directory(ancestor)?,
                    Some(parent) => open_child(
                        parent,
                        ancestor.file_name().ok_or_else(denied)?,
                        true,
                        false,
                    )?,
                };
                validate_directory(&directory, ancestor)?;
                directories.push(directory);
            }
            let mut path = root.to_path_buf();
            for part in parents {
                path.push(part);
                let directory = open_child(
                    directories.last().ok_or_else(denied)?,
                    std::ffi::OsStr::new(part),
                    true,
                    true,
                )?;
                validate_directory(&directory, &path)?;
                directories.push(directory);
            }
            Ok(Self {
                path,
                _directories: directories,
            })
        }

        pub(super) fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
            let parent = self._directories.last().ok_or_else(denied)?;
            validate_directory(parent, &self.path)?;
            let mut temporary = Temporary::create(parent)?;
            temporary.file.write_all(bytes)?;
            temporary.publish(self._directories.last().ok_or_else(denied)?, name)
        }

        pub(super) fn remove(&self, name: &str) -> io::Result<()> {
            // 全部祖先仍禁止共享删除；remove_file 删除最终目录项，不跟随文件链接。
            std::fs::remove_file(self.path.join(name))
        }
    }

    struct Temporary {
        file: File,
        published: bool,
    }

    impl Temporary {
        fn create(parent: &File) -> io::Result<Self> {
            loop {
                match open_child(parent, std::ffi::OsStr::new(&temporary_name()), false, true) {
                    Ok(file) => {
                        return Ok(Self {
                            file,
                            published: false,
                        })
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
        }

        fn publish(&mut self, parent: &File, target: &str) -> io::Result<()> {
            let name: Vec<u16> = target.encode_utf16().collect();
            let name_bytes = name.len() * std::mem::size_of::<u16>();
            let size = std::mem::size_of::<FILE_RENAME_INFO>() + name_bytes;
            let size_u32 = u32::try_from(size).map_err(|_| denied())?;
            // usize 缓冲区保证 FILE_RENAME_INFO 的指针对齐，并为尾部变长名称留足空间。
            let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // IO_STATUS_BLOCK 为一个 Status/Pointer 联合体和一个 ULONG_PTR，均按指针对齐。
            let mut io_status = [0usize; 2];
            // SAFETY: 对齐缓冲区包含完整结构及名称；FileNameLength 是字节数，
            // 最终路径的全部祖先仍被 Parent 持有；源句柄未允许共享删除，不能被偷换。
            let result = unsafe {
                (*info).Anonymous.ReplaceIfExists = BOOLEAN(1);
                (*info).RootDirectory = HANDLE(parent.as_raw_handle());
                (*info).FileNameLength = name_bytes as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    name.len(),
                );
                NtSetInformationFile(
                    HANDLE(self.file.as_raw_handle()),
                    io_status.as_mut_ptr(),
                    info.cast(),
                    size_u32,
                    10, // FileRenameInformation；结构布局与 FILE_RENAME_INFO 一致。
                )
            };
            nt_result(result)?;
            self.published = true;
            Ok(())
        }
    }

    impl Drop for Temporary {
        fn drop(&mut self) {
            if !self.published {
                let info = FILE_DISPOSITION_INFO {
                    DeleteFile: BOOLEAN(1),
                };
                // SAFETY: 仅删除本次 create_new 获得的对象，不按名称清理别人的文件。
                // 删除失败时宁可留下自己的 tmp，也不改用有替换窗口的路径删除。
                let _ = unsafe {
                    SetFileInformationByHandle(
                        HANDLE(self.file.as_raw_handle()),
                        FileDispositionInfo,
                        std::ptr::from_ref(&info).cast(),
                        std::mem::size_of_val(&info) as u32,
                    )
                };
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::fs::File;
    use std::io::Write as _;
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;

    pub(super) struct Parent(File);

    fn c_name(name: &[u8]) -> io::Result<CString> {
        CString::new(name).map_err(|_| denied())
    }

    fn open_directory(parent: &File, name: &CStr) -> io::Result<File> {
        // SAFETY: parent 持有有效目录 fd，name 为 NUL 结尾；不跟随最终链接。
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: 新 fd 的唯一所有权移交 File。
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    impl Parent {
        pub(super) fn open(root: &Path, parents: &[&str]) -> io::Result<Self> {
            let mut directory = File::open("/")?;
            // canonicalize 后仍逐层 no-follow 打开，避免根的祖先被换成链接。
            for part in root.components() {
                match part {
                    Component::RootDir => (),
                    Component::Normal(name) => {
                        directory = open_directory(&directory, &c_name(name.as_bytes())?)?
                    }
                    _ => return Err(denied()),
                }
            }
            for part in parents {
                let name = c_name(part.as_bytes())?;
                directory = match open_directory(&directory, &name) {
                    Ok(child) => child,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        // SAFETY: 新目录相对固定父 fd 创建，名称已校验为单个 Normal 组件。
                        if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } < 0
                        {
                            let error = io::Error::last_os_error();
                            if error.kind() != io::ErrorKind::AlreadyExists {
                                return Err(error);
                            }
                        }
                        open_directory(&directory, &name)?
                    }
                    Err(error) => return Err(error),
                };
            }
            Ok(Self(directory))
        }

        pub(super) fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
            let target = c_name(name.as_bytes())?;
            let mut temporary = Temporary::create(&self.0)?;
            temporary.file.write_all(bytes)?;
            temporary.publish(&target)
        }

        pub(super) fn remove(&self, name: &str) -> io::Result<()> {
            let name = c_name(name.as_bytes())?;
            // SAFETY: 单个已验证名称相对固定父 fd 删除，不跟随最终链接或被替换的父路径。
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    struct Temporary<'a> {
        parent: &'a File,
        name: CString,
        file: File,
    }

    impl<'a> Temporary<'a> {
        fn create(parent: &'a File) -> io::Result<Self> {
            loop {
                let name = c_name(temporary_name().as_bytes())?;
                // SAFETY: O_EXCL 拒绝既有目录项（含链接），只创建本次独占的新 tmp。
                let fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_WRONLY
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                };
                if fd >= 0 {
                    // SAFETY: 新 fd 的唯一所有权移交 File。
                    return Ok(Self {
                        parent,
                        name,
                        file: unsafe { File::from_raw_fd(fd) },
                    });
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }

        fn publish(&self, target: &CStr) -> io::Result<()> {
            // SAFETY: 两端都相对同一个固定目录 fd；renameat 移动目录项，不跟随源/目标链接。
            // 名称可能已被同权限进程换掉，因此 rename 成功还不能向调用方报告成功。
            if unsafe {
                libc::renameat(
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                    self.parent.as_raw_fd(),
                    target.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: 仍从固定父 fd 打开单个目标名，NOFOLLOW 拒绝替换链接；
            // NONBLOCK 避免攻击者放入 FIFO 后让核验一直等待，打开后只取 fd 元数据。
            let fd = unsafe {
                libc::openat(
                    self.parent.as_raw_fd(),
                    target.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: 新 fd 的唯一所有权移交 File；核验失败也只关闭句柄，不删除目标。
            let published = unsafe { File::from_raw_fd(fd) };
            let actual = published.metadata()?;
            let original = self.file.metadata()?;
            if !actual.is_file() || actual.dev() != original.dev() || actual.ino() != original.ino()
            {
                return Err(denied());
            }
            Ok(())
        }
    }

    // 不实现按名称删除的 Drop：核对 inode 后再 unlinkat 仍能误删刚被替换的条目。
    // 出错只关闭持有的 fd，唯一 tmp 留给独立的受控清理流程。

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::io::Write as _;
        use std::os::unix::ffi::OsStrExt as _;

        #[test]
        fn replaced_tmp_symlink_cannot_redirect_write_or_report_success() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let original = outside.path().join("original");
            fs::write(&original, b"outside").unwrap();
            let parent = Parent::open(&fs::canonicalize(root.path()).unwrap(), &[]).unwrap();
            let mut temporary = Temporary::create(&parent.0).unwrap();
            let name = root
                .path()
                .join(std::ffi::OsStr::from_bytes(temporary.name.to_bytes()));
            let held = root.path().join("held");
            fs::rename(&name, &held).unwrap();
            std::os::unix::fs::symlink(&original, &name).unwrap();

            // 名称被换成外部链接后，写入仍落在 create 时持有的 inode。
            temporary.file.write_all(b"ours").unwrap();
            assert!(temporary.publish(&c_name(b"artifact").unwrap()).is_err());
            drop(temporary);
            assert_eq!(fs::read(&held).unwrap(), b"ours");
            assert_eq!(fs::read(&original).unwrap(), b"outside");
            // rename 已移动了链接；核验失败不能为了回滚而删除它。
            assert_eq!(
                fs::read_link(root.path().join("artifact")).unwrap(),
                original
            );
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
        }

        #[test]
        fn replaced_tmp_regular_file_fails_inode_check_without_deleting_target() {
            let root = tempfile::tempdir().unwrap();
            let parent = Parent::open(&fs::canonicalize(root.path()).unwrap(), &[]).unwrap();
            let mut temporary = Temporary::create(&parent.0).unwrap();
            let name = root
                .path()
                .join(std::ffi::OsStr::from_bytes(temporary.name.to_bytes()));
            let held = root.path().join("held");
            fs::rename(&name, &held).unwrap();
            fs::write(&name, b"other").unwrap();
            temporary.file.write_all(b"ours").unwrap();

            let error = temporary
                .publish(&c_name(b"artifact").unwrap())
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            drop(temporary);
            assert_eq!(fs::read(&held).unwrap(), b"ours");
            assert_eq!(fs::read(root.path().join("artifact")).unwrap(), b"other");
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    use super::*;
    pub(super) struct Parent;
    impl Parent {
        pub(super) fn open(_: &Path, _: &[&str]) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "artifact IO unsupported",
            ))
        }
        pub(super) fn write(&self, _: &str, _: &[u8]) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "artifact IO unsupported",
            ))
        }

        pub(super) fn remove(&self, _: &str) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "artifact IO unsupported",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn artifact_removal_rejects_traversal_and_external_parent_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        directory_link(outside.path(), &root.path().join("linked"));
        assert!(remove_cached_artifact(root.path(), "linked/keep").is_err());
        assert!(remove_cached_artifact(root.path(), "../keep").is_err());
        assert!(remove_cached_artifact(
            root.path(),
            &outside.path().join("keep").to_string_lossy()
        )
        .is_err());
        // 硬链接删除只移除缓存目录项，不改写或删除源路径。
        fs::hard_link(outside.path().join("keep"), root.path().join("hard")).unwrap();
        remove_cached_artifact(root.path(), "hard").unwrap();
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    }

    fn directory_link(target: &Path, link: &Path) {
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn rejects_link_swapped_in_after_canonicalize() {
        let sandbox = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("cache");
        fs::create_dir(&root).unwrap();
        let canonical = fs::canonicalize(&root).unwrap();
        // 两个路径都属于本用例的临时沙箱，固定名称；不操作用户目录。
        let moved = sandbox.path().join("moved");
        assert!(root.starts_with(sandbox.path()) && moved.starts_with(sandbox.path()));
        fs::rename(&root, &moved).unwrap();
        directory_link(outside.path(), &root);
        assert!(platform::Parent::open(&canonical, &[]).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn hard_link_target_is_replaced_without_modifying_original() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let original = outside.path().join("original");
        fs::write(&original, b"outside").unwrap();
        fs::hard_link(&original, root.path().join("artifact")).unwrap();
        write_cached_artifact(root.path(), "artifact", b"new").unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"outside");
        assert_eq!(fs::read(root.path().join("artifact")).unwrap(), b"new");
    }

    #[test]
    fn failed_publish_preserves_other_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("artifact")).unwrap();
        fs::write(root.path().join("artifact/keep"), b"keep").unwrap();
        fs::write(root.path().join("other.tmp"), b"other").unwrap();
        assert!(write_cached_artifact(root.path(), "artifact", b"bad").is_err());
        assert_eq!(
            fs::read(root.path().join("artifact/keep")).unwrap(),
            b"keep"
        );
        assert_eq!(fs::read(root.path().join("other.tmp")).unwrap(), b"other");
        #[cfg(windows)]
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        #[cfg(unix)]
        {
            let entries: Vec<_> = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(entries.len(), 3);
            let temporary = entries
                .iter()
                .find(|path| {
                    let name = path.file_name().unwrap().to_str().unwrap();
                    name.starts_with(".artifact-") && name.ends_with(".tmp")
                })
                .unwrap();
            assert_eq!(fs::read(temporary).unwrap(), b"bad");
        }
    }

    #[cfg(unix)]
    #[test]
    fn parent_swap_does_not_redirect_open_fd() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent =
            platform::Parent::open(&fs::canonicalize(root.path()).unwrap(), &["sub"]).unwrap();
        fs::rename(root.path().join("sub"), root.path().join("moved")).unwrap();
        directory_link(outside.path(), &root.path().join("sub"));
        parent.write("artifact", b"inside").unwrap();
        assert_eq!(
            fs::read(root.path().join("moved/artifact")).unwrap(),
            b"inside"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
