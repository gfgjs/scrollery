# Picasa Next — 百万级性能加固与架构改进计划 v2（定稿）

> 🔴 **已废弃(2026-07-10 文档治理补标)**:坐标平移滚动路径已被 T16 方案 B(bucket 分段引擎)取代(refactor_2026/T16_方案B_bucket分段_可行性与改造范围评估.md);Part0 §0.3 降历史参考。

> **目标**：保持 Tauri + Vue3 + SQLite，使应用稳定流畅浏览 10 万 ~ 100 万张媒体。
> **本版来源**：合并三方评审 —— 内部代码审查 + 外部专家 A（坐标平移/向量/分块）+ 外部专家 B（V2：viewport 异步元数据 / O(1) 索引）。本文取代 `perf_hardening_plan_v1.md`。
> **已锁定的 4 项架构决策**（用户拍板）：
> 1. **布局缓存策略 = 渲染项常驻 + O(1) 索引回写**（几何 + thumbhash + thumb_status/path + is_favorited 常驻；EXIF 等重元数据走异步 viewport 拉取）。
> 2. **AI 向量常驻精度 = f16**（百万≈1GB）。
> 3. **`panic = "abort"` → `unwind`**，配合解码 `catch_unwind` 降级。
> 4. **本轮纳入"滚动坐标平移"**（突破浏览器高度上限，真正解锁百万张单视图）。

---

## 实施进度

| 任务 | 状态 | 验证 |
|------|------|------|
| A2 O(1) 布局索引（回写 + 相邻翻页） | ✅ 已完成 | cargo check + 2 单元测试通过 |
| A1 元数据剥离 + `get_meta_for_viewport` 懒加载 | ✅ 已完成 | 后端 cargo check、前端 vue-tsc 通过 |
| A3 布局查询整体进 `spawn_blocking` | ✅ 已完成 | cargo check 通过 |
| D1 `panic=abort`→`unwind` + 解码 `catch_unwind` | ✅ 已完成 | cargo check 通过 |
| D2 退出前 `wal_checkpoint(TRUNCATE)` | ✅ 已完成 | cargo check 通过 |
| E2 合并 3×`get_summary` | ✅ 已完成（随 A3） | cargo check 通过 |
| B1 滚动坐标平移 | 🔧 已实现 + 已尝试修复溢出泄漏 bug（待运行时验证） | vue-tsc 通过 |
| C1 embedding 常驻 f16 + rayon | ✅ 已完成 | cargo check 通过 |
| D3 收藏/删除 O(1) 缓存同步 | ✅ 已完成 | cargo check + 3 单元测试通过 |
| E1 安全收敛、README/architecture_notes | ✅ 已完成（asset scope 待运行时验证） | cargo check 通过 |

