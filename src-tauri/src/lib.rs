// src-tauri/src/lib.rs
//! 库入口点 — 模块声明和 Tauri 应用程序构建器。
//!
//! D-450:`setup` 闭包已按域拆成各模块的 boot/bootstrap 函数,本文件退化为**编排壳**——
//! 只留模块声明、feature 守卫、`run()` 的调用链与致命启动错误出口。启动顺序不变量见
//! `docs/planning/2026-07-25-超长文件拆分方案/analysis/lib-rs.md` §3.1(共 9 条),
//! 各 boot 函数的文档注释里也复述了与自己相关的那几条——**调整调用顺序前必读**。

pub mod ai;
pub mod audio;
pub mod backup;
/// 配置文件子系统:设置类键唯一真源在 `<app_data_dir>/config.toml`(A1 建模块+单测,A2 接线
/// 启动/IPC/watcher——见 `config::boot` 与 `config::watcher`、`ipc::config_commands`)。
pub mod config;
pub mod db;
/// 精确内容去重：与常规扫描的 change fingerprint 分离的摘要与逻辑单元身份。
pub mod dedup;
pub mod derive;
pub mod download;
pub mod editing;
pub mod engine;
pub mod enhance;
pub mod error;
pub mod exotic;
pub mod export;
/// 已注册格式的运行时并集(内置表 ∪ exotic Catalog)与 UI 投影。
/// 独立成模块因 `utils::format` 不能引 `exotic::catalog`(反向依赖已存在,放一起成环)。
pub mod formats;
pub mod ipc;
pub mod layout;
/// 应用生命周期回调:窗口事件拦截 + `RunEvent`(退出 WAL checkpoint / 后台任务优雅停止)。
pub mod lifecycle;
pub mod logging;
pub mod proofread;
pub mod reader;
pub mod scanner;
pub mod state;
pub mod storage;
/// 启动期后台任务聚合装配(5 个常驻任务 + 句柄池)。
pub mod tasks;
pub mod thumbnail;
/// 系统托盘装配。
pub mod tray;
/// 文件树的文件系统数据源(「所有文件」两态);DB 模式仍走 db::queries 快路径。
pub mod tree;
pub mod utils;
pub mod video;
pub mod viewer_color;
/// 窗口材质(毛玻璃,window_material 配置键):DWM 背板 mica/acrylic/none 的应用器。
pub mod window_material;

/// Compile-time build variant marker (§1.4.4) — "lite" (default) or "perf".
/// Surfaced to the UI/telemetry so the app can show a variant badge and gate
/// "needs Perf / missing component" hints.
/// 编译期构建变体标记（§1.4.4）—— "lite"（默认）或 "perf"。
/// 暴露给 UI/埋点，用于显示变体角标并提示「需性能版 / 缺组件」。
pub const BUILD_VARIANT: &str = if cfg!(feature = "perf") {
    "perf"
} else {
    "lite"
};

use std::sync::Arc;

use tauri::Manager;
use tracing::info;

use crate::state::AppState;

/// 启动期致命失败的域无关载体:各 boot 函数只上抛「人可读上下文 + 明细」,由 `run()`
/// 编排层统一交给 [`fatal_startup_error`]——域模块因此**不反向依赖** `tauri::AppHandle`
/// 与 dialog 插件(拆分方案 §2.1 原则 4)。
#[derive(Debug, thiserror::Error)]
#[error("{context}: {detail}")]
pub struct StartupFailure {
    /// 双语上下文短语,直接进 stderr / 原生对话框标题行。
    pub context: &'static str,
    /// 可诊断明细(通常是底层错误的 `Display`)。
    pub detail: String,
}

