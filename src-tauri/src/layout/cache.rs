// src-tauri/src/layout/cache.rs
//! 存储在 `AppState` 中的布局缓存。
//!
//! `compute_layout` 将结果存储于此；`get_layout_rows` 读取切片。
//! `layout_version` 计数器用于防止读取过期数据。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use rustc_hash::FxHashMap;

use serde::{Deserialize, Serialize};

use crate::layout::geometry::{GallerySeparatorKind, LayoutRow};

static LAYOUT_VERSION_COUNTER: AtomicU64 = AtomicU64::new(0);
static ORDER_VERSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparatorInfo {
    pub label: String,
    pub y: f64,
    pub group_id: Option<String>,
    #[serde(
        rename = "separatorKind",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub separator_kind: Option<GallerySeparatorKind>,
    #[serde(
        rename = "duplicateGroupOrdinal",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub duplicate_group_ordinal: Option<u32>,
    #[serde(
        rename = "duplicateMemberCount",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub duplicate_member_count: Option<u32>,
    #[serde(
        rename = "duplicateFolderCount",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub duplicate_folder_count: Option<u32>,
    #[serde(
        rename = "duplicateUnitSize",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub duplicate_unit_size: Option<i64>,
    #[serde(
        rename = "parentGroupOrdinal",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub parent_group_ordinal: Option<u32>,
    #[serde(
        rename = "parentGroupFolderCount",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub parent_group_folder_count: Option<u32>,
    #[serde(
        rename = "parentGroupGroupCount",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub parent_group_group_count: Option<u32>,
    /// 该分组的媒体项数：date 分组=该「日」项数，folder 分组=该文件夹项数。与月桶同法在同一次行
    /// 遍历累加（紧跟本分隔符的 Normal 行项数累加进当前分隔符）。供时间轴放大镜显示每项数量。
    pub count: usize,
    /// date 分组下该「日」的 epoch 天数（`sort_datetime.div_euclid(86400)`，UTC 日界，与
    /// `GroupMark::Day` 同源）；folder/none 分组无「日」概念 = `None`。P3 时间比例坐标据此把
    /// separator 按日历时间线性铺行 + 相邻 epoch_day 间隔 >1 检测空日间隙（前端零填充成低谷）。
    pub epoch_day: Option<i64>,
}

/// 月密度桶（T14 §3.8.3）：date 分组下把同月的多个「日分隔符」合并为一个桶，供 Part5 时间轴
/// scrubber 按**时间均布**（而非逻辑高度均布）+ 密度条渲染。**仅 date 分组非空**——folder/none
/// 分组无「月」概念（其 group_id 为 dir_id / 无），合并时被自然跳过 → `month_buckets` 为空。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthBucket {
    pub year: i32,
    pub month: u32, // 1-12
    /// 该月媒体项数（密度条高度依据）。
    pub count: usize,
    /// 该月**布局序最靠前**分隔符的逻辑 y（DESC 序下=该月最新一天；scrubber 跳转定位）。
    pub y: f64,
    /// `"YYYY-MM"`——前端按月→y 定向滚动键（对齐 `get_separator_y_by_group_id`）。
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSummary {
    pub total_rows: usize,
    pub total_height: f64,
    pub layout_version: u64,
    /// 成员与排列的身份；纯几何重排沿用。
    pub order_version: u64,
    pub total_items: usize,
    pub separators: Vec<SeparatorInfo>,
    /// 月密度桶（date 分组才非空，见 [`MonthBucket`]）。与 separators 同一次行遍历构建，
    /// 与 `layout_version` 原子一致（无需独立 IPC 二次往返）。
    pub month_buckets: Vec<MonthBucket>,
}

/// id → 扁平下标索引（S3.3）。自增主键下 id 域紧凑（max_id ≈ N），用 `Vec<u32>` 直址
/// （`u32::MAX` = 空位）：构建为顺序读 + L2/L3 级随机写，较 1M 次哈希插入快一个量级
/// （哈希表随机访存是 store 段剩余大头，实测 ~112ms 中占多半）；id 域稀疏
/// （max_id > 4N + 1024，如大量删除后的老库）退回 FxHashMap，查询行为等价。
/// flat 下标恒 < u32::MAX（百万级 << 40 亿），u32 存储安全。
pub enum IdToFlat {
    Dense(Vec<u32>),
    Sparse(FxHashMap<i64, u32>),
}

impl IdToFlat {
    /// 从布局序 id 全集构建（flat 下标 = 数组位置）。
    fn build(flat_ids: &[i64]) -> Self {
        if flat_ids.is_empty() {
            return IdToFlat::Dense(Vec::new());
        }
        let (min_id, max_id) = flat_ids.iter().fold((i64::MAX, i64::MIN), |(lo, hi), &id| {
            (lo.min(id), hi.max(id))
        });
        // 密集判据带常数余量：小库即便 id 有空洞也直址（内存上限不触发）；
        // 防御:负 id（正常库不存在）一律退稀疏形态。
        if min_id >= 0 && (max_id as usize) < flat_ids.len() * 4 + 1024 {
            let mut v = vec![u32::MAX; max_id as usize + 1];
            for (flat, &id) in flat_ids.iter().enumerate() {
                v[id as usize] = flat as u32;
            }
            IdToFlat::Dense(v)
        } else {
            let mut m: FxHashMap<i64, u32> =
                FxHashMap::with_capacity_and_hasher(flat_ids.len(), Default::default());
            for (flat, &id) in flat_ids.iter().enumerate() {
                m.insert(id, flat as u32);
            }
            IdToFlat::Sparse(m)
        }
    }

    /// O(1) 查询：id → flat 下标。未知 id / 负 id → None。
    pub fn get(&self, id: i64) -> Option<usize> {
        match self {
            IdToFlat::Dense(v) => match v.get(usize::try_from(id).ok()?) {
                Some(&f) if f != u32::MAX => Some(f as usize),
                _ => None,
            },
            IdToFlat::Sparse(m) => m.get(&id).map(|&f| f as usize),
        }
    }
}

/// 同一成员顺序共享的全集与索引，让邻接导航和按 ID 重锚定保持 O(1)。
pub struct ViewOrder {
    pub version: u64,
    pub flat_ids: Vec<i64>,
    pub id_to_flat: IdToFlat,
}

/// 几何按布局代更新；全集顺序及 ID 索引在几何重排之间共享。
pub struct LayoutCacheData {
    pub rows: Vec<LayoutRow>,
    pub total_height: f64,
    pub layout_version: u64,
    pub total_items: usize,

