// src-tauri/src/exotic/installer.rs
//! 冷门格式插件 · 安装编排（v3 Part3 §6.4 第 8/10/11 步 + 卸载/修复）。
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
    // 🔴 P0-3(Part6 §3.2.1):协议版本前置比对——安装期检查挡不住「装时合法、之后 host
    // 升级」的时序(离线升主程序/插件更新失败/回滚),而帧层硬等值校验会让旧协议 worker
    // 在已付 spawn 代价的握手期才被拒且报泛错。此处早退:错误码 protocol_mismatch 即
    // needs_reinstall 语义(前端据此提示重装匹配版本;商店自动重下载随 Part8)。
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

/// 解析运行期 Worker 路径（Part3 §3.6 启动顺序第一步：「验证安装记录与当前文件 hash」）。
///
/// dev/test 环境变量 `EXOTIC_PSD_WORKER_PATH` 优先且**不验签**（保留 Part2 开发入口与 e2e 测试）。
/// 🔒 **仅 debug 构建生效**：整条旁路经 `#[cfg(debug_assertions)]` 门控，Release 构建
/// （`debug_assertions=false`）下该分支**整体不编入二进制**——杜绝经环境变量加载未验签 worker 的
/// 信任链击穿（SEC-02，对齐 D8「Release 不得有验签/授权旁路」红线，与 `exotic-dev-fixtures` 同姿态）。
/// 否则对已装插件**先完整性复核**（重新验签 manifest + 全文件 hash），通过才返回 worker 路径——
/// 篡改的已装 worker 复核失败 → None → Coordinator 视为不可用、不拉起进程。
pub fn resolve_worker_path(
    install_root: &Path,
    plugin_id: &str,
    keyset: &VerifyingKeyset,
    now: i64,
) -> Option<PathBuf> {
    // 🔒 dev/test 旁路仅在 debug 构建编入；Release 因 #[cfg(debug_assertions)] 整条剔除（编译期，非运行期判断）。
    // 按 plugin_id 分发到各自专属 env——PSD/RAW 现有行为不变，video-extended 保留 env 覆盖，
    // 并在 debug 下自动尝试主程序 target/debug 同目录的 video-worker.exe。
    #[cfg(debug_assertions)]
    {
        if plugin_id == crate::exotic::coordinator::PSD_PLUGIN_ID {
            if let Some(p) = std::env::var_os("EXOTIC_PSD_WORKER_PATH") {
                return Some(PathBuf::from(p));
            }
        } else if plugin_id == crate::exotic::coordinator::RAW_PLUGIN_ID {
            // RAW 是 builtin distribution(catalog.rs distribution=="builtin"):无安装包、
            // 无 installed-plugin 验签记录——下方 verify_installed_integrity 对它必然落空。
            // dev 用专属 env 指向 gnu-only sidecar(独立 target,与 msvc 默认 target/debug 分离)。
            if let Some(p) = std::env::var_os("EXOTIC_RAW_WORKER_PATH") {
                return Some(PathBuf::from(p));
            }
            // dev 未设 env:不误落进下方 installed-plugin 流程(RAW 无安装记录必然 None,
            // 但语义不清)——明确拒绝并留可诊断日志,不 panic、不误用其他 worker 二进制。
            tracing::warn!(
                "{plugin_id} 是 builtin worker(RAW gnu sidecar),dev 需设 EXOTIC_RAW_WORKER_PATH \
                 指向 raw-worker.exe 才能拉起;prod 打包留待 H2b-prod(tauri externalBin 跨工具链 sidecar)"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::catalog::CatalogStore;
    use crate::exotic::crypto::test_support::{keyset_json, sign, signing_key, KeySpec};
    use std::io::{Cursor, Write as _};

    const NOW: i64 = 1_790_000_000;
    const TARGET: &str = "x86_64-pc-windows-msvc";
    const PID: &str = "exotic-image-psd";

    fn release_keyset(sk: &ring::signature::Ed25519KeyPair) -> VerifyingKeyset {
        let json = keyset_json(&[KeySpec {
            key_id: "release-test",
            purpose: "release",
            sk,
            status: "active",
            not_before: 0,
            not_after: None,
        }]);
        VerifyingKeyset::parse(&json).unwrap()
    }

    fn sha_hex(b: &[u8]) -> String {
        crate::utils::hash::sha256_hex(b)
    }

    /// 构造合法包：plugin-manifest.json(psd/thumbnail) + 一个 worker 文件。返回 zip 路径。
    /// `version`/`seq` 可变以测升级。`plugin_fmt` 可注入非法格式测 catalog reject。
    fn build_zip(
        tag: &str,
        sk: &ring::signature::Ed25519KeyPair,
        version: &str,
        seq: i64,
        plugin_fmt: &str,
        worker_bytes: &[u8],
    ) -> PathBuf {
        let plugin_manifest = format!(
            r#"{{"plugin_id":"{PID}","formats":["{plugin_fmt}"],"capabilities":["thumbnail"]}}"#
        );
        let payload: Vec<(&str, Vec<u8>, bool)> = vec![
            ("bin/psd-worker.exe", worker_bytes.to_vec(), true),
            (PLUGIN_MANIFEST, plugin_manifest.into_bytes(), false),
        ];
        let files_json: Vec<String> = payload
            .iter()
            .map(|(p, c, exe)| {
                // bin/ 下的可执行标 kind=worker，使 installed_worker_path 可定位。
                let kind = if p.starts_with("bin/") { "worker" } else { "file" };
                format!(
                    r#"{{"path":"{p}","size":{},"sha256":"{}","kind":"{kind}","executable":{exe}}}"#,
                    c.len(),
                    sha_hex(c)
                )
            })
            .collect();
        let manifest = format!(
            r#"{{"schema":1,"key_id":"release-test","plugin_id":"{PID}","version":"{version}",
              "package_sequence":{seq},"target":"{TARGET}","min_host_version":"0.1.0",
              "protocol_version":{proto},"compliance_review_id":"r-1","files":[{}]}}"#,
            files_json.join(","),
            proto = exotic_protocol::PROTOCOL_VERSION
        );
        let sig = sign(sk, manifest.as_bytes());

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(PACKAGE_MANIFEST, opt).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.start_file(PACKAGE_MANIFEST_SIG, opt).unwrap();
            zip.write_all(&sig).unwrap();
            for (p, c, _) in &payload {
                zip.start_file(*p, opt).unwrap();
                zip.write_all(c).unwrap();
            }
            zip.finish().unwrap();
        }
        let path = std::env::temp_dir().join(format!("exotic-er-{tag}-{}.zip", std::process::id()));
        std::fs::write(&path, &buf).unwrap();
        path
    }

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c
    }

    fn dirs(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("exotic-er-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        (base.join("install"), base.join("staging"))
    }

    fn expect_v(version: &'static str, seq: i64) -> RegistryExpect<'static> {
        RegistryExpect {
            plugin_id: PID,
            version,
            target: TARGET,
            package_sequence: seq,
        }
    }

    /// 手工铺一个「已装」目录:manifest 以指定 protocol_version 合法签名——模拟
    /// 「装时协议匹配、之后 host 升版」的时序(安装期检查挡不住,P0-3 场景)。
    fn lay_installed(tag: &str, sk: &ring::signature::Ed25519KeyPair, proto: u16) -> PathBuf {
        let (install_root, _) = dirs(tag);
        let dir = plugin_install_dir(&install_root, PID).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let worker = b"WORKER".to_vec();
        let pm =
            format!(r#"{{"plugin_id":"{PID}","formats":["psd"],"capabilities":["thumbnail"]}}"#);
        std::fs::write(dir.join("bin/psd-worker.exe"), &worker).unwrap();
        std::fs::write(dir.join(PLUGIN_MANIFEST), pm.as_bytes()).unwrap();
        let files_json = format!(
            r#"{{"path":"bin/psd-worker.exe","size":{},"sha256":"{}","kind":"worker","executable":true}},
               {{"path":"{PLUGIN_MANIFEST}","size":{},"sha256":"{}","kind":"manifest"}}"#,
            worker.len(),
            sha_hex(&worker),
            pm.len(),
            sha_hex(pm.as_bytes()),
        );
        let manifest = format!(
            r#"{{"schema":1,"key_id":"release-test","plugin_id":"{PID}","version":"1.0.0",
              "package_sequence":1,"target":"{TARGET}","min_host_version":"0.1.0",
              "protocol_version":{proto},"compliance_review_id":"r-1","files":[{files_json}]}}"#
        );
        let sig = sign(sk, manifest.as_bytes());
        std::fs::write(dir.join(PACKAGE_MANIFEST), manifest.as_bytes()).unwrap();
        std::fs::write(dir.join(PACKAGE_MANIFEST_SIG), &sig).unwrap();
        install_root
    }

    #[test]
    fn stale_protocol_installed_worker_refused_before_spawn() {
        // P0-3:旧协议已装插件在 resolve(spawn 前)即被拒,错误码 protocol_mismatch。
        let sk = signing_key(7);
        let ks = release_keyset(&sk);
        let old = exotic_protocol::PROTOCOL_VERSION - 1;
        let root = lay_installed("staleproto", &sk, old);
        let err = verify_installed_integrity(&root, PID, &ks, NOW).unwrap_err();
        assert_eq!(err.code(), "protocol_mismatch");
        assert!(resolve_worker_path(&root, PID, &ks, NOW).is_none());

        // 对照:当前协议版本同款铺设 → 通过并可解析 worker 路径。
        let root2 = lay_installed("curproto", &sk, exotic_protocol::PROTOCOL_VERSION);
        assert!(verify_installed_integrity(&root2, PID, &ks, NOW).is_ok());
        assert!(resolve_worker_path(&root2, PID, &ks, NOW).is_some());
    }

    /// H2b(dev):RAW 是 builtin distribution(无安装包/无验签记录),资源路径走专属
    /// `EXOTIC_RAW_WORKER_PATH`(镜像 PSD 的 `EXOTIC_PSD_WORKER_PATH` 旁路语义,不验签);
    /// 未设该 env 时必须明确拒绝(None + 日志),不得误落进下方 installed-plugin 验签流程
    /// (RAW 无 DB 安装记录,那条路径对它从设计上不适用)、也不得 panic。
    #[test]
    fn raw_builtin_worker_path_dev_env_override_and_absence() {
        let sk = signing_key(11);
        let ks = release_keyset(&sk);
        let root = std::env::temp_dir(); // builtin 分支不摸盘,任意存在目录即可
        const RAW_PID: &str = crate::exotic::coordinator::RAW_PLUGIN_ID;

        // 未设 env → 明确拒绝(H2b-prod 打包待办),不是 panic、也不是误用 PSD 二进制。
        std::env::remove_var("EXOTIC_RAW_WORKER_PATH");
        assert!(
            resolve_worker_path(&root, RAW_PID, &ks, NOW).is_none(),
            "builtin RAW 未设专属 env 时必须拒绝拉起(prod 打包留待 H2b-prod)"
        );

        // 设了 env → 不验签直接返回该路径(dev 旁路,镜像 PSD 现有行为)。
        let fake_worker = root.join("raw-worker-test-fixture.exe");
        std::env::set_var("EXOTIC_RAW_WORKER_PATH", &fake_worker);
        let resolved = resolve_worker_path(&root, RAW_PID, &ks, NOW);
        std::env::remove_var("EXOTIC_RAW_WORKER_PATH");
        assert_eq!(resolved, Some(fake_worker));
    }

    /// builtin worker 的 prod 选路依赖「exe 目录 + 二进制名 → 候选路径」的纯拼装：RAW →
    /// raw-worker、video-extended → video-worker；未登记 plugin_id(含 PSD——它走安装包/验签流程)
    /// 返回 None。
    #[test]
    fn builtin_worker_candidate_maps_builtin_workers_and_rejects_unknown() {
        use crate::exotic::coordinator::{PSD_PLUGIN_ID, RAW_PLUGIN_ID, VIDEO_PLUGIN_ID};
        let dir = Path::new("some").join("exe-dir");
        // RAW → <dir>/raw-worker(+平台后缀)。
        let p = builtin_worker_candidate(&dir, RAW_PLUGIN_ID).expect("RAW 应有 builtin 映射");
        assert_eq!(p.parent().unwrap(), dir.as_path());
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            format!("raw-worker{}", std::env::consts::EXE_SUFFIX)
        );
        // windows 平台后缀断言。
        #[cfg(windows)]
        assert!(p.to_string_lossy().ends_with("raw-worker.exe"));
        // video-extended → <dir>/video-worker(+平台后缀)。
        let p = builtin_worker_candidate(&dir, VIDEO_PLUGIN_ID)
            .expect("video-extended 应有 builtin 映射");
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            format!("video-worker{}", std::env::consts::EXE_SUFFIX)
        );
        // 未登记 plugin_id → None(非 builtin,不走本路径)。
        assert!(builtin_worker_candidate(&dir, "exotic-nope").is_none());
        assert!(
            builtin_worker_candidate(&dir, PSD_PLUGIN_ID).is_none(),
            "PSD 走安装包/验签流程,非 builtin,不应有 sidecar 映射"
        );
    }

    #[test]
    fn install_then_upgrade_then_uninstall() {
        let sk = signing_key(1);
        let ks = release_keyset(&sk);
        let catalog = CatalogStore::from_builtin().unwrap();
        let snap = catalog.snapshot();
        let (install_root, staging_root) = dirs("flow");
        let writer = std::sync::Mutex::new(mem_db());
        let ctx = InstallContext {
            install_root: &install_root,
            staging_root: &staging_root,
            keyset: &ks,
            catalog: &snap,
            host_version: "0.1.0",
        };

        // 首装 v1.0.0 seq=3。
        let z1 = build_zip("v1", &sk, "1.0.0", 3, "psd", b"WORKER-V1");
        let rec = install_staged_zip(&ctx, &z1, &expect_v("1.0.0", 3), NOW, &writer).unwrap();
        assert_eq!(rec.version, "1.0.0");
        assert_eq!(rec.install_state, "installed");
        let current = install_root.join(PID);
        assert_eq!(
            std::fs::read(current.join("bin/psd-worker.exe")).unwrap(),
            b"WORKER-V1"
        );
        // DB 记录在。
        assert_eq!(
            q::get_exotic_plugin(&writer.lock().unwrap(), PID)
                .unwrap()
                .unwrap()
                .package_sequence,
            3
        );

        // 完整性复核通过。
        verify_installed_integrity(&install_root, PID, &ks, NOW).unwrap();

        // 升级 v1.1.0 seq=4：内容替换、backup 已丢弃。
        let z2 = build_zip("v2", &sk, "1.1.0", 4, "psd", b"WORKER-V2-LONGER");
        let rec2 = install_staged_zip(&ctx, &z2, &expect_v("1.1.0", 4), NOW + 10, &writer).unwrap();
        assert_eq!(rec2.version, "1.1.0");
        assert_eq!(
            std::fs::read(current.join("bin/psd-worker.exe")).unwrap(),
            b"WORKER-V2-LONGER"
        );
        assert!(
            !install_root.join(format!("{PID}.backup")).exists(),
            "成功后 backup 应已丢弃"
        );
        // installed_at 保留首装值，package_sequence 升到 4。
        let got = q::get_exotic_plugin(&writer.lock().unwrap(), PID)
            .unwrap()
            .unwrap();
        assert_eq!(got.package_sequence, 4);
        assert_eq!(got.installed_at, NOW, "升级保留首装 installed_at");

        // 卸载：目录移除 + DB 记录删除。
        assert!(uninstall_plugin(&install_root, PID, &writer.lock().unwrap()).unwrap());
        assert!(!current.exists());
        assert!(q::get_exotic_plugin(&writer.lock().unwrap(), PID)
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(&install_root);
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    #[test]
    fn worker_path_and_record_rebuild() {
        let sk = signing_key(4);
        let ks = release_keyset(&sk);
        let catalog = CatalogStore::from_builtin().unwrap();
        let snap = catalog.snapshot();
        let (install_root, staging_root) = dirs("wp");
        let writer = std::sync::Mutex::new(mem_db());
        let ctx = InstallContext {
            install_root: &install_root,
            staging_root: &staging_root,
            keyset: &ks,
            catalog: &snap,
            host_version: "0.1.0",
        };
        let z = build_zip("wp", &sk, "2.0.0", 7, "psd", b"WORKER-BIN");
        install_staged_zip(&ctx, &z, &expect_v("2.0.0", 7), NOW, &writer).unwrap();

        // worker 定位：取 kind=worker 文件的绝对路径，存在。
        let wp = installed_worker_path(&install_root, PID).unwrap();
        assert!(wp.ends_with("bin/psd-worker.exe") || wp.ends_with("bin\\psd-worker.exe"));
        assert!(wp.exists());

        // record_from_installed 重建：版本/序号与已装一致。
        let rec = record_from_installed(&install_root, PID, &ks, NOW).unwrap();
        assert_eq!(rec.version, "2.0.0");
        assert_eq!(rec.package_sequence, 7);
        assert_eq!(rec.install_state, "installed");

        // 未装插件 → None。
        assert!(installed_worker_path(&install_root, "exotic-nope").is_none());

        // resolve_worker_path：完好 → Some；篡改 worker → 完整性复核失败 → None。
        // （EXOTIC_PSD_WORKER_PATH 注入的 e2e 环境会短路 env 分支，故该环境下跳过此断言。）
        if std::env::var_os("EXOTIC_PSD_WORKER_PATH").is_none() {
            assert!(resolve_worker_path(&install_root, PID, &ks, NOW).is_some());
            std::fs::write(wp, b"TAMPERED-WORKER").unwrap();
            assert!(
                resolve_worker_path(&install_root, PID, &ks, NOW).is_none(),
                "篡改已装 worker 后不应返回路径"
            );
        }

        let _ = std::fs::remove_dir_all(&install_root);
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    #[test]
    fn catalog_reject_blocks_install() {
        // 插件 manifest 声明非本插件的格式（jpg）→ commit 前拒，current 不落地。
        let sk = signing_key(2);
        let ks = release_keyset(&sk);
        let catalog = CatalogStore::from_builtin().unwrap();
        let snap = catalog.snapshot();
        let (install_root, staging_root) = dirs("creject");
        let writer = std::sync::Mutex::new(mem_db());
        let ctx = InstallContext {
            install_root: &install_root,
            staging_root: &staging_root,
            keyset: &ks,
            catalog: &snap,
            host_version: "0.1.0",
        };
        let z = build_zip("creject", &sk, "1.0.0", 3, "jpg", b"W");
        let r = install_staged_zip(&ctx, &z, &expect_v("1.0.0", 3), NOW, &writer);
        assert!(
            matches!(r, Err(InstallError::CatalogReject(_))),
            "got {r:?}"
        );
        assert!(!install_root.join(PID).exists(), "拒绝后 current 不应存在");
        assert!(
            q::get_exotic_plugin(&writer.lock().unwrap(), PID)
                .unwrap()
                .is_none(),
            "拒绝后无 DB 记录"
        );
        let _ = std::fs::remove_dir_all(&install_root);
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    #[test]
    fn integrity_detects_tampered_file() {
        let sk = signing_key(3);
        let ks = release_keyset(&sk);
        let catalog = CatalogStore::from_builtin().unwrap();
        let snap = catalog.snapshot();
        let (install_root, staging_root) = dirs("tamper");
        let writer = std::sync::Mutex::new(mem_db());
        let ctx = InstallContext {
            install_root: &install_root,
            staging_root: &staging_root,
            keyset: &ks,
            catalog: &snap,
            host_version: "0.1.0",
        };
        let z = build_zip("tamper", &sk, "1.0.0", 3, "psd", b"GOOD-WORKER");
        install_staged_zip(&ctx, &z, &expect_v("1.0.0", 3), NOW, &writer).unwrap();
        // 篡改已装文件。
        let worker = install_root.join(PID).join("bin/psd-worker.exe");
        std::fs::write(&worker, b"TAMPERED").unwrap();
        assert!(matches!(
            verify_installed_integrity(&install_root, PID, &ks, NOW),
            Err(InstallError::HashMismatch(_))
        ));
        let _ = std::fs::remove_dir_all(&install_root);
        let _ = std::fs::remove_dir_all(&staging_root);
    }
}

/// dev registry 工具产物核验(scripts/exotic-dev-registry.mjs)。#[ignore]:依赖
/// 已生成的 .dev-registry 目录与外部环境变量,CI/常规 test 不跑。跑法(PowerShell):
///   $env:PICASA_EXOTIC_DEV_FILE_URLS='1'
///   cargo test -p scrollery --lib dev_registry_artifacts -- --ignored
/// 用**生产同一套**校验器全链核验:keyset 解析 → index 验签+条目校验 → zip
/// verify_and_extract(验签/清单白名单/zip 加固/逐文件 hash)。
#[cfg(test)]
mod dev_registry_artifact_tests {
    use crate::exotic::crypto::VerifyingKeyset;
    use crate::exotic::install::{verify_and_extract, InstallLimits, RegistryExpect};

    #[test]
    #[ignore]
    fn dev_registry_artifacts_pass_production_validators() {
        assert_eq!(
            std::env::var("PICASA_EXOTIC_DEV_FILE_URLS").as_deref(),
            Ok("1"),
            "请以 PICASA_EXOTIC_DEV_FILE_URLS=1 运行本测试(file:// 条目校验开关)"
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.dev-registry");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let ks_json = std::fs::read_to_string(root.join("dev-keyset.json"))
            .expect("先运行 node scripts/exotic-dev-registry.mjs");
        let ks = VerifyingKeyset::parse(&ks_json).expect("dev keyset 解析");
        let index = std::fs::read(root.join("index.json")).unwrap();
        let sig = std::fs::read(root.join("index.sig")).unwrap();
        let v = crate::exotic::registry::verify_and_parse(&index, &sig, &ks, now, 0)
            .expect("index 验签+条目校验");
        assert!(!v.expired, "dev index 不应过期(重跑生成工具刷新)");
        let e = v
            .index
            .select("exotic-image-psd", "x86_64-pc-windows-msvc")
            .expect("PSD 条目");

        let zip = root.join("exotic-image-psd.zip");
        let staging = std::env::temp_dir().join(format!("dev-reg-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        let expect = RegistryExpect {
            plugin_id: &e.plugin_id,
            version: &e.version,
            target: &e.target,
            package_sequence: e.package_sequence,
        };
        let ex = verify_and_extract(
            &zip,
            &ks,
            &expect,
            "0.1.0",
            now,
            &staging,
            &InstallLimits::default(),
        )
        .expect("zip 生产校验链");
        assert!(ex.manifest.files.iter().any(|f| f.kind == "worker"));
        assert!(staging.join("psd-worker.exe").exists());
        let _ = std::fs::remove_dir_all(&staging);
    }
}

/// 内测 registry 工具产物核验(scripts/exotic-internal-registry.mjs)。#[ignore]:依赖
/// 已生成的 .internal-signing/ 目录,CI/常规 test 不跑。跑法:
///   cargo test -p scrollery --lib internal_registry_artifacts -- --ignored
/// 与 dev 版差异:keyset 为「占位+内测键」超集、package_url 为真实 HTTPS(无需 dev
/// file:// 开关)——正是内测安装包在生产验证链下将经历的形态(2026-07-05 内测链)。
#[cfg(test)]
mod internal_registry_artifact_tests {
    use crate::exotic::crypto::VerifyingKeyset;
    use crate::exotic::install::{verify_and_extract, InstallLimits, RegistryExpect};

    #[test]
    #[ignore]
    fn internal_registry_artifacts_pass_production_validators() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.internal-signing");
        let root = base.join("registry");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let ks_json = std::fs::read_to_string(base.join("internal-keyset.json"))
            .expect("先运行 node scripts/exotic-internal-registry.mjs");
        let ks = VerifyingKeyset::parse(&ks_json).expect("内测 keyset 解析");
        let index = std::fs::read(root.join("index.json")).unwrap();
        let sig = std::fs::read(root.join("index.sig")).unwrap();
        let v = crate::exotic::registry::verify_and_parse(&index, &sig, &ks, now, 0)
            .expect("index 验签+条目校验");
        assert!(!v.expired, "内测 index 不应过期(重跑生成工具刷新)");
        let e = v
            .index
            .select("exotic-image-psd", "x86_64-pc-windows-msvc")
            .expect("PSD 条目");
        assert!(
            e.package_url.starts_with("https://"),
            "内测条目必须是真实 HTTPS 直链(不同于 dev 的 file://)"
        );

        let zip = root.join("exotic-image-psd.zip");
        let staging =
            std::env::temp_dir().join(format!("internal-reg-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        let expect = RegistryExpect {
            plugin_id: &e.plugin_id,
            version: &e.version,
            target: &e.target,
            package_sequence: e.package_sequence,
        };
        let ex = verify_and_extract(
            &zip,
            &ks,
            &expect,
            "0.1.0",
            now,
            &staging,
            &InstallLimits::default(),
        )
        .expect("zip 生产校验链");
        assert!(ex.manifest.files.iter().any(|f| f.kind == "worker"));
        assert!(staging.join("psd-worker.exe").exists());
        let _ = std::fs::remove_dir_all(&staging);
    }
}
