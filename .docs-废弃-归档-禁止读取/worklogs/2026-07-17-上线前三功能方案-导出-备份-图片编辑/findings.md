---
status: 快照
type: 工作记忆
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-17
---

# 发现与决策:上线前三功能方案(导出 / 数据备份 / 图片简单编辑)

## 需求
- 用户总结的上线前要紧功能三项:①导出(用户整理好后的内容)②数据备份③图片简单编辑。
- 要求:出详细方案并落盘待审(不施工)。
- 2026-07-18 补充:方案形成后代码已有大量改动,必须按当前实现重新审阅,不能只做文字润色。

## 发现
- docs/todo.md(2026-07-17)全文无既有「导出/备份/图片编辑」功能线;三项均为新功能。唯二相邻:批1 曾修 save_version 空备份覆盖源 bug(todo.md:330);前后端分离调研把「移动自动备份」列为远景分水岭(todo.md:379),与本地备份不同层,不冲突。
- 设计稿样式基线:同日「树内文件行打开与拖拽/设计-path预览与文件行拖拽.md」——frontmatter(id/status/type/design/line/created)+ 范围裁定 / 安全自评 / 后端 / 前端分节。本线三稿沿用。
> 下列「摸底 1–3」是 2026-07-17 原始快照。与后文「代码漂移复审」冲突时,以后文为准。

### 摸底 1:数据模型与持久化(agent 回报,2026-07-17 原始快照)
- Schema=Rust 常量 V1..V19(db/schema.rs),迁移器 db/migration.rs `CURRENT_VERSION=19`,版本存 `app_config.schema_version`(非 PRAGMA user_version);migrate_step 每版一事务、幂等有测试。
- DB 单文件 `%APPDATA%\com.scrollery.app\scrollery.db`(lib.rs:164);WAL+busy_timeout 5000+synchronous NORMAL(connection.rs:28-37);启动时 TRUNCATE checkpoint(connection.rs:68);写连接 Mutex + r2d2 读池 8。
- **用户整理态清单**(纯磁盘重扫不可再生,=备份/导出的最小保护集):`albums`+`album_items`(含手排 sort_order)、`tags`+`item_tags`、`media_items.{is_favorited,rating,color_label,is_deleted,deleted_at}`、`persons.{name,is_named,is_hidden,is_ignored}`、`faces.{person_id,is_confirmed,is_unassigned}`、`face_rejections`、`reading_progress`、`reader_bookmarks`、`reader_book_prefs`、`doc_replacements`、`document_versions`(+盘上 `appdata/documents/{item_id}/` 文件)、`storage_backends`(cred 走 keyring 不落库)、`scan_roots`、`app_config`。
- 重扫可再生(备份可不含):directories、image/video/audio/document_meta、ai_embeddings(可再生但代价高)、faces 检测框、text_book_index、face_coverage、缩略图全家(cache/ 五类目录)。
- **媒体行身份**:`UNIQUE(directory_id, file_name)`(schema.rs:109);`cache_key=xxh3("{rel_path}/{file_name}|{mtime}")` 不含盘符(hash.rs:29-40)。用户态挂在 media_items 行内(同行混布派生列+用户列,schema.rs:95-98)。
- **路径=root_id+rel_path 相对制**,绝对路径查询期拼装(queries/scan.rs:464-481);已有 `relink_scan_root`(scan_commands.rs:278,抽样100+size/mtime≥95%匹配,零重扫零重生成)→ 备份跨机/移盘恢复天然友好。
- 设置主存=DB `app_config` 表(configStore.ts 走 IPC),**无 tauri-plugin-store**;localStorage 仅短平 UI 态(标题栏模式等),不入备份保护集。
- appdata 布局:`scrollery.db` / `cache/`(thumbnails 5档+ai_thumbs+face_thumbs+sprites+motion_videos+audio_covers,LRU+孤儿GC)/ `models/`(ONNX)/ `exotic/`(插件)/ `documents/`(文档版本树)/ `logs/`。

