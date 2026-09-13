//! DDL：CREATE TABLE / CREATE INDEX 语句 — 中期版本（V10..V17：卷可用性 + 阅读器）。
//! 从 `schema/mod.rs` 重导出，外部路径 `crate::db::schema::SCHEMA_Vn` 不变。

/// 模式版本 10 的所有 DDL —— 卷可用性模型:移动盘/网络盘插拔感知，「离线 ≠ 删除」的数据基础（Part0 §6 / Part1 §3.2）。
///
/// **纯加表 + 加列 + 加索引，零破坏**（既有行经 DEFAULT 自动在线/向后兼容）：
///   - `volumes`            ：卷登记表（稳定身份锚点 = Win 卷GUID / mac 卷UUID / 网络 UNC）
///   - `scan_roots` 扩列    ：`volume_id` / `volume_subpath`
///   - `media_items` 扩列：`volume_id`（冗余免JOIN）/ `volume_relative_path` / `availability` 三态、
///     `color_label`（Part5 T16 硬前置）、`content_identifier`（Live Photo/HEIC，Part2 硬前置）
///   - `persons.model_name` ：人脸模型轨隔离（Part4 T6 硬前置；旧 persons 经 DEFAULT 归 default 轨）
///   - `face_rejections`    ：人脸「不是这个人」负样本（Part4 §3.5.1 / §8.4 硬前置）
///
/// 关键设计：
///   - `availability` 与 `is_deleted` **正交**：前者扫描/卷驱动（online/offline/missing，可自动复原），
///     后者用户驱动（回收站，仅用户可逆）——扫描路径永不触碰 is_deleted（Part2 §3.2.4）。
///   - 多个后续 Part 的零散加列在 terminal review 时**合并进 V10**，防「跨 Part 落空」+ 免去仅为
///     一列而起 V11/V12（迁移单向，V10 落库后无法回补）。
///   - 回填 DML 与 DDL **同事务**（migrate_step 的 unchecked_transaction）：失败整块回滚、版本不前进。
pub const SCHEMA_V10: &str = "
-- ── SCHEMA_V10：卷可用性（移动盘/网络盘插拔感知，Part0 §6）──────────────

-- 卷登记表：稳定身份锚点（Win 卷GUID / mac 卷UUID / 网络 UNC）
CREATE TABLE IF NOT EXISTS volumes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    stable_id       TEXT    NOT NULL UNIQUE,            -- Win '{GUID}' / mac UUID / 规范化UNC '//host/share'
    label           TEXT,                               -- 卷标(展示用，可重命名)
    kind            TEXT    NOT NULL DEFAULT 'local',    -- 'local'|'removable'|'network'
    last_mount_path TEXT,                               -- 最近挂载点/盘符(提示+运行期路径重组，非身份键)
    last_seen       INTEGER,                            -- 最近在线 unix 秒
    is_online       INTEGER NOT NULL DEFAULT 0,         -- 运行期状态(启动 probe_volumes 刷新)
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- scan_roots：关联卷 + 卷内子路径(path 列语义降级为'最后已知绝对路径')
-- ALTER ADD COLUMN 带 REFERENCES 时列必须可空(SQLite 约束)；NULL=本地固定路径(向后兼容)。
-- ON DELETE SET NULL 与 media_items.volume_id 对称、与'删卷后根 volume_id 置空'语义一致。
ALTER TABLE scan_roots ADD COLUMN volume_id      INTEGER REFERENCES volumes(id) ON DELETE SET NULL;
ALTER TABLE scan_roots ADD COLUMN volume_subpath TEXT;  -- 卷内子路径，与 volumes.last_mount_path 拼合得绝对路径

-- media_items：冗余 volume_id(免三表JOIN批量切换整盘) + 卷内相对路径 + 可用性三态
ALTER TABLE media_items ADD COLUMN volume_id            INTEGER REFERENCES volumes(id) ON DELETE SET NULL;
ALTER TABLE media_items ADD COLUMN volume_relative_path TEXT;                     -- 卷根起完整相对路径(正斜杠)，重挂载重链接键
ALTER TABLE media_items ADD COLUMN availability         TEXT NOT NULL DEFAULT 'online';  -- 'online'|'offline'|'missing'

