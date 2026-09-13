// crates/scrollery-exotic-trust/src/crypto.rs
//! 冷门格式插件 · 信任根与 Ed25519 验签原语（v3 Part3 §5.1 / D10/D11）。
//!
//! 【Part6 §3.9.1a 去环 ③a】本模块自 `src-tauri/src/exotic/crypto.rs` 迁入本叶 crate——
//! `VerifyingKeyset` 结构 + `verify_strict` 调用属通用 ring 封装、无秘密价值。下沉动机：让验签
//! 原语脱离 `src-tauri`，供 registry / license 两用途共享同一信任根。src-tauri 侧
//! `crate::exotic::crypto` 退化为再导出薄壳，既有引用路径不变。
//!
//! Host **只验签、不签名**：私钥离线/HSM，永不入二进制（§5.1）。本模块提供：
//!   - [`VerifyingKeyset`]：编入 Host 的公钥集（key_id → 公钥 + 用途 + 状态 + 有效期窗口）。
//!   - [`VerifyingKeyset::verify`]：按 key_id/用途/状态/时间窗口门控后 `verify_strict` 验签。
//!   - base64url（无填充）编解码：License token 的 `payload.sig` 两段用之（§5.2）。
//!
//! 安全要点：
//!   - **信任根分离**（§5.1）：release key 签 Registry/package；license key 签 License token。
//!     验签强校验 `purpose`——license key 不能用来伪造 Registry，反之亦然。
//!   - **支持新旧 key 重叠轮换**（§5.1）：keyset 可同列多把 key，按 key_id 选取；删除旧 key 前
//!     须保证已发行永久 License 仍可验证，故旧 key 仅置 `revoked` 而非立即移除。
//!   - 用 `verify_strict`（拒绝非规范 R / 小阶公钥点），堵住 Ed25519 可锻造性。
//!   - 错误码稳定且不含密钥材料：日志/遥测/IPC 可安全输出 [`CryptoError::code`]。

use std::collections::HashMap;

use base64::Engine as _;
use serde::Deserialize;

/// 内置生产公钥集(编译期嵌入 = 随应用签名发布)。Release **不含**测试公钥。
/// 内容经 build.rs 装配(部署配置注入点):默认 = resources/exotic-keyset.json 占位集
/// (逐位一致);构建时设 PICASA_EXOTIC_KEYSET_FILE 可替换为内测/发布流水线 keyset。
/// 注入只在编译期,产物信任根固定——Release 运行时无任何 keyset 旁路(SEC-02)。
const BUILTIN_KEYSET_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/exotic-keyset.json"));

/// 本 Host 支持的 keyset schema 版本。
const SUPPORTED_KEYSET_SCHEMA: u32 = 1;

/// 密钥用途（信任根分离，§5.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyPurpose {
    /// 签 Registry index 与 package manifest（发行物）。
    Release,
    /// 签 License token（用户授权）。
    License,
}

/// 密钥状态。轮换期旧 key 置 `Revoked`（仍在 keyset 中以便诊断，但拒绝验签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyStatus {
    Active,
    Revoked,
}

/// 验签 / keyset 解析错误。`code()` 提供稳定字符串，不泄露任何密钥/签名材料。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("keyset JSON 解析失败：{0}")]
    Parse(String),
    #[error("不支持的 keyset schema 版本：{0}（支持 {SUPPORTED_KEYSET_SCHEMA}）")]
    UnsupportedSchema(u32),
    #[error("非法公钥（非 32 字节 Ed25519）：{0}")]
    BadPublicKey(String),
    #[error("base64 解码失败")]
    Base64,
    #[error("未知 key_id：{0}")]
    UnknownKey(String),
    #[error("key 用途不符（期望 {expected:?}）：{key_id}")]
    WrongPurpose {
        key_id: String,
        expected: KeyPurpose,
    },
    #[error("key 已吊销：{0}")]
    RevokedKey(String),
    #[error("key 未生效（not_before 未到）：{0}")]
    KeyNotYetValid(String),
    #[error("key 已过期（not_after 已过）：{0}")]
    KeyExpired(String),
    #[error("签名长度非法（非 64 字节）")]
    BadSignatureLen,
    #[error("签名验证失败")]
    BadSignature,
}