> **M1（地基）+ M2（破百万）已完成（代码层）**：常驻内存大幅下降、滚动写锁 O(N×M) 与方向键 O(N) 根除、损坏文件不再中止进程、退出不再泄漏 WAL、突破浏览器 ~1677 万 px 高度上限。
>
> **B1 实现要点**：`useVirtualScroll` 内置物理↔逻辑线性映射；物理占位封顶 `SAFE_MAX=10,000,000`；行渲染在一个"渲染层"中，层 transform **命令式**（直接写 `style.transform`，不触发每帧 Vue 重渲染）把可视窗口钉到视口；逐行偏移锚定到窗口顶部以保证 4000 万 px 尺度下的精度。普通模式（≤25 万张）δ=0，行为与改造前完全一致。所有"逻辑坐标"入口（`scrollToY`/`scrollToLabel`/活动分隔符判定）均经映射转换。
>
> **B1 bug 根因 + 修复（待运行时验证）**：根因 —— 平移模式下缓冲行被 transform 定位到 `[0, spacerHeight]` 之外（如底部附近 ~11200 > 10000），而行是 `.media-grid__content`（无裁剪）的绝对定位后代，其 transform 后位置**泄漏进滚动容器的可滚动溢出区**，使 `scrollHeight` 超过 `spacerHeight` → `scrollTop` 超出映射假设的 `physMax` → 失控（滚动条忽长忽短、跳到底部、错位）。普通模式行落在 `spacerHeight` 内故不触发。**修复**：给 `.media-grid__content` 加 `overflow: hidden` 裁剪渲染层，使 `scrollHeight` 恒等于 `spacerHeight`；可视行始终在 `[0, spacerHeight]` 内不被裁剪，仅裁剪视口外缓冲行。其它固有点：平移模式滚动条粒度随比例变粗（`SAFE_MAX` 越小越粗，10M 下百万项 ~4x 比例较平滑）；文件夹分组 sticky 头本就近乎失效，无新增回归。
>
> **验证方法**：临时将 `SAFE_MAX` 设为略小于当前库总高度的值（使比例 ~2–4x，贴近百万级真实情形），`tauri dev` 后确认：滚动条尺寸稳定、能顺滑滚到真正底部（最后一批照片）、无跳底、行不重叠/不错位、顶/底位置映射正确。`SAFE_MAX=10000` 也应正确（仅因比例极大而很粗，属预期）。
>
> **C1 实现要点**：`AppState.ai_embedding_cache: RwLock<Option<EmbeddingCache>>` 常驻 f16 连续缓冲；首次搜索从读连接池一次性加载，rayon 跨全部行并行点积；写锁仅在持久化结果时短暂持有（打分阶段不再阻塞 DB 写）。嵌入向量每批写入/重置时 `invalidate_embedding_cache()`。百万项常驻约 1GB（若超预算再上 C2 ANN）。
>
> **D3 实现要点**：收藏（单个 + 批量）经 `set_favorite_in_cache` O(1) 同步常驻布局缓存的 `is_favorited`，修复"滚出再滚回收藏标记回退"。软删除/恢复改变项集（位置重排），前端 `batchDelete` 已 `compute()` 重算，无需后端原地改。写锁均在缓存同步前释放（保持 db_writer→layout_cache 锁序）。
>
> **E1 实现要点 + ⚠️待验证**：① `tauri.conf.json` 移除整盘通配 `C:/** … G:/**`，保留 `$PICTURE/$APPDATA/...` 等用户目录；② 扫描根 + 缓存目录改为**运行时**经 `app.asset_protocol_scope().allow_directory(path, true)` 授权（`lib.rs` 启动时遍历授权 + `add_scan_root` 新增时授权）；③ CSP `unsafe-eval` 经排查为 **vue-i18n 运行时消息编译所需**，保留（移除需预编译 locale）。**⚠️ 运行时验证**：`tauri dev` 后确认各扫描根（尤其非用户目录、如 `D:\Photos`）的图片能正常加载；若裂图，说明运行时授权未覆盖该路径 → `git revert` E1 的 tauri.conf.json 改动恢复整盘通配。

---

## 〇、优先级矩阵

| ID | 任务 | 严重度 | 工作量 | 依赖 |
|----|------|--------|--------|------|
| **A1** | 元数据剥离：缓存只留渲染必需项；EXIF 走异步 viewport 拉取 | 🔴 必做 | M | 地基 |
| **A2** | `LayoutState` 引入 `item_index`+`flat_ids`，O(1) 回写 & O(1) 相邻 | 🔴 必做 | M | 随 A1 |
| **A3** | 布局"查询+计算"整体进 `spawn_blocking`，不阻塞 tokio | 🟠 高 | S | — |
| **B1** | 滚动坐标平移，突破 WebView 单元素高度上限 | 🔴 1M 硬阻断 | L | A1 |
| **C1** | embedding 常驻内存(f16) + rayon 并行余弦 | 🟠 高 | M | — |
| **C2** | ANN 索引（hnsw_rs / sqlite-vec），二选一，后续里程碑 | 🟡 中 | L | C1 |
| **D1** | 移除 `panic=abort`，解码入口 `catch_unwind` 降级 | 🟠 高 | S | — |
| **D2** | 退出前 `PRAGMA wal_checkpoint(TRUNCATE)` | 🟡 中 | S | — |
| **D3** | 收藏/删除经 O(1) 索引同步缓存（或标脏） | 🟡 中 | S | A2 |
| **E1** | 收紧 assetProtocol scope + 排查 CSP `unsafe-eval` | 🟢 安全 | S | — |
| **E2** | 合并 3×`get_summary`、`IN` 参数绑定、补 README/architecture_notes | 🟢 清理 | S | — |
| ~~F1~~ | ~~删除冗余缩略图流水线~~ | ⏸ 暂缓 | — | 按反馈保留双流水线 |

