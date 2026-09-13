//! 数据备份 IPC 命令(方案 B §5/§8 app 接线,阶段 2b)。
//!
//! 纯备份引擎在 `crate::backup::core`;本模块负责:文件任务门闩获取、文档一致性 write guard、
//! backup_id / 时间戳注入、**分离任务**执行(webview 关闭也跑到底、gate 必释放)、进度事件 +
//! 快照 + `backup_status` 查询(Channel 随 webview 死,故用 app 级事件 + AppState 快照,方案 §5.1)。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use tokio_util::sync::CancellationToken;

use crate::backup::core::{
    self, dest_writable, documents_consistent, run_backup, BackupParams, CODE_DIR_UNSET, CODE_IO,
    CODE_JOB_BUSY,
};
use crate::backup::manifest::{BackupKind, Manifest, BACKUP_FILE_EXT};
use crate::backup::RestoreStageResult;
use crate::error::{AppError, Result};
use crate::state::{AppState, FILE_JOB_BACKUP, FILE_JOB_RESTORE};

/// 备份进度传输事件名(app 级,广播给当前含重建后的 webview)。
pub const BACKUP_PROGRESS_EVENT: &str = "backup:progress";

/// 备份进度/状态快照(方案 §5.1)。备份是单次操作(非分块),进度粗粒度:running → 终态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgressPayload {
    /// "idle" | "running" | "completed" | "failed" | "cancelled"。
    pub status: String,
    /// "manual" | "auto"(运行中/终态所属类型)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 完成时正式包路径(用户自选目录,展示用)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 完成时包体字节数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// 失败时的稳定错误码(前端据此分流话术)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl BackupProgressPayload {
    fn running(kind: BackupKind) -> Self {
        Self {
            status: "running".into(),
            kind: Some(kind.file_infix().to_string()),
            path: None,
            bytes: None,
            code: None,
        }
    }
    fn idle() -> Self {
        Self {
            status: "idle".into(),
            kind: None,
            path: None,
            bytes: None,
            code: None,
        }
    }
}

/// 更新快照并广播事件(快照先行,保证事件消费者查询到的状态不落后于事件)。
fn publish_backup_progress(app: &AppHandle, state: &AppState, payload: BackupProgressPayload) {
    *state
        .backup_progress
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(payload.clone());
    let _ = app.emit(BACKUP_PROGRESS_EVENT, payload);
}

/// 备份状态查询(webview 重载恢复用):最近进度快照 + 运行态真相(token 存在)。
/// 快照说 "running" 但 token 已不在(异常终止)→ 报 "failed",避免恢复出永不结束的假运行态。
#[tauri::command]
pub fn backup_status(state: State<'_, Arc<AppState>>) -> Result<BackupProgressPayload> {
    let is_running = state.backup_token.is_running();
    let mut payload = state
        .backup_progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(BackupProgressPayload::idle);
    // 快照说 running 但 token 已不在:仅当**非取消中**才判「异常终止 → failed」。取消中
    // (cancel 已 take 空 token、收尾任务尚未发布 cancelled 终态)不误报 failed——保持 running,
    // 稍后收尾任务落 cancelled 终态(审查 #13)。
    if !is_running && payload.status == "running" && !state.is_backup_cancelling() {
        payload.status = "failed".into();
        payload.code = Some(CODE_IO.into());
    }
    Ok(payload)
}

/// preflight 结果(方案 §5.1)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightResult {
    /// 解析出的目的目录(arg 或 config backup_dir)。
    pub dest_dir: String,
    /// 目的地可写。
    pub writable: bool,
    /// 目的地与 appdata 同卷(同卷不能防物理盘损坏,UI 提示)。`None`=无法判定。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_volume_warning: Option<bool>,
    /// 文档行↔文件全部一致(不一致则备份会硬失败)。
    pub documents_consistent: bool,
    /// 估算包体上界(DB + WAL + documents 未压缩总字节)。
    pub estimated_bytes: u64,
}

/// 备份列表条目(设置页展示)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupListEntry {
    pub path: String,
    pub file_name: String,
    pub kind: String,
    pub created_at_utc: String,
    pub backup_id: String,
    pub bytes: u64,
}