    pub order: Arc<ViewOrder>,
    /// 不含几何的排序输入；必须同时匹配 source_items 的快照身份才允许复用。
    order_key: String,
    /// 与 `order.flat_ids` 并行：扁平下标 → (行下标, 行内项下标)。
    pub flat_rowcol: Vec<(u32, u32)>,

    /// S3.1 幂等去重键：本代布局的构建输入指纹（布局参数 + 过滤器键 + 数据代）。
    /// compute_layout 据此短路「同参数同数据代」的重复触发（见 dedup_summary）。
    pub gen_key: String,

    /// S3.5：分隔符/月桶随 store_layout 同遍物化（原 get_summary 每次全行扫描重建，
    /// 行数级成本挪到换代一次性支付；摘要退化为 O(分隔符数) 克隆）。
    pub separators: Vec<SeparatorInfo>,
    pub month_buckets: Vec<MonthBucket>,

    /// 重复镜头逐项投影（2026-09-02 方案 §11.1）：出口 hydrate 按可视区查表填
    /// `duplicateBucket`/序号字段。普通画廊布局恒 None（零成本）；镜头布局随本代
    /// 缓存一起换代（gen_key 已含镜头维度与 dedup_view_epoch）。
    pub lens_projection: Option<Arc<crate::layout::lens::LensProjection>>,
    /// 与几何一起发布的载荷快照。测试/纯几何调用可不提供；生产出口必须有。
    pub source_items: Option<Arc<crate::layout::items_cache::ItemsCache>>,
}

/// 布局缓存 — 存储在 `AppState` 中的 `RwLock` 后面。
pub type LayoutCache = RwLock<Option<LayoutCacheData>>;

/// 创建一个全新的布局缓存（初始为空）。
pub fn new_layout_cache() -> LayoutCache {
    RwLock::new(None)
}

/// 存储新的布局，自动递增版本号。`gen_key` 为本代布局的构建输入指纹（见
/// LayoutCacheData::gen_key）；不关心去重的调用方（测试等）传 String::new()，空键永不命中。
pub fn store_layout(
    cache: &LayoutCache,
    rows: Vec<LayoutRow>,
    total_height: f64,
    gen_key: String,
) -> u64 {
    store_layout_with_lens(cache, rows, total_height, gen_key, None)
}

/// 带（可选）镜头投影的存储（2026-09-02 方案 §11.1）：镜头布局调用方传投影表，
/// 普通画廊传 None。`Arc` 包裹使出口读侧 clone 廉价（每可视区调用一次）。
pub fn store_layout_with_lens(
    cache: &LayoutCache,
    rows: Vec<LayoutRow>,
    total_height: f64,
    gen_key: String,
    lens_projection: Option<Arc<crate::layout::lens::LensProjection>>,
) -> u64 {
    let prepared = prepare_layout(
        cache,
        rows,
        total_height,
        gen_key,
        lens_projection,
        None,
        String::new(),
    );
    publish_layout(cache, prepared).layout_version
}

/// 已在锁外物化的布局和摘要。
pub struct PreparedLayout {
    pub data: LayoutCacheData,
    summary: LayoutSummary,
}

/// 索引和摘要在发布锁外准备，避免大集合物化阻塞请求登记与视口读取。
/// `order_key` 必须包含所有非几何的顺序输入；空键或无快照时不复用顺序。
pub fn prepare_layout(
    cache: &LayoutCache,
    rows: Vec<LayoutRow>,
    total_height: f64,
    gen_key: String,
    lens_projection: Option<Arc<crate::layout::lens::LensProjection>>,
    source_items: Option<Arc<crate::layout::items_cache::ItemsCache>>,
    order_key: String,
) -> PreparedLayout {
    // 只证明同一快照与顺序输入，不扫描/哈希全集去猜测两个不同快照是否恰好同序。
    let reused_order = {
        let guard = cache.read().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .filter(|data| {
                !order_key.is_empty()
                    && data.order_key == order_key
                    && data
                        .source_items
                        .as_ref()
                        .zip(source_items.as_ref())
                        .is_some_and(|(old, new)| Arc::ptr_eq(old, new))
            })
            .map(|data| data.order.clone())
    };
    let order_reused = reused_order.is_some();
    // 在仍持有 `rows` 时一次遍历构建扁平索引。S3.2：预扫总项数（10^5 行级轻扫）
    // 换三容器零重分配/零重哈希——1M 项下多轮扩容重哈希此前占换代耗时大头。
    let total: usize = rows
        .iter()
        .map(|r| match r {
            LayoutRow::Normal { items, .. } => items.len(),
            _ => 0,
        })
        .sum();
    let mut flat_ids: Vec<i64> = Vec::with_capacity(if reused_order.is_some() { 0 } else { total });
    let mut flat_rowcol: Vec<(u32, u32)> = Vec::with_capacity(total);
    // 月桶（§3.8.3）：与扁平索引同一次遍历构建。date 分组分隔符 group_id="YYYY-MM"（§3.8.2）→
    // 解析为 (year, month)；同月相邻分隔符并入同桶，Normal 行项数累加进**当前**桶。folder/none
    // 分组的 group_id（dir_id / None）解析失败 → 不建桶 → month_buckets 为空。
    let mut separators: Vec<SeparatorInfo> = Vec::new();
    let mut month_buckets: Vec<MonthBucket> = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        match row {
            LayoutRow::Normal { items, .. } => {
                for (ii, item) in items.iter().enumerate() {
                    if reused_order.is_none() {
                        flat_ids.push(item.id);
                    }
                    flat_rowcol.push((ri as u32, ii as u32));
                }
                // Normal 行紧跟其所属分隔符之后 → 累加进当前（最后一个）月桶；none 分组无桶则跳过。
                if let Some(bucket) = month_buckets.last_mut() {
                    bucket.count += items.len();
                }
                // 同法累加进当前分隔符（date=该日项数 / folder=该文件夹项数）；none 分组无分隔符则跳过。
                if let Some(sep) = separators.last_mut() {
                    sep.count += items.len();
                }
            }
            LayoutRow::Separator {
                y,
                separator_label,
                group_id,
                epoch_day,
                separator_kind,
                lens_group,
                lens_folder,
                ..
            } => {
                separators.push(SeparatorInfo {
                    label: separator_label.clone(),
                    y: *y,
                    group_id: group_id.clone(),
                    separator_kind: *separator_kind,
                    duplicate_group_ordinal: lens_group.as_ref().map(|g| g.ordinal),
                    duplicate_member_count: lens_group.as_ref().map(|g| g.member_count),
                    duplicate_folder_count: lens_group.as_ref().map(|g| g.folder_count),
                    duplicate_unit_size: lens_group.as_ref().map(|g| g.unit_size),
                    parent_group_ordinal: lens_folder.as_ref().map(|f| f.parent_group_ordinal),
                    parent_group_folder_count: lens_folder
                        .as_ref()
                        .map(|f| f.parent_group_folder_count),
                    parent_group_group_count: lens_folder
                        .as_ref()
                        .map(|f| f.parent_group_group_count),
                    count: 0, // 由紧随的 Normal 行累加
                    epoch_day: *epoch_day,
                });
                if let Some((year, month)) = group_id.as_deref().and_then(parse_year_month) {
                    // 仅当「月」变化时开新桶；同月的后续日分隔符沿用当前桶（y 已为该月首个=最新一天）。
                    let same_month = month_buckets
                        .last()
                        .is_some_and(|b| b.year == year && b.month == month);
                    if !same_month {
                        month_buckets.push(MonthBucket {
                            year,
                            month,
                            count: 0,
                            y: *y,
                            group_id: format!("{year}-{month:02}"),
                        });
                    }
                }
            }
        }
    }
    // S3.3：id 索引从 flat_ids 二次构建（顺序读 + 密集时 L2/L3 级随机写直址表），
    // 替代循环内 1M 次哈希插入——后者的随机访存是 store 段的剩余大头。
    let order = reused_order.unwrap_or_else(|| {
        Arc::new(ViewOrder {
            version: ORDER_VERSION_COUNTER.fetch_add(1, Ordering::Relaxed) + 1,
            id_to_flat: IdToFlat::build(&flat_ids),
            flat_ids,
        })
    });
    let total_items = total;

    let data = LayoutCacheData {
        rows,
        total_height,
        layout_version: 0,
        total_items,
        order,
        order_key,
        flat_rowcol,
        gen_key,
        separators,
        month_buckets,
        lens_projection,
        source_items,
    };
    let summary = summary_of(&data);
    tracing::debug!(
        order_version = summary.order_version,
        order_reused,
        total_items,
        "layout order index | 顺序索引物化/复用"
    );
    PreparedLayout { data, summary }
}

