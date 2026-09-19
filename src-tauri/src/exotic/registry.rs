// src-tauri/src/exotic/registry.rs
//! 冷门格式插件 · 签名 Registry index（v3 Part3 §6.1）。
//!
//! Registry 是**远程发行真相**：列出每个插件包的下载地址、大小、SHA-256、`package_sequence`
//! 与 offering 元数据。文件对 = `index.json` + `index.sig`（release key 签**原始 index bytes**）。
//!
//! 安全规则（§6.1）：
//!   - **先验签，再解析/使用任何 URL**：未验签的 index 不得驱动下载（前端只能传 plugin_id，不能传 URL）。
//!   - `registry_sequence` 安全单调（R11）：小于本地最高已接受值即拒绝（防整个索引回滚/冻结）。
//!   - index 过期：允许展示缓存与已安装插件，但**不允许**从过期元数据执行新安装。
//!   - 远程 offering **不得**覆盖常见格式（与内置 Catalog 同纵深防御）。
//!   - 只接受 HTTPS 下载地址；package_sha256 必须是 64 位小写 hex。
//!   - 本地缓存：临时文件 + 原子替换，保存最后有效 index 及其 sequence。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::exotic::crypto::{CryptoError, KeyPurpose, VerifyingKeyset};
use crate::utils::format::classify_media_type;

/// 本 Host 支持的 Registry schema 版本。
const SUPPORTED_REGISTRY_SCHEMA: u32 = 1;
/// index.json 大小上限（防超大输入；正常索引仅 KB 级）。
const MAX_INDEX_LEN: usize = 4 * 1024 * 1024;

/// Registry 解析/验签错误。`code()` 稳定，可安全输出。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("index 超长")]
    TooLarge,
    #[error("index 签名验证失败")]
    BadSignature,
    #[error("index JSON 解析失败：{0}")]
    Parse(String),
    #[error("不支持的 registry schema 版本：{0}")]
    UnsupportedSchema(u32),
    #[error("registry sequence 回滚：收到 {got} < 本地已接受 {have}")]
    RollbackRejected { got: u64, have: u64 },
    #[error("条目非法（{plugin_id}）：{reason}")]
    InvalidEntry { plugin_id: String, reason: String },
    #[error("缓存 IO 失败：{0}")]
    Io(String),
}

impl RegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            RegistryError::TooLarge => "too_large",
            RegistryError::BadSignature => "bad_signature",
            RegistryError::Parse(_) => "parse",
            RegistryError::UnsupportedSchema(_) => "schema",
            RegistryError::RollbackRejected { .. } => "rollback_rejected",
            RegistryError::InvalidEntry { .. } => "invalid_entry",
            RegistryError::Io(_) => "io",
        }
    }
}

/// 密码学层错误 → Registry：除已知签名失败外一律折叠为 BadSignature（不外泄信任根细节）。
fn map_crypto(_e: CryptoError) -> RegistryError {
    RegistryError::BadSignature
}

/// 单个插件包条目（§6.1）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RegistryEntry {
    pub plugin_id: String,
    /// 展示用版本字符串（安全单调性看 `package_sequence`）。
    pub version: String,
    /// 包安全单调序号（R11；安装只允许更高，防包回滚）。
    pub package_sequence: i64,
    pub media_kind: String,
    pub formats: Vec<String>,
    pub capabilities: Vec<String>,
    pub sku: String,
    pub min_host_version: String,
    /// rust target triple（平台选择）。
    pub target: String,
    /// 下载地址（必须 HTTPS）。
    pub package_url: String,
    pub package_size: u64,
    /// 64 位小写 hex SHA-256。
    pub package_sha256: String,
    #[serde(default)]
    pub store_url: Option<String>,
    /// 模型权重 blob 清单(Part4 §3.7.1/T12):独立于插件 zip **分步下载**的大文件。
    /// 旧 index 无此字段 → 默认空(向后兼容)。
    #[serde(default)]
    pub model_blobs: Vec<ModelBlob>,
}

