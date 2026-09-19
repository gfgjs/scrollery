---
id: 2026-07-24-Spec12_配置状态日志
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec12-配置状态日志

> 本篇讲 Scrollery 后端的三块基建:配置子系统(`src-tauri/src/config/`)、全局应用状态
> (`src-tauri/src/state.rs`)、日志能力(`src-tauri/src/logging.rs`)。服务对象是需要新增
> 设置项、新增后台流水线取消令牌槽、或排查日志/诊断问题的工程师与代理。读前无需先读其他
> 篇;本篇是 as-built 现状描述,理由链见文末「7. 关联」。

## 1. 概览

- **配置子系统**:管理用户可配置设置(界面/画廊/缩略图/AI/阅读器/日志/备份等,全量键以
  `SETTING_DEFS` 为准)。全部全局用户偏好集中保存于 `<app_data_dir>/config.toml`,前端经
  `get_settings_snapshot` / `set_app_settings` / `clear_settings` 批量读写(§3.1)。
  config.toml 是 TOML 文本文件、带中文行内说明,支持外部编辑器直接改、改后热加载生效;另有
  一组"状态类键"(应用自己记账、非用户表单可编辑,如 `schema_version`/`first_launch`)
  仍留 SQLite `app_config` 表。
- **全局状态**:`AppState` 是贯穿全部 Tauri 命令的单例(`Arc<AppState>` 经 `app.manage()`
  注入),持有 DB 连接池、各后台流水线的取消令牌槽(`RunTokenSlot`)、多级缓存、GPU/文件任务
  互斥门闩等。
- **日志能力**:基于 `tracing`/`tracing-subscriber` 的结构化 JSONL 日志内核,含运行期级别
  热切换、独立日志窗口的内存环形缓冲实时流、span 手动计时、诊断包导出(脱敏)、worker 子
  进程日志汇入主日志体系。

**代码位置**:

| 文件 | 职责 |
|---|---|
| `src-tauri/src/config/mod.rs` | `ConfigManager` 门面:加载/读取/读改写落盘/watcher 挂载 |
| `src-tauri/src/config/settings.rs` | 设置统一入口编排:提交门、批量提交、外部重读、恢复默认、广播 |
| `src-tauri/src/config/schema.rs` | 键定义单源 `SETTING_DEFS` + 状态键清单 `STATE_KEYS` |
| `src-tauri/src/config/file.rs` | TOML 读写:语法/类型校验、模板渲染、原子写盘、指纹 |
| `src-tauri/src/config/watcher.rs` | 文件监听(`notify`)+ 防自写回环 + diff 纯函数 |
| `src-tauri/src/state.rs` | `AppState` 结构体 + `RunTokenSlot` 通用取消令牌槽范式 |
| `src-tauri/src/logging.rs` | 日志内核:JSONL 信封格式化器、环形缓冲层、`SpanTimer`、脱敏 |
| `src-tauri/src/ipc/config_commands.rs` | 配置/状态类 IPC 命令(设置快照、批量提交、恢复默认、启动批、配置状态与路径查询等)+ `LOG_RELOAD` 静态句柄 |
| `src-tauri/src/ipc/log_commands.rs` | 6 条日志类 IPC 命令(日志窗口/历史分页/诊断包) |
| `src-tauri/src/ipc/system_commands.rs` | `log_frontend_events`:前端日志桥接入口(system 分类下) |
| `src-tauri/src/exotic/supervisor.rs` | worker 子进程 stderr 逐行日志转发(`forward_stderr_line`) |
| `crates/exotic-protocol/src/stderr_log.rs` | `WorkerLogLine` 行协议(worker 侧序列化) |

**在整机中的位置**(ASCII 图,箭头=调用方向):

```
lib.rs::run() setup 钩子(启动序,一次性)
  ├─ ConfigManager::load_or_init(config.toml 路径)        ← config/mod.rs
  │    ├─ file::render_template() 写盘      (仅文件不存在时:当前模板初始化)
  │    └─ file::load()                       (整文件语法校验)
  ├─ tracing subscriber 建立(见 §3.3):EnvFilter(reload) + 文件层 + 环形缓冲层 [+ dev 控制台层]
  ├─ AppState::new(..., config, log_ring, log_dropped_counter, ...)
  └─ spawn_config_file_watcher(app, config_path, config_manager)  ← config/watcher.rs

前端 IPC 调用 ──▶ ipc::config_commands 设置入口(快照/批量提交/恢复默认) ──▶ config::settings ──▶ ConfigManager
前端 IPC 调用 ──▶ ipc::config_commands::{get,set}_app_config(仅内部状态键) ──▶ db::queries::config
前端 IPC 调用 ──▶ ipc::log_commands::* ──▶ AppState.log_dir / AppState.log_ring
外部编辑器改 config.toml ──▶ notify watcher(500ms debounce,只发"文件变了"信号)
                          ──▶ config::settings::reload_from_disk(提交门内重新读盘)
                          ──▶ config::effects::apply_batch (热应用)
                          ──▶ emit("config-file-changed", 完整快照)
tracing::info!/warn!/... (全仓任意调用点) ──▶ 全局 Subscriber ──▶ 文件层(JSONL 落盘)
                                                              ──▶ 环形缓冲层(日志窗口开启时)
exotic worker 子进程 stderr ──▶ supervisor::forward_stderr_line ──▶ tracing::info!(target="scrollery::worker")
前端 logger.ts 队列(2s/50条) ──▶ system_commands::log_frontend_events ──▶ tracing::info!(target="scrollery::frontend")
```

