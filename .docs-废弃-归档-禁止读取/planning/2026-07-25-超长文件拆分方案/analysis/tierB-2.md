---
id: 2026-07-25-tierB-2
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
line: 超长文件拆分方案
group: tierB-2
---

# Tier B 简案 · 组2:video_commands.rs / supervisor.rs / SettingsView.vue / enhance/service.rs

> 纯结构移动立场,不建议行为改动。锚点以符号名为主,行号仅供定位参考(并行注释精简线行号会漂)。

## src-tauri/src/ipc/video_commands.rs(60KB)

**① 拆(缓半步)** — 文件本体是「IPC 命令壳 + 派生编排内核」两层揉在一起,命令壳必须留在原文件(契约面),但编排内核(resolve_impl/resolve_derived/spawn_playable_job 这条重逻辑链)可整体搬出,壳变薄。

**② 主刀口**
- 新增 `src-tauri/src/video/playback_orchestrator.rs`(或作 `ipc/video_commands/` 子模块),迁出:`resolve_impl` → `apply_force_transcode` → `db_fallback` → `resolve_derived` → `spawn_playable_job` → `finish_and_emit_error` → `emit_progress`/`emit_progress_percent` → `spawn_progress_forwarder` 这条私有调用链(约 750–1116 行区间,占全文件近一半)。
- `PREPARING_OUTPUTS`/`PLAYBACK_PROGRESS` 两个 `static LazyLock` 及其读写小函数(`register_in_use`/`unregister_in_use`/`in_use_snapshot`/`set_progress_snapshot`/`clear_progress_snapshot`/`progress_snapshot`)须随编排内核一起搬,因为 `video_playback_progress_snapshot`、`cancel_video_playback` 两个命令壳也要读这两个 static——这是唯一的跨文件耦合点,建议以 `pub(crate)` 暴露读接口,不暴露 static 本身。
- 命令壳(`resolve_video_playback`/`confirm_video_playback`/`video_cache_stats`/`cancel_video_playback`/`video_component_status`/`download_video_component`/`video_playback_progress_snapshot`)+ DTO(`VideoPlaybackResolution`/`VideoCacheStats`/`VideoPlaybackProgressSnapshot`)+ `map_tools_err` 留原文件,`#[tauri::command]` 属性、函数名、`Result<T, AppError>` 错误契约原样不动,仅内部改为调用迁出模块的函数。

**③ 最大风险**:`PREPARING_OUTPUTS`/`PLAYBACK_PROGRESS` 两个 static 被命令壳与编排内核双向读写,拆分时若接口划分不清会造成一次改两处忘同步(尤其 `cancel_video_playback` 的清理时机与 `spawn_playable_job` 的写入时机存在隐式先后依赖)。

---

## src-tauri/src/exotic/supervisor.rs(59KB)

**① 缓** — `WorkerSupervisor` 是单一生命周期对象(握手/请求/会话/kill-reap/shutdown 全绑在一个 struct 的私有字段上),核心 impl 块拆到多文件收益低、风险不低;只有「stderr 行日志转发」这一段是真正独立的工具子系统,值得单独搬。

**② 主刀口**
- 新增 `src-tauri/src/exotic/worker_log.rs`,迁出行 58–218 区间的独立单元:`WORKER_LOG_TARGET`/`LINE_RESIDUAL_CAP` 常量、`LineScanner` 结构体+impl、`ParsedWorkerLine`、`parse_worker_log_line`、`emit_worker_log_line`、`forward_stderr_line`、`worker_kind_label`——这段不触碰 `WorkerSupervisor` 私有字段,是纯函数/自包含状态机,当前已自带确定性单测(mod tests 里对应用例需一并迁移)。
- `spawn_stderr_drain`(590 行)是两者的唯一连接点(被 `WorkerSupervisor::spawn` 调用、内部调用 `forward_stderr_line`),留在 supervisor.rs 或随迁均可,建议随迁到 worker_log.rs 并 `pub(crate)` 导出,保持 `spawn` 处只多一个 `use`。
- `WorkerSupervisor` 本体(struct + 全部 impl,256–635 行)、`SessionDescriptor`、`ChildHandle` trait、`Drop` 实现原样不动——这是 D-311/D-315(日志能力线)已裁决的稳定结构,不建议动。

**③ 最大风险**:`STDERR_RING_CAP`/`stderr_ring` 是字节级环形缓冲(崩溃诊断),与迁出的行级 `LineScanner` 是两套并行机制(D-313 已裁决双写可接受)——迁移时如果误把两套缓冲机制合并「顺手简化」,会违反既有裁决且改变诊断语义,必须原样保持双轨。

---

## src/views/SettingsView.vue(58KB)

**① 拆** — 模板层已是 registry 驱动(`SETTINGS_MAP`/`sectionSettingKeys`)+ 五大 `<section>` 分区 + 已抽出多个子组件(ReaderSettingsSection/FaceModelLibrary/OcrModelSection/EnhanceSettingsSection/CollapsibleCard),不需要再拆模板;真正臃肿的是 `<script setup>` 里还没下沉的几坨状态管理,可按项目规则（composables 下沉）继续搬。

