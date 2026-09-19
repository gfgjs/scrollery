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
//! - [`watcher`]:文件监听 + diff 纯函数(`diff_changed_keys`)+ 启动期挂载入口。
//! - [`boot`]:启动期装配(`ConfigManager` 构造 + 启动期一次性读取的设置键)。

pub mod boot;
pub mod effects;
pub mod file;
pub mod schema;
pub mod settings;
pub mod value;
pub mod watcher;
pub mod window;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;

pub use file::{ConfigFileError, KeyWarning, LoadedConfig};
pub use schema::{SettingDef, SettingKind, SETTING_DEFS, STATE_KEYS};

/// 全部已注册设置的当前生效值 + 本进程的 revision/generation(方案 §5.1 的统一快照)。
///
/// - `values`:现行规范文本(结构类键为规范 JSON 文本)。**含 schema 默认值**——没有这个键的
///   文件(或注释态键)也会出现在这里,消费方无需自己判空、查 schema 或补默认值。
/// - `revision`:每次成功提交递增,供前端丢弃过时响应/重复事件。
/// - `generation`:只在「恢复默认设置」时递增。前端把它随每次写入回传,**旧代次写入一律被拒**,
///   从而堵住「重置后其他窗口的迟到请求把旧值写回」这条路径。
#[derive(Debug, Clone, Serialize)]
pub struct SettingsSnapshot {
    pub values: BTreeMap<String, String>,
    pub revision: u64,
    pub generation: u64,
}

/// 一次设置提交(用户保存、窗口几何、外部编辑、完全重置)的统一结果。
///
/// `config-file-changed` 事件与三个写入 IPC 都发这一份结构:前端只应用快照、按 `keys` 更新
/// 相关界面、按 `restart_required` 提示重启、按 `apply_failed` 提示「已保存但未全部生效」。
#[derive(Debug, Clone, Serialize)]
pub struct SettingsChange {
    pub snapshot: SettingsSnapshot,
    /// 本次真正变了的键(按 schema 顺序);空表示无实质变化(如仅改注释/排版)。
    pub keys: Vec<String>,
    /// `keys` 中需重启应用才生效的子集。
    pub restart_required: Vec<String>,
    /// 已落盘但运行时影响应用失败的键:值已是新的,但相关功能要等下次触发或重启才跟上。
    /// **不得**把这类结果报成「完全成功」,也不得报成「保存失败」。
    pub apply_failed: Vec<String>,
}

/// 批量提交的结果(不含快照——快照由上层在提交后统一构造,避免同一批里两次读表)。
#[derive(Debug, Default, Clone)]
pub struct CommitOutcome {
    pub keys: Vec<String>,
    pub restart_required: Vec<String>,
}

/// 提交被拒的原因。
#[derive(Debug)]
pub enum CommitError {
    /// 调用方携带的 generation 已过期(通常是「恢复默认设置」之后其他窗口的迟到回写)。
    StaleGeneration { expected: u64, current: u64 },
    /// 写盘失败:内存值与磁盘内容都保持提交前的状态。
    Write(ConfigFileError),
}

/// 进程级提交门:从「校验后的提交」到「广播完成」整段串行,外部重读与全量重置共用同一道门。
///
/// 放在本模块而非编排层,是因为它守护的资源就是 ConfigManager:凡是要同时改「磁盘 + 内存 + 运行时
/// 影响」三者的操作,都必须在同一道门内完成,否则会出现「A 已写完盘、副作用还没跑完,B 又写完盘」
/// 这类交错。以 tokio Mutex 跨 .await 持有;盘上的阻塞工作仍在 spawn_blocking 里用 std 锁
/// (不跨 .await 持 std 锁,硬约束)。
pub(crate) static COMMIT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 外部重读失败:原始错误已记进 ConfigManager(供 get_config_status 展示),此处只带出面向用户
/// 的说明与行号。这样调用方无须持有错误本体就能如实回报,也不会把内部错误类型透到更外层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadFailure {
    pub message: String,
    pub line: Option<usize>,
}