-- persons：人脸模型轨隔离(切轨维度,与 faces.model_name 对称；Part4 T6 硬前置)
-- 旧 persons 经 DEFAULT 自动归 'yunet-sface' 默认轨(回填随本事务)。
ALTER TABLE persons ADD COLUMN model_name TEXT NOT NULL DEFAULT 'yunet-sface';

-- media_items 颜色标签(Part5 星级颜色标签 T16 硬依赖)
-- ⚠️ 色数(7)与值域未经产品/用户调研,属临时产品决策——Part5 T16 接前端时可改。
--    暂不加 CHECK 约束(SQLite 给已有列加 CHECK 需重建表,代价高且当前无脏数据来源)。
ALTER TABLE media_items ADD COLUMN color_label INTEGER NOT NULL DEFAULT 0;  -- 0=无 / 1-7 色档

-- media_items HEIC ContentIdentifier(Part2 §3.5.2 Live Photo 匹配/HEIC 元数据硬依赖)
-- 可空，enricher 读 HEIC EXIF/QuickTime 元数据回填；NULL=非 Live Photo/未读取。
ALTER TABLE media_items ADD COLUMN content_identifier TEXT;
CREATE INDEX IF NOT EXISTS idx_media_content_id ON media_items(content_identifier) WHERE content_identifier IS NOT NULL;

