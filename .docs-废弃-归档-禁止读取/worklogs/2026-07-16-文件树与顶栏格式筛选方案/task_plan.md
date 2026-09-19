---
status: 快照
type: working-memory
line: 文件树与顶栏格式筛选方案
created: 2026-07-16
---

# 任务计划:文件树与顶栏格式筛选方案

## 目标
基于当前源码给出文件树可见性/支持范围与顶栏媒体大类/细分格式筛选的统一方案，待用户确认后再实施。

## 当前阶段
阶段 7 收官:修复批 1~3 全部落地(2188bff/973709c/13b727d + 破窗 d293b53);全线余项 = P4 真机验收 + 两个待裁立项(问题②方案B / 树内文件行拖拽)

## 阶段

### 阶段 1:现状与契约核查
- [x] 核查文件树数据源、隐藏文件判定和格式注册表
- [x] 核查顶栏筛选状态、查询契约与媒体分类模型
- **状态:** completed

### 阶段 2:交互与领域模型设计
- [x] 定义文件树三种显示状态、来源边界和未注册文件行为
- [x] 定义常显媒体大类、折叠细分格式与组合语义
- **状态:** completed

### 阶段 3:实施与验证方案
- [x] 列出前后端改动面、迁移策略、测试矩阵和验收标准
- [x] 提交用户确认，不修改业务代码
- **状态:** completed

### 阶段 4:方案复核与用户裁决
- [x] 逐条回查方案断言，就地更正 3 处事实错误（索引 / 谓词构造点 / 契约层数）
- [x] 生产库只读实测（543,449 项）：格式分布 + facet 查询成本 + EXPLAIN
- [x] 出详细对比裁决清单（8 项，每项含选项对比与推荐）
- [x] 用户 2026-07-16 **采纳全部推荐** → D-001~D-012 拍板
- [x] 独立复核文件树身份、动态 exotic 注册、reveal 权限、facet 基础谓词和验收口径；用户采纳全部修订，并要求预留未来冷门格式扩展 → D-001/D-002/D-004/D-005/D-007 修订，新增 D-013
- **状态:** completed

### 阶段 5:S 线正式设计件
- [x] 第三轮核实（其它会话复核稿）：`format.rs:122-127` psd 测试、`walker.rs:18-19` common-first、`media.ts:23` + `FoldersSection` 数值 id 全链、`queries.rs:1291-1295` 基础谓词、生产库 543,445 基础行 / raw==base==29 巧合 —— **全部为真**
- [x] 第三轮**新发现一处不准**：Catalog loader 校验（`CommonFormatConflict`/`DuplicateFormat`/`InvalidFormat`）**已全部存在**，P0 复用不新建；且「小写规范化」是**拒绝**非小写而非转换（`is_valid_format` `:323-328`，`:226` 原样克隆，`norm_formats` 与 `:214` 注释是误称）→ 已就地更正 findings + §7 P0
- [x] `docs/README.md` 字母登记表取 **S** 号 + 推进「下一空闲字母」S→T（索引门抓到该不变量）
- [x] 建 `docs/designs/2026-07-16-文件树显示范围与格式筛选.md`（normative 线契约，§3–§8 为施工准绳）
- [x] `docs/todo.md` 加 S 分节
- **状态:** completed

### 阶段 6:施工（P0→P4）
- [x] **P0 可扩展注册表与契约测试**（`b3321fd` P0-a 内置表收敛 + `3fc96cf` doc_subtype 接线修 bug + `7b4c868` P0-b 合并层 + 删死镜像）
- [x] P1 文件树三态（P1-a 后端六提交 + F-015/F-017 四提交 + P1-b 五提交;逐批记录在 todo.md S 节）
- [x] P2 一级媒体类型（`dfcd0d3`）
- [x] P3 细分格式全链（`2afe1e1`/`8161b12`/`d7200f1`/`a334127`/`73c44bb`）
- [ ] P4 验证、真机与文档收口（八门本地全绿,余真机验收）
- **状态:** in_progress

