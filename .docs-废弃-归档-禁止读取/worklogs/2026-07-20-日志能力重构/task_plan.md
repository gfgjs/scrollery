---
status: 快照
type: working-memory
line: 日志能力重构
created: 2026-07-20
---

# 任务计划:日志能力重构

## 目标
产出 Scrollery 日志能力重构方案(调研报告 + 落地方案 + 待裁决清单),满足:功能全面、应用内日志 UI、分析能力、人类友好、agent 读取友好。本任务只到方案落盘,施工待用户裁决后另行开工。

## 当前阶段
阶段 11(S6 收尾)完成。日志能力重构线 S1..S6 全部落地,方案 §9.4 分步 DoD 逐条核对通过。下一步=用户批准后 push(全线 9 个 commit + 本次 S6 收尾提交均未 push)。

## 阶段

### 阶段 1:现状摸底
- 已完成:现状全貌落 findings.md「发现」节(tracing 骨架在、文本格式、每条 fsync、默认级别 debug、无 panic hook、前端无桥、无既有设计文档)
- **状态:** complete

### 阶段 2:联网调研(sonnet researcher ×3)
- 已完成:三报告(A 后端+性能补问 / B 应用内 UI / C agent 格式+治理)全文落 attachments/,索引与要点在 findings「外部资料」节
- **状态:** complete

### 阶段 3:汇总方案 + 裁决清单
- 已完成:五层架构 + 性能专节 + 8 项裁决(Q1..Q8 各带推荐)综合成稿
- **状态:** complete

### 阶段 4:报告与方案落盘
- 已完成:方案落 docs/designs/2026-07-20-日志能力重构方案.md(draft);todo.md 登记;lines/日志能力重构.md 开线;三件套回写
- **状态:** complete

### 阶段 5:裁决落定 + Sonnet 5 施工适配
- 已完成:用户裁 Q1..Q8 全采纳推荐+主动建议(D-305);方案增补 §9 施工指引(硬护栏 10 条/陷阱表 9 项/实现定型 A-E/S1-S6 分步 DoD/会话配置),§3.1 文件层由内置 .json() 改定型自定义 FormatEvent(信封字段注入需要),重复压缩定型 error.rs:205 调用点辅助函数非 Layer(D-306)
- **状态:** complete

### 阶段 6:S1 内核施工(Sonnet 5,方案 §9.4)
- 已完成:新增 `src-tauri/src/logging.rs`(EnvelopeFormat JSONL 信封格式化器 §9.3-A、generate_session_id、enforce_size_budget 大小兜底);lib.rs 日志初始化段重写(删 RealTimeDailyAppender,换 RollingFileAppender daily+max_log_files=14、guard 改 app.manage 而非 Box::leak、控制台层 cfg(debug_assertions) 门控、panic hook 接 tracing-panic、默认级别 debug→info 落 Q2 裁决);Cargo.toml 加 tracing-subscriber json feature + tracing-panic + criterion dev-dep + `[[bench]] logging`;settingsMap.ts/zh-CN.ts/en-US.ts 加 logLevel=off 选项(off 档复用既有 LOG_RELOAD,EnvFilter 原生识别 "off" 关键字,后端 config_commands.rs 零改动);`benches/logging.rs` criterion 微基准(off/filtered/enabled 三态每事件成本)+流水线宏基准(真实 CPU 解码+编码 30 张合成 PNG/批,off/info/debug 三档);logging.rs 新增 off 档全链路特征化测试(scoped `with_default`,验证「off 期间事件从未触达写入器」+「切回 info 恢复」+「guard drop 阻塞至后台线程写完(退出 flush)」三件事,替代需要真机 GUI 的手测)
- 验证:cargo test 785 passed/0 failed/6 ignored;cargo clippy --all-targets 零新增警告(唯一 1 处 pre-existing `scan_commands.rs:96` manual_inspect,与本线无关,基线红不代修);criterion 实测回写方案 §3.2:每事件成本 off=0.18ns/filtered=0.18ns/enabled=1.60µs,流水线宏基准 off=104.74ms、info=105.54ms、debug=105.30ms(30 张/批),off 态相对 info/debug 差值 0.53%~0.76%,低于 >1% 未完成门槛,**验收基准通过**;前端 eslint/vue-tsc/vitest(1280 passed)均绿
- reviewer 深审回合(方案 §9.5「每期收口先跑 reviewer 深审」):0 严重、2 警告(lib.rs:478 session_id 重复挂字段致信封+attributes 各一份/bench 首版 writer 直用 `io::sink` 未套 `non_blocking` 低估真实入队成本)+4 建议(size budget 清理结果补 info 汇总日志可见化、补 3 个边界单测、log_level 结构化非本期强制、AppHandle::exit 兜底路径 guard 不 flush 的边缘场景非本期必需)。已修 2 警告 + 前 2 条建议(enforce_size_budget 返回 `PurgeSummary` 并在 init 后补记 info、新增 3 个边界单测);后 2 条建议按 reviewer 自评「非本期必需」明确不做,不计入遗留
- **状态:** complete

