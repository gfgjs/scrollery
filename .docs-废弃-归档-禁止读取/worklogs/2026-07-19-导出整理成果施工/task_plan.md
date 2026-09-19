---
status: 快照
type: 工作记忆
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-19
---

# 任务计划:方案 A 施工 —— 导出整理成果

## 目标

按 `docs/worklogs/2026-07-17-上线前三功能方案-导出-备份-图片编辑/方案A-导出整理成果.md`(2026-07-19 二次核对稿)落地导出功能。用户已裁决:采纳复审建议(A-1..A-6 全部照建议),B→A→C 顺序中 B(备份)已交付,现施工 A。无人值守施工,代码有较大改动,开工前先核实方案假设。

## 开工前核实结论(与方案的差异)

- 方案假设的 A/B 共享文件任务门闩(`file_job_owner` + `RunTokenSlot` + `FILE_JOB_EXPORT` 常量)**已由 B 落地并验证**(`state.rs`),`FILE_JOB_EXPORT = "export"` 常量已预留,直接复用,无需重新设计。
- **方案 §4 前端部分零先例**:`backup`/`restore` 的 IPC 命令(`preflight_backup`/`start_backup`/...)**未接入任何前端**——`src/constants/ipc.ts` 无对应条目,`src` 下无 `backup` 字样的 `.vue`/`.ts`。B 线的"⏸GUI 真机"实为**后端已通、前端未接**,不是"前端已建、只差真机点验"。这意味着 `BackgroundFileJobIndicator`(方案称"A/B 共用")实际由本次 A 施工从零建立,B 的前端接入留待后续(超出本次范围,不在本线扩大)。
- 模块拆分:方案 §3 只提了单文件 `ipc/export_commands.rs`;实读 B 的实现后,后端遵循 `backup/core.rs`(纯引擎)+ `ipc/backup_commands.rs`(IPC 接线/门闩/事件)两层拆分。A 照此仓库既有惯例拆为 `export/core.rs`(命名/清理/复制/manifest 引擎)+ `ipc/export_commands.rs`(命令/门闩/进度快照),而非塞进单文件——决策见下表 D-002。
- 错误码:方案 §3.3 写的 `export_already_running` 是方案早期猜测;实读 `backup/core.rs` 的 `CODE_JOB_BUSY = "file_job_busy"` 是**域共享稳定码**(A/B 复用同一字符串,前端按此分流「有文件任务在跑」,不分 backup/export)。A 照实现惯例复用 `"file_job_busy"`,不新造 `export_already_running`——决策见 D-003。
- 无批量(chunk)元数据查询/无 tags·albums 批量取值函数先例,需新写(`db/queries/export.rs`)。`scan_roots.alias` 列已存在,直接作 manifest 的 `rootAlias` 源,无需另造别名机制。
- 无文件名合规化(Windows 保留名/非法字符/结尾点空格)工具函数先例,需新写。

## 阶段

### 阶段 1:后端核心引擎

- [x] `error.rs`:新增 `AppError::Export { code, message }` + serialize 映射 + 稳定码常量
- [x] `state.rs`:新增 `export_token: RunTokenSlot`、`export_progress: Mutex<Option<ExportProgressPayload>>`、`export_cancelling: AtomicBool` + `cancel_export`/`is_export_cancelling`/`clear_export_cancelling`(镜像 backup 三件)
- [x] `db/queries/export.rs`:分块(750)批量取路径三段(root.path/alias + rel_path + file_name)+ 用户态字段(rating/color_label/favorited/view_rotation/sort_datetime/file_size/availability)+ tags 名称 + albums 名称
- [x] `export/{core,naming,manifest}.rs`:命名档(original/sequence/date)+ 文件名合规化(保留名/非法字符/结尾点空格/大小写冲突/长路径)、staging 目录 + 逐项 `.tmp`→rename、mtime 保留(新增 `filetime` 依赖)、库内目标判定(canonicalize 后按路径分量比较,不用字符串 `starts_with`)、manifest 结构与原子写入、取消检查点(`Err(CODE_CANCELLED)` 姿态,对齐 backup)
- **状态:** complete

### 阶段 2:后端 IPC 接线

- [x] `ipc/export_commands.rs`:`preflight_export`/`start_export`/`export_status`/`stop_export`,门闩用 `FILE_JOB_EXPORT`,进度=快照+app事件(镜像 `backup_commands.rs` 的 `publish_backup_progress`/`begin_backup`/`finalize_payload` 姿态,终态门控用 `finish()` 返回值姿态,不用 ai/face 的 `is_cancelled` 姿态)
- [x] `ipc/registry.rs` 注册 + `src/constants/ipc.ts` 同步命令名 + `EVENTS.EXPORT_PROGRESS`
- [x] Rust 单元测试:命名合规化边界、chunk 查询不重不漏、staging→正式目录/取消清理只删本 job 路径、mtime 一致、manifest 无绝对路径(23 项新测试,`cargo test --lib` 706→729 全绿;`clippy -D warnings` 本次改动零告警)
- **状态:** complete