/// 单个模型权重 blob(Part4 §3.7.1/T12)。
///
/// 不进插件 zip → `InstallLimits` 的 zip-bomb 检查(压缩比/总解压量/512MB 总量)对其
/// **天然不适用**(这正是「大模型分步下载」的动机:ViT-L fp32 ≈1.7GB 无法进 512MB zip,
/// 且 ONNX 近 1:1 压缩比使 zip 无收益)。自身防线 = HTTPS + size 精确封顶(下载层)+
/// sha256 + 落盘文件名白名单(本文件 `validate_entry`)+ 单文件大小硬上限。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelBlob {
    /// 下载地址(必须 HTTPS)。
    pub url: String,
    /// 64 位小写 hex(对**分发形态**字节;加密权重即密文的 sha256,Part6 §8.3)。
    pub sha256: String,
    pub size: u64,
    /// "model_weight" 等(展示/策略用,同 `PackageFile.kind` 风格)。
    pub kind: String,
    /// 落盘文件名:单路径分量(白名单校验),落 models 目录(D1 Level A:
    /// `SessionInit` 传路径即可用,无需再过安装器)。
    pub file_name: String,
}

/// index 顶层（§6.1）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RegistryIndex {
    pub schema: u32,
    pub key_id: String,
    /// registry_sequence（R11；全局单调，防整索引回滚）。
    pub sequence: u64,
    pub generated_at: i64,
    pub expires_at: i64,
    pub plugins: Vec<RegistryEntry>,
}

impl RegistryIndex {
    /// 选取某 plugin_id + target 的条目（安装命令据此取下载坐标）。
    pub fn select<'a>(&'a self, plugin_id: &str, target: &str) -> Option<&'a RegistryEntry> {
        self.plugins
            .iter()
            .find(|e| e.plugin_id == plugin_id && e.target == target)
    }
}

/// 已验签的 registry（含过期标志）。过期仍返回——供展示缓存/已装，但安装路径须拒绝过期元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRegistry {
    pub index: RegistryIndex,
    /// `now > expires_at`：过期。不得据此执行新安装（§6.1）。
    pub expired: bool,
}

/// 校验 + 解析 index（**验签先于一切解析/使用**）。
///
/// 顺序：大小 → 验签(release 用途，原始 bytes) → 解析 → schema → 逐条目校验 → 过期标志。
/// 验签与解析严格分离:验签对象恒为**收到的原始 bytes**(缓存亦原样落盘 index.json/.sig),
/// 解析结果只读、绝不重新序列化回写;故 index 中多出的未知字段被 serde 忽略,不影响验签结论。
/// `min_accepted_sequence`：本地已接受的最高 registry_sequence；收到更小者拒绝（防回滚）。
pub fn verify_and_parse(
    index_bytes: &[u8],
    sig_bytes: &[u8],
    keyset: &VerifyingKeyset,
    now: i64,
    min_accepted_sequence: u64,
) -> Result<VerifiedRegistry, RegistryError> {
    if index_bytes.len() > MAX_INDEX_LEN {
        return Err(RegistryError::TooLarge);
    }
    // **验签先于任何解析**（§6.1）：对全部 release 用途有效 key 逐一验原始 index bytes，
    // 命中即过。不先解析未验签内容（连 key_id 探针都不做）——篡改任一字节必然 BadSignature。
    keyset
        .verify_any(KeyPurpose::Release, index_bytes, sig_bytes, now)
        .map_err(map_crypto)?;

    // 验签通过后才完整解析并信任内容。
    let index: RegistryIndex =
        serde_json::from_slice(index_bytes).map_err(|e| RegistryError::Parse(e.to_string()))?;
    if index.schema != SUPPORTED_REGISTRY_SCHEMA {
        return Err(RegistryError::UnsupportedSchema(index.schema));
    }
    // 防回滚：sequence 必须 ≥ 本地已接受最高值。
    if index.sequence < min_accepted_sequence {
        return Err(RegistryError::RollbackRejected {
            got: index.sequence,
            have: min_accepted_sequence,
        });
    }
    // 逐条目结构/安全校验（任一非法即整体拒绝——签名权威，不做部分接受）。
    for e in &index.plugins {
        validate_entry(e)?;
    }
    let expired = now > index.expires_at;
    Ok(VerifiedRegistry { index, expired })
}