/// 发布已准备好的布局并返回它自己的摘要，不在发布后重读全局状态。
pub fn publish_layout(cache: &LayoutCache, mut prepared: PreparedLayout) -> LayoutSummary {
    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    prepared.data.layout_version = LAYOUT_VERSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    prepared.summary.layout_version = prepared.data.layout_version;
    let old_gen = guard.replace(prepared.data);
    drop(guard);
    // 旧代堆释放(数十万 Vec + 1M 索引)卸到后台线程:数据已离开缓存,无锁无共享,纯释放
    // 工作,不必占用重排关键路径(实测 1M 库该段数十 ms)。
    if old_gen.is_some() {
        std::thread::spawn(move || drop(old_gen));
    }
    prepared.summary
}

// ── 失效契约（R1-5 裁决，2026-07-02）───────────────────────────────────────────
//
// **软删除/恢复有意不失效、不 bump 布局缓存**：T18 暂存删除 UX 要求前端在退出选择模式前
// 继续按旧布局滚动（暂存项仅置灰，commitPendingReflow 统一重排时才 compute 换代）。
// 若删除后立即 bump version，暂存窗口内所有 get_layout_rows(_by_y) 按旧 version 取行都会
// 失效 → 虚拟滚动破图，恰好毁掉该 UX——审查 R1-5 原案「删除后 bump」据此作废。
//
// 此 stale 窗口的**写路径正确性**由 SQL 层双守卫兜底（queries.rs 有测试锁定）：
//   ① SelectAll 解析走 view_to_sql → push_query_body 恒带 is_deleted 谓词，已删项不进目标集；
//   ② 批量写 UPDATE 恒带 `AND is_deleted = 0`，对已删 id 是 no-op。
// 故缓存残留已删项的 flat_ids/rows 只影响展示（前端自知并自行重排），不影响数据正确性。
// 扫描插入/回收站恢复后的缓存刷新由前端既有重算触发器承担（scan 完成事件 / 视图切换 watcher /
// 撤销路径的显式 compute）。
//
// **D3 布局侧 patch 已随 S3 退役**（thumb/favorite/rating/color 四组）：行内不再驻留载荷
// 字段，滚出滚回的新鲜度由出口拼装（items_cache::hydrate_rows）自 items 取数缓存天然获得，
// patch 单点化到 items_cache（见 AppState 组合函数）。

/// 取按布局序的视图全集 id（T14.5 `get_view_ids` 后端）。
///
/// 直接返回缓存内已物化的 `flat_ids` —— 它由 `compute_layout`（经 `query_layout_items`）产出，与
/// `view_to_sql` 的 id-only 路径**同源同序**（同一 `push_query_body` 的 ORDER BY），故功能等价于
/// `resolve_selection(SelectAll{view, []})`，但无需 DB 往返（复制/传输仍为 O(N)，设计 §5.3：
/// 百万级 Ctrl+A / 框选不应每次重查 DB；单一事实源诉求由 S3 批量写路径的 `resolve_selection` 承担）。
///
/// `expected_version` 与当前顺序不一致 → 返回 `None`；纯几何重排不使全集失效。
pub fn get_view_ids(cache: &LayoutCache, expected_version: Option<u64>) -> Option<Vec<i64>> {
    let order = {
        let guard = cache.read().unwrap_or_else(|e| e.into_inner());
        let data = guard.as_ref()?;
        if expected_version.is_some_and(|ver| data.order.version != ver) {
            return None;
        }
        data.order.clone()
    };
    Some(order.flat_ids.clone())
}

/// 只读当前顺序身份，不物化全集或摘要。
pub fn current_order_version(cache: &LayoutCache) -> Option<u64> {
    cache
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|data| data.order.version)
}

/// 只读当前布局的镜头身份：确实是镜头布局时返回其 `layout_version`，普通布局或无布局返回
/// `None`。镜头邻接的详情 await 后复验用——普通布局不得让迟到的镜头详情落地。
pub fn current_lens_layout_version(cache: &LayoutCache) -> Option<u64> {
    cache
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|data| data.lens_projection.is_some())
        .map(|data| data.layout_version)
}

