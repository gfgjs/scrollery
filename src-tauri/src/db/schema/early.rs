//! DDL：CREATE TABLE / CREATE INDEX 语句 — 早期版本（V1..V9：基础表 + exotic Part1）。
//! 从 `schema/mod.rs` 重导出，外部路径 `crate::db::schema::SCHEMA_Vn` 不变。

/// 模式版本 1 的所有 DDL。
pub const SCHEMA_V1: &str = "
-- ── app_config ──────────────────────────────────────────────────────────────
-- ── 应用配置 ──────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS app_config (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

-- Seed defaults (INSERT OR IGNORE = safe to re-run)
-- 播种默认值（INSERT OR IGNORE = 安全重新运行）
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('schema_version',    '1'),
    -- 512 是有效档位（[64,128,256,512,1024]），不会被 snap_to_tier 改变。选 512 的原因：
    -- AI 分析按短边裁到 image_size（B/16·L/14=224）。缩略图按长边等比缩放，512 长边时
    -- 3:2/4:3/16:9 的短边均 ≥288 ≥224，使「用缩略图喂 CLIP」近乎全覆盖、免去解原图（见 ai/pipeline.rs）。
    ('thumb_size',        '512'),
    ('thumb_format',      'webp'),
    ('thumb_quality',     '80'),
    ('thumb_skip_max_kb', '200'),
    ('thumb_strategy',    'gpu'),
    ('gpu_engine',        'wic'),
    -- AI 高清缓存（opt-in，默认关）：开启后后台静默为每张图生成短边≥336 的 WebP 缓存，
    -- 使 CLIP 分析解码该小缓存而非全分辨率原图（见 derive/image.rs、ai/pipeline.rs）。
    ('ai_hq_cache_enabled', 'false'),
    ('theme',             'system'),
    ('last_directory_id', ''),
    ('last_sort_by',      'sort_datetime'),
    ('last_sort_order',   'desc'),
    ('sidebar_width',     '260');

-- ── scan_roots ───────────────────────────────────────────────────────────────
-- ── 扫描根目录 ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS scan_roots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    path            TEXT    NOT NULL UNIQUE,
    alias           TEXT,
    scan_status     TEXT    DEFAULT 'idle',
    scan_progress   INTEGER DEFAULT 0,
    total_files     INTEGER DEFAULT 0,
    last_scan_at    INTEGER,
    is_active       INTEGER DEFAULT 1,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- ── directories ──────────────────────────────────────────────────────────────
-- ── 目录 ──────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS directories (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    root_id         INTEGER NOT NULL REFERENCES scan_roots(id) ON DELETE CASCADE,
    parent_id       INTEGER REFERENCES directories(id) ON DELETE CASCADE,
    rel_path        TEXT    NOT NULL,
    name            TEXT    NOT NULL,
    depth           INTEGER NOT NULL DEFAULT 0,
    media_count     INTEGER NOT NULL DEFAULT 0,
    mtime           INTEGER,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    UNIQUE(root_id, rel_path)
);
CREATE INDEX IF NOT EXISTS idx_dir_root   ON directories(root_id);
CREATE INDEX IF NOT EXISTS idx_dir_parent ON directories(parent_id);

-- ── media_items ───────────────────────────────────────────────────────────────
-- ── 媒体项 ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS media_items (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    directory_id    INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,

    file_name       TEXT    NOT NULL,
    file_size       INTEGER NOT NULL,
    file_mtime      INTEGER NOT NULL,
    file_format     TEXT    NOT NULL,

    media_type      TEXT    NOT NULL DEFAULT 'image',  -- image/video/audio/document;4 类为当前划分,Part9 冷门格式时可扩(TEXT 不锁死)
    width           INTEGER NOT NULL DEFAULT 0,
    height          INTEGER NOT NULL DEFAULT 0,
    duration_ms     INTEGER,

    sort_datetime   INTEGER NOT NULL,
    cache_key       INTEGER NOT NULL,

    thumb_status    INTEGER NOT NULL DEFAULT 0,
    thumb_path      TEXT,
    thumbhash       BLOB,

    is_favorited    INTEGER NOT NULL DEFAULT 0,
    is_deleted      INTEGER NOT NULL DEFAULT 0,
    deleted_at      INTEGER,
    rating          INTEGER DEFAULT 0,    -- ⚠️ 评分制(5星 vs 10分)未定、无值域约束;临时产品决策,UI 明确后可改

    is_live_photo       INTEGER DEFAULT 0,
    has_embedded_video  INTEGER DEFAULT 0,
    companion_of        INTEGER REFERENCES media_items(id) ON DELETE SET NULL,

    content_hash    TEXT,

    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),

    UNIQUE(directory_id, file_name)
);

