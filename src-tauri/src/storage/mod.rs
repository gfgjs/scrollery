// src-tauri/src/storage/mod.rs
//! Storage backend abstraction (P5 8B, §1.4.1 / §3.8) — the capability-trait layer for walking,
//! stat-ing and ranged-reading media from local disk OR a remote share, behind one interface.
//!
//! 存储后端抽象（P5 8B，§1.4.1 / §3.8）—— 在统一接口下「遍历 / stat / 按范围读取」本地磁盘或远程
//! 共享的能力 trait 层。与视频 `VideoBackend`（§3.2）同构。
//!
//! 变体边界（§1.1）：`LocalFs` 纯 `std::fs`，两变体都在 —— **8A（OS 挂载盘 / UNC）即用它**
//! （`backend_id IS NULL` 的扫描根直接走本地路径，网络盘靠 OS 映射/UNC，见 `normalize_root_path`）。
//! `WebDavBackend`（原生 WebDAV，无需 OS 挂载）依赖 `reqwest_dav`，仅 `netfs`(perf) 编入。
//!
//! 现状（与 D3「8B 后置」一致）：本层 + `LocalFs` + `WebDavBackend` 的连接/列目录/读取/连通性测试
//! 均已落地，IPC 可注册并**测试** WebDAV 连接。**仍待办**：把 scanner 改为走本 trait 遍历远程，
//! 以及自定义 Tauri URI 协议的流式代理（Range 边下边播）—— 属大改造，后续阶段补。

pub mod local;
#[cfg(feature = "netfs")]
pub mod webdav;

use crate::error::{AppError, Result};

/// One directory entry from a storage backend (local or remote). Paths are backend-relative,
/// forward-slash, relative to the backend's `base_path`.
/// 来自存储后端（本地或远程）的一条目录项。路径为后端相对、正斜杠、相对其 `base_path`。
#[derive(Debug, Clone)]
pub struct RemoteEntry {
    /// Last path component (file or directory name). | 末段路径（文件或目录名）。
    pub name: String,
    /// Path relative to the backend base (forward-slash). | 相对后端 base 的路径（正斜杠）。
    pub rel_path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Modification time, unix seconds (0 if unknown). | 修改时间，unix 秒（未知为 0）。
    pub mtime: i64,
}

/// Connection parameters for building a backend. Password is passed in-memory only (sourced from
/// keyring at the IPC layer) — never persisted by this module.
/// 构建后端的连接参数。密码仅在内存传递（由 IPC 层从 keyring 取）—— 本模块绝不持久化。
#[derive(Debug, Clone, Default)]
pub struct BackendConfig {
    pub kind: String, // 'local' | 'webdav' | 'smb'
    pub host: Option<String>,
    pub base_path: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Storage capability backend (§1.4.1 / §3.8). Sync interface so the (future) scanner can walk a
/// remote share exactly like the local FS; the WebDAV impl bridges async `reqwest_dav` internally.
/// 存储能力后端（§1.4.1 / §3.8）。同步接口，使（未来的）scanner 能像本地 FS 一样遍历远程共享；
/// WebDAV 实现内部桥接异步 `reqwest_dav`。
pub trait StorageBackend: Send + Sync {
    /// Stable backend id: "local" | "webdav" | "smb". | 稳定后端 id。
    fn kind(&self) -> &'static str;

    /// List entries directly under `rel_path` (one level; `""` = base). | 列出 `rel_path` 下一层项（`""`=base）。
    fn list_dir(&self, rel_path: &str) -> Result<Vec<RemoteEntry>>;

    /// Stat a single path. | stat 单个路径。
    fn stat(&self, rel_path: &str) -> Result<RemoteEntry>;

    /// Read a byte range `[start, start+len)` of a file (`len=None` → to EOF). Backs the streaming
    /// proxy for remote originals (§3.8). | 读取文件字节范围（左闭右开区间 `[start, start+len)`，
    /// `len=None` → 至文件尾）。支撑远程原图流式代理。
    fn read_range(&self, rel_path: &str, start: u64, len: Option<u64>) -> Result<Vec<u8>>;

    /// 连通性 + 凭据检查（默认：尝试列出 base 目录）。
    fn test(&self) -> Result<()> {
        self.list_dir("").map(|_| ())
    }
}

/// Build a backend from a config (§1.4.3 runtime selection). `local`/`smb` → `LocalFs`
/// (SMB stays on the OS mount per D3); `webdav` → native client (feature `netfs`), else a clear
/// "needs perf variant" error so Lite degrades gracefully to 8A (OS-mounted drives).
/// 按配置构建后端（§1.4.3 运行期选择）。`local`/`smb` → `LocalFs`（SMB 依 D3 走 OS 挂载）；
/// `webdav` → 原生客户端（feature `netfs`），否则返回清晰的「需性能版」错误，使 Lite 优雅降级到
/// 8A（OS 映射盘）。
pub fn build_backend(cfg: &BackendConfig) -> Result<Box<dyn StorageBackend>> {
    match cfg.kind.as_str() {
        // SMB 继续依赖 OS 挂载（UNC 路径即 base_path），故等同本地（§3.8 / D3）。
        "local" | "smb" => Ok(Box::new(local::LocalFs::new(
            cfg.base_path.clone().unwrap_or_default(),
        ))),
        "webdav" => {
            #[cfg(feature = "netfs")]
            {
                Ok(Box::new(webdav::WebDavBackend::new(cfg)?))
            }
            #[cfg(not(feature = "netfs"))]
            {
                Err(AppError::UnsupportedFormat(
                    "原生 WebDAV 需性能版（netfs feature）；轻量版请用 OS 映射盘 / UNC（8A）\
                     | native WebDAV requires the perf variant (netfs); Lite uses an OS-mapped drive (8A)"
                        .into(),
                ))
            }
        }
        other => Err(AppError::UnsupportedFormat(format!(
            "unknown storage kind '{other}' | 未知存储后端类型"
        ))),
    }
}
