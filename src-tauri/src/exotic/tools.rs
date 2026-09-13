// src-tauri/src/exotic/tools.rs
//! 视频格式扩展子系统 · FFmpeg 工具包生命周期管理（design.md §3.1/§3.2/§3.3/§8 V2）。
//!
//! 与 [`crate::exotic::registry`]/[`crate::exotic::installer`] 的插件安装同姿态，但 FFmpeg
//! **不是签名插件包**——它是一个第三方（BtbN）预构建的 LGPL-shared 工具包，无 Registry 签名链，
//! 完整性只靠**钉死的 zip 整包 sha256 + 逐可执行文件 sha256**双重校验（本文件 manifest 常量即真源，
//! 生产 registry entry 签发归 Part8）。
//!
//! 目录布局：`{app_data}/exotic/tools/ffmpeg/{tag}/bin/{ffmpeg.exe,ffprobe.exe,*.dll}`；
//! `{tag}/.ready` 标记文件存在 = 该 tag 已下载、解压、逐文件 hash 复核全部通过、可安全启动。
//! 下载中的 zip 落 `{app_data}/exotic/tools/ffmpeg/{tag}.zip.tmp`，校验通过后同卷 rename 为
//! `{tag}.zip`（整包留档，供 [`install_ffmpeg_package`] 解压；校验不过则删除 `.tmp`，下次重下）。
//!
//! 安全要点：
//!   - zip 只解压 `bin/` 下的文件（exe + 运行所需 DLL），doc/include/lib/presets 等不落盘——
//!     缩小落盘面积与攻击面。
//!   - 每个 zip entry 先过 [`crate::exotic::package::is_safe_relative_path`] 白名单（拒绝
//!     `..`/绝对路径/盘符/反斜杠/NUL 等），再对目标路径的父目录 canonicalize 校验前缀落在
//!     `{tag}.extract.tmp/bin/` 内——双保险防 zip-slip（校验全过后整目录同卷 rename 为 `{tag}/`）。
//!   - 解压完成后对 `ffmpeg.exe`/`ffprobe.exe` 逐文件 sha256 二次校验，全过才落 `.ready`；
//!     任一失败作废 `.extract.tmp` 目录，已就绪终名目录不受影响。

use std::path::{Path, PathBuf};

use crate::download::{self, DownloadError, TimeoutPolicy};
use crate::utils::hash::sha256_hex_of_file;

// ── 钉死常量（实测证据：docs/worklogs/2026-07-24-视频格式扩展子系统/scratchpad/v2-ffmpeg-evidence.md）──
//
// BtbN/FFmpeg-Builds 的 dated autobuild release（非浮动 `latest` tag）；win64 LGPL-shared 变体，
// 已实测确认 configuration 无 `--enable-gpl`、`--disable-libx264`/`--disable-libx265`，且
// `-encoders` 含 `h264_mf` 与原生 `aac`。生产 registry entry 签发归 Part8，当前 manifest 常量即真源。

/// BtbN release tag（钉死，非浮动 `latest`）。互注(V7 项11):升级须同步改 `PluginStoreView.vue` 源码链 tag。
pub const BTBN_RELEASE_TAG: &str = "autobuild-2026-07-24-13-32";
/// zip 下载地址（HTTPS，钉死具体 tag 下的具体 asset）。
const BTBN_ZIP_URL: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-24-13-32/ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1.zip";
/// zip 整包大小（字节，实测）。
const BTBN_ZIP_SIZE: u64 = 62_417_810;
/// zip 整包 sha256（实测，`sha256sum`）。
const BTBN_ZIP_SHA256: &str = "b809e561254cc0634d9fe4ce469c02ac2e194d560e079bb242b96906948ffbeb";
/// `bin/ffmpeg.exe` sha256（实测）。`pub(crate)`:worker_service session init 下发该 manifest
/// 常量作 worker 侧防换包比对基准(而非本地现算值),闭合钉定链(§2.4 深审 V4-4)。
pub(crate) const FFMPEG_EXE_SHA256: &str =
    "1d53a27637354c9e81317b48ec63940260e1312b57f8f9ba9139639f6ff38f75";
/// `bin/ffprobe.exe` sha256（实测）。
const FFPROBE_EXE_SHA256: &str = "05cf93b999f8f2a7988b99c91e04d9ac2e88e2a106aabea1bcfdf3c946b47b4a";