### 阶段 7:施工审查与真机问题定性（2026-07-16）
- [x] 全量代码审查（4 并行域 + 主线程复核）→ [审查快照](../../reviews/2026-07-16-S线文件树与格式筛选施工审查.md):2 🔴 + 5 🟠 + 14 🟡
- [x] 真机三问题定性:①隐藏项显示混淆=真 bug（R-01/R-06）②隐藏目录 .md 不能应用内打开=D-001 有意行为,给 A/B/C 三案 ③拖拽=文件行从未实现 + FS 模式误伤（R-07),默认模式完好
- [x] 修复批 1~3 施工(用户 go;开工前逐项核实——报告后经历 T/U 线大重构,后端行号全漂移但发现全部仍在):批1 `2188bff` / 批2 `973709c` / 批3 `13b727d` / 破窗 `d293b53`;R-07=修法a、R-17=挪位、R-18=declare;R-13/15/16/20/21 记录不动
- **状态:** completed

## 方案（已定稿）

### 一、术语先定形

- **已注册格式**：当前 Host 能分类并允许入库的格式，即内置 common 格式表与 exotic Catalog 的并集；Catalog 已登记但插件尚未安装/授权的 PSD 仍算「已注册」。
- **当前可用**：能否在本机预览/派生，是另一条 availability 能力，不作为本次文件树显示范围的判据。
- **所有文件**：扫描根内的所有普通文件，包括无扩展名和未知扩展名；不因此进入媒体库、画廊、搜索或批量操作。
- UI 使用「已注册格式」而不是「支持格式」，避免把「能识别」误解为「当前一定能预览」。

### 二、文件树交互

文件树区块标题右侧增加「显示内容」菜单，提供三种互斥状态：

1. `已注册格式`（默认，保持当前行为和性能）。
2. `所有文件`（不含隐藏项）。
3. `所有文件 + 隐藏项`。

这样正好覆盖需求，不额外制造「已注册格式 + 隐藏项」这一当前扫描契约无法兑现的第四种状态 —— 硬证据：`fast_scan.rs:469` 的 `ensure_dir_chain` 只由已分类文件触发，**只含未知/隐藏文件的目录连目录行都不存在**，不只是「隐藏的已注册媒体未入库」。设置按本机持久化。~~切换后保留能以 `rootId + relPath` 对上的展开节点，失配节点安全收起~~（2026-07-16 修复批 R-18 裁决更正：实现为**切模式折回根、不保留展开态**——节点集合本身变了，硬套旧展开态轻则重展开不存在的键、重则让用户误以为目录空了；键统一〔D-013〕的价值不含跨模式恢复展开）。

隐藏项口径：

- Windows：文件名点前缀或 `FILE_ATTRIBUTE_HIDDEN`。
- macOS/iOS：点前缀，并由平台适配层识别可用的 hidden flag。
- Linux/Android：点前缀。
- 「隐藏项」同时涵盖文件和目录；symlink 不跟随，越出扫描根的目标不可展开/打开。

### 三、文件树数据架构

不把未知文件塞入 `media_items`，也不扩大扫描器和画廊语义；采用双数据源：

- `已注册格式`继续走现有 DB 目录树、媒体计数和文件分页，零回归。
- 两种`所有文件`模式走新的文件系统惰性枚举 IPC；目录展开时才读取直接子项，分页 200。
- 新 IPC 只接受 `rootId + relPath + displayMode + cursor`，后端查可信扫描根、规范化/`canonicalize` 后做根目录边界检查；文件 IO 放 `spawn_blocking`。
- 返回统一 `TreeEntry`：`nodeKey/parentKey/rootId/relPath/name/kind/hidden/registered/directoryId?/mediaType?/mediaId?`。`nodeKey`/`parentKey` 是跨 DB/FS 模式稳定的路径身份；`directoryId`/`mediaId` 只在确有数据库实体时赋值，禁止用路径 hash 或临时序号伪造数据库 id（`mediaRoute.ts:43-47` 对非 doc/audio 兜底到 `/view/{id}`，伪造 id 即误送图片查看器）。
- FS-only 目录允许展开和键盘导航，但没有 `directoryId` 时不得进入 `/folder/:id`、画廊滚动锚点、现有移动/复制 IPC，也不得作为媒体拖拽源或目标；右键菜单只保留刷新/复制路径等不依赖 DB 实体的动作。DB/FS adapter 必须显式区分「树节点身份」与「媒体库实体身份」。
- 超大单目录使用有界 LRU 目录快照缓存支撑稳定分页，切模式/显式刷新/扫描完成时失效；不让每一页都重新全量 `read_dir + sort`。
- 所有文件模式不展示现有「递归媒体数」角标，避免把媒体数冒充文件数；可显示已加载直接项数。
- 卷不可用（移动盘拔出）时 FS 枚举失败 → 给结构化错误 + 空态，不回落 DB 模式（回落会让「所有文件」静默变成「已注册格式」，比报错更坏）。

