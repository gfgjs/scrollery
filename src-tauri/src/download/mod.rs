//! R10 通用下载引擎（Part6 §3.1.2）。
//!
//! 此前 exotic（`exotic/fetch.rs`）与 AI/face（今 `ipc/model_download.rs::download_assets`）各写一套下载逻辑：
//! sha256 循环、reqwest client 构建、`.part` 原子改名、size/sha 校验重复两份；且 exotic 缺 Range
//! 续传/镜像回退/进度，AI 缺 HTTPS 强制/超时加固。本模块把**可共享的机制原语**收敛到一处：
//! 安全 client（HTTPS 强制 + 重定向加固 + 分级超时）、单文件流式下载（Range 续传）、镜像回退、
//! sha256 校验。各调用方保留自己的**领域编排**（exotic 的包校验、AI 的多资产清单/进度聚合），
//! 仅机制下沉——延续「找真正解耦的单元、不强抽纠缠的胶水」原则。
//!
//! 安全不降级（合并自 exotic 的更严策略）：全程仅 HTTPS、拒非 HTTPS 重定向降级、连接超时防慢速挂起。

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt as _;

/// 下载/校验错误。`code()` 稳定，可安全跨边界输出（不泄露内部细节）。
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("非 HTTPS 下载地址")]
    NotHttps,
    #[error("HTTP 请求失败：{0}")]
    Http(String),
    #[error("HTTP 状态码 {0}")]
    Status(u16),
    #[error("下载量超出上限（服务器超发）")]
    TooLarge,
    #[error("大小校验失败：期望 {expected} 实得 {got}")]
    SizeMismatch { expected: u64, got: u64 },
    #[error("sha256 校验失败（文件损坏或被篡改）")]
    HashMismatch,
    #[error("IO 失败：{0}")]
    Io(String),
}

impl DownloadError {
    pub fn code(&self) -> &'static str {
        match self {
            DownloadError::NotHttps => "not_https",
            DownloadError::Http(_) => "http",
            DownloadError::Status(_) => "status",
            DownloadError::TooLarge => "too_large",
            DownloadError::SizeMismatch { .. } => "size_mismatch",
            DownloadError::HashMismatch => "hash_mismatch",
            DownloadError::Io(_) => "io",
        }
    }
}

/// 超时策略。🔴 大文件不可套用小文件的整体超时——否则慢速链路下 ~1GB 模型 blob（Part6 T7）
/// 会被整体超时**误杀**。故按调用方语义分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutPolicy {
    /// 小文件（Registry index/sig、MB 级插件包）：连接 15s + **整体 300s 封顶**。
    SmallFile,
    /// 大文件（模型 blob ~GB）：仅连接 15s 超时，**不设整体上限**（续传 + 连接超时已足够防挂起）。
    LargeFile,
}

/// 进度回调：以「本文件累计已接收字节」为参数。引擎内部已按 ~200ms 节流，回调侧无需再节流。
/// `Send + Sync` 边界必需——回调会被跨 `.await` 持有，否则下载 future 非 Send、无法在多线程
/// runtime 上的 Tauri 命令里 spawn（编译错误）。
pub type OnBytes<'a> = dyn Fn(u64) + Send + Sync + 'a;

/// 🔒 dev-only file:// 传输旁路总开关(插件商店本地 registry 测试;SEC-02 姿态,同
/// `EXOTIC_PSD_WORKER_PATH`):仅 debug 构建编入 + 环境变量 `PICASA_EXOTIC_DEV_FILE_URLS=1`
/// 显式开启,双重门控。**只**替换「传输」一步——验签/sha256/size 等完整性校验全部原样
/// 保留(生产等价);Release 构建该分支整体不存在,file:// 一律走 NotHttps 拒绝。
#[cfg(debug_assertions)]
pub(crate) fn dev_file_urls_enabled() -> bool {
    std::env::var("PICASA_EXOTIC_DEV_FILE_URLS").is_ok_and(|v| v == "1")
}
#[cfg(not(debug_assertions))]
pub(crate) fn dev_file_urls_enabled() -> bool {
    false
}

/// `file:///D:/x/y.zip` → 本地路径(仅开关开启时 Some)。不做百分号解码——dev registry
/// 工具(scripts/exotic-dev-registry.mjs)生成的路径不含空格/转义字符,从紧即可。
#[cfg(debug_assertions)]
fn dev_file_url_path(url: &str) -> Option<std::path::PathBuf> {
    if !dev_file_urls_enabled() {
        return None;
    }
    url.strip_prefix("file:///").map(std::path::PathBuf::from)
}

