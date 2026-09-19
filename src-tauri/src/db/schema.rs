// src-tauri/src/db/schema.rs
//! 当前数据库结构的事实源与初始化入口。
//!
//! 结构正文即当前格式的完整 DDL(表 / 索引 / 必需种子)。**没有版本升迁语义**:本文件就是当前
//! 格式,不为旧库提供迁移桥,也没有串行升级块。
//!
//! 格式契约:
//! - 标识键 app_config.schema_version,当前值为常量 SCHEMA_VERSION,只作格式判别。
//! - 库内无任何用户表 = 全新库 → 建立当前结构(单事务)。
//! - 标识等于当前值 = 当前库 → 直接放行(幂等)。
//! - 其余(标识缺失 / 不符 / 未来格式)= 不兼容:报错提示重置,不自动清理用户数据与源资产,
//!   也不在旧结构上叠建。

use rusqlite::Connection;
use tracing::info;

use crate::error::{AppError, Result};

/// 当前格式标识。仅作格式判别,无升级语义;比较一律用相等判断。
pub const SCHEMA_VERSION: u32 = 34;

/// 当前格式的全部 DDL + 必需种子(单事务执行)。
const CURRENT_SCHEMA: &str = r#"
CREATE TABLE ai_embeddings (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    model_name TEXT NOT NULL,
    embedding BLOB NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (item_id, model_name)
);

CREATE TABLE ai_search_results (
    file_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    similarity REAL NOT NULL
);

CREATE TABLE album_items (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    sort_order INTEGER DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (album_id, item_id)
);

CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    cover_item_id INTEGER REFERENCES media_items(id) ON DELETE SET NULL,
    sort_order INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    kind TEXT DEFAULT 'user',
    media_type_filter TEXT,
    icon TEXT,
    deleted_at INTEGER
);

CREATE TABLE app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE audio_meta (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    audio_codec TEXT,
    artist TEXT,
    album_title TEXT,
    track_title TEXT,
    track_no INTEGER,
    year INTEGER,
    genre TEXT,
    lyrics_source TEXT,
    lyrics_path TEXT
);

CREATE TABLE dedup_index (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL,
    hash_version INTEGER NOT NULL,
    quick_digest BLOB,
    exact_digest BLOB,
    unit_digest BLOB,
    unit_size INTEGER,
    physical_key BLOB,
    status TEXT NOT NULL,
    error_code TEXT,
    checked_at INTEGER NOT NULL
);

CREATE TABLE dedup_index_working (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL,
    hash_version INTEGER NOT NULL,
    quick_digest BLOB,
    exact_digest BLOB,
    unit_digest BLOB,
    unit_size INTEGER,
    physical_key BLOB,
    status TEXT NOT NULL,
    error_code TEXT,
    checked_at INTEGER NOT NULL
);