/// 把「文件中出现的实值表」补成「全部已注册键的生效值表」(缺的键取 schema 默认值)。
/// 与 `ConfigManager::get` 同一口径,供快照、批量提交与 watcher 差异比较共用。
pub fn effective_values(raw: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    SETTING_DEFS
        .iter()
        .map(|def| {
            let v = raw
                .get(def.key)
                .map_or_else(|| def.default.to_string(), Clone::clone);
            (def.key.to_string(), v)
        })
        .collect()
}

/// 比较两份**生效值表**(均为全部键齐备),算出真正变了的键与其中需重启的子集。
/// 按 `SETTING_DEFS` 顺序输出,保证同一批变更的事件载荷稳定可比。
pub fn diff_effective(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> CommitOutcome {
    let mut out = CommitOutcome::default();
    for def in SETTING_DEFS {
        let old_v = old.get(def.key).map_or(def.default, String::as_str);
        let new_v = new.get(def.key).map_or(def.default, String::as_str);
        if old_v != new_v {
            out.keys.push(def.key.to_string());
            if !def.hot {
                out.restart_required.push(def.key.to_string());
            }
        }
    }
    out
}

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
    /// 每次成功提交(含重置)递增,供前端丢弃过时响应与重复事件。只在持有 `values` 写锁时递增,
    /// 故 `snapshot` 持读锁读到的 revision 与值表必属同一次提交(不会出现「新版本号配旧值」)。
    revision: AtomicU64,
    /// 只在「恢复默认设置」时递增;写入请求携带它,旧代次一律被拒。
    generation: AtomicU64,
}

impl ConfigManager {
    /// 从磁盘加载(文件不存在则先以当前模板生成),返回门面 + 加载期警告(供 A2 转发到
    /// 设置页展示)。
    ///
    pub fn load_or_init(path: PathBuf) -> Result<(Self, Vec<KeyWarning>), ConfigFileError> {
        if !path.exists() {
            file::write_atomic(&path, &file::render_template(&BTreeMap::new()))?;
        }
        let loaded = file::load(&path)?;

        let manager = Self {
            path,
            values: RwLock::new(loaded.values),
            last_own_fingerprint: Arc::new(Mutex::new(None)),
            last_error: RwLock::new(None),
            write_lock: Mutex::new(()),
            revision: AtomicU64::new(0),
            generation: AtomicU64::new(0),
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
            revision: AtomicU64::new(0),
            generation: AtomicU64::new(0),
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
    #[cfg(test)]
    pub fn set_and_persist(&self, key: &str, value: &str) -> Result<(), ConfigFileError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut doc = self.read_doc_for_edit()?;
        file::set_value(&mut doc, key, value);
        let content = doc.to_string();
        let fp = file::write_atomic(&self.path, &content)?;
        {
            let mut values = self
                .values
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            values.insert(key.to_string(), value.to_string());
            // 单键写也要递增 revision:快照的 revision 必须对**所有**写路径单调,否则「后端内部
            // 同步改写设置」(ai_active_model / proofread_* / viewer_color_*)之后前端拿到的
            // revision 会停住不动,过时响应的判定随之失效。
            self.revision.fetch_add(1, Ordering::Release);
        }
        *self
            .last_own_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
        Ok(())
    }

    /// 重置某键为默认值:删除文件中的实值行(该键从此按注释态处理,`get` 回退 schema 默认值)。
    /// 同 `set_and_persist` 持 `write_lock` 串行化整个读改写序列(理由同上)。
    #[cfg(test)]
    pub fn reset_key(&self, key: &str) -> Result<(), ConfigFileError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut doc = self.read_doc_for_edit()?;
        file::unset_value(&mut doc, key);
        let content = doc.to_string();
        let fp = file::write_atomic(&self.path, &content)?;
        {
            let mut values = self
                .values
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            values.remove(key);
            self.revision.fetch_add(1, Ordering::Release);
        }
        *self
            .last_own_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // ── 统一快照与代次(方案 §5.1)────────────────────────────────────────────

    /// 当前 revision(内存读,零 IO;可在 async 上下文中随时调用)。
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// 当前 generation(内存读,零 IO)。写入请求须回传它;重置后旧值会被拒。
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// 全部已注册键的生效值(缺省键补 schema 默认值)。
    pub fn effective_values(&self) -> BTreeMap<String, String> {
        let guard = self
            .values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effective_values(&guard)
    }

    /// 统一快照:生效值 + revision + generation。
    ///
    /// 一致性:提交方在**持有 `values` 写锁期间**更新值表并递增 revision/generation,本函数持读锁
    /// 期间读取三者,故不会出现「新 revision 配旧值」或反之(那会让前端误判自己已是最新)。
    pub fn snapshot(&self) -> SettingsSnapshot {
        let guard = self
            .values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let values = effective_values(&guard);
        let revision = self.revision.load(Ordering::Acquire);
        let generation = self.generation.load(Ordering::Acquire);
        SettingsSnapshot {
            values,
            revision,
            generation,
        }
    }

    // ── 批量提交与全量重置(方案 §5.1/§5.3)──────────────────────────────────

    /// 一次提交多个键:**只读改写一次 TOML、只落盘一次**。
    ///
    /// - 基于磁盘最新文档合入(不用可能过期的内存快照覆盖文件,避免抹掉其他写入方刚落的键)。
    /// - `expected_generation` 与当前不一致即整批拒绝(不写盘、不改内存值)。
    /// - 失败时文件与内存值都保持提交前状态(写盘是 tmp + rename,故不存在「写了一半」)。
    pub fn commit_batch(
        &self,
        patch: &BTreeMap<String, String>,
        expected_generation: u64,
    ) -> Result<CommitOutcome, CommitError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ensure_generation(expected_generation)?;
        self.write_patch_locked(patch).map_err(CommitError::Write)
    }