## 2. 数据模型与状态

### 2.1 配置键值真源

- **schema 设置类键**(`config::schema::SETTING_DEFS`):唯一真源是 `<app_data_dir>/config.toml`,
  覆盖界面、画廊、缩略图、视频、AI、人脸、冷门格式、影像增强、阅读器、日志、备份、查看器、
  校对与高级调优全部分组。**数量以该清单本身为准(本文不再硬编码计数,新增键即变化)**。每项
  `SettingDef` 含 `section`/`key`/`kind`/`default`/`comment_zh`/`hot` 六个字段。
  `hot=true` 表示改值可在运行期经效果层即时生效;`hot=false` 表示只在启动期读入一次,运行期
  改值需重启才生效(如 `log_dir`/`max_log_dir_bytes`/`log_ring_buffer_capacity`/
  `thumb_cache_max_mb`),此类键在提交回执与广播里进 `restartRequired`。
- **状态类键**(`config::schema::STATE_KEYS`):只保留内部状态/运行期标记,留在 SQLite
  `app_config` 表(`key TEXT PRIMARY KEY, value TEXT NOT NULL`,全表结构见
  [Spec01 数据层](./Spec01_数据层.md))。当前含 `schema_version`、`last_directory_id`、
  `first_launch`、`guide_seen`、`ai_gpu_name`、`ai_provider`、`exotic_paused`、
  `ai_analysis_active`、`face_analysis_active`、`derivation_active`、
  `backup_last_success_at`、`last_cover_stat_reconcile`(DAO 见
  `db/queries/config.rs` 的 `get_config`/`set_config`,单参数化查询)。
  **阅读器排版偏好已不在状态键里**:17 项 `doc_reader_*` 与 `doc_pager_mode` 等统一归入
  config.toml(见 §3.1),阅读**位置**仍属业务数据(报告进度表)。
- **未知键**:两清单外的键(用户手误 / 前端拼写错误 / 已删键)在设置与状态的写入入口一律拒绝,
  不再落 DB 兜底;设置类入口回稳定码 `config_unknown_key`(值非法则 `config_invalid_value`);
  `config.toml` 文件里出现的未知键仍只记 `KeyWarning` 并跳过,不影响其余键加载(`file.rs`)。

```
pub struct ConfigManager {
    path: PathBuf,
    values: RwLock<BTreeMap<String, String>>,      // 仅含文件中确实出现的实值键
    last_own_fingerprint: Arc<Mutex<Option<u64>>>, // 防 watcher 自写回环
    last_error: RwLock<Option<ConfigFileError>>,   // 最近一次整文件加载失败
    write_lock: Mutex<()>,                          // 串行化单次「读回文档→改写→原子写盘→更新内存/指纹」
}
```

`values` 只存"用户取消注释覆盖的键";未覆盖的键由 `get()` 回退 `SettingDef.default`
(`effective_values`),消费方无需判空/查 schema。归属:内存(`Arc<ConfigManager>` 存于
`AppState.config`),watcher 回调与 IPC 命令共享同一实例。

**两级串行化,职责不重叠**:

- `write_lock`(std `Mutex`,存于 `ConfigManager` 内,**不跨 `.await`**):把每一次
  "读回文档 → 就地改写 → 原子写盘 → 更新内存值表与写盘指纹"整个序列串起来,防并发写交错
  导致后写覆盖前写、或指纹记录与实际内容错配(`commit_batch` / `commit_struct_map_entry` / `reset_all` / `reload_from_disk` 均持它)。
  盘上的阻塞工作只在这把锁下于 `spawn_blocking` 内进行。

- `COMMIT_GATE`(声明在 `config/mod.rs` 的模块级 `tokio::sync::Mutex`,由设置编排层取用、**可跨
  `.await`**):把"取门 → 写盘 → 应用运行时效果 → 组装快照 → 广播"整段互斥起来,使 UI 提交、
  外部文件重读与全量重置三者不会交错,并让重置的"暂停采集 → 替换文件 → 恢复采集"不被插入。

两者不可互相替代:少了 `write_lock`,单次读改写序列本身就可能被并发写撕开;少了
`COMMIT_GATE`,效果应用与广播会与其他入口的写入交错。

### 2.3 `AppState`(`src-tauri/src/state.rs:27-274`)

