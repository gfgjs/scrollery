---
status: 施工中
type: 工作记忆
line: 扫描分析进度可感知
created: 2026-08-26
---

# 进度日志:扫描分析进度可感知

## 会话:2026-08-26
- 做了:读取项目级规则、docs 文档契约、planning 与 UI/UX 技能；建立调研计划和三件套。
- 验证:已确认项目存在 docs/planning/ 且 docs/README.md 将其定义为施工中长任务工作记忆目录。
- 做了:完成添加文件夹、快扫、元数据富化、scanStore、侧栏进度和 AppStatusBar 的调用链核对；补充 UI/UX 进度反馈与动态状态播报规则。
- 验证:代码证据已记录在 findings.md；当前实现的扫描相关测试覆盖计数/终态时序，但尚未覆盖字节进度模型。
- 做了:收敛单遍扫描、快速扫描剪枝、跨媒体元数据阶段的字节口径，补充 `runId` 并发边界、底栏布局策略、分批上报/节流、快照恢复和测试验收方案。
- 验证: `npm run test -- src/stores/scanStore.spec.ts` 通过，8 个测试通过；`npm run typecheck` 通过；Rust scanner 过滤测试被 Tauri build script 生成权限文件时的 Windows PermissionDenied 阻断，未进入测试本身。
- 结论:本轮完成代码审查与实现方案，未修改生产代码；待后续实现批次按 findings.md 的 MVP → 元数据 → 快速扫描/恢复顺序落地。
- 做了:实现 MVP 后端字节进度、`runId` 过期事件过滤、跨图片/视频/音频元数据累计、scanStore 多根聚合和底栏 `ScanProgressIndicator`；补充中英文文案、辅助技术状态语义和表征测试。
- 验证:Rust `cargo check -p scrollery --lib --locked` 通过；`cargo test -p scrollery scanner:: --lib --locked` 通过（62/62）；`cargo test -p scrollery db::queries::metadata --lib --locked` 通过（3/3）；前端 `npm run test` 通过（142 文件、1601 测试），扫描 store 定向测试最终 9/9；`npm run typecheck`、`npm run lint`、`npm run build` 均通过，构建入口预算通过。
- 结果:生产代码已修改并完成本轮验证；活动任务快照恢复、quick `checkedBytes` 和首屏精确总量仍按方案留作后续批次。

## 回顾(收口时填)
- 亮点:沿用现有 Channel 与 enrichment 事件，只扩展稳定字段，降低了 IPC 迁移面；`runId` 同时解决进度展示和旧任务终态污染。
- 教训:文件大小在 Rust 数据层实际为 `i64`，不能直接与 `u64` 累加；数据库队列新增列时需保持既有游标列序，避免破坏 keyset/队列表测试。
- 意外:首次 Rust 测试的 Tauri 权限文件写入阻断在后续构建中自行消失，最终未形成代码层阻断。

## 会话：2026-08-26（性能复核）
- 做了:读取当前应用日志并拆分 fast scan、图片富化、视频富化、音频富化阶段；对当前数据库只读执行新增候选总量 SQL 的 EXPLAIN QUERY PLAN 与 5 次耗时测量。
- 验证:当前候选总量查询约 21–26 ms；首个图片批次在富化开始约 115 ms 后完成；真实富化总耗时 42,123 ms，其中图片段约 37,765 ms，批次间隔中位数约 220 ms、最大约 2,074 ms。
- 结论:未发现本轮进度展示改动引入解析热循环或并发度退化；“变慢”的主要观感来自新实现把图片、视频、音频统一计入真实分母。真实耗时仍应在独立性能批次中继续定位，本轮不改生产代码。
- 做了:建立一次性只读诊断程序，复现当前 `parse_exif_meta_buf` 的回退条件并按扩展名汇总；诊断程序随后删除，未留下生产代码。
- 验证:当前真实库图片 56,381 个中 41,830 个（74.2%）会触发“头缓冲解析失败且截断 → 重新打开原文件解析”分支；其中 PNG 16,035/16,615，JPEG 25,595/39,566。`kamadak-exif` 的 PNG/WebP/JPEG 无 EXIF 路径返回 `Error::NotFound`，说明当前“所有错误都回退”存在明确的可压榨空间。
- 结论:元数据解析的第一优化批次应先修正回退错误分类，并保留低频回退计数；耗时继续用真实导入日志做前后对比，不先调整进度统计 SQL 或无依据地提高并发。

