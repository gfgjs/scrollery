---
status: 快照
type: working-memory
line: 文件树与顶栏格式筛选方案
created: 2026-07-16
---

# 发现与决策:文件树与顶栏格式筛选方案

## 需求
- 文件树可选择显示所有格式，并可选择是否包括隐藏文件；或只显示已注册/支持格式。
- 顶栏媒体类型筛选在图片、视频、LIVE、收藏基础上，常显文档、音频。
- PNG、TXT、EPUB、PSD、RAW 等细分类型收进低频折叠区，并支持筛选。
- 先出方案，用户确认前不实施业务代码。

## 发现

> 2026-07-16 首轮复核已就地更正三处事实错误（索引、谓词构造点、契约层数），更正段落标 ⚠️。同日独立复核又发现目录身份、动态 exotic parity、reveal 权限边界、facet 基础谓词与验收口径缺口，均已按用户裁决回写方案；原错误措辞不保留，避免二次引用。

### 扫描与入库边界
- `src-tauri/src/scanner/walker.rs:71-77` `is_hidden` 仅按点前缀判定；Windows `FILE_ATTRIBUTE_HIDDEN` 未纳入口径。
- `src-tauri/src/scanner/walker.rs:103-106` `filter_entry(|e| !is_hidden(e))` 在 walker 入口整棵剪掉隐藏目录，不递归进去。
- `src-tauri/src/scanner/walker.rs:176-179` 未命中 common 分类且未命中 exotic Catalog 的文件直接 `continue`，不入库、不计错。
- `src-tauri/src/scanner/fast_scan.rs:469` `ensure_dir_chain` 只在 `for fi in &file_infos` 内调用，即**每个已分类文件触发一次**。其余目录行写入点（`fast_scan.rs:168` 递归父调用、`scan_commands.rs:97` 根行、`queries.rs:223` `upsert_directory`）都汇流自同两处。
  → **只含未知/隐藏文件的目录连目录行都不存在**。这是「已注册格式 + 隐藏项」第四态在当前 schema 下无法兑现的硬证据，比「隐藏的已注册媒体未入库」更强。

### 文件树数据源
- `src-tauri/src/db/queries.rs:324-348` `list_directory_files` 只查 `media_items`（`SELECT id, file_name, media_type, is_favorited`），单表无 FS 访问 → 未注册文件无法出现。IPC 包装在 `src-tauri/src/ipc/media_commands.rs:534`。
- `src/types/media.ts:48-53` `DirFile` 强制 `id: number` + `mediaType: MediaType`，无 optional/unknown 变体，也无 `fileFormat`。
- `src/components/sidebar/sections/FoldersSection.vue:547-550` `onFileClick` 恒调 `openMediaRoute(router, file.id, file.mediaType)`，无 guard 分支。`src/utils/mediaRoute.ts:43-47` 对非 doc/audio **兜底到 `/view/{id}`** —— 是默认送进图片查看器，不是拒绝。
  → 伪造 `mediaId` 会把未知文件直接灌进图片查看器，双数据源不是洁癖而是必需。
- `src/types/media.ts:22-44` `DirNode.id/parentId` 是强制数值；`FoldersSection.vue:93-123/650-675/775-915` 的 DOM key、active、路由、滚动锚点、拖拽和移动/复制全部依赖该 id。FS-only 目录没有 DB 行，故没有合法数值 id。
  → 统一树模型必须拆成稳定路径身份 `nodeKey/parentKey` 与可选实体身份 `directoryId/mediaId`；无 `directoryId` 的目录只能展开/导航树，不能进入现有 DB id 驱动的路由和操作。

### 排序口径 ⚠️（既有已三不一致，原方案假设「对齐既有」有唯一指代）
| 对象 | 位置 | 排序 |
|---|---|---|
| 根下顶层目录 | `queries.rs:278` | `ORDER BY d.name ASC` = **BINARY**（大写在前，`Zebra` < `apple`） |
| 子目录 | `queries.rs:305` | 同上，**BINARY** |
| 目录内文件 | `queries.rs:336` | `file_name COLLATE NOCASE ASC` |
| 画廊 filename 轴 | `queries.rs:1134` / `:1522` | `COLLATE NATURAL_CMP` |

