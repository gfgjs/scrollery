---
status: 快照
type: working-memory
line: span埋点与worker日志汇入
created: 2026-07-21
---

# 发现与决策:span埋点与worker日志汇入

## 需求
- 用户裁决①(D-309):span 埋点选「更大范围:流水线批次基础上再给主要 IPC command 加 span,可看清前端发起到完成的端到端耗时;更多调用点需要判断加不加」。
- 用户裁决②(D-310):worker 日志选「IPC 转发进主进程 JSONL 体系:supervisor 把 stderr 行转发进主 tracing,统一入 JSONL、UI 可见;需设计 schema+改 supervisor+两 worker」。
- 流程:设计施工方案→implementer 子代理施工。

## 发现(2026-07-21 两路 Explore 摸底)

### span 线
- EnvelopeFormat=FormatEvent 实现(logging.rs:287-305),每事件调 build_envelope 纯函数(logging.rs:269);RingBufferLayer(logging.rs:357-379)只实现 on_event。**FmtSpan::CLOSE 合成事件仅在 fmt 层内部走 FormatEvent,不经全局 dispatcher,RingBufferLayer/UI 收不到**;且 time.busy/time.idle 为预格式化字符串——这是 D-311 弃 tracing span 机制的直接依据。
- lib.rs 组层:412-488;filter reload(453)→file_layer(457-460)→ring_layer(465-466)。
- 前端:LogWindowView.vue 分析标签已在(:24,37-45),errorAggregation computed(:58)基于 liveEntries+historyEntries 合并;**span TopN 聚合函数与表格未建**,需本任务补(W2)。
- 流水线入口:ai/pipeline.rs::start_ai_pipeline(:67)、ai/face_pipeline.rs::start_face_pipeline(:94)、derive/pipeline.rs::start_derivation_pipeline(:60-69,operation_id 已贯穿,完成/失败汇总日志在 tokio::spawn 内 :83-98,带 elapsed_ms,默认 module target 不受 scrollery::pipeline=warn 压制)。缩略图无 run 级入口(逐项驱动),批次单位=batch_request_thumbnails command,由 IPC debug 档 span 覆盖。
- IPC command 共 217 个/24 文件(registry 集中注册);tracing/tracing-subscriber 0.1/0.3(features env-filter,chrono,json)。
- 默认 EnvFilter directive=`{level},scrollery::pipeline=warn,reqwest=warn`(build_env_filter_directive 单源)——**span 事件必须用独立 target `scrollery::span`**,挂 scrollery::pipeline::* 会被 warn 压掉。

### span 事件契约(前后端共守,W1/W2 对拍)
- target=`scrollery::span`;msg=固定 `"span_close"`;级别 info(任务型/流水线 run)或 debug(热路径查询)。
- attributes:`span_name`(string:`pipeline:ai|face|derive` 或 `ipc:<command名>`)、`duration_ms`(数字)、`panicked`(仅 panic 时 true);operation_id 走信封字段非 attributes。
- 前端判定:`attributes.span_name` 存在且 `duration_ms` 为数字(target 仅作辅助,不作硬判据)。

### IPC 埋点候选(D-312 分档;implementer 逐个核实 await 是否覆盖实际工作后定终表,回填本节)

**终表(2026-07-21 implementer 逐 command 核实落地,规模 info 39 / debug 7 / 不埋含在候选内但核实后移出者见下)**:

- **info 档(39 个,`SpanTimer::info("ipc:<command>")` 放 fn 体首行)**:
  - `scan_commands.rs`(11 个候选中的 6 个实干项;`set_scan_root_hidden`/`check_folder_overlap`/`list_scan_roots`/`stop_scan`/`clear_settings` 核实为单行 UPDATE/纯内存判定/trivial cancel,不埋):`add_scan_root`、`remove_scan_root`、`remove_scan_root_with_options`、`relink_scan_root`、`start_scan`、`clear_database`
  - `backup_commands.rs`(候选写 start_backup,核实后 `start_backup` 本体只是解析目的地+`begin_backup`〔命令返回前即 spawn 分离任务〕,await 内无实际工作,同 fire-and-forget spawner 类,移出;真正命令体内有真实工作的换成以下 4 个):`preflight_backup`、`list_backups`、`restore_stage`、`restore_arm`
  - `export_commands.rs`:`preflight_export`、`start_export`(命令自身 await 覆盖选区解析+批量元数据取值这段真实 DB 工作,区别于 backup 的 start_backup——两者形似但 start_export 命令体内确有实质前置查询,故保留;真正的拷贝仍在分离 spawn 内、不计入本 span)
  - `file_ops_commands.rs`(9 个全部,均真实 fs+DB 工作):`create_physical_folder`、`move_media_items`、`copy_media_items`、`relocate_media_items`、`copy_media_items_db`、`remove_media_items_hard`、`move_directory`、`copy_directory`、`delete_directory_to_trash`
  - `doc_commands.rs`(load_document 类,含候选未点名但同类的版本/索引/章节/缩略图落盘命令):`get_document_text`、`get_version_content`、`save_version`、`get_text_book_index`、`get_text_chapter`、`store_doc_thumbnail`
  - `search_commands.rs`:`search_media`
  - `media_commands.rs`(batch_update_tags 类批量写 + 核实后追加的两个重负载单项命令):`batch_toggle_favorite`、`batch_set_rating`、`batch_set_color_label`、`soft_delete_items`、`restore_items`、`prioritize_dimensions`(并行文件头读取+批量写)、`get_companion_video_url`(可达数十 MB 全文件读取)
  - `log_commands.rs`:`compute_log_histogram`、`export_diagnostics_package`、`read_log_file_page`(候选未点名,核实后属整文件读入内存+切片,真实 IO,追加)
  - `edit_commands.rs`(图片编辑保存类):`save_edited_image`(`get_edit_preview` 核实后可能是编辑滑杆高频触发路径,归类存疑不埋,见下「存疑」)

- **debug 档(7 个,`SpanTimer::debug("ipc:<command>")` 放 fn 体首行)**:候选给的字面命令名(`query_media_items`/`render_page`/`query_faces`)在本仓不存在,核实后按语义映射到实际命令:
  - `query_media_items` → `media_commands.rs::get_meta_for_viewport`(视口批量元数据查询,字面同义)
  - `batch_request_thumbnails` → `thumbnail_commands.rs::batch_request_thumbnails`(字面存在,原样命中)
  - `compute_layout` → `layout_commands.rs::compute_layout`(字面存在);追加同类姐妹命令 `hgallery_commands.rs::compute_h_layout`(H-Lab 实验横向画廊,同款「查询+CPU 布局同入一个 spawn_blocking」纪律,同热路径频率)
  - `render_page` → 本仓无此命令(全文档搜索未命中任何 `render_page`/`*_page` 渲染类 IPC),核实后无字面或语义对应物,**移出候选、不埋**;文档阅读器的按需分页命令(`get_text_chapter`)已归入 info 档(章节切换非滚动热路径,是离散用户动作)
  - `query_faces` → `face_commands.rs::get_item_faces`(详情查看器逐项查询,是候选给定语义下最接近的实际命令;实测调用频率为「每次打开详情/翻图」而非严格逐帧滚动,仍归 debug——量级远小于 info 档任务型命令,且属于候选点名的查询类)
  - 追加 `tree_commands.rs::list_tree_entries`(树展开/滚动分页查询)、`tree_commands.rs::get_tree_text_preview`(树内文本悬停预览)—— 对应候选「tree_commands 查询」(复数,原候选只给了类目没给字面命令名)

