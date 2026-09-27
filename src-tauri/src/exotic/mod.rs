// src-tauri/src/exotic/mod.rs
//! 冷门格式插件子系统（v3 总纲）。
//!
//! 三份真相（v3 §5.1）分模块：
//!   - [`catalog`]：能力真相（某格式有无产品、属哪类、提供哪些能力、哪些平台）。
//!   - 安装真相 / 授权真相：Part3 落地；Part1 用只读桩（无安装记录即 `AvailableUninstalled`）。
//!   - [`task`]：处理真相（能力级任务状态机）。
//!
//! 本卷（Part1）**不**启动 Worker、不下载、不验签。

pub mod catalog;
pub mod coordinator;
pub mod crypto;
pub mod fetch;
pub mod fingerprint;
pub mod install;
pub mod installer;
pub mod license;
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
pub use catalog::{Capability, CatalogOffering, CatalogSnapshot, CatalogStore, MediaKind};
pub use crypto::VerifyingKeyset;
use license::FreeStubEntitlement;
pub use license::{EntitlementProvider, LicenseStatus};
pub use task::{ExoticTaskRow, ExoticTaskStatus};

/// Host 语义版本（min_host_version 兼容门控用）。
const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 格式可用态（v3 §5.2）。**只**描述可用性；任务的 pending/running/done/error 由 [`task`] 描述。
/// 序列化为 camelCase，前端 `ExoticAvailability` 直接对齐（Part4 §7.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    /// 有产品、未安装 → 显示购买占位。
    AvailableUninstalled,
    /// 已安装、未授权 → 显示激活。
    InstalledUnlicensed,
    /// 已授权、可运行。
    Authorized,
    /// License 过期。
    LicenseExpired,
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
    pub store_url: Option<String>,
    pub installed_version: Option<String>,
    /// builtin offering(D-OCR-5):无安装包,直接验 license;前端据此免走「购买/安装」文案。
    pub builtin: bool,
}

/// 已安装插件信息（安装真相投影；前端市场用）。Part1 安装表为空，命令返回空列表。
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
/// Part3 安装/升级写入；resolve_format 据 `install_state` 区分 Authorized/InstalledUnlicensed/
/// InvalidInstallation；Supervisor 启动前据 `manifest_hash` 复核安装完整性（Part2 §3.6 接真实校验）。
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

/// 组合三份真相的 Host（v3 §2.1 / Part3 §5）。
/// **不创造能力**，只把 catalog（能力真相）+ installed（安装真相）+ licenses（授权真相）
/// 折叠成 [`FormatResolution`]。Release 主路径用真实 DB + keyring；无全局跳过授权开关。
pub struct ExoticHost {
    catalog: Arc<CatalogStore>,
    installed: Arc<dyn InstalledSource>,
    licenses: Arc<dyn EntitlementProvider>,
    /// **仅** dev fixture / 测试注入：把某 plugin_id 直接视为已授权（绕过真实 installed+license）。
    ///
    /// 防御纵深（§5.4，安全评审 medium 加固）：字段本身用 `#[cfg]` 门控——Release 构建
    /// （非 debug 或无 `exotic-dev-fixtures`）下该字段**编译期不存在**，连同 `availability_of`
    /// 的对应分支一并被消除。即便将来误增构造路径试图写它，也是编译错误而非静默授权旁路。
    #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
    authorized_fixture: Option<String>,
}

/// 某插件的授权判定 DTO（前端 gate / 购买引导，Part6 §3.8）。camelCase 序列化对齐前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntitlement {
    pub plugin_id: String,
    /// 折叠后的可用态（平台 / 版本 / 安装 / 授权门控结果）。
    pub availability: Availability,
    /// 授权来源渠道（`"direct"` = keyring 直销 / `"free"` = fail-closed 回退桩），
    /// 取自 [`EntitlementProvider::source_tag`]。
    pub source_tag: String,
    /// 付费插件的 sku（免费 / 无 sku 插件为 None）。
    pub sku: Option<String>,
    /// 购买 / 商店链接（未授权时的购买引导用）。
    pub store_url: Option<String>,
}

/// 组合根 · 授权 provider 装配点（Part6 §3.9 **唯一 swap 点**）。
///
/// 当前唯一渠道 = 直销(direct),即下方 keyring 装配(空渠道 msstore/steam 已删,P16 2026-09-15)。
///
/// direct = keyring 直销验签([`license::KeyringLicenseStore`]) + 信任根公钥集
/// ([`trusted_keyset`]——registry 与 license 共用同一集,验签路径不分叉)。
/// 随仓 keyset 只含占位公钥(私钥已弃);使用正式或自建签发链时，经
/// `PICASA_EXOTIC_KEYSET_FILE` 编译期注入对应公钥。
/// 信任根解析失败即 fail-closed 降级 [`FreeStubEntitlement`](授权一律拒绝,绝不放行)。
/// 🔒 dev/test 旁路(仅 debug 构建编入,SEC-02 姿态,同 `EXOTIC_PSD_WORKER_PATH`):
/// `PICASA_EXOTIC_DEV_KEYSET=<path>` 指向本地 keyset JSON 时**整组替换**——配合
/// `scripts/exotic-dev-registry.mjs` 生成的 dev 签名链,开发期端到端测试插件商店
/// (拉取/验签/安装/启动前完整性复核)。Release 构建该分支不存在,恒为内置集。
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

