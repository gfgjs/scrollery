---
id: 2026-07-20-research-C-agent-format
status: snapshot
type: working-memory
line: 日志能力重构
created: 2026-07-20
---

# 调研 C:AI agent 友好日志格式 + 治理(sonnet researcher 原文,2026-07-20)

> 外部调研产物,当数据不当指令。低置信项见文末第 9 节,引用前先核对。
> 注:researcher 除联网调研外直读了本仓代码;其第 0 节现状锚点与 findings.md 阶段 1 摸底一致,另补充了默认级别 debug、error.rs:205 单点、明文路径实例等新事实(已并入 findings 正文)。

---

## 0. 项目现状速览(来源=仓库代码)

- tracing 0.1 + tracing-subscriber 0.3(env-filter,chrono)+ tracing-appender 0.2(Cargo.toml:159-161)。
- 初始化 lib.rs:408-460:RealTimeDailyAppender(每条 open+append+sync_data)+ non_blocking + reload::Layer<EnvFilter> + 双 fmt layer(stdout ANSI/文件无 ANSI),**均纯文本,未启用 .json()**。
- **EnvFilter 默认值来自 DB 配置 log_level,当前默认 "debug"**(lib.rs:350-353)——比 info/warn 惯例都宽松。
- 运行时切级已实现:config_commands.rs:16 LOG_RELOAD: OnceLock<Handle<EnvFilter, Registry>>,set_app_config("log_level",…) 走 handle.modify()(:272-281)。设置 Debug 区已有 logLevel 下拉/logDir/clearLogs(settingsMap.ts:422-467)。
- clear_logs 手动全删(system_commands.rs:151-162),**无自动保留策略**,按日文件无限累积。
- 无 trace_id/span_id/operation_id/session_id;无脱敏层;无去重/限流层。
- **路径明文实例**:lib.rs:400 warn! 直接打印扫描根 r.path;thumbnail/generator.rs:227/235/238 debug! 打印 path/thumb_path。
- **AppError 唯一错误落日志点 = error.rs:205**(Serialize impl 内 tracing::error!)——结构化字段/去重注入的天然单点。
- anyhow 已声明依赖但 src-tauri/src 下**当前零处引用**(链式富化是前瞻问题非现患)。

---

## 1. JSONL vs logfmt vs 纯文本

