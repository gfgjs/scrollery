//! 设置写入的统一编排(方案 §5.1/§5.3):提交、外部重读、全量重置三者共用一条串行路径。
//!
//! # 串行范围(为什么需要一整道门,而不只是「写盘时加锁」)
//! 写盘本身的串行由 ConfigManager 的 write_lock 保证,但那**只覆盖到文件为止**。一次提交的完整
//! 语义是「改文件与内存 → 应用运行时影响 → 广播快照」:
//!
//! - 若只串行写盘:提交 A 写完盘、副作用还没跑完,B 就写完了,于是 A 的副作用作用在 B 的配置上,
//!   而 A 广播出去的 keys 又配上 B 的快照——前端按 A 的 keys 去对照 B 的值,刷新范围与提示都会错。
//! - 重置尤其危险:重置要让「重置前的写入影响」全部作废,若某次提交已经越过写盘、正卡在副作用上,
//!   它会在默认配置生效后继续改运行时状态,把重置结果搅浑。
//!
//! 故本模块持一道**进程级 tokio Mutex**,把「提交 → 应用影响 → 广播」整段串起来;外部文件重读
//! (watcher)与重置走同一道门。门以 tokio 形式跨 .await 持有,盘上的阻塞工作仍在 spawn_blocking
//! 里用 std 锁(不跨 .await 持 std 锁,硬约束)。
//!
//! # 代次(generation)
//! generation 只在重置时递增,每个写入请求携带调用方见到的值,旧代次一律被拒
//! (config_stale_generation)。串行门保证「重置期间不会有写入插进来」,代次则保证「重置之前已在
//! 排队、重置之后才轮到的写入」不会把旧值写回默认配置——两者缺一不可。

use std::collections::BTreeMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use super::file::ConfigFileError;
use super::{CommitError, CommitOutcome, SettingDef, SettingsChange, COMMIT_GATE};
use crate::error::{AppError, Result};
use crate::state::AppState;

/// config-file-changed 事件名:外部编辑、UI 提交与重置共用,载荷是完整的 SettingsChange。
pub const CONFIG_FILE_CHANGED: &str = "config-file-changed";

/// config-file-error 事件名:配置文件解析失败(生效值不变,仅提示用户)。
pub const CONFIG_FILE_ERROR: &str = "config-file-error";

/// 整批校验并规范化:任一键非法(未知键/类型不符/越界/结构里出现未知字段或未登记标签)即整批拒绝,
/// 不做「跳过坏项、写入其余」的部分提交——用户提交的是一组意图,半批落盘比整批失败更难收拾。
pub fn canonicalize_patch(patch: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (key, raw) in patch {
        let Some(def) = SettingDef::find(key) else {
            return Err(AppError::Config {
                code: "config_unknown_key",
                message: format!("未知设置键 {key}:不在当前版本的设置清单内,整批已拒绝"),
            });
        };
        let canonical =
            super::value::text_to_canonical(def, raw).map_err(|msg| AppError::Config {
                code: "config_invalid_value",
                message: format!("{key} {msg}"),
            })?;
        out.insert(key.clone(), canonical);
    }
    Ok(out)
}

/// 统一提交入口:一批具名键值 + 调用方见到的 generation。
///
/// 返回值与广播的事件载荷同型,调用方可直接用返回值,不必等事件回环。
pub async fn submit_settings_patch(
    app: &AppHandle,
    state: &Arc<AppState>,
    patch: BTreeMap<String, String>,
    generation: u64,
) -> Result<SettingsChange> {
    let canonical = canonicalize_patch(&patch)?;
    let _gate = COMMIT_GATE.lock().await;
    let config = Arc::clone(&state.config);
    let outcome = tokio::task::spawn_blocking(move || config.commit_batch(&canonical, generation))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
        .map_err(commit_error_to_app)?;
    finish_commit(app, state, outcome, Vec::new(), super::effects::ApplyOrigin::Commit).await
}

