// src-tauri/src/exotic/package.rs
//! 冷门格式插件 · 包清单与安全相对路径净化（v3 Part3 §6.3/§6.4）。
//!
//! `package-manifest.json` 列出包内**全部** payload 文件的规范相对路径、大小、SHA-256、类型、
//! 执行位（仅排除无法自哈希的 manifest 自身与其 `.sig`）。manifest 由 release key 签名（覆盖原始
//! bytes），是「安装真相」的来源：解包前先验签 manifest，再以它为**白名单**逐一复核 zip 内容。
//!
//! 安全要点：
//!   - 包是**不可信输入**（即使来自已验签 Registry，仍假设 zip 内容可能被构造攻击）。
//!   - [`is_safe_relative_path`] 是核心防线：拒绝绝对路径 / `..` / 盘符 / UNC / 反斜杠 / NUL /
//!     Windows 保留设备名（`CON`/`NUL`/`COM1`… 即便无穿越也会在写入时触发设备 IO）。
//!   - 该校验同时用于 manifest 解析与 zip entry 扫描（[`crate::exotic::install`]），保证两侧一致。

use std::collections::HashSet;

use serde::Deserialize;

use crate::exotic::crypto::{CryptoError, KeyPurpose, VerifyingKeyset};

/// 本 Host 支持的 package manifest schema 版本。
const SUPPORTED_PACKAGE_SCHEMA: u32 = 1;
/// manifest JSON 大小上限。
const MAX_MANIFEST_LEN: usize = 1024 * 1024;

/// 包清单/校验错误。`code()` 稳定，可安全输出。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PackageError {
    #[error("manifest 超长")]
    TooLarge,
    #[error("manifest 签名验证失败")]
    BadSignature,
    #[error("manifest JSON 解析失败：{0}")]
    Parse(String),
    #[error("不支持的 package schema 版本：{0}")]
    UnsupportedSchema(u32),
    #[error("manifest 字段与 Registry 不一致：{0}")]
    RegistryMismatch(String),
    #[error("非安全相对路径：{0}")]
    UnsafePath(String),
    #[error("重复文件路径：{0}")]
    DuplicatePath(String),
    #[error("非法 sha256（须 64 位小写 hex）：{0}")]
    BadSha256(String),
    #[error("files 清单为空")]
    EmptyFiles,
}

impl PackageError {
    pub fn code(&self) -> &'static str {
        match self {
            PackageError::TooLarge => "too_large",
            PackageError::BadSignature => "bad_signature",
            PackageError::Parse(_) => "parse",
            PackageError::UnsupportedSchema(_) => "schema",
            PackageError::RegistryMismatch(_) => "registry_mismatch",
            PackageError::UnsafePath(_) => "unsafe_path",
            PackageError::DuplicatePath(_) => "duplicate_path",
            PackageError::BadSha256(_) => "bad_sha256",
            PackageError::EmptyFiles => "empty_files",
        }
    }
}

/// 单个 payload 文件的完整性记录。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PackageFile {
    /// 规范相对路径（正斜杠、无 `..`、无盘符）。
    pub path: String,
    pub size: u64,
    /// 64 位小写 hex SHA-256。
    pub sha256: String,
    /// worker / dylib / resource / license / sbom / compliance / model …(展示与策略用)。
    /// model = zip 内随包小模型;GB 级权重不进 zip,走 `RegistryEntry.model_blobs` 分步下载(§3.7.1/T12)。
    pub kind: String,
    /// 执行位（worker/可执行需要；安装时据此设置 unix 权限）。
    #[serde(default)]
    pub executable: bool,
}

/// 包清单顶层（§6.3）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub schema: u32,
    pub key_id: String,
    pub plugin_id: String,
    pub version: String,
    pub package_sequence: i64,
    pub target: String,
    pub min_host_version: String,
    pub protocol_version: u16,
    pub compliance_review_id: String,
    pub files: Vec<PackageFile>,
}

impl PackageManifest {
    /// 与 Registry 条目交叉核对（§6.4 第 4 步）：plugin_id/version/target/package_sequence 一致。
    /// 防止「已验签 manifest 描述的是另一个包」。
    pub fn check_matches_registry(
        &self,
        plugin_id: &str,
        version: &str,
        target: &str,
        package_sequence: i64,
    ) -> Result<(), PackageError> {
        let mismatch = |field: &str| Err(PackageError::RegistryMismatch(field.to_string()));
        if self.plugin_id != plugin_id {
            return mismatch("plugin_id");
        }
        if self.version != version {
            return mismatch("version");
        }
        if self.target != target {
            return mismatch("target");
        }
        if self.package_sequence != package_sequence {
            return mismatch("package_sequence");
        }
        Ok(())
    }

