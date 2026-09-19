# Picasa Next — Implementation Plan v1.2

> 📦 **已归档(2026-07-10 文档治理)**:refactor_2026 之前的初代总纲(母设计),已被 Part0–8 整体取代(Part0 §0.3 降历史参考)。现行架构 → refactor_2026/Part0_总纲与产品定稿.md。

> **产品定位**：面向 15万+ 媒体文件的本地高性能资产浏览器及管理工具
> **核心能力**：统一管理**图片、视频、音频、文本/文档**四大类媒体内容
> **技术路线**：Rust (Tauri V2) + Vue 3 (Vite + TS)，极限性能，跨平台就绪
> **开发模式**：个人开发者适用，兼顾工程化基础与开发效率
> **开发平台**：Windows 11

---

## 一、格式支持

### 1.1 四大媒体类型

- **图片 (`image`)**：Justified Layout + 大图预览 + EXIF + 动态照片 → **Phase 1 核心**
- **视频 (`video`)**：关键帧缩略图 + 时长角标 + 内建播放器 → Phase 2
- **音频 (`audio`)**：封面提取 + 时长角标 + 内建播放器 + 标签元数据 → Phase 2
- **文档 (`document`)**：首页渲染 (PDF/SVG) + 类型图标 + 预览/外部打开 → Phase 2

### 1.2 Phase 1 图片格式

| 格式 | 扩展名 | 引擎 |
|------|--------|------|
| JPEG | .jpg, .jpeg | ImageRsEngine |
| PNG | .png | ImageRsEngine |
| WebP | .webp | ImageRsEngine |
| BMP | .bmp | ImageRsEngine |
| GIF | .gif (取第一帧) | ImageRsEngine |
| TIFF | .tif, .tiff | ImageRsEngine |

### 1.3 Phase 1 动态照片

- **Apple Live Photo**：.jpg/.jpeg + .mov 配对，文件名茎匹配 + EXIF ContentIdentifier
- **Google Motion Photo**：单个 .jpg (嵌入 MP4)，XMP `GCamera:MotionPhoto=1`
- **Samsung Motion Photo**：单个 .jpg (嵌入 MP4)，文件尾部标记 / XMP

### 1.4 Phase 2 扩展格式

- **HEIC/AVIF** (.heic, .heif, .avif) → HeicEngine (`libheif-rs`)
- **RAW** (.cr2, .cr3, .nef, .arw, .dng, .raf, .orf, .rw2, .pef, .srw) → RawEngine (`rawler`)
- **PSD** (.psd, 扁平合成层) → ImageRsEngine（归为图片扩展，非 Phase 1 核心）
- **视频** (.mp4, .m4v, .mov, .avi, .mkv, .webm, .wmv, .flv, .mpg, .mpeg, .3gp, .3g2, .ts, .mts, .m2ts, .ogv, .asf) → FFmpeg Sidecar
- **音频** (.mp3, .flac, .wav, .aac, .m4a, .ogg, .oga, .opus, .wma, .aiff, .aif, .ape, .alac) → `lofty`
- **文档** PDF (.pdf) → mupdf；SVG (.svg) → resvg；Office/文本/其他 → 类型图标占位

---

## 二、核心决策汇总

| # | 决策项 | 方案 |
|---|--------|------|
| Q1 | 哈希算法 | `xxHash3` (xxh3_64)，`cache_key` 为 INTEGER (i64) 按位重解释 |
| Q2 | 主题色分析 | Phase 2，缩略图生成时顺带 MMCQ 提取 |
| Q3 | 缩略图格式 | WebP 优先 → JPEG 降级 |
| Q4 | 两阶段扫描 | **快速扫描**（`image_dimensions` + Orientation 标签，≤3s/万张）→ 立即出 UI；**后台充实**（EXIF/XMP/动态照片，静默） |
| Q5 | EXIF 库 | `kamadak-exif`，纯 Rust |
| Q6 | XMP 解析 | `quick-xml` 手动解析（动态照片检测） |
| Q7 | 目录树 | `parent_id` + `depth` + `name` 递归树 |
| Q8 | HEIC/RAW | `libheif-rs` / `rawler`，Phase 2 |
| Q9 | 数据库驱动 | 仅 `rusqlite`，统一读写 |
| Q10 | 主题系统 | CSS Variables + `data-theme`，Light/Dark/System 三态 |
| Q11 | 连接模型 | 写 `Mutex<Connection>` + 读 `r2d2` 连接池（桌面 4 连接，移动端 2） |
| Q12 | 路径存储 | 相对路径 + 锚点架构 |
| Q13 | 缓存键 | `xxh3_64("{rel_path}/{file_name}\|{file_mtime}")` → i64，不含尺寸；文件名 hex 用`format!("{:016x}", cache_key as u64)`避免负号 |
| Q14 | 缩略图缓存 | **按尺寸分桶**：`cache/thumbnails/{size}/xx/xxxx.webp` |
| Q15 | 布局策略 | **后端计算 Justified Layout + 行级分段加载** ；布局缓存附带 
`layout_version: u64`
 版本号，计算时原子递增，
`get_layout_rows`
 携带版本校验防止竞争 |
| Q16 | 虚拟滚动 | 自研行级虚拟化，配合后端行数据 |
| Q17 | 图片预光栅化 | `Image.decode()` 消除滚动掉帧 |
| Q18 | 大图预览 | 模态覆盖层，CSS `transform` 缩放/拖拽 |
| Q19 | 数据表拆分 | 主表 `media_items` + 4 个扩展表 (`image_meta`/`video_meta`/`audio_meta`/`document_meta`) |
| Q20 | 动态照片 | Phase 1 实现，配对+嵌入检测，视频延迟提取 |
| Q21 | 视频播放 | HTML5 `<video>` + `convertFileSrc()` |
| Q22 | 音频元数据 | `lofty` crate |
| Q23 | 窗口持久化 | `tauri-plugin-window-state` + `app_config` |
| Q24 | 搜索 | Phase 1 文件名 LIKE 搜索，Phase 3 FTS5 |
| Q25 | 任务边界 | `tokio` 负责异步 IO/IPC，`rayon` 负责 CPU 并行，`spawn_blocking` 桥接 |

---

## 三、技术栈

### 3.1 后端 (Rust / Tauri V2)

**Phase 1**：`tauri ^2`, `rusqlite ^0.31 (bundled)`, `r2d2 + r2d2_sqlite`, `kamadak-exif`, `quick-xml`, `xxhash-rust ^0.8`, `image ^0.25`, `fast_image_resize`, `thumbhash`, `tokio ^1 (full)`, `walkdir ^2`, `serde + serde_json ^1`, `tracing + tracing-subscriber`, `thiserror`, `rayon`, `tokio-util ^0.7`

**Phase 2**：`trash ^5`, `libheif-rs ^1.0`, `rawler`, `color-thief`, FFmpeg (Sidecar), `lofty`, `mupdf (mupdf-rs)`, `resvg`

**Phase 3+**：`notify ^7`, `blake3 ^1.5`

### 3.2 前端 (Vue 3 + TypeScript)

Vue 3 (Composition API) + Pinia + Vue Router 4 + Vite + TypeScript strict + `@tauri-apps/api` + Vanilla CSS + CSS Variables