### 阶段 3:前端

- [x] `exportStore`(Pinia):事件订阅(`export:progress`)+ `export_status` 快照恢复 + 发起/取消(2026-07-19 复审补:先订阅后拉快照存在"事件先到、旧 running 快照后到"竞态窗口,`applyProgress` 已加终态防倒退覆盖守卫)
- [x] `ExportDialog.vue`:目的父目录选择、命名档、冲突策略(rename/skip)、manifest 开关(默认关、记忆)、预检摘要(条数/估算体积/离线数/非零旋转数/库内警告)、`ViewStale` 重算确认(2026-07-19 复审补:落地——捕获 `ViewStale` 错误码后重取当前选区描述符 + 重跑预检,提示用户核实后再点开始;同批修了 `runPreflight` 的请求竞态,只接受最新一次结果)
- [x] `BackgroundFileJobIndicator.vue`:`AppStatusBar` 右区新增,展示当前文件任务进度/取消/完成入口(仅接 export;为 B 预留但不本线接入)
- [x] 入口接线:选区工具条(SelectionToolbar 命令)+ 媒体右键菜单(ContextMenu 本地命令,选区保留逻辑同 move/copy)→ 统一生成 `SelectionDescriptor`。**相册/当前视图工具栏入口未接线**,见 D-005(已于 2026-07-21 清账波 U-2 落地,commit 5a97bd4)
- [x] zh-CN/en-US 文案 + 错误码分流(`ipcErrorMessage` 通用错误展示,未逐码定制文案——预检/门闩繁忙等场景已够用)
- **状态:** complete

### 阶段 4:验证与收尾

- [x] `cargo test` / `cargo clippy` 全绿(受影响面):706→729,clippy `-D warnings` 本次改动零告警(仓库既有 `scan_commands.rs` 一处 pre-existing 告警未碰)
- [x] `vue-tsc` / `eslint` / vitest 相关用例全绿:1234→1239(新增 `exportStore.spec.ts` 5 例,镜像 `scanStore.spec.ts` 缩略图进度恢复姿态)
- [x] 更新方案文档状态、`docs/todo.md`、登记候选 F-NNN/D-NNN
- [x] 手动 GUI 项列清单标注"不自动化"(真机验收留给用户,见 progress.md)
- **状态:** complete(GUI 真机验收 + D-005 后续小补丁留待用户接续,不阻塞本阶段收尾判定)

## 关键决策

| 决策 | 理由 | 候选 ID |
|---|---|---|
| 施工前先核实方案 §3/§4 假设与当前 HEAD 的差异 | 用户明确提醒方案后代码可能有较大改动 | |
| 后端拆 `export/core.rs` + `ipc/export_commands.rs` 两层,不塞单文件 | 与仓库已落地的 `backup/core.rs` + `ipc/backup_commands.rs` 惯例一致,方案 §3 的单文件只是早期草案 | D-002 |
| 错误码复用域共享 `file_job_busy`,不新造 `export_already_running` | 实读 backup 实现确认该码是 A/B 共享稳定码,前端按同一码分流 | D-003 |
| `BackgroundFileJobIndicator` 本线只接 export,不顺带补 B 的前端 | B 前端接入是独立工作量,超出"施工方案 A"范围,避免范围蔓延 | |
| A-1..A-6 全部采纳复审建议(manifest 默认关/命名档 sequence/不做 zip/free 全量/v1 原字节+提示/不留 overwrite) | 用户已明确"采纳建议" | |
| 取消语义改用 `Err(AppError{code:export_cancelled})`,不用自造的 `Ok(outcome{cancelled:true})` | 对照 `backup/core.rs` 的 `cancelled()` 精读后发现的既有惯例,分块任务的部分结果计数不必靠这条路径回传(靠事件已推过) | D-004 |
| 相册/当前视图工具栏入口本次不接线,只接选区工具条 + 右键菜单(后续已由 2026-07-21 清账波 U-2 接线,commit 5a97bd4) | 后二者已覆盖核心交互面(单选/多选/全选);相册视图工具栏组件本次未及深入摸底,强行接线风险大于收益,同一 `exportStore.openExportDialog` 调用点后续补一行即可 | D-005 |

## 待用户裁决索引

无——A-1..A-6 已由用户裁决"采纳建议"。若施工中出现方案未覆盖的新分歧点,记入本表候选并继续,不阻塞无人值守施工。

## 错误账

| 错误 | 尝试 | 解法 |
|---|---|---|
| (待施工中登记) | | |
