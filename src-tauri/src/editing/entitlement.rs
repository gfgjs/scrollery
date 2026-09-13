//! 图片编辑高级功能授权门。
//!
//! 编辑代码编译在 Host 内，不是可下载格式插件；这里以固定的虚拟 feature id / SKU
//! 复用 exotic 的 [`EntitlementProvider`]，不伪造 catalog offering 或安装记录。

use crate::error::{AppError, Result};
use crate::exotic::{Availability, EntitlementProvider, LicenseStatus, PluginEntitlement};

/// 内建图片编辑功能的稳定授权主体。字符集与现有 plugin id 契约一致。
pub const EDITING_PLUGIN_ID: &str = "feature-editing";
/// SKU 只来自编译期可信常量，绝不接受前端或 token 提供的值。
pub const EDITING_SKU: &str = "editing-tools-2026";
pub const CODE_NOT_ENTITLED: &str = "edit_not_entitled";

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 把共享 provider 的授权真相投影为既有 `PluginGate` DTO。
pub fn editing_entitlement(provider: &dyn EntitlementProvider) -> PluginEntitlement {
    editing_entitlement_at(provider, now_secs())
}

fn editing_entitlement_at(provider: &dyn EntitlementProvider, now: i64) -> PluginEntitlement {
    let availability = match provider.evaluate(EDITING_PLUGIN_ID, EDITING_SKU, now) {
        LicenseStatus::Authorized => Availability::Authorized,
        LicenseStatus::Expired => Availability::LicenseExpired,
        LicenseStatus::Unlicensed | LicenseStatus::KeyringUnavailable => {
            Availability::InstalledUnlicensed
        }
    };
    PluginEntitlement {
        plugin_id: EDITING_PLUGIN_ID.to_string(),
        availability,
        source_tag: provider.source_tag().to_string(),
        sku: Some(EDITING_SKU.to_string()),
        // 定价与生产商店坐标归发行线；没有可信 URL 时按钮保持禁用，不编造地址。
        store_url: None,
    }
}

/// 后端真门：无法证明授权时一律拒绝，前端 gate 只负责购买/激活体验。
pub fn require_editing_entitlement(provider: &dyn EntitlementProvider) -> Result<()> {
    match provider.evaluate(EDITING_PLUGIN_ID, EDITING_SKU, now_secs()) {
        LicenseStatus::Authorized => Ok(()),
        LicenseStatus::Expired | LicenseStatus::Unlicensed | LicenseStatus::KeyringUnavailable => {
            Err(AppError::Edit {
                code: CODE_NOT_ENTITLED,
                message: "图片编辑高级功能尚未授权 | image editing is not entitled".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::license::{ActivationInfo, LicenseError};
    use std::sync::Mutex;

    struct FakeProvider {
        status: Mutex<LicenseStatus>,
    }

    impl FakeProvider {
        fn new(status: LicenseStatus) -> Self {
            Self {
                status: Mutex::new(status),
            }
        }
    }

    impl EntitlementProvider for FakeProvider {
        fn evaluate(&self, plugin_id: &str, sku: &str, _now: i64) -> LicenseStatus {
            assert_eq!(plugin_id, EDITING_PLUGIN_ID);
            assert_eq!(sku, EDITING_SKU);
            *self.status.lock().expect("fake provider lock")
        }

        fn source_tag(&self) -> &'static str {
            "test"
        }

        fn activate(
            &self,
            _plugin_id: &str,
            _sku: &str,
            _credential: &str,
            _now: i64,
        ) -> std::result::Result<ActivationInfo, LicenseError> {
            *self.status.lock().expect("fake provider lock") = LicenseStatus::Authorized;
            Ok(ActivationInfo { enc_seed: None })
        }

        fn deactivate(&self, _plugin_id: &str) -> std::result::Result<(), LicenseError> {
            *self.status.lock().expect("fake provider lock") = LicenseStatus::Unlicensed;
            Ok(())
        }
    }

    #[test]
    fn entitlement_maps_all_provider_states_fail_closed() {
        let cases = [
            (LicenseStatus::Authorized, Availability::Authorized),
            (LicenseStatus::Expired, Availability::LicenseExpired),
            (LicenseStatus::Unlicensed, Availability::InstalledUnlicensed),
            (
                LicenseStatus::KeyringUnavailable,
                Availability::InstalledUnlicensed,
            ),
        ];
        for (status, expected) in cases {
            let provider = FakeProvider::new(status);
            let entitlement = editing_entitlement_at(&provider, 123);
            assert_eq!(entitlement.plugin_id, EDITING_PLUGIN_ID);
            assert_eq!(entitlement.sku.as_deref(), Some(EDITING_SKU));
            assert_eq!(entitlement.availability, expected);
            assert_eq!(entitlement.source_tag, "test");
            assert!(entitlement.store_url.is_none());
        }
    }

    #[test]
    fn activation_and_revocation_change_backend_gate() {
        let provider = FakeProvider::new(LicenseStatus::Unlicensed);
        let err = require_editing_entitlement(&provider).expect_err("未授权须拒绝");
        assert!(matches!(
            err,
            AppError::Edit {
                code: CODE_NOT_ENTITLED,
                ..
            }
        ));

        provider
            .activate(EDITING_PLUGIN_ID, EDITING_SKU, "test", 123)
            .expect("activate fake provider");
        require_editing_entitlement(&provider).expect("已授权须放行");

        provider
            .deactivate(EDITING_PLUGIN_ID)
            .expect("deactivate fake provider");
        assert!(require_editing_entitlement(&provider).is_err());
    }
}