### 3.3 Tauri 插件

`tauri-plugin-dialog`（目录选择）、`tauri-plugin-fs`（convertFileSrc）、`tauri-plugin-shell`（资源管理器 + FFmpeg sidecar）、`tauri-plugin-window-state`（窗口状态持久化）

### 3.4 tokio / rayon 任务边界

- **tokio**：文件 I/O、IPC 命令处理、Channel 推送、定时器、文件监听事件
- **rayon**：缩略图批量生成、后台 EXIF 批量解析、布局计算
- **桥接**：IPC 命令中通过 `tokio::task::spawn_blocking(|| { rayon::scope(...) })` 进入 rayon 上下文；禁止在 rayon 线程池内 `.await`

---

## 四、项目目录结构

```
picasa-next/
├── src-tauri/
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── icons/
│   ├── binaries/                       # Sidecar (Phase 2: ffmpeg)
│   └── src/
│       ├── main.rs                     # Desktop 入口
│       ├── lib.rs                      # 模块声明
│       ├── error.rs                    # 统一错误类型（Serialize 结构体）
│       ├── state.rs                    # AppState
│       ├── db/
│       │   ├── mod.rs
│       │   ├── connection.rs           # 写连接 + 读连接池 + PRAGMA
│       │   ├── migration.rs            # 版本化迁移
│       │   ├── schema.rs              # 建表 SQL
│       │   ├── models.rs
│       │   └── queries.rs
│       ├── scanner/
│       │   ├── mod.rs
│       │   ├── fast_scan.rs           # 快速扫描（dimensions + orientation）
│       │   ├── enricher.rs            # 后台充实（EXIF/XMP/动态照片）
│       │   ├── walker.rs              # 递归遍历 + 格式分类
│       │   ├── metadata.rs            # EXIF / XMP 解析
│       │   ├── live_photo.rs          # 动态照片检测
│       │   └── watcher.rs             # Phase 3
│       ├── thumbnail/
│       │   ├── mod.rs
│       │   ├── generator.rs           # 统一入口（按 media_type 分发）
│       │   ├── exif_thumb.rs
│       │   ├── cache.rs              # 尺寸分桶缓存
│       │   └── thumbhash.rs
│       ├── engine/
│       │   ├── mod.rs
│       │   ├── traits.rs
│       │   ├── image_rs.rs
│       │   ├── heic.rs               # Phase 2
│       │   └── raw.rs                # Phase 2
│       ├── layout/
│       │   ├── mod.rs
│       │   ├── justified.rs          # Justified Layout 算法（Rust 侧）
│       │   └── cache.rs             # 布局缓存
│       ├── video/                     # Phase 2
│       │   ├── mod.rs
│       │   ├── ffmpeg.rs
│       │   ├── frame_extractor.rs
│       │   └── metadata.rs
│       ├── audio/                     # Phase 2
│       │   ├── mod.rs
│       │   ├── metadata.rs
│       │   └── cover_art.rs
│       ├── document/                  # Phase 2
│       │   ├── mod.rs
│       │   ├── pdf_thumb.rs
│       │   └── svg_render.rs
│       ├── color/                     # Phase 2
│       │   ├── mod.rs
│       │   └── extractor.rs
│       ├── ipc/
│       │   ├── mod.rs
│       │   ├── scan_commands.rs
│       │   ├── media_commands.rs
│       │   ├── layout_commands.rs     # 布局计算 + 行级加载
│       │   ├── thumbnail_commands.rs
│       │   ├── search_commands.rs     # 搜索
│       │   ├── system_commands.rs
│       │   └── config_commands.rs
│       └── utils/
│           ├── mod.rs
│           ├── hash.rs
│           ├── path.rs               # normalize_db_path / resolve_media_path
│           └── format.rs             # 格式检测 + 四大类型分类
├── src/
│   ├── App.vue
│   ├── main.ts
│   ├── env.d.ts
│   ├── assets/styles/
│   │   ├── index.css
│   │   ├── variables.css
│   │   ├── theme-dark.css
│   │   ├── theme-light.css
│   │   ├── reset.css
│   │   └── animations.css
│   ├── components/
│   │   ├── layout/
│   │   │   ├── AppShell.vue
│   │   │   ├── AppSidebar.vue        # 拖拽调整宽度
│   │   │   ├── AppToolbar.vue        # 视图切换、排序、搜索(150ms debounce)、类型筛选芯片
│   │   │   └── AppStatusBar.vue
│   │   ├── media/
│   │   │   ├── MediaGrid.vue
│   │   │   ├── MediaCard.vue         # 角标系统
│   │   │   ├── MediaDetail.vue
│   │   │   ├── ImageViewer.vue
│   │   │   ├── VideoPlayer.vue       # Phase 2
│   │   │   ├── AudioPlayer.vue       # Phase 2
│   │   │   ├── DocumentViewer.vue    # Phase 2
│   │   │   └── DateSeparator.vue     # 全宽独立行，固定 36px
│   │   ├── sidebar/
│   │   │   ├── FolderTree.vue        # 无目录时内嵌一键「添加文件夹」按钮（与主区域 EmptyState 联动）
│   │   │   ├── SmartAlbums.vue
│   │   │   └── ColorFilter.vue       # Phase 2
│   │   └── common/
│   │       ├── EmptyState.vue         # 空状态（图标+文字+操作按钮）
│   │       ├── ProgressBar.vue
│   │       ├── ThemeToggle.vue
│   │       ├── ErrorBoundary.vue
│   │       ├── ContextMenu.vue        # Phase 3
│   │       └── Toast.vue
│   ├── composables/
│   │   ├── useVirtualScroll.ts
│   │   ├── useJustifiedLayout.ts      # 前端行级消费 + 滚动状态
│   │   ├── useRequestQueue.ts
│   │   ├── useThumbnail.ts
│   │   ├── useMediaDetail.ts          # 组件级 composable
│   │   ├── useFolderTree.ts
│   │   ├── useTheme.ts
│   │   ├── useSelection.ts           # Phase 3
│   │   ├── useSidebarResize.ts
│   │   └── useColorFilter.ts         # Phase 2
│   ├── stores/
│   │   ├── mediaStore.ts
│   │   ├── scanStore.ts
│   │   ├── uiStore.ts                # 状态持久化
│   │   └── filterStore.ts
│   ├── constants/
│   │   ├── formats.ts
│   │   ├── defaults.ts
│   │   └── ipc.ts
│   ├── types/
│   │   ├── media.ts
│   │   ├── layout.ts
│   │   ├── ipc.ts
│   │   └── ui.ts
│   ├── router/index.ts
│   └── utils/
│       ├── thumbhash.ts
│       └── format.ts
├── index.html
├── vite.config.ts
├── tsconfig.json
├── package.json
└── README.md
```

---

## 五、数据库设计

### 5.1 PRAGMA

每条连接（写连接 + 读连接池每条连接通过 `r2d2::CustomizeConnection::on_acquire`）初始化时执行：

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;
```

### 5.2 连接架构

```rust
pub struct AppState {
    pub db_writer: Mutex<Connection>,                    // 写序列化
    pub db_read_pool: Pool<SqliteConnectionManager>,     // WAL 多读并发
    pub scan_tokens: Mutex<HashMap<i64, CancellationToken>>,
    pub layout_cache: RwLock<Option<LayoutCacheData>>,   // 布局缓存
}
```

读连接池大小：桌面端 4 连接，移动端 2 连接（通过编译时 feature 或运行时配置）。读连接以 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` 打开。