> **关键路径**：A1+A2 是地基 → B1 解锁百万 → C1 解决搜索。D/E 可并行。

---

## 一、Phase A — 内存与数据通路（地基）

### A1. 元数据剥离 + 可视区异步元数据拉取

**现状**：[`query_layout_items`](../src-tauri/src/db/queries.rs) 把 28 字段（含 9 个 EXIF、`file_name`、`dir_path`、gps）全量加载进 `Vec`，原样塞进常驻 [`LayoutRowItem`](../src-tauri/src/layout/justified.rs)。百万项 250MB~1GB，`collect()` 阻塞数秒。

**目标架构（渲染项常驻 + EXIF 异步）**

1. **缩减布局查询**（[queries.rs](../src-tauri/src/db/queries.rs)）
   - `query_layout_items` 取消 `LEFT JOIN image_meta`，只查排版+卡片骨架所需：
     `id, width, height, sort_datetime, media_type, is_live_photo, duration_ms, thumb_status, thumb_path, thumbhash, is_favorited`，以及（group_by=folder 时）`dir_path/dir_id` 用于分隔符。
   - 多表 JOIN 降维 → 排版查询瞬间完成。

2. **缩减常驻结构**（[models.rs](../src-tauri/src/db/models.rs) / [justified.rs](../src-tauri/src/layout/justified.rs)）
   - `LayoutItem` / `LayoutRowItem` **移除**：`exif_make/model/lens/focal/aperture/shutter/iso`、`gps_lat/lng`、`file_name`、`dir_path`、`exif_*`。
   - **保留常驻**：`id, x, y, w, h, media_type, is_live_photo, duration_ms, thumb_status, thumb_path, thumbhash, is_favorited`。
   - 估算常驻 ~150–180 B/项 → **百万 ~150–180MB**（较现状降 ~5–7×）。
   - 微优化（可选）：`media_type` 用 `u8` enum 替代 String。

3. **新增按 ID 批量元数据查询**（[queries.rs](../src-tauri/src/db/queries.rs)）
   - `get_media_meta_batch(ids: &[i64]) -> Vec<MediaMeta>`：主键索引 `IN(...)`，返回 `file_name, dir_path, exif_*, gps`。

4. **新增 IPC**（[media_commands.rs](../src-tauri/src/ipc/media_commands.rs)）
   - `get_meta_for_viewport(ids: Vec<i64>) -> Vec<MediaMeta>`：供前端按需取可视区元数据。

5. **前端异步拉取**（[mediaStore.ts](../src/stores/mediaStore.ts) / [useVirtualScroll.ts](../src/composables/useVirtualScroll.ts)）
   - `visibleRows` 更新时，收集当前 ±2 屏的 id，**防抖**调 `get_meta_for_viewport`，结果响应式合并到视图。
   - 支撑"用户勾选任意信息（机型/镜头/光圈/GPS…）显示在缩略图卡片上"且滚动不掉帧（短暂停顿后文字补齐）。

**收益**：常驻内存降一个数量级；排版查询不再搬运重型字符串；卡片自定义信息流畅显示。

### A2. `LayoutState` O(1) 索引（回写 + 相邻）

**现状**：缩略图状态回写在 [thumbnail_commands.rs](../src-tauri/src/ipc/thumbnail_commands.rs) 共 6 处，双层 `for` 全表 + 线性 `find`，复杂度 `O(总项数 × 批大小)`，且持写锁饿死滚动读；`get_adjacent_item`（[cache.rs](../src-tauri/src/layout/cache.rs)）每次方向键 O(N) 展平全表。

**方案**（[cache.rs](../src-tauri/src/layout/cache.rs)）
- `LayoutCacheData` 新增：
  - `item_index: HashMap<i64, (usize, usize)>` —— id → (行下标, 行内下标)。
  - `flat_ids: Vec<i64>` —— 按布局顺序的 id 序列。