### 摸底 2:图像管线与查看器(agent 回报,2026-07-17 原始快照)
- 解码:ImageRsEngine(jpg/jpeg/png/webp/bmp/gif/tif/tiff,engine/image_rs.rs:22)优先,WIC 兜底(另加 heic/heif/avif/ico,Windows-only,engine/gpu/wic_engine.rs:25;名带 gpu 实为纯 CPU 软件路径)。无 wgpu/turbojpeg/mozjpeg;真 GPU 仅视频(D3D11/MF)。
- **编码面极窄**:仅 `encode_as_webp`(libwebp,q100=无损 VP8L,exif_thumb.rs:109)+ `encode_as_jpeg`(image crate q85 兜底,exif_thumb.rs:135),全部服务派生缓存;**无任何「保存原图」IPC**。注意 image crate 已启 `png` feature → PNG 编码能力在依赖里现成,只是无代码路径。
- EXIF:读齐全(kamadak-exif,scanner/metadata.rs);orientation 解码期烤进像素(仅 JPEG+WIC 的 HEIC;PNG/TIFF/WebP 为既知盲区 enricher.rs:324);**所有写出路径剥光 EXIF/ICC/XMP**(exif_thumb.rs:23-28 明写)。
- 变换现状:resize 多路、rotate90/180/270+fliph/flipv 原语已在用(image_rs.rs:178 等);**crop 像素级为零**(image crate 自带 crop_imm 可用);**调色为零**。查看器「旋转」纯 CSS transform 非破坏(useMediaDetail.ts:138)。
- 原子写盘:`write_atomic`(generator.rs:76,`.{seq}.tmp`+同目录 rename)现成可复用。
- **失效链=mtime 键**:cache_key 含 mtime;SourceChanged→`invalidate_derived_for_item`(queries/scan.rs:690)删 meta+embeddings+**faces**+重置 ai/face_status——**就地覆盖编辑会连坐清掉该图人脸确认/嵌入**;且①目录 mtime 剪枝可能漏检就地改(fast_scan.rs:214)②查看器无「此图已变」事件、全尺寸 `<img>`(convertFileSrc 无 query)会吃 WebView 陈旧缓存。
- 查看器:ContentViewer.vue 原图直显 `<img>`+CSS transform;动作挂点=底部控制条(:111-217)+顶栏 ContextualToolbar 命令注册表(commands/builtins/viewer-image.ts)+ViewerApi(defineExpose,:890)。已有剪贴板复制/设壁纸系统命令(ipc/system_commands.rs:192,212)。

### 摸底 3:文件操作先例与 IPC 面(agent 回报,2026-07-17 原始快照)
- **无通用导出/备份/恢复/压缩写出**;既有可复用件:`copy_media_items`(std::fs::copy 至任意 target_dir,file_ops_commands.rs:183)、`copy_directory` 递归复制(:839)、trash 5.2.6、`zip = "2"(deflate)` 已在依赖但**仅读**(EPUB 容器);文档「保存前自动备份版本」先例(doc_commands.rs:260)。
- **长任务范式最佳样板=`start_scan`**(scan_commands.rs:488):Tauri v2 typed `Channel<ScanChannelPayload>`(Progress|Completed|Error tagged enum)+ `spawn_blocking` + `CancellationToken` 注册表在 state.rs(scan_tokens HashMap);另一风格=AI/face 的 2s 轮询 pull。新长任务抄 Channel 式。
- 命令注册集中 `ipc/registry.rs`(~150 条单表);前端常量镜像 `src/constants/ipc.ts`。
- **能力面蓄意收窄**:capabilities 仅 default.json(core/dialog:allow-open/shell:allow-open/window-state/os);**无 tauri-plugin-fs**(JS 侧零文件系统 ACL);opener 不注册插件、只作 Rust 函数走自定义路径校验 IPC。dialog 无 allow-save(要 save 对话框须加权限;选文件夹用 open+directory:true 已可)。
- selection 有后端描述符:`resolve_selection`/`count_selection`(registry.rs:64-65)——批量操作可不传全量 id;但**顺序**语义在前端视图(画廊排序/相册手排),导出序号命名需前端传有序 id 或另立约定。
- 挂点:SelectionActions/SelectionToolbar(selectionCommands 数组 MediaGrid.vue:1582)、右键菜单 resolveMediaContextCommands(:674)、设置页 storage 区(NetworkStorageSection 同侧)、**无任务中心**——长任务进度全走 AppStatusBar 内联(scanStore/aiStore 模式)。
- 撤销:historyStore 结构化可逆记录(move/copy/relocate+CallbackRecord)。
- 「同步管线」=Copybara 源码树同步(私有→公开镜像),与用户数据零关系;唯一远程数据面=WebDAV 网络盘(netfs feature)。
- **free/pro gating 仅存在于 exotic 插件实体**(EntitlementProvider 编译期换装,exotic/mod.rs:252);无通用 per-feature 开关原语——三功能要 gate 得新建机制(不建议),默认全量进 free 与「免费核心完整可用」口径一致。
- `db:media_updated` 常量已声明(ipc.ts:301)但 Rust 侧零发射=僵尸常量。