### 5.3 表结构

#### `app_config` — 键值配置

```sql
CREATE TABLE IF NOT EXISTS app_config (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
-- 默认值：schema_version=1, thumb_size=300, thumb_format=webp,
-- thumb_quality=80, thumb_skip_max_kb=200, theme=system,
-- last_directory_id='', last_sort_by=sort_datetime,
-- last_sort_order=desc, sidebar_width=260
```

#### `scan_roots` — 路径锚点

```sql
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
```

#### `directories` — 递归树（相对路径）

```sql
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
CREATE INDEX idx_dir_root   ON directories(root_id);
CREATE INDEX idx_dir_parent ON directories(parent_id);
```

#### `media_items` — 核心主表（通用字段）

```sql
CREATE TABLE IF NOT EXISTS media_items (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    directory_id    INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,

    file_name       TEXT    NOT NULL,
    file_size       INTEGER NOT NULL,
    file_mtime      INTEGER NOT NULL,
    file_format     TEXT    NOT NULL,             -- 小写扩展名

    media_type      TEXT    NOT NULL DEFAULT 'image',
    width           INTEGER NOT NULL,             -- 快速扫描阶段由 image_dimensions() 获取
    height          INTEGER NOT NULL,             -- 后台充实阶段按 Orientation 矫正
    duration_ms     INTEGER,                      -- 视频/音频时长（角标展示用）

    sort_datetime   INTEGER NOT NULL,             -- 快扫=file_mtime，充实后=COALESCE(exif_datetime, file_mtime)
    cache_key       INTEGER NOT NULL,             -- xxh3_64 → as i64

    thumb_status    INTEGER NOT NULL DEFAULT 0,   -- 0=待生成 1=已生成 2=失败 3=源文件直显
    thumb_path      TEXT,
    thumbhash       BLOB,

    is_favorited    INTEGER NOT NULL DEFAULT 0,
    is_deleted      INTEGER NOT NULL DEFAULT 0,
    deleted_at      INTEGER,
    rating          INTEGER DEFAULT 0,

    is_live_photo   INTEGER DEFAULT 0,
    has_embedded_video INTEGER DEFAULT 0,
    companion_of    INTEGER REFERENCES media_items(id) ON DELETE SET NULL,

    content_hash    TEXT,                          -- Phase 4: BLAKE3

    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),

    UNIQUE(directory_id, file_name)
);

CREATE INDEX idx_media_directory ON media_items(directory_id);
CREATE INDEX idx_media_sort      ON media_items(sort_datetime DESC)
                                  WHERE is_deleted = 0 AND companion_of IS NULL;
CREATE INDEX idx_media_cache_key ON media_items(cache_key);
CREATE INDEX idx_media_format    ON media_items(file_format);
CREATE INDEX idx_media_type      ON media_items(media_type) WHERE is_deleted = 0;
CREATE INDEX idx_media_thumb     ON media_items(thumb_status) WHERE thumb_status != 1;
CREATE INDEX idx_media_fav       ON media_items(is_favorited)
                                  WHERE is_favorited = 1 AND is_deleted = 0;
CREATE INDEX idx_media_del       ON media_items(is_deleted) WHERE is_deleted = 1;
CREATE INDEX idx_media_rating    ON media_items(rating) WHERE is_deleted = 0 AND rating > 0;
CREATE INDEX idx_media_hash      ON media_items(content_hash) WHERE content_hash IS NOT NULL;
CREATE INDEX idx_media_companion ON media_items(companion_of) WHERE companion_of IS NOT NULL;
CREATE INDEX idx_media_live      ON media_items(is_live_photo) WHERE is_live_photo = 1;
```

> **图片宽高**：快速扫描阶段统一由 `image::image_dimensions()` 读取文件头获取（~0.1ms，仅解析头部不解码像素）。后台充实阶段读取 EXIF Orientation，若需旋转则交换 width/height 并更新 DB。

> **默认宽高**：音频 400×400（方形封面），PDF 文档 595×842（A4），其他文档 400×400。

> **伴侣过滤**：所有面向用户的查询追加 `AND companion_of IS NULL`。

#### `image_meta` — 图片扩展

```sql
CREATE TABLE IF NOT EXISTS image_meta (
    item_id         INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    orientation     INTEGER DEFAULT 1,

    exif_datetime   INTEGER,
    exif_make       TEXT,
    exif_model      TEXT,
    exif_lens       TEXT,
    exif_focal_length REAL,
    exif_aperture   REAL,
    exif_shutter    TEXT,
    exif_iso        INTEGER,
    exif_gps_lat    REAL,
    exif_gps_lng    REAL,

    dominant_hue    INTEGER,                      -- Phase 2
    dominant_sat    INTEGER,
    dominant_lum    INTEGER,
    dominant_hex    TEXT,
    is_monochrome   INTEGER DEFAULT 0
);

CREATE INDEX idx_img_hue ON image_meta(dominant_hue, is_monochrome)
                          WHERE dominant_hue IS NOT NULL;
```

#### `video_meta` — 视频扩展

```sql
CREATE TABLE IF NOT EXISTS video_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    video_codec  TEXT
);
```

#### `audio_meta` — 音频扩展

```sql
CREATE TABLE IF NOT EXISTS audio_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    audio_codec  TEXT,
    artist       TEXT,
    album_title  TEXT,
    track_title  TEXT
);
CREATE INDEX idx_audio_artist ON audio_meta(artist) WHERE artist IS NOT NULL;
```

#### `document_meta` — 文档扩展

```sql
CREATE TABLE IF NOT EXISTS document_meta (
    item_id      INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    page_count   INTEGER,
    doc_subtype  TEXT     -- 'pdf','svg','office','text','other'
);
```

#### `albums` / `album_items`（Phase 3）

```sql
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
    album_id   INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    item_id    INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    sort_order INTEGER DEFAULT 0,
    added_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (album_id, item_id)
);
```

#### `tags` / `item_tags`（Phase 3）

```sql
CREATE TABLE IF NOT EXISTS tags (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    color      TEXT,
    parent_id  INTEGER REFERENCES tags(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS item_tags (
    item_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (item_id, tag_id)
);
```

### 5.4 数据库迁移

版本号递增式迁移：读 `app_config.schema_version`，逐级执行 `if version < N` 块。每次 schema 变更添加一个迁移块，启动时自动执行，开发中无需手动删库。

### 5.5 两阶段扫描架构

#### 阶段 1：快速扫描（阻塞式，立即出 UI）

每个文件仅执行以下轻量操作：

1. `walkdir` 遍历 + `file_stat` (mtime/size) + 扩展名分类
2. `image::image_dimensions()` → 读文件头获取宽高（~0.1ms/张，仅解析头部不解码）
   - ⚠️ **TIFF 保护**：TIFF 头解析需读取更多字节，对 `.tif/.tiff` 文件加 50ms 超时保护（`tokio::time::timeout`），超时则 width/height 置 0，后台充实阶段重新获取