/// 不发请求即拒非 HTTPS（首跳；后续跳由 client 的重定向策略把关）。
fn require_https(url: &str) -> Result<(), DownloadError> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(DownloadError::NotHttps)
    }
}

/// 构建强制全程 HTTPS 的安全 client：连接 15s；重定向跳非 https 即拒、>10 跳即停；
/// 整体超时按 `policy` 分级。HF `resolve/` → CDN 的 302 是 HTTPS，故 AI 大文件下载兼容
/// （只拒**降级**到非 HTTPS 的跳转，不拒 HTTPS 跳转）。
/// `user_agent(..)`：reqwest 默认不发 User-Agent 头；部分源（如 ModelScope）前置 WAF 对
/// 空 UA 直接 403（2026-07-23 实测钉死：空 UA 必 403，非空 UA 即通，与 cookie 无关——curl
/// 默认自带 UA 从未复现过这个 403，此前误判为 cookie 缺失）。`cookie_store(true)` 一并开
/// 顺手对齐浏览器语义（同 client 内后续文件受益），非本次 403/size mismatch 的必要条件。
pub fn secure_client(policy: TimeoutPolicy) -> Result<reqwest::Client, DownloadError> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .cookie_store(true)
        .user_agent(concat!("scrollery/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("重定向到非 HTTPS 地址被拒")
            } else if attempt.previous().len() > 10 {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }));
    if policy == TimeoutPolicy::SmallFile {
        builder = builder.timeout(Duration::from_secs(300));
    }
    builder
        .build()
        .map_err(|e| DownloadError::Http(e.to_string()))
}

/// 下载 `url` 全文到内存，流式封顶 `max_len`（仅 HTTPS）。供 Registry index/sig 用：体量极小、
/// 无预知 size/sha（完整性由 Ed25519 验签在 `RegistryCache::accept` 内把关），此处只做传输安全。
pub async fn download_to_vec(
    client: &reqwest::Client,
    url: &str,
    max_len: u64,
) -> Result<Vec<u8>, DownloadError> {
    // dev-only file:// 旁路:只换传输,大小封顶保留(内容完整性由调用方验签把关)。
    #[cfg(debug_assertions)]
    if let Some(p) = dev_file_url_path(url) {
        let meta = std::fs::metadata(&p).map_err(|e| DownloadError::Io(e.to_string()))?;
        if meta.len() > max_len {
            return Err(DownloadError::TooLarge);
        }
        return std::fs::read(&p).map_err(|e| DownloadError::Io(e.to_string()));
    }
    require_https(url)?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Status(resp.status().as_u16()));
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?
    {
        let next = (buf.len() as u64)
            .checked_add(chunk.len() as u64)
            .ok_or(DownloadError::TooLarge)?;
        if next > max_len {
            return Err(DownloadError::TooLarge); // 服务器超发 → 立即中止
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// 把单个文件流式写入 `part_path`，`resume_from>0` 时经 HTTP Range 续传。边收边发节流进度（~5/s）。
/// 请求了 Range 但服务器返回整文件（200 而非 206）→ 从 0 重写（覆盖 part）。
/// 写入前以 `expected_size` 封顶（含续传前缀）；短读及哈希仍由调用方最终校验。
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    resume_from: u64,
    expected_size: u64,
    on_bytes: &OnBytes<'_>,
) -> Result<(), DownloadError> {
    // dev-only file:// 旁路:整文件复制到 part(忽略续传——本地复制幂等,MB 级包无续传
    // 需求);size/sha256 校验仍由调用方(fetch_package/fetch_model_blob)对 part 执行。
    #[cfg(debug_assertions)]
    if let Some(src) = dev_file_url_path(url) {
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| DownloadError::Io(e.to_string()))?;
        }
        return copy_local_download(&src, part_path, expected_size, on_bytes).await;
    }
    require_https(url)?;
    if let Some(parent) = part_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?;
    }

    // 续传偏移必须对应当前 part；超长或过期残留从零请求，不能把错误前缀拼入新响应。
    let resume_from = valid_resume_offset(part_path, resume_from, expected_size).await;
    let mut req = client.get(url);
    if resume_from > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    write_download_response(resp, part_path, resume_from, expected_size, on_bytes).await
}