**排序（D-002）**：FS 枚举沿用现有 DB 模式的 comparator，即目录 `name ASC`（BINARY）在前、文件 `NOCASE ASC` 在后。所有文件模式会插入 FS-only 项，因此完整序列不可能逐字节相等；硬要求改为**共有项相对序稳定（common-subsequence stability）**：切模式前后的共有目录不重排、共有文件不重排，额外 FS 项按同一 comparator 插入。目录共有项对拍 DB 目录查询，文件共有项对拍 `list_directory_files`。既有三序不一致（目录 BINARY / 文件 NOCASE / 画廊 NATURAL_CMP）是独立债，另立线统一到 `natural_cmp`，改完后 FS 侧同步 comparator。

**文件行为（D-001）**：

- 有 `mediaId`：沿用当前应用内查看器路由。
- 无 `mediaId`：单击只选中；Desktop 双击经自有 `reveal_tree_entry(rootId, relPath)` IPC **在文件管理器中显示（reveal）**，不调用系统默认应用打开。IPC 必须复用可信根查询、`canonicalize` 和根边界检查，再由 Rust 内部调用 Opener；不向 WebView 授 `opener:allow-reveal-item-in-dir`。
- Android/iOS 的 Opener 不支持 reveal：UI 隐藏/禁用该动作，后端若被直接调用则返回稳定 `unsupported_platform`，不降级为 `openPath`。
- 未注册文件始终不进入画廊选择、收藏、评分、删除/移动等媒体批处理。
- 隐藏目录内的已注册文件（如隐藏目录里的 PNG）`registered=true` 但**无 `mediaId`**（扫描器 `walker.rs:106` 整棵剪掉，从未见过它）→ 走 reveal 而非应用内查看器。**这是三态设计的必然推论，是有意行为**，不是缺陷。

### 四、顶栏一级筛选

常显顺序调整为：

`图片 | 视频 | 文档 | 音频 | LIVE | 收藏 | 格式…`

- 文档、音频补进 `mediaTypes`；`MediaType` 后端（`format.rs:15-20`）和前端本来就已有四大类，无需新增 DB 枚举。
- 实测支撑：库内文档 4,818 项（主体 txt 4,615）→ 文档 chip 有实际价值；音频仅 32 项 → 近乎空但无害。
- 六个常用 chip 都是一级入口；正常宽度常显，极窄窗口仍服从现有 Priority+ 引擎折入「筛选 ⋯」，不破坏小窗可用性。
- LIVE、收藏不是媒体类型，而是与媒体类型相交的布尔 facet；仅在 UI 上同列，领域模型保持分离。
- `格式…`是一个一级触发器，不把 PNG/TXT 等几十个 chip 直接铺在顶栏。
- 新顺序后 chip 总数为 **10/11**（图片/视频/文档/音频/LIVE/收藏/格式/评分/颜色/日期 + 清除），不是现有的 7/8。

### 五、细分格式弹层

`格式…`打开可搜索、分组的 popover：

- 按图片/视频/文档/音频四组展示当前媒体库实际存在的已注册扩展名，标签统一大写（PNG、TXT、EPUB、PSD）。
- **首版只启用库内实际存在的格式**。当前快照是注册表 67 个（内置 66 + exotic PSD）vs 库内实际 29 个 → 砍掉当前 38 个为 0 的死选项；67 只是当前 Catalog 快照，不是协议常量。以后 Catalog 增加冷门格式，注册表并集和 UI facet 自动扩展，不修改中央计数、白名单或筛选链路。扫描完成或 Catalog snapshot 更新后刷新格式 facet。
- 支持搜索、全选当前组、清除；触发器显示 `格式`或`格式 N`，具体已选项在弹层中回显。
- exotic 格式按注册表出现；availability 只作为「需插件/未授权」等附加 badge，不影响是否能筛出 PSD 项（库内 psd 62 项，可真机验收）。

