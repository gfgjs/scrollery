---
id: 2026-07-24-Spec01_数据层
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# 01-数据层

> 一句话:本篇讲 Scrollery 的 SQLite 持久层——当前格式 33 张表 + 37 个显式索引(单一 `db/schema.rs` 完整 DDL,无版本升迁)、建库与格式判别、连接/并发模型、自定义排序算法;服务需要读写这些表或扩展 schema 的工程师与编码代理。读前无需先读其余篇,本篇自包含。

---

## 1. 概览

`src-tauri/src/db/` 是全应用**唯一**的持久化层:所有子系统(扫描、画廊、缩略图/派生、AI、人脸、收藏、标签、文档阅读器、exotic 插件、存储卷)都通过这里的表与查询模块读写状态,没有旁路数据库或影子存储。

代码位置(主目录 `src-tauri/src/db/`):

| 文件 | 职责 |
|---|---|
| `mod.rs` | 模块声明 + `register_custom_collations()` 单一注册入口(`NATURAL_CMP` collation、`TREE_SORT_KEY` 标量函数) |
| `connection.rs` | 写连接(`Mutex<Connection>`)与读连接池(`r2d2::Pool`)的创建、PRAGMA 应用、启动期 WAL checkpoint |
| `schema.rs` | 当前格式的**全部** DDL + 建库必需种子 + 建库入口 `initialize_schema`;`SCHEMA_VERSION` 常量只作格式判别(§2.1) |
| `boot.rs` | 启动期装配:恢复交换 → 写连接 + `initialize_schema`(不兼容时含恢复回滚分支)→ 恢复收尾 → 读池;tracing 就绪后的启动自愈四项 |
| `models/*.rs` | Rust 侧数据结构,与表 / 查询结果对应 |
| `queries/*.rs` | 查询子模块,按子系统分工(见 §3.4) |

在整机中的位置(上游调用者 → db 层 → SQLite 文件):

```
Tauri commands (src-tauri/src/commands/*)
         │  spawn_blocking 包裹
         ▼
   db::queries::*  ──── DbWriter (Mutex<Connection>)  ──→  scrollery.db (WAL)
         │                                                       ▲
         └──── DbPool (r2d2, 只读) ──────────────────────────────┘
```

- 上游:各 Tauri IPC command handler、扫描器(scanner)、富化器(enricher)、派生任务 worker、AI/人脸 pipeline。它们都不直接 `rusqlite::Connection::open`,而是拿注入的 `DbWriter` 或 `DbPool`。
- 下游:单一 SQLite 文件(WAL 模式,同卷放置),无外部数据库依赖。

---

## 2. 数据模型与状态

### 2.1 Schema 版本

- 当前格式标识:**34**(`src-tauri/src/db/schema.rs` `pub const SCHEMA_VERSION: u32 = 34;`)。它**只用于判别库/备份包是否为本程序当前格式**,不含任何「从哪个版本升上来」的语义,比较一律用相等判断。
- 标识存于 `app_config` 表的 `schema_version` 键:建库事务内与结构一同写入(`initialize_schema`);读不到键(空库 / 旧库)返回 0(`read_schema_version`,`.ok().and_then(parse).unwrap_or(0)`)。
- 建库与格式判别只有一条路径 `initialize_schema(conn)`(`schema.rs`),判据三条:
  1. 标识 == `SCHEMA_VERSION` → 当前库,**直接放行**(幂等,可重复调用,不清数据);
  2. 库内无任何用户表(`sqlite_%` 引擎表不计)→ 全新库 → **单事务**执行 `CURRENT_SCHEMA`(全部 DDL + 必需种子)并写入格式标识,失败整块回滚不留半成品;
  3. 其余(有表但标识缺失 / 不符 / 未来值)→ `AppError::SchemaIncompatible`(稳定码 `db_schema_incompatible`),**不改动任何数据**,也不在旧结构上叠建。
- 因此代码里**没有** V1..33 升级步骤、没有按版本切分的 DDL 常量文件、没有回填 DML、没有旧 `config` 迁移;发现旧格式库时应用报错提示重置,不自动清理用户数据与源资产。
- 备份/恢复复用同一个 `SCHEMA_VERSION`(见 §7 与 [Spec08](./Spec08_存储备份导出文件操作.md) §3.2)。

### 2.2 表清单总览(33 张)

以下核心表给全列;附属表精炼为关键列 + 一句话职责,DDL 均以 `src-tauri/src/db/schema.rs` 现码为准。

#### 核心表(全列)

**`app_config`**(`schema.rs` `CURRENT_SCHEMA`)—— 应用状态 + UI 记忆偏好键值表

| 列 | 类型 | 约束 |
|---|---|---|
| key | TEXT | PRIMARY KEY |
| value | TEXT | NOT NULL |

本表**不是**设置项的真源:77 项设置类键的唯一权威是 `<app_data_dir>/config.toml`(键定义单源 `src-tauri/src/config/schema.rs` 的 `SETTING_DEFS`)。本表只存两类键:

- **应用状态 + UI 记忆偏好**:由 `config/schema.rs` 的 `STATE_KEYS` 声明,当前 **36 项**(应用状态 19 项 + 阅读器 UI 记忆偏好 17 项);两清单之外的键由 `get_app_config`/`set_app_config` 两个入口经共用判定以稳定码 `config_unknown_key` 拒绝(既不读 DB 也不写 DB)。
- **格式标识**:`schema_version` 也在 `STATE_KEYS` 内(上述 36 项已含),由建库事务写入(§2.1)。

建库种子(`CURRENT_SCHEMA` 内 `INSERT OR IGNORE`)只留当前真实需要默认值的项:`last_directory_id`、`last_sort_by`、`last_sort_order`、`sidebar_width`、`ai_provider`、`ai_gpu_name`、`exotic_paused`;其余状态键由应用在实际使用时写入。旧设置种子(`thumb_*`/`ai_enabled`/`clip_model`/`face_*`/`exotic_enabled` 等)已随设置真源切到 `config.toml` 退出本表。