### 代码漂移复审(2026-07-18,覆盖冲突的原始摸底)

- 复审基线:三份初稿在提交 `247d05c` 落盘;本轮检查其后的相关提交与当前工作树。结论只更新设计文档,不把并行未提交改动当成已落地能力。
- **Schema 已从 V19 演进到 V21**:`CURRENT_VERSION=21`;V20 新增持久化 `media_items.view_rotation`,V21 新增隐藏 root/相关设置。所有 SQL、迁移、导出与恢复测试必须保留 V20/V21 语义,不能沿用初稿 V19 假设。
- **SelectionDescriptor 已具后端顺序契约**:`Explicit` 保持输入顺序并限制 100k;`SelectAll` 通过 `ViewDescriptor`、layoutVersion 和 excluded ids 按当前布局 SQL 排序解析,V21 同时排除隐藏 root。方案 A 不应让前端物化并传递全部有序 IDs;应分块消费后端解析结果。
- **长任务可靠范式已变化**:当前代码明确记录 Tauri `Channel` 会随发起 WebView 消失;恢复链改用 AppState progress snapshot、app-wide event、status query 与 `RunTokenSlot` generation 比较清理。A/B 若只抄旧 `start_scan` Channel 模式,窗口重载后会丢进度并可能被旧任务清掉新任务状态。
- **状态栏布局已有更严格优先级**:左侧在查看器文件信息、docked selection 等状态间切换;A/B 进度不应抢占左侧。共享右侧轻量 `BackgroundFileJobIndicator` 更符合当前结构。
- **document_versions 不是纯 DB 数据**:`abs_path` 保存 appdata `documents/{item_id}/...` 的绝对路径;当前保存链存在「先插 DB 行→写文件→回填路径」窗口,删除也不是 DB+文件原子操作。直接复制 DB+documents 可能捕获不一致;跨机恢复还会遗留旧机器绝对路径。
- **完整 DB 快照包含派生表**:`VACUUM INTO`/backup API 复制整个 catalog,不会自动排除 ai_embeddings、faces 检测框等可再生数据。v1 应如实定义为完整 DB + documents;选择性构造「最小 DB」需要维护 schema/外键/迁移专用导出器,降到 P2。
- **恢复必须先于常规 DB pool 初始化处理文件交换**:需要 staged DB 校验/迁移、document 路径 rebase、预恢复回滚包和 phase marker。仅在运行中 rename DB,或只留一个 `restore_pending` 布尔值,不足以覆盖 current moved/installed/verified 间的崩溃。
- **V20 显示旋转影响 A/C**:A 若导出原始字节不会烤入 `view_rotation`,需 preflight/manifest 明示;C 编辑器必须以当前显示旋转为初始视觉状态,保存副本时物理烤入像素,新 item 置 0。
- **图像依赖能力比初稿复杂**:当前 `image 0.25.10` 提供 orientation/EXIF/ICC 读取及部分 encoder 写入接口,但 `image` 自带 WebP encoder 仅无损;现有有损 WebP helper 走另一 crate。不能未验证就断言 `img-parts` 足以解决 JPEG/PNG/WebP 的 EXIF/ICC/orientation/内嵌 thumbnail 一致性。
- **GIF/HEIC/AVIF 契约未成立**:查看器没有把 GIF 当前帧号传给编辑后端;HEIC/AVIF 主要经 Windows WIC,与跨平台目标不一致。v1 应先限定静态 image-rs 主链格式,而非把「当前帧」或 Windows-only 解码写成已支持。
- **编辑内存原估算不安全**:268MP 的单份 RGBA 已约 1GiB,实际还会同时存在源/目标/编码缓冲。上限应由 decoder `total_bytes`、checked 峰值预算和 24/50/100MP 基准共同决定,并在分配前拒绝。
- **落盘与入库需要部分成功语义**:元数据注入必须在 temp 内完成后再最终 rename;若文件已发布但 targeted ingest 失败,应返回 `savedNeedsIndex` 并触发恢复扫描,普通失败会诱导用户重复生成副本。
- 本地只读样本(非容量承诺):复审时 `%APPDATA%/com.scrollery.app/scrollery.db` 为 25,411,584 bytes,WAL 为 0。备份 UI 的估算必须运行时测量 DB、WAL、documents,不能据此写死。

