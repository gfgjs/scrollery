//! config.toml 文件监听(A1:本批只实现,不挂载到应用启动流程——挂载是 A2)。
//!
//! 用户手改 config.toml 后,应用需要感知变化并热加载;本模块提供该能力的独立实现,
//! A2 会在启动时调用 `spawn_config_watcher` 并把回调接到 `ConfigManager` 的重载逻辑上。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;
use tokio::sync::mpsc;

use super::file::{fingerprint, load, ConfigFileError, LoadedConfig};

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("创建配置文件监听器失败:{0}")]
    Init(#[from] notify::Error),
}

/// debounce 聚合窗口:多数编辑器 / 程序化写入的"保存"动作在文件系统层面会拆成多条事件
/// (常见模式是「写临时文件 → rename 替换」,一次保存触发 create + modify + remove 交错),
/// 聚合窗口内的多次触发只回调一次,避免同一次保存重复加载多次。
const DEBOUNCE: Duration = Duration::from_millis(500);

/// 判断一次文件变更是否是"自己刚写的"(防自触发回环)的纯函数部分:比较当前磁盘内容指纹
/// 与调用方记录的「最近一次本进程写盘」指纹。拆成纯函数是为了不必真起 watcher/tokio 运行时
/// 就能单测这条判定逻辑本身(任务范围要求"指纹自写跳过判定纯函数级即可,不必真起 watcher")。
///
/// `current` 为 `None`(如文件被并发删除导致读取失败)时视为"非自写"(不跳过)——宁可多触发
/// 一次回调,也不要在读不到内容时武断放弃通知调用方。
pub fn is_self_write(current: Option<u64>, last_own: Option<u64>) -> bool {
    match (current, last_own) {
        (Some(cur), Some(last)) => cur == last,
        _ => false,
    }
}

/// A2:比较 watcher 重载前后的「生效值表」,算出真正变了的键(`keys`)与其中需要重启才生效的
/// 子集(`restart_required`,对称于 `SettingDef.hot`)。纯函数,单测不必起真 watcher(A2 任务
/// 清单 §8 要求)。
///
/// 传入的两个表都是**原始 values 表**(只含文件里出现的实值,缺省键不在表内,与
/// `ConfigManager::raw_values_snapshot`/`file::LoadedConfig.values` 同型)——本函数内部按
/// `SETTING_DEFS` 解析各键的「实际生效值」(表里有则取表值,否则取 schema 默认值)再比较,
/// 与 `ConfigManager::get` 的解析口径一致,避免「表面上值没变、但一个从注释态一个从实值态
/// 恰好撞出同一默认值」这类误判。
pub fn diff_changed_keys(
    old_values: &std::collections::BTreeMap<String, String>,
    new_values: &std::collections::BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut keys = Vec::new();
    let mut restart_required = Vec::new();
    for def in super::schema::SETTING_DEFS {
        let old = old_values
            .get(def.key)
            .map(String::as_str)
            .unwrap_or(def.default);
        let new = new_values
            .get(def.key)
            .map(String::as_str)
            .unwrap_or(def.default);
        if old != new {
            keys.push(def.key.to_string());
            if !def.hot {
                restart_required.push(def.key.to_string());
            }
        }
    }
    (keys, restart_required)
}

