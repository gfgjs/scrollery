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
