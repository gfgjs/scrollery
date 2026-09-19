---
id: 2026-07-25-lib-rs
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# lib.rs 拆分详案

> 快照说明:本详案基于 2026-07-25 读到的 `src-tauri/src/lib.rs`(1166 行 / 72336 字节,`wc -c` 实测)。并行会话正在做全仓注释精简,行号会漂移——**本文档的一切定位以符号名/段落标题注释为主锚点,行号区间仅作当次快照参考,施工时以符号名重新定位**。本详案纯分析,未改动任何代码。

## 0. 结论先行

`lib.rs` 的体积几乎全部来自 `pub fn run()` 单个函数(约占全文件 82%,90–1042 行),其中又几乎全部来自 `.setup(|app| { ... })` 这一个闭包(约 145–949 行,占 `run()` 本体的 84%)。文件其余部分(模块声明、feature 互斥守卫、两个工具函数)体量很小、职责单一,不构成拆分对象。因此本方案的本体是:**把 `setup` 闭包按域拆成若干 `boot`/聚合函数,`run()` 退化为编排壳**,`invoke_handler` 的聚合早在 2026-07-16(U-P1-a)已下沉到 `ipc::registry::handler()`,可直接复用其模式,无需重新设计。

## 1. 现状结构图

### 1.1 顶层结构(文件级)

| 段落 | 行区间(快照) | 符号锚点 | 职责 | 约行数 |
|---|---|---|---|---|
| channel feature 互斥守卫 | 1–23 | 两个 `compile_error!` cfg 块 | 编译期哨兵:三渠道两两互斥 + 零 channel 报错 | 23 |
| 模块树声明 | 25–56 | `pub mod ai; … pub mod viewer_color;` | 29 个 `pub mod` 声明(crate 模块树入口) | 32 |
| 构建变体常量 | 58–67 | `pub const BUILD_VARIANT` | 编译期 lite/perf 标记,供 UI/埋点读 | 10 |
| 顶层 imports | 69–79 | `use std::sync::Arc; use tauri::{…}; …` | run() 及工具函数共用的导入 | 11 |
| Steam 桩函数 | 81–87 | `steam_restart_if_necessary_stub()`(`cfg(feature = "channel-steam")`) | Part8 前占位,保 channel-steam 组合可编译 | 7 |
| **`pub fn run()`** | **89–1042** | — | **Tauri 应用入口,本文件体量主体** | **954**(占全文件 82%) |
| `fatal_startup_error` | 1044–1060 | `fn fatal_startup_error(...) -> !` | 致命启动错误统一出口(stderr + 原生对话框 + exit(1)) | 17 |
| `spawn_config_file_watcher` | 1062–1166 | `fn spawn_config_file_watcher(...)` | A2:config.toml 热加载 watcher 挂载 | 105 |

### 1.2 `run()` 内部结构

```
run()
├─ steam_restart_if_necessary_stub() 调用(cfg)                                  90-92
├─ tauri::Builder::default()
│   ├─ .plugin(window_state)  .plugin(dialog)  .plugin(os)  .plugin(shell)      93-118
│   └─ [channel-direct] 条件性 .plugin(updater)(探测 context 是否带 updater 配置块)  119-141
├─ .setup(|app| { … })  ← 本文件真正的"巨函数体"，22 个子段(见 1.3)              145-949
├─ .invoke_handler(ipc::registry::handler())  ← 已外置,仅一行调用                950-953
├─ .on_window_event(|window, event| { … })  CloseRequested 拦截 + Focused→QoS  954-972
├─ .build(tauri_context).expect(...)                                            973-974
└─ .run(|app_handle, event| match event { … })  RunEvent::ExitRequested/Exit(WAL
   checkpoint + 后台任务 join) / RunEvent::Ready(启动计时 + smoke test)          975-1041
```

### 1.3 `setup` 闭包内部 22 段(职责 + 行区间 + 关键符号)

