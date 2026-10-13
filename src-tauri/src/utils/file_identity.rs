//! 扫描只读取物理身份，不读取文件内容；文件和目录使用同一平台身份编码。

use std::fs::OpenOptions;
use std::path::Path;

use crate::dedup::hash::{physical_identity_for_file, physical_key, reject_link_components};

/// 读取授权根内普通文件/目录的身份；不支持身份的平台返回 `None`。
pub(crate) fn read_physical_key(root: &Path, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let canonical_root = dunce::canonicalize(root)?;
    let canonical_path = dunce::canonicalize(path)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    }
    reject_link_components(path)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // 零访问权限足够查询身份；目录使用 BACKUP_SEMANTICS，重解析点不跟随。
        options
            .access_mode(0)
            .custom_flags(0x0200_0000 | 0x0020_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.read(true).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(any(unix, windows)))]
    options.read(true);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    reject_link_components(path)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    Ok(physical_identity_for_file(&file, &metadata)
        .ok()
        .map(physical_key))
}
