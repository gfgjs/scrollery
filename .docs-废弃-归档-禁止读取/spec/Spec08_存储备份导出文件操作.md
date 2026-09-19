---
id: 2026-07-24-Spec08_存储备份导出文件操作
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec08-存储备份导出文件操作

> 一句话:本篇讲 Scrollery 后端「存储后端抽象(本地/WebDAV)」「数据备份与恢复(方案 B)」
> 「整理成果导出(方案 A)」「文件/文件夹操作(file_ops)」四个子系统的 as-built 机制。
> 服务读者:实现/扩展这四块的工程师与低能力编码代理。读前不需先读其余分篇;数据层全表定义见
> [Spec01_数据层](./Spec01_数据层.md),IPC/错误契约总表见 [Spec10_IPC与错误契约.md](./Spec10_IPC与错误契约.md)
> (本篇未落地前二者可能为空,交叉引用留作最终解析)。

## 1. 概览

四个子系统各自独立成模块,通过 `AppState` 的文件任务门闩(`file_job_owner`)与
`document_storage_guard` 互斥协调,避免同时改写 appdata/文档存储引发数据竞争。

| 子系统 | 主目录 | 职责 |
|---|---|---|
| 存储后端 | `src-tauri/src/storage/` | 统一 trait 抽象本地文件系统与远程 WebDAV,供(未来)scanner 与 IPC 连通性测试复用 |
| 备份与恢复 | `src-tauri/src/backup/` | 完整 catalog DB + `documents/**` 一致快照打包/校验/暂存/交换 |
| 导出 | `src-tauri/src/export/` | 选区/相册/视图的媒体集合原样字节复制到用户目录 |
| 文件操作 | `src-tauri/src/ipc/file_ops_commands.rs` | 媒体项/文件夹的移动、复制、重定位、硬删除 |

**代码位置速查**:

| 文件 | 职责 |
|---|---|
| `storage/mod.rs` | 存储连接测试入口 `count_entries(kind, &ConnParams) -> usize` |
| `storage/local.rs` | 本地/OS 挂载盘连接测试(`std::fs::read_dir` 计数),8A(OS 挂载盘/UNC)复用 |
| `storage/webdav.rs` | 原生 WebDAV 连接测试,仅 `netfs`(perf)编入 |
| `backup/core.rs` | 备份引擎:preflight → VACUUM 快照 → 文档一致性校验 → zip 打包 → 原子落盘 → retention |
| `backup/manifest.rs` | 备份包 `manifest.json` 结构 |
| `backup/dbread.rs` | 备份/恢复共用只读查询(counts/roots/external/quick_check 单一事实源) |
| `backup/restore.rs` | 恢复暂存:白名单 + zip-slip 防守 + 逐条 SHA-256 校验 + 跨机 rebase |
| `backup/swap.rs` | 恢复的启动期交换状态机(phase marker + 文件存在性双判) |
| `export/core.rs` | 导出引擎:staging 复制 + 命名 + manifest + 库内目标判定 |
| `export/naming.rs` | 命名档(original/sequence/date)+ 文件名合规化(纯函数) |
| `export/manifest.rs` | 导出 `manifest.scrollery.json` 结构与原子写入 |
| `ipc/file_ops_commands.rs` | 媒体/文件夹 move/copy/relocate/delete 的 IPC 命令 |
| `ipc/backup_commands.rs` | 备份/恢复 app 层接线(门闩、进度事件、自动备份调度器) |
| `ipc/export_commands.rs` | 导出 app 层接线(门闩、进度事件) |
| `ipc/storage_commands.rs` | 存储后端 CRUD + 连通性测试(密码走 keyring) |
| `ipc/volume_commands.rs` | 已知卷面板(列表/重命名/忘记) |

**在整机中的位置**:

```
前端 backupStore/exportStore/fileJobStore ──IPC──▶ ipc::{backup,export,file_ops,storage,volume}_commands
                                                        │
              ┌─────────────────────────────────────────┼───────────────────────────────┐
              ▼                                         ▼                               ▼
       backup::core / restore / swap            export::core / naming / manifest   AppState(db_writer/
              │  (纯函数,调用方注入时间戳/id)              │ (纯函数,同姿态)         db_read_pool/
              ▼                                         ▼                          file_job_owner/
       db::schema / db::queries(scan/                   fs::copy + filetime         document_storage_guard/
       config 等) + fs(zip/VACUUM INTO)                                             tree_snapshots/layout_cache)
```

存储层目前只提供连接测试:由 `ipc::storage_commands::test_backend` 调用 `storage::count_entries`,
返回 base 目录下的项数。原先预留的 `StorageBackend` trait、`RemoteEntry`、`stat`/`read_range`
与 `build_backend` 工厂因无生产消费者已删除(P15);scanner 不经该层,走本地路径(见 §5 边界)。