CREATE INDEX IF NOT EXISTS idx_media_directory ON media_items(directory_id);
CREATE INDEX IF NOT EXISTS idx_media_sort      ON media_items(sort_datetime DESC)
                                               WHERE is_deleted = 0 AND companion_of IS NULL;
CREATE INDEX IF NOT EXISTS idx_media_cache_key ON media_items(cache_key);
CREATE INDEX IF NOT EXISTS idx_media_format    ON media_items(file_format);
CREATE INDEX IF NOT EXISTS idx_media_type      ON media_items(media_type)  WHERE is_deleted = 0;
CREATE INDEX IF NOT EXISTS idx_media_thumb     ON media_items(thumb_status) WHERE thumb_status != 1;
CREATE INDEX IF NOT EXISTS idx_media_fav       ON media_items(is_favorited)
                                               WHERE is_favorited = 1 AND is_deleted = 0;
CREATE INDEX IF NOT EXISTS idx_media_del       ON media_items(is_deleted) WHERE is_deleted = 1;
CREATE INDEX IF NOT EXISTS idx_media_rating    ON media_items(rating) WHERE is_deleted = 0 AND rating > 0;
CREATE INDEX IF NOT EXISTS idx_media_hash      ON media_items(content_hash) WHERE content_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_media_companion ON media_items(companion_of) WHERE companion_of IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_media_live      ON media_items(is_live_photo) WHERE is_live_photo = 1;

-- ── image_meta ────────────────────────────────────────────────────────────────
-- ── 图像元数据 ────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS image_meta (
    item_id           INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    orientation       INTEGER DEFAULT 1,

    exif_datetime     INTEGER,
    exif_make         TEXT,
    exif_model        TEXT,
    exif_lens         TEXT,
    exif_focal_length REAL,
    exif_aperture     REAL,
    exif_shutter      TEXT,
    exif_iso          INTEGER,
    exif_gps_lat      REAL,
    exif_gps_lng      REAL,

    -- 主色调列（预留，尚无写入路径 — 供未来颜色检索特性,含 dominant_hue/sat/lum/hex 四列）：
    -- 当前仅 SELECT 读回（queries/media.rs），无任何 INSERT/UPDATE 落值,故四列实测均恒为
    -- NULL/默认;idx_img_hue 同为预建索引。接线颜色分析写入器前,勿把这些列当作有效数据源。
    dominant_hue      INTEGER,
    dominant_sat      INTEGER,
    dominant_lum      INTEGER,
    dominant_hex      TEXT,
    is_monochrome     INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_img_hue ON image_meta(dominant_hue, is_monochrome)
                                       WHERE dominant_hue IS NOT NULL;

-- ── video_meta (Phase 2 — table created now, populated later) ────────────────
-- ── 视频元数据（阶段 2 — 现在创建表，稍后填充） ────────────────
CREATE TABLE IF NOT EXISTS video_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    video_codec  TEXT
);

-- ── audio_meta (Phase 2) ──────────────────────────────────────────────────────
-- ── 音频元数据（阶段 2） ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS audio_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    audio_codec  TEXT,
    artist       TEXT,
    album_title  TEXT,
    track_title  TEXT
);
CREATE INDEX IF NOT EXISTS idx_audio_artist ON audio_meta(artist) WHERE artist IS NOT NULL;