3. JPEG 额外读取 EXIF Orientation 标签（位于文件头 1KB 以内，~0.1ms/张）→ 若需旋转则交换 width/height
4. `cache_key` 生成 (xxh3_64)
5. 批量 INSERT 主表（`sort_datetime = file_mtime`，`image_meta` 暂不写入）

**10,000 张 + rayon 8 线程 ≈ 1-3 秒**。完成后立即触发 `compute_layout`，用户看到完整网格。

#### 阶段 2：后台充实（非阻塞，静默逐批处理）

快速扫描完成后自动启动后台任务：

1. 全量 EXIF 解析 (`kamadak-exif`) → 写入 `image_meta` 扩展表
2. XMP Motion Photo 检测 (`quick-xml`) → 更新 `is_live_photo` / `has_embedded_video`
3. Live Photo 配对（文件名茎匹配）→ 更新 `companion_of`
4. `sort_datetime` 校正为 `COALESCE(exif_datetime, file_mtime)`
5. 每处理完一批（500 项）→ 发送 `db:media_enriched` 事件
6. 全部完成 → 发送 `enrichment:completed` 事件 → 前端触发 `compute_layout` 重算（sort_datetime 可能变化）

后台充实不阻塞 UI，用户在充实过程中可正常浏览、滚动、预览。充实完成后 LIVE 角标自动出现，排序自动校正。

#### 增量扫描策略

| 场景 | 检测方式 | 处理 |
|------|----------|------|
| 新文件 | `UNIQUE(directory_id, file_name)` INSERT 成功 | 快扫插入主表 → 后台充实 |
| 文件未变 | `file_mtime` 未变 | 跳过 |
| 文件已修改 | `file_mtime` 变化 | 更新主表 + 重新充实 + 重生成缩略图 + 更新 cache_key |
| 文件被删除 | 扫描后 DB 中有、磁盘上无 | 标记 `is_deleted=1` 或删除记录（可配置） |
| 文件重命名 | 旧名不存在 + 新名出现 | 按新文件处理 |

### 5.6 Re-scan 策略

移除 `scan_root` → CASCADE 删除所有 `directories` 和 `media_items`（含扩展表）。重新添加同路径 → 新 ID + 全量重扫。缩略图缓存以 `cache_key` 索引，不受影响。

### 5.7 路径工具

- `normalize_db_path`：统一正斜杠。
- `resolve_media_path(root, rel, name)`：运行时拼接 `PathBuf`。
- `resolve_media_path_by_id(conn, id)`：JOIN 查询后拼接。
- **空 rel_path 处理**：文件直接在 root 下时 `rel_path=""`，`cache_key` 输入为 `"/file_name\|mtime"`（前导 `/` 不影响哈希唯一性，保持一致即可）。

### 5.8 格式分类

`classify_media_type(ext) -> Option<MediaType>` 按扩展名返回 `Image|Video|Audio|Document`。Phase 1 扫描仅处理 `Image` + 动态照片 MOV 伴侣，其余格式已注册，Phase 2 启用开关后无缝支持。PSD 归入 Phase 2 图片扩展。

---

## 六、IPC 架构

### 6.1 Tauri Command

#### 扫描管理 (`scan_commands.rs`)

- `add_scan_root({ path })` → `ScanRoot`
- `remove_scan_root({ id })` → void
- `list_scan_roots()` → `ScanRoot[]`
- `start_scan({ root_id, on_progress: Channel })` → void（异步，两阶段：快速扫描完成即返回进度 100%，后台充实自动启动）
- `stop_scan({ root_id })` → void（同时取消快扫和充实）

#### 布局计算 (`layout_commands.rs`) — 行级分段加载

- `compute_layout({ directory_id?, filters?, container_width, row_height, gap })` → `{ total_rows, total_height }`
  - 后端查询全部符合条件的 `media_items`（仅 id/width/height/sort_datetime/media_type/is_live_photo/duration_ms）
  - 执行 Justified Layout 算法，按 `sort_datetime` 跨日期边界插入 DateSeparator 行
  - 结果缓存在 `AppState.layout_cache`（RwLock）
  - 参数变化时自动替换缓存

- `get_layout_rows({ start_row, end_row })` → `LayoutRow[]`
  - 从缓存读取指定行范围
  - 每行包含：`{ y, height, row_type, items?, separator_label? }`
  - `row_type = 'normal' | 'separator'`
  - normal 行的 items：`[{ id, x, w, h, media_type, is_live_photo, duration_ms?, thumb_status, thumb_path?, thumbhash? }]`
  - separator 行：`{ separator_label: "2024年3月15日" }`

#### 媒体查询 (`media_commands.rs`)

- `get_media_detail({ id })` → `MediaDetail`（完整信息 + 扩展表数据 + 绝对路径）
- `get_companion_video_url({ item_id })` → string
- `toggle_favorite({ item_id })` → void
- `set_rating({ item_id, rating })` → void
- `soft_delete_items({ item_ids })` → void
- `restore_items({ item_ids })` → void
- `get_trash({ offset, limit })` → `MediaPage`
- `get_stats()` → `AppStats`（含各类型计数）
- `get_directory_tree({ root_id })` → `DirNode[]`
- `get_directory_children({ parent_id })` → `DirNode[]`（懒加载）

#### 搜索 (`search_commands.rs`)

- `search_media({ query, directory_id?, filters?, limit })` → `SearchResult[]`
  - Phase 1：`file_name LIKE '%query%'`（参数绑定）
  - 返回：`[{ id, file_name, media_type, thumb_path, thumbhash }]`
  - Phase 3：迁移到 FTS5 全文搜索
  - **前端节流**：搜索框输入需加 150ms debounce，避免每次击键触发 IPC（在 `AppToolbar.vue` 或 `useSearch` composable 中实现）

#### 缩略图 (`thumbnail_commands.rs`)

- `batch_request_thumbnails({ item_ids, size? })` → `ThumbResult[]`（**主用接口**，攒 16-32 个）
- 返回结果按请求 `item_ids` 的原始顺序排列，前端可直接按索引对齐显示，无需重新排序
- `request_thumbnail({ item_id, size? })` → `ThumbResult`（单项补充）

`size` 参数默认为 `app_config.thumb_size`（300），用于尺寸分桶。

#### 系统/配置

- `show_in_explorer({ item_id })` → void
- `move_to_trash({ item_ids })` → void（Phase 2，`trash` crate）
- `get_app_config({ key })` → string?
- `set_app_config({ key, value })` → void

#### `filters` 参数结构

```typescript
interface MediaFilter {
  mediaTypes?: ('image' | 'video' | 'audio' | 'document')[]
  livePhotoOnly?: boolean
  favoritedOnly?: boolean
  minRating?: number
  dateRange?: { from: number; to: number }
  hueBuckets?: number[]  // Phase 2
}
```

后端 SQL 动态拼接（参数绑定），`mediaTypes` 空数组 = 全部。

### 6.2 Tauri Channel / Event（流式推送）

**快速扫描阶段**（Channel 推送）：
- `ScanProgressPayload { root_id, scanned, total, current_dir }` — 每 500 项或每秒
- `ScanCompletedPayload { root_id, total_items, elapsed_ms }` — 快扫完成，UI 可交互
- `ScanErrorPayload { root_id, error }`

