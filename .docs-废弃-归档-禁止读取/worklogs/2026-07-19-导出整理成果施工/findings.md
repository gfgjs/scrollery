---
status: 快照
type: 工作记忆
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-19
---

# 调研发现:方案 A 施工

## 候选提升表(收口时逐条处置)

| 候选 ID | 发现 | 建议去向 |
|---|---|---|
| F-001 | B 线的 backup/restore IPC 命令**完全没有前端接入**(`src/constants/ipc.ts` 无条目,`src` 下无 backup 相关 `.vue`/`.ts`)。B 的"⏸GUI 真机"实指后端已通、前端未建,非"待真机点验"。 | 登记 experience 或 B 线 memory 更正,防止后续误判 B 前端已就绪 |
| F-002 | `scan_roots` 表已有 `alias TEXT` 列(schema.rs:45),可直接作 manifest `rootAlias` 源,方案未提及此列已存在 | no-promotion(实现细节,代码即文档) |
| F-003 | `db/queries/layout.rs` 的 `SELECTION_BATCH_CHUNK = 5_000`(layout.rs:758)是选区解析用的分块常量,与方案 §2.1 建议的元数据查询分块(500~1000)是两个不同用途的常量,不应共用同一值 | no-promotion |
| F-004 | `.spec.ts` 缺失是本仓的默认态而非例外:`FolderCreateDialog.vue`/`FolderTreeSelectorDialog.vue`/`derivationStore.ts` 均无专属单测,但 `scanStore.ts` 的缩略图生成进度恢复(app 事件+快照姿态)有专属 `describe` 块。判定标准=「是否存在与当前改动同构的既有测试先例」,存在则跟随、不存在则按风险自行判断,不是「有 spec 文件就该抄同名 spec」 | experience.md 候选:测试覆盖决策的判据说明 |
| F-005 | `resolve_media_path(root_path, rel_path, file_name)` 拼接用 `/` 分隔且不做跨平台反斜杠归一化;`db/queries/export.rs` 测试中 `rel_path=""` 时需与 `resolve_media_path` 内部空 rel_path 分支行为对齐(已验证一致,见 `run_export_conflict_skip_records_and_keeps_first` 测试用例的双源路径场景) | no-promotion(实现细节) |

## 代码考古证据

### 文件任务门闩(B 已落地,A 直接复用)

`src-tauri/src/state.rs`:
- `pub const FILE_JOB_BACKUP/FILE_JOB_RESTORE/FILE_JOB_EXPORT: &str`(已预留 export 常量)
- `file_job_owner: Mutex<Option<&'static str>>` + `try_acquire_file_job(owner) -> bool` + `release_file_job(owner)`(严格 claim,不可重入,不抢占)
- `backup_token: RunTokenSlot` + `backup_progress: Mutex<Option<BackupProgressPayload>>` + `backup_cancelling: AtomicBool` 三件套,`cancel_backup`/`is_backup_cancelling`/`clear_backup_cancelling` —— A 照此镜像出 export 三件套

### RunTokenSlot API(全仓统一范式,F-025 已收编 thumb/derive/ai/face 四槽)

`begin() -> (generation, CancellationToken)` / `cancel()` / `finish(generation) -> bool`(compare-and-clear,终态发布权)/ `is_running()` / `is_current(generation)`(只读判定,终态发布顺序:先落快照后清 token)。

### backup_commands.rs 的任务生命周期姿态(`src-tauri/src/ipc/backup_commands.rs:250-326` `begin_backup`)

`try_acquire_file_job` → `token.begin()` → 发布 running 快照 → `tauri::async_runtime::spawn` 分离任务(webview 关闭也跑到底)→ `spawn_blocking` 执行引擎 → 终态顺序:**先落终态快照(`is_current` 判发布权)→ `finish(generation)` → 清 cancelling 标志 → `release_file_job` 最后释放**。A 的 `start_export` 照此顺序,唯一差异:导出是分块任务(非单次操作),需要中途多次 `publish_export_progress` 更新处理计数。

### 路径解析与批量查询精度(Explore agent 核实,2026-07-19)

- `utils::path::resolve_media_path(root_path: &str, rel_path: &str, file_name: &str) -> String`:唯一源路径拼装函数,`root_path` 取自 `scan_roots.path` 原始值。
- 无现成"多 id 一次性拿 root+rel_path+tags+albums"的批量函数,需新写。单项先例:`media.rs` 的 `get_media_detail`(`SELECT d.rel_path, r.path FROM directories d JOIN scan_roots r ON d.root_id=r.id WHERE d.id=?1`)。
- tags 表:`tags`(schema.rs:202)+ `item_tags`(schema.rs:210);albums 表:`album_items`(schema.rs:192)。均无批量按多 id 取名称的现成查询,需新写 JOIN + GROUP BY id 或后处理分组。
- 分块 IN 查询先例:`media.rs:242`(`batch_update_set_value`)用 `ids.chunks(SELECTION_BATCH_CHUNK)` + 占位符拼接,可作分块写法参照(注意其常量 5000 不用于本次,本次自定义 500~1000)。

### 文件名合规化 / dest_writable 先例

`backup/core.rs` 有私有 `ensure_dest_writable`(随机前缀临时文件写入探测,`dest_writable` 是其 bool 包装)。无 Windows 保留名/非法字符处理先例,`export/core.rs` 需新写。