CREATE TABLE directories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    root_id INTEGER NOT NULL REFERENCES scan_roots(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES directories(id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    name TEXT NOT NULL,
    depth INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    tree_sort_key BLOB NOT NULL DEFAULT X'',
    UNIQUE(root_id, rel_path)
);

CREATE TABLE directory_move_journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    stage TEXT NOT NULL,
    -- 'intent' | 'published' | 'source_leftover'
    source_dir_id INTEGER NOT NULL,
    source_root_id INTEGER NOT NULL,
    source_rel_path TEXT NOT NULL,
    source_abs_path TEXT NOT NULL,
    target_root_id INTEGER NOT NULL,
    target_rel_path TEXT NOT NULL,
    target_abs_path TEXT NOT NULL,
    target_parent_id INTEGER NOT NULL,
    staging_abs_path TEXT,
    payload_digest TEXT,
    payload_files INTEGER NOT NULL DEFAULT 0,
    affected_dirs INTEGER NOT NULL DEFAULT 0,
    affected_media INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE doc_replacements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_kind TEXT NOT NULL,
    -- 'item' | 'group' | 'global'
    scope_id INTEGER,
    -- item_id 或书籍系列 id；global 为 NULL
    find TEXT NOT NULL,
    replace TEXT NOT NULL,
    is_regex INTEGER DEFAULT 0,
    enabled INTEGER DEFAULT 1,
    sort_order INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE document_meta (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    page_count INTEGER,
    doc_subtype TEXT
);

CREATE TABLE document_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    -- 原始件
    parent_id INTEGER REFERENCES document_versions(id) ON DELETE SET NULL,
    -- 父版本（成树）
    label TEXT,
    -- 'AI校对稿' / '我的修订'
    storage TEXT NOT NULL,
    -- 'appdata' | 'external'
    abs_path TEXT NOT NULL,
    source TEXT NOT NULL,
    -- 'user' | 'ai-local' | 'ai-remote'
    note TEXT,
    content_hash TEXT,
    is_current INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE exotic_catalog_formats (
    format TEXT PRIMARY KEY,
    -- 小写扩展名，[a-z0-9]{1,16}
    plugin_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    media_kind TEXT NOT NULL,
    -- image / video / audio / document
    capabilities_json TEXT NOT NULL,
    -- JSON 数组，如 ["thumbnail"]
    license_tier TEXT NOT NULL,
    -- free / paid
    platforms_json TEXT NOT NULL,
    -- JSON 数组，rust target triple
    min_host_version TEXT NOT NULL,
    store_url TEXT,
    catalog_sequence INTEGER NOT NULL,
    -- 防目录回滚（R11；安全单调）
    source            TEXT NOT NULL                  -- builtin / remote
);

CREATE TABLE exotic_plugins (
    plugin_id TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    -- 展示用版本字符串
    manifest_hash TEXT NOT NULL,
    package_sequence INTEGER NOT NULL,
    -- 防包回滚（R11；安全单调），升级只许更高
    install_state TEXT NOT NULL,
    -- installed / disabled / broken ...
    installed_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE exotic_tasks (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    plugin_id TEXT NOT NULL,
    capability TEXT NOT NULL,
    status INTEGER NOT NULL DEFAULT 0,
    input_fingerprint TEXT,
    -- SHA-256(规范化结构)，源/版本/参数变化即失效
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER,
    claimed_at INTEGER,
    -- 租约时间戳（R2）
    lease_owner TEXT,
    -- 进程级 instance_id（仅内存生成，落库防跨实例覆盖，R2）
    last_error_code TEXT,
    last_error_message TEXT,
    output_path TEXT,
    worker_version TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, plugin_id, capability)
);

CREATE TABLE face_coverage (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    model_name TEXT NOT NULL,
    analyzed_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (item_id, model_name)
);

CREATE TABLE face_rejections (
    face_id INTEGER NOT NULL REFERENCES faces(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES persons(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (face_id, person_id)
);

CREATE TABLE faces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    person_id INTEGER REFERENCES persons(id) ON DELETE SET NULL,
    model_name TEXT NOT NULL,
    -- 嵌入模型身份 = 向量空间
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    -- 归一化 [0,1]，与显示分辨率解耦
    landmarks BLOB,
    -- 5 关键点（对齐+展示，f32 LE 5×2）
    det_score REAL NOT NULL,
    quality REAL NOT NULL DEFAULT 0,
    -- 综合质量分（挑 cover / 滤低质聚类）
    embedding BLOB NOT NULL,
    -- 嵌入向量（f32 LE，维度由 FaceProfile 定）
    is_confirmed INTEGER NOT NULL DEFAULT 0,
    -- 用户确认/手动指派（重聚类不打散）
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    is_unassigned INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE image_meta (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    orientation INTEGER DEFAULT 1,
    exif_datetime INTEGER,
    exif_make TEXT,
    exif_model TEXT,
    exif_lens TEXT,
    exif_focal_length REAL,
    exif_aperture REAL,
    exif_shutter TEXT,
    exif_iso INTEGER,
    exif_gps_lat REAL,
    exif_gps_lng REAL,
    -- 主色调列（预留，尚无写入路径 — 供未来颜色检索特性,含 dominant_hue/sat/lum/hex 四列）：
    -- 当前仅 SELECT 读回（queries/media.rs），无任何 INSERT/UPDATE 落值,故四列实测均恒为
    -- NULL/默认;idx_img_hue 同为预建索引。接线颜色分析写入器前,勿把这些列当作有效数据源。
    dominant_hue INTEGER,
    dominant_sat INTEGER,
    dominant_lum INTEGER,
    dominant_hex TEXT,
    is_monochrome INTEGER DEFAULT 0
);

CREATE TABLE item_tags (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (item_id, tag_id)
);

CREATE TABLE media_derivations (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    -- 'video_cover'|'video_keyframes'|'doc_thumb'|'audio_cover'|'audio_meta'|...
    status INTEGER NOT NULL DEFAULT 0,
    payload_path TEXT,
    -- 产物相对路径（sprite/封面等），可空
    error TEXT,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    orphan_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, kind)
);

CREATE TABLE media_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    directory_id INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    file_mtime INTEGER NOT NULL,
    file_format TEXT NOT NULL,
    media_type TEXT NOT NULL DEFAULT 'image',
    -- image/video/audio/document;4 类为当前划分,Part9 冷门格式时可扩(TEXT 不锁死)
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    sort_datetime INTEGER NOT NULL,
    cache_key INTEGER NOT NULL,
    thumb_status INTEGER NOT NULL DEFAULT 0,
    thumb_path TEXT,
    thumbhash BLOB,
    is_favorited INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,
    rating INTEGER DEFAULT 0,
    -- ⚠️ 评分制(5星 vs 10分)未定、无值域约束;临时产品决策,UI 明确后可改
    is_live_photo INTEGER DEFAULT 0,
    has_embedded_video INTEGER DEFAULT 0,
    companion_of INTEGER REFERENCES media_items(id) ON DELETE SET NULL,
    content_hash TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    ai_status INTEGER NOT NULL DEFAULT 0,
    face_status INTEGER NOT NULL DEFAULT 0,
    volume_id INTEGER REFERENCES volumes(id) ON DELETE SET NULL,
    volume_relative_path TEXT,
    availability TEXT NOT NULL DEFAULT 'online',
    color_label INTEGER NOT NULL DEFAULT 0,
    content_identifier TEXT,
    view_rotation INTEGER NOT NULL DEFAULT 0,
    playback_position_ms INTEGER NOT NULL DEFAULT 0,
    file_mtime_ns INTEGER,
    source_revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE(directory_id, file_name)
);

CREATE TABLE persons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    -- NULL=未命名
    cover_face_id INTEGER,
    -- 代表脸（挑 quality 最高），关联 faces.id
    centroid BLOB,
    -- 簇质心向量（增量归类用，f32 LE）
    face_count INTEGER NOT NULL DEFAULT 0,
    is_named INTEGER NOT NULL DEFAULT 0,
    is_hidden INTEGER NOT NULL DEFAULT 0,
    -- 用户隐藏（不入人物墙）
    is_ignored INTEGER NOT NULL DEFAULT 0,
    -- 误检/非人脸 归类桶
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    model_name TEXT NOT NULL DEFAULT 'yunet-sface'
);

