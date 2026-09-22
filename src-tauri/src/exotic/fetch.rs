// src-tauri/src/exotic/fetch.rs
//! 冷门格式插件 · 包下载（v3 Part3 §6.4 第 2 步）。
//!
//! **R10 收敛后（Part6 §3.1.2）的薄适配器**：下载机制（安全 client / Range / 镜像回退 / sha256 /
//! `.part` 原子改名）已下沉 `crate::download` 通用引擎；本文件只保留 exotic 的**领域契约**——
//! 稳定的 `FetchError` 错误码（供 `exotic_commands` 跨 IPC 边界使用，调用方零改动）+ Registry/包
//! 两个下载入口。机制不再在此重复实现。
//!
//! 安全：只接受 HTTPS；下载量精确封顶到 `expected_size`（超出立即中止）；size+sha256 双校验后才
//! `.part` → 原子 rename——这些策略现由通用引擎统一保证。
//!
//! 三个入口同属直销面:`fetch_package`/`fetch_model_blob` 是下载-执行面,
//! `download_registry_index` 取签名元数据(验签在 registry.rs,不在此)。

// Path 仅被两个下载入口使用(registry 入口收 &str)。
use std::path::Path;

use crate::download::{self, DownloadError, TimeoutPolicy};

/// 下载/校验错误。`code()` 稳定，可安全输出。
/// （exotic 的公开错误契约；机制错误来自通用引擎的 `DownloadError`，此处 1:1 折叠以保持稳定码。）
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("非 HTTPS 下载地址")]
    NotHttps,
    #[error("HTTP 请求失败：{0}")]
    Http(String),
    #[error("HTTP 状态码 {0}")]
    Status(u16),
    #[error("下载量超出声明大小（服务器超发）")]
    TooLarge,
    #[error("大小校验失败：期望 {expected} 实得 {got}")]
    SizeMismatch { expected: u64, got: u64 },
    #[error("sha256 校验失败（包损坏或被篡改）")]
    HashMismatch,
    #[error("IO 失败：{0}")]
    Io(String),
}

impl FetchError {
    pub fn code(&self) -> &'static str {
        match self {
            FetchError::NotHttps => "not_https",
            FetchError::Http(_) => "http",
            FetchError::Status(_) => "status",
            FetchError::TooLarge => "too_large",
            FetchError::SizeMismatch { .. } => "size_mismatch",
            FetchError::HashMismatch => "hash_mismatch",
            FetchError::Io(_) => "io",
        }
    }
}

/// 通用引擎错误 → exotic 稳定错误码（1:1，码值不变）。
impl From<DownloadError> for FetchError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::NotHttps => FetchError::NotHttps,
            DownloadError::Http(s) => FetchError::Http(s),
            DownloadError::Status(c) => FetchError::Status(c),
            DownloadError::TooLarge => FetchError::TooLarge,
            DownloadError::SizeMismatch { expected, got } => {
                FetchError::SizeMismatch { expected, got }
            }
            DownloadError::HashMismatch => FetchError::HashMismatch,
            DownloadError::Io(s) => FetchError::Io(s),
        }
    }
}

/// Registry index/sig 单文件大小上限（index 仅 KB 级；防服务器超发巨型输入）。
/// 与 registry.rs 的 `MAX_INDEX_LEN`(4MiB) 同量级——下载层先封顶，解析层再校验，双保险。
const MAX_REGISTRY_FILE: u64 = 4 * 1024 * 1024;

/// 大文件同步校验经阻塞线程池执行，避免占用运行时线程。
/// 保持 DownloadError，由调用点映射为 FetchError；阻塞任务异常沿用 IO 错误码。
async fn verify_size_sha_blocking(
    path: std::path::PathBuf,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), DownloadError> {
    let sha = expected_sha256.to_owned();
    tokio::task::spawn_blocking(move || {
        #[cfg(test)]
        VERIFY_EXEC_PROBE.hook(&path);
        download::verify_size_sha(&path, expected_size, Some(&sha))
    })
    .await
    .unwrap_or_else(|e| Err(DownloadError::Io(e.to_string())))
}