**`scan_roots`**(`schema.rs` `CURRENT_SCHEMA`)—— 用户添加的扫描根目录。远程根的身份由 `volume_id` 承担:`backend_id` 及指向 `storage_backends` 的外键已删除,网络/网络盘绑定走 `volumes`。

| 列 | 类型 | 约束/备注 |
|---|---|---|
| id | INTEGER | PK AUTOINCREMENT |
| path | TEXT | NOT NULL UNIQUE |
| alias | TEXT | |
| scan_status | TEXT | DEFAULT 'idle' |
| scan_progress | INTEGER | DEFAULT 0 |
| total_files | INTEGER | DEFAULT 0 |
| last_scan_at | INTEGER | |
| is_active | INTEGER | DEFAULT 1 |
| volume_id | INTEGER | FK→volumes(id) ON DELETE SET NULL;所在卷 |
| volume_subpath | TEXT | 卷内相对路径 |
| is_hidden | INTEGER | NOT NULL DEFAULT 0;库范围隐藏开关 |
| created_at / updated_at | INTEGER | DEFAULT strftime('%s','now') |

**`directories`**(`schema.rs` `CURRENT_SCHEMA`,含索引 `idx_dir_root`/`idx_dir_parent`)—— 目录树节点

| 列 | 类型 | 约束/备注 |
|---|---|---|
| id | INTEGER | PK AUTOINCREMENT |
| root_id | INTEGER | NOT NULL FK→scan_roots(id) CASCADE |
| parent_id | INTEGER | FK→directories(id) CASCADE(自引用成树) |
| rel_path | TEXT | NOT NULL |
| name | TEXT | NOT NULL |
| depth | INTEGER | NOT NULL DEFAULT 0 |
| media_count | INTEGER | NOT NULL DEFAULT 0 |
| mtime | INTEGER | |
| tree_sort_key | BLOB | NOT NULL DEFAULT X'';前序 DFS 排序键,持久化(见 §3.2) |
| created_at | INTEGER | DEFAULT strftime('%s','now') |
| — | | UNIQUE(root_id, rel_path) |

索引:`idx_dir_root(root_id)`、`idx_dir_parent(parent_id)`(树导航)。

**`media_items`**(`schema.rs` `CURRENT_SCHEMA`)—— 媒体条目,全库最核心表。下表列齐当前格式的全部列,与 `schema.rs` 中该表 DDL 同源(本篇不复制 DDL 正文,源码是唯一事实源)。

| 列 | 类型 | 约束/备注 |
|---|---|---|
| id | INTEGER | PK AUTOINCREMENT |
| directory_id | INTEGER | NOT NULL FK→directories(id) CASCADE |
| file_name | TEXT | NOT NULL |
| file_size | INTEGER | NOT NULL |
| file_mtime | INTEGER | NOT NULL |
| file_format | TEXT | NOT NULL |
| media_type | TEXT | NOT NULL DEFAULT 'image';image/video/audio/document |
| width / height | INTEGER | NOT NULL DEFAULT 0 |
| duration_ms | INTEGER | |
| sort_datetime | INTEGER | NOT NULL |
| cache_key | INTEGER | NOT NULL |
| thumb_status | INTEGER | NOT NULL DEFAULT 0 |
| thumb_path | TEXT | |
| thumbhash | BLOB | |
| is_favorited | INTEGER | NOT NULL DEFAULT 0 |
| is_deleted | INTEGER | NOT NULL DEFAULT 0;软删除标志 |
| deleted_at | INTEGER | |
| rating | INTEGER | DEFAULT 0;待核实(5 星制还是 10 分制未在 schema 层定案,注释原文如此) |
| is_live_photo | INTEGER | DEFAULT 0 |
| has_embedded_video | INTEGER | DEFAULT 0 |
| companion_of | INTEGER | FK→media_items(id) SET NULL(Live Photo 配对) |
| content_hash | TEXT | |
| ai_status | INTEGER | NOT NULL DEFAULT 0;0=pending/1=processing/2=done/3=error |
| volume_id | INTEGER | FK→volumes(id) SET NULL;批量卷切换合并键 |
| volume_relative_path | TEXT | 卷根起完整相对路径,重新挂载后重连接键 |
| availability | TEXT | NOT NULL DEFAULT 'online';online\|offline\|missing,卷驱动、与 is_deleted 正交 |
| color_label | INTEGER | NOT NULL DEFAULT 0;0=无/1-7 色板 |
| content_identifier | TEXT | HEIC ContentIdentifier,Live Photo 匹配/元数据 |
| face_status | INTEGER | NOT NULL DEFAULT 0;0=pending/1=processing/2=done/3=error,与 ai_status 独立 |
| view_rotation | INTEGER | NOT NULL DEFAULT 0;用户查看器旋转角(0/90/180/270 顺时针),与 video_meta.rotation 正交 |
| playback_position_ms | INTEGER | NOT NULL DEFAULT 0;播放续播位置 |
| file_mtime_ns | INTEGER | 文件 mtime 纳秒部分(与 file_mtime 秒级配对) |
| source_revision | INTEGER | NOT NULL DEFAULT 1;源修订号,字节变化即递增 |
| created_at / updated_at | INTEGER | DEFAULT strftime('%s','now') |
| — | | UNIQUE(directory_id, file_name) |