## 2. 数据模型与状态

### 2.1 DB 表(权威定义见 [Spec01_数据层.md](./Spec01_数据层.md),此处仅摘要关键列)

- `storage_backends`(`db/schema.rs` `CURRENT_SCHEMA`):`id, kind('local'|'smb'|'webdav'),
  name, host, base_path, username, cred_ref, options, created_at`——**连接管理表**(配置与连通性测试)。
  密码**不落库**,仅存 `cred_ref`(keyring 账户名 `storage_backend_<id>`,见 `ipc/storage_commands.rs:26`)。
  媒体/扫描根的卷身份不在此表,由 `volumes` 承担;`scan_roots.backend_id` 列与外键已删除。
- `volumes`:`id, stable_id(UNIQUE), label, kind('local'|'removable'|
  'network'), last_mount_path, last_seen, is_online, created_at`。`scan_roots.volume_id` /
  `media_items.volume_id` 均 `ON DELETE SET NULL`(忘记卷不删媒体行,见 `ipc/volume_commands.rs:81`)。
- 备份/导出**不新增专属 DB 表**——备份包 manifest 与导出 manifest 均为磁盘上的 JSON 文件
  (见 §2.2),不进 DB。

### 2.2 内存/文件状态

| 状态 | 归属 | 说明 |
|---|---|---|
| `AppState.file_job_owner: Mutex<Option<&'static str>>` | 内存 | A/B 文件任务门闩(严格 claim-if-free),owner ∈ `FILE_JOB_BACKUP\|RESTORE\|EXPORT`(`state.rs:294-296`) |
| `AppState.document_storage_guard: RwLock<()>` | 内存 | 文档一致性写守卫;`run_backup`/`arm_restore` 须在其 write guard 下调用(`state.rs:232`) |
| `AppState.backup_token` / `export_token: RunTokenSlot` | 内存 | 取消令牌槽,带运行代次(generation),防止终态发布被后一轮顶掉(`state.rs:242,256`) |
| `AppState.backup_progress` / `export_progress` | 内存 | 最近一次进度/终态快照,供 webview 重载后 `backup_status`/`export_status` 查询 |
| `<app_data>/pending-restore.json` | 磁盘(`backup/swap.rs`) | 恢复交换的持久状态机 marker(`PendingRestore { backup_id, staging_dir, rollback_path, phase }`) |
| `<app_data>/restore-staging/{backupId}/` | 磁盘 | `restore_stage` 解压落点(校验通过后保留,供 arm/swap 用) |
| `<app_data>/restore-old/{backupId}/` | 磁盘 | 交换时移出的旧活库现场(回滚源) |
| `<app_data>/restore-rollback/` | 磁盘 | arm 阶段生成的 pre-restore 回滚包(≥7 天,UI 手动清理) |
| `config.toml` 的 `backup_dir` / `backup_auto_enabled` / `backup_retention` | 磁盘(config) | 备份目的地与自动备份策略(A2 决策:唯一真源已切到 `ConfigManager`) |
| `app_config.backup_last_success_at`(DB 键值) | DB | 自动备份判据用的状态类键(仍走 DB,非 schema 设置类键) |

### 2.3 备份包 manifest(`backup/manifest.rs`)

`manifest.json`(包内固定条目名 `ENTRY_MANIFEST`)自描述:

```
Manifest {
  format_version: u32,          // BACKUP_FORMAT_VERSION=1
  backup_id: String,             // 稳定标识,暂存目录命名用
  kind: "manual" | "auto",
  app_version, schema_version,   // 运行时读,非硬编码
  created_at_utc: String,        // RFC3339
  roots: Vec<RootEntry{id,alias?,hidden}>,
  counts: Counts{items,albums,tags,named_persons},
  external_document_versions: i64,
  payload: Vec<PayloadEntry{path, bytes, sha256}>,  // db/scrollery.db + documents/{item_id}/{file}
}
```

包内固定条目:`db/scrollery.db`(`ENTRY_DB`)、`documents/{item_id}/{file_name}`(`ENTRY_DOCUMENTS_PREFIX`)、
`manifest.json`。**不写凭据**;`manifest.json` 单条目读取上限 `MAX_MANIFEST_BYTES = 64MiB`
(`manifest.rs:18`,所有读 manifest 的路径——恢复扫描/备份列表/retention 校验——都受此封顶)。

### 2.4 导出 manifest(`export/manifest.rs`)

`manifest.scrollery.json`(`MANIFEST_FILE_NAME`)只记录**成功**导出项;失败/跳过明细走任务
IPC 结果,不进 manifest。不写源绝对路径/凭据/人脸框/人物身份。

