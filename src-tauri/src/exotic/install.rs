// src-tauri/src/exotic/install.rs
//! 冷门格式插件 · 安全解包与完整性校验（v3 Part3 §6.4 第 2-8 步）。
//!
//! 把一个**不可信** zip 包安全地校验并解压到 staging 目录。原子切换/备份/回滚/Coordinator
//! quiesce/DB 更新（§6.4 第 9-11 步）属安装编排，见 [`crate::exotic`] 后续命令层；本模块只做
//! 安全关键的「验签先于解包 + 白名单复核 + zip 加固 + 逐文件 hash 复核」。
//!
//! 防线（§6.4）：
//!   1. 只先有界读取 `package-manifest.json` + `.sig` 并验签（release key）——其余 entry 一律不信。
//!   2. **清单即白名单**：zip 文件 entry 集合必须恰好 = 已签名 files ∪ 两份签名元数据；
//!      多/缺文件、大小写碰撞、重复路径全拒。
//!   3. **路径净化**（[`crate::exotic::package::is_safe_relative_path`]）：拒绝 `..`/绝对/盘符/UNC/
//!      反斜杠/NUL/保留设备名；拒绝符号链接（unix mode S_IFLNK）。
//!   4. **zip bomb 上限**：单文件解压大小、总解压大小、文件数、单文件压缩比。
//!   5. 解压用安全相对路径 + `create_new`（不跟随既有/符号链接），边写边算 sha256/size 复核。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::exotic::crypto::VerifyingKeyset;
use crate::exotic::package::{
    is_safe_relative_path, verify_manifest, PackageError, PackageManifest,
};

/// 两份签名元数据文件名（不在 manifest.files 中，但 zip 必含）。
const MANIFEST_NAME: &str = "package-manifest.json";
const MANIFEST_SIG_NAME: &str = "package-manifest.sig";

/// zip bomb 防护上限。
#[derive(Debug, Clone)]
pub struct InstallLimits {
    /// 单文件解压大小上限。
    pub max_file_size: u64,
    /// 全部文件解压总大小上限。
    pub max_total_size: u64,
    /// entry 数上限。
    pub max_files: usize,
    /// 单文件压缩比上限（解压/压缩；防高压缩比炸弹）。压缩尺寸为 0 时按解压尺寸判定。
    pub max_ratio: u64,
}

impl Default for InstallLimits {
    fn default() -> Self {
        InstallLimits {
            max_file_size: 256 * 1024 * 1024,
            max_total_size: 512 * 1024 * 1024,
            max_files: 4096,
            max_ratio: 200,
        }
    }
}

/// 期望值（来自已验签 Registry 条目；安装命令传入，绝不来自前端）。
pub struct RegistryExpect<'a> {
    pub plugin_id: &'a str,
    pub version: &'a str,
    pub target: &'a str,
    pub package_sequence: i64,
}

