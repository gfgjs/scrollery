// src-tauri/src/storage/webdav.rs
//! Native WebDAV `StorageBackend` (P5 8B, feature `netfs`) via `reqwest_dav` (pure Rust, rustls).
//! No OS mount required — connect/list/stat/ranged-read straight over HTTP(S).
//!
//! 原生 WebDAV `StorageBackend`（P5 8B，feature `netfs`），基于 `reqwest_dav`（纯 Rust，rustls）。
//! 无需 OS 挂载 —— 直接经 HTTP(S) 连接/列目录/stat/按范围读取。
//!
//! 同步 trait 桥接：本结构持有一个 current-thread tokio 运行时，`block_on` 异步 `reqwest_dav`，
//! 使 WebDAV 能像 `LocalFs` 一样被同步的 scanner 使用。**注意**：其方法必须在无环境运行时的线程
//! 调用（即 IPC 层用 `spawn_blocking`），否则 `block_on` 会因「运行时套运行时」而 panic。

use reqwest_dav::re_exports::reqwest;
use reqwest_dav::types::list_cmd::ListEntity;
use reqwest_dav::{Auth, Client, ClientBuilder, Depth};
use tokio::runtime::Runtime;

use crate::error::{AppError, Result};
use crate::storage::{BackendConfig, RemoteEntry, StorageBackend};

/// A `StorageBackend` over a remote WebDAV server (e.g. Nextcloud, Apache mod_dav).
/// 远程 WebDAV 服务器（如 Nextcloud、Apache mod_dav）的 `StorageBackend`。
pub struct WebDavBackend {
    rt: Runtime,
    client: Client,
    /// Base path under the host (forward-slash, no leading/trailing slash). | host 下的 base 路径。
    base: String,
}

fn map_err(e: reqwest_dav::Error) -> AppError {
    AppError::internal("WebDAV 操作失败 | WebDAV operation failed", e)
}

impl WebDavBackend {
    pub fn new(cfg: &BackendConfig) -> Result<Self> {
        let host = cfg
            .host
            .clone()
            .filter(|h| !h.trim().is_empty())
            .ok_or_else(|| {
                AppError::System("WebDAV host/base_url missing | 缺少 WebDAV 地址".into())
            })?;
        let host = host.trim().trim_end_matches('/').to_string();
        if !(host.starts_with("http://") || host.starts_with("https://")) {
            return Err(AppError::System(
                "WebDAV base_url must be http(s) | WebDAV 地址须为 http(s)".into(),
            ));
        }

        // current-thread 运行时，使 block_on 在 spawn_blocking 内可用（无环境运行时）。
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(AppError::from)?;

        let auth = match (&cfg.username, &cfg.password) {
            (Some(u), Some(p)) if !u.is_empty() => Auth::Basic(u.clone(), p.clone()),
            _ => Auth::Anonymous,
        };

        let client = ClientBuilder::new()
            .set_host(host)
            .set_auth(auth)
            .build()
            .map_err(map_err)?;

        let base = cfg
            .base_path
            .clone()
            .unwrap_or_default()
            .trim_matches('/')
            .to_string();

        Ok(Self { rt, client, base })
    }

    /// 把 base + 后端相对路径拼为服务器路径（前导斜杠、规范化）。
    fn server_path(&self, rel_path: &str) -> String {
        let rel = rel_path.trim_matches('/');
        let joined = match (self.base.is_empty(), rel.is_empty()) {
            (true, true) => String::new(),
            (true, false) => rel.to_string(),
            (false, true) => self.base.clone(),
            (false, false) => format!("{}/{}", self.base, rel),
        };
        format!("/{joined}")
    }
}

/// 由 WebDAV `href` 与 base 前缀推导项名 + 后端相对路径。
fn rel_from_href(href: &str, base: &str) -> (String, String) {
    // href 为服务器绝对路径且 URL 编码；解码百分号转义以便显示/拼接。
    let decoded = percent_decode(href);
    let trimmed = decoded.trim_matches('/');
    // 剥掉 base 前缀得到后端相对路径。
    let rel = trimmed
        .strip_prefix(base)
        .unwrap_or(trimmed)
        .trim_matches('/')
        .to_string();
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    (name, rel)
}

