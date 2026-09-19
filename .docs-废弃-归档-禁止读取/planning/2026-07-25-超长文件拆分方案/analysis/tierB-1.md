---
id: 2026-07-25-tierB-1
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# Tier B 简案 · 组1:message.rs / worker_service.rs / state.rs / fast_scan.rs

> 纯结构移动立场,不建议任何行为改动;锚点以符号名为主,行号为辅(并行注释精简线在跑,行号会漂)。

## crates/exotic-protocol/src/message.rs(69KB,1518 行)

**① 缓 / 微拆** — wire 协议类型定义整体职责单一(host↔worker 契约),类型本体不宜拆;但内联单元测试 `mod tests` 占篇幅近半(锚点 `mod tests`,约 740/1518 行),可安全抽离,是本文件唯一高确定性的拆分动作。

**② 主刀口**
1. message.rs → message/tests.rs:整段剪切 `#[cfg(test)] mod tests { ... }`,原文件改 `#[cfg(test)] mod tests;`,逐字迁移不重排,mechanical。
2. message.rs → message/{embed,face,ocr,enhance,video}.rs(二期可选):按算子域拆出各域专属辅助型——EmbedItem/EmbedResult/EmbedBatchSuccess/TextEmbedSuccess;FaceItem/FaceDet/FaceItemResult/FaceBatchSuccess;OcrItem/OcrLine/OcrItemResult/OcrBatchSuccess/OcrSessionReadyBody;EnhanceTask/EnhanceStep/EnhanceDone;VideoSessionInfo/VideoProbeInfo/VideoAudioTrack/VideoOutInfo/VideoFramesInfo/VideoFramesMode。已核实 `lib.rs` 对外是显式 allowlist `pub use message::{...}`,内部挪位只需在 message.rs(或改造后的 mod.rs)顶层保留同名 `pub use`,外部路径 `exotic_protocol::XxxItem` 不变。
3. 不拆:RequestBody/SuccessBody 两个"总线"枚举与 HelloBody/ReadyBody/ModelDescriptor/WorkerErrorCode/ProgressBody/FailureBody 留主文件——它们是 `#[serde(tag = "op")]` 分发的物理载体,拆变体定义会牺牲"一处看全部 op"的可读性,无实质收益。

**③ 最大风险**:serde tag 枚举依赖整体定义,跨文件搬移必须逐字剪切粘贴(不可誊写/重排字段),否则可能悄悄改变 JSON 结构;crate 由 host+worker 两侧共享编译,契约错漏在编译期两端同时炸(风险可控但需完整回归握手 + 各 op 往返测试)。

---

## src-tauri/src/video/worker_service.rs(67KB,1622 行)

**① 拆(小到中)** — 一个文件混了 4 类不同心跳:纯类型/枚举(VideoOp/VideoKind/VideoPriority/VideoOutput/VideoServiceError + `op_timeout` 表)、服务引擎本体(VideoWorkerService struct+impl,~330 行去重队列/提交/取消)、后台驱动循环(driver_loop/run_job/deliver,~170 行)、worker 进程适配器(WorkerVideoRunner,~150 行),外加内联测试(锚点 `mod tests`,~500 行,近 1/3 篇幅)。天然按职责分域,拆分收益明确。

**② 主刀口**
1. worker_service.rs → worker_service/tests.rs:`mod tests`(~500 行)整体迁出,mechanical。
2. worker_service.rs → worker_service/types.rs:VideoOp/VideoKind/VideoPriority/VideoOutput/VideoServiceError(+ `impl From<VideoServiceError> for AppError`)+ `mod op_timeout` + 纯函数 `map_outcome`/`map_worker_code`(锚点 `enum VideoOp` … `fn map_worker_code`)。
3. worker_service.rs → worker_service/runner.rs:WorkerVideoRunner struct+impl+`impl VideoJobRunner for WorkerVideoRunner`(锚点 `struct WorkerVideoRunner` … `raw_outcome_label`)——桥接真实 worker 进程的适配层,与队列引擎解耦充分,可独立成文件。
4. 核心引擎不再二次细分:VideoWorkerService(Job/Inner/SharedState + impl + `impl Drop` + driver_loop/run_job/deliver)内部共享 `Arc<SharedState>`/锁/去重队列语义耦合紧,建议整体留一处,避免把锁语义耦合的状态机拆得过碎。

**③ 最大风险**:VideoWorkerService 是跨线程调度引擎(Arc<SharedState>+Mutex+后台线程),纯移动文件位置零风险,但拆分时若顺手重新组织 driver_loop/run_job 的调用关系(哪怕只是换文件),review 时容易看错并发时序;已核实 10 处外部调用点(state.rs/ipc/exotic/derive 等)只认 `crate::video::worker_service::VideoWorkerService` 路径,拆完须 `pub use` 保路径不变。

