---
id: 2026-07-17-方案B-数据备份
status: active
type: design
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-17
last-verified: 2026-07-19
---

# 方案 B:数据备份与恢复

> 2026-07-18 复审结论:**方向正确，但原稿不可直接施工**。本稿已按 `247d05c..HEAD` 的代码漂移
> 补上跨文件一致性、`document_versions` 跨机路径重写、V21 schema、可恢复进度与崩溃恢复状态机。
> 2026-07-19 二次核对:对 HEAD(`5f92f1d`)复核 §3 两项 P0 前置与全部依赖断言，结论与工程量不变。
> 本线只更新方案，不施工。

## 1. 目标与威胁模型

- 保护对象 = Scrollery 数据库中的全部目录身份、整理态、设置与高成本索引，以及 `appdata/documents/**` 文档版本文件。
- 媒体原文件不入包；备份不替代用户对照片/视频/音频/文档原件的磁盘备份。
- 威胁：appdata 误删、系统盘损坏、数据库升级故障、换机迁移、用户误恢复。
- 备份 ≠ 导出（方案 A）≠ 多端同步。全量进 free，数据安全不设付费墙。
- v0.1 交付面按当前产品裁定为 Windows-first；包格式、路径契约与测试保持 macOS 可实现，不把 Windows 绝对路径写进可移植 manifest。

## 2. 内容与边界

### 2.1 v1 完整备份

| 包内内容 | 契约 |
|---|---|
| `db/scrollery.db` | 一致的 SQLite 快照，包含运行时 schema_version 与全部表 |
| `documents/**` | `document_versions.storage='appdata'` 引用的版本文件，强制包含 |
| `manifest.json` | 格式版本、应用/schema 版本、根摘要、payload 大小与 SHA-256 |

### 2.2 明确排除

- `cache/`、`models/`、`exotic/plugins`、`logs/` 不入包；它们可再生、可下载或不属于用户状态。
- keyring 凭据不入包；`storage_backends.cred_ref` 只是引用。恢复后仅在凭据实际不可用时要求重输，不宣称跨机迁移凭据。
- `document_versions.storage='external'` 指向的用户自有文件不入包，manifest 记录数量并在恢复摘要中提示。

### 2.3 原稿口径修正

- `VACUUM INTO` 复制的是**整库**。因此 v1 虽排除了文件系统 `cache/`，仍会包含 DB 内可再生/高成本数据，例如 `ai_embeddings`、媒体 meta、face coverage 等。
- 这不是“最小用户态薄备份”，而是“完整目录快照”。v1 取完整快照以降低漏表和恢复语义风险；按表裁剪的 lean backup 进入 P2，施工前不得继续沿用“可再生表不会进包”的错误表述。
- `faces` 同时承载检测结果与用户确认/人物关联，不能在没有独立契约与迁移测试时粗暴裁掉。

## 3. 两个 P0 前置修复

### 3.1 文档版本跨文件一致性门

当前 `save_version` 是“插 DB 行 → 写文件 → 回填绝对路径”，`delete_version` 是“删行 → 删文件”；备份若在中间穿入，会得到缺文件或空路径的假完整包。

施工 B 前先增加 `AppState.document_storage_guard: std::sync::RwLock<()>`：

- 所有同时改 `document_versions` 与 `appdata/documents/**` 的 blocking 闭包持 read guard，覆盖整个 DB+文件操作；锁不跨 `.await`。
- 备份在同一 blocking 线程持 write guard，从 DB 快照开始直至 documents 打包完毕，保证二者来自同一逻辑时点。
- `save_version` 改为单事务内插行、写原子文件、回填路径、commit；失败回滚 DB，最多留下可 GC 的孤儿文件，不允许 DB 引用缺失文件。
- 备份前逐行校验 `storage='appdata'`：路径非空、文件存在、位于受控 `documents/` 下、已存 content_hash 时必须匹配。任一失败返回 `backup_document_inconsistent`，不产出“成功”包。

### 3.2 appdata 绝对路径重写

当前 `document_versions.abs_path` 保存本机 appdata 绝对路径。把 DB 原样搬到另一台机器后，版本树仍指向旧机器路径。

- 包内 documents 一律使用相对路径 `documents/{item_id}/{file_name}`。
- `restore_stage` 在**暂存 DB** 内遍历 `storage='appdata'` 行，以绑定参数把 `abs_path` 重写为目标机器当前 `app_data_dir/documents/...`；不得字符串替换旧前缀。
- 重写时用 `item_id + file_name` 与包内白名单交叉验证，拒绝 `..`、绝对路径、盘符、符号链接逃逸和缺件。
- 自动化必须用“源/目标 appdata 路径不同”的 fixture；同路径 roundtrip 不能证明跨机恢复成立。

## 4. 包格式

文件名：

- 手动：`Scrollery-backup-{YYYYMMDD-HHmmss}.scrollerybackup`
- 自动：`Scrollery-auto-{YYYYMMDD-HHmmss}.scrollerybackup`

容器为 zip（现有 `zip = "2"` + deflate 可写，无需新增压缩库）：

