// src-tauri/src/storage/local.rs
//! Local-filesystem `StorageBackend` (pure `std::fs`, both variants). This is what 8A uses:
//! an OS-mounted network drive or UNC share is just a local path here (§3.8).
//! 本地文件系统 `StorageBackend`（纯 `std::fs`，两变体都含）。8A 即用它：OS 映射的网络盘 / UNC
//! 共享在此就是一条本地路径（§3.8）。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, Result};
use crate::storage::{RemoteEntry, StorageBackend};

/// 以本地（或 OS 挂载）目录为根的 `StorageBackend`。
pub struct LocalFs {
    base: PathBuf,
}

impl LocalFs {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// 把用户/远端传入的相对路径解析到 `base` 内(U-3 方案 A)。**不 canonicalize**——
    /// list/stat 等场景可能查询尚不存在的路径，canonicalize 会把它们一律误判为不存在。
    /// 改用逐组件过滤：`ParentDir`(`..`)/`RootDir`(前导 `/`)/`Prefix`(盘符 `C:` 或 UNC
    /// `\\server`)一律拒绝，只把 `Normal` 段逐个 push 到 `base`（同 `utils::path::
    /// resolve_within_root` 的「切段绕开 `PathBuf::join` 整体替换陷阱」姿态，但不落地判断需要
    /// 真实存在的 canonicalize 步）。join 完再做一次 `Path::starts_with`（同 `export::core::
    /// is_inside_library` 的组件级边界断言，非字符串前缀）兜底，防实现漂移。
    fn abs(&self, rel_path: &str) -> Result<PathBuf> {
        if rel_path.is_empty() {
            return Ok(self.base.clone());
        }
        let mut joined = self.base.clone();
        for comp in Path::new(rel_path).components() {
            match comp {
                Component::Normal(seg) => joined.push(seg),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(AppError::PathResolution(
                        "相对路径含越界穿越或非法前缀 | relative path escapes base or has an invalid prefix"
                            .into(),
                    ));
                }
            }
        }
        if !joined.starts_with(&self.base) {
            return Err(AppError::PathResolution(
                "相对路径越出基目录 | relative path escapes base directory".into(),
            ));
        }
        Ok(joined)
    }
}

fn entry_from(base: &Path, p: &Path, is_dir: bool, size: u64, mtime: i64) -> RemoteEntry {
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let rel_path = p
        .strip_prefix(base)
        .ok()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| name.clone());
    RemoteEntry {
        name,
        rel_path,
        is_dir,
        size,
        mtime,
    }
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl StorageBackend for LocalFs {
    fn kind(&self) -> &'static str {
        "local"
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<RemoteEntry>> {
        let dir = self.abs(rel_path)?;
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(AppError::from)? {
            let entry = entry.map_err(AppError::from)?;
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            out.push(entry_from(
                &self.base,
                &entry.path(),
                meta.is_dir(),
                meta.len(),
                mtime_secs(&meta),
            ));
        }
        Ok(out)
    }

    fn stat(&self, rel_path: &str) -> Result<RemoteEntry> {
        let p = self.abs(rel_path)?;
        let meta = std::fs::metadata(&p).map_err(AppError::from)?;
        Ok(entry_from(
            &self.base,
            &p,
            meta.is_dir(),
            meta.len(),
            mtime_secs(&meta),
        ))
    }

    fn read_range(&self, rel_path: &str, start: u64, len: Option<u64>) -> Result<Vec<u8>> {
        let mut f = std::fs::File::open(self.abs(rel_path)?).map_err(AppError::from)?;
        if start > 0 {
            f.seek(SeekFrom::Start(start)).map_err(AppError::from)?;
        }
        let mut buf = Vec::new();
        match len {
            Some(n) => {
                buf.resize(n as usize, 0);
                let read = read_full(&mut f, &mut buf)?;
                buf.truncate(read);
            }
            None => {
                f.read_to_end(&mut buf).map_err(AppError::from)?;
            }
        }
        Ok(buf)
    }
}

/// 读取至多 `buf.len()` 字节，容忍接近文件尾的短读。返回实际读取字节数。
fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(AppError::from(e)),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_fs(base: &str) -> LocalFs {
        LocalFs::new(PathBuf::from(base))
    }

    #[test]
    fn abs_accepts_normal_relative_path() {
        let f = mk_fs("C:\\lib");
        let got = f.abs("sub/inner.txt").unwrap();
        assert_eq!(got, PathBuf::from("C:\\lib").join("sub").join("inner.txt"));
    }

    #[test]
    fn abs_accepts_empty_path_as_base() {
        let f = mk_fs("C:\\lib");
        assert_eq!(f.abs("").unwrap(), PathBuf::from("C:\\lib"));
    }

    #[test]
    fn abs_rejects_leading_parent_traversal() {
        let f = mk_fs("C:\\lib");
        assert!(f.abs("../secret.txt").is_err());
    }

    #[test]
    fn abs_rejects_embedded_parent_traversal() {
        let f = mk_fs("C:\\lib");
        assert!(f.abs("sub/../../secret.txt").is_err());
        // 组件级过滤：即便 `..` 被后续段“抵消”回根内，也一律拒绝（不做穿越抵消判定）。
        assert!(f.abs("sub/../sub/inner.txt").is_err());
    }

    #[test]
    fn abs_rejects_pure_absolute_path() {
        let f = mk_fs("C:\\lib");
        assert!(f.abs("/etc/passwd").is_err());
    }

    #[test]
    fn abs_accepts_curdir_components() {
        let f = mk_fs("C:\\lib");
        let got = f.abs("./sub/inner.txt").unwrap();
        assert_eq!(got, PathBuf::from("C:\\lib").join("sub").join("inner.txt"));
    }

    // Linux 上 `\` 非路径分隔符,整串会被解析成单个 Normal 段而通过,断言假 panic,故仅 Windows 跑。
    #[cfg(windows)]
    #[test]
    fn abs_rejects_windows_drive_letter_injection() {
        let f = mk_fs("C:\\lib");
        assert!(f.abs("C:\\Windows\\System32").is_err());
    }

    // 同上:Linux 上 `\\server\...` 无 Prefix 语义,仅 Windows 跑。
    #[cfg(windows)]
    #[test]
    fn abs_rejects_unc_injection() {
        let f = mk_fs("C:\\lib");
        assert!(f.abs("\\\\server\\share\\file.txt").is_err());
    }

    #[test]
    fn abs_result_stays_inside_base_for_accepted_paths() {
        let f = mk_fs("C:\\lib");
        let got = f.abs("a/b/c.jpg").unwrap();
        assert!(got.starts_with("C:\\lib"));
    }
}