全局单例,`Arc<AppState>` 经 `app.manage()` 注入 Tauri 状态容器,贯穿全部 `#[tauri::command]`。
本篇只列与配置/状态/日志直接相关的字段(完整字段清单见文件本身,总计约 40 个字段,涵盖 DB
连接、AI/人脸/派生/backup/export/exotic 等全部子系统的运行期状态):

| 字段 | 类型 | 归属 | 用途 |
|---|---|---|---|
| `config` | `Arc<ConfigManager>` | 内存(镜像 config.toml) | schema 设置类键读写门面 |
| `log_dir` | `PathBuf` | 内存(启动期解析一次) | 日志文件所在目录 |
| `log_ring` | `Arc<LogRingBuffer>` | 内存 | 日志窗口实时流环形缓冲 |
| `log_dropped_counter` | `tracing_appender::non_blocking::ErrorCounter` | 内存 | non_blocking 丢弃行计数 |
| `app_data_dir` | `PathBuf` | 内存 | 应用数据根,models/documents 等均从此派生 |
| `thumb_gen_token`/`ai_analysis_token`/`face_analysis_token`/`derivation_token`/`backup_token`/`export_token` | `RunTokenSlot` | 内存 | 各后台流水线的取消令牌槽(§2.4) |
| `data_version` | `AtomicU64` | 内存 | 画廊视图失效版本号(与配置/日志无直接关系,仅作为 `AppState` 结构示例列出) |

`AppState::new()` 的构造参数(`state.rs:394-411`)里,`config`/`log_dir`/`log_ring`/
`log_dropped_counter`/`app_data_dir` 均是"外部先算好再传入"——因为 `log_ring`/
`log_dropped_counter` 在 tracing 全局 subscriber 建立时(早于 `AppState::new()` 调用)就已产生
(`lib.rs:479, 505`),不在构造函数内部现建。

### 2.4 `RunTokenSlot`(`state.rs:317-389`)

后台流水线取消令牌槽的通用范式,全仓当前 6 个槽实例(`thumb_gen_token`/`ai_analysis_token`/
`face_analysis_token`/`derivation_token`/`backup_token`/`export_token`)均基于此类型:

```
pub struct RunTokenSlot {
    generation: AtomicU64,                          // 单调递增运行代次
    slot: Mutex<Option<(u64, CancellationToken)>>,   // 当前轮 (代次, token)
}
```

四个方法(`state.rs:332-382`):

- `begin() -> (generation, token)`:安装新一轮,不取消旧轮(调用方需先显式 `cancel`)。
- `cancel()`:take + cancel 槽内任意轮次(用户停止 / 新一轮启动前)。
- `finish(generation) -> bool`:完成回调的 compare-and-clear——槽内仍是本轮代次则清空返回
  `true`;槽已空返回 `true`(用户已显式 stop,本轮仍持"发布终态"的权力);槽被更新一轮占用
  返回 `false`(旧轮收尾不得再动全局状态)。
- `is_running() -> bool` / `is_current(generation) -> bool`:只读判定。

**为什么需要代次**:没有代次时,"停止→立即重启"的竞态会让旧轮迟到的完成回调 take+cancel
新一轮刚安装的 token,静默中断刚重启的运行(2026-07-10 审查 F10 首次发现,2026-07-18 审查
F-01/F-02 在 thumb/derive 复现同款问题后,才把该纪律沉淀为本可复用类型)。

### 2.5 日志行结构(信封,`logging.rs:268-284`)

全部 JSONL 落盘行与环形缓冲行共用同一套字段收集逻辑(`build_envelope`,防两处手写漂移):

```json
{
  "ts": "2026-07-24T10:00:00.000+08:00",
  "level": "INFO",
  "target": "scrollery::span",
  "session_id": "s-7f2a9c1e",
  "operation_id": null,
  "msg": "span_close",
  "attributes": { "...": "..." }
}
```

- `ts`/`level`/`target`/`session_id`/`operation_id`/`msg` 六字段固定在信封顶层;领域字段进
  `attributes`。`operation_id`/`error_chain`/`error_code`/`frontend_context`/`worker_context`
  五个字段名在 `FieldCollector::record_value`(`logging.rs:199-244`)有特判——后两者解码回真
  JSON 对象存进 `attributes.context`,而非原样字符串。
- `WorkerLogLine` 行协议(`crates/exotic-protocol/src/stderr_log.rs:16-22`):worker 子进程侧
  单行 JSON 写 stderr 的格式,`{ lvl, msg, fields }` 三字段,`fields` 省略即空对象。

## 3. 关键流程与算法

### 3.1 配置:加载 / 热加载 / 写入

**启动期加载**(`ConfigManager::load_or_init`):

1. 文件不存在 → file::render_template() 渲染当前模板并原子写盘(当前格式初始化)
2. loaded = file::load(path)  // 语法错 → Err,调用方保留上次成功配置不触碰磁盘
3. 构造 ConfigManager { values: loaded.values, ... };未出现的键由 get() 回退 schema 默认值

