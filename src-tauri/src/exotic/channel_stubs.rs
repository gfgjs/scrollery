// src-tauri/src/exotic/channel_stubs.rs
//! 渠道授权 Provider 骨架桩 + 未授权回退桩(Part7-T12 / Part7 §3.6.3)。
//!
//! `channel-msstore` / `channel-steam` 构建在组合根(`super::default_entitlement_provider`)
//! 选择对应桩,保证四种渠道组合都能编译;真实实现(MsStore `StoreContext` 收据 / Steam DLC
//! ownership)归 Part8 D5-D8,届时在商业渠道层覆写。
//! [`FreeStubEntitlement`] 则是 direct 渠道内置信任根解析失败时的 fail-closed 回退(恒
//! `Unlicensed`;source_tag=free / 激活拒绝 / 撤销幂等)。
//!
//! **裁决注(2026-07-02,无人值守分叉)**:设计草稿口径是「运行 `unimplemented!()`」,此处改为
//! **fail-closed 不 panic**——`evaluate` 恒 `Unlicensed`、`activate` 走 trait 默认
//! `ActivationUnsupported`。骨架 build 被误运行时表现为「全部付费插件不可用」而非进程崩溃,
//! 与免费桩的失败哲学一致(无法证明授权 = 未授权),语义同样诚实且严格更安全。
//!
//! 桩本身**无条件编译**(不含任何 DRM/updater 敏感代码,§3.6.2 物理排除面不涉及它),
//! 使默认 CI 矩阵天然覆盖其单测;仅工厂的**选择**按 channel feature 门控。

use scrollery_plugin_api::{ActivationInfo, EntitlementProvider, LicenseError, LicenseStatus};

/// MS Store 渠道骨架桩:授权真相将来自 `StoreContext` 平台收据(Part8 D5-D7)。
/// 接线前无从证明任何授权 → 恒 `Unlicensed`,激活 fail-closed。
pub struct MsStoreEntitlementStub;

impl EntitlementProvider for MsStoreEntitlementStub {
    fn evaluate(&self, _plugin_id: &str, _sku: &str, _now: i64) -> LicenseStatus {
        LicenseStatus::Unlicensed
    }

    fn source_tag(&self) -> &'static str {
        "ms_store"
    }
    // activate/deactivate 用 trait 默认:fail-closed ActivationUnsupported / 幂等 Ok。
}

/// Steam 渠道骨架桩:授权真相将来自 Steamworks DLC ownership(Part8 D6/D8)。
/// 同上,接线前恒 `Unlicensed`。
pub struct SteamEntitlementStub;

impl EntitlementProvider for SteamEntitlementStub {
    fn evaluate(&self, _plugin_id: &str, _sku: &str, _now: i64) -> LicenseStatus {
        LicenseStatus::Unlicensed
    }

    fn source_tag(&self) -> &'static str {
        "steam"
    }
}

/// 未授权回退 / 免费桩:恒 `Unlicensed`。direct 渠道内置信任根解析失败时组合根降级到本桩,
/// 使所有付费插件不可用(核心免费功能完整)。本桩不持密钥与验签逻辑。
pub struct FreeStubEntitlement;

impl EntitlementProvider for FreeStubEntitlement {
    fn evaluate(&self, _plugin_id: &str, _sku: &str, _now: i64) -> LicenseStatus {
        LicenseStatus::Unlicensed
    }

    fn source_tag(&self) -> &'static str {
        "free"
    }

    /// 无验签逻辑、无凭据存储 → 稳定错误码 `activation_unsupported`。亦覆盖组合根 fail-closed
    /// 回退场景:信任根解析失败降级本桩时,激活同样被拒。
    fn activate(
        &self,
        _plugin_id: &str,
        _sku: &str,
        _credential: &str,
        _now: i64,
    ) -> Result<ActivationInfo, LicenseError> {
        Err(LicenseError::ActivationUnsupported)
    }

    /// 无凭据可撤,幂等成功(Part0 §9.1 设计注:「activate→Err、deactivate→Ok」)。
    fn deactivate(&self, _plugin_id: &str) -> Result<(), LicenseError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrollery_plugin_api::LicenseError;

    /// 两桩 fail-closed 契约:恒 Unlicensed、激活 ActivationUnsupported、撤销幂等、
    /// source_tag 与 trait 文档预留值("ms_store"/"steam")一致——前端 gate 按 tag 分支展示。
    #[test]
    fn channel_stubs_are_fail_closed() {
        let ms = MsStoreEntitlementStub;
        let steam = SteamEntitlementStub;
        for p in [&ms as &dyn EntitlementProvider, &steam] {
            assert_eq!(p.evaluate("any", "sku", 0), LicenseStatus::Unlicensed);
            assert_eq!(
                p.activate("any", "sku", "receipt", 0),
                Err(LicenseError::ActivationUnsupported)
            );
            assert_eq!(p.deactivate("any"), Ok(()));
        }
        assert_eq!(ms.source_tag(), "ms_store");
        assert_eq!(steam.source_tag(), "steam");
    }

    /// 免费桩(未授权回退)契约锁:恒 Unlicensed、source_tag=free、激活 fail-closed
    /// (`activation_unsupported`)、撤销幂等。
    #[test]
    fn free_stub_is_fail_closed() {
        let stub = FreeStubEntitlement;
        assert_eq!(
            stub.evaluate("any-plugin", "any-sku", 0),
            LicenseStatus::Unlicensed
        );
        assert_eq!(stub.source_tag(), "free");
        assert_eq!(
            stub.activate("any-plugin", "any-sku", "any-token", 0),
            Err(LicenseError::ActivationUnsupported)
        );
        assert_eq!(stub.deactivate("any-plugin"), Ok(()));
    }
}