**格式 facet 取数（D-005）**：`SELECT DISTINCT file_format FROM media_items WHERE is_deleted=0 AND companion_of IS NULL`（实测 **0.0ms**，走 `idx_media_format`）+ **分组在 registry 侧做**。基础谓词与正常媒体视图一致，避免只存在于回收站或 Live Photo companion 的格式污染全局 facet。媒体大类归属是 registry 的知识（`png → Image`），不是 DB 的知识；把 `media_type` 拉进 SQL 要 114ms，再要 counts 要 216ms。

- **首版不显示 per-format counts**（counts 唯一用途是预判结果数，点下去就知道；不值 216ms + 缓存 + 失效一整套）。
- 0ms 的查询**不建缓存层、不做失效逻辑**，每次开弹层现查。
- facet 是**全局口径**（库内存在什么），不随视图收窄。文件夹视图里可能出现选中后 0 结果的格式 —— **这是有意行为**，与评分/颜色 chip 的既有语义一致（评分 chip 在空文件夹里也不消失）。语义定义为「库能筛什么」而非「此处有什么」。

**别名归一（D-003）**：`FormatDescriptor` 带 `group` 字段，把「一个 UI 概念对应多个物理扩展名」统一成一套机制：

- `JPEG` = `{jpg, jpeg}`（实测必需：库内 jpg 515,728 与 jpeg 2,530 **并存**，不归一则 JPEG chip 是个 2,530 项的心智陷阱）。
- `TIFF` = `{tif, tiff}`（同在 `format.rs:39` 一行）。
- `RAW` = `{cr2, cr3, nef, arw, dng, raf, orf, rw2, pef, srw}`。
- 核心状态存**规范化的具体扩展名**，`group` 只是 UI 概念，不写进 DB/API。
- RAW 列表**必须由后端 descriptor 下发**，不得在前端硬编码 —— 那是 `format.rs:47-48` 的复制，且该 match arm 里 `heic/heif/avif` 与 RAW 混在一起，光靠 `media_type` 分不出 RAW。

**RAW 快捷组（D-004）：首版支持，但库内为 0 时不常显**。实测生产库 RAW 与 heic/heif/avif **全部为 0** → 按「只列库内实际存在的格式」规则，本机不会显示 RAW。走 D-003 的 group 机制后，**RAW 是注册数据而非条件代码**：库里出现任一 RAW 扩展名时自动显示 RAW 组；核心状态展开为当前存在且被选中的具体扩展名。

组合语义采用标准 facet：

- 同一维度内 OR：`PNG + JPEG`。
- 不同维度间 AND：`图片 AND (PNG OR JPEG) AND 收藏 AND 评分≥3`。
- 选了媒体大类时，格式弹层只展示兼容组；取消某大类时同步移除该类已选细分格式，杜绝不可见的冲突筛选。
- `mediaTypes=[]`与`fileFormats=[]`分别表示该维度不限；清除筛选同时清空二者。

### 六、筛选契约贯通

新增 `fileFormats`，不能只改画廊 SQL：

- 前端：`filterStore`、`MediaFilter`、`GalleryFilterDto`、`useJustifiedLayout` watch、`useViewDescriptor`、URL codec。
- URL：新增 `formats=png,txt,epub`，小写、去重、长度/数量有界；刷新和深链可复现。**同时修 `galleryQuery.ts:61` encode 侧无白名单的非对称**（现会写出 `types=document` 再被 decode 丢弃）。
- 后端：`MediaFilter.file_formats`、`GalleryFilter.file_formats`、`ViewDescriptor::to_media_filter`。注意二者不是两层独立契约 —— `models.rs:476` `to_media_filter()` 是下沉函数、单一事实源，加字段动这 3 处即可。
- SQL：两个构造点 —— `push_where_predicates`（`queries.rs:1286`，共享 builder，喂 canonical layout / query_layout_items / view_to_sql）与 `search_media`（`queries.rs:2910`）。参数绑定的 `file_format IN (...)`；媒体大类和细分格式取 AND。现成先例：`queries.rs:1817` 已在写 `file_format IN ('pdf','svg')`。
- 选择：SelectAll 描述符必须携带 `fileFormats`，保证批量收藏/评分/删除的目标集与画面一致（`useViewDescriptor.ts:4-7` 自标 🔴 双维护）。
- **DB：不加新索引（D-006）**。`schema.rs:116` 的 `idx_media_format ON media_items(file_format)` 在 `SCHEMA_V1` 内，**所有存量库已有**；实测 EXPLAIN 证明足够（facet 0ms 走覆盖索引；格式谓词与 media_type 同形，走既有路径）。若日后 EXPLAIN 显示格式谓词成为瓶颈再评估 —— 届时须新开 `SCHEMA_V20` + `CURRENT_VERSION` 19→20（`migration.rs:24`），**改 `SCHEMA_V1` 对存量库是 no-op**；且 `queries.rs:1081-1082` 已有 partial index 被规划器误选的前科，需重跑全部 layout 查询的 EXPLAIN。