本表 18 个显式索引(当前格式一次性建齐,无「增删索引」的版本史):`idx_media_cache_key`、`idx_media_format`、`idx_media_thumb`(WHERE thumb_status != 1)、`idx_media_fav`、`idx_media_del`、`idx_media_rating`、`idx_media_hash`、`idx_media_companion`、`idx_media_live`、`idx_media_ai`、`idx_media_face`、`idx_media_content_id`、`idx_media_avail`、`idx_media_volume`、`idx_media_sort`(复合 `(sort_datetime DESC, id DESC)`,WHERE 未删且非配对项)、`idx_media_trash`(`(deleted_at DESC, id DESC)`,WHERE 已删)、`idx_media_type_id(media_type,id)`、`idx_media_type_sort(media_type,sort_datetime DESC,id DESC)`(后两者 WHERE `is_deleted=0`,富化 keyset 用)。`directory_id` 无显式索引:`UNIQUE(directory_id,file_name)` 的隐式索引 `sqlite_autoindex_media_items_1` 已覆盖同一查询面。

**`app_config` / `scan_roots` / `directories` / `media_items` 为核心表,以下为附属表**,按功能分组精炼列出(表名、来源、关键列、职责):

##### 媒体元数据扩展(1:1 挂 media_items)

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `image_meta` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`orientation`、EXIF 全套(`exif_datetime/make/model/lens/focal_length/aperture/shutter/iso/gps_lat/gps_lng`)、`dominant_hue/sat/lum/hex`、`is_monochrome`。**主色调四列 + `idx_img_hue` 目前无写入路径**(仅被读回,实测恒为 NULL/默认),接线颜色分析写入器前不得当作有效数据源 | 图像 EXIF + 主色调分析(预留)。索引 `idx_img_hue` |
| `video_meta` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`video_codec`、`fps`、`bitrate`、`rotation`(内禀旋转,与 `media_items.view_rotation` 正交)、`has_audio`、`cover_time_ms` | 视频编解码 + 封面帧时间点 |
| `audio_meta` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`audio_codec`、`artist`、`album_title`、`track_title`、`track_no`、`year`、`genre`、`lyrics_source`('embedded'\|'lrc'\|'none')、`lyrics_path` | 音频标签 + 歌词来源。索引 `idx_audio_artist` |
| `document_meta` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`page_count`、`doc_subtype` | 文档页数/子类型 |
| `reading_progress` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`position` TEXT NOT NULL;`updated_at` | 通用阅读/播放进度(与阅读器专表并列) |

##### 收藏与标签

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `albums` | `CURRENT_SCHEMA` | `id` PK;`name`、`description`、`cover_item_id` FK SET NULL、`kind`('system'\|'user')、`media_type_filter`、`icon`(lucide 图标名)、`sort_order`、`deleted_at`(软删除)、`created_at/updated_at` | 收藏夹/系统收藏集;建库种子写入 4 个系统收藏(图片/视频/音频/文档,kind='system') |
| `album_items` | `CURRENT_SCHEMA` | PK(album_id, item_id);两 FK 均 CASCADE;`sort_order`、`added_at` | 收藏夹-媒体多对多 |
| `tags` | `CURRENT_SCHEMA` | `id` PK;`name` UNIQUE;`color`;`parent_id` 自引用 CASCADE(层级标签) | 标签定义 |
| `item_tags` | `CURRENT_SCHEMA` | PK(item_id, tag_id);两 FK CASCADE | 标签-媒体多对多 |

##### AI 与向量检索

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `ai_embeddings` | `CURRENT_SCHEMA` | PK(item_id, model_name);`item_id` FK CASCADE;`embedding` BLOB NOT NULL;`version`;`created_at` | 嵌入模型向量存储(f32 小端 BLOB)。索引 `idx_embed_model` |
| `ai_search_results` | `CURRENT_SCHEMA` | `file_id` PK FK CASCADE;`similarity` REAL NOT NULL | 语义搜索结果暂存 |

##### 派生任务与文档版本

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `media_derivations` | `CURRENT_SCHEMA` | PK(item_id, kind);`kind`(video_cover/video_keyframes/doc_thumb/audio_cover/audio_meta 等);`status`(0-3);`payload_path`;`error`;`orphan_count`(孤儿重置计数,达阈值判 error,防毒任务) | 各类派生产物任务状态机。索引 `idx_deriv_pending` |
| `doc_replacements` | `CURRENT_SCHEMA` | `id` PK;`scope_kind`('item'\|'group'\|'global');`scope_id`;`find`/`replace` 两列均**未加引号**(与 SQL 关键字 `REPLACE` 同名,当前 DDL 按原样书写) | 文档文字替换规则。索引 `idx_repl_scope` |
| `document_versions` | `CURRENT_SCHEMA` | `id` PK;`item_id` FK CASCADE;`parent_id` 自引用 SET NULL(成树);`label`;`storage`('appdata'\|'external');`abs_path`;`source`('user'\|'ai-local'\|'ai-remote');`note`;`content_hash`;`is_current` | 文档编辑版本树。索引 `idx_docver_item` |

##### 存储后端与卷

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `storage_backends` | `CURRENT_SCHEMA` | `id` PK;`kind`('local'\|'smb'\|'webdav');`name`;`host`;`base_path`;`username`;`cred_ref`(keyring 引用,密码不落库);`options`(JSON);`created_at` | 存储后端连接管理表(配置与连通性测试;不再承载插件渠道/权益。媒体绑定的卷身份在 `volumes`) |
| `volumes` | `CURRENT_SCHEMA` | `id` PK;`stable_id` UNIQUE(Win GUID/mac UUID/规范化 UNC);`label`;`kind`('local'\|'removable'\|'network');`last_mount_path`;`last_seen`;`is_online`(运行时探测态) | 可移动/网络卷身份追踪,支撑重挂载重连接 |

