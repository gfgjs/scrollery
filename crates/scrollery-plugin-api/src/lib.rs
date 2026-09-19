// crates/scrollery-plugin-api/src/lib.rs
//! 插件平台开源契约（Part6 §3.8 / §3.9.1a）。
//!
//! 本叶 crate 只持**抽象与 DTO**：`EntitlementProvider`（授权真相数据源 trait）+ `LicenseStatus`
//! （授权态）+ `LicenseError`（验签/授权错误）。**无任何密钥材料、无门控逻辑**——抽象本身不构成
//! 授权能力，实现恒住宿主 `src-tauri/src/exotic/`：直销 keyring 实现（`KeyringLicenseStore`）
//! 与未授权回退桩（`FreeStubEntitlement`）。
//!
//! 单向依赖图（无环，§3.9.1a 实证）:`plugin-api`（叶）← `exotic-trust` / `src-tauri`。

#![forbid(unsafe_code)]

/// OS 凭据库(keyring)服务名——存储后端密码 / 校对 API key / 直销激活凭据共用的**单一事实源**
/// (R2-7 改名施工收敛,2026-07-06:原多处硬编码分散,改名时极易漏改分叉)。
/// 值变更属状态坐标级操作(旧服务名下的既有凭据对新版不可见,用户须重录/重激活),
/// 见改名施工计划 §1;品牌再变更时只动这里。
pub const KEYRING_SERVICE: &str = "scrollery";

/// 授权态评估结果（由 [`EntitlementProvider`] 给出;宿主据此折叠最终「格式可用态」）。
///
/// 变体名与宿主 `availability_of` 的 match 一一对应，迁移前后零行为变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseStatus {
    /// 有效授权。
    Authorized,
    /// token 存在但已过期 → 停止新派发，但**不**视为错误（保留已处理结果）。
    Expired,
    /// 无 token / 不匹配 / 验签失败 → 一律按未授权。
    Unlicensed,
    /// 凭据存储（keyring 等）读取失败 → 无法证明授权，按未授权对待，但单列以便诊断。
    KeyringUnavailable,
}

/// License 验签 / 授权错误。`code()` 稳定，可安全跨边界输出（不含 token / 密钥材料）。
///
/// 错误码集合与原 `src-tauri/src/exotic/license.rs` 完全一致（§5.2 防御纵深的各错误码）;
/// 迁至本叶 crate 以便各 provider 实现（宿主 keyring / 渠道桩）共用同一错误类型。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LicenseError {
    #[error("无 License token")]
    Missing,
    #[error("token 结构非法：{0}")]
    Malformed(String),
    #[error("不支持的 token 版本：{0}")]
    UnsupportedVersion(u32),
    #[error("未知签名 key")]
    UnknownKey,
    #[error("签名验证失败")]
    BadSignature,
    #[error("plugin_id 不符")]
    PluginMismatch,
    #[error("sku 不符")]
    SkuMismatch,
    #[error("License 未生效（not_before 未到）")]
    NotYetValid,
    #[error("License 已过期")]
    Expired,
    #[error("keyring 不可用：{0}")]
    KeyringUnavailable(String),
    /// 当前 provider 不支持激活操作（免费桩 / 只读测试桩 / 信任根不可用的 fail-closed 回退）。
    #[error("当前构建不支持激活操作")]
    ActivationUnsupported,
}

impl LicenseError {
    /// 稳定错误码（§5.2：missing/malformed/unknown_key/bad_signature/plugin_mismatch/
    /// sku_mismatch/not_yet_valid/expired/keyring_unavailable）。
    pub fn code(&self) -> &'static str {
        match self {
            LicenseError::Missing => "missing",
            LicenseError::Malformed(_) => "malformed",
            LicenseError::UnsupportedVersion(_) => "unsupported_version",
            LicenseError::UnknownKey => "unknown_key",
            LicenseError::BadSignature => "bad_signature",
            LicenseError::PluginMismatch => "plugin_mismatch",
            LicenseError::SkuMismatch => "sku_mismatch",
            LicenseError::NotYetValid => "not_yet_valid",
            LicenseError::Expired => "expired",
            LicenseError::KeyringUnavailable(_) => "keyring_unavailable",
            LicenseError::ActivationUnsupported => "activation_unsupported",
        }
    }
}