### 阶段 7:S2 schema 治理(Sonnet 5,方案 §9.4 S2)
- 已完成:①operation_id——`src/utils/ipc.ts` 新增 `generateOperationId()`(crypto.randomUUID + 降级);`derivationStore.ts` 的 `startVideo()` 生成并经 `IPC.START_DERIVATION` 的 `operationId` 透传;`start_derivation` command 加 `operation_id: Option<String>` 参数,经 `launch_derivation_pipeline`→`start_derivation_pipeline` 一路带到 `tokio::spawn` 内的启动/完成/失败/panic 四条任务级汇总日志(未深入生产者/消费者内部——per §3.3「任务级汇总」定位,内部高频细节本就不进日志,D-303)。②error.rs 富化+重复压缩——`(code,msg)` 匹配前移;新增 `error_chain_strings()` 沿 `source()` 链收集展示文本;`error_log_dedup_check()`(静态 `LazyLock<Mutex<HashMap<(target,code),Entry>>>`,30s 窗口:首见放行 `repeat_count=1`,窗口内吞掉计数,窗口切换首条放行 `suppressed=N`);`clear_logs` command 调 `reset_error_log_dedup()` 清表;`logging.rs` 的 `FieldCollector` 新增字段名特判 `error_chain`(JSON 字符串解析回真数组,而非 `{:?}` 撕裂文本)/`error_code`。③pipeline target 归一——新增 `build_env_filter_directive()` 单一事实源(lib.rs 启动路径 + config_commands.rs 热切路径共用),追加 `scrollery::pipeline=warn,reqwest=warn` 后缀;thumbnail/generator.rs 全部 per-item 调用点(trace/debug/info/warn 共 11 处)挂 `target: "scrollery::pipeline::thumb"`;ai/pipeline.rs+ai/face_pipeline.rs 的生产者让步 debug 挂 `ai`/`face`;derive/pipeline.rs 按 `task.kind` 分三支挂 `video`(VideoCover/VideoKeyframes)/`ai`(AiThumb)/不挂(DocThumb/AudioCover/AudioMeta,无对应桶,不误标);derive/image.rs 的 AI 缓存 GPU 回退日志挂 `ai`(非 video)。④高敏调用点结构化——lib.rs:396/402(asset scope 授权失败,路径不再插值进 message)、generator.rs 全部路径/文件名类调用点同批结构化(与③合并施工)。
- 验证(reviewer 修复前):cargo test 789 passed(较 S1 净增 4:error_chain_strings/dedup 状态机/JSONL 信封断言/directive 拼接各一);clippy 零新增;criterion 复测(仅微基准+流水线宏基准,验证 S2 改动未劣化 S1 已达标的吞吐):off=108.25ms/info=109.19ms/debug=108.29ms(30 张/批,较 S1 首测的 104.74/105.54/105.30ms 整体偏高,判读为 dev-machine-hw-instability 记忆记录的同一台开发机负载/热噪声,非代码劣化——同批三档互相对照的相对差值才是验收依据),off 相对 info/debug 差值 0.037%~0.87%,仍低于 1% 门槛,**S2 无回退**;抽 3 行真实 JSONL(经临时 `--nocapture` 断言取样,取样后已移除):
  ```json
  {"attributes":{"error.chain":["exotic error [envelope_test_only]: disk full"],"error.code":"envelope_test_only","repeat_count":1},"level":"ERROR","msg":"AppError occurred (to frontend): Exotic { code: \"envelope_test_only\", message: \"disk full\" }","operation_id":null,"session_id":"s-test","target":"scrollery_lib::error","ts":"2026-07-20T20:13:59.267+08:00"}
  {"attributes":{},"level":"INFO","msg":"marker-before-off","operation_id":null,"session_id":"s-test","target":"scrollery_lib::logging::tests","ts":"2026-07-20T20:13:59.267+08:00"}
  {"attributes":{"item_id":12345,"path":"IMG_0042.jpg"},"level":"DEBUG","msg":"CACHE_HIT","operation_id":null,"session_id":"s-7f2a9c1e","target":"scrollery::pipeline::thumb","ts":"2026-07-20T20:13:59.267+08:00"}
  ```