impl CryptoError {
    /// 稳定错误码（日志/遥测/IPC 安全输出；不含密钥材料）。
    pub fn code(&self) -> &'static str {
        match self {
            CryptoError::Parse(_) => "keyset_parse",
            CryptoError::UnsupportedSchema(_) => "keyset_schema",
            CryptoError::BadPublicKey(_) => "bad_public_key",
            CryptoError::Base64 => "base64",
            CryptoError::UnknownKey(_) => "unknown_key",
            CryptoError::WrongPurpose { .. } => "wrong_purpose",
            CryptoError::RevokedKey(_) => "revoked_key",
            CryptoError::KeyNotYetValid(_) => "key_not_yet_valid",
            CryptoError::KeyExpired(_) => "key_expired",
            CryptoError::BadSignatureLen => "bad_signature_len",
            CryptoError::BadSignature => "bad_signature",
        }
    }
}

/// JSON keyset 顶层。
#[derive(Debug, Deserialize)]
struct RawKeyset {
    schema: u32,
    keys: Vec<RawKey>,
}

/// JSON 单条公钥。
#[derive(Debug, Deserialize)]
struct RawKey {
    key_id: String,
    purpose: KeyPurpose,
    /// 32 字节 Ed25519 公钥（标准 base64，带填充）。
    public_key_b64: String,
    status: KeyStatus,
    /// 生效起点（unix 秒）。
    not_before: i64,
    /// 失效终点（unix 秒；null=永不过期）。
    not_after: Option<i64>,
}

/// 单把已解析公钥（运行时形态）。存 32 字节裸公钥；验签时构造 ring `UnparsedPublicKey`。
struct KeyEntry {
    pubkey: [u8; 32],
    purpose: KeyPurpose,
    status: KeyStatus,
    not_before: i64,
    not_after: Option<i64>,
}

/// Host 信任根：一组可验签的公钥。**只读**——不持任何私钥。
pub struct VerifyingKeyset {
    keys: HashMap<String, KeyEntry>,
}