## 会话：2026-08-26（元数据热路径优化）
- 做了:实现 `ExifParsePath` 诊断路径；JPEG 头部 marker 探测；PNG/WebP chunk 头 seek 扫描；BMP/GIF/PSD 已知不支持容器短路；快扫 JPEG orientation 复用无 EXIF 判断。
- 验证:新增 5 个回归用例覆盖大 JPEG/PNG/WebP 无 EXIF、PNG 大 payload 后置 EXIF 和不支持 PSD；元数据测试 16/16、扫描器相关测试 67/67 通过。
- 做了:对 root=14 当前真实库运行只读 profile，并在完成后删除临时 profile 程序。
- 验证:56,381 个图片中 `NoMetadata=39,709`、`HeaderBuffer=14,390`、`FullFileFallback=2,108`、`Unsupported=174`；回退比例从 74.2% 降至 3.7%，约减少 95%。
- 验证: `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --locked -- -D warnings`、`cargo test --workspace --locked` 均通过（workspace 主库 1,105 passed，6 ignored）。
- 结论:本批生产代码已完成并通过 Rust 门槛；暂不调整缓冲上限或线程池，保留真实导入日志作为下一轮端到端耗时 A/B 依据。

## 会话：2026-08-26（并发与磁盘占用诊断）

- 做了:读取随后一次同目录导入（root=16）日志，并核对元数据线程池、头缓冲、回退探测、批事务和富化取数顺序。
- 验证:root=16 快扫 3,590 ms、富化 31,543 ms；图片主段约 27,348 ms；图片批次间隔 P50=145 ms、P90=556 ms、最大=1,167 ms。机器 20 逻辑处理器，图片富化池为 19 worker。
- 结论:CPU 约 50% + 磁盘活跃约 90% 更符合多路小块/跨文件 I/O 等待；当前不是“只要把线程从 19 加到更多就能吃满 5 GB/s”。低内存占用符合当前批量级设计，增加 RAM 只有在引入有界预取/句柄复用后才可能转化为吞吐。
- 遗留:下一性能批次应做受控 A/B（已知 file_size、二次 open 消除、I/O reader 数、批大小、按路径顺序），本次不修改生产代码。

## 会话：2026-08-26（并发与磁盘占用优化实现）

- 做了:图像富化改为“有界头读 → 解析”两级批处理；头读线程池按逻辑处理器数的 2 倍计算并封顶 32，解析池继续保留一个核；每项把快扫已记录的 `file_size` 传入头读，并保留同一个句柄给 PNG/WebP 探测和完整 EXIF 回退。
- 做了:图像批次由 500 调整为 1,000，视频/音频仍为 500；日志增加实际头读/解析 worker 数和批次，便于下一次真实导入做 A/B。
- 验证:新增“尾部 PNG EXIF 通过保留句柄完成回退”回归用例；元数据测试 17/17、扫描器测试 68/68、`cargo check --workspace --locked` 与 `cargo clippy -p scrollery --lib --locked -- -D warnings` 通过。
- 备注:全量 `cargo clippy --workspace --all-targets -- -D warnings` 仍会命中已有的 `items_after_test_module`（`db/queries/faces/wall.rs`、`lifecycle.rs`）问题，本批未扩大范围；路径顺序未改，避免牺牲按视图顺序优先补全的体验。

## 会话：2026-08-26（扫描+解析端到端耗时统计）

- 做了:在 `start_scan` 入口创建带 `runId` 的 `scan:pipeline` `SpanTimer`，把它移动到后台 enrichment 闭包，覆盖“根路径准备 → 快速扫描 → 元数据富化 → 富化终态”。原有 `ipc:start_scan`、`Fast scan done`、`Enrichment complete` 分阶段计时保留。
- 做了:新增统一汇总日志字段 `fast_scan_ms`、`enrichment_ms`、`orchestration_ms`、`total_ms` 和 `outcome`；快扫成功后额外记录阶段完成点，快扫失败、富化取消/失败/崩溃也会记录最终耗时。
- 验证:workspace `cargo test --workspace --locked` 通过（主库 1106 passed、6 ignored）；`cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --locked -- -D warnings` 通过。
- 口径:下一次同目录导入应以 `Scan pipeline complete` 的 `total_ms` 做端到端比较，以 `fast_scan_ms`/`enrichment_ms` 定位阶段；`orchestration_ms` 是总耗时中无法归入两阶段的调度与准备时间，不等同于磁盘耗时。

## 会话：2026-08-26（重跑无扫描日志诊断）