##### 人脸识别

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `persons` | `CURRENT_SCHEMA` | `id` PK;`name`(NULL=未命名簇);`model_name` NOT NULL DEFAULT 'yunet-sface'(模型轨道隔离);`cover_face_id`;`centroid` BLOB(簇质心);`face_count`;`is_named`;`is_hidden`;`is_ignored`(误检/非人脸桶) | 人脸聚类结果(人物) |
| `faces` | `CURRENT_SCHEMA` | `id` PK;`item_id` FK CASCADE;`person_id` FK SET NULL;`model_name` NOT NULL(嵌入模型身份=向量空间);`bbox_x/y/w/h`(归一化[0,1]);`landmarks` BLOB;`det_score`;`quality`;`embedding` BLOB NOT NULL;`is_confirmed`(用户确认,防重聚类覆盖);`is_unassigned`(用户主动取消分配,孤儿对账标记) | 单张脸检测+嵌入记录。索引 `idx_faces_item`/`idx_faces_person`/`idx_faces_model` |
| `face_rejections` | `CURRENT_SCHEMA` | PK(face_id, person_id);两 FK CASCADE | 用户明确拒绝的脸-人物匹配(负样本) |
| `face_coverage` | `CURRENT_SCHEMA` | PK(item_id, model_name);`item_id` FK CASCADE;`analyzed_at` | 按模型追踪哪些条目已跑过人脸分析 |

##### Exotic 插件平台

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `exotic_catalog_formats` | `CURRENT_SCHEMA` | `format` PK(小写扩展名);`plugin_id`;`display_name`;`media_kind`;`capabilities_json`;`license_tier`('free'\|'paid');`platforms_json`;`min_host_version`;`store_url`;`catalog_sequence`(防回滚);`source`('builtin'\|'remote') | 冷门格式能力目录 |
| `exotic_plugins` | `CURRENT_SCHEMA` | `plugin_id` PK;`version`;`manifest_hash`;`package_sequence`(防回滚);`install_state`;`installed_at`/`updated_at`。**`entitlement_source` 列已删除**(渠道/权益不再落库) | 已安装插件登记 |
| `exotic_tasks` | `CURRENT_SCHEMA` | `id` PK;`item_id` FK CASCADE;`plugin_id`;`capability`;`status`(retryable/terminal error 分级);`input_fingerprint`(SHA-256,缓存失效判据);`attempts`;`next_retry_at`;`claimed_at`/`lease_owner`(跨实例互斥租约);`last_error_code/message`;`output_path`;`worker_version`;UNIQUE(item_id,plugin_id,capability) | 插件任务队列。索引 `idx_exotic_tasks_ready`、`idx_exotic_tasks_item` |

##### 阅读器专表

| 表 | 来源 | 关键列 | 职责 |
|---|---|---|---|
| `text_book_index` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`src_key`(源指纹,含 mtime/size 或版本 id);`encoding`;`confidence`('bom'\|'detected'\|'manual'\|'lossy');`chapters`(JSON 章节数组) | 纯文本书籍章节索引 |
| `reader_book_prefs` | `CURRENT_SCHEMA` | `item_id` PK FK CASCADE;`prefs`(版本化 JSON,相对全局默认的差量);`updated_at` | 单本阅读器偏好覆盖 |
| `reader_bookmarks` | `CURRENT_SCHEMA` | `id` PK;`item_id` FK CASCADE;`locator`('cfi:<epubcfi>');`label`;`fraction`(0..1 进度);UNIQUE(item_id,locator) | 阅读书签。索引 `idx_reader_bookmarks_item` |

33 张表清单核对:`app_config, scan_roots, directories, media_items, image_meta, video_meta, audio_meta, document_meta, reading_progress, albums, album_items, tags, item_tags, ai_embeddings, ai_search_results, media_derivations, doc_replacements, document_versions, storage_backends, volumes, persons, faces, face_rejections, face_coverage, exotic_catalog_formats, exotic_plugins, exotic_tasks, text_book_index, reader_book_prefs, reader_bookmarks, dedup_index, dedup_index_working, directory_move_journal` —— 计 33 张,与 `schema.rs` 现码逐一核对无遗漏。37 个显式索引亦全部落在 `CURRENT_SCHEMA` 一处(本篇不复制索引 DDL 正文)。

### 2.3 Rust 数据模型

`src-tauri/src/db/models/` 定义 Rust 侧 `pub struct`/`pub enum`,与表结构或查询投影对应。非穷举,代表性映射:

| Rust 类型 | 主表 | 备注 |
|---|---|---|
| `ScanRoot` | scan_roots | 无 `backend_id` 字段(列已删) |
| `Directory` | directories | |
| `MediaItem` | media_items | 主体结构 |
| `LayoutItem` | media_items(子集) | 画廊布局查询专用投影 |
| `MediaDetail` | media_items + image_meta + video_meta | 详情面板聚合 |
| `Volume`/`VolumeKind` | volumes | |
| `PersonSummary`/`FaceBox`/`FaceThumb` | persons/faces | 人脸墙查询投影 |
| `AiStatus`/`FaceStatus` | media_items.ai_status/face_status | 枚举镜像 INTEGER 状态机 |
| `MediaFilter`/`ViewDescriptor`/`ViewScope` | (查询构建器,非直接映射单表) | 画廊筛选/排序描述符 |
| dedup / 存储投影类型 | dedup_index / dedup_index_working / storage_backends | 去重索引行与存储后端投影(去重算法见 [Spec02](./Spec02_扫描与画廊.md)) |

完整清单见 `models/` 下的 `struct`/`enum` 定义,本篇不逐一复制。

### 2.4 状态归属

- Schema 定义:仅存于代码(`db/schema.rs` 的 `CURRENT_SCHEMA` + `SCHEMA_VERSION`),不在运行时可变,也不存在运行时升级路径。
- 业务数据:全部落 SQLite 文件(WAL 模式),无内存态权威副本——前端 store 与内存缓存均为只读投影,写入必须回 DB。
- 应用级配置(主题、缩略图策略等 77 项设置):`<app_data_dir>/config.toml`,键定义单源 `config/schema.rs::SETTING_DEFS`(见 [Spec12](./Spec12_配置状态日志.md))。
- 应用状态与 UI 记忆偏好(36 项,含格式标识 `schema_version`):DB `app_config` 表,见 §2.2。