/// 下载地址 scheme 白名单:HTTPS 恒放行;dev 构建 + `PICASA_EXOTIC_DEV_FILE_URLS=1`
/// 显式开关下额外放行 file://(本地 registry 测试;传输与完整性语义见 download 模块
/// dev 段)。Release 构建开关恒 false → 恒 HTTPS-only,行为与改动前逐位一致。
fn url_scheme_ok(url: &str) -> bool {
    url.starts_with("https://")
        || (crate::download::dev_file_urls_enabled() && url.starts_with("file://"))
}

/// 单条目校验：plugin_id/format 合规、不撞常见格式、HTTPS、sha256 hex、size>0、media_kind 已知。
fn validate_entry(e: &RegistryEntry) -> Result<(), RegistryError> {
    let bad = |reason: &str| RegistryError::InvalidEntry {
        plugin_id: e.plugin_id.clone(),
        reason: reason.to_string(),
    };
    if !is_valid_plugin_id(&e.plugin_id) {
        return Err(bad("plugin_id 非法"));
    }
    if !matches!(
        e.media_kind.as_str(),
        "image" | "video" | "audio" | "document"
    ) {
        return Err(bad("未知 media_kind"));
    }
    if e.formats.is_empty() {
        return Err(bad("formats 为空"));
    }
    for f in &e.formats {
        if !is_valid_format(f) {
            return Err(bad("format 非法（须小写、无点、[a-z0-9]{1,16}）"));
        }
        // 纵深防御：远程 offering 不得覆盖常见格式（与内置 Catalog 同策略）。
        if classify_media_type(f).is_some() {
            return Err(bad("format 撞常见格式（远程不得覆盖）"));
        }
    }
    if e.capabilities.is_empty() {
        return Err(bad("capabilities 为空"));
    }
    if e.sku.is_empty() {
        return Err(bad("sku 为空"));
    }
    // 只接受 HTTPS（前端只传 plugin_id，URL 来自已验签 index；仍强制 scheme;
    // dev file:// 经 url_scheme_ok 显式开关放行）。
    if !url_scheme_ok(&e.package_url) {
        return Err(bad("package_url 必须为 HTTPS"));
    }
    if e.package_size == 0 {
        return Err(bad("package_size 为 0"));
    }
    if !is_sha256_hex(&e.package_sha256) {
        return Err(bad("package_sha256 必须为 64 位小写 hex"));
    }
    if e.target.is_empty() || !e.target.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(bad("target 非法"));
    }
    // model blobs(§3.7.1/T12):独立分步下载的大文件,数据面在此设防(执行面在 fetch)。
    for b in &e.model_blobs {
        if !url_scheme_ok(&b.url) {
            return Err(bad("model_blob url 必须为 HTTPS"));
        }
        if !is_sha256_hex(&b.sha256) {
            return Err(bad("model_blob sha256 必须为 64 位小写 hex"));
        }
        if b.size == 0 || b.size > MAX_MODEL_BLOB_SIZE {
            return Err(bad("model_blob size 非法(0 或超单文件上限)"));
        }
        if b.kind.is_empty() || !b.kind.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(bad("model_blob kind 非法"));
        }
        if !is_safe_blob_file_name(&b.file_name) {
            return Err(bad("model_blob file_name 非法(须安全单路径分量)"));
        }
    }
    Ok(())
}

/// 模型 blob 单文件大小硬上限(§3.7.1/T12):ViT-L fp32 ≈1.7GB,8GiB 留足余量;
/// 防条目被构造成天文数字致磁盘耗尽(下载层另有 size 精确封顶,双保险)。
const MAX_MODEL_BLOB_SIZE: u64 = 8 * 1024 * 1024 * 1024;

/// blob 落盘文件名白名单:单路径分量(无分隔符/盘符/父引用),防写出 models 目录。
/// 字符集从紧([A-Za-z0-9._-],1-96 字节,不以点开头——顺带排除 "."/".."/隐藏文件);
/// 文件名由我方 Registry 签发,从紧无兼容代价。
fn is_safe_blob_file_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=96).contains(&b.len())
        && b[0] != b'.'
        && b.iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// 本地 Registry 缓存：持已接受最高 sequence + 缓存目录（index.json/.sig 原子落地）。
