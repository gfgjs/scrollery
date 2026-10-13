// src-tauri/src/exotic/installer.rs
//! 冷门格式插件的安装、卸载与修复编排。
//!
//! 把安全原语（[`crate::exotic::install`] 的 verify+extract / commit / rollback）按 §6.4 严格次序
//! 串成完整安装流程：**verify+extract → Catalog 子集校验 → 原子切换 → DB 安装记录**。
//! 任一步失败回滚 backup 且不写 DB。Catalog 校验在 commit **之前**——坏插件永不落地为 current。
//!
//! 调用方（P6.4 命令层）前置：下载并 size/sha256 校验 zip 到 staging（暂留桩待 R10/P6.2）、
//! Coordinator quiesce 该 plugin + kill/wait Worker + 释放句柄（否则 Windows 占用目录无法改名）。
//! 本模块为纯编排（仅文件 + DB），可离线测试。

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Deserialize;

use crate::db::queries as q;
use crate::exotic::catalog::{Capability, CatalogSnapshot};
use crate::exotic::crypto::VerifyingKeyset;
use crate::exotic::install::{
    commit_install, discard_backup, plugin_install_dir, rollback_to_backup, verify_and_extract,
    InstallError, InstallLimits, RegistryExpect,
};
use crate::exotic::package::{verify_manifest, PackageManifest};
use crate::exotic::{install_state, InstalledPluginRecord};

/// 两份签名元数据 + 插件自身 manifest 文件名。
const PACKAGE_MANIFEST: &str = "package-manifest.json";
const PACKAGE_MANIFEST_SIG: &str = "package-manifest.sig";
const PLUGIN_MANIFEST: &str = "plugin-manifest.json";

/// 插件自身 manifest（§6.3 payload 之一）：声明该插件实现的 formats/capabilities。
/// 安装时须确认其为 Catalog 子集（§6.4 第 8 步）——插件不得声明 Catalog 未授权的格式/能力。
#[derive(Debug, Deserialize)]
struct PluginManifest {
    plugin_id: String,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

/// 安装上下文（运行期注入；测试用临时目录 + 内置 Catalog）。
pub struct InstallContext<'a> {
    /// 插件安装根（各插件装到 `<root>/<plugin_id>`）。
    pub install_root: &'a Path,
    /// 解包暂存根（commit 前先 extract 到 `<root>/<plugin_id>.staging`）。
    pub staging_root: &'a Path,
    pub keyset: &'a VerifyingKeyset,
    pub catalog: &'a CatalogSnapshot,
    pub host_version: &'a str,
}

/// backup 目录路径（与 current 同级，`<plugin_id>.backup`）。plugin_id 已由调用链校验。
fn backup_dir(install_root: &Path, plugin_id: &str) -> PathBuf {
    install_root.join(format!("{plugin_id}.backup"))
}

/// staging 解包目录（`<staging_root>/<plugin_id>.staging`）。
fn staging_dir(staging_root: &Path, plugin_id: &str) -> PathBuf {
    staging_root.join(format!("{plugin_id}.staging"))
}

