---
status: 快照
type: working-memory
line: span埋点与worker日志汇入
created: 2026-07-21
---

# 进度日志:span埋点与worker日志汇入

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:阶段 1..5 全部完成,全线收官;遗留=用户批 push(五 commit:2b65fc9/247f1f7/995f904/eae75e9/6822875+收尾文档 commit)+真机 GUI 验收(见下手测清单)
- 未解错误:无
- 关键指针:D-309..D-315(task_plan 决策表);span 事件契约+IPC 分档终表=findings「span 事件契约」「IPC 埋点候选」节;worker 线定级清单(含阶段 4 batch.rs 补充)=findings「worker 线施工落地」节
- 施工链:阶段2 span 线 implementer(完成)→ 阶段3 worker 线 implementer(完成)→ 阶段4 reviewer 深审+主线修复(完成)→ 阶段5 收尾文档回写

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-21
- 做了:两路 Explore 摸底(logging 基建+span 链路 / supervisor+双 worker)落 findings;施工方案定型 D-311..D-314;三件套建档
- 验证:尚未施工,无门禁运行
- 遗留:阶段 2..5 全部

## 会话:2026-07-21(阶段 2 施工)
- 做了:
  - W1 后端:logging.rs 新增 `SpanTimer`(SpanLevel::Info/Debug,`with_operation_id` 链式,`emit_span_close` 私有关联函数+Drop 调用)+ 4 条单测(同步内存 writer 捕获,不走 non_blocking/guard 时序)。
  - 三处流水线 run 级埋点:`ai/pipeline.rs::start_ai_pipeline`(`pipeline:ai`)、`ai/face_pipeline.rs::start_face_pipeline`(`pipeline:face`)、`derive/pipeline.rs::start_derivation_pipeline`(`pipeline:derive`,挂 operation_id),均在 tokio::spawn async 块首行构造,块尾自然 Drop 覆盖正常/失败/panic 三路径。
  - IPC command 埋点:info 39 个(scan 6/backup 4/file_ops 9/doc 6/search 1/media 7/log 3/edit 1/export 2)+ debug 7 个(get_meta_for_viewport/batch_request_thumbnails/compute_layout/compute_h_layout/list_tree_entries/get_tree_text_preview/get_item_faces),逐 command 核实「await 是否覆盖实际工作」后定档,候选给的字面命令名(query_media_items/render_page/query_faces)在本仓不存在,已按语义映射或确认移出(render_page 无对应物、start_backup 核实后是 spawner 移出),终表已回填 findings。
  - W2 前端:logAggregation.ts 新增 `aggregateSpanDurations`(判据 span_name 为非空字符串+duration_ms 为有限数字,target 不作硬判据;lastTs 按字符串比较取 max,对拍乱序输入)+ 8 条 vitest;LogWindowView.vue 分析标签新增「span 耗时 Top N」表(spanAggregation computed,live+history 合并,同 errorAggregation 姿态)+ en-US/zh-CN 双语 i18n 键(spanAgg* 系列)。
