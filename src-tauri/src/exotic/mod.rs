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
pub mod channel_stubs;
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
use channel_stubs::FreeStubEntitlement;
pub use crypto::VerifyingKeyset;
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
    /// 安装来源渠道(T13 多渠道预留):direct / steam_depot / store_bundled(现恒 direct;
    /// 与 DB `entitlement_source` 列同值域,见 installer::InstallSource)。
    pub entitlement_source: String,
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
    /// 授权来源渠道（"direct" / "free"，后续 "ms_store" / "steam"），取自 [`EntitlementProvider::source_tag`]。
    pub source_tag: String,
    /// 付费插件的 sku（免费 / 无 sku 插件为 None）。
    pub sku: Option<String>,
    /// 购买 / 商店链接（未授权时的购买引导用）。
    pub store_url: Option<String>,
}

/// 组合根 · 授权 provider 装配点（Part6 §3.9 **唯一 swap 点**）。
///
/// **Part7-T12 渠道工厂化**:分发渠道 feature(互斥,lib.rs compile_error! 守卫)在编译期决定
/// provider 家族——msstore/steam 现为 fail-closed 骨架桩(恒 Unlicensed,见 [`channel_stubs`]),
/// 真实 StoreContext/DLC ownership 归 Part8 D5-D8;direct 走下方 keyring 直销装配。
/// §3.6.2 的 DRM 物理排除因此有了工厂侧的选择点。
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
    #[cfg(feature = "channel-msstore")]
    return Arc::new(channel_stubs::MsStoreEntitlementStub);

    #[cfg(feature = "channel-steam")]
    return Arc::new(channel_stubs::SteamEntitlementStub);

    #[cfg(feature = "channel-direct")]
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
            store_url: off.store_url.clone(),
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
            // 不可用"无 sku 即放行"代替——否则会误放行 builtin 的 paid 插件(sku 缺失时应 fail-closed)。
            if off.license_tier == "free" {
                return Availability::Authorized;
            }
            let Some(sku) = off.sku.as_deref() else {
                return Availability::InstalledUnlicensed;
            };
            return match self.licenses.evaluate(&off.plugin_id, sku, now_secs()) {
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
        // 已安装且启用 → 查授权。expected_sku 取自**可信** Catalog（绝不取 token 自身，§5.2）。
        let Some(sku) = off.sku.as_deref() else {
            // paid 插件却无 SKU = 无法验签 → 已装也只能未授权。
            return Availability::InstalledUnlicensed;
        };
        match self.licenses.evaluate(&off.plugin_id, sku, now_secs()) {
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
            sku: off.sku.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin_host() -> ExoticHost {
        let store = Arc::new(CatalogStore::from_builtin().unwrap());
        ExoticHost::new(store)
    }

    /// 测试假源：固定返回某安装记录。
    struct FakeInstalled(Option<InstalledPluginRecord>);
    impl InstalledSource for FakeInstalled {
        fn get(&self, _plugin_id: &str) -> Option<InstalledPluginRecord> {
            self.0.clone()
        }
    }
    /// 测试假源：固定返回某授权态。
    struct FakeLicense(LicenseStatus);
    impl EntitlementProvider for FakeLicense {
        fn evaluate(&self, _plugin_id: &str, _sku: &str, _now: i64) -> LicenseStatus {
            self.0
        }
        fn source_tag(&self) -> &'static str {
            "test"
        }
    }

    /// 构造平台无关的测试 Catalog（PSD offering 的平台 = 当前 triple，sku 已设），避免矩阵
    /// 测试在非 win/mac 平台退化为 UnsupportedPlatform。
    fn local_catalog() -> Arc<CatalogStore> {
        let json = format!(
            r#"{{"schema":1,"sequence":1,"offerings":[
              {{"plugin_id":"exotic-image-psd","name":"PSD","media_kind":"image","formats":["psd"],
               "capabilities":["thumbnail"],"license_tier":"paid","sku":"psd-engine-2026",
               "platforms":["{}"],"min_host_version":"0.1.0"}}
            ]}}"#,
            current_target_triple()
        );
        Arc::new(CatalogStore::with_snapshot(
            CatalogSnapshot::parse(&json).unwrap(),
        ))
    }

    fn installed_rec(state: &str) -> InstalledPluginRecord {
        InstalledPluginRecord {
            plugin_id: "exotic-image-psd".into(),
            version: "1.0.0".into(),
            manifest_hash: "deadbeef".into(),
            package_sequence: 3,
            install_state: state.into(),
            installed_at: 1,
            updated_at: 1,
            entitlement_source: "direct".into(),
        }
    }

    fn host_with(installed: Option<InstalledPluginRecord>, lic: LicenseStatus) -> ExoticHost {
        ExoticHost::with_sources(
            local_catalog(),
            Arc::new(FakeInstalled(installed)),
            Arc::new(FakeLicense(lic)),
        )
    }

    #[test]
    fn unknown_format_is_no_offering() {
        let host = builtin_host();
        let r = host.resolve_format("jpg");
        assert!(matches!(r.availability, Availability::NoOffering));
        assert!(r.plugin_id.is_none());
    }

    #[test]
    fn list_includes_psd() {
        let host = builtin_host();
        let all = host.list_resolutions();
        assert!(all.iter().any(|r| r.format == "psd"));
    }

    #[test]
    fn not_installed_is_available_uninstalled() {
        // 未安装 → AvailableUninstalled（即使授权态恰为 Authorized 也不抢跑——先要装）。
        let host = host_with(None, LicenseStatus::Authorized);
        let r = host.resolve_format("psd");
        assert_eq!(r.availability, Availability::AvailableUninstalled);
        assert!(r.installed_version.is_none());
        assert!(!host.is_task_runnable("exotic-image-psd", Capability::Thumbnail));
    }

    #[test]
    fn installed_unlicensed() {
        let host = host_with(
            Some(installed_rec(install_state::INSTALLED)),
            LicenseStatus::Unlicensed,
        );
        let r = host.resolve_format("psd");
        assert_eq!(r.availability, Availability::InstalledUnlicensed);
        assert_eq!(r.installed_version.as_deref(), Some("1.0.0"));
        assert!(!host.is_task_runnable("exotic-image-psd", Capability::Thumbnail));
    }

    #[test]
    fn installed_and_authorized_is_runnable() {
        let host = host_with(
            Some(installed_rec(install_state::INSTALLED)),
            LicenseStatus::Authorized,
        );
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::Authorized
        );
        assert!(host.is_task_runnable("exotic-image-psd", Capability::Thumbnail));
    }

    #[test]
    fn entitlement_of_reports_source_sku_and_handles_unknown() {
        // 已装 + 授权 → Authorized；source_tag 取自 EntitlementProvider（FakeLicense="test"）；
        // sku 取自 catalog offering（PSD 已设 sku）。
        let host = host_with(
            Some(installed_rec(install_state::INSTALLED)),
            LicenseStatus::Authorized,
        );
        let ent = host
            .entitlement_of("exotic-image-psd")
            .expect("PSD 在 catalog");
        assert_eq!(ent.availability, Availability::Authorized);
        assert_eq!(ent.source_tag, "test");
        assert!(ent.sku.is_some());
        // catalog 无此插件 → None（命令层转 no_offering）。
        assert!(host.entitlement_of("nonexistent-plugin").is_none());
    }

    #[test]
    fn license_expired_maps_through() {
        let host = host_with(
            Some(installed_rec(install_state::INSTALLED)),
            LicenseStatus::Expired,
        );
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::LicenseExpired
        );
        assert!(!host.is_task_runnable("exotic-image-psd", Capability::Thumbnail));
    }

    #[test]
    fn broken_install_is_invalid() {
        let host = host_with(
            Some(installed_rec(install_state::BROKEN)),
            LicenseStatus::Authorized,
        );
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::InvalidInstallation
        );
    }

    #[test]
    fn disabled_install() {
        let host = host_with(
            Some(installed_rec(install_state::DISABLED)),
            LicenseStatus::Authorized,
        );
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::Disabled
        );
    }

    #[test]
    fn keyring_unavailable_is_unlicensed() {
        let host = host_with(
            Some(installed_rec(install_state::INSTALLED)),
            LicenseStatus::KeyringUnavailable,
        );
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::InstalledUnlicensed
        );
    }

    #[test]
    fn fixture_authorizes_without_install_or_license() {
        // dev/test fixture：无安装记录、无 token 也 Authorized（仅测试/dev 路径）。
        let store = local_catalog();
        let host = ExoticHost::with_authorized_fixture(store, "exotic-image-psd");
        assert_eq!(
            host.resolve_format("psd").availability,
            Availability::Authorized
        );
    }

    /// builtin offering 测试专用 catalog(distribution="builtin",与 local_catalog 平台策略同款)。
    fn local_ocr_catalog() -> Arc<CatalogStore> {
        let json = format!(
            r#"{{"schema":1,"sequence":1,"offerings":[
              {{"plugin_id":"exotic-ocr","name":"OCR","media_kind":"image","formats":["ocr"],
               "capabilities":["text"],"license_tier":"paid","sku":"ocr-engine-2026",
               "platforms":["{}"],"min_host_version":"0.1.0","distribution":"builtin"}}
            ]}}"#,
            current_target_triple()
        );
        Arc::new(CatalogStore::with_snapshot(
            CatalogSnapshot::parse(&json).unwrap(),
        ))
    }

    #[test]
    fn builtin_offering_no_token_is_available_uninstalled() {
        // builtin:无安装真相输入(installed=None)也不判 AvailableUninstalled-by-install——
        // 直接验 license;无 token → AvailableUninstalled(D-OCR-5)。
        let host = ExoticHost::with_sources(
            local_ocr_catalog(),
            Arc::new(FakeInstalled(None)),
            Arc::new(FakeLicense(LicenseStatus::Unlicensed)),
        );
        assert_eq!(
            host.resolve_format("ocr").availability,
            Availability::AvailableUninstalled
        );
    }

    #[test]
    fn builtin_offering_fixture_is_authorized() {
        // dev fixture 对 builtin 分支同样生效(平台/版本门之后即命中,不经安装/授权真相)。
        let host = ExoticHost::with_authorized_fixture(local_ocr_catalog(), "exotic-ocr");
        assert_eq!(
            host.resolve_format("ocr").availability,
            Availability::Authorized
        );
    }

    #[test]
    fn builtin_offering_keyring_unavailable_diverges_from_package_flow() {
        // 锁分叉:同一输入(KeyringUnavailable)下 package 流给 InstalledUnlicensed(见
        // keyring_unavailable_is_unlicensed),builtin 流有意给 AvailableUninstalled——
        // 防未来"统一"两分支回归。
        let host = ExoticHost::with_sources(
            local_ocr_catalog(),
            Arc::new(FakeInstalled(None)),
            Arc::new(FakeLicense(LicenseStatus::KeyringUnavailable)),
        );
        assert_eq!(
            host.resolve_format("ocr").availability,
            Availability::AvailableUninstalled
        );
    }

    #[test]
    fn builtin_offering_expired_license_maps_through() {
        let host = ExoticHost::with_sources(
            local_ocr_catalog(),
            Arc::new(FakeInstalled(None)),
            Arc::new(FakeLicense(LicenseStatus::Expired)),
        );
        assert_eq!(
            host.resolve_format("ocr").availability,
            Availability::LicenseExpired
        );
    }

    /// 组合根装配冒烟:按渠道 feature 断言 source_tag——direct = keyring 直销(内置 keyset 可解析;
    /// 解析失败会降级 FreeStubEntitlement 使标签变 "free",故本断言同时锁住 fail-closed 回退面),
    /// msstore/steam = fail-closed 骨架桩。渠道 feature 互斥(lib.rs compile_error!),三选一。
    #[test]
    fn default_provider_matches_channel_feature() {
        let p = default_entitlement_provider();
        #[cfg(feature = "channel-msstore")]
        assert_eq!(p.source_tag(), "ms_store");
        #[cfg(feature = "channel-steam")]
        assert_eq!(p.source_tag(), "steam");
        #[cfg(feature = "channel-direct")]
        assert_eq!(p.source_tag(), "direct");
    }

    #[test]
    fn host_meets_min_version_compare() {
        assert!(host_meets_min("0.1.0", "0.1.0"));
        assert!(host_meets_min("0.1.0", "0.2.0"));
        assert!(host_meets_min("0.1.0", "1.0.0"));
        assert!(!host_meets_min("0.2.0", "0.1.9"));
        assert!(!host_meets_min("1.0.0", "0.9.9"));
        assert!(host_meets_min("0.1", "0.1.0")); // 段数不等
        assert!(!host_meets_min("weird", "0.1.0")); // min 非数值 → 保守拒绝（fail-closed）
        assert!(!host_meets_min("1.0.0-rc.1", "1.0.0")); // 含预发布标记 → 保守拒绝
    }
}
