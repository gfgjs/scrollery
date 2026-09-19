// src-tauri/src/storage/mod.rs
//! 存储后端**连接测试**（P5 8B，§1.4.1 / §3.8）：设置页在保存前验证一条网络盘配置是否可达、
//! 凭据是否有效，并返回 base 目录下的项数。
//!
//! 变体边界（§1.1）：`local`/`smb` 走纯 `std::fs`（OS 挂载盘 / UNC 即一条本地路径，8A 形态），
//! 两变体都含；`webdav`（原生 WebDAV，无需 OS 挂载）依赖 `reqwest_dav`，仅 `netfs`(perf) 编入，
//! 轻量版返回「需性能版」错误以便降级到 8A。
//!
//! 当前范围（2026-09-15 P15 消融后）：**只做连接测试**。远程目录遍历与流式 Range 代理尚未接入，
//! 扫描不经本模块、走本地路径（OS 映射盘 / UNC）；原先预留的 `RemoteEntry` DTO、`StorageBackend`
//! trait 与 `Box<dyn StorageBackend>` 工厂因无生产消费者已删除，届时按真实需求重新引入。

pub mod local;
#[cfg(feature = "netfs")]
pub mod webdav;

use crate::error::{AppError, Result};

/// 连接测试参数（借用 IPC 表单字段，避免 clone 透传）。密码仅在内存传递（由 IPC 层从 keyring 取）
/// —— 本模块绝不持久化。
#[derive(Debug)]
pub struct ConnParams<'a> {
    pub host: Option<&'a str>,
    pub base_path: Option<&'a str>,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
}

/// 连接测试（§1.4.3 运行期选择）：按 `kind` 验证可达性/凭据，返回 base 目录下的项数。
/// `webdav` 在轻量版（无 `netfs`）返回清晰的「需性能版」错误，使 Lite 优雅降级到 8A（OS 映射盘）。
pub fn count_entries(kind: &str, p: &ConnParams<'_>) -> Result<usize> {
    match kind {
        // SMB 继续依赖 OS 挂载（UNC 路径即 base_path），故等同本地（§3.8 / D3）。
        "local" | "smb" => local::count_entries(p.base_path.unwrap_or_default()),
        "webdav" => {
            #[cfg(feature = "netfs")]
            {
                webdav::count_entries(p)
            }
            #[cfg(not(feature = "netfs"))]
            {
                let _ = p;
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
