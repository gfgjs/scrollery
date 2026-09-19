---
status: 快照
type: working-memory
line: span埋点与worker日志汇入
created: 2026-07-21
---

# 任务计划:span埋点与worker日志汇入

## 目标
日志能力重构线两项 defer 的独立后续:①给「span 耗时 Top N」补数据源——流水线批次+主要 IPC command 级耗时埋点,并建前端 TopN 聚合与表格;②worker 子进程(ai-worker/psd-worker)stderr 日志经 supervisor 转发进主进程 tracing/JSONL 体系,UI 可见。全程复用既有信封管线(EnvelopeFormat/build_envelope/RingBufferLayer),不引新依赖。

## 当前阶段
阶段 1..5 全部完成,全线收官。遗留=用户批准 push(五 commit)+真机 GUI 验收(progress 手测清单)。

## 阶段

### 阶段 1:摸底+施工方案
- 已完成:两路 Explore 摸底(logging 基建+span 链路;supervisor+双 worker),关键事实落 findings;方案定型见「关键决策」D-311..D-314
- **状态:** complete

### 阶段 2:W1+W2 span 线(单 implementer,两 commit)
- 已完成:logging.rs 新增 SpanTimer(info/debug 两档 RAII 守卫)+ 4 条单测(信封字段齐全/operation_id 提升/debug 档 info-filter 下零输出/panic 路径 panicked=true);三处流水线 run 级埋点(pipeline:ai/pipeline:face/pipeline:derive,derive 附 operation_id)落地,既有完成/失败汇总日志不动;IPC command 埋点终表落地(info 39 个、debug 7 个,逐 command 核实清单+存疑/移出项见 findings「IPC 埋点候选」节);前端 aggregateSpanDurations(logAggregation.ts)+ 8 条 vitest 单测 + LogWindowView.vue 分析标签新增「span 耗时 Top N」表(live+history 合并,空态文案「无 span 数据(部分埋点仅 debug 档记录)」)+ 中英双语 i18n 键。验证:cargo test --workspace --locked 全绿、clippy 零新增(唯一 pre-existing scan_commands.rs:96,行号随插入偏移至 98 非新增)、fmt 触碰文件零漂移(logging.rs 手工对齐两处;export_commands.rs 既有漂移与本线无关未动)、bench off=106.22ms/info=106.64ms(+0.40%)/debug=106.02ms(-0.19%)均 <1%;前端 vue-tsc/eslint/vitest(108 files/1336 tests)全绿
- **状态:** complete

### 阶段 3:W3+W4 worker 线(单 implementer,两 commit)
- 已完成:exotic-protocol 新增 `WorkerLogLine{lvl,msg,fields}`+`emit`/`emit_stderr_log`(stderr_log.rs,round-trip/fields 缺省 2 单测)不 bump PROTOCOL_VERSION;ai-worker `log()` 拆 4 档(info/warn/error/debug)22 调用点逐点定级+`tracing_subscriber::fmt()` 换自定义 `WorkerLogFormat` 收编 ai-core 内部 tracing 日志(D-314,新增直接依赖 `tracing="0.1"`——自定义 FormatEvent 硬需求,已在依赖树同版本解析、零新增外部代码,见 findings 说明)+`serde_json`;psd-worker 同款 4 档拆分 10 调用点定级+新增 `serde_json` 依赖;supervisor.rs 新增 `LineScanner`(纯函数,残段上限 64KiB)+`parse_worker_log_line`+`emit_worker_log_line`(固定 target `scrollery::worker`+`worker`/`worker_context` 字段,五臂 match+未知 lvl 兜底)+`forward_stderr_line`,`spawn_stderr_drain` 加 `worker_kind` 形参(由 `worker_kind_label(&WorkerSpec.expected_worker_id)` 派生,未碰 worker.rs/WorkerSpec);logging.rs `FieldCollector` 新增 `worker_context` 特判(同 frontend_context 姿态,落 attributes.context)。定级清单+施工细节见 findings.md「worker 线施工落地」节。
- **状态:** complete