---

## 3. 关键流程与算法

### 3.1 建库与格式判别(`db/schema.rs`)

单一入口 `pub fn initialize_schema(conn: &Connection) -> Result<()>`:生产启动([boot.rs](../../src-tauri/src/db/boot.rs))与所有建库夹具都走它;`CURRENT_SCHEMA` 常量即当前格式的全部 DDL + 必需种子,源码是唯一事实源,本篇不复制 DDL 正文。

```
initialize_schema(conn):
  1. version = read_schema_version(conn)       -- SELECT app_config.schema_version;缺表/缺键/不可解析 → 0
  2. version == SCHEMA_VERSION(34) → 记 info 后返回(幂等放行,不动任何行)
  3. version != 0 || !is_fresh_database(conn)  -- is_fresh:sqlite_master 中非 sqlite_% 表数 == 0
       → Err(AppError::SchemaIncompatible)      -- 旧格式 / 缺标识 / 未来值:不改数据、不叠建
  4. 全新库:
       tx = conn.unchecked_transaction()
       tx.execute_batch(CURRENT_SCHEMA)         -- 33 表 + 37 索引 + 种子
       tx.execute("INSERT INTO app_config(key,value) VALUES('schema_version', ?1)", [SCHEMA_VERSION])
       tx.commit()                              -- 结构 + 标识同事务提交;任一步失败整块回滚,不留半成品
```

要点:

- **标识只是格式判别**,比较一律相等,不存在 `version < N` 分支:同一份代码里没有 V1..33 升级步骤、没有按年代拆分的 DDL 常量文件、没有回填 DML、没有旧 `config` 迁移。发现旧格式(或缺标识的非空库)即报错提示重置,**不自动清理用户数据与源资产**,也不在旧结构上叠建。
- 种子(`CURRENT_SCHEMA` 内):`app_config` 7 个默认键(§2.2)+ 4 个系统收藏夹(`albums`,kind='system')。两句只在新库执行一次,故无需存在性守卫。
- 建库用 `unchecked_transaction`(DEFERRED)而非把签名改为 `&mut Connection`:建库在启动期独占运行、无并发写,足够原子,且不波及调用方签名。
- 失败与崩溃语义:结构或标识任一步失败 → 整块回滚 → 库内既无表也无标识 → 下次启动仍判为全新库,可安全重试;不会出现「半套结构 + 标识已写」的悬挂态,也不存在 `duplicate column` 式的重复执行问题(没有 `ALTER` 步骤)。
- 不兼容的呈现:`AppError::SchemaIncompatible`(稳定码 `db_schema_incompatible`)。`boot.rs` 在恢复场景下另有回滚分支:换入库建库/判别失败时先 `drop(db_writer)` 释放 Windows 文件句柄,再 `rollback_restore_at_boot` 逆回原始库、重开连接重跑 `initialize_schema`;非恢复场景直接以可诊断文案 fatal。
- 测试形态(`schema.rs` 的 `#[cfg(test)] mod tests`):建库后代表表/索引存在、系统收藏夹 4 条、`scan_roots` 默认值语义、外键与 `integrity_check` 干净、重复初始化幂等且不清库、缺标识旧库与未知标识被拒且原数据不变、中途失败整体回滚。

### 3.2 自定义排序:`NATURAL_CMP` 与 `TREE_SORT_KEY`

两者均通过唯一入口 `pub fn register_custom_collations(conn: &Connection) -> rusqlite::Result<()>`(`mod.rs`)注册,调用点两处:
1. `create_write_connection()`(写连接初始化,`connection.rs:58`)
2. `ReadPoolCustomiser::on_acquire()`(每个读池连接获取时,`connection.rs:122`)

建库不再注册函数:当前建库 SQL 不调用 `TREE_SORT_KEY()`(该列取 DDL 默认值 X'',由写路径落值),`initialize_schema` 因此不依赖注册顺序。

**`NATURAL_CMP` collation**(`mod.rs:25`):
- 委托 `crate::utils::natural_sort::natural_cmp`(`src-tauri/src/utils/natural_sort.rs:40`),数字感知的自然序比较。
- 替换动机:原 `lexicmp::natural_cmp` 对 ≥20 位连续数字的文件名做 `n*10` 运算会整数溢出 panic(`mod.rs:17-18` 注释),回归测试 `natural_cmp_collation_sorts_long_digit_names_without_panic`(`mod.rs:46-79`)用 21 位数字段真实文件名验证不再 panic 且排序正确。
- 用法:`ORDER BY name COLLATE NATURAL_CMP`。
- 示例(`mod.rs:71-77` 断言):`["img2.jpg","img10.jpg"]` 按此 collation 排序为 `img2 < img10`(纯字典序会错成 `img10 < img2`)。

**`TREE_SORT_KEY(rel_path)` 标量函数**(`mod.rs:26-34`):
- 签名:`TREE_SORT_KEY(rel_path: TEXT) → BLOB`,标记 `SQLITE_DETERMINISTIC | SQLITE_UTF8`(查询优化器可折叠常量)。
- 委托 `crate::utils::path::encode_tree_sort_key`(`src-tauri/src/utils/path.rs:123`)。
- 编码规则:按 `/` 分隔每个路径段,段间以 NUL 字节分隔终止,使前序 DFS(深度优先先序遍历)顺序与 BLOB 的 memcmp(逐字节比较)顺序天然一致。
- 关键契约:SQLite BLOB `memcmp` ≡ Rust `Vec<u8>::cmp`(SQL 路径与内存态 `build_dir_rank` 用同一逻辑生成的键,两侧必须同构,否则 SQL 排序结果与内存排序结果会不一致)。
- 落地:`directories.tree_sort_key` 列是该函数结果的**持久化**(`BLOB NOT NULL DEFAULT X''`),目录写入路径落键,之后按目录树排序读列而非每次现算;也可在查询里直接用 `ORDER BY TREE_SORT_KEY(d.rel_path)`。