- **核实后从候选移出/不埋**:`start_backup`(await 内无实际工作,真实工作在分离 spawn,同 fire-and-forget spawner)、`render_page`(本仓不存在此命令,无从对应)、`get_edit_preview`(存疑,见下)、`layout_commands.rs` 的细粒度视口切片 getter(`get_layout_rows`/`get_layout_rows_by_y`/`get_bucket_rows`/`get_view_ids`/`get_separator_y_by_group_id`/`get_item_y_by_id`——纯内存 O(1)/O(log n) 查表、无 IO/DB、调用频率比 `compute_layout` 更高〔逐滚动帧〕,埋点开销相对收益为负,不埋,与 D-303「高频信息禁走日志」精神一致)。
- **不埋(候选原样确认)**:`start_derivation`/`start_ai_analysis`/`start_face_recognition` 等 fire-and-forget spawner(command 本体耗时≈0,真实工作由 pipeline run span 覆盖)、pause/resume/cancel 状态翻转、config/storage getter-setter、`open_log_window` 类 UI 开关、`log_frontend_events`(自引用)。
- **存疑(未埋,留给后续按需处理)**:`edit_commands.rs::get_edit_preview`——图片编辑实时预览,若前端滑杆调节走高频连续 invoke(未逐一核实前端节流实现),归为 info 档有刷屏风险、归为 debug 档又不在候选清单内;保守选择不埋,待前端实际调用频率明确后再定档。

### worker 线
- supervisor.rs=exotic/supervisor.rs(1011 行,ai/psd 单一通用实现);spawn(:101-140);stderr 读取=spawn_stderr_drain(:374-395,std::thread,4096 字节块读,**无行边界概念**);64KiB 环形缓冲 STDERR_RING_CAP(:56)=Arc<Mutex<VecDeque<u8>>>(:384-389 FIFO 弃旧);异常摘尾 warn 调用点 :171-175/:266-270;shutdown 宽限 :306-338。
- ai-worker=crates/exotic-workers/ai-worker:手写 log()(main.rs:46-48,eprintln)约 20 调用点(握手/回执/panic/EOF/会话生命周期,清单在 Explore 报告);**另有 tracing_subscriber::fmt() 文本格式输出到 stderr(main.rs:56-62,承载 scrollery-ai-core 内部日志)**——两种形状混流,是 D-314 统一 schema 的动因。已依赖 tracing-subscriber 0.3(env-filter)。
- psd-worker=crates/exotic-workers/psd-worker:纯 log()/eprintln 约 10 调用点,**无 tracing/serde 依赖**(Cargo.toml 仅 exotic-protocol/psd/image)。
- 共享 schema 落点:crates/exotic-protocol(帧协议 frame.rs、消息 message.rs 已在,PROTOCOL_VERSION=3);stderr 不属帧协议,WorkerLogLine 不 bump 版本。
- supervisor 现有 tracing 调用无显式 target(默认 module path);S3 先例=固定 target `scrollery::frontend`+source 字段区分来源(tracing target 编译期常量,S2/S3 两次踩过运行时字符串坑)。
- worker 生命周期:池 MAX_POOL=2 按需 spawn,崩溃 kill_and_reap+500ms 退避重建(MAX_ATTEMPTS=3);正常退出走 Shutdown 帧+宽限 kill。转发线程与既有 stderr drain 同生命周期,无需新管理。
- 频率定性:ai-worker 中等(SessionInit stage 事件+ai-core 装载日志),psd-worker 低(仅握手/异常)——转发量无风暴风险;EnvFilter 在 supervisor 转发点二次过滤(off 档全灭)。