/// 从缓存中检索与 [top_y, bottom_y] 相交的行切片。
pub fn get_rows_by_y(
    cache: &LayoutCache,
    top_y: f64,
    bottom_y: f64,
    expected_version: Option<u64>,
) -> Option<Vec<LayoutRow>> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;

    if let Some(ver) = expected_version {
        if data.layout_version != ver {
            return None;
        }
    }

    let start_idx = match data.rows.binary_search_by(|r| {
        r.y()
            .partial_cmp(&top_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };

    let mut end_idx = start_idx;
    while end_idx < data.rows.len() && data.rows[end_idx].y() <= bottom_y {
        end_idx += 1;
    }

    Some(data.rows[start_idx..end_idx].to_vec())
}

/// Retrieve the rows whose y lies in [start_y, end_y) — exact bucket membership
/// for segmented virtual scrolling (T16 方案 B / B0).
/// 检索 y 落在 [start_y, end_y) 内的行——bucket 分段虚拟滚动的精确段归属(T16 方案 B / B0)。
///
/// 与 [`get_rows_by_y`] 的「视口相交」语义不同:那里为覆盖跨顶行会向前多取一行、尾部用 `<=`,
/// 拿来取整段会把邻桶的行掺进来。bucket 边界恰是分隔符行的 y、行与行不重叠,故按行首 y 的
/// 半开区间判归属即精确。边界由调用方供给(month_buckets / separators 的相邻 y,末段用
/// total_height),本函数不限定「月」——folder 分组(B3)可原样复用。两端均 partition_point
/// 二分,超大桶亦 O(log n) 定位。
pub fn get_bucket_rows(
    cache: &LayoutCache,
    start_y: f64,
    end_y: f64,
    expected_version: Option<u64>,
) -> Option<Vec<LayoutRow>> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;

    if let Some(ver) = expected_version {
        if data.layout_version != ver {
            return None;
        }
    }

    let start_idx = data.rows.partition_point(|r| r.y() < start_y);
    let end_idx = start_idx + data.rows[start_idx..].partition_point(|r| r.y() < end_y);
    Some(data.rows[start_idx..end_idx].to_vec())
}

/// 获取布局摘要（行数 + 总高度 + 版本）。
pub fn get_summary(cache: &LayoutCache) -> Option<LayoutSummary> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(summary_of)
}

/// S3.1 幂等去重探针：现行布局代若由**完全相同的输入**构建（gen_key 相等），返回其摘要，
/// 供 compute_layout 直接复用——免一次全量重排与版本换代。版本不变意味着前端 bucket
/// 引擎的 layoutVersion watcher 不会被虚假换代惊动（段表零重建）。空键永不命中。
pub fn dedup_summary(cache: &LayoutCache, gen_key: &str) -> Option<LayoutSummary> {
    if gen_key.is_empty() {
        return None;
    }
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;
    if data.gen_key != gen_key {
        return None;
    }
    Some(summary_of(data))
}

/// 摘要构建（get_summary / dedup_summary 共用）：单次遍历行集收集分隔符与月桶。
fn summary_of(data: &LayoutCacheData) -> LayoutSummary {
    // S3.5：分隔符/月桶已随 store_layout 物化——摘要为 O(分隔符数) 克隆（原每次全行扫描）。
    LayoutSummary {
        total_rows: data.rows.len(),
        total_height: data.total_height,
        layout_version: data.layout_version,
        order_version: data.order.version,
        total_items: data.total_items,
        separators: data.separators.clone(),
        month_buckets: data.month_buckets.clone(),
    }
}

/// 当前布局的镜头投影（普通画廊 = None；镜头布局 = Arc 表，出口 hydrate 据此填
/// 逐项镜头字段）。短读锁取 Arc clone——与 items 读锁先后独立取放（S1 纪律）。
pub fn get_lens_projection(
    cache: &LayoutCache,
) -> Option<Arc<crate::layout::lens::LensProjection>> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|d| d.lens_projection.clone())
}

/// 解析 `"YYYY-MM"` → `(year, month)`。非此形（folder dir_id 如 `"42"`、或其它）返回 `None`。
/// date 分组的 group_id 必为 `"YYYY-MM"`（justified.rs `timestamp_to_year_month`），故可据此判别。
fn parse_year_month(group_id: &str) -> Option<(i32, u32)> {
    let (y, m) = group_id.split_once('-')?;
    let year: i32 = y.parse().ok()?;
    let month: u32 = m.parse().ok()?;
    if (1..=12).contains(&month) {
        Some((year, month))
    } else {
        None
    }
}

/// 一步邻接查询结果（普通画廊与镜头两个入口共用）。
///
/// 该枚举把缓存状态、上下文归属、当前项缺失与真实首尾边界分开，调用方可以只把
/// `OutOfBounds` 映射为「没有上一项/下一项」，其它不一致一律拒绝——不静默落回别的
/// 集合、别的顺序或当前已加载的行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjacentLookup {
    LayoutNotReady,
    ViewStale,
    CurrentMissing,
    OutOfBounds,
    Found {
        id: i64,
        index: usize,
        total_count: usize,
    },
}

/// 在已持读锁的缓存数据上解析一步相邻项（版本与布局类别由两个入口各自先行判定）。
fn adjacent_in_order(data: &LayoutCacheData, current_id: i64, offset: isize) -> AdjacentLookup {
    // 通过 id 索引 O(1) 完成 — 不再每步导航都展平全表。
    let Some(current_idx) = data.order.id_to_flat.get(current_id) else {
        return AdjacentLookup::CurrentMissing;
    };
    let Some(target_idx) = (current_idx as isize).checked_add(offset) else {
        return AdjacentLookup::OutOfBounds;
    };
    if target_idx < 0 {
        return AdjacentLookup::OutOfBounds;
    }
    let Some(&id) = data.order.flat_ids.get(target_idx as usize) else {
        return AdjacentLookup::OutOfBounds;
    };
    AdjacentLookup::Found {
        id,
        index: target_idx as usize,
        total_count: data.total_items,
    }
}

/// 普通画廊的一步邻接查询，必须携带打开查看器时的 `order_version`。
///
/// 只认同一顺序代的**普通**布局：镜头布局有各自契约，不能冒充普通邻接序（版本相同也不
/// 例外）。版本不符或当前缓存是镜头布局 → `ViewStale`；当前项不在本顺序集合 →
/// `ViewStale`（不得借当前已加载行或别的集合虚构邻居）；目标越过首尾才是
/// `OutOfBounds`。纯几何重排沿用同一 `order_version`，故不使导航失效。
pub fn get_adjacent_item(
    cache: &LayoutCache,
    current_id: i64,
    offset: isize,
    order_version: u64,
) -> AdjacentLookup {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_ref() else {
        return AdjacentLookup::LayoutNotReady;
    };
    if data.order.version != order_version || data.lens_projection.is_some() {
        return AdjacentLookup::ViewStale;
    }
    adjacent_in_order(data, current_id, offset)
}