**② 主刀口**
- 新增 `useSettingsCacheStats` composable,迁出 `thumbCacheDir`/`logDir`/`cacheStats`/`cacheStatsLoading`/`videoCacheStats`/`DEFAULT_VIDEO_CACHE_MAX_BYTES`/`refreshCacheStats`/`refreshVideoCacheStats`/`cacheStatRows`(约 686–753 行 + `CacheStatsPayload`/`VideoCacheStatsPayload` 接口),`onMounted` 里对应两次 `void refresh*()` 调用改为调用 composable 暴露的方法。
- 新增 `useIccProfileManager`,迁出 `iccProfiles`/`ICC_IMPORT_ERROR_KEY_BY_CODE`/`refreshIccProfiles`/`importIccProfile`/`selectIccProfile`/`deleteIccProfile`(约 755–829 行),对外只暴露模板已引用的同名 ref/函数,签名不变。
- 新增 `useSettingsScrollSpy`(或并入已有 `useSettingsCards` 同级新文件),迁出 `currentSection`/`spySuppressed`/`spyReleaseTimer`/`releaseSpy`/`onSettingsScroll`/`scrollToSection`/`selectSection`/两个 `watch`(route.params.section、settingsQuery)+ `settingsContentRef`(约 538–658 行);`normalizeSection`/`settingsSections`/`SettingsNavId` 类型作为参数传入或保留顶层。
- `settingSearchText`/`sectionSearchCorpus`/`sectionMatches`/`hasSearchResults`(546–570 行,i18n 强耦合 `t`/`bt`/`SETTINGS_MAP`)可选一并搬入同一个 search composable,或维持原地——耦合度略高,视上面三块搬完后剩余体量再定。

**③ 最大风险**:模板里大量绑定按**同名变量**直接引用(如 `cacheStats`、`iccProfiles`、`currentSection`),composable 抽取必须原样保留返回值的变量名与响应性(ref/computed),否则会出现「脚本编译通过但模板悄悄读到 undefined」这类 vue-tsc 不一定能全部拦住的静默错位。

CSS 外置(D-451):单一 scoped 块可外置 SettingsView.styles.css,scope 语义不变;定位可选先行批,详见各详案附注。

---

## src-tauri/src/enhance/service.rs(57KB)

**① 拆(小切口)** — `EnhanceService` 是持有 `Mutex<WorkerHandle>`/`Mutex<JobBook>` 的单一服务对象,job 编排链(enqueue/execute/process_item)彼此强耦合不建议拆;但文件尾部的「容器级 EXIF 注入」是与 `EnhanceService` 状态完全无耦合的纯字节操作单元,是全文件唯一干净刀口。命令壳不在本文件(在 `src-tauri/src/ipc/enhance_commands.rs`),故本文件拆分不涉及 Tauri 命令契约。

**② 主刀口**
- 新增 `src-tauri/src/enhance/exif_inject.rs`,迁出 `build_source_exif`/`read_date_time_original`/`inject_exif_container`/`inject_jpeg_app1`/`inject_png_exif`/`crc32`(约 1127–1240 行),这组函数只吃 `&Path`/`&[u8]` 返回 `Vec<u8>`/`Option<...>`,零 `AppState`/`EnhanceService` 依赖,`finalize_output`(1086 行)是唯一调用点,改为 `use` 即可。
- `OutputFormatChoice`/`EnhanceParams`/`JobStatus`/`JobDto`/`JobRecord`/`JobBook`/`WorkerHandle`/`PreviewPaths`(66–190 行 DTO/内部记录簇)可选迁到 `enhance/types.rs`——纯类型定义,但 `JobRecord`/`JobBook`/`WorkerHandle` 是 `EnhanceService` 私有字段的类型,迁移后仍需同 crate 内可见(`pub(crate)`),风险低于第一刀但收益也小,建议视第一刀落地后是否还嫌肿再做。
- `EnhanceService` 核心 impl(`new`/`enqueue`/`cancel`/`run_job_blocking`/`execute`/`process_item`/`send_request`/`map_worker_failure`/`spawn_enhance_worker`/`finalize_output` 等,197–1126 行)与准入/校验纯函数簇(`task_order`/`sort_steps`/`validate_strengths`/`admission_check`/`estimate_tiles_total` 等,793–935 行,均为 `pub fn` 供 `enhance_commands.rs` 直接调用)原样留在本文件,不建议动——它们是 job 编排与前置校验的共同上下文。

**③ 最大风险**:`sort_steps`/`validate_strengths`/`admission_check`/`estimate_tiles_total`/`item_is_raw` 等 `pub fn` 大概率被 `ipc/enhance_commands.rs` 越过 `EnhanceService` 直接调用(未逐一核实调用点,仅结构判断)——EXIF 注入刀口不涉及这些符号,风险仅限于「如果后续想连带搬这批纯函数,需先 grep 确认调用方 import 路径」。

---

## 顺手发现
无。
