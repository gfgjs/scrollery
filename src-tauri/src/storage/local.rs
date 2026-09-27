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