**语法错时的降级**(`ConfigManager::degraded`):不写盘、不覆盖用户的坏文件,全部键回退
`SettingDef.default`,记录错误供 `get_config_status` IPC 展示;应用照常
启动(硬约束:绝不因配置文件写错而拒绝启动)。

**外部编辑热加载**(`config::watcher::spawn_config_watcher` → `config::settings::reload_from_disk`):

```
notify 监听父目录(按文件名过滤,防 rename 替换保存丢监听)
  → 收到事件 push 进 unbounded channel(同步回调线程,只做这一件轻量事)
  → tokio task:500ms 空闲去抖,合并保存产生的多条文件事件
  → spawn_blocking 计算当前文件指纹,命中自写指纹则消费指纹并跳过
  → 否则回调只发「需要重读」信号,不携带读取内容
  → config::settings::reload_from_disk:在提交门内 spawn_blocking 重新读取并解析文件
  → diff 出真正变化的键 → 应用本批效果 → 广播完整快照
```

**为什么在门内重新读盘**:防抖窗口里早先读到的那份文件可能已经过期,带着它进门会让旧快照覆盖
新状态(重置后的旧值复活即由此而来)。重读失败只记录并广播 `config-file-error`,当前生效值保持
不变;成功后按整批差异统一应用运行时影响,并向所有窗口广播同一份完整快照。

**集中保存的读写入口**(`config::settings`,`ipc::config_commands` 暴露):

```
get_settings_snapshot  → SettingsSnapshot{values, revision, generation}(全量生效值,内存读零 IO)
set_app_settings       → {patch, generation} → canonicalize_patch(整批校验/规范化)
                          → 提交门内一次写盘 → 应用本批效果 → SettingsChange 回执 + 广播
clear_settings         → 恢复默认设置(见下)
get_startup_config     → {settings: SettingsSnapshot, state: {firstLaunch, guideSeen}} 单次往返
```

- **一次 patch = 一次落盘**:键值先在内存合并,写盘只发生一次;失败则整批不落盘、报告后回滚到
  最后确认值。写盘走读回 `toml_edit::DocumentMut` 文档 → 就地改目标键 → `write_atomic`
(同卷 `*.tmp` + rename),保留其余键的注释与排版。
- **整批拒绝**:任一键非法(未知键 / 类型不符 / 越界 / 结构里出现未知字段或未登记标签)即整批
  失败,不做"跳过坏项、写入其余"的部分提交。稳定码区分两类:值类
  (`config_invalid_value`/`config_unknown_key`)重试无意义,IO 类(`config_write_failed`)
  可重试。
- **generation / revision**:`generation` 只在重置时递增,提交必须带上调用方见到的代次,旧代次
  一律被拒(防「重置后其他窗口的迟到回写把默认值覆盖回旧偏好」);`revision` 每次提交递增,
  前端据此丢弃过期回执与重复事件。
- **效果去重**:同一批里同时改多个会触发派生重启的键,只安排一次重启,不逐键反复重启流水线。

**恢复默认设置**(`config::settings::reset_settings`,`clear_settings` 命令):从 schema 生成全默认
模板原子替换 config.toml 一次;`generation` 自此递增,在途写入全部失效。**不触碰数据库**——
资产、标签、阅读/播放进度、扫描根、任务启停标志与 `first_launch`/`guide_seen` 在重置前后
保持不变,故重置**不重放首次引导、也不会把已停止的任务重新拉起**。这取代了旧的"删 app_config
清表"实现(旧实现会连带删掉任务与引导标记,且根本没重置普通设置文件)。

**退出前落盘**:后端发 `settings-flush-requested` 事件,前端 flush 在途与待保存的防抖提交后以
`settings_flush_done` 回执,后端收齐再退出——不依赖浏览器 unload 里的异步 IPC。隐藏到托盘不算
退出。窗口几何由窗口模块自己防抖合并后提交,不占前端的待保存集合。

**内部状态两路(仅状态键)**:`get_app_config`/`set_app_config` 退化为内部状态键的逐键读写
(symmetric:读走 DB 单次往返)。**传入设置类键会以稳定码拒绝**并指向上述设置入口——不再承担
用户设置的读写分流。

### 3.2 状态:后台流水线取消令牌生命周期

以 AI 分析流水线为例(`derivation`/`backup`/`export` 同构):

```
用户点"开始" → new_ai_analysis_token() → ai_analysis_token.begin() → (generation, token)
                                        → tauri::async_runtime::spawn(流水线任务, token, generation)
用户点"停止" → cancel_ai_analysis() → ai_analysis_token.cancel() → token.cancel()
流水线任务收尾 → finish_ai_analysis(generation) → compare-and-clear:
    仍是本轮 → 清槽,返回 true → 门控终态副作用(释放 GPU 槽/清 active 标志)
    已被新轮取代 → 不清槽,返回 false → 旧轮收尾不得再动全局状态
```

`ai`/`face` 两槽的终态副作用门控在 `!token.is_cancelled()`(见 `state.rs:144-146` 注释);
`thumb`/`derive`/`backup`/`export` 四槽门控在 `finish()` 的返回值上——两种姿态刻意不同,详见
`state.rs` 对应字段文档(§4 契约表有摘要)。

