---
status: 快照
type: working-memory
line: 数据备份与恢复施工
created: 2026-07-19
---

# 任务计划:数据备份与恢复(方案 B 施工)

## 目标

按 `docs/worklogs/2026-07-17-上线前三功能方案-导出-备份-图片编辑/方案B-数据备份.md`(复审二次核对稿)
落地 v1 数据备份与恢复:完整 catalog DB + `documents/**` + manifest 的一致快照;跨机可恢复;
staged-restart 崩溃安全恢复状态机;自动备份策略;设置页前端。采纳复审建议(B-1..B-6 见下)。

**采纳的裁决(复审建议,用户已隐式接受=「采纳建议开始施工」):**
- B-1 自动备份:未选目录=关;选目录时显式建议「每日 + 保留 5」由用户接受。
- B-2 documents 不可关闭(缺它非完整备份)。
- B-3 接受 staged-restart(活进程换库风险不值)。
- B-4 v1 不加密(明示隐私,AES 归 P2)。
- B-5 v1 完整库(非按表裁剪;lean backup 归 P2)。
- B-6 自动包只轮转 `kind=auto` 且 manifest 验证通过的包,永不碰手动包/陌生文件。

## 当前阶段

阶段 6:测试硬化 + 收口——阶段 1-5 已完成，余跨层硬化、真机恢复交换验收与文档收口

## 阶段

### 阶段 1:P0 文档存储一致性门(方案 §3.1)—— commit 5018c76
- [x] `AppState.document_storage_guard: RwLock<()>`(改 document_versions+文件的闭包持 read;备份持 write)
- [x] `save_version` 改单事务 `write_version_tx`:插行→派生路径→写原子文件→回填路径→commit;失败回滚(最多留可 GC 孤儿文件,DB 永不引用缺失文件)
- [x] `delete_version` 自建 spawn_blocking 块 + 持 read guard(与备份写快照窗互斥)
- [x] 公开 schema 版本访问器 `current_schema_version()`/`read_schema_version()`(恢复门读,不硬编码 21)
- [x] 单测:崩溃语义(文件写失败→零残留行)、正常提交行+文件;R1-3 门 + RunTokenSlot 回归绿
- **状态:** complete

### 阶段 2:备份核心(方案 §4/§5)—— 拆 2a(纯引擎)/2b(app 接线)
**2a 纯引擎(commit 待填)** — 已落地:
- [x] `AppError::Backup { code, message }`(稳定小写 code;不泄漏绝对路径/SQL)+ 契约测试
- [x] manifest 类型(formatVersion/backupId/kind/appVersion/schemaVersion/roots/counts/payload sha256)+ 往返测试
- [x] preflight:目的地可写(写-删探针)——同卷警告归 2b(需卷身份,UI 咨询)
- [x] VACUUM INTO(独立连接+apply_pragmas 单一事实源,参数绑定)→ quick_check → counts/roots/external 读
- [x] 文档一致性逐行校验(§3.1:非空/存在/受控 documents 下 canonicalize/content_hash 匹配)→ 不一致返回 `backup_document_inconsistent` 且不产包
- [x] documents 流式入包(相对路径 `documents/{item_id}/{file}`)+ 逐条未压缩 SHA-256(读一遍边喂 hasher 边 deflate)
- [x] manifest 最后写;`*.tmp` → same-volume rename;工作目录 RAII 清理
- [x] retention 仅 auto 包(文件名前缀 + manifest.kind==auto 验证,保留最新 N,永不碰手动/陌生/无法解析)
- [x] 单测:端到端产包+每条 payload sha256 逐字节吻合、不一致拒绝零产包、retention 保 N 保手动(6/6 绿)
**2b app 接线(commit 待填)** — 已落地:
- [x] AppState:`backup_token`(RunTokenSlot,finish 姿态)+ `file_job_owner`(A/B 共用门闩,try_acquire/release,占用返回 `file_job_busy` 不抢占)+ `backup_progress` 快照
- [x] 备份在持 `document_storage_guard` write guard 下调 run_backup;backup_id(ring SystemRandom 16B hex)+时间戳(chrono UTC)注入
- [x] IPC:preflight_backup / start_backup(**分离任务**,webview 关闭也跑到底、门闩必释放)/ backup_status(event `backup:progress` + snapshot 查询)/ list_backups / stop_backup
- [x] 同卷警告(cfg:Windows 盘符前缀 / Unix dev())+ registry 注册 5 命令 + ipc/mod.rs 挂模块(自定义命令无需 capability;dialog:allow-open 已在)
- [x] 单测:same_volume 同目录 / backup_id hex / estimate 空目录零;R1-3 阻塞门回归绿(SQL 全在 blocking)
- **状态:** complete(2a+2b done;同卷警告是 advisory best-effort,真机验收归 GUI)

