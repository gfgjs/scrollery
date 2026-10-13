//! 导出源的授权检查：返回已固定对象的句柄，调用方不得再次按路径打开。

use std::fs::File;
use std::io;
#[cfg(test)]
use std::path::Path;

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "export source outside scan root",
    )
}

/// 校验数据库中的相对目录和单个文件名，并打开扫描根内的普通文件。
pub(crate) fn open(root: &str, relative_dir: &str, file_name: &str) -> io::Result<File> {
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

#[path = "../../../crates/shared/authorized_file.rs"]
#[allow(dead_code)] // 严格缓存入口由 AI worker 使用；导出保持既有允许根内链接的入口。
mod authorized_file;
use authorized_file::open_resolved;

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
