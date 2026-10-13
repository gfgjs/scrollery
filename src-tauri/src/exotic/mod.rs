// src-tauri/src/exotic/mod.rs
//! 冷门格式插件子系统。
//!
//! 能力目录、安装记录、免费激活记录和处理任务各自独立。
//! 宿主合并这些事实决定是否可运行；激活不替代安装、平台和完整性检查。

pub mod catalog;
pub mod coordinator;
pub mod crypto;
pub mod fetch;
pub mod fingerprint;
pub mod install;
pub mod installer;
pub mod limiter;
pub mod outcome;
pub mod package;
pub mod pipeline;
pub mod registry;
pub mod sink;
pub mod supervisor;
pub mod task;
pub mod tools;
pub mod validate;
pub mod worker;
pub(crate) mod worker_log;
pub mod worker_traits;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db::connection::DbPool;
use crate::feature_activation::{self, ActivationSource};
pub use catalog::{Capability, CatalogOffering, CatalogSnapshot, CatalogStore, MediaKind};
pub use crypto::VerifyingKeyset;
pub use task::{ExoticTaskRow, ExoticTaskStatus};

/// Host 语义版本（min_host_version 兼容门控用）。
const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 格式可用态（v3 §5.2）。**只**描述可用性；任务的 pending/running/done/error 由 [`task`] 描述。
/// 序列化为 camelCase，前端 `ExoticAvailability` 直接对齐（Part4 §7.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    /// 插件尚未安装。
    AvailableUninstalled,
    /// 尚未免费激活。
    Inactive,
    /// 激活与安装等条件齐备，可运行。
    Ready,
    /// 当前平台无对应包。
    UnsupportedPlatform,
    /// Host 版本不满足 min_host_version。
    IncompatibleHost,
    /// 安装损坏（hash/清单不符）。
    InvalidInstallation,
    /// 子系统/插件被禁用。
    Disabled,
    /// 无 offering（如远程删除后仍有历史任务/媒体）。
    NoOffering,
}

/// 结构化格式解析结果（v3 §5.2）。后端返回它而非压缩成三态枚举。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatResolution {
    pub format: String,
    pub media_kind: MediaKind,
    pub plugin_id: Option<String>,
    /// 展示名（来自 Catalog offering.display_name；无 offering 时回退为格式名）。
    #[serde(default)]
    pub display_name: String,
    pub capabilities: Vec<Capability>,
    pub availability: Availability,
    pub activation_required: bool,
    pub activated: bool,
    pub installed_version: Option<String>,
    /// builtin offering(D-OCR-5):无安装包,读取本地激活状态;前端据此区分插件安装与内置组件准备。
    pub builtin: bool,
}

/// 已安装插件信息（安装真相投影，供前端市场使用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExoticPlugin {
    pub plugin_id: String,
    pub version: String,
    pub package_sequence: i64,
    pub install_state: String,
    pub installed_at: i64,
    pub updated_at: i64,
}

/// 安装真相完整行（**内部**用；含 `manifest_hash`，不对前端序列化）。
/// 安装/升级写入；`resolve_format` 根据 `install_state` 区分可用状态。
/// Worker 启动前依据 `manifest_hash` 复核安装完整性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPluginRecord {
    pub plugin_id: String,
    pub version: String,
    pub manifest_hash: String,
    pub package_sequence: i64,
    /// installed / disabled / broken（§5.2/§6）。
    pub install_state: String,
    pub installed_at: i64,
    pub updated_at: i64,
}

/// 安装状态常量（与 DB `exotic_plugins.install_state` 列一致）。
pub mod install_state {
    /// 正常安装、文件完整。
    pub const INSTALLED: &str = "installed";
    /// 用户/系统禁用。
    pub const DISABLED: &str = "disabled";
    /// 完整性校验失败 / 安装损坏。
    pub const BROKEN: &str = "broken";
}

