//! 配置文件子系统:设置类键唯一真源在 `<app_data_dir>/config.toml`(带中文注释),状态类键
//! 仍留 SQLite `app_config` 表(键分类见 `schema` 模块文档)。
//!
//! A1(建模块+单测)+ A2(接线)已全部落地:启动时 `lib.rs::run()` 调用 `config::boot::init`
//! 装配 `ConfigManager` 并存进 `AppState`;`ipc::config_commands` 的 `get_app_config`/
//! `set_app_config`/`get_startup_config` 按 schema 键 ∪ 状态键 ∪ 未知键三路路由;
//! `watcher::spawn_config_file_watcher` 由 `lib.rs::run()` 的 setup 尾部挂载,驱动外部
//! 编辑热加载(`config-file-changed`/`config-file-error` 事件)。
//!
//! 子模块:
//! - [`schema`]:键定义单源(`SETTING_DEFS`)+ 状态键清单(`STATE_KEYS`)。
//! - [`file`]:TOML 读写(加载校验 / 模板渲染 / 就地改写 / 原子写盘 / 内容指纹)。
//! - [`migrate`]:DB → config.toml 一次性迁移。
//! - [`watcher`]:文件监听 + diff 纯函数(`diff_changed_keys`)+ 启动期挂载入口。
//! - [`boot`]:启动期装配(`ConfigManager` 构造 + 启动期一次性读取的设置键)。

pub mod boot;
pub mod file;
pub mod migrate;
pub mod schema;
pub mod watcher;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

pub use file::{ConfigFileError, KeyWarning, LoadedConfig};
pub use schema::{SettingDef, SettingKind, SETTING_DEFS, STATE_KEYS};

/// 配置管理器门面:持有 config.toml 路径、当前生效值表、最近一次本进程写盘内容的指纹。
///
/// 「生效值表」只存**文件中确实出现的实值**(用户取消注释覆盖的键);未出现的键由 [`get`]
/// 回退 [`SettingDef::default`],消费方无需重复判空/查 schema。
///
/// [`get`]: ConfigManager::get
pub struct ConfigManager {
    path: PathBuf,
    values: RwLock<BTreeMap<String, String>>,
    /// watcher 防自写回环用的指纹锁;`Arc` 包裹是为了 A2 挂载 watcher 时可以把同一把锁
    /// 共享给 `watcher::spawn_config_watcher`,而不必在两处各自维护一份、还要保证同步。
    last_own_fingerprint: Arc<Mutex<Option<u64>>>,
    /// A2:最近一次整文件加载失败(启动期语法错 / watcher 外部编辑语法错)。`None` = 上次加载
    /// 成功。**不影响已生效值表**——加载失败时 `values` 保留上一次成功加载的内容(启动期首次
    /// 失败则为空,全部回退 schema 默认值),供 `get_config_status` IPC 展示错误提示。
    last_error: RwLock<Option<ConfigFileError>>,
    /// `set_and_persist`/`reset_key` 的写串行锁:read_doc→toml_edit 改写→原子写盘→指纹更新
    /// 全程持锁,防止两次并发写交错导致后写覆盖前写、或指纹记录与实际落盘内容错配(错配会
    /// 让 watcher 的自写跳过判定误判/漏判)。调用点均在 `spawn_blocking` 的同步上下文里,
    /// 不跨 `.await` 持有(硬约束)。
    write_lock: Mutex<()>,
}