伪码(前序 DFS 排序键生成,对应 `encode_tree_sort_key`):

```
encode_tree_sort_key(rel_path):
    key = []
    for segment in rel_path.split('/'):
        key.extend(segment.as_bytes())
        key.push(0x00)   -- NUL 终止符,保证前缀不会被更长的同前缀段"吃掉"
    return key
```

### 3.3 连接与并发模型(`connection.rs`)

**写路径**:
- 类型 `DbWriter = Mutex<Connection>`(`connection.rs:22`),单个应用实例仅一个写连接,所有写操作靠 `Mutex` 串行化。
- 创建:`create_write_connection(db_path: &Path) -> Result<DbWriter>`(`connection.rs:51-61`):`Connection::open` → 应用 PRAGMA → 注册 collation/函数。
- 消费方:启动建库 / 格式判别(`db::boot::init`)、扫描器、富化器、派生任务 worker。

**读路径**:
- 类型 `DbPool = r2d2::Pool<SqliteConnectionManager>`(`connection.rs:21`)。
- 创建:`create_read_pool(db_path: &Path, pool_size: u32) -> Result<DbPool>`(`connection.rs:123`)。
- 池大小:调用方决定。生产桌面调用点传 **8**(`src-tauri/src/db/boot.rs` `create_read_pool(&db_path, 8)`),原因:前台 `compute_layout` + 可视区元数据 + 缩略图批读取与后台派生/AI 读取交错,连接过少会让前台排队(「布局被后台读饿死」的次要成因),WAL 下额外读连接开销低。测试路径传更小值(如 2)。
- 连接标志:`SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX | SQLITE_OPEN_URI`(`connection.rs:138-142`)。
- `min_idle(Some(0))`:延迟到首次使用才建立连接,避免冷启动 `setup()` 阶段被 4 次 SQLite open + PRAGMA 批次阻塞(`connection.rs:146-149`)。
- `ReadPoolCustomiser`(`connection.rs:116-125`)在每个连接 `on_acquire` 时应用 PRAGMA + 注册 collation/函数——**读连接同样需要 `NATURAL_CMP`/`TREE_SORT_KEY`**,因为生产查询的 `ORDER BY ... COLLATE` 实际在读池连接上执行。

**PRAGMA 配置**(`connection.rs:28-37`,写连接与每个读池连接均应用同一批,单一事实源 `const PRAGMAS`):

```sql
PRAGMA journal_mode = WAL;
PRAGMA journal_size_limit = 67108864;   -- 64 MiB WAL 上限,checkpoint 后截回
PRAGMA synchronous  = NORMAL;           -- 每次提交 fsync,比 FULL 快且崩溃安全
PRAGMA cache_size   = -64000;           -- ~64 MiB 页缓存
PRAGMA foreign_keys = ON;               -- 强制外键完整性
PRAGMA busy_timeout = 5000;             -- 5000 ms 锁等待超时
PRAGMA temp_store   = MEMORY;           -- 临时表走内存
PRAGMA mmap_size    = 268435456;        -- 256 MiB 内存映射 I/O
```

**启动期 WAL checkpoint**(`checkpoint_wal_at_boot`,`connection.rs:69-109`):
- 时机:tracing 订阅器就绪后、管线拉起前调用(不能更早,否则 `info!`/`warn!` 被静默丢弃,`connection.rs:65-68` 注释记录了此坑)。
- 动作:`PRAGMA wal_checkpoint(TRUNCATE)` 回收 WAL 文件空间。
- 阈值告警:耗时 > 500ms 或截断前 WAL > 64MB 时升级为 `warn!`,否则 `info!`(`connection.rs:95-106`)。
- 失败容忍:仅告警不阻断启动(`connection.rs:107`)。
- 已被启动探针数据证伪的假设(`connection.rs:26-27` 注释):"WAL 膨胀致热启动慢" 这一假设已经全日实测(WAL ≤ 10.4MB)推翻,该 checkpoint 现属通用卫生改动而非该问题的修复手段——**若要在其他文档复用"热启动慢"论证,须先确认此已被证伪的前提**。

### 3.4 Queries 子模块

`src-tauri/src/db/queries/` 按子系统分文件,职责边界按功能域划分(非本篇复制点):`config`(app_config 状态键)、`scan/`(scan_roots/directories/media_items 扫描与缺失标记)、`media`(CRUD/软删除)、`metadata`(EXIF/编解码等富化)、`layout/`(画廊查询构建、筛选、选区解析)、`thumbnail`(缩略图状态)、`collections`(albums)、`ai`(ai_embeddings/ai_search_results)、`faces/`(persons/faces/face_coverage/face_rejections)、`derivations`(media_derivations 任务派发)、`dedup`(dedup_index/dedup_index_working)、`move_journal`(directory_move_journal)、`exotic`(插件三表)、`documents`(document_versions/doc_replacements/reader 三表)、`storage`(storage_backends/volumes)、`export`(批量导出/批操作)。语义检索的运行时检索控制在 `ai/search_control.rs`,不属本目录。

---

## 4. 契约与不变量(施工红线)