impl VerifyingKeyset {
    /// 解析 + 校验 keyset JSON。任一公钥非法即整体拒绝（不做部分接受）。
    pub fn parse(json: &str) -> Result<Self, CryptoError> {
        let raw: RawKeyset =
            serde_json::from_str(json).map_err(|e| CryptoError::Parse(e.to_string()))?;
        if raw.schema != SUPPORTED_KEYSET_SCHEMA {
            return Err(CryptoError::UnsupportedSchema(raw.schema));
        }
        let mut keys = HashMap::with_capacity(raw.keys.len());
        for k in raw.keys {
            // 公钥用标准 base64（带填充）。32 字节 → VerifyingKey；非 32 字节或非法点 → 拒绝。
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(k.public_key_b64.trim())
                .map_err(|_| CryptoError::BadPublicKey(k.key_id.clone()))?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| CryptoError::BadPublicKey(k.key_id.clone()))?;
            // 注：ring 在 verify 时才校验公钥是否为合法曲线点；此处仅保证长度=32。
            keys.insert(
                k.key_id,
                KeyEntry {
                    pubkey: arr,
                    purpose: k.purpose,
                    status: k.status,
                    not_before: k.not_before,
                    not_after: k.not_after,
                },
            );
        }
        Ok(VerifyingKeyset { keys })
    }

    /// 解析内置生产公钥集（编译期嵌入）。内置数据应始终合法；失败即配置 bug。
    pub fn builtin() -> Result<Self, CryptoError> {
        Self::parse(BUILTIN_KEYSET_JSON)
    }

    /// 验证 `sig` 是 `key_id` 对 `msg` 的有效签名。
    ///
    /// 门控顺序（任一失败立即返回对应错误码）：
    /// 1. key_id 存在；2. 用途匹配；3. 未吊销；4. `now` 在 [not_before, not_after] 窗口内；
    /// 5. 签名长度 = 64；6. `verify_strict` 通过。
    pub fn verify(
        &self,
        key_id: &str,
        purpose: KeyPurpose,
        msg: &[u8],
        sig: &[u8],
        now: i64,
    ) -> Result<(), CryptoError> {
        let entry = self
            .keys
            .get(key_id)
            .ok_or_else(|| CryptoError::UnknownKey(key_id.to_string()))?;
        if entry.purpose != purpose {
            return Err(CryptoError::WrongPurpose {
                key_id: key_id.to_string(),
                expected: purpose,
            });
        }
        if entry.status == KeyStatus::Revoked {
            return Err(CryptoError::RevokedKey(key_id.to_string()));
        }
        if now < entry.not_before {
            return Err(CryptoError::KeyNotYetValid(key_id.to_string()));
        }
        if let Some(na) = entry.not_after {
            if now > na {
                return Err(CryptoError::KeyExpired(key_id.to_string()));
            }
        }
        if sig.len() != 64 {
            return Err(CryptoError::BadSignatureLen);
        }
        // ring 的 Ed25519 verify 遵循 RFC 8032 严格校验（拒非规范签名/非法公钥点）。
        let pk = ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            entry.pubkey.as_slice(),
        );
        pk.verify(msg, sig).map_err(|_| CryptoError::BadSignature)
    }

    /// 用**任一** `purpose` 用途的有效 key 验签——严格「验签先于解析」场景用（如签名 Registry：
    /// 整段 index bytes 都被签名，无需先解析出 key_id 即可验证，避免解析未验签内容）。
    /// 轮换期可能多把同用途 key；命中任一即通过。无可用候选 key 时返回 UnknownKey。
    pub fn verify_any(
        &self,
        purpose: KeyPurpose,
        msg: &[u8],
        sig: &[u8],
        now: i64,
    ) -> Result<(), CryptoError> {
        if sig.len() != 64 {
            return Err(CryptoError::BadSignatureLen);
        }
        let mut saw_candidate = false;
        for entry in self.keys.values() {
            if entry.purpose != purpose || entry.status == KeyStatus::Revoked {
                continue;
            }
            if now < entry.not_before {
                continue;
            }
            if let Some(na) = entry.not_after {
                if now > na {
                    continue;
                }
            }
            saw_candidate = true;
            let pk = ring::signature::UnparsedPublicKey::new(
                &ring::signature::ED25519,
                entry.pubkey.as_slice(),
            );
            if pk.verify(msg, sig).is_ok() {
                return Ok(());
            }
        }
        if saw_candidate {
            Err(CryptoError::BadSignature)
        } else {
            Err(CryptoError::UnknownKey(
                "<no active key for purpose>".into(),
            ))
        }
    }

    /// keyset 内全部 key_id（无授权旁路自检 / 诊断用）。
    pub fn key_ids(&self) -> impl Iterator<Item = &String> {
        self.keys.keys()
    }
}

/// base64url（**无填充**）解码——License token 的 payload/signature 两段（§5.2）。
pub fn b64url_decode(s: &str) -> Result<Vec<u8>, CryptoError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| CryptoError::Base64)
}