-- face_rejections：人脸'不是这个人'负样本(Part4 §3.5.1 reject_face_candidate；recluster 跳过已拒绝对，防质心相近反复误聚)
CREATE TABLE IF NOT EXISTS face_rejections (
    face_id    INTEGER NOT NULL REFERENCES faces(id)   ON DELETE CASCADE,
    person_id  INTEGER NOT NULL REFERENCES persons(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (face_id, person_id)
);

-- 部分索引：仅离线/异常项与按卷过滤建索引(99% 在线项不占索引)
CREATE INDEX IF NOT EXISTS idx_media_avail  ON media_items(availability) WHERE availability != 'online';
CREATE INDEX IF NOT EXISTS idx_media_volume ON media_items(volume_id)    WHERE volume_id IS NOT NULL;

-- ── 卷回填(DML 与 DDL 同事务)──────────────────────────────────────────────────
-- 为每个现有 scan_root 生成一条 volumes(临时 stable_id='pending:<id>' 占位，probe 后覆写真实 GUID/UUID)。
INSERT OR IGNORE INTO volumes (stable_id, label, kind, last_mount_path, is_online)
SELECT 'pending:' || sr.id, sr.alias, 'local', sr.path, 0 FROM scan_roots sr;
-- scan_roots.volume_id 回填(按 path 关联刚建的 volumes)；volume_subpath 留空。
UPDATE scan_roots SET volume_id = (
    SELECT v.id FROM volumes v WHERE v.last_mount_path = scan_roots.path
), volume_subpath = '' WHERE volume_id IS NULL;
-- media_items.volume_id 回填(经 directory→scan_root→volume 链)。
-- volume_relative_path 留空(百万行 UPDATE 较重)，由 Part2 扫描时填。
UPDATE media_items SET volume_id = (
    SELECT sr.volume_id FROM directories d JOIN scan_roots sr ON sr.id = d.root_id
    WHERE d.id = media_items.directory_id
) WHERE volume_id IS NULL;
";

/// 模式版本 11 的所有 DDL — keyset 分页支撑（Part1 §3.5 / T10）:复合排序索引 + 回收站 keyset seek 索引。
///
/// **纯索引重建，零数据变更**：
///   - `idx_media_sort` 单列 `(sort_datetime DESC)` → 复合 `(sort_datetime DESC, id DESC)`：
///     给 `query_layout_items` 的 `ORDER BY sort_datetime` 一个**确定性 tiebreaker**（同秒时间戳
///     稳定序、消除布局抖动），并让默认画廊排序**吃满索引**。⚠️ 索引≠tiebreaker：`query_layout_items`
///     的 `ORDER BY` 须**同时**追加 `, m.id {dir}` 次键（已在 queries.rs 统一追加），缺一不可。
///   - 新增 `idx_media_trash (deleted_at DESC, id DESC) WHERE is_deleted=1`：支撑回收站 keyset seek
///     翻页（`get_trash_keyset`，取代 OFFSET，百万行恒定 <5ms）。
pub const SCHEMA_V11: &str = "
-- idx_media_sort 单列 → 复合键。DROP+CREATE：旧索引无次键、ALTER 不能改索引列。
DROP INDEX IF EXISTS idx_media_sort;
CREATE INDEX IF NOT EXISTS idx_media_sort ON media_items(sort_datetime DESC, id DESC)
    WHERE is_deleted = 0 AND companion_of IS NULL;

-- 回收站 keyset seek 复合索引（行值比较 (deleted_at,id)<(?,?) 走此索引）。
CREATE INDEX IF NOT EXISTS idx_media_trash ON media_items(deleted_at DESC, id DESC)
    WHERE is_deleted = 1;
";

/// v12(Part6-T13 多渠道预留,§8.4):安装真相加安装来源渠道列。
/// 既有行回填 DEFAULT 'direct'——v12 前所有安装均来自直销 Registry,回填语义为真;
/// 值域见 exotic::installer::InstallSource(direct / steam_depot / store_bundled)。
pub const SCHEMA_V12: &str = "
ALTER TABLE exotic_plugins ADD COLUMN entitlement_source TEXT NOT NULL DEFAULT 'direct';
";

/// v13(阅读器完善方案 R1,§6.1):txt 章节索引缓存表。
/// 首开检测编码 + 分章的结果按 (item_id) 缓存;源指纹(`src_key`)变即失效重建
/// (布局缓存 bump 纪律同族)。字节偏移留 Rust,前端只见章序号 + 标题 + 字符数。
///
/// `src_key` 语义:源文件 = `"src:<mtime>:<size>[:<override>]"`;生效版本 = `"ver:<id>[:<override>]"`。
///   —— 换当前版本 / 源文件被改 / 手动切编码,指纹都变 → 缓存自动失效重建。
///   分段/重排是 get_text_chapter 的显示层变换、**不改章界**,故 reflow 开关**不进** src_key
///   (方案 DDL 注示「+reflow 开关位」经此工程判断收敛:章索引与 reflow 无关,避免无谓重建)。
/// `confidence` 值域:'bom' | 'detected' | 'manual' | 'lossy'(替换率超阈)。
pub const SCHEMA_V13: &str = "
CREATE TABLE IF NOT EXISTS text_book_index (
    item_id    INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    src_key    TEXT NOT NULL,
    encoding   TEXT NOT NULL,
    confidence TEXT NOT NULL,
    chapters   TEXT NOT NULL,               -- JSON [{t:标题, s:byte_start, e:byte_end, n:char_len}, ...]
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
";

/// v14(阅读器完善方案 R1/R3,§6.1):每书阅读偏好表。只存与全局默认的 diff(版本化 JSON),读时 merge。
/// R1 阶段先用于承载**手动编码覆盖**(§5.1 P0「手动切换编码」的持久化落点),完成编码功能闭环;
/// R3 起扩充承载每书竖排/主题/字号等(prefs JSON 加字段即可,无需改表)。
/// prefs 示例:`{"v":1,"encoding":"gb18030","vertical":true,...}`。
pub const SCHEMA_V14: &str = "
CREATE TABLE IF NOT EXISTS reader_book_prefs (
    item_id    INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    prefs      TEXT NOT NULL,               -- 版本化 JSON,仅存与全局默认的 diff
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
";

/// v15(阅读器完善方案 R4,§6.2):书签表。一书多书签;`locator` 存位置串(现 foliate CFI
/// "cfi:<epubcfi>",与 reading_progress 同源;loc1 落地后可存 "loc1:<json>",列不变)。
/// `UNIQUE(item_id, locator)` 令同位置书签幂等(重复添加即刷新标签/进度/时间,不产生重复行);
/// 索引 `(item_id, fraction)` 支持按全书进度列出某书书签。
pub const SCHEMA_V15: &str = "
CREATE TABLE IF NOT EXISTS reader_bookmarks (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id    INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    locator    TEXT NOT NULL,               -- 位置串（cfi:<epubcfi> / 未来 loc1:<json>）
    label      TEXT NOT NULL DEFAULT '',    -- 展示标签（章名 / 摘录）
    fraction   REAL NOT NULL DEFAULT 0,     -- 全书进度 0..1（排序 + 百分比）
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, locator)
);
CREATE INDEX IF NOT EXISTS idx_reader_bookmarks_item ON reader_bookmarks(item_id, fraction);
";

/// DDL delta for schema version 16 — 人脸「用户移出」判别位(2026-07-10 审查 F3)。
///
/// `faces.person_id IS NULL` 有三种来源:低于 min_quality(有意不聚)、用户 unassign/reject
/// (不得自动归回)、崩溃/失效丢失聚类(**应**对账归簇)。前两者可由 quality 与本列判别,
/// 孤儿对账(face_pipeline 启动扫描)才不会对抗用户意图。
/// 语义:1 = 用户显式移出;任何把脸归给 person 的路径(reassign/create/聚类/重建)清零。
/// 注:`ALTER TABLE ... ADD COLUMN` 非幂等,由 migrate_step 的版本事务保证恰好执行一次(同 V4)。
pub const SCHEMA_V16: &str = "
ALTER TABLE faces ADD COLUMN is_unassigned INTEGER NOT NULL DEFAULT 0;
";

/// DDL delta for schema version 17 — 「图 × 人脸模型」覆盖记录(2026-07-11 加固批 B-3)。
///
/// 动机:此前「该图是否被某模型扫过」以「有无 `faces` 行」判定,而**扫过但零检出**的图
/// 没有 faces 行——切轨 sync 会把它们误归 Pending 全量重扫;X2(start 路径补 sync)因此
/// 于 2026-07-11 撤销。本表把「扫过」独立记账:`batch_finish_face_items` 在写脸行+置
/// Done 的**同一事务**内落一行覆盖(零脸图也落),从此:
///   - `sync_face_status_for_model` 以本表为覆盖真相(零脸图切轨/续传不再重扫);
///   - 流水线启动 sync 成为无损自愈(A3/F11 竞态误标的正版承接——迟到 writer 连
///     status 带覆盖行一起写,不再产生「Done 而无账」的错位);
///   - `reset_face_data`(重扫语义)连带清本表该模型的账。
///
/// 回填①:有脸项按既有 `faces` 行逐模型回填(严格真相)。
/// 回填②:零脸 Done 项(无 faces 行)归**当前激活模型**——一次性近似:face_status 是
/// 全局列,status=2 几乎必然出自当次激活轨;误差仅存在于「旧轨扫过零脸后从未切回」的
/// 项,代价是切到该旧轨时重扫一次零脸图,与迁移前行为持平、不劣化。
pub const SCHEMA_V17: &str = "
CREATE TABLE IF NOT EXISTS face_coverage (
    item_id     INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    model_name  TEXT    NOT NULL,
    analyzed_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (item_id, model_name)
);
CREATE INDEX IF NOT EXISTS idx_face_coverage_model ON face_coverage(model_name);
INSERT OR IGNORE INTO face_coverage (item_id, model_name)
    SELECT DISTINCT item_id, model_name FROM faces;
INSERT OR IGNORE INTO face_coverage (item_id, model_name)
    SELECT m.id,
           COALESCE((SELECT value FROM app_config WHERE key='face_model_active'), 'yunet-sface')
    FROM media_items m
    WHERE m.face_status = 2 AND m.media_type = 'image';
";