/// 当前构建的 rust target triple（平台门控用）。
/// 用 cfg 组合，避免依赖 build.rs 注入的 `TARGET`（普通 `cargo build` 不设该环境变量）。
pub fn current_target_triple() -> &'static str {
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        return "x86_64-pc-windows-msvc";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "windows"))]
    {
        return "aarch64-pc-windows-msvc";
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        return "x86_64-apple-darwin";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        return "aarch64-apple-darwin";
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        return "x86_64-unknown-linux-gnu";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        return "aarch64-unknown-linux-gnu";
    }
    #[allow(unreachable_code)]
    {
        "unknown"
    }
}

/// 安装真相数据源抽象（Host 经此查 exotic_plugins；Release=只读连接池，stub/test=内存）。
pub trait InstalledSource: Send + Sync {
    /// 取某插件安装真相完整行（未安装 → None）。
    fn get(&self, plugin_id: &str) -> Option<InstalledPluginRecord>;
}

/// 运行时实现：从只读连接池查 `exotic_plugins`。
pub struct DbInstalledSource {
    pool: DbPool,
}

impl InstalledSource for DbInstalledSource {
    fn get(&self, plugin_id: &str) -> Option<InstalledPluginRecord> {
        let conn = self.pool.get().ok()?;
        crate::db::queries::get_exotic_plugin(&conn, plugin_id)
            .ok()
            .flatten()
    }
}

/// 空安装源（无任何安装记录；Part1 只读桩 / 单测）。
pub struct EmptyInstalledSource;

impl InstalledSource for EmptyInstalledSource {
    fn get(&self, _plugin_id: &str) -> Option<InstalledPluginRecord> {
        None
    }
}

/// 组合能力、安装与逐项免费激活状态。
pub struct ExoticHost {
    catalog: Arc<CatalogStore>,
    installed: Arc<dyn InstalledSource>,
    activations: Arc<dyn ActivationSource>,
    /// **仅** dev fixture / 测试注入：把某 plugin_id 直接视为已激活（绕过真实 installed+activation）。
    ///
    /// 防御纵深（§5.4，安全评审 medium 加固）：字段本身用 `#[cfg]` 门控——Release 构建
    /// （非 debug 或无 `exotic-dev-fixtures`）下该字段**编译期不存在**，连同 `availability_of`
    /// 的对应分支一并被消除。即便将来误增构造路径试图写它，也是编译错误而非静默授权旁路。
    #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
    authorized_fixture: Option<String>,
}

/// 插件可用状态；激活记录与安装、平台检查分开显示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAvailability {
    pub plugin_id: String,
    pub availability: Availability,
    pub activation_required: bool,
    pub activated: bool,
}

struct NoActivations;
impl ActivationSource for NoActivations {
    fn is_activated(&self, _feature_id: &str) -> crate::error::Result<bool> {
        Ok(false)
    }
}

struct DbActivationSource {
    pool: DbPool,
}
impl ActivationSource for DbActivationSource {
    fn is_activated(&self, feature_id: &str) -> crate::error::Result<bool> {
        let conn = self.pool.get().map_err(crate::error::AppError::from)?;
        Ok(feature_activation::status(&conn, feature_id)?.activated)
    }
}

/// 插件 registry 与发行包验签使用的信任根，独立于免费激活。
pub(crate) fn trusted_keyset() -> Result<VerifyingKeyset, crypto::CryptoError> {
    #[cfg(debug_assertions)]
    if let Ok(p) = std::env::var("PICASA_EXOTIC_DEV_KEYSET") {
        if !p.is_empty() {
            // 路径已给但不可读 → fail-fast 报错(可诊断),不静默回退内置集。
            let json = std::fs::read_to_string(&p)
                .map_err(|e| crypto::CryptoError::Parse(format!("dev keyset 不可读({p}):{e}")))?;
            return VerifyingKeyset::parse(&json);
        }
    }
    VerifyingKeyset::builtin()
}