/// 从已落地 staging 的 zip 安全安装（§6.4）。返回安装真相记录。
///
/// `expect` 来自**已验签 Registry** 条目（plugin_id/version/target/package_sequence），绝不来自前端。
pub fn install_staged_zip(
    ctx: &InstallContext<'_>,
    zip_path: &Path,
    expect: &RegistryExpect<'_>,
    now: i64,
    writer: &std::sync::Mutex<Connection>,
) -> Result<InstalledPluginRecord, InstallError> {
    let plugin_id = expect.plugin_id;
    let extract = staging_dir(ctx.staging_root, plugin_id);
    let _ = std::fs::remove_dir_all(&extract); // 清上次残留

    // 1. 验签先于解包 + 白名单 + zip 加固 + 逐文件 hash 复核（§6.4 第 3-7 步）。
    let extracted = verify_and_extract(
        zip_path,
        ctx.keyset,
        expect,
        ctx.host_version,
        now,
        &extract,
        &InstallLimits::default(),
    )?;

    // 2. Catalog 子集校验（**commit 前**，§6.4 第 8 步）。失败即清 staging。
    if let Err(e) = check_catalog_subset(&extract, plugin_id, ctx.catalog) {
        let _ = std::fs::remove_dir_all(&extract);
        return Err(e);
    }

    // 3. manifest_hash = sha256(package-manifest.json)（安装真相完整性锚，供后续 Supervisor 复核）。
    let manifest_hash = sha256_file(&extract.join(PACKAGE_MANIFEST))
        .map_err(|e| InstallError::Io(format!("manifest hash：{e}")))?;

    // 4. 原子切换（§6.4 第 10 步）：旧 current→backup，staging→current。
    let current = plugin_install_dir(ctx.install_root, plugin_id)
        .ok_or_else(|| InstallError::UnsafeEntry(plugin_id.to_string()))?;
    let backup = backup_dir(ctx.install_root, plugin_id);
    commit_install(&current, &extract, &backup)?;

    // 5. DB 安装记录（§6.4 第 10 步）。失败 → 回滚目录、不留半装（§6.4 第 11 步）。
    let rec = InstalledPluginRecord {
        plugin_id: plugin_id.to_string(),
        version: extracted.manifest.version.clone(),
        manifest_hash,
        package_sequence: extracted.manifest.package_sequence,
        install_state: install_state::INSTALLED.to_string(),
        installed_at: now,
        updated_at: now,
    };
    // 仅此一步需写 DB——短暂持锁，**不**在前面的解压/hash/rename 期间占用 db_writer（安全评审 medium，
    // 防止扫描入库等所有 DB 写路径被长时间饿死）。
    let db_result = {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        q::upsert_exotic_plugin(&conn, &rec)
    };
    if let Err(e) = db_result {
        // DB 失败 → 回滚目录。回滚也失败则为破损态，记录以便诊断（安全评审 medium）。
        if let Err(rb) = rollback_to_backup(&current, &backup) {
            tracing::error!("DB 写入失败且目录回滚也失败（安装处于破损态）：db={e} rollback={rb}");
        }
        return Err(InstallError::Db(e.to_string()));
    }

    // 6. 成功 → 丢弃 backup（产品策略可改为保留有限期）。
    let _ = discard_backup(&backup);
    Ok(rec)
}

