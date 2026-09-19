---
id: 2026-06-26-Part2_扫描与画廊流水线
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 2 · 扫描与画廊流水线

> 依赖：[Part0](Part0_总纲与产品定稿.md)（尤其 §6 卷可用性、§11 波次、§12 约定）、[Part1 数据层](Part1_数据层.md)（消费其 `volumes`/`availability`/`backend_id` DAO、复合索引、keyset）。
> 状态：定稿待执行（已并入 terminal review；正文即权威）。执行前必读 Part0 §13 + Part1 §7 + 本文。**旧 docs/记忆不可轻信，以代码实测为准。**

---

## §1 目标与范围

### 1.1 本 Part 解决什么

扫描与画廊布局是「百万级流畅浏览」核心卖点的引擎。本 Part 交付：

1. **卷探测与插拔感知**（落实 Part0 §6 的行为侧）：`scanner/volume_probe.rs`（Win 卷 GUID / mac 卷 UUID 枚举）+ 后台插拔监听线程（`WM_DEVICECHANGE` / DiskArbitration + 轮询兜底）+ 扫描编排**离线守门**。消费 Part1 的 `volumes` DAO。
2. 🔴 **缺失检测（挂卷在线守门）**（Part0 §1.3 删除检测缺失 + §6 离线≠删除）：fast_scan 后差集标记缺失项 `availability='missing'`（🔴 第 8 轮 P0-1：**非 `is_deleted`**，与用户删除分离、支持重现自动恢复，§3.2.4），但**仅对在线卷生效** + 写前再校验在线（防 TOCTOU）+ fast_scan 入口卷离线即 `return`。**这是会上社区头条的数据安全点。**
3. 🔴 **SourceChanged 重 enrich**（Part0 §1.3 矛盾、元数据永不更新）：文件变更时(✅ 2026-07-02 起为 mtime→size→content_hash 三层判据,§3.4)删旧 `image/video/audio_meta` 并重置 enrichment 状态，使 EXIF/日期/GPS/尺寸真正更新。
4. **增量扫描**：跳过未变更文件（避免百万文件全量重处理）〔✅ 2026-07-02 全量落地:(mtime,size) 双字段判据 + content_hash 三环二次确认,见 §3.4 横幅〕。
5. **live/motion photo 配对强化**：补 HEIC（现代 iPhone 默认）+ 评估 ContentIdentifier 双重验证（Part0 §1.3 矛盾 #... plan 称双重验证实际只茎匹配）。
6. 🔴 **坐标平移滚动 bug 修复**（Part0 §1.3 矛盾 #3，「百万级」当前未真正达成的根因）：解决 >~25 万项时滚动错位，坐实核心卖点。〔现状：正确性已修（overflow:hidden 兜底）；顺滑度走 T16 方案B，已排期 2026-07-02，见 §3.6 横幅〕
7. **百万级布局加固**：justified 布局 + 驻留缓存 + O(1) `id→(row,col)` 索引；fast_scan 内存峰值（流式分块，避免百万 `WalkedFile` 全持内存）。
8. **时间轴滚动条（后端支持）**：年/月密度直方图查询，供 Part5 右侧 time scrubber UX。
9. **零散正确性修复**：`justified.rs` 日期标签本地时区〔🔴 已推翻暂缓：naive Local 双重偏移有害，现行有意保持 UTC，见 §3.9.1 横幅/T9 行〕；TIFF 解析超时保护（Part0 §1.3 矛盾 plan 称 tokio 超时实际只 thread::scope）；fire-and-forget 异常经 Channel 上报（避免 UI 永久 running）。

### 1.2 不在本 Part（归属其它 Part）

- 卷**数据模型/DAO/迁移**（`volumes` 表、`availability` 列、`bulk_set_availability`）→ **Part1**（本 Part 消费）。
- 画廊**前端渲染**（卡片、多选、星级、拖放、时间轴滚动条 UI、离线灰显/角标）→ **Part5**（本 Part 提供布局数据 + 时间直方图 + 离线态字段）。
- 缩略图/派生**生成**（含 `apply_thumb_results` 的生成侧）→ **Part3**（本 Part 只管布局缓存的 O(batch) 回填接口）。
- AI/人脸**分析触发**（enrichment 让步阶梯里的 AI/face）→ Part4（本 Part 维持现有让步阶梯，不改 AI 内部）。
- IPC 命令错误 `String`→`AppError` 统一 → Part5（本 Part 新增命令直接用 `AppError`）。

### 1.3 全局约定（继承 Part0 §12）

让步阶梯：派生/AI 对交互让步（`note_interaction`），**enrichment 绝不让步**（会与 2s 自动重排反馈循环饿死导入）→ `reserved_core_pool`；rayon 内永不 `.await`；面向用户查询追加 `is_deleted=0 AND companion_of IS NULL`；新增 DAO/命令返回 `AppError`；中英双语注释；改后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（代码取证，文件:行）

> 本节为 workflow `wm59xvwwe`（3 agent 并行测绘扫描/布局/坐标平移）回传结果，逐项 file:line 核实。**与旧 plan-docs 冲突处，以本节为准**（已纠正旧 plan 多处不实陈述，见各小节 ⚠️）。

### 2.1 扫描两阶段流水线

**Phase 1 · fast_scan**（[fast_scan.rs](../../src-tauri/src/scanner/fast_scan.rs) 1-463）：

- 单线程 `walkdir` → 全部文件收进 `Vec<WalkedFile>` 常驻内存（[walker.rs](../../src-tauri/src/scanner/walker.rs) 64-129）→ `order_for_view` 排序（**再 clone 一遍**）。
- 前 500 项 `par_iter`（rayon 全局池）提取真实像素尺寸（含 JPEG orientation）；其余非图片给廉价占位比例（16:9 / 400×400 / 595×842），Phase1 图片给 `(0,0)` 占位。
- 分 500 行批写 `media_items`（INSERT + UPDATE，每批一个 `unchecked_transaction`，fast_scan.rs:361/415）。
- **仅图片类型在此拿到入库宽高**；视频/音频/文档维度留待 Phase 2。

**Phase 2 · enricher**（[enricher.rs](../../src-tauri/src/scanner/enricher.rs) 80-554，整体在 `spawn_blocking`）串行三段循环：

| 段 | 过滤条件 | 批 | 池 | 写入 |
|----|---------|----|----|------|
| ① 图片 | `image_meta IS NULL`（enricher.rs:99-103） | 500 `par_iter` | **rayon 全局池**（无 reserved 保护） | `image_meta` + `update_media_dimensions`(补0×0) + `update_sort_datetime` + `update_live_photo_flags` → 循环直到空 → 调一次 `pair_live_photos` |
| ② 视频 | `vm.item_id IS NULL`（queries.rs:1302） | 500 `par_iter` | `reserved_core_pool` | `video_meta` + `update_video_dimensions` |
| ③ 音频 | `am.item_id IS NULL`（queries.rs:1333） | 500 `par_iter` | `reserved_core_pool` | `audio_meta` + `duration` |

批常量：`BATCH_SIZE=500`（fast_scan.rs:44）、`ENRICHMENT_BATCH=500`（enricher.rs:33）。

### 2.2 增量 / upsert / SourceChanged

`upsert_fast_scan_item`（[queries.rs](../../src-tauri/src/db/queries.rs) 536-597）三分支：

1. `SELECT id,file_mtime WHERE directory_id=?1 AND file_name=?2`；
2. 存在且 **mtime 相同** → `Unchanged(id)`，fast_scan.rs:407 直接 `continue`（不更新 DB、不触发 enrichment）；
3. 存在且 **mtime 不同** → `UPDATE … thumb_status=0/thumb_path=NULL/thumbhash=NULL` → `SourceChanged(id)`；
4. 不存在 → `INSERT` → `Inserted(rowid)`。

- 增量判据：~~**仅 mtime**（`std::fs::Metadata::modified` as unix secs，walker.rs:99-104）。`content_hash` 字段存在 schema 但 upsert **不读不写**；`file_size` 仅在 SourceChanged 时被写新值、**不参与比较**~~〔本行为历史快照;✅ 2026-07-02 已升级三层判据,见 §3.4 横幅〕。
- 🔴 **真 bug #1（SourceChanged 不清旧 meta）**（queries.rs:553-573 + enricher.rs:99-103）：SourceChanged 只重置缩略图三字段，**不删 `image/video/audio_meta` 旧行**。enricher 用 `… IS NULL` 过滤 → 旧 meta 行仍在 → 永不被重选 → **文件替换后 EXIF / 拍摄日期 / GPS / 时长 / 编码信息永久停留在旧值**。（对比：exotic 任务走 `invalidate_exotic_tasks_for_item` 正确退回 pending，fast_scan.rs:402——普通 meta 没有对应失效逻辑。）

### 2.3 删除检测（当前：完全无）

🔴 **真 bug #2（无删除检测）**（fast_scan 全段 + queries.rs:155-162）：walker 只做正向发现，upsert 只 insert/update；扫描结束**不遍历 DB 找「本次未出现」的旧行**、不算差集。`is_deleted`（queries.rs:55/1593）**仅**由用户手动「移入垃圾桶」`trash_media_items` 写入。`finish_scan_root` 只更新 `scan_status/total_files`，不做差集标记。→ **磁盘已删除的文件永远滞留画廊**。

### 2.4 live / motion photo 配对

- 配对策略：**纯文件名茎匹配**（[live_photo.rs](../../src-tauri/src/scanner/live_photo.rs) 39-114）。查 `file_format IN ('jpg','jpeg','mov') AND companion_of IS NULL` → 按 `(directory_id, file_stem)` 分组 → 同组同时有 jpeg+mov → JPEG 标 `is_live_photo=1`，MOV 标 `companion_of=jpeg_id`。`file_stem` 取 `rsplit_once('.')`（live_photo.rs:61-64）纯字符串比较。
- ⚠️ **旧 plan 称「ContentIdentifier 双重验证」不实**：实际**不读** EXIF `ContentIdentifier`（Apple 官方配对依据）。同名不同拍会误配，跨目录/重命名会漏配。
- ⚠️ **不支持 HEIC**（live_photo.rs:43-53 仅 `jpg/jpeg/mov`）——现代 iPhone 默认 HEIC，**当前 Live Photo 基本失效**。
- Motion Photo（Google/Samsung 嵌入式）：`detect_motion_photo_xmp`（metadata.rs:185-189/219-240）仅设 `is_live_photo=1/has_embedded_video=1` flag，**不配对 companion video**，且仅限 jpg/jpeg。

### 2.5 让步模型与 reserved_core_pool

- `reserved_core_pool()`（enricher.rs:45-50）：`available_parallelism().saturating_sub(1).max(1)` 线程的私有 rayon 池。仅视频段（enricher.rs:356）/音频段（enricher.rs:474）使用；**图片段未用，直接占全局池**（issue ⑧）。
- 🔑 **enrichment 绝不对交互让步**（enricher.rs:45-50 注释）：enrichment 是用户等待的导入工作，进度事件自动触发 relayout；若对交互让步会与「2s 自动重排反馈循环」互相饿死 → 改用「保留 1 核给前台」策略，既保 UI 跟手又不拖慢导入。
- `note_interaction` 由 layout_commands.rs:46/128/144（`compute_layout`/`get_layout_rows`/`get_layout_rows_by_y`）触发 `INTERACTIVE_WINDOW_MS` 窗口；期间 `should_yield_derivation()`/`should_yield_exotic()` 返回 true，**派生/AI/exotic 让步，enrichment 不参与此路径**。（继承 Part0 §12，本 Part 不改让步阶梯。）