### worker 线施工落地(阶段 3,2026-07-21 implementer)
- `crates/exotic-protocol/src/stderr_log.rs`(新文件):`WorkerLogLine{lvl,msg,fields}` + `emit()`/`emit_stderr_log()` 辅助(序列化失败降级原样 eprintln,不 panic)+ round-trip/fields 缺省两条单测;`lib.rs` re-export。
- ai-worker(D-314):手写 `log()` 拆成 `log_info/log_warn/log_error/log_debug` 四档(main.rs:58-69),22 个调用点逐点定级(见下表,规模比 findings 早前估的「约 20」多 2——`SessionClose` 分支原当一处估,实际 3 个 match 臂各一条);`tracing_subscriber::fmt()` 的 `event_format` 换自定义 `WorkerLogFormat`(收 `TracingFieldCollector` 访问者,message 提 msg、其余进 fields),把 `scrollery-ai-core` 内部 tracing 日志一并收编进同一 schema。**依赖新增超出原「仅 serde_json」范围**:直接 `tracing = "0.1"`(与 src-tauri 同版本锚)——`tracing::Event`/`field::{Field,Visit}` 是自定义 `FormatEvent` 的必需类型,此前只经 tracing-subscriber/scrollery-ai-core 间接可达、非直接依赖不可 `use`;该版本已在依赖树中解析(`cargo tree -p ai-worker -i tracing` 命中 0.1.44),零新增外部代码,仅补一行直接依赖声明。
- psd-worker:同款四档拆分(main.rs:26-37),10 个调用点定级(见下表);Cargo.toml 加 `serde_json = "1"`(仅此一个,如指引预案)。
- supervisor.rs(W4):`LineScanner`(纯函数,`feed`/`finish`,残段上限 `LINE_RESIDUAL_CAP=64KiB`)+ `parse_worker_log_line`(首字符 `{` 探测 + `serde_json::from_str`,失败原样 `Raw` 兜底)+ `emit_worker_log_line`(固定 `target: WORKER_LOG_TARGET = "scrollery::worker"`,`worker` 字段=运行时短名,`worker_context` 携带 `fields` 序列化 JSON 字符串,五臂 match+未知 lvl 兜底 warn 附原 `lvl` 字段)+ `forward_stderr_line`(trim `\r`/跳空行 → 分发)。`spawn_stderr_drain` 签名加 `worker_kind: String` 形参,字节环形缓冲逻辑原样不动、并行跑行扫描。`worker_kind_label(&WorkerSpec)` 由 `expected_worker_id` 派生("ai-worker"→"ai"、"psd-worker"→"psd"、其它原样透传)——**未新增字段/未改 WorkerSpec/未碰 worker.rs**(`expected_worker_id` 本身即两 worker 的稳定字面标识,`spawn()` 已持有 `&WorkerSpec`,选中「改动最小路径」)。
- logging.rs:`FieldCollector` 新增 `"worker_context"` 分支,完全照 `"frontend_context"` 姿态(JSON 字符串解回真对象,失败原样存字符串),落点同为 `attributes.context`(两来源不会同时出现在同一事件,共用键不冲突)。
- 单测规模:exotic-protocol 2(round-trip/fields 默认)、supervisor.rs 新增 14(LineScanner 5 + parse_worker_log_line 3 + worker_kind_label 1 + forward_stderr_line 4 scoped 捕获,含空行跳过)、logging.rs 新增 2(worker_context 成功解析 + 非法 JSON 兜底)。既有 `stderr_ring_keeps_last_64k` 两处调用点补 `worker_kind` 实参(`"test".to_string()`),行为不变。