/// FFmpeg 工具包错误。`code()` 稳定，可安全跨 IPC 边界输出（不泄露内部串）。
#[derive(Debug, thiserror::Error)]
pub enum ToolsError {
    #[error("下载失败：{0}")]
    Download(#[from] DownloadError),
    #[error("打开 zip 失败：{0}")]
    OpenZip(String),
    #[error("zip entry 路径非法：{0}")]
    UnsafeEntry(String),
    #[error("zip 内缺少 ffmpeg.exe/ffprobe.exe")]
    MissingBinary,
    #[error("文件 hash 与 manifest 不符：{0}")]
    HashMismatch(String),
    #[error("IO 失败：{0}")]
    Io(String),
}

impl ToolsError {
    pub fn code(&self) -> &'static str {
        match self {
            ToolsError::Download(_) => "download",
            ToolsError::OpenZip(_) => "open_zip",
            ToolsError::UnsafeEntry(_) => "unsafe_entry",
            ToolsError::MissingBinary => "missing_binary",
            ToolsError::HashMismatch(_) => "hash_mismatch",
            ToolsError::Io(_) => "io",
        }
    }
}

/// FFmpeg 工具包当前状态（`ffmpeg_tool_status` 返回）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    /// 未下载：目录不存在，或存在但无 `.ready` 标记且无下载中残留。
    NotDownloaded,
    /// 下载中：zip `.tmp` 残留文件存在（上次下载未完成，`Downloading` 供前端展示进度/重试）。
    Downloading,
    /// 就绪：`.ready` 标记存在且 `ffmpeg.exe`/`ffprobe.exe` 均在，返回绝对路径。
    Ready {
        ffmpeg_exe: PathBuf,
        ffprobe_exe: PathBuf,
    },
}

/// `{app_data}/exotic/tools/ffmpeg` 根目录。
pub fn tools_root(app_data: &Path) -> PathBuf {
    app_data.join("exotic").join("tools").join("ffmpeg")
}

/// 钉死 tag 的安装目录（`{tools_root}/{BTBN_RELEASE_TAG}`）。
pub fn tag_dir(app_data: &Path) -> PathBuf {
    tools_root(app_data).join(BTBN_RELEASE_TAG)
}

/// ready 标记文件路径（存在 = 该 tag 目录已完整校验通过）。
fn ready_marker(dir: &Path) -> PathBuf {
    dir.join(".ready")
}

/// 已落地/待下载的 zip 整包路径。
fn zip_path(app_data: &Path) -> PathBuf {
    tools_root(app_data).join(format!("{BTBN_RELEASE_TAG}.zip"))
}

/// 下载中间态（`*.tmp` + 同卷 rename）。
fn zip_tmp_path(app_data: &Path) -> PathBuf {
    let dest = zip_path(app_data);
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("ffmpeg.zip");
    dest.with_file_name(format!("{name}.tmp"))
}

/// 查询当前 FFmpeg 工具包状态（未下载/下载中/就绪）。纯文件系统检查，不触网。
pub fn ffmpeg_tool_status(app_data: &Path) -> ToolStatus {
    let dir = tag_dir(app_data);
    if ready_marker(&dir).exists() {
        let ffmpeg_exe = dir.join("bin").join("ffmpeg.exe");
        let ffprobe_exe = dir.join("bin").join("ffprobe.exe");
        if ffmpeg_exe.exists() && ffprobe_exe.exists() {
            return ToolStatus::Ready {
                ffmpeg_exe,
                ffprobe_exe,
            };
        }
        // ready 标记在但可执行文件缺失（用户手工删除/磁盘故障）：不能证明可用，按未下载处理，
        // 不 panic、不误报 Ready（fail-closed）。
    }
    if zip_tmp_path(app_data).exists() {
        return ToolStatus::Downloading;
    }
    ToolStatus::NotDownloaded
}

/// `verify_size_sha` 对 62MB 级 zip 全量流式读取算 sha256，是同步阻塞调用；在 async fn 内直跑
/// 会占住 tokio 执行器线程，故统一经 `spawn_blocking` 甩到阻塞线程池。
/// （备注：`download/fetch.rs:130/150` 同型同步哈希调用是基线存量，此处不修，待随下载引擎线收口。）
async fn verify_size_sha_blocking(path: PathBuf) -> Result<(), DownloadError> {
    tokio::task::spawn_blocking(move || {
        download::verify_size_sha(&path, BTBN_ZIP_SIZE, Some(BTBN_ZIP_SHA256))
    })
    .await
    .unwrap_or_else(|e| Err(DownloadError::Io(e.to_string())))
}