/// 镜头查看器的一步邻接查询，沿用既有 `layoutVersion` 契约（镜头几何换代即过期），
/// 并要求当前缓存确实是镜头布局——普通布局不得冒充镜头导航序。
pub fn get_lens_adjacent_item(
    cache: &LayoutCache,
    current_id: i64,
    offset: isize,
    expected_version: u64,
) -> AdjacentLookup {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_ref() else {
        return AdjacentLookup::LayoutNotReady;
    };
    if data.layout_version != expected_version || data.lens_projection.is_none() {
        return AdjacentLookup::ViewStale;
    }
    adjacent_in_order(data, current_id, offset)
}

/// Find the Y coordinate of the row containing the item with `item_id` (O(1) via the
/// id index). Used to re-anchor the viewport to a previously-viewed item after a
/// layout reflow (e.g. a thumbnail row-height change): once the total height changes
/// the old physical scrollTop maps to a different logical position, so we look up the
/// item's new row Y and scroll back to it instead.
/// 查找包含 `item_id` 的行的 Y 坐标（通过 id 索引 O(1)）。用于在布局重排（如缩略图
/// 行高变化）后把视口重新锚定到之前浏览的项：总高度变化后，旧的物理 scrollTop 对应的
/// 逻辑位置已不同，因此查出该项的新行 Y 并滚回去。
///
/// `expected_version` 为调用方持有的布局代；同一读锁内校验，无布局或版本不符 → 外层
/// `None`（调用方据此重算 layout）；版本一致但当前布局无此项 → `Some(None)`（该项确已
/// 不在本视图，区别于「缓存还没就绪」）。
pub fn get_item_y_by_id(
    cache: &LayoutCache,
    item_id: i64,
    expected_version: u64,
) -> Option<Option<f64>> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;
    if data.layout_version != expected_version {
        return None;
    }
    let Some(flat) = data.order.id_to_flat.get(item_id) else {
        return Some(None);
    };
    let Some(&(ri, _ii)) = data.flat_rowcol.get(flat) else {
        return Some(None);
    };
    Some(data.rows.get(ri as usize).map(|r| r.y()))
}