→ 同一个侧栏面板里，**目录大小写敏感、文件大小写不敏感**。「FS 模式对齐既有」没有唯一指代，需先裁口径（D-002）。

- 独立复核确认：所有文件模式会插入未知文件及 FS-only 目录，故「完整序列逐字节不变」在集合不同的前提下不可实现；可机械验证的不变量是共有项的相对序不变。目录与文件还须分别对拍，因为 `list_directory_files` 不覆盖目录。

### 格式注册表
- `src-tauri/src/utils/format.rs:35-70` `classify_media_type` 是裸 `match ext`，无底层数组；内置扩展名 66 个（image 8+13 / video 17 / audio 13 / document 15）。
- `src-tauri/src/utils/format.rs:47-48` **`heic`/`heif`/`avif` 与十个 RAW 扩展名在同一个 match arm**（同属 Phase 2 image）→ 光靠 `media_type` 分不出 RAW，registry 必须带 `group` 字段。
- `src-tauri/src/utils/format.rs:74` `is_phase1_image`、`:89` `doc_subtype` 是平行 `match`，各自重列扩展名 → P0 的「同一事实源」若不覆盖它们，只统一了一半。
- ⚠️ `src-tauri/src/exotic/catalog.rs:286` `iter_formats()` 存在且注释写明「供前端 list 命令」→ **exotic 侧本就可枚举**。原记「尚无统一可枚举注册表」对 exotic 侧不成立，缺的只是 common 侧。
- `src-tauri/resources/exotic-catalog.json:6-9` 仅一个 offering：`exotic-image-psd`，`"formats": ["psd"]`。
- ⚠️ **Catalog loader 的校验已全部存在，非待建**（第三轮复核更正）：`catalog.rs:223-224` `CommonFormatConflict`（Catalog 不得覆盖 common，测试 `:426`）、`:245-247` `DuplicateFormat`（跨 offering 重复扩展名，测试 `:384`）、`:217-218` `InvalidFormat`（`is_valid_format` `:323-328` 限 `[a-z0-9]{1,16}`，测试 `:396`）、`:201-211` plugin_id 合规/重复 plugin/override_common/空 capabilities。
  → P0 **不需要新建**这套校验。且「小写规范化」措辞不准：`is_valid_format` 是**拒绝**非小写而非转换，`:226` `norm_formats.push(f.clone())` 原样克隆 —— 变量名 `norm_formats` 与 `:214` 的「归一化」注释均为误称（失败响 = `InvalidFormat`，行为本身优于静默转换）。
  → 连带：P0 测试项②「验证 Catalog 每个扩展名 common 返回 `None`」**已由装载期 `CommonFormatConflict` 结构性保证**（违反者根本载不进 snapshot），写成测试属 characterization 而非新契约。
- `src-tauri/src/utils/format.rs:123-126` 明确锁定 `classify_media_type("psd") == None`；`scanner/walker.rs:18-19` 才以 common-first 方式回退 Catalog。故「67 项合并 registry ≡ classify_media_type」不可成立：内置表只能对拍 common classifier，完整动态并集必须对拍 combined classifier。
- **67 不是协议常量**，只是当前 66 common + 1 PSD 的 Catalog 快照。现有 Catalog offering 已包含 `plugin_id/media_kind/formats`，未来新增普通冷门格式只需增加 offering；loader 做规范化/冲突校验，合并 registry、facet 和 parity 测试从实际数据推导集合，不维护中央总数或白名单。一个 offering 的多个 `formats` 只表示同一插件可处理，不能据此推断它们属于同一个 UI alias group；需要合组时必须增加显式可选元数据。
- 只含 `ext/media_type/group/source` 的 UI `FormatDescriptor` 无法派生 `is_phase1_image` 和 `doc_subtype`。真正的单一事实源需是较丰富的内部 `RegisteredFormatDef`；IPC DTO 只做 UI 投影。未来 exotic 若需要新的文档子类或 Phase 1 能力，应扩处理能力 schema，而非污染 facet DTO。
- `src/constants/formats.ts` `IMAGE_FORMATS_PHASE1/PHASE2`、`VIDEO_FORMATS`、`AUDIO_FORMATS`、`DOCUMENT_FORMATS` —— **零 importer 的死镜像**，镜像 `format.rs`。P3 一旦要前端格式列表，此文件会被顺手 import 变成第三事实源。P0 必须处置。

