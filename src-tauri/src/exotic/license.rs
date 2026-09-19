// src-tauri/src/exotic/license.rs
//! 冷门格式插件 · 授权 provider 实现（v3 Part3 §5.2/§5.3）。
//!
//! 【Part6 §3.9.1a 去环 ③a】纯验签逻辑（`verify_token`/`evaluate_token`/`LicensePayload`）已迁至
//! 叶 crate `scrollery-exotic-trust`（无秘密价值）。本文件保留**真实 keyring I/O** 实现
//! `KeyringLicenseStore`(依赖 keyring crate,是直销渠道唯一的 keyring 授权实现),并
//! `pub use` 再导出迁走的原语，使既有
//! `crate::exotic::license::{verify_token, LicensePayload, ...}` 引用路径不变。授权 DTO / trait
//! （`EntitlementProvider`/`LicenseStatus`/`LicenseError`）住更底层的叶 crate `scrollery-plugin-api`。
//!
//! 本文件同时提供两个 [`EntitlementProvider`] 实现:keyring 直销 [`KeyringLicenseStore`] 与
//! fail-closed 回退 [`FreeStubEntitlement`](信任根解析失败时组合根的降级目标)。keyring **crate**
//! 不属 DRM 专用:storage/proofread 的 API Key 凭据存储复用同一 crate。
//!
//! 三份真相中的「授权真相」（§5.1）：token 存系统 keyring（service 固定、account=plugin_id），
//! DB 不保存 token；日志/遥测/panic/IPC **绝不**输出 token 或 subject_hash（§5.2）。

use std::sync::Arc;

use crate::exotic::crypto::VerifyingKeyset;

// 授权 DTO / trait 住 plugin-api 叶 crate（§3.9.1a）；此处 `pub use` 再导出使引用路径不变。
pub use scrollery_plugin_api::{EntitlementProvider, LicenseError, LicenseStatus};
// 纯验签原语迁至 exotic-trust 叶 crate（§3.9.1a ③a）；`pub use` 再导出保持
// `crate::exotic::license::{verify_token, evaluate_token, LicensePayload}` 引用路径不变，
// 并令下方 `KeyringLicenseStore` 内部调用直接可见。
pub use scrollery_exotic_trust::{evaluate_token, verify_token, LicensePayload};

/// keyring service（与既有 proofread/storage key 同 service，account 区分用途）。
use scrollery_plugin_api::KEYRING_SERVICE;

/// keyring account = plugin_id（§5.2）。集中此处，便于审计「token 存放坐标」。
fn license_account(plugin_id: &str) -> &str {
    plugin_id
}

// `LicenseSource` trait 升格为 plugin-api 的 `EntitlementProvider`（上方 `pub use`）;
// 始终未授权的桩为本文件的 `FreeStubEntitlement`（组合根 fail-closed 回退用）。
// `KeyringLicenseStore` 的验签逻辑经 exotic-trust 复用(§8.7):信任根=编译期内置公钥集
// (默认占位集,发布经 PICASA_EXOTIC_KEYSET_FILE 注入受控签发机公钥)。

/// keyring 实现：token 存系统凭据库；验签用编入 Host 的信任根公钥集。
pub struct KeyringLicenseStore {
    keyset: Arc<VerifyingKeyset>,
}

impl KeyringLicenseStore {
    pub fn new(keyset: Arc<VerifyingKeyset>) -> Self {
        KeyringLicenseStore { keyset }
    }

    fn entry(plugin_id: &str) -> Result<keyring::Entry, LicenseError> {
        keyring::Entry::new(KEYRING_SERVICE, license_account(plugin_id))
            .map_err(|e| LicenseError::KeyringUnavailable(e.to_string()))
    }

    /// 读取 keyring 中的 token（NoEntry → None）。
    pub fn get_token(&self, plugin_id: &str) -> Result<Option<String>, LicenseError> {
        let entry = Self::entry(plugin_id)?;
        match entry.get_password() {
            Ok(t) => Ok(Some(t)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(LicenseError::KeyringUnavailable(e.to_string())),
        }
    }

    /// 激活：先验签（防止存入无效 token），通过才写 keyring（§6.6：失败不覆盖现有有效 token）。
    /// 返回验证通过的 payload（调用方可缓存授权结果与检查时间，但**不**存 token）。
    ///
    /// **调用方契约**（§5.2，安全评审）：
    /// - `plugin_id`/`sku` **必须**取自可信 Catalog（`CatalogOffering`），**绝不**取自前端输入或
    ///   token 自身——否则攻击者传任意 sku 即可让 sku_A 的 token 冒充 sku_B。
    /// - 返回的 `LicensePayload` 含 `subject_hash`，**不得**整体跨 IPC 下发给前端；Part3 激活命令
    ///   须投影为不含 `subject_hash` 的 DTO（`LicensePayload` 的 `Debug` 已脱敏，但 `Serialize` 未）。
    pub fn activate(
        &self,
        plugin_id: &str,
        sku: &str,
        token: &str,
        now: i64,
    ) -> Result<LicensePayload, LicenseError> {
        let payload = verify_token(token, &self.keyset, plugin_id, sku, now)?;
        let entry = Self::entry(plugin_id)?;
        entry
            .set_password(token)
            .map_err(|e| LicenseError::KeyringUnavailable(e.to_string()))?;
        Ok(payload)
    }

    /// 移除授权（卸载时的「移除授权」操作，§6.5）。NoEntry 视为已移除。
    pub fn remove_token(&self, plugin_id: &str) -> Result<(), LicenseError> {
        let entry = Self::entry(plugin_id)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(LicenseError::KeyringUnavailable(e.to_string())),
        }
    }
}

impl EntitlementProvider for KeyringLicenseStore {
    fn evaluate(&self, plugin_id: &str, sku: &str, now: i64) -> LicenseStatus {
        match self.get_token(plugin_id) {
            Err(_) => LicenseStatus::KeyringUnavailable,
            Ok(opt) => evaluate_token(opt.as_deref(), &self.keyset, plugin_id, sku, now),
        }
    }

    /// 直销渠道（keyring + Ed25519 验签）。
    fn source_tag(&self) -> &'static str {
        "direct"
    }

    /// 激活（R1-1 收敛：IPC 命令层改走本 trait，不再直构 store）。委托 inherent
    /// [`KeyringLicenseStore::activate`]（先验签后存，失败不覆盖现有有效 token）；
    /// `LicensePayload`（含 subject_hash，§5.2 禁跨 IPC）止步于本层，不向 trait 消费者投影。
    fn activate(
        &self,
        plugin_id: &str,
        sku: &str,
        credential: &str,
        now: i64,
    ) -> Result<(), LicenseError> {
        // 显式走 inherent 方法（与本 trait 方法同名，避免歧义误读为递归）。
        let _payload = KeyringLicenseStore::activate(self, plugin_id, sku, credential, now)?;
        Ok(())
    }

    /// 撤销 = 移除 keyring token（NoEntry 幂等成功，§6.5）。
    fn deactivate(&self, plugin_id: &str) -> Result<(), LicenseError> {
        self.remove_token(plugin_id)
    }
}

/// 未授权回退 / 免费桩：恒 `Unlicensed`。组合根在信任根解析失败时降级到本桩，
/// 使所有付费插件不可用（核心免费功能完整）。本桩不持密钥与验签逻辑。
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
    ) -> Result<(), LicenseError> {
        Err(LicenseError::ActivationUnsupported)
    }

    /// 无凭据可撤,幂等成功(设计注:「activate→Err、deactivate→Ok」)。
    fn deactivate(&self, _plugin_id: &str) -> Result<(), LicenseError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