```
Manifest { format_version:1, app:"Scrollery", app_version, exported_at_utc,
  source: ExportSource::{Selection | Album{id,name} | View{name}},   // 结构化枚举,非自由字符串
  items: Vec<ManifestItem{file, root_alias, root_relative_path, sort_index,
    view_rotation, rating, color_label, favorited, tags, albums}> }
```

## 3. 关键流程与算法

### 3.1 备份(`backup::core::run_backup`,`core.rs:134-221`)

```
0. ensure_dest_writable(dest_dir)                          — 写-删探针文件,失败→ backup_dir_not_writable
1. work_dir = app_data_dir/.scrollery-backup-work-{id}
   vacuum_into(source_db, work_dir/scrollery.db)            — 独立连接+同生产 PRAGMA+自定义 collation 注册
                                                               (VACUUM INTO ?1,参数绑定路径)
   cancel? → backup_cancelled
2. 打开暂存库快照:quick_check(PRAGMA quick_check=='ok') → 不通过 backup_io
   读 schema_version / counts / roots / external_document_versions
3. validate_appdata_documents(verify_hash=false)            — 路径非空/文件存在/canonicalize 后位于
                                                               documents/ 之下;不合规 → backup_document_inconsistent
                                                               (不比对内容哈希,下沉到第4步单遍读)
4. write_package(tmp_path):
     zip 流式写 db → 各 document → manifest(最后写)
     每个文件 add_file_streaming: 边读边写 zip 边算 SHA-256(入 manifest)
     + 若 expected_xxh3 存在同时算 xxh3 与之比对（融合校验,§3.1 一致性硬门下沉于此）
     不符/取消 → 清 tmp,返回错误,不留半截包
5. std::fs::rename(tmp_path, final_path)                    — 同卷原子落名;此后视为已提交,不再取消
6. kind==Auto 且指定 retention → apply_auto_retention        — 仅删「文件名前缀 Scrollery-auto- +
                                                               manifest.kind==auto 验证通过」的包,
                                                               按文件名(时间戳)保留最新 keep 份
```

**前置不变量**(`backup/mod.rs:12`):`run_backup` 须在持 `AppState.document_storage_guard`
write guard 的 blocking 上下文调用——保证 VACUUM 快照与 documents 文件来自同一逻辑时点,
文档写不穿插(为什么:否则备份期间保存新文档版本会造成"部分新、部分旧"的不一致快照)。