impl ExoticHost {
    /// 只读桩 Host：无安装、未激活（前端 list 命令在无真实数据源时回退用；多数解析为
    /// `AvailableUninstalled`）。Release 主路径请用 [`ExoticHost::for_runtime`]。
    pub fn new(catalog: Arc<CatalogStore>) -> Self {
        ExoticHost {
            catalog,
            installed: Arc::new(EmptyInstalledSource),
            activations: Arc::new(NoActivations),
            #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
            authorized_fixture: None,
        }
    }

    /// 以指定数据源构建（运行期 / 测试矩阵）。
    pub fn with_sources(
        catalog: Arc<CatalogStore>,
        installed: Arc<dyn InstalledSource>,
        activations: Arc<dyn ActivationSource>,
    ) -> Self {
        ExoticHost {
            catalog,
            installed,
            activations,
            #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
            authorized_fixture: None,
        }
    }

    /// 测试 fixture：把某插件标为已激活（仅单测/集成；不经任何 Release 代码路径）。
    #[cfg(test)]
    pub fn with_authorized_fixture(catalog: Arc<CatalogStore>, plugin_id: &str) -> Self {
        let mut h = ExoticHost::new(catalog);
        h.authorized_fixture = Some(plugin_id.to_string());
        h
    }

    /// 安装和免费激活均从当前本地数据库读取，激活后现有宿主立即生效。
    pub fn for_runtime(catalog: Arc<CatalogStore>, pool: DbPool) -> Self {
        ExoticHost {
            catalog,
            installed: Arc::new(DbInstalledSource { pool: pool.clone() }),
            activations: Arc::new(DbActivationSource { pool }),
            #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
            authorized_fixture: dev_authorized_fixture(),
        }
    }

    /// 解析某扩展名（小写）的可用态——三份真相折叠（§5.2 全状态矩阵）。
    pub fn resolve_format(&self, format: &str) -> FormatResolution {
        let snap = self.catalog.snapshot();
        let Some(off) = snap.resolve_format(format) else {
            return FormatResolution {
                format: format.to_string(),
                // 无 offering：无从得知媒体类。占位 Image，调用方仅对 catalog 已知格式调用本函数。
                media_kind: MediaKind::Image,
                plugin_id: None,
                display_name: format.to_string(),
                capabilities: Vec::new(),
                availability: Availability::NoOffering,
                activation_required: false,
                activated: false,
                installed_version: None,
                builtin: false,
            };
        };
        let installed = self.installed.get(&off.plugin_id);
        let installed_version = installed.as_ref().map(|r| r.version.clone());
        let activation_required = feature_activation::requires_activation(&off.plugin_id);
        let activated = activation_required
            && self
                .activations
                .is_activated(&off.plugin_id)
                .unwrap_or_else(|error| {
                    tracing::warn!("读取免费激活状态失败: {error}");
                    false
                });
        let availability = self.availability_of(off, installed.as_ref(), activated);
        FormatResolution {
            format: format.to_string(),
            media_kind: off.media_kind,
            plugin_id: Some(off.plugin_id.clone()),
            display_name: off.display_name.clone(),
            capabilities: off.capabilities.clone(),
            availability,
            activation_required,
            activated,
            installed_version,
            builtin: off.builtin,
        }
    }

    /// 折叠某 offering 的可用态。门控顺序（即防御纵深）：
    /// 平台 → Host 版本 → (dev fixture) → 安装状态 → 激活状态。
    fn availability_of(
        &self,
        off: &CatalogOffering,
        installed: Option<&InstalledPluginRecord>,
        activated: bool,
    ) -> Availability {
        if !off.supports_platform(current_target_triple()) {
            return Availability::UnsupportedPlatform;
        }
        if !host_meets_min(&off.min_host_version, HOST_VERSION) {
            return Availability::IncompatibleHost;
        }
        // dev/test fixture：平台/版本已过即视为可运行。Release 构建该分支**编译期消除**（§5.4 防御纵深）。
        #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
        if self.authorized_fixture.as_deref() == Some(off.plugin_id.as_str()) {
            return Availability::Ready;
        }
        if !off.builtin {
            let Some(rec) = installed else {
                return Availability::AvailableUninstalled;
            };
            match rec.install_state.as_str() {
                install_state::BROKEN => return Availability::InvalidInstallation,
                install_state::DISABLED => return Availability::Disabled,
                _ => {}
            }
        }
        if feature_activation::requires_activation(&off.plugin_id) && !activated {
            Availability::Inactive
        } else {
            Availability::Ready
        }
    }