### 3.3 日志:subscriber 组装与运行时级别热切

**启动期建立**(`lib.rs:435-528`,按顺序):

```
1. 解析 log_dir(config 或默认 <app_data_dir>/logs) → create_dir_all
2. enforce_size_budget(log_dir, max_log_dir_bytes)   // tracing 尚未就绪,清理结果先存着待补记
3. generate_session_id()                              // 4 字节随机数,如 "s-7f2a9c1e"
4. RollingFileAppender::builder().rotation(DAILY).max_log_files(14).build(log_dir)
5. tracing_appender::non_blocking(file_appender) → (non_blocking, guard)
      guard 经 app.manage(guard) 交给 Tauri 管理(局部变量会立即 drop 致日志静默全丢)
6. EnvFilter::new(build_env_filter_directive(log_level))
7. tracing_subscriber::reload::Layer::new(env_filter) → (filter, reload_handle)
      reload_handle 存入 ipc::config_commands::LOG_RELOAD(OnceLock<Handle<EnvFilter,Registry>>)
8. file_layer = fmt::layer().event_format(EnvelopeFormat{session_id}).with_writer(non_blocking)
9. log_ring = LogRingBuffer::new(session_id, log_ring_buffer_capacity)
   ring_layer = RingBufferLayer::new(log_ring.clone())
10. registry = tracing_subscriber::registry().with(filter).with(file_layer).with(ring_layer)
    [+ dev 构建下额外 .with(控制台文本层) ]
    .init()
11. panic hook: tracing_panic::panic_hook(panic 消息作为最后一条 ERROR 落盘)
```

**运行时级别热切**(经 `config::effects::apply_batch` 的 `log_level` 效果,由设置提交、外部编辑
热重载与恢复默认共用同一条效果路径):`build_env_filter_directive(value)` 生成新 directive →
`LOG_RELOAD.get()` 取出 reload handle → `handle.modify(|f| *f = new_filter)`。
`build_env_filter_directive`(`logging.rs:97-111`)在用户选 trace/debug/info(比 warn 更啰嗦)
时追加 `,scrollery::pipeline=warn,reqwest=warn` 后缀压住四条高频流水线(缩略图/AI/人脸/派生-
视频);选 warn/error 或 off 时不追加(off 必须保持纯 `"off"`——追加后缀会让 pipeline target
在 off 态仍以 warn 放行,与"完全关闭"矛盾)。

**日志窗口实时流**(100ms 抽干任务,`lib.rs:690-703`):

```
tokio::time::interval(100ms) 循环
  → log_ring.drain()   // 空则 None,调用方跳过一次 emit(无订阅者时零广播)
  → app_handle.emit("log:batch", batch)
```

`RingBufferLayer::on_event`(`logging.rs:374-386`)先查 `subscribed` 原子标志,未订阅(日志
窗口未开)直接返回、不格式化不入队。订阅标志由 `open_log_window`(`ipc/log_commands.rs:21-45`)
建窗时置真、`WindowEvent::Destroyed` 回调置假——用窗口生命周期本身而非再发一轮 IPC,防止
"靠 beforeunload flush"在页面销毁竞态下不可靠。

**span 手动计时**(`SpanTimer`,`logging.rs:409-479`):`SpanTimer::info(name)` /
`SpanTimer::debug(name)` 构造后持有到作用域结束(可跨 `.await`),`Drop` 时发一条
`target: "scrollery::span"` 的固定字面量 `"span_close"` 事件,携带 `span_name`/`duration_ms`
(f64 毫秒)/`operation_id`/`panicked` 四个 attributes 字段。

**worker 子进程日志汇入**(`exotic::supervisor::forward_stderr_line`,
`exotic/supervisor.rs:185-202`):worker 子进程按行写 `WorkerLogLine` JSON 到 stderr →
supervisor 逐行扫描 → 能解析 → `emit_worker_log_line`(`supervisor.rs:146-180`,按 `lvl`
多臂 match 转发 tracing 事件,`fields` 经 `worker_context` 字段携带)→ 不能解析(非 JSON)→
按 WARN + `unparsed=true` 转发原文,不丢内容。

**前端日志桥**(`ipc::system_commands::log_frontend_events`,`system_commands.rs:238-243`):
前端 `logger.ts` 队列按 2s/50 条阈值或 `onerror`/`unhandledrejection` 触发,批量传入
`FrontendLogEvent[]`(`system_commands.rs:180-195`),逐条 `emit_frontend_log_event`
(`target: "scrollery::frontend"`,`context` 经 `frontend_context` 字段解回真对象)。`off` 档
的丢弃在前端侧完成(读 `configStore.logLevel`),后端收到即视为"值得记",走既有 `EnvFilter`
过滤路径。