**后台充实阶段**（Event 推送）：
- `db:media_enriched → { root_id, enriched_count, total }` — 每批 500 项；前端 StatusBar 展示「正在充实 EXIF... 12,453 / 50,000」
- `enrichment:completed → { root_id, elapsed_ms }` — 全部充实完成，前端触发 `compute_layout` 重算

**通用 Event**：
- `db:media_updated → { action, item_ids }` — 触发布局缓存失效

### 6.3 媒体获取

所有媒体文件（缩略图、原图、视频、音频）通过 `convertFileSrc(absolutePath)` 直读磁盘。

### 6.4 错误传递

IPC 错误使用可序列化结构体：`{ code: string, message: string }`，前端可按 `code` 差异化处理（如 `DB_CORRUPTED` 触发全屏错误页）。

---

## 七、媒体处理引擎

### 7.1 ImageEngine trait

```rust
pub trait ImageEngine: Send + Sync {
    fn name(&self) -> &str;
    fn supported_formats(&self) -> &[&str];
    fn can_handle(&self, format: &str) -> bool { self.supported_formats().contains(&format) }
    fn decode(&self, file_path: &Path) -> Result<DecodedImage, EngineError>;
    fn extract_embedded_thumb(&self, file_path: &Path) -> Result<Option<Vec<u8>>, EngineError> { Ok(None) }
}
```

- `ImageRsEngine`：jpg/jpeg/png/webp/bmp/gif/tiff → Phase 1
- `HeicEngine`：heic/heif/avif → Phase 2
- `RawEngine`：cr2/cr3/nef/arw/dng/raf/orf/rw2/pef/srw → Phase 2

EngineArena 按格式分发，Phase 2 引擎失败时降级到可用引擎。

### 7.2 EXIF Orientation

- **快速扫描阶段**：仅读取 Orientation 标签（文件头 1KB 以内），若值为 5-8（需旋转 90°/270°）则交换 `width/height` 存入 DB，确保布局比例正确
- **缩略图生成阶段**：完整解码后按 Orientation 矫正像素再缩放，否则缩略图和 ThumbHash 会横躺/颠倒

### 7.3 XMP 解析

`kamadak-exif` 不支持 XMP。动态照片检测（**后台充实阶段**）通过 `quick-xml` 读取文件前 128KB 搜索 `GCamera:MotionPhoto="1"` / Samsung 标记。仅用于标记检测，不需完整 XMP 库。

### 7.4 Phase 2 引擎

- **视频**：FFmpeg Sidecar 帧提取 (`ffmpeg -ss 1 -i <path> -vframes 1 ...`) + ffprobe 元数据
- **音频**：`lofty` crate 封面提取 + 元数据解析
- **文档**：PDF → `mupdf` 首页渲染；SVG → `resvg` 光栅化；其他 → 类型图标占位

---

## 八、缩略图流水线

### 8.1 统一生成入口 (`generator.rs`)

1. 路径拼接 → 缓存命中检测（按 cache_key + size 桶查找）→ 命中直接返回
2. **小文件直显判定**：`file_size <= thumb_skip_max_kb * 1024`（默认 200KB，用户可配置）
   - 命中 → 跳过缩略图生成，`thumb_status=3, thumb_path=NULL`
   - 仍生成 ThumbHash（小图解码极快，占位图仍需要）
   - `get_layout_rows` 返回时检测 status=3 → 解析源文件绝对路径作为 thumb URL
3. 按 `media_type` 分发：
   - **image**：EXIF 快速路径 → 或 Engine 完整解码 → Orientation 矫正 → `fast_image_resize` → WebP(80)/JPEG(85) 降级
   - **video** (Phase 2)：FFmpeg 帧提取 → 缩放 → WebP
   - **audio** (Phase 2)：lofty 封面提取 → 有封面则缩放 / 无封面则音符图标占位
   - **document** (Phase 2)：PDF mupdf 首页 / SVG resvg / 其他图标占位
4. ThumbHash 生成（缩放后像素 → 100×100 以内 → ~28 bytes）
5. DB 更新：`thumb_status=1, thumb_path=..., thumbhash=<blob>`

### 8.2 缓存目录（尺寸分桶）

```
{app_data_dir}/cache/
├── thumbnails/
│   ├── 300/                  # 默认尺寸
│   │   ├── a3/
│   │   │   └── a3f4b2c1d0e9f7a1.webp
│   │   └── ...
│   └── 600/                  # 未来大尺寸缩略图
│       └── ...
└── motion_videos/            # 嵌入式动态照片提取的视频
    └── c2/
        └── c2a1b3f4e5d6c7a8.mp4
```

`thumb_path` 存储为 `{size}/{两位前缀}/{cache_key_hex}.webp`。切换缩略图尺寸时，新尺寸桶中无缓存的项自动触发重新生成，旧尺寸桶可保留或定期清理。

### 8.3 批量策略

前端请求队列攒 16-32 个 `item_id` 后调用 `batch_request_thumbnails`，单次 IPC 批量处理。Rust 端用 rayon 并行生成。每次 IPC 开销 ~0.5ms，批量减少 94% round-trip。快速滚动时取消离开视口的请求。

---

## 九、动态照片（Phase 1）

### 9.1 检测流程（后台充实阶段执行）

动态照片检测在**后台充实阶段**完成（非快速扫描），不影响首次出图速度：

1. **配对检测 (Apple Live Photo)**：充实阶段收集同目录 .mov 文件，文件名茎匹配。主体设 `is_live_photo=1`，伴侣设 `companion_of=主体.id`（网格自动隐藏）。
2. **嵌入检测 (Google/Samsung)**：XMP 解析阶段检查 `GCamera:MotionPhoto` / Samsung 标记。设 `is_live_photo=1, has_embedded_video=1`。**仅标记不提取**。
3. 充实完成 → `enrichment:completed` 事件 → 前端刷新 → LIVE 角标自动出现。

### 9.2 嵌入视频延迟提取

首次交互时（MediaCard hover 1s / MediaDetail 点击 LIVE 按钮）→ 请求 `get_companion_video_url` → Rust 检测 `cache/motion_videos/` → 未缓存则从 JPEG 尾部提取 MP4 → 写入缓存 → 返回 `convertFileSrc` URL。首次延迟 ~50-200ms，后续直读缓存。

### 9.3 前端交互

- **MediaCard**：左上角 `LIVE` 角标；悬停 1 秒自动播放短视频（静音循环）
- **MediaDetail**：默认静态图；点击 LIVE 按钮 → `<video>` 覆盖播放
- `get_companion_video_url` 统一处理配对式和嵌入式

---

## 十、前端渲染架构

### 10.1 行级分段加载

Justified Layout 计算在 Rust 后端完成，前端仅按需加载可视行：