/// 下载 FFmpeg zip 整包（幂等：已落地且 size+sha256 全符时不触网直接返回）。
/// 走通用下载引擎（HTTPS 强制/重定向加固/Range 续传）；校验不过删除 `.tmp`，下次重下（整包作废）。
pub async fn download_ffmpeg_package(app_data: &Path) -> Result<PathBuf, ToolsError> {
    let root = tools_root(app_data);
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| ToolsError::Io(e.to_string()))?;
    let dest = zip_path(app_data);
    // 幂等跳过：已就位且哈希对得上 → 不重下。
    if verify_size_sha_blocking(dest.clone()).await.is_ok() {
        return Ok(dest);
    }
    let tmp = zip_tmp_path(app_data);
    let client = download::secure_client(TimeoutPolicy::LargeFile)?;
    // 续传：残留 .tmp 超出目标大小时引擎自动清零重来（download_with_fallback 内置该逻辑）。
    let resume_from = tokio::fs::metadata(&tmp)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(BTBN_ZIP_SIZE);
    if resume_from == BTBN_ZIP_SIZE {
        // tmp 恰好完整（上次下载在校验/rename 前中断）：不再发 Range 请求（避免服务端对「从
        // 文件末尾续传 0 字节」返回 416 死循环），直接校验；过则落终名，不过则删 tmp 重下。
        if verify_size_sha_blocking(tmp.clone()).await.is_ok() {
            tokio::fs::rename(&tmp, &dest)
                .await
                .map_err(|e| ToolsError::Io(e.to_string()))?;
            return Ok(dest);
        }
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    let resume_from = tokio::fs::metadata(&tmp)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(BTBN_ZIP_SIZE);
    let noop = |_: u64| {};
    download::download_with_fallback(
        &client,
        &[BTBN_ZIP_URL],
        &tmp,
        resume_from,
        BTBN_ZIP_SIZE,
        &noop,
    )
    .await?;
    if let Err(e) = verify_size_sha_blocking(tmp.clone()).await {
        // 校验不过：整包作废，删 .tmp，下次从零重下（不留半信任产物）。
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.into());
    }
    tokio::fs::rename(&tmp, &dest)
        .await
        .map_err(|e| ToolsError::Io(e.to_string()))?;
    Ok(dest)
}

/// 从已下载并整包校验通过的 zip 安装 FFmpeg 工具包到钉死 tag 目录。
/// 幂等：`.ready` 且 bin/ffmpeg.exe、bin/ffprobe.exe 均在则直接返回（与 ffmpeg_tool_status fail-closed 判据镜像）。**阻塞 IO**——调用方须在 `spawn_blocking`/专用阻塞线程内跑
/// （参考 registry.rs/installer.rs 既有做法，本函数与 async 运行时无关）。
pub fn install_ffmpeg_package(
    app_data: &Path,
    downloaded_zip: &Path,
) -> Result<PathBuf, ToolsError> {
    let dir = tag_dir(app_data);
    // 幂等短路判据与 `ffmpeg_tool_status` 镜像（fail-closed）：.ready 在但可执行文件缺失
    // （用户手工删除/磁盘故障）不能证明可用，落入下方重解压（extract 自带清目录，不与残局混杂）。
    if ready_marker(&dir).exists()
        && dir.join("bin").join("ffmpeg.exe").exists()
        && dir.join("bin").join("ffprobe.exe").exists()
    {
        return Ok(dir);
    }
    // zip 整包重验：防跨会话窗口内 zip 被篡改/损坏后不经校验直接解压落盘 DLL（extract 内的逐
    // 文件 hash 只核 exe，不核随包落盘的其余 DLL，故此处补整包重验兜底）。
    download::verify_size_sha(downloaded_zip, BTBN_ZIP_SIZE, Some(BTBN_ZIP_SHA256))?;
    extract_ffmpeg_zip(downloaded_zip, &dir, FFMPEG_EXE_SHA256, FFPROBE_EXE_SHA256)?;
    Ok(dir)
}