- reviewer 深审回合(方案 §9.5):0 严重、4 警告、1 存疑。已修 4 警告——(a) `derive/image.rs` 的 AI 缓存 GPU 回退日志误挂 `video`(实际服务 AI/人脸分析缓存,与视频派生无关),改挂 `ai`;(b)(c) `derive/pipeline.rs` 两处误把「6 种 kind 共用」的通用生产者/消费者日志统一挂 `video`(doc/audio/ai 派生按 video 过滤会漏看/按 ai 过滤会漏看),改为:让步循环那条(无 kind 上下文)去掉 target 回退默认、失败日志那条(`task.kind` 在场)按 kind 分三支各自挂字面量 target(tracing target 是编译期常量,不能塞运行时字符串,只能分支各写一份宏调用);(d) `error.rs` 的 chain/chain_json 在 dedup 判定前就已算好,Swallow 分支(热路径)白付序列化开销,改为先判决策、只在两个 Emit 分支现算 chain。另有 2 处注释锚点漂移(两处写「见 error.rs:205」,该行经本轮编辑已挪到不相关代码)已改按函数名引用。**存疑项按方案 §9.1 护栏 #10 不擅自变更定型,保留报告用户**:dedup 签名 `(target,code)` 因当前唯一调用点 target 恒为 `module_path!()`,事实上退化为仅按 `code` 去重——两个毫不相关但共享泛化 code(如 Io/Db/System/Os/Internal,横跨全仓各功能路径)的错误若 30 秒内相继发生,后者会被整段吞掉且直到窗口翻转前无 `suppressed` 提示。方案 §9.3-C 明文定型 Sig=(target,error code),此为该定型在「单一调用点」现状下的真实副作用,非本次改动引入的新缺陷,不擅自扩充签名(如加进 chain[0] 摘要)——按护栏留待用户确认是否接受,或另案裁决。**已裁(同日,D-307,commit 于 S2 后单独小提交)**:三候选(接受现状/扩签名/Swallow 降档 debug)对比后用户选 C——Swallow 臂降档以 debug 级落全量证据行(`dedup_swallowed=true`+error.code/error.chain),error 档保风暴压缩、debug 档保真相;不扩签名(扩 message/chain[0] 在风暴场景签名基数爆炸、压缩失效)。已落地+新增回归测试 `swallowed_repeat_demoted_to_debug_level`(debug 档 2 行且第 2 行 DEBUG 带标记/info 档仅首见 1 行),cargo test 791 passed、clippy 零新增、方案 §9.3-C 正文同步改。
- 验证(reviewer 修复后复测):cargo test 790 passed/0 failed/6 ignored(新增 1 处 warn/error 档不追加后缀的回归测试);cargo clippy --all-targets 零新增警告(同 S1 唯一 pre-existing `scan_commands.rs:96`);rustfmt 对本阶段 5 个改动文件(`error.rs`/`logging.rs`/`derive/pipeline.rs`/`derive/image.rs`/`ipc/system_commands.rs`)`--check` 通过;前端 eslint/vue-tsc/vitest(1280 passed)未再变动(本阶段前端改动仅 `ipc.ts`/`derivationStore.ts` 两处新增函数调用,无既有测试覆盖该分支,已过 typecheck+lint);criterion 宏基准未因 reviewer 修复轮重跑——修复面在 error.rs/logging.rs/derive/pipeline.rs,未触碰宏基准实测的缩略图生成热路径(thumbnail/generator.rs 在修复轮前就已定稿),此前一次实测结果仍代表数
- **状态:** complete