/// 测试埋点（仅 `#[cfg(test)]`）：挂钩在真实哈希闭包内、即 `spawn_blocking` 的阻塞线程上执行，
/// 回报执行线程 id。供单测确定性断言「整文件哈希不跑在 runtime 执行器线程上」（不靠耗时阈值）。
/// 按**探测路径**筛选（测试用独立临时目录），并发跑测互不干扰。生产构建整体不编译。
#[cfg(test)]
struct VerifyExecProbe {
    thread_reports: std::sync::Mutex<
        Vec<(
            std::path::PathBuf,
            std::sync::mpsc::Sender<std::thread::ThreadId>,
        )>,
    >,
}

#[cfg(test)]
impl VerifyExecProbe {
    const fn new() -> Self {
        Self {
            thread_reports: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn hook(&self, path: &Path) {
        if let Ok(reports) = self.thread_reports.lock() {
            for (dir, tx) in reports.iter() {
                if path.starts_with(dir) {
                    let _ = tx.send(std::thread::current().id());
                }
            }
        }
    }
}

#[cfg(test)]
static VERIFY_EXEC_PROBE: VerifyExecProbe = VerifyExecProbe::new();

/// 下载 `url` 到 `dest`，对照 `expected_size`/`expected_sha256` 校验通过后原子就位。
/// 失败不留下 `dest`（清理 `.part`）。`expected_size` 即硬上限：候选源失败回退时据此判断残留续传。
///
/// 包体量小（MB 级），暂不接进度回调（插件商店安装进度 UI 待 Part8）；但已走通用引擎，
/// **Range 续传/镜像回退能力随引擎天然具备**，后续接入仅需传候选源列表 + 进度回调。
pub async fn fetch_package(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), FetchError> {
    let client = download::secure_client(TimeoutPolicy::SmallFile)?;
    let part = dest.with_extension("part");

    // 下载（单一源；引擎支持镜像回退，exotic Registry 暂无镜像字段，故传单元素候选）。
    let noop = |_: u64| {};
    let dl =
        download::download_with_fallback(&client, &[url], &part, 0, expected_size, &noop).await;
    if let Err(e) = dl {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e.into());
    }

    // 下载后完整性校验（size + sha256）；同步哈希甩出执行器线程。
    if let Err(e) = verify_size_sha_blocking(part.clone(), expected_size, expected_sha256).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e.into());
    }

    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| FetchError::Io(e.to_string()))?;
    Ok(())
}

/// 下载模型权重 blob 到 `dest`(Part4 §3.7.1/T12 分步下载)。与 [`fetch_package`] 同构,但:
/// - `TimeoutPolicy::LargeFile`(GB 级传输);
/// - **幂等跳过**:`dest` 已存在且 size+sha256 全符 → 不触网直接 Ok(安装重试/多插件
///   共享同名权重时天然去重);
/// - **断点续传**:失败保留 `.part`,下次调用从残留处 Range 续传(GB 级重下代价高,
///   与 fetch_package 的「小文件失败即删」策略刻意不同);校验失败的 `.part` 必删
///   (损坏数据上续传只会叠加损坏);
/// - blob 不进 zip → `InstallLimits` 的 zip-bomb 检查对其天然不适用,自身防线 =
///   HTTPS + size 精确封顶 + sha256 + 文件名白名单(registry 数据面校验)。
pub async fn fetch_model_blob(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), FetchError> {
    // 幂等:已就位且校验通过 → 跳过(不触网)。
    if verify_size_sha_blocking(dest.to_path_buf(), expected_size, expected_sha256)
        .await
        .is_ok()
    {
        return Ok(());
    }
    let client = download::secure_client(TimeoutPolicy::LargeFile)?;
    // 追加式 ".part" 后缀(非 with_extension 替换——"a.onnx"/"a.bin" 两 blob 不得撞同一 part)。
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().and_then(|s| s.to_str()).unwrap_or("blob")
    ));
    // 从上次失败残留处续传;残留超长交引擎清零重来。
    let resume_from = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(expected_size);
    let noop = |_: u64| {};
    download::download_with_fallback(&client, &[url], &part, resume_from, expected_size, &noop)
        .await
        .map_err(FetchError::from)?; // 失败保留 .part 供续传

    if let Err(e) = verify_size_sha_blocking(part.clone(), expected_size, expected_sha256).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e.into());
    }
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| FetchError::Io(e.to_string()))?;
    Ok(())
}