    /// 结构映射类键的**单条目标签**更新:在写锁内基于当前生效值合并,只改动 `sub_key` 一项,
    /// 其他标签原样保留。窗口几何据此提交——若由调用方先读出整表再整表写回,两个窗口的并发
    /// 提交会互相覆盖。
    pub fn commit_struct_map_entry(
        &self,
        key: &str,
        sub_key: &str,
        sub_value_json: &str,
        expected_generation: u64,
    ) -> Result<CommitOutcome, CommitError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ensure_generation(expected_generation)?;

        let Some(def) = SettingDef::find(key) else {
            return Err(CommitError::Write(ConfigFileError::UnknownKey(
                key.to_string(),
            )));
        };
        // 合并基准取自**磁盘最终文档**而非内存:内存里这份映射可能缺少其他窗口/写入方刚落下的
        // 条目(见 write_patch_locked 的说明),以内存为基准合并会把它们抹掉。
        let current = self
            .canonical_from_disk_locked(def)
            .map_err(CommitError::Write)?;
        let merged = value::struct_map_upsert(def, &current, sub_key, sub_value_json)
            .map_err(CommitError::Write)?;
        let mut patch = BTreeMap::new();
        patch.insert(key.to_string(), merged);
        self.write_patch_locked(&patch).map_err(CommitError::Write)
    }

    /// 恢复默认设置:用 schema 生成的全默认模板**原子替换**整份 config.toml,并把内存值表清空
    /// (全部键回退 schema 默认值)。
    ///
    /// - 显式重置**允许替换语法损坏的文件**(不先解析它),这正是「坏文件也能重置」的实现方式。
    /// - 只动 config.toml:数据库里的内部状态(首次引导标记、任务启停标志等)不参与重置。
    /// - generation 递增,使重置前的在途写入全部失效。
    /// - 手写注释会被恢复成模板注释(方案 §7 第 3 步的既有约定)。
    pub fn reset_all(&self) -> Result<CommitOutcome, ConfigFileError> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let old = self.effective_values();
        let content = file::render_template(&BTreeMap::new());
        let fp = file::write_atomic(&self.path, &content)?;
        {
            let mut values = self
                .values
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            values.clear();
            // 重置后的首份内容即本进程所写,指纹同步以避免 watcher 把自己的写盘再当外部编辑重载。
            *self
                .last_own_fingerprint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
            self.generation.fetch_add(1, Ordering::Release);
            self.revision.fetch_add(1, Ordering::Release);
        }
        // 坏文件已被模板替换,之前的加载错误不再成立。
        self.clear_load_error();
        let new = self.effective_values();
        Ok(diff_effective(&old, &new))
    }

    fn ensure_generation(&self, expected: u64) -> Result<(), CommitError> {
        let current = self.generation();
        if expected == current {
            Ok(())
        } else {
            Err(CommitError::StaleGeneration { expected, current })
        }
    }

    /// 已持 `write_lock` 的写盘实现:读最新文档 → 逐键改写 → 原子写盘 → 更新内存值表 →
    /// 递增 revision → 返回本次真正变了的键。
    fn write_patch_locked(
        &self,
        patch: &BTreeMap<String, String>,
    ) -> Result<CommitOutcome, ConfigFileError> {
        let mut doc = self.read_doc_for_edit()?;
        for (key, val) in patch {
            file::set_value(&mut doc, key, val);
        }
        let content = doc.to_string();
        let fp = file::write_atomic(&self.path, &content)?;
        // 生效值表**从最终文档重建**,而不是只把本次 patch 的键塞进内存。
        //
        // 理由:写盘时读回的是磁盘最新文档,里面可能含有本进程内存里还没有的键——例如外部编辑刚写
        // 进去、watcher 的重读尚未轮到(或那次重读被自写指纹跳过)的键,以及别的写入方刚落的值。
        // 若内存只插本批的键,这些键会一直缺席,磁盘与快照长期背离,直到下一次全量重读才补上;而
        // 「快照是前端唯一真源」的前提正是两者一致。重建同时也让本批的差异比较覆盖这些真实变化。
        let loaded = file::validate_doc(doc);
        for warning in &loaded.warnings {
            tracing::warn!(
                key = %warning.key,
                message = %warning.message,
                "config.toml 提交后发现键警告(已跳过该键) | key warning after commit, skipped"
            );
        }
        *self
            .last_own_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fp);
        Ok(self.replace_loaded_values(loaded.values))
    }

    /// 取某键在**磁盘最终文档**里的规范值(文件里没有该键则取 schema 默认值)。
    /// 调用方必须已持有 `write_lock`。用于结构映射类键的单条目合并:内存值可能落后于磁盘
    /// (见 `write_patch_locked` 的说明),以内存为准合并会抹掉其他写入方刚落的其他条目。
    fn canonical_from_disk_locked(&self, def: &SettingDef) -> Result<String, ConfigFileError> {
        let doc = self.read_doc_for_edit()?;
        match doc.get(def.key) {
            Some(item) => value::item_to_canonical(def, item)
                .map(|parsed| parsed.text)
                .map_err(ConfigFileError::InvalidValue),
            None => Ok(def.default.to_string()),
        }
    }

    /// A2:取当前生效值表的快照(仅含文件中确实出现的实值键,与 `LoadedConfig.values` 同型)。
    /// 供 watcher 回调在替换前先取旧值算 diff(`watcher::diff_changed_keys`)。
    pub fn raw_values_snapshot(&self) -> BTreeMap<String, String> {
        self.values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 外部编辑重载:整表替换生效值并递增 revision,返回本次真正变化的键。
    ///
    /// 为什么不复用 `replace_values`(仅替换、不递增):revision 是前端丢弃过时响应的唯一依据,
    /// 若外部编辑改了值而 revision 不动,前端拿到的新快照看起来和旧的一样「新」,过时判定随之失效。
    /// 不改指纹锁——那是「本进程自写」的记录,外部编辑走的正是「指纹不匹配」分支。
    pub fn replace_loaded_values(&self, values: BTreeMap<String, String>) -> CommitOutcome {
        let mut guard = self
            .values
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let old = effective_values(&guard);
        *guard = values;
        let new = effective_values(&guard);
        self.revision.fetch_add(1, Ordering::Release);
        diff_effective(&old, &new)
    }

    /// 从磁盘重读整份配置(外部编辑的唯一应用路径)。**调用方必须已持有提交门**。
    ///
    /// 两个要点:
    /// - **函数自己读盘**,不接受调用方「之前读到的一份内容」。watcher 在防抖窗口里读到的东西可能
    ///   已经过期(用户又存了一次,或期间发生了重置),把那份旧内容带进门就等于让旧快照覆盖新状态
    ///   ——这正是「重置后旧值复活」的来路。故外部编辑只提供「需要重读」这一个信号,真正的读取发生
    ///   在门内、拿到锁之后。
    /// - 读取失败(语法错/IO 错)时**只记录错误、保留当前生效值**:较新的有效状态不会被一次过期或
    ///   失败的读取抹掉。调用方据此广播错误事件。
    pub fn reload_from_disk(&self) -> Result<CommitOutcome, ReloadFailure> {
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match file::load(&self.path) {
            Ok(loaded) => {
                let outcome = self.replace_loaded_values(loaded.values);
                self.clear_load_error();
                Ok(outcome)
            }
            Err(e) => {
                let (message, line) = e.to_status_message();
                self.record_load_error(e);
                Err(ReloadFailure { message, line })
            }
        }
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

    #[test]
    fn load_or_init_creates_file_and_get_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, warnings) = ConfigManager::load_or_init(path.clone()).unwrap();
        assert!(warnings.is_empty());
        assert!(path.exists());
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512")); // schema 默认值
        assert_eq!(manager.get("no_such_key"), None);
    }

    #[test]
    fn set_and_persist_then_get_reflects_new_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path).unwrap();
        manager.set_and_persist("thumb_size", "128").unwrap();
        assert_eq!(manager.get("thumb_size").as_deref(), Some("128"));
    }

    #[test]
    fn reset_key_falls_back_to_default_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path).unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path).unwrap();
        manager.record_load_error(super::file::ConfigFileError::Io(std::io::Error::other(
            "boom",
        )));
        assert!(manager.load_error_status().is_some());
        manager.clear_load_error();
        assert!(manager.load_error_status().is_none());
    }

    // ── 生效值表与差异比较(提交、外部编辑、重置三条路径共用)────────────────────────

    /// 「注释态回默认值」与「显式写回同默认值」等价于无变化:按解析后的生效值比较,不按「键是否
    /// 出现在表里」比较——否则用户把某个键注释掉就会被报成一次变更。
    #[test]
    fn diff_treats_explicit_default_same_as_commented_default() {
        let old = BTreeMap::new(); // thumb_size 缺省 → schema 默认 512
        let mut new = BTreeMap::new();
        new.insert("thumb_size".to_string(), "512".to_string()); // 显式写回默认值
        let outcome = diff_effective(&effective_values(&old), &effective_values(&new));
        assert!(
            !outcome.keys.contains(&"thumb_size".to_string()),
            "写回默认值不应被报成变更:{:?}",
            outcome.keys
        );
    }

    /// hot 键变化只进 keys;cold 键变化同时进 restart_required(对称于 SettingDef.hot)。
    #[test]
    fn diff_splits_restart_required_by_hot_flag() {
        let old = BTreeMap::new();
        let mut new = BTreeMap::new();
        new.insert("thumb_size".to_string(), "256".to_string()); // hot
        new.insert("log_dir".to_string(), "D:/logs".to_string()); // cold
        let outcome = diff_effective(&effective_values(&old), &effective_values(&new));
        assert!(outcome.keys.contains(&"thumb_size".to_string()));
        assert!(!outcome.restart_required.contains(&"thumb_size".to_string()));
        assert!(outcome.keys.contains(&"log_dir".to_string()));
        assert!(outcome.restart_required.contains(&"log_dir".to_string()));
    }

    // ── 批量提交 ─────────────────────────────────────────────────────────────

    /// 多键提交只落盘一次,且返回的变化键就是全部实际变化的键。
    #[test]
    fn commit_batch_persists_all_keys_in_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();
        let revision_before = manager.revision();
        let patch = BTreeMap::from([
            ("thumb_size".to_string(), "256".to_string()),
            ("log_level".to_string(), "debug".to_string()),
        ]);
        let outcome = manager.commit_batch(&patch, 0).unwrap();
        assert_eq!(outcome.keys.len(), 2);
        assert_eq!(
            manager.revision(),
            revision_before + 1,
            "一次批量提交只应递增一次 revision"
        );

        // 重新加载磁盘,确认两个键都已落盘。
        let reloaded = super::file::load(&path).unwrap();
        assert_eq!(
            reloaded.values.get("thumb_size").map(String::as_str),
            Some("256")
        );
        assert_eq!(
            reloaded.values.get("log_level").map(String::as_str),
            Some("debug")
        );
    }

    /// 写盘路径不做二次加工:内存值表里存的就是调用方给的规范文本(规范化是上层编排的职责)。
    #[test]
    fn commit_batch_stores_value_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = ConfigManager::load_or_init(dir.path().join("config.toml")).unwrap();
        let patch = BTreeMap::from([("language".to_string(), "en-US".to_string())]);
        manager.commit_batch(&patch, 0).unwrap();
        assert_eq!(manager.get("language").as_deref(), Some("en-US"));
    }

    // ── generation:重置之后旧写入必须被拒 ──────────────────────────────────────

    /// 重置递增 generation;携带旧代次的提交被拒、且**不写盘也不改内存值**。
    /// 这是「重置后其他窗口的迟到回写不得复活旧值」的核心防线。
    #[test]
    fn stale_generation_write_is_rejected_after_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();

        // 用户先把缩略图尺寸改成 256,并记下当时的代次(前端每个窗口都会带着它)。
        manager
            .commit_batch(
                &BTreeMap::from([("thumb_size".to_string(), "256".to_string())]),
                0,
            )
            .unwrap();
        let stale_generation = manager.generation();

        // 恢复默认设置。
        manager.reset_all().unwrap();
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512"));
        assert_eq!(
            manager.generation(),
            stale_generation + 1,
            "重置必须递增 generation"
        );

        // 重置前已发出的迟到请求带着旧代次回来:必须被拒,且不得把 256 写回。
        let late = BTreeMap::from([("thumb_size".to_string(), "256".to_string())]);
        match manager.commit_batch(&late, stale_generation) {
            Err(CommitError::StaleGeneration { expected, current }) => {
                assert_eq!(expected, stale_generation);
                assert_eq!(current, stale_generation + 1);
            }
            other => panic!("旧代次写入应被拒,实得 {other:?}"),
        }
        assert_eq!(
            manager.get("thumb_size").as_deref(),
            Some("512"),
            "被拒的写入不得改内存值"
        );
        let on_disk = super::file::load(&path).unwrap();
        assert!(
            !on_disk.values.contains_key("thumb_size"),
            "被拒的写入不得落盘:{:?}",
            on_disk.values
        );
    }

    /// 携带当前代次的写入正常通过(判据是代次相等,不是「重置过就一律拒绝」)。
    #[test]
    fn current_generation_write_succeeds_after_reset() {
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = ConfigManager::load_or_init(dir.path().join("config.toml")).unwrap();
        manager.reset_all().unwrap();
        let current = manager.generation();
        manager
            .commit_batch(
                &BTreeMap::from([("thumb_size".to_string(), "256".to_string())]),
                current,
            )
            .unwrap();
        assert_eq!(manager.get("thumb_size").as_deref(), Some("256"));
    }

    // ── 重置 ────────────────────────────────────────────────────────────────

    /// 重置把键恢复为 schema 默认值、清掉加载错误、递增 revision/generation,并且**允许替换语法
    /// 损坏的文件**(不先解析它)。
    #[test]
    fn reset_all_restores_defaults_even_from_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();
        let revision_before = manager.revision();

        manager
            .commit_batch(
                &BTreeMap::from([
                    ("thumb_size".to_string(), "256".to_string()),
                    ("ui_font_size".to_string(), "18".to_string()),
                ]),
                0,
            )
            .unwrap();
        // 用户把文件写坏(TOML 语法错)。
        std::fs::write(&path, "thumb_size = [1, 2\n").unwrap();
        assert!(manager.reload_from_disk().is_err(), "坏文件应加载失败");
        assert!(manager.load_error_status().is_some());

        // 明确重置:替换坏文件为默认模板。
        let outcome = manager.reset_all().unwrap();
        assert!(outcome.keys.contains(&"thumb_size".to_string()));
        assert_eq!(manager.get("thumb_size").as_deref(), Some("512"));
        assert_eq!(manager.get("ui_font_size").as_deref(), Some("13"));
        assert!(
            manager.load_error_status().is_none(),
            "重置后不应还留着旧文件的加载错误"
        );
        assert!(manager.revision() > revision_before);

        // 磁盘上是一份能被正常重新加载的默认模板。
        let reloaded = super::file::load(&path).unwrap();
        assert!(reloaded.warnings.is_empty(), "{:?}", reloaded.warnings);
    }

    /// 重置**只动配置文件**:内部状态键不在 schema 内,故重置既不读也不写它们
    /// (「重置不重放引导、不改变任务启停意图」在配置层的表述)。
    #[test]
    fn reset_all_touches_only_settings_keys() {
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = ConfigManager::load_or_init(dir.path().join("config.toml")).unwrap();
        let outcome = manager.reset_all().unwrap();
        for key in outcome.keys {
            assert!(
                SettingDef::find(&key).is_some(),
                "{key} 不在设置清单内,重置不该触碰它"
            );
            assert!(
                !super::schema::STATE_KEYS.contains(&key.as_str()),
                "{key} 是内部状态键,配置重置不得涉及"
            );
        }
    }

    // ── 外部重读:函数自己读盘,不接受调用方带进来的旧内容 ─────────────────────────

    /// 外部编辑写入磁盘后,reload_from_disk 读到的就是磁盘当下的内容。
    #[test]
    fn reload_from_disk_reads_current_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();
        std::fs::write(&path, "thumb_size = 128\n").unwrap();
        let outcome = manager.reload_from_disk().unwrap();
        assert_eq!(outcome.keys, vec!["thumb_size".to_string()]);
        assert_eq!(manager.get("thumb_size").as_deref(), Some("128"));
    }

    /// 回归:先读到旧文件 → 期间发生重置 → 重读发生在门内。
    /// **旧内容不得复活**:重读函数自己去读磁盘,拿到的是重置后的默认模板,重置前的
    /// thumb_size=256 不会被写回。
    #[test]
    fn reload_after_reset_does_not_resurrect_old_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();

        // 用户把设置改成 256(这就是「旧文件内容」)。
        manager
            .commit_batch(
                &BTreeMap::from([("thumb_size".to_string(), "256".to_string())]),
                0,
            )
            .unwrap();

        // 重置:文件被默认模板替换,generation 递增。
        manager.reset_all().unwrap();
        let disk_after_reset = std::fs::read_to_string(&path).unwrap();
        assert!(
            !disk_after_reset.contains("thumb_size = 256"),
            "重置后的文件不应还留着 256"
        );

        // 外部编辑信号到来(可能来自重置之前的保存动作):重读在门内进行、读到重置后的文件,
        // 故不会把 256 复活。
        let outcome = manager.reload_from_disk().unwrap();
        assert!(
            outcome.keys.is_empty(),
            "重置后的重读不应产生变更:{:?}",
            outcome.keys
        );
        assert_eq!(
            manager.get("thumb_size").as_deref(),
            Some("512"),
            "重置前的值不得被外部重读复活"
        );
    }

    /// 语法错的读取**不改动当前生效值**:较新的有效状态不会被一次失败的重读抹掉。
    #[test]
    fn failed_reload_keeps_effective_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (manager, _) = ConfigManager::load_or_init(path.clone()).unwrap();
        manager
            .commit_batch(
                &BTreeMap::from([("thumb_size".to_string(), "256".to_string())]),
                0,
            )
            .unwrap();
        std::fs::write(&path, "this is not toml = = =\n").unwrap();
        assert!(manager.reload_from_disk().is_err());
        assert_eq!(
            manager.get("thumb_size").as_deref(),
            Some("256"),
            "重读失败时生效值必须保持"
        );
        assert!(manager.load_error_status().is_some());
    }
}
