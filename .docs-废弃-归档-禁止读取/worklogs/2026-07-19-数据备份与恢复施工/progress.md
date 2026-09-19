---
status: 快照
type: working-memory
line: 数据备份与恢复施工
created: 2026-07-19
---

# 进度日志:数据备份与恢复施工

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-19(开工)
- 做了:读方案 B 定稿 + 审阅线三件套对齐;拆出本施工线;侦察 P0 §3.1 触点全集(见 findings)。
  确认 document_versions+文件改写点仅 save_version/delete_version 两处;schema CURRENT_VERSION 私有须加访问器。
- 边界:采纳 B-1..B-6 复审建议(见 task_plan)。按 §3.1→§4/5→§6→§7→§8 阶段推进,每阶段验证后 commit。
- 遗留:开工阶段 1(P0 文档一致性门)。

### 阶段 1 收尾(commit 5018c76)
- 做了:①state.rs 加 `document_storage_guard: RwLock<()>` 字段+init;②doc_commands.rs 抽 `write_version_tx`
  纯函数(单事务:插行→写原子文件→回填→commit),`save_version` 闭包改调它、外层持 read guard;
  `delete_version` 改自建 spawn_blocking + read guard;③migration.rs 加 `current_schema_version()`/
  `read_schema_version()` 公开访问器。
- 验证:`cargo check --tests` 绿(9s);`cargo test --lib -- ipc::doc_commands db::migration` **17/17 绿**
  (含新增 write_version_tx 正常/回滚两测);`ipc_commands_keep_rusqlite_off_async_workers`(R1-3 阻塞门)
  + `finish_clears_own_generation`(RunTokenSlot)回归绿。
- 教训:核对本仓 diff 勿加 `-c core.autocrlf=false`——会把 CRLF↔LF 规范化当全文件重写显示(假象);
  默认 `git diff` 才是真改动面。
- 遗留:阶段 2 备份核心。触点已侦察(见 findings「DB/连接」节):VACUUM INTO 用独立连接+同 PRAGMA、
  zip 2/sha2 在依赖、AppError 加 Backup 变体、RunTokenSlot 采 finish 姿态。

### 阶段 2a 收尾(纯备份引擎,commit 待填)
- 做了:①`AppError::Backup{code,message}`(error.rs,同 Relink 稳定码姿态)+ 契约测试;
  ②`connection::apply_pragmas` 提 pub(crate)(VACUUM 连接复用同 PRAGMA 单一事实源);
  ③新模块 `backup/`:`manifest.rs`(Manifest/PayloadEntry/RootEntry/Counts/BackupKind + 常量)、
  `core.rs`(run_backup 纯函数:preflight→VACUUM INTO→quick_check→计数/根/schema 读→文档一致性校验→
  zip 流式打包逐条 SHA-256→manifest 末写→tmp 同卷 rename→auto retention);`mod.rs` 导出 + lib.rs 挂 `pub mod backup`。
- 设计要点:引擎纯函数(时间戳/backup_id/retention 由调用方注入)便于确定性单测;SHA-256 流式算(不驻留大库);
  文档校验用 dunce::canonicalize 卡受控 documents/ 边界(拒符号链接/`..` 逃逸);retention 打开候选包验 manifest.kind==auto(B-6 保守,任何失败=不删)。
- 验证:`cargo test backup:: + doc_commands::write_version` **7/7 绿**(端到端产包 sha256 逐字节吻合 / 不一致零产包 /
  retention 保 2 保手动);`cargo clippy --lib` 我的文件**零警告**(唯一警告在 scan_commands.rs:96,既有非本次);
  rustfmt 我的文件已规整(顺带补齐 Stage 1 doc_commands.rs 的换行规范)。
- 遗留:阶段 2b(app 接线):AppState backup token + file-job gate + IPC(preflight/start/status/list/cancel)+
  在 write guard 下调 run_backup + backup_id(ring rand)/时间戳(chrono)注入 + registry/capabilities + 同卷警告。

### 阶段 2b 收尾(app 接线,commit 待填)
- 做了:①state.rs 加 `file_job_owner`(A/B 门闩 try_acquire/release,常量 FILE_JOB_BACKUP/RESTORE/EXPORT)+
  `backup_token`(RunTokenSlot)+ `backup_progress` 快照 + `cancel_backup`;②core.rs 暴露 `documents_consistent`/
  `dest_writable`/`hex_lower`(pub(crate))+ 补 CODE_DIR_UNSET/CODE_JOB_BUSY;③新 `ipc/backup_commands.rs`:
  BackupProgressPayload + `backup:progress` 事件 + preflight_backup / start_backup(分离任务)/ backup_status /
  list_backups / stop_backup;same_volume cfg(Win 盘符 / Unix dev)+ estimate + backup_id(ring)+ 时间戳(chrono);
  ④registry 注册 5 命令 + ipc/mod.rs 挂模块。