// ── 同卷判定(best-effort,advisory)────────────────────────────────────────────
#[cfg(windows)]
fn volume_key(path: &Path) -> Option<String> {
    use std::path::Component;
    // 规范化后取路径前缀(盘符 C: / \\?\Volume{GUID} / UNC 服务器\共享)作卷键。
    let canon = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match canon.components().next()? {
        Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().to_uppercase()),
        _ => None,
    }
}
#[cfg(unix)]
fn volume_key(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev().to_string())
}

/// appdata 与 dest 是否同卷。`None`=无法判定。
fn same_volume(a: &Path, b: &Path) -> Option<bool> {
    match (volume_key(a), volume_key(b)) {
        (Some(x), Some(y)) => Some(x == y),
        _ => None,
    }
}

/// 估算包体上界:DB + WAL + documents 目录未压缩总字节(方案 §5.1;deflate 后通常更小)。
fn estimate_backup_bytes(app_data_dir: &Path) -> u64 {
    let file_len = |p: PathBuf| std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    let db = file_len(app_data_dir.join("scrollery.db"));
    let wal = file_len(app_data_dir.join("scrollery.db-wal"));
    let docs: u64 = walkdir::WalkDir::new(app_data_dir.join("documents"))
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    db.saturating_add(wal).saturating_add(docs)
}

/// 解析目的目录:显式 arg 优先,否则 config `backup_dir`。二者皆空返回 `backup_dir_unset`。
/// A2:`backup_dir` 是 schema 设置类键,唯一真源已切到 `ConfigManager`(内存读,不再需要
/// read_blocking 借读池连接查 DB)。
async fn resolve_dest(dest: Option<String>, state: &State<'_, Arc<AppState>>) -> Result<PathBuf> {
    if let Some(d) = dest.filter(|s| !s.trim().is_empty()) {
        return Ok(PathBuf::from(d));
    }
    match state
        .config
        .get("backup_dir")
        .filter(|s| !s.trim().is_empty())
    {
        Some(d) => Ok(PathBuf::from(d)),
        None => Err(AppError::Backup {
            code: CODE_DIR_UNSET,
            message: "未设置备份目的地 | backup dir unset".into(),
        }),
    }
}

/// backup_id 生成(16 字节随机 hex;复用 ring SystemRandom,失败兜底 pid+纳秒)。仅本地标识用。
fn gen_backup_id() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = [0u8; 16];
    if SystemRandom::new().fill(&mut buf).is_ok() {
        return crate::utils::hash::to_hex_lower(&buf);
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), nanos)
}