/// 启动 config.toml 的文件监听,返回的 `RecommendedWatcher` 须由调用方持有(drop 即停止监听)。
///
/// # 设计要点
/// - **监听父目录、按文件名过滤**,而非直接监听文件本身:多数编辑器与本模块自己的
///   `write_atomic` 都是「写临时文件 + rename 替换」的原子保存模式,直接监听目标文件路径
///   会在 rename 后失去对新 inode 的关注(Windows/Linux 皆有此坑),必须听父目录、在回调里
///   按文件名匹配。
/// - **500ms debounce**:见 `DEBOUNCE` 注释,tokio task 聚合,不在 notify 的同步回调线程里做。
/// - **指纹跳过自写**:`last_own_fingerprint` 由调用方(`ConfigManager::set_and_persist`)在
///   自己写盘后更新;命中则跳过本次回调,防止「写 → 触发监听 → 重新加载 → 消费方可能又写
///   一次」的自激回环。
/// - 阻塞 IO(读文件、算指纹)经 `spawn_blocking`,不占用 tokio 事件循环线程(硬约束)。
/// - 内部只在同步的 `spawn_blocking` 闭包里短暂持有 `std::sync::Mutex` 锁,不跨 `.await`
///   持有(硬约束)。
/// - **不依赖 ambient runtime**:聚合任务 spawn 到调用方传入的 `runtime` 句柄上,故可在
///   无 tokio 上下文的线程(如 Tauri setup hook 主线程)安全调用——此处若直接 `tokio::spawn`
///   会 panic("there is no reactor running")。
pub fn spawn_config_watcher<F>(
    path: PathBuf,
    last_own_fingerprint: Arc<Mutex<Option<u64>>>,
    runtime: tokio::runtime::Handle,
    callback: F,
) -> Result<RecommendedWatcher, WatcherError>
where
    F: Fn(Result<LoadedConfig, ConfigFileError>) + Send + 'static,
{
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = path.file_name().map(std::ffi::OsStr::to_os_string);

    // notify 的回调跑在其内部监听线程上,只做「命中文件名就发一个信号」这件轻量事,
    // 真正的读文件/算指纹/回调都挪到下面的 tokio task 里做防抖聚合后再执行。
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        let hit = event.paths.iter().any(|p| {
            file_name
                .as_ref()
                .is_some_and(|n| p.file_name() == Some(n.as_os_str()))
        });
        if hit {
            let _ = signal_tx.send(());
        }
    })?;
    watcher.watch(&parent, RecursiveMode::NonRecursive)?;

    runtime.spawn(async move {
        loop {
            // 等待第一个事件;sender 全部 drop(watcher 被调用方丢弃)则退出聚合任务。
            if signal_rx.recv().await.is_none() {
                return;
            }
            // 固定窗口去抖:聚合窗口内的后续事件全部吞掉。用固定窗口(不随新事件重置)
            // 是有意的——重置窗口在连续保存动作下可能无限推迟回调。
            loop {
                tokio::select! {
                    () = tokio::time::sleep(DEBOUNCE) => break,
                    more = signal_rx.recv() => if more.is_none() { return },
                }
            }

            let path_for_read = path.clone();
            let last_fp = Arc::clone(&last_own_fingerprint);
            let outcome = tokio::task::spawn_blocking(move || {
                let content = std::fs::read_to_string(&path_for_read).ok();
                let current_fp = content.as_deref().map(fingerprint);
                let last = *last_fp
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if is_self_write(current_fp, last) {
                    // 一次性消费(P2 #4):命中后立即清空自写指纹,而不是留着让它对这份内容
                    // 长期免疫——否则用户外部把文件改回与本进程最近一次写盘完全相同的字节
                    // 内容,会被永久当成"自己写的"而吞掉,往后再也收不到这次外部改动的热更新。
                    *last_fp
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                    None
                } else {
                    Some(load(&path_for_read))
                }
            })
            .await;

            if let Ok(Some(result)) = outcome {
                callback(result);
            }
        }
    });

    Ok(watcher)
}