// 响应处理独立于 HTTPS 请求门禁，便于用本地响应验证真实流式写入行为。
async fn write_download_response(
    mut resp: reqwest::Response,
    part_path: &Path,
    resume_from: u64,
    expected_size: u64,
    on_bytes: &OnBytes<'_>,
) -> Result<(), DownloadError> {
    if !resp.status().is_success() {
        return Err(DownloadError::Status(resp.status().as_u16()));
    }

    // 请求了 Range 且服务器以 206 应答 → 追加续写；否则（含 200 整文件）从头创建。
    let appending = resume_from > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let range = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes "))
            .and_then(|v| v.split_once('/'))
            .and_then(|(range, total)| {
                let (start, end) = range.split_once('-')?;
                Some((
                    start.parse::<u64>().ok()?,
                    end.parse::<u64>().ok()?,
                    total.parse::<u64>().ok()?,
                ))
            });
        if !matches!(range, Some((start, end, total)) if start == resume_from && start <= end && end < total && total == expected_size)
        {
            return Err(DownloadError::Http(
                "续传响应的 Content-Range 不匹配".into(),
            ));
        }
    }
    let mut file_received = if appending { resume_from } else { 0 };
    if let Some(length) = resp.content_length() {
        checked_download_size(file_received, length, expected_size)?;
    }
    let mut file = if appending {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part_path)
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?
    } else {
        tokio::fs::File::create(part_path)
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?
    };

    if appending {
        let got = file
            .metadata()
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?
            .len();
        if got != resume_from {
            return Err(DownloadError::SizeMismatch {
                expected: resume_from,
                got,
            });
        }
    }
    let mut last = std::time::Instant::now();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?
    {
        let next = match checked_download_size(file_received, chunk.len() as u64, expected_size) {
            Ok(next) => next,
            Err(e) => {
                // 已接受的前缀先落盘，镜像回退才能从真实长度续传；超额块不写入。
                file.flush()
                    .await
                    .map_err(|e| DownloadError::Io(e.to_string()))?;
                return Err(e);
            }
        };
        file.write_all(&chunk)
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        file_received = next;
        if last.elapsed().as_millis() >= 200 {
            last = std::time::Instant::now();
            on_bytes(file_received);
        }
    }
    file.flush()
        .await
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    Ok(())
}

fn checked_download_size(
    received: u64,
    additional: u64,
    expected: u64,
) -> Result<u64, DownloadError> {
    received
        .checked_add(additional)
        .filter(|n| *n <= expected)
        .ok_or(DownloadError::TooLarge)
}

async fn valid_resume_offset(path: &Path, requested: u64, expected: u64) -> u64 {
    if requested > 0 && requested <= expected {
        if let Ok(meta) = tokio::fs::metadata(path).await {
            if meta.len() == requested {
                return requested;
            }
        }
    }
    0
}

#[cfg(debug_assertions)]
async fn copy_local_download(
    src: &Path,
    dest: &Path,
    expected_size: u64,
    on_bytes: &OnBytes<'_>,
) -> Result<(), DownloadError> {
    use tokio::io::AsyncReadExt as _;
    let mut source = tokio::fs::File::open(src)
        .await
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    let length = source
        .metadata()
        .await
        .map_err(|e| DownloadError::Io(e.to_string()))?
        .len();
    checked_download_size(0, length, expected_size)?;
    let mut dest = tokio::fs::File::create(dest)
        .await
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut buffer = [0; 64 * 1024];
    let mut received = 0;
    loop {
        let n = source
            .read(&mut buffer)
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        received = checked_download_size(received, n as u64, expected_size)?;
        dest.write_all(&buffer[..n])
            .await
            .map_err(|e| DownloadError::Io(e.to_string()))?;
    }
    dest.flush()
        .await
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    on_bytes(received);
    Ok(())
}

/// 按候选源顺序逐一尝试下载到 `part_path`，首个成功即返回；全失败返回最后一次错误。
/// 镜像重试时从失败尝试留下的 `.part` 处续传；残留 `.part` 超过 `expected_size` 则清零重来。
pub async fn download_with_fallback(
    client: &reqwest::Client,
    urls: &[&str],
    part_path: &Path,
    mut resume_from: u64,
    expected_size: u64,
    on_bytes: &OnBytes<'_>,
) -> Result<(), DownloadError> {
    let mut last_err = DownloadError::Http("无候选下载源".to_string());
    for url in urls {
        match download_file(client, url, part_path, resume_from, expected_size, on_bytes).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                // 下一个候选源从已落地的 .part 处续传；若残留超出目标大小则丢弃重来。
                resume_from = tokio::fs::metadata(part_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                if resume_from > expected_size {
                    let _ = tokio::fs::remove_file(part_path).await;
                    resume_from = 0;
                }
            }
        }
    }
    Err(last_err)
}