/// preflight:校验目的地可写、同卷警告、文档一致性、估算大小(方案 §5.1)。
#[tauri::command]
pub async fn preflight_backup(
    dest: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<PreflightResult> {
    // span 埋点(W1,D-312 info 档:目的地可写探测+文档一致性校验+目录估算,真实 IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:preflight_backup");
    let dest_dir = resolve_dest(dest, &state).await?;
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || -> Result<PreflightResult> {
        let app_data = state_arc.app_data_dir.clone();
        let writable = dest_writable(&dest_dir);
        let same = same_volume(&app_data, &dest_dir);
        let consistent = {
            let conn = state_arc.db_read_pool.get()?;
            documents_consistent(&conn, &app_data)
        };
        let estimated = estimate_backup_bytes(&app_data);
        Ok(PreflightResult {
            dest_dir: dest_dir.to_string_lossy().to_string(),
            writable,
            same_volume_warning: same,
            documents_consistent: consistent,
            estimated_bytes: estimated,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 立即备份(手动)。见 [`begin_backup`]。命令立即返回(前端轮询 `backup_status`)。
#[tauri::command]
pub async fn start_backup(
    app: AppHandle,
    dest: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let dest_dir = resolve_dest(dest, &state).await?;
    begin_backup(
        app,
        Arc::clone(&*state),
        dest_dir,
        BackupKind::Manual,
        None, // 手动包不轮转(B-6)
        false,
    )
}

/// 备份的共享启动核心(手动/自动共用)。获取 A/B 文件任务门闩(占用返回 `file_job_busy`,不抢占)
/// → 分离任务在持文档一致性 write guard 下执行 `run_backup` → 终态经事件+快照发布。分离任务保证
/// webview 关闭也跑到底、门闩必释放(方案 §5.1)。`update_last_success`=成功后回写
/// `backup_last_success_at`(自动备份判据用,方案 §7)。
fn begin_backup(
    app: AppHandle,
    state_arc: Arc<AppState>,
    dest_dir: PathBuf,
    kind: BackupKind,
    auto_retention: Option<usize>,
    update_last_success: bool,
) -> Result<()> {
    if !state_arc.try_acquire_file_job(FILE_JOB_BACKUP) {
        return Err(AppError::Backup {
            code: CODE_JOB_BUSY,
            message: "已有文件任务在运行 | file job busy".into(),
        });
    }
    let (generation, cancel_token) = state_arc.backup_token.begin();
    state_arc.clear_backup_cancelling(); // 新一轮:清上一轮可能遗留的「取消中」标志(审查 #13)
    publish_backup_progress(&app, &state_arc, BackupProgressPayload::running(kind));

    let app_data = state_arc.app_data_dir.clone();
    let db_path = app_data.join("scrollery.db");
    let backup_id = gen_backup_id();
    let now = chrono::Utc::now();
    let timestamp_label = now.format("%Y%m%d-%H%M%S").to_string();
    let created_at_utc = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let last_success_secs = now.timestamp();

    tauri::async_runtime::spawn(async move {
        let sb = Arc::clone(&state_arc);
        let result =
            tokio::task::spawn_blocking(move || -> Result<crate::backup::BackupOutcome> {
                // 文档一致性 write guard 覆盖整个备份(VACUUM 快照 + documents 打包同一逻辑时点,§3.1)。
                let _guard = sb
                    .document_storage_guard
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                let params = BackupParams {
                    source_db_path: &db_path,
                    app_data_dir: &app_data,
                    dest_dir: &dest_dir,
                    kind,
                    app_version: env!("CARGO_PKG_VERSION").to_string(),
                    backup_id,
                    created_at_utc,
                    timestamp_label,
                    auto_retention,
                };
                let outcome = run_backup(&params, &cancel_token)?;
                if update_last_success {
                    // 回写上次成功时刻(自动备份判据)。写锁在同一 blocking 上下文,不跨 await。
                    let conn = sb.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = crate::db::queries::set_config(
                        &conn,
                        "backup_last_success_at",
                        &last_success_secs.to_string(),
                    );
                }
                Ok(outcome)
            })
            .await;

        // 终态发布顺序(审查 #13):**先落终态快照、再清 token、门闩最后释放**。
        // - 发布前用 is_current(只读)判发布权:发布期间 token 仍在(is_running 仍 true),
        //   关闭「token 已清、快照仍 running」被 backup_status 误报 failed 的窗口;
        // - finish 之后快照已是终态(completed/failed/cancelled),backup_status 不再触发 running 推断;
        // - release_file_job 放最后:严格门闩下,新一轮须等门闩释放才能 begin,避免其「running」
        //   快照与本轮终态发布交错(自动调度器每小时的 begin 亦被门闩挡在本轮收尾之后)。
        let terminal = finalize_payload(kind, result);
        if state_arc.backup_token.is_current(generation) {
            publish_backup_progress(&app, &state_arc, terminal);
        }
        state_arc.backup_token.finish(generation);
        state_arc.clear_backup_cancelling();
        state_arc.release_file_job(FILE_JOB_BACKUP);
    });

    Ok(())
}

// ── 自动备份策略(方案 §7)──────────────────────────────────────────────────────

/// 自动备份触发判据(纯函数,可测):auto 开启 + 已设目的地 + 距上次成功 ≥24h。
/// 未选目录 / 未开启 → 恒 false(B-1:未选目录自动备份为关)。
fn should_run_auto_backup(
    now_secs: i64,
    last_success_secs: Option<i64>,
    auto_enabled: bool,
    has_backup_dir: bool,
) -> bool {
    if !auto_enabled || !has_backup_dir {
        return false;
    }
    match last_success_secs {
        None => true, // 从未成功 → 首次可备
        Some(last) => now_secs.saturating_sub(last) >= 24 * 3600,
    }
}

/// 自动备份一次检查:前台繁忙让步 → 读 config 判据 → 目的地可写 → 启动 Auto 备份(方案 §7)。
/// 启动扫描/全量缩略图/派生/活跃交互期间不启动(§7:运行后不强抢用户交互)。
async fn maybe_run_auto_backup(app: AppHandle, state_arc: Arc<AppState>) {
    if state_arc.is_scan_or_thumb_running()
        || state_arc.is_derivation_running()
        || state_arc.is_interactive()
        || state_arc.backup_token.is_running()
    {
        // 审查 #9:显式跳过「已有备份在跑」——严格门闩已从底层堵死双开,此处提前让步只是避免
        // 每小时空转 spawn 一个必被 file_job_busy 拒的任务(手动/上一轮自动备份进行中)。
        return;
    }
    // A2:backup_auto_enabled/backup_dir/backup_retention 均为 schema 设置类键,唯一真源已切到
    // ConfigManager(内存读);backup_last_success_at 是状态类键,仍走 DB。
    let auto = state_arc.config.get("backup_auto_enabled").as_deref() == Some("true");
    let dir = state_arc.config.get("backup_dir").unwrap_or_default();
    let retention = state_arc
        .config
        .get("backup_retention")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5);
    let sa = Arc::clone(&state_arc);
    let last = tokio::task::spawn_blocking(move || -> Option<i64> {
        let conn = sa.db_read_pool.get().ok()?;
        crate::db::queries::get_config(&conn, "backup_last_success_at")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
    })
    .await
    .ok()
    .flatten();
    let now = chrono::Utc::now().timestamp();
    if !should_run_auto_backup(now, last, auto, !dir.trim().is_empty()) {
        return;
    }
    let dest = PathBuf::from(&dir);
    let dest_probe = dest.clone();
    let writable = tokio::task::spawn_blocking(move || dest_writable(&dest_probe))
        .await
        .unwrap_or(false);
    if !writable {
        return;
    }
    // 忽略 file_job_busy(别的文件任务在跑时本轮跳过,下轮再试)。
    let _ = begin_backup(
        app,
        state_arc,
        dest,
        BackupKind::Auto,
        Some(retention),
        true,
    );
}

/// 启动自动备份调度器(方案 §7):启动后 idle ~2min 首检,此后每小时复检(距上次成功不足 24h
/// 时判据自然返回 false)。**须在 setup 内、app_state 就绪后调用**。dormant until 用户开启
/// `backup_auto_enabled`(B-1)。
pub fn start_auto_backup_scheduler(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        loop {
            maybe_run_auto_backup(app.clone(), Arc::clone(&state)).await;
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

/// 把 spawn_blocking 结果映射为终态快照。
fn finalize_payload(
    kind: BackupKind,
    result: std::result::Result<Result<crate::backup::BackupOutcome>, tokio::task::JoinError>,
) -> BackupProgressPayload {
    let kind_str = Some(kind.file_infix().to_string());
    match result {
        Ok(Ok(outcome)) => BackupProgressPayload {
            status: "completed".into(),
            kind: kind_str,
            path: Some(outcome.path.to_string_lossy().to_string()),
            bytes: Some(outcome.bytes),
            code: None,
        },
        Ok(Err(AppError::Backup { code, .. })) => BackupProgressPayload {
            status: if code == core::CODE_CANCELLED {
                "cancelled".into()
            } else {
                "failed".into()
            },
            kind: kind_str,
            path: None,
            bytes: None,
            code: Some(code.to_string()),
        },
        Ok(Err(_other)) => BackupProgressPayload {
            status: "failed".into(),
            kind: kind_str,
            path: None,
            bytes: None,
            code: Some(CODE_IO.into()),
        },
        Err(_join) => BackupProgressPayload {
            status: "failed".into(),
            kind: kind_str,
            path: None,
            bytes: None,
            code: Some(CODE_IO.into()),
        },
    }
}

/// 取消当前备份(只清当前 job staging,已正式落名的旧包不碰,方案 §5.1)。
#[tauri::command]
pub fn stop_backup(state: State<'_, Arc<AppState>>) -> Result<()> {
    state.cancel_backup();
    Ok(())
}

/// 列出目的目录内的备份包(设置页展示;读每包 manifest)。newest first。
#[tauri::command]
pub async fn list_backups(
    dest: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<BackupListEntry>> {
    // span 埋点(W1,D-312 info 档:逐包读 zip manifest,真实文件 IO)。
    let _span = crate::logging::SpanTimer::info("ipc:list_backups");
    let dest_dir = resolve_dest(dest, &state).await?;
    tokio::task::spawn_blocking(move || -> Result<Vec<BackupListEntry>> {
        let mut out = Vec::new();
        let Ok(read) = std::fs::read_dir(&dest_dir) else {
            return Ok(out);
        };
        for entry in read.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.ends_with(&format!(".{BACKUP_FILE_EXT}")) {
                continue;
            }
            if let Some(e) = read_backup_entry(&path) {
                out.push(e);
            }
        }
        // 按 created_at_utc 降序(RFC3339 字典序 == 时间序);缺失回退文件名。
        out.sort_by(|a, b| b.created_at_utc.cmp(&a.created_at_utc));
        Ok(out)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 读一个备份包的 manifest → 列表条目。任何失败(非法 zip / 无 manifest)返回 None(跳过)。
fn read_backup_entry(path: &Path) -> Option<BackupListEntry> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let entry = archive
        .by_name(crate::backup::manifest::ENTRY_MANIFEST)
        .ok()?;
    // 读取封顶(同 restore 侧):目的目录里一个损坏/敌意包声明多 GB manifest,不封顶则打开设置页
    // (list_backups)即分配 GB 级 String → OOM/长冻。超上限直接跳过该条目。
    if entry.size() > crate::backup::manifest::MAX_MANIFEST_BYTES {
        return None;
    }
    let mut s = String::new();
    entry
        .take(crate::backup::manifest::MAX_MANIFEST_BYTES)
        .read_to_string(&mut s)
        .ok()?;
    let m: Manifest = serde_json::from_str(&s).ok()?;
    Some(BackupListEntry {
        path: path.to_string_lossy().to_string(),
        file_name: path.file_name()?.to_string_lossy().to_string(),
        kind: m.kind.file_infix().to_string(),
        created_at_utc: m.created_at_utc,
        backup_id: m.backup_id,
        bytes,
    })
}

// ── 恢复(方案 B §6)────────────────────────────────────────────────────────────

/// 恢复第一步:把用户选的包校验并暂存(方案 §6.1),返回摘要供双确认。**不动活库**:
/// 只解压到 `restore-staging/`、校验、rebase。包路径是用户自选外部文件(合法在 appdata 外),
/// 不做 bounds-check;包内容当不可信输入由 restore_stage 全套白名单/zip-slip/sha256 校验兜住。
#[tauri::command]
pub async fn restore_stage(
    package_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<RestoreStageResult> {
    // span 埋点(W1,D-312 info 档:多 GB 解压+校验,真实重负载 IO/CPU)。
    let _span = crate::logging::SpanTimer::info("ipc:restore_stage");
    // 审查 #8:多 GB 解压/校验须纳入 A/B 文件门闩,与备份/arm 互斥。否则并发两次 restore_stage
    // (或与备份同时)会互删对方半解压的 staging(起始 remove_dir_all + StagingGuard RAII 交叉清理),
    // 最坏一方 disarm「成功」但 staging 已被删,后续 arm+重启在 Prepared 见 staging 缺失静默 no-op、
    // 无错上报。占用即以 file_job_busy 拒。
    if !state.try_acquire_file_job(FILE_JOB_RESTORE) {
        return Err(AppError::Backup {
            code: CODE_JOB_BUSY,
            message: "已有文件任务在运行 | file job busy".into(),
        });
    }
    let state_arc = Arc::clone(&*state);
    let result = tokio::task::spawn_blocking(move || -> Result<RestoreStageResult> {
        crate::backup::restore_stage(Path::new(&package_path), &state_arc.app_data_dir)
    })
    .await;
    // 门闩必释放(即便任务 panic:await 返回 Err 亦走到此)。
    state.release_file_job(FILE_JOB_RESTORE);
    result.map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 恢复第二步(用户双确认后):arm。获取 FILE_JOB_RESTORE 门闩 + 持文档一致性 write guard →
/// 生成 pre-restore 回滚包(成功前绝不 arm)+ 写 pending-restore 标记(§6.2.1-2)。成功后前端调
/// `relaunch_app` 触发重启,启动期 `perform_swap_at_boot` 完成交换。`backup_id`/`staging_dir` 取自
/// 上一步 restore_stage 结果。
#[tauri::command]
pub async fn restore_arm(
    backup_id: String,
    staging_dir: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:回滚包生成+文档一致性 write guard,真实重负载 IO)。
    let _span = crate::logging::SpanTimer::info("ipc:restore_arm");
    // backup_id 来自 IPC(webview 可被绕过),当不可信输入校验:拒穿越/绝对/盘符/多段。staging 目录
    // 一律由本机 app_data + 校验后的 backup_id **服务端派生**,不信前端传来的 staging_dir——否则开机
    // 交换会据裸路径把活库移出 appdata / 装入任意 DB(违反「canonicalize + bounds-check user paths」硬约束)。
    if !crate::backup::is_safe_backup_id(&backup_id) {
        return Err(AppError::Restore {
            code: crate::backup::restore::CODE_PATH_INVALID,
            message: "非法 backupId | invalid backup id".into(),
        });
    }
    let app_data = state.app_data_dir.clone();
    let staging = app_data.join("restore-staging").join(&backup_id);
    // 前端传来的 staging_dir 仅作一致性核对(非空且不匹配即拒);真实路径以派生为准。
    if !staging_dir.trim().is_empty() && Path::new(&staging_dir) != staging.as_path() {
        return Err(AppError::Restore {
            code: crate::backup::restore::CODE_PATH_INVALID,
            message: "staging 路径与 backupId 不符 | staging path mismatch".into(),
        });
    }
    // 暂存库须已存在(restore_stage 已成功暂存),否则无可 arm。
    if !staging.join("db").join("scrollery.db").exists() {
        return Err(AppError::Restore {
            code: crate::backup::restore::CODE_IO,
            message: "暂存库缺失,请先暂存 | staging db missing".into(),
        });
    }
    if !state.try_acquire_file_job(FILE_JOB_RESTORE) {
        return Err(AppError::Backup {
            code: CODE_JOB_BUSY,
            message: "已有文件任务在运行 | file job busy".into(),
        });
    }
    let state_arc = Arc::clone(&*state);
    let rollback_backup_id = gen_backup_id();
    let now = chrono::Utc::now();
    let timestamp_label = now.format("%Y%m%d-%H%M%S").to_string();
    let created_at_utc = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let app_version = env!("CARGO_PKG_VERSION").to_string();

    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        // 文档一致性 write guard 覆盖回滚包生成(run_backup 前置不变量,§3.1)。
        let _guard = state_arc
            .document_storage_guard
            .write()
            .unwrap_or_else(|e| e.into_inner());
        crate::backup::arm_restore(
            &app_data,
            &backup_id,
            &staging, // 服务端派生的安全路径(非前端裸串)
            &rollback_backup_id,
            &created_at_utc,
            &timestamp_label,
            &app_version,
            &CancellationToken::new(),
        )
    })
    .await;
    state.release_file_job(FILE_JOB_RESTORE);
    // arm 结果:JoinError → System;内层 Restore 错误原样透出(前端按稳定码分流)。
    result.map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 触发应用重启(恢复 arm 成功后由前端调用;启动期完成交换)。`app.restart()` 发散不返回。
#[tauri::command]
pub fn relaunch_app(app: AppHandle) -> Result<()> {
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同目录必然同卷(same_volume 对同一路径/子路径应判 true)。
    #[test]
    fn same_volume_same_dir_is_true() {
        let dir = std::env::temp_dir();
        assert_eq!(same_volume(&dir, &dir), Some(true));
    }

    /// backup_id:16 字节 hex = 32 字符(随机路径),或兜底非空。
    #[test]
    fn backup_id_is_hex_len_32() {
        let id = gen_backup_id();
        assert!(!id.is_empty());
        // 随机路径 = 32 hex;兜底路径长度不定但非空。此处只锁随机路径常态。
        if id.len() == 32 {
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    /// 自动备份判据(方案 §7 / B-1):未选目录 / 未开启恒 false;从未成功过首次可备;
    /// 距上次成功 ≥24h 触发、不足则否。
    #[test]
    fn auto_backup_decision() {
        let day = 24 * 3600;
        // 未开启 → false(即便已设目录、从未备过)。
        assert!(!should_run_auto_backup(day, None, false, true));
        // 已开启但未设目录 → false(B-1)。
        assert!(!should_run_auto_backup(day, None, true, false));
        // 开启 + 已设目录 + 从未成功 → true(首次)。
        assert!(should_run_auto_backup(day, None, true, true));
        // 距上次成功恰 24h → true;差 1 秒 → false。
        assert!(should_run_auto_backup(2 * day, Some(day), true, true));
        assert!(!should_run_auto_backup(2 * day - 1, Some(day), true, true));
    }

    /// 估算:空 appdata 目录(无 db/wal/documents)估算为 0,不 panic。
    #[test]
    fn estimate_empty_is_zero() {
        let dir = std::env::temp_dir().join(format!("scrollery_est_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(estimate_backup_bytes(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