/// 窗口几何提交:只改动 `label` 这一个窗口的记录,其他窗口的几何原样保留(合并发生在配置写锁
/// 内,故两个窗口的并发提交不会互相覆盖)。`value_json` 是该 label 的几何结构 JSON 文本。
///
/// 这是窗口模块的唯一写入口:采集侧的防抖合并由窗口模块自己完成,故每次调用只落盘一次。
pub async fn submit_window_geometry(
    app: &AppHandle,
    state: &Arc<AppState>,
    label: &str,
    value_json: &str,
    generation: u64,
) -> Result<SettingsChange> {
    // 该键的 label 合法性与结构校验在配置写锁内完成(合并也必须在那里做,否则两个窗口的并发
    // 提交会互相覆盖)。此处只把调用方的原始输入透传进去。
    require_def("window_geometry")?;
    let label = label.to_string();
    let value_json = value_json.to_string();
    let _gate = COMMIT_GATE.lock().await;
    let config = Arc::clone(&state.config);
    let outcome = tokio::task::spawn_blocking(move || {
        config.commit_struct_map_entry("window_geometry", &label, &value_json, generation)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    .map_err(commit_error_to_app)?;
    finish_commit(app, state, outcome, Vec::new(), super::effects::ApplyOrigin::Commit).await
}

/// 恢复默认设置:用 schema 生成的全默认模板原子替换 config.toml,然后把运行时影响按整批差异统一
/// 应用并广播默认快照。
///
/// 不触碰数据库:资产、阅读进度、扫描根、任务启停标志与开机引导标记在重置前后保持不变
/// (方案 §3.2/§7 第 3 步)。因此重置**不会**把已停止或已暂停的派生/分析任务重新启动,也不写任何
/// 「续跑」意图——是否重启任务由用户或既有的显式入口决定。
///
/// 顺序(窗口侧钩子见 config::window 模块文档):
/// 1. **先取提交门**:并发第二次重置会在此排队,不会与本次的暂停/恢复交错。
/// 2. 暂停窗口几何采集并丢弃待保存值——否则重置瞬间的迟到几何会把旧位置写回刚重置的文件。
/// 3. 原子替换文件并递增 generation(在途写入自此全部失效)。
/// 4. 让常驻窗口回到创建默认几何(程序化落位,不回写配置)。
/// 5. 无论成败恢复采集(finally 语义)。
pub async fn reset_settings(app: &AppHandle, state: &Arc<AppState>) -> Result<SettingsChange> {
    let _gate = COMMIT_GATE.lock().await;
    super::window::suspend_for_reset();
    let outcome = async {
        let config = Arc::clone(&state.config);
        let outcome = tokio::task::spawn_blocking(move || config.reset_all())
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
            .map_err(AppError::from)?;
        // 窗口默认几何未落位属「已保存但未生效」:配置文件已是默认值,只是某个窗口没搬过去。
        // 既不能回滚文件、也不能报成保存失败,故并入 apply_failed 如实告知(前端按同一字段提示)。
        let mut apply_failed = Vec::new();
        if let Err(e) = super::window::apply_default_geometry(app) {
            tracing::warn!("恢复默认窗口几何未生效(设置已保存) | applying default geometry failed: {e}");
            apply_failed.push("window_geometry".to_string());
        }
        // 重置来源:派生流水线只在原本「期望运行」时才重启,不把已停止/已暂停的任务拉起来。
        finish_commit(app, state, outcome, apply_failed, super::effects::ApplyOrigin::Reset).await
    }
    .await;
    // finally 兜底:任何失败路径都要恢复采集,不能因为一次重置失败就让窗口几何永久停摆。
    // 门仍在此持有,故恢复动作不会被另一次重置的暂停抢先或覆盖。
    super::window::resume_after_reset();
    outcome
}

/// 外部文件重读(由 watcher 的「文件变了」信号触发)。
///
/// **在门内重新读盘**,而不是应用 watcher 早先读到的那份内容:防抖窗口里读到的文件可能已经过期,
/// 带着它进门就会让旧快照覆盖新状态(重置后的旧值复活即由此而来)。读取失败只记录错误并广播
/// config-file-error,当前生效值保持不变。
pub async fn reload_from_disk(
    app: &AppHandle,
    state: &Arc<AppState>,
) {
    let _gate = COMMIT_GATE.lock().await;
    let config = Arc::clone(&state.config);
    let result = tokio::task::spawn_blocking(move || config.reload_from_disk()).await;
    match result {
        Ok(Ok(outcome)) => {
            if outcome.keys.is_empty() {
                // 无实质变化(仅改注释/排版,或把键写回其默认值)→ 不广播,避免前端空刷新。
                return;
            }
            if let Err(e) = finish_commit(
                app,
                state,
                outcome,
                Vec::new(),
                super::effects::ApplyOrigin::Commit,
            )
            .await
            {
                tracing::error!("外部编辑应用运行时影响失败 | applying external reload effects failed: {e}");
            }
        }
        Ok(Err(e)) => {
            // 读取失败或语法错:值表保持上一次成功加载的内容,只把错误告知前端。
            tracing::warn!(
                "config.toml 外部编辑加载失败,已保留上一次成功配置(不应用不崩) | external edit load failed, kept last-good config: {}",
                e.message
            );
            emit_load_error(app, e.message, e.line);
        }
        Err(e) => {
            tracing::error!("外部编辑重读任务失败 | config reload task failed: {e}");
        }
    }
}

/// 广播配置文件解析错误(前端据此显示横幅;不改变任何生效值)。
fn emit_load_error(app: &AppHandle, message: String, line: Option<usize>) {
    if let Err(e) = app.emit(CONFIG_FILE_ERROR, serde_json::json!({ "message": message, "line": line }))
    {
        tracing::warn!(
            "config-file-error 广播失败 | broadcasting config error failed: {e}"
        );
    }
}

/// 提交被拒的原因 → 稳定 IPC 错误(不透传内部字符串)。
///
/// **两个类别的稳定码必须分开**,调用方据此决定「重试」还是「丢弃」:
/// - `config_invalid_value` / `config_unknown_key`:值本身不被接受,重试多少次都不会成功;
/// - `config_write_failed`:写盘/IO 失败,属可恢复的瞬时故障,值得重试。
fn commit_error_to_app(err: CommitError) -> AppError {
    match err {
        CommitError::StaleGeneration { .. } => AppError::Config {
            code: "config_stale_generation",
            message: "设置已被恢复默认刷新,本次写入基于旧状态,已拒绝;请按最新设置重试".to_string(),
        },
        CommitError::Write(ConfigFileError::InvalidValue(message)) => AppError::Config {
            code: "config_invalid_value",
            message,
        },
        CommitError::Write(ConfigFileError::UnknownKey(key)) => AppError::Config {
            code: "config_unknown_key",
            message: format!("未知设置键 {key}"),
        },
        CommitError::Write(e) => AppError::from(e),
    }
}

fn require_def(key: &str) -> Result<&'static SettingDef> {
    SettingDef::find(key).ok_or_else(|| AppError::Config {
        code: "config_unknown_key",
        message: format!("未知设置键 {key}"),
    })
}

/// 提交后统一收尾:应用本批的运行时影响 → 组装快照 → 广播。
///
/// **调用方必须已持有提交门**——本函数内部不再取门,以免自锁。
async fn finish_commit(
    app: &AppHandle,
    state: &Arc<AppState>,
    outcome: CommitOutcome,
    mut apply_failed: Vec<String>,
    origin: super::effects::ApplyOrigin,
) -> Result<SettingsChange> {
    // 只对「无需重启」的键应用运行时影响:重启类键已在内存与文件中生效,副作用统一留到下次启动,
    // 与外部编辑路径同一口径。
    let apply: Vec<(String, String)> = outcome
        .keys
        .iter()
        .filter(|key| !outcome.restart_required.iter().any(|k| k == *key))
        .map(|key| {
            let value = state.config.get(key).unwrap_or_default();
            (key.clone(), value)
        })
        .collect();
    if !apply.is_empty() {
        apply_failed.extend(super::effects::apply_batch(app, state, &apply, origin).await);
    }

    let change = SettingsChange {
        snapshot: state.config.snapshot(),
        keys: outcome.keys,
        restart_required: outcome.restart_required,
        apply_failed,
    };
    broadcast_change(app, &change);
    Ok(change)
}

/// 广播一次设置变更(所有存活窗口共用同一份完整快照,前端只应用、不触发保存)。
/// 广播失败只记日志:配置已落盘,事件只是通知。
pub fn broadcast_change(app: &AppHandle, change: &SettingsChange) {
    if let Err(e) = app.emit(CONFIG_FILE_CHANGED, change) {
        tracing::warn!(
            "config-file-changed 广播失败(设置已落盘) | broadcasting settings change failed: {e}"
        );
    }
}