### 阶段 8:S3 前端桥施工(Sonnet 5,方案 §9.4 S3)
- 已完成:①`src-tauri/src/ipc/system_commands.rs` 新增 `log_frontend_events` command + `FrontendLogEvent` struct + `emit_frontend_log_event`(固定 `target: "scrollery::frontend"`,来源经 `source` 字段区分,非塞进 tracing target——同 S2 derive/pipeline.rs 教训);`logging.rs` FieldCollector 新增 `frontend_context` 特判(JSON 字符串解回真对象,落 `attributes.context`,同 error_chain 惯例);registry.rs 登记。②`src/utils/logger.ts`(§9.3-E 定型):模块级队列,2s/50 条阈值 flush,`setLoggerEnabled` 由 configStore 在 `loadConfig`/`setLogLevel` 同步 off 档;`installGlobalErrorHandlers()` 挂 window error/unhandledrejection,已在 main.ts 挂载;`logger.spec.ts` 11 测覆盖入队/flush/off档/全局兜底(node 环境无 DOM,`window` 用 `vi.stubGlobal` 打桩,模块用 `vi.resetModules()` 逐用例换新实例防 setInterval 状态跨用例污染)。③harness/ipcFixtures.ts 补 LOG_FRONTEND_EVENTS 分支。④约 100 处(设计稿估的 84 处随开发新增)裸 console.* 机械替换为 logger.*(34 文件),`uiStore.ts` 近 20 处 `.catch(console.error)` 收敛为 `logConfigSaveError(key)` 辅助函数;`epubScriptGate.ts`+spec 同步改 mock 目标。⑤刻意保留 console 三处(eslint no-console 规则 ignores):logger.ts 本体、useGalleryPerfProbe.ts(localStorage dev 门控性能探针)、harness/ipcFixtures.ts(仅 UI dev harness 场景)。
- 验证:cargo test 792 passed(净增1,`emit_frontend_log_event_writes_envelope_and_context`)、clippy 零新增;前端 eslint/vue-tsc/vitest(1291 passed,净增11)全绿;两个 commit(3c996a2 基础设施+onerror,d92c51b console 替换+eslint 规则)。
- reviewer 深审(c6b54a4..d92c51b):1 严重+1 存疑,均已修复,详见 progress.md 会话段。严重项=off 档联动此前仅在 configStore.loadConfig(设置页才调)同步,典型会话从未生效——修复=`log_level` 并入 `get_startup_config`(R2-4 单批模式,25 键)+ uiStore 启动 hydration 早期同步。存疑项=两处 dev 调试诊断误从 console.info 降级 logger.debug,改回 logger.info。
- **状态:** complete(测试覆盖缺口:uiStore.ts 无既有 spec 基建,本次未补自动化测试锁定 hydration 路径,详见 progress.md)

### 阶段 9:S4 独立日志窗口 MVP(Sonnet 5,方案 §9.4 S4)
- 已完成:①UI 环形缓冲层——`logging.rs` 抽 `build_envelope` 纯函数供 JSONL 文件层与新增 `RingBufferLayer` 共用(防两处手写字段收集逻辑漂移);`LogRingBuffer`(`AtomicBool` 订阅标志+`Mutex<VecDeque>` 有界队列,20k 安全阀,§9.3-D);`lib.rs` 接入 Layer 到 registry 链 + 新增 100ms 周期抽干任务(空批跳过 emit,纳入既有 `handles_pool` 生命周期管理);`AppState` 新增 `log_ring` 字段(构造参数注入,同 `log_dir` 姿态)。②独立日志窗口——`ipc/log_commands.rs` 新增 `open_log_window`(既有则 show+focus,否则建窗+订阅置真+挂 `WindowEvent::Destroyed` 回调复位订阅)/`list_log_files`/`read_log_file_page`;`capabilities/logs.json` 独立窄权限文件(仅 `core:default`,不复用 default.json 的 shell/dialog 等主窗口专属权限)。③前端——`main.ts` 按 Tauri 窗口 label 分流出 `LogWindowView` 独立挂载路径(跳过 AppShell/router);`logWindowStore`(实时流+Pause/Resume/Clear+级别/target/文本过滤+历史累积分页);`LogVirtualList.vue` 用 `@tanstack/vue-virtual`(新增依赖)的 `anchorTo`/`followOnAppend`/`measureElement` 三原语实现跟随底部+上滚退出跟随+跳到最新,`getItemKey` 用 `_seq`(单调递增,非 index)防裁剪导致的 key 漂移;设置页 debug 区加「打开日志窗口」入口按钮。
- 验证(施工首版):cargo test 792→801(净增 9);clippy 零新增(唯一 pre-existing `scan_commands.rs:96`);前端 eslint/vue-tsc 零错误,vitest 1291→1304(净增 13)。
- reviewer 深审(方案 §9.5):0 严重、5 警告,全部已修——(a) `capabilities/default.json` 把 `"logs"` 直接并入 `windows` 数组会让日志窗口继承 shell:allow-open/dialog 等主窗口专属权限,改建独立 `capabilities/logs.json`(仅 `core:default`);(b) `read_log_file_page` 原按「距文件当前末尾的相对偏移」分页——若浏览的是仍在持续写入的当日文件(`list_log_files` 默认排序里最新那个),两次调用之间新追加的行会让下一页整体前移、与上一页产生重叠,改为**绝对行号锚点**(`before_line: Option<usize>` + 响应携带 `oldest_loaded_line` 原样带回下一次请求),Rust 侧抽 `slice_page_bounds` 纯函数 + 3 单测锁定「锚点跨文件增长保持稳定」+「文件变短时钳制不 panic」;(c) `main.ts` 的 `getCurrentWindow()` 在裸浏览器 dev(无 `?ui-harness=` 参数、非 Tauri 环境)下会同步抛错致整段脚本中断白屏,补 `isTauri`(`'__TAURI_INTERNALS__' in window`)兜底;(d) `logWindowStore` 暂停期间的 `pendingEntries` 原无长度上限(后端环形缓冲对"暂停"无感知持续推送),补与 `appendLive` 同口径的 renderCap FIFO 裁剪;(e) `LOG_LEVELS` 漏了 `trace`(settingsMap 的 logLevel 选项含 trace,但日志窗口过滤集合没有,选 trace 档后这些行永久被过滤且无勾选框可加回),补齐并配 `.log-row__level--trace` 样式。
- 验证(修复后复测):cargo test 804 passed/0 failed/6 ignored(净增 12,含 3 个 `slice_page_bounds` 单测);clippy --all-targets 零新增;前端 eslint/vue-tsc 零错误,vitest 1307 passed(净增 16,`logWindowStore.spec.ts` 16 测覆盖过滤/Pause-Resume-Clear+裁剪/renderCap 钳制/历史分页锚点稳定性/trace 级别)。commit 8fe0555。
- 施工中的环境坑(记录避免复踩):`cargo fmt`(无论是否带 `-- <file>` 参数限定)在本仓这台机器上会重排**整个 crate**,不止目标文件——两次误将 backup/core.rs、export_commands.rs 等无关文件带出格式漂移,均已用 `git checkout --` 撤销复原(未代修,基线红属其它线)。此后只用 `cargo fmt --check`(只读)核实我方文件区,不再对本仓执行 `cargo fmt` 写入模式。
- **状态:** complete;真机 GUI 验收(开窗见实时流/过滤生效/10k 行滚动/关窗后订阅复位)留待 S6 前统一批量验收,不单列