**B-file-iii 非回归断言（必须写进设计件，防施工时抄错）**：

1. `filter_key` = `MediaFilter` canonical JSON + `data_version`，**仅 `items_cache` 内存态**（`items_cache.rs:8/79/247`；已核实 `hcache.rs` 不含 `filter_key`，无磁盘持久）→ 加 `file_formats` 字段自动改键、正确失效，**无需缓存迁移**。
2. `GlobalFilenameRank` 的 filter-invariance（`items_cache.rs:139-145`：自然序是全序，限制到任意子集保持相对序）**对格式筛选依然成立** —— 格式筛选是默认可见基集的子集，**全局 rank 不需重建**。加对拍测试：格式筛选下 filename 序 ≡ 全局 rank 派生序。
3. `items_cache.rs:525` `sensitive(&filter)` 的 patch 路径（favorite/rating/color）**不纳入格式**。那三个字段有 patch 语义是因为运行时会变；**文件格式运行时不变**。

### 七、实施批次

#### P0：可扩展注册表与契约测试

- 把内置格式从仅可匹配的 `match` 收敛为内部可枚举 `RegisteredFormatDef`，`classify_media_type`、`is_phase1_image`、`doc_subtype` 均从同一事实源派生：
  ```
  ext: &'static str
  media_type: MediaType
  group: Option<&'static str>
  phase1_image: bool
  document_subtype: Option<DocumentSubtype>
  ```
- 与 exotic Catalog（`catalog.rs:286` `iter_formats()` 已可枚举）在运行时合并，投影成只读 IPC `FormatDescriptor`：
  ```
  ext: String            // 规范化小写扩展名，DB/API 存这个
  media_type: MediaType  // 四大类
  group: Option<String>  // 显示分组：JPEG={jpg,jpeg} / TIFF={tif,tiff} / RAW={cr2,...}
  source: Builtin | Exotic { plugin_id: String }
  ```
  `group` 是必需的：`format.rs:47-48` 把 `heic/heif/avif` 与十个 RAW 塞在同一 match arm，`media_type` 分不出 RAW。内部处理字段不下发，避免把扫描/派生能力误当 UI facet。
