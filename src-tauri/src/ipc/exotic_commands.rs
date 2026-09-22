//! 冷门格式插件 · 前端查询命令（Part1 §2.3）。
//!
//! 本卷只读：返回能力解析与（Part1 为空的）安装真相。处理控制命令
//! （start/pause/stop/retry…）与下载/激活在 Part2/Part3 落地。
//! 所有 DTO 以 camelCase 序列化，前端类型直接对齐，避免手写字段转换漂移。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::exotic::{ExoticTaskStatus, FormatResolution, InstalledExoticPlugin, PluginEntitlement};
use crate::state::AppState;

/// 列出 Catalog 中**全部**格式的解析结果。未安装 PSD 也会得到 `availableUninstalled`
/// （首次离线也显示购买占位）。
#[tauri::command]
pub async fn list_exotic_format_resolutions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<FormatResolution>> {
    // 运行期 Host：含真实安装/授权真相（已安装并激活的 PSD 显示 Authorized 而非 AvailableUninstalled）。
    // R1-3：host 解析内部是 rusqlite 读 + keyring 系统调用，离开 tokio worker。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || Ok(state_arc.exotic_host().list_resolutions()))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 单个媒体项的 exotic 状态（可用态 + 处理态分离，对齐前端）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExoticItemState {
    pub item_id: i64,
    pub format: String,
    /// 该格式可用态；非 catalog 格式为 None。
    pub resolution: Option<FormatResolution>,
    /// thumbnail 任务处理态：none/pending/processing/done/retryableError/terminalError。
    pub task_state: String,
}

/// 任务状态 → 前端 `ExoticTaskState` 字符串。
fn task_state_str(s: Option<ExoticTaskStatus>) -> &'static str {
    match s {
        None => "none",
        Some(ExoticTaskStatus::Pending) => "pending",
        Some(ExoticTaskStatus::Processing) => "processing",
        Some(ExoticTaskStatus::Done) => "done",
        Some(ExoticTaskStatus::RetryableError) => "retryableError",
        Some(ExoticTaskStatus::TerminalError) => "terminalError",
    }
}