    /// 列出 Catalog 中全部格式的解析结果（前端 `list_exotic_format_resolutions` 用）。
    /// 即使未安装 PSD 也会出现 `AvailableUninstalled`（首次离线也显示安装占位）。
    pub fn list_resolutions(&self) -> Vec<FormatResolution> {
        let snap = self.catalog.snapshot();
        snap.iter_formats()
            .map(|(fmt, _)| self.resolve_format(fmt))
            .collect()
    }

    /// 某 (plugin, capability) 当前是否**可领取处理**（Scheduler/Coordinator 门控用，v3 §5.3）。
    ///
    /// runnable ⟺ 有 offering、平台兼容、Host 版本兼容、已安装且激活（或 dev fixture）、声明了该能力。
    /// 未安装/未激活/平台不支持/禁用/损坏都返回 false——这些状态**不**写任务状态，由本门控拦在领取前。
    pub fn is_task_runnable(&self, plugin_id: &str, capability: Capability) -> bool {
        let snap = self.catalog.snapshot();
        // 找到该插件的任一 format，按其可用态判定（同一插件多 format 共享 offering）。
        let Some((fmt, off)) = snap.iter_formats().find(|(_, o)| o.plugin_id == plugin_id) else {
            return false;
        };
        if !off.claims_capability(capability) {
            return false;
        }
        matches!(self.resolve_format(fmt).availability, Availability::Ready)
    }

    /// 查询单项插件的激活与资源可用状态。
    pub fn availability_of_plugin(&self, plugin_id: &str) -> Option<PluginAvailability> {
        let snap = self.catalog.snapshot();
        let (fmt, _) = snap
            .iter_formats()
            .find(|(_, off)| off.plugin_id == plugin_id)?;
        let res = self.resolve_format(fmt);
        Some(PluginAvailability {
            plugin_id: plugin_id.to_owned(),
            availability: res.availability,
            activation_required: res.activation_required,
            activated: res.activated,
        })
    }
}

/// dev fixture 可用插件（仅 debug + `exotic-dev-fixtures`；Release 恒 None，§5.4）。
/// 函数本身按字段同条件门控——Release 不编入，避免「字段被消除后函数变死代码」告警。
#[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
fn dev_authorized_fixture() -> Option<String> {
    #[cfg(all(debug_assertions, feature = "exotic-dev-fixtures"))]
    {
        Some("exotic-image-psd".to_string())
    }
    #[cfg(not(all(debug_assertions, feature = "exotic-dev-fixtures")))]
    {
        None
    }
}

/// Host 版本是否满足插件声明的 `min_host_version`（纯数值点分版本，X.Y.Z…）。
/// 逐段数值比较。`min_host_version` 不可解析为纯数值点分串时**保守拒绝**（IncompatibleHost）：
/// 不能证明兼容即不放行，避免不兼容插件在旧 Host 上尝试加载而崩溃（安全评审 low，fail-closed）。
pub(crate) fn host_meets_min(min: &str, host: &str) -> bool {
    let parse =
        |s: &str| -> Option<Vec<u64>> { s.split('.').map(|p| p.parse::<u64>().ok()).collect() };
    match (parse(min), parse(host)) {
        (Some(min_v), Some(host_v)) => {
            let n = min_v.len().max(host_v.len());
            for i in 0..n {
                let m = min_v.get(i).copied().unwrap_or(0);
                let h = host_v.get(i).copied().unwrap_or(0);
                if h != m {
                    return h > m;
                }
            }
            true // 完全相等 → 满足
        }
        // min 无法解析 → 保守拒绝（无法证明兼容）。host 来自 CARGO_PKG_VERSION 恒可解析。
        _ => false,
    }
}