/// A2:挂载 config.toml 文件监听——用户手改配置文件后的热加载入口。
///
/// [`spawn_config_watcher`] 的回调契约是**同步闭包**(见该函数文档),但真正的处理
/// (diff → 逐 hot 键 `apply_setting_effects`(内含 `.await` 的 DB 写)→ emit)必须在 async
/// 上下文做,故回调本身只做一件事:把 `result` 连同已捕获的 `AppHandle`/`ConfigManager`
/// 一起丢进一个新 `tauri::async_runtime::spawn` task。
///
/// 失败契约:加载失败(语法错/IO)→ 记 `last_error` + emit `config-file-error`,**不应用不
/// 崩**,旧配置照常生效;单键类型警告(A1 语义,`file::load` 已实现)→ 跳过该键、warn 日志,
/// 警告键不进 `keys`(因为它没能进入新的生效值表,diff 视角下确实没变)。
///
/// 顺序不变量(拆分方案 §3.1 第 9 条):挂载点必须在 `app.manage(app_state)` **之后**——
/// 回调内 `try_state::<Arc<AppState>>()` 需要它,故置于 setup 尾部,不可因「watcher 感觉
/// 该早点挂」而前移。
pub fn spawn_config_file_watcher(
    app: &tauri::AppHandle,
    config_path: PathBuf,
    config: Arc<super::ConfigManager>,
) -> Option<notify::RecommendedWatcher> {
    use tauri::{Emitter, Manager};

    // watcher 内部聚合任务须 spawn 进 tokio runtime,但本函数在 setup hook 主线程被调,
    // 无 ambient runtime 上下文(直接 tokio::spawn 会 panic)。经 block_on 短暂进入 tauri
    // 全局 runtime 拿其 Handle,显式传给 watcher。
    let runtime = tauri::async_runtime::block_on(async { tokio::runtime::Handle::current() });
    let fingerprint_lock = config.fingerprint_lock();
    let app_for_cb = app.clone();
    let config_for_cb = config.clone();

    let result = spawn_config_watcher(config_path, fingerprint_lock, runtime, move |load_result| {
        let app = app_for_cb.clone();
        let config = config_for_cb.clone();
        tauri::async_runtime::spawn(async move {
            match load_result {
                Ok(loaded) => {
                    for w in &loaded.warnings {
                        tracing::warn!(
                            key = %w.key,
                            message = %w.message,
                            "config.toml 外部编辑键警告(已跳过,沿用默认值) | external-edit key warning, skipped"
                        );
                    }

                    let old_values = config.raw_values_snapshot();
                    let (keys, restart_required) = diff_changed_keys(&old_values, &loaded.values);
                    config.replace_values(loaded.values);
                    config.clear_load_error();

                    if keys.is_empty() {
                        // 无实际生效值变化(如仅改了注释/排版,或写回与默认值相同的值)→
                        // 不广播,避免前端空刷新。
                        return;
                    }

                    let restart_set: std::collections::HashSet<&str> =
                        restart_required.iter().map(String::as_str).collect();
                    if let Some(state) = app.try_state::<Arc<crate::state::AppState>>() {
                        for key in &keys {
                            if restart_set.contains(key.as_str()) {
                                continue; // 仅存值(已在 replace_values 生效),下次重启才应用副作用。
                            }
                            let value = config.get(key).unwrap_or_default();
                            if let Err(e) = crate::ipc::config_commands::apply_setting_effects(
                                &app, &state, key, &value,
                            )
                            .await
                            {
                                tracing::warn!(
                                    "config.toml 外部编辑应用键 {} 的热应用副作用失败(值已生效,副作用可能需手动触发) | applying hot-effect for key {} failed: {}",
                                    key, key, e
                                );
                            }
                        }
                    }

                    let _ = app.emit(
                        "config-file-changed",
                        serde_json::json!({ "keys": keys, "restart_required": restart_required }),
                    );
                }
                Err(e) => {
                    let (message, line) = e.to_status_message();
                    tracing::warn!(
                        "config.toml 外部编辑加载失败,已保留上一次成功配置(不应用不崩) | external edit load failed, kept last-good config: {message}"
                    );
                    config.record_load_error(e);
                    let _ = app.emit(
                        "config-file-error",
                        serde_json::json!({ "message": message, "line": line }),
                    );
                }
            }
        });
    });

    match result {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(
                "config.toml 文件监听启动失败,热更新不可用(设置页读写配置仍正常) | config watcher init failed: {e}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_write_is_skipped_when_fingerprints_match() {
        assert!(is_self_write(Some(42), Some(42)));
    }

    #[test]
    fn external_write_is_not_skipped_when_fingerprints_differ() {
        assert!(!is_self_write(Some(42), Some(7)));
    }

    #[test]
    fn missing_last_own_is_not_treated_as_self_write() {
        // 尚未有过自写记录(如刚启动、watcher 挂载早于第一次 set_and_persist)→ 不跳过。
        assert!(!is_self_write(Some(42), None));
    }

    #[test]
    fn unreadable_current_content_is_not_treated_as_self_write() {
        // 读取失败(如文件被并发删除)→ 宁可多通知一次,不武断当成自写跳过。
        assert!(!is_self_write(None, Some(42)));
    }

    // ── diff_changed_keys ────────────────────────────────────────────────────

    #[test]
    fn diff_reports_no_change_for_identical_tables() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("thumb_size".to_string(), "256".to_string());
        let (keys, restart) = diff_changed_keys(&m, &m.clone());
        assert!(keys.is_empty());
        assert!(restart.is_empty());
    }

    /// hot 键(如 thumb_size)变化 → 进 `keys`,不进 `restart_required`。
    #[test]
    fn diff_hot_key_change_is_not_restart_required() {
        let old = std::collections::BTreeMap::new(); // 空表 = 全部走默认值
        let mut new = std::collections::BTreeMap::new();
        new.insert("thumb_size".to_string(), "256".to_string()); // 默认 512 → 覆盖 256,hot=true
        let (keys, restart) = diff_changed_keys(&old, &new);
        assert!(keys.contains(&"thumb_size".to_string()));
        assert!(!restart.contains(&"thumb_size".to_string()));
    }

    /// 非 hot 键(如 log_dir,restart_required=true)变化 → 同时进 `keys` 与 `restart_required`。
    #[test]
    fn diff_cold_key_change_is_restart_required() {
        let old = std::collections::BTreeMap::new();
        let mut new = std::collections::BTreeMap::new();
        new.insert("log_dir".to_string(), "D:/logs".to_string());
        let (keys, restart) = diff_changed_keys(&old, &new);
        assert!(keys.contains(&"log_dir".to_string()));
        assert!(restart.contains(&"log_dir".to_string()));
    }

    /// 「注释态回默认值」与「显式写回同默认值」在 diff 视角下等价于无变化——按解析后的生效值
    /// 比较,不按「键是否出现在表里」比较。
    #[test]
    fn diff_treats_explicit_default_same_as_commented_default() {
        let old = std::collections::BTreeMap::new(); // thumb_size 缺省 → 默认 512
        let mut new = std::collections::BTreeMap::new();
        new.insert("thumb_size".to_string(), "512".to_string()); // 显式写回默认值
        let (keys, _restart) = diff_changed_keys(&old, &new);
        assert!(!keys.contains(&"thumb_size".to_string()));
    }
}