/// WebDAV href 的最小百分号解码（空格、中文等）。纯函数，无额外依赖。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl StorageBackend for WebDavBackend {
    fn kind(&self) -> &'static str {
        "webdav"
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<RemoteEntry>> {
        let path = self.server_path(rel_path);
        let base = self.base.clone();
        let entities = self
            .rt
            .block_on(self.client.list(&path, Depth::Number(1)))
            .map_err(map_err)?;

        // 首项通常是被列目录自身 —— 跳过等于请求路径的项。
        let self_rel = path
            .trim_matches('/')
            .strip_prefix(base.as_str())
            .unwrap_or("")
            .trim_matches('/')
            .to_string();
        let mut out = Vec::new();
        for ent in entities {
            let (name, rel, is_dir, size, mtime) = match ent {
                ListEntity::File(f) => {
                    let (n, r) = rel_from_href(&f.href, &base);
                    (
                        n,
                        r,
                        false,
                        f.content_length.max(0) as u64,
                        f.last_modified.timestamp(),
                    )
                }
                ListEntity::Folder(d) => {
                    let (n, r) = rel_from_href(&d.href, &base);
                    (n, r, true, 0u64, d.last_modified.timestamp())
                }
            };
            if rel == self_rel || name.is_empty() {
                continue;
            }
            out.push(RemoteEntry {
                name,
                rel_path: rel,
                is_dir,
                size,
                mtime,
            });
        }
        Ok(out)
    }

    fn stat(&self, rel_path: &str) -> Result<RemoteEntry> {
        let path = self.server_path(rel_path);
        let base = self.base.clone();
        let entities = self
            .rt
            .block_on(self.client.list(&path, Depth::Number(0)))
            .map_err(map_err)?;
        let ent = entities.into_iter().next().ok_or_else(|| {
            AppError::System(format!("WebDAV stat not found: {rel_path} | 未找到"))
        })?;
        Ok(match ent {
            ListEntity::File(f) => {
                let (name, rel) = rel_from_href(&f.href, &base);
                RemoteEntry {
                    name,
                    rel_path: rel,
                    is_dir: false,
                    size: f.content_length.max(0) as u64,
                    mtime: f.last_modified.timestamp(),
                }
            }
            ListEntity::Folder(d) => {
                let (name, rel) = rel_from_href(&d.href, &base);
                RemoteEntry {
                    name,
                    rel_path: rel,
                    is_dir: true,
                    size: 0,
                    mtime: d.last_modified.timestamp(),
                }
            }
        })
    }

    fn read_range(&self, rel_path: &str, start: u64, len: Option<u64>) -> Result<Vec<u8>> {
        let path = self.server_path(rel_path);
        self.rt.block_on(async {
            let mut req = self
                .client
                .start_request(reqwest::Method::GET, &path)
                .await
                .map_err(map_err)?;
            // Range 头用于部分读取（流式代理 / 远程大原图，§3.8）。
            if let Some(n) = len {
                let end = start + n.saturating_sub(1);
                req = req.header("Range", format!("bytes={start}-{end}"));
            } else if start > 0 {
                req = req.header("Range", format!("bytes={start}-"));
            }
            let resp = req
                .send()
                .await
                .map_err(|e| AppError::internal("WebDAV 读取失败 | WebDAV GET failed", e))?;
            // 状态码校验(2026-07-06 审查 R8):不校验则 404/500 的错误体会被当成文件数据返回。
            let status = resp.status();
            if !status.is_success() {
                return Err(AppError::System(format!(
                    "WebDAV GET 非成功状态 {status} | non-success status"
                )));
            }
            // 带 Range 请求时,服务器应回 206 Partial Content;若回 200 说明它忽略了 Range、
            // 返回整个文件——调用方拿到的字节会远超请求窗口,宁可报错也不返错误数据。
            let ranged = len.is_some() || start > 0;
            if ranged && status != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(AppError::System(format!(
                    "WebDAV 忽略 Range(状态 {status},期望 206)| server ignored Range header"
                )));
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| AppError::internal("WebDAV 响应体读取失败 | body read failed", e))?;
            Ok(bytes.to_vec())
        })
    }
}