/// 下载 Registry 的 `index.json` + `index.sig`（**验签前**的原始字节）。
/// **不在此验签/防回滚**——交 `RegistryCache::accept`（验签 + 单调防回滚先于解析，registry.rs:242）。
/// 返回 `(index_bytes, sig_bytes)`。`base_url` 末尾斜杠容错。
pub async fn download_registry_index(base_url: &str) -> Result<(Vec<u8>, Vec<u8>), FetchError> {
    let client = download::secure_client(TimeoutPolicy::SmallFile)?;
    let base = base_url.trim_end_matches('/');
    let index =
        download::download_to_vec(&client, &format!("{base}/index.json"), MAX_REGISTRY_FILE)
            .await?;
    let sig =
        download::download_to_vec(&client, &format!("{base}/index.sig"), MAX_REGISTRY_FILE).await?;
    Ok((index, sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https() {
        // 非 https 在引擎层即拒（不发请求）；适配器透传为 FetchError::NotHttps。
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("package.zip");
        let part = dest.with_extension("part");
        std::fs::write(&part, b"partial").unwrap();
        let r = rt.block_on(fetch_package("http://x.invalid/a.zip", &dest, 1, "00"));
        assert!(matches!(r, Err(FetchError::NotHttps)));
        assert!(!part.exists());
        assert!(!dest.exists());
    }

    #[test]
    fn model_blob_rejects_non_https() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("blob.onnx");
        let part = dir.path().join("blob.onnx.part");
        std::fs::write(&part, b"partial").unwrap();
        let r = rt.block_on(fetch_model_blob(
            "http://x.invalid/a.onnx",
            &dest,
            9,
            &"0".repeat(64),
        ));
        assert!(matches!(r, Err(FetchError::NotHttps)));
        assert_eq!(std::fs::read(&part).unwrap(), b"partial");
        assert!(!dest.exists());
    }

    #[test]
    fn model_blob_idempotent_skip_when_already_valid() {
        // 已就位且 size+sha 全符 → 不触网直接 Ok(URL 即便非法也不会被访问)。
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let dest = std::env::temp_dir().join("exotic-blob-idem.onnx");
        std::fs::write(&dest, b"weights").unwrap();
        let sha = crate::download::sha256_hex_of_file(&dest).unwrap();
        let r = rt.block_on(fetch_model_blob("http://x.invalid/a.onnx", &dest, 7, &sha));
        assert!(r.is_ok(), "已就位应幂等跳过:{r:?}");
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn registry_index_rejects_non_https() {
        // Registry 下载同样强制 HTTPS：非 https 基址在发请求前即拒（不触网）。
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let r = rt.block_on(download_registry_index("http://x.invalid/exotic"));
        assert!(matches!(r, Err(FetchError::NotHttps)));
    }

    // ── F07：整文件 verify_size_sha 移出异步运行时线程的表征/回归 ────────────────────────────
    //
    // 用 download 引擎的 dev-only `file://` 传输旁路（与 installer 的 dev registry 测试同一开关）
    // 在**无网络**下走完「取字节 → size+sha256 校验 → part 原子改名」真实生产链。校验步改经
    // `spawn_blocking`，由 `VERIFY_EXEC_PROBE` 在真实哈希闭包内做确定性判定（不靠耗时阈值）。

    /// 两个 dev 旁路表征测试共享同一进程环境变量，必须串行置位/恢复（并行跑测否则互相踩）。
    static DEV_FILE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `file://` 传输旁路开关守卫：置位后于 Drop 恢复旧值，不污染同进程其他测试。
    /// （仅 debug 构建 + 显式环境变量生效；Release 下 file:// 恒按 NotHttps 拒绝。）
    struct DevFileUrlsGuard(Option<std::ffi::OsString>);

    impl DevFileUrlsGuard {
        fn enable() -> Self {
            let prev = std::env::var_os("PICASA_EXOTIC_DEV_FILE_URLS");
            std::env::set_var("PICASA_EXOTIC_DEV_FILE_URLS", "1");
            Self(prev)
        }
    }

    impl Drop for DevFileUrlsGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("PICASA_EXOTIC_DEV_FILE_URLS", v),
                None => std::env::remove_var("PICASA_EXOTIC_DEV_FILE_URLS"),
            }
        }
    }

    /// 唯一临时目录（RAII 清理）：隔离各测试文件，并作为埋点的路径作用域。
    fn temp_dir(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("exotic-f07-{tag}-"))
            .tempdir()
            .unwrap()
    }

    fn file_url(path: &Path) -> String {
        format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
    }

    fn current_thread_rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn package_hash_mismatch_deletes_part_and_keeps_error_code() {
        // 表征：包下载「校验失败即删 .part、不落 dest」，错误码稳定为 hash_mismatch。
        let _serial = DEV_FILE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _dev = DevFileUrlsGuard::enable();
        let dir = temp_dir("pkg-hash");
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"package-bytes").unwrap();
        let dest = dir.path().join("pkg.zip");
        let part = dest.with_extension("part");

        let err = current_thread_rt()
            .block_on(fetch_package(&file_url(&src), &dest, 13, &"0".repeat(64)))
            .unwrap_err();
        assert!(matches!(err, FetchError::HashMismatch), "{err:?}");
        assert_eq!(err.code(), "hash_mismatch");
        assert!(!part.exists(), "包校验失败须删 .part");
        assert!(!dest.exists(), "包失败不得落下 dest");
    }

    #[test]
    fn model_blob_size_mismatch_deletes_part_and_keeps_error_code() {
        // 表征：模型 blob「大小不符 → 删 .part、保留稳定 size_mismatch 码」（与下载失败保留
        // .part 供续传的策略刻意不同——损坏数据上续传只会叠加损坏）。
        // 用**短读**（实得 7、期望 9）触发：超长（7 期望 5）现由下载引擎流式封顶提前判 TooLarge。
        let _serial = DEV_FILE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _dev = DevFileUrlsGuard::enable();
        let dir = temp_dir("blob-size");
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"weights").unwrap(); // 7 字节，声明 9
        let dest = dir.path().join("blob.onnx");
        let part = dest.with_file_name("blob.onnx.part");

        let err = current_thread_rt()
            .block_on(fetch_model_blob(&file_url(&src), &dest, 9, &"0".repeat(64)))
            .unwrap_err();
        assert!(
            matches!(
                err,
                FetchError::SizeMismatch {
                    expected: 9,
                    got: 7
                }
            ),
            "{err:?}"
        );
        assert_eq!(err.code(), "size_mismatch");
        assert!(!part.exists(), "blob 校验失败须删 .part");
        assert!(!dest.exists());
    }

    #[test]
    fn hash_verify_runs_on_blocking_thread_not_runtime_worker() {
        // 机制锁：整文件哈希经 `spawn_blocking` 在阻塞线程池执行，与驱动 future 的 runtime
        // 执行器线程不同。埋点在真实哈希闭包内回报线程 id，确定性判定（不含耗时阈值）。
        let dir = temp_dir("blob-thread");
        let src = dir.path().join("src.bin");
        let data = vec![7u8; 64 * 1024];
        std::fs::write(&src, &data).unwrap();
        let sha = crate::download::sha256_hex_of_file(&src).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        VERIFY_EXEC_PROBE
            .thread_reports
            .lock()
            .unwrap()
            .push((dir.path().to_path_buf(), tx));

        let rt = current_thread_rt();
        let driver_thread = std::thread::current().id();
        let r = rt.block_on(verify_size_sha_blocking(
            src.clone(),
            data.len() as u64,
            &sha,
        ));
        assert!(r.is_ok(), "哈希校验应通过：{r:?}");

        let exec_thread = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("哈希埋点未回报执行线程（校验未在阻塞闭包内运行）");
        assert_ne!(
            exec_thread, driver_thread,
            "哈希须在阻塞线程池执行，不得占用 runtime 执行器线程"
        );
        VERIFY_EXEC_PROBE.thread_reports.lock().unwrap().clear();
    }
}