impl StartupFailure {
    pub fn new(context: &'static str, detail: impl std::fmt::Display) -> Self {
        Self {
            context,
            detail: detail.to_string(),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // ── 插件 ───────────────────────────────────────────────────────
        // 窗口几何不再由独立插件持久化:位置/尺寸/scale/最大化统一走 config.toml 的 window_geometry
        // 键(见 config::window 与 setup 里的装载点),因此这里只留对话与平台探测插件。
        .plugin(tauri_plugin_dialog::init())
        // os:前端平台探测(自绘标题栏 mac/windows 分叉,顶栏重构 L0)。仅暴露 os:default
        // 只读查询面(platform/arch/type 等),无写能力。
        .plugin(tauri_plugin_os::init())
        // 2026-07-06 审查 P1-22:移除 tauri-plugin-fs——前端零使用(所有文件操作走自定义 Rust
        // command),而 fs:read-all/write-all 授予 webview 读写 $APPDATA(含 scrollery.db 与
        // 已装 exotic worker exe)的能力,构成「webview 沦陷 → 改写 worker 二进制 → 本地执行」
        // 升级链。删权限 + 摘插件 + 去依赖,零功能损失。
        .plugin(tauri_plugin_shell::init());
    // T7/T9(Part7 §3.5):updater 属直销(direct)发布面(dep 为普通依赖,渠道 feature 已退役),
    // 日后接入第三方渠道时再按渠道重新定门控。更新检查走
    // Rust 侧 API(app.updater()),**不开放 webview ACL 权限/不引前端包**——现无更新 UI
    // 消费者,IPC 面越小越好;Part8 做更新 UI 时再定「Rust 驱动+事件通知」或「JS 驱动
    // +capability」。签名公钥/endpoints 见 tauri.direct-release.conf.json(dev 密钥,
    // 首发前轮换或转正,.gitignore .release-keys 注有决策链)。
    // T11/§3.6.5①(Part7):updater 配置块住 direct 发布 overlay,基座 conf 保持天生干净
    // (tauri 的 conf 合并只能加/改不能删)。因此按「编译进 context 的最终配置是否携带
    // plugins.updater」决定注册:插件 init 对缺配置是硬失败(Config.pubkey 必填),
    // dev/无 overlay 构建不带块 → 不注册,tauri dev 与普通 build 不受影响;
    // 正式 direct 发布走 tauri:build:direct-release。
    let tauri_context = tauri::generate_context!();
    // 启动计时锚(2026-07-13 排查热启动变慢):RunEvent::Ready 读它算 Rust boot→Ready 总耗时,
    // 用于一刀切分「后端 setup 耗时」与「前端 Vite/WebView2/Vue 挂载耗时」。
    let _ = lifecycle::BOOT_INSTANT.set(std::time::Instant::now());
    let builder = if tauri_context.config().plugins.0.contains_key("updater") {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    builder
        // ── 应用程序设置 ─────────────────────────────────────────────────────
        // 各段落已下沉到域内 boot 函数,本闭包只做调用编排 + 局部变量传递。**调用顺序即
        // 启动顺序不变量**(拆分方案 §3.1 九条),不得重排/合并。
        .setup(|app| {
            // 自绘标题栏(顶栏重构 L1):非 macOS 平台去掉原生装饰(frameless),窗口三键改由前端
            // WindowChrome 自绘;macOS 保留原生装饰(config decorations:true)+ titleBarStyle:Overlay。
            // 先修正隐藏窗口的装饰；必须等 AppState 可供前端 IPC 使用后才显示，避免 WebView
            // 抢跑调用 get_startup_config 导致主题/材质水合失败。
            if let Some(main_win) = app.get_webview_window("main") {
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = main_win.set_decorations(false);
                }
            }

            let app_data_dir = match app.path().app_data_dir() {
                Ok(d) => d,
                Err(e) => fatal_startup_error(
                    app.handle(),
                    "无法获取应用数据目录 / cannot resolve app data dir",
                    &e.to_string(),
                ),
            };
            if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
                fatal_startup_error(
                    app.handle(),
                    "无法创建应用数据目录 / cannot create app data dir",
                    &e.to_string(),
                );
            }

            // ORT 动态库路径解析(纯环境变量,与 DB/日志无先后依赖)。
            ai::engine_boot::resolve_ort_dylib_path();

            // 恢复交换 → 写连接 + 迁移(含回滚分支)→ 恢复收尾 → 读池(§3.1 不变量 1/3)。
            let db_boot = db::boot::init(&app_data_dir)
                .unwrap_or_else(|f| fatal_startup_error(app.handle(), f.context, &f.detail));

            // 配置文件初始化 + 启动期键读取(自 P24 起不再读 DB,与 DB 初始化的先后不再构成不变量)。
            let cfg = config::boot::init(&app_data_dir);

            // 保存首帧材质值；窗口仍保持隐藏，待 AppState manage 完成后同步应用并显示。
            let startup_material = cfg
                .manager
                .get("window_material")
                .unwrap_or_else(|| "none".to_string());

            let cache_dir = cfg
                .custom_cache_dir
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| app_data_dir.join("cache"));
            std::fs::create_dir_all(&cache_dir).unwrap_or_default();

            // ── Asset-protocol scope (E1) ─────────────────────────────────────
            // Grant read access only to the thumbnail cache + actual scan roots, rather
            // than blanket whole-drive globs. Source images live under arbitrary scan-root
            // paths, so they're granted at runtime here (and on add_scan_root).
            // 仅授予缩略图缓存 + 实际扫描根目录的读取权限，取代整盘通配。
            // 源图位于任意扫描根路径下，故在此（及 add_scan_root 时）运行时授权。
            {
                let scope = app.asset_protocol_scope();
                if let Err(e) = scope.allow_directory(&cache_dir, true) {
                    // §3.4/§9.2 高敏调用点结构化改造(S2):路径不再插值进 message,
                    // 脱敏 Layer(未来)只能拦结构化字段,烧进 message 的内容无法事后过滤。
                    tracing::warn!(path = %cache_dir.display(), error = %e, "Failed to allow cache_dir in asset scope");
                }
                if let Ok(pool) = db_boot.read_pool.get() {
                    if let Ok(roots) = crate::db::queries::list_scan_roots(&pool) {
                        for r in &roots {
                            if let Err(e) = scope.allow_directory(&r.path, true) {
                                tracing::warn!(path = %r.path, error = %e, "Failed to allow scan root in asset scope");
                            }
                        }
                        info!(scan_root_count = roots.len(), "Asset scope granted for scan root(s) + cache dir");
                    }
                }
            }

            // ── 日志记录 ───────────────────────────────────────────────────────────
            let log_dir = cfg
                .custom_log_dir
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| app_data_dir.join("logs"));
            std::fs::create_dir_all(&log_dir).unwrap_or_default();

            let logging_boot =
                logging::init_subscriber(&log_dir, &cfg.manager, &cfg.log_level)
                    .unwrap_or_else(|f| fatal_startup_error(app.handle(), f.context, &f.detail));
            // guard 交 Tauri 托管(而非 Box::leak),退出时正常 flush。
            app.manage(logging_boot.guard);

            info!("Scrollery starting up, database path: {:?} | Scrollery 正在启动，数据库路径: {:?}", db_boot.db_path, db_boot.db_path);
            // session_id 已由 EnvelopeFormat 无条件挂在信封顶层,此处不再重复当字段传——
            // 否则 FieldCollector 会把它当普通字段塞进 attributes,顶层+attributes 各出现一份。
            info!("Log level set to: {} | 日志级别已设置为: {}", cfg.log_level, cfg.log_level);
            if logging_boot.purge_summary.purged_files > 0 {
                info!(
                    purged_files = logging_boot.purge_summary.purged_files,
                    freed_bytes = logging_boot.purge_summary.freed_bytes,
                    "Log directory size budget exceeded at startup, oldest files purged | 启动时日志目录超出大小兜底,已清理最旧文件"
                );
            }

            // 启动期 WAL 截断 + 三项自愈:**须在 tracing 就绪后、派生管线拉起前**
            // (§3.1 不变量 4/5),故调用点在此而非 db::boot::init 内。
            db::boot::run_startup_reconciliation(&db_boot.writer, &db_boot.db_path, &cache_dir);

            // P0-1 目录移动半完成收尾（审查 §7.1-B）：启动期只做可证的短收敛——已发布行的
            // 索引重放 + 目标存在性检查（见 dir_move::reconcile_at_startup 的文档）。不删源、
            // 不做整树摘要校验、不做大拷贝；仍在 intent 阶段的行（可能含 GB 级拷贝）留给用户
            // 显式重试。调用点位于启动路径上：管线尚未拉起、无并发写者，故此处是受控阻塞。
            match crate::ipc::dir_move::reconcile_at_startup(
                crate::ipc::dir_move::MoveDb::Writer(&db_boot.writer),
                &cache_dir,
            ) {
                Ok(reports) if reports.is_empty() => {}
                Ok(reports) => {
                    let retry_needed = reports.iter().filter(|r| r.needs_retry).count();
                    info!(
                        "目录移动收尾 {} 条，仍需重试 {} 条 | directory move recovery: {} handled, {} still pending",
                        reports.len(),
                        retry_needed,
                        reports.len(),
                        retry_needed
                    );
                }
                Err(e) => tracing::warn!(
                    "[Startup] 目录移动收尾失败（不致命） | directory move recovery failed: {e}"
                ),
            }

            // ── 构建 AppState ─────────────────────────────────────────────
            // 内置冷门格式能力目录（编译期嵌入）。解析失败不致命：降级为空目录并告警，
            // 主功能不受影响（仅 exotic 识别失效）。
            let exotic_catalog = Arc::new(
                crate::exotic::CatalogStore::from_builtin().unwrap_or_else(|e| {
                    tracing::error!("内置 exotic Catalog 解析失败，降级为空目录 | {e}");
                    crate::exotic::CatalogStore::with_snapshot(
                        crate::exotic::CatalogSnapshot::empty(),
                    )
                }),
            );

            // 视频可播产物独立池启动清扫(§5.4/§9.6):清 {cache_dir}/video/*.tmp 残留
            // (转码中途崩溃/应用退出遗留);终名 .mp4 产物保留。noop 若池未建。
            if let Err(e) = crate::video::playable_cache::sweep_tmp(&cache_dir) {
                tracing::warn!(
                    "[Startup] 视频池 .tmp 残留清扫失败（不致命） | video pool .tmp sweep failed: {}",
                    e
                );
            }

            let cache_dir_for_tasks = cache_dir.clone();
            let config_manager_for_watcher = cfg.manager.clone();
            let log_ring_for_tasks = logging_boot.log_ring.clone();
            let app_state = Arc::new(AppState::new(
                db_boot.writer,
                db_boot.read_pool,
                cfg.manager,
                cache_dir,
                log_dir,
                logging_boot.log_ring,
                logging_boot.dropped_counter,
                app_data_dir.clone(),
                app_data_dir.join("exotic"),
                cfg.thumb_size,
                cfg.thumb_skip_max_kb,
                cfg.thumb_strategy,
                cfg.gpu_engine,
                cfg.ai_hq_cache,
                cfg.thumb_webp_quality,
                exotic_catalog,
            ));

            // ── Pre-warm one read-pool connection ─────────────────────────────
            // Eagerly acquire (and immediately release) one read connection so the pool
            // establishes its first SQLite file handle during WebView2 cold-start, which
            // runs concurrently. By the time the frontend's first IPC calls arrive the
            // connection is already open and warmed up, saving ~50-100ms per call.
            //
            // ── 预热一个读连接池连接 ──────────────────────────────────────────
            // 提前获取（立即释放）一个读连接，让连接池在 WebView2 冷启动（并发进行）期间
            // 建立第一个 SQLite 文件句柄。等前端首批 IPC 调用到达时，连接已就绪，
            // 每次调用可节省约 50-100ms。
            drop(app_state.db_read_pool.get());

            // §3.1 不变量 7:manage 必须先于下方 coordinator / video service / 后台任务 /
            // watcher 挂载(它们或持 clone、或经 try_state 取用)。
            app.manage(app_state.clone());
            info!("AppState initialised | 应用状态 (AppState) 初始化完成");

            // ── 窗口几何持久化(设计 §6)──────────────────────────────────────────
            // 位置/正常尺寸/scale factor/最大化统一进 config.toml 的 window_geometry 键,取代已退役的
            // tauri-plugin-window-state(它的独立状态文件既不参与恢复默认设置,又是第二个持久化出口)。
            // 配置读写缝由配置核心提供(唯一写盘入口);采集总闸暂关,等恢复与展示走完再打开。
            // 提交恒走配置核心的统一编排入口(config::settings::submit_window_geometry:async 串行门
            // + 写锁内按 label 合并 + 统一广播),因此窗口几何没有第二个写盘路径,也不绕过核心的串行门。
            config::window::install(app.handle().clone(), app_state.clone());
            // 先按记录恢复边界(缺记录/离屏则回默认值居中),再进下面的展示流程 —— 与旧插件
            // 在 setup 前恢复的可见结果一致,但不再有第二个文件。
            config::window::restore_before_show(app.handle(), config::window::MAIN_LABEL);

            // 主窗口必须在 AppState 就绪后才显示，否则 WebView 可能在 setup 完成前发起
            // IPC，导致 startupConfigPromise 失败、html[data-glass] 未写入。材质先同步挂载，
            // 再显示同一个完整尺寸的静态启动层，避免透明窗口闪过；show 后再异步重挂一次，
            // 覆盖 tauri#12854 的可见性重置行为。
            if let Some(main_win) = app.get_webview_window("main") {
                crate::window_material::apply_now(&main_win, &startup_material);
                let _ = main_win.show();
                let _ = main_win.set_focus();
                crate::window_material::apply(app.handle(), &startup_material);
            }
            // 到这里启动期程序化落位已结束,打开采集总闸:此后的 move/resize 才算用户调整。
            config::window::activate_capture();

            // ── 自动备份调度器（方案 B §7）────────────────────────────────────────
            // 启动后 idle ~2min 首检、此后每小时复检。dormant until 用户开启 backup_auto_enabled（B-1）。
            crate::ipc::backup_commands::start_auto_backup_scheduler(
                app.handle().clone(),
                app_state.clone(),
            );

            crate::exotic::coordinator::bootstrap(app.handle().clone(), app_state.clone());
            crate::video::worker_service::bootstrap(app_state.clone());

            // ── 后台任务(§3.1 不变量 8:句柄池在内部创建后立即 manage)──────────────
            tasks::spawn_all(
                app,
                &app_state,
                log_ring_for_tasks,
                cache_dir_for_tasks,
                cfg.thumb_cache_max_mb,
            );

            tray::build(app)?;

            // ── config.toml 文件监听(A2)────────────────────────────────────────
            // 置于 setup 末尾:AppState 此刻已 `app.manage()`(watcher 回调内 `try_state`
            // 需要它,§3.1 不变量 9),挂载失败(如平台监听器初始化失败)只降级为
            // 「热更新不可用」,不阻断启动。
            if let Some(watcher) = crate::config::watcher::spawn_config_file_watcher(
                app.handle(),
                cfg.path,
                config_manager_for_watcher,
            ) {
                app.manage(watcher);
            }

            Ok(())
        })
        // ── IPC 命令处理器 ───────────────────────────────────────────
        // 命令清单已下沉 ipc/registry.rs(U-P1-a):新增命令只碰 registry + 命令文件。
        .invoke_handler(ipc::registry::handler())
        .on_window_event(lifecycle::on_window_event)
        .build(tauri_context)
        .expect("Error while building Tauri application")
        .run(lifecycle::on_run_event);
}

/// 致命启动错误的统一出口：取代裸 `.expect()` 的不可读 panic。
/// 写清晰可诊断信息到 stderr（日志子系统在 DB 初始化后才装好，此前唯 stderr 保证可见）
/// + 尽力弹原生对话框（失败不影响退出）+ 受控退出码 1（区别于 panic 的 101）。
fn fatal_startup_error(app: &tauri::AppHandle, context: &str, detail: &str) -> ! {
    eprintln!("[FATAL][startup] {context} | 启动失败: {detail}");
    let msg = format!(
        "Scrollery 启动失败：{context}\n\n详情 / Detail: {detail}\n\n\
         可尝试：检查数据库文件是否损坏（可备份后删除以重置），或查看日志后重试。"
    );
    use tauri_plugin_dialog::DialogExt;
    let _ = app
        .dialog()
        .message(msg)
        .title("Scrollery 启动失败 / Startup failed")
        .blocking_show();
    std::process::exit(1);
}