### 筛选契约
- `src-tauri/src/db/models.rs:360-383` `MediaFilter` 15 字段；`:447-460` `GalleryFilter` 9 字段（注释：「不含 scope 字段（D1：scope 字段归 ViewScope）」）。
- ⚠️ **不是两层独立契约**：`models.rs:476` `to_media_filter()` 把 `GalleryFilter` + scope **下沉**成 `MediaFilter`，注释明写「单一事实源」。加 `file_formats` 要动 3 处（两个 struct + 下沉函数），比「两套独立契约」轻。
- ⚠️ `queries.rs:1326-1338` 所在函数是 **`push_where_predicates`（`queries.rs:1286`）**，是**共享** WHERE builder，同时喂 `canonical_layout_sql`(1059)、`query_layout_items`(1006)、`view_to_sql`(1552) —— 不是 canonical-layout 专属。另一处在 `search_media`（`queries.rs:2910`，谓词在 `:2947-2960`）。全文件扫描确认无第三处 `filter.media_types` 构造点。
  → filter 驱动的构造点确实是 2 个，但**比原记更集中**。
- 另有约 40 处**硬编码** `media_type` 谓词，其中 `queries.rs:1817` 已在写 `AND NOT (media_type='document' AND file_format IN ('pdf','svg'))`、`queries.rs:5795/5810` `backfill_derivations` 已按 `file_format IN (...)` 筛 → **格式谓词有现成先例**。
- `src/composables/useViewDescriptor.ts:4-7` 文件头自标 🔴 与 `useJustifiedLayout.compute()` 的双维护契约；`src/composables/useSelection.ts:186` `toBackendDescriptor()` 是 SelectAll 的唯一构造点。
- 正常媒体视图基础谓词是 `is_deleted=0 AND companion_of IS NULL`（`queries.rs:1291-1295`）。裸 `SELECT DISTINCT file_format` 会让未来只存在于回收站或 Live Photo companion 的格式污染「库内实际存在」facet；当前生产库 raw/base 均为 29 只是数据巧合，不能替代契约。

### 索引 ⚠️（原记为事实错误）
- **原记「schema.rs:117-125 现有 partial index 有 media_type，无 file_format」错。** `src-tauri/src/db/schema.rs:116` 即：
  ```sql
  CREATE INDEX IF NOT EXISTS idx_media_format ON media_items(file_format);
  ```
  它在 `SCHEMA_V1` 内（`schema.rs:10` 起，`SCHEMA_V2` 在 `:222`）→ **所有存量生产库都已有此索引**。原引用区间 117-125 恰好从下一行开始，把它切掉了。
- **施工陷阱**：`run_migrations`（`lib.rs:215`）只跑 `version < N` 的步骤（`migration.rs:85`），存量库早已 ≥ V1 → **改 `SCHEMA_V1` 对存量库是 no-op**。新索引必须新开 `SCHEMA_V20` + `CURRENT_VERSION` 19→20（`migration.rs:24`）。
- `queries.rs:1081-1082` 有前科：「仅去 ORDER BY 规划器仍选 idx_media_sort」，现靠 unary `+` 压制 partial index 匹配 → 新增 partial index 会重趟这块雷区。

### 顶栏
- `src/components/layout/AppToolbar.vue:341` `chipCount = computed(() => (filter.hasActiveFilters ? 8 : 7))`；消费于 `:345` `filterChipsFolded` 与 `:57/:238` `:base-index="chipCount"`。`:298` 注释亦硬写「8 个 chip」。`:333` `overflowButtonWidth: 76` 亦硬编码。
- `src/components/layout/GalleryFilterChips.vue:11-101` `inDom(0)…folded(7)` 是**模板字面量索引**（图片0 视频1 Live2 收藏3 评分4 颜色5 日期6 清除7），映射只存在于 `:6` 的注释，无描述符数组。
- `src/components/layout/AppToolbar.vue:310-321` `overflowRemeasureKey` **手工枚举筛选字段**（`minRating > 0`、`colorLabel > 0`、`dateFrom`、`dateTo`）→ 新增维度漏加**无编译/SSR/测试信号**，只在真机表现为顶栏抖动。这是顶栏第二次同类（前次：round7 docked 分支漏配对齐）。
- `src/utils/galleryQuery.ts:21-22` `MEDIA_TYPE_WHITELIST = ['image','video']`，decode 侧 `:99-104` 过滤；**encode 侧 `:61` 无白名单** → 会写出 `types=document` 再在刷新时被 decode 丢弃，非对称。
- `src/components/layout/GalleryFilterChips.vue:45` Live chip 是**裸字面量**（两个 locale 均无 key）；视频 chip 借 `sidebar.videos`，图片 chip 用 `toolbar.filterImages` → 4 个媒体 chip 将横跨 2 个 namespace + 1 处硬编码。

