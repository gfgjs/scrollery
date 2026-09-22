//! 官方版一次购买授权；权益列表由宿主固定，格式安装和模型准备各自独立。

use std::sync::LazyLock;

use serde::Deserialize;

use crate::exotic::{Availability, EntitlementProvider, LicenseStatus, PluginEntitlement};

/// 官方版商品元数据，只接受编译期资源。
#[derive(Deserialize)]
pub struct Product {
    pub product_id: String,
    pub sku: String,
    pub sales_open: bool,
    pub store_url: Option<String>,
}

/// 编译进宿主的商品配置，前端和 token 均不能改变授权主体。
pub fn product() -> &'static Product {
    static PRODUCT: LazyLock<Product> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../resources/official-product.json"))
            .expect("official-product.json must be valid")
    });
    &PRODUCT
}

/// 只有明确属于官方版的能力才能消费套装授权。
pub fn includes(feature: &str) -> bool {
    matches!(
        feature,
        "feature-editing" | "exotic-ocr" | "exotic-enhance" | "exotic-image-psd"
    )
}

/// 拒绝开发占位地址；未开放销售时不向任何入口暴露购买链接。
pub fn store_url() -> Option<String> {
    let p = product();
    p.sales_open
        .then_some(p.store_url.as_deref())
        .flatten()
        .filter(|s| valid_store_url(s))
        .map(str::to_owned)
}

/// 校验购买地址的协议、主机和凭据，排除占位地址及裸 IP。
pub fn valid_store_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && host != "localhost"
        && !host.contains(':')
        && !host.ends_with(".localhost")
        && ![
            "invalid",
            "example",
            "test",
            "example.com",
            "example.net",
            "example.org",
        ]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        && host.parse::<std::net::IpAddr>().is_err()
}

/// 授权时间窗使用的 Unix 秒数。
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 从同一主体和 SKU 读取套装授权状态。
pub fn status(provider: &dyn EntitlementProvider, now: i64) -> LicenseStatus {
    let p = product();
    provider.evaluate(&p.product_id, &p.sku, now)
}

/// 供界面消费的授权快照；系统凭据不可读时返回查询错误。
pub fn entitlement(provider: &dyn EntitlementProvider) -> crate::error::Result<PluginEntitlement> {
    let availability = match status(provider, now_secs()) {
        LicenseStatus::Authorized => Availability::Authorized,
        LicenseStatus::Expired => Availability::LicenseExpired,
        LicenseStatus::Unlicensed => Availability::InstalledUnlicensed,
        LicenseStatus::KeyringUnavailable => {
            return Err(crate::error::AppError::Exotic {
                code: "keyring_unavailable",
                message: "无法读取本机授权，请重试".into(),
            })
        }
    };
    Ok(PluginEntitlement {
        plugin_id: product().product_id.clone(),
        availability,
        source_tag: provider.source_tag().into(),
        sku: Some(product().sku.clone()),
        store_url: store_url(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purchase_url_rejects_placeholders_and_non_web_targets() {
        for raw in [
            "https://example.invalid/plugins/psd",
            "https://shop.example.com",
            "http://shop.scrollery.app",
            "file:///c:/x",
            "https://localhost",
            "https://127.0.0.1",
            "https://[::1]",
            "https://u:p@shop.scrollery.app",
        ] {
            assert!(!valid_store_url(raw), "{raw}");
        }
        assert!(valid_store_url(
            "https://shop.scrollery.app/products/official"
        ));
        assert!(!product().sales_open);
        assert!(store_url().is_none());
    }

    #[test]
    fn only_four_features_belong_to_official_license() {
        for feature in [
            "feature-editing",
            "exotic-ocr",
            "exotic-enhance",
            "exotic-image-psd",
        ] {
            assert!(includes(feature));
        }
        for feature in ["exotic-image-raw", "video-extended", "future-plugin", ""] {
            assert!(!includes(feature));
        }
    }

    struct StatusProvider(LicenseStatus);
    impl EntitlementProvider for StatusProvider {
        fn evaluate(&self, id: &str, sku: &str, _: i64) -> LicenseStatus {
            assert_eq!(id, product().product_id);
            assert_eq!(sku, product().sku);
            self.0
        }
        fn source_tag(&self) -> &'static str {
            "test"
        }
        fn activate(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: i64,
        ) -> std::result::Result<(), crate::exotic::license::LicenseError> {
            unreachable!()
        }
        fn deactivate(
            &self,
            _: &str,
        ) -> std::result::Result<(), crate::exotic::license::LicenseError> {
            unreachable!()
        }
    }

    #[test]
    fn status_keeps_query_failure_distinct_from_missing_license() {
        for (input, expected) in [
            (LicenseStatus::Authorized, Availability::Authorized),
            (LicenseStatus::Unlicensed, Availability::InstalledUnlicensed),
            (LicenseStatus::Expired, Availability::LicenseExpired),
        ] {
            let provider = StatusProvider(input);
            let result = entitlement(&provider).unwrap();
            assert_eq!(result.plugin_id, product().product_id);
            assert_eq!(result.availability, expected);
            assert_eq!(
                crate::editing::entitlement::require_editing_entitlement(&provider).is_ok(),
                input == LicenseStatus::Authorized
            );
        }
        assert!(entitlement(&StatusProvider(LicenseStatus::KeyringUnavailable)).is_err());
        assert!(
            crate::editing::entitlement::require_editing_entitlement(&StatusProvider(
                LicenseStatus::KeyringUnavailable
            ))
            .is_err()
        );
    }

    #[test]
    fn signed_official_token_is_valid_and_old_single_feature_token_is_rejected() {
        use crate::exotic::crypto::{
            test_support::{keyset_json, sign, signing_key, KeySpec},
            VerifyingKeyset,
        };
        use crate::exotic::license::verify_token;
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let key = signing_key(69);
        let keyset = VerifyingKeyset::parse(&keyset_json(&[KeySpec {
            key_id: "test-official",
            purpose: "license",
            sk: &key,
            status: "active",
            not_before: 0,
            not_after: None,
        }]))
        .unwrap();
        let token = |id: &str, sku: &str| {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "version":1,"key_id":"test-official","license_id":"test-license",
                "plugin_id":id,"sku":sku,"issued_at":0,"not_before":0,"expires_at":null
            }))
            .unwrap();
            format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(&bytes),
                URL_SAFE_NO_PAD.encode(sign(&key, &bytes))
            )
        };
        assert!(verify_token(
            &token(&product().product_id, &product().sku),
            &keyset,
            &product().product_id,
            &product().sku,
            100
        )
        .is_ok());
        assert!(verify_token(
            &token("feature-editing", "editing-tools-2026"),
            &keyset,
            &product().product_id,
            &product().sku,
            100
        )
        .is_err());
        assert!(verify_token(
            &token("exotic-image-psd", "psd-engine-2026"),
            &keyset,
            &product().product_id,
            &product().sku,
            100
        )
        .is_err());
    }
}