### 二次核对(2026-07-19,HEAD=5f92f1d)

- 漂移面仅 3 提交(35b1201 / 0bf5ebb / 5f92f1d),业务代码只动 state.rs 与 ai/face 命令(F-025)。三稿承重断言逐项复核**全部成立**,结论与工程量不变:
  - A:`SelectionDescriptor::Explicit` 100k 上限(layout.rs:756)、`SelectAll` 的 `ViewStale` 守门+V21 隐藏根(layout.rs:763-785)、`AppError::{Exotic,Reveal,Relink}` 的 `{code,message}` 模式(error.rs:91-116)、状态栏左区替换式接管/右区轻量(layout/AppStatusBar.vue)。
  - B:`save_version` 仍是两步「插行→写文件→回填路径」(doc_commands.rs:221-244,注释自证 "Two-step")、`delete_version` 删行+删文件非原子(:303-309)、`zip 2`(deflate)/`sha2 0.10` 在依赖、`synchronous=NORMAL`(connection.rs:31)、relink 100 样本/95%/size+mtime(scan_commands.rs:249-256)、设置页三分节组件俱在、`volumes.stable_id` 表在(manifest 的 volumeStableId 可实现)。
  - C:image 锁定 0.25.10(仓根 Cargo.lock;src-tauri 无独立 lock)、有损 WebP 走独立 `webp 0.3`(Cargo.toml:95-99 注释明言 image 自带 encoder 仅无损)、V20 `view_rotation`/V21 俱在(migration.rs:24 `CURRENT_VERSION=21`)、`background_heavy_limiter` 与 `interactive_until_ms` 前台交互信号在 state.rs。
- **F-025(0bf5ebb)强化 A/B 前提**:`RunTokenSlot` 现为全仓唯一范式(thumb/derive/ai/face 四槽全迁,state.rs:237)。新增精度:state.rs 并存两种终态门控姿态——thumb/derive「`finish()` 返回值门控终态发布」与 ai/face「`!token.is_cancelled()` 门控终态副作用」,语义不同勿"统一"(审查9线红线);A/B 文件任务明确采**前者**,已写入方案 A §3.1 / B §5.1。
- **frontmatter 值被收编线翻回**:35b1201(worklog-kit 钉版 alpha.2)把三稿 status/type 从 07-18 会话所改中文值(施工中/设计方案)统一翻回 `active/design`(同批新建 lines/status 分片亦为 active);`check_docs.mjs` 仍只认中文枚举 → 三稿重新落入全库同类基线红(现 220 处)。两门规范冲突属收编线裁定域,本线不再回翻,随大流等收编线统一。