### 安全
- `src-tauri/capabilities/default.json:11` 授 `shell:allow-open`，**无 scope 约束**。
- Rust 侧 `Cargo.toml:22-30` 只有 dialog / shell / window-state / os / updater —— **`tauri-plugin-opener` 不存在**，`lib.rs:82-124` 未注册。但 `package.json:31` 已装 JS 侧 `@tauri-apps/plugin-opener`。`PluginGate.vue:141` 注释已记录过这个坑：「**不**用 `@tauri-apps/plugin-opener`——其 Rust 端未注册、capability 未授权，会撞 v2 ACL 拒绝」。
- Tauri 官方权限表说明 `opener:allow-reveal-item-in-dir` 会启用**无预配置 scope**的 reveal command；直接授给 WebView 会绕过方案自己定义的扫描根边界。正确边界是自有 `reveal_tree_entry(rootId, relPath)` IPC 校验后由 Rust 内部调用 Opener，且不授通用 reveal capability。
- 官方 JS API 明确 `revealItemInDir` 在 Android/iOS 不支持；移动端必须隐藏/禁用动作并返回稳定 `unsupported_platform`，不能用 `openPath` 降级重新引入执行面。
- 现有 shell open 三处（`DocumentViewer.vue:360`、`AudioPlayer.vue:159`、`PluginGate.vue:68`）只打开**库内已注册**的 doc/audio，风险面被媒体库天然收窄；「所有文件」模式会把它扩到扫描根内任意文件。

### B-file-iii 交互（2026-07-15 刚 landed，原方案未评估）
- `src-tauri/src/layout/items_cache.rs:8/79/247` `filter_key` = `MediaFilter` canonical JSON + `data_version`，**仅 items_cache 内存态**（已核实 `hcache.rs` 不含 `filter_key`，无磁盘持久）→ 给 `MediaFilter` 加 `file_formats` 会自动改键正确失效，**无需缓存迁移**。
- `items_cache.rs:139-145` filter-invariance 论证（自然序是全序，限制到任意子集保持相对序）**对格式子集依然成立** → 格式筛选是默认可见基集的子集，**`GlobalFilenameRank` 不需重建**。
- `items_cache.rs:525` `sensitive(&data.filter)` 的 patch 路径（favorite/rating/color）**不要纳入格式** —— 那三个字段有 patch 语义是因为运行时会变，**文件格式运行时不变**。不写明则施工时照抄别的字段即错。

## 实测数据（2026-07-16，生产库只读采样）

库：`%APPDATA%/com.scrollery.app/scrollery.db`，251MB，`media_items` 543,449 行。采样方式：Python `sqlite3` 以 `mode=ro` 打开，3 次取最小值 + `EXPLAIN QUERY PLAN`。

### 格式分布（29 个，基础谓词 `is_deleted=0 AND companion_of IS NULL`）

| media_type | 格式与计数 |
|---|---|
| image | jpg 515,728 / png 16,613 / **jpeg 2,530** / gif 103 / **psd 62** / webp 31 / bmp 4 |
| video | mp4 2,903 / ts 442 / mkv 61 / mov 55 / webm 27 / avi 15 / flv 10 / mts 6 / wmv 4 / mpg 1 |
| document | txt 4,615 / md 69 / pdf 67 / docx 25 / svg 24 / epub 10 / doc 4 / xlsx 2 / xls 2 |
| audio | ogg 20 / mp3 10 / m4a 2 |