- 验证(实测末行,先落盘内容再跑门):
  - `cargo test --lib`(src-tauri):`test result: ok. 820 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 12.19s`
  - `cargo test --workspace --locked`:全部子 crate `test result: ok.`,0 failed(scrollery-lib 820、scrollery-exotic-trust 26、scrollery-free-stub 2、scrollery-plugin-api 3、scrollery-pro 1,余为 0-test 占位/doc-tests)
  - `cargo clippy --workspace --locked --all-targets`:`warning: scrollery (lib) generated 1 warning` —— 即既有基线 `manual_inspect`(scan_commands.rs,原报告行号 96,因本线在其上方插入 2 行注释+埋点代码,现移至行 98,非新增)
  - `cargo fmt -p scrollery -- --check`:本线触碰文件(logging.rs/三处 pipeline.rs/12 处 ipc/*.rs)零漂移(logging.rs 两处新增测试代码手工对齐 rustfmt 期望格式后确认);export_commands.rs 报的三处漂移(210/286/438 行)经 git diff 核对,均为本线插入前既已存在的格式(与 crate 内其余多个未触碰文件〔backup/core.rs 等〕同源的环境级漂移),未动
  - `cargo bench --bench logging -- logging_pipeline_thumbnail_batch`:off 106.22ms、info 106.64ms(+0.40%)、debug 106.02ms(-0.19%),均 <1% 阈值
  - `npx vue-tsc --noEmit`:零输出(零错误)
  - `npx eslint .`:零输出(零错误)
  - `npx vitest run`:`Test Files  108 passed (108)` / `Tests  1336 passed (1336)`
- 遗留:两 commit 待用户批 push;阶段 3(worker 线)+ 阶段 4(reviewer 深审)+ 阶段 5(收尾文档回写)未开工;`edit_commands.rs::get_edit_preview` 埋点档位存疑未定(见 findings)

## 会话:2026-07-21(阶段 3 施工 W3+W4)
- 做了:
  - W3:`crates/exotic-protocol/src/stderr_log.rs` 新增 `WorkerLogLine{lvl,msg,fields}` + `emit()`/`emit_stderr_log()` 辅助(序列化失败降级原样 eprintln,不 panic)+ 2 条单测;`lib.rs` re-export。不动帧协议、不 bump PROTOCOL_VERSION。
  - ai-worker:手写 `log()` 拆 `log_info/log_warn/log_error/log_debug` 四档,main.rs 22 个调用点逐点定级(清单见 findings);`tracing_subscriber::fmt()` 的 `event_format` 换自定义 `WorkerLogFormat`(`TracingFieldCollector` 访问者收 message→msg、其余进 fields),收编 `scrollery-ai-core` 内部 tracing 日志进同一 schema(D-314)。Cargo.toml 新增 `tracing="0.1"`(直接依赖,自定义 FormatEvent 硬需求——已在依赖树同版本解析,零新增外部代码,详见 findings)+ `serde_json="1"`。
  - psd-worker:同款四档拆分,main.rs 10 个调用点定级;Cargo.toml 新增 `serde_json="1"`。
  - W4:`supervisor.rs` 新增 `LineScanner`(纯函数,残段上限 64KiB 超限强冲)+ `parse_worker_log_line`(首字符 `{` 探测)+ `emit_worker_log_line`(固定 `target: "scrollery::worker"`,`worker`/`worker_context` 字段,五臂 match+未知 lvl 兜底 warn)+ `forward_stderr_line`(trim `\r`/跳空行)。`spawn_stderr_drain` 加 `worker_kind: String` 形参,字节环形缓冲逻辑不动、并行跑行扫描(崩溃路径双写已接受,D-313)。`worker_kind_label(&WorkerSpec)` 由既有 `expected_worker_id` 派生,未碰 `worker.rs`/`WorkerSpec` 定义(改动最小路径)。`spawn()` 调用点补 `worker_kind_label(spec)` 实参。
  - `logging.rs::FieldCollector` 新增 `"worker_context"` 分支,完全照 `"frontend_context"` 姿态(JSON 字符串解回真对象,失败原样存字符串),落点同为 `attributes.context`。
  - 单测新增:exotic-protocol 2、supervisor.rs 14(LineScanner 5/parse_worker_log_line 3/worker_kind_label 1/forward_stderr_line 4 scoped 捕获,含空行跳过)、logging.rs 2(worker_context 成功解析+非法 JSON 兜底);既有 `stderr_ring_keeps_last_64k` 两处调用点补 `worker_kind` 实参,行为不变。
  - fmt 手工对齐:supervisor.rs 两处(新增测试代码换行姿态)+ logging.rs 一处(新增测试断言换行姿态),均按 `cargo fmt -- --check` 报告的期望格式手改;export_commands.rs/backup/core.rs 等既有漂移与本次改动无关,未动。
- 验证(实测末行,先落盘内容再跑门):
  - `cargo test --workspace --offline`(先补 Cargo.lock,两 worker crate 新增依赖):全部子 crate `test result: ok.`,0 failed(scrollery-lib 835、exotic-protocol 27、ai-worker 14、psd-worker 8,余为既有子 crate 计数/0-test 占位/doc-tests)
  - `cargo test --workspace --locked`(补 lock 后复核):`test result: ok. 835 passed; 0 failed; 6 ignored`(scrollery-lib)+ 其余全部 `ok.`,与 offline 结果一致
  - `cargo clippy --workspace --locked --all-targets`:`warning: scrollery (lib) generated 1 warning` —— 即既有基线 `manual_inspect`(scan_commands.rs:98),非新增
  - `cargo fmt -p exotic-protocol -- --check` / `-p ai-worker -- --check` / `-p psd-worker -- --check`:三者均 EXIT:0(零漂移)
  - `cargo fmt -p scrollery -- --check`:本线触碰文件(logging.rs/exotic/supervisor.rs)手工对齐后零漂移;报出的 10 处漂移(backup/core.rs ×4、backup/restore.rs、db/queries/export.rs、export/core.rs、ipc/export_commands.rs ×3)均为本线插入前既存的环境级漂移,未动
- 遗留:四 commit(阶段 2 两个 + 阶段 3 两个)待用户批 push;阶段 4(reviewer 深审)+ 阶段 5(收尾文档回写)未开工;`edit_commands.rs::get_edit_preview` 埋点档位存疑未定(见 findings,阶段 2 遗留);GUI 手测清单(跑一次 AI 分析→日志窗口可见 `target=scrollery::worker` 行)留阶段 5

## GUI 手测清单(not automated,真机验收用)
1. 跑一次视频派生(设置→派生控制)→打开日志窗口「分析」标签→「span 耗时 Top N」表出现 `pipeline:derive` 行(耗时>0);默认 info 档下浏览若干目录后应见 `ipc:*` 任务型行。
2. logLevel 切 debug→滚动画廊→TopN 出现 `ipc:batch_request_thumbnails`/`ipc:compute_layout` 等 debug 档行且 avgMs 非恒 0(f64 修复验证点)。
3. 跑一次 AI 分析→日志窗口实时流可见 `target=scrollery::worker` 行(worker=ai,握手/会话就绪 info 行;SessionInit 期间 ai-core 装载日志应为结构化行而非 unparsed WARN)。
4. 打开含 PSD 的目录触发 psd-worker→异常路径(可选:人为断连)stderr 行进 JSONL 且级别正确。
5. logLevel 切 off→以上全部停增;切回 info 恢复。

## 会话:2026-07-21(阶段 4 深审+修复,主线亲手修)
- 做了:reviewer 子代理深审 915eba9..eae75e9(四 commit),0 严重/1 警告/3 建议/1 存疑。四项全修:batch.rs 收编 WorkerLogLine(warn×3+info×1)、supervisor Err 臂补残段 flush、duration_ms 改 f64(record_f64 已核实落 JSON number)、frontend_context/worker_context 臂合并;存疑项裁 D-315(不加 outcome 字段,理由见 task_plan 决策表)。深审通过面:SpanTimer Drop/IPC 终表逐一对拍/前端聚合类型收窄/测试真实性/LineScanner 边界/转发形状对拍/commit pathspec 纯净等,详审查报告结论已折入 task_plan 阶段 4。
- 验证(实测末行):
  - `cargo test --workspace --locked`:全部子 crate `test result: ok.`,0 failed(scrollery-lib 835/exotic-protocol 27/ai-worker 14/psd-worker 8)
  - `cargo clippy --workspace --locked --all-targets`:唯一 `warning: scrollery (lib) generated 1 warning`=既有基线 manual_inspect,零新增
  - `cargo fmt -p scrollery -- --check`:仅既有其它线 5 文件漂移(backup/core.rs 等),本线文件零漂移;`-p ai-worker -- --check` 零输出
- 遗留:阶段 5(收尾)

## 会话:2026-07-21(阶段 5 收尾)
- 做了:方案文档五处正文改写(状态行/§3.4/§5 P2/§6 表/§8,D-310 取代 D-308 均改正文)+todo.md 条目正文改写+「2026-07-21 增补」段;GUI 手测清单落本文件;三件套终态回写
- 验证(实测末行):
  - 后端门禁沿用阶段 4 修复后实测(本阶段零代码改动):cargo test --workspace 全绿/clippy 零新增/fmt 本线零漂移
  - `npx vue-tsc --noEmit`:EXIT:0;`npx eslint .`:EXIT:0
  - `npx vitest run`:`Test Files 108 passed (108)` / `Tests 1336 passed (1336)`
  - `worklog-kit check`:9 处强制违反=全部已知存量基线红(工具 alpha.4 升级遗留,与日志线 S1 记录同一批,本线零新增,不代修);`worklog-kit index`:索引不变量门通过
- 遗留:用户批 push+真机 GUI 验收(手测清单见上)