| # | 段落标题(源码注释) | 行区间 | 关键符号/调用 | 职责 |
|---|---|---|---|---|
| a | 自绘标题栏 | 146–155 | `main_win.set_decorations(false)` | 非 macOS frameless(顶栏重构 L1) |
| b | app_data_dir | 157–171 | `app.path().app_data_dir()` | 目录解析 + 创建,失败→`fatal_startup_error` |
| c | 恢复启动交换 | 175–187 | `backup::perform_swap_at_boot` | **必须在 DB 连接创建前**执行 |
| d | ORT DLL 路径解析 | 189–227 | `std::env::var("ORT_DYLIB_PATH")` | 3 个坑的注释 + exe 旁 DLL 探测 |
| e | 写连接 + 迁移 | 230–298 | `create_write_connection` / `run_migrations` / `backup::rollback_restore_at_boot` | 迁移失败时按"是否恢复场景"分叉回滚 |
| f | 恢复 verify + 收尾 | 300–306 | `backup::finalize_restore_verified` | 恢复成功后写 Verified、清理 marker |
| g | 读连接池 | 308–322 | `create_read_pool(&db_path, 8)` | 桌面端 8 连接池 |
| h | 配置文件初始化 + 读取 | 324–404 | `config::ConfigManager::load_or_init/degraded` | 59 个设置键唯一真源装配 + 10 元组读取 |
| i | asset-protocol scope | 406–434 | `app.asset_protocol_scope()` / `db::queries::list_scan_roots` | 授权 cache_dir + 扫描根,取代整盘通配 |
| j | 日志子系统 | 436–529 | `RollingFileAppender` / `non_blocking` / `EnvFilter` / `reload::Layer` / `EnvelopeFormat` / `RingBufferLayer` / `tracing_subscriber::registry()` | dev/release 双分支 subscriber 装配 |
| k | panic hook + 启动日志 | 531–545 | `tracing_panic::panic_hook` | — |
| l | 启动期 DB 自愈(同一把锁) | 547–618 | `checkpoint_wal_at_boot` / `reconcile_cover_thumbs` / `reset_in_flight_derivations_for_kind` / `reconcile_missing_cover_thumbs`(7 天节流) | 4 项自愈,**必须 tracing 就绪后、pipeline 拉起前** |
| m | exotic_catalog + AppState 构建 | 620–660 | `exotic::CatalogStore::from_builtin` / `AppState::new(...)`(16 参) | 依赖前面几乎全部局部变量 |
| n | 读池预热 + manage | 662–683 | `app.manage(app_state)` | 之后所有 `try_state::<Arc<AppState>>()` 的前提 |
| o | 自动备份调度器 | 685–690 | `backup_commands::start_auto_backup_scheduler` | — |
| p | exotic Coordinator 装配 | 692–707 | `ExoticCoordinator::start` / `set_exotic_coordinator` / `wake_exotic` | — |
| q | 视频扩展 Service 装配 | 709–719 | `VideoWorkerService::new` / `set_video_worker_service` / `video::bind_app_state` | 惰性起 worker |
| r | 后台任务池 | 721–879 | `handles_pool` + 5 个 `tauri::async_runtime::spawn`(log ring drain / `volume_watch::spawn` / filename rank build / PRAGMA optimize 循环 h1 / 缓存治理循环 h2) | 本节段最长(158 行) |
| s | 强制隐藏主窗口 | 881–891 | `main_win.hide()` | 覆盖 window-state 插件可能恢复的 visible |
| t | 系统托盘 | 893–937 | `TrayIconBuilder` / `Menu` / `MenuItem` | 纯 UI 装配,零跨域依赖 |
| u | config.toml watcher 挂载 | 939–946 | `spawn_config_file_watcher(...)` | 置于 setup 尾部(需 AppState 已 manage) |
| v | `Ok(())` | 948 | — | — |

## 2. 拆分方案

### 2.1 设计原则

1. **对齐既有先例**:`invoke_handler` 已在 U-P1-a(2026-07-16)下沉为 `ipc::registry::handler()` 一行调用——本方案对 `setup` 闭包做同构处理:每个域新增/复用一个 `boot`(或聚合)函数,`run()` 内只留调用点 + 局部变量传递胶水。
2. **归属域优先于新建模块**:能塞进现有域目录(`db/`、`config/`、`logging.rs`、`ai/`、`exotic/`、`video/`)的逻辑不新开顶层模块;找不到自然归属(纯 UI 装配、生命周期回调编排)的才新开顶层文件。
3. **不与并行简案冲突**:`state.rs`(64KB)是 Tier B 简案清单里另一代理的对象,本方案涉及 `AppState::new` 调用点(段 m)时只描述"调用点搬到哪里合适",不预判 `state.rs` 是否会被目录化——留一个显式待裁决项(见 §4 P3)。
4. **不改变错误契约**:凡当前失败即 `fatal_startup_error(app.handle(), ...)` 退出的路径,拆分后仍由 `run()` 编排层持有 `AppHandle` 并调用它;下沉到域模块的函数只返回 `Result<_, DomainError>`,不下沉 `tauri::AppHandle`/`tauri_plugin_dialog` 依赖(保持依赖方向单向:域模块不反向依赖 Tauri 运行时细节)。

