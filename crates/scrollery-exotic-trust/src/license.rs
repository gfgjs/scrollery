// crates/scrollery-exotic-trust/src/license.rs
//! 冷门格式插件 · License token 验签与「授权真相」纯函数（v3 Part3 §5.2/§5.3）。
//!
//! 【Part6 §3.9.1a 去环 ③a】本模块的**纯验签逻辑**（`verify_token`/`evaluate_token`/`LicensePayload`）
//! 自 `src-tauri/src/exotic/license.rs` 迁入本叶 crate——无秘密价值（算法公开、公钥非秘密）。真实
//! keyring I/O 实现 `KeyringLicenseStore` **留在 src-tauri**（依赖 keyring crate，是直销渠道
//! 唯一实现）。`LicenseStatus`/`LicenseError` DTO 仍住更底层的叶 crate `scrollery-plugin-api`。
//!
//! token 编码（§5.2）：`base64url(payload_json_utf8) + "." + base64url(ed25519_signature)`。
//! 签名覆盖**收到的原始 payload bytes**——Verifier **不**重新序列化后验签（§5.2 核心约定）。
//!
//! 校验顺序（§5.2，即防御纵深）：结构/大小 → version → `verify_strict` → plugin_id → sku → 时间窗。

use serde::{Deserialize, Serialize};

use crate::crypto::{b64url_decode, CryptoError, KeyPurpose, VerifyingKeyset};
use scrollery_plugin_api::{LicenseError, LicenseStatus};

/// 本 Host 支持的 token 结构版本（§5.2 / R11：先于验签门控解析）。
const SUPPORTED_TOKEN_VERSION: u32 = 1;
/// token 总长上限（防超大输入；正常 token 仅几百字节）。
const MAX_TOKEN_LEN: usize = 8 * 1024;
/// payload JSON 字节上限。
const MAX_PAYLOAD_LEN: usize = 4 * 1024;

/// License payload（验签通过后才可信）。字段顺序由签发工具稳定；安全性不依赖重序列化（§5.2）。
///
/// **不** derive `Debug`——手写 [`std::fmt::Debug`] 把 `subject_hash` 脱敏为 `<redacted>`，
/// 使 `{:?}` / panic 回溯 / `tracing` 不会泄露订阅主体散列（§5.2「绝不输出 subject_hash」，
/// 安全评审 medium）。`Serialize` 保留供签发工具/测试构造签名字节；跨 IPC 下发须经投影结构
/// （Part3 激活命令返回不含 subject_hash 的 DTO，见 `KeyringLicenseStore::activate` 注释）。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensePayload {
    pub version: u32,
    pub key_id: String,
    pub license_id: String,
    pub plugin_id: String,
    pub sku: String,
    /// 订阅主体散列（可空）。**绝不**进日志/遥测。
    #[serde(default)]
    pub subject_hash: Option<String>,
    pub issued_at: i64,
    pub not_before: i64,
    /// null = 永久授权（v3 首发主路径，§5.2）。
    pub expires_at: Option<i64>,
}