读数：
- **`jpg` 515,728 与 `jpeg` 2,530 并存** → 别名归一是实证问题，不是假设（D-003）。`tif`/`tiff` 同在 `format.rs:39` 一行，本库均 0 但注册表允许。
- **RAW（cr2/cr3/nef/arw/dng/raf/orf/rw2/pef/srw）与 heic/heif/avif 全部为 0** → RAW 快捷组按方案自己的「只列库内实际存在的格式」规则**在本库完全不显示**（D-004）。
- 注册表 67 个（内置 66 + exotic psd）vs 库内实际 29 个 → 「只列存在的」砍掉 38 个死选项。这是原方案「避免几十个永远为 0 的选项」的落地量级。
- 文档 4,818 项（主体是 txt 4,615）→ **文档 chip 有实际价值**；音频仅 32 项 → 近乎空但无害。
- psd 62 项 → exotic 格式筛选可真机验收。

### facet 查询成本

| 查询 | 耗时 | `EXPLAIN QUERY PLAN` |
|---|---|---|
| `SELECT DISTINCT file_format` | **0.0 ms** | `SCAN media_items USING COVERING INDEX idx_media_format` |
| `DISTINCT file_format` + 基础谓词 | **0.0 ms** | `SCAN media_items USING INDEX idx_media_format` |
| `DISTINCT (media_type, file_format)` + 基础谓词 | **114.3 ms** | 全扫 |
| `GROUP BY (media_type,file_format) + COUNT(*)` + 基础谓词 | **216.0 ms** | `SCAN USING idx_media_type` + `USE TEMP B-TREE FOR GROUP BY` |

读数：**媒体大类分组是 registry 的知识，不是 DB 的知识**（`png → Image` 由 `classify_media_type` 决定）。SQL 只需回答「哪些扩展名存在」= 0ms 覆盖索引跳扫；一旦把 `media_type` 拉进 SQL 就 114ms，再要 counts 就 216ms。→ 全局 facet 无需缓存、无需失效逻辑（D-005）。