### 2.2 目标模块清单

| 目标路径 | 新增/复用 | 职责 | 迁移符号(源) | 预估行数 |
|---|---|---|---|---|
| `src-tauri/src/tray.rs` | 新建(顶层) | 系统托盘装配 | 段 t | ~65 |
| `src-tauri/src/lifecycle.rs` | 新建(顶层) | `on_window_event` + `RunEvent` 处理 | `run()` 的 `.on_window_event(...)` 闭包 + `.run(|app_handle, event| ...)` 闭包 | ~120 |
| `src-tauri/src/tasks.rs` | 新建(顶层) | 后台任务聚合 spawn | 段 r(5 个 spawn + `handles_pool`) | ~200 |
| `src-tauri/src/db/boot.rs` | 新建(复用 `db/` 目录) | 写连接+迁移+恢复回滚+读池+启动期 4 项自愈 | 段 c(调用点)/e/f/g/l | ~240 |
| `src-tauri/src/config/watcher.rs` | 复用(追加函数) | config.toml 热加载 watcher 挂载 | `spawn_config_file_watcher`(原样迁移) | +105(238→~343) |
| `src-tauri/src/config/boot.rs` | 新建(复用 `config/` 目录) | ConfigManager 装配 + 10 元组配置键读取 | 段 h | ~140 |
| `src-tauri/src/logging.rs` 或 `src-tauri/src/logging/boot.rs` | 新建函数(不动存量代码) | RollingFileAppender/EnvFilter/subscriber 装配 + panic hook | 段 j + k(panic hook) | ~150 |
| `src-tauri/src/ai/engine_boot.rs`(命名可议) | 新建(复用 `ai/` 目录) | ORT_DYLIB_PATH 解析 | 段 d | ~40 |
| `src-tauri/src/exotic/coordinator.rs` | 复用(追加聚合函数 `bootstrap(...)`) | host 构造→`ExoticCoordinator::start`→`set_exotic_coordinator`→`wake_exotic` | 段 p | +25 |
| `src-tauri/src/video/worker_service.rs` | 复用(追加聚合函数 `bootstrap(...)`) | `VideoWorkerService::new`→`set_video_worker_service`→`bind_app_state` | 段 q | +20 |
| `src-tauri/src/lib.rs`(拆后) | 保留 | 编排壳:模块声明 + feature 守卫 + `run()` 调用链 + `fatal_startup_error` | 段 a/b/i/m/n/o/s/u/v(胶水,不强拆) | ~260–320 |

**不建议单独拆出的段**(留在 `run()` 编排层做胶水,过度拆分增加认知负担、收益低):
- 段 a(自绘标题栏,10 行)、段 s(强制隐藏窗口,11 行):平台特定的几行窗口操作,拆出模块反而多一层跳转。
- 段 i(asset scope 授权,~29 行):调用 `db::queries::list_scan_roots` + `app.asset_protocol_scope()`,是"一次性把两个已有能力接起来"的胶水,归属感弱。
- 段 m 的 `AppState::new(...)` 调用点本身:构造函数已在 `state.rs`,`run()` 只是把前面各 boot 函数的返回值填进 16 个参数——这正是编排壳该做的事,不需要再包一层。
- 段 o(自动备份调度器 5 行)、段 u(watcher 挂载调用 5 行):均已是"调用既有域函数"的一行胶水,无需再包装。

### 2.3 `run()` 编排壳示意(仅描述调用序列,非代码)