### 阶段 3:恢复状态机(方案 §6)—— 拆 3a(校验暂存)/3b(启动交换)/3c(IPC)
**3a 校验暂存(commit 待填)** — 已落地:
- [x] `AppError::Restore { code, message }`(8 稳定码)+ 契约测试
- [x] `restore_stage`:central directory 扫描(is_safe_relative_path 复用 exotic/package,拒 zip-slip/盘符/符号链接/大小写碰撞)+ 白名单(manifest.payload)+ checked 尺寸 + 可用空间余量(Win GetDiskFreeSpaceExW,他平台跳过)
- [x] 解压逐条硬封顶(声明字节+1,防 bomb)+ sha256 校验 + create_new 拒跟随链接 + 落地后 symlink 兜底 → 解到 appdata 同卷 `restore-staging/{backupId}/`;失败 RAII 清理
- [x] 暂存 DB 校验:quick_check + foreign_key_check + formatVersion + schemaVersion≤runtime;老版在暂存副本跑迁移器后再 quick/fk check
- [x] §3.2 abs_path 跨机 rebase(绑定参数改写为目标机路径,只取 basename 不字符串替换旧前缀,item_id+file_name 与包内解压交叉验证,拒逃逸/缺件)
- [x] 单测:跨机 rebase(源/目标 appdata 不同,abs_path 无源机残留)、篡改→corrupt、zip-slip→path_invalid、schema 过新→schema_too_new(4/4 绿)
**3b 启动交换(commit 待填)** — 已落地:
- [x] `backup/swap.rs`:PendingRestore/RestorePhase(prepared/current_moved/installed/verified)+ 原子 marker 读写(write_atomic)
- [x] `perform_swap_at_boot`:无 marker 零开销返回;按相位 + 文件存在性幂等推进(Prepared→CurrentMoved→Installed)或逆向(暂存不可用从 old 恢复原始)+ 损坏 marker 拒
- [x] `finalize_restore_verified`:迁移成功后写 Verified + 清 old/staging/marker(回滚包保留)
- [x] `arm_restore`:pre-restore 回滚包(复用 run_backup)成功后才写 marker(§6.2.1)
- [x] lib.rs 钩子:db_path 后、write 连接前调 perform_swap_at_boot;迁移成功后 finalize(休眠安全,arm 未接前无 marker)
- [x] 崩溃矩阵单测 8/8:完整交换/CurrentMoved 续装/install 后补相位/逆向/Prepared 中止/Verified 收尾/损坏 marker/无 marker
- ⚠ 真机启动交换验收归 GUI(进程重启不可单测)
**3c 恢复 IPC(commit 待填)** — 已落地:
- [x] `restore_stage` 命令(暂存校验,返回摘要,spawn_blocking)
- [x] `restore_arm` 命令(FILE_JOB_RESTORE gate + write guard 下 arm_restore:回滚包 + marker)
- [x] `relaunch_app` 命令(app.restart() 触发重启,启动期交换)
- [x] registry 注册 3 命令;R1-3 门绿
- **状态:** complete(3a+3b+3c;恢复后端全通;真机启动交换验收归 GUI)

### 阶段 4:自动备份策略(方案 §7)—— commit 待填
- [x] `should_run_auto_backup` 纯判据(auto 开 + 已设目录 + 距上次成功≥24h;未选目录恒关 B-1)+ 测试
- [x] `begin_backup` 共享核心(手动/自动共用;成功回写 `backup_last_success_at`)
- [x] `maybe_run_auto_backup`(前台繁忙/派生/交互让步 → 读 config 判据 → 可写 → Auto 备份 retention 默认 5)
- [x] `start_auto_backup_scheduler`(idle~2min 首检 + 每小时复检)+ lib.rs setup 接入(dormant until 开启)
- [x] 失败经 backup:progress 快照留可见态(前端消费,不弹窗)
- ⚠ 真机 idle/24h/退出不阻断 归 GUI 验收
- **状态:** complete(config 键经既有 get/set_app_config 通用命令读写,无需新命令)

### 阶段 5:前端(方案 §8)—— commit 835fd73
- [x] 设置页 `BackupSection`(目的地/同卷警告/自动开关/保留数/上次成功失败/立即备份/列表/从文件恢复)
- [x] 恢复向导(选包→校验→摘要→双确认→staged→重启)
- [x] A/B 共用 `BackgroundFileJobIndicator` + file-job store;zh-CN/en-US
- [x] 设置搜索、窄窗口、卡片展开 scroll-spy 与 lazy bundle 预算回归
- **状态:** complete(代码/自动化/harness GUI 已验；真实文件选择、实际备份包与重启交换留 Stage 6 真机验收)

### 阶段 6:测试硬化 + 收口
- [ ] 自动化:DB 快照并发一致/文档屏障交错/跨机 rebase fixture/包完整性矩阵/restore crash matrix/retention
- [ ] 三件套回写 + 契约文档 + docs 门禁
- **状态:** pending

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| save_version 改单事务(插行+写文件+回填在同一 tx,提交前写文件) | 提交后行必有文件;崩溃只留可 GC 孤儿文件,DB 永不引用缺失文件 | D-101 |
| document_storage_guard 用 std RwLock,仅在 spawn_blocking 闭包内持 | 备份写快照与文档写读写互斥;不跨 .await(项目硬约束) | D-102 |
| 备份前端与专属双语字典随状态栏指示器/设置页 lazy chunk 加载 | 首屏入口仅余 8.88 kB 预算；保持完整双语与跨页后台任务恢复，同时不击穿 620 kB gate | D-103 |
| 文件任务终态门控采 thumb/derive 的 finish 返回值姿态,非 ai/face 的 !is_cancelled | 审阅线 F-013 精度;两姿态并存语义不同,勿"统一" | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