### 阶段 10:S5 分析(Sonnet 5,方案 §9.4 S5,P1→P2)
- 开工前置裁决:方案 §5 P2「span 耗时 Top N 表」现状核实——全仓零 tracing span 埋点(`grep info_span!|debug_span!|#\[instrument\]` 零命中),该子功能无数据源,方案与现实冲突(方案 §9 前言「三停规则」之一)。呈报用户三选一(本轮跳过 defer S6 / 最小埋点 4 条流水线批次 span / 更大范围含 IPC command 级 span),用户选**本轮跳过 defer S6**——待 S6 前先裁定具体埋点范围(缩略图/AI/人脸/视频批次是候选),不在本阶段临时拍板埋点范围。
- 已完成(P1):①丢弃计数——lib.rs 捕获 `non_blocking.error_counter()`(`tracing_appender::non_blocking::ErrorCounter`,内部 `Arc<AtomicUsize>`,`Clone` 廉价),新增 `AppState.log_dropped_counter` 字段(构造参数注入,同 `log_ring` 姿态)。②错误去重快照——error.rs 新增 `ErrorDedupSnapshotEntry`/`error_dedup_snapshot()`,只列当前窗口内 `swallowed>0` 的签名(避免正常签名淹没面板)。③`get_log_diagnostics` IPC 命令(log_commands.rs)聚合上述两者,纯内存读取不 spawn_blocking。④前端 `logWindowStore.ts` 加正则搜索模式(`textFilterMode`,非法正则降级为不过滤+`textFilterInvalid` 提示,不静默清空视图)、过滤 preset(localStorage 持久化,`saveCurrentAsPreset`/`applyPreset`/`deletePreset`,同名覆盖不重复堆积)、`refreshDiagnostics()`。⑤LogWindowView.vue 新增「分析」第三标签:诊断面板(丢弃计数+压缩表格)、错误聚合表(见下)、preset 管理列表、正则开关按钮(`.` 图标切换)。
- 已完成(P1,错误聚合):`src/utils/logAggregation.ts` 新增 `aggregateErrorEntries()`——按 `attributes['error.code']` 分组计数+首末时间+样本(复用 AppError 稳定 code 契约,方案 §5 P1 明文理由)。
- 已完成(P2):①`compute_log_histogram` IPC 命令(log_commands.rs)——按方案 §5 P2 定型「JSONL 按需导入 rusqlite 临时表,Rust 侧 GROUP BY strftime 分桶」:每次调用开一个独立于主库连接池的短生命周期内存连接(`rusqlite::Connection::open_in_memory()`,spawn_blocking 内),核心逻辑抽成纯函数 `compute_histogram_from_content`(可单测,不必搭 tauri State,同 S4 `slice_page_bounds` 惯例);`strip_tz_suffix` 裁掉 RFC3339 时间戳的显式数字时区后缀(`EnvelopeFormat` 恒 6 字符 `+HH:MM`,从不落 `Z`)得到朴素本地时间字符串再喂 `strftime`——若带偏移量直接喂,SQLite 日期函数会先转 UTC 再分桶,桶边界相对用户在窗口里看到的本地时间挪移一个时区差。②前端 `LogHistogramChart.vue`(新增,手写内联 SVG 堆叠柱状图,不引图表库,同 §9.1 护栏 #1 精神)+ LogWindowView.vue 直方图控件(文件选择+小时/天粒度+生成按钮)+ 级别分布图例(客户端从 buckets 聚合,不额外 IPC)。③`export_diagnostics_package` IPC 命令——system-info(应用版本/OS/架构/GPU **设置值**非硬件探测/DB 大小经 `PRAGMA page_count*page_size`/扫描根数量)+ 最新日志文件尾部(2000 行)+ 脱敏后打 zip,`.tmp` 再同卷 rename(项目硬约束)。脱敏——`logging.rs::redact_diagnostics_text`,只脱敏 Windows/Unix 用户主目录路径的用户名段(不脱敏其余路径,方案 §7 Q3 裁决理由:本地日志明文对自查/agent 调试价值大,脱敏只在诊断包这个"出机"通道口做)。④前端诊断包导出区(说明文字+导出按钮+导出后"打开所在目录")。⑤双窗格上下文(klogg 范式)——`logWindowStore.ts` 新增 `selectEntry`/`contextView`(选中一条可见行,在同一来源的**未过滤**数组里找到它并展开前后 15 条,零额外 IPC 往返);`LogRow.vue` 加点击 emit + 选中高亮;LogWindowView.vue 底部上下文面板(点行可重新聚焦、Esc 等效关闭按钮)。
- reviewer 深审(方案 §9.5):1 严重 + 3 警告 + 2 建议 + 1 存疑,全部处理——**严重(已修)**:`redact_diagnostics_text` 早期实现直接对序列化后的 JSONL **原始字节**跑正则,但 JSON 转义会把路径分隔符 `\` 编码成 `\\`(双写),正则按单反斜杠匹配,对真实落盘数据(如 `path = %cache_dir.display()` 一类结构化字段写入的 Windows 路径)**完全不命中**——诊断包（明确的"出机"数据通道）会把真实用户名明文带出,三条早期单测因只喂了未转义裸字符串而给出假绿灯。修复:改为逐行解析 JSON、递归脱敏字符串叶子节点(`redact_json_value`)、重新序列化(解析失败的史前纯文本行走原文正则 fallback);新增两条回归测试(`redact_diagnostics_text_masks_username_inside_real_jsonl_line` 用真实 `serde_json::to_string` 产出验证、`redact_diagnostics_text_handles_mixed_jsonl_and_plain_lines` 验证混合输入)。**警告(已修)**:①`compute_histogram_from_content` 的 `row.get::<_, String>(0)` 读到 `strftime` 对无法解析时间戳返回的 SQL NULL 会 `Err`,经 `collect::<rusqlite::Result<_>>()` 短路成**整份查询失败**(而非"该行不进桶"的原意),SQL 补 `WHERE strftime(?1, ts_naive) IS NOT NULL` + 新增回归测试;②`aggregateErrorEntries` 的 firstTs/lastTs 依赖"输入按时间正序"假设,但 View 层拼接 `liveEntries.concat(historyEntries)` 在浏览当天仍被写入的同一文件时会时间倒挂,改按时间戳字符串比较取 min/max(RFC3339 固定宽度+同会话固定时区偏移,字典序即时间序)+ 新增乱序输入回归测试;③诊断包导出区原无任何说明(方案 §7 Q3 把"用户把文件发给别人"的残留风险交给 UI 导出提示承担),补一行说明文字(含 GPU 为设置值+仅脱敏用户名段的措辞)。**建议(已修)**:①`LogHistogramChart.vue` 的 `shortLabel` 原按字符串形状(是否以 `T00:00:00` 结尾)猜测天/小时粒度,小时粒度里恰好落在 00 时的桶会被误判——改为父组件显式传入 `bucket` prop(`store.histogramBucketUsed`,记录"生成这批数据时实际请求的粒度"而非"下拉框此刻的值",两者可能因用户改了选择但未重新生成而不同);②`total_lines`/`parsed_lines`(方案设计的"覆盖率可见化")此前只在后端算好从未渲染,补直方图区一行"已解析 X / Y 行"。**存疑(已评估,轻量兜底)**:诊断包文件名精确到秒,同秒内两次调用(如误触重复点击)会撞同名 tmp 路径——`UiButton` 的 `loading` prop 已隐式 `disabled`(`exporting` ref 在导出期间禁用按钮),正常单击路径已被挡;补时间戳到毫秒精度(`%.3f`)进一步缩小理论窗口,不加计数器/随机后缀(收益不值当复杂度)。
- 验证(reviewer 修复后终态):`cargo test --lib` 816 passed/0 failed/6 ignored(较 S4 收尾的 804 净增 12:P1/P2 新增 9 + reviewer 修复新增 3);`cargo clippy --all-targets` 零新增(唯一 pre-existing `scan_commands.rs:96`);`cargo fmt --check` 对本阶段实际改动的 6 个后端文件(error.rs/logging.rs/log_commands.rs/registry.rs/lib.rs/state.rs)核实,其中 logging.rs/log_commands.rs 的行宽格式化点已按 rustfmt 偏好手工对齐(未对整仓执行写入模式,踩过的环境坑见 progress.md);前端 `npx vue-tsc --noEmit`/`npx eslint .` 零错误,`npx vitest run` 1328 passed(较 S4 的 1307 净增 21:P1/P2 新增 20 + reviewer 修复新增 1)。
- **状态:** complete;真机 GUI 验收(分析标签三区块+直方图渲染+诊断包导出实测)留待 S6 前统一批量验收,不单列

### 阶段 11:S6 收尾(Sonnet 5,方案 §9.4 S6)
- 开工前置裁决(呈报用户,详见 AskUserQuestion 记录):①worker 子进程日志汇入策略——摸底汇报(ai-worker/psd-worker 均走 stderr,低频,主进程 `supervisor.rs` 已有 64KiB 环形缓冲+异常时 `tracing::warn!` 摘尾部,正常运行不落盘)后三候选(现状已足够不额外施工/各写各文件 RollingFileAppender/IPC 汇入主 JSONL)用户选**现状已足够,不额外施工**(D-308)。②span 耗时 Top N 埋点范围(S5 遗留)——三候选(4 条流水线批次 span/更大范围含 IPC command 级/本期继续跳过 defer 独立任务)用户选**本期继续跳过,defer 到独立任务**。两项均为零代码变更结论。
- 已完成:①摸底调研(Explore 子代理只读调研 supervisor.rs/ai-worker/psd-worker,厘清与 bin/ 下 sort_profile/worker_e2e 诊断二进制的边界——后者确认无关,不在本线摸底范围)。②方案文档回写:状态行标注 S1-S6 全部落地;§3.4 open item 改写为 D-308 已闭环说明;§6 施工分期表 S6 行补勾;§8 defer 清单补 span TopN 与 worker 两条终态说明;§9.4 S6 分步补完成注记+DoD 实测数字。③CI 接线检查:核对 `.github/workflows/ci.yml` 的 `rust`(cargo fmt --check/check/clippy/test --workspace)与 `frontend`(eslint/vue-tsc/vitest/build)两个 job 均按 workspace/仓库根扫描,本线新增文件(logging.rs/log_commands.rs/logWindowStore.ts 等)天然被覆盖,无需新增 job 或改动 workflow。④检查中意外发现 `src-tauri/benches/logging.rs`(S1 引入)有 4 处 rustfmt 漂移,从未被 `--check` 覆盖过(S1 未跑、S2 的 `--check` 范围未含此文件)——若 push 后触发 CI,`cargo fmt --check` 步骤会红。判定为本线自身遗留(非其它线基线红),逐处手工对齐修复(未对整仓跑 `cargo fmt` 写入模式,规避已知环境坑);同批 `cargo fmt -p scrollery -- --check` 扫出的另外 4 个文件(backup/core.rs、backup/restore.rs、db/queries/export.rs、export/core.rs、ipc/export_commands.rs)经 `git log` 核实最后触碰提交均不属本线,判定为其它线基线红,未代修。
- 验证:`cargo test --workspace --locked` 816 passed/0 failed/6 ignored(scrollery_lib,较 S5 终态无变化,本阶段零功能代码改动);`cargo clippy --workspace --locked -- -D warnings` 唯一 1 处 pre-existing `scan_commands.rs:96` manual_inspect(与本线无关,历次 S1-S5 均如此,未代修);`cargo fmt -p scrollery -- --check --files-with-diff` 复跑后本线文件(含修复后的 benches/logging.rs)零漂移;`cargo check --workspace --locked --benches` 通过,确认 bench 手工格式化后仍可编译;前端 `npx vue-tsc --noEmit` 零错误、`npm run lint` 零错误、`npx vitest run` 1328 passed(108 files,与 S5 终态一致)。
- **状态:** complete;日志能力重构线全线(S1..S6)收官,待用户批准 push

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 本任务范围=调研+方案,不含施工 | 用户要求「报告和方案落盘」,裁决后才施工 | |
| 调研分三路并行(后端/UI/agent+分析) | 三域独立、并行省时 | |
| 内核保留 tracing 三件套,不引 tauri-plugin-log / OTel SDK / 新兴限流与滚动 crate | 同栈实证(GitButler/Spacedrive)+现有依赖同线;plugin-log 结构化弱;OTel 单机无收集端;新兴 crate BUS 因子 | D-301 |
| 完全关闭=off 档走既有 LOG_RELOAD reload,不卸 Subscriber、不做编译期变体 | 稳态 callsite 缓存 ~1ns 已够;编译期关闭与「运行时可开回」冲突 | D-302 |
| 进度/心跳类高频信息不走日志,走既有事件体系;日志只记状态变化+异常+任务级汇总 | 流水线性能硬要求的源头减量;derivationStore 事件体系已在 | D-303 |
| operation_id 显式字符串传参贯穿 IPC 链,不依赖 span 传播 | span 不跨 spawn_blocking(调研 C 查证),本仓 DB 全走 spawn_blocking | D-304 |
| 待用户裁决 Q1..Q8(格式/级别/隐私/双语/保留/UI 形态/minidump/迁移范围) | 价值判断类,方案 §7 各带推荐 | |
| 用户裁决(2026-07-20):Q1..Q8 全部采纳推荐项 + 主动建议一并采纳 | 用户原话「采纳全部建议」;推荐即终态,详方案 §7 | D-305 |
| 施工由 Sonnet 5 承担,方案增补 §9 施工指引(护栏/陷阱/实现定型/分步 DoD);§9 与前文冲突以 §9 为准 | 弱模型施工需低歧义定型:信封 FormatEvent 取代内置 .json()(信封字段注入)、重复压缩定型调用点辅助非 Layer(Filter::event_enabled+summary 重入复杂度不值) | D-306 |
| dedup 签名退化存疑项裁决:Swallow 臂降档 debug 留证,签名 (target,code) 不动 | 扩签名(message/chain[0])在风暴场景基数爆炸反杀压缩主目的;降档后 error 档保压缩、debug 档保真相,默认 info 档热路径成本不变(tracing 宏 enabled 检查在字段求值前) | D-307 |
| S5 期间核实方案 §5 P2「span 耗时 Top N」全仓零埋点数据源,与现实冲突;用户裁本轮跳过,defer S6 前另裁具体埋点范围 | 方案 §9 前言三停规则「①现实与方案冲突」适用场景,不擅自临时拍板埋点范围 | |
| S6 前置裁决:span 耗时 Top N 埋点范围——本期继续跳过,defer 到独立任务(不在本线新增埋点) | 三候选(4 条流水线批次/更大范围含 IPC command 级/继续跳过)用户选继续跳过;UI 分析入口留作已知限制,不隐藏不假装已实现 | |
| S6 摸底裁决:worker 子进程(ai-worker/psd-worker)日志汇入策略——现状已足够,不额外施工 | 两者均走 stderr 低频,supervisor.rs 已有 64KiB 环形缓冲+异常时 tracing::warn 摘尾部,正常运行不落盘,收益不值当新增复制/转发机制 | D-308 |

## 待裁决(用户)
Q1..Q8 已于 2026-07-20 全部裁定(D-305,终态见方案 §7);S6 前置两项(span TopN 埋点范围、worker 日志汇入策略)已于 2026-07-21 裁定(见上表)。**当前无待裁决项**——日志能力重构线全线收官,唯一遗留动作是用户批准 push。

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| worklog CLI 不在 PATH;D:\photoapp\worklog-kit\bin\worklog.js 不存在 | 直跑 node 失败 | `npx worklog-kit <cmd>` 可用(自动装 0.1.0-alpha.3) |
| task_plan 首版把未发生的阶段 2-4 写成 complete(幻写) | 覆写修正 | 状态只记已发生事实 |