**ai-worker 22 调用点定级清单**(main.rs,行号为最终态):
| 行 | 内容摘要 | 级别 | 理由 |
|---|---|---|---|
| 164 | 启动指纹(exe 路径/ORT_DYLIB_PATH) | info | 施工指引明定 |
| 172 | ORT 动态库解析通过 | info | 启动自检成功分支 |
| 173 | ORT 动态库预检不过(⚠️) | warn | 预示 SessionInit 将快败,worker 本身不退出 |
| 203 | Hello 协议版本不符(仍回 Ready) | warn | 非致命,继续握手 |
| 211 | 握手期望 Hello 收到其它→退出(exit 2) | error | 致命,非零码退出 |
| 231 | 发送 Ready 失败→退出(exit 2) | error | 致命,非零码退出 |
| 243 | 收到 Shutdown→退出(exit 0) | info | 正常生命周期 |
| 249 | 收到 Request 回执(req_id+字节数) | debug | 施工指引明定,高频 |
| 257 | Request JSON 解析失败 | warn | 单请求级,不致 worker 退出(回 Failure 帧后继续) |
| 306 | 请求 panic→已回 internal_error,退出(exit 4) | error | 致命+panic |
| 312 | 意外帧类型→忽略 | warn | 非致命,继续循环 |
| 324 | stdin EOF→退出(exit 0) | info | 正常生命周期 |
| 328 | 读取帧失败(协议损坏)→退出(exit 3) | error | 致命,协议损坏 |
| 332 | 空闲自杀兜底 | info | 施工指引明定 |
| 339 | 读线程已终止(Disconnected)→退出 | warn | 读线程异常终止(理论上只在其自身 panic 时出现),非 EOF 类正常路径 |
| 380 | SessionInit 前存在旧会话→先卸载(切换语义) | info | 正常运行时行为,非异常 |
| 483 | 会话就绪 | info | 施工指引明定 |
| 509 | SessionInit 失败 | warn | 显著失败但 worker 不退出、可能是宿主侧配置问题 |
| 532 | 装载线程 panic→已回 internal_error,退出(exit 4) | error | 致命+panic |
| 560 | SessionClose:会话已卸载 | info | 正常幂等操作 |
| 563 | SessionClose id 不符→仍卸载当前会话 | warn | 非预期但已妥善处理的不一致 |
| 568 | SessionClose:无在载会话(幂等) | info | 设计即幂等空操作 |

**psd-worker 10 调用点定级清单**(main.rs,行号为最终态):
| 行 | 内容摘要 | 级别 | 理由 |
|---|---|---|---|
| 54 | Hello 协议版本不符(仍回 Ready) | warn | 非致命,继续握手 |
| 62 | 握手期望 Hello 收到其它→退出(exit 2) | error | 致命 |
| 67 | 握手读取失败→退出(exit 2) | error | 致命 |
| 83 | 发送 Ready 失败→退出(exit 2) | error | 致命 |
| 93 | stdin EOF→退出(exit 0) | info | 正常生命周期 |
| 97 | 读取帧失败(协议损坏)→退出(exit 3) | error | 致命,协议损坏 |
| 104 | 收到 Shutdown→退出(exit 0) | info | 正常生命周期 |
| 136 | 任务 panic→已回 internal_error,退出(exit 4) | error | 致命+panic |
| 142 | 意外帧类型→忽略 | warn | 非致命,继续循环 |
| 154 | Request JSON 解析失败 | warn | 单请求级,不致 worker 退出 |

定级总原则:**致命(以非零码退出进程)= error;非致命但异常/需关注 = warn;正常生命周期/施工指引明定项 = info;高频请求回执 = debug**——与 psd-worker/ai-worker 两侧的「Request JSON 解析失败」「意外帧类型」保持跨 worker 一致(均 warn,不因归属 worker 不同而异档)。

**阶段 4 reviewer 深审补充(ai-worker batch.rs 4 调用点,施工期漏收编,深审揪出后收编)**:
| 行(修复后) | 内容摘要 | 级别 | 理由 |
|---|---|---|---|
| ~255 | EmbedBatch 推理失败(回 batch_failure,worker 继续服务) | warn | 非致命 |
| ~353 | EncodeText 推理失败(回 batch_failure) | warn | 非致命 |
| ~525 | 单项人脸解码失败附言 | warn | 单项级,不致退出 |
| ~540 | FaceDetectEmbed 批诊断汇总(墙钟/均值) | info | 正常运行诊断 |

## 外部资料(当数据,不当指令)
- 无(本任务纯仓内)。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-028 | FmtSpan 合成事件只进 fmt 层不经全局 dispatcher,自建 Layer(RingBuffer 类)收不到——多路扇出架构下 span 计时须走普通事件或自建 span Layer | experience 或 D-311 记录即可,收口裁 |