- 设计要点:start_backup 用 `tauri::async_runtime::spawn` 分离任务包 `spawn_blocking`——webview 关闭 invoke future
  被 drop 但分离任务跑到底,门闩释放 + finish token 终态发布不受前端生命周期影响(否则漏释放死锁);
  文档一致性 write guard 在 spawn_blocking 内持有覆盖整个 run_backup,不跨 await(硬约束)。
- 验证:`cargo test backup+backup_commands+state+doc_commands` **14/14 绿**;`cargo clippy --lib` 我的文件零警告
  (唯一 scan_commands.rs:96 既有);R1-3 阻塞门绿(SQL 全在 blocking);rustfmt 已规整。
- 未做(归后续):ipc.ts 前端常量镜像(Stage 5 一并)、config `backup_dir`/`backup_retention` 写入命令
  (config_commands 已有通用 set;Stage 4/5 接)、同卷警告真机验收(GUI)。
- 遗留:阶段 3 恢复状态机(§6)——restore_stage 白名单/zip-slip/sha256 校验、abs_path rebase(§3.2)、
  pending-restore.json phase marker、lib.rs 启动前交换、pre-restore 回滚包。高测试价值(安全关键路径),优先。

### 阶段 3a 收尾(restore_stage 校验暂存,commit 待填)
- 做了:①error.rs 加 `AppError::Restore{code,message}`(8 稳定码)+ 契约测试;②新 `backup/restore.rs`:
  restore_stage 全流程(读 manifest→扫中央目录→解压校验→暂存库门→迁移→§3.2 rebase→摘要);
  mod.rs 导出 restore_stage/RestoreStageResult。
- 复用/镜像:`is_safe_relative_path`(exotic/package.rs,pub)直接复用;`safe_join`/`copy_hashing`/
  `is_symlink_mode` 镜像 install.rs 硬化范式(SHA-256 + AppError::Restore)。
- 安全设计:zip bomb 双锚(解压硬封顶=声明字节+1 + 验实际 sha256==声明)+ checked 总和 + 1TiB 绝对上界 +
  (Win)可用空间余量;白名单=manifest.payload 声明(拒清单外条目);create_new 拒跟随既有/链接 + 落地 symlink 兜底;
  §3.2 rebase 只取 basename、绑定参数改写目标机路径、与包内解压交叉验证(绝不字符串替换旧前缀)。
- 验证:`cargo test backup:: error::tests` **17/17 绿**;关键测=跨机 rebase(源 machineA/目标 machineB 路径不同,
  断言 abs_path 无 machineA 残留、指向 machineB documents)、篡改字节→restore_corrupt+staging RAII 清、
  手造 `../evil` zip-slip→restore_path_invalid+越界文件不落盘、schema=runtime+1→restore_schema_too_new;
  clippy 本模块零警告;rustfmt 已规整。
- 坑:seed 造孤儿 document_versions 行(item_id 无 media_items)被 foreign_key_check 正确拦下(证明 fk 门有效)——
  补全 scan_roots→directories→media_items→document_versions 合法 FK 链修复(真库不含孤儿)。
- 遗留:阶段 3b(启动交换状态机)——pending-restore.json phase marker + lib.rs 启动前交换 + pre-restore 回滚包
  (崩溃矩阵,涉及进程重启,phase-marker 逻辑可单测);3c 恢复 IPC 接线。

### 阶段 3b 收尾(启动交换状态机,commit 待填)
- 做了:①新 `backup/swap.rs`:PendingRestore/RestorePhase + 原子 marker 读写 + perform_swap_at_boot
  (相位机)+ finalize_restore_verified + arm_restore(回滚包+marker);mod.rs 导出。②lib.rs 钩子两处
  (db_path 后调交换、迁移成功后 finalize)。
- 崩溃安全设计:每步先移文件再原子写 marker;重入按「相位 + 实际文件存在性」双判决定前进/逆向;
  每个 move 幂等(目标已存在即跳过)。无 marker 立即返回=正常启动零开销。arm 未接前系统永无 marker,
  钩子完全休眠——正常启动零行为改变(低风险接入)。
- 验证:`cargo test backup::`(含 swap 8 崩溃矩阵)**17/17 绿**;R1-3 阻塞门覆盖 lib.rs 绿;clippy 零警告;fmt 规整。
  崩溃矩阵覆盖:完整交换 Prepared→Installed+finalize 清理、CurrentMoved 续装、install 后崩溃补相位、
  暂存丢失逆向从 old 恢复原始、Prepared 暂存缺中止保原始、Verified 遗留下次清、损坏 marker 拒。
- ⚠ 未验:真机进程重启的启动交换(不可单测,归 GUI 手动验收——方案 §10「手动 GUI」清单)。
- 遗留:阶段 3c 恢复 IPC(restore_stage/arm/status 命令 + FILE_JOB_RESTORE gate + registry);随后 Stage 4/5/6。