    /// 全部 payload 文件路径集合（zip 扫描时作白名单；加上两份签名元数据即 entry 全集）。
    pub fn file_paths(&self) -> HashSet<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }
}

/// 密码学层错误 → Package：折叠为 BadSignature（不外泄信任根细节）。
fn map_crypto(_e: CryptoError) -> PackageError {
    PackageError::BadSignature
}

/// 校验 + 解析 package manifest（**验签先于解析**，§6.4 第 3 步）。
///
/// 顺序：大小 → 验签(release 用途，全部有效 key 逐一，原始 bytes) → 解析 → schema →
/// 逐文件路径/sha256 校验 + 路径去重。任一非法整体拒绝。
pub fn verify_manifest(
    manifest_bytes: &[u8],
    sig_bytes: &[u8],
    keyset: &VerifyingKeyset,
    now: i64,
) -> Result<PackageManifest, PackageError> {
    if manifest_bytes.len() > MAX_MANIFEST_LEN {
        return Err(PackageError::TooLarge);
    }
    // 验签先于解析：对全部 release 用途有效 key 逐一验原始 manifest bytes。
    keyset
        .verify_any(KeyPurpose::Release, manifest_bytes, sig_bytes, now)
        .map_err(map_crypto)?;

    let manifest: PackageManifest =
        serde_json::from_slice(manifest_bytes).map_err(|e| PackageError::Parse(e.to_string()))?;
    if manifest.schema != SUPPORTED_PACKAGE_SCHEMA {
        return Err(PackageError::UnsupportedSchema(manifest.schema));
    }
    if manifest.files.is_empty() {
        return Err(PackageError::EmptyFiles);
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for f in &manifest.files {
        if !is_safe_relative_path(&f.path) {
            return Err(PackageError::UnsafePath(f.path.clone()));
        }
        if !is_sha256_hex(&f.sha256) {
            return Err(PackageError::BadSha256(f.path.clone()));
        }
        if !seen.insert(f.path.as_str()) {
            return Err(PackageError::DuplicatePath(f.path.clone()));
        }
    }
    Ok(manifest)
}

/// 路径是否为**安全的规范相对路径**。核心解包防线（§6.4 第 5 步），同时用于 manifest 与 zip 扫描。
///
/// 拒绝：空 / 绝对路径 / 盘符(`C:`) / UNC / 反斜杠 / NUL / 控制字符 / `.`/`..` 段 / 空段（`a//b`）/
/// 段首尾空格或点（Windows 会静默剥除 → 别名穿越）/ Windows 保留设备名（`CON`/`NUL`/`COM1`…）。
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 1024 {
        return false;
    }
    // 反斜杠、冒号、NUL、控制字符一律拒（统一只用正斜杠）。
    if path
        .bytes()
        .any(|b| b == b'\\' || b == b':' || b == 0 || b.is_ascii_control())
    {
        return false;
    }
    // 绝对路径 / 盘符。
    if path.starts_with('/') {
        return false;
    }
    let first = path.split('/').next().unwrap_or("");
    if first.len() >= 2 && first.as_bytes()[1] == b':' {
        return false; // 形如 C: 盘符
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return false; // 空段（含尾部 `/`、`a//b`）、当前/上级目录
        }
        // 段首尾空格或点（规范 §6.4）：Windows 静默剥除尾点/空格可造成别名；段首点(如 .NUL)可绕过
        // 设备名检测且属隐藏名变体——一并拒绝（插件包不应含点前缀文件，安全评审 low）。
        if seg.starts_with(' ') || seg.starts_with('.') || seg.ends_with(' ') || seg.ends_with('.')
        {
            return false;
        }
        if is_reserved_device_name(seg) {
            return false;
        }
    }
    true
}