### 2.6 布局算法 + 缓存 + O(1) 索引

**justified 算法**（[justified.rs](../../src-tauri/src/layout/justified.rs) 96-405，整个 query+算法在 `spawn_blocking` 单任务内 layout_commands.rs:69-102）：

- 常量：`SEPARATOR_HEIGHT=36`、`LAST_ROW_JUSTIFY_THRESHOLD=0.6`（末行 `ar_sum*target_h < available_w*0.6` 不拉伸）、`MAX_ROW_HEIGHT_FACTOR=2.0`（行高上限=target×2，防极宽容器单竖图撑到 6000px）。
- 每项 `aspect_ratio` clamp(0.2, 5.0)；未测量 0×0 项用 `median_measured_aspect` O(n)（`select_nth_unstable`，justified.rs:381）。
- 整数像素误差按比例左→右逐位 ±1 分配；`row_h.ceil()` 写入行高，`y += row_h.ceil() + gap`。

**缓存**（[cache.rs](../../src-tauri/src/layout/cache.rs) 49-68，`LayoutCache = RwLock<Option<LayoutCacheData>>`）：

- `rows`（含分隔符）/ `total_height` / `layout_version`（全局原子 `fetch_add+1`）/ `total_items` / `flat_ids` / `flat_rowcol`（平行于 flat_ids：扁平下标→(行,行内列)）/ `id_to_flat`（HashMap）。
- `LayoutRowItem`（justified.rs:73-91）**最小投影 17 字段**（id/x/w/h/file_size/format/type/is_live/duration/thumb_*/thumbhash/favorited/similarity/orig_w/orig_h/sort_datetime），**无** dir_path/exif/gps/filename 等重型字段（重型走 `get_meta_for_viewport` 按需拉，MediaGrid.vue:1165）。

**O(1) 索引**（store_layout 一次遍历建三并行结构 O(N)）：

| 操作 | 复杂度 | 路径 |
|------|--------|------|
| `apply_thumb_results` | O(batch) | id_to_flat→flat→flat_rowcol→rows[ri].items[ii] 直写（取代旧 O(rows×items×results) 全表扫描，cache.rs:115-136） |
| `set_favorite_in_cache` | O(batch) | 同路径（cache.rs:143-162） |
| `get_adjacent_item` | O(1) | id_to_flat→flat_ids[idx±offset] |
| `get_item_y_by_id` | O(1) | id_to_flat→flat_rowcol→rows[ri].y() |
| `get_rows_by_y` | O(log N + 命中行) | `binary_search_by rows[].y()` + 线性扩展（cache.rs:192-222） |

### 2.7 坐标平移机制 + 滚动 bug 根因

**机制**（[useVirtualScroll.ts](../../src/composables/useVirtualScroll.ts)）：`SAFE_MAX=10_000_000`（约 25 万项触发，行 40px 时）；`logicalTotal > SAFE_MAX` → `isTranslated=true`。`spacerHeight` 钉死 `SAFE_MAX`；`ratio = logMax/physMax` 纯线性；每帧命令式写 `layer.style.transform`（不触发 Vue 重渲染）；`renderAnchor = floor(requestTop)`，行在层内 `translate3d(0, row.y - renderAnchor, 0)`。

🔴 **滚动 bug 根因**（核心不变量 `scrollHeight == spacerHeight` 被破坏）：

1. 普通模式：`renderAnchor ≈ scrollTop`，`row.y - renderAnchor` 是小正数，行恒落 `[0, spacerHeight]` 内 → 不变量成立。
2. 平移模式：`renderAnchor` 可达 ~11,000,000，而底部缓冲行（bufferH ~1200px）`translate` 后位置**超出 `SAFE_MAX`**；`.media-grid__content`（修复前无裁剪）是绝对定位后代的包含块，浏览器把出界行纳入 **scrollable overflow** → `scrollHeight` 膨胀（>spacerHeight）。
3. 映射分母 `physMax = spacerHeight - viewH` 失真 → `physicalToLogical(scrollTop)` 越界 → 触发更偏下的 `fetchRowsByY` → 新行又被 translate 到更外侧 → **正反馈循环**：滚动条忽长忽短、内容跳底、行错位。

**浏览器物理天花板**（治标 vs 治本的边界）：

| 引擎 | 单元素高度上限 |
|------|---------------|
| Chromium / **WebView2** / Chromium-Edge | **16,777,216 px (2²⁴)** |
| Firefox | 17,187,496 px |
| Safari | 33,554,428 px |

> 超限后浏览器把高度钳为 0 或截断，`scrollTop` 到不了底。线性平移把逻辑高度「压」进 `SAFE_MAX` 以规避，但**代价是滚动条粒度变粗**（百万项 ~4×，每 1px ≈ 4 行）；bucket 分段（Immich）才治本，但单 bucket 仍受同一上限约束（Immich issue #28861：单月 50k+ 资产、3,764,850px 仍触顶）。

✅ **关键修正**：核心修复 **`overflow:hidden` 已加**（MediaGrid.vue:1310，裁剪渲染层在 `[0, spacerHeight]` 内，恒等不变量），代码已提交，正确性兜底仍在。🔴 **「待运行时验证 / 中期演进」定性已被推翻（2026-07-02 回写）**：真机验证已做（2026-06-29），方案A 顺滑度结构性治不好、三轮优化补丁全 revert（`ba084b5`）；方案B 经 R1-6 拍板正式排期（2026-07-02，~2 周，见 [T16 评估文档](T16_方案B_bucket分段_可行性与改造范围评估.md)）。→ §3.6 原「验证 + 防御加固 + 中期 bucket 演进」框架随之过期，见该节横幅。

### 2.8 时间轴现状

- 已有 mini-timeline 侧栏（MediaGrid.vue:119-132）：以 `separators` 列表为数据源，每分隔符一圆点；`top = sep.y / totalHeight`（**按逻辑高度线性，非时间密度**）；点击 `scrollToY(sep.y)`。侧栏宽 24px 可折叠。
- 缺：无年/月直方图（圆点等权，不反映密度）；date 模式 `group_id=None`（cache.rs:285，只能用 y 跳转、无法按日期 label 查）；无年份刻度文字；无月/年聚合层级（百万库数千节点密度失控）；节点按高度均布而非时间均布。

### 2.9 现状问题清单（编号 → §3 设计逐一对应）

| # | 问题 | 证据 | 对应设计 |
|---|------|------|---------|
| P1 | SourceChanged 不清旧 meta，元数据永不更新 | queries.rs:547-573 / enricher.rs:99-103,1302,1333 | §3.3 |
| P2 | 无删除检测，删档滞留 | fast_scan 全段 / queries.rs:155-162 | §3.2 |
| P3 | 百万级 fast_scan 内存峰值（全量 Vec + clone 双倍，~600MB@100万） | walker.rs:64-129 / fast_scan.rs:290-310 | §3.7 |
| ~~P4~~ ✅ | TIFF 无真超时（thread::scope join 仍阻塞）→ 已修 commit 9938651 | metadata.rs | §3.9.2 |
| P5 | HEIC Live Photo 不支持 | live_photo.rs:43-53 | §3.5 |
| P6 | Live Photo 不读 ContentIdentifier，误配/漏配 | live_photo.rs:39-114 | §3.5 |
| P7 ✅ | Motion Photo 只置 flag、不配 companion（T15 已补分体式 mp4 守门配对） | live_photo.rs:39-161 | §3.5.3 |
| P8 | 图片 enrichment 段用全局池、无 reserved 保护 | enricher.rs:175-215 | §3.7 |
| P9 | 日期标签强制 UTC〔T9 核验：对有 EXIF 多数照片 UTC 恰好正确，naive Local 有害→暂缓〕 | justified.rs:397-405 | §3.9 |
| P10 | 坐标平移滚动错位（正确性已修；顺滑度结构性问题转 T16 方案B，已排期 2026-07-02） | useVirtualScroll.ts / MediaGrid.vue:1310 | §3.6 |
| P11 | 时间轴无密度直方图/年月聚合/刻度 | MediaGrid.vue:119-132 / cache.rs:230-285 | §3.8 |
| P12 | 卷离线无感知（插拔/离线守门缺失） | （Part0 §6，本 Part 落实行为侧） | §3.1 §3.2 |

---

## §3 设计方案

> 设计原则：**数据安全 > 卖点 > 体验 > 性能微调**。P1/P2（元数据停滞、删档滞留）与 P12（卷离线守门）是数据正确性红线，先落。坐标平移（P10）卖点核心、正确性修复已在；顺滑度经真机否定方案A，现走 T16 方案B（已排期）。所有新增 DAO/命令返回 `AppError`，面向用户查询续 `is_deleted=0 AND companion_of IS NULL`。

### 3.1 卷探测 + 插拔监听（落实 Part0 §6 行为侧）

消费 Part1 的 `volumes` 表 / `upsert_volume` / `set_volume_online` / `bulk_set_availability` DAO。本 Part 新增 `scanner/volume_probe.rs` + 后台监听线程。

**3.1.1 卷稳定 ID 枚举**（`volume_probe.rs`，跨平台 trait）：

```rust
/// 卷稳定标识：插拔后仍能复认同一物理卷。
/// Win = 卷 GUID 路径(\\?\Volume{GUID}\)；mac = 卷 UUID(DADiskCopyDescription)。
pub struct ProbedVolume {
    pub stable_id: String,   // 写入 volumes.stable_id
    pub mount_path: String,  // 当前挂载点(盘符/挂载目录)，写 last_mount_path
    pub label: Option<String>,
    pub kind: VolumeKind,    // Fixed/Removable/Network/Unknown
    pub is_online: bool,
}
pub trait VolumeProber {
    fn enumerate(&self) -> Result<Vec<ProbedVolume>, AppError>;
    fn resolve_path(&self, path: &str) -> Result<Option<String>, AppError>; // 绝对路径→stable_id
}
```