---

## src-tauri/src/state.rs(64KB,1178 行)

**① 缓(整体)/ 拆(局部,限 RunTokenSlot)** — AppState 是 Tauri 全局状态装配点,字段本体(约 50+ 域)必须留一处(tauri::State 注入只认一个类型),"职责不单一"是装配点本质而非坏味道,不建议拆字段;但 impl 块内可分离出一个自包含、可独立测试的通用子类型。

**② 主刀口**
1. state.rs → state/run_token.rs:RunTokenSlot struct + `impl RunTokenSlot` + `impl Default` + 其专属单元测试(`finish_clears_own_generation`/`stale_finish_does_not_steal_new_run`/`finish_after_explicit_stop_keeps_publish_right`/`is_current_matches_finish_semantics_without_clearing`/`back_to_back_begin_still_generation_safe`)整体迁出,`pub use run_token::RunTokenSlot;` 保路径。这是文件里唯一与 AppState 本体解耦、可独立验证生成号语义的通用小工具,收益/风险比最好。
2. （二期候选,非本次必做)`impl AppState`(约 700 行 / 约 50 个方法)可选按域拆成多个扩展块分文件——gpu/file-job 信号量(try_acquire_gpu_analysis/release_gpu_analysis/try_acquire_file_job/release_file_job)、exotic 生命周期(quiesce_exotic/resume_after_quiesce/wake_exotic/set_exotic_coordinator)、双缓存写路径(apply_thumb_results/set_*_cached/invalidate_embedding_cache)。Rust 允许同 crate 内多处 `impl AppState` 无需 trait,调用点 `state.xxx()` 不受定义文件影响。
3. 不动:字段声明区及其中文域说明注释——是理解全局状态的唯一入口,拆开反而增加"要翻几个文件才看全字段"的认知成本。

**③ 最大风险**:state.rs 是活跃 WIP 高频接触点(视频格式扩展线刚合并、画廊轴线等多条并行线都在改 AppState 字段/方法),此刻做纯移动仍可能与其他分支产生行号级合并冲突;二期若真拆 impl 扩展块,须逐方法核对有无跨方法共享的私有 helper(如 `now_millis`、`global_rank_building` 原子字段)被误留在错误文件导致可见性问题。

---

## src-tauri/src/scanner/fast_scan.rs(64KB,1251 行)

**① 缓(核心逻辑)/ 拆(测试与载荷类型)** — `run_fast_scan`(锚点 `pub fn run_fast_scan`,~380 行)是单一巨型函数但职责单一(跑一次快速扫描),硬拆内部牵扯扫描状态机行为改动,不建议函数级拆分;文件级有两块可安全挪:IPC 载荷类型(ScanProgressPayload/ScanCompletedPayload/ScanErrorPayload/ScanChannelPayload)与四个测试模块(exotic_seed_gate_tests/finalize_tests/dir_baseline_tests/quick_scan_tests,约 544 行 / 43% 篇幅)。

**② 主刀口**
1. fast_scan.rs → fast_scan/tests/{seed_gate,finalize,dir_baseline,quick_scan}.rs(或单文件内四个 `mod`):四个测试模块整体迁出,锚点 `mod exotic_seed_gate_tests`/`mod finalize_tests`/`mod dir_baseline_tests`/`mod quick_scan_tests`,mechanical、零风险。
2. fast_scan.rs → fast_scan/payload.rs(可选):ScanProgressPayload/ScanCompletedPayload/ScanErrorPayload/ScanChannelPayload 四个 IPC 载荷 DTO 搬出,与算法本体解耦,`pub use` 保路径。
3. 不拆:FileInfo/cheap_phase2_dimensions/extract_dimensions/ensure_dir_chain/decide_dir_pruned/finalize_missing_detection/run_fast_scan/seed_gate_admits 这条"扫描算法+直接辅助函数"主干链——尤其 `seed_gate_admits` 是视频格式扩展子系统刚落地的裁决点(rmvb/vob builtin 豁免,红线见记忆),此刻不建议挪动物理位置,避免与仍在收尾的相关线产生不必要的 diff 噪音。

**③ 最大风险**:`run_fast_scan` 内部维护扫描增量/目录剪枝/软删检测等多阶段状态,`ensure_dir_chain`/`decide_dir_pruned`/`finalize_missing_detection` 均是其 private helper,彼此靠调用顺序与共享的 walked/db 事务上下文耦合;纯搬移测试/载荷类型风险低,但若顺手把这些 private helper 一并挪出会打破"同文件内一眼看到调用序列+事务边界"的可读性,且误改任一 `pub(crate)` 可见性会致编译错误——建议严格只动测试块与载荷类型区,不碰算法主干区间。

---

## 顺手发现

无。