/// 卸载（§6.5）：移走安装目录 + 删 DB 安装记录。**不**删媒体记录与历史任务。
/// 调用方前置 quiesce + kill/wait Worker。插件卸载不移除官方版套装授权。
pub fn uninstall_plugin(
    install_root: &Path,
    plugin_id: &str,
    conn: &Connection,
) -> Result<bool, InstallError> {
    let current = plugin_install_dir(install_root, plugin_id)
        .ok_or_else(|| InstallError::UnsafeEntry(plugin_id.to_string()))?;
    if current.exists() {
        std::fs::remove_dir_all(&current).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    let _ = discard_backup(&backup_dir(install_root, plugin_id));
    let _ = std::fs::remove_dir_all(staging_dir(install_root, plugin_id));
    q::delete_exotic_plugin(conn, plugin_id).map_err(|e| InstallError::Db(e.to_string()))
}

/// 已安装完整性复核（修复命令 §6.5 用）：重新验签已装 manifest + 逐文件 hash 比对。
/// 任一不符 → `HashMismatch`（命令层据此置 `broken` 并提示重装）。
pub fn verify_installed_integrity(
    install_root: &Path,
    plugin_id: &str,
    keyset: &VerifyingKeyset,
    now: i64,
) -> Result<(), InstallError> {
    let current = plugin_install_dir(install_root, plugin_id)
        .ok_or_else(|| InstallError::UnsafeEntry(plugin_id.to_string()))?;
    let mbytes = std::fs::read(current.join(PACKAGE_MANIFEST))
        .map_err(|_| InstallError::HashMismatch("缺 package-manifest.json".into()))?;
    let sbytes = std::fs::read(current.join(PACKAGE_MANIFEST_SIG))
        .map_err(|_| InstallError::HashMismatch("缺 package-manifest.sig".into()))?;
    // 重新验签：防安装后 manifest 被连同文件一起篡改。
    let manifest = verify_manifest(&mbytes, &sbytes, keyset, now)?;
    // 每次启动前复核协议版本，挡住安装后 host 升级、插件回滚等版本错配；
    // 早退可直接报告 protocol_mismatch，避免启动后才在握手期失败。
    if manifest.protocol_version != exotic_protocol::PROTOCOL_VERSION {
        return Err(InstallError::ProtocolMismatch {
            pkg: manifest.protocol_version,
            host: exotic_protocol::PROTOCOL_VERSION,
        });
    }
    for f in &manifest.files {
        let mut p = current.clone();
        for seg in f.path.split('/') {
            p.push(seg);
        }
        let got = sha256_file(&p).map_err(|_| InstallError::HashMismatch(f.path.clone()))?;
        if got != f.sha256 {
            return Err(InstallError::HashMismatch(f.path.clone()));
        }
    }
    Ok(())
}

/// 校验插件 manifest 的 formats/capabilities 为 Catalog 子集（§6.4 第 8 步）。
fn check_catalog_subset(
    extract: &Path,
    plugin_id: &str,
    catalog: &CatalogSnapshot,
) -> Result<(), InstallError> {
    let reject = |s: String| InstallError::CatalogReject(s);
    let bytes = std::fs::read(extract.join(PLUGIN_MANIFEST))
        .map_err(|_| reject("缺 plugin-manifest.json".into()))?;
    let pm: PluginManifest =
        serde_json::from_slice(&bytes).map_err(|e| reject(format!("plugin-manifest 解析：{e}")))?;
    if pm.plugin_id != plugin_id {
        return Err(reject(format!("plugin_id 不符：{}", pm.plugin_id)));
    }
    if pm.formats.is_empty() {
        return Err(reject("formats 为空".into()));
    }
    // 每个 format 必须由 Catalog 登记且归属本 plugin。
    for f in &pm.formats {
        match catalog.resolve_format(f) {
            Some(off) if off.plugin_id == plugin_id => {}
            _ => return Err(reject(format!("format {f} 非本插件的 Catalog 子集"))),
        }
    }
    // 每个 capability 必须在该 offering 声明范围内。
    let off = pm
        .formats
        .first()
        .and_then(|f| catalog.resolve_format(f))
        .ok_or_else(|| reject("无对应 offering".into()))?;
    for c in &pm.capabilities {
        let cap = match c.as_str() {
            "thumbnail" => Capability::Thumbnail,
            "metadata" => Capability::Metadata,
            "text" => Capability::Text,
            "embedding" => Capability::Embedding,
            "face_detect_embed" => Capability::FaceDetectEmbed,
            "enhance" => Capability::Enhance,
            other => return Err(reject(format!("未知 capability：{other}"))),
        };
        if !off.claims_capability(cap) {
            return Err(reject(format!("capability {c} 非 Catalog 子集")));
        }
    }
    Ok(())
}

/// 文件 sha256（小写 hex）。
fn sha256_file(path: &Path) -> Result<String, String> {
    crate::utils::hash::sha256_hex_of_file(path).map_err(|e| e.to_string())
}

/// 定位已装插件的 Worker 可执行文件：读已装 package-manifest，取 kind=="worker" 的文件路径。
/// 仅定位（不重新验签）；**必须**在 [`verify_installed_integrity`] 通过后调用——故收窄为 `pub(crate)`
/// 防止外部跳过验签直接信任 manifest 内路径（安全评审 low）。`resolve_worker_path` 已封装该次序。
pub(crate) fn installed_worker_path(install_root: &Path, plugin_id: &str) -> Option<PathBuf> {
    let current = plugin_install_dir(install_root, plugin_id)?;
    let bytes = std::fs::read(current.join(PACKAGE_MANIFEST)).ok()?;
    let manifest: PackageManifest = serde_json::from_slice(&bytes).ok()?;
    let worker = manifest.files.iter().find(|f| f.kind == "worker")?;
    let mut p = current;
    for seg in worker.path.split('/') {
        p.push(seg);
    }
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// builtin worker(随主程序打包的 sidecar,非插件商店安装)的二进制基名映射。
/// RAW 与 video-extended 均为 builtin distribution，经 Tauri externalBin 与主程序同目录
/// 分发(无安装包/无 DB 安装记录)。未登记的 plugin_id 返回 None，不走本路径。
fn builtin_worker_basename(plugin_id: &str) -> Option<&'static str> {
    if plugin_id == crate::exotic::coordinator::RAW_PLUGIN_ID {
        Some("raw-worker")
    } else if plugin_id == crate::exotic::coordinator::VIDEO_PLUGIN_ID {
        Some("video-worker")
    } else {
        None
    }
}

/// 由「主程序所在目录 + plugin_id」拼 builtin worker 候选路径(加平台可执行后缀:windows 为 .exe)。
/// why 抽成纯函数:dev/prod 选路都需要「exe 目录 + 二进制名 → 候选路径」的拼装，
/// 把它剥离出来即可单测(路径拼装/后缀/未知 id)。本函数不摸盘、逻辑不依赖 cfg；
/// 是否真存在由调用方 `is_file()` 判断。
fn builtin_worker_candidate(exe_dir: &Path, plugin_id: &str) -> Option<PathBuf> {
    let base = builtin_worker_basename(plugin_id)?;
    Some(exe_dir.join(format!("{base}{}", std::env::consts::EXE_SUFFIX)))
}

#[cfg(debug_assertions)]
fn debug_raw_worker_path(
    override_path: Option<PathBuf>,
    exe_dir: Option<&Path>,
) -> Option<PathBuf> {
    // GNU 产物同样会由 Tauri 复制到主程序旁；仅查该固定位置，不扫描其它目录。
    override_path.or_else(|| {
        exe_dir
            .and_then(|dir| {
                builtin_worker_candidate(dir, crate::exotic::coordinator::RAW_PLUGIN_ID)
            })
            .filter(|path| path.is_file())
    })
}

/// 解析运行期 Worker 路径并复核已安装文件。
///
/// `EXOTIC_PSD_WORKER_PATH` 旁路只在 `#[cfg(debug_assertions)]` 下编入，且不验签；
/// Release 不会通过该环境变量加载 worker。已安装插件须重新验签 manifest 并复核全部文件 hash；
/// 失败时不返回路径，Coordinator 不启动 worker。
pub fn resolve_worker_path(
    install_root: &Path,
    plugin_id: &str,
    keyset: &VerifyingKeyset,
    now: i64,
) -> Option<PathBuf> {
    // 🔒 dev/test 旁路仅在 debug 构建编入；Release 因 #[cfg(debug_assertions)] 整条剔除（编译期，非运行期判断）。
    // 按 plugin_id 分发到各自专属 env；RAW/video 未覆盖时与正式打包共用同目录 sidecar 路径。
    #[cfg(debug_assertions)]
    {
        if plugin_id == crate::exotic::coordinator::PSD_PLUGIN_ID {
            if let Some(p) = std::env::var_os("EXOTIC_PSD_WORKER_PATH") {
                return Some(PathBuf::from(p));
            }
        } else if plugin_id == crate::exotic::coordinator::RAW_PLUGIN_ID {
            // RAW 是 builtin distribution(catalog.rs distribution=="builtin"):无安装包、
            // 无 installed-plugin 验签记录——下方 verify_installed_integrity 对它必然落空。
            // 专属 env 优先；未覆盖时读取 Tauri externalBin 已复制到主程序旁的 GNU 产物。
            let exe = std::env::current_exe().ok();
            if let Some(p) = debug_raw_worker_path(
                std::env::var_os("EXOTIC_RAW_WORKER_PATH").map(PathBuf::from),
                exe.as_deref().and_then(Path::parent),
            ) {
                return Some(p);
            }
            // 无可用产物时明确失败，不落入 RAW 不具备的 installed-plugin 安装记录查询。
            tracing::warn!(
                "{plugin_id} 是 builtin worker(RAW gnu sidecar),dev 需设 EXOTIC_RAW_WORKER_PATH \
                 或将 raw-worker.exe 放在主程序同目录才能拉起"
            );
            return None;
        } else if plugin_id == crate::exotic::coordinator::VIDEO_PLUGIN_ID {
            // video-extended 是 builtin distribution(catalog.rs distribution=="builtin")：无安装包/
            // 无验签记录。显式 env 优先，便于自定义 worker；默认取 tauri dev 的同目录 debug 产物。
            if let Some(p) = std::env::var_os("EXOTIC_VIDEO_WORKER_PATH") {
                return Some(PathBuf::from(p));
            }
            let candidate = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
                .and_then(|dir| builtin_worker_candidate(&dir, plugin_id));
            if let Some(p) = candidate.filter(|p| p.is_file()) {
                return Some(p);
            }
            tracing::warn!(
                "{plugin_id} 是 builtin worker(video-worker),dev 需设 EXOTIC_VIDEO_WORKER_PATH \
                 或将 video-worker.exe 放在 target/debug 同目录才能拉起"
            );
            return None;
        }
    }
    // Release 构建：builtin worker 经 Tauri externalBin 与主程序同目录分发。
    // 镜像 ai_worker_exe 的「current_exe 同目录 + 二进制名 + 平台后缀」策略定位 sidecar。
    // 不落进下方 installed-plugin 验签流程(RAW/video 均无安装包/无 DB 安装记录)。
    #[cfg(not(debug_assertions))]
    if plugin_id == crate::exotic::coordinator::RAW_PLUGIN_ID
        || plugin_id == crate::exotic::coordinator::VIDEO_PLUGIN_ID
    {
        let candidate = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .and_then(|dir| builtin_worker_candidate(&dir, plugin_id));
        return match candidate {
            Some(p) if p.is_file() => Some(p),
            Some(p) => {
                tracing::warn!(
                    "{plugin_id} builtin sidecar 未在主程序同目录找到({}):externalBin 未随包分发?拒绝拉起",
                    p.display()
                );
                None
            }
            None => {
                // current_exe 失败或无 builtin 映射，兜底不 panic。
                tracing::warn!(
                    "{plugin_id} 无法定位 builtin sidecar 路径(current_exe/映射缺失),拒绝拉起"
                );
                None
            }
        };
    }
    // 启动前完整性复核(含 P0-3 协议版本比对):失败即不返回路径,并留稳定码日志
    // (protocol_mismatch=需重装匹配版本;hash_mismatch=被篡改/损坏)。
    if let Err(e) = verify_installed_integrity(install_root, plugin_id, keyset, now) {
        // 复核失败去重(2026-07-13):Coordinator 每 30s RetryDue 都会调本函数,protocol_mismatch
        // 这类「不重装不会自愈」的失败会把日志刷成每 30s 一条 WARN(实测一天 871 条,淹没真问题)。
        // 同一 (plugin, code) 仅首次 WARN,后续降 debug;code 变化(如修好后又坏)重新 WARN。
        // **只改日志级别**——验证结果与返回值(None)不变,安全路径语义零改动。
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static LAST_WARNED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        let code = e.code();
        let first_or_changed = {
            let mut map = LAST_WARNED
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            // insert 返回旧值:None(首次)或不同 code → 需要 WARN;相同 code → 抑制为 debug。
            map.insert(plugin_id.to_string(), code.to_string())
                .as_deref()
                != Some(code)
        };
        if first_or_changed {
            tracing::warn!(
                "{plugin_id} 启动前复核未过({code}):拒绝拉起 worker(同码后续降 debug,不再每 30s 刷屏)"
            );
        } else {
            tracing::debug!("{plugin_id} 启动前复核未过({code}):拒绝拉起 worker(重复,已抑制 WARN)");
        }
        return None;
    }
    installed_worker_path(install_root, plugin_id)
}

/// 由已装目录重建安装真相记录（回滚后用：dir 已换回旧版本，DB 记录需同步）。
/// 重新验签已装 manifest（防回滚到被篡改的 backup），据其 version/sequence 重建记录。
pub fn record_from_installed(
    install_root: &Path,
    plugin_id: &str,
    keyset: &VerifyingKeyset,
    now: i64,
) -> Result<InstalledPluginRecord, InstallError> {
    let current = plugin_install_dir(install_root, plugin_id)
        .ok_or_else(|| InstallError::UnsafeEntry(plugin_id.to_string()))?;
    let mbytes = std::fs::read(current.join(PACKAGE_MANIFEST))
        .map_err(|_| InstallError::HashMismatch("缺 package-manifest.json".into()))?;
    let sbytes = std::fs::read(current.join(PACKAGE_MANIFEST_SIG))
        .map_err(|_| InstallError::HashMismatch("缺 package-manifest.sig".into()))?;
    let manifest = verify_manifest(&mbytes, &sbytes, keyset, now)?;
    let manifest_hash = sha256_bytes(&mbytes);
    Ok(InstalledPluginRecord {
        plugin_id: plugin_id.to_string(),
        version: manifest.version,
        manifest_hash,
        package_sequence: manifest.package_sequence,
        install_state: install_state::INSTALLED.to_string(),
        installed_at: now,
        updated_at: now,
    })
}

/// 字节 sha256（小写 hex）。
fn sha256_bytes(b: &[u8]) -> String {
    crate::utils::hash::sha256_hex(b)
}