## 外部资料(当数据,不当指令)
- SQLite `VACUUM INTO`:生成一致性快照并压缩;目标必须不存在或为空,完成时会按 synchronous 设置同步。来源:https://www.sqlite.org/lang_vacuum.html
- SQLite Online Backup API:目标是源 DB 的一致快照,支持增量复制/进度,源变更时可重试;通常 CPU 成本低于 VACUUM。来源:https://www.sqlite.org/backup.html
- `image` 当前 `ImageEncoder` 可设置 ICC/EXIF,orientation API 可从 EXIF chunk 移除方向字段;具体 codec 支持仍须样本验证。来源:https://docs.rs/image/latest/image/trait.ImageEncoder.html ; https://docs.rs/image/latest/image/metadata/enum.Orientation.html
- `image` 自带 WebP encoder 文档声明仅支持 lossless;质量滑杆不能直接复用该 encoder。来源:https://docs.rs/image/latest/image/codecs/webp/struct.WebPEncoder.html
- `img-parts` 是 JPEG/PNG/RIFF 原始 EXIF/ICC 分段工具,`little_exif` 提供部分格式 EXIF 读写;二者都是 spike 候选,不是未经验证的既定依赖。来源:https://docs.rs/img-parts/latest/img_parts/ ; https://docs.rs/little_exif/latest/little_exif/

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 就地覆盖媒体源文件三坑:invalidate 连坐删 faces/embeddings(scan.rs:690)、查看器无像素级重载事件+convertFileSrc 无 cache-buster、目录 mtime 剪枝漏检就地改(fast_scan.rs:214)——凡「改写库内源文件」类功能先过这三关 | experience(通用预警);方案C §2 已载 |
| F-002 | 用户整理态最小保护集=重扫不可再生表/列清单(albums/tags/收藏评级色标/人脸命名确认/阅读态/文档版本/roots/app_config) | 方案B §1 为契约基准;experience 候选 |
| F-003 | 无 tauri-plugin-fs+opener 不注册插件=蓄意安全姿态,一切盘面操作走自定义路径校验 IPC;新功能沿用勿引 fs plugin | experience 或项目 CLAUDE.md |
| F-004 | zip crate 已在依赖但仅读(EPUB);dialog 仅 allow-open——选目录 open+directory:true 已够,save 对话框才需加 allow-save | 施工备忘(方案A/B 已载),倾向 no-promotion |
| F-005 | `db:media_updated` 常量声明(ipc.ts:301)但 Rust 零发射=僵尸常量 | todo 小项(顺手清理或编辑线复用为像素重载事件) |
| F-006 | Tauri Channel 与发起 WebView 同生命周期;可恢复长任务须用 AppState snapshot + app event + status query + generation 清理 | experience;方案A/B 已载 |
| F-007 | `document_versions.abs_path` 持有旧 appdata 绝对路径,备份跨机恢复须 staged rebase | design/experience;方案B 已载 |
| F-008 | documents 的 DB 行与文件写删不是原子事务;备份需 storage guard 和一致性校验 | design/experience;方案B 已载 |
| F-009 | V20 `view_rotation` 是持久化非破坏显示态;原字节导出不烤入,图片编辑另存时必须烤入且新 item 归零 | experience;方案A/C 已载 |
| F-010 | SelectionDescriptor 已在后端提供当前布局顺序与 V21 隐藏 root 过滤;批量功能不要让前端物化全量 IDs | experience;方案A 已载 |
| F-011 | 图片另存的 orientation/EXIF/ICC/thumbnail/WebP 元数据是格式矩阵,必须 golden spike 后再承诺 | design/experience;方案C 已载 |
| F-012 | SQLite 全库快照不会排除 DB 内派生表;「完整 DB」与「精简业务态」是两种不同备份产品 | design;方案B 已载 |
| F-013 | state.rs 两种终态门控姿态并存(F-025 后):thumb/derive=finish 返回值门控终态发布,ai/face=!is_cancelled 门控终态副作用;新长任务(导出/备份类文件任务)须明确采前者,勿"统一" | experience;方案A/B 已载 |
| F-014 | check_docs.mjs 只认中文 status 枚举,而 worklog-kit alpha.2 收编(35b1201)已把全库 frontmatter 统一为 active/design,两门规范冲突;三稿被翻回后随大流落基线红 | 收编线处置;本线不修不回翻 |