/// 解压核心（可测试性拆分：生产常量由 [`install_ffmpeg_package`] 传入；单测传入自造 zip 对应的
/// 期望 hash，不依赖真 ffmpeg 二进制/网络）。
///
/// 步骤：清理 `{dest_dir}.extract.tmp/` 残留 → 建 `{dest_dir}.extract.tmp/bin/` → 只解压 zip
/// 顶层文件夹下 `bin/` 的**扁平**文件（路径先过安全白名单，再 canonicalize 校验前缀落在
/// `bin/` 内，双保险防 zip-slip）→ 确认 ffmpeg.exe/ffprobe.exe 都在 → 逐文件 sha256 复核 →
/// 全过才将 `.extract.tmp/` 整目录同卷 rename 为终名 `dest_dir`，再落 `.ready`（`*.tmp` + 同卷
/// rename，CLAUDE.md 派生文件红线字面）。任一步失败：只删 `.extract.tmp`，终名目录不动。
fn extract_ffmpeg_zip(
    zip_path: &Path,
    dest_dir: &Path,
    ffmpeg_sha256: &str,
    ffprobe_sha256: &str,
) -> Result<(), ToolsError> {
    let tag_name = dest_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("ffmpeg-tag");
    let extract_tmp = dest_dir.with_file_name(format!("{tag_name}.extract.tmp"));
    // 清理可能的上次失败残留：从零装,不与旧内容混杂。
    let _ = std::fs::remove_dir_all(&extract_tmp);
    let bin_dir = extract_tmp.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| ToolsError::Io(e.to_string()))?;
    let canonical_bin =
        std::fs::canonicalize(&bin_dir).map_err(|e| ToolsError::Io(e.to_string()))?;

    let extract_result = (|| -> Result<(), ToolsError> {
        let file = std::fs::File::open(zip_path).map_err(|e| ToolsError::OpenZip(e.to_string()))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| ToolsError::OpenZip(e.to_string()))?;

        let mut saw_ffmpeg_exe = false;
        let mut saw_ffprobe_exe = false;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| ToolsError::Io(e.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let raw_name = entry.name().to_string();
            // 剥离 zip 顶层文件夹前缀（release zip 固定单一顶层目录）；无子路径的顶层文件跳过
            // （非目录 entry 却无 '/' 理论上不会含 bin/ 下内容，安全跳过不解压）。
            let Some((_, rest)) = raw_name.split_once('/') else {
                continue;
            };
            // 只取 bin/ 下的文件（exe + 运行所需 DLL）；doc/include/lib/presets 等不落盘。
            let Some(bin_rel) = rest.strip_prefix("bin/") else {
                continue;
            };
            // 路径净化白名单（installer.rs 同款函数）：拒绝 `..`/绝对/盘符/反斜杠/NUL 等。
            if bin_rel.is_empty() || !crate::exotic::package::is_safe_relative_path(bin_rel) {
                return Err(ToolsError::UnsafeEntry(raw_name));
            }
            // bin/ 下不应再有子目录：出现即视为可疑 entry（从紧,亦规避多级越界组合）。
            if bin_rel.contains('/') {
                return Err(ToolsError::UnsafeEntry(raw_name));
            }
            let out_path = bin_dir.join(bin_rel);
            // zip-slip 二次防线：写入前 canonicalize 父目录并核对前缀落在 `bin/` 内（与模块顶部
            // 注释「前缀落在 `{tag}/bin/` 内」声明一致，entry 终址必须落在 bin/ 内，不放宽到父层）。
            let canon_parent = out_path
                .parent()
                .map(std::fs::canonicalize)
                .transpose()
                .map_err(|e| ToolsError::Io(e.to_string()))?
                .unwrap_or_else(|| canonical_bin.clone());
            if !canon_parent.starts_with(&canonical_bin) {
                return Err(ToolsError::UnsafeEntry(raw_name));
            }
            let mut out =
                std::fs::File::create(&out_path).map_err(|e| ToolsError::Io(e.to_string()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| ToolsError::Io(e.to_string()))?;
            drop(out);
            if bin_rel.eq_ignore_ascii_case("ffmpeg.exe") {
                saw_ffmpeg_exe = true;
            } else if bin_rel.eq_ignore_ascii_case("ffprobe.exe") {
                saw_ffprobe_exe = true;
            }
        }

        if !saw_ffmpeg_exe || !saw_ffprobe_exe {
            return Err(ToolsError::MissingBinary);
        }

        // 逐文件 sha256 二次校验（manifest 常量即真源）。
        let ffmpeg_exe = bin_dir.join("ffmpeg.exe");
        let got_ffmpeg =
            sha256_hex_of_file(&ffmpeg_exe).map_err(|e| ToolsError::Io(e.to_string()))?;
        if !got_ffmpeg.eq_ignore_ascii_case(ffmpeg_sha256) {
            return Err(ToolsError::HashMismatch("ffmpeg.exe".to_string()));
        }
        let ffprobe_exe = bin_dir.join("ffprobe.exe");
        let got_ffprobe =
            sha256_hex_of_file(&ffprobe_exe).map_err(|e| ToolsError::Io(e.to_string()))?;
        if !got_ffprobe.eq_ignore_ascii_case(ffprobe_sha256) {
            return Err(ToolsError::HashMismatch("ffprobe.exe".to_string()));
        }
        Ok(())
    })();

    if let Err(e) = extract_result {
        // 任一步失败：只作废 `.extract.tmp`，终名目录未被触碰，不留半装态。
        let _ = std::fs::remove_dir_all(&extract_tmp);
        return Err(e);
    }

    // 全部落盘+校验通过：先清掉终名目录残留（如有），再将 `.extract.tmp` 整目录同卷 rename 为
    // 终名（CLAUDE.md `*.tmp` + rename 红线）。
    let _ = std::fs::remove_dir_all(dest_dir);
    std::fs::rename(&extract_tmp, dest_dir).map_err(|e| ToolsError::Io(e.to_string()))?;

    // 全过才落 ready 标记（*.tmp + 同卷 rename）。
    let ready = ready_marker(dest_dir);
    let ready_tmp = dest_dir.join(".ready.tmp");
    std::fs::write(&ready_tmp, b"ready").map_err(|e| ToolsError::Io(e.to_string()))?;
    std::fs::rename(&ready_tmp, &ready).map_err(|e| ToolsError::Io(e.to_string()))?;
    Ok(())
}