- `store_layout` 末尾顺手构建两索引。
- 新增 `update_item_thumb(id, status, path, hash)`：经 `item_index` 直达 `(row, col)` 修改，纳秒级，持写锁极短。
- `get_adjacent_item` 改用 `item_index` + `flat_ids`，O(1)。
- [thumbnail_commands.rs](../src-tauri/src/ipc/thumbnail_commands.rs) 6 处回写改调 `update_item_thumb`（双流水线均适配，**暂不删除任一流水线**）。

**测试**：对 `LayoutState` 写单测 —— 用 id 更新状态后，断言对应 `LayoutRow` 项被准确修改。

### A3. 布局查询/计算整体进 `spawn_blocking`
- [`compute_layout`](../src-tauri/src/ipc/layout_commands.rs) 现在在 async 体内**同步** `query+collect`，再把算法放进 `spawn_blocking` —— 前半段阻塞 tokio worker。
- 将"查询 + 排版算法"整体包进**同一个** `spawn_blocking`。

---

## 二、Phase B — 突破浏览器渲染高度上限（B1，1M 硬性前置）

**问题**：[useVirtualScroll.ts](../src/composables/useVirtualScroll.ts) 用 `totalHeight` 撑开容器。Chromium/WebView2 单元素高度安全阈值约 **16,777,216 px**，超出被钳制 → 滚动条失效/到不了底。百万张含分隔符 `totalHeight ≈ 4000 万 px+`，约 25–50 万行即击穿。

**方案：滚动坐标平移（Scroll Coordinate Translation）**
1. **阈值守卫**：`totalHeight ≤ SAFE_MAX(10,000,000 px)` 走现状路径（≤~25 万张无感）。
2. 超阈值启用虚拟坐标系（封装 `useTranslatedScroll`，与 `useVirtualScroll` 按阈值切换，互不污染）：
   - DOM 容器高度固定 `SAFE_MAX`；`ratio = logicalTotalHeight / SAFE_MAX`。
   - 拦截 `scroll`：`logicalY = scrollTop * ratio`，据此 `get_layout_rows_by_y(logicalY ± buffer)`，可视行用 `translateY` 修正到物理视口。
   - 拦截 `wheel`：按物理像素步进换算逻辑步进，保证滚轮线性手感。
   - 键盘 `PageUp/Down/Home/End` 走逻辑坐标。
3. **漂移修正**：比例映射使滚动条 1px 对应多行，停止滚动时按"当前顶部行"锚点重对齐，消除累积误差。
4. resize 后重算 `ratio`。

**风险/回退**：本项是最重前端改造（滚动条精度变粗、需处理触摸板惯性）。先在 50 万级数据集专项测试；保留阈值切换可随时回退旧路径。

---

## 三、Phase C — AI 向量搜索

**现状**：[`semantic_search`](../src-tauri/src/ai/search.rs) 每次查询从 SQLite 读**全部** embedding（百万 ≈ 2GB/查询）再单线程余弦 —— 主瓶颈是"每查询读盘"。

### C1. 常驻 + 并行（本轮，f16）
- 引入 `RwLock<EmbeddingCache>`，启动/首搜时一次性把全部向量以 **f16** 载入内存（百万 ≈ 1GB）。
- 余弦用 `rayon::par_iter`（+ 可选 SIMD / `ndarray`）。
- 增量：新分析完成的 embedding 增量写入常驻缓存；`reset/rebuild` 时清空重建。
- 效果：百万级精确 Top-K ~10–30ms。

### C2. ANN 索引（后续里程碑，二选一，届时再定）
- 当 C1 内存/延迟在 1M 不可接受时引入近似最近邻：
  - **hnsw_rs**：纯 Rust，无原生构建；但索引常驻内存偏大。
  - **sqlite-vec**：C 扩展静态链接进 rusqlite，自带量化 + mmap，内存随索引而非全量；新增构建依赖。
- 权衡：ANN 近似（可能漏召回边缘 Top-K）。**本轮不做，留作 C1 实测后的决策。**

---

## 四、Phase D — 健壮性与一致性

### D1. 解码崩溃保护（panic=abort → unwind）
- [Cargo.toml](../src-tauri/Cargo.toml)：移除 `panic = "abort"`（恢复默认 `unwind`）。
- 在解码热点（WIC / `image` / `fast_image_resize`）入口包 `std::panic::catch_unwind`，畸形/损坏图片 panic → 捕获并降级（JPEG 或 `thumb_status=2`），杜绝整程闪退。
- 验证：注入损坏 webp/极端尺寸图，全量缩略图不静默退出。

