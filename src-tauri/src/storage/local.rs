// src-tauri/src/storage/local.rs
//! 本地 / OS 挂载盘的连接测试（纯 `std::fs`，两变体都含）。OS 映射的网络盘 / UNC 共享在这里就是
//! 一条本地路径（§3.8 8A）——扫描不经本模块，此处只服务设置页的连接测试。

use crate::error::{AppError, Result};

/// 连接可达性 + 凭据检查：列出 `base` 目录直属项并计数。
/// 路径按原样交给 OS（UNC / 映射盘同一条路径），错误即不可达。
pub fn count_entries(base: &str) -> Result<usize> {
    let mut n = 0;
    for entry in std::fs::read_dir(base).map_err(AppError::from)? {
        entry.map_err(AppError::from)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_str(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn counts_entries_in_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"y").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert_eq!(count_entries(&as_str(dir.path())).unwrap(), 3);
    }

    #[test]
    fn empty_directory_counts_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_entries(&as_str(dir.path())).unwrap(), 0);
    }

    #[test]
    fn trailing_separator_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let with_sep = format!("{}{}", as_str(dir.path()), std::path::MAIN_SEPARATOR);
        assert_eq!(count_entries(&with_sep).unwrap(), 1);
    }

    #[test]
    fn missing_path_is_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(count_entries(&as_str(&dir.path().join("nope"))).is_err());
    }

    #[test]
    fn file_path_is_error_not_zero() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(count_entries(&as_str(&f)).is_err());
    }

    #[test]
    fn empty_base_is_error() {
        assert!(count_entries("").is_err());
    }
}