/// base64url（**无填充**）编码——签发工具/测试构造 token 用。
pub fn b64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 测试支撑：用确定性种子生成签名密钥并构造 keyset JSON。
///
/// **仅测试**：`SigningKey::from_bytes` 无需 CSPRNG（确定性），故测试不引入 rand。
/// 这些是**测试密钥**——绝不进入 Release。
///
/// 【Part6 §3.9.1a ③a】门控由 `#[cfg(test)]` 升为 `#[cfg(any(test, feature = "test-support"))]`：
/// crypto 迁出 src-tauri 后，src-tauri 的 installer/registry/package/install/license 测试仍需这些
/// 构造工具，而 `#[cfg(test)]` 项**不跨 crate 可见**。故经 `test-support` feature 暴露，由 src-tauri
/// 在 `[dev-dependencies]` 启用（生产依赖不启用 → 测试密钥仍绝不编入 Release）。
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// 由单字节种子确定性派生签名密钥对（测试可复现）。`from_seed_unchecked` 无需 CSPRNG。
    pub fn signing_key(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).expect("32 字节种子合法")
    }

    /// 用签名密钥对 `msg` 签名，返回 64 字节签名。
    pub fn sign(sk: &Ed25519KeyPair, msg: &[u8]) -> Vec<u8> {
        sk.sign(msg).as_ref().to_vec()
    }

    /// 单条 key 的 JSON 条目描述。
    pub struct KeySpec<'a> {
        pub key_id: &'a str,
        pub purpose: &'a str, // "release" | "license"
        pub sk: &'a Ed25519KeyPair,
        pub status: &'a str, // "active" | "revoked"
        pub not_before: i64,
        pub not_after: Option<i64>,
    }

    /// 把若干 [`KeySpec`] 拼成 keyset JSON 串（走真实 `parse` 路径，测试不绕过校验）。
    pub fn keyset_json(specs: &[KeySpec<'_>]) -> String {
        let entries: Vec<String> = specs
            .iter()
            .map(|s| {
                let pk = base64::engine::general_purpose::STANDARD
                    .encode(s.sk.public_key().as_ref());
                let na = match s.not_after {
                    Some(v) => v.to_string(),
                    None => "null".to_string(),
                };
                format!(
                    r#"{{"key_id":"{}","purpose":"{}","public_key_b64":"{}","status":"{}","not_before":{},"not_after":{}}}"#,
                    s.key_id, s.purpose, pk, s.status, s.not_before, na
                )
            })
            .collect();
        format!(r#"{{"schema":1,"keys":[{}]}}"#, entries.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    const NOW: i64 = 1_790_000_000; // 2026 年中，晚于内置/测试 not_before

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

    #[test]
    fn builtin_keyset_parses() {
        let ks = VerifyingKeyset::builtin().expect("内置 keyset 必须合法");
        // 注入无关断言:两种用途各至少一把 active 键——默认占位集、内测超集、
        // 真钥注入集(D1 ceremony 产物)都必须成立。
        let raw: RawKeyset =
            serde_json::from_str(BUILTIN_KEYSET_JSON).expect("内置 keyset JSON 必须可解析");
        for purpose in [KeyPurpose::Release, KeyPurpose::License] {
            assert!(
                raw.keys
                    .iter()
                    .any(|k| k.purpose == purpose && k.status == KeyStatus::Active),
                "内置 keyset 缺少 active 的 {purpose:?} 键"
            );
        }
        // 仅默认(未注入)构建再钉死占位 key_id,防占位集被误改。注入构建跳过——
        // 2026-07-06 D1 彩排实证:生产 keyset 只含 *-prod-* 键,若此处仍钉占位 id,
        // 真钥注入会在编译测试期红;而为过测把占位键并进生产信任根属倒因为果
        // (占位键与开源占位同值,等于给生产信任根留门),故断言按注入态门控。
        let injected = option_env!("PICASA_EXOTIC_KEYSET_FILE").is_some_and(|v| !v.is_empty());
        if !injected {
            assert!(ks.key_ids().any(|k| k == "release-2026-01"));
            assert!(ks.key_ids().any(|k| k == "license-2026-01"));
        }
    }

    /// 随仓 keyset 必须同时含正名 key_id 与历史别名:已发凭证可能持旧 key_id,别名须与正名同公钥值/
    /// 用途/状态/有效期,否则旧凭证失效。
    /// 断言直读**资源文件**(而非 builtin()):debug 构建会自取内测集、流水线可整组注入,两者都属
    /// 「整组替换」语义,不应要求替换集含占位别名;这里锁的是随仓发布的那一份。
    #[test]
    fn shipped_keyset_preserves_pro_key_id_aliases() {
        let raw: RawKeyset = serde_json::from_str(include_str!("../resources/exotic-keyset.json"))
            .expect("随仓 keyset JSON 必须可解析");
        let key_of = |id: &str| {
            raw.keys
                .iter()
                .find(|k| k.key_id == id)
                .unwrap_or_else(|| panic!("随仓 keyset 缺 key_id:{id}"))
        };
        for (canonical, alias, purpose) in [
            (
                "release-2026-01",
                "release-prod-placeholder",
                KeyPurpose::Release,
            ),
            (
                "license-2026-01",
                "license-prod-placeholder",
                KeyPurpose::License,
            ),
        ] {
            let (c, a) = (key_of(canonical), key_of(alias));
            assert_eq!((c.purpose, a.purpose), (purpose, purpose));
            assert_eq!(
                (c.status, a.status),
                (KeyStatus::Active, KeyStatus::Active),
                "别名与正名都须 active:{alias}"
            );
            assert_eq!(
                c.public_key_b64, a.public_key_b64,
                "别名 {alias} 与正名 {canonical} 必须同一公钥(防抄写串错)"
            );
            assert_eq!(
                (c.not_before, c.not_after),
                (a.not_before, a.not_after),
                "别名 {alias} 的有效期须与正名一致"
            );
        }
    }

    /// 无授权旁路自检（§5.4）：Release 信任根不得含测试 key。
    /// 1) key_id 不得带 test/dev 字样；2) 内置公钥**不得**等于任何确定性测试种子派生的公钥——
    ///    否则其私钥即公开（`from_seed_unchecked([s;32])`），任何人可伪造合法授权 token。
    #[test]
    fn builtin_keyset_has_no_test_keys() {
        use base64::Engine as _;
        use ring::signature::KeyPair as _;

        let ks = VerifyingKeyset::builtin().unwrap();
        for id in ks.key_ids() {
            assert!(
                !id.contains("test") && !id.contains("dev"),
                "Release 信任根不得含测试/开发 key：{id}"
            );
        }

        // 收集**全 u8 域**测试种子派生的公钥（base64 标准编码）。
        // 注：本自检只覆盖 `from_seed_unchecked([s;32])` 模式；其他来源固定密钥须靠人工/CI 审计。
        let test_pubs: Vec<String> = (0u8..=255)
            .map(|s| {
                base64::engine::general_purpose::STANDARD
                    .encode(signing_key(s).public_key().as_ref())
            })
            .collect();
        // 解析内置 JSON 取各公钥串，逐一比对。
        let raw: super::RawKeyset = serde_json::from_str(super::BUILTIN_KEYSET_JSON).unwrap();
        for k in &raw.keys {
            assert!(
                !test_pubs.contains(&k.public_key_b64),
                "内置公钥 {} 的私钥为公开测试种子 → 授权可被伪造",
                k.key_id
            );
        }
    }

    #[test]
    fn valid_signature_verifies() {
        let sk = signing_key(1);
        let ks = release_keyset(&sk);
        let msg = b"hello registry index bytes";
        let sig = sign(&sk, msg);
        assert!(ks
            .verify("release-test", KeyPurpose::Release, msg, &sig, NOW)
            .is_ok());
    }

    #[test]
    fn tampered_message_fails() {
        let sk = signing_key(2);
        let ks = release_keyset(&sk);
        let sig = sign(&sk, b"original");
        assert_eq!(
            ks.verify("release-test", KeyPurpose::Release, b"tampered", &sig, NOW),
            Err(CryptoError::BadSignature)
        );
    }

    #[test]
    fn tampered_signature_fails() {
        let sk = signing_key(3);
        let ks = release_keyset(&sk);
        let msg = b"payload";
        let mut sig = sign(&sk, msg);
        sig[0] ^= 0xff; // 翻一位
        assert_eq!(
            ks.verify("release-test", KeyPurpose::Release, msg, &sig, NOW),
            Err(CryptoError::BadSignature)
        );
    }

    #[test]
    fn wrong_signing_key_fails() {
        let sk = signing_key(4);
        let other = signing_key(5);
        let ks = release_keyset(&sk);
        let msg = b"payload";
        let sig = sign(&other, msg); // 用别的私钥签
        assert_eq!(
            ks.verify("release-test", KeyPurpose::Release, msg, &sig, NOW),
            Err(CryptoError::BadSignature)
        );
    }

    #[test]
    fn unknown_key_id_fails() {
        let sk = signing_key(6);
        let ks = release_keyset(&sk);
        let sig = sign(&sk, b"x");
        assert!(matches!(
            ks.verify("nope", KeyPurpose::Release, b"x", &sig, NOW),
            Err(CryptoError::UnknownKey(_))
        ));
    }

    #[test]
    fn wrong_purpose_fails() {
        // license key 不能验 release 物（信任根分离）。
        let sk = signing_key(7);
        let json = keyset_json(&[KeySpec {
            key_id: "k",
            purpose: "license",
            sk: &sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let msg = b"a registry pretending";
        let sig = sign(&sk, msg);
        assert!(matches!(
            ks.verify("k", KeyPurpose::Release, msg, &sig, NOW),
            Err(CryptoError::WrongPurpose { .. })
        ));
    }

    #[test]
    fn revoked_key_fails() {
        let sk = signing_key(8);
        let json = keyset_json(&[KeySpec {
            key_id: "k",
            purpose: "release",
            sk: &sk,
            status: "revoked",
            not_before: 0,
            not_after: None,
        }]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let msg = b"x";
        let sig = sign(&sk, msg);
        assert!(matches!(
            ks.verify("k", KeyPurpose::Release, msg, &sig, NOW),
            Err(CryptoError::RevokedKey(_))
        ));
    }

    #[test]
    fn key_window_enforced() {
        let sk = signing_key(9);
        let json = keyset_json(&[KeySpec {
            key_id: "k",
            purpose: "release",
            sk: &sk,
            status: "active",
            not_before: 1000,
            not_after: Some(2000),
        }]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let msg = b"x";
        let sig = sign(&sk, msg);
        // 太早。
        assert!(matches!(
            ks.verify("k", KeyPurpose::Release, msg, &sig, 999),
            Err(CryptoError::KeyNotYetValid(_))
        ));
        // 太晚。
        assert!(matches!(
            ks.verify("k", KeyPurpose::Release, msg, &sig, 2001),
            Err(CryptoError::KeyExpired(_))
        ));
        // 窗口内。
        assert!(ks.verify("k", KeyPurpose::Release, msg, &sig, 1500).is_ok());
    }

    #[test]
    fn bad_signature_length() {
        let sk = signing_key(10);
        let ks = release_keyset(&sk);
        assert_eq!(
            ks.verify("release-test", KeyPurpose::Release, b"x", &[0u8; 10], NOW),
            Err(CryptoError::BadSignatureLen)
        );
    }

    #[test]
    fn verify_any_handles_rotation() {
        // 轮换期：keyset 含两把 release key；token 由第二把签 → verify_any 仍命中。
        let sk1 = signing_key(20);
        let sk2 = signing_key(21);
        let json = keyset_json(&[
            KeySpec {
                key_id: "release-old",
                purpose: "release",
                sk: &sk1,
                status: "active",
                not_before: 0,
                not_after: None,
            },
            KeySpec {
                key_id: "release-new",
                purpose: "release",
                sk: &sk2,
                status: "active",
                not_before: 0,
                not_after: None,
            },
        ]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let msg = b"signed registry index";
        let sig = sign(&sk2, msg);
        assert!(ks.verify_any(KeyPurpose::Release, msg, &sig, NOW).is_ok());
        // 用途不符（无 license 候选）→ UnknownKey。
        assert!(matches!(
            ks.verify_any(KeyPurpose::License, msg, &sig, NOW),
            Err(CryptoError::UnknownKey(_))
        ));
        // 错误签名 → BadSignature。
        let mut bad = sig.clone();
        bad[0] ^= 0xff;
        assert_eq!(
            ks.verify_any(KeyPurpose::Release, msg, &bad, NOW),
            Err(CryptoError::BadSignature)
        );
    }

    #[test]
    fn b64url_roundtrip() {
        let data = b"\x00\x01\xfe\xff payload~with+url/unsafe=chars";
        let enc = b64url_encode(data);
        // 无填充、无 +/。
        assert!(!enc.contains('='));
        assert!(!enc.contains('+'));
        assert!(!enc.contains('/'));
        assert_eq!(b64url_decode(&enc).unwrap(), data);
    }

    #[test]
    fn reject_bad_public_key() {
        // 公钥非 32 字节 → 整体拒绝。
        let json = r#"{"schema":1,"keys":[{"key_id":"k","purpose":"release","public_key_b64":"YWJj","status":"active","not_before":0,"not_after":null}]}"#;
        assert!(matches!(
            VerifyingKeyset::parse(json),
            Err(CryptoError::BadPublicKey(_))
        ));
    }

    #[test]
    fn reject_unsupported_schema() {
        let json = r#"{"schema":9,"keys":[]}"#;
        assert!(matches!(
            VerifyingKeyset::parse(json),
            Err(CryptoError::UnsupportedSchema(9))
        ));
    }
}