/// 安全解包/校验错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InstallError {
    #[error("打开 zip 失败：{0}")]
    OpenZip(String),
    #[error("zip 缺少签名元数据：{0}")]
    MissingMeta(&'static str),
    #[error("manifest 校验失败：{0}")]
    Manifest(#[from] PackageError),
    #[error("Host 版本不满足 min_host_version")]
    IncompatibleHost,
    #[error("协议版本不兼容：包 {pkg} != host {host}")]
    ProtocolMismatch { pkg: u16, host: u16 },
    #[error("zip entry 路径非法：{0}")]
    UnsafeEntry(String),
    #[error("zip 含符号链接：{0}")]
    Symlink(String),
    #[error("zip 含清单外的额外文件：{0}")]
    ExtraFile(String),
    #[error("zip 缺少清单声明的文件：{0}")]
    MissingFile(String),
    #[error("大小写碰撞路径：{0}")]
    CaseCollision(String),
    #[error("超出 zip 上限：{0}")]
    LimitExceeded(&'static str),
    #[error("文件大小/hash 与清单不符：{0}")]
    HashMismatch(String),
    #[error("解包 IO 失败：{0}")]
    Io(String),
    // ── 编排层（installer.rs，§6.4 第 8/10 步）──────────────────────────────
    #[error("plugin manifest 的 formats/capabilities 非 Catalog 子集：{0}")]
    CatalogReject(String),
    #[error("安装记录写入失败：{0}")]
    Db(String),
    /// T13 多渠道预留:Steam/Store 渠道的安装路径(跳 Registry 验签、保 manifest/hash
    /// 复核)随 Part8 实装;实装前 fail-closed,不预铺无测试保护的弱验签分支。
    #[error("安装渠道未实装：{0}（Steam/Store 随 Part8 落地）")]
    ChannelUnsupported(&'static str),
}

impl InstallError {
    /// 稳定错误码（R1-4，跨 IPC 边界；前端按 code 分支处理——如区分 zip 损坏（open_zip）/
    /// 签名失败（bad_signature）/ 磁盘满（install_io），取代原命令层的泛码 "install_failed"）。
    /// `Manifest` 委托 [`PackageError::code`]（bad_signature/parse/registry_mismatch/…），
    /// 使 manifest 层粒度直接透出，不再折叠。码集由锁测试钉死，改名即破坏性契约变更。
    pub fn code(&self) -> &'static str {
        match self {
            InstallError::OpenZip(_) => "open_zip",
            InstallError::MissingMeta(_) => "missing_meta",
            InstallError::Manifest(e) => e.code(),
            InstallError::IncompatibleHost => "incompatible_host",
            InstallError::ProtocolMismatch { .. } => "protocol_mismatch",
            InstallError::UnsafeEntry(_) => "unsafe_entry",
            InstallError::Symlink(_) => "symlink",
            InstallError::ExtraFile(_) => "extra_file",
            InstallError::MissingFile(_) => "missing_file",
            InstallError::CaseCollision(_) => "case_collision",
            InstallError::LimitExceeded(_) => "limit_exceeded",
            InstallError::HashMismatch(_) => "hash_mismatch",
            InstallError::Io(_) => "install_io",
            InstallError::CatalogReject(_) => "catalog_reject",
            InstallError::Db(_) => "install_db",
            InstallError::ChannelUnsupported(_) => "channel_unsupported",
        }
    }
}

/// 解包成功结果。
#[derive(Debug)]
pub struct ExtractedPackage {
    /// staging 目录（已写入、已复核全部 payload 文件）。
    pub dir: PathBuf,
    pub manifest: PackageManifest,
}

/// unix mode 是否为符号链接（S_IFLNK）。
fn is_symlink_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o120000
}

/// 校验 + 解压（§6.4 第 2-8 步）。zip 为不可信输入；任一检查失败即整体拒绝，不留半文件。
pub fn verify_and_extract(
    zip_path: &Path,
    keyset: &VerifyingKeyset,
    expect: &RegistryExpect<'_>,
    host_version: &str,
    now: i64,
    staging_dir: &Path,
    limits: &InstallLimits,
) -> Result<ExtractedPackage, InstallError> {
    let file = std::fs::File::open(zip_path).map_err(|e| InstallError::OpenZip(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| InstallError::OpenZip(e.to_string()))?;

    // ── 1. 有界读取 manifest + sig 并验签（其余 entry 一律不信）──────────────────
    let manifest_bytes = read_meta(&mut archive, MANIFEST_NAME)?;
    let sig_bytes = read_meta(&mut archive, MANIFEST_SIG_NAME)?;
    let manifest = verify_manifest(&manifest_bytes, &sig_bytes, keyset, now)?;

    // ── 2. manifest 与 Registry 期望交叉核对 + Host/协议兼容 ─────────────────────
    manifest.check_matches_registry(
        expect.plugin_id,
        expect.version,
        expect.target,
        expect.package_sequence,
    )?;
    if !crate::exotic::host_meets_min(&manifest.min_host_version, host_version) {
        return Err(InstallError::IncompatibleHost);
    }
    let host_proto = exotic_protocol::PROTOCOL_VERSION;
    if manifest.protocol_version != host_proto {
        return Err(InstallError::ProtocolMismatch {
            pkg: manifest.protocol_version,
            host: host_proto,
        });
    }

    // ── 3. 扫描中央目录：清单即白名单，逐 entry 路径/符号链接/上限校验 ───────────
    let whitelist = manifest.file_paths();
    let entry_count = archive.len();
    if entry_count > limits.max_files {
        return Err(InstallError::LimitExceeded("文件数"));
    }
    let mut seen_files: HashSet<String> = HashSet::new(); // 实际 zip 中的 payload 文件（小写去碰撞）
    let mut total_uncompressed: u64 = 0;
    for i in 0..entry_count {
        let f = archive
            .by_index(i)
            .map_err(|e| InstallError::Io(e.to_string()))?;
        let raw_name = f.name().to_string();
        // 符号链接（即便名字安全也拒）。
        if let Some(mode) = f.unix_mode() {
            if is_symlink_mode(mode) {
                return Err(InstallError::Symlink(raw_name));
            }
        }
        if f.is_dir() {
            // 目录 entry：仅校验名字安全（去尾斜杠），不计入文件白名单。
            let trimmed = raw_name.trim_end_matches('/');
            if !trimmed.is_empty() && !is_safe_relative_path(trimmed) {
                return Err(InstallError::UnsafeEntry(raw_name));
            }
            continue;
        }
        // 文件 entry：路径必须安全。
        if !is_safe_relative_path(&raw_name) {
            return Err(InstallError::UnsafeEntry(raw_name));
        }
        // 两份签名元数据**只能作为有界读取源**，不允许 zip 内含同名文件 entry 落地（由代码写入
        // 已验签 bytes，§6.4；安全评审：堵住 ExtraFile 对元数据名的隐式豁免盲点）。read_meta 读的是
        // 同名 entry 的内容用于验签——验签通过后此处仍拒绝把它当 payload 解压/计入白名单。
        if raw_name == MANIFEST_NAME || raw_name == MANIFEST_SIG_NAME {
            // 已被 read_meta 用于验签；不计入 payload，也不重复解压。跳过即可（非 ExtraFile：
            // 它本就是合法的元数据 entry，且不在 manifest.files 白名单内、不会被解压循环触及）。
            continue;
        }
        // 必须在清单白名单内（多余文件拒）。
        if !whitelist.contains(raw_name.as_str()) {
            return Err(InstallError::ExtraFile(raw_name));
        }
        // 大小写碰撞（Windows 大小写不敏感盘）。
        let lower = raw_name.to_ascii_lowercase();
        if !seen_files.insert(lower) {
            return Err(InstallError::CaseCollision(raw_name));
        }
        // zip bomb：单文件解压大小 + 压缩比。
        let usize_uncompressed = f.size();
        if usize_uncompressed > limits.max_file_size {
            return Err(InstallError::LimitExceeded("单文件解压大小"));
        }
        let comp = f.compressed_size().max(1);
        if usize_uncompressed / comp > limits.max_ratio {
            return Err(InstallError::LimitExceeded("压缩比"));
        }
        total_uncompressed = total_uncompressed
            .checked_add(usize_uncompressed)
            .ok_or(InstallError::LimitExceeded("总解压大小溢出"))?;
        if total_uncompressed > limits.max_total_size {
            return Err(InstallError::LimitExceeded("总解压大小"));
        }
    }
    // 清单声明的文件必须都在 zip 里（缺文件拒）。
    for path in &whitelist {
        if !seen_files.contains(&path.to_ascii_lowercase()) {
            return Err(InstallError::MissingFile((*path).to_string()));
        }
    }

    // ── 4. 解压到 staging：安全 join + create_new + 边写边复核 hash/size ─────────
    // 失败即清理整个 staging（不留半装产物）。
    let result = (|| -> Result<(), InstallError> {
        std::fs::create_dir_all(staging_dir).map_err(|e| InstallError::Io(e.to_string()))?;
        for fmeta in &manifest.files {
            let dest = safe_join(staging_dir, &fmeta.path)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| InstallError::Io(e.to_string()))?;
            }
            let mut zf = archive
                .by_name(&fmeta.path)
                .map_err(|e| InstallError::Io(e.to_string()))?;
            // create_new：拒绝跟随既有文件/符号链接（防解压前被布置的链接逃逸）。
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
                .map_err(|e| InstallError::Io(format!("{}: {e}", fmeta.path)))?;
            let (size, hex) = copy_hashing(&mut zf, &mut out, fmeta.size, limits.max_file_size)?;
            if size != fmeta.size || hex != fmeta.sha256 {
                return Err(InstallError::HashMismatch(fmeta.path.clone()));
            }
            // 解压 symlink 安全性（安全评审 high：unix_mode 在 Windows 恒 None，早检查失效）：
            // 解压**绝不创建符号链接**——extract 目录每次 install 前 remove_dir_all 重建(无预置 junction)，
            // 仅以 create_new 写普通文件(拒跟随既有/链接)，且只写 manifest.files 内安全相对路径。
            // 下方落地后再核 symlink_metadata 作纵深兜底：若意外为链接(理论不可达)即拒。
            let ft = std::fs::symlink_metadata(&dest)
                .map_err(|e| InstallError::Io(e.to_string()))?
                .file_type();
            if ft.is_symlink() {
                return Err(InstallError::Symlink(fmeta.path.clone()));
            }
            #[cfg(unix)]
            if fmeta.executable {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
        }
        // 同时落地已验签的 manifest + sig，使安装目录**自包含**——修复（§6.5）可重新验签 + 复核
        // 全文件 hash，无需原始 zip。两份元数据由本函数信任来源(已验签)，直接写入。
        std::fs::write(staging_dir.join(MANIFEST_NAME), &manifest_bytes)
            .map_err(|e| InstallError::Io(e.to_string()))?;
        std::fs::write(staging_dir.join(MANIFEST_SIG_NAME), &sig_bytes)
            .map_err(|e| InstallError::Io(e.to_string()))?;
        Ok(())
    })();

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(staging_dir);
        return Err(e);
    }

    Ok(ExtractedPackage {
        dir: staging_dir.to_path_buf(),
        manifest,
    })
}