**诊断包导出**(`export_diagnostics_package`,`ipc/log_commands.rs:349-445`):
system-info(版本/OS/架构/GPU 引擎设置值/DB 大小/扫描根数量)+ 最新日志文件尾部 2000 行
(`DIAGNOSTICS_TAIL_LINES`,脱敏后)打成 zip,写 `.tmp` 再同卷 rename。脱敏见 §4。

**线程/并发归属**:tracing subscriber 的 `on_event` 回调在**调用 `tracing::info!` 等宏的那个
线程**上同步执行(不是独立线程)——`RingBufferLayer` 因此要求极短临界区(内存 push_back +
定长裁剪,std `Mutex`)。`non_blocking` file writer 把实际磁盘 IO 卸到 tracing-appender 自建
的后台线程。`config::watcher` 的 notify 回调跑在其内部监听线程,聚合去抖跑在调用方传入的
tokio runtime handle 上(watcher 本身在 Tauri setup hook 主线程被调用,无 ambient runtime,
故显式传 handle,直接 `tokio::spawn` 会 panic)。IO 密集操作(读文件/写盘/zip 打包)均经
`tokio::task::spawn_blocking`。

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令摘要

全量清单权威见 [Spec10 IPC 与错误契约](./Spec10_IPC与错误契约.md)(命令总数以该篇为准,本文不固定计数);
本篇只摘配置/日志两族的命令名与要点。

| 命令 | 模块 | 要点 |
|---|---|---|
| `get_settings_snapshot`/`set_app_settings` | config_commands | 设置统一入口:全量快照 / 批量提交(整批校验,带 generation) |
| `clear_settings` | config_commands | 恢复默认设置:默认模板原子替换 config.toml,generation 递增,不触碰 DB |
| `get_app_config`/`set_app_config` | config_commands | 仅内部状态键的逐键读写;设置类键以稳定码拒绝并指向上述入口 |
| `get_startup_config` | config_commands | 单次往返取 `{settings 快照, state{firstLaunch, guideSeen}}`(启动用) |
| `settings_flush_done` | config_commands | 退出前落盘回执:后端发 `settings-flush-requested` 后等齐各窗口回执 |
| `get_config_status` | config_commands | config.toml 路径/是否存在/最近加载错误 |
| `open_config_file` | config_commands | 外部编辑器打开,失败回退 reveal_item_in_dir |
| `get_cache_stats` | config_commands | 缓存占用统计(与 [Spec03](./Spec03_缩略图与派生.md) 交叉) |
| `get_thumb_cache_dir`/`get_log_dir` | config_commands | 解析后绝对路径查询 |
| `open_log_window` | log_commands | 建窗即订阅环形缓冲,`Destroyed` 事件即取消订阅 |
| `list_log_files`/`read_log_file_page` | log_commands | 按修改时间降序列目录 / 绝对行号锚点分页 |
| `get_log_diagnostics` | log_commands | 丢弃行数 + 错误去重快照 |
| `compute_log_histogram` | log_commands | 按小时/天分桶(内存 SQLite 临时表) |
| `export_diagnostics_package` | log_commands | 脱敏诊断包导出 |
| `log_frontend_events` | system_commands | 前端日志批量汇入 |

错误均走 `crate::error::AppError`(`serde::Serialize`,稳定 code/variant,不泄漏内部字符串),
详见 [Spec10](./Spec10_IPC与错误契约.md)。

### 4.2 不变量清单