pub fn default_entitlement_provider() -> Arc<dyn EntitlementProvider> {
    match trusted_keyset() {
        Ok(ks) => Arc::new(license::KeyringLicenseStore::new(Arc::new(ks))),
        Err(e) => {
            tracing::error!("内置信任根公钥集解析失败，exotic 授权一律拒绝 | {e}");
            Arc::new(FreeStubEntitlement)
        }
    }
}

impl ExoticHost {
    /// 只读桩 Host：无安装、无授权（前端 list 命令在无真实数据源时回退用；多数解析为
    /// `AvailableUninstalled`）。Release 主路径请用 [`ExoticHost::for_runtime`]。
    pub fn new(catalog: Arc<CatalogStore>) -> Self {
        ExoticHost {
            catalog,
            installed: Arc::new(EmptyInstalledSource),
            licenses: Arc::new(FreeStubEntitlement),
            #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
            authorized_fixture: None,
        }
    }

    /// 以指定数据源构建（运行期 / 测试矩阵）。
    pub fn with_sources(
        catalog: Arc<CatalogStore>,
        installed: Arc<dyn InstalledSource>,
        licenses: Arc<dyn EntitlementProvider>,
    ) -> Self {
        ExoticHost {
            catalog,
            installed,
            licenses,
            #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
            authorized_fixture: None,
        }
    }

    /// 测试 fixture：把某插件标为已授权（仅单测/集成；不经任何 Release 代码路径）。
    #[cfg(test)]
    pub fn with_authorized_fixture(catalog: Arc<CatalogStore>, plugin_id: &str) -> Self {
        let mut h = ExoticHost::new(catalog);
        h.authorized_fixture = Some(plugin_id.to_string());
        h
    }