impl ConfigManager {
    /// 从磁盘加载(不存在则先跑一次性 DB 迁移生成),返回门面 + 加载期警告(供 A2 转发到
    /// 设置页展示)。
    ///
    /// 顺序是硬约束:先 `file::load` 校验整文件语法(语法错在此提前 `Err` 返回,**不触碰
    /// 磁盘**——绝不能在还没确认文件合法之前就往上面追加内容);只有加载成功后,才会尝试
    /// 「自愈补全」——若当前 schema 里有键连注释态都不在磁盘文件里(典型场景是应用版本升级
    /// 后 `SETTING_DEFS` 新增了设置项,而用户的 config.toml 是旧版本写出的),把这些新键的
    /// 模板片段追加到文件末尾并重新写盘——只追加,不改动用户已有内容/注释/排版,让用户能在
    /// 文件里发现新版本带来的新设置项,而不是必须去翻更新日志。
    pub fn load_or_init(
        path: PathBuf,
        db_conn: &rusqlite::Connection,
    ) -> Result<(Self, Vec<KeyWarning>), ConfigFileError> {
        migrate::migrate_if_needed(db_conn, &path)?;

        let loaded = file::load(&path)?;

        if let Ok(existing) = std::fs::read_to_string(&path) {
            if let Some(healed) = file::append_missing_keys(&existing) {
                file::write_atomic(&path, &healed)?;
            }
        }

        let manager = Self {
            path,
            values: RwLock::new(loaded.values),
            last_own_fingerprint: Arc::new(Mutex::new(None)),
            last_error: RwLock::new(None),
            write_lock: Mutex::new(()),
        };
        Ok((manager, loaded.warnings))
    }

    /// A2:整文件加载失败(启动期语法错 / IO 错)时的降级构造——**不写盘、不覆盖用户的坏
    /// 文件**,全部键回退 schema 默认值(空值表,`get` 全部走 `SettingDef::default` 分支),
    /// 并记录本次错误供 `get_config_status` 展示;应用照常启动(A2 硬约束:绝不因配置文件
    /// 写错而拒绝启动)。
    pub fn degraded(path: PathBuf, error: ConfigFileError) -> Self {
        let manager = Self {
            path,
            values: RwLock::new(BTreeMap::new()),
            last_own_fingerprint: Arc::new(Mutex::new(None)),
            last_error: RwLock::new(None),
            write_lock: Mutex::new(()),
        };
        manager.record_load_error(error);
        manager
    }

    /// 记录一次加载失败(watcher 外部编辑触发的语法错走此口)。
    pub fn record_load_error(&self, err: ConfigFileError) {
        *self
            .last_error
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(err);
    }

    /// 清空加载错误(watcher 成功重载后调用——错误已被新的成功加载取代)。
    pub fn clear_load_error(&self) {
        *self
            .last_error
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// 取当前记录的加载错误,转成 `get_config_status` IPC 契约的 `(message, line)`。
    pub fn load_error_status(&self) -> Option<(String, Option<usize>)> {
        self.last_error
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(ConfigFileError::to_status_message)
    }

    /// 按键取当前生效值:文件里有实值 → 该值;否则回退 schema 默认值;键完全不在 schema 内
    /// → `None`(A2 的 IPC 层据此区分「合法但未覆盖」与「压根不认识这个键」)。
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some(v) = self
            .values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
        {
            return Some(v.clone());
        }
        SettingDef::find(key).map(|d| d.default.to_string())
    }