impl std::fmt::Debug for LicensePayload {
    /// 脱敏 `subject_hash`（§5.2）：即便被 `{:?}` 或 panic 回溯打印也不泄露主体散列。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LicensePayload")
            .field("version", &self.version)
            .field("key_id", &self.key_id)
            .field("license_id", &self.license_id)
            .field("plugin_id", &self.plugin_id)
            .field("sku", &self.sku)
            .field(
                "subject_hash",
                &self.subject_hash.as_ref().map(|_| "<redacted>"),
            )
            .field("issued_at", &self.issued_at)
            .field("not_before", &self.not_before)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// 密码学层错误 → License 错误：除「未知 key」单列外，其余（用途不符/吊销/key 窗口/签名）
/// 一律折叠为 `BadSignature`——对外不暴露密钥内部状态，避免逆向探测信任根细节。
fn map_crypto(e: CryptoError) -> LicenseError {
    match e {
        CryptoError::UnknownKey(_) => LicenseError::UnknownKey,
        _ => LicenseError::BadSignature,
    }
}

/// 验证 License token 并返回 payload（§5.2 全序）。**纯函数**：不碰 keyring，便于穷举测试。
///
/// `keyset` 须含 `purpose=license` 的签名公钥；`expected_plugin_id/expected_sku` 来自
/// 安装 manifest / Catalog（不可由前端任意传入）；`now` 为 unix 秒。
pub fn verify_token(
    token: &str,
    keyset: &VerifyingKeyset,
    expected_plugin_id: &str,
    expected_sku: &str,
    now: i64,
) -> Result<LicensePayload, LicenseError> {
    // 1. 大小。
    if token.len() > MAX_TOKEN_LEN {
        return Err(LicenseError::Malformed("token 超长".into()));
    }
    // 2. 结构：恰好两段 `payload.sig`，均非空。
    let mut parts = token.split('.');
    let (Some(p0), Some(p1), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(LicenseError::Malformed("须为 payload.sig 两段".into()));
    };
    if p0.is_empty() || p1.is_empty() {
        return Err(LicenseError::Malformed("空段".into()));
    }
    // 3. base64url 解码。**签名覆盖 payload_bytes 原始字节**——后续验签直接用之，不重序列化。
    let payload_bytes =
        b64url_decode(p0).map_err(|_| LicenseError::Malformed("payload base64".into()))?;
    let sig_bytes = b64url_decode(p1).map_err(|_| LicenseError::Malformed("sig base64".into()))?;
    if payload_bytes.len() > MAX_PAYLOAD_LEN {
        return Err(LicenseError::Malformed("payload 超长".into()));
    }
    // 4. version 门控**先于完整解析**（R11）：只取最小 {version}，避免对未知版本结构做完整反序列化，
    //    使版本迁移期能干净区分 UnsupportedVersion 与 Malformed。
    #[derive(Deserialize)]
    struct VersionProbe {
        version: u32,
    }
    let probe: VersionProbe = serde_json::from_slice(&payload_bytes)
        .map_err(|e| LicenseError::Malformed(format!("payload version：{e}")))?;
    if probe.version != SUPPORTED_TOKEN_VERSION {
        return Err(LicenseError::UnsupportedVersion(probe.version));
    }
    // 5. 完整解析 payload（版本已确认；仅为读字段，信任建立在第 6 步验签）。
    let payload: LicensePayload = serde_json::from_slice(&payload_bytes)
        .map_err(|e| LicenseError::Malformed(format!("payload json：{e}")))?;
    // 6. 验签：用原始 payload_bytes；key 须为 license 用途、未吊销、在有效窗口内。
    //    对外错误码折叠（map_crypto），但内部 warn 保留密码学层细码以利安全可观测性
    //    （区分「跨用途伪造尝试」与「key 轮换吊销」等，安全评审 low）。不输出 token/key 材料。
    keyset
        .verify(
            &payload.key_id,
            KeyPurpose::License,
            &payload_bytes,
            &sig_bytes,
            now,
        )
        .map_err(|e| {
            tracing::warn!(
                "exotic License 验签失败：crypto_code={} key_id={:?}",
                e.code(),
                payload.key_id
            );
            map_crypto(e)
        })?;
    // 7. 业务字段：plugin_id → sku（验签后才比对，防止未验证字段被用作决策）。
    if payload.plugin_id != expected_plugin_id {
        return Err(LicenseError::PluginMismatch);
    }
    if payload.sku != expected_sku {
        return Err(LicenseError::SkuMismatch);
    }
    // 8. 时间窗：not_before → expires_at。
    if now < payload.not_before {
        return Err(LicenseError::NotYetValid);
    }
    if let Some(exp) = payload.expires_at {
        if now > exp {
            return Err(LicenseError::Expired);
        }
    }
    Ok(payload)
}

/// 由「可选 token」纯函数地评估授权态。keyring I/O 抽到外层 → 本函数离线可测。
pub fn evaluate_token(
    token: Option<&str>,
    keyset: &VerifyingKeyset,
    plugin_id: &str,
    sku: &str,
    now: i64,
) -> LicenseStatus {
    match token {
        None => LicenseStatus::Unlicensed,
        Some(t) => match verify_token(t, keyset, plugin_id, sku, now) {
            Ok(_) => LicenseStatus::Authorized,
            Err(LicenseError::Expired) => LicenseStatus::Expired,
            // 其余（未授权/不匹配/畸形/未知 key/未生效）一律按未授权处理。
            Err(_) => LicenseStatus::Unlicensed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::test_support::{keyset_json, signing_key, KeySpec};
    use crate::crypto::{b64url_encode, VerifyingKeyset};
    use ring::signature::Ed25519KeyPair;

    const NOW: i64 = 1_790_000_000;
    const PLUGIN: &str = "exotic-image-psd";
    const SKU: &str = "psd-engine-2026";

    /// 用 license key 签发一个 token（测试签发工具）。
    fn make_token(sk: &Ed25519KeyPair, payload: &LicensePayload) -> String {
        let bytes = serde_json::to_vec(payload).unwrap();
        let sig = sk.sign(&bytes);
        format!("{}.{}", b64url_encode(&bytes), b64url_encode(sig.as_ref()))
    }

    fn payload(key_id: &str) -> LicensePayload {
        LicensePayload {
            version: 1,
            key_id: key_id.to_string(),
            license_id: "lic_test".into(),
            plugin_id: PLUGIN.into(),
            sku: SKU.into(),
            subject_hash: None,
            issued_at: 1_700_000_000,
            not_before: 1_700_000_000,
            expires_at: None,
        }
    }

    /// 单 license key 的 keyset（确定性签名密钥）。
    fn license_keyset(sk: &Ed25519KeyPair) -> VerifyingKeyset {
        let json = keyset_json(&[KeySpec {
            key_id: "license-test",
            purpose: "license",
            sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        VerifyingKeyset::parse(&json).unwrap()
    }

    #[test]
    fn valid_permanent_token() {
        let sk = signing_key(1);
        let ks = license_keyset(&sk);
        let tok = make_token(&sk, &payload("license-test"));
        let p = verify_token(&tok, &ks, PLUGIN, SKU, NOW).unwrap();
        assert_eq!(p.plugin_id, PLUGIN);
        assert_eq!(p.sku, SKU);
    }

    #[test]
    fn tampered_payload_fails_signature() {
        // 用 plugin A 的 bytes 签名，再把 sig 拼到 plugin B 的 payload 上 → 验签失败。
        let sk = signing_key(2);
        let ks = license_keyset(&sk);
        let a = payload("license-test");
        let a_bytes = serde_json::to_vec(&a).unwrap();
        let sig = sk.sign(&a_bytes);
        let mut b = payload("license-test");
        b.license_id = "lic_TAMPERED".into();
        let b_bytes = serde_json::to_vec(&b).unwrap();
        let forged = format!(
            "{}.{}",
            b64url_encode(&b_bytes),
            b64url_encode(sig.as_ref())
        );
        assert_eq!(
            verify_token(&forged, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn unknown_key_id() {
        let sk = signing_key(3);
        let ks = license_keyset(&sk);
        let tok = make_token(&sk, &payload("some-other-key"));
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::UnknownKey)
        );
    }

    #[test]
    fn wrong_purpose_release_key_rejected() {
        // 用 release 用途的 key 签 License → 验签要求 license 用途 → BadSignature（折叠 WrongPurpose）。
        let sk = signing_key(4);
        let json = keyset_json(&[KeySpec {
            key_id: "license-test",
            purpose: "release",
            sk: &sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        let ks = VerifyingKeyset::parse(&json).unwrap();
        let tok = make_token(&sk, &payload("license-test"));
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn plugin_mismatch() {
        let sk = signing_key(5);
        let ks = license_keyset(&sk);
        let tok = make_token(&sk, &payload("license-test"));
        assert_eq!(
            verify_token(&tok, &ks, "exotic-other", SKU, NOW),
            Err(LicenseError::PluginMismatch)
        );
    }

    #[test]
    fn sku_mismatch() {
        let sk = signing_key(6);
        let ks = license_keyset(&sk);
        let tok = make_token(&sk, &payload("license-test"));
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, "wrong-sku", NOW),
            Err(LicenseError::SkuMismatch)
        );
    }

    #[test]
    fn not_yet_valid() {
        let sk = signing_key(7);
        let ks = license_keyset(&sk);
        let mut p = payload("license-test");
        p.not_before = NOW + 1000;
        let tok = make_token(&sk, &p);
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::NotYetValid)
        );
    }

    #[test]
    fn expired() {
        let sk = signing_key(8);
        let ks = license_keyset(&sk);
        let mut p = payload("license-test");
        p.expires_at = Some(NOW - 1);
        let tok = make_token(&sk, &p);
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::Expired)
        );
    }

    #[test]
    fn unsupported_version() {
        let sk = signing_key(9);
        let ks = license_keyset(&sk);
        let mut p = payload("license-test");
        p.version = 2;
        let tok = make_token(&sk, &p);
        assert_eq!(
            verify_token(&tok, &ks, PLUGIN, SKU, NOW),
            Err(LicenseError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn malformed_structures() {
        let sk = signing_key(10);
        let ks = license_keyset(&sk);
        for bad in ["no-dot", "a.b.c", "onlyone.", ".onlysig", "@@@.@@@"] {
            assert!(
                matches!(
                    verify_token(bad, &ks, PLUGIN, SKU, NOW),
                    Err(LicenseError::Malformed(_))
                ),
                "应判 Malformed：{bad}"
            );
        }
    }

    #[test]
    fn evaluate_token_maps_status() {
        let sk = signing_key(11);
        let ks = license_keyset(&sk);
        // 无 token → Unlicensed。
        assert_eq!(
            evaluate_token(None, &ks, PLUGIN, SKU, NOW),
            LicenseStatus::Unlicensed
        );
        // 有效 → Authorized。
        let tok = make_token(&sk, &payload("license-test"));
        assert_eq!(
            evaluate_token(Some(&tok), &ks, PLUGIN, SKU, NOW),
            LicenseStatus::Authorized
        );
        // 过期 → Expired。
        let mut p = payload("license-test");
        p.expires_at = Some(NOW - 1);
        let exp = make_token(&sk, &p);
        assert_eq!(
            evaluate_token(Some(&exp), &ks, PLUGIN, SKU, NOW),
            LicenseStatus::Expired
        );
        // 篡改 → Unlicensed。
        assert_eq!(
            evaluate_token(Some("garbage.token"), &ks, PLUGIN, SKU, NOW),
            LicenseStatus::Unlicensed
        );
    }
}