-- ── document_meta (Phase 2) ───────────────────────────────────────────────────
-- ── 文档元数据（阶段 2） ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS document_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    page_count   INTEGER,
    doc_subtype  TEXT
);

-- ── albums / album_items (Phase 3) ────────────────────────────────────────────
-- ── 相册 / 相册项（阶段 3） ────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS albums (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    description     TEXT,
    cover_item_id   INTEGER REFERENCES media_items(id) ON DELETE SET NULL,
    sort_order      INTEGER DEFAULT 0,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS album_items (
    album_id   INTEGER NOT NULL REFERENCES albums(id)      ON DELETE CASCADE,
    item_id    INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    sort_order INTEGER DEFAULT 0,
    added_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (album_id, item_id)
);

-- ── tags / item_tags (Phase 3) ────────────────────────────────────────────────
-- ── 标签 / 项目标签（阶段 3） ────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tags (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    color      TEXT,
    parent_id  INTEGER REFERENCES tags(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS item_tags (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)        ON DELETE CASCADE,
    PRIMARY KEY (item_id, tag_id)
);
";

/// 模式版本 2 的 DDL 增量 — AI 嵌入向量。
///
/// 注意：带 `DEFAULT 0` 的 `ALTER TABLE ... ADD COLUMN` 在 SQLite 中是安全的。
pub const SCHEMA_V2: &str = "
-- ── AI embeddings ─────────────────────────────────────────────────────────────
-- ── AI 嵌入向量 ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ai_embeddings (
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    model_name   TEXT    NOT NULL,
    embedding    BLOB    NOT NULL,
    version      INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (item_id, model_name)
);
CREATE INDEX IF NOT EXISTS idx_embed_model ON ai_embeddings(model_name);

-- ── ai_status on media_items ──────────────────────────────────────────────────
-- ── media_items 上的 ai_status 字段 ──────────────────────────────────────────
-- ai_status: 0=pending, 1=processing, 2=done, 3=error
-- ai_status: 0=待处理, 1=处理中, 2=已完成, 3=错误
ALTER TABLE media_items ADD COLUMN ai_status INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_media_ai ON media_items(ai_status) WHERE ai_status < 3;

-- ── AI config defaults ────────────────────────────────────────────────────────
-- ── AI 配置默认值 ─────────────────────────────────────────────────────────────
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('ai_provider',     ''),
    ('ai_gpu_name',     ''),
    ('ai_enabled',      '1'),
    ('ai_auto_analyze', '1'),
    ('clip_model',      'cn-clip-vit-b16');
";

/// 模式版本 3 的 DDL 增量 — AI 搜索结果。
pub const SCHEMA_V3: &str = "
-- ── ai_search_results ─────────────────────────────────────────────────────────
-- ── AI 搜索结果临时表（持久化存储会话数据） ──────────────────────────────────
CREATE TABLE IF NOT EXISTS ai_search_results (
    file_id    INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    similarity REAL NOT NULL
);
";

/// 模式版本 4 的 DDL 增量 — 功能扩展 P0 地基（见 docs/archive/feature_expansion_plan_v1.md §2.2/§3.2/§3.6/§4）。
///
/// 包含：派生任务状态机表、video_meta/audio_meta 扩列、阅读进度表。
/// 注意：`ALTER TABLE ... ADD COLUMN` 非幂等，但迁移器用 `if version < 4` 守护本块，仅执行一次。
pub const SCHEMA_V4: &str = "
-- ── media_derivations：派生任务状态机（每个 (item, kind) 一行）──────────────────
-- ── 派生任务（视频封面/关键帧、文档缩略图、音频封面/元数据…）的可续传调度状态 ──
-- status: 0 待处理 / 1 处理中 / 2 完成 / 3 错误（复用 ai_status 语义，支持断点续传 + 孤儿恢复）
CREATE TABLE IF NOT EXISTS media_derivations (
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL,            -- 'video_cover'|'video_keyframes'|'doc_thumb'|'audio_cover'|'audio_meta'|...
    status       INTEGER NOT NULL DEFAULT 0,
    payload_path TEXT,                         -- 产物相对路径（sprite/封面等），可空
    error        TEXT,
    updated_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (item_id, kind)
);
-- 部分索引只覆盖未完成任务（status<2），生产者扫描待处理项时走索引、命中极小。
CREATE INDEX IF NOT EXISTS idx_deriv_pending ON media_derivations(kind, status) WHERE status < 2;

-- ── video_meta 扩列（现仅 video_codec，远不够）────────────────────────────────
ALTER TABLE video_meta ADD COLUMN fps           REAL;
ALTER TABLE video_meta ADD COLUMN bitrate       INTEGER;
ALTER TABLE video_meta ADD COLUMN rotation      INTEGER DEFAULT 0;   -- 旋转元数据，与图片 EXIF orientation 同理交换宽高
ALTER TABLE video_meta ADD COLUMN has_audio     INTEGER DEFAULT 0;
ALTER TABLE video_meta ADD COLUMN cover_time_ms INTEGER;             -- 封面取自哪一帧

-- ── audio_meta 扩列（现有 artist/album_title/track_title）─────────────────────
ALTER TABLE audio_meta ADD COLUMN track_no      INTEGER;
ALTER TABLE audio_meta ADD COLUMN year          INTEGER;
ALTER TABLE audio_meta ADD COLUMN genre         TEXT;
ALTER TABLE audio_meta ADD COLUMN lyrics_source TEXT;   -- 'embedded'|'lrc'|'none'
ALTER TABLE audio_meta ADD COLUMN lyrics_path   TEXT;   -- 外部 .lrc 路径

-- ── reading_progress：文档/EPUB 阅读进度（页码 / CFI / 滚动比例）──────────────
CREATE TABLE IF NOT EXISTS reading_progress (
    item_id    INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    position   TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
";

/// 模式版本 5 的 DDL 增量 — 收藏夹（需求7, §3.7）。
///
/// 不另造机制：复用既有 `is_favorited`（快速收藏标志，红心/索引/缓存快路径全不动）
/// 与 `albums`/`album_items`（通用多对多）。本迁移为 `albums` 扩 3 列并播种 4 个系统收藏夹。
/// 系统夹是「虚拟」的：成员 = 该类型 + is_favorited（走 idx_media_fav 快路径，无需写 album_items）；
/// 用户夹是「实体」的：成员存 album_items。详见 list_collections 注释。
pub const SCHEMA_V5: &str = "
-- ── albums 扩列：区分系统/用户夹 + 系统夹的类型过滤 + 图标 ──────────────────────
ALTER TABLE albums ADD COLUMN kind              TEXT DEFAULT 'user';   -- 'system' | 'user'
ALTER TABLE albums ADD COLUMN media_type_filter TEXT;                  -- 系统夹：image/video/audio/document
ALTER TABLE albums ADD COLUMN icon              TEXT;                  -- lucide 图标名（前端映射组件）

-- ── 播种 4 个系统收藏夹（图/视/音/文档），幂等：仅当不存在时插入 ──────────────────
INSERT INTO albums (name, kind, media_type_filter, icon, sort_order)
SELECT '图片收藏', 'system', 'image', 'Image', 1
WHERE NOT EXISTS (SELECT 1 FROM albums WHERE kind='system' AND media_type_filter='image');
INSERT INTO albums (name, kind, media_type_filter, icon, sort_order)
SELECT '视频收藏', 'system', 'video', 'Video', 2
WHERE NOT EXISTS (SELECT 1 FROM albums WHERE kind='system' AND media_type_filter='video');
INSERT INTO albums (name, kind, media_type_filter, icon, sort_order)
SELECT '音频收藏', 'system', 'audio', 'Music', 3
WHERE NOT EXISTS (SELECT 1 FROM albums WHERE kind='system' AND media_type_filter='audio');
INSERT INTO albums (name, kind, media_type_filter, icon, sort_order)
SELECT '文档收藏', 'system', 'document', 'FileText', 4
WHERE NOT EXISTS (SELECT 1 FROM albums WHERE kind='system' AND media_type_filter='document');
";

/// 模式版本 6 的 DDL 增量 — 文档浏览器/编辑（需求5.2/5.3, §3.5/§4）。替换规则（角色扮演/人名替换）+ 文档版本管理（类 git 快照树）。
///
/// 注：`replace` 是 SQLite 函数名，作列名时在所有查询中加引号 `"replace"` 以消歧。
pub const SCHEMA_V6: &str = "
-- ── doc_replacements：替换规则（§5.2）──────────────────────────────────────────
-- 纯展示层替换（不改源文件）：可绑 item / group（同系列丛书） / global。
CREATE TABLE IF NOT EXISTS doc_replacements (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_kind TEXT NOT NULL,        -- 'item' | 'group' | 'global'
    scope_id   INTEGER,              -- item_id 或书籍系列 id；global 为 NULL
    find       TEXT NOT NULL,
    replace    TEXT NOT NULL,
    is_regex   INTEGER DEFAULT 0,
    enabled    INTEGER DEFAULT 1,
    sort_order INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_repl_scope ON doc_replacements(scope_kind, scope_id) WHERE enabled = 1;

-- ── document_versions：文档版本（§5.3，类 git 全量快照 + 按需 diff）──────────────
-- 源文件不可变为基线；版本独立成文件 + 元数据成树（parent_id）。版本不进画廊。
CREATE TABLE IF NOT EXISTS document_versions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE, -- 原始件
    parent_id    INTEGER REFERENCES document_versions(id) ON DELETE SET NULL,   -- 父版本（成树）
    label        TEXT,                 -- 'AI校对稿' / '我的修订'
    storage      TEXT NOT NULL,        -- 'appdata' | 'external'
    abs_path     TEXT NOT NULL,
    source       TEXT NOT NULL,        -- 'user' | 'ai-local' | 'ai-remote'
    note         TEXT,
    content_hash TEXT,
    is_current   INTEGER DEFAULT 0,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_docver_item ON document_versions(item_id);
";

/// 模式版本 7 的 DDL 增量 — 网络盘（需求8 8B, §3.8/§4）。存储后端抽象（`storage_backends`）+ `scan_roots.backend_id`（NULL=本地）。
///
/// 8A（OS 挂载盘/UNC）不依赖本表 —— `backend_id IS NULL` 即走本地 `LocalFs`。8B 原生 VFS
/// （WebDAV，feature `netfs`）的连接信息存此表；密码不落库，仅存 keyring 引用（`cred_ref`）。
pub const SCHEMA_V7: &str = "
-- ── storage_backends：存储后端连接（§3.8 8B）──────────────────────────────────
-- 一行 = 一个已配置的存储后端（local / smb / webdav）。密码绝不落库，仅存 keyring 引用。
CREATE TABLE IF NOT EXISTS storage_backends (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,        -- 'local'|'smb'|'webdav'
    name       TEXT NOT NULL,
    host       TEXT,                 -- 或 base_url（webdav）
    base_path  TEXT,
    username   TEXT,
    cred_ref   TEXT,                 -- keyring 引用，密码不落库
    options    TEXT,                 -- JSON（扩展项，如 TLS 校验开关）
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ── scan_roots.backend_id：扫描根归属的存储后端（NULL=本地/OS 挂载，即 8A）──────────
ALTER TABLE scan_roots ADD COLUMN backend_id INTEGER REFERENCES storage_backends(id);
";

/// 模式版本 8 的 DDL 增量 — 人脸识别（Face Recognition F1 地基）:persons（人物簇）+ faces（人脸实例，一图多脸）+ media_items.face_status。
///
/// # 设计要点
/// - 人脸破 `ai_embeddings` 的 `(item_id, model_name)` 单主键范式（一图多脸）→ `faces` 每脸自增 id。
/// - `faces.model_name` = 嵌入模型身份 = 向量空间；换模型则该空间向量失效须重算（同 CLIP 不变量）。
/// - `face_status` 独立于 `ai_status`，使人脸分析与 CLIP 语义分析可分别开关、互不阻塞。
/// - `embedding`/`centroid` 为 BLOB（f32 小端）；维度由 `FaceProfile` 决定（SFace=128 / ArcFace=512）。
pub const SCHEMA_V8: &str = "
-- ── persons：人物簇（聚类结果）────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS persons (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT,                              -- NULL=未命名
    cover_face_id INTEGER,                           -- 代表脸（挑 quality 最高），关联 faces.id
    centroid      BLOB,                              -- 簇质心向量（增量归类用，f32 LE）
    face_count    INTEGER NOT NULL DEFAULT 0,
    is_named      INTEGER NOT NULL DEFAULT 0,
    is_hidden     INTEGER NOT NULL DEFAULT 0,        -- 用户隐藏（不入人物墙）
    is_ignored    INTEGER NOT NULL DEFAULT 0,        -- 误检/非人脸 归类桶
    created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ── faces：人脸实例（一图多脸；person_id 可空=未归类）──────────────────────────
CREATE TABLE IF NOT EXISTS faces (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    person_id    INTEGER REFERENCES persons(id) ON DELETE SET NULL,
    model_name   TEXT    NOT NULL,                    -- 嵌入模型身份 = 向量空间
    bbox_x       REAL NOT NULL, bbox_y REAL NOT NULL,
    bbox_w       REAL NOT NULL, bbox_h REAL NOT NULL, -- 归一化 [0,1]，与显示分辨率解耦
    landmarks    BLOB,                                -- 5 关键点（对齐+展示，f32 LE 5×2）
    det_score    REAL NOT NULL,
    quality      REAL NOT NULL DEFAULT 0,             -- 综合质量分（挑 cover / 滤低质聚类）
    embedding    BLOB NOT NULL,                       -- 嵌入向量（f32 LE，维度由 FaceProfile 定）
    is_confirmed INTEGER NOT NULL DEFAULT 0,          -- 用户确认/手动指派（重聚类不打散）
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_faces_item   ON faces(item_id);
CREATE INDEX IF NOT EXISTS idx_faces_person ON faces(person_id);
CREATE INDEX IF NOT EXISTS idx_faces_model  ON faces(model_name);

-- ── media_items.face_status：人脸检测状态机（仿 ai_status，独立开关）────────────
-- 0=待处理 / 1=处理中 / 2=完成 / 3=错误（支持断点续传 + 孤儿恢复）
ALTER TABLE media_items ADD COLUMN face_status INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_media_face ON media_items(face_status) WHERE face_status < 3;

-- ── 人脸配置默认值 ────────────────────────────────────────────────────────────
-- face_auto_analyze 默认 0：人脸分析较重，不随扫描自动触发，由用户主动开启。
-- face_model_active 默认 'yunet-sface'：商用友好（YuNet MIT + SFace Apache-2.0）。
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('face_enabled',      '1'),
    ('face_auto_analyze', '0'),
    ('face_model_active', 'yunet-sface');
";

/// 模式版本 9 的所有 DDL —— 冷门格式插件子系统 Part1（v3 总纲 §5.3 / Part1 §1.3）。
///
/// 三份真相分表，互不推导（v3 §5.1）：
///   - exotic_catalog_formats：能力真相（某格式有无产品/属哪类/提供哪些能力/哪些平台）
///   - exotic_plugins        ：安装真相（磁盘装了什么版本、各文件应是什么 hash）
///   - exotic_tasks          ：处理真相（能力级任务，独立重试/失效；非 media_items 上的状态列）
///
/// 关键设计：
///   - 不新增 `media_items.exotic_status`（会把普通媒体全标待处理；见 v3 §2.2）。
///   - exotic_tasks 含 `claimed_at` + `lease_owner`（R2 必选；项目无单实例插件，不得以单实例兜底）。
///   - 禁新增 `exotic_dev_mode` 配置（Release 不得有授权旁路；D8 / Part2 §3.5）。
pub const SCHEMA_V9: &str = "
-- ── exotic_catalog_formats：能力真相（内置 Catalog + 远程签名 Catalog 的本地投影）──────
-- 主键 = 规范化扩展名（小写、无点）。format → offering / media_kind / capabilities。
CREATE TABLE IF NOT EXISTS exotic_catalog_formats (
    format            TEXT PRIMARY KEY,              -- 小写扩展名，[a-z0-9]{1,16}
    plugin_id         TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    media_kind        TEXT NOT NULL,                 -- image / video / audio / document
    capabilities_json TEXT NOT NULL,                 -- JSON 数组，如 [\"thumbnail\"]
    license_tier      TEXT NOT NULL,                 -- free / paid
    platforms_json    TEXT NOT NULL,                 -- JSON 数组，rust target triple
    min_host_version  TEXT NOT NULL,
    store_url         TEXT,
    catalog_sequence  INTEGER NOT NULL,              -- 防目录回滚（R11；安全单调）
    source            TEXT NOT NULL                  -- builtin / remote
);

-- ── exotic_plugins：安装真相（已验证的 Package Manifest 落地）─────────────────────────
CREATE TABLE IF NOT EXISTS exotic_plugins (
    plugin_id          TEXT PRIMARY KEY,
    version            TEXT NOT NULL,                -- 展示用版本字符串
    manifest_hash      TEXT NOT NULL,
    package_sequence   INTEGER NOT NULL,             -- 防包回滚（R11；安全单调），升级只许更高
    install_state      TEXT NOT NULL,               -- installed / disabled / broken ...
    installed_at       INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

-- ── exotic_tasks：处理真相（能力级任务表）────────────────────────────────────────────
-- status：0=pending / 1=processing / 2=done / 3=retryable_error / 4=terminal_error
-- 未安装/未授权/禁用不写任务状态；Scheduler 领取时经 FormatResolution 门控（v3 §5.3）。
CREATE TABLE IF NOT EXISTS exotic_tasks (
    id                 INTEGER PRIMARY KEY,
    item_id            INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    plugin_id          TEXT NOT NULL,
    capability         TEXT NOT NULL,
    status             INTEGER NOT NULL DEFAULT 0,
    input_fingerprint  TEXT,                          -- SHA-256(规范化结构)，源/版本/参数变化即失效
    attempts           INTEGER NOT NULL DEFAULT 0,
    next_retry_at      INTEGER,
    claimed_at         INTEGER,                        -- 租约时间戳（R2）
    lease_owner        TEXT,                           -- 进程级 instance_id（仅内存生成，落库防跨实例覆盖，R2）
    last_error_code    TEXT,
    last_error_message TEXT,
    output_path        TEXT,
    worker_version     TEXT,
    created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, plugin_id, capability)
);

-- 领取索引：按 (plugin_id, capability, status, next_retry_at) 取就绪任务（百万库覆盖索引）。
CREATE INDEX IF NOT EXISTS idx_exotic_tasks_ready
ON exotic_tasks(plugin_id, capability, status, next_retry_at);
-- 跨流水线门控索引：按 item 查某 capability 是否仍未完成（CLIP/face/derive 的 NOT EXISTS）。
CREATE INDEX IF NOT EXISTS idx_exotic_tasks_item
ON exotic_tasks(item_id, capability, status);

-- ── 配置默认值（Part1 §1.3；禁 exotic_dev_mode）──────────────────────────────────────
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('exotic_enabled',      'true'),
    ('exotic_auto_process', 'true'),
    ('exotic_paused',       'false'),
    ('exotic_max_workers',  '0');     -- 0 = 由 Host 自动决定并发上限
";