/// 文件 sha256(小写 hex)。实现收拢于 utils::hash(R2-6);re-export 保持既有
/// `crate::download::sha256_hex_of_file` 调用路径零破坏。
pub use crate::utils::hash::sha256_hex_of_file;

/// `expected` 为 `None`（无需校验）或文件 sha256 与之相等（大小写不敏感）时返回 true。
pub fn sha256_matches(path: &Path, expected: Option<&str>) -> bool {
    let expected = match expected {
        Some(e) => e,
        None => return true,
    };
    match sha256_hex_of_file(path) {
        Ok(hex) => hex.eq_ignore_ascii_case(expected),
        Err(_) => false,
    }
}

/// 下载后完整性校验：先核大小，再核 sha256（`expected_sha=None` 时跳过哈希）。
/// 任一失败返回对应错误，不删文件（由调用方决定清理 `.part`）。
pub fn verify_size_sha(
    path: &Path,
    expected_size: u64,
    expected_sha: Option<&str>,
) -> Result<(), DownloadError> {
    let got = std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    if got != expected_size {
        return Err(DownloadError::SizeMismatch {
            expected: expected_size,
            got,
        });
    }
    if !sha256_matches(path, expected_sha) {
        return Err(DownloadError::HashMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop() -> Box<OnBytes<'static>> {
        Box::new(|_| {})
    }

    // 仅测试响应处理：生产入口仍先拒绝非 HTTPS，测试不访问外部网络。
    async fn response(status: &str, headers: &str, body: &str) -> reqwest::Response {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let wire = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n{body}");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket.write_all(wire.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let resp = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
            .get(format!("http://{address}/file"))
            .send()
            .await
            .unwrap();
        server.await.unwrap();
        resp
    }

    #[tokio::test]
    async fn chunked_exact_and_short_downloads_preserve_final_verification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        let resp = response(
            "200 OK",
            "Transfer-Encoding: chunked\r\n",
            "5\r\nhello\r\n0\r\n\r\n",
        )
        .await;
        write_download_response(resp, &path, 0, 5, &|_| {})
            .await
            .unwrap();
        assert!(verify_size_sha(&path, 5, None).is_ok());

        let resp = response(
            "200 OK",
            "Transfer-Encoding: chunked\r\n",
            "3\r\nhel\r\n0\r\n\r\n",
        )
        .await;
        write_download_response(resp, &path, 0, 5, &|_| {})
            .await
            .unwrap();
        assert!(matches!(
            verify_size_sha(&path, 5, None),
            Err(DownloadError::SizeMismatch {
                expected: 5,
                got: 3
            })
        ));
    }

    #[tokio::test]
    async fn range_response_appends_and_ignored_range_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        std::fs::write(&path, b"he").unwrap();
        let resp = response(
            "206 Partial Content",
            "Content-Length: 3\r\nContent-Range: bytes 2-4/5\r\n",
            "llo",
        )
        .await;
        write_download_response(resp, &path, 2, 5, &|_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");

        let resp = response("200 OK", "Content-Length: 5\r\n", "world").await;
        write_download_response(resp, &path, 2, 5, &|_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"world");
    }

    #[tokio::test]
    async fn chunked_oversend_never_writes_beyond_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        let resp = response(
            "200 OK",
            "Transfer-Encoding: chunked\r\n",
            "5\r\nhello\r\n1\r\n!\r\n0\r\n\r\n",
        )
        .await;
        let result = write_download_response(resp, &path, 0, 5, &|_| {}).await;
        assert!(matches!(result, Err(DownloadError::TooLarge)), "{result:?}");
        assert!(std::fs::metadata(&path).unwrap().len() <= 5);
    }

    #[tokio::test]
    async fn declared_zero_rejects_nonempty_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        let resp = response(
            "200 OK",
            "Transfer-Encoding: chunked\r\n",
            "1\r\nx\r\n0\r\n\r\n",
        )
        .await;
        let result = write_download_response(resp, &path, 0, 0, &|_| {}).await;
        assert!(matches!(result, Err(DownloadError::TooLarge)), "{result:?}");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn resumed_oversend_counts_existing_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        std::fs::write(&path, b"he").unwrap();
        let resp = response(
            "206 Partial Content",
            "Transfer-Encoding: chunked\r\nContent-Range: bytes 2-4/5\r\n",
            "3\r\nllo\r\n1\r\n!\r\n0\r\n\r\n",
        )
        .await;
        let result = write_download_response(resp, &path, 2, 5, &|_| {}).await;
        assert!(matches!(result, Err(DownloadError::TooLarge)), "{result:?}");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"he") && bytes.len() <= 5);
    }

    #[tokio::test]
    async fn wrong_range_start_does_not_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        std::fs::write(&path, b"he").unwrap();
        let resp = response(
            "206 Partial Content",
            "Content-Length: 3\r\nContent-Range: bytes 1-3/5\r\n",
            "llo",
        )
        .await;
        assert!(write_download_response(resp, &path, 2, 5, &|_| {})
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"he");
    }

    #[tokio::test]
    async fn changed_part_length_does_not_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        std::fs::write(&path, b"hel").unwrap();
        let resp = response(
            "206 Partial Content",
            "Content-Length: 3\r\nContent-Range: bytes 2-4/5\r\n",
            "llo",
        )
        .await;
        assert!(write_download_response(resp, &path, 2, 5, &|_| {})
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"hel");
    }

    #[tokio::test]
    async fn declared_oversend_preserves_existing_part() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        std::fs::write(&path, b"he").unwrap();
        let resp = response("200 OK", "Content-Length: 6\r\n", "hello!").await;
        assert!(matches!(
            write_download_response(resp, &path, 2, 5, &|_| {}).await,
            Err(DownloadError::TooLarge)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"he");
    }

    #[tokio::test]
    async fn invalid_initial_resume_offset_restarts_from_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        assert_eq!(valid_resume_offset(&path, 2, 5).await, 0);
        std::fs::write(&path, b"he").unwrap();
        assert_eq!(valid_resume_offset(&path, 2, 5).await, 2);
        assert_eq!(valid_resume_offset(&path, 1, 5).await, 0);
        std::fs::write(&path, b"hello!").unwrap();
        assert_eq!(valid_resume_offset(&path, 5, 5).await, 0);
        assert_eq!(valid_resume_offset(&path, 6, 5).await, 0);
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn local_download_enforces_the_same_size_budget() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let dest = dir.path().join("file.part");
        std::fs::write(&src, b"hello").unwrap();
        std::fs::write(&dest, b"he").unwrap();
        assert!(matches!(
            copy_local_download(&src, &dest, 4, &|_| {}).await,
            Err(DownloadError::TooLarge)
        ));
        assert_eq!(std::fs::read(&dest).unwrap(), b"he");
        copy_local_download(&src, &dest, 5, &|n| assert_eq!(n, 5))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    }

    #[test]
    fn require_https_rejects_http() {
        assert!(matches!(
            require_https("http://x.invalid/a"),
            Err(DownloadError::NotHttps)
        ));
        assert!(require_https("https://x.invalid/a").is_ok());
    }

    #[test]
    fn secure_client_builds_both_policies() {
        assert!(secure_client(TimeoutPolicy::SmallFile).is_ok());
        assert!(secure_client(TimeoutPolicy::LargeFile).is_ok());
    }

    #[test]
    fn download_file_rejects_non_https() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = secure_client(TimeoutPolicy::SmallFile).unwrap();
        let dest = std::env::temp_dir().join("dl-nohttps.part");
        let cb = noop();
        let r = rt.block_on(download_file(
            &client,
            "http://x.invalid/a",
            &dest,
            0,
            5,
            cb.as_ref(),
        ));
        assert!(matches!(r, Err(DownloadError::NotHttps)));
    }

    #[test]
    fn download_to_vec_rejects_non_https() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = secure_client(TimeoutPolicy::SmallFile).unwrap();
        let r = rt.block_on(download_to_vec(
            &client,
            "http://x.invalid/index.json",
            4096,
        ));
        assert!(matches!(r, Err(DownloadError::NotHttps)));
    }

    #[test]
    fn sha256_matches_and_hex() {
        let path = std::env::temp_dir().join("dl-sha-test.bin");
        std::fs::write(&path, b"hello").unwrap();
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let expect = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(sha256_hex_of_file(&path).unwrap(), expect);
        assert!(sha256_matches(&path, Some(expect)));
        assert!(sha256_matches(&path, Some(&expect.to_uppercase())));
        assert!(sha256_matches(&path, None)); // None = 不校验
        assert!(!sha256_matches(&path, Some("00")));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn verify_size_sha_catches_size_mismatch() {
        let path = std::env::temp_dir().join("dl-verify-test.bin");
        std::fs::write(&path, b"hello").unwrap(); // 5 bytes
        assert!(matches!(
            verify_size_sha(&path, 99, None),
            Err(DownloadError::SizeMismatch {
                expected: 99,
                got: 5
            })
        ));
        assert!(verify_size_sha(&path, 5, None).is_ok());
        let _ = std::fs::remove_file(&path);
    }
}
