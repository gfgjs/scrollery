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