pub struct RegistryCache {
    dir: PathBuf,
    accepted_sequence: u64,
}

impl RegistryCache {
    /// 从缓存目录加载（读已保存的 sequence；无缓存则 0）。
    pub fn load(dir: PathBuf) -> Self {
        let accepted_sequence = std::fs::read_to_string(dir.join("index.seq"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        RegistryCache {
            dir,
            accepted_sequence,
        }
    }

    pub fn accepted_sequence(&self) -> u64 {
        self.accepted_sequence
    }

    /// 读缓存的 index.json/.sig 并验签解析（list_registry / install 用）。
    /// 无缓存、验签失败或回滚 → None。过期仍返回（含 `expired` 标志，安装路径据此拒绝）。
    pub fn load_verified(&self, keyset: &VerifyingKeyset, now: i64) -> Option<VerifiedRegistry> {
        let bytes = std::fs::read(self.dir.join("index.json")).ok()?;
        let sig = std::fs::read(self.dir.join("index.sig")).ok()?;
        verify_and_parse(&bytes, &sig, keyset, now, self.accepted_sequence).ok()
    }

    /// 校验并接受新 index：验签 + 防回滚通过后，原子写入缓存（index.json/.sig/.seq）。
    /// 返回已验签 registry。回滚/验签失败时缓存不变。
    pub fn accept(
        &mut self,
        index_bytes: &[u8],
        sig_bytes: &[u8],
        keyset: &VerifyingKeyset,
        now: i64,
    ) -> Result<VerifiedRegistry, RegistryError> {
        let verified =
            verify_and_parse(index_bytes, sig_bytes, keyset, now, self.accepted_sequence)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| RegistryError::Io(e.to_string()))?;
        atomic_write(&self.dir.join("index.json"), index_bytes)?;
        atomic_write(&self.dir.join("index.sig"), sig_bytes)?;
        atomic_write(
            &self.dir.join("index.seq"),
            verified.index.sequence.to_string().as_bytes(),
        )?;
        self.accepted_sequence = verified.index.sequence;
        Ok(verified)
    }
}

/// 原子写：临时文件 + flush + rename（避免半写缓存）。
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    use std::io::Write as _;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| RegistryError::Io(e.to_string()))?;
        f.write_all(bytes)
            .map_err(|e| RegistryError::Io(e.to_string()))?;
        f.flush().map_err(|e| RegistryError::Io(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| RegistryError::Io(e.to_string()))?;
    Ok(())
}