CREATE TABLE reader_book_prefs (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    prefs TEXT NOT NULL,
    -- 版本化 JSON,仅存与全局默认的 diff
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE reader_bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    locator TEXT NOT NULL,
    -- 位置串（cfi:<epubcfi> / 未来 loc1:<json>）
    label TEXT NOT NULL DEFAULT '',
    -- 展示标签（章名 / 摘录）
    fraction REAL NOT NULL DEFAULT 0,
    -- 全书进度 0..1（排序 + 百分比）
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, locator)
);

CREATE TABLE reading_progress (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    position TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE scan_roots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    alias TEXT,
    scan_status TEXT DEFAULT 'idle',
    scan_progress INTEGER DEFAULT 0,
    total_files INTEGER DEFAULT 0,
    last_scan_at INTEGER,
    is_active INTEGER DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    volume_id INTEGER REFERENCES volumes(id) ON DELETE SET NULL,
    volume_subpath TEXT,
    is_hidden INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE storage_backends (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    -- 'local'|'smb'|'webdav'
    name TEXT NOT NULL,
    host TEXT,
    -- 或 base_url（webdav）
    base_path TEXT,
    username TEXT,
    cred_ref TEXT,
    -- keyring 引用，密码不落库
    options TEXT,
    -- JSON（扩展项，如 TLS 校验开关）
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    parent_id INTEGER REFERENCES tags(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE text_book_index (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    src_key TEXT NOT NULL,
    encoding TEXT NOT NULL,
    confidence TEXT NOT NULL,
    chapters TEXT NOT NULL,
    -- JSON [{t:标题, s:byte_start, e:byte_end, n:char_len}, ...]
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE video_meta (
    item_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    video_codec TEXT,
    fps REAL,
    bitrate INTEGER,
    rotation INTEGER DEFAULT 0,
    has_audio INTEGER DEFAULT 0,
    cover_time_ms INTEGER
);

CREATE TABLE volumes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    stable_id TEXT NOT NULL UNIQUE,
    -- Win '{GUID}' / mac UUID / 规范化UNC '//host/share'
    label TEXT,
    -- 卷标(展示用，可重命名)
    kind TEXT NOT NULL DEFAULT 'local',
    -- 'local'|'removable'|'network'
    last_mount_path TEXT,
    -- 最近挂载点/盘符(提示+运行期路径重组，非身份键)
    last_seen INTEGER,
    -- 最近在线 unix 秒
    is_online INTEGER NOT NULL DEFAULT 0,
    -- 运行期状态(启动 probe_volumes 刷新)
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ── 索引(37) ──

CREATE INDEX idx_audio_artist ON audio_meta(artist) WHERE artist IS NOT NULL;

CREATE INDEX idx_dedup_exact_candidate
    ON dedup_index(exact_digest, item_id)
    WHERE exact_digest IS NOT NULL;

CREATE INDEX idx_dedup_quick_candidate
    ON dedup_index(quick_digest, item_id)
    WHERE quick_digest IS NOT NULL;

CREATE INDEX idx_dedup_unit_group
    ON dedup_index(unit_digest, unit_size, item_id)
    WHERE unit_digest IS NOT NULL;

CREATE INDEX idx_dedup_working_quick
    ON dedup_index_working(quick_digest, item_id)
    WHERE quick_digest IS NOT NULL;

CREATE INDEX idx_deriv_pending ON media_derivations(kind, status) WHERE status < 2;

CREATE INDEX idx_dir_parent ON directories(parent_id);

CREATE INDEX idx_dir_root   ON directories(root_id);

CREATE INDEX idx_docver_item ON document_versions(item_id);

CREATE INDEX idx_embed_model ON ai_embeddings(model_name);

CREATE INDEX idx_exotic_tasks_item
ON exotic_tasks(item_id, capability, status);

CREATE INDEX idx_exotic_tasks_ready
ON exotic_tasks(plugin_id, capability, status, next_retry_at);

CREATE INDEX idx_face_coverage_model ON face_coverage(model_name);

CREATE INDEX idx_faces_item   ON faces(item_id);

CREATE INDEX idx_faces_model  ON faces(model_name);

CREATE INDEX idx_faces_person ON faces(person_id);

CREATE INDEX idx_img_hue ON image_meta(dominant_hue, is_monochrome)
                                       WHERE dominant_hue IS NOT NULL;

CREATE INDEX idx_media_ai ON media_items(ai_status) WHERE ai_status < 3;

CREATE INDEX idx_media_avail  ON media_items(availability) WHERE availability != 'online';

CREATE INDEX idx_media_cache_key ON media_items(cache_key);

CREATE INDEX idx_media_companion ON media_items(companion_of) WHERE companion_of IS NOT NULL;

CREATE INDEX idx_media_content_id ON media_items(content_identifier) WHERE content_identifier IS NOT NULL;

CREATE INDEX idx_media_del       ON media_items(is_deleted) WHERE is_deleted = 1;

CREATE INDEX idx_media_face ON media_items(face_status) WHERE face_status < 3;

CREATE INDEX idx_media_fav       ON media_items(is_favorited)
                                               WHERE is_favorited = 1 AND is_deleted = 0;

CREATE INDEX idx_media_format    ON media_items(file_format);

CREATE INDEX idx_media_hash      ON media_items(content_hash) WHERE content_hash IS NOT NULL;

CREATE INDEX idx_media_live      ON media_items(is_live_photo) WHERE is_live_photo = 1;

CREATE INDEX idx_media_rating    ON media_items(rating) WHERE is_deleted = 0 AND rating > 0;

CREATE INDEX idx_media_sort ON media_items(sort_datetime DESC, id DESC)
    WHERE is_deleted = 0 AND companion_of IS NULL;

CREATE INDEX idx_media_thumb     ON media_items(thumb_status) WHERE thumb_status != 1;

CREATE INDEX idx_media_trash ON media_items(deleted_at DESC, id DESC)
    WHERE is_deleted = 1;

CREATE INDEX idx_media_type_id
    ON media_items(media_type, id) WHERE is_deleted = 0;

CREATE INDEX idx_media_type_sort
    ON media_items(media_type, sort_datetime DESC, id DESC) WHERE is_deleted = 0;

CREATE INDEX idx_media_volume ON media_items(volume_id)    WHERE volume_id IS NOT NULL;

CREATE INDEX idx_reader_bookmarks_item ON reader_bookmarks(item_id, fraction);

CREATE INDEX idx_repl_scope ON doc_replacements(scope_kind, scope_id) WHERE enabled = 1;

-- ── 种子 ────────────────────────────────────────────────────────────────────
-- 只留当前真实需要默认值的项:旧设置种子(thumb_*/ai_enabled/clip_model/face_*/exotic_enabled 等)
-- 的真源已切到 config.toml(SETTING_DEFS),不再进库。
-- schema_version = 单一格式标识(无 v1..33 升级语义)。
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('last_directory_id', ''),
    ('last_sort_by', 'sort_datetime'),
    ('last_sort_order', 'desc'),
    ('sidebar_width', '260'),
    ('ai_provider', ''),
    ('ai_gpu_name', ''),
    ('exotic_paused', 'false');

-- 4 个系统收藏夹(kind='system'):仅新库建立时执行一次,故无需存在性守卫。
INSERT INTO albums (name, kind, media_type_filter, icon, sort_order) VALUES
    ('图片收藏', 'system', 'image', 'Image', 1),
    ('视频收藏', 'system', 'video', 'Video', 2),
    ('音频收藏', 'system', 'audio', 'Music', 3),
    ('文档收藏', 'system', 'document', 'FileText', 4);
"#;

/// 读取库内的格式标识(表或键缺失 → 0)。
pub fn read_schema_version(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT value FROM app_config WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<u32>().ok())
    .unwrap_or(0)
}

/// 初始化数据库结构 —— 生产启动与所有建库夹具的唯一入口。
///
/// - 标识 == SCHEMA_VERSION → 当前库,直接放行(幂等,可重复调用)。
/// - 全新库(无任何用户表)→ 单事务建立当前结构:DDL 与格式标识同事务提交,失败整块回滚不留半成品。
/// - 其余(有表但标识缺失 / 不符 / 未来格式)→ AppError::SchemaIncompatible,不改动任何数据。
pub fn initialize_schema(conn: &Connection) -> Result<()> {
    let version = read_schema_version(conn);
    if version == SCHEMA_VERSION {
        info!("DB schema is current (format {SCHEMA_VERSION}) | 数据库结构为当前格式 ({SCHEMA_VERSION})");
        return Ok(());
    }

    // 非全新库(已有用户表)而标识不是当前值 → 明确不兼容:缺标识的旧库**不得**当作全新库叠建。
    if version != 0 || !is_fresh_database(conn)? {
        info!(
            "DB schema incompatible: found format {version}, expected {SCHEMA_VERSION} | 数据库结构不兼容:当前标识 {version},期望 {SCHEMA_VERSION}"
        );
        return Err(AppError::SchemaIncompatible);
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(CURRENT_SCHEMA)?;
    // 格式标识与结构同事务写入:版本值的唯一事实源是常量,DDL 文本里不再出现版本字面量。
    tx.execute(
        "INSERT INTO app_config (key, value) VALUES ('schema_version', ?1)",
        rusqlite::params![SCHEMA_VERSION.to_string()],
    )?;
    tx.commit()?;
    info!("DB schema created (format {SCHEMA_VERSION}) | 已建立数据库结构(格式 {SCHEMA_VERSION})");
    Ok(())
}

/// 库内是否没有任何用户表(全新库判据)。sqlite_ 前缀为引擎内部表,不计。
fn is_fresh_database(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    Ok(count == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建当前结构:关键表/索引就位 + 系统收藏夹种子 + 默认值语义 + 外键与完整性干净 + 格式标识写入。
    /// (不做表/索引数量镜像 —— 只断言业务上必须存在的代表对象与默认行为。)
    #[test]
    fn initialize_creates_current_schema_with_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), SCHEMA_VERSION);

        for obj in [
            "media_items",
            "directories",
            "scan_roots",
            "albums",
            "album_items",
            "image_meta",
            "media_derivations",
            "faces",
            "dedup_index",
            "directory_move_journal",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [obj],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺表 {obj}");
        }
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_media_content_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "缺当前索引 idx_media_content_id");

        // 系统收藏夹种子(当前业务必要)。
        let sys: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE kind = 'system'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sys, 4);

        // 默认值语义:只给 path/alias 建根,其余列取当前默认。
        conn.execute(
            "INSERT INTO scan_roots (path, alias) VALUES ('C:/photos', '图库')",
            [],
        )
        .unwrap();
        let (status, progress, active, hidden, alias): (String, i64, i64, i64, String) = conn
            .query_row(
                "SELECT scan_status, scan_progress, is_active, is_hidden, alias FROM scan_roots",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), progress, active, hidden, alias.as_str()),
            ("idle", 0, 1, 0, "图库")
        );

        assert!(
            foreign_key_violations(&conn).is_empty(),
            "建库后不得有外键违规"
        );
        assert!(integrity_ok(&conn), "建库后完整性须通过");
    }

    /// 当前库重复初始化 = 幂等放行,且不得清空既有行。
    #[test]
    fn initialize_is_idempotent_and_keeps_existing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES ('layout_mode', 'grid')",
            [],
        )
        .unwrap();
        assert_eq!(
            conn.execute("INSERT INTO scan_roots (path) VALUES ('D:/a')", [])
                .unwrap(),
            1
        );

        initialize_schema(&conn).unwrap();

        let v: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key='layout_mode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "grid", "当前库再次初始化不得清库");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM scan_roots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "当前库再次初始化不得清库");
    }

    /// 缺格式标识的非空旧库:**明确不兼容**,不得被当作全新库叠建、不得改动原数据。
    #[test]
    fn initialize_rejects_legacy_database_without_touching_data() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO app_config (key, value) VALUES ('thumb_size', '512');
             CREATE TABLE legacy_only (id INTEGER PRIMARY KEY);",
        )
        .unwrap();

        let e = initialize_schema(&conn).unwrap_err();
        assert!(
            matches!(e, AppError::SchemaIncompatible),
            "旧库须报不兼容,得 {e}"
        );

        let kept: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key='thumb_size'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, "512", "旧库数据不得被改动");
        let built: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='media_items'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(built, 0, "不得在旧库上叠建当前结构");
        assert_eq!(read_schema_version(&conn), 0, "不得写入当前格式标识");
    }

    /// 标识存在但不是当前值(历史值或未来值)→ 不兼容,且不做任何「升级」。
    #[test]
    fn initialize_rejects_unknown_format_marker() {
        for bogus in [1u32, 33, SCHEMA_VERSION + 1] {
            let conn = Connection::open_in_memory().unwrap();
            initialize_schema(&conn).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES ('schema_version', ?1)",
                rusqlite::params![bogus.to_string()],
            )
            .unwrap();

            let e = initialize_schema(&conn).unwrap_err();
            assert!(
                matches!(e, AppError::SchemaIncompatible),
                "标识 {bogus} 须报不兼容,得 {e}"
            );
            assert_eq!(read_schema_version(&conn), bogus, "不得被「修好」");
        }
    }

    /// 建库中途失败 → 整块回滚,不留半成品。
    /// 制造点:预置一个与**末段某表**同名的视图 —— 前面的表已成功建立,轮到它时建表必然失败,
    /// 以此证明已执行的 DDL 会随事务整体回滚(而不是留下半套结构)。
    #[test]
    fn initialize_rolls_back_partially_applied_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIEW directory_move_journal AS SELECT 1 AS id;")
            .unwrap();

        let e = initialize_schema(&conn).unwrap_err();
        assert!(matches!(e, AppError::Db(_)), "中途失败须如实报错,得 {e}");
        assert!(
            format!("{e}").contains("directory_move_journal"),
            "失败点须落在预置的同名表上,得 {e}"
        );

        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "失败后不得残留半成品表(前面的 DDL 须整体回滚)");
        assert_eq!(read_schema_version(&conn), 0, "失败后不得写入格式标识");
    }

    fn foreign_key_violations(conn: &Connection) -> Vec<String> {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn integrity_ok(conn: &Connection) -> bool {
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .map(|s| s == "ok")
            .unwrap_or(false)
    }
}
