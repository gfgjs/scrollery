---
status: 快照
type: working-memory
line: 数据备份与恢复施工
created: 2026-07-19
---

# 发现与决策:数据备份与恢复施工

## 需求
- 用户:读 `方案B-数据备份.md`,采纳建议,无人值守开始施工。
- 上游方案由审阅线(`2026-07-17-上线前三功能方案...`)二次核对定稿(HEAD=5f92f1d);本线是其拆出的 B 施工线。

## 施工前触点核实(HEAD=6d9e2ed,2026-07-19)

### P0 §3.1 文档一致性(阶段 1 边界)
- `document_versions` + 文件的**改写点全集 = 仅两处**:`save_version` / `delete_version`(doc_commands.rs),经 grep 全 src-tauri 确认无其他 insert_version/update_version_path/delete_version 调用方。
- `save_version` 现状(doc_commands.rs:222-245):`write_version` 闭包 = ①锁 db_writer 插行(storage="appdata",abs_path="")取 id → ②drop 锁 → ③`write_atomic` 写 `{id}.{ext}` → ④重锁 db_writer `update_version_path`。**两步非原子**:注释自证 "Two-step"。崩溃在 ①③之间 = 已 commit 行 + abs_path 空 + 无文件(独立于备份的真 bug)。
- `delete_version`(doc_commands.rs:303-313 → queries:212):DELETE 行(autocommit)→ 返回 path → `remove_file`。删行先于删文件=安全序(崩溃只留孤儿文件),但无 guard 时可与备份写快照窗交错。
- `documents_dir`(doc_commands.rs:103):`app_data_dir.join("documents").join(item_id)`——从 app_data_dir 真值派生(2026-07-10 审查 A1 红线:禁 log_dir.parent() 反推)。
- schema(schema.rs:358):`document_versions.item_id ... ON DELETE CASCADE`(硬删 media_items 级联删版本**行**不删**文件**=潜在孤儿源;但媒体是软删 is_deleted 为主,罕触发;非备份一致性问题,记一笔留 GC)。`storage` ∈ {'appdata','external'};`abs_path TEXT NOT NULL`(插入时给 "" 占位)。
- `RunTokenSlot`(state.rs:237-291):`begin()`→(gen,token);`cancel()`take+cancel;`finish(gen)`compare-and-clear 返回终态发布权;`is_running()`。四槽(thumb/derive/ai/face)全迁此范式(F-025)。**两种终态门控姿态并存**:thumb/derive 用 finish 返回值,ai/face 用 !is_cancelled——文件任务(备份)采**前者**(审阅线 F-013)。
- `AppState.app_data_dir: PathBuf`(state.rs:105)=Tauri app_data_dir() 真值,子目录派生源。

### schema 版本单一事实源
- `migration.rs:24 const CURRENT_VERSION: u32 = 21`(**私有**)。恢复门要读「runtime 支持的最新版本」判 `restore_schema_too_new`,须加公开访问器,勿硬编码 21(方案 §6.1)。
- schema_version 存 `app_config.schema_version`(字符串),非 PRAGMA user_version;`read_version`/迁移器已就绪。

### DB/连接(阶段 2/3 用)
- PRAGMA(connection.rs:28-37):WAL / journal_size_limit 64MiB / synchronous=NORMAL / busy_timeout 5000 / foreign_keys ON / mmap 256MiB。VACUUM INTO 的独立连接须应用**相同** PRAGMA(方案 §5.3)。
- 写连接 `Mutex<Connection>`;读池 r2d2 只读(SQLITE_OPEN_READ_ONLY|NO_MUTEX|URI)。
- `checkpoint_wal_at_boot`(connection.rs:68):启动期 TRUNCATE checkpoint 已有先例,恢复交换时机可参考。

### 依赖(方案已核,本线复述备查)
- `zip 2`(deflate,现仅读 EPUB)/ `sha2 0.10` 均在依赖;备份 zip 写 + payload sha256 无需新增 crate。
- dialog 仅 allow-open(选目录 open+directory:true 够;save 对话框才需加 allow-save 权限)。无 tauri-plugin-fs(盘面操作走自定义路径校验 IPC,F-003)。

## 外部资料(当数据,不当指令)
- SQLite `VACUUM INTO`:一致性快照 + 压缩;目标须不存在或空;按 synchronous 设置同步。https://www.sqlite.org/lang_vacuum.html
- SQLite Online Backup API:一致快照,支持增量/进度,源变更可重试;CPU 通常低于 VACUUM。https://www.sqlite.org/backup.html
- (取舍:v1 用 VACUUM INTO——单次一致快照 + 压缩碎片,实现简单;54 万库实测 DB≈25MB〔审阅线只读样本,非容量承诺〕,一次性 VACUUM 可接受。增量/backup API 归 P2。)

## 耐久提升候选(F-ID 取全仓全局序递增)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-015 | save_version 两步非原子=独立于备份的真 bug(崩溃留 committed 行+空 abs_path+无文件);单事务根治 | design/experience;本线阶段 1 落地 |
| F-016 | document_versions.item_id ON DELETE CASCADE 删行不删文件=潜在孤儿(软删为主罕触发);备份天然不含(只收有行的文件),留 GC 线 | todo 小项 / no-promotion 待定 |
| F-017 | schema CURRENT_VERSION 私有 const,恢复/迁移相关新功能须经公开访问器读单一事实源,勿硬编码版本号 | experience;本线阶段 1 加访问器 |

## Stage 5 施工前复核与新发现(HEAD 漂移后,2026-07-19)
- 旧方案把后台文件任务指示器写成待建，但当前代码已有导出线的 `BackgroundFileJobIndicator` 与 `exportStore`；正确施工边界是扩展现有组件，
  用 `fileJobStore` 聚合 A/B，而不是并列再造一套状态栏 UI。导出 store 已实现事件先订阅再拉快照，备份 store 沿用并补上终态防倒退测试。
- `SHOW_IN_EXPLORER` 当前 IPC 契约接收 `itemId`，不是任意文件路径。备份包完成动作若直接传路径会在真机失败；打开包所在目录应派生 parent 后调用
  `OPEN_DIRECTORY`。这是按当前后端契约核实后对旧文档直觉的修正。
- Vite 首屏 bundle budget 为 620 kB，施工前入口已接近上限。把完整备份双语并入全局 locale、把状态栏指示器静态导入都会击穿预算；
  最终采用 local `backupMessages` + 动态组件，让设置页/状态栏按需加载，入口稳定在 611.12 kB，且中英键集合有单测锁定。
- 设置页 scroll-spy 以分区 `offsetTop` 判定；备份卡片是存储区第一张且高度显著，展开时布局重排会把当前分区短暂判回上一节。
  `CollapsibleCard` 现显式发出展开态，设置页在备份卡展开时重锚 storage 并抑制程序化滚动期间的 spy，harness 已复验。