```
1. 前端 → compute_layout(filters, containerWidth, rowHeight=200, gap=4)
   Rust: 查询全部符合条件的 media_items (id/w/h/sort_datetime/media_type/...)
         → 执行 Justified Layout 算法
         → 按 sort_datetime 跨日期边界插入 DateSeparator 行 (固定 36px)
         → 缓存结果
         → 返回 { total_rows, total_height }

2. 前端 virtualScroll 计算可视行范围 [startRow, endRow]（含上下各 3 行缓冲）
   → get_layout_rows(startRow, endRow)
   → 返回行数据（含每个 item 的 x/y/w/h + thumb_path + thumbhash）
   → 直接渲染，无需前端计算布局

3. 滚动 → 新的可视行范围 → get_layout_rows（缓存命中，纯内存读取）

4. 窗口 resize → debounce 300ms → compute_layout(newWidth)
   → 返回新的 total_rows/total_height → 重置滚动位置
```

> **性能**：Rust 端 Justified Layout 计算 15 万项 ≤ 80ms。`get_layout_rows` 纯内存读取 ≤ 1ms。前端零内存压力（仅持有可视行数据）。

### 10.2 DateSeparator

后端布局算法在 `sort_datetime` 跨日期边界时插入 separator 行：
- `row_type = 'separator'`，`height = 36px`，全宽独立行
- `separator_label = "2024年3月15日"`（后端格式化）
- 前端渲染 `DateSeparator.vue` 组件

### 10.3 虚拟滚动

容器总高度 = `total_height`（来自 `compute_layout`）。仅渲染可视行 + 上下各 3 行缓冲。非可视区域用 `padding-top/bottom` 占位。

### 10.4 缩略图加载流程

1. thumbhash (28B) → 32×32 DataURL → `blur(20px)` 占位
2. 可视行 items 中 `thumb_status=1` 的项 → 执行异步解码预热（见 10.4.1）
3. `thumb_status=0` 的项 → 请求队列攒批 → `batch_request_thumbnails` IPC → rayon 并行生成
4. 快速滚动 → 结合 `AbortController` 取消离开视口的预加载/解码请求

#### 10.4.1 图片异步解码（GPU 纹理上传预热）

在等高网格快速滚动时，即便虚拟滚动极其流畅，DOM 元素的插入依然可能因为浏览器主线程忙于同步解码图片而造成微小的卡顿（Jank）。
**技术细节与落地**：在 Vue 3 虚拟滚动渲染器中，当检测到图片即将进入视口边缘时（可结合 IntersectionObserver 外延缓冲区），在内存中动态创建 `new Image()`，并调用原生的 `img.decode().then(...)` 异步解码接口。当 Promise 回调成功后再将其真实挂载到 DOM 上。此时该图片的 GPU 纹理已在后台线程上传完毕，图片会瞬间亮起（配合 `opacity 0→1` 动画），彻底消灭极速滚动时的掉帧（Frame Drop）。
为防止极速滚动撑爆内存，需配合并发请求队列（如最大并发数 10-20），并在图片划出缓冲区时立即清空 src/中断拉取，以达到比肩原生 App 的丝滑体验。

### 10.5 媒体类型筛选芯片 (AppToolbar)

`[全部] [图片] [视频] [音频] [文档]` — 点击单选，Ctrl+点击多选，点击已激活的唯一芯片回到全部。

筛选变化 → `filterStore.mediaTypes` 更新 → 触发 `compute_layout` 重新计算 → 虚拟滚动重置到顶部。

智能相册是"视图级"筛选（全部/收藏/回收站/目录），类型芯片是"叠加级"筛选，两者可叠加。

### 10.6 MediaCard 角标系统

- **image**：无角标（纯图片）
- **image + live**：左上 `LIVE` 角标
- **video**：左上 `▶` + 右下时长
- **audio**：左上 `♪` + 右下时长
- **document**：左上类型标签 (PDF/SVG/...) + 右下页数

### 10.7 空状态 (`EmptyState.vue`)

| 场景 | 图标 | 文字 | 操作按钮 |
|------|------|------|----------|
| 首次启动无 scan_root | 文件夹图标 | "添加文件夹开始浏览" | "添加文件夹" |
| 目录无匹配结果 | 搜索图标 | "没有找到匹配的媒体" | "清除筛选" |
| 回收站为空 | 回收站图标 | "回收站是空的" | — |
| 搜索无结果 | 搜索图标 | "没有找到相关文件" | — |

### 10.8 目录树

扁平数组 + `parent_id` + `depth` 缩进。展开的节点参与虚拟列表。懒加载子节点。

---

## 十一、路由与导航

### 11.1 路由表

- `/` → MediaGrid（全部媒体）
- `/folder/:id` → MediaGrid（目录筛选）
- `/favorites` → MediaGrid（收藏）
- `/trash` → MediaGrid（回收站）

MediaDetail 为模态覆盖层（非路由），关闭后保持滚动位置。

### 11.2 智能相册

| 相册 | 条件 | 阶段 |
|------|------|------|
| 全部媒体 | `companion_of IS NULL AND is_deleted=0` | P1 |
| 图片 | `+ media_type='image'` | P1 |
| 动态照片 | `+ is_live_photo=1` | P1 |
| 最近导入 | `+ created_at > threshold` | P1 |
| 收藏 | `+ is_favorited=1` | P1 |
| 视频/音频/文档 | `+ media_type='video'/'audio'/'document'` | P2 |

### 11.3 浏览状态恢复

启动时从 `app_config` 读取 `last_directory_id`、`last_sort_by`、`last_sort_order`、`sidebar_width`。路由/排序变更时自动持久化。窗口位置/大小由 `tauri-plugin-window-state` 自动处理。

---

## 十二、媒体详情预览

### 12.1 统一入口

`MediaDetail.vue` 根据 `media_type` 分发：

- **image** → `ImageViewer.vue`：`convertFileSrc()` 直读原图，缩略图放大 → `img.decode()` 原图 → 平滑替换。CSS `transform` 缩放/拖拽，双击 fit/1x，EXIF 面板 (I 键)，LIVE 按钮播放
- **video** → `VideoPlayer.vue` (Phase 2)：HTML5 `<video>` 原生控件
- **audio** → `AudioPlayer.vue` (Phase 2)：封面 + 播放控件 + 元数据面板
- **document** → `DocumentViewer.vue` (Phase 2)：PDF 内联 / SVG 渲染 / "外部程序打开"

### 12.2 键盘快捷键

- `← / →`：上一个/下一个
- `Escape`：退出预览
- `Space`：播放/暂停（视频/音频/动态照片）
- `+ / - / 滚轮`：缩放（图片）
- `0`：重置缩放
- `I`：信息面板
- `F`：收藏
- `Delete`：软删除

MediaGrid 键盘导航（方向键选择、Enter 打开、Ctrl+A 全选）→ Phase 3 与 `useSelection.ts` 一起实现。

### 12.3 状态管理

`useMediaDetail` 是**组件级 composable**（非 Pinia store），每次打开创建新实例。`scale/translateX/translateY` 等高频 UI 状态使用 `ref()` 管理。

### 12.4 错误处理

| 场景 | 处理 |
|------|------|
| 扫描失败 | Toast + StatusBar 错误状态 |
| 缩略图失败 | 裂图占位 + 点击重试 |
| IPC 超时 | Toast "操作超时" |
| 数据库损坏 | 全屏错误页 + "重置数据库" |
| 格式不支持 | 格式名 + "外部程序打开" |

---

## 十三、主题系统

### 13.1 切换机制