```
setup(|app| {
    a: decorations（保留）
    b: app_data_dir = ...（保留，或 utils::resolve_app_data_dir）
    d: ai::engine_boot::resolve_ort_dylib_path()
    (c+e+f+g+l): let db_boot = db::boot::init(app.handle(), &app_data_dir, &db_path)?;
                 // 内部含 perform_swap_at_boot → create_write_connection → run_migrations
                 //   → 回滚分支 → finalize_restore_verified → create_read_pool
                 //   → 4 项启动期自愈（tracing 就绪后调用，见 §3 顺序不变量）
    h: let cfg_boot = config::boot::init(&app_data_dir, &db_boot.writer_conn)?;
    i: asset scope 授权（保留，调用 db_boot / cfg_boot 已得的值）
    j+k: logging::boot::init_subscriber(&app_data_dir, &cfg_boot)?;
    l(自愈四项，需 cache_dir，故实际调用点在 cache_dir 算出之后): db::boot::run_startup_reconciliation(...)
    m: exotic_catalog = ...; let app_state = Arc::new(AppState::new(...));
    n: app.manage(app_state.clone());
    o: backup_commands::start_auto_backup_scheduler(...)
    p: exotic::coordinator::bootstrap(app.handle(), app_state.clone());
    q: video::worker_service::bootstrap(app_state.clone());
    r: tasks::spawn_all(app, &app_state, ...);
    s: hide main window（保留）
    t: tray::build(app)?;
    u: config::watcher::spawn_config_file_watcher(app.handle(), cfg_boot.path, cfg_boot.manager)
       .map(|w| app.manage(w));
    Ok(())
})
.invoke_handler(ipc::registry::handler())   // 不变
.on_window_event(lifecycle::on_window_event)
.build(tauri_context).expect(...)
.run(lifecycle::on_run_event)
```

### 2.4 依赖方向

```
lib.rs::run()  (编排层，唯一持有 AppHandle 生命周期的顶层调用者)
   ├─→ db::boot            (依赖 backup::*, db::connection/migration/queries — 均已存在)
   ├─→ config::boot / config::watcher
   ├─→ logging::boot(或 logging.rs 内新增 fn)
   ├─→ ai::engine_boot
   ├─→ exotic::coordinator::bootstrap  (依赖 state::AppState, exotic::* — 已存在)
   ├─→ video::worker_service::bootstrap (依赖 state::AppState, video::* — 已存在)
   ├─→ tasks::spawn_all     (依赖 scanner::volume_watch::spawn, state::AppState — 已存在)
   ├─→ tray::build           (仅依赖 tauri::menu/tray)
   └─→ lifecycle::{on_window_event, on_run_event}  (依赖 state::AppState, thumbnail::qos)
```

所有新模块单向依赖 `crate::state::AppState` 与各自域内既有符号,**不反向依赖 `lib.rs`**,不产生新环。`ipc::registry::handler()` 保持现状,不在本方案改动范围。

## 3. 风险与不变量

### 3.1 启动顺序不变量(拆分后必须逐条保真)

1. `backup::perform_swap_at_boot` 必须先于 `create_write_connection`/`create_read_pool`——否则读写的是恢复交换前的旧库文件。
2. `run_migrations` 必须先于 `ConfigManager::load_or_init`——后者内部 `migrate_if_needed` 要读 `app_config` 表做一次性迁移。
3. 迁移失败 + 恢复场景 → 回滚分支要求**先 `drop(db_writer)` 释放 Windows 文件句柄,才能物理回滚 old→current**,再重新 `create_write_connection` + 重新迁移;这个 drop/重开时序如果被函数边界打断(例如把 `db_writer` 提前移动出作用域),必须保留"回滚后重开"的写法,不能简化。
4. tracing subscriber 装好(段 j)之前的一切错误只能走 `eprintln!` / `fatal_startup_error` 的原生对话框——启动期 DB 自愈(段 l)必须放在 subscriber 初始化**之后**,否则自愈日志静默丢失。
5. 启动期 4 项自愈必须在**同一把 `db_writer` 锁内、后台派生流水线拉起前、无并发读者时**一次性收敛;拆分后若把 4 项自愈分散到不同函数/不同锁获取,会破坏"无并发读者"这个前提,产生竞态窗口。
6. `AppState::new(...)` 依赖前面几乎所有阶段的输出(`db_writer`/`db_read_pool`/`config_manager`/`cache_dir`/`log_dir`/`log_ring`/`log_dropped_counter`/`app_data_dir`/`thumb_*` 配置值/`exotic_catalog`),16 个参数——拆分后必须保证这些值的产出顺序不变,任何一个 boot 函数如果尝试提前构造依赖后续阶段输出的值会编译失败(强类型天然兜底),但**跨函数传参的顺序仍需人工核对一遍**。
7. `app.manage(app_state)` 必须先于:`exotic::coordinator::bootstrap`、`video::worker_service::bootstrap`、`tasks::spawn_all`(部分任务用 `state_for_xxx.clone()` 而非 `try_state`,但 `RunEvent` 的 Exit 分支要用 `app_handle.try_state::<Arc<AppState>>()`)、`config::watcher` 回调(内部 `app.try_state::<Arc<AppState>>()`)。
8. `handles_pool` 必须在创建后立即 `app.manage(handles_pool.clone())`——`RunEvent::ExitRequested`/`Exit` 分支靠 `try_state` 取出它来 abort + join 所有后台任务,退出优雅停止的前提是这个 manage 调用没有被拆分遗漏。
9. `spawn_config_file_watcher` 挂载(段 u)必须放在 `app.manage(app_state)` **之后**(回调内 `try_state` 需要它),这也是当前代码把它放在 `setup` 尾部而非更靠前位置的原因,拆分后不能因为"watcher 逻辑感觉该早点挂"而误挪到 `app.manage` 之前。