/// Windows 保留设备名（大小写不敏感，含带扩展名形式如 `NUL.txt`）。
fn is_reserved_device_name(seg: &str) -> bool {
    // 取首个 `.` 前的基名。
    let base = seg.split('.').next().unwrap_or(seg);
    let upper = base.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// 64 位小写 hex。
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::crypto::test_support::{keyset_json, sign, signing_key, KeySpec};

    const NOW: i64 = 1_790_000_000;

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

    fn good_manifest() -> String {
        format!(
            r#"{{"schema":1,"key_id":"release-test","plugin_id":"exotic-image-psd",
              "version":"1.0.0","package_sequence":3,"target":"x86_64-pc-windows-msvc",
              "min_host_version":"0.1.0","protocol_version":1,
              "compliance_review_id":"review-2026-psd-001",
              "files":[
                {{"path":"bin/x86_64-pc-windows-msvc/psd-worker.exe","size":1048576,
                  "sha256":"{h}","kind":"worker","executable":true}},
                {{"path":"plugin-manifest.json","size":256,"sha256":"{h}","kind":"manifest"}},
                {{"path":"LICENSES/psd.txt","size":1024,"sha256":"{h}","kind":"license"}}
              ]}}"#,
            h = "b".repeat(64)
        )
    }

    fn signed(sk: &ring::signature::Ed25519KeyPair, json: &str) -> (Vec<u8>, Vec<u8>) {
        let bytes = json.as_bytes().to_vec();
        let sig = sign(sk, &bytes);
        (bytes, sig)
    }

    #[test]
    fn valid_manifest_verifies_and_matches_registry() {
        let sk = signing_key(1);
        let ks = release_keyset(&sk);
        let (bytes, sig) = signed(&sk, &good_manifest());
        let m = verify_manifest(&bytes, &sig, &ks, NOW).unwrap();
        assert_eq!(m.files.len(), 3);
        assert!(m
            .check_matches_registry("exotic-image-psd", "1.0.0", "x86_64-pc-windows-msvc", 3)
            .is_ok());
        // 任一字段不符 → 拒绝。
        assert!(m
            .check_matches_registry("exotic-image-psd", "9.9.9", "x86_64-pc-windows-msvc", 3)
            .is_err());
        assert!(m
            .check_matches_registry("exotic-image-psd", "1.0.0", "x86_64-pc-windows-msvc", 4)
            .is_err());
    }

    #[test]
    fn tampered_manifest_fails_signature() {
        let sk = signing_key(2);
        let ks = release_keyset(&sk);
        let (mut bytes, sig) = signed(&sk, &good_manifest());
        bytes[10] ^= 0xff;
        assert_eq!(
            verify_manifest(&bytes, &sig, &ks, NOW),
            Err(PackageError::BadSignature)
        );
    }

    #[test]
    fn unsafe_paths_rejected_in_manifest() {
        let sk = signing_key(3);
        let ks = release_keyset(&sk);
        // 把合法路径替换为穿越路径 → 验签会变（需重签）。直接构造并签名。
        let json = good_manifest().replace(
            "bin/x86_64-pc-windows-msvc/psd-worker.exe",
            "../../evil.exe",
        );
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_manifest(&bytes, &sig, &ks, NOW),
            Err(PackageError::UnsafePath(_))
        ));
    }

    #[test]
    fn duplicate_path_rejected() {
        let sk = signing_key(4);
        let ks = release_keyset(&sk);
        let json = good_manifest().replace("LICENSES/psd.txt", "plugin-manifest.json");
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_manifest(&bytes, &sig, &ks, NOW),
            Err(PackageError::DuplicatePath(_))
        ));
    }

    #[test]
    fn bad_sha256_rejected() {
        let sk = signing_key(5);
        let ks = release_keyset(&sk);
        let json = good_manifest().replacen(&"b".repeat(64), "NOTHEX", 1);
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_manifest(&bytes, &sig, &ks, NOW),
            Err(PackageError::BadSha256(_))
        ));
    }

    #[test]
    fn safe_path_validator_matrix() {
        // 合法。
        for ok in [
            "a",
            "bin/worker.exe",
            "LICENSES/psd.txt",
            "a/b/c/d.dat",
            "name.with.dots.json",
        ] {
            assert!(is_safe_relative_path(ok), "应合法：{ok}");
        }
        // 非法。
        for bad in [
            "",
            "/abs",
            "../escape",
            "a/../b",
            "a/./b",
            "C:/win",
            "c:\\win",
            "back\\slash",
            "a//b",
            "trail/",
            "with\u{0}nul",
            "CON",
            "nul.txt",
            "COM1",
            "lpt9.dat",
            "dir/CON/x",
            "space ",  // 尾空格
            "dot.",    // 尾点
            " lead",   // 首空格
            ".hidden", // 段首点
            "dir/.git/x",
            ".NUL", // 段首点 + 设备名变体
        ] {
            assert!(!is_safe_relative_path(bad), "应非法：{bad:?}");
        }
    }
}
