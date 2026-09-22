//! 图片编辑高级功能授权门。
//!
//! 编辑仍为宿主内建能力，使用官方版套装授权，不进入格式目录。

use crate::error::{AppError, Result};
use crate::exotic::{EntitlementProvider, LicenseStatus};
use crate::official;

pub const CODE_NOT_ENTITLED: &str = "edit_not_entitled";

/// 后端真门：无法证明授权时一律拒绝，前端 gate 只负责购买/激活体验。
pub fn require_editing_entitlement(provider: &dyn EntitlementProvider) -> Result<()> {
    match official::status(provider, official::now_secs()) {
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
    use crate::exotic::license::LicenseError;
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
            assert_eq!(plugin_id, &official::product().product_id);
            assert_eq!(sku, &official::product().sku);
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
        ) -> std::result::Result<(), LicenseError> {
            *self.status.lock().expect("fake provider lock") = LicenseStatus::Authorized;
            Ok(())
        }

        fn deactivate(&self, _plugin_id: &str) -> std::result::Result<(), LicenseError> {
            *self.status.lock().expect("fake provider lock") = LicenseStatus::Unlicensed;
            Ok(())
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
            .activate(
                &official::product().product_id,
                &official::product().sku,
                "test",
                123,
            )
            .expect("activate fake provider");
        require_editing_entitlement(&provider).expect("已授权须放行");

        provider
            .deactivate(&official::product().product_id)
            .expect("deactivate fake provider");
        assert!(require_editing_entitlement(&provider).is_err());
    }
}