```text
db/scrollery.db
documents/<item_id>/<version-file>
manifest.json
```

manifest 示例（数值在运行时读取，禁止把 V21 写死进实现）：

```json
{
  "formatVersion": 1,
  "backupId": "018f...",
  "kind": "manual",
  "appVersion": "0.1.0",
  "schemaVersion": 21,
  "createdAtUtc": "2026-07-18T12:00:00Z",
  "roots": [{ "id": 1, "alias": "照片库", "volumeStableId": "...", "hidden": false }],
  "counts": { "items": 540000, "albums": 12, "tags": 80, "namedPersons": 15 },
  "externalDocumentVersions": 3,
  "payload": [{
    "path": "db/scrollery.db",
    "bytes": 25411584,
    "sha256": "..."
  }]
}
```

- manifest 不写凭据；扫描根路径属于恢复必要信息但含隐私，UI 必须提示备份包含个人路径、人名、标签等信息。
- zip CRC 不能替代 payload SHA-256；`sha2` 已是直接依赖。

## 5. 备份流程

1. `preflight_backup`：校验目的目录、文档一致性、估算 DB + documents 大小、目标可写性；比较 appdata 与目的地卷，若同卷提示“不能防物理盘损坏”。
2. 获取 A/B 共用文件 job gate，再获取 `document_storage_guard` write guard；已占用返回 `file_job_busy`，不取消旧任务。
3. 用独立、应用相同 PRAGMA 的 rusqlite 连接执行参数绑定的 `VACUUM INTO ?1` 到 job staging。当前 `synchronous=NORMAL` 下，SQLite 会在命令成功返回前同步输出；仍要确保文件已关闭再入 zip。
4. 对 DB 快照执行 `PRAGMA quick_check`，收集 schema_version 与 counts；流式加入 zip。
5. 在 guard 仍持有时遍历 documents，只收 DB 中受控 appdata 行；逐项计算 SHA-256 并写 payload 清单。
6. manifest 最后写入；zip 文件写在目标目录同级 `*.tmp`，flush/close 后 same-volume rename 为正式包。
7. 只有新包完整成功后才执行 retention；仅删除名称、manifest.kind、格式与目标目录都匹配的**自动备份**，永不轮转手动备份或不可解析文件。
8. 释放 guard 与 file job gate，发布完成状态。

### 5.1 进度与取消

- 不以 Channel 作为唯一进度源；沿 2026-07-18 缩略图修复后的模式，使用 app 级事件 + `AppState` 快照 + `backup_status` 查询。
- 使用 `RunTokenSlot` generation，完成回调 compare-and-clear；旧轮不得覆盖新轮终态。终态门控姿态同方案 A §3.1:采 thumb/derive 的「finish 返回值门控终态发布」，勿仿 ai/face 的 `!is_cancelled` 姿态(F-025 后两姿态并存且语义不同)。
- 取消只清当前 job staging，已正式落名的旧备份不碰。
- 自动备份在扫描、全量缩略图、派生、A 导出或活跃交互期间不启动；运行后不强抢用户交互，必要时在文件边界让步。

## 6. 恢复流程

恢复采用 **validate → stage → arm → restart → swap → verify/rollback**，不在活进程内拆写连接与 r2d2 池。

### 6.1 暂存与校验

1. `restore_stage(file)` 把包当不可信输入；先读 central directory，条目白名单只允许 `db/scrollery.db`、`documents/**`、`manifest.json`。
2. 拒绝绝对路径、`..`、盘符、重复条目、符号链接；以 checked arithmetic 汇总条目数和解压大小，并按可用空间留安全余量，不能只信 manifest 声明防 zip bomb。
3. 校验全部 payload 大小 + SHA-256，再解到 appdata 同卷的 `restore-staging/{backupId}/`。
4. 暂存 DB 依次校验 `quick_check`、`foreign_key_check`、`formatVersion`、`schemaVersion <= runtime CURRENT_VERSION`。runtime 版本从代码单一事实源读取，不硬编码 21。
5. 老 schema 在暂存副本上运行现有迁移器后再次 quick/foreign-key check；新于当前二进制则返回 `restore_schema_too_new`。
6. 按 §3.2 重写 appdata 文档路径并复核每行有对应文件；返回摘要供双确认。

### 6.2 回滚包与启动交换

1. 用户双确认后，先用同一备份核心在 `appdata/restore-rollback/` 生成当前状态的 pre-restore 完整包；成功前绝不 arm。
2. 原子写 `pending-restore.json`，记录 backupId、staging、rollback 包与 phase。phase 至少有 `prepared`、`current_moved`、`installed`、`verified`。
3. relaunch 后在 `lib.rs` 创建 DB writer/read pool **之前**执行交换。当前 `scrollery.db`、`-wal`、`-shm` 与 `documents/` 先移到本 job 的 old 目录，再把 staging 同卷 rename 到位；每一步完成后原子更新 marker。
4. 每个 phase 都必须可幂等重入：崩溃后根据 marker 与实际文件存在性继续或逆向恢复，不能靠“固定顺序大概会成功”。
5. 新库打开、迁移、quick check 且应用到 Ready 后写 `verified`；失败则用 old 目录或 pre-restore 包回滚。rollback 包至少保留 7 天并由 UI 提供手动清理。
6. 成功后重探 volumes；失联 root 走现有 `relink_scan_root`（100 样本、size+mtime、95% 门）。V21 `scan_roots.is_hidden` 与 V20 `view_rotation` 因整库快照自动保留。