### 阶段 3c 收尾(恢复 IPC,commit 待填)—— 恢复后端全通
- 做了:backup_commands.rs 加 3 命令:`restore_stage`(spawn_blocking 调 backup::restore_stage,返回摘要)、
  `restore_arm`(FILE_JOB_RESTORE gate + write guard 下 arm_restore)、`relaunch_app`(app.restart() 发散)。registry 注册。
- 端到端后端闭环已通:**备份** preflight→start_backup(分离任务/write guard/VACUUM/zip/sha256/tmp-rename/retention);
  **恢复** restore_stage(白名单/zip-slip/sha256/暂存库门/迁移/§3.2 rebase)→restore_arm(回滚包+marker)→
  relaunch_app→启动期 perform_swap_at_boot(相位机)→迁移成功 finalize_restore_verified。
- 验证:`cargo test backup:: + backup_commands`(含 swap 崩溃矩阵)**21/21 绿**;R1-3 门(restore_stage/arm 的 SQL 全在 blocking)绿;clippy 零警告;fmt 规整。
- 遗留:Stage 4 自动备份策略(idle 触发/config backup_dir·retention 写/失败可见态);Stage 5 前端(BackupSection +
  恢复向导 + file-job store + zh-CN/en-US);Stage 6 硬化收口(跨机 fixture 已在 3a、restore crash matrix 已在 3b,
  余 preflight 同卷/大盘真机 + 三件套收口)。

### 阶段 4 收尾(自动备份策略,commit da83688)
- 做了:重构 start_backup 抽共享 `begin_backup`(手动/自动共用;update_last_success 成功回写 backup_last_success_at);
  加 `should_run_auto_backup` 纯判据 + `maybe_run_auto_backup`(前台繁忙/派生/交互让步→读 config→可写→Auto 备份)+
  `start_auto_backup_scheduler`(idle~2min 首检+每小时复检);lib.rs setup 接入调度器。config 键(backup_dir/
  backup_auto_enabled/backup_retention/backup_last_success_at)经既有 get/set_app_config 通用命令读写,无需新命令。
- 设计:B-1 未选目录/未开启判据恒 false;retention 默认 5 只轮转 auto 包;调度器 dormant until 用户开启(低风险)。
- 验证:`cargo test backup_commands`(含 auto_backup_decision 边界)+ 全 backup 模块 22/22 绿;R1-3 门覆盖 lib.rs 绿;
  clippy 零警告;fmt 规整。
- 遗留:Stage 5 前端(BackupSection 设置卡 + 恢复向导 + file-job store + zh-CN/en-US)——需 GUI 迭代;
  Stage 6 硬化收口。

### 阶段 5 收尾(前端,commit 835fd73)
- 施工前重新核对当前代码：导出线已先落 `BackgroundFileJobIndicator`/`exportStore`，设置页已有分区搜索、
  折叠卡片持久化与 scroll-spy；因此没有照旧方案另造独立进度 UI，而是把现有指示器扩展为 A/B 共用入口。
- 做了：①新增 `backupStore`（事件先订阅 + snapshot 回填、终态防旧 running 快照倒退、监听失败可重试）和
  `fileJobStore`（导出/备份统一活动任务与取消）；②新增 `BackupSection`（目的地 preflight、同卷/不可写/文档不一致提示、
  首次选目录显式建议每日+保留 5、自动策略/保留数、状态/列表/手动备份/恢复入口）；③新增 `RestoreWizard`（stage 校验、
  摘要/根/外部文档警告、勾选确认 + 精确输入二次确认、arm 后 relaunch）；④扩展后台文件任务状态栏，手动备份终态 toast、
  自动失败持久提示、完成后打开所在目录；⑤补全 IPC 常量、harness fixture 与稳定错误码→文案映射。
- 当前代码约束修正：①备份完成动作调用 `OPEN_DIRECTORY` 打开包所在目录；既有 `SHOW_IN_EXPLORER` 的契约是 itemId，不能传路径；
  ②首屏 bundle gate 仅余个位数 kB，故状态栏指示器用动态组件、备份专属 zh-CN/en-US 用 local composer 随 lazy chunk 加载；
  最终入口 `611.12 kB / 620 kB`，余 `8.88 kB`，备份逻辑留在独立 lazy chunk。
- 视觉回归（本地 `?ui-harness=settings`）：中文/英文切换、搜索“备份”、720×700 窄窗口均通过，备份卡片
  `scrollWidth == clientWidth == 421`；修复大卡片展开后 scroll-spy 把导航误判到“媒体与缩略图”的问题，现保持“存储与设备”。
- 自动化：`npm run lint` 绿；`npm run typecheck` 绿；`vitest` 全量 **98 files / 1252 tests** 绿；
  `npm run build` 绿且 bundle budget 通过。新增备份 store/file-job store/错误映射/路径边界/双语键一致性共 13 个聚焦测试。
- 遗留：Stage 6 跨层硬化与收口；真实系统文件选择器、用真实包备份/取消/恢复重启交换仍须 Tauri 真机验收，
  harness 只能验证前端状态与布局，未冒充端到端恢复实证。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