/// plugin_id 合规：`[a-z0-9-]`，1..=64（与 catalog 同约束）。
fn is_valid_plugin_id(id: &str) -> bool {
    let len = id.len();
    (1..=64).contains(&len)
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// format 合规：`[a-z0-9]`，1..=16，无点。
fn is_valid_format(f: &str) -> bool {
    let len = f.len();
    (1..=16).contains(&len)
        && f.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// 64 位小写 hex。
fn is_sha256_hex(s: &str) -> bool {
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

    fn good_index(sequence: u64, expires_at: i64) -> String {
        format!(
            r#"{{"schema":1,"key_id":"release-test","sequence":{sequence},
              "generated_at":1700000000,"expires_at":{expires_at},
              "plugins":[{{
                "plugin_id":"exotic-image-psd","version":"1.0.0","package_sequence":3,
                "media_kind":"image","formats":["psd"],"capabilities":["thumbnail"],
                "sku":"psd-engine-2026","min_host_version":"0.1.0",
                "target":"x86_64-pc-windows-msvc",
                "package_url":"https://cdn.example.invalid/psd-1.0.0-win.zip",
                "package_size":7340032,
                "package_sha256":"{}",
                "store_url":"https://store.example.invalid/psd"
              }}]}}"#,
            "a".repeat(64)
        )
    }

    fn signed(sk: &ring::signature::Ed25519KeyPair, json: &str) -> (Vec<u8>, Vec<u8>) {
        let bytes = json.as_bytes().to_vec();
        let sig = sign(sk, &bytes);
        (bytes, sig)
    }

    /// 带一个 model blob 的 index(T12);file_name/url/size 参数化以覆盖各拒绝分支。
    fn index_with_blob(file_name: &str, url: &str, size: u64) -> String {
        format!(
            r#"{{"schema":1,"key_id":"release-test","sequence":1,
              "generated_at":1700000000,"expires_at":{exp},
              "plugins":[{{
                "plugin_id":"exotic-ai-clip","version":"1.0.0","package_sequence":1,
                "media_kind":"image","formats":["clipx"],"capabilities":["embedding"],
                "sku":"clip-engine-2026","min_host_version":"0.1.0",
                "target":"x86_64-pc-windows-msvc",
                "package_url":"https://cdn.example.invalid/clip-1.0.0-win.zip",
                "package_size":1048576,
                "package_sha256":"{pk}",
                "model_blobs":[{{"url":"{url}","sha256":"{sh}","size":{size},
                  "kind":"model_weight","file_name":"{file_name}"}}]
              }}]}}"#,
            exp = NOW + 1000,
            pk = "a".repeat(64),
            sh = "b".repeat(64),
        )
    }

    #[test]
    fn model_blobs_parse_and_legacy_default_empty() {
        let sk = signing_key(9);
        let ks = release_keyset(&sk);
        // 旧 index(无 model_blobs 字段)→ 默认空(向后兼容)。
        let (bytes, sig) = signed(&sk, &good_index(1, NOW + 1000));
        let v = verify_and_parse(&bytes, &sig, &ks, NOW, 0).unwrap();
        assert!(v.index.plugins[0].model_blobs.is_empty());

        // 合法 blob 条目通过并可读出。
        let (bytes, sig) = signed(
            &sk,
            &index_with_blob(
                "vit-l-14.onnx",
                "https://cdn.example.invalid/vit-l-14.onnx",
                1_700_000_000,
            ),
        );
        let v = verify_and_parse(&bytes, &sig, &ks, NOW, 0).unwrap();
        let blobs = &v.index.plugins[0].model_blobs;
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].file_name, "vit-l-14.onnx");
        assert_eq!(blobs[0].kind, "model_weight");
        assert_eq!(blobs[0].size, 1_700_000_000);
    }

    #[test]
    fn model_blob_invalid_entries_rejected() {
        let sk = signing_key(9);
        let ks = release_keyset(&sk);
        let cases: &[(&str, &str, u64)] = &[
            // 非 HTTPS
            ("vit.onnx", "http://cdn.example.invalid/v.onnx", 100),
            // 父引用(以点开头一并拦截)
            ("..evil.onnx", "https://cdn.example.invalid/v.onnx", 100),
            // 路径分隔符
            ("a/b.onnx", "https://cdn.example.invalid/v.onnx", 100),
            // 隐藏文件/点开头
            (".hidden", "https://cdn.example.invalid/v.onnx", 100),
            // size=0
            ("vit.onnx", "https://cdn.example.invalid/v.onnx", 0),
            // 超单文件上限(8GiB)
            (
                "vit.onnx",
                "https://cdn.example.invalid/v.onnx",
                9 * 1024 * 1024 * 1024,
            ),
        ];
        for (name, url, size) in cases {
            let (bytes, sig) = signed(&sk, &index_with_blob(name, url, *size));
            assert!(
                matches!(
                    verify_and_parse(&bytes, &sig, &ks, NOW, 0),
                    Err(RegistryError::InvalidEntry { .. })
                ),
                "应拒绝:{name} {url} {size}"
            );
        }
        // 非法 sha256(替换为等长非 hex)同样拒绝。
        let bad_sha = index_with_blob("vit.onnx", "https://cdn.example.invalid/v.onnx", 100)
            .replace(&"b".repeat(64), &"z".repeat(64));
        let (bytes, sig) = signed(&sk, &bad_sha);
        assert!(matches!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn valid_index_verifies_and_selects() {
        let sk = signing_key(1);
        let ks = release_keyset(&sk);
        let (bytes, sig) = signed(&sk, &good_index(42, NOW + 1000));
        let v = verify_and_parse(&bytes, &sig, &ks, NOW, 0).unwrap();
        assert!(!v.expired);
        assert_eq!(v.index.sequence, 42);
        let e = v
            .index
            .select("exotic-image-psd", "x86_64-pc-windows-msvc")
            .unwrap();
        assert_eq!(e.package_sequence, 3);
        assert!(v
            .index
            .select("exotic-image-psd", "aarch64-apple-darwin")
            .is_none());
    }

    #[test]
    fn tampered_index_fails_signature() {
        let sk = signing_key(2);
        let ks = release_keyset(&sk);
        let (mut bytes, sig) = signed(&sk, &good_index(42, NOW + 1000));
        bytes[20] ^= 0xff; // 篡改
        assert_eq!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::BadSignature)
        );
    }

    #[test]
    fn license_key_cannot_sign_registry() {
        // 信任根分离：license 用途的 key 不能验 Registry（release 用途）。
        let sk = signing_key(3);
        let json = keyset_json(&[KeySpec {
            key_id: "release-test",
            purpose: "license",
            sk: &sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let (bytes, sig) = signed(&sk, &good_index(42, NOW + 1000));
        assert_eq!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::BadSignature)
        );
    }

    #[test]
    fn sequence_rollback_rejected() {
        let sk = signing_key(4);
        let ks = release_keyset(&sk);
        let (bytes, sig) = signed(&sk, &good_index(10, NOW + 1000));
        // 本地已接受 20 > 收到 10 → 拒绝。
        assert!(matches!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 20),
            Err(RegistryError::RollbackRejected { got: 10, have: 20 })
        ));
        // 等于已接受值 → 放行（幂等刷新）。
        assert!(verify_and_parse(&bytes, &sig, &ks, NOW, 10).is_ok());
    }

    #[test]
    fn expired_flagged_not_rejected() {
        let sk = signing_key(5);
        let ks = release_keyset(&sk);
        let (bytes, sig) = signed(&sk, &good_index(42, NOW - 1));
        let v = verify_and_parse(&bytes, &sig, &ks, NOW, 0).unwrap();
        assert!(v.expired, "过期应置标志（仍返回供展示，安装路径另行拒绝）");
    }

    #[test]
    fn http_url_rejected() {
        let sk = signing_key(6);
        let ks = release_keyset(&sk);
        let json = good_index(42, NOW + 1000).replace("https://cdn", "http://cdn");
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn common_format_entry_rejected() {
        let sk = signing_key(7);
        let ks = release_keyset(&sk);
        let json = good_index(42, NOW + 1000).replace(r#"["psd"]"#, r#"["jpg"]"#);
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn bad_sha256_rejected() {
        let sk = signing_key(8);
        let ks = release_keyset(&sk);
        let json = good_index(42, NOW + 1000).replace(&"a".repeat(64), "XYZ");
        let (bytes, sig) = signed(&sk, &json);
        assert!(matches!(
            verify_and_parse(&bytes, &sig, &ks, NOW, 0),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn cache_accept_persists_and_blocks_rollback() {
        let sk = signing_key(9);
        let ks = release_keyset(&sk);
        let dir = std::env::temp_dir().join(format!("exotic-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cache = RegistryCache::load(dir.clone());
        assert_eq!(cache.accepted_sequence(), 0);

        let (b1, s1) = signed(&sk, &good_index(5, NOW + 1000));
        cache.accept(&b1, &s1, &ks, NOW).unwrap();
        assert_eq!(cache.accepted_sequence(), 5);
        assert!(dir.join("index.json").exists());

        // 重新 load → 从磁盘恢复 sequence。
        let reloaded = RegistryCache::load(dir.clone());
        assert_eq!(reloaded.accepted_sequence(), 5);

        // 旧 sequence 包被拒，缓存不变。
        let (b0, s0) = signed(&sk, &good_index(3, NOW + 1000));
        let mut cache2 = RegistryCache::load(dir.clone());
        assert!(cache2.accept(&b0, &s0, &ks, NOW).is_err());
        assert_eq!(cache2.accepted_sequence(), 5);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