- **未来 exotic 扩展预留（D-007）**：不预留硬编码槽位或固定总数，而是预留动态注册协议。新增普通冷门格式只需在 Catalog 增加 offering 的 `plugin_id/media_kind/formats`；合并 registry、facet、URL/SQL 筛选及 parity 测试自动覆盖。**Catalog loader 的校验已全部存在，P0 不新建**：`catalog.rs:223-224` `CommonFormatConflict`（不得覆盖 common）、`:245-247` `DuplicateFormat`（跨 offering 重复扩展名）、`:217-218` `InvalidFormat`（`is_valid_format` 限 `[a-z0-9]{1,16}`，即**拒绝**非小写而非转换 —— `:226` 原样克隆，`norm_formats` 变量名与 `:214` 注释是误称）、`:201-211` plugin_id/重复 plugin/override_common/空 capabilities。P0 只需复用，并在合并层补 builtin↔Catalog 的交叉校验。当前 Catalog 的一个 offering 已可声明多个格式，但这只表示同一插件处理它们，**不自动等于一个 UI alias group**；未来若冷门格式需要别名合组，再给 Catalog schema 增加显式可选 group 元数据，禁止从 offering 边界猜测。若某 exotic 需要新的 `document_subtype` 或 Phase 1 能力，那是处理能力 schema 的独立扩展，不偷塞进 UI descriptor。
- **收敛范围（D-007）= 5 处**：`classify_media_type`(`format.rs:35-70`)、`is_phase1_image`(`:74`)、`doc_subtype`(`:89`)、exotic `iter_formats()`(`catalog.rs:286`)、前端 `src/constants/formats.ts`（零 importer 死镜像，**删除或改由 IPC 灌**，否则 P3 会被顺手 import 变成第三事实源）。
- **有意不收**：`derive/kind.rs:128` `DOC_THUMB_FORMATS`、`thumbnail/cache.rs:416` `AUDIO_COVER_EXTS`、`video/media_foundation.rs:32` `MF_VIDEO_EXTS`、`ai/face_pipeline.rs:611` `WORKER_DECODABLE_FORMATS` —— 它们回答的是「能不能派生/解码」= availability 维度，与「是不是已注册」正交（§1 已切开）。强收会把两个概念揉死。
- **测试（必须，不硬编码 67）**：①枚举全部 builtin 定义，双向对拍 `builtin registry ≡ classify_media_type`；②对当前 Catalog snapshot 的每个扩展名验证 common 返回 `None`、Catalog 返回声明媒体大类（**注**：装载期 `CommonFormatConflict` 已结构性保证「Catalog 不含 common 格式」—— 违反者载不进 snapshot，故本项是 characterization 而非新契约，仍保留以锁定 common-first 语义）；③动态合并集对拍 common-first combined classifier，集合基数由 builtin/Catalog 实际数据推导；④`is_phase1_image` / `doc_subtype` 从 builtin registry 派生后逐值等价。新增第二个 exotic fixture，证明无需修改中央白名单或期望总数即可自动进入合并集和 facet。
- 锁定 common-first、当前 PSD Catalog、未来多 exotic、group 定义和四大类映射。

#### P1：文件树三态

- 新增受限文件系统枚举 IPC、平台隐藏判定、稳定分页缓存。
- 排序沿用 DB comparator（D-002），分别对拍共有目录/共有文件，并验证插入 FS-only 项后共有项的相对序不变。
- 注册 Rust `tauri-plugin-opener`（Cargo 依赖 + `lib.rs` 注册；JS 侧 `package.json:31` 已就位但本链路不直接调用），新增受限 `reveal_tree_entry(rootId, relPath)` IPC；**不授** `opener:allow-reveal-item-in-dir`。顺带更新 `PluginGate.vue:141` 那条「Rust 端未注册」的旧注释。
- 在 `useFolderTree` 引入 DB/FS adapter 和路径稳定 `nodeKey/parentKey`；扩展虚拟树行模型与未知文件图标/行为，显式隔离可选 `directoryId/mediaId`。FS-only 目录不得进入依赖 DB id 的路由、滚动锚点、拖拽和媒体操作。
- 加区块内显示菜单与持久化，保留现有 DB 快路径。
- 卷不可用降级：结构化错误 + 空态，不回落 DB 模式。

#### P2：一级媒体类型

- 补文档/音频 chip、i18n、动态 chip 索引与 Priority+ 测量。
- **描述符化范围（D-008）= 3 处**（方案原只点了第 1 处）：
  1. `AppToolbar.vue:341` `chipCount` 7/8 → 由描述符数组推导（新值 10/11）；
  2. `GalleryFilterChips.vue:11-101` `inDom(0)…folded(7)` 模板字面量索引 → 数组驱动；
  3. `AppToolbar.vue:310-321` `overflowRemeasureKey` 手工枚举筛选字段 → 由描述符派生。**第 3 处漏配无编译/SSR/测试信号**，只在真机表现为「格式」→「格式 3」变宽不触发重测 = 闪展再收同类 bug。
- 顺手归一 i18n（零成本）：`GalleryFilterChips.vue:45` Live chip 裸字面量补 key；视频 chip 从 `sidebar.videos` 迁到 `toolbar.*`，与图片 chip 同 namespace。
- URL `types` 白名单扩为四大类，并修 encode 侧非对称。

#### P3：细分格式全链