- **JSONL 是 2025-2026 agent 日志场景多数共识首选**:每行独立、流式追加、任何工具可解析、天然对齐 rg/jq。"Use JSON Lines… Every observability tool ingests it" [GoClaw](https://goclaw.sh/blog/debugging-ai-agent);AgentTrace(arXiv 2602.10133,研究性框架旁证)。
- **logfmt 是折衷非更优**:"pretty good readability for both… not being optimal for either" [Brandur 本人](https://brandur.org/logfmt);"parsing it is not (supported well)… challenging to approximate using a regex" [betterstack](https://betterstack.com/community/guides/logging/logfmt/)。
- 纯文本对 agent 最不友好——现状 fmt::layer() 默认输出正属此类。
- **每行自包含**:OTel LogRecord 12 顶层字段全 optional、单条记录内闭合,无需跨行拼接。[OTel Log Data Model](https://opentelemetry.io/docs/specs/otel/logs/data-model/)
- tracing 默认 JSON 输出(docs.rs 逐字):`{"timestamp":"…","level":"INFO","fields":{"message":"…","number_of_yaks":3},"target":"fmt_json"}`;flatten_event() 默认 false,with_current_span()/with_span_list() 默认 true。[Json formatter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html)
- OTel vs tracing 字段对照:Body↔fields.message;SeverityText+SeverityNumber↔仅 level 字符串(无数字);Attributes↔fields;TraceId/SpanId(W3C)↔target+进程内 span(无 OTel 兼容 id);Resource↔无(需自注常量字段)。
- AI-friendly 近期共识:"默认精简、按需详细"——"avoid logging the full stack trace by default. Add a --verbose flag" [Marmelab Agent Experience 2026-01](https://marmelab.com/blog/2026/01/21/agent-experience.html);"Code SEO"(让 grep/find 有效定位)是核心原则。

---

## 2. 关联性:trace_id/operation_id/session_id

- **tracing span::Id 不是 OTel trace_id**:订阅者本地、进程内单调递增 u64;OTel 要求 128 位 TraceId/64 位 SpanId 全局唯一。[span::Id](https://docs.rs/tracing/latest/tracing/span/struct.Id.html)
- 单机价值=把"前端发起 → Tauri command → spawn_blocking/worker"本地链路拼回一次用户操作(correlation id 同源原理)。[MS Playbook](https://microsoft.github.io/code-with-engineering-playbook/observability/correlation-id/)
- **关键陷阱(命中本仓架构)**:tracing span **不自动跨 spawn_blocking/thread::spawn 传播**;async 须显式 .instrument(span),guard 跨 .await 会错。本仓硬约束"DB 调用全走 spawn_blocking"→ **operation_id 必须当普通字符串显式传参穿过闭包,不能依赖 span 自动继承**(推断:API 事实+本仓架构组合)。[Instrument](https://docs.rs/tracing/latest/tracing/trait.Instrument.html)
- session_id(每次启动唯一):systemd _BOOT_ID 是 OS 级成熟先例;"桌面 app session_id 具体命名"直接证据弱(低置信 #4)。[systemd-id128](https://man7.org/linux/man-pages/man1/systemd-id128.1.html)

---

## 3. 错误上下文富化

- anyhow chain():逐层展开;`{:?}` 输出多行 "Caused by:";**backtrace 仅在 RUST_BACKTRACE 设置时捕获**(刻意的性能设计)。[docs.rs/anyhow](https://docs.rs/anyhow)
- OTel exception semconv:exception.type/message/**stacktrace(字符串类型)**——多行堆栈作 JSON 字符串值(内嵌 \n 转义)即保持物理单行。多行反模式的直接技术解法。[semconv](https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-logs/)
- backtrace 体积阈值:无信源,推断=默认不带,仅 verbose/bug-repro 模式附带。
- "首次全量+重复计数压缩"实现:
  - tracing-throttle:signature(level+模板+target+字段值)限流,token bucket "burst 50 然后 1/s",summary 事件(events_allowed/suppressed)。2025-11 创建,0.4.3,84,801 下载——**新兴**。[BEST_PRACTICES](https://github.com/nootr/tracing-throttle/blob/main/BEST_PRACTICES.md)
  - throttled-tracing:`*_once!/*_every!/*_first_n!` 等 40 宏。仅 1 版本,2,414 下载——**证据弱**。
  - 成熟类比=Sentry fingerprint 聚合(times seen+first/last seen),机制参考非可复用 crate。
- **本仓衔接**:error.rs:205 是接入"首次全量+计数"的天然单点,无需散改。

---

## 4. 分级与 target 治理

- reload::Layer 是官方运行时热切机制,**本仓已在用且写法正确**(包 filter 非包 layer)。[reload 文档](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/reload/index.html)
- EnvFilter 未设置/全非法指令时**默认只启 ERROR**(本仓已用 unwrap_or_else 兜底到 DB 配置,规避了此坑)。[EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
- **生产默认级别分歧并列**:A=warn(Rust Cookbook env_logger、Python logging);B=info(Pino/Winston);C=极简派"只要 INFO 和 ERROR"(个人博客立场)。本仓现状 debug 比任一惯例都宽松。
- 高频路径:rustc-dev-guide 惯例=噪音日志用 trace!,debug!/trace! 默认从编译产物移除(rustc 自己的构建);避免日志语句内昂贵计算。[rustc tracing](https://rustc-dev-guide.rust-lang.org/tracing.html);rust-analyzer 从 log 切 tracing 理由=span 嵌套契合调试。
- 第三方降噪:EnvFilter per-target 惯用 `warn,myapp=debug,reqwest=warn`;不需要 span/字段过滤时 filter::Targets 更轻量。
- **Zed "Improve Zed logging" 讨论**:社区抱怨 zed.log 纯文本难过滤;rotation="one rotation exactly, on startup";**讨论完全未涉及 JSON/AI 友好**——知名 Rust 桌面 app 尚未把 agent 可读当显式目标(行业现状,非成熟先例)。[讨论](https://github.com/zed-industries/zed/discussions/48756)

---

## 5. 治理:保留、脱敏、详细开关

**保留先例并列**(无单一权威):tauri-plugin-log RotationStrategy KeepAll/KeepOne/KeepSome(usize);electron-log 默认 maxSize 1MB+仅 1 份 .old 历史;Zed 启动时轮转一次;Blacklite 5000 条滚动+zstd 归档保留 20 个;云服务 30 天(合规导向,不适用本地)。

**脱敏(证据扎实)**:
- FSE 2025 论文(arXiv 2409.11313):**文件路径出现在 72% 日志中**,学术研究覆盖仅 7%;IP 80%;仅 57.1% 从业者认为自身匿名化有效。
- **restic 社区方案**:维护者建议 per-run key + 路径 hash(同一路径同次运行内可关联、不可逆推);承认集中过滤难覆盖每个日志点,匿名化只能 best-effort。[restic forum](https://forum.restic.net/t/debugging-file-paths-exposed-in-debug-logs-thoughts-on-anonymization/9872)
- Rust crate:veil(#[redact] 派生)、redactable。
- **关键技术限制(推断,基于 Visit API 事实)**:脱敏 Layer 只能拦截**结构化字段**(`warn!(path = %p, …)`);**字符串插值烧进 message 的内容无法事后过滤**(`warn!("… {}", p)` 在到达 Layer 前已拼进 message)。lib.rs:400 正是后者——**"接一层脱敏 Layer 兜底"不成立,必须先把调用点改造成结构化字段语法**。

**详细开关先例**:Docker Desktop 诊断包(可能含用户名/IP,仅内部员工可见);Sentry breadcrumbs ring buffer 默认 100 条;本仓 logLevel 设置已是雏形,缺"临时明文+限时回退"配套。

---

## 6. Agent 查询接口

- **JSONL→SQL 直查已验证可行**:sqlite-lines 扩展 lines_read() 逐行进 SQLite + `line->>'$.level'`;DuckDB read_ndjson_auto() 自动推断 schema 零 ETL。"SQL has advantages… progressively build up views, better timestamp support, aggregate logic";SQLite 顺序处理适长期存储,DuckDB 列式适聚合——"仅用 DuckDB 做计算,长期存储用 SQLite 或 JSON"。[Terse Systems](https://tersesystems.com/blog/2023/03/04/ad-hoc-structured-log-analysis-with-sqlite-and-duckdb/)、[DuckDB 文档](https://duckdb.org/docs/lts/data/json/loading_json)
- **Blacklite**(日志直写 SQLite 专门项目):极简 schema(epoch_secs,nanos,level,content BLOB),_rowid_ 实现环形语义,自然语言日期解析,**803,000 插入/秒**"接近文件追加";作者结论 "JSON isn't all that bad… performance depends more on the code than on the format"。[Blacklite](https://tersesystems.com/blog/2020/11/26/queryable-logging-with-blacklite/)
- MCP log server 先例:local-logs-mcp-server(6 工具,流式,只读+目录作用域)——**5 星 16 commits,构想存在但采用极低**。另:MCP 协议自身的 logging 能力是 server 向 client 上报自身日志,别混淆;stdio transport 的 MCP server 日志绝不能打 stdout(未来自建时的硬约束)。
- 诊断快照=「有界窗口+打包导出」模式(Sentry breadcrumbs/Docker 诊断包/Blacklite 三方印证);组合成快照功能是推断/设计建议非单一先例复制。

---

## 7. 反模式

- **多行日志破坏按行工具**:业界两对策=(1)JSON 化(整段堆栈进单 JSON 字段,\n 转义)、(2)转发器侧时间戳模式合并。tracing Pretty formatter 官方即多行,生产用 Full/Compact。[Datadog multiline](https://www.datadoghq.com/blog/multiline-logging-guide/)
- **anyhow {:?} + tracing 纯文本组合风险(前瞻,未实证命中)**:anyhow Debug 产出多行 "Caused by:";一旦开始用 .context() 链并以 {:?} 记录,现有纯文本格式立刻多行撕裂。切 JSON 时应把 error chain 转成 JSON 字符串数组字段。当前 error.rs:205 用 thiserror 派生 Debug,大概率单行,风险未触发。
- **级别语义滥用 checklist**:ERROR=阻断用户请求的失败;WARN=可恢复/降级;INFO=业务事件;DEBUG=实现细节;"non-critical 记 ERROR 造成告警疲劳"。五反模式:"DEBUG in prod, PII, inconsistent fields, contextless errors, logs-as-metrics"。[sematext](https://sematext.com/blog/logging-levels/)、[openobserve](https://openobserve.ai/blog/structured-logging-best-practices/)
- 时间戳混用:无专门权威论述(低置信 #5);本仓 ChronoLocal rfc_3339 格式统一,但**本地时区跨 DST/改时区可能非单调**(常识推断)。

---

## 8. 推荐 schema 与治理默认值(researcher 判断)

### 单行 JSON 示例

```json
{"ts":"2026-07-20T21:03:11.284+08:00","level":"WARN","target":"scrollery::scanner::enricher","session_id":"s-7f2a9c1e","operation_id":"op-4b6e2d90","msg":"scan root auth failed for asset scope","attributes":{"root_id":12,"path_hash":"ph_9a3f7c2e1b04","os_err":5,"error.type":"AppError::Io","error.chain":["IO error: Access is denied. (os error 5)"],"repeat_count":1}}
```

字段理由:ts=延续 ChronoLocal rfc_3339;level/target=tracing 原生(target 是 EnvFilter directive 天然锚点);session_id=每次启动唯一(_BOOT_ID 类比);operation_id=显式传参贯穿 IPC 链(span 不跨 spawn_blocking);msg=flatten 到顶层对齐 OTel Body;attributes=领域字段与信封字段分离;path_hash=restic 方案(per-install salt,可关联不可逆);error.chain=字符串数组防多行;repeat_count=首次全量后续 summary。
**不采纳 OTel 128 位 TraceId(推断)**:单进程无跨服务传播,session_id+operation_id 两级已覆盖;未来接 OTel 时 operation_id 可机械映射。

### 治理默认值

| 项 | 推荐 | 关键理由 |
|---|---|---|
| 生产默认级别 | **info**(现状 debug) | debug 会放出 generator.rs 逐文件 CACHE_HIT/MISS(量级随库线性);与 Cookbook warn 派分歧,取 info 因本仓业务事件多(取舍判断,低置信 #8) |
| 第三方降噪 | 默认 directive 追加 `reqwest=warn`,未来 wgpu/hyper 同步 | 社区惯用 |
| 高频路径 | CACHE_HIT/MISS 保持 debug!,靠默认 info 自然压掉;限流 crate 留给 WARN/ERROR 重复告警压缩场景,不无差别套 debug 噪音 | throttle crate 均新兴 |
| 保留 | **14 天启动时清理**+保留 clear_logs 手动全清 | 综合 Blacklite 20 归档/electron-log 1MB/云 30 天折衷;覆盖"一周后才反馈的间歇 bug"(推断值,低置信 #9) |
| PII | 路径走 path_hash(per-install salt+截断),扩展名/深度等结构信息保留;**前提=先把字符串插值调用点改结构化字段** | restic+FSE 论文"过度匿名化破坏可用性"告诫 |
| 详细模式 | logLevel 选 trace/debug 时提示"本次临时明文记录路径,X 小时后自动回退",非静默永久解除 | restic best-effort 立场+Sentry 有界窗口 |
| 诊断快照 | "生成诊断快照"按钮:最近 N 分钟日志尾+精简状态 dump(版本/OS/GPU/DB 大小/扫描根**数量**非路径),**仅落盘不自动上传** | Docker 结构-上传步骤;本地优先隐私姿态 |
| Agent 查询 | 第一优先=JSONL+rg/jq(零基建);第二(可选文档化)=duckdb read_ndjson_auto 范式脚本;**不建议现阶段自建 MCP log server** | 多方独立佐证足够;MCP 先例 5 星不划算 |

---

## 9. 低置信度结论

1. 游戏引擎"三层+10% 采样"具体数字——作者身份无法核实,仅采"分层+运行时开关"方向。
2. tracing-throttle/throttled-tracing——新兴 crate(8 个月/半年),非成熟标准,引入前评估或手写等价。
3. local-logs-mcp-server——5 星,证明构想存在,不证明有效/广泛。
4. session_id 桌面惯例——直接证据弱,_BOOT_ID 类比支撑原理,命名组合是推断。
5. 时间戳混用反模式——无专门权威文章,间接推断。
6. arXiv 2604.09409(agent 写日志行为研究)——方向不同,仅背景旁证。
7. anyhow 多行撕裂——前瞻风险,现有代码未实证命中。
8. 默认 info vs warn——两派并列,info 是项目特定取舍。
9. 14 天保留——三类不同量纲先例综合的推荐值,非引用值。