/// 启动清扫（boot sweep）：清理 `tools/ffmpeg/` 下的残留：
///   - 当前 tag（[`BTBN_RELEASE_TAG`]）的下载中 `{tag}.zip.tmp` **豁免**（保跨重启断点续传，
///     其完整性由 [`download_ffmpeg_package`] 的 resume+verify 兜底），其余 `*.tmp` 文件照删；
///   - 非当前 tag 的目录（含旧 tag 的已就绪目录、本轮解压残留的 `{tag}.extract.tmp` 中间目录）
///     整体清除——旧 tag 无消费方，升级残留回收；当前 tag 但无 `.ready` 标记的半装目录（中断的
///     解压）同样清除；
///   - 非当前 tag 的 `*.zip` 整包（升级残留）清除，当前 tag 的 zip 留档供 `install` 复用。
///
/// 不知从哪一步中断，整体清扫不做部分保留。目录不存在视为无需清扫（首次运行）。
pub fn boot_sweep(app_data: &Path) -> std::io::Result<()> {
    let root = tools_root(app_data);
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let current_zip_tmp_name = format!("{BTBN_RELEASE_TAG}.zip.tmp");
    let current_zip_name = format!("{BTBN_RELEASE_TAG}.zip");
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            if file_name == current_zip_tmp_name {
                // 当前 tag 下载中 zip：豁免，跨重启断点续传（第 3 项）。
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("tmp") {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if ext == Some("zip") && file_name != current_zip_name {
                // 非当前 tag 的 zip 整包（升级残留，旧 tag 无消费方）→ 清除。
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        if file_type.is_dir() {
            if file_name != BTBN_RELEASE_TAG {
                // 非当前 tag 的目录 → 整体清除：升级残留 tag 目录（无论是否曾 ready）与本轮解压
                // 残留的 `{tag}.extract.tmp` 中间目录（第 4 项引入）均落在此分支。
                let _ = std::fs::remove_dir_all(&path);
                continue;
            }
            if !ready_marker(&path).exists() {
                // 当前 tag 但无 ready 标记（中断的解压）→ 清除。
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::hash::sha256_hex;
    use std::io::Write as _;

    /// 构造一个「BtbN release zip」形态的测试 zip：单一顶层文件夹 + `bin/` 下若干文件。
    /// `entries`：(zip 内 bin/ 后的相对路径, 内容)。`prefix` 为顶层文件夹名。
    fn build_zip(prefix: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (rel, content) in entries {
                zip.start_file(format!("{prefix}/bin/{rel}"), opt).unwrap();
                zip.write_all(content).unwrap();
            }
            zip.finish().unwrap();
        }
        let path = std::env::temp_dir().join(format!(
            "exotic-tools-test-{}-{}.zip",
            prefix.replace(['/', '\\'], "_"),
            std::process::id()
        ));
        std::fs::write(&path, &buf).unwrap();
        path
    }

    fn tmp_dest(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("exotic-tools-dest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// `extract_ffmpeg_zip` 内部使用的 `{dest_dir}.extract.tmp` 中间目录路径（第 4 项引入），
    /// 测试用以断言成功/失败两路均不留该残留。
    fn extract_tmp_of(dest_dir: &Path) -> PathBuf {
        let name = dest_dir.file_name().and_then(|s| s.to_str()).unwrap();
        dest_dir.with_file_name(format!("{name}.extract.tmp"))
    }

    #[test]
    fn good_zip_installs_and_marks_ready() {
        let ffmpeg_bytes = b"FAKE-FFMPEG-EXE";
        let ffprobe_bytes = b"FAKE-FFPROBE-EXE";
        let zip = build_zip(
            "ffmpeg-fake",
            &[
                ("ffmpeg.exe", ffmpeg_bytes),
                ("ffprobe.exe", ffprobe_bytes),
                ("avcodec-61.dll", b"DLL"),
            ],
        );
        let dest = tmp_dest("good");
        let r = extract_ffmpeg_zip(
            &zip,
            &dest,
            &sha256_hex(ffmpeg_bytes),
            &sha256_hex(ffprobe_bytes),
        );
        assert!(r.is_ok(), "{r:?}");
        assert!(dest.join(".ready").exists());
        assert!(dest.join("bin/ffmpeg.exe").exists());
        assert!(dest.join("bin/ffprobe.exe").exists());
        assert!(dest.join("bin/avcodec-61.dll").exists());
        assert!(
            !extract_tmp_of(&dest).exists(),
            "成功后 .extract.tmp 应已 rename 走，不留残留"
        );
        let _ = std::fs::remove_file(&zip);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn bad_hash_rejected_and_dir_discarded() {
        let zip = build_zip(
            "ffmpeg-badhash",
            &[("ffmpeg.exe", b"REAL"), ("ffprobe.exe", b"REAL2")],
        );
        let dest = tmp_dest("badhash");
        let r = extract_ffmpeg_zip(&zip, &dest, &"a".repeat(64), &"b".repeat(64));
        assert!(matches!(r, Err(ToolsError::HashMismatch(_))), "{r:?}");
        assert!(!dest.exists(), "hash 不符应整个目录作废");
        assert!(
            !extract_tmp_of(&dest).exists(),
            "hash 不符应连 .extract.tmp 一并作废"
        );
        let _ = std::fs::remove_file(&zip);
    }

    #[test]
    fn zip_slip_backslash_entry_rejected() {
        // 直接构造含 `..\` 的恶意 entry 名（越过 bin/ 前缀剥离后仍需被路径净化拦下）。
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("ffmpeg-evil/bin/..\\..\\evil.exe", opt)
                .unwrap();
            zip.write_all(b"EVIL").unwrap();
            zip.start_file("ffmpeg-evil/bin/ffmpeg.exe", opt).unwrap();
            zip.write_all(b"REAL").unwrap();
            zip.start_file("ffmpeg-evil/bin/ffprobe.exe", opt).unwrap();
            zip.write_all(b"REAL2").unwrap();
            zip.finish().unwrap();
        }
        let zip_path =
            std::env::temp_dir().join(format!("exotic-tools-evil-{}.zip", std::process::id()));
        std::fs::write(&zip_path, &buf).unwrap();
        let dest = tmp_dest("evil");
        let r = extract_ffmpeg_zip(
            &zip_path,
            &dest,
            &sha256_hex(b"REAL"),
            &sha256_hex(b"REAL2"),
        );
        assert!(matches!(r, Err(ToolsError::UnsafeEntry(_))), "{r:?}");
        assert!(!dest.exists(), "越界 entry 应整个目录作废，不留半装");
        assert!(
            !extract_tmp_of(&dest).exists(),
            "越界 entry 应连 .extract.tmp 一并作废"
        );
        // 确认越界目标本身未被写出（校验入口目录不存在即可，已在上面断言；这里再确认无逃逸文件）。
        let escaped = std::env::temp_dir().join("evil.exe");
        assert!(!escaped.exists());
        let _ = std::fs::remove_file(&zip_path);
    }

    #[test]
    fn zip_entry_with_subdirectory_under_bin_rejected() {
        // zip 内 `top/bin/sub/x.dll` 子目录 entry：bin/ 下不应再有子目录，视为可疑 entry 拒收，
        // 目录整体作废（不因子目录形态放松第 4/5 项引入的前缀/落盘校验）。
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("ffmpeg-subdir/bin/sub/x.dll", opt).unwrap();
            zip.write_all(b"SUB").unwrap();
            zip.start_file("ffmpeg-subdir/bin/ffmpeg.exe", opt).unwrap();
            zip.write_all(b"REAL").unwrap();
            zip.start_file("ffmpeg-subdir/bin/ffprobe.exe", opt)
                .unwrap();
            zip.write_all(b"REAL2").unwrap();
            zip.finish().unwrap();
        }
        let zip_path =
            std::env::temp_dir().join(format!("exotic-tools-subdir-{}.zip", std::process::id()));
        std::fs::write(&zip_path, &buf).unwrap();
        let dest = tmp_dest("subdir");
        let r = extract_ffmpeg_zip(
            &zip_path,
            &dest,
            &sha256_hex(b"REAL"),
            &sha256_hex(b"REAL2"),
        );
        assert!(matches!(r, Err(ToolsError::UnsafeEntry(_))), "{r:?}");
        assert!(!dest.exists(), "子目录 entry 应整个目录作废");
        assert!(
            !extract_tmp_of(&dest).exists(),
            "子目录 entry 应连 .extract.tmp 一并作废"
        );
        let _ = std::fs::remove_file(&zip_path);
    }

    #[test]
    fn missing_binary_in_zip_rejected() {
        // zip 里只有 ffmpeg.exe，缺 ffprobe.exe → MissingBinary，目录作废。
        let zip = build_zip("ffmpeg-missing", &[("ffmpeg.exe", b"REAL")]);
        let dest = tmp_dest("missing");
        let r = extract_ffmpeg_zip(&zip, &dest, &sha256_hex(b"REAL"), &"b".repeat(64));
        assert!(matches!(r, Err(ToolsError::MissingBinary)), "{r:?}");
        assert!(!dest.exists());
        assert!(!extract_tmp_of(&dest).exists());
        let _ = std::fs::remove_file(&zip);
    }

    #[test]
    fn status_ready_marker_absence_means_not_ready() {
        // 目录存在、可执行文件都在，但无 .ready 标记 → 视为未就绪（不误报 Ready）。
        let app_data = tmp_dest("status-no-marker");
        let dir = tag_dir(&app_data);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/ffmpeg.exe"), b"x").unwrap();
        std::fs::write(dir.join("bin/ffprobe.exe"), b"x").unwrap();
        assert_eq!(ffmpeg_tool_status(&app_data), ToolStatus::NotDownloaded);
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn status_ready_with_binaries_is_ready() {
        let app_data = tmp_dest("status-ready");
        let dir = tag_dir(&app_data);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/ffmpeg.exe"), b"x").unwrap();
        std::fs::write(dir.join("bin/ffprobe.exe"), b"x").unwrap();
        std::fs::write(dir.join(".ready"), b"ready").unwrap();
        match ffmpeg_tool_status(&app_data) {
            ToolStatus::Ready {
                ffmpeg_exe,
                ffprobe_exe,
            } => {
                assert!(
                    ffmpeg_exe.ends_with("bin/ffmpeg.exe")
                        || ffmpeg_exe.ends_with("bin\\ffmpeg.exe")
                );
                assert!(ffprobe_exe.exists());
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn status_tmp_zip_means_downloading() {
        let app_data = tmp_dest("status-downloading");
        let root = tools_root(&app_data);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(zip_tmp_path(&app_data), b"partial").unwrap();
        assert_eq!(ffmpeg_tool_status(&app_data), ToolStatus::Downloading);
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn status_empty_is_not_downloaded() {
        let app_data = tmp_dest("status-empty");
        assert_eq!(ffmpeg_tool_status(&app_data), ToolStatus::NotDownloaded);
    }

    #[test]
    fn boot_sweep_clears_tmp_and_stale_tags_keeps_current_ready_tag_and_resume_tmp() {
        let app_data = tmp_dest("sweep");
        let root = tools_root(&app_data);
        std::fs::create_dir_all(&root).unwrap();
        // 非当前 tag 的 .tmp 残留 → 应清扫。
        std::fs::write(root.join("stale.zip.tmp"), b"partial").unwrap();
        // 半装 tag 目录（无 .ready）→ 应清扫。
        let incomplete = root.join("incomplete-tag");
        std::fs::create_dir_all(incomplete.join("bin")).unwrap();
        std::fs::write(incomplete.join("bin/ffmpeg.exe"), b"x").unwrap();
        // 当前 tag 的已就绪目录 → 应保留。
        let ready_dir = root.join(BTBN_RELEASE_TAG);
        std::fs::create_dir_all(ready_dir.join("bin")).unwrap();
        std::fs::write(ready_dir.join(".ready"), b"ready").unwrap();
        // 旧 tag 的已就绪目录（升级残留，无消费方）→ 应清扫，即便曾 ready。
        let stale_ready_dir = root.join("stale-old-tag");
        std::fs::create_dir_all(stale_ready_dir.join("bin")).unwrap();
        std::fs::write(stale_ready_dir.join(".ready"), b"ready").unwrap();
        // 本轮解压中断残留的 `.extract.tmp` 中间目录 → 应清扫。
        let extract_tmp = root.join(format!("{BTBN_RELEASE_TAG}.extract.tmp"));
        std::fs::create_dir_all(extract_tmp.join("bin")).unwrap();
        // 旧 tag 的 zip 整包（升级残留）→ 应清扫。
        let stale_zip = root.join("stale-old-tag.zip");
        std::fs::write(&stale_zip, b"zip").unwrap();
        // 当前 tag 的 zip 整包 → 应保留（供 install 复用）。
        let current_zip = root.join(format!("{BTBN_RELEASE_TAG}.zip"));
        std::fs::write(&current_zip, b"zip").unwrap();
        // 当前 tag 的下载中 .zip.tmp → 应豁免清扫（保跨重启断点续传，第 3/6 项）。
        let current_zip_tmp = root.join(format!("{BTBN_RELEASE_TAG}.zip.tmp"));
        std::fs::write(&current_zip_tmp, b"partial").unwrap();

        boot_sweep(&app_data).unwrap();

        assert!(
            !root.join("stale.zip.tmp").exists(),
            "非当前 tag 的 .tmp 应被清扫"
        );
        assert!(!incomplete.exists(), "无 ready 标记的半装目录应被清扫");
        assert!(ready_dir.exists(), "当前 tag 的已就绪目录不应被清扫");
        assert!(ready_dir.join(".ready").exists());
        assert!(
            !stale_ready_dir.exists(),
            "旧 tag 的已就绪目录应被清扫（升级残留回收）"
        );
        assert!(!extract_tmp.exists(), "`.extract.tmp` 中间目录残留应被清扫");
        assert!(!stale_zip.exists(), "旧 tag 的 zip 整包应被清扫");
        assert!(current_zip.exists(), "当前 tag 的 zip 整包应保留");
        assert!(
            current_zip_tmp.exists(),
            "当前 tag 的 .zip.tmp 应豁免清扫（断点续传兜底）"
        );
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn boot_sweep_missing_root_is_noop() {
        let app_data = tmp_dest("sweep-noop");
        // tools_root 不存在（从未下载过）：boot_sweep 不应报错。
        assert!(boot_sweep(&app_data).is_ok());
    }

    #[test]
    fn install_ffmpeg_package_idempotent_when_already_ready() {
        let app_data = tmp_dest("install-idem");
        let dir = tag_dir(&app_data);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        // .ready 且两枚可执行文件均在，短路判据（镜像 ffmpeg_tool_status）才成立。
        std::fs::write(dir.join("bin/ffmpeg.exe"), b"x").unwrap();
        std::fs::write(dir.join("bin/ffprobe.exe"), b"x").unwrap();
        std::fs::write(dir.join(".ready"), b"ready").unwrap();
        // 传入不存在的 zip 路径——已 ready 且可执行文件齐全时应直接短路返回，不去读 zip。
        let bogus_zip = app_data.join("does-not-exist.zip");
        let r = install_ffmpeg_package(&app_data, &bogus_zip);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(r.unwrap(), dir);
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn install_ffmpeg_package_reinstalls_when_binary_missing_despite_ready() {
        // `.ready` 在但 ffmpeg.exe 缺失（用户手工删除/磁盘故障）：不能短路，须落入重解压路径。
        let app_data = tmp_dest("install-missing-exe");
        let dir = tag_dir(&app_data);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(".ready"), b"ready").unwrap();
        // ffprobe.exe 存在但 ffmpeg.exe 缺失——任一缺失都不应短路。
        std::fs::write(dir.join("bin/ffprobe.exe"), b"x").unwrap();
        let bogus_zip = app_data.join("does-not-exist.zip");
        let r = install_ffmpeg_package(&app_data, &bogus_zip);
        // 未走短路的证据：真的尝试了 zip 整包重验，bogus_zip 不存在 → 校验失败（Download 错误链），
        // 而非静默返回 Ok。
        assert!(matches!(r, Err(ToolsError::Download(_))), "{r:?}");
        let _ = std::fs::remove_dir_all(&app_data);
    }
}