| 不变量 | 为什么 | 违反后果 |
|---|---|---|
| rusqlite only,严禁引入第二套 DB 客户端 | 单一事实源,避免连接池/事务语义分裂 | 双客户端会绕开 `register_custom_collations`,导致 `NATURAL_CMP`/`TREE_SORT_KEY` 在旁路连接上不可用,查询直接报 `no such function`/`no such collation` |
| 每条 SQL 全参数绑定,不做字符串拼接 | 防 SQL 注入;`rusqlite::params!` 强制类型检查 | 拼接用户可控字段(路径、文件名)可致注入或引号转义错误 |
| async command 内每次 rusqlite 调用都经 `spawn_blocking` 或专用 DB 线程 | `rusqlite::Connection` 非 async,阻塞调用会卡住 tokio 执行器线程 | UI 线程/IPC 响应卡顿,严重时整个 async runtime 饿死 |
| 桌面端 WAL + `busy_timeout=5000` | WAL 允许读写并发不互斥;busy_timeout 让偶发锁冲突自动重试而非立即失败 | 关闭 WAL 会让读写互斥,失去并发读能力;busy_timeout 过短会让并发写偶发 `SQLITE_BUSY` 直接失败 |
| 写操作单连接(`Mutex<Connection>`)策略 | SQLite 单连接写足够快且避免多写连接间的锁升级复杂度 | 多写连接会引入写-写竞争,WAL 模式下仍可能因长事务互相 `SQLITE_BUSY` |
| `PRAGMA foreign_keys = ON` 全程开启 | 强制引用完整性,依赖级联删除(`ON DELETE CASCADE`)自动清理派生数据 | 关闭后孤儿行不会被拒绝插入,`media_items` 删除后子表(image_meta/faces/…)残留 |
| 结构变更 = 改 `CURRENT_SCHEMA` + 提升 `SCHEMA_VERSION`,不写升级步骤 | 建库只面向「全新库」与「当前库」两态,不存在增量迁移路径 | 若把旧库里缺失的列/索引当作「原地补上」的兼容处理,会重新引入无事务的隐式迁移与半迁移风险,并让旧库继续承载与新库不同的结构 |
| 旧格式库一律拒绝(`db_schema_incompatible`),不叠建、不清库 | 只保留当前格式一种结构,杜绝双结构共存与静默数据改写 | 「顺手兼容旧库」会让建库路径出现第二套 DDL,后续结构变更无从收敛 |
| keyset 分页复合排序键(`sort_datetime, id`)与 `id` tiebreaker,SQL `ORDER BY` 与前端排序逻辑必须一致 | 两端序不一致会导致分页跳页/重复/丢项 | 用户滚动画廊时出现重复条目或漏项 |
| `is_deleted` 与 `availability` 正交 | `is_deleted` 是用户主动软删除;`availability` 是卷驱动的在线/离线/缺失状态,两者互不影响(D-项裁决,见关联) | 混淆二者会导致"卷离线的文件被误判为已删除"或反之 |
| `ai_status` 与 `face_status` 独立状态机 | AI 语义分析与人脸检测是两条独立 pipeline,各自失败/重试互不阻塞 | 合并状态机会导致一条 pipeline 失败阻塞另一条的进度展示 |
| `view_rotation` 与 `video_meta.rotation` 正交 | 前者是用户在查看器里手动设的显示旋转,后者是视频文件内禀旋转元数据 | 混用会导致旋转角度叠加或抵消错误 |

**「不可擅改」项**:
- `NATURAL_CMP` 溢出修复的算法选择(委托 `utils::natural_sort::natural_cmp`)——回归测试 `mod.rs:46` 锁定,勿回退到会溢出的实现。
- `tree_sort_key` 的 BLOB memcmp ≡ Rust `Vec<u8>::cmp` 契约——任一侧改编码规则必须同步另一侧,否则内存/SQL 双路排序会分裂(契约文字见 `src-tauri/src/utils/path.rs::encode_tree_sort_key` 文档注释;注册处见 `src-tauri/src/db/mod.rs`)。
- `is_deleted` ⊥ `availability` 正交设计是既有裁决(`media_items` 建表注释「关键设计」段明确写明二者正交、扫描路径永不触碰 `is_deleted`),非本篇新定,勿在扩展时合并。

---

## 5. 边界与失败

- **建库失败**:结构与标识同事务,任一步失败 → 整块回滚 → 库内无表无标识 → 下次启动仍判为全新库、可安全重试;不会出现「半套结构 + 标识已写」的悬挂态。
- **格式不兼容**:旧格式库、缺标识的非空库、未来格式一律 `AppError::SchemaIncompatible`(`db_schema_incompatible`),不改动库内数据,由调用方提示用户重置;恢复场景另走 `boot.rs` 的回滚分支(见 §3.1)。
- **并发写**:全应用单写连接 + `Mutex` 串行化,不存在"两个写请求真正同时执行"的场景;写请求在 `Mutex` 上排队等待,极端情况下配合 `busy_timeout=5000` 兜底跨进程/跨连接的锁等待(如 VACUUM INTO 备份连接短暂持锁)。
- **软删除与卷离线正交**:`albums.deleted_at`、`media_items.is_deleted` 是用户驱动的可撤销删除;`scan_roots.is_hidden` 是库范围可见性开关;`media_items.availability` 是卷驱动的 online/offline/missing。三者互不改写彼此,查询层需要按需组合过滤条件(如画廊默认查询同时过滤 `is_deleted=0` 与 `availability='online'`,取决于具体视图)。
- **孤儿计数防毒任务**(`media_derivations.orphan_count`):派生任务重置计数达到阈值判定为终态 `error`,防止某类损坏输入反复触发"重置→重跑→失败→重置"的无限循环消耗系统资源。
- **跨卷/离线场景**:`volumes.is_online` 是运行时探测态而非持久权威,重启后需要重新探测;`media_items.volume_relative_path` 是重新挂载后重新关联条目的键,离线时段内条目仍保留元数据但 `availability != 'online'`。
- **失败呈现**:DB 层错误经 `AppError`(`crate::error`)向上传播,具体错误码/变体的稳定契约见 [Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)。

