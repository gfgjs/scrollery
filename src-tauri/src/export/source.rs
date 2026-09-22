//! 导出源的授权检查：返回已固定对象的句柄，调用方不得再次按路径打开。

use std::fs::File;
use std::io;
use std::path::Path;

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "export source outside scan root",
    )
}

/// 校验数据库中的相对目录和单个文件名，并打开扫描根内的普通文件。
pub(super) fn open(root: &str, relative_dir: &str, file_name: &str) -> io::Result<File> {
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains(['/', '\\'])
        || relative_dir.starts_with(['/', '\\'])
    {
        return Err(denied());
    }
    let root = std::fs::canonicalize(root)?;
    let mut path = root.clone();
    for part in relative_dir
        .split(['/', '\\'])
        .chain(std::iter::once(file_name))
    {
        if part == ".." {
            return Err(denied());
        }
        #[cfg(windows)]
        if part.contains(':') {
            return Err(denied());
        }
        if !part.is_empty() && part != "." {
            path.push(part);
        }
    }
    let path = std::fs::canonicalize(path)?;
    if !path.starts_with(&root) {
        return Err(denied());
    }
    let file = open_resolved(&root, &path)?;
    if !file.metadata()?.is_file() {
        return Err(denied());
    }
    Ok(file)
}

#[cfg(windows)]
fn open_resolved(root: &Path, path: &Path) -> io::Result<File> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFinalPathNameByHandleW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        VOLUME_NAME_DOS,
    };

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(denied());
    }
    // 最后一个组件之外的父目录也可能被换成 junction；读取句柄最终路径后才允许读内容。
    let handle = HANDLE(file.as_raw_handle());
    // SAFETY: 句柄由 file 持有；空切片仅查询所需长度，未读取文件内容。
    let needed = unsafe { GetFinalPathNameByHandleW(handle, &mut [], VOLUME_NAME_DOS) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0; needed as usize];
    // SAFETY: 有效句柄及 API 返回长度分配的可写缓冲区。
    let length = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_DOS) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= buffer.len() {
        return Err(denied()); // 两次调用间发生重命名，不能使用截断路径授权。
    }
    let actual = std::path::PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    if !actual.starts_with(root) {
        return Err(denied());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_resolved(_root: &Path, path: &Path) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::Component;

    // canonicalize 已判根；从 / 的句柄逐层打开，任一组件被换成链接都会失败。
    // 持有父目录句柄后再 openat，避免父路径检查与下一次打开之间再次解析同一路径。
    let mut parent = File::open("/")?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            if component == Component::RootDir {
                continue;
            }
            return Err(denied());
        };
        let name = CString::new(name.as_bytes()).map_err(|_| denied())?;
        let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        if components.peek().is_some() {
            flags |= libc::O_DIRECTORY;
        }
        // SAFETY: parent 持有有效目录 fd，name 为 NUL 结尾字符串；不使用 O_CREAT。
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat 返回的新 fd 唯一移交 File 所有权。
        parent = unsafe { File::from_raw_fd(fd) };
    }
    Ok(parent)
}

#[cfg(not(any(unix, windows)))]
fn open_resolved(_root: &Path, _path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "export source handle validation unsupported",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

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
    fn opens_normal_source_and_rejects_invalid_components() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.jpg"), b"inside").unwrap();
        let root_text = root.path().to_string_lossy();
        let mut bytes = Vec::new();
        open(&root_text, "", "a.jpg")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"inside");
        assert!(open(&root_text, "..", "a.jpg").is_err());
        assert!(open(&root_text, "/", "a.jpg").is_err());
        assert!(open(&root_text, "", "../a.jpg").is_err());
        assert!(open(&root_text, "", &root.path().join("a.jpg").to_string_lossy()).is_err());
        #[cfg(windows)]
        assert!(open(&root_text, "", "a.jpg:stream").is_err());
        assert!(open(&root_text, "", ".").is_err());
    }

    #[test]
    fn allows_internal_link_and_rejects_external_directory_link() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sub = root.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.jpg"), b"inside").unwrap();
        std::fs::write(outside.path().join("a.jpg"), b"secret").unwrap();
        directory_link(&sub, &root.path().join("inside-link"));
        directory_link(outside.path(), &root.path().join("outside-link"));
        let root_text = root.path().to_string_lossy();
        assert!(open(&root_text, "inside-link", "a.jpg").is_ok());
        assert!(open(&root_text, "outside-link", "a.jpg").is_err());
    }

    #[test]
    fn rejects_parent_replacement_after_path_validation() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sub = root.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.jpg"), b"inside").unwrap();
        std::fs::write(outside.path().join("a.jpg"), b"secret").unwrap();
        let canonical_root = std::fs::canonicalize(root.path()).unwrap();
        let canonical_source = std::fs::canonicalize(sub.join("a.jpg")).unwrap();
        // 两个目标都由本用例新建的临时根及固定组件组成。
        let moved = root.path().join("original-sub");
        assert!(sub.starts_with(root.path()) && moved.starts_with(root.path()));
        std::fs::rename(&sub, &moved).unwrap();
        directory_link(outside.path(), &sub);
        assert!(open_resolved(&canonical_root, &canonical_source).is_err());
    }
}