### 3.2 Tauri 入口/权限语义不变量(本次拆分的红线)

- `#[cfg_attr(mobile, tauri::mobile_entry_point)]` + `pub fn run()` 必须保持在 crate 根 `lib.rs`,签名不变——`main.rs` 通过 `scrollery_lib::run()` 调用,mobile entry point attribute 需要作用在 crate 公开导出的入口函数上,不能下沉到子模块。
- `.plugin(...)` 注册链(window-state/dialog/os/shell + channel-direct 条件性 updater)的**顺序与参数**不变;`capabilities/default.json`、`capabilities/logs.json` 的权限声明与这些插件注册保持一一对应关系,本方案不改动这两个 JSON 文件,拆分后也不应导致插件注册集合发生任何增减。
- `.invoke_handler(ipc::registry::handler())` 调用点/返回类型不变,本方案不涉及命令注册面。
- 两个 `compile_error!` feature 互斥守卫必须留在文件顶部、`pub mod` 声明之前(现状即如此),它们是 crate 级别的编译期哨兵,不属于任何具体域,不应被"域拆分"误吸收进某个子模块。
- `.setup()` 闭包返回类型 `tauri::Result<()>` 不变——闭包内用 `?` 处理 `Menu::with_items`/`TrayIconBuilder::build` 等返回 `tauri::Result` 的调用;拆出的各 boot 函数可以有自己的 `Result<T, DomainError>`,在 `run()` 编排层内 `.map_err(...)` 或 `match` 转 `fatal_startup_error`,不强行统一错误类型。

### 3.3 与并行任务的耦合风险

- `state.rs`(64KB)在 Tier B 简案清单中由另一代理并行分析,若其结论是把 `state.rs` 目录化,则段 m(`AppState::new` 调用点前后的胶水)可以进一步移入 `state::boot`;若结论是保持单文件,则段 m 继续留在 `run()` 编排层即可——**本详案不预判,列为待与 Tier B 结论对齐的开放项**。
- `logging.rs`(46KB,在 Tier C 观察名单,本线不出方案)——本方案建议往其中新增一个 `boot`/`init_subscriber` 函数而不改动存量代码,不会与 Tier C 的"一行处置"结论冲突,但若日后 Tier C 决定把 `logging.rs` 也目录化,新增的 boot 函数天然可以搬进 `logging/boot.rs`,无需返工。
- 并行注释精简会话:本详案的行号会漂移,**施工阶段必须先用符号名(段落标题注释 + 函数名)重新定位一遍**,再动手搬移,不可直接按本文档行号做 diff。

## 4. 收益与优先级

### 4.1 预估拆后大小

- `lib.rs`:1166 行 / 72336 字节 → 预估 **~260–320 行 / ~18–20KB**(降至现状的 25–28%),彻底脱离 Tier A(≥70KB)乃至 Tier B(50–70KB)门槛。
- 新增/扩容文件均为中小文件,单个最大预估 `tasks.rs`(~200 行)、`db/boot.rs`(~240 行),远低于 40KB 观察线,不会制造新的超长文件。