/// 授权真相数据源抽象（Part6 §3.8：由原 `LicenseSource` 升格而来）。
///
/// 宿主（含 `ExoticHost`）经 `Arc<dyn EntitlementProvider>` 持有：读路径走 `evaluate`、写路径走
/// `activate`/`deactivate`，装配收敛在组合根单点（`exotic::default_entitlement_provider`）。
///
/// **R1-1（2026-07-02 审查裁决）**：`activate` / `deactivate` 按 Part0 §9.1 原始设计补入本 trait。
/// 此前「先收敛读路径、写路径走具体实现 inherent 方法」的瘦身造成**信任根分裂**——`evaluate` 走
/// 注入 provider，`activate` 却在 IPC 命令层直构另一份 keyset 的 `KeyringLicenseStore`，两者可各持
/// 不同信任根。收敛后激活 / 撤销 / 评估全经同一装配点的 provider。
pub trait EntitlementProvider: Send + Sync {
    /// 评估 `(plugin_id, sku)` 在 `now`（unix 秒）的授权态。
    fn evaluate(&self, plugin_id: &str, sku: &str, now: i64) -> LicenseStatus;

    /// 渠道 / 来源标识（`"direct"`（keyring 直销）/ `"free"`（未授权回退桩））。供前端 gate
    /// 展示与诊断;**不**参与授权判定本身。
    fn source_tag(&self) -> &'static str;

    /// 激活：验证 `credential`（direct = Ed25519 token）并持久化授权。
    ///
    /// **调用方契约（§5.2 / §6.6）**：`plugin_id` / `sku` **必须**取自可信 Catalog，绝不取自前端
    /// 输入或 credential 自身；实现方失败时**不得**覆盖既有有效授权。错误经 `code()` 跨边界，
    /// 绝不含 token / 密钥材料。授权凭据止步于实现层：`LicensePayload`（含 `subject_hash`）
    /// 不向消费者投影（§5.2）。
    ///
    /// 默认实现 fail-closed（`ActivationUnsupported`）——只读 provider（测试桩 / 免费桩）不覆写
    /// 即天然安全；真实渠道（direct）必须覆写。
    fn activate(
        &self,
        _plugin_id: &str,
        _sku: &str,
        _credential: &str,
        _now: i64,
    ) -> Result<(), LicenseError> {
        Err(LicenseError::ActivationUnsupported)
    }

    /// 撤销授权（卸载的「移除授权」/ 退订）。无授权可撤视为成功（幂等，NoEntry ≠ 错误）。
    fn deactivate(&self, _plugin_id: &str) -> Result<(), LicenseError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 错误码跨 IPC 稳定（§5.2）：前端按 code 分支处理，任何改名都是破坏性契约变更——
    /// 全集在此锁死（含 R1-1 新增 activation_unsupported）。
    #[test]
    fn license_error_codes_are_stable() {
        let cases: &[(LicenseError, &str)] = &[
            (LicenseError::Missing, "missing"),
            (LicenseError::Malformed("x".into()), "malformed"),
            (LicenseError::UnsupportedVersion(9), "unsupported_version"),
            (LicenseError::UnknownKey, "unknown_key"),
            (LicenseError::BadSignature, "bad_signature"),
            (LicenseError::PluginMismatch, "plugin_mismatch"),
            (LicenseError::SkuMismatch, "sku_mismatch"),
            (LicenseError::NotYetValid, "not_yet_valid"),
            (LicenseError::Expired, "expired"),
            (
                LicenseError::KeyringUnavailable("x".into()),
                "keyring_unavailable",
            ),
            (
                LicenseError::ActivationUnsupported,
                "activation_unsupported",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), *code, "错误码必须稳定：{err:?}");
        }
    }

    /// trait 默认实现 fail-closed：只读 provider 不覆写 activate 也绝不放行授权写入。
    struct ReadOnlyProvider;
    impl EntitlementProvider for ReadOnlyProvider {
        fn evaluate(&self, _: &str, _: &str, _: i64) -> LicenseStatus {
            LicenseStatus::Unlicensed
        }
        fn source_tag(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn default_activate_is_fail_closed_and_deactivate_idempotent() {
        let p = ReadOnlyProvider;
        assert_eq!(
            p.activate("id", "sku", "token", 0),
            Err(LicenseError::ActivationUnsupported)
        );
        assert_eq!(p.deactivate("id"), Ok(()));
    }
}