/// 有界读取签名元数据 entry（缺失/超长即错）。
fn read_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &'static str,
) -> Result<Vec<u8>, InstallError> {
    let mut f = match archive.by_name(name) {
        Ok(f) => f,
        Err(_) => return Err(InstallError::MissingMeta(name)),
    };
    // 元数据本身有界（manifest ≤ 1 MiB，sig 64 B）。
    if f.size() > 1024 * 1024 {
        return Err(InstallError::LimitExceeded("元数据过大"));
    }
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf)
        .map_err(|e| InstallError::Io(e.to_string()))?;
    Ok(buf)
}

/// 把 staging_dir 与**已校验安全**的相对路径 join，并二次确认结果仍在 staging_dir 下。
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, InstallError> {
    if !is_safe_relative_path(rel) {
        return Err(InstallError::UnsafeEntry(rel.to_string()));
    }
    let mut p = base.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    // 纵深防御:即便 is_safe_relative_path 已过,仍显式确认 join 结果未逃出 base。
    if !p.starts_with(base) {
        return Err(InstallError::UnsafeEntry(rel.to_string()));
    }
    Ok(p)
}

/// 边复制边算 sha256/size，并在写入阶段再次封顶单文件大小（防声明小、实际大）。
fn copy_hashing(
    src: &mut impl Read,
    dst: &mut impl std::io::Write,
    _declared: u64,
    cap: u64,
) -> Result<(u64, String), InstallError> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    let mut total: u64 = 0;
    loop {
        let n = src
            .read(&mut buf)
            .map_err(|e| InstallError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(InstallError::LimitExceeded("文件大小溢出"))?;
        if total > cap {
            return Err(InstallError::LimitExceeded("解压时单文件超限"));
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n])
            .map_err(|e| InstallError::Io(e.to_string()))?;
    }
    let hex = crate::utils::hash::to_hex_lower(&hasher.finalize());
    Ok((total, hex))
}