    /// 写入新值:读回磁盘当前文档(就地改保留其余键注释与排版)→ 改目标键 → 原子写盘 →
    /// 更新内存生效值表与「最近自写」指纹。**全程持 `write_lock`**(读回旧文档到指纹更新
    /// 之间不释放),防止与另一次并发写(`set_and_persist`/`reset_key`)交错导致后写基于
    /// 过期文档覆盖前写、或指纹记录的内容与实际落盘内容错配。调用点在 `spawn_blocking` 的
    /// 同步上下文内,不跨 `.await` 持锁(硬约束)。
    pub fn set_and_persist(&self, key: &str, value: &str) -> Result<(), ConfigFileError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut doc = self.read_doc_for_edit()?;
        file::set_value(&mut doc, key, value);
        let content = doc.to_string();
        let fp = file::write_atomic(&self.path, &content)?;
        self.values
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.to_string(), value.to_string());
        *self
            .last_own_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
        Ok(())
    }

    /// 重置某键为默认值:删除文件中的实值行(该键从此按注释态处理,`get` 回退 schema 默认值)。
    /// 同 `set_and_persist` 持 `write_lock` 串行化整个读改写序列(理由同上)。
    pub fn reset_key(&self, key: &str) -> Result<(), ConfigFileError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut doc = self.read_doc_for_edit()?;
        file::unset_value(&mut doc, key);
        let content = doc.to_string();
        let fp = file::write_atomic(&self.path, &content)?;
        self.values
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
        *self
            .last_own_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A2:取当前生效值表的快照(仅含文件中确实出现的实值键,与 `LoadedConfig.values` 同型)。
    /// 供 watcher 回调在替换前先取旧值算 diff(`watcher::diff_changed_keys`)。
    pub fn raw_values_snapshot(&self) -> BTreeMap<String, String> {
        self.values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A2:整表替换生效值(watcher 外部编辑重载成功后调用)。不改指纹锁——那是「本进程自写」
    /// 的记录,外部编辑走的正是「指纹不匹配」分支才会走到这里,不应回填。
    pub fn replace_values(&self, values: BTreeMap<String, String>) {
        *self
            .values
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = values;
    }

    /// A2 挂载 watcher 时需要与本门面共享同一把指纹锁(而不是各自持有互不知情的两份),
    /// 故对外暴露一个 `Arc` 克隆——watcher 收到外部变更后经它读最近自写指纹判自触发。
    pub fn fingerprint_lock(&self) -> Arc<Mutex<Option<u64>>> {
        Arc::clone(&self.last_own_fingerprint)
    }

    /// 取当前磁盘内容并解析成可编辑文档,供 `set_and_persist`/`reset_key` 内部复用。
    /// 文件在 `load_or_init` 之后正常不会消失;此处仍兜底处理「运行期文件被外部删除」的
    /// 极端场景——退回全新模板(全默认注释态)而不是直接报错,使编辑操作仍可完成。
    fn read_doc_for_edit(&self) -> Result<toml_edit::DocumentMut, ConfigFileError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                file::render_template(&BTreeMap::new())
            }
            Err(e) => return Err(ConfigFileError::Io(e)),
        };
        file::parse_doc(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        conn
    }

    #[test]
    fn load_or_init_creates_file_and_get_falls_back_to_default() {
        let conn = make_conn();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, warnings) = ConfigManager::load_or_init(path.clone(), &conn).unwrap();
        assert!(warnings.is_empty());
        assert!(path.exists());
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512")); // schema 默认值
        assert_eq!(manager.get("no_such_key"), None);
    }

    #[test]
    fn set_and_persist_then_get_reflects_new_value() {
        let conn = make_conn();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path, &conn).unwrap();
        manager.set_and_persist("thumb_size", "128").unwrap();
        assert_eq!(manager.get("thumb_size").as_deref(), Some("128"));
    }

    #[test]
    fn reset_key_falls_back_to_default_again() {
        let conn = make_conn();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path, &conn).unwrap();
        manager.set_and_persist("thumb_size", "128").unwrap();
        manager.reset_key("thumb_size").unwrap();
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512"));
    }

    /// A2:降级构造不写盘(路径压根不存在)、全部键回退默认值、且记录的错误可查。
    #[test]
    fn degraded_falls_back_to_defaults_and_records_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml"); // 故意不创建
        let err = super::file::ConfigFileError::Syntax {
            line: 3,
            column: 5,
            message: "示例语法错".to_string(),
        };
        let manager = ConfigManager::degraded(path.clone(), err);
        assert!(!path.exists(), "degraded 不得写盘/覆盖用户文件");
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512"));
        let (msg, line) = manager.load_error_status().expect("应记录加载错误");
        assert_eq!(line, Some(3));
        assert!(msg.contains("示例语法错"));
    }

    /// A2:watcher 成功重载后清错——`clear_load_error` 使 `load_error_status` 回到 `None`。
    #[test]
    fn clear_load_error_resets_status() {
        let conn = make_conn();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path, &conn).unwrap();
        manager.record_load_error(super::file::ConfigFileError::Io(std::io::Error::other(
            "boom",
        )));
        assert!(manager.load_error_status().is_some());
        manager.clear_load_error();
        assert!(manager.load_error_status().is_none());
    }
}