### 4.2 施工优先级(建议顺序:风险从低到高,每级可独立验证)

| 优先级 | 内容 | 理由 |
|---|---|---|
| P0(零跨域依赖,先做) | `tray.rs`、`lifecycle.rs`、`ai::engine_boot`(ORT DLL 解析) | 纯搬移 + 签名固定(Tauri 回调类型/`TrayIconBuilder` API),编译即可验证,无启动顺序风险 |
| P1(域边界清晰) | `exotic::coordinator::bootstrap`、`video::worker_service::bootstrap`、`tasks.rs` | 各自域已有承接位置;`tasks.rs` 需仔细核对 5 个 spawn 的闭包捕获变量显式传参,无逻辑变化 |
| P2(顺序不变量密集,需按 §3.1 逐条核对) | `db::boot`、`config::boot` + `config::watcher` 追加、`logging` boot 函数 | 涉及 DB 迁移/恢复回滚/日志就绪时序,建议每个单独一次 commit + `cargo check` + 启动冒烟双重验证 |
| P3(视 Tier B 结论排期) | `AppState::new` 调用点段是否下沉 `state::boot` | 与并行 `state.rs` 简案可能写同一文件,避免冲突,建议其定案后再排期 |

## 5. 验证策略

1. **编译检查**:`cargo check --manifest-path src-tauri/Cargo.toml`(至少覆盖一个具体 channel feature,如 `--no-default-features --features channel-direct`,同时验证顶部两个 `compile_error!` 守卫仍生效——即"零 channel"与"两两互斥"两种非法组合仍应编译失败)。
2. **静态检查**:`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`(纯移动不应引入新 warning;注意 `#[allow(clippy::too_many_arguments)]` 等既有属性若挂在被搬移的函数/调用上,需随函数体一并迁移,否则会冒出新的 clippy 提示)。
3. **启动冒烟**(既有契约,不可回归):设置 `PICASA_SMOKE_TEST` 环境变量跑一次构建产物,断言进程在到达 `RunEvent::Ready` 时以退出码 0 退出;这一契约本身就是为了在合并前挡住"开机 panic"类回归,拆分工作完成后必须重跑一次。
4. **手工核对清单**(对照 §3.1 的 9 条顺序不变量逐条过一遍,标注 GUI/手测,不在自动化范围):
   - 数据库迁移确实先于 `ConfigManager` 初始化(可加临时 `tracing::debug!` 断言顺序,验证后移除)。
   - 故意损坏一次迁移(或复用 backup 测试线现成 fixture)触发"迁移失败→回滚"分支,确认原始库仍能正常打开。
   - config.toml 外部编辑热加载:手改文件后确认前端收到 `config-file-changed` 事件、非 `restart_required` 的键即时生效。
   - 托盘菜单 show/quit、左键点击呼出主窗口行为不变。
   - 窗口关闭拦截:点击关闭仍发出 `window-close-requested` 而非直接退出。
   - 退出时 WAL checkpoint 仍执行(可用 `sqlite3 scrollery.db "PRAGMA wal_checkpoint;"` 或直接看 `-wal` 文件是否被截断为核对)。
   - 后台任务(log ring drain / volume watch / filename rank build / PRAGMA optimize / 缓存治理)在退出时能在 3 秒超时内优雅停止(既有 `tokio::time::timeout` 契约)。
5. **capabilities 契约核对**:一次性核对 `src-tauri/capabilities/default.json`、`logs.json` 的权限清单与拆分后 `.plugin(...)` 注册集合仍然一一对应,确认没有代码路径意外增删插件注册(本方案不改动这两个 JSON 文件本身)。
6. **不要求新增单元测试**:本次是纯结构移动、零行为变化,不强制新增测试;若 `db::boot`/`config::boot` 拆出后接口足够干净,可选择性给"迁移失败→回滚"路径补一个 characterization test(参考 backup 线既有测试风格),非本方案强制项。

## 附:顺手发现

无(未在阅读 `lib.rs` 及相关调用点过程中发现计划外的代码问题;`ORT_DYLIB_PATH`/恢复回滚等逻辑注释详尽、已知坑点均有文档化说明,未见明显 bug)。