// ── 原子安装切换 / 备份 / 回滚（§6.4 第 9-11 步的纯函数核心）───────────────────────
//
// 前置（调用方负责，见 P6.4 命令层）：Coordinator quiesce 该 plugin、kill/wait 全部 Worker、
// 释放 Windows 文件句柄——否则被占用目录无法改名。本层只做目录级原子切换与回滚，可离线测试。

/// 插件安装根目录：**只**用已验证 plugin_id 拼接，绝不接受前端原始输入（§6.4 末段）。
/// plugin_id 非法（非 `[a-z0-9-]{1,64}`）→ None，拒绝路径注入。
pub fn plugin_install_dir(base: &Path, plugin_id: &str) -> Option<PathBuf> {
    let len = plugin_id.len();
    let ok = (1..=64).contains(&len)
        && plugin_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Some(base.join(plugin_id))
    } else {
        None
    }
}

/// 原子安装切换（§6.4 第 10 步）：旧 `current` → `backup`（保留供回滚），`staging` → `current`。
/// 第二步失败时把 backup 还原回 current，保证不留下「无 current」的破损态。
pub fn commit_install(current: &Path, staging: &Path, backup: &Path) -> Result<(), InstallError> {
    let io = |e: std::io::Error| InstallError::Io(e.to_string());
    if let Some(parent) = current.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    // 清理陈旧 backup（上次失败遗留）。
    if backup.exists() {
        std::fs::remove_dir_all(backup).map_err(io)?;
    }
    // 旧版本让位到 backup。
    let had_current = current.exists();
    if had_current {
        std::fs::rename(current, backup).map_err(io)?;
    }
    // 新版本就位；失败则还原 backup。
    if let Err(e) = std::fs::rename(staging, current) {
        if had_current {
            let _ = std::fs::rename(backup, current); // 尽力还原
        }
        return Err(InstallError::Io(e.to_string()));
    }
    Ok(())
}