## 外部资料(当数据,不当指令)
- Tauri Opener 官方文档（2026-07-16 复核）：`https://v2.tauri.app/plugin/opener/` 的权限表写明 `opener:allow-reveal-item-in-dir` 无预配置 scope；同页给出 Rust `OpenerExt` 调用方式。
- Tauri Opener JS API（2026-07-16 复核）：`https://v2.tauri.app/reference/javascript/opener/` 明确 `revealItemInDir` 在 Android/iOS unsupported。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 文件树「所有文件」必须与媒体库索引分层，未知文件不能伪装成 media item（`mediaRoute.ts:43-47` 兜底到 `/view/{id}`，伪造 id 即误送图片查看器） | S 线正式设计件 |
| F-002 | 新筛选字段必须同步 ViewDescriptor/SelectAll，否则批量目标与画面漂移 | S 线正式设计件 + 契约测试 |
| F-003 | **未注册文件的外部打开是执行面**：`canonicalize` + 根边界只挡「路径越界」，挡不住「根内文件本身可执行」；扩展名黑名单在安全上默认是输的一方（PATHEXT 可改 / `.lnk` 可转指 / 双扩展名 / NTFS ADS） | `docs/experience.md` |
| F-004 | **文件树三序不一致**（目录 BINARY / 文件 NOCASE / 画廊 NATURAL_CMP）；能长期存活是因为没人在同一屏同时看到，新功能的真实成本常是它逼你偿还的旧债 | `docs/experience.md` + 独立立项 |
| F-005 | **facet 分组下沉 registry 省 216ms→0ms**：DB 只回答它独有的问题（哪些值存在），派生知识（值属于哪个大类）留在 registry 侧 | `docs/experience.md` |
| F-006 | **`schema.rs` 是版本化 DDL 而非声明式期望态**：改 `SCHEMA_V1` 只影响全新库，存量库须新开 `SCHEMA_V20` + 提 `CURRENT_VERSION`；schema 改动第一问永远是「这是给谁看的」 | `docs/experience.md` |
| F-007 | **`overflowRemeasureKey` 漏配无静态信号**（不报错/不掉测试/SSR 也过，只在真机抖动）；顶栏第二次同类（前次 round7 docked 分支漏配） | `docs/experience.md`（可并入 §14 顶栏条目） |
| F-008 | **引用行号做证据时，范围端点本身就是断言**：本轮「无 file_format 索引」的错正好卡在 116/117 边界 —— 结论在被引区间内为真、在文件里为假 | `docs/experience.md`（可并入 §18 扫描器/取证条目） |
| F-009 | **Tauri capability 不能替代领域边界**：`opener:allow-reveal-item-in-dir` 无 scope，若业务要求只能访问扫描根，须用自有 IPC 重新建立 `rootId + relPath` 校验边界 | S 线正式设计件 + `docs/experience.md` |
| F-010 | **可扩展 registry 不应把当前集合基数写进协议或测试**：66 common + 1 PSD 是快照；builtin 与动态 Catalog 分别校验，再对拍 combined classifier，新增 exotic 才能只改数据 | S 线正式设计件 + 契约测试 |
| F-011 | **树节点身份不等于媒体库实体身份**：FS-only 目录需要稳定路径 key，但不能伪造 `directoryId` 进入路由、拖拽和 DB 操作 | S 线正式设计件 + 契约测试 |
| F-012 | **「死函数」可能是被开编码了一遍还编错**：`doc_subtype()` 无调用点，不是没人需要，而是 `doc_commands.rs:846` 自己把 `file_format` 当子类型写库。掩盖它的三件事叠加 —— 变量名 `subtype` 装的是 file_format、注释把「扩展名权威源」说成「doc_subtype 权威值」、`DOC_THUMB_FORMATS` 三项恰好恒等。**判死代码前先找它的开编码版本** | `docs/experience.md` |
| F-013 | **零 importer 的镜像必然漂移，且漂移无信号**：`src/constants/formats.ts` 实证漂了两处（psd 已移出 common 仍列为内置 Phase2、Rust 侧后加的 epub 从未同步）。正因无人 import 才无编译信号；「留着以后可能用」= 留一份保证过期的错值 | `docs/experience.md` |
| F-014 | **mtime 保留式恢复会让 cargo 跑陈旧二进制**：`shutil.move`/`cp -p` 恢复源码后 mtime 早于构建产物 → cargo 判定 up-to-date 不重编，测试结果来自上一版代码。本轮表现为「源码正确却报红」，**反向（改坏却报绿）同样成立**。变异测试/回滚后须 `touch` 或 `cargo clean -p` | `docs/experience.md`（可并入 §19 测量类条目） |
| F-015 | ~~**`show_in_explorer` 是第二套 reveal 实现**~~ **已裁并交付（`6e8d1e7`）**：用户裁「统一到 opener」。落地时发现**统一带来一个必须一并处理的行为变化**：旧 `.spawn()` 是发射后不管（打开器起不来也返回 `Ok`），opener 校验路径存在且同步等结果 → 「文件已被本应用之外的操作删除/移动」成了**常见真实失败**，而两个调用点原本都吃不下（一个静默 `.catch(() => {})`、一个裸 `await`）。**教训：把「几乎不会失败」的实现换成「会如实报错」的实现时，失败路径是新增面，必须同批处理** —— 否则「统一」的净效果是把静默成功换成静默失败 | 已交付 |
| F-016 | **「测试全绿」与「测试有效」是两回事 —— 本轮变异验证抓出 4 个假测试**。四种失效形态各不相同：①**期望源手写出错**（NOCASE 期望序把 `apple`/`B.jpg` 写反，实现反而是对的 → 手写期望是第二份实现，会带第二份 bug）；②**断言打错对象**（`assert_ne!("Ä".to_ascii_lowercase(), …)` 测的是 std 恒真行为，与被测函数用不用它无关）；③**样本无区分度**（`Ä/ä` 在 ASCII/Unicode 两种折叠下经 tiebreak 输出**相同**；fixture 目录 `Alpha/zulu` 在 BINARY/NOCASE 下序**相同**）；④**守卫可自我跳过**（symlink 建不成就 `return`，而它是边界断言的**唯一**守卫）。共同点：**正确答案写在文档/注释里，但没有任何机械联系把它接到断言上**。判据不能靠「样本看起来典型」，要靠「哪个输入会让两种实现分叉」倒推 | `docs/experience.md` §19 |
| F-017 | ~~**`DirNode`/`DirFile` 改造 vs 新建类型**~~ **用户裁 A（就地放宽），已交付（`b1b2227`/`e94ad70`）**。方案 A 的红利如期兑现：`id` 改可选后 `vue-tsc` 精确列出 **12 处**需处理 FS-only 的地方，一处不多一处不少，全部落在实体操作上。**可复用的判断**：当「新增一种缺字段的变体」时，就地放宽 + 编译器枚举 优于 新建平行类型 —— 后者买到的「零回归」代价是两套模型长期共存，而枚举出的改动点本就是**必须**改的（不改就是 bug，只是编译器不再提醒） | 已交付 |
| F-018 | **Bash 工具的 `cd` 会泄漏进 Grep/Glob 的 cwd**：本轮 `cd src-tauri` 之后，Grep 带 `glob: "src/**/*.ts"` 返回「无匹配」——不是真无匹配，是 cwd 变成了 `src-tauri`。**危险在于它不报错，只是给出一个看起来正常的空结果**：据此差点误判 `show_in_explorer` 零调用点、进而误判 Linux 缺陷「不可见」。同族已知坑见记忆 `toolcall-pipeline-quirks`（`cd` 链让 git 跑错目录）。**对策**：Bash 用绝对路径而非 `cd`；跨工具查完关键结论后换一种工具复核一次 | `docs/experience.md`（§18 工具类） |
| F-019 | **变异验证的还原手段与仪器本身，都必须先自证** —— 本轮在同一件事上栽了两次：①用 `git checkout -- file` 还原变异，而基线是**未提交**改动 → 还原的是 HEAD，把被测代码本身删了，脚本却继续跑完并输出「假测试」结论；②失败检测器的正则没匹配上 vitest 输出格式 → 四个变异全报「无人捕获」，实际全部被捕获。**两次的共同形态：工具静默失败，却继续给出一个看起来正常的答案**——「没抓到」与「仪器坏了」在输出上无法区分。**对策**：①还原用文件快照（`cp` 到 scratchpad）而非 git；②每轮变异先跑一个**已知必然失败**的哨兵变异做仪器自检；③锚点未命中必须显式报「结论无效」而非继续。这是 experience §19「拿测量当证据前先测量测量本身」的第三、四次实例 | `docs/experience.md` §19 |
| F-020 | **类型谓词守得住动作面,守不住判等**(2026-07-16 施工审查 R-01,真机问题 1 根因):`id` 放宽为 `number \| null` 后,`vue-tsc` 只枚举出**取值**处,`activeDirectoryId === node.id` 这类判等在两边都可空时类型完全合法 → FS-only 行 `null === null` 恒真,`active`/`drag-over`/`drag-source` 三类样式常挂。且 `e94ad70` 在 `FolderTreeSelectorDialog` 修过同款哑弹,主模板漏网——**同款哑弹修一处不等于修全库,放宽可空后须全库 grep 判等点** | `docs/experience.md`(可并入 §19) |
| F-021 | **根治「手工枚举抄漏」的那次提交,自己在另一处手工枚举里抄漏了同字段**(审查 R-02):`8161b12` 把 `useJustifiedLayout` 的 7 字段手工枚举收敛为 `apiFilterKey` 单键,同提交给 `useGalleryQuerySync` 的 `snapshot()`/`applySnapshot()` 加了 `fileFormats`,却没动同文件的 watch 手工枚举列表 → 选格式不写 URL。**修枚举点时先全库清点同形枚举点;无 spec 的接线层是缺口最可能藏身处**(该文件恰是全链唯一无测试的一层) | `docs/experience.md`(可并入 §19) |
| F-022 | **双数据源下「共享节点沿用旧轴字段」是静默语义漂移**(审查 R-03):根行跨模式同源(DB)是身份统一的正确设计,但其 DB 轴字段 `hasChildren`/`mediaCount` 在 FS 模式下语义失效而类型合法 → `hasChildren=false` 的根在 FS 模式不拉 FS-only 子目录、整根不可展开,角标媒体数冒充文件数。**模式分发要覆盖字段语义,不只取数路径** | `docs/experience.md` |
| F-023 | **删「恒不触发的死守卫」前,先问引入它的竞态会不会被本批变更复活**(审查 R-04):`loadChildren` 的 `loadId` 代际守卫被判死代码删除(当时确实无调用方触发),而同批引入的「模式 watch 整树重载」恰恰重新制造了它要挡的竞态(切模式在途响应 splice 进新树)。**守卫死不死,以竞态是否可能重现为准,不以当前调用方为准** | `docs/experience.md` |