---

## 6. 重建指引(从零实现)

**依赖顺序**:

1. 引入依赖 crate(核实自 `src-tauri/Cargo.toml:43-45`):
   - `rusqlite = { version = "0.31", features = ["bundled", "collation", "functions"] }` —— `collation`/`functions` feature 缺一不可,否则 `create_collation`/`create_scalar_function` 不可用。
   - `r2d2 = "0.8"`
   - `r2d2_sqlite = "0.24"`
2. 先写 `schema.rs`:`CURRENT_SCHEMA`(当前格式一份完整 DDL + 种子)+ `SCHEMA_VERSION` + `read_schema_version`/`initialize_schema`。**不写任何版本的增量脚本**。
3. 再写 `connection.rs`(连接创建 + PRAGMA)与 `mod.rs`(collation/函数注册入口),`queries/*.rs` 依赖以上三者,最后实现;`models/` 可与 `queries/` 并行推进,按需补 struct。

**坑与教训**:
- **collation 必须在建连接时注册,否则查询报错**:`ORDER BY ... COLLATE NATURAL_CMP` 在未注册该 collation 的连接上会直接报 `no such collation: NATURAL_CMP`。写连接与读池每个连接都要经过 `register_custom_collations`;裸内存连接(测试)要跑依赖 `TREE_SORT_KEY`/`NATURAL_CMP` 的 SQL 时须自行注册。
- **结构变更的正确姿势**:改 `CURRENT_SCHEMA` 正文并提升 `SCHEMA_VERSION`(见 §4)。不要写「检测旧库缺列就补列」的兼容分支——那是无事务保护的隐式迁移,且会让旧库长期停留在两套结构之间。
- **启动日志时机**:`checkpoint_wal_at_boot` 必须在 tracing 初始化之后调用,否则日志静默丢失(`connection.rs:65-68`)。
- **NATURAL_CMP 溢出history**:曾用第三方 `lexicmp` crate,对 ≥20 位连续数字文件名溢出 panic,后自研 `utils::natural_sort::natural_cmp` 替换,回归测试用真实超长数字文件名锁定(`mod.rs:42-79`)。

**验收**:

| 测试 | 位置 | 覆盖 |
|---|---|---|
| `test_collation` | `connection.rs:157-194` | 共享缓存内存库验证写连接 + 读池均正确注册 `NATURAL_CMP` |
| `natural_cmp_collation_sorts_long_digit_names_without_panic` | `mod.rs:45-79` | 端到端:真实超长数字文件名 `ORDER BY COLLATE NATURAL_CMP` 不 panic 且序正确 |
| `initialize_creates_current_schema_with_defaults` | `schema.rs` `#[cfg(test)] mod tests` | 建库:代表表/索引就位、系统收藏夹 4 条、`scan_roots` 默认值、外键与完整性干净、标识写入 |
| `initialize_is_idempotent_and_keeps_existing_rows` | 同上 | 当前库重复初始化幂等、不清库 |
| `initialize_rejects_legacy_database_without_touching_data` | 同上 | 缺标识非空旧库被拒且数据不变、不叠建 |
| `initialize_rejects_unknown_format_marker` | 同上 | 历史值 / 未来标识一律不兼容、不被「修好」 |
| `initialize_rolls_back_partially_applied_schema` | 同上 | 建库中途失败整体回滚、不写标识 |
| `test_collation` | `connection.rs` | 共享缓存内存库验证写连接 + 读池均正确注册 `NATURAL_CMP` |
| `natural_cmp_collation_sorts_long_digit_names_without_panic` | `mod.rs` | 端到端:真实超长数字文件名 `ORDER BY COLLATE NATURAL_CMP` 不 panic 且序正确 |

Gate 命令:`cargo test --package scrollery --lib db::`(数据层单测子集);全量门禁参见 `.github/workflows/ci.yml` 的 Rust 检查 job(broad/release 场景才需全量跑,详见 AGENTS.md 项目规则)。

---

## 7. 关联

- 上游计划:[Part1 数据层](../refactor_2026/Part1_数据层.md) —— 记录了卷可用性模型(卷 / 存储后端分离、`availability` 三态、keyset 分页的性能理由)与迁移事务化的历史背景;其中按版本重放的实施指令已由本篇的单一 `CURRENT_SCHEMA` 取代(该文已标注退役)。
- 相关规格篇:[Spec12 配置状态日志](./Spec12_配置状态日志.md)(77 项设置 / 36 项状态键的路由与 `config.toml` 真源)、[Spec08 存储备份导出文件操作](./Spec08_存储备份导出文件操作.md)(备份包对 `SCHEMA_VERSION` 的校验与恢复交换)、[Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)(DB 层错误如何映射为 IPC 错误码)。

---

## 交付自检记录

- 数据模型 / 建库 / 连接 / 排序各节按 `db/schema.rs`、`db/boot.rs`、`db/connection.rs`、`db/mod.rs` 与 `config/schema.rs` 核对;表清单计 33 张、显式索引计 37 个。
- 结构与种子不在本篇复制 DDL 正文:`db/schema.rs` 的 `CURRENT_SCHEMA` 是单一事实源,本篇只描述形状、约束语义与建库流程。
- **2026-09-15 消融更新**:删除 V1..33 迁移引擎、按年代拆分的 DDL 常量文件与旧 `config` 迁移叙述;`SCHEMA_VERSION` 只作格式判别;删除已落地的 `scan_roots.backend_id`、`exotic_plugins.entitlement_source` 两列声明与对应外键叙述;`storage_backends` 作为连接管理表保留;设置真源为 77 项 `config.toml`、状态键 36 项。
- 术语首次出现有定义(WAL、PRAGMA、collation、DFS、keyset 分页等);中文叙述、英文术语。
