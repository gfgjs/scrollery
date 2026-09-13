//! DDL：CREATE TABLE / CREATE INDEX 语句 — 近期版本（V18..V31：近期小增量）。
//! 从 `schema/mod.rs` 重导出，外部路径 `crate::db::schema::SCHEMA_Vn` 不变。

/// DDL delta for schema version 18 — 收藏夹软删除（可撤销删除，S5 阶段 11）。
///
/// 动机：收藏夹删除此前是 `DELETE FROM albums`（硬删除，`album_items` 随 `ON DELETE CASCADE`
/// 一并消失），一次确认即永久丢失命名夹及其成员，与 Persons 隐藏/忽略的**可逆软标志**范式
/// 不一致——undo 能力是数据模型的属性，硬删除后无可撤销的对象。本列把删除改为「置位软删除
/// 时间戳」，删除→置 `deleted_at`、恢复→清零，成员关系原样保留，使删除可撤销：
///   - `delete_collection` 改为 `UPDATE ... SET deleted_at`（仅 `kind='user'`）；
///   - 新增 `restore_collection`（清零 `deleted_at`）承接 undo；
///   - `list_collections` / `recent_collections` 过滤 `deleted_at IS NULL`（软删项不出现）。
///
/// `deleted_at` 无 DEFAULT → 既有行与新建夹均为 NULL（未删除），系统夹永不置位。
///
/// 注：`ALTER TABLE ... ADD COLUMN` 非幂等，由 migrate_step 的版本事务保证恰好执行一次（同 V16）。
pub const SCHEMA_V18: &str = "
ALTER TABLE albums ADD COLUMN deleted_at INTEGER;
";

/// DDL delta for schema version 19 — 目录排序统一（方案 B：目录序持久化前序 DFS 排序键列）。
///
/// 动机：画廊 `push_order_by` 与 `enricher` 的 folder 目录序此前按**每条媒体行**调
/// `TREE_SORT_KEY(d.rel_path)` 标量函数求值（`d.rel_path` 是随行变化的列参数，
/// `SQLITE_DETERMINISTIC` 无法折叠 → 逐行 SQLite→Rust FFI + `Vec<u8>` 分配）。本列把该键
/// **预计算并持久化**，消费者改读 `d.tree_sort_key` 列（memcpy），省掉每行 FFI 与分配。
///
/// # 键编码与内存侧同一逻辑
/// 键 = `TREE_SORT_KEY(rel_path)` = [`crate::utils::path::encode_tree_sort_key`]
/// （每个 `/`-分隔 segment 后追加 `0x00` 终止字节的 BLOB）。SQLite BLOB memcmp = Rust
/// `Vec<u8>::cmp`，故与内存 `build_dir_rank` 的比较键**逐位同构** —— 内存/SQL folder 序
/// 刚性等价契约（`canonical_derive_order_matches_sql_order`）成立的根据。
///
/// # 迁移要点
/// - `NOT NULL DEFAULT X''`：有行表上 `NOT NULL` 的 `ADD COLUMN` 须带常量 DEFAULT；`X''`
///   （空 blob）既满足约束，又恰是根目录（`rel_path=''`）的**合法**键值（根键本就为空）。
/// - `ADD COLUMN` 带常量 DEFAULT 是 O(1) 元数据操作（不重写行）；随后 `UPDATE` 覆写非根行。
/// - 回填 `UPDATE ... WHERE rel_path <> ''`：根目录保持 `X''`，其余按标量函数回填。回填 SQL
///   依赖 `TREE_SORT_KEY` 已注册 —— `run_migrations` 顶部自注册保证任何连接（含裸测试连接）就绪。
/// - 纯 SQL 建不出 NUL 终止 BLOB（`||` 强转 TEXT、无 blob 拼接符、`char(0)` 遇 NUL 截断），
///   故回填只能靠 Rust 编码函数（此处经已注册的标量函数调用）。
///
/// 注：`ALTER TABLE ... ADD COLUMN` 非幂等，由 migrate_step 的版本事务保证恰好执行一次（同 V18）。
pub const SCHEMA_V19: &str = "
ALTER TABLE directories ADD COLUMN tree_sort_key BLOB NOT NULL DEFAULT X'';
UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path) WHERE rel_path <> '';
";