`<html data-theme="dark">`，三态循环：🌙 Dark → ☀️ Light → 💻 System

### 13.2 Dark 主题

```css
[data-theme="dark"] {
    --color-bg-primary: #0f0f17;
    --color-bg-secondary: #1a1a2e;
    --color-bg-surface: #222240;
    --color-bg-elevated: #2a2a4a;
    --color-bg-overlay: rgba(0,0,0,0.6);
    --color-text-primary: #e6e6f0;
    --color-text-secondary: #9090a8;
    --color-text-tertiary: #606078;
    --color-accent: #e94560;
    --color-accent-hover: #ff6b81;
    --color-accent-subtle: rgba(233,69,96,0.15);
    --color-border: rgba(255,255,255,0.08);
    --color-border-strong: rgba(255,255,255,0.15);
    --shadow-sm: 0 1px 3px rgba(0,0,0,0.4);
    --shadow-md: 0 4px 12px rgba(0,0,0,0.5);
    --shadow-lg: 0 8px 24px rgba(0,0,0,0.6);
    --color-scrollbar-thumb: rgba(255,255,255,0.15);
    --color-success: #34c759;
    --color-warning: #ff9500;
    --color-error: #ff3b30;
    --color-info: #5ac8fa;
}
```

### 13.3 Light 主题

```css
[data-theme="light"] {
    --color-bg-primary: #f5f5f7;
    --color-bg-secondary: #ffffff;
    --color-bg-surface: #ffffff;
    --color-bg-elevated: #f0f0f5;
    --color-bg-overlay: rgba(0,0,0,0.3);
    --color-text-primary: #1d1d1f;
    --color-text-secondary: #6e6e80;
    --color-text-tertiary: #aeaeb2;
    --color-accent: #d63050;
    --color-accent-hover: #c02040;
    --color-accent-subtle: rgba(214,48,80,0.10);
    --color-border: rgba(0,0,0,0.08);
    --color-border-strong: rgba(0,0,0,0.15);
    --shadow-sm: 0 1px 3px rgba(0,0,0,0.08);
    --shadow-md: 0 4px 12px rgba(0,0,0,0.1);
    --shadow-lg: 0 8px 24px rgba(0,0,0,0.12);
    --color-scrollbar-thumb: rgba(0,0,0,0.2);
    --color-success: #28a745;
    --color-warning: #e68a00;
    --color-error: #d63031;
    --color-info: #0984e3;
}
```

### 13.4 设计令牌

```css
:root {
    --spacing-xs: 4px; --spacing-sm: 8px; --spacing-md: 16px;
    --spacing-lg: 24px; --spacing-xl: 32px;
    --radius-sm: 4px; --radius-md: 8px; --radius-lg: 12px;
    --font-sans: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
    --font-mono: 'JetBrains Mono', monospace;
    --transition-fast: 150ms ease; --transition-normal: 300ms ease;
    --sidebar-width: 260px; --toolbar-height: 48px; --statusbar-height: 28px;
}
```

### 13.5 过渡性能

禁止对 `*` 全局添加 `transition`（15 万缩略图容器会产生合成开销）。仅对布局骨架元素（`.app-shell/.app-sidebar/.app-toolbar/.app-statusbar/.theme-toggle`）添加主题切换过渡。

---

## 十四、前端交互设计

### 14.1 微动效

- **卡片悬停**：`scale(1.03) + box-shadow`
- **收藏点击**：弹簧缩放 1.0→1.3→0.9→1.0 (300ms)
- **侧边栏折叠**：`max-height + cubic-bezier`
- **缩略图加载**：`opacity 0→1 (300ms)`
- **LIVE 播放**：角标高亮 `scale(1.1) + glow`
- **主色调辉光** (Phase 2)：`box-shadow: 0 0 20px var(--dominant-color)`

### 14.2 侧边栏拖拽调整

`useSidebarResize.ts`：右边缘 4px 拖拽手柄，`mousedown` → `mousemove` 更新 CSS variable `--sidebar-width` → `mouseup` 持久化到 `app_config.sidebar_width`。最小宽度 180px，最大 400px。

### 14.3 主题色筛选 (Phase 2)

12 色相桶 (0°-360° / 30°) + 1 黑白。双模式：色块矩阵 + 色谱滑动条。仅图片有效。

---

## 十五、分阶段开发计划

### Phase 1：核心骨架 + 动态照片 + 行级加载 (6-8 周)

#### P1-1 项目初始化
- [ ] Tauri V2 + Vue 3 + TS + Vite 脚手架
- [ ] Cargo.toml Phase 1 依赖
- [ ] CSS 设计系统 + `.editorconfig`

#### P1-2 数据库层
- [ ] AppState（写 Mutex + 读连接池 + 布局缓存 RwLock）
- [ ] PRAGMA + 建表（`media_items` 主表 + `image_meta` 扩展表，其余扩展表 Phase 2 建）
- [ ] migration.rs 版本化迁移
- [ ] models / queries / path.rs / format.rs

#### P1-3 文件扫描器（两阶段架构）
- [ ] fast_scan.rs：快速扫描（walkdir + image_dimensions + Orientation 标签 + 批量 INSERT）
- [ ] enricher.rs：后台充实（全量 EXIF → image_meta + XMP 动态照片检测 + Live Photo 配对 + sort_datetime 校正）
- [ ] walker.rs：递归遍历 + 格式分类
- [ ] metadata.rs：EXIF / XMP 解析
- [ ] live_photo.rs：配对+嵌入检测（仅标记）
- [ ] 增量扫描策略（mtime 比对）+ 批量事务 + Channel/Event 进度 + CancellationToken

#### P1-4 缩略图引擎
- [ ] ImageEngine trait + ImageRsEngine
- [ ] EngineArena 调度
- [ ] generator.rs（Phase 1 仅 image 分支）
- [ ] EXIF 快速路径 + Orientation 矫正
- [ ] ThumbHash + 尺寸分桶缓存 (`cache/thumbnails/{size}/`) + WebP/JPEG 降级

#### P1-5 布局引擎（Rust 侧）
- [ ] Justified Layout 算法（Rust 实现）
- [ ] DateSeparator 行插入（按 sort_datetime 跨日期边界）
- [ ] 布局缓存 + compute_layout / get_layout_rows IPC

#### P1-6 动态照片
- [ ] Motion Photo 嵌入视频延迟提取
- [ ] get_companion_video_url IPC
- [ ] MediaCard LIVE 角标 + 悬停播放
- [ ] MediaDetail 动态播放

#### P1-7 IPC 层
- [ ] scan / media / layout / thumbnail / search / config / system 全部命令
- [ ] Channel + Event 定义

#### P1-8 前端 UI
- [ ] AppShell / Sidebar（拖拽调整） / Toolbar / StatusBar
- [ ] FolderTree + SmartAlbums
- [ ] 类型筛选芯片（UI 骨架，视频/音频/文档 P2 启用数据）
- [ ] MediaGrid + MediaCard + DateSeparator
- [ ] MediaDetail + ImageViewer
- [ ] EmptyState（首次启动/无结果/空回收站）
- [ ] ThemeToggle + ErrorBoundary + Toast
- [ ] 搜索框（文件名搜索）