| 不变量 | 出处 | 违反后果 |
|---|---|---|
| **config.toml 单一真源(A1)**:全部 schema 设置类键(含阅读器排版偏好)唯一权威在 `<app_data_dir>/config.toml`;DB `app_config` 表只留内部状态/运行期标记键 | `config/schema.rs` 键分类 | 双写会导致真源分裂,谁覆盖谁不确定 |
| **一次 patch = 一次落盘,整批拒绝**:任一键非法则整批不落盘,不做"跳过坏项写其余"的部分提交 | `config/settings.rs` `canonicalize_patch` | 半批落盘会让用户意图与文件内容不一致,且难以收拾 |
| **提交必须带当前 generation**:重置后旧代次的迟到回写一律被拒;`revision` 每次提交递增供前端丢弃过期回执 | `config/settings.rs` 提交门 + 前端 `settingsPersistence` | 重置后旧窗口把默认值覆盖回旧偏好 |
| **重置不触碰数据库**:资产/进度/扫描根/任务标志/引导标记在重置前后不变,故不重放引导、不拉起已停止任务 | `config/settings.rs::reset_settings` | 用清表实现重置会清掉任务与引导标记,且根本没重置设置文件 |
| **schema 键与状态键互斥**(`SETTING_DEFS` ∩ `STATE_KEYS` = ∅) | `config/schema.rs` 单测锁定 | 路由判定产生"既是设置又是状态"的歧义键 |
| **写盘全程持 `write_lock`**(std 锁),读回文档到写指纹之间不释放 | `config/mod.rs` | 并发写交错导致后写覆盖前写、或指纹记录与实际内容错配 |
| **跨 `.await` 的编排互斥走 `COMMIT_GATE`**(tokio 锁),std `write_lock` 不得跨 `.await` 持有 | 项目硬约束(AGENTS.md) + `config/mod.rs` | std 锁跨 await 持有可能死锁/阻塞执行器;缺编排门则效果应用与广播和写入交错 |
| **`RunTokenSlot::finish` 必须按代次 compare-and-clear**,不得无条件清槽 | `state.rs:309-316` | "停止→立即重启"竞态下旧轮收尾误杀新轮 token(2026-07-10 F10 / 2026-07-18 F-01/F-02) |
| **诊断包必须对解码后的 JSON 值脱敏,不得对序列化后的原始字节跑正则** | `logging.rs:154-165` | JSON 转义会使反斜杠双写,直接对字节跑正则完全不命中(2026-07-20 reviewer 深审发现的真实 bug,已修) |
| **span 合成事件不经全局 dispatcher(D-311)**:弃 `tracing span`/`#[instrument]`/`FmtSpan`,改「手动计时 + 普通结构化事件」 | `logging.rs:403-408` | `FmtSpan` 合成的 close 事件只在 `fmt::Layer` 内部走 `FormatEvent`,`RingBufferLayer` 等自建 Layer 完全收不到 |
| **日志行不加 outcome 字段(D-315)** | 理由链见 [span 埋点与 worker 日志汇入方案](../worklogs/2026-07-21-span埋点与worker日志汇入/) | 保持 §9.1 护栏"禁动态插值进 message,时长/名称走结构化字段"精神一致,outcome 属过度设计,未在代码中留痕迹字段 |
| **`duration_ms` 保持 `f64`(非整毫秒截断)** | `logging.rs:449-452` | debug 档热路径查询常 <1ms,`as_millis()` 截断会让该档 TopN 的 totalMs/avgMs 恒 0 失真 |
| **off 档必须保持纯 `"off"`,不追加 pipeline 降噪后缀** | `logging.rs:93-94, 106-110` | 追加后缀会让 `scrollery::pipeline` target 在 off 态仍以 warn 放行,与"完全关闭"矛盾 |
| **`log_ring`/`log_dropped_counter` 是启动期读一次的字段,`hot=false`** | `config/schema.rs`(`log_ring_buffer_capacity`) | 环形缓冲挂在 tracing 全局 subscriber 上,构造后不可替换 |

### 4.3 「不可擅改」项

- **RunTokenSlot 是全仓后台流水线取消令牌唯一范式**,新增流水线取消逻辑应复用此类型,不得
  另起手写代次比较(`state.rs:309-316` 注释:F-025 已收编 ai/face 两槽)。
- **ai/face 终态副作用门控在 `!token.is_cancelled()`,thumb/derive/backup/export 门控在
  `finish()` 返回值**——两种姿态刻意不同,不得"统一"改法(见审查线红线记录,亦见
  `state.rs:144-146, 240-241` 注释)。

## 5. 边界与失败

- **配置文件损坏**(整文件 TOML 语法错):`ConfigFileError::Syntax { line, column, message }`
  带行列号(来自 `toml_edit` 的 span),`ConfigManager::degraded` 降级构造——不写盘、全部键
  回退默认值、应用照常启动,`get_config_status` 展示错误供用户在设置页定位修复。
- **单键类型不符/未知键**:不使整份配置失败,记入 `KeyWarning`,该键回退默认值,其余键照常
  生效(`config/file.rs::load`)。
- **热加载竞态(自写回环)**:`is_self_write` 比较当前磁盘内容指纹与"最近一次本进程写盘"
  指纹,命中即跳过本次 watcher 回调;命中后**一次性清空**指纹(而非长期免疫)——否则用户
  外部把文件改回与本进程最近一次写盘完全相同的字节内容,会被永久当成"自己写的"而吞掉。
- **运行期文件被外部删除**:`ConfigManager::read_doc_for_edit` 兜底退回全新模板(全默认注释
  态)而非直接报错,使编辑操作(设置提交与恢复默认的写盘)仍可完成。
- **日志目录独立崩溃隔离**:日志窗口是独立 Tauri 窗口(`webview "logs"`),其崩溃/关闭不影响
  主窗口;`RingBufferLayer` 无订阅者时零成本(不格式化不入队),日志窗口未开时对主流程性能
  无影响。
- **诊断包大小**:日志尾部截断为 `DIAGNOSTICS_TAIL_LINES = 2000` 行,不整份最新日志文件打包
  (`log_commands.rs:344`);日志目录总量另有 `MAX_LOG_DIR_BYTES = 512MB`(§7 兜底,启动期
  `enforce_size_budget` 按最旧文件优先删除)与 `RollingFileAppender` 的 `max_log_files(14)`
  两重上限(`lib.rs:465`)。
- **worker 日志乱序**:worker 子进程 stderr 逐行转发,supervisor 端不做跨行重排/去重;非 JSON
  行降级为 WARN + `unparsed=true` 原文转发,不 panic 不丢内容(`supervisor.rs:185-202`)。