- **Win**：`GetVolumePathName`（路径→挂载根）→ `GetVolumeNameForVolumeMountPoint`（根→`\\?\Volume{GUID}\`）→ `GetVolumeInformation`（label/序列号兜底）。removable 判定 `GetDriveType`。
- **mac**：DiskArbitration `DADiskCopyDescription` 取 `DAVolumeUUID`；`statfs` 取挂载点；`f_flags & MNT_REMOVABLE`/网络卷判定。
- **兜底**：取不到 GUID/UUID（异常文件系统/某些网络盘）→ `stable_id = "path:" + normalize_root_path(规范化绝对根)`（与 Part1 §3.3 网络盘 UNC 规范化一致），保证至少不崩、退化为路径级标识。

**3.1.2 插拔监听线程**（`scanner/volume_watch.rs`，单后台线程，启动时 spawn）：

- **Win**：隐藏窗口收 `WM_DEVICECHANGE`（`DBT_DEVICEARRIVAL`/`DBT_DEVICEREMOVECOMPLETE`）→ 防抖 500ms → 重新 `enumerate` → diff → 写 `set_volume_online` → 发事件 `volume-availability-changed`。
- **mac**：DiskArbitration 注册 `DARegisterDiskAppearedCallback`/`DiskDisappeared` → 同上。
- 🔑 **轮询兜底**（两平台都挂）：每 15s `enumerate` 一次比对（防事件丢失/网络盘无设备事件）。事件驱动为主、轮询为补。
- 监听线程**只读卷状态 + 写 `volumes.is_online` + 调 `bulk_set_availability`**，绝不触发扫描（扫描由用户/编排显式发起）。

**3.1.3 启动期与扫描前对账**：

- App 启动：`enumerate` 一次 → 全量刷新 `volumes.is_online` + `media_items.availability`（在线卷下的项=`online`，离线卷下=`offline`；枚举值对齐 Part1 §3.2 `'online'|'offline'|'missing'`）。
- 每次 fast_scan 入口：先 `resolve_path(root)` → 查该卷 `is_online`，**离线即 `return Ok(Skipped)`**（不报错、不动 DB），这是 §3.2 离线守门的第一道闸。

### 3.2 离线守门 + 删除检测（P2 + P12，数据安全红线）

> **核心铁律（Part0 §6）：离线 ≠ 删除。** 卷拔出绝不能触发差集删除。删除检测必须三重守门。

**3.2.1 删除检测落点**：fast_scan 一个 scan_root 全部 upsert 完成后、`finish_scan_root` 前，执行差集标记。

**3.2.2 三重守门差集算法**（新增 `mark_missing`，queries.rs；🔴 第 8 轮 P0-1：写 `availability='missing'` 非 is_deleted，详见 §3.2.4）：

```sql
-- 守门 1：仅在线卷参与（WHERE volume_id IN 在线集）
-- 守门 2：仅本次扫描根子树（directory 属于本 scan_root）
-- 守门 3：本次未出现 = id NOT IN (本次 upsert 命中的 id 集)
UPDATE media_items
   SET availability = 'missing'   -- 🔴 P0-1(第8轮核验)：扫描缺失改写 availability='missing'，不写 is_deleted（与用户回收站分离、支持重现自动恢复，详见 3.2.4）
 -- ⚠️ 前置：volume_id 列须 Part1 SCHEMA_V10 迁移先就绪（schema.rs 当前无此列，未迁移即报 no such column）
 -- 守门 2：经 directories.root_id 关联本 scan_root 子树
 -- 🔴 media_items 无 scan_root_id 列（schema.rs:76 仅 directory_id）→ 必须经 directories 子查询
 WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?root)
   AND volume_id IN (/* 在线卷 id 列表，运行时再查一次确认在线 */)
   AND is_deleted = 0
   AND id NOT IN (/* 本次扫描 seen_ids */);