/// V20：看图台用户旋转持久化(2026-07-18 内容页需求)。`view_rotation` 存**用户在看图台施加的
/// 展示旋转**(归一化 0/90/180/270,顺时针),与 `video_meta.rotation`(拍摄内在方向元数据)正交——
/// 前者是用户偏好、可随时改回,后者是文件固有属性。默认 0(既有行=未旋转,零成本迁移)。
/// 追加在 media_items 末列,不动既有列位(同 color_label 的末列追加约定)。
///
/// 注：`ALTER TABLE ... ADD COLUMN` 非幂等,由 migrate_step 的版本事务保证恰好执行一次。
pub const SCHEMA_V20: &str = "
ALTER TABLE media_items ADD COLUMN view_rotation INTEGER NOT NULL DEFAULT 0;  -- 用户看图台展示旋转 0/90/180/270
";

/// V21:根文件夹显隐(2026-07-18 设置页需求)。`is_hidden` = 用户在设置页把某扫描根隐藏——隐藏后
/// 该根媒体从画廊「全部」/时间轴/搜索/统计/侧栏文件树/全选全部排除(库级),取消即恢复。默认 0
/// (既有根=可见,零成本迁移)。有意用**专用列**而非复用死列 `is_active`(后者语义含糊、从未被任何
/// 查询消费)——意图清晰,仿 `persons.is_hidden`。
///
/// 排除在查询侧走条件子查询(`m.directory_id NOT IN (SELECT id FROM directories WHERE root_id IN
/// (隐藏根)))`),仅当有隐藏根时才追加谓词——无隐藏时画廊 canonical 查询逐字节不变、免 JOIN 红线
/// 不触碰。不反规范化到 media_items(避免 move/copy/rescan 的多点同步泄漏)。
///
/// 注:`ALTER TABLE ... ADD COLUMN` 非幂等,由 migrate_step 的版本事务保证恰好执行一次。
pub const SCHEMA_V21: &str = "
ALTER TABLE scan_roots ADD COLUMN is_hidden INTEGER NOT NULL DEFAULT 0;  -- 用户设置页隐藏该根(库级排除)
";

/// V22:派生流水线毒任务防线(2026-07-22 故障复盘)。`orphan_count` 记录该 (item,kind) 派生行
/// 被 `reset_processing_derivations`(孤儿复位)命中的次数——流水线挂死/崩溃后遗留的
/// status=1 行,原逻辑无条件退回 status=0 重新领取,毒文件(如引发 MF 挂死的 mkv)因此每次
/// 启动都被同一批复现领取、无限循环(本次故障 125 个孤儿循环 6+ 轮)。加此计数列后,孤儿复位
/// 达到阈值(orphan_count>=2,即第 3 次孤儿)时转 status=3(error)而非再投入领取;任务正常
/// 完成(见 `batch_finish_derivations`)时归零,防偶发崩溃累计误杀良性任务。默认 0(既有行=
/// 从未孤儿过,零成本迁移)。
///
/// 注:`ALTER TABLE ... ADD COLUMN` 非幂等,由 migrate_step 的版本事务保证恰好执行一次。
pub const SCHEMA_V22: &str = "
ALTER TABLE media_derivations ADD COLUMN orphan_count INTEGER NOT NULL DEFAULT 0;
";

/// V23:播放器播放位置记忆(2026-07-22 播放器线)。`playback_position_ms` 记录用户上次退出
/// 播放时的播放进度(毫秒),重开同一视频时据此续播。与 `view_rotation` 同姿态——用户会话
/// 偏好、随时可覆写,非文件固有属性。默认 0(既有行=从头播放,零成本迁移)。追加在
/// media_items 末列,不动既有列位(同 view_rotation 的末列追加约定)。
///
/// 注:`ALTER TABLE ... ADD COLUMN` 非幂等,由 migrate_step 的版本事务保证恰好执行一次。
pub const SCHEMA_V23: &str = "
ALTER TABLE media_items ADD COLUMN playback_position_ms INTEGER NOT NULL DEFAULT 0;  -- 播放位置记忆,ms
";

/// V24:富化选批索引(2026-08-21 入库扫描性能线)。enrichment 图片段改 keyset 分页后,
/// `ORDER BY m.sort_datetime, m.id` 需要 `(media_type, sort_datetime, id)` 索引消掉每批
/// TEMP B-TREE 重排;视频/音频段改 `m.id > cursor` keyset 后,`(media_type, id)` 让
/// `media_type=? AND id>?` 可直接 seek,不再每批从 rowid 头跳过已处理前缀。
///
/// 同时删除 `idx_media_type(media_type)`:其前缀能力被新复合索引完全覆盖(同部分谓词
/// `WHERE is_deleted=0`),保留会在媒体项 INSERT/UPDATE 时多维护一棵冗余 B-tree。
///
/// 注:DDL 幂等由 `IF NOT EXISTS`/`IF EXISTS` 保证,版本事务保证恰好执行一次。
pub const SCHEMA_V24: &str = "
DROP INDEX IF EXISTS idx_media_type;
CREATE INDEX IF NOT EXISTS idx_media_type_id
    ON media_items(media_type, id) WHERE is_deleted = 0;