/// 安装/健康检查失败回滚（§6.4 第 11 步）：丢弃 current，把 backup 还原回 current。
pub fn rollback_to_backup(current: &Path, backup: &Path) -> Result<(), InstallError> {
    let io = |e: std::io::Error| InstallError::Io(e.to_string());
    if !backup.exists() {
        return Err(InstallError::Io("无可回滚的 backup".into()));
    }
    if current.exists() {
        std::fs::remove_dir_all(current).map_err(io)?;
    }
    std::fs::rename(backup, current).map_err(io)?;
    Ok(())
}

/// 安装成功后丢弃 backup（产品策略可改为保留有限期；此为即时清理）。
pub fn discard_backup(backup: &Path) -> Result<(), InstallError> {
    if backup.exists() {
        std::fs::remove_dir_all(backup).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::crypto::test_support::{keyset_json, sign, signing_key, KeySpec};
    use std::io::{Cursor, Write as _};

    const NOW: i64 = 1_790_000_000;
    const TARGET: &str = "x86_64-pc-windows-msvc";

    fn release_keyset(sk: &ring::signature::Ed25519KeyPair) -> VerifyingKeyset {
        let json = keyset_json(&[KeySpec {
            key_id: "release-test",
            purpose: "release",
            sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        VerifyingKeyset::parse(&json).unwrap()
    }

    fn sha_hex(b: &[u8]) -> String {
        crate::utils::hash::sha256_hex(b)
    }

    /// 构造 manifest JSON（依据给定 payload 文件计算 hash），protocol = host 当前协议。
    fn manifest_json(files: &[(&str, &[u8], bool)]) -> String {
        let entries: Vec<String> = files
            .iter()
            .map(|(p, c, exe)| {
                format!(
                    r#"{{"path":"{p}","size":{},"sha256":"{}","kind":"file","executable":{exe}}}"#,
                    c.len(),
                    sha_hex(c)
                )
            })
            .collect();
        format!(
            r#"{{"schema":1,"key_id":"release-test","plugin_id":"exotic-image-psd",
              "version":"1.0.0","package_sequence":3,"target":"{TARGET}",
              "min_host_version":"0.1.0","protocol_version":{proto},
              "compliance_review_id":"review-1","files":[{}]}}"#,
            entries.join(","),
            proto = exotic_protocol::PROTOCOL_VERSION
        )
    }

    /// 写一个 zip 到临时文件，返回路径。`extra` 为 (name, content, is_symlink) 额外 entry。
    fn write_zip(
        name: &str,
        manifest_bytes: &[u8],
        sig: &[u8],
        payload: &[(&str, &[u8], bool)],
        extra: &[(&str, &[u8])],
    ) -> PathBuf {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(MANIFEST_NAME, stored).unwrap();
            zip.write_all(manifest_bytes).unwrap();
            zip.start_file(MANIFEST_SIG_NAME, stored).unwrap();
            zip.write_all(sig).unwrap();
            for (p, c, _exe) in payload {
                zip.start_file(*p, stored).unwrap();
                zip.write_all(c).unwrap();
            }
            for (p, c) in extra {
                zip.start_file(*p, stored).unwrap();
                zip.write_all(c).unwrap();
            }
            zip.finish().unwrap();
        }
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, &buf).unwrap();
        path
    }

    fn expect() -> RegistryExpect<'static> {
        RegistryExpect {
            plugin_id: "exotic-image-psd",
            version: "1.0.0",
            target: TARGET,
            package_sequence: 3,
        }
    }

    fn unique_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("exotic-inst-{tag}-{}", std::process::id()))
    }

    #[test]
    fn valid_package_extracts_and_verifies() {
        let sk = signing_key(1);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[
            ("bin/psd-worker.exe", b"WORKER-BINARY", true),
            ("plugin-manifest.json", b"{\"x\":1}", false),
        ];
        let mj = manifest_json(payload);
        let sig = sign(&sk, mj.as_bytes());
        let zip = write_zip("valid.zip", mj.as_bytes(), &sig, payload, &[]);
        let dir = unique_dir("ok");
        let _ = std::fs::remove_dir_all(&dir);

        let out = verify_and_extract(
            &zip,
            &ks,
            &expect(),
            "0.1.0",
            NOW,
            &dir,
            &InstallLimits::default(),
        )
        .unwrap();
        assert_eq!(out.manifest.files.len(), 2);
        assert_eq!(
            std::fs::read(dir.join("bin/psd-worker.exe")).unwrap(),
            b"WORKER-BINARY"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extra_file_rejected() {
        let sk = signing_key(2);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let mj = manifest_json(payload);
        let sig = sign(&sk, mj.as_bytes());
        // 夹带清单外文件。
        let zip = write_zip(
            "extra.zip",
            mj.as_bytes(),
            &sig,
            payload,
            &[("evil.dll", b"X")],
        );
        let dir = unique_dir("extra");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(
            verify_and_extract(
                &zip,
                &ks,
                &expect(),
                "0.1.0",
                NOW,
                &dir,
                &InstallLimits::default()
            ),
            Err(InstallError::ExtraFile(_))
        ));
        assert!(!dir.exists(), "拒绝后不应留 staging");
    }

    #[test]
    fn missing_file_rejected() {
        let sk = signing_key(3);
        let ks = release_keyset(&sk);
        // 清单声明两文件，zip 只放一个。
        let declared: &[(&str, &[u8], bool)] =
            &[("a.txt", b"AAA", false), ("b.txt", b"BBB", false)];
        let mj = manifest_json(declared);
        let sig = sign(&sk, mj.as_bytes());
        let present: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let zip = write_zip("missing.zip", mj.as_bytes(), &sig, present, &[]);
        let dir = unique_dir("missing");
        assert!(matches!(
            verify_and_extract(
                &zip,
                &ks,
                &expect(),
                "0.1.0",
                NOW,
                &dir,
                &InstallLimits::default()
            ),
            Err(InstallError::MissingFile(_))
        ));
    }

    #[test]
    fn hash_mismatch_rejected() {
        let sk = signing_key(4);
        let ks = release_keyset(&sk);
        let declared: &[(&str, &[u8], bool)] = &[("a.txt", b"DECLARED", false)];
        let mj = manifest_json(declared);
        let sig = sign(&sk, mj.as_bytes());
        // zip 里同名文件内容不同 → hash 不符。
        let actual: &[(&str, &[u8], bool)] = &[("a.txt", b"TAMPERED", false)];
        let zip = write_zip("hash.zip", mj.as_bytes(), &sig, actual, &[]);
        let dir = unique_dir("hash");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(
            verify_and_extract(
                &zip,
                &ks,
                &expect(),
                "0.1.0",
                NOW,
                &dir,
                &InstallLimits::default()
            ),
            Err(InstallError::HashMismatch(_))
        ));
        assert!(!dir.exists(), "hash 不符后清理 staging");
    }

    #[test]
    fn traversal_entry_rejected() {
        let sk = signing_key(5);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let mj = manifest_json(payload);
        let sig = sign(&sk, mj.as_bytes());
        // 夹带穿越路径 entry（不在清单，但路径本身非法 → 在白名单比对前的路径校验即拒）。
        let zip = write_zip(
            "trav.zip",
            mj.as_bytes(),
            &sig,
            payload,
            &[("../../evil.exe", b"X")],
        );
        let dir = unique_dir("trav");
        let r = verify_and_extract(
            &zip,
            &ks,
            &expect(),
            "0.1.0",
            NOW,
            &dir,
            &InstallLimits::default(),
        );
        assert!(matches!(r, Err(InstallError::UnsafeEntry(_))), "got {r:?}");
    }

    #[test]
    fn file_count_limit() {
        let sk = signing_key(6);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let mj = manifest_json(payload);
        let sig = sign(&sk, mj.as_bytes());
        let zip = write_zip("count.zip", mj.as_bytes(), &sig, payload, &[]);
        let dir = unique_dir("count");
        let limits = InstallLimits {
            max_files: 1, // zip 实际 3 entry（manifest+sig+a.txt）> 1
            ..InstallLimits::default()
        };
        assert!(matches!(
            verify_and_extract(&zip, &ks, &expect(), "0.1.0", NOW, &dir, &limits),
            Err(InstallError::LimitExceeded(_))
        ));
    }

    #[test]
    fn registry_mismatch_rejected() {
        let sk = signing_key(7);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let mj = manifest_json(payload);
        let sig = sign(&sk, mj.as_bytes());
        let zip = write_zip("regmis.zip", mj.as_bytes(), &sig, payload, &[]);
        let dir = unique_dir("regmis");
        let mut e = expect();
        e.package_sequence = 999; // 与 manifest(3) 不符
        assert!(matches!(
            verify_and_extract(&zip, &ks, &e, "0.1.0", NOW, &dir, &InstallLimits::default()),
            Err(InstallError::Manifest(PackageError::RegistryMismatch(_)))
        ));
    }

    #[test]
    fn incompatible_host_rejected() {
        let sk = signing_key(8);
        let ks = release_keyset(&sk);
        let payload: &[(&str, &[u8], bool)] = &[("a.txt", b"AAA", false)];
        let mj = manifest_json(payload); // min_host_version=0.1.0
        let sig = sign(&sk, mj.as_bytes());
        let zip = write_zip("host.zip", mj.as_bytes(), &sig, payload, &[]);
        let dir = unique_dir("host");
        // host 0.0.1 < min 0.1.0 → 拒。
        assert!(matches!(
            verify_and_extract(
                &zip,
                &ks,
                &expect(),
                "0.0.1",
                NOW,
                &dir,
                &InstallLimits::default()
            ),
            Err(InstallError::IncompatibleHost)
        ));
    }

    #[test]
    fn symlink_mode_detected() {
        assert!(is_symlink_mode(0o120777));
        assert!(!is_symlink_mode(0o100644)); // 普通文件
        assert!(!is_symlink_mode(0o040755)); // 目录
    }

    #[test]
    fn plugin_install_dir_rejects_bad_id() {
        let base = std::env::temp_dir();
        assert!(plugin_install_dir(&base, "exotic-image-psd").is_some());
        for bad in ["", "../evil", "a/b", "A_B", "x".repeat(65).as_str(), "a:b"] {
            assert!(
                plugin_install_dir(&base, bad).is_none(),
                "应拒绝 plugin_id：{bad:?}"
            );
        }
    }

    /// 在唯一临时目录里建一个含标记文件的目录。
    fn mkdir_with(tag: &str, marker: &str, content: &[u8]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("exotic-swap-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(marker), content).unwrap();
        d
    }

    #[test]
    fn commit_install_fresh_then_upgrade_then_rollback() {
        let root = std::env::temp_dir().join(format!("exotic-swaproot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let current = root.join("current");
        let backup = root.join("backup");

        // 首装：current 不存在 → staging 直接就位。
        let staging1 = mkdir_with("s1", "ver.txt", b"v1");
        commit_install(&current, &staging1, &backup).unwrap();
        assert_eq!(std::fs::read(current.join("ver.txt")).unwrap(), b"v1");
        assert!(!staging1.exists(), "staging 应已移走");
        assert!(!backup.exists(), "首装无旧版本 → 无 backup");

        // 升级：旧 current → backup，新 staging → current。
        let staging2 = mkdir_with("s2", "ver.txt", b"v2");
        commit_install(&current, &staging2, &backup).unwrap();
        assert_eq!(std::fs::read(current.join("ver.txt")).unwrap(), b"v2");
        assert_eq!(
            std::fs::read(backup.join("ver.txt")).unwrap(),
            b"v1",
            "旧版本进 backup"
        );

        // 回滚：current(v2) 丢弃，backup(v1) 还原。
        rollback_to_backup(&current, &backup).unwrap();
        assert_eq!(std::fs::read(current.join("ver.txt")).unwrap(), b"v1");
        assert!(!backup.exists(), "回滚后 backup 已消费");

        // 成功路径清理 backup。
        let staging3 = mkdir_with("s3", "ver.txt", b"v3");
        commit_install(&current, &staging3, &backup).unwrap();
        discard_backup(&backup).unwrap();
        assert!(!backup.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// R1-4 错误码稳定性锁（同 error.rs / plugin-api 的既有模式）：前端按 code 分支处理，
    /// 任何改名都是破坏性契约变更——全集在此钉死；Manifest 委托 PackageError::code 亦锁一例。
    #[test]
    fn install_error_codes_are_stable() {
        use crate::exotic::package::PackageError;
        let cases: &[(InstallError, &str)] = &[
            (InstallError::OpenZip("x".into()), "open_zip"),
            (InstallError::MissingMeta("m"), "missing_meta"),
            (
                InstallError::Manifest(PackageError::BadSignature),
                "bad_signature",
            ),
            (InstallError::IncompatibleHost, "incompatible_host"),
            (
                InstallError::ProtocolMismatch { pkg: 2, host: 1 },
                "protocol_mismatch",
            ),
            (InstallError::UnsafeEntry("p".into()), "unsafe_entry"),
            (InstallError::Symlink("p".into()), "symlink"),
            (InstallError::ExtraFile("p".into()), "extra_file"),
            (InstallError::MissingFile("p".into()), "missing_file"),
            (InstallError::CaseCollision("p".into()), "case_collision"),
            (InstallError::LimitExceeded("l"), "limit_exceeded"),
            (InstallError::HashMismatch("p".into()), "hash_mismatch"),
            (InstallError::Io("e".into()), "install_io"),
            (InstallError::CatalogReject("r".into()), "catalog_reject"),
            (InstallError::Db("e".into()), "install_db"),
            (
                InstallError::ChannelUnsupported("steam_depot"),
                "channel_unsupported",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), *code, "错误码必须稳定：{err:?}");
        }
    }
}