VACUUM 暂存库特意放在 `app_data_dir`(本地源卷)而非 `dest_dir`(可能是慢速 USB/NAS)——只有
最终 tmp→正式包的 rename 须与正式包同卷(`core.rs:139-142`,审查 #15)。

### 3.2 恢复:validate → stage → arm → restart → swap → verify/rollback(方案 B §6)

**Stage 1 — `restore::restore_stage`(`restore.rs:113-169`)**,把备份包当**不可信输入**:

```
1. 打开包 + 读 manifest(取上限 MAX_MANIFEST_BYTES);format_version 超本二进制支持 → restore_format_unsupported
   backup_id 须通过 is_safe_backup_id(单段、拒 `..`/绝对/盘符/反斜杠/保留设备名) → 否则 restore_path_invalid
2. scan_central_directory:                                    — 中央目录逐条目扫描
     条目名安全(is_safe_relative_path) + 非符号链接
     白名单:仅 manifest.json 或 manifest.payload 声明的路径,多余条目 → restore_corrupt
     checked 累加解压尺寸,超 MAX_ENTRIES(4M)/MAX_TOTAL_UNCOMPRESSED(1TiB) → restore_size_limit
     可用空间余量判定(Windows GetDiskFreeSpaceExW,`restore.rs:456`)不足 → restore_size_limit
3. extract_and_verify → target_app_data_dir/restore-staging/{backupId}/
     每 payload: safe_join + OpenOptions::create_new(拒跟随既有文件/符号链接)
     边写边核 size+sha256,与声明不符 → restore_corrupt;落地后再核非符号链接兜底
4. validate_staged_db: quick_check + foreign_key_check + **格式标识必须等于当前 SCHEMA_VERSION**
     包内标识 != 当前格式(过旧或过新,同一路径)→ restore_schema_incompatible
     不做任何结构变更,也不在暂存副本上「升级」(已无迁移桥)
5. rebase_appdata_documents(§3.2 跨机):
     只取旧 abs_path 的 basename(不字符串替换旧前缀,兼容跨平台分隔符)
     构造 rel = documents/{item_id}/{file_name},须通过 is_safe_relative_path
     与包内已解压路径**交叉核对**存在且非符号链接 → 否则 restore_document_missing/restore_path_invalid
     绑定参数 UPDATE document_versions SET abs_path=?2 WHERE id=?1（rebase 到目标机路径）
6. 从暂存库读 counts/roots/external_document_versions 供 UI 双确认摘要
```

`validate_staged_db`(第 4 步)**这一步只读**:quick_check + foreign_key_check + 读格式标识,**不执行任何历史 DDL/DML**(迁移桥已随单一当前格式退役),即不再存在「对不可信暂存库跑迁移」的路径。其后第 5 步 `rebase_appdata_documents` 仍会写暂存库:以**绑定参数** `UPDATE document_versions SET abs_path=?2 WHERE id=?1` 把 appdata 文档版本改写为目标机路径。故校验阶段只读、暂存副本整体并非只读;包内其余不可信输入仍由白名单 / 路径安全 / SHA-256 各层承担。

**Stage 2 — `swap::arm_restore`(`swap.rs:275-310`)**:双确认后,先生成 pre-restore 回滚包
(`run_backup` 打当前活库全量备份到 `restore-rollback/`)**成功后才写** `pending-restore.json`
(phase=`Prepared`)——回滚包失败绝不 arm。之后前端调 `relaunch_app` 重启。

**Stage 3 — 启动期交换 `swap::perform_swap_at_boot`(`swap.rs:163-223`)**,须在 DB 写连接/
读池创建**之前**调用:

```
phase 相位机(marker + 实际文件存在性双判,非固定顺序假设):

Prepared:
  暂存库缺失 → restore_current_from_old(幂等,未移动则 no-op) + 删 marker → 中止(Ok(None))
  否则 move_current_to_old → 写 CurrentMoved → install_staging_to_live → 写 Installed → Ok(Some(id))

CurrentMoved:
  活库已在位 或 暂存 db 仍在 → 无条件再跑 install_staging_to_live(幂等补装 documents,
    §#2 修复:即使 db 已装,documents 仍可能滞留 staging——旧实现只补写 phase 不 install,
    导致 finalize 时随 old 一并删掉 staging 里的 documents,造成文档永久丢失)
    → 写 Installed → Ok(Some(id))
  否则(活库已移走且暂存不可用) → 逆向 restore_current_from_old + 删 marker → Ok(None)

Installed:
  交换已完成,继续正常启动;由调用方在建库 / 格式判别通过后调 finalize_restore_verified

Verified:
  上次已 verify 但清理前崩溃 → 本次续清(finalize_restore_verified) → Ok(None)

无 marker → 立即 Ok(None)(正常启动零开销)
```

`finalize_restore_verified`(成功收尾):先写 `Verified`(便于清理前崩溃续清)→ 清
`restore-old/`+`restore-staging/{id}`+marker;**回滚包保留**(≥7 天,UI 手动清)。

`rollback_restore_at_boot`(换入库不可用,典型格式判别失败):撤下已装入的新库文件/documents →
`restore_current_from_old` 逆向恢复 → 清半装 staging 与 marker。**须在活库连接已 drop 后调用**
(Windows 被占用文件不可删/移)。

### 3.3 导出(`export::core::run_export`,`core.rs:169-343`)

```
staging_dir = target_parent/.scrollery-export-{job_id}.tmp
预占 manifest 文件名(include_manifest 时,防导出项与 manifest 撞名被覆盖,审查 P2)
for (index, meta) in items:
  cancel? → 清 staging,返回 export_cancelled
  离线/缺失(meta.availability != "online") → 记 source_missing,continue(不阻断整批)
  build_file_name(scheme, index, total, file_name, sort_datetime)   — naming.rs 纯函数
  resolve_conflict(candidate, used, allow_rename)                   — 大小写不敏感去重,Skip/Rename 二选一
  路径长度 > MAX_STAGING_PATH_LEN(240,Windows MAX_PATH 保守护栏) → path_too_long,continue
  tmp_target = staging/.export-tmp-{index}（与 final_name 脱钩,防 `.X.tmp` 撞最终名,审查 P2）
  fs::copy(src, tmp_target) → filetime::set_file_mtime(来自 DB meta.file_mtime,非复制瞬间源 mtime)
    → fs::rename(tmp_target, final_target)
  StorageFull → 任务级故障(继续只会耗尽整批),清 staging,返回 export_io
  其它 IO 失败 → 记 item_io,continue
include_manifest → Manifest::write_into(staging_dir)                — tmp→rename 双保险原子写
终局目录名去重(撞名追加 -2/-3,审查 P1)
fs::rename(staging_dir, final_dir)                                  — 整目录原子落正式目录
```

`ensure_target_writable`(`core.rs:91-118`):canonicalize + 随机后缀探针文件写删。
`is_inside_library`(`core.rs:123-130`):目标 canonicalize 后与全部 `scan_roots.path`
(亦 canonicalize)做**组件级** `Path::starts_with` 比较(非字符串前缀),库内需前端二次确认
(`ipc/export_commands.rs` 的 `export_target_inside_library`)。

### 3.4 文件操作(`ipc/file_ops_commands.rs`)

| 命令 | 机制 |
|---|---|
| `create_physical_folder` | `tokio::fs::create_dir_all`,不在任何既有扫描根内则自动 `add_scan_root` |
| `move_media_items` | 逐条:读路径 → `fs::rename` → `delete_media_item_hard`(DB 行删,CASCADE 重算 person 派生);源目录变空则移入系统回收站(`trash::delete`) |
| `copy_media_items` | 逐条 `fs::copy`,不改 DB(前端随后触发重扫纳入) |
| `relocate_media_items` | 同 item id 重定位(`fs::rename` + `UPDATE directory_id`),保留缩略图/AI 嵌入,返回 `from_dir_id` 供撤销精确逆操作 |
| `copy_media_items_db` | 复制文件 + `duplicate_media_item_into_dir`(新行同 cache_key,复用缩略图) |
| `remove_media_items_hard` | `trash::delete` + `delete_media_item_hard` + 按 cache_key 清 4 档缩略图/ai_thumb/sprite/motion 派生孤儿 |
| `move_directory` | 子树整体重写:`remap_rel` 重算全部后代 `rel_path/depth/root_id/tree_sort_key`,`compute_cache_key` 重算受影响媒体的 cache_key,`relocate_cache_files` 尽力重定位缓存文件;跨卷用 `move_path_with_fallback`(rename 失败→复制整树+删源) |
| `copy_directory` | `copy_dir_recursive`;新文件靠随后重扫纳入为全新资产 |
| `delete_directory_to_trash` | 先 `cancel_scan(root_id)` 防竞态,再 `trash::delete` + `delete_directory_by_id`(CASCADE) |

**`InvalidateOnWrite` RAII 守卫**(`file_ops_commands.rs:26-60`):四个批量命令(move/relocate/
copy_db/remove_hard)逐条自动提交 DB,中途某项失败时前面项已生效。守卫在 `Drop` 时按
`dirty` 标志决定失效布局缓存 + bump 数据版本 + 清文件树快照,成功/报错路径共用同一收尾
(为什么见 §4 不变量表)。

**并发/线程模型**:全部 fs 重操作(rename/copy/VACUUM/zip 打包/解压)下沉
`tokio::task::spawn_blocking`,不占 tokio worker;WebDAV 用独立 current-thread `Runtime` +
`block_on`,必须在无环境运行时的线程调用(否则「运行时套运行时」panic,`webdav.rs:8-10`)。
备份/恢复/导出的实际执行体在**分离任务**(`tauri::async_runtime::spawn`)里跑,webview 关闭
也跑到底,门闩在收尾 `finally` 式释放。

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令(要点摘要;全量入参出参/错误码权威表见 [Spec10_IPC与错误契约.md](./Spec10_IPC与错误契约.md))

| 命令 | 模块 | 关键错误码 |
|---|---|---|
| `preflight_backup` / `start_backup` / `stop_backup` / `backup_status` / `list_backups` | `backup_commands.rs` | `backup_dir_unset` `backup_dir_not_writable` `backup_document_inconsistent` `backup_cancelled` `backup_io` `file_job_busy` |
| `restore_stage` / `restore_arm` / `relaunch_app` | `backup_commands.rs` | `restore_format_unsupported` `restore_schema_incompatible` `restore_corrupt` `restore_path_invalid` `restore_size_limit` `restore_document_missing` `restore_io` `restore_rollback_failed` `restore_marker_corrupt` |
| `preflight_export` / `start_export` / `stop_export` / `export_status` | `export_commands.rs` | `export_target_invalid` `export_target_not_writable` `export_cancelled` `export_io` `export_target_inside_library` `file_job_busy` |
| `list_backends` / `test_backend` / `add_backend` / `remove_backend` | `storage_commands.rs` | 统一走 `AppError::System`(keyring/DB 错误不泄漏内部串) |
| `list_volumes` / `rename_volume` / `forget_volume` | `volume_commands.rs` | `AppError::System`(空标签等校验错误) |
| `create_physical_folder` / `move_media_items` / `copy_media_items` / `relocate_media_items` / `copy_media_items_db` / `remove_media_items_hard` / `move_directory` / `copy_directory` / `delete_directory_to_trash` | `file_ops_commands.rs` | `CreateFolder` `MoveFile` `CopyFile` `InvalidMove` `DirectoryExists` `MediaNotFound` |

### 4.2 不变量清单

| 不变量 | 违反后果 | 出处 |
|---|---|---|
| `run_backup`/`arm_restore` 须在 `document_storage_guard` write guard 下调用 | VACUUM 快照与 documents 文件来自不同逻辑时点,产出"部分新部分旧"的不一致备份 | `backup/mod.rs:12`,`ipc/backup_commands.rs:285-289,616-620` |
| A/B 文件任务门闩严格 claim-if-free(不可重入) | 若同名可重入,自动调度器每小时 begin 会在手动备份进行中再抢同一逻辑槽,致 UI 永卡 running、门闩被提前让出 | `state.rs:696-715`(审查 #9) |
| 备份失败/取消路径必清 `*.tmp`,绝不留半截包 | 用户看到"备份成功"但包实际损坏 | `backup/core.rs:196-203` |
| 只有 tmp→正式包 rename 须与正式包同卷,VACUUM 暂存不必 | 反之在慢速 dest 卷上三次全量过慢链,且全程占文档写 guard | `backup/core.rs:139-142`(审查 #15) |
| `apply_auto_retention` 仅删「文件名前缀 + manifest.kind==auto 验证通过」的包 | 手动包 / 陌生文件 / 无法解析的包一律不碰,防误删用户手动包 | `backup/core.rs:458-460`(B-6) |
| 恢复把备份包当**不可信输入**:白名单 + zip-slip + 符号链接拒 + checked 尺寸 | 缺任一层,敌意包可任意写盘/OOM/权限提升 | `backup/restore.rs` 全文,`restore.rs:2-5` |
| `backup_id` 恒须过 `is_safe_backup_id`(单段、拒穿越/绝对/盘符) | 它被直接拼进 `restore-staging/{id}`/`restore-old/{id}` 并进 `remove_dir_all`,不校验则任意路径可被清理/覆盖 | `restore.rs:67-74`,`ipc/backup_commands.rs:580-585`(IPC 层不信前端裸 `staging_dir`,服务端派生) |
| `perform_swap_at_boot` 须在 DB 写连接/读池创建**之前**调用 | 交换涉及移动/替换 `scrollery.db` 文件,活连接会阻塞 rename(尤其 Windows 文件锁) | `swap.rs:159-161` |
| `CurrentMoved` 相位无条件重跑 `install_staging_to_live`(即使活库已就位) | 否则 documents 可能永久滞留 staging,`finalize` 随 old 一并删除致文档丢失(§#2 修复) | `swap.rs:196-201` |
| `Prepared` 相位暂存缺失时先 `restore_current_from_old` 再删 marker | 崩溃可能已发生在"活库移入 old"之后、marker 更新之前;不逆向会让活库位置为空、app 建全新空库(§#6) | `swap.rs:172-184` |
| 导出/备份的 manifest 一律「全部文件落地后最后写、tmp→rename」 | 保证任何中途检视 staging 都不会看到半写 manifest | `export/manifest.rs:67-70`,`backup/core.rs:394-402` |
| `InvalidateOnWrite` 守卫在 Drop 时按 `dirty` 统一失效(不是循环尾) | 批量命令中途失败时前 N-1 项已生效,若失效挂循环尾会被 `?` 提前跳过,画廊持续端出已删/已移项 | `ipc/file_ops_commands.rs:19-60`(2026-07-06 审查 P1-6) |
| 导出 tmp 文件名与最终文件名脱钩(用循环下标而非 final_name 派生) | 否则某项最终名恰为 `.X.tmp`(源文件本名如此)会被后续同名项的 tmp 写入静默覆盖 | `export/core.rs:256-259`(审查 P2) |
| VACUUM INTO 目标路径参数绑定(`core.rs:250-251`);SQL 全参数绑定为项目通则(非备份模块独有,详见 `./Spec14_不变量与约定.md`/`./Spec01_数据层.md`) | 防 SQL 注入(Hard Constraint) | `backup/core.rs:250-251` |
| cred(存储后端密码)不落库,只存 keyring,DB 仅记 `cred_ref` | 数据库泄漏不连带凭据泄漏 | `ipc/storage_commands.rs:9-11,89-92` |

### 4.3「不可擅改」项(链裁决出处)

- `validate_staged_db` **不得**对不可信包执行任何结构变更或历史迁移 DDL/DML;唯一允许的写入是
  §3.2 第 5 步对 `document_versions.abs_path` 的**绑定参数** rebase。包内格式不等于当前
  `SCHEMA_VERSION` 时一律 `restore_schema_incompatible` 拒绝,勿再引入任何「升级暂存库」的路径。
- 门闩"严格 claim,不可重入"(§4.2)与 AI/face 槽的"可重入 + 显式 cancel 续接"故意不同姿态,
  勿套用后者的模式"统一"改造。

## 5. 边界情况与失败模式

| 边界 | 处理 |
|---|---|
| 备份目的地空间不足 | `estimate_backup_bytes` 仅供 preflight 展示上界,不做硬校验;真正硬门在写入失败时的 `StorageFull`/`backup_io` |
| 恢复目标空间不足 | `scan_central_directory` 在解压前 `available_space`(Windows `GetDiskFreeSpaceExW`)+ `SPACE_MARGIN` 余量判定,不足 → `restore_size_limit`;非 Windows 平台该项检查被跳过(`restore.rs:468-471`,**待核实**:非 Windows 无此层防线是否有替代兜底) |
| 跨卷移动(文件夹) | `move_path_with_fallback`:`rename` 失败(Windows `ERROR_NOT_SAME_DEVICE`)回退为整树复制 + 删源(`file_ops_commands.rs:639-659`) |
| 换入库不可用 boot-loop 防守 | `rollback_restore_at_boot` 供调用方在换入库建库 / 格式判别失败时撤新装库、逆向恢复原始现场、清 marker,避免 `Installed` marker 永久卡启动(`swap.rs` 相位机测试) |
| 备份库格式与当前不符(过旧或过新) | 统一走 `restore_schema_incompatible`(`restore.rs::validate_staged_db`),无 old/new 分支;不升级、不改写暂存副本,由用户用当前版本重新导出 |
| 离线卷(volumes.is_online=false) | 备份/导出/文件操作均不特殊处理离线卷——导出侧靠 `meta.availability != "online"` 记 `source_missing` 跳过;`forget_volume` 走 `ON DELETE SET NULL`,媒体行保留(离线≠删除) |
| 损坏备份包 | `restore_stage` 各层(zip 打开失败/manifest 解析失败/SHA-256 不符)统一映射 `restore_corrupt`,staging 由 `StagingGuard` RAII 清理不留残迹 |
| 路径穿越攻击面 | 本地/OS 挂载盘连接测试不拼接相对路径(整条 base 原样交给 `read_dir`),无路径穿越面;恢复侧 `is_safe_relative_path`(复用自 `exotic::package`)+ `safe_join`;导出侧 `is_inside_library` 组件级比较非字符串前缀 |
| 取消(备份/导出) | 备份:`cancel.is_cancelled()` 在关键节点检查,取消清 tmp/工作目录,不留残迹;导出:同姿态,仅清本 job 的 staging,不碰已落名的旧正式目录 |
| 并发文件任务冲突 | A/B 门闩(`file_job_owner`)+ 各自 `RunTokenSlot`(generation 判发布权)双重防线,占用中返回 `file_job_busy`,不抢占、不排队 |
| 大小写碰撞(Windows 不敏感文件系统) | 恢复:`scan_central_directory` 对条目名小写化去重;导出:`resolve_conflict` 同样小写键判冲突 |

## 6. 重建指引(从零实现)

### 6.1 依赖顺序

1. `storage::local::count_entries` + `storage::count_entries` 入口(无外部依赖,纯 `std::fs`)。
2. `backup::manifest` 类型(纯数据结构,先于 core/restore 存在)。
3. `backup::dbread`(counts/roots/quick_check 单一事实源)→ `backup::core`(依赖 db::schema
   读格式标识)→ `backup::restore`(依赖 core 的 manifest 类型 + `exotic::package::
   is_safe_relative_path`)→ `backup::swap`(依赖 core::run_backup 生成回滚包)。
4. `export::naming`(纯函数,穷举边界单测优先写)→ `export::manifest` → `export::core`。
5. `ipc::backup_commands` / `ipc::export_commands`(依赖 `AppState` 门闩/token/guard 已存在)。
6. `ipc::file_ops_commands`(依赖 `db::queries` 的 `get_item_path_info`/`get_directory_subtree`
   等,及 `thumbnail::generator::THUMB_TIERS`/`utils::hash::compute_cache_key`)。
7. `storage::webdav`(仅 `netfs` feature,依赖 `reqwest_dav` + 独立 tokio Runtime)。
8. `ipc::storage_commands` / `ipc::volume_commands`(依赖 keyring + `db::queries::storage`/
   `db::queries` 卷相关函数)。

### 6.2 外部 crate

| crate | 用途 |
|---|---|
| `zip` (2.x) | 备份/导出包的 zip 读写(`CompressionMethod::Deflated`, `large_file(true)` 支持 ZIP64) |
| `sha2` | 备份包 payload / 恢复校验的 SHA-256 |
| `xxhash-rust`(xxh3) | 文档内容 xxh3 校验(与 `content_hash` 列比对) |
| `ring` | Ed25519(exotic 签名,交叉复用)+ `SystemRandom` 生成 backup_id/job_id |
| `filetime` | 导出复制后回写 DB 记录的 `file_mtime`(跨平台 mtime 保留) |
| `dunce` | Windows 路径前缀去除(`\\?\` 规范化,canonicalize 场景) |
| `walkdir` | preflight 估算 `documents/` 目录总字节 |
| `keyring` | 存储后端密码的 OS 凭据库存取 |
| `reqwest_dav`(`netfs`/`perf` feature only) | 原生 WebDAV 客户端 |
| `tokio_util::sync::CancellationToken` | 备份/导出的取消协作 |
| `trash` | 文件/文件夹移入系统回收站(可恢复,比硬删除更安全) |

### 6.3 坑与教训(链 `docs/experience.md`)

- 门闩重入 vs 严格 claim 的姿态选择差异见 `state.rs:696-703` 内联注释(审查 #9)。
- `InvalidateOnWrite` 挂 Drop 而非循环尾,是"批量命令部分成功仍需失效"这一类缺陷的通用修法
  (2026-07-06 审查 P1-6),遇到类似"逐条自动提交 + 早退 `?`"的批量命令应默认套用此姿态。
- 恢复的"融合校验"(单遍读同时算 SHA-256 与 xxh3)是为避免 validate 阶段与打包阶段各整读一次
  文件(审查 #14,双读 + 无界内存);新增校验维度时优先塞进现有单遍读,而非新增一趟读。

### 6.4 验收

- 单测(纯函数/引擎层,无需真机):
  - `src-tauri/src/backup/core.rs`(`#[cfg(test)] mod tests`,含 SHA-256 逐字节校验、
    文档不一致拒绝、篡改检测、retention 保留策略)
  - `src-tauri/src/backup/restore.rs`(跨机 rebase、篡改拒绝、zip-slip、恶意 backup_id、
    格式标识不符拒绝:过旧与过新同一条路径)
  - `src-tauri/src/backup/swap.rs`(全部相位崩溃恢复场景:Prepared/CurrentMoved/Installed/
    Verified 各断点续跑、逆向恢复、rollback)
  - `src-tauri/src/export/core.rs`(happy path、取消清理、离线跳过、冲突去重、tmp 名脱钩、
    manifest 文件名预占、终局目录名去重)
  - `src-tauri/src/export/naming.rs`(合规化穷举:非法字符/保留设备名/上标变体/序号位宽)
  - `src-tauri/src/storage/local.rs`(路径穿越拒绝穷举:`..`/绝对/盘符/UNC 注入)
- Gate 命令(项目级,见 `.github/workflows/ci.yml`):`cargo test`(workspace)、`cargo clippy`、
  `cargo fmt --check`。本篇四子系统均无 GUI 专属测试脚本,真机验收(备份→恢复真实重启交换、
  导出到网络盘、WebDAV 连通性)标记为**未自动化,需人工步骤**。

## 7. 关联

- 上游正典:存储后端抽象属 `docs/refactor_2026/Part5_前端体验重构.md`(P5 §1.4.1/§3.8,
  8A/8B 设计理由);备份(方案 B)与导出(方案 A)的完整设计文档见
  `docs/lines/上线前三功能方案-导出-备份-图片编辑.md` 及
  `docs/worklogs/2026-07-17-上线前三功能方案-导出-备份-图片编辑/方案A-导出整理成果.md` /
  `方案B-数据备份.md`(理由/权衡/历史,不复制其论证段落)。
- 相关 worklog/decisions:`docs/worklogs/2026-07-19-数据备份与恢复施工/`、
  `docs/worklogs/2026-07-19-导出整理成果施工/`、`docs/planning/2026-07-21-备份测试硬化专项/
  findings.md`(`#5` 指纹守卫 deferred;`#10` 不可信 DB 迁移加固随迁移桥退役而失效)。
- DB 全表定义:[Spec01_数据层.md](./Spec01_数据层.md)(`storage_backends`/`volumes`/
  `document_versions` 权威 schema)。
- 导出与备份共享 `filetime` mtime 保留姿态(导出:DB 记录值回写;备份:zip 内不单独保留 mtime,
  依赖 SHA-256 内容校验)。
- 路径安全总则(canonicalize + bounds-check、tmp→same-volume rename、组件级而非字符串前缀比较)
  交叉引用 [Spec14_不变量与约定.md](./Spec14_不变量与约定.md)。
- 前端消费:`backupStore`/`exportStore`/`fileJobStore` 见 [Spec11_前端架构.md](./Spec11_前端架构.md)。
- IPC 命令入参/出参/错误码全量权威表:[Spec10_IPC与错误契约.md](./Spec10_IPC与错误契约.md)。