CREATE INDEX IF NOT EXISTS idx_media_type_sort
    ON media_items(media_type, sort_datetime DESC, id DESC) WHERE is_deleted = 0;
";

/// V25:删除冗余目录前缀索引(2026-08-21 入库扫描性能线)。`media_items` 的
/// `UNIQUE(directory_id, file_name)` 约束自带隐式唯一索引,已完整覆盖
/// `idx_media_directory(directory_id)` 的同前缀查询能力;保留它只会在每次媒体项
/// INSERT/UPDATE 时多维护一棵 B-tree(百万级导入的稳定写放大)。
///
/// 查询 `WHERE directory_id=?` 与 `(directory_id,file_name)=?` 均由隐式唯一索引服务,
/// 不改变任何 SQL 文本/参数绑定。
pub const SCHEMA_V25: &str = "
DROP INDEX IF EXISTS idx_media_directory;
";

/// V26：精确内容去重索引（P1）。`content_hash` 仍是扫描变化侦测用的抽样 change
/// fingerprint，不是内容身份；精确摘要只进入此 sidecar，避免把完整内容哈希带入常规扫描
/// 热路径。`source_revision` 是媒体源代次：源文件或 Live Photo 组合发生变化时由后续扫描逻辑
/// 递增，摘要写回必须以 `(item_id, source_revision)` 为条件，防止旧任务覆盖新源。
///
/// `file_mtime_ns` 保留纳秒级 mtime，供去重任务在打开前后复核文件是否漂移。SQLite 的
/// `ADD COLUMN ... DEFAULT 1` 使用常量默认值，既能为 V25 既有行补出代次，又对 fresh DB
/// 合法；`file_mtime_ns` 不设默认值，旧行的未知纳秒 mtime 保持 NULL。
///
/// `dedup_index` 是按媒体项一对一的 sidecar：缺行代表 pending，不预建全库空行。所有摘要
/// (`quick_digest`/`exact_digest`/`unit_digest`) 与物理身份键均为 BLOB；`hash_version` 同时
/// 覆盖摘要算法、域分隔和组合编码，升级编码后旧行自然不再混用。`unit_digest` 表示普通文件
/// 或主文件+companion 的逻辑单元摘要，不能把 Live Photo 主文件和 companion 的摘要简单拆成
/// 两个独立重复组。`physical_key` 只用于硬链接识别/空间估算，不是跨挂载的永久身份。
///
/// `status`/`error_code` 为可恢复流水线状态：状态可表达 `stale`、`unstable`、`missing`、
/// `error`（及正常完成状态），错误码承载具体稳定原因。两个部分索引只收录已有摘要，分别
/// 支持精确候选与逻辑单元分组；首版不创建永久 `duplicate_groups` 表，重复组由查询动态生成。
///
/// 注：两条 `ALTER TABLE` 由版本迁移事务各执行一次；表和索引使用 `IF NOT EXISTS`，使本
/// DDL 在迁移失败回滚后重试时不会重复创建对象，也不使用 SQLite 不允许的表达式 DEFAULT。
pub const SCHEMA_V26: &str = "
ALTER TABLE media_items ADD COLUMN file_mtime_ns INTEGER;
ALTER TABLE media_items ADD COLUMN source_revision INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS dedup_index (
    item_id          INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    source_revision  INTEGER NOT NULL,
    hash_version     INTEGER NOT NULL,
    quick_digest     BLOB,
    exact_digest     BLOB,
    unit_digest      BLOB,
    unit_size        INTEGER,
    physical_key     BLOB,
    status           TEXT NOT NULL,
    error_code       TEXT,
    checked_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dedup_exact_candidate
    ON dedup_index(exact_digest, item_id)
    WHERE exact_digest IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_dedup_unit_group
    ON dedup_index(unit_digest, unit_size, item_id)
    WHERE unit_digest IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_dedup_quick_candidate
    ON dedup_index(quick_digest, item_id)
    WHERE quick_digest IS NOT NULL;
";

/// V27-V29 为开发期退役功能保留空槽，避免倒退 schema 版本波及备份/恢复判定。
pub const SCHEMA_V27: &str = "";

/// V28：保留空槽。quick digest 候选索引已并入当前 V26 定义。
pub const SCHEMA_V28: &str = "";

/// V29：保留空槽。
pub const SCHEMA_V29: &str = "";

/// V30：把 V10 时代仅由路径推导、未经过原生卷类型确认的卷降为 `unknown`。
pub const SCHEMA_V30: &str = "
UPDATE volumes
   SET kind = 'unknown'
 WHERE stable_id LIKE 'pending:%' OR stable_id LIKE 'path:%';
";

/// V31：补建 quick digest 候选索引。
///
/// quick 索引在当前 V26 定义中创建，但已经处于旧 V26/V30 的数据库不会重放 V26；
/// 这个幂等迁移修复旧库的访问路径，不改变任何数据或去重语义。
pub const SCHEMA_V31: &str = "
CREATE INDEX IF NOT EXISTS idx_dedup_quick_candidate
    ON dedup_index(quick_digest, item_id)
    WHERE quick_digest IS NOT NULL;
";

/// V32：去重分析工作表。分析只读写此表，成功完成后才在单个事务中替换已发布的
/// `dedup_index`，因此主画廊不会看到半轮结果，停止/失败也自然保留上一版。
pub const SCHEMA_V32: &str = "
CREATE TABLE IF NOT EXISTS dedup_index_working (
    item_id          INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    source_revision  INTEGER NOT NULL,
    hash_version     INTEGER NOT NULL,
    quick_digest     BLOB,
    exact_digest     BLOB,
    unit_digest      BLOB,
    unit_size        INTEGER,
    physical_key     BLOB,
    status           TEXT NOT NULL,
    error_code       TEXT,
    checked_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dedup_working_quick
    ON dedup_index_working(quick_digest, item_id)
    WHERE quick_digest IS NOT NULL;
";

/// V33：目录移动阶段日志。
///
/// 目录移动的物理动作（rename 或跨卷「拷贝→发布→删源」）先于数据库事务完成，两者之间任何一步
/// 失败或进程退出都会留下「磁盘已变、库未变」的半完成状态。此前这种状态只经 ? 变成一条通用
/// 错误：用户看不到文件真实位置，也没有可重试的落点（审查 §7.1-B）。
///
/// 本表只记**未完成**的目录移动，是「搬到哪一步」的持久账，成功收尾即删行：
///   - stage=intent           ：已登记意图，物理搬运尚未确认完成（可能留下我们自己的暂存目录）
///   - stage=published        ：目标目录已完整落盘，数据库尚未改写（可幂等重放 DB 段）
///   - stage=source_leftover  ：数据库已改写，源目录树残留（删源中断，重试即续删）
///
/// target_parent_id 与 payload_digest / payload_files 是恢复期的判定依据：
///   - target_parent_id：目标父目录行 id（落在扫描根下时是 rel_path='' 的根目录行）。恢复时
///     父目录必须能解析到真实目录行，绝不写 NULL（NULL 会把普通目录变成树里的伪根）。
///   - payload_digest / payload_files：**跨卷复制时对实际写出树算出的内容凭据**，在发布 rename
///     **之前**持久化——这样「发布后、删源前」崩溃时日志已能证明目标属于本次移动，恢复不会把
///     双端存在的状态误判成外部冲突。同卷 rename 路径没有「我们写出的树」，故为 NULL，其发布由
///     「源已不在」这一原子事实自证。
///
/// staging_abs_path 是**我们自己**在目标卷上创建的独占暂存目录（跨卷拷贝的落脚点）。清理只认
/// 这一列记录的绝对路径 + 它的**同级**标记文件（`<staging>.scrollery-staging`，由
/// `dir_move::staging_marker_path` 拼出），绝不按名字模式去猜——目标卷上同名的既有目录是
/// 用户数据，不是我们的临时产物。
///
/// 无外键：日志必须比被引用的行活得更久（源根被删、目录行被级联清理时仍要能报告真实状态并收尾）。
pub const SCHEMA_V33: &str = "
CREATE TABLE IF NOT EXISTS directory_move_journal (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    stage            TEXT    NOT NULL,          -- 'intent' | 'published' | 'source_leftover'
    source_dir_id    INTEGER NOT NULL,
    source_root_id   INTEGER NOT NULL,
    source_rel_path  TEXT    NOT NULL,
    source_abs_path  TEXT    NOT NULL,
    target_root_id   INTEGER NOT NULL,
    target_rel_path  TEXT    NOT NULL,
    target_abs_path  TEXT    NOT NULL,
    target_parent_id INTEGER NOT NULL,
    staging_abs_path TEXT,
    payload_digest   TEXT,
    payload_files    INTEGER NOT NULL DEFAULT 0,
    affected_dirs    INTEGER NOT NULL DEFAULT 0,
    affected_media   INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at       INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
";