```

实现要点：

- 🔑🔴 **完整扫描门闩（数据安全红线，terminal review 补）**：差集删除依赖 `seen_ids` **完整**。但现 [walker.rs:71](../../src-tauri/src/scanner/walker.rs#L71) `.filter_map(|e| e.ok())` 静默丢弃 WalkDir 遍历错误、[walker.rs:93-95](../../src-tauri/src/scanner/walker.rs#L93) `metadata()` 失败 `continue`——权限/IO/网络盘半断的**真实文件不进 seen_ids**，差集会**误删**它们（🔴 **在线卷亦然，非仅离线**）。修复：`walk_media_files` 返回 `WalkReport{ files, errors, complete }`；**仅 `complete==true`（零遍历/metadata 错误）才允许差集删除**；有错误则只上报/灰显、**绝不写 is_deleted**。不变量升级：「**不完整扫描 ≠ 删除**」（比「离线≠删除」更广）。
- 🔑 **TOCTOU 防护**：`seen_ids` 在扫描中用 `HashSet<i64>` 累积（upsert 三分支的 `id` 全收，含 Unchanged）。写删前**再查一次** `set_volume_online` 状态，确认目标卷仍在线才执行 UPDATE（防扫描中途拔盘 → 误删）。
- 🔑 **入口守门**：§3.1.3 已保证离线卷根本不进 fast_scan，故差集天然不会跑在离线卷上；此处 `volume_id IN 在线集` 是第二层冗余防护。
- **标记缺失（非删除）**：标 `availability='missing'`（🔴 P0-1：**不写 is_deleted**），**不 DELETE 行**（保 thumb/AI/face 派生；重现自动恢复见 3.2.4；用户回收站是独立的 is_deleted 态；真正物理清理走独立 GC 命令，本 Part 不含）。
- **大集合优化**：`seen_ids` 百万级时 `NOT IN` 慢 → 改为临时表 `temp.seen_ids(id INTEGER PRIMARY KEY)` 批插后 `LEFT JOIN … WHERE seen.id IS NULL`，或按 directory 分批差集（每目录 seen 集小）。

**3.2.3 离线态联动**（不删、只灰）：卷离线时 §3.1.2 监听线程把该卷下项 `availability='offline'`；前端（Part5）灰显+角标；面向用户查询默认仍可见离线项（只读元数据/缩略图缓存），点开原图时提示「卷未连接」。**离线 ≠ is_deleted**，两字段正交。

**3.2.4 🔴 删除来源区分 + missing 自动恢复（P0-1，第 8 轮核验）**：

原 §3.2.2 差集把「在线卷上本次未出现」标 `is_deleted=1`，与用户主动删除（`soft_delete_items` 同写 is_deleted）**共用同列、无判别字段** → 二者混入同一回收站（`get_trash WHERE is_deleted=1`），且缺失项重现无法安全自动恢复（怕误恢复用户删除项）。分离两条**正交**状态机：

- **系统态 `availability`（扫描/卷驱动，全自动）**：`'online' | 'offline' | 'missing'`
  - `online→offline`：卷离线（§3.1.2 监听，非删除）。
  - `online→missing`：**卷在线但文件本次未出现**（§3.2.2 差集写 `availability='missing'`，**不写 is_deleted**）——文件真没了但非用户操作。
  - `missing→online`：**自动恢复**——重现时 upsert UPDATE 分支检测 `availability='missing'` → 复位 `'online'`（落点：queries.rs upsert UPDATE 加 `availability = CASE WHEN availability='missing' THEN 'online' ELSE availability END`；**不碰 is_deleted/offline**）。
- **用户态 `is_deleted`（用户驱动，仅显式）**：
  - `active→trashed`：`soft_delete_items` 写 `is_deleted=1 + deleted_at`（进回收站；可选加审计列 `deletion_reason='user_trashed'`）。
  - `trashed→restored`：`restore_items` 写 `is_deleted=0`（仅用户显式；**扫描路径永不触碰 is_deleted**）。
- **两态正交**：一个 missing 项也可同时被用户 trashed，查询/UI 各自处理（与 §3.2.3 offline 正交同理）。
- **UI（Part5）**：missing 项单独「可能已移动/删除」分组（**非回收站**），提示「文件在卷上消失，重新扫描/重连后自动恢复」；is_deleted 项进回收站。
- **Part1 依赖**：`availability` 列已在 SCHEMA_V10（Part1 §3.2 取值加 `'missing'`）；`deletion_reason` 审计列可选（Part1 V10 补）。
- **不变量**：「扫描缺失 ≠ 用户删除」——前者改 `availability`、可自动复原；后者改 `is_deleted`、仅用户可逆。

### 3.3 SourceChanged 重 enrich（P1，元数据停滞 bug）

> 文件 mtime 变 → 当前只重置缩略图，旧 EXIF/时长/编码永久停留。修复=变更时**失效全部派生 + meta**，使 enricher 重新选中。

**3.3.1 upsert SourceChanged 分支补全**（queries.rs:553-573 扩写）：

```rust
// SourceChanged：文件已被替换/编辑，旧派生与元数据全部失效
// 1) 缩略图（原有）：thumb_status=0/thumb_path=NULL/thumbhash=NULL
// 2) 新增：删除三类 meta 旧行，使 enricher 的 `IS NULL` 过滤重新命中
conn.execute("DELETE FROM image_meta WHERE item_id=?1", [id])?;
conn.execute("DELETE FROM video_meta WHERE item_id=?1", [id])?;
conn.execute("DELETE FROM audio_meta WHERE item_id=?1", [id])?;
// 3) 失效 AI/face/exotic 派生（与 fast_scan.rs:402 invalidate_exotic_tasks_for_item 对齐）
//    复位 sort_datetime 由 enricher 重算；live_photo 配对随图片段重跑
```

- 为何删而非 UPDATE：`IS NULL` 过滤是 enricher 的唯一驱动；DELETE 最小改动即可让现有循环自然重 enrich，无需改 enricher 选择逻辑。
- 🔑 **失效要全**：除三类 meta，还需失效 AI 向量（Part4 `ai_status`）、face（Part4 `face_status`）、exotic 任务（已有 `invalidate_exotic_tasks_for_item`）——否则文件换了内容、AI/人脸结果仍是旧图的。本 Part 落 meta + 调用既有 exotic 失效；AI/face 失效钩子在 Part4 对接（此处留 `invalidate_derived_for_item(id)` 统一入口）。
- **事务内**：DELETE + UPDATE 必须与 upsert 同一 `unchecked_transaction`，避免半失效。

**3.3.2 mtime 误判加固**〔✅ 已实施(2026-07-02,同日晚于左侧审查回写):upsert 判据 (mtime,size),「mtime 变 size 同」→ `SuspectChanged` 零写返回,fast_scan 批事务外算指纹后经 `resolve_suspect_change` 定案,见 §3.4 横幅〕：纯 mtime 在某些同步盘（OneDrive/坚果云）会因「占位→落地」误报变更 → 叠加 `file_size` 比较。但 🔴 **mtime 变 size 同 ≠ 内容未变**（EXIF/GPS/方向/评分写回等编辑常**不改文件大小**）→ 不能简单 touch 跳过、否则元数据变更漏检（terminal review）。**正解：mtime 变 size 同 → 「可疑变更」路径**——小文件（<阈值）全 `content_hash` 比对、大文件抽样 hash / 快速 metadata fingerprint；hash 变才判 SourceChanged，hash 同才仅 touch。纯占位误报（hash 同）被滤掉，真实同大小编辑（hash 变）不漏。见 §3.4。

### 3.4 增量扫描（P 增量加固）

> ✅ **本节「(mtime,size) 双字段 + content_hash 三环」已实施(2026-07-02 当日回写后落地)**:upsert 判据升级 (mtime,size);「mtime 变 size 同」→ `SuspectChanged` **零写**返回,fast_scan 在批事务提交后(写锁外)算内容指纹——`utils::hash::content_fingerprint`,≤64MB 全文 / 大于 64MB 头中尾 3×256KB+长度抽样,带 `sha256:`/`sha256s:` 算法前缀(换算法时旧基线识别为异类 → 走 NULL 兜底,不静默错比)——再经 `resolve_suspect_change` 短写定案:基线同 → touch(只更 mtime,meta/派生/cache_key 全保留,旧缩略图继续命中);基线 NULL → 保守 SourceChanged + 建基线;基线异 → SourceChanged + 更新基线;指纹失败 → 保守失效 + 基线置 NULL;size 变路径直接失效并清陈旧基线。
> **裁决注(无人值守分叉,2026-07-02)**:哈希以在树 sha2(SHA-256)替代设计定的 BLAKE3——离线 cargo 缓存缺该 crate(G3 联网预取项);语义等价、性能取舍,联网后可换,前缀机制保证换算法安全。`content_hash` 列自此激活;`idx_media_hash` 索引仍无查询消费(比对是逐行的),留作未来 by-hash 查找,不再算死索引口径。测试:`content_hash_three_rings` / `suspect_change_returns_without_writes` / `content_fingerprint_full_vs_sampled`(仅本地验证)。本节末 seen_ids 收集与目录级剪枝(T17a/b)先前已实施。

- **现状**：仅 mtime。**改进**：`(mtime, size)` 双字段判据——
  - 都相同 → `Unchanged`（跳过）；
  - `size` 不同 → `SourceChanged`（走 §3.3 全失效）；
  - 🔴 **`mtime` 变但 `size` 同 → 「可疑变更」走 §3.3.2 `content_hash` 二次确认**（P1-4 对齐，第 6 轮独立核验）：hash **变** → `SourceChanged` 失效；hash **同** → 仅 `touch`（滤同步盘占位误报）。**不可无条件判 SourceChanged、亦不可无条件仅 touch**（后者漏同大小元数据编辑）。
  - 不存在 → `Inserted`。
- `content_hash`：schema 已有列,✅ 2026-07-02 起由「可疑变更」路径读写。🔴 **本 Part 启用「可疑变更」按需 hash（非全量，P1-4 对齐 §3.3.2）**——仅 `mtime 变 size 同` 子集触发（占全量极小比例）：小文件全 `content_hash`、大文件抽样 hash / 快速 metadata fingerprint；hash 变才判 SourceChanged。**全量 hash 仍不启用**（百万文件读全文不可接受）；常规 `Unchanged`/`size 变` 路径不算 hash。
- 🔴 **content_hash 基线 + 首次兜底 + 抽样算法（P1-2，第 8 轮核验）**〔✅ 三环已实施 2026-07-02;INSERT 仍不写 hash 系设计如此——基线由首次可疑变更建立〕：新行/历史行 `content_hash` 初始 NULL，首次「可疑变更」无基线可比。补三环：
  - **建立基线**：「可疑变更」算出 hash 后**写回该行 `content_hash`**（`UPDATE … SET content_hash=?`），下次可疑变更即有基线。
  - **NULL 兜底（首次必保守）**：可疑变更时若 `content_hash IS NULL`（历史/新插入行无基线）→ 算当前 hash 写回 + **本次保守判 `SourceChanged`**（无旧值无法证明内容未变，宁可重 enrich；下次有基线再精确比对）。
  - **大文件抽样 + 漏检边界**：文件 > 阈值（64MB）不全文 hash → `fingerprint = 头 256KB ‖ 中 256KB ‖ 尾 256KB ‖ file_size` 的摘要〔实现:SHA-256 替代 BLAKE3(离线缺 crate,裁决见节首横幅),前缀 `sha256s:`〕。**已知漏检边界**：采样区间外字节变化且总大小不变（如长视频中段替换单帧）会漏判 → 记为可接受边界（元数据编辑通常改头部 EXIF、落采样区，覆盖主用例）；小文件（≤阈值）走全文 hash 无漏检。
- **未变更跳过**：`Unchanged` 分支继续 `continue`（零 DB 写、零 enrich），但其 `id` **仍须收进 `seen_ids`**（§3.2 差集守门依赖，否则未变更文件会被误标删除——当前 bug 的潜在陷阱）。
- **目录级剪枝**（百万级提速，可选）✅ **已实施(2026-06-29，T17a+T17b)**：
  - **T17a 基线**：扫描期 `ensure_dir_chain` 写每目录 FS mtime → `directories.mtime`（此前恒 None），流式循环累积每目录「直接媒体计数」收尾写回 `directories.media_count`（该列此前是死列，UI 角标走递归子查询、不读它）。安全、默认扫描行为不变。
  - **T17b opt-in 快速扫描**（`run_fast_scan(quick=true)`，IPC `start_scan{quick}`）：mtime 未变的目录跳过其直接文件 per-file 工作（stat/cache_key/upsert/exotic），并把该目录全部未删媒体 id **回填 seen 防误删**。
  - 🔴 **关键纠偏（实现期发现）**：原计划「**整目录/整子树**跳过」**不安全**——目录 mtime 不向上冒泡，子目录内容变不改父目录 mtime，`skip_current_dir` 整子树跳会漏掉**嵌套目录的新增/删除**（结构性漏扫）。故安全版改为**逐目录文件级跳过、仍遍历整棵树**（每个子目录各自比对 mtime）。runtime 判据**仅用 mtime**（count 作为 belt-and-suspenders 需现算 read_dir，KISS 暂不入 runtime gate；count 基线仍写、留待未来 USN journal/Merkle）。
  - **唯一漏检边界**：文件**就地编辑**（内容变、父目录 mtime 不变）——快速扫描设计取舍，全量扫描兜底；增量首次（无基线）等价全量。
  - **真正治本**仍需 USN journal（NTFS 变更日志）/ FSEvents / Merkle 式递归哈希（属 C5 平台层，已 defer）。

### 3.5 live / motion photo 配对强化（P5 + P6 + P7）

> 现状纯文件名茎匹配、不支持 HEIC、不读 ContentIdentifier。分三档加固，**HEIC 优先**（卖点级，现代 iPhone 默认）。

**3.5.1 HEIC 配对（P5，必做）**：`pair_live_photos` 查询的 `file_format IN` 列表加 `heic`/`heif`：

```sql
-- 原: WHERE file_format IN ('jpg','jpeg','mov')
-- 改: WHERE file_format IN ('jpg','jpeg','heic','heif','mov')
```

分组后「图片侧」候选 = jpg/jpeg/heic/heif，「视频侧」= mov；同 stem 配对逻辑不变。**最小改动即恢复现代 iPhone Live Photo**。

**3.5.2 ContentIdentifier 双重验证（P6，应做）**：纯 stem 匹配在「同名不同拍」（IMG_0001.HEIC vs 另一次拍的 IMG_0001.MOV）会误配。Apple 官方依据是 EXIF/QuickTime `ContentIdentifier`（同次拍摄 HEIC 与 MOV 共享一个 UUID）。

- enricher 图片段已读 EXIF → 顺带提 `ContentIdentifier`（HEIC 在 `MakerApple` tag 0x11；MOV 在 `keys`/`mdta com.apple.quicktime.content.identifier`）写入新列 `media_items.content_identifier`（Part1 补列，或借 image_meta/video_meta）。
- 配对优先级：**先按 `content_identifier` 精确配**（最可靠）→ 该列为空时**回退 stem 匹配**（兼容无 UUID 的旧素材）。两级策略既准又不漏。

**3.5.3 Motion Photo companion 配对（P7，可选）** ✅ **已实施(2026-06-29，T15)**：当前 `detect_motion_photo_xmp` 只置 flag。补：

- **嵌入式**（Google/Samsung MP 把 MP4 嵌进 JPEG 尾部）：已由 `get_companion_video_url` 解析 offset 提取，本 Part 不改（属 Part3 派生/播放）。**T15 未触碰**。
- **分体式**（部分机型 JPG+MP4 分文件）：✅ 纳入 `pair_live_photos` 同套 stem 配对，视频侧候选加 `mp4`。判定 motion vs 普通视频：**仅当图片侧 `has_embedded_video=1`**（enrichment 对 XMP `GCamera:MotionPhoto=1`/Samsung 标记置位的持久化形态）时才配，避免把普通同名 mp4 误吞。MOV 配对不受守门影响（沿用 T7 纯 stem，mov 近乎 iPhone 专属容器，误配风险低）。
  - 落地：`Group{image_id, image_is_motion, mov_id, mp4_id}` 按 (dir, stem) 聚合；`[mov_id, mp4_companion].into_iter().flatten()` 统一标 companion，至少一个伴随视频才标静图 `is_live_photo=1`。
  - ⚠️ **ContentIdentifier 双重验证（§3.5.2）仍未做**：纯 stem 在「同名不同拍」仍可能误配（mov 侧尤甚），属 T15 后续；需 Part1 补 `content_identifier` 列。

### 3.6 坐标平移修复（P10，卖点核心）

> 🔴 **本节「验证→加固→中期演进」框架已被实测推翻（2026-07-02 回写）**：§3.6.1 真机验证已于 2026-06-29 执行——方案A 线性平移的不顺滑为**结构性**、框架内治不好，三轮优化补丁全部 revert（`ba084b5`，共四轮真机测试失败）；`overflow:hidden` 正确性兜底（`scrollHeight==spacerHeight`）保留在代码。现行方案=**方案B bucket 分段，已由用户 2026-07-02 拍板正式排期**（R1-6，话术不降级；范围与工作量 ~2 周见 [T16 评估文档](T16_方案B_bucket分段_可行性与改造范围评估.md)）；todo.md F 节为滚动现状源。下方原文留作历史参照。

> **关键认知（§2.7 已纠正）**：核心修复 `overflow:hidden`（MediaGrid.vue:1310）**已提交**，维护 `scrollHeight==spacerHeight` 不变量。本节非「修 bug」，而是 **M2：运行时验证现有修复 → 防御加固 → 中期 bucket 演进（方案 B）**。详细方案对比见 §3.6.4。

**3.6.1 运行时验证（M2 验收前置，必做）** 🔵 **真机项（按 DoD「未自动覆盖」）**（依 `perf_hardening_plan_v2.md` §B1）〔🔴 已执行（2026-06-29）并致方案A 被推翻，见节首横幅；四步清单留作历史，`debug.safeMax` 便利化仍在代码可复用〕：

> **已落地便利化（T6）**：不再需要改 `SAFE_MAX` 重编译——dev 期在浏览器/WebView 控制台执行
> `localStorage.setItem('picasa.debug.safeMax','9000000')` 后刷新即强制进入平移模式；验证完
> `localStorage.removeItem('picasa.debug.safeMax')` 刷新即复原（`useVirtualScroll.resolveSafeMax`，仅 `import.meta.env.DEV`）。

1. 设 `localStorage['picasa.debug.safeMax']=9_000_000`（略小于测试库总高度，迫使 `ratio ≈ 2–4×` 进平移模式）→ 刷新。
2. 逐项确认：滚动条尺寸稳定 / 能到真正底部（末批照片可见）/ 无跳底 / 行不重叠不错位 / 顶底映射正确 / **wheel 与触摸板惯性手感正常**（补偿后逻辑速度应与普通模式一致）。
3. 验证 `=10_000`（极端）行为正确（仅粒度极粗，属预期）。
4. 清除 override → 刷新复原。

**3.6.2 防御性加固** 🟡 **部分实施(2026-06-29，T6)，部分经核验从简**：

- ✅ **wheel 惯性补偿**（已实施）：拦 `wheel`，物理步进缩为 `deltaY / ratio`（`ratio=logMax/physMax`），消除平移模式 `ratio` 倍漂移（触摸板惯性尤甚）。**仅平移模式 imperative 挂载**（随 `isTranslated` `add/removeEventListener({passive:false})`）→ 普通模式无任何非被动 wheel 监听、原生滚动零回归。`deltaMode` 归一像素（行×16/页×viewH）。
- ⚪ **底部缓冲行守卫**（核验后**不做**，非偷工）：原意「省无效绘制 + 双保险」。但核心修复 `overflow:hidden`（`.media-grid__content`）已确保 `scrollHeight==spacerHeight` 正确性；而把「跳过越界行」做成依赖 `logicalScrollTop`（每帧变）的 Vue 过滤会**每帧重渲染行列表**，正好摧毁本模块刻意的「命令式 transform、不每帧 re-render」设计（§ 文件头）。收益（几行缓冲的绘制）远小于代价 → 维持现状。
- ⚪ **renderAnchor 重对齐**（核验后**不做**）：原意抵消大比例精度损失。但 `logicalToPhysical/physicalToLogical` 全 f64，40M/10M 尺度尾数富余（~25 bit ≪ 52 bit），无实际精度损失；且 `scrollToY` 的 smooth scroll 全程触发 `@scroll → updateVisible` 自动重锚（`renderAnchor=floor(requestTop)`）→ 无需显式重设。

**3.6.3 已知遗留（平移模式，非本次回归，记录）**：folder 分组 sticky 头在平移模式近乎失效（坐标映射使 sticky 参照系失真）→ 由 §3.6.4 方案 B 根治。

**3.6.4 方案 B 分段虚拟列表**〔🔴 定性更新(2026-07-02):原「中期演进/>50 万项稳定后」已被推翻——R1-6 拍板正式排期,实施范围以 [T16 评估文档](T16_方案B_bucket分段_可行性与改造范围评估.md) 的 B0-B3 为准〕〔🔶 落地进展(2026-07-04):**B0+B1.5 已交付**(`73cbab6`+`2e36b76`)——get_bucket_rows 半开区间取段 + useBucketVirtualScroll **等高算术分段**(B1 语义边界+IO 方案当日被真机四根因推翻重写:愿望清单单飞取数/视口优先/离窗丢弃,三分组含 none 统一覆盖)+ 一键开关双引擎(方案 A enabled 门控保留、即切即回退);B1.5/B2 真机验收 ✅ 通过 → B3 `7f8092b` + B3.1 `cce30c1`(边缘重修:输入源分类,初版钉边立即偿债被真机推翻)已交付(段级坐标映射:1:1 印记 + 拖动逐事件比例重锚 + 停稳原子偿债,解除总高上限)→ B3.2 `4ec898b` 自研逻辑滚动条(隐原生条,拇指=纯逻辑百分比,偿债拇指回跳根治)真机验收 ✅ 通过 → **收尾:bucket 已转默认引擎 `9c8a61c`(方案 A 降为回退开关)+ 行模板抽公共组件 MediaGridRow `19c8845` + B3.2.1 拇指闪烁滞回修复**——细节见评估文档 §2 横幅与 §5.2〕:

五方案对比（取证 workflow 回传，详见 §2.7 浏览器上限）：

| 方案 | 思路 | 取舍 |
|------|------|------|
| **A 修正坐标平移**（当前已落） | overflow:hidden + 守卫 | ✅ 改动最小、普通模式零回归、帧率最高 ❌ 滚动条粒度粗、sticky 头失效、惯性漂移 |
| **B 分段虚拟列表**（Immich，**推荐中期**） | 每 bucket（月/年/文件夹）独立 DOM 段、IntersectionObserver 驱动 | ✅ 彻底规避高度上限、sticky 天然可用、scrollTo 精确 ❌ 前后端重构大、跨 bucket 边界复杂、超大单月仍触顶(#28861) |
| C CSS scaleY 压缩 | transform 等比压真实高度 | ❌ 与 fixed/tooltip 层叠冲突、WebView2 行为差异、字体亚像素糊 |
| D 无 spacer 双锚点 | globalAnchor+localOffset，抛弃原生 scrollbar | ❌ 须自绘 scrollbar、全量接管 wheel/touch/kbd、a11y 差 |
| E relative 行堆叠 | 只渲可见行无 spacer | ❌ 无物理坐标、scrollToDir/timeline 全废 |

**决策（2026-07-02 更新）：原「A 当前落地、B 中期演进」已被真机结果推翻——A 仅保留 `overflow:hidden` 正确性兜底（顺滑补丁已 revert `ba084b5`）；B 经 R1-6 正式排期，详见 T16 评估文档。**

方案 B 落地要点（早期草案；现行细节以 T16 评估文档 B0-B3 为准）：
- 后端 `get_rows_by_y` 增 `bucket_id` 维度或按分组分页；`cache.rs` 按 bucket 分段存储或 `bucket_id` 查询。
- 前端每分组（年/月/文件夹）渲染为独立虚拟列表段，段高 ≤ SAFE_MAX → 彻底消除映射精度 + sticky 失效。
- 参考 Immich v1.142 Timeline：bucket 级懒加载 + IntersectionObserver 进出视口加载/卸载。
- ⚠️ **不是银弹**：单 bucket（如单月 50k+ 资产）内部仍受 16.7M px 约束（Immich #28861），需对超大 bucket 二次分页。

### 3.7 百万级布局加固 + 内存（P3 + P8）

**3.7.1 fast_scan 流式 walk（P3，内存峰值）** ✅ **已实施(2026-06-29，T12)**：现 `walker` 全量 `Vec<WalkedFile>` + `order_for_view` clone → ~600MB@100 万。改进：

- **分块入库**：walker 改 streaming（边走边按 500 批 yield），不全量持有；排序需求与流式冲突 →
- **排序权衡**：`order_for_view` 当前为「入库即视图序」。百万级改为**入库不强序**（按目录遍历序入），视图序完全交给布局层 `compute_layout`（已按 `sort_datetime DESC,id DESC` 排，Part1 §3.5 复合索引）→ 去掉 fast_scan 的全量排序 + clone，内存峰值降一半。
- **代价**：fast_scan 期间画廊瞬时非时间序（导入中）；enrichment 补完 sort_datetime 后首次 relayout 即正确。可接受（导入态本就在变动）。

**落地实测（实现细节 + 设计取舍补全）**：
- **实现**：`MediaWalker`（`impl Iterator<Item=WalkedFile>`，`Box<dyn Iterator>` 抹掉 `filter_entry` 隐藏目录剪枝闭包的匿名类型）逐项产出；遍历/metadata 错误累积进内部 `errors`，遍历结束 `finish()→WalkOutcome{errors, complete, cancelled}`——`complete = errors.is_empty() && !cancelled`，即「不完整不删」门闩的承载体（替原 `WalkReport`）。`run_fast_scan` 外层 `loop`：`walker.by_ref()` 拉至多 `BATCH_SIZE` → 取消即 `Err(Cancelled)`（丢当前批、不进 finalize）→ 本批前 `eager` 项 `par_iter` 提真实尺寸 → `drain(..)` 移动消费成 `FileInfo`（**不 clone**）→ 一批一事务入库。
- **取舍② eager-dim 对齐降级（原设计未显式记，补）**：旧实现靠 `order_for_view` 保证「最先入库（且 eager 提尺寸）的恰是最先展示的项」→ 首屏 reflow-free。流式无全局排序，eager 预算（前 `EAGER_DIM_COUNT`，跨批累计）改按**遍历序**取首批。folder 分组下遍历序≈视图序仍大体对齐；date 分组下不一致 → 首屏部分项以占位入库，由 enrichment 秒级回填（仅导入态可见、非数据问题）。
- **取舍③ 进度 indeterminate（原设计未显式记，补）**：流式 walk+insert 合一、不预扫总数，故扫描期进度 `total=0`（旧实现的「discovering」阶段本就 `total=0`，仅「scanning」阶段是 determinate——退化的只是这段较快的尾部）。`ScanCompletedPayload` 仍携带准确计数。
- **正确性保障**：`seen` 收集（含 Unchanged，守门3）、exotic 播种/失效、TOCTOU 复查、四道闸 finalize **逻辑全未变**——仅入库循环的驱动从「全量 Vec chunks」换成「流式批」。227 单测全绿、clippy 净。

**3.7.2 图片 enrichment 段 reserved_core_pool（P8）** ✅ **已实施(2026-06-29，T13)**：图片段（enricher.rs:175-215）当前用全局 rayon 池，与派生/AI/exotic 抢全部核。改：图片段也走 `reserved_core_pool`（保留 1 核给前台 `compute_layout`），与视频/音频段一致。

**落地**：在图片批循环外建池一次 `let img_pool = reserved_core_pool();`，把批内 `path_infos.par_iter().map(...).collect()` 包成闭包 `parse_batch`，按既有视频段同款模式 `match &img_pool { Some(p) => p.install(parse_batch), None => parse_batch() }` 执行——`install` 内 `par_iter` 自动用保留核池；建池失败回退全局池。**逻辑零改动**（解析/回填/写库不变），仅换执行池。

- ⚠️ **注意非对称**：fast_scan 前 500 项的维度提取仍可用全局池（一次性、量小、用户强等待）；持续性的 enricher 图片段才需 reserved 保护。区分「一次性突发」与「持续后台」。

**3.7.3 布局缓存内存**：`LayoutRowItem` 17 字段最小投影已是良好实践（重型字段按需 `get_meta_for_viewport`）。百万项 `flat_ids`(8B)+`flat_rowcol`(8B)+`id_to_flat`(HashMap ~48B/项) ≈ 64MB@100 万，可接受；若 >500 万考虑 `id_to_flat` 换 `Vec` 排序二分（省 HashMap 开销，属远期）。

### 3.8 时间轴直方图（后端支持，P11）

> 供 Part5 右侧 time scrubber。现 mini-timeline 按逻辑高度均布圆点、无密度、无年月聚合、date 模式无 group_id。

**3.8.1 独立 IPC `get_date_histogram`** ⚪ **不建（采纳 §3.8.3 内联，2026-06-29，T14）**：原设计的独立命令——

```rust
// （未实现的备选方案，保留作记录）返回年月密度 + 对应逻辑 y
pub struct DateBucket { pub year: i32, pub month: u32, pub count: u32, pub y: Option<f64> }
fn get_date_histogram(granularity: Granularity /*Year|Month*/) -> Result<Vec<DateBucket>, AppError>;
```

§3.8.3 已明示「3.8.1/3.8.3 **二选一**、推荐 Summary 内联」。内联避免二次 IPC 往返、且与 `layout_version` 原子一致；独立 IPC 是被否方案，**不实现**（年/月聚合由前端按 `month_buckets` rollup，无需后端 granularity 参数）。

**3.8.2 date 模式补 group_id** ✅ **已实施(2026-06-29，T14)**：date 分组日分隔符原 `group_id=None`（justified.rs:288），改附 `group_id = timestamp_to_year_month(sort_datetime)`（`"YYYY-MM"`，月零填充）→ 前端可按月→y 定向滚动（复用 `get_separator_y_by_group_id`，DESC 序下命中该月最新一天=月段顶部）。**与 `timestamp_to_date_label` 同 UTC 基准**，确保月桶边界严丝合日分隔符（且契合 T9：EXIF 墙钟当 UTC 存）。folder 分组 group_id 仍为 dir_id（纯数字），二者格式天然不混。

**3.8.3 LayoutSummary 增 `month_buckets`** ✅ **已实施(2026-06-29，T14)**：`get_summary` 在**与 separators 同一次行遍历**中合并月桶——遇 `group_id` 可解析为 `"YYYY-MM"` 的分隔符即按月开桶（同月沿用），Normal 行 `items.len()` 累加进当前桶；`y` 取该月首个分隔符。`MonthBucket{year,month,count,y,group_id}`，folder/none 分组解析失败 → 空 vec。前端 `MonthBucket` TS 类型已补、`LayoutSummary.monthBuckets` required。

前端（Part5）据此：scrubber 按**时间均布**（非高度比例）、每月一 tick、tick 高=桶 count/总数（密度条）、年份 label 浮层。

### 3.9 零散正确性修复（P4 + P9 + fire-and-forget）

> 🔴 **本小节 naive Local 方案已被 T9 核验推翻（2026-06-29；正文回写 2026-07-02）**：`sort_datetime` 的 EXIF 分量按「墙钟当 UTC」存（metadata.rs），显示层再转 Local 会**双重偏移**把多数照片归错天——现行代码**有意保持 UTC**（justified.rs:525、536-538 注释自承并引 T9 结论）。正确修法=统一入库语义（mtime 也转「本地墙钟当 UTC」，属扫描管道改+迁移），成本高收益小→**暂缓**（§4 T9 行）。下方原文与示例代码留作历史，勿按此施工。

**3.9.1 日期标签本地时区（P9）**（justified.rs:397-405）：现 `Utc.timestamp_opt` 强制 UTC → 东 8 区 0~8 点照片归错天。改用本地时区：

```rust
// 现: Utc.timestamp_opt(ts,0) → format YYYY年MM月DD日
// 改: 用 chrono::Local 或注入用户时区偏移
use chrono::Local;
let dt = Local.timestamp_opt(ts, 0).single().unwrap_or_else(|| Local::now());
```

- ⚠️ **时区来源**：`sort_datetime` 入库时若已是 UTC unix 秒，则此处转 Local 正确；若入库时已含偏移则会双重偏移 → 需核对 enricher 写 `sort_datetime` 的时区语义（metadata.rs EXIF DateTimeOriginal 通常无时区，按本地拍摄解读）。**先核对入库语义再改显示**，避免引入新偏移 bug。

**3.9.2 TIFF 真超时（P4）✅ 已实施（commit 9938651 / Lane C·C3）**（metadata.rs）：原 `thread::scope` spawn + `join()` 仍阻塞等子线程完成、无超时 → 损坏/恶意 TIFF 挂死 enrich 工作线程。已落地：

- 抽象泛型 `run_with_timeout<T,F>(timeout, f)`：`std::thread::spawn`（detached，非 scope）+ `mpsc::recv_timeout`，超时即放弃该文件（维度回落 `(0,0)`），工作线程不被拖死。`TIFF_DIMENSION_TIMEOUT=5s` 容忍慢盘/合法大文件。`recv` 的 `Err` 同时覆盖 Timeout 与 Disconnected（子线程 panic 未发送即丢弃 tx），均归 `None`。
- 泛型核心脱离 TIFF 文件即可单测：快闭包返回值、慢闭包超时放弃（测试自身仅阻塞 ~50ms 证「不 join」生效）、panic 经 Disconnected 归 `None`，共 +5 测试。
- ⚠️ **残留（暂不阻断，可后续加固）**：被放弃的子线程仍在后台跑完才退（Rust 无法强杀线程）。当前未对落单线程设并发硬上限——常规场景 TIFF 极少真挂起，风险可接受;若未来遇「海量畸形 TIFF」攻击面，再补「header magic 预校验 + 最大尺寸限制」或「放弃线程计数上限」。

**3.9.3 fire-and-forget 异常上报 ✅ 已实施（commit 463dea8 / Lane C·C4）**：

- **核验修正（以代码实测为准）**：原始症状「UI 永久 running」**前人已修**——fast_scan 出错经 `spawn_blocking().await?` + `if let Err` 两道冒泡成命令 Err（前端 `try/catch` 清 running）；enrichment 出错/panic 经 `catch_unwind` 补发 `enrichment:completed`（前端监听清 running）。故 plan 写时的描述已过时。
- **残留真缺口（本次修）**：enrichment 失败时补发的是「长得像成功」的 `completed` 事件 → UI 虽停转圈，但用户**看不出补全其实失败了**，错误只进 `tracing` 日志、对用户不可见。
- **落地**：`EnrichmentCompletedPayload` 加 `error_code: Option<String>`（配 `::ok` / `::failed` 构造器，三处构造点统一）；携带**稳定粗粒度码**（`enrich_failed` / `enrich_panicked`）而非原始错误串，遵循 IPC 边界错误契约。**区分「取消」与「失败」**：`AppError::Cancelled` 走 `::ok`（用户主动停止属正常终态，不弹警告）；真错误才带 code。前端据 `errorCode` 弹 warning toast（延续 Lane A 缺失检测 toast 的「失败必可见」思路）。

---

### 3.10 ViewDescriptor + 百万级全选契约（P0-2，🔴 第 8 轮核验·真 P0·跨 Part5 接缝）

> 📐 **可实施设计已出**（2026-06-29，待评审）：本节的类型/编译器/解析/守门/迁移已结合真实代码现状（`MediaFilter`、`query_layout_items`、`LayoutCacheData`、`Vec<i64>` 批量命令、前端 `Set` 选区）落成完整设计文档 **[T18_ViewDescriptor_选择契约_设计.md](T18_ViewDescriptor_选择契约_设计.md)**（含分阶段 S0–S4 + 6 个决策点）。下方草图为其来源。

> Part5 §3.1.4 已冻结 Ctrl+A 全选手势 + 批量操作；其后端落点（`SelectionDescriptor` 类型 + filter→ids SQL 解析）被 Part5:173 指派给**本 Part**，但此前零定义零任务（全文 grep `SelectionDescriptor/GalleryFilter/ViewDescriptor`=零）→ 开工即缺前置，否则 Ctrl+A 仅停在 UI 展示层。本节补全，关闭契约悬空。

**3.10.1 `ViewDescriptor`（不可变视图描述符）**——确定性唯一复现「当前画廊视图全集 + 序」。现状：视图由 uiStore 三态（`activeSmartAlbum`/`activeDirectoryId`/`activeCollection`）+ 排序/布局零散决定，无单一类型可复现 → 全选无法在 SQL 层确定性解析。定义：

```rust
/// 不可变视图描述符：确定性复现一个画廊视图的全集 + 序（Serialize 供前端透传）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ViewDescriptor {
    pub scope: ViewScope,        // 视图类型 + scope（决定 FROM/JOIN）
    pub filter: GalleryFilter,   // 附加筛选（决定 WHERE）
    pub sort: SortSpec,          // 默认 (sort_datetime DESC, id DESC)，与 Part1 §3.5 复合索引一致
    pub layout_version: u64,     // 与 LayoutCache.layout_version 对齐；版本失效即拒解析（3.10.3）
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewScope {
    AllMedia,
    Directory { scan_root_id: i64, recursive: bool },   // 经 directories 子查询取 directory_id 集
    SmartAlbum { kind: SmartAlbumKind },                // favorites/recent/videos/...
    Collection { collection_id: i64 },
    Person { person_id: i64, model_name: String },      // 人脸：按 model_name 隔离（Part4 §3.5.2）
    SemanticSearch { query_embedding_id: i64, top_k: u32 }, // CLIP 语义：有序、非纯 SQL（见 3.10.2）
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GalleryFilter {
    pub media_types: Option<Vec<MediaType>>,
    pub favorite_only: bool,
    pub rating_gte: Option<u8>,
    pub date_range: Option<(i64, i64)>,   // (sort_datetime 起, 止]
    pub color_label: Option<String>,      // Part1 V10 media_items.color_label
}
```

**3.10.2 `SelectionDescriptor` + filter→ids SQL 解析**（对接 Part5:173）：

```rust
pub enum SelectionDescriptor {
    Explicit(Vec<i64>),                                          // 离散勾选
    SelectAll { view: ViewDescriptor, excluded_ids: Vec<i64> },  // 全选 − 排除集
}
```

- 新增 `resolve_selection(conn, &SelectionDescriptor) -> Result<Vec<i64>>`：
  - `Explicit(ids)` → 上限校验（3.10.4）后直接返回。
  - `SelectAll{view, excluded}` → `view_to_sql(view)` 把 ViewDescriptor **编译为单条 SQL** 取全集 id（流式游标，百万级不一次性 collect 到 Vec 即交批量命令分块消费），扣 `excluded`。
- **`view_to_sql`（单一编译器，杜绝双套视图定义）**：scope→FROM/JOIN（Directory 经 `directories` 子查询、Person 经 `faces JOIN` + `model_name` 过滤）、filter→WHERE、sort→ORDER BY。**SemanticSearch 例外**：先 `VectorStore`（Part1）取向量 top_k 有序 id，再回表过 filter，不走纯 SQL。
- 🔑 **与 `get_view_ids` 同源**：T14.5 的 `get_view_ids()` 退化为 `resolve_selection(SelectAll{current_view, []})` 的特例，二者共用 `view_to_sql` → 视图定义单一事实源，杜绝漂移。

**3.10.3 版本失效 + snapshot 一致性**（防「全选后视图已变、误操作新集合」）：
- 解析时 `view.layout_version != 当前 LayoutCache.layout_version` → 返回 `AppError::ViewStale`，前端重取视图再发批量命令。
- 进阶（P2）：服务端 snapshot token——`SelectAll` 先落一份 id 快照返回 token，批量命令引 token，避免大批量跨多命令时集合漂移。v1 先用 `layout_version` 守门即可。

**3.10.4 批量命令逐一迁 `SelectionDescriptor`**（签名迁移任务）：
- 全部批量命令（`soft_delete_items`/`restore_items`/move/copy/`set_rating`/`set_favorite`/`add_to_collection`/`set_color_label` …）签名 `Vec<i64>` → `SelectionDescriptor`，入口先 `resolve_selection` 展开。
- **预览数量**：UI 显示「将操作 N 项」→ 加 `count_selection(&SelectionDescriptor) -> Result<u64>`（SelectAll 走 `COUNT(*)` 不取全 id）。
- **上限/排除集**：`Explicit` 上限（如 100k，超限强制走 SelectAll）；`excluded_ids` 上限（如 10k，超限退化为反向 filter 并入 WHERE）。

## §4 分步任务清单（优先级 + 依赖）

> 优先级：**P0 数据安全红线 → P1 卖点/现代设备 → P2 正确性体验 → P3 演进**。依赖以 `←` 标注。每任务完成即中文 commit。

### P0 · 数据安全红线（必须先落，否则越扫越错）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T1** ✅ **已实施(2026-06-29)** | 卷在线判定 `VolumeOnlineCheck`+`PathProber`（路径可访问性，可注入 mock 测拔盘）+ 卷身份 `VolumeResolver`：`PlatformVolumeResolver`（Windows 原生卷 GUID `GetVolumeNameForVolumeMountPointW`，抗盘符重映射；失败/非 Windows 回退 `PathVolumeResolver` 路径派生）。🔵 mac DiskArbitration `DAVolumeUUID` 留后续（功能不退化，走 path 派生）。commit `530924c` | `scanner/volume_probe.rs` | Part1 `volumes` DAO | §3.1.1 + 7 单测（含同盘并卷/三平台 derive） |
| **T2** 🟡 **轮询监听已实施(2026-06-29)** | 插拔监听线程（poll）：冷启动延迟 5s 后每 15s 对账已知卷在线态，实时翻 `volumes.is_online` + `media_items.availability(online↔offline)`，emit `volumes:changed` → 画廊刷新离线徽标。**正交铁律**：只切 online↔offline，绝不碰 is_deleted / 'missing'（5 单测含此项）。**原生 GUID（抗盘符重映射）= Piece A 已实施**（commit `530924c`，见 T1）。🔵 Windows `WM_DEVICECHANGE` 即时唤醒 + mac DAVolumeUUID 仍后续（前者仅提前唤醒轮询、逻辑不变）。commit `ba4f452`。scoping：[`2026-06-29-卷监听与原生卷ID-scoping.md`](../archive/2026-06-29-卷监听与原生卷ID-scoping.md) | `scanner/volume_watch.rs`、lib.rs setup、前端 MediaGrid | ← T1 | §3.1.2-3 / 拔插 U 盘 ≤15s 反映 |
| **T3** ✅ **已实施(2026-06-29)** | 缺失标记三重守门 `mark_missing`（P0-1：写 `availability='missing'` 非 is_deleted；在线集+子树+seen 差集，dry_run 参数）+ **完整扫描门闩 `WalkReport`**（不完整不删，比离线不删更广）+ TOCTOU 写删前复查 + upsert 重现自动恢复 `missing→online`（实测纠偏：Unchanged 分支早返回零写，故 Unchanged 也做条件恢复） | `db/queries.rs`、`scanner/walker.rs`、fast_scan 收尾 | ← T1,T5 | §3.2 / 拔盘不删·缺失非删除。**5 阶段提交** de4e0d7/9299b99/58eb038/b9664b3/0c83741 |
| **T4** ✅ **已实施(2026-06-29)** | SourceChanged 全失效：新增 `invalidate_derived_for_item`（DELETE image/video/audio_meta），upsert 的 SourceChanged 分支**同一事务内**调用 → enricher 据 `IS NULL` 重选重算。🔵 exotic 失效已在 fast_scan 单独接（不重复）；AI/face（ai_status/face_status）留 Part4；content_hash 二次确认（§3.3.2）后续。**刻意只改 queries.rs 不碰 fast_scan**（与缺失检测测试解耦） | `db/queries.rs`、enricher 重选 | — | §3.3 / 替换后 EXIF 更新 + 1 单测（SourceChanged 删三 meta、Unchanged 不删） |
| **T5** ✅ **已实施(2026-06-29)** | **`Unchanged` 的 id 也收 `seen_ids`**（fast_scan 批量循环、放 exotic `continue` 之前——守门3 依赖）。~~🔵 双字段 `(mtime,size)` 增量未改~~〔✅ 2026-07-02 已升级:(mtime,size)+content_hash 三环,见 §3.4 横幅〕 | fast_scan.rs（seen 累积） | — | §3.4 / 未变更不误删 + `finalize` 单测锁 |

> 🔵 **缺失检测最小闭环收尾（2026-06-29）**：P0 数据安全红线的**核心闭环已落地**（A WalkReport 门闩 → B missing→online 自动恢复 → C PathProber 在线判定 → D mark_missing 三重守门 → E 接入 run_fast_scan，**四道闸 + 软标记兜底**）。落地方案见 [`docs/archive/2026-06-29-缺失检测落地方案.md`](../archive/2026-06-29-缺失检测落地方案.md)。**用户决策**：直接实写（非 dry-run 灰度）、T2 插拔监听 / T4 SourceChanged 留后续。`run_fast_scan` 整体端到端依赖 Tauri Channel 不可单测——四道闸判定逻辑已抽 `finalize_missing_detection` 单测，端到端按 DoD 标「未自动覆盖」。

> 🔴 **volume_id 管道缺口修复（2026-06-29，C5 Piece1/Piece2）——闭环曾对新数据休眠**：上方闭环落地后发现一处致命缺口——`volume_id` 的端到端赋值只完成了 V10 迁移那次性回填，**新数据从未补卷**：① `upsert_fast_scan_item` 新项 INSERT 不写 volume_id（恒 NULL，schema.rs:608「Part2 扫描时填」未实现）；② `add_scan_root` 新根不建卷（volume_id 恒 NULL）。于是守门1 `volume_id IN (online)` 把所有扫描后新增 media 排除 → **缺失检测对新根 / 新文件完全休眠**（首轮真机「未发现问题」实为徽标从不出现）。已修：Piece1（`4e19297`，upsert 传扫描上下文卷 + COALESCE 治愈历史 NULL）+ Piece2（`86ca662`，`VolumeResolver`/`PathVolumeResolver` 路径派生卷根 + `set_scan_root_volume`，add_scan_root 建卷绑定）。原生卷 GUID/UUID 仍随 T2 后续（换 resolver 实现即可，数据模型已按卷根并卷、前向兼容）。**首轮真机测试结论作废，C5 后需重测。**

### P1 · 卖点核心 + 现代设备

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T6** 🟡 **加固已实施（2026-06-29）；真机验证已执行——顺滑度不达标，方案A 优化 revert `ba084b5`，转 T16 方案B** | 坐标平移**运行时验证**（SAFE_MAX=9M）+ 防御加固。**已实施**：① `SAFE_MAX` dev 覆盖（`localStorage['picasa.debug.safeMax']`，免重编译强制平移模式，专为 §3.6.1 验证）；② wheel 惯性补偿（`dy/ratio` 消除压缩漂移，**仅平移模式 imperative 挂载**→普通模式零回归）。**经核验从简（非偷工）**：底部缓冲守卫（overflow:hidden 已治根因 `scrollHeight==spacerHeight`，per-frame Vue 过滤反害 imperative-transform 性能）+ renderAnchor 重对齐（40M/10M 在 f64 下精度充裕、@scroll 已驱动重锚）均**不需要**，留档说明。🔵 运行时验证（§3.6.1 四步）已于 2026-06-29 真机执行，结论=方案A 顺滑度治不好，见 T16 行。 | `useVirtualScroll.ts` | 修复已在(MediaGrid 1310 overflow:hidden) | §3.6.1-3 / vue-tsc + vite build 净 |
| **T7** ✅ **已实施(2026-06-29)** | HEIC/HEIF Live Photo 配对（候选 SQL `file_format IN` + 分组匹配均加 heic/heif，静图侧）→ 现代 iPhone 的 HEIC+MOV 成组。+4 单测（HEIC/HEIF 配对、JPEG 回归、HEIC 无 MOV 不误标）。commit `621f584` | `scanner/live_photo.rs` | — | §3.5.1 / HEIC+MOV 成组 |

### P2 · 正确性与体验

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T8** | ContentIdentifier 双重验证（UUID 精确优先、stem 回退） | enricher 图片段、`live_photo.rs`、Part1 补列 | ← T7 | §3.5.2 |
| **T9** ⚠️ **暂缓——naive 修法有害（2026-06-29 核验）** | 日期标签本地时区。**核验结论**：`sort_datetime = COALESCE(exif_datetime, file_mtime)`，而 `exif_datetime` 是 EXIF 墙钟时间**当 UTC 存**（metadata.rs）→ 对**有 EXIF 的照片（绝大多数），当前 UTC 格式化恰好正确**；naive 改 `localtime` 会**双重偏移**、把多数照片日期搞错。仅"无 EXIF 文件（mtime=真 UTC 瞬间）"跨午夜差一天（小众）。**正确修法**=统一 sort_datetime 语义（mtime 也转"本地墙钟当 UTC"），属扫描管道改 + 迁移，成本高收益小 → 暂缓。`justified.rs:525` 注释已自承用 UTC（行号 2026-07-02 校正） | `justified.rs:525`、`enricher.rs:268` | — | §3.9.1 |
| **T10** ✅ | TIFF 真超时（`recv_timeout` 替 `thread::scope.join`）—— commit 9938651 | `metadata.rs` | — | §3.9.2 |
| **T11** ✅ | fire-and-forget 异常经 Channel 上报终态（永久 running 前人已修；本次补「失败可观测」+ 取消/失败区分）—— commit 463dea8 | `scanner/enricher.rs`、`ipc/scan_commands.rs`、前端 `scanStore.ts` | — | §3.9.3 |
| **T12** ✅ **已实施(2026-06-29)** | fast_scan **流式 walk**：新增 `MediaWalker`（实现 `Iterator`，逐项产出、不再全量收 `Vec`）+ `WalkOutcome`（替 `WalkReport`/`walk_media_files`）；`run_fast_scan` 改「攒满 `BATCH_SIZE` 即提尺寸+入库」一批一事务，**一次只持有一批** → 内存峰值 O(N)→O(batch)（去掉 `order_for_view` 的全量排序 + clone）。**三处设计取舍**（见 §3.7.1 + §5）：①入库序让渡布局层（视图正确性不受影响——`query_layout_geometry` 独立 ORDER BY 重排）；②eager-dim 改按遍历序取首 `EAGER_DIM_COUNT`（date 分组下与「最先展示」不再严格对齐，残留首屏占位 enrichment 秒级回填）；③进度转 indeterminate（不预扫总数）。`run_fast_scan` 签名去 `group_by/sort_*` 三参（仍单独驱动 enrichment 补全序）。+1 单测（`walk_cancelled_is_incomplete`），walker 两测改驱 `MediaWalker`。 | `scanner/walker.rs`、`fast_scan.rs`、`ipc/scan_commands.rs` | — | §3.7.1 / 100 万内存峰值降半（实测：227 单测全绿、clippy 净） |
| **T13** ✅ **已实施(2026-06-29)** | 图片 enrichment 段并行解析（EXIF + 0×0 占位真实尺寸提取）改跑**保留核池** `reserved_core_pool`（与视频/音频段一致：建池一次、整段复用 `install`/回退全局池），为前台 `compute_layout` 留一个核 → 海量图片导入不再占满全局 rayon 池、与「2s 自动重排」反馈循环互相饿死。fast_scan 首屏 eager-dim（一次性突发）仍用全局池（§3.7.2 非对称）。227 单测全绿、enricher clippy 净。 | `enricher.rs` 图片段 | — | §3.7.2 |

### P3 · 演进 / 增强（卖点深化，可迭代）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T14** ✅ **已实施(2026-06-29)** | 时间直方图。**采纳 §3.8.3 Summary 内联**（`LayoutSummary.month_buckets`，与 separators 同次行遍历构建、与 `layout_version` 原子一致）+ **§3.8.2** date 分隔符附 `group_id="YYYY-MM"`（`timestamp_to_year_month`，与日标签同 UTC 基准 → 月桶边界严丝合日分隔符）。**§3.8.1 独立 IPC `get_date_histogram` 不建**——计划明示「3.8.1/3.8.3 二选一、推荐内联」，独立 IPC 是被否的二次往返冗余方案。+3 单测（同月合并/folder 空桶/UTC 零填充）。230 单测全绿、clippy 净、vue-tsc/vite build 净。前端 `MonthBucket` TS 类型已补。 | `layout/justified.rs`、`layout/cache.rs`、`ipc/layout_commands.rs`、`types/layout.ts` | — | §3.8 |
| **T14.5** 🔴**(实为 P0：Part5 多选前置)** | **暴露 `flat_ids` 给前端**：新增 `#[tauri::command] get_view_ids() -> Result<Vec<i64>>`，从 `LayoutCache` 读 `flat_ids` 只读切片返回（现 flat_ids 仅在内部 `LayoutCacheData`、IPC 零暴露 cache.rs:57）。返回随 `layout_version` 失效；百万 ids 整包 ~8MB JSON 可接受（前端按需缓存、不每帧拉）。🔴 **第 8 轮核验 P0-2 补**：`get_view_ids` 仅返回只读整包、**不接受 filter**——百万级全选/批量须先定**不可变 `ViewDescriptor`**（视图类型/scope/查询/排序/filter/layout 版本）+ 后端 `SelectAll{filter}` 在 SQL 层解析全集 + 推荐 snapshot token，Part5:173 把此 owner 指派给本 Part 但当前零定义（见 review_2026-06-28 §六）；否则 Ctrl+A 仅停在 UI 展示层 | `layout/cache.rs:57`、`ipc/layout_commands.rs` | — | **Part5 §3.1.1 / T4 选区脱离 DOM** |
| **T15** ✅ **已实施(2026-06-29)** | Motion Photo 分体 companion 配对。`pair_live_photos` 候选集加 `mp4`，并取静图侧 `has_embedded_video` 作 motion 守门：**仅当图片带 XMP motion 信号时才把同名 mp4 配为 companion**（普通照片 + 同名普通 mp4 不误吞）。MOV 配对沿用 T7 不守门（mov 近乎 iPhone 专属容器）。一图可 0/1/2 个伴随视频（mov+mp4 各计一对）。+5 单测（分体配对/守门拒配/MOV 不守门/双伴随/孤立 mp4），235 单测全绿、live_photo.rs clippy 净。 | `scanner/live_photo.rs` | ← T7 | §3.5.3 |
| **T16** 🅿️ 🟢 **已排期（2026-07-02，R1-6 用户拍板：话术不降级→方案B 实施；todo.md F 节为现状源）** | 坐标平移**方案 B** bucket 分段——根治平移模式滚动不顺滑（面向市场须扛 >100 万项库）。方案 A 真机 4 轮优化均失败（已 revert ba084b5），确认线性平移结构性治不好、需方案 B 正解。可行性 + 改造范围 + 分阶段（B0–B3）+ 工作量（~2 周）见 **[T16 评估文档](T16_方案B_bucket分段_可行性与改造范围评估.md)**。已于 2026-07-02 排期。 | 前后端联动重构（`useVirtualScroll.ts`/`MediaGrid.vue` + 后端 `get_bucket_rows`） | T14 month_buckets 地基已就绪 | §3.6.4 / 评估文档 |
| **T17** ✅ **已实施(2026-06-29)** | 目录级剪枝增量。**T17a**：扫描期写 `directories.mtime` + 直接 `media_count` 基线（默认扫描行为不变）。**T17b**：opt-in 快速扫描（`quick` 标志），mtime 未变目录跳过 per-file 工作 + 回填 seen 防误删。🔴 **实现期纠偏**：原「整子树跳过」不安全（漏嵌套目录新增/删除，因 mtime 不向上冒泡）→ 改安全版「逐目录文件级跳过、仍遍历整树」，唯一漏检边界＝就地编辑（全量兜底）。+6 单测（基线 mtime/count + 剪枝判定/回填/已删不复活/变更不剪/无基线不剪），241 全绿、clippy 净、前端无破。 | `scanner/fast_scan.rs`、`db/queries.rs`、`ipc/scan_commands.rs` | ← T5 | §3.4 |
| **T18** 🔴**(实为 P0：Part5 全选/批量前置)** · 📐**设计待评审** | `ViewDescriptor`/`ViewScope`/`GalleryFilter`/`SelectionDescriptor` 类型 + `view_to_sql` 单一编译器 + `resolve_selection`/`count_selection` + 批量命令逐一迁 `SelectionDescriptor` + `layout_version` 守门（`ViewStale`）。**完整设计见 [T18 设计文档](T18_ViewDescriptor_选择契约_设计.md)**（分阶段 S0–S4 + 6 决策点，待你拍板开工） | `db/queries.rs`、`ipc/*_commands.rs`、`layout/cache.rs` | ← T14.5 | §3.10 / Part5 §3.1.4 多选 |

---

> **本轮审查回写（2026-06-30，5 路 agent 取证）**：
> - 🟢 **T18 / T14.5 选择契约：D1–D6 已拍板，S0–S3 后端已交付（2026-06-30，commit 4b12286→c13d5e3）**。
>   原「Part5 唯一硬阻塞、零代码」状态已解除：`ViewDescriptor`/`SelectionDescriptor`/`view_to_sql`/
>   `resolve_selection`/`count_selection`/`get_view_ids` 均落地（30 单测），`AppError::ViewStale` 守门就位，
>   `expand_companions` 顺带修了 Live Photo 删除孤儿 bug（D5）。**剩 S4 前端对接计入 Part5 T4**；8 批量命令
>   签名迁移 `Vec<i64>`→`SelectionDescriptor` 亦并入 Part5/S4（与前端重写同批，避免破现网 app + 返工）。
>   详见 [T18 文档](T18_ViewDescriptor_选择契约_设计.md) §10 决策表 + 施工状态。
>   **plan 阅读陷阱（保留备忘）**：此二项在 §4 挂 P3 演进行但实为 P0，勿被表格优先级误导。
> - **P0–P2 主体扎实**：T1–T5（数据安全红线）、T6 加固 / T7（卖点/现代设备）、T10–T13（正确性）、
>   T14/T15/T17（演进）均落地并有单测，plan 无虚标。
> - **不阻塞 Part5 的遗留**：T8（ContentIdentifier 配对精度，需 Part1 补 `content_identifier` 列）、T9（本地时区，
>   暂缓有据）、**T16（bucket 分段滚动，backlog，方案 A overflow:hidden 已兜底，>100 万项顺滑度是结构性遗留）**〔2026-07-02 更新：已经 R1-6 拍板正式排期，不再 backlog〕〔2026-07-04:B0-B3 全部落地(B1 当日被真机推翻重写为等高分段;B1.5/B2 真机验收通过;B3 边缘规则当日亦被真机推翻、B3.1 输入源分类重修百万库基本通过 → B3.2 自研逻辑滚动条根治原生拇指回跳、验收通过;收尾:bucket 转默认引擎+行模板抽 MediaGridRow),见 T16 评估文档〕、
>   T6 运行时真机验证〔2026-07-02 更新：已于 2026-06-29 真机执行，顺滑度不达标致方案A 优化 revert，正确性兜底成立；本条「待联调」已过期〕。

## §5 风险与回滚

| 风险 | 触发 | 缓解 / 回滚 |
|------|------|------------|
| 🔴 **缺失误标**（最高危） | 扫描中途拔盘 / seen_ids 漏收 Unchanged / 卷 stable_id 识别错 | 软标记（`availability='missing'`，重现自动恢复，P0-1 §3.2.4，**不写 is_deleted**）；T3 写前 TOCTOU 复查在线；T5 强制 Unchanged 进 seen；灰度：先「dry-run 只统计将标数、不写」跑一轮人工核对再开 |
| 卷 stable_id 跨平台识别失败 | 异常文件系统 / 网络盘无 GUID | 路径兜底 `path:` 前缀退化为路径级标识；识别失败的卷**视为始终在线**（宁可不删、不可误删） |
| SourceChanged 全失效误删派生 | mtime 抖动（同步盘占位→落地） | 🔴 **P1-4 对齐 §3.3.2（非「仅 touch」）**：mtime 变 size 同 → content_hash 二次确认——hash **同**才 touch（滤占位误报）、hash **变**才失效（不漏同大小元数据编辑）；派生软失效（非物理删 thumb，可重建）〔✅ 已实施(2026-07-02):`resolve_suspect_change` 定案,hash 同 → touch 滤占位误报、派生全保留;该遗留清除〕 |
| 坐标平移验证不通过 | overflow:hidden 未根治 / WebView2 差异 | A 方案守卫兜底；验证失败则限制 SAFE_MAX 触发阈值（提高到实际不可达），暂以「分页加载更多」过渡，转 §3.6.4 方案 B〔已成事实：顺滑度验证未过，2026-07-02 已排期方案B〕 |
| 流式 walk 改排序语义 | 导入中画廊瞬时非时间序 | 仅导入态可见；enrichment 补 sort_datetime 后 relayout 即正确；可加「导入中」提示条 |
| 本地时区改动引双重偏移 | `sort_datetime` 入库已含偏移 | T9 强制「先核对入库语义」前置；改前写单测固定时间戳→预期 label〔风险已证实→T9 暂缓（2026-06-29）〕 |
| HEIC 配对漏判 | 非 iPhone HEIC（无 MOV） | 无 MOV 自然不配，仅标普通图片，无副作用 |

---

## §6 验收标准

**P0 数据安全（硬门槛，全过才可合并）**：

1. **拔盘不删**：扫描在线盘 → 中途拔移动盘 → 该盘项 `availability='offline'` 且 `is_deleted=0`（一条不被标删）；重插→恢复 `online`。
2. **缺失检测**：磁盘删 N 个文件 → 重扫 → 恰好这 N 项 `availability='missing'`（🔴 P0-1：**非 is_deleted**）、其余不变；未变更文件 0 误标；重现文件再扫 → 自动恢复 `availability='online'`（upsert UPDATE）。
3. **元数据刷新**：替换一张图（改 EXIF/GPS/拍摄日期，mtime+size 变）→ 重扫 → `image_meta` 反映新值、`sort_datetime` 重排、缩略图重生。
4. **增量跳过**：百万库二次扫描，未变更文件 0 DB 写、0 enrich，全部 id 进 seen（无误删）。

**P1 卖点**：

5. **坐标平移**：SAFE_MAX=9M 强制平移模式 → 滚动条稳定、可达真底、无跳底/错位/重叠、顶底映射正确（§3.6.1 全项）。〔口径更新：顺滑度验收随 T16 方案B；方案A 仅保正确性兜底〕
6. **HEIC Live Photo**：iPhone HEIC+MOV 同 stem → JPEG/HEIC 侧 `is_live_photo=1`、MOV `companion_of` 正确；面向用户查询 MOV 被 `companion_of IS NULL` 隐藏。

**P2/P3**：

7. **时区**：〔已随 T9 暂缓作废（2026-06-29 核验）：对有 EXIF 的多数照片现行 UTC 本就归对天，naive Local 反而归错；原验收「东 8 区 0~8 点归对天」不再适用〕。
8. **TIFF 超时**：投损坏/超大 TIFF → N 秒后放弃、记 warn、扫描继续（rayon 线程不挂死）。
9. **异常上报**：人为令一次扫描 panic → UI 收 `scan-finished{err}`、running 态清除（不永久转圈）。
10. **时间轴**：`get_date_histogram` 返回年月密度 + y；前端 scrubber 按时间均布 + 密度条 + 年份 label（Part5 联调）。

---

## §7 执行提示词（新会话直接用）

```
任务：实施 Picasa Next 重构 Part2（扫描与画廊流水线）。

【先读】
1. docs/refactor_2026/Part0_总纲与产品定稿.md §6(卷可用性)/§11(波次)/§12(约定)/§13(总提示词)
2. docs/refactor_2026/Part1_数据层.md §7(提示词) —— 本 Part 消费其 volumes/availability/backend_id DAO、复合索引、keyset
3. docs/refactor_2026/Part2_扫描与画廊流水线.md 全文（本文件，§2 现状实测为代码取证基线，§3 设计，§4 任务表）

【铁律】
- 数据安全 > 卖点 > 体验 > 性能。P0(T1-T5)先落，删除检测=最高危，软删除+TOCTOU 复查+seen_ids 收 Unchanged 三重守门，缺一不可。
- 离线 ≠ 删除：卷离线绝不触发差集；识别失败的卷视为始终在线（宁可不删）。
- enrichment 绝不对交互让步（reserved_core_pool）；rayon 内永不 .await；面向用户查询续 `is_deleted=0 AND companion_of IS NULL`。
- 新增 DAO/命令返回 AppError；中英双语注释；改后中文 commit；仅用户通知时 push；大文件小步 Edit。
- **旧 docs/记忆不可轻信**：§2 已纠正「ContentIdentifier 双重验证」「tokio 超时」等不实旧述，以代码实测为准。

【顺序】
P0: T1 volume_probe → T2 volume_watch+启动对账 → T5 双字段增量+seen 守门 → T3 删除检测三重守门(先 dry-run 核对) → T4 SourceChanged 全失效
P1: T6 坐标平移运行验证(SAFE_MAX=9M; 已执行,顺滑度不达标转 T16 方案B)+加固 → T7 HEIC 配对
P2: T8 ContentIdentifier → T9 本地时区(已核对:naive 有害,暂缓勿做) → T10 TIFF 真超时 → T11 异常上报 → T12 流式 walk → T13 图片段 reserved
P3: T14 时间直方图 ✅ → T15 Motion Photo 分体 ✅ → T16 方案B bucket(已排期 2026-07-02, 范围见 T16 评估文档) → T17 目录剪枝 ✅(T17a 基线+T17b opt-in 快速扫描)

【验收】按 §6 十条逐项过；P0 四条是合并硬门槛。
【关键认知】坐标平移核心修复(overflow:hidden, MediaGrid.vue:1310)已提交且保留; 真机验证已执行(2026-06-29)、顺滑度否定方案A(已 revert ba084b5); 现行=T16 方案B 正式排期(2026-07-02)。
```

---

> **Part 2 正文完**。下游：Part3（缩略图/派生/GPU，消费本 Part 的布局缓存 O(batch) 回填 + SourceChanged 失效钩子）、Part5（前端渲染，消费布局数据 + 时间直方图 + 离线态字段）。
> 执行前必读：Part0 §13 + Part1 §7 + 本文 §7。