### D2. 退出前 WAL checkpoint
- [lib.rs](../src-tauri/src/lib.rs) 在 `RunEvent::ExitRequested`/`Exit` 处理里，`process::exit` 之前显式 `PRAGMA wal_checkpoint(TRUNCATE)`，避免 `-wal` 无限膨胀。

### D3. 收藏/删除缓存一致性（经 A2 的 O(1) 索引）
- [media_commands.rs](../src-tauri/src/ipc/media_commands.rs) `toggle_favorite`：DB 写后用 `item_index` O(1) 同步缓存内 `is_favorited`。
- `soft_delete_items`：经索引从布局移除 / 标脏，解决"删了仍短暂停留"。

---

## 五、Phase E — 安全与清理

### E1. 安全面收敛
- [tauri.conf.json](../src-tauri/tauri.conf.json)：`assetProtocol.scope` 从全盘 `C:/** … G:/**` 收紧到 `scan_roots` 实际根目录（运行时动态授权）。
- 排查并尽量移除 CSP 中 `script-src 'unsafe-eval'`（定位是哪个依赖要求）。

### E2. 小清理
- [layout_commands.rs](../src-tauri/src/ipc/layout_commands.rs)：`compute_layout` 内 3 次 `get_summary` 合并为 1 次取锁。
- `batch_request_thumbnails` 的 `IN(...)` 改参数绑定（[thumbnail_commands.rs](../src-tauri/src/ipc/thumbnail_commands.rs)）。
- 补写 [architecture_notes.md](architecture_notes.md)（当前为空）、刷新 [README.md](../README.md)（仍是 Tauri 模板）。

> **暂缓**：删除 Scheme1/Scheme2 冗余缩略图流水线（按反馈保留）。A2 的 O(1) 回写对两套均适配。

---

## 六、实施顺序与里程碑

- **M1（地基）**：A1 → A2 → A3 + D1。完成后内存降至 ~1/5、滚动写锁争用根除、损坏文件不再拖垮全程。30–50 万级流畅。
- **M2（破百万）**：B1。解锁百万张单视图浏览。
- **M3（搜索）**：C1（f16 常驻 + rayon）。
- **M4（打磨）**：D2、D3、E1、E2；视 C1 实测决定是否上 C2。

---

## 七、验证与基准（先建基准，再改）

合成 N = 1万 / 10万 / 50万 / 100万 数据集，测量：

| 指标 | 通过线 |
|------|--------|
| `compute_layout` 耗时 | 100万 ≤ 1.5s |
| 布局缓存常驻内存 | 100万 ≤ 200MB |
| `get_layout_rows_by_y` 窗口查 | ≤ 2ms |
| 可视区 EXIF 异步拉取 | 停顿后正确显示，拖拽不掉帧 |
| 快速滚动 FPS（含坐标平移） | 全程 ≥ 55–60 |
| 滚到底/到顶（坐标平移） | 正常，无断层 |
| 语义搜索延迟（f16 常驻） | 100万 ≤ 200ms |
| 含损坏文件全量缩略图 | 不闪退，降级标记 |

### 自动化
- `LayoutState` O(1) 索引单测：按 id 更新后准确改到对应 `LayoutRow` 项。

### 手动
- 勾选全部 EXIF 展示，快速滚动瀑布流，验证异步拉取在短停顿后正确补字、不掉帧。
- 点"全部照片"完成排版后，后端内存应降至原预估的 ~1/3 以内。

---

## 八、风险

| 风险 | 应对 |
|------|------|
| B1 坐标平移交互回归（滚轮/键盘/触摸板惯性） | 阈值切换 + 独立 composable + 50 万专项测试，可回退旧路径 |
| A1 viewport 异步拉取在极速滚动下抖动 | ±2 屏预取 + 防抖 + AbortController 取消离场请求 |
| f16 精度损失影响召回 | 实测对比 f32 Top-K 重合度；必要时关键路径回 f32 |
| `unwind` 体积/性能回归 | 基准对比；仅解码热点包 catch_unwind |
| C2 ANN 近似漏召回 / 原生构建 | 本轮只做 C1（精确）；C2 作为可选 feature |