### 阶段 4:reviewer 深审+修复
- 已完成:深审 915eba9..eae75e9 四 commit,0 严重/1 警告/3 建议/1 存疑。警告+三建议全修(单 commit):①ai-worker batch.rs 漏收编——4 处裸 eprintln 收编 WorkerLogLine(推理/单项失败=warn、批诊断=info,原状会被 supervisor 以 WARN+unparsed 兜底、正常批诊断抬成 WARN 噪音);②supervisor spawn_stderr_drain 的 Err 读错误臂补 scanner.finish() 残段冲出(与 EOF 对称);③SpanTimer duration_ms 由 as_millis u64 截断改 as_secs_f64()*1000.0 发 f64(debug 档 <1ms 查询不再恒 0,FieldCollector::record_f64 落 JSON number,前端判据不变);④FieldCollector frontend_context/worker_context 两臂合并(消第三份拷贝)。存疑项裁 D-315 不加 outcome。验证:cargo test --workspace 835/27/14/8 全绿、clippy 零新增(唯一基线 manual_inspect)、fmt --check 本线文件零漂移(仅既有其它线 5 文件漂移未动)
- **状态:** complete

### 阶段 5:收尾
- 已完成:方案文档五处正文改写(状态行/§3.4 worker 条目 D-310 取代 D-308/§5 P2 span TopN 已落地注/§6 分期表补「后续独立任务」行/§8 两 defer 条目改已落地——均正文非脚注);todo.md 日志线条目正文改写+新增「2026-07-21 增补」段;全量验证汇总见 progress 会话段(cargo workspace/clippy/fmt/vue-tsc/eslint/vitest 1336/docs check+index 门全过,docs 9 处红=已知存量基线红零新增);GUI 手测清单落 progress(not automated)
- **状态:** complete(真机 GUI 验收待用户,见 progress 手测清单)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 用户裁(2026-07-21):span 埋点范围=流水线批次+主要 IPC command 级(推翻此前「继续跳过」defer) | 覆盖面广,可看清前端发起到完成的端到端耗时;用户在两候选中明选更大范围 | D-309 |
| 用户裁(2026-07-21):worker 日志汇入=IPC 转发进主进程 JSONL 体系(推翻 D-308「现状已足够」) | 统一入 JSONL、UI 可见,收益最大;用户明选,原 D-308 正文须同步改写 | D-310 |
| span 记录机制=手动计时(SpanTimer RAII)+普通结构化事件,不用 tracing span/FmtSpan::CLOSE | FmtSpan 合成事件只进 fmt 文件层,RingBufferLayer/UI 全看不见;time.busy 是预格式化字符串需二次解析;普通事件走既有信封管线 file+ring+UI 三路免费统一;与 D-304(span 不跨 spawn_blocking,机制已边缘化)一致;零新 Layer 零 re-entrancy 风险 | D-311 |
| IPC span 分两档:任务型/低频=info,视口/滚动热路径查询=debug,spawner/琐碎 getter 不埋 | D-303 硬护栏(高频禁走日志)与「端到端耗时可见」的折中:默认 info 档不被滚动查询刷屏,诊断模式(debug 档)可见全量;spawner 类 command 耗时≈0,真实工作由流水线 run span 覆盖 | D-312 |
| worker stderr 行协议=exotic-protocol 新增 WorkerLogLine{lvl,msg,fields} 单行 JSON;supervisor 固定 target="scrollery::worker"+worker 字段;非 JSON 行 WARN 兜底;64KiB 环形缓冲+崩溃摘尾不动(崩溃路径行双写,接受) | tracing target 编译期常量不能塞运行时字符串(S2/S3 两次踩过),固定 target+来源字段是 S3 已定先例;保留摘尾=不动已验证的崩溃诊断路径,双写仅崩溃时发生 | D-313 |
| ai-worker 内嵌 tracing_subscriber 换自定义 FormatEvent 输出 WorkerLogLine 形状;psd-worker 手写 log() 换 schema 输出 | 单一 schema 单一解析器,supervisor 不养两套解析分支;ai-worker 已有 tracing-subscriber 依赖(scrollery-ai-core 内部日志经它出),换格式化器即可全量收编;psd-worker 无 tracing 依赖,加 serde 比引整套 tracing 轻 | D-314 |
| span_close 不加 outcome/成败字段(仅 panicked;reviewer 深审存疑项裁决) | RAII 单行埋点是设计核心,加成败标记需 46 处调用点显式收尾调用;失败可见性已由 error.code 聚合(S5)承担;失败路径耗时属真实用户等待,计入统计是特性非缺陷 | D-315 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
