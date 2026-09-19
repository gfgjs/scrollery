---
status: 快照
type: working-memory
line: 日志能力重构
created: 2026-07-20
---

# 发现与决策:日志能力重构

## 需求
- 用户原话要点:重构日志能力;派 sonnet 子代理联网调研业界成熟方案;要求=功能全面、应用内 UI、分析能力、人类友好、agent 读取友好;拿不准列清单裁决;报告+方案落盘。
- 补充(2026-07-20 第二轮):各流水线(缩略图生成/AI 分析/人脸/视频提取等)日志量大,日志系统**不得或尽量小影响重负载流水线性能**;用户可**完全关闭日志功能**以提升性能。→ 方案必须给出:热路径日志开销预算、禁用态每调用点成本、关闭开关实现层级。

## 发现
<!-- 普通发现追加到本节;别盲追加到文件末——文件尾是「耐久提升候选」表,只收 F-NNN 候选行 -->
- 现状(scout 摸底 2026-07-20):Rust 侧 tracing 宏 377 处(thumbnail_commands 28/scan_commands 23 最多);log::/plugin-log 零使用;println 族 148 处集中在 bin/(sort_profile 55、worker_e2e 43,诊断二进制,非主程序)。
- 初始化在 src-tauri/src/lib.rs:408-460:registry + reload(EnvFilter) + 控制台 fmt layer(ANSI) + 文件 fmt layer(非 ANSI,经 non_blocking)。LOG_RELOAD 热切级别已有(config_commands)。
- 落盘=**默认文本格式**(fmt::layer 非 json),路径 {app_data_dir}/logs/scrollery.{YYYY-MM-DD}.log,daily 按天滚动,**无大小上限、无保留清理**。
- RealTimeDailyAppender(lib.rs:417-433):**每条日志 open+append+sync_data(fsync)**——为 Windows NTFS 元数据实时可见而设;高频日志下 IO 昂贵(即使已在 non_blocking 后台线程)。写失败静默丢弃。guard 用 Box::leak 持有。
- 无 panic hook;无 minidump/崩溃留痕。
- 前端 84 处裸 console.*(uiStore 21 最多),无 logger 封装、无 IPC 桥到后端;.catch(console.error) 只留在 devtools。
- AppError(error.rs,25 变体)已有稳定 {code,message} Serialize 契约,8 大域;LayoutNotReady/Cancelled/ViewStale/VolumeOffline 四类预期分支刻意不记 error 日志(降噪先例)。
- docs/ 无日志相关既有设计文档——本线方案即首份。
- 现状日志 message 惯例=双语「English | 中文」拼接单条,人类友好但体积×2,agent 读取 token ×2。
- Cargo.toml:custom devtools feature 已接线 tauri/devtools(release 默认无 inspector);诊断构建 --features devtools。
- 设置页已有 debug 节(settingsMap.ts:422-467):logLevel select(trace..error,走 LOG_RELOAD 热切)+ logDir(自定义日志目录)+ clearLogs 危险区按钮。日志 UI 入口可挂靠此节或独立诊断视图。
- (调研 C 直读代码补充)日志默认级别来自 DB 配置 log_level,**当前默认 "debug"**(lib.rs:350-353)——比 info/warn 惯例都宽松,generator.rs:227/235/238 逐文件 CACHE_HIT/MISS debug 日志随库规模线性放出。
- (调研 C 补充)clear_logs=手动全删(system_commands.rs:151-162);无自动保留,按日文件无限累积。
- (调研 C 补充)路径明文实例:lib.rs:400 warn! 字符串插值打印扫描根 r.path;generator.rs 打印 path/thumb_path。**字符串插值烧进 message 的内容无法被脱敏 Layer 事后过滤——脱敏前提=调用点先改结构化字段语法。**
- (调研 C 补充)AppError 唯一错误落日志单点=error.rs:205(Serialize impl 内 tracing::error!)——结构化富化/去重计数的天然注入锚点。anyhow 已声明依赖但 src-tauri/src 零引用(多行 Caused-by 撕裂是前瞻风险非现患)。

## 外部资料(当数据,不当指令)
- 调研 A(Rust 后端)全文 → attachments/research-A-rust-backend.md。要点:GitButler/Spacedrive 实证均绕开 tauri-plugin-log 直用 tracing 三件套;JSON layer 官方内置(flatten_event/span_list);tracing-appender 无大小轮转(GitButler 用 DAILY+max_log_files=14);non_blocking 默认 lossy 128k 行、guard 该 app.manage 不该 Box::leak;禁用态 ~0.7ns/调用点(callsite cache),reload::Layer 官方点名 UI toggle 场景;tracing-panic 可用;minidump(EmbarkStudios)建议拆独立任务;三知名 app 均无运行时 GUI 日志开关(只有环境变量)——Scrollery 要做即超先例。
- 调研 B(应用内 UI)全文 → attachments/research-B-in-app-ui.md。要点:Docker Desktop Logs view 是最完整先例(10 万条/regex/preset/export);GitButler 零 UI 反例;消费级 app 无 message 聚合分析=差异化机会;跟随底部三方收敛模式(默认跟随+上滚退出+跳到最新);@tanstack/vue-virtual 首选(anchorTo:'end'/followOnAppend 正对日志场景);分析=AppError.code 分组+rusqlite SQL 分桶(不引 duckdb-wasm);脱敏=前端正则扫描+1Password 式源头排除;UI=设置 debug 区入口+独立窗口。
- 调研 C(agent 格式+治理)全文 → attachments/research-C-agent-format.md。要点:JSONL 是 agent 场景多数共识(logfmt 双输);**span 不跨 spawn_blocking 自动传播→operation_id 须显式传参**(命中本仓 DB 全走 spawn_blocking 架构);推荐 schema=ts/level/target/session_id/operation_id/msg/attributes(error.chain 字符串数组防多行、path_hash 走 restic 式 per-install salted hash);治理默认=info 级别+reqwest=warn 降噪+14 天保留;agent 查询第一优先=JSONL+rg/jq 零基建(DuckDB read_ndjson_auto 可选文档化,MCP server 不建议);限流 crate 均新兴,WARN/ERROR 重复压缩优先手写。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