#### P1-9 前端逻辑
- [ ] useVirtualScroll / useJustifiedLayout（消费后端行数据）/ useRequestQueue
- [ ] useThumbnail / useMediaDetail / useFolderTree / useTheme / useSidebarResize
- [ ] filterStore + mediaStore + scanStore + uiStore（含状态持久化）
- [ ] router

#### P1 验收标准

- ✅ 10,000 照片快速扫描 ≤ 3s 即出 UI + 后台静默充实
- ✅ 行级分段加载 + ThumbHash → batch_request → Image.decode() → 缩略图
- ✅ 快速滚动 10000+ FPS ≥ 55
- ✅ 大图预览（缩放/拖拽/EXIF）
- ✅ 30MB+ PNG 不 OOM
- ✅ Light/Dark 主题
- ✅ 后台充实完成后 LIVE 角标自动出现 + 悬停播放
- ✅ 文件名搜索可用
- ✅ 空状态正确展示
- ✅ 窗口/UI 状态恢复 + 侧边栏拖拽
- ✅ 错误通知正常显示

---

### Phase 2：四大类型全面支持 (5-6 周)

#### P2-1 视频支持
- [ ] FFmpeg sidecar + video/ 模块 + `video_meta` 建表
- [ ] generator.rs 视频分支 + 扫描器启用
- [ ] MediaCard 视频角标 + VideoPlayer.vue + 智能相册

#### P2-2 音频支持
- [ ] audio/ 模块 (lofty) + `audio_meta` 建表
- [ ] generator.rs 音频分支 + 扫描器启用
- [ ] MediaCard 音频角标 + AudioPlayer.vue + 智能相册

#### P2-3 文档支持
- [ ] document/ 模块 (mupdf + resvg) + `document_meta` 建表 + PSD 图片扩展
- [ ] generator.rs 文档分支 + 扫描器启用
- [ ] MediaCard 文档角标 + DocumentViewer.vue + 智能相册

#### P2-4 多引擎
- [ ] HeicEngine + RawEngine + EngineArena 降级链

#### P2-5 主题色
- [ ] MMCQ 提取 + 12 桶量化 + ColorFilter.vue

#### P2-6 管理功能
- [ ] 收藏 / 评分 / 软删除 / 恢复 / 回收站
- [ ] 系统回收站 (trash crate) + 多排序 + 缩略图大小滑块

---

### Phase 3：高级交互 (2-3 周)

- [ ] `notify` 文件监听 → 自动同步
- [ ] ContextMenu.vue 右键菜单
- [ ] useSelection.ts 批量选择 + MediaGrid 键盘导航
- [ ] 自定义相册 + 标签系统
- [ ] 多根目录管理
- [ ] FTS5 全文搜索

---

### Phase 4：AI 与高级特性（未来）

BLAKE3 去重、CLIP 语义搜索、人脸识别、GPS 地图、时间轴视图、移动端、音频波形、视频时间线缩略图条、Office 丰富预览、云端挂载

---

## 十六、工程实践

### 16.1 编码

所有文件 UTF-8，`.editorconfig` 强制。

### 16.2 SQL 安全

所有动态查询**参数绑定**，禁止字符串拼接。

### 16.3 批量事务

500-1000 条/事务，性能提升百倍。

### 16.4 路径规范

数据库统一正斜杠 `/`，运行时 `PathBuf::join()` 自动适配 OS。

### 16.5 错误处理

```rust
#[derive(Debug, thiserror::Error, serde::Serialize)]
pub enum AppError {
    Io(String), Db(String), Pool(String), Exif(String), Xmp(String),
    UnsupportedFormat(String), Engine(String), PathResolution(String),
    FFmpeg(String), AudioMetadata(String), DocumentRender(String),
    LayoutNotReady,
}
```

IPC 返回 `Result<T, AppError>`，前端接收序列化的 `{ code, message }`。

### 16.6 日志

`tracing` 分级：ERROR（不可恢复）、WARN（可恢复/降级）、INFO（关键事件）、DEBUG（缓存命中/耗时）、TRACE（SQL 参数）。

### 16.7 测试策略

- **Rust 单元测试**：scanner（格式分类/mtime 比对）、thumbnail（cache_key 计算/Orientation 矫正）、layout（Justified Layout 算法正确性）
- **前端纯函数测试**：thumbhash 编解码、日期/文件大小格式化
- 不追求覆盖率，聚焦核心数据流正确性

### 16.8 Tauri V2 注意事项

- `capabilities/default.json` 声明权限
- `convertFileSrc()` 需要 `tauri-plugin-fs` scope
- FFmpeg sidecar → `tauri.conf.json` → `externalBin`
- `cache_key` i64 仅 Rust 内部使用，不传前端（JS 精度限制）
- `thumbhash` BLOB 前端接收为 `number[]` → `Uint8Array`

### 16.9 跨平台预留

- macOS 数据目录 `~/Library/Application Support/`：`app_data_dir()` 自动适配
- macOS 路径大小写敏感：存储原始大小写
- iOS/Android WebView 性能较弱：行级分段加载 + 批量请求已适配
- iOS/Android 无 FFmpeg sidecar：Phase 4 评估替代方案
- 全平台路径分隔符：数据库统一 `/`，运行时 `PathBuf` 适配
- 读连接池移动端降至 2 连接

---

## 十七、性能目标

| 场景 | 目标 |
|------|------|
| **快速扫描** 10,000 图片（UI 可用） | **≤ 3s** |
| **快速扫描** 150,000 项 | **≤ 30s** |
| **后台充实** 10,000 图片（静默） | ≤ 10s |
| **后台充实** 150,000 项 | ≤ 3min |
| 缩略图 EXIF 快速路径 | ≤ 5ms/张 |
| 缩略图 JPEG 标准路径 | ≤ 50ms/张 |
| 缩略图 30MB+ PNG | ≤ 500ms/张 |
| compute_layout 15万项 | ≤ 100ms |
| get_layout_rows | ≤ 1ms（内存读取） |
| 虚拟滚动 FPS | ≥ 55 |
| 内存 15万项 | ≤ 300MB（前端仅持有可视行） |
| 大图预览 30MB+ PNG | ≤ 3s |
| 动态照片首次提取 | ≤ 200ms |

---

## 十八、风险与应对

| 风险 | 应对 |
|------|------|
| libheif 编译失败 | EngineArena 降级 |
| rawler 不支持某相机 | 日志 + 后续 libraw-rs |
| WAL 网络文件系统 | 检测提醒 + DB 仅本地 |
| 15万首扫慢 | Channel 进度 + 批量事务 |
| Windows 长路径 | `\\?\` + std::path |
| WebP 崩溃 | catch_unwind + JPEG 降级 |
| 大文件 OOM | rayon 并发限制 ≤ 2 |
| FFmpeg 缺失/80MB 包体 | 启动检测 + 图标降级 / 可选下载 |
| mupdf 复杂 PDF 崩溃 | catch_unwind + 图标降级 |
| Motion Photo 解析失败 | 降级为普通照片 |
| r2d2 连接池耗尽 | max_size + busy_timeout 兜底 |
| Schema 迁移失败 | 备份 DB + 回滚日志 |
| 布局缓存失效竞争 | RwLock + compute_layout 幂等 |