/// Find the FIRST separator (in layout order) whose group_id is in `ids`, returning its
/// (group_id, y). Used to scroll a clicked folder that has no direct media to its first
/// descendant subfolder that does (问题1) — pass the clicked dir's whole subtree id set.
/// 查找布局顺序中首个 group_id 命中 `ids` 的分隔符，返回其 (group_id, y)。用于把点击的
/// 「无直接媒体」文件夹滚动到其首个「有媒体」的后代子文件夹（问题1）——传入该文件夹的整棵
/// 子树 id 集合。
pub fn get_first_separator_y_in_set(
    cache: &LayoutCache,
    ids: &std::collections::HashSet<String>,
) -> Option<(String, f64)> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref()?;
    for row in &data.rows {
        if let LayoutRow::Separator {
            y,
            group_id: Some(gid),
            ..
        } = row
        {
            if ids.contains(gid) {
                return Some((gid.clone(), *y));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::geometry::{LayoutRow, SlimRowItem};
    use std::sync::Arc;

    fn mk_item(id: i64) -> SlimRowItem {
        SlimRowItem {
            id,
            x: 0.0,
            w: 100.0,
            h: 100.0,
        }
    }

    /// 分隔符 + 两个普通行；扁平项顺序为 [10, 11, 12]。
    fn sample_layout() -> Vec<LayoutRow> {
        vec![
            LayoutRow::Separator {
                y: 0.0,
                height: 36.0,
                separator_label: "d1".into(),
                group_id: None,
                epoch_day: None,
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 36.0,
                height: 100.0,
                items: vec![mk_item(10), mk_item(11)],
            },
            LayoutRow::Normal {
                y: 140.0,
                height: 100.0,
                items: vec![mk_item(12)],
            },
        ]
    }

    /// date 分组布局：March 两个日分隔符（应并入同一月桶）+ February 一个。
    /// 验证 §3.8.2/3.8.3：同月合并、count 累加、y 取该月首个分隔符。
    fn date_layout() -> Vec<LayoutRow> {
        vec![
            LayoutRow::Separator {
                y: 0.0,
                height: 36.0,
                separator_label: "2024年3月20日".into(),
                group_id: Some("2024-03".into()),
                epoch_day: Some(19802), // 2024-03-20 UTC 日序
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 36.0,
                height: 100.0,
                items: vec![mk_item(10), mk_item(11)],
            },
            // 同月不同日 → 不开新桶，count 继续累加进 3 月桶。
            LayoutRow::Separator {
                y: 200.0,
                height: 36.0,
                separator_label: "2024年3月19日".into(),
                group_id: Some("2024-03".into()),
                epoch_day: Some(19801), // 2024-03-19 UTC 日序（较上一日 -1，相邻无空隙）
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 236.0,
                height: 100.0,
                items: vec![mk_item(12)],
            },
            LayoutRow::Separator {
                y: 400.0,
                height: 36.0,
                separator_label: "2024年2月15日".into(),
                group_id: Some("2024-02".into()),
                epoch_day: Some(19768), // 2024-02-15 UTC 日序（距上一日 33 天，含空日间隙）
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 436.0,
                height: 100.0,
                items: vec![mk_item(13), mk_item(14)],
            },
        ]
    }

    #[test]
    fn get_view_ids_returns_flat_order_and_guards_version() {
        // T18 S2：flat_ids 即按布局序的视图全集 id（分隔符不计）。
        let cache = new_layout_cache();
        store_layout(&cache, sample_layout(), 240.0, String::new());
        let version = current_order_version(&cache).unwrap();

        // 无版本约束 → 直接拿 flat 序 [10, 11, 12]。
        assert_eq!(get_view_ids(&cache, None), Some(vec![10, 11, 12]));
        // 版本一致 → 同结果。
        assert_eq!(get_view_ids(&cache, Some(version)), Some(vec![10, 11, 12]));
        // 版本不符 → None（命令层据此抛 ViewStale）。
        assert_eq!(get_view_ids(&cache, Some(version + 99)), None);
    }

    #[test]
    fn get_view_ids_none_when_no_layout() {
        let cache = new_layout_cache();
        assert_eq!(
            get_view_ids(&cache, None),
            None,
            "无布局 → None（命令层抛 LayoutNotReady）"
        );
    }

    #[test]
    fn month_buckets_merge_same_month_and_sum_counts() {
        let cache = new_layout_cache();
        store_layout(&cache, date_layout(), 540.0, String::new());
        let s = get_summary(&cache).unwrap();

        assert_eq!(s.month_buckets.len(), 2, "March 两个日分隔符应并为一个月桶");
        let mar = &s.month_buckets[0];
        assert_eq!((mar.year, mar.month), (2024, 3));
        assert_eq!(mar.count, 3, "3 月跨两天共 3 项应累加");
        assert_eq!(mar.y, 0.0, "y 取该月首个（最新一天）分隔符");
        assert_eq!(mar.group_id, "2024-03");
        let feb = &s.month_buckets[1];
        assert_eq!((feb.year, feb.month, feb.count), (2024, 2, 2));
        assert_eq!(feb.y, 400.0);
    }

    /// P3 时间比例坐标：separator 的 epoch_day 须从 LayoutRow 原样物化进 SeparatorInfo，
    /// 且相邻 epoch_day 间隔可供前端检测空日间隙（19801→19768 差 33 天 = 空日低谷）。
    #[test]
    fn separators_carry_epoch_day_for_time_coord() {
        let cache = new_layout_cache();
        store_layout(&cache, date_layout(), 540.0, String::new());
        let s = get_summary(&cache).unwrap();

        let days: Vec<Option<i64>> = s.separators.iter().map(|sep| sep.epoch_day).collect();
        assert_eq!(
            days,
            vec![Some(19802), Some(19801), Some(19768)],
            "epoch_day 应原样穿过 store_layout（含 3/19→2/15 的 33 天空隙）"
        );
    }

    #[test]
    fn month_buckets_empty_for_folder_grouping() {
        // folder 分组：group_id 是 dir_id（纯数字，无 '-'）→ 解析失败 → 无月桶。
        let cache = new_layout_cache();
        let rows = vec![
            LayoutRow::Separator {
                y: 0.0,
                height: 36.0,
                separator_label: "相册/2024".into(),
                group_id: Some("42".into()),
                epoch_day: None, // folder 分组无「日」概念
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 36.0,
                height: 100.0,
                items: vec![mk_item(10)],
            },
        ];
        store_layout(&cache, rows, 136.0, String::new());
        let s = get_summary(&cache).unwrap();
        assert!(s.month_buckets.is_empty(), "folder 分组不应产出月桶");
        assert_eq!(s.separators.len(), 1, "但分隔符仍照常产出");
    }

    /// T16 方案B B0:半开区间 [start_y, end_y) 的精确段归属——起点边界行含入、
    /// 终点边界行不含入;并与 get_rows_by_y 的相交语义对照(后者会掺邻桶行)。
    #[test]
    fn get_bucket_rows_half_open_exact_membership() {
        let cache = new_layout_cache();
        let version = store_layout(&cache, date_layout(), 540.0, String::new());

        // 3 月桶 = [0, 400):两日共 4 行全含,不含 2 月首行(y=400 恰为 end_y → 排除)。
        let mar = get_bucket_rows(&cache, 0.0, 400.0, Some(version)).unwrap();
        assert_eq!(mar.len(), 4, "3 月桶应恰为 4 行");
        assert_eq!(mar[0].y(), 0.0, "起点边界行(分隔符)应含入");
        assert_eq!(mar[3].y(), 236.0);

        // 对照:相交语义对同区间会把 y=400 的邻桶分隔符也带上(尾部 <=)——
        // 这正是需要独立 bucket 查询而非复用 get_rows_by_y 的原因。
        let by_y = get_rows_by_y(&cache, 0.0, 400.0, Some(version)).unwrap();
        assert_eq!(by_y.len(), 5, "相交语义应多含邻桶首行");

        // 2 月桶 = [400, 540)(末桶 end 用 total_height):恰 2 行。
        let feb = get_bucket_rows(&cache, 400.0, 540.0, Some(version)).unwrap();
        assert_eq!(feb.len(), 2);
        assert_eq!(feb[0].y(), 400.0);
    }

    /// T16 方案B B0:版本守卫/空缓存 → None;有布局但区间外 → 空集(语义区分)。
    #[test]
    fn get_bucket_rows_guards_version_and_out_of_range() {
        let cache = new_layout_cache();
        assert!(
            get_bucket_rows(&cache, 0.0, 100.0, None).is_none(),
            "无布局 → None(命令层抛 LayoutNotReady)"
        );

        let version = store_layout(&cache, date_layout(), 540.0, String::new());
        assert!(
            get_bucket_rows(&cache, 0.0, 100.0, Some(version + 1)).is_none(),
            "版本不符 → None"
        );
        assert!(
            get_bucket_rows(&cache, 1000.0, 2000.0, Some(version))
                .unwrap()
                .is_empty(),
            "区间在所有行之后 → 空集而非 None"
        );
    }

    /// S3：store_layout 的扁平索引物化不受瘦行影响（flat 序、总数、id 索引仍正确）。
    #[test]
    fn store_layout_materializes_flat_indices_for_slim_rows() {
        let cache = new_layout_cache();
        store_layout(&cache, sample_layout(), 240.0, String::new());
        let guard = cache.read().unwrap();
        let data = guard.as_ref().unwrap();
        assert_eq!(data.order.flat_ids, vec![10, 11, 12]);
        assert_eq!(data.total_items, 3);
        assert_eq!(data.order.id_to_flat.get(11), Some(1));
        assert_eq!(data.flat_rowcol[1], (1, 1), "id 11 应在第 1 行第 1 列");
    }

    #[test]
    fn geometry_reflow_preserves_flat_order_and_updates_item_position() {
        let cache = new_layout_cache();
        let source = Arc::new(crate::layout::items_cache::new_items_cache());
        let first = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "wide".into(),
                None,
                Some(source.clone()),
                "filename:asc".into(),
            ),
        );
        let original_order = cache.read().unwrap().as_ref().unwrap().order.clone();
        let ids = get_view_ids(&cache, None).unwrap();
        let rows = vec![
            LayoutRow::Normal {
                y: 0.0,
                height: 100.0,
                items: vec![mk_item(10)],
            },
            LayoutRow::Normal {
                y: 100.0,
                height: 100.0,
                items: vec![mk_item(11), mk_item(12)],
            },
        ];
        let second = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                rows,
                200.0,
                "narrow".into(),
                None,
                Some(source),
                "filename:asc".into(),
            ),
        );
        assert_ne!(first.layout_version, second.layout_version);
        assert_eq!(first.order_version, second.order_version);
        assert!(
            Arc::ptr_eq(
                &original_order,
                &cache.read().unwrap().as_ref().unwrap().order
            ),
            "几何重排不重新物化全集和 ID 索引"
        );
        assert_eq!(
            get_view_ids(&cache, Some(first.order_version)).unwrap(),
            ids
        );
        assert_eq!(
            get_item_y_by_id(&cache, 11, second.layout_version),
            Some(Some(100.0))
        );
        assert_eq!(
            get_item_y_by_id(&cache, 11, first.layout_version),
            None,
            "几何换代后旧版本不得再定位"
        );
        assert_eq!(
            get_item_y_by_id(&cache, 999, second.layout_version),
            Some(None),
            "版本一致但项不在本布局 → Some(None)"
        );
        assert_eq!(
            cache.read().unwrap().as_ref().unwrap().flat_rowcol[1],
            (1, 0)
        );
    }

    #[test]
    fn changed_source_or_order_key_cannot_reuse_order_identity() {
        let cache = new_layout_cache();
        let source = Arc::new(crate::layout::items_cache::new_items_cache());
        let first = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "a".into(),
                None,
                Some(source.clone()),
                "asc".into(),
            ),
        );
        let second = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "b".into(),
                None,
                Some(source),
                "desc".into(),
            ),
        );
        assert_ne!(first.order_version, second.order_version);
        assert!(get_view_ids(&cache, Some(first.order_version)).is_none());
        let new_source = Arc::new(crate::layout::items_cache::new_items_cache());
        let third = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "c".into(),
                None,
                Some(new_source),
                "desc".into(),
            ),
        );
        assert_ne!(
            second.order_version, third.order_version,
            "不同 SQL/语义快照不能靠相同排序键复用"
        );
    }

    /// T5：普通邻接只在同一 orderVersion 的普通布局里按 O(1) 索引找一步邻居。
    /// 真实首尾边界（OutOfBounds）与「当前项不在本集合」（ViewStale）必须区分开——
    /// 后者不得被当作正常无结果，也不得回退别的集合。
    #[test]
    fn normal_adjacent_requires_order_version_and_distinguishes_boundary() {
        let cache = new_layout_cache();
        assert_eq!(
            get_adjacent_item(&cache, 10, 1, 1),
            AdjacentLookup::LayoutNotReady
        );

        store_layout(&cache, sample_layout(), 240.0, String::new());
        let order_version = current_order_version(&cache).unwrap();
        assert_eq!(
            get_adjacent_item(&cache, 10, 1, order_version),
            AdjacentLookup::Found {
                id: 11,
                index: 1,
                total_count: 3,
            }
        );
        assert_eq!(
            get_adjacent_item(&cache, 11, -1, order_version),
            AdjacentLookup::Found {
                id: 10,
                index: 0,
                total_count: 3,
            }
        );
        assert_eq!(
            get_adjacent_item(&cache, 12, 1, order_version),
            AdjacentLookup::OutOfBounds,
            "越过末尾是正常无结果"
        );
        assert_eq!(
            get_adjacent_item(&cache, 10, -1, order_version),
            AdjacentLookup::OutOfBounds,
            "越过开头是正常无结果"
        );
        assert_eq!(
            get_adjacent_item(&cache, 999, 1, order_version),
            AdjacentLookup::CurrentMissing,
            "当前项不在本集合不得冒充正常边界"
        );
        assert_eq!(
            get_adjacent_item(&cache, 10, 1, order_version + 99),
            AdjacentLookup::ViewStale,
            "旧 orderVersion 必须拒绝"
        );
    }

    /// T5：同一 currentId 在不同成员顺序下邻居不同；换代后旧 orderVersion 失效，
    /// 新版本给出新邻居（不得沿用旧序缓存）。
    #[test]
    fn same_current_id_yields_different_neighbor_per_order_version() {
        let cache = new_layout_cache();
        let source = Arc::new(crate::layout::items_cache::new_items_cache());
        let asc = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "a".into(),
                None,
                Some(source.clone()),
                "asc".into(),
            ),
        );
        // 反转成员顺序（不同排序键 → 新 orderVersion）。
        let reversed_rows = vec![
            LayoutRow::Normal {
                y: 0.0,
                height: 100.0,
                items: vec![mk_item(12)],
            },
            LayoutRow::Normal {
                y: 100.0,
                height: 100.0,
                items: vec![mk_item(11), mk_item(10)],
            },
        ];
        let desc = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                reversed_rows,
                200.0,
                "b".into(),
                None,
                Some(source),
                "desc".into(),
            ),
        );
        assert_ne!(asc.order_version, desc.order_version);
        assert_eq!(
            get_adjacent_item(&cache, 11, 1, asc.order_version),
            AdjacentLookup::ViewStale,
            "换代后旧序不得再产出邻居"
        );
        assert_eq!(
            get_adjacent_item(&cache, 11, 1, desc.order_version),
            AdjacentLookup::Found {
                id: 10,
                index: 2,
                total_count: 3,
            },
            "同 currentId 在新顺序下邻居不同"
        );
    }

    /// T5：纯几何重排沿用同一 orderVersion，导航保持有效。
    #[test]
    fn geometry_reflow_keeps_normal_adjacent_usable() {
        let cache = new_layout_cache();
        let source = Arc::new(crate::layout::items_cache::new_items_cache());
        let first = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                sample_layout(),
                240.0,
                "wide".into(),
                None,
                Some(source.clone()),
                "filename:asc".into(),
            ),
        );
        let reflowed_rows = vec![
            LayoutRow::Normal {
                y: 0.0,
                height: 100.0,
                items: vec![mk_item(10)],
            },
            LayoutRow::Normal {
                y: 100.0,
                height: 100.0,
                items: vec![mk_item(11), mk_item(12)],
            },
        ];
        let second = publish_layout(
            &cache,
            prepare_layout(
                &cache,
                reflowed_rows,
                200.0,
                "narrow".into(),
                None,
                Some(source),
                "filename:asc".into(),
            ),
        );
        assert_ne!(first.layout_version, second.layout_version);
        assert_eq!(first.order_version, second.order_version);
        assert_eq!(
            get_adjacent_item(&cache, 11, 1, first.order_version),
            AdjacentLookup::Found {
                id: 12,
                index: 2,
                total_count: 3,
            },
            "几何重排沿用 orderVersion，导航仍可用"
        );
    }

    /// T5：镜头布局即使 orderVersion 相同也不能冒充普通邻接序（反之亦然）。
    #[test]
    fn lens_layout_cannot_serve_normal_adjacent() {
        let cache = new_layout_cache();
        store_layout_with_lens(
            &cache,
            sample_layout(),
            240.0,
            String::new(),
            Some(Arc::new(crate::layout::lens::LensProjection::new())),
        );
        let order_version = current_order_version(&cache).unwrap();
        assert_eq!(
            get_adjacent_item(&cache, 10, 1, order_version),
            AdjacentLookup::ViewStale,
            "镜头序不得作为普通邻接序"
        );
    }

    #[test]
    fn lens_adjacent_lookup_requires_lens_version_and_current_item() {
        let cache = new_layout_cache();
        let normal_version = store_layout(&cache, sample_layout(), 240.0, String::new());
        assert_eq!(
            get_lens_adjacent_item(&cache, 10, 1, normal_version),
            AdjacentLookup::ViewStale,
            "普通布局不能作为镜头导航序"
        );

        let lens_version = store_layout_with_lens(
            &cache,
            sample_layout(),
            240.0,
            String::new(),
            Some(Arc::new(crate::layout::lens::LensProjection::new())),
        );
        assert_eq!(
            get_lens_adjacent_item(&cache, 10, 1, lens_version - 1),
            AdjacentLookup::ViewStale
        );
        assert_eq!(
            get_lens_adjacent_item(&cache, 999, 1, lens_version),
            AdjacentLookup::CurrentMissing,
            "当前项不在镜头缓存时不能把它误当作正常边界"
        );
        assert_eq!(
            get_lens_adjacent_item(&cache, 11, 1, lens_version),
            AdjacentLookup::Found {
                id: 12,
                index: 2,
                total_count: 3,
            }
        );
        assert_eq!(
            get_lens_adjacent_item(&cache, 12, 1, lens_version),
            AdjacentLookup::OutOfBounds,
            "只有目标越过首尾才是正常无结果"
        );
    }

    /// T5：镜头邻接的详情 await 后复验身份——普通布局或无布局都不得让迟到镜头详情落地。
    #[test]
    fn lens_layout_version_identity_rejects_normal_layout() {
        let cache = new_layout_cache();
        assert_eq!(
            current_lens_layout_version(&cache),
            None,
            "无布局 → 无镜头身份"
        );

        store_layout(&cache, sample_layout(), 240.0, String::new());
        assert_eq!(
            current_lens_layout_version(&cache),
            None,
            "普通布局不得冒充镜头身份"
        );

        let lens_version = store_layout_with_lens(
            &cache,
            sample_layout(),
            240.0,
            String::new(),
            Some(Arc::new(crate::layout::lens::LensProjection::new())),
        );
        assert_eq!(current_lens_layout_version(&cache), Some(lens_version));
        // 几何换代（重新发布镜头布局）→ 身份随 layoutVersion 改变。
        let newer = store_layout_with_lens(
            &cache,
            sample_layout(),
            240.0,
            String::new(),
            Some(Arc::new(crate::layout::lens::LensProjection::new())),
        );
        assert_eq!(current_lens_layout_version(&cache), Some(newer));
        assert_ne!(lens_version, newer);
    }

    /// S3.3：id 直址索引——密集走 Dense、稀疏退 Sparse，两形态查询行为等价。
    #[test]
    fn id_to_flat_dense_and_sparse_equivalent() {
        // 密集:ids 10..=12(max 12 < 3·4+1024)→ Dense;未知/负 id → None。
        let cache = new_layout_cache();
        store_layout(&cache, sample_layout(), 240.0, String::new());
        {
            let guard = cache.read().unwrap();
            let data = guard.as_ref().unwrap();
            assert!(
                matches!(data.order.id_to_flat, IdToFlat::Dense(_)),
                "紧凑 id 域应走直址"
            );
            assert_eq!(data.order.id_to_flat.get(10), Some(0));
            assert_eq!(data.order.id_to_flat.get(999), None);
            assert_eq!(data.order.id_to_flat.get(-1), None);
        }
        // 稀疏:单项 id=1_000_000 ≫ 4N+1024 → Sparse,行为等价。
        let rows = vec![LayoutRow::Normal {
            y: 0.0,
            height: 100.0,
            items: vec![mk_item(1_000_000)],
        }];
        store_layout(&cache, rows, 100.0, String::new());
        let guard = cache.read().unwrap();
        let data = guard.as_ref().unwrap();
        assert!(
            matches!(data.order.id_to_flat, IdToFlat::Sparse(_)),
            "稀疏 id 域应退哈希表"
        );
        assert_eq!(data.order.id_to_flat.get(1_000_000), Some(0));
        assert_eq!(data.order.id_to_flat.get(10), None);
    }

    /// S3.1：幂等去重探针——键相等才复用摘要；空键/键不等/空缓存皆不命中。
    #[test]
    fn dedup_summary_requires_exact_gen_key_match() {
        let cache = new_layout_cache();
        assert!(dedup_summary(&cache, "k1").is_none(), "空缓存不命中");

        let version = store_layout(&cache, date_layout(), 540.0, "k1".to_string());
        let s = dedup_summary(&cache, "k1").expect("同键应命中");
        assert_eq!(s.layout_version, version, "复用现行代——版本不换");
        assert_eq!(s.total_rows, 6);
        assert_eq!(
            s.month_buckets.len(),
            2,
            "摘要与 get_summary 同构（月桶照常）"
        );
        assert!(dedup_summary(&cache, "k2").is_none(), "键不等不命中");

        // 空键存入（测试便利路径）→ 探针即便也传空键，仍不命中（未知输入不参与去重）。
        store_layout(&cache, sample_layout(), 240.0, String::new());
        assert!(dedup_summary(&cache, "").is_none(), "空键永不命中");
    }

    /// 并发回归(审查 R0-3):版本号在写锁内递增后,「版本单调序 == 写入序」——
    /// 缓存最终存留的 layout_version 必等于本组线程拿到的最大版本号。
    /// (修复前:锁外取号可发生「后取号者先写、先取号者后写」,缓存留下小版本号旧行集,
    /// 而调用方已握有大版本号 → get_layout_rows 恒不匹配。)
    /// 注:计数器是进程级全局,其它并行测试也会递增它,故只断言相对性质、不断言绝对值。
    #[test]
    fn store_layout_version_matches_last_writer_under_concurrency() {
        use std::sync::Arc;

        let cache = Arc::new(new_layout_cache());
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || {
                    store_layout(&cache, sample_layout(), 240.0, String::new())
                })
            })
            .collect();
        let returned: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let cached_version = cache.read().unwrap().as_ref().unwrap().layout_version;
        let max_returned = *returned.iter().max().unwrap();
        assert_eq!(
            cached_version, max_returned,
            "cache must hold the version of the last writer (largest handed-out version)"
        );
    }
}