## 7. 自动备份策略

- 未设置 `backup_dir` 时自动备份为关闭；首次选择目录时，在同一确认面明确建议“每日自动备份 + 保留 5 份”，由用户显式接受，不伪装成已默认保护。
- 触发：启动完成后 idle 约 2 分钟，距上次成功 ≥24h，目的地可写且无更高优先级/文件 job。
- 不在退出时启动备份；失败不阻断退出。
- 自动失败在设置页与右侧 `BackgroundFileJobIndicator` 留可见状态；不每次弹窗打断。连续失败需要可操作的“重选目录/立即重试”。
- retention 只作用自动包；默认 5 是初始值，不是不可变协议。

## 8. 前端设计

- 设置页 `storage` 已有 `RootFolderVisibilitySection`、`NetworkStorageSection`、`KnownVolumesSection`；新增 `BackupSection`，并同步设置页搜索语料。
- 卡片：目的地、同卷警告、自动开关、保留数、上次成功/失败、立即备份、备份列表、从文件恢复。
- 恢复向导：选包 → 校验进度 → 摘要 → 双确认 → staged → 重启；新 schema、损坏包、缺文档、空间不足分别按稳定码呈现。
- A/B 共用 `BackgroundFileJobIndicator` 与 file-job store；恢复阶段另在向导内显示，不做通用任务中心。
- zh-CN/en-US 完整。

## 9. 安全与错误契约

- 新增 `AppError::Backup { code, message }` 与 `AppError::Restore { code, message }`，稳定小写 code 原样序列化，message 不泄漏绝对路径/SQL/内部错误串。
- 备份至少：`backup_dir_unset`、`backup_dir_not_writable`、`backup_document_inconsistent`、`backup_cancelled`、`backup_io`、`file_job_busy`。
- 恢复至少：`restore_format_unsupported`、`restore_schema_too_new`、`restore_corrupt`、`restore_path_invalid`、`restore_size_limit`、`restore_document_missing`、`restore_rollback_failed`。
- 备份包未加密，UI 明示其中含个人数据；v1 不上传、不外发。AES 进入 P2，且不得自造与 Part6 既有 AES 线并行的第二套密码学实现。

## 10. 测试与验收

### 自动化

- DB 快照：并发 DB 写下仍一致；schema_version 动态读取；`quick_check` 通过。
- 文档屏障：`save_version`/`delete_version` 与备份交错，包内 DB 行与文件始终成对；故障注入不产出正式包。
- 跨机：源、目标 appdata 路径不同，恢复后每个 appdata version 可读；external version 不被错误改写。
- 包完整性：单字节损坏、缺件、重复条目、zip-slip、超大声明、SHA 不符、旧/新 schema。
- restore crash matrix：在每个 marker phase 注入中断，下一次启动可继续或回滚，最终只存在一套自洽 DB+documents。
- pre-restore：恢复前自动包可 roundtrip；当前 WAL 有已提交页时也不丢数据。
- retention：只删已验证 auto 包，手动包/陌生文件不删。
- 任务态：WebView 重载恢复、generation 迟到收尾、A/B 互斥、取消只清本 job staging。

### 手动 GUI（不自动化）

- 真机重启交换、强杀各阶段、恢复失败后的回滚提示、失联 roots relink、网络盘/可移动盘拔出、磁盘满。

## 11. 工作量（复审估算，未实测）

- P0 文档一致性与路径重写 1.5～2 天；备份核心/自动策略 2～2.5 天；恢复状态机 2～3 天；前端 1.5～2 天；测试 1.5～2 天。
- 合计约 **8.5～11.5 天**。原稿 4～5 天漏算跨文件一致性、跨机路径、崩溃恢复矩阵与进度恢复，已作废。

## 12. 待裁决

| # | 问题 | 复审建议 |
|---|---|---|
| B-1 | 自动备份默认开 | 未选目录时关；选目录时显式建议每日+保 5 |
| B-2 | documents 是否可关闭 | 不可关闭；缺它不是完整备份，体积问题用预检与 P2 lean 模式解决 |
| B-3 | staged-restart 是否接受 | 接受；活进程换库风险不值 |
| B-4 | v1 加密 | 否；明示隐私，沿既有 AES 线做 P2 |
| B-5 | v1 完整库还是按表裁剪 | 完整库；lean backup 需独立表契约与迁移测试 |
| B-6 | 自动包是否轮转手动包 | 否；只轮转 `kind=auto` 且 manifest 验证通过的包 |

## 13. P2 池

- lean backup（去 DB 可再生状态）、增量备份、AES 加密、云端/移动备份、存储预算轮转、备份健康定期演练。