- **日志级别 off 档**:顶层 `EnvFilter` 直接拦下全部事件(测试见 `logging.rs:492-545` 的
  off→info→off 全链路特征化测试),确保"完全关闭"语义真实生效,而非仅调高阈值。

## 6. 重建指引(从零实现)

**依赖顺序**(先建什么后建什么):

1. `config::schema`(`SETTING_DEFS`/`SettingDef`/`SettingKind`)—— 键定义单源,无其余模块依赖。
2. `config::file`(TOML 读写:`load`/`render_template`/`set_value`/`unset_value`/`write_atomic`/
   `fingerprint`)—— 依赖 `schema`。
3. `config::mod::ConfigManager` —— 依赖 `file`/`schema`,门面聚合。
4. `config::watcher`(notify 监听 + debounce + diff)—— 依赖 `file`(仅 `fingerprint`/`load` 类型)。
5. `logging.rs` 日志内核(独立性:与 config 并行,无互相依赖)
6. `state::RunTokenSlot`(独立通用类型)→ `state::AppState`(聚合以上 + 其余子系统句柄)。
7. `lib.rs::run()` setup 钩子:按 §3.3 顺序组装 subscriber → 构造 `AppState` → 挂载
   `spawn_config_file_watcher`。
9. `ipc::config_commands`/`ipc::log_commands`/`ipc::system_commands::log_frontend_events` ——
   最后接 IPC 层,依赖以上全部。

**外部 crate**(节选,完整清单见 `src-tauri/Cargo.toml`):

| crate | 用途 |
|---|---|
| `tracing`/`tracing-subscriber`/`tracing-appender`/`tracing-panic` | 日志内核:事件宏、Layer 组合、滚动文件+non_blocking、panic hook |
| `notify` | config.toml 文件监听 |
| `toml_edit` | 保留注释/排版的 TOML 读写(非 `serde` 反序列化,理由见 `config/file.rs` 模块文档) |
| `regex` | 诊断包脱敏(用户名段正则) |
| `ring` | 会话 id 生成(`SystemRandom`) |
| `zip` | 诊断包打包 |
| `rusqlite` | `app_config` 表读写(状态类键) |

**坑与教训**(链 `docs/experience.md`):暂无与本子系统直接对应的编号条目(§24「状态持久化
选层」讨论的是前端 store 选层,与本篇 DB/TOML 双真源分层是不同层面的问题,不强行嫁接;
诊断包脱敏 bug 与 span dispatcher 不可达两条教训已在 §4.2 不变量表内联,未重复编号引用)。

**验收**:

- `src-tauri/src/config/mod.rs`/`file.rs`/`watcher.rs`/`schema.rs` 各自
  `#[cfg(test)]` 模块(单键校验、模板渲染、原子写盘、diff 纯函数、自写回环判定等)。
- `src-tauri/src/logging.rs` `#[cfg(test)]` 模块:off 档全链路特征化测试、日志目录大小兜底、
  脱敏(含真实 JSONL 转义回归测试)、`SpanTimer` 四类断言(info/operation_id/debug 过滤/panic
  路径)、`RingBufferLayer` 四类断言(未订阅零广播/订阅抽干/取消订阅清空/容量钳制)。
- `src-tauri/src/ipc/log_commands.rs` `#[cfg(test)]` 模块:分页锚点稳定性(文件增长/缩短)、
  目录穿越拒绝、时区后缀裁剪、直方图分桶(含 NULL 桶回归测试)。
- gate 命令:`cargo test -p scrollery`(或对应包名,`--lib` 覆盖以上全部单测);无独立前端
  测试(配置/日志窗口 UI 见 [Spec11](./Spec11_前端架构.md))。

## 7. 关联

- 相关设计:[docs/designs/2026-07-20-日志能力重构方案.md](../designs/2026-07-20-日志能力重构方案.md)
  (§3.1-§9.4 提出信封格式/环形缓冲/span 埋点等设计理由,本篇只写落地机制,不复述其论证)。
- 相关 Part(理由链,非机制权威):`docs/refactor_2026/` 各 Part 文档对配置/日志子系统的原始
  设计意图;当与现状冲突时以本篇 as-built 描述为准。
- 相关经验教训:[docs/experience.md §24](../experience.md)(状态持久化选层:逐条目用户状态进
  DB、全局纯前端偏好进 localStorage——与本篇"schema 键进 config.toml、状态键留 DB"的双真源
  分层是同一方法论在前后端两侧的呼应)。
- app_config 表结构权威:[Spec01 数据层](./Spec01_数据层.md)。
- IPC 命令/错误码全量权威:[Spec10 IPC 与错误契约](./Spec10_IPC与错误契约.md)。
- 前端消费方(configStore/uiStore/logWindowStore 等):[Spec11 前端架构](./Spec11_前端架构.md)。
- 不变量总目录:[Spec14 不变量与约定](./Spec14_不变量与约定.md)。