    /// 构造运行期 Host：安装真相 ← 只读连接池；授权真相由**组合根注入**（§3.9 去环）。
    /// dev 构建额外注入 fixture（Release 无此路径）。
    ///
    /// 【Part6 §3.9.1a ①】`licenses` 由调用方（组合根 `State::exotic_host` → [`default_entitlement_provider`]）
    /// 注入，本函数不再内建 keyring provider——使 provider 装配收敛到单点 swap 函数，且 `for_runtime`
    /// 对具体授权渠道保持无知（与 [`ExoticHost::with_sources`] 同哲学）。
    pub fn for_runtime(
        catalog: Arc<CatalogStore>,
        pool: DbPool,
        licenses: Arc<dyn EntitlementProvider>,
    ) -> Self {
        let installed: Arc<dyn InstalledSource> = Arc::new(DbInstalledSource { pool });
        ExoticHost {
            catalog,
            installed,
            licenses,
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
                store_url: None,
                installed_version: None,
                builtin: false,
            };
        };
        let installed = self.installed.get(&off.plugin_id);
        let installed_version = installed.as_ref().map(|r| r.version.clone());
        let availability = self.availability_of(off, installed.as_ref());
        FormatResolution {
            format: format.to_string(),
            media_kind: off.media_kind,
            plugin_id: Some(off.plugin_id.clone()),
            display_name: off.display_name.clone(),
            capabilities: off.capabilities.clone(),
            availability,
            store_url: if crate::official::includes(&off.plugin_id) {
                crate::official::store_url()
            } else {
                None
            },
            installed_version,
            builtin: off.builtin,
        }
    }

    /// 折叠某 offering 的可用态。门控顺序（即防御纵深）：
    /// 平台 → Host 版本 → (dev fixture) → 安装状态 → 授权状态。
    fn availability_of(
        &self,
        off: &CatalogOffering,
        installed: Option<&InstalledPluginRecord>,
    ) -> Availability {
        if !off.supports_platform(current_target_triple()) {
            return Availability::UnsupportedPlatform;
        }
        if !host_meets_min(&off.min_host_version, HOST_VERSION) {
            return Availability::IncompatibleHost;
        }
        // dev/test fixture：平台/版本已过即授权。Release 构建该分支**编译期消除**（§5.4 防御纵深）。
        #[cfg(any(test, all(debug_assertions, feature = "exotic-dev-fixtures")))]
        if self.authorized_fixture.as_deref() == Some(off.plugin_id.as_str()) {
            return Availability::Authorized;
        }
        // builtin offering(D-OCR-5):无安装包,跳过安装态门,直接验 license。
        if off.builtin {
            // free 档(D-427 一期免费 RAW):显式判 license_tier=="free" 才放行,
            // 付费能力只有固定套装权益可验权，不能因配置缺 SKU 放行。
            if off.license_tier == "free" {
                return Availability::Authorized;
            }
            if !crate::official::includes(&off.plugin_id) {
                return Availability::InstalledUnlicensed;
            }
            return match crate::official::status(self.licenses.as_ref(), now_secs()) {
                LicenseStatus::Authorized => Availability::Authorized,
                LicenseStatus::Expired => Availability::LicenseExpired,
                // 有意与 package 流(KeyringUnavailable→InstalledUnlicensed)分叉:builtin 无安装态可退,
                // 统一投射为可购态(fail-closed,不误授权);keyring 瞬时故障恢复后下次查询自愈。
                LicenseStatus::Unlicensed | LicenseStatus::KeyringUnavailable => {
                    Availability::AvailableUninstalled
                }
            };
        }
        let Some(rec) = installed else {
            return Availability::AvailableUninstalled;
        };
        match rec.install_state.as_str() {
            install_state::BROKEN => return Availability::InvalidInstallation,
            install_state::DISABLED => return Availability::Disabled,
            _ => {}
        }
        if off.license_tier == "free" {
            return Availability::Authorized;
        }
        if !crate::official::includes(&off.plugin_id) {
            return Availability::InstalledUnlicensed;
        }
        match crate::official::status(self.licenses.as_ref(), now_secs()) {
            LicenseStatus::Authorized => Availability::Authorized,
            LicenseStatus::Expired => Availability::LicenseExpired,
            // 无 token / 不匹配 / keyring 不可用 → 已装未授权（不可证明授权即按未授权，fail-closed）。
            LicenseStatus::Unlicensed | LicenseStatus::KeyringUnavailable => {
                Availability::InstalledUnlicensed
            }
        }
    }

    /// 列出 Catalog 中全部格式的解析结果（前端 `list_exotic_format_resolutions` 用）。
    /// 即使未安装 PSD 也会出现 `AvailableUninstalled`（首次离线也显示购买占位）。
    pub fn list_resolutions(&self) -> Vec<FormatResolution> {
        let snap = self.catalog.snapshot();
        snap.iter_formats()
            .map(|(fmt, _)| self.resolve_format(fmt))
            .collect()
    }

    /// 某 (plugin, capability) 当前是否**可领取处理**（Scheduler/Coordinator 门控用，v3 §5.3）。
    ///
    /// runnable ⟺ 有 offering、平台兼容、Host 版本兼容、已安装且授权（或 dev fixture）、声明了该能力。
    /// 未安装/未授权/平台不支持/禁用/损坏都返回 false——这些状态**不**写任务状态，由本门控拦在领取前。
    pub fn is_task_runnable(&self, plugin_id: &str, capability: Capability) -> bool {
        let snap = self.catalog.snapshot();
        // 找到该插件的任一 format，按其可用态判定（同一插件多 format 共享 offering）。
        let Some((fmt, off)) = snap.iter_formats().find(|(_, o)| o.plugin_id == plugin_id) else {
            return false;
        };
        if !off.claims_capability(capability) {
            return false;
        }
        matches!(
            self.resolve_format(fmt).availability,
            Availability::Authorized
        )
    }

    /// 某插件的授权判定（前端 gate / 购买引导用，Part6 §3.8）：折叠其任一 format 的可用态，
    /// 附 [`EntitlementProvider`] 的来源渠道 + 该插件 sku + 购买链接。判定全在后端，前端不持验签逻辑。
    /// catalog 无此插件 → `None`（命令层转 `no_offering` 错误）。
    pub fn entitlement_of(&self, plugin_id: &str) -> Option<PluginEntitlement> {
        let snap = self.catalog.snapshot();
        // 同一插件多 format 共享 offering，取任一即可（与 is_task_runnable 同法）。
        let (fmt, off) = snap
            .iter_formats()
            .find(|(_, o)| o.plugin_id == plugin_id)?;
        let res = self.resolve_format(fmt);
        Some(PluginEntitlement {
            plugin_id: plugin_id.to_string(),
            availability: res.availability,
            source_tag: self.licenses.source_tag().to_string(),
            sku: (off.license_tier != "free" && crate::official::includes(plugin_id))
                .then(|| crate::official::product().sku.clone()),
            store_url: res.store_url,
        })
    }
}

/// dev fixture 授权插件（仅 debug + `exotic-dev-fixtures`；Release 恒 None，§5.4）。
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

/// 当前 unix 秒（License 时间窗判定用）。时钟早于 UNIX_EPOCH（VM/容器时钟异常）时返回 0：
/// fail-closed（now=0 < 内置 not_before → 一律未授权，绝不误授权），但单独告警以便运维定位
/// 「激活失败实为系统时钟异常而非 token 问题」（安全评审 low）。不输出任何 token 材料。
fn now_secs() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => {
            tracing::warn!("系统时钟早于 UNIX_EPOCH，exotic 授权按 fail-closed 处理（now=0）");
            0
        }
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