/// 查某 item 的可用态 + thumbnail 任务态。
#[tauri::command]
pub async fn get_exotic_item_state(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<ExoticItemState> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<ExoticItemState> {
        let conn = state_arc.db_read_pool.get().map_err(AppError::from)?;
        let item = crate::db::queries::get_media_item(&conn, item_id)?;

        let snap = state_arc.exotic_catalog.snapshot();
        let resolution = if snap.resolve_format(&item.file_format).is_some() {
            let host = state_arc.exotic_host();
            let resolved = host.resolve_format(&item.file_format);
            if resolved
                .plugin_id
                .as_deref()
                .is_some_and(crate::official::includes)
            {
                crate::official::entitlement(state_arc.entitlement_provider().as_ref())?;
            }
            Some(resolved)
        } else {
            None
        };

        let task_map =
            crate::db::queries::exotic_thumbnail_task_status_for_items(&conn, &[item_id])?;
        let task_state = task_state_str(task_map.get(&item_id).copied()).to_string();

        Ok(ExoticItemState {
            item_id,
            format: item.file_format,
            resolution,
            task_state,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 列出已安装插件（Part1 安装表为空 → 空列表）。
#[tauri::command]
pub async fn list_installed_exotic_plugins(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<InstalledExoticPlugin>> {
    super::blocking::read_blocking(&state, crate::db::queries::list_installed_exotic_plugins).await
}

/// 某插件的授权判定（前端 gate / 购买引导用，Part6 §3.8）。判定全在后端 EntitlementProvider；
/// 前端据此 gate / 购买引导，**不持任何验签逻辑**。catalog 无此插件 → `no_offering`（前端可按 code 分流）。
#[tauri::command]
pub async fn get_plugin_entitlement(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<PluginEntitlement> {
    // R1-3：entitlement_of 内含 DB 读 + keyring 验签（同步系统调用），离开 tokio worker。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        if crate::official::includes(&plugin_id) {
            crate::official::entitlement(state_arc.entitlement_provider().as_ref())?;
        }
        state_arc
            .exotic_host()
            .entitlement_of(&plugin_id)
            .ok_or_else(|| AppError::Exotic {
                code: "no_offering",
                message: format!("Catalog 无此插件：{plugin_id}"),
            })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ── 处理控制命令（Part2 §4.5）──────────────────────────────────────────────────
//
// 语义区分：
//   - start：清 paused + UserRequested wake（绕过 auto 门控，即使 exotic_auto_process=false
//     也跑本轮；不修改 auto 配置——恢复「随扫描自动处理」由设置项 exotic_auto_process 控制）。
//   - pause：paused=true，不再启动新一轮 Pipeline；在途运行自然完成其批次。
//   - stop：取消**本次**运行（在途任务退回 pending）；**不**等于永久禁用（不动 paused）。
// 状态变化经 `exotic:status-changed` 事件推送前端。

use std::time::Duration;

use crate::exotic::catalog::Capability;
use crate::exotic::coordinator::{WakeReason, PSD_PLUGIN_ID};
use crate::exotic::crypto::VerifyingKeyset;
use crate::exotic::install::RegistryExpect;
use crate::exotic::install::{plugin_install_dir, rollback_to_backup};
use crate::exotic::installer;
use crate::exotic::installer::InstallContext;
use crate::exotic::registry::RegistryCache;

const CAPABILITY_STR: &str = "thumbnail";

/// 安装/卸载前静默 exotic 子系统的等待上限（kill→wait Worker、释放句柄）。
const QUIESCE_TIMEOUT: Duration = Duration::from_secs(8);

/// 当前 unix 秒（License 时间窗判定）。
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 恢复自动处理：清 paused 并唤醒调度。
#[tauri::command]
pub async fn start_exotic_processing(state: State<'_, Arc<AppState>>) -> Result<()> {
    super::blocking::write_blocking(&state, |c| {
        crate::db::queries::set_config(c, "exotic_paused", "false")
    })
    .await?;
    // 用户显式开始：绕过 auto 门控（即使 exotic_auto_process=false 也运行本轮，P2）。
    state.wake_exotic(WakeReason::UserRequested);
    Ok(())
}

/// 暂停：置 paused（不再启动新一轮；在途批次自然结束）。
#[tauri::command]
pub async fn pause_exotic_processing(state: State<'_, Arc<AppState>>) -> Result<()> {
    super::blocking::write_blocking(&state, |c| {
        crate::db::queries::set_config(c, "exotic_paused", "true")
    })
    .await
}

/// 停止本次运行：取消在途 Pipeline（任务退回 pending）。不修改 paused（非永久禁用）。
#[tauri::command]
pub async fn stop_exotic_processing(state: State<'_, Arc<AppState>>) -> Result<()> {
    state.cancel_exotic_analysis();
    Ok(())
}

/// 处理状态摘要（对齐前端 camelCase）。`blockedByAvailability` 单列「未购买/平台不支持」而卡住的项，
/// 避免进度条永久停 0%（Part2 §4.5）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExoticProcessingStatus {
    /// 待处理（pending + 待重试）。
    pub pending: i64,
    pub processing: i64,
    pub done: i64,
    pub error: i64,
    /// 因不可领取（未授权/平台不支持/未安装）而卡住的待处理数。
    pub blocked_by_availability: i64,
    pub running: bool,
    pub paused: bool,
}

/// 取处理状态摘要。
#[tauri::command]
pub async fn get_exotic_processing_status(
    state: State<'_, Arc<AppState>>,
) -> Result<ExoticProcessingStatus> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<ExoticProcessingStatus> {
        let conn = state_arc.db_read_pool.get().map_err(AppError::from)?;
        let (pending, processing, done, error) =
            crate::db::queries::count_exotic_tasks_by_status(&conn, PSD_PLUGIN_ID, CAPABILITY_STR)?;
        let paused = crate::db::queries::get_config(&conn, "exotic_paused")?
            .map(|v| v == "true")
            .unwrap_or(false);

        let host = state_arc.exotic_host();
        let runnable = host.is_task_runnable(PSD_PLUGIN_ID, Capability::Thumbnail);
        // 不可领取时，待处理项实为「被可用态阻塞」——单列以免前端误判为进度卡死。
        let blocked_by_availability = if runnable { 0 } else { pending };

        Ok(ExoticProcessingStatus {
            pending,
            processing,
            done,
            error,
            blocked_by_availability,
            running: state_arc.is_exotic_running(),
            paused,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 处理详情行(商店进度「展开详情」,2026-07-05 内测需求):文件级任务投影。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExoticTaskDetail {
    pub item_id: i64,
    pub file_name: String,
    /// 所在目录相对扫描根的路径(根目录为空串)。
    pub dir_path: String,
    pub format: String,
    /// pending / retrying / processing / done / error(与进度摘要四桶对齐,3 细分为 retrying)。
    pub status: String,
    pub attempts: i64,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

/// 取处理详情列表。`bucket`:None=全部,或 pending/processing/done/error(与摘要四桶一致);
/// limit 钳 1..=200、offset ≥0(「加载更多」分页)。排序=活动优先(processing→待重试→pending→error→done)。
#[tauri::command]
pub async fn list_exotic_task_details(
    bucket: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ExoticTaskDetail>> {
    // 桶白名单:未知值显式拒绝(稳定错误码),不静默降级为全量。
    if let Some(b) = bucket.as_deref() {
        if !matches!(b, "pending" | "processing" | "done" | "error") {
            return Err(AppError::Exotic {
                code: "bad_bucket",
                message: format!("未知筛选桶:{b}"),
            });
        }
    }
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<ExoticTaskDetail>> {
        let conn = state_arc.db_read_pool.get().map_err(AppError::from)?;
        let rows = crate::db::queries::list_exotic_task_details(
            &conn,
            PSD_PLUGIN_ID,
            CAPABILITY_STR,
            bucket.as_deref(),
            limit,
            offset,
        )?;
        Ok(rows
            .into_iter()
            .map(|r| ExoticTaskDetail {
                item_id: r.item_id,
                file_name: r.file_name,
                dir_path: r.dir_path,
                format: r.format,
                status: match r.status {
                    1 => "processing",
                    2 => "done",
                    3 => "retrying",
                    4 => "error",
                    _ => "pending",
                }
                .into(),
                attempts: r.attempts,
                last_error_code: r.last_error_code,
                last_error_message: r.last_error_message,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 重试某插件全部失败任务（error → pending）并唤醒。
#[tauri::command]
pub async fn retry_exotic_plugin_failures(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    super::blocking::write_blocking(&state, move |c| {
        crate::db::queries::reset_exotic_plugin_failures(c, &plugin_id)
    })
    .await?;
    // 用户点重试全部失败：绕过 auto 门控（P2）。
    state.wake_exotic(WakeReason::UserRequested);
    Ok(())
}

// ── 安装 / 卸载 / 修复 / 回滚 / Registry（Part3 §6.4-6.6）──────────────────────────
//
// 命令参数**只**接受 plugin_id（已验证字符集），绝不接受 URL/路径/hash/可执行路径（§6.6）。
// 安装目录/下载坐标均由已验签 Registry 与 AppState 派生路径决定。替换/删除目录前先 quiesce。

const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

fn builtin_keyset() -> Result<VerifyingKeyset> {
    // dev keyset 旁路统一收敛在 exotic::trusted_keyset(debug-only,详见彼处)。
    crate::exotic::trusted_keyset().map_err(|e| AppError::Exotic {
        code: e.code(),
        message: format!("信任根不可用：{}", e.code()),
    })
}

/// Registry 条目 DTO（前端市场用；camelCase；不暴露内部下载坐标 hash/size/url）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExoticRegistryEntry {
    pub plugin_id: String,
    /// 展示名（来自内置 Catalog；Catalog 未知时回退为 plugin_id）。
    pub name: String,
    pub version: String,
    pub formats: Vec<String>,
    pub capabilities: Vec<String>,
    pub sku: String,
    pub target: String,
    pub package_sequence: i64,
    pub store_url: Option<String>,
    /// 该 Registry 是否已过期（过期仍展示，但不允许新装，§6.1）。
    pub registry_expired: bool,
}

/// 远程签名 Registry 基址(部署配置点)。下载坐标 = `{base}/index.json` + `{base}/index.sig`。
/// 解析优先级(高→低):
/// 1. **运行时**环境变量 `PICASA_REGISTRY_BASE`(dev / 自托管 / 测试用,不重编即可切换);
/// 2. **编译期** `option_env!("PICASA_REGISTRY_BASE_DEFAULT")`——内测/发布流水线把默认
///    发行源烘焙进安装包,装机用户零配置(2026-07-05 内测链落地);
/// 3. 🔴 占位域名(RFC 2606 保留 `.invalid`,绝不解析到真实主机):正式 CDN 待命名/法务
///    定稿后替换。后续如需「按安装实例覆盖」可下沉至 app_config,当前常量足矣。
const DEFAULT_REGISTRY_BASE_URL: &str = match option_env!("PICASA_REGISTRY_BASE_DEFAULT") {
    Some(v) => v,
    None => "https://registry.example.invalid/exotic/v1",
};

/// 解析当前生效的 Registry 基址：环境变量优先（非空），否则部署默认常量。
fn registry_base_url() -> String {
    std::env::var("PICASA_REGISTRY_BASE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_BASE_URL.to_string())
}

/// `fetch_exotic_registry` 拉取结果摘要（前端「刷新」后用：装得了几个、序号、是否过期）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySummary {
    /// 本次接受且对当前用户可见（当前平台 + Catalog 已知）的条目数。
    pub plugin_count: usize,
    /// 已接受的 registry_sequence（单调防回滚基线）。
    pub sequence: u64,
    /// 该 index 是否已过期（过期仍写缓存供展示，但安装路径拒绝，§6.1）。
    pub expired: bool,
}

/// 判断 Registry 条目是否应当展示给当前用户：
/// 1. target 必须匹配当前平台；
/// 2. plugin_id 必须已登记在当前内置 Catalog（否则安装后也会 catalog_reject）。
fn registry_entry_visible(
    entry: &crate::exotic::registry::RegistryEntry,
    catalog: &crate::exotic::CatalogSnapshot,
) -> bool {
    let target = crate::exotic::current_target_triple();
    entry.target == target
        && catalog
            .iter_formats()
            .any(|(_, o)| o.plugin_id == entry.plugin_id && o.supports_platform(target))
}

/// 取 Registry 条目的展示名：优先 Catalog 的 display_name，找不到则回退 plugin_id。
fn registry_entry_name(
    entry: &crate::exotic::registry::RegistryEntry,
    catalog: &crate::exotic::CatalogSnapshot,
) -> String {
    catalog
        .iter_formats()
        .find(|(_, o)| o.plugin_id == entry.plugin_id)
        .map(|(_, o)| o.display_name.clone())
        .unwrap_or_else(|| entry.plugin_id.clone())
}

/// 🔴 P0 阻断修复：拉取远程签名 Registry → 验签 + 单调防回滚 → 原子写本地缓存。
/// 全新设备本地缓存为空 → `list_exotic_registry` 返回空 → 装不了任何插件；本命令补上「下载 + accept」
/// 这一薄层，闭合「全新设备可装插件」链路。前端首启 / 进插件商店 / 点「刷新」时调用（Part5 §3.5.1 消费）。
///
/// 安全（§6.6 纵深防御）：命令**只触发**，不接受任何 URL/路径——下载坐标由部署常量（可 env 覆盖）决定；
/// 验签 + 防回滚由 `RegistryCache::accept` 在解析前完成（registry.rs:242），前端无从注入下载源或绕过验签。
#[tauri::command]
pub async fn fetch_exotic_registry(state: State<'_, Arc<AppState>>) -> Result<RegistrySummary> {
    // 与 install/uninstall/rollback 串行：避免「拉取改写缓存」与「安装读取缓存」并发产生撕裂窗口。
    let _guard = state.exotic_install_lock.lock().await;
    let keyset = builtin_keyset()?;
    let now = now_secs();
    let base = registry_base_url();

    // 1. 下载原始 index.json + index.sig（仅 HTTPS、大小封顶；**不在此验签**）。
    let (index_bytes, sig_bytes) = crate::exotic::fetch::download_registry_index(&base)
        .await
        .map_err(|e| AppError::Exotic {
            code: e.code(),
            message: format!("拉取 Registry 失败：{}", e.code()),
        })?;

    // 2. accept：验签 + 单调防回滚先于解析，通过后原子写缓存（index.json/.sig/.seq）。
    //    收到更低 sequence（回滚攻击/冻结）→ RollbackRejected，缓存不变、报错——这是安全红线，不可绕过。
    let mut cache = RegistryCache::load(state.exotic_registry_dir());
    let verified = cache
        .accept(&index_bytes, &sig_bytes, &keyset, now)
        .map_err(|e| AppError::Exotic {
            code: e.code(),
            message: format!("Registry 验签/接受失败：{}", e.code()),
        })?;

    let catalog = state.exotic_catalog.snapshot();
    let plugin_count = verified
        .index
        .plugins
        .iter()
        .filter(|e| registry_entry_visible(e, catalog.as_ref()))
        .count();

    Ok(RegistrySummary {
        plugin_count,
        sequence: verified.index.sequence,
        expired: verified.expired,
    })
}

/// 列出签名 Registry 的可安装条目（从本地缓存读 + 验签）。无缓存 → 空列表。
#[tauri::command]
pub async fn list_exotic_registry(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ExoticRegistryEntry>> {
    let keyset = builtin_keyset()?;
    let cache = RegistryCache::load(state.exotic_registry_dir());
    let Some(v) = cache.load_verified(&keyset, now_secs()) else {
        return Ok(Vec::new());
    };
    let expired = v.expired;
    let catalog = state.exotic_catalog.snapshot();
    Ok(v.index
        .plugins
        .iter()
        .filter(|e| registry_entry_visible(e, catalog.as_ref()))
        .map(|e| ExoticRegistryEntry {
            plugin_id: e.plugin_id.clone(),
            name: registry_entry_name(e, catalog.as_ref()),
            version: e.version.clone(),
            formats: e.formats.clone(),
            capabilities: e.capabilities.clone(),
            sku: e.sku.clone(),
            target: e.target.clone(),
            package_sequence: e.package_sequence,
            store_url: e.store_url.clone(),
            registry_expired: expired,
        })
        .collect())
}

/// 安装插件（§6.4）：从已验签 Registry 选条目 →（下载 zip 到 staging，**待 P6.2**）→ 安全安装。
#[tauri::command]
pub async fn install_exotic_plugin(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 串行化安装/卸载/回滚（防并发目录变更产生破损窗口，安全评审 medium）。
    let _guard = state.exotic_install_lock.lock().await;
    let keyset = builtin_keyset()?;
    let now = now_secs();
    let install_root = state.exotic_install_dir();
    // 显式校验 plugin_id 字符集（不依赖 Registry lookup 副作用，§6.6 纵深防御）。
    plugin_install_dir(&install_root, &plugin_id).ok_or_else(|| AppError::Exotic {
        code: "invalid_plugin_id",
        message: "非法 plugin_id".into(),
    })?;

    // 1. 从已验签 Registry 选 plugin_id + 当前 target 条目。
    let cache = RegistryCache::load(state.exotic_registry_dir());
    let verified = cache
        .load_verified(&keyset, now)
        .ok_or_else(|| AppError::Exotic {
            code: "no_registry_cache",
            message: "无可用 Registry 缓存（需先刷新）".into(),
        })?;
    if verified.expired {
        return Err(AppError::Exotic {
            code: "registry_expired",
            message: "Registry 已过期，不能从过期元数据执行新安装".into(),
        });
    }
    let target = crate::exotic::current_target_triple();
    let entry = verified
        .index
        .select(&plugin_id, target)
        .cloned()
        .ok_or_else(|| AppError::Exotic {
            code: "plugin_not_in_registry",
            message: format!("Registry 无 {plugin_id} @ {target}"),
        })?;

    // 1.5 模型权重 blob 分步下载(Part4 §3.7.1/T12):独立于 zip、GB 级、幂等跳过+断点续传。
    // 排在 zip 之前:失败时无 staging 残留需清理;已就位的权重是内容寻址的无害产物,
    // 重试安装天然复用(fetch 幂等跳过),不做回滚。直接落 models 目录(D1 Level A:
    // SessionInit 传路径即可用,不再过安装器)。
    if !entry.model_blobs.is_empty() {
        let models = crate::ai::runtime_config::models_dir(&state);
        std::fs::create_dir_all(&models).ok();
        for blob in &entry.model_blobs {
            let dest = models.join(&blob.file_name);
            crate::exotic::fetch::fetch_model_blob(&blob.url, &dest, blob.size, &blob.sha256)
                .await
                .map_err(|e| AppError::Exotic {
                    code: e.code(),
                    message: format!("模型权重下载失败({}):{}", blob.file_name, e.code()),
                })?;
        }
    }

    // 2. 下载包到 staging 并对照条目 size/sha256 严格校验（exotic 专用 fetch；R10 统一后置）。
    let staging = state.exotic_staging_dir();
    std::fs::create_dir_all(&staging).ok();
    let zip = staging.join(format!("{plugin_id}.zip"));
    crate::exotic::fetch::fetch_package(
        &entry.package_url,
        &zip,
        entry.package_size,
        &entry.package_sha256,
    )
    .await
    .map_err(|e| AppError::Exotic {
        code: e.code(),
        message: format!("下载失败：{}", e.code()),
    })?;

    // 3. quiesce → 安全安装 → resume。quiesce 超时（Worker 仍占句柄）即中止，**不**强行切目录。
    let (prev_paused, quiesced) = state.quiesce_exotic(QUIESCE_TIMEOUT).await;
    if !quiesced {
        state.resume_after_quiesce(prev_paused).await;
        let _ = std::fs::remove_file(&zip);
        return Err(AppError::Exotic {
            code: "worker_quiesce_timeout",
            message: "Worker 未在限期内停止，安装中止（请重试）".into(),
        });
    }
    // R1-3：解包/逐文件 hash/原子 rename + upsert 短锁 db_writer 全是重阻塞，下沉 blocking
    // （quiesce/resume 留在 async 侧）；InstallContext/RegistryExpect 持引用，故在闭包内重建。
    // 注意 join 失败也必须走 resume + 清 zip，故先接住 join 结果再统一收尾。
    let join_result = {
        let state_arc = state.inner().clone();
        let zip_c = zip.clone();
        let plugin_id_c = plugin_id.clone();
        tokio::task::spawn_blocking(move || {
            let snap = state_arc.exotic_catalog.snapshot();
            let ctx = InstallContext {
                install_root: &install_root,
                staging_root: &staging,
                keyset: &keyset,
                catalog: &snap,
                host_version: HOST_VERSION,
            };
            let expect = RegistryExpect {
                plugin_id: &plugin_id_c,
                version: &entry.version,
                target,
                package_sequence: entry.package_sequence,
            };
            // install_staged_zip 仅在 upsert 时短锁 db_writer（不在解包/hash/rename 期间持锁）。
            installer::install_staged_zip(&ctx, &zip_c, &expect, now, &state_arc.db_writer)
        })
        .await
    };
    state.resume_after_quiesce(prev_paused).await;
    let _ = std::fs::remove_file(&zip); // 无论成败清理已用 zip
    let result =
        join_result.map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    // R1-4：透传 InstallError 的稳定码（open_zip/bad_signature/install_io/…），
    // 前端可按码区分 zip 损坏/签名失败/磁盘满，不再折叠为泛码 install_failed。
    result.map_err(|e| AppError::Exotic {
        code: e.code(),
        message: format!("安装失败：{e}"),
    })?;
    state.wake_exotic(WakeReason::PluginInstalled);
    Ok(())
}

/// 修复（§6.5）：重新验签已装 manifest + 逐文件 hash 复核。完好→确保 installed；损坏→置 broken。
#[tauri::command]
pub async fn repair_exotic_plugin(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let keyset = builtin_keyset()?;
    let install_root = state.exotic_install_dir();
    // 显式校验 plugin_id（防任意字符串写入 DB install_state，安全评审 medium）。
    plugin_install_dir(&install_root, &plugin_id).ok_or_else(|| AppError::Exotic {
        code: "invalid_plugin_id",
        message: "非法 plugin_id".into(),
    })?;
    // R1-3：逐文件 hash 复核（重 IO+CPU）+ db_writer 短锁整段下沉 blocking。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        match installer::verify_installed_integrity(&install_root, &plugin_id, &keyset, now_secs())
        {
            Ok(()) => {
                let conn = state_arc
                    .db_writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let _ = crate::db::queries::set_exotic_plugin_state(
                    &conn,
                    &plugin_id,
                    crate::exotic::install_state::INSTALLED,
                );
                Ok(())
            }
            Err(e) => {
                {
                    let conn = state_arc
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let _ = crate::db::queries::set_exotic_plugin_state(
                        &conn,
                        &plugin_id,
                        crate::exotic::install_state::BROKEN,
                    );
                }
                state_arc.wake_exotic(WakeReason::ConfigChanged); // 置 broken → 不再领取
                Err(AppError::Exotic {
                    code: "repair_corrupt",
                    message: format!("安装损坏（已标记 broken，请重装）：{e}"),
                })
            }
        }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 回滚到本机已验证 backup（§6.5）：quiesce → 目录换回 → 据已装 manifest 重建 DB 记录。
#[tauri::command]
pub async fn rollback_exotic_plugin(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let _guard = state.exotic_install_lock.lock().await; // 与 install/uninstall 串行
    let keyset = builtin_keyset()?;
    let install_root = state.exotic_install_dir();
    let current =
        plugin_install_dir(&install_root, &plugin_id).ok_or_else(|| AppError::Exotic {
            code: "invalid_plugin_id",
            message: "非法 plugin_id".into(),
        })?;
    let backup = install_root.join(format!("{plugin_id}.backup"));

    // quiesce 超时即中止，不强行换目录（避免句柄占用致 current 丢失）。
    let (prev_paused, quiesced) = state.quiesce_exotic(QUIESCE_TIMEOUT).await;
    if !quiesced {
        state.resume_after_quiesce(prev_paused).await;
        return Err(AppError::Exotic {
            code: "worker_quiesce_timeout",
            message: "Worker 未在限期内停止，回滚中止（请重试）".into(),
        });
    }

    // R1-3：目录换回（fs rename/删除）+ 重验签（逐文件 hash）+ DB 记录重建整段下沉 blocking；
    // resume 统一放在 join 之后（含 join 失败路径），保持「单次 resume」不变式。
    let join_result = {
        let state_arc = state.inner().clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            // 目录换回旧版本。失败即报错（resume 在外层统一执行）。
            rollback_to_backup(&current, &backup).map_err(|e| AppError::Exotic {
                code: "rollback_failed",
                message: format!("回滚失败：{e}"),
            })?;
            // 重新验签并重建 DB 记录（防回滚到被篡改 backup）。失败 → 置 broken（磁盘不可信），状态一致。
            match installer::record_from_installed(&install_root, &plugin_id, &keyset, now_secs()) {
                Ok(rec) => {
                    let conn = state_arc
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    crate::db::queries::upsert_exotic_plugin(&conn, &rec).map_err(|e| {
                        AppError::Exotic {
                            code: "rollback_record_failed",
                            message: format!(
                                "回滚后 DB 记录更新失败（磁盘已回滚，状态不一致）：{e}"
                            ),
                        }
                    })
                }
                Err(e) => {
                    let conn = state_arc
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let _ = crate::db::queries::set_exotic_plugin_state(
                        &conn,
                        &plugin_id,
                        crate::exotic::install_state::BROKEN,
                    );
                    Err(AppError::Exotic {
                        code: "rollback_record_failed",
                        message: format!(
                            "回滚后 manifest 校验失败（backup 可能被篡改，已标记 broken）：{e}"
                        ),
                    })
                }
            }
        })
        .await
    };
    state.resume_after_quiesce(prev_paused).await; // 单次 resume
    join_result.map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    state.wake_exotic(WakeReason::PluginInstalled);
    Ok(())
}

/// 卸载（§6.5）：quiesce → 移走安装目录 + 删 DB 记录（不删媒体/任务）。
/// 只卸载插件；官方版授权由统一入口管理。
#[tauri::command]
pub async fn uninstall_exotic_plugin(
    plugin_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let _guard = state.exotic_install_lock.lock().await; // 与 install/rollback 串行
    let install_root = state.exotic_install_dir();
    // 显式校验 plugin_id（纵深防御）。
    plugin_install_dir(&install_root, &plugin_id).ok_or_else(|| AppError::Exotic {
        code: "invalid_plugin_id",
        message: "非法 plugin_id".into(),
    })?;
    // quiesce 超时即中止（避免句柄占用致 remove_dir_all 失败留半删目录）。
    let (prev_paused, quiesced) = state.quiesce_exotic(QUIESCE_TIMEOUT).await;
    if !quiesced {
        state.resume_after_quiesce(prev_paused).await;
        return Err(AppError::Exotic {
            code: "worker_quiesce_timeout",
            message: "Worker 未在限期内停止，卸载中止（请重试）".into(),
        });
    }
    // R1-3：安装目录 remove_dir_all + 删 DB 记录整段下沉 blocking；resume 在 join 后统一执行。
    let join_result = {
        let state_arc = state.inner().clone();
        let plugin_id_c = plugin_id.clone();
        tokio::task::spawn_blocking(move || {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            installer::uninstall_plugin(&install_root, &plugin_id_c, &conn)
        })
        .await
    };
    state.resume_after_quiesce(prev_paused).await;
    join_result
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
        .map_err(|e| AppError::Exotic {
            code: "uninstall_failed",
            message: format!("卸载失败：{e}"),
        })?;

    state.wake_exotic(WakeReason::ConfigChanged);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认 Registry 基址必须 HTTPS：否则 `download_to_vec` 运行期即拒（NotHttps），
    /// 但更重要的是防「手滑把常量改成 http:// 造成静默降级」——编译期/CI 即锁死。
    #[test]
    fn default_registry_base_is_https() {
        assert!(
            DEFAULT_REGISTRY_BASE_URL.starts_with("https://"),
            "Registry 基址绝不可为非 HTTPS（安全红线）"
        );
    }
}