- 新增 `fileFormats` 前后端契约、SQL（两个构造点）、ViewDescriptor、缓存 key、搜索和 URL。
- 新增格式 facet IPC（带 `is_deleted=0 AND companion_of IS NULL` 的 `DISTINCT file_format`，无缓存无 counts）与 `FormatFilterPopover`，接 D-003 的 group 别名机制与 exotic availability badge。
- 加 B-file-iii 非回归对拍（§6 断言 2）。

#### P4：验证、真机与文档收口

- focused tests 通过后，因涉及共享筛选/选择契约与 DB 查询，执行完整前端与 Rust suite、typecheck、ESLint、rustfmt、clippy、build 和 docs gate。
- Windows 真机验收隐藏属性/点文件、受限 IPC 未知文件 reveal、超大目录分页、窄窗折叠、CN/EN 文案、PSD 格式筛选。
- macOS 验证 reveal；iOS/Android 验证动作不呈现或禁用、直调返回 `unsupported_platform`。无设备时只给 compile evidence 与明确手测步骤，不伪称 GUI 已验证。

## 验收矩阵

- 文件树默认状态与当前完全一致；切所有文件后可见未知扩展名和无扩展名文件。
- 切换显示模式时，共有目录和共有文件的**相对顺序分别不变**；新增 FS-only 项按相同 comparator 插入（D-002 common-subsequence 对拍）。
- 关闭隐藏时点前缀、Windows hidden attribute、隐藏目录均不可见；开启后均出现。
- 未注册文件不进入画廊/搜索/收藏/批量操作；已注册文件仍能从树内打开。
- Desktop 未注册文件双击经受限 `rootId + relPath` IPC reveal，不触发任何外部程序执行；越界路径被拒绝。Android/iOS 不提供 reveal，直调稳定返回 `unsupported_platform`。
- 文档、音频一级 chip 可单选/多选、URL 刷新可恢复；`types=document` 的 encode/decode 往返对称。
- PNG/TXT/EPUB/PSD 可筛；JPEG chip 同时命中 `.jpg` 与 `.jpeg`（库内 515,728 + 2,530）；组合 AND/OR 语义与 UI 回显一致。
- 用 CR2+NEF fixture 验证 RAW 组自动出现并同时命中两者；URL/核心状态只存具体扩展名。无 RAW 的生产库不显示 RAW。
- 当前生产库格式弹层只列基础谓词下存在的 29 个格式，不列当前另外 38 个；只存在于回收站/companion 的 fixture 格式不出现。新增 exotic fixture 后，注册表并集和 facet 无需改中央白名单即可自动扩展。
- 筛选后的 Ctrl+A/批量操作目标数与可见集合一致。
- 格式筛选下 filename 排序序 ≡ 全局 rank 派生序（B-file-iii 非回归）。
- 100k 单目录分页不一次灌入前端，文件树 DOM 仍受既有虚拟化窗口约束。
- 切筛选不恢复既有「计数变宽导致顶栏闪展再收」回归（含「格式 N」文字变宽路径）。

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 「已注册格式/所有文件/所有文件+隐藏项」三态，而非让隐藏开关与注册范围任意组合 | 精确覆盖需求并保持默认 DB 快路径；第四态无法兑现的硬证据是 `fast_scan.rs:469` —— 只含未知/隐藏文件的目录连**目录行**都不存在 | D-009 |
| 未知文件永不进入媒体批处理，且不伪造 `mediaId` | 保持文件树可用又不污染媒体库领域模型；`mediaRoute.ts:43-47` 对非 doc/audio 兜底 `/view/{id}`，伪造 id 即误送图片查看器 | D-010 |
| 大类与细分格式跨维度 AND、维度内 OR | 符合标准 facet，并能与收藏/LIVE/评分等现有筛选一致组合 | D-011 |
| 细分格式仅列库内实际存在项（当前 29/当前 registry 快照 67），核心状态存具体扩展名 | 当前砍掉 38 个为 0 的选项；registry 总数从 builtin + Catalog 动态推导，UI 别名不进 DB/API | D-012 |
| **未注册文件 Desktop 双击经自有受限 IPC reveal，不走系统默认应用 open；不向 WebView 授通用 reveal 权限；移动端返回 unsupported** | 黑名单挡不住根内可执行文件；Tauri 的 `opener:allow-reveal-item-in-dir` 无预配置 scope，直接授给 WebView 会绕过扫描根边界；Rust 自有 IPC 可复用 `rootId + relPath` 校验，且能统一表达 Android/iOS 不支持 | D-001 |
| **FS 模式沿用现有 DB comparator，共有项保持 common-subsequence 稳定**（目录 BINARY / 文件 NOCASE）；三序统一 `natural_cmp` 另立线 | 所有文件模式必然插入额外项，完整序列逐字节相等不可实现；真正的交互不变量是共有目录/文件不相互重排，且可分对象对拍 | D-002 |
| **别名归一 = `FormatDescriptor.group`**（JPEG/TIFF/RAW 同一机制），不入库归一 | 实测 jpg 515,728 与 jpeg 2,530 并存，不归一即 2,530 项心智陷阱；入库归一会让 DB 与磁盘失真且不可逆；JPEG 别名与 RAW 分组本是同一问题（一个 UI 概念对多个物理扩展名），不需要第二套机制 | D-003 |
| **RAW 首版支持但零数据时不常显**，registry 以 `group` 数据表达 | 实测库内 RAW 与 heic/heif/avif 全部为 0；库里出现 `.cr2`/`.nef` 时自动出现 RAW 组，fixture 锁定该能力，避免“首版不做”与用户需求歧义 | D-004 |
| **格式 facet = 带正常媒体基础谓词的 `DISTINCT file_format` + registry 分组，无 counts、无缓存** | 基础谓词阻止回收站/companion 独有格式污染全局 facet；实测仍为 0.0ms，媒体大类归属是 registry 知识，counts 不值得引入缓存与失效逻辑 | D-005 |
| **不加新 `file_format` 索引** | `schema.rs:116` `idx_media_format` 在 `SCHEMA_V1`，存量库全有；实测 EXPLAIN 已足；`queries.rs:1081-1082` 有 partial index 被规划器误选的前科；新索引须 `SCHEMA_V20`+`CURRENT_VERSION` 19→20（改 V1 对存量库 no-op） | D-006 |
| **内部 `RegisteredFormatDef` + 动态 Catalog 合并 + 投影 `FormatDescriptor`；parity 不硬编码 67**；有意不收 4 处 availability 表 | 当前 66 common + 1 PSD 只是数据快照。新增普通 exotic 只改 Catalog，合并 registry/facet/测试自动扩展；内部 phase1/doc subtype 字段才能真正收敛五处事实源，UI DTO 不承载处理能力 | D-007 |
| **顶栏 chip 全量描述符化 3 处**（含 `overflowRemeasureKey`） | 方案原只点 `chipCount`，漏 `GalleryFilterChips` 索引字面量与 `overflowRemeasureKey`；后者漏配**无编译/SSR/测试信号**，只在真机抖动，且顶栏已踩过一次同类（round7 docked 分支漏配） | D-008 |
| **树节点路径身份与 DB 实体身份分离**：`nodeKey/parentKey` 必有，`directoryId/mediaId` 可选 | FS-only 目录没有 DB id；当前路由、滚动锚点、拖拽和移动/复制都依赖数值 id。伪造 id 会误路由或误操作，adapter 必须显式限制无实体节点能力 | D-013 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| findings 原记「`schema.rs:117-125` 无 `file_format` 索引」→ 方案据此设计「加 partial index 前先 EXPLAIN」批次 | 引用区间 117-125 起点恰好切掉 `:116` 的 `idx_media_format`；结论在被引区间内为真、在文件里为假 | 复核轮回读 `schema.rs:1-126` 全段发现；就地更正 findings + 改 §6 为「不加新索引」+ 补 `SCHEMA_V20` 陷阱注记（D-006、F-008） |
| 复核轮据记忆断言「D 号用到 D-020、本任务从 D-021 起」 | 从 `docs/` 全局 grep 到 `D-009..D-020` 即推全局递增，未查这些 ID 的**来源文件** | 实测 `D-009..D-020` 全部来自 `2026-07-14-统一文件树与画廊目录排序/` 单一任务，且 `docs/worklogs/` 另有独立 D-001..D-004 → **D 号与 F 号同为任务内局部编号**；本任务从 D-001 起 |