- 做了:核对当前 Tauri 进程、应用日志文件、日志配置、扫描入口和扫描根数据库状态。
- 验证:日志文件 `C:/Users/gf/AppData/Roaming/com.scrollery.app/logs/scrollery.2026-08-26.log` 已正常创建并持续写入；当前会话含 75 行启动/后台任务日志，但扫描关键词和 `start_scan` 入口日志均为 0。
- 验证:当前进程于 09:07:41 启动；扫描根的 `last_scan_at` 仍为 08:21:32，说明本次进程没有收到 `start_scan` 调用，问题不在日志初始化、目录权限或非阻塞写入。
- 发现:文件夹选择向导的 `pickFolder()` 只调用 `addScanRoot()`，不会自动调用 `startScan()`；若用户从该入口“重新导入”，只会登记根目录而不会启动扫描。另发现清理日志会尝试删除当前正在写入的日志文件，是潜在的日志生命周期缺陷，但本次无 `Logs cleared by user` 证据。
- 下一步:用当前 Tauri 窗口侧栏扫描根右侧的“重新扫描”按钮复现；点击后应立即出现 `User action: Starting scan`，并更新扫描根时间。若仍无该行，应继续查前端点击/IPC 触发链，而不是调整日志配置。

## 会话：2026-08-26（最新导入耗时与元数据质量分析）

- 做了:读取 session `s-771fbe6f` 的完整流水线日志，并与上一轮同目录导入的阶段耗时、批间隔和数据库元数据状态对齐。
- 验证:本轮 `root_id=23` 成功完成；快扫 3,579ms，元数据富化 28,906ms，编排 1ms，总耗时 32,486ms。快扫发现 63,685 个文件，元数据候选 58,866 个、逻辑文件大小 493,297,360,580 字节。
- 验证:相较上一轮快扫 3,590ms、富化 31,543ms，本轮快扫基本持平（-0.3%），富化减少 2,637ms（-8.4%）；图片 1,000 项批次的 P50/P90/最大间隔为 266.5/729/1,697ms，考虑批大小已从 500 提至 1,000，单位文件吞吐仍有改善。
- 发现:图片 56,381 项中 `header=14,390`、`full_file_fallback=2,091`、`no_metadata=39,726`、`unsupported=174`、`failed=0`；视频 2,453 项中 2,138 项填充真实元数据，315 项写入最小行。315 项与日志中的 Media Foundation 错误数量精确相等，错误为 314 个 `0xC00D36C4` 和 1 个 `0xC00D36E5`。
- 定位:315 个视频失败集中在 TS=307/442、MTS=6/6、MP4=2/1,803；`enrich_videos` 将探测失败转换为最小 `video_meta` 后继续计入 `enriched_total`，所以流水线显示 completed，但这 315 项实际没有宽高/时长等完整视频元数据。当前错误日志也没有文件路径，无法从日志直接定位具体文件。
- 风险:格式选择器按扩展名优先返回 Media Foundation；即使运行时 MF 探测失败，也不会再尝试已就绪的视频 worker。TS/MTS 的失败集中度说明这里仍有运行时后端回退空间；`AppError::os` 在预期失败被 `.ok()` 吞掉前就逐项写 ERROR，造成错误风暴和诊断噪声。

## 会话：2026-08-26（视频探测回退与日志降噪修复）

- 做了:新增 `VideoProbeRuntime`，在富化阶段只检查一次 worker 能力；MF 按扩展名可用但运行时探测失败时，复用已就绪的 video worker 进行回退，MF 不负责的扩展名继续直接走 worker。
- 做了:为 MF 批量 `probe` 增加静默错误映射，避免 `AppError::os()` 在单文件失败被吞掉前逐条写 ERROR；视频富化结束时新增总计与按格式的 attempted/populated/failed/unsupported/fallback 统计。
- 验证:新增视频统计聚合测试通过；`cargo fmt --all -- --check`、`cargo test -p scrollery --lib --locked`（1107 passed、6 ignored）和 `cargo clippy -p scrollery --lib --locked -- -D warnings` 均通过。
- 遗留:尚未在真实库重跑；下一次日志应重点确认 `fallback_populated` 是否覆盖原先 TS/MTS 的 313 项失败，以及 ERROR 风暴是否消失。若仍失败，再按 worker 汇总结果判断是源文件损坏还是 worker/编解码器能力缺口。
- 做了:补齐旧的最小 `video_meta` 行重试条件：只有在 worker 已就绪时才把这类行重新纳入工作量和视频队列，避免真实重跑时被旧的失败占位行直接跳过。
- 验证:新增“最小视频行可在回退后重试”数据库测试通过；`cargo fmt --all -- --check`、`cargo test -p scrollery --lib --locked`（1108 passed、6 ignored）和 `cargo clippy -p scrollery --lib --locked -- -D warnings` 均通过。
- 遗留:尚未在真实库重跑；下一次日志应重点确认 `fallback_populated` 是否覆盖原先 TS/MTS 的 313 项失败，以及 ERROR 风暴是否消失。若仍失败，再按 worker 汇总结果判断是源文件损坏还是 worker/编解码器能力缺口。
