//! S1 视图取数缓存（Part2 重排提速，2026-07-04）。
//!
//! 病根：compute_layout 每次触发都全量重跑「1M 行 SQL → 布局 → 物化」三段 O(N) 流水线，
//! 而滑块/窗宽/布局模式/分组轴这些**几何交互根本不改变视图集合**（WHERE 子句不变）。
//! 本缓存把「取数」从「几何」中拆出：
//!
//! - 命中键 = `filter_key`（MediaFilter 的 canonical JSON）+ `data_version`（AppState 全局
//!   数据版本，任何成员/几何/顺序写路径 bump）+ 序形态匹配（见 [`CachedOrder`]）。
//! - `sort_within = datetime`（默认）家族存**基准序**（`sort_datetime DESC, id DESC`，
//!   经 `query_layout_items_canonical` 免 JOIN 取回）；date/none/folder 三轴与 asc/desc
//!   方向全部由 [`derive_order`] 内存派生 —— 轴切换从「5s 级 SQL 字符串排序」变为亚秒内存排序。
//! - **双键统一缓存（D-015）+ B-file-iii**：filename 轴同样内存派生 —— datetime 基准经惰性
//!   `filename_rank` 跨键派 filename，filename 基准（[`CachedOrder::CanonicalFilename`]）以下标为位次；
//!   **none/folder/date 三种分组的 filename 序皆可派生**（date+filename 用 UTC 日桶 `div_euclid`，
//!   见 [`can_derive_axis`]），单槽缓存服务任意 group×sort、根治换轴 thrash。
//! - similarity（ai_search）按 SQL 序原样缓存（`reusable=false`，随每次搜索整表重写）。
//! - ai_search 视图**不缓存**（ai_search_results 随每次搜索整表重写）。
//!
//! **序等价契约（刚性）**：`derive_order` 的输出必须与 `push_query_body` 的 SQL ORDER 逐项
//! 等价 —— `get_view_ids`（flat_ids）与 `view_to_sql`（SelectAll 解析）分别源于两条路径，
//! 错位即选区漂移。等价性由 queries.rs 的对拍测试锁定。
//!
//! **锁纪律**：本缓存与 layout_cache 是两把独立 RwLock，任何路径不得同时持有两把
//! （compute_layout 在 items 读锁内跑布局，出锁后才 store_layout）。
//!
//! **S3（几何/载荷分离）后的双重身份**：本缓存同时是**布局行的载荷源**——布局行仅存
//! id + 几何，get_layout_rows 系命令出口经 [`hydrate_rows`] 从这里取载荷拼装线上行。
//! 因此「视图敏感写」不再整体置 None（会饿死出口拼装），而是降级 `reusable = false`：
//! compute 端视同 MISS 重查换代，本快照继续服务旧布局的取行（值照常 patch，视觉即时）。

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use rustc_hash::FxHashMap;

use crate::db::models::{DirLabel, LayoutItem, MediaFilter, ThumbResult};
use crate::layout::geometry::{hydrate_item, placeholder_item, HydratedRow, LayoutRow};
use crate::thumbnail::serve::{serve_need_px, ServeRequest, ThumbServe};

/// 缓存内容的序形态（**双键统一缓存**：两种基准都同时服务 datetime 与 filename 两轴——
/// 每个 `LayoutItem` 都自带 `sort_datetime`，故任一基准都能免费派生 datetime 序；filename 序需
/// 整数位次，filename 基准用下标、datetime 基准用惰性 [`ItemsCacheData::filename_rank`]。变体名
/// 只标记 **items 的物理存储序**，不再限定它只能服务哪一轴）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CachedOrder {
    /// datetime 基准序（`sort_datetime DESC, id DESC`）——items 按时间物理排列。
    /// datetime 轴任意 group/方向由 [`derive_order`] 内存派生；filename 轴（none/folder）经惰性
    /// [`ItemsCacheData::filename_rank`] 跨键实排派生（首次切 filename 补一次 id-only 查询）。
    Canonical,
    /// filename 基准序（`file_name COLLATE NATURAL_CMP ASC, id ASC`，B-file-i）——items 存自然序基准，
    /// **下标即 `filename_rank`**；filename 轴（none/folder/**date**，任意方向）在整数下标上内存派生。
    /// datetime 轴（任意 group）则取 items 各自的 `sort_datetime` 跨键实排派生（免额外查询）。
    /// date+filename（B-file-iii/A′）用 **UTC 日桶** `sort_datetime.div_euclid(86400)` 分桶（每项自带），
    /// 与 run_layout 分组及 SQL（date+filename 已去 `'localtime'` 改 UTC 日界）逐值等价，无需 SQL 日期列。
    CanonicalFilename,
    /// 特殊排序（similarity 等无法内存派生的轴）：按 SQL ORDER 原样缓存，任一排序参数变化即 miss。
    /// 注：date+filename 自 B-file-iii/A′ 起改由 [`CanonicalFilename`]/[`Canonical`] 内存派生，不再落此变体。
    Sql {
        group_by: String,
        sort_within: String,
        sort_order: String,
    },
}

/// [`derive_order`] **全部实排分支**共用的排序置换 memo（items 下标序列）。
///
/// **覆盖范围（T2）**：folder（任意轴/方向）、date+filename（UTC 日桶主键）与跨键
/// none/date+datetime 三条实排路径共用这**唯一一个最近排列**槽——同一份 items 上，
/// 几何重排（滑块/窗宽/行高）不改变任何排序键，命中即免 O(N log N) 排序、仅 O(N) 还原引用序。
/// 不扩为多轴常驻缓存：只留最近一次排列，换轴/换向即替换（内存增量恒为一个 `Vec<u32>`）。
///
/// memo 键含 `group_by` 与 `sort_within`——双键统一缓存下同一份 items 会被
/// folder+datetime 与 folder+filename 两轴各派生一次，置换不同，键须区分轴（否则 datetime
/// 的置换会被误当 filename 复用）。
pub struct PermMemo {
    pub group_by: String,
    pub sort_within: String,
    pub sort_order: String,
    /// 派生序 → 基准序下标的置换。
    pub perm: Vec<u32>,
}

#[cfg(test)]
thread_local! {
    /// **仅测试**的置换 memo 命中计数（生产构建不编译本项，热路径零开销）。
    /// 定向验证用：证明「相同排序参数下的连续几何重排」确实走命中还原路径，而非重新排序——
    /// 这是对**排序是否真的省下**的直接判据，不是测试里镜像一遍实现逻辑。
    /// `thread_local`：并发测试各自独立计数，互不干扰。
    pub(crate) static PERM_MEMO_HITS: std::cell::Cell<u32> = std::cell::Cell::new(0);
}

/// 驻留的视图取数缓存体。
pub struct ItemsCacheData {
    /// MediaFilter 的 canonical JSON —— 视图集合签名（WHERE 子句的等价物）。
    pub filter_key: String,
    pub order: CachedOrder,
    /// 填充时的全局数据版本（`AppState::data_version`）：写路径 bump 后不再命中。
    pub data_version: u64,
    pub items: Vec<LayoutItem>,
    /// id → items 下标：thumb/favorite/rating/color 的 O(1) 就地 patch。
    /// S3.2：FxHashMap（同 cache.rs id_to_flat——1M 级整数键构建提速数倍）。
    pub id_to_idx: FxHashMap<i64, u32>,
    pub dir_labels: HashMap<i64, DirLabel>,
    /// dir_id → 前序 DFS 唯一序秩。键 = `(root_created_at, root_id, encode_tree_sort_key(rel_path),
    /// dir_id)`：root 序使不同扫描根整棵子树连续（不按相同 rel_path 跨根交错），rel_path 的 DFS
    /// 字节键使目录序严格等于文件树前序遍历，dir_id 稳定裁决退化并发。该顺序与 SQL
    /// `ORDER BY r.created_at, r.id, TREE_SORT_KEY(d.rel_path), d.id` 逐项一致（见 build_dir_rank）。
    pub dir_rank: HashMap<i64, u32>,
    /// 缓存 filter 本体：favorite/rating/color patch 时判断「该字段是否影响本视图成员」
    /// （如 favoritedOnly 视图下取消收藏 = 成员变化 → 整体失效而非 patch）。
    pub filter: MediaFilter,
    /// 是否可作为 compute_layout 的命中源（S3）。false = 仅作载荷源服务出口拼装：
    /// ①视图敏感写后（成员已变，须重查换代）②ai_search 视图（结果表随每次搜索整表重写）。
    pub reusable: bool,
    /// 宽高输入就地改变时递增；几何去重不能只看集合的数据代。
    pub geometry_revision: u64,
    /// 已测量项宽高比中位数的惰性缓存（S3.5）：只依赖项集、不依赖布局参数，justified
    /// 每次重排免 O(N) 重算。就地 patch（尺寸回填）有意不失效——中位数轻微漂移仅影响
    /// 0×0 占位项的形状，容差内；随缓存体整体换代自动重算。
    pub median_aspect: std::sync::OnceLock<f64>,
    /// 派生序的**唯一最近排列** memo（T2：folder / date+filename / 跨键 none 三条实排路径共用）：
    /// 同 (group_by, sort_within, sort_order) 的后续派生免排序、O(N) 还原。
    /// 就地 patch 均不触碰排序键 (dir_id, sort_datetime/filename_rank, id)，故 memo 只需随缓存体
    /// 整体换代（MISS 重建）自动失效，无需单独失效逻辑——data_version 换代即新 ItemsCacheData。
    pub perm_memo: Mutex<Option<PermMemo>>,
    /// filename 自然序位次的惰性缓存（双键统一缓存）：`filename_rank[i]` = `items[i]` 的**全局
    /// NATURAL_CMP 位次**（0 起）。仅 datetime 基准（[`CachedOrder::Canonical`]）需要——items 非
    /// filename 序，无从取位次；用户首次从 datetime 切到 filename 时，compute_layout 在 items 读锁外
    /// 补一次 id-only NATURAL_CMP 查询填入，此后 datetime↔filename 互切全命中（免整行重查）。
    /// [`CachedOrder::CanonicalFilename`] 基准 items 本按 filename 序存（下标即位次），恒不填充。
    /// `OnceLock`：一次性写、经 `&self` 在读锁下 `set`、`.get()` 读；竞态双填时后者 `set` 返 Err 忽略。
    pub filename_rank: std::sync::OnceLock<Vec<u32>>,
}

pub type ItemsCache = RwLock<Option<ItemsCacheData>>;

/// 单槽保存当前快照句柄；布局保留同一句柄，换集合时旧行不会读到新集合载荷。
pub type ItemsCacheSlot = RwLock<std::sync::Arc<ItemsCache>>;

/// 创建尚无载荷的当前快照槽。
pub fn new_items_cache_slot() -> ItemsCacheSlot {
    RwLock::new(std::sync::Arc::new(new_items_cache()))
}

pub fn new_items_cache() -> ItemsCache {
    RwLock::new(None)
}

/// 存入新缓存体（整体替换）。
pub fn store_items(cache: &ItemsCache, data: ItemsCacheData) {
    *cache.write().unwrap_or_else(|e| e.into_inner()) = Some(data);
}

/// 显式整体清空（硬失效）。S3 后本缓存是布局行的载荷源——仅用于布局缓存同步清空的场景
/// （如 clear_database），否则会让出口拼装全部退化为占位行项；常规失效走 data_version
/// bump，视图敏感降级走 `reusable = false`（见 patch_or_degrade）。
pub fn invalidate(cache: &ItemsCache) {
    *cache.write().unwrap_or_else(|e| e.into_inner()) = None;
}

// ── B-file-iii：全局 filter-invariant filename rank ───────────────────────────────

/// 全库 filename 自然序位次（**filter-invariant**，B-file-iii）。id → 该 id 在「全部默认可见媒体项
/// 按 `file_name COLLATE NATURAL_CMP ASC` 排序」中的位次。
///
/// **filter 无关性（正确性论证）**：`natural_cmp` 是全序关系；对任意筛选子集 S，「S 按 filename 排序」
/// = 「S 按各项全局位次整数排序」——限制全序到子集保持相对序。∴ **一份全局位次服务所有 filter 子集**，
/// 无需按 filter 重算。这把 B-file-i「每 filter 一份 rank（每次筛选付一次 546ms NATURAL_CMP 查询）」
/// 降为「全库一份、开机建一次」。
///
/// **基集 = 默认可见集**（`MediaFilter::default()` = `is_deleted=0 AND companion_of IS NULL`）。绝大多数
/// filter（视频/收藏/星级/目录）是其**子集**，全覆盖；非默认基集（回收站 `is_deleted=1` / 含隐藏项）的
/// 项不在 map 内 → 消费方（`AppState::try_global_filename_ranks`）覆盖校验失败即退化 B-file-i（§3.5，
/// 绝不以坏序命中）。稀疏映射（子集只取部分 id）仍保序，故无需稠密 `[0,N)`。
pub struct GlobalFilenameRank {
    /// 构建时的全局数据代（`AppState::data_version`）；与当前不符即 stale，须后台重建。
    pub data_version: u64,
    /// id → 全局 NATURAL_CMP 位次（0 起）。FxHashMap：543k 整数键构建/查询快（同 `id_to_idx`）。
    pub id_to_rank: FxHashMap<i64, u32>,
}

impl GlobalFilenameRank {
    /// 为给定 items 构建**与之平行**的 rank 数组（消费入口，`AppState::try_global_filename_ranks`
    /// 在锁内委托此方法）。**两关全过才返 Some**：① `data_version` 相符（非 stale）；② **所有** item.id
    /// 在 map 内（基集覆盖——非默认基集如回收站的项不在其中）。任一不满足返 `None` → 调用方退化
    /// B-file-i（绝不以 `u32::MAX` 兜坏序）。稀疏子集（filter 只取部分 id）仍保序：自然序是全序，
    /// 限制到子集保持相对序。
    pub fn ranks_for(&self, items: &[LayoutItem], data_version: u64) -> Option<Vec<u32>> {
        if self.data_version != data_version {
            return None;
        }
        let mut ranks = Vec::with_capacity(items.len());
        for it in items {
            // 任一 id 缺失（非默认基集 / 入库-重建竞态）→ 整体退化，不塞末尾坏序。
            ranks.push(*self.id_to_rank.get(&it.id)?);
        }
        Some(ranks)
    }
}

/// 全局 rank 的驻留槽（`AppState` 持有）。开机后台预建、`data_version` 变更后台重建；
/// 读多写少（每 data_version 写一次），`RwLock` 合适。
pub type GlobalRankCell = RwLock<Option<GlobalFilenameRank>>;

pub fn new_global_rank_cell() -> GlobalRankCell {
    RwLock::new(None)
}

/// 构建全局 filename rank（**须在 `spawn_blocking` 内调用**：一次全库 id-only NATURAL_CMP 查询，
/// 真机 543k 约 546ms release / 4.6s dev）。取数经生产同一函数 `query_item_ids_filename_order` +
/// **默认 filter**（全库默认可见集），枚举下标即位次。`data_version` 由调用方在查询前采样传入，
/// 供消费方校验 stale。
pub fn build_global_filename_rank(
    conn: &rusqlite::Connection,
    data_version: u64,
) -> crate::error::Result<GlobalFilenameRank> {
    let ids = crate::db::queries::query_item_ids_filename_order(conn, &MediaFilter::default())?;
    let mut id_to_rank: FxHashMap<i64, u32> =
        FxHashMap::with_capacity_and_hasher(ids.len(), Default::default());
    for (rank, id) in ids.into_iter().enumerate() {
        id_to_rank.insert(id, rank as u32);
    }
    Ok(GlobalFilenameRank {
        data_version,
        id_to_rank,
    })
}

/// 双键统一缓存的**轴可派生性**单一事实源：canonical/filename 任一基准能否内存派生给定
/// `(group_by, sort_within)`。datetime 轴任意 group 皆可（每项自带 sort_datetime）；filename 轴
/// **none/folder/date 皆可**（B-file-iii）：date+filename 用 **UTC 日桶** `sort_datetime.div_euclid(86400)`
/// 内存分桶——与 run_layout 分组（同 `div_euclid`）及 SQL `push_order_by`（date+filename 已去
/// `'localtime'` 改 UTC 日界，见 §D-018 A′）**构造性逐值等价**，故无需 SQL 日期列、无对齐风险。
/// 时区前提坐实：`sort_datetime` = EXIF 墙钟当 UTC 存（`metadata.rs::parse_exif_datetime`），UTC 桶
/// 才与显示标签/月桶一致；原 SQL 的 `localtime` 是双重时区偏移 bug，本轮顺带修正。
/// `is_hit_valid` 与 compute_layout 的 HIT `order_ok` 必须共用本函数——改一处即改两处，杜绝判据漂移。
pub fn can_derive_axis(group_by: &str, sort_within: &str) -> bool {
    match sort_within {
        "datetime" => true,
        "filename" => group_by == "none" || group_by == "folder" || group_by == "date",
        _ => false,
    }
}

/// S3.1 幂等去重预检：镜像 compute_layout 的 HIT 守卫（reusable + 序形态 + 数据代 +
/// 过滤器键），只回答「快照当前是否可作命中源」，不派生序、不触碰 items。
/// 与 compute_layout 内的 HIT 判定必须保持同一判据——改一处必改另一处。
pub fn is_hit_valid(
    cache: &ItemsCache,
    filter_key: &str,
    data_version: u64,
    group_by: &str,
    sort_within: &str,
    sort_order: &str,
) -> bool {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_ref() else {
        return false;
    };
    let order_ok = match &data.order {
        // 双键统一缓存：两种基准判据相同——datetime 轴任意 group 可派生（items 皆带 sort_datetime）；
        // filename 轴 none/folder/date 皆可派生（date+filename 用 UTC 日桶 div_euclid，B-file-iii/A′）。
        // 唯一实现差异（datetime 基准派 filename 需惰性 filename_rank）由 compute_layout 补齐，
        // 不影响本纯预检的判据——此处只回答「该基准能否服务该轴」。
        CachedOrder::Canonical | CachedOrder::CanonicalFilename => {
            can_derive_axis(group_by, sort_within)
        }
        CachedOrder::Sql {
            group_by: g,
            sort_within: s,
            sort_order: o,
        } => g.as_str() == group_by && s.as_str() == sort_within && o.as_str() == sort_order,
    };
    data.reusable && order_ok && data.data_version == data_version && data.filter_key == filter_key
}

/// 构建 id → 下标索引（O(N) 一次，patch O(1)）。S3.2：预留容量 + FxHash——
/// MISS 换代路径同享构建提速。
pub fn build_id_index(items: &[LayoutItem]) -> FxHashMap<i64, u32> {
    let mut m: FxHashMap<i64, u32> =
        FxHashMap::with_capacity_and_hasher(items.len(), Default::default());
    for (i, it) in items.iter().enumerate() {
        m.insert(it.id, i as u32);
    }
    m
}

/// 构建 dir_rank（语义见 [`ItemsCacheData::dir_rank`] 字段文档）。目录量级远小于媒体项
/// （D≈10^4，非百万媒体级），故在此把完整目录序压成唯一 u32 rank，可保持百万项排序键仍为
/// 紧凑元组，避免为每个媒体项再携带目录排序字段。
///
/// 排序键 = `(root_created_at, root_id, encode_tree_sort_key(rel_path), dir_id)`，即前序 DFS：
/// root 序（created_at, id）最高位使不同扫描根整棵子树连续，其次是 rel_path 的 DFS 字节键，
/// 末位 dir_id 稳定裁决同 (root, rel_path) 的退化并发。该键与 SQL `push_order_by` 的 folder
/// 目录序前缀 `r.created_at, r.id, TREE_SORT_KEY(d.rel_path), d.id` **逐位同构**（Vec<u8> 的
/// Ord = 字节字典序 = SQLite BLOB memcmp），是内存/SQL 刚性等价契约成立的根据。
///
/// 即时算键的成本落在**目录数**而非媒体数上：一次装饰扫描算 D 次键（<10ms），媒体排序仍用
/// 压好的 u32 rank——「per-media 比较器反复拆路径」的顾虑对本处不成立。
pub fn build_dir_rank(dir_labels: &HashMap<i64, DirLabel>) -> HashMap<i64, u32> {
    use crate::utils::path::encode_tree_sort_key;
    let mut dirs: Vec<(i64, i64, Vec<u8>, i64)> = dir_labels
        .iter()
        .map(|(id, label)| {
            (
                label.root_created_at,
                label.root_id,
                encode_tree_sort_key(&label.rel_path),
                *id,
            )
        })
        .collect();
    // 元组默认 Ord = 按字段序字典比较，恰为目标目录序 (root_created_at, root_id, key, dir_id)。
    dirs.sort_unstable();
    dirs.into_iter()
        .enumerate()
        .map(|(rank, (_, _, _, id))| (id, rank as u32))
        .collect()
}

/// 从**基准序缓存**派生任意 group/轴/方向的引用序（**双键统一缓存**）。请求轴由 `sort_within`
/// 传入，不再从 `data.order` 反推——同一份缓存既能派 datetime 也能派 filename：
/// - **datetime 请求**：次键取 `it.sort_datetime`（每项自带，任一基准免费）。
/// - **filename 请求**：次键取「filename 位次」——filename 基准（items 按自然序存）用**下标 `i`**；
///   datetime 基准用惰性 [`ItemsCacheData::filename_rank`]`[i]`（首次切 filename 由 compute_layout 补）。
///
/// **date+filename（B-file-iii / D-018 A′）**：独立分支，`(UTC 日桶 div_euclid(86400), filename 位次,
///   id)` 全键按方向实排——同键反转快捷只输出纯 filename 序、不含日分桶，对 date 轴无效，故单列。
///
/// **同键 vs 跨键**（`请求轴 == 基准物理序` 与否）决定 none/date+datetime 能否走反转快捷：
/// - **同键 none/date+datetime**：items 已按请求键物理排列 → 恒等/整体反转（请求方向 == 基准方向则恒等）。
///   datetime 基准 DESC：desc=恒等/asc=反转；filename 基准 ASC：asc=恒等/desc=反转。与旧行为逐值等价。
/// - **跨键 none/date+datetime**：items 未按请求键排列 → 必须实排 `(次键, id)`（不能反转）。此分支只会是
///   none（filename 或 datetime）或 date+datetime（epoch_day 是 sort_datetime 的单调函数，按
///   `(sort_datetime,id)` 平铺排 == 分桶排的行序，分隔符下游算）——date+filename 已在上面独立分支处理。
/// - **folder（同键/跨键统一）**：`(前序 DFS 目录唯一秩 ASC, 次键, id)` 全键排序 —— 目录序恒 ASC、
///   方向仅作用于次键/id，与 SQL `ORDER BY <dir_order>, {sort_within} {dir}, m.id {dir}` 逐项一致
///   （刚性对拍锁定）。folder 恒实排，故跨键无需特殊处理，仅次键来源不同。
///
/// 注：缓存查询免 directories JOIN，故孤儿 dir_id（目录行已删但媒体残留，FK 级联下不应
/// 出现）在此排 u32::MAX 末尾而非像 INNER JOIN 那样被剔除 —— 防御性差异，正常库无此形态。
/// 同理 datetime 基准缺 filename_rank（惰性未及填的防御路径）时该项次键取 i64::MAX 排末尾，不 panic。
///
/// 性能（S1.1，2026-07-04 真机回归修复）：folder 轴走「装饰-排序-还原 + 置换 memo」。
/// 直接对 `&LayoutItem` 排序时每次比较 = 2 次查秩 + 2 次对 ~200B 大结构体的随机访存，
/// 1M 项 ≈ 2000 万次比较实测 5-7s；排序键抽进紧凑连续元组后亚秒，置换存入
/// [`ItemsCacheData::perm_memo`]（键含 `sort_within`），同轴同向的后续交互（滑块/窗宽）免排序。
///
/// **T2：memo 覆盖全部实排分支**。除 folder 外，date+filename（UTC 日桶主键）与跨键
/// none/date+datetime（CanonicalFilename 基准派 datetime 等）同样在实排后写入**同一个**
/// 最近排列槽，命中即 O(N) 还原引用序。故「相同数据体 + 相同排序参数 + 只改几何」的连续
/// 重排（宽度/行高/布局模式）一次排序后零重新排序。同键 identity/reverse 快路径本就是 O(N)
/// 线性遍历，不占用 memo。**只保留一个最近排列**，不引入多视图常驻缓存。
pub fn derive_order<'a>(
    data: &'a ItemsCacheData,
    group_by: &str,
    sort_within: &str,
    sort_order: &str,
) -> Vec<&'a LayoutItem> {
    let asc = sort_order == "asc";
    let want_filename = sort_within == "filename";
    // 基准物理序：CanonicalFilename 的 items 按 filename 自然序存（下标即位次），Canonical 按 datetime。
    let baseline_is_filename = matches!(data.order, CachedOrder::CanonicalFilename);
    // 请求轴 == 基准物理序 → 同键（none/date 可走反转快捷）；否则跨键（须实排）。
    let same_key = want_filename == baseline_is_filename;
    // datetime 基准派 filename 时的位次数据源（惰性补，compute_layout 保证到此已就绪）。
    let fname_rank: Option<&[u32]> = if want_filename && !baseline_is_filename {
        data.filename_rank.get().map(|v| v.as_slice())
    } else {
        None
    };
    // 请求轴在下标 i 上的次键值：filename → 位次（下标 or filename_rank[i]）；datetime → sort_datetime。
    let secondary = |i: usize, it: &LayoutItem| -> i64 {
        if want_filename {
            if baseline_is_filename {
                i as i64
            } else {
                fname_rank.map(|r| r[i] as i64).unwrap_or(i64::MAX)
            }
        } else {
            it.sort_datetime
        }
    };
    // 是否走实排分支：folder 恒实排、date+filename 恒实排、none/date 跨键实排。
    // 其余（none/date 同键）是线性恒等/反转快捷，本就不需要排序，故不占 memo。
    let real_sort = group_by == "folder" || (group_by == "date" && want_filename) || !same_key;
    // 次键数据是否真就绪：datetime 基准派 filename 时位次是**惰性**填入的（OnceLock 在缓存体
    // 存活期间 set），缺位次则次键退化为 i64::MAX 的防御序。该退化序不能进 memo——否则位次
    // 填入后仍会命中早先的退化置换。生产路径由 HIT 守卫（rank_ready）与 MISS 预填保证此处恒就绪。
    let memo_ok = real_sort && (!want_filename || baseline_is_filename || fname_rank.is_some());
    // T2 共享置换 memo 命中（三条实排分支同一槽）：置换只由 (group_by, sort_within, sort_order)
    // 与基准序决定，而基准序 = 本缓存体自身，故键相符即免排序、按下标 O(N) 还原引用序。
    // 命中判定在锁内完成并就地物化引用（不 clone 置换）。
    if memo_ok {
        let memo = data.perm_memo.lock().unwrap();
        if let Some(m) = memo.as_ref() {
            if m.group_by == group_by && m.sort_within == sort_within && m.sort_order == sort_order
            {
                #[cfg(test)]
                PERM_MEMO_HITS.with(|c| c.set(c.get() + 1));
                return m.perm.iter().map(|&i| &data.items[i as usize]).collect();
            }
        }
    }
    match group_by {
        "folder" => {
            let rank = |it: &LayoutItem| -> u32 {
                it.dir_id
                    .and_then(|id| data.dir_rank.get(&id).copied())
                    .unwrap_or(u32::MAX)
            };
            // 装饰-排序-还原：一次线性扫描抽键（顺序访存，预取友好），在 32B 紧凑元组的
            // 连续数组上排序 —— 比较器内零哈希查找、零大结构体随机访存。
            let mut keys: Vec<(u32, i64, i64, u32)> = data
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| (rank(it), secondary(i, it), it.id, i as u32))
                .collect();
            keys.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0).then_with(|| {
                    let key = (a.1, a.2).cmp(&(b.1, b.2));
                    if asc {
                        key
                    } else {
                        key.reverse()
                    }
                })
            });
            let perm: Vec<u32> = keys.iter().map(|k| k.3).collect();
            store_perm(data, group_by, sort_within, sort_order, perm, memo_ok)
        }
        // date+filename（B-file-iii / D-018 A′ 全面 UTC 桶）：UTC 日桶主键 + filename 位次次键 +
        // id 末键，整键按方向。桶 = `sort_datetime.div_euclid(86400)`（每项自带，与 run_layout 分组
        // `group_mark` 同算式）；位次由 `secondary` 给出（CanonicalFilename 基准=下标 i、Canonical 基准=
        // filename_rank[i]）。与 SQL `ORDER BY date(m.sort_datetime,'unixepoch') {dir},
        // m.file_name COLLATE NATURAL_CMP {dir}, m.id {dir}` 逐值等价（date_expr 已去 'localtime' 改
        // UTC 日界，刚性对拍锁定）。**必须独立实排**：同键反转快捷（下面 `same_key` 分支）只会输出
        // 纯 filename 序、不含日分桶，对 date 轴无效；故此分支置于 folder 之后、none/date 通用分支之前。
        "date" if want_filename => {
            let mut keys: Vec<(i64, i64, i64, u32)> = data
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    (
                        it.sort_datetime.div_euclid(86400),
                        secondary(i, it),
                        it.id,
                        i as u32,
                    )
                })
                .collect();
            keys.sort_unstable_by(|a, b| {
                let key = (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2));
                if asc {
                    key
                } else {
                    key.reverse()
                }
            });
            // T2：与 folder/跨键分支共用同一最近排列 memo（几何重排免二次排序）。
            let perm: Vec<u32> = keys.iter().map(|k| k.3).collect();
            store_perm(data, group_by, sort_within, sort_order, perm, memo_ok)
        }
        // none/date 轴（datetime 任意方向、none+filename；date+filename 已在上面独立分支处理）。
        _ if same_key => {
            // 同键：items 已按请求键物理排列 → 恒等/整体反转。
            // datetime 基准 DESC（baseline_desc=true）：desc=恒等/asc=反转（与旧代码逐值等价）；
            // filename 基准 ASC（baseline_desc=false）：asc=恒等/desc=反转。
            let baseline_desc = !want_filename;
            let requested_desc = !asc;
            if requested_desc == baseline_desc {
                data.items.iter().collect()
            } else {
                data.items.iter().rev().collect()
            }
        }
        _ => {
            // 跨键：items 未按请求键排列 → 实排 (次键, id)。此处 group_by 恒为 none（filename/datetime）
            // 或 date+datetime（按 sort_datetime 平铺 == 分桶行序，分隔符下游算），故无分组键——
            // date+filename 已被上面独立分支拦截（需 UTC 日桶主键，不能只按次键平铺）。
            let mut keys: Vec<(i64, i64, u32)> = data
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| (secondary(i, it), it.id, i as u32))
                .collect();
            keys.sort_unstable_by(|a, b| {
                let key = (a.0, a.1).cmp(&(b.0, b.1));
                if asc {
                    key
                } else {
                    key.reverse()
                }
            });
            // T2：与 folder/date+filename 分支共用同一最近排列 memo（几何重排免二次排序）。
            let perm: Vec<u32> = keys.iter().map(|k| k.2).collect();
            store_perm(data, group_by, sort_within, sort_order, perm, memo_ok)
        }
    }
}

/// 实排结果写入**唯一最近排列** memo 并物化引用序（T2）。folder / date+filename / 跨键
/// none 三条实排分支共用，避免三处重复写锁与还原代码；置换留在 memo 内（不 clone）。
/// `store` 为 false（次键未就绪的防御性退化序）时只物化、不落 memo，避免退化序被后续命中。
fn store_perm<'a>(
    data: &'a ItemsCacheData,
    group_by: &str,
    sort_within: &str,
    sort_order: &str,
    perm: Vec<u32>,
    store: bool,
) -> Vec<&'a LayoutItem> {
    let refs: Vec<&LayoutItem> = perm.iter().map(|&i| &data.items[i as usize]).collect();
    if store {
        *data.perm_memo.lock().unwrap() = Some(PermMemo {
            group_by: group_by.to_string(),
            sort_within: sort_within.to_string(),
            sort_order: sort_order.to_string(),
            perm,
        });
    }
    refs
}

/// 出口拼装①(P1-5,载荷读锁内,**零 IO**):为可视行收集多档选档请求——只读载荷与算几何,
/// 磁盘探测/存在性校验交给调用方在阻塞线程内做(见 crate::thumbnail::serve::prepare_offloaded),
/// 应用阶段([`hydrate_rows`])再用同一上下文重写路径。
///
/// 守卫与 [`hydrate_item`] 的选档分支逐条同构(仅 `thumb_status == 1` 且有非空 `thumb_path`
/// 的项参与);两处判据若漂移,最坏是漏一次重写(应用阶段查表失配 → 保留 DB 路径)。存在性只在
/// serve 的 prepare 时刻确认,此后文件被删由前端 404 自愈兜底(见 thumbnail::serve 模块说明)。
pub fn collect_serve_requests(
    cache: &ItemsCache,
    rows: &[LayoutRow],
    dpr: f64,
) -> Vec<ServeRequest> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_ref() else {
        return Vec::new();
    };
    let mut requests = Vec::new();
    for row in rows {
        let LayoutRow::Normal { items, .. } = row else {
            continue;
        };
        for slot in items {
            let Some(&i) = data.id_to_idx.get(&slot.id) else {
                continue;
            };
            let Some(item) = data.items.get(i as usize) else {
                continue;
            };
            if item.thumb_status != 1 {
                continue;
            }
            let Some(db_path) = item.thumb_path.as_deref().filter(|p| !p.is_empty()) else {
                continue;
            };
            requests.push(ServeRequest {
                cache_key: item.cache_key,
                need_px: serve_need_px(slot.w, slot.h, dpr),
                db_path: db_path.to_string(),
            });
        }
    }
    requests
}

/// 出口拼装（S3 几何/载荷分离）：瘦布局行 → 线上行。逐 id 经 id_to_idx 取载荷；查无此 id
/// （布局与快照换代的竞态窗口/清库后未重算的瞬态）→ 占位行项（几何保留、载荷置空），
/// 下次布局换代自愈——不丢行不 panic，保虚拟滚动行形稳定。仅对可视区调用（10^2 级行项），
/// 读锁 + 载荷克隆成本无关紧要。
///
/// `serve`(多档源,2026-08-16 阶段 2;2026-09-12 P1-5):透传 hydrate_item 做按需选档;None = 不改写。
/// **P1-5 起本函数零 IO**:probe/存在性 stat 已由 `thumbnail::serve::prepare_offloaded` 在阻塞线程内
/// 完成,此处只查 [`ThumbServe::rewrite_path`] 的内存表(载荷读锁内不触盘,锁纪律见模块头)。
///
/// `lens`(重复镜头,2026-09-02 方案 §11.1):Some 时按 id 查表填逐项镜头字段
///（duplicateBucket/组序号/组内序号/成员数）并透传 separator 的 separatorKind；
/// None（普通画廊）= 全部镜头字段 None，线上 JSON 不含这些键（wire 不膨胀）。
pub fn hydrate_rows(
    cache: &ItemsCache,
    rows: Vec<LayoutRow>,
    serve: Option<&ThumbServe>,
    lens: Option<&crate::layout::lens::LensProjection>,
) -> Vec<HydratedRow> {
    let guard = cache.read().unwrap_or_else(|e| e.into_inner());
    let data = guard.as_ref();
    rows.into_iter()
        .map(|row| match row {
            LayoutRow::Separator {
                y,
                height,
                separator_label,
                group_id,
                separator_kind,
                lens_folder,
                lens_group,
                // epoch_day 不入逐行网格 IPC（HydratedRow）——scrubber 走 LayoutSummary.separators
                // 取该字段，网格行渲染无需时间坐标。
                ..
            } => HydratedRow::Separator {
                y,
                height,
                separator_label,
                group_id,
                // separatorKind 随常驻行透传（普通画廊 date/folder 恒 None，wire 不含键）。
                separator_kind,
                // folders 文件夹头增量（§7.1/§7.2/§11.2）：常驻 lens_folder struct 展开到
                // wire 平铺字段；普通画廊/镜头组头 = None，线上 JSON 不含这些键。
                parent_group_id: lens_folder.as_ref().map(|f| f.parent_group_id.clone()),
                // 簇头文本由前端按 locale 生成；路径仍由 separator_label 原样透传。
                parent_group_start: lens_folder.as_ref().map(|f| f.parent_group_start),
                parent_group_ordinal: lens_folder.as_ref().map(|f| f.parent_group_ordinal),
                parent_group_folder_count: lens_folder
                    .as_ref()
                    .map(|f| f.parent_group_folder_count),
                parent_group_group_count: lens_folder.as_ref().map(|f| f.parent_group_group_count),
                duplicate_count: lens_folder.as_ref().map(|f| f.duplicate_count),
                unconfirmed_count: lens_folder.as_ref().map(|f| f.unconfirmed_count),
                unique_count: lens_folder.as_ref().map(|f| f.unique_count),
                unique_hidden: lens_folder.as_ref().map(|f| f.unique_hidden),
                duplicate_group_ordinal: lens_group.as_ref().map(|g| g.ordinal),
                duplicate_member_count: lens_group.as_ref().map(|g| g.member_count),
                duplicate_folder_count: lens_group.as_ref().map(|g| g.folder_count),
                duplicate_unit_size: lens_group.as_ref().map(|g| g.unit_size),
            },
            LayoutRow::Normal { y, height, items } => HydratedRow::Normal {
                y,
                height,
                items: items
                    .iter()
                    .map(|slot| {
                        let mut wire = data
                            .and_then(|d| {
                                d.id_to_idx
                                    .get(&slot.id)
                                    .and_then(|&i| d.items.get(i as usize))
                            })
                            .map(|it| hydrate_item(it, slot, serve))
                            .unwrap_or_else(|| placeholder_item(slot));
                        // 镜头投影按 id 覆写（无投影 / 查无 id 的占位行项 → 保持 None）。
                        if let Some(p) = lens.and_then(|m| m.get(&slot.id)) {
                            wire.duplicate_bucket = Some(p.bucket);
                            wire.duplicate_group_ordinal = Some(p.group_ordinal);
                            wire.duplicate_member_ordinal = p.member_ordinal;
                            wire.duplicate_member_count = p.member_count;
                        }
                        wire
                    })
                    .collect(),
            },
        })
        .collect()
}

// ── 就地 patch(S3 后唯一 patch 目标,经 AppState 组合调用) ─────────────

/// 通用 patch：对每个命中 id 应用 `f`；若 `sensitive`（该写会改变本缓存视图的**成员**）
/// 则同时降级 `reusable = false`——下次 compute 视同 MISS 重查。S3 后不可整体置 None：
/// 本缓存还是现存布局行的载荷源，置 None 会让出口拼装全部退化为占位行项（可视区破图）；
/// 值仍照常 patch，使旧布局在重查换代前的展示即时正确。
fn patch_or_degrade<F: Fn(&mut LayoutItem)>(
    cache: &ItemsCache,
    ids: &[i64],
    sensitive: impl FnOnce(&MediaFilter) -> bool,
    f: F,
) {
    if ids.is_empty() {
        return;
    }
    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_mut() else { return };
    if sensitive(&data.filter) {
        data.reusable = false;
    }
    for id in ids {
        let Some(&i) = data.id_to_idx.get(id) else {
            continue;
        };
        if let Some(it) = data.items.get_mut(i as usize) {
            f(it);
        }
    }
}

/// 缩略图结果就地 patch。**不失效不 bump**：缩略图不改变视图成员/几何，仅展示字段 ——
/// 这是 S1 的关键设计（浏览时的持续缩略图生成若走失效，缓存将长期冰冷）。
pub fn apply_thumb_results(cache: &ItemsCache, results: &[ThumbResult]) {
    if results.is_empty() {
        return;
    }
    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_mut() else { return };
    for r in results {
        let Some(&i) = data.id_to_idx.get(&r.item_id) else {
            continue;
        };
        if let Some(it) = data.items.get_mut(i as usize) {
            // 写边界防降级(与 db::queries::update_thumb_result 同语义,DB/常驻缓存两层必须一致):
            // 失败 patch(status=2)不得覆盖已有产物项(1=已生成/3=直显)。否则 DB 层守住了、
            // 此缓存仍被滞后的失败写冲掉,fetchRowsByY 出口拼装自此缓存 → 会话内封面照旧裂图。
            if r.thumb_status == 2 && (it.thumb_status == 1 || it.thumb_status == 3) {
                continue;
            }
            it.thumb_status = r.thumb_status;
            it.thumb_path = r.thumb_path.clone();
            it.thumbhash = r.thumbhash.clone();
        }
    }
}

/// 尺寸回填 patch（0×0 占位 → 真实尺寸；守卫同 SQL update_media_dimensions：仅 0×0 项）。
/// 尺寸是布局几何**输入**，patch 后下次重排（缓存命中）即产出正确比例——不失效不 bump
/// （补尺寸阶段的高频写走失效会让缓存长期冰冷）。dims = (id, w, h)。
pub fn set_dimensions(cache: &ItemsCache, dims: &[(i64, i64, i64)]) {
    if dims.is_empty() {
        return;
    }
    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    let Some(data) = guard.as_mut() else { return };
    let mut changed = false;
    for &(id, w, h) in dims {
        let Some(&i) = data.id_to_idx.get(&id) else {
            continue;
        };
        if let Some(it) = data.items.get_mut(i as usize) {
            if (it.width <= 0 || it.height <= 0) && (it.width != w || it.height != h) {
                it.width = w;
                it.height = h;
                changed = true;
            }
        }
    }
    if changed {
        data.geometry_revision += 1;
    }
}

/// 收藏 patch；favoritedOnly 视图（写改成员）→ 降级不可复用（reusable=false）。
pub fn set_favorite(cache: &ItemsCache, ids: &[i64], value: bool) {
    patch_or_degrade(
        cache,
        ids,
        |flt| flt.favorited_only == Some(true),
        |it| it.is_favorited = value,
    );
}

/// 评分 patch；minRating 过滤视图 → 降级不可复用。
pub fn set_rating(cache: &ItemsCache, ids: &[i64], rating: i64) {
    patch_or_degrade(
        cache,
        ids,
        |flt| flt.min_rating.is_some(),
        |it| it.rating = rating,
    );
}

/// 色标 patch；colorLabel 过滤视图 → 降级不可复用。
pub fn set_color_label(cache: &ItemsCache, ids: &[i64], color_label: i64) {
    patch_or_degrade(
        cache,
        ids,
        |flt| flt.color_label.is_some(),
        |it| it.color_label = color_label,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_item(id: i64, ts: i64, dir_id: i64) -> LayoutItem {
        LayoutItem {
            id,
            width: 100,
            height: 100,
            file_size: 0,
            sort_datetime: ts,
            file_format: "jpg".into(),
            media_type: "image".into(),
            is_live_photo: false,
            duration_ms: None,
            thumb_status: 0,
            thumb_path: None,
            thumbhash: None,
            is_favorited: false,
            rating: 0,
            color_label: 0,
            availability: "online".into(),
            dir_id: Some(dir_id),
            similarity: None,
            cache_key: 0,
        }
    }

    fn dl(rel: &str) -> DirLabel {
        // 单根测试助手：root_created_at/root_id 恒 0 → 目录序退化为 (DFS 键, dir_id)。
        DirLabel {
            rel_path: rel.into(),
            display: format!("C:/root/{rel}"),
            name: rel.into(),
            root_created_at: 0,
            root_id: 0,
        }
    }

    /// 基准序 fixture：ts DESC, id DESC(含同 ts 的 id tiebreaker)。
    fn canonical_data() -> ItemsCacheData {
        // (id, ts, dir): 基准序按 (ts DESC, id DESC)。
        let items = vec![
            mk_item(5, 300, 2),
            mk_item(4, 200, 1),
            mk_item(3, 200, 2), // 同 ts=200:id DESC → 4 在 3 前
            mk_item(1, 100, 1),
        ];
        let mut dir_labels = HashMap::new();
        dir_labels.insert(1, dl("a"));
        dir_labels.insert(2, dl("b"));
        let dir_rank = build_dir_rank(&dir_labels);
        let id_to_idx = build_id_index(&items);
        ItemsCacheData {
            filter_key: "{}".into(),
            order: CachedOrder::Canonical,
            data_version: 1,
            items,
            id_to_idx,
            dir_labels,
            dir_rank,
            filter: MediaFilter::default(),
            reusable: true,
            geometry_revision: 0,
            median_aspect: std::sync::OnceLock::new(),
            perm_memo: Mutex::new(None),
            filename_rank: std::sync::OnceLock::new(),
        }
    }

    fn ids(refs: &[&LayoutItem]) -> Vec<i64> {
        refs.iter().map(|it| it.id).collect()
    }

    /// 尺寸回填只改变几何输入；集合、时间序与取数命中资格必须保留。
    #[test]
    fn dimensions_patch_preserves_items_and_order() {
        let cache = new_items_cache();
        let mut data = canonical_data();
        data.items[0].width = 0;
        data.items[0].height = 0;
        store_items(&cache, data);
        set_dimensions(&cache, &[(5, 800, 400), (4, 900, 600)]);
        assert!(is_hit_valid(&cache, "{}", 1, "date", "datetime", "desc"));
        let guard = cache.read().unwrap();
        let data = guard.as_ref().unwrap();
        assert_eq!((data.items[0].width, data.items[0].height), (800, 400));
        assert_eq!((data.items[1].width, data.items[1].height), (100, 100));
        assert_eq!(
            ids(&derive_order(data, "date", "datetime", "desc")),
            vec![5, 4, 3, 1]
        );
        assert_eq!(data.geometry_revision, 1);
        drop(guard);
        // 重复回填与不存在的 id 不制造新几何代，避免无意义重排。
        set_dimensions(&cache, &[(5, 800, 400), (99, 800, 400)]);
        assert_eq!(cache.read().unwrap().as_ref().unwrap().geometry_revision, 1);
    }

    /// 写边界防降级(与 db::queries::update_thumb_result 同语义):失败 patch(2)不得覆盖
    /// 产物项(1/3);成功 patch(1)与对 pending 项(0)的失败 patch 照常落地。
    #[test]
    fn apply_thumb_results_refuses_failure_downgrade() {
        let cache = new_items_cache();
        let mut data = canonical_data();
        // id=5 已生成(1),id=4 直显(3),id=3 待生成(0)。
        data.items[0].thumb_status = 1;
        data.items[0].thumb_path = Some("480/aa/x.webp".into());
        data.items[1].thumb_status = 3;
        data.items[1].thumb_path = Some("/r/d.jpg".into());
        *cache.write().unwrap() = Some(data);

        let fail = |id: i64| ThumbResult {
            item_id: id,
            thumb_status: 2,
            thumb_path: None,
            thumbhash: None,
            source_revision: 0,
            cache_key: 0,
        };
        apply_thumb_results(&cache, &[fail(5), fail(4), fail(3)]);

        let guard = cache.read().unwrap();
        let items = &guard.as_ref().unwrap().items;
        assert_eq!(
            (items[0].thumb_status, items[0].thumb_path.as_deref()),
            (1, Some("480/aa/x.webp")),
            "已生成项不被失败 patch 降级"
        );
        assert_eq!(items[1].thumb_status, 3, "直显项不被失败 patch 降级");
        assert_eq!(items[2].thumb_status, 2, "pending 项正常标失败");
        drop(guard);

        // 成功 patch 恒无条件:失败项被治愈。
        apply_thumb_results(
            &cache,
            &[ThumbResult {
                item_id: 3,
                thumb_status: 1,
                thumb_path: Some("480/cc/y.webp".into()),
                thumbhash: None,
                source_revision: 0,
                cache_key: 0,
            }],
        );
        let guard = cache.read().unwrap();
        assert_eq!(guard.as_ref().unwrap().items[2].thumb_status, 1);
    }

    #[test]
    fn derive_identity_desc_and_reversed_asc() {
        let data = canonical_data();
        assert_eq!(
            ids(&derive_order(&data, "date", "datetime", "desc")),
            vec![5, 4, 3, 1]
        );
        // asc = 整体反转 → (ts ASC, id ASC)。
        assert_eq!(
            ids(&derive_order(&data, "date", "datetime", "asc")),
            vec![1, 3, 4, 5]
        );
        // none 轴同 date(无分隔符差异,序一致)。
        assert_eq!(
            ids(&derive_order(&data, "none", "datetime", "desc")),
            vec![5, 4, 3, 1]
        );
    }

    #[test]
    fn derive_folder_sorts_by_rel_path_rank_then_ts_id() {
        let data = canonical_data();
        // folder desc:dir a(rank 0)先 → 其内 (ts DESC,id DESC) = [4,1];dir b → [5,3]。
        assert_eq!(
            ids(&derive_order(&data, "folder", "datetime", "desc")),
            vec![4, 1, 5, 3]
        );
        // folder asc:rel_path 仍 ASC,组内 (ts ASC,id ASC) = [1,4] / [3,5]。
        assert_eq!(
            ids(&derive_order(&data, "folder", "datetime", "asc")),
            vec![1, 4, 3, 5]
        );
    }

    #[test]
    fn datetime_baseline_derives_filename_via_rank() {
        // 双键统一：datetime 基准 + 合成 filename_rank → 跨键派生 filename 序（不碰 DB）。
        // canonical_data items 存序（datetime DESC）：idx0=id5, idx1=id4, idx2=id3, idx3=id1。
        // 令自然序 id1<id3<id4<id5 → filename_rank[i] = items[i] 的位次 = [3,2,1,0]。
        let data = canonical_data();
        data.filename_rank.set(vec![3, 2, 1, 0]).unwrap();

        // none+filename：按 (rank,id) 实排。asc = 位次升序 → id1,id3,id4,id5。
        assert_eq!(
            ids(&derive_order(&data, "none", "filename", "asc")),
            vec![1, 3, 4, 5]
        );
        assert_eq!(
            ids(&derive_order(&data, "none", "filename", "desc")),
            vec![5, 4, 3, 1]
        );
        // folder+filename：dir a(rank0)={id4,id1} dir b(rank1)={id5,id3}，组内按位次。
        // asc: a[id1,id4] b[id3,id5]；desc: a[id4,id1] b[id5,id3]。
        assert_eq!(
            ids(&derive_order(&data, "folder", "filename", "asc")),
            vec![1, 4, 3, 5]
        );
        assert_eq!(
            ids(&derive_order(&data, "folder", "filename", "desc")),
            vec![4, 1, 5, 3]
        );
        // 同一份 datetime 基准仍能派 datetime（双键并存，互不干扰）。
        assert_eq!(
            ids(&derive_order(&data, "none", "datetime", "desc")),
            vec![5, 4, 3, 1]
        );
    }

    /// B-file-iii `GlobalFilenameRank::ranks_for` 判据（错则选区漂移）：dv 符 + 全 id 覆盖才返 Some
    /// 平行 rank；稀疏子集保序；dv 不符 / 缺 id 一律 None（退化 B-file-i，绝不塞末尾坏序）。
    #[test]
    fn global_rank_ranks_for_covers_and_guards() {
        let mut m: FxHashMap<i64, u32> = FxHashMap::default();
        for (id, r) in [(1, 0u32), (2, 1), (3, 2), (4, 3), (5, 4)] {
            m.insert(id, r);
        }
        let global = GlobalFilenameRank {
            data_version: 7,
            id_to_rank: m,
        };

        // 全覆盖 + dv 符 → 平行 rank（顺序随 items，非按 rank 重排）。
        let items = vec![mk_item(3, 0, 1), mk_item(1, 0, 1), mk_item(5, 0, 1)];
        assert_eq!(
            global.ranks_for(&items, 7),
            Some(vec![2, 0, 4]),
            "平行 rank 应随 items 顺序"
        );
        // 稀疏子集仍保序（子集 = {5,2} → 位次 [4,1]，自然序限制到子集保序）。
        let subset = vec![mk_item(5, 0, 1), mk_item(2, 0, 1)];
        assert_eq!(global.ranks_for(&subset, 7), Some(vec![4, 1]));
        // dv 不符 → None（stale，退化）。
        assert_eq!(global.ranks_for(&items, 8), None, "dv 不符须退化");
        // 任一 id 缺失（99 不在 map）→ 整体 None（非默认基集/竞态，退化不坏序）。
        let with_missing = vec![mk_item(1, 0, 1), mk_item(99, 0, 1)];
        assert_eq!(
            global.ranks_for(&with_missing, 7),
            None,
            "缺 id 须整体退化(不塞 u32::MAX 坏序)"
        );
    }

    #[test]
    fn folder_perm_memo_reused_and_replaced() {
        let data = canonical_data();
        assert!(data.perm_memo.lock().unwrap().is_none());
        let first = ids(&derive_order(&data, "folder", "datetime", "desc"));
        assert!(
            data.perm_memo.lock().unwrap().is_some(),
            "首次 folder 派生应写入置换 memo"
        );
        // memo 命中路径必须还原出与首次完全一致的序。
        assert_eq!(
            ids(&derive_order(&data, "folder", "datetime", "desc")),
            first
        );
        // 换方向 → memo 替换,序仍正确(等价于 derive_folder 测试的 asc 期望)。
        assert_eq!(
            ids(&derive_order(&data, "folder", "datetime", "asc")),
            vec![1, 4, 3, 5]
        );
        let memo = data.perm_memo.lock().unwrap();
        assert_eq!(memo.as_ref().unwrap().sort_order, "asc");
    }

    /// **T2 表征（date+filename）**：Canonical 基准下 date+filename 现在也写入共享 memo；
    /// 相同排序参数的连续几何重排必须走命中还原（命中计数 > 0）、不重新排序，且序不变；
    /// 换方向 → memo 替换、序按新方向。命中计数是「排序真的省下」的直接判据。
    #[test]
    fn date_filename_perm_memo_reused_on_geometry_repeat() {
        let data = canonical_data();
        data.filename_rank.set(vec![3, 2, 1, 0]).unwrap();
        assert!(data.perm_memo.lock().unwrap().is_none());

        let hits = || PERM_MEMO_HITS.with(|c| c.get());
        let first = ids(&derive_order(&data, "date", "filename", "desc"));
        assert_eq!(
            first,
            vec![5, 4, 3, 1],
            "date 单桶 desc = filename 位次降序"
        );
        assert!(
            data.perm_memo.lock().unwrap().is_some(),
            "date+filename 应写共享 memo"
        );

        // 同参数重复派生（模拟只改宽度/行高的几何重排）：命中 memo，序与首次一致。
        let before = hits();
        for _ in 0..5 {
            assert_eq!(ids(&derive_order(&data, "date", "filename", "desc")), first);
        }
        assert_eq!(
            hits() - before,
            5,
            "同参数几何重排必须走 memo 命中(零重新排序)"
        );
        assert_eq!(
            data.perm_memo.lock().unwrap().as_ref().unwrap().group_by,
            "date"
        );

        // 换方向 → memo 替换为 asc 置换（不是新增第二个槽）。
        let asc_before = hits();
        assert_eq!(
            ids(&derive_order(&data, "date", "filename", "asc")),
            vec![1, 3, 4, 5]
        );
        assert_eq!(hits(), asc_before, "换方向是重排(memo 未命中)");
        let memo = data.perm_memo.lock().unwrap();
        let m = memo.as_ref().unwrap();
        assert_eq!(
            (m.group_by.as_str(), m.sort_order.as_str()),
            ("date", "asc")
        );
    }

    /// **T2 表征（none+filename 跨键实排）**：datetime 基准派 filename 是跨键实排，现在同样
    /// 写入共享 memo；同参数重复派生零重新排序。`filename_rank` 未就绪的防御序不入 memo
    /// （否则位次填入后仍会命中退化置换）。
    #[test]
    fn cross_key_none_filename_perm_memo_reused_and_guarded() {
        // ① 位次未就绪：防御序只物化、不落 memo，避免退化置换被后续命中。
        let data = canonical_data();
        let hits = || PERM_MEMO_HITS.with(|c| c.get());
        assert_eq!(
            ids(&derive_order(&data, "none", "filename", "asc")),
            vec![1, 3, 4, 5],
            "缺位次 → 次键 i64::MAX 的防御序(全部同键,按 id 决胜)"
        );
        assert!(
            data.perm_memo.lock().unwrap().is_none(),
            "次键未就绪的退化序不得进 memo"
        );

        // ② 位次就绪：实排后落 memo，同参数重复派生全命中。
        data.filename_rank.set(vec![3, 2, 1, 0]).unwrap();
        let first = ids(&derive_order(&data, "none", "filename", "asc"));
        assert_eq!(first, vec![1, 3, 4, 5]);
        assert!(
            data.perm_memo.lock().unwrap().is_some(),
            "跨键实排应写共享 memo"
        );
        let before = hits();
        for _ in 0..3 {
            assert_eq!(ids(&derive_order(&data, "none", "filename", "asc")), first);
        }
        assert_eq!(hits() - before, 3, "跨键同参数几何重排必须走 memo 命中");
    }

    /// **T2 表征（同键快路径不占 memo）**：none/date+datetime 同键是线性恒等/反转，
    /// 本就不排序，故不写 memo、也不产生命中——避免把线性快路径误报成缓存收益。
    #[test]
    fn same_key_fast_paths_do_not_touch_perm_memo() {
        let data = canonical_data();
        let hits = || PERM_MEMO_HITS.with(|c| c.get());
        let before = hits();
        assert_eq!(
            ids(&derive_order(&data, "date", "datetime", "desc")),
            vec![5, 4, 3, 1]
        );
        assert_eq!(
            ids(&derive_order(&data, "none", "datetime", "asc")),
            vec![1, 3, 4, 5]
        );
        assert_eq!(hits(), before, "同键快路径不产生 memo 命中");
        assert!(
            data.perm_memo.lock().unwrap().is_none(),
            "同键快路径不占唯一 memo 槽"
        );
    }

    /// **T2 表征（就地 patch 不使顺序 memo 失效）**：尺寸回填只改几何输入（geometry_revision
    /// 递增），排序键 (sort_datetime/filename_rank/dir_id/id) 未动 → 同一缓存体上的 memo 仍命中，
    /// 序不变。这正是「data geometry_revision 就地 patch 不失效顺序 memo」的锁定。
    #[test]
    fn dimensions_patch_keeps_date_filename_perm_memo_hit() {
        let cache = new_items_cache();
        let mut data = canonical_data();
        data.filename_rank.set(vec![3, 2, 1, 0]).unwrap();
        data.items[0].width = 0;
        data.items[0].height = 0;
        store_items(&cache, data);

        let hits = || PERM_MEMO_HITS.with(|c| c.get());
        let first = {
            let guard = cache.read().unwrap();
            ids(&derive_order(
                guard.as_ref().unwrap(),
                "date",
                "filename",
                "desc",
            ))
        };
        assert_eq!(first, vec![5, 4, 3, 1]);

        // 就地补尺寸：几何代 +1，排序键未动。
        set_dimensions(&cache, &[(5, 800, 400)]);
        {
            let guard = cache.read().unwrap();
            let data = guard.as_ref().unwrap();
            assert_eq!(data.geometry_revision, 1, "尺寸 patch 只推进几何代");
            let before = hits();
            assert_eq!(
                ids(&derive_order(data, "date", "filename", "desc")),
                first,
                "尺寸 patch 后序不变"
            );
            assert_eq!(hits() - before, 1, "尺寸 patch 不使顺序 memo 失效(仍命中)");
        }

        // 展示字段 patch（缩略图/收藏/评分/色标）同样不触碰排序键。
        apply_thumb_results(
            &cache,
            &[ThumbResult {
                item_id: 4,
                thumb_status: 1,
                thumb_path: Some("t/4.webp".into()),
                thumbhash: None,
                source_revision: 0,
                cache_key: 0,
            }],
        );
        let guard = cache.read().unwrap();
        let before = hits();
        assert_eq!(
            ids(&derive_order(
                guard.as_ref().unwrap(),
                "date",
                "filename",
                "desc"
            )),
            first
        );
        assert_eq!(hits() - before, 1, "缩略图 patch 不使顺序 memo 失效");
    }

    #[test]
    fn same_rel_path_uses_dir_id_tiebreaker_and_stays_contiguous() {
        // 两根同 rel_path "x"，基准序刻意交错两个目录；folder 派生后必须按目录连续成组，
        // 否则 layout 会为同一目录生成多段 separator，滑块联动会在同名目录间跳动。
        let items = vec![
            mk_item(4, 400, 10),
            mk_item(3, 300, 11),
            mk_item(2, 200, 10),
            mk_item(1, 100, 11),
        ];
        let mut dir_labels = HashMap::new();
        dir_labels.insert(10, dl("x"));
        dir_labels.insert(11, dl("x"));
        let dir_rank = build_dir_rank(&dir_labels);
        assert!(
            dir_rank[&10] < dir_rank[&11],
            "同 rel_path 由较小目录 id 稳定裁决"
        );
        let id_to_idx = build_id_index(&items);
        let data = ItemsCacheData {
            filter_key: "{}".into(),
            order: CachedOrder::Canonical,
            data_version: 1,
            items,
            id_to_idx,
            dir_labels,
            dir_rank,
            filter: MediaFilter::default(),
            reusable: true,
            geometry_revision: 0,
            median_aspect: std::sync::OnceLock::new(),
            perm_memo: Mutex::new(None),
            filename_rank: std::sync::OnceLock::new(),
        };
        assert_eq!(
            ids(&derive_order(&data, "folder", "datetime", "desc")),
            vec![4, 2, 3, 1],
            "同 rel_path 的两个目录不得按时间跨目录交错"
        );
    }

    #[test]
    fn thumb_patch_updates_in_place_without_invalidation() {
        let cache = new_items_cache();
        store_items(&cache, canonical_data());
        apply_thumb_results(
            &cache,
            &[ThumbResult {
                item_id: 3,
                thumb_status: 1,
                thumb_path: Some("t/3.webp".into()),
                thumbhash: Some(vec![9]),
                source_revision: 0,
                cache_key: 0,
            }],
        );
        let guard = cache.read().unwrap();
        let data = guard.as_ref().expect("thumb patch 不应失效缓存");
        let it = &data.items[data.id_to_idx[&3] as usize];
        assert_eq!(it.thumb_status, 1);
        assert_eq!(it.thumb_path.as_deref(), Some("t/3.webp"));
    }

    #[test]
    fn favorite_patches_normal_view_but_invalidates_favorites_view() {
        // 普通视图:就地 patch。
        let cache = new_items_cache();
        store_items(&cache, canonical_data());
        set_favorite(&cache, &[4], true);
        {
            let guard = cache.read().unwrap();
            let data = guard.as_ref().unwrap();
            assert!(data.items[data.id_to_idx[&4] as usize].is_favorited);
        }
        // favoritedOnly 视图:成员变化 → 降级不可复用(S3:数据保留,继续服务出口拼装)。
        let mut sensitive = canonical_data();
        sensitive.filter.favorited_only = Some(true);
        store_items(&cache, sensitive);
        set_favorite(&cache, &[4], false);
        let guard = cache.read().unwrap();
        let data = guard
            .as_ref()
            .expect("敏感写应保留数据(载荷源),不得置 None");
        assert!(!data.reusable, "敏感写应降级 reusable=false");
        assert!(
            !data.items[data.id_to_idx[&4] as usize].is_favorited,
            "降级同时值仍应被 patch(旧布局展示即时正确)"
        );
    }

    /// S3.1:is_hit_valid 与 HIT 守卫同判据——reusable/序形态/数据代/过滤器键四关全过才 true。
    #[test]
    fn is_hit_valid_mirrors_hit_guard() {
        let cache = new_items_cache();
        assert!(
            !is_hit_valid(&cache, "{}", 1, "date", "datetime", "desc"),
            "空缓存不可命中"
        );

        store_items(&cache, canonical_data());
        // 双键统一缓存:datetime 轴任意 group 可派生(items 自带 sort_datetime)。
        assert!(is_hit_valid(&cache, "{}", 1, "date", "datetime", "desc"));
        assert!(is_hit_valid(&cache, "{}", 1, "folder", "datetime", "asc"));
        // datetime 基准也服务 filename 轴 none/folder/date(经惰性 filename_rank 补;预检只判可派生性)。
        assert!(is_hit_valid(&cache, "{}", 1, "none", "filename", "asc"));
        assert!(is_hit_valid(&cache, "{}", 1, "folder", "filename", "desc"));
        // date+filename 现亦可派生(B-file-iii/A′:UTC 日桶 div_euclid 内存分桶)。
        assert!(is_hit_valid(&cache, "{}", 1, "date", "filename", "desc"));
        assert!(is_hit_valid(&cache, "{}", 1, "date", "filename", "asc"));
        // 数据代/过滤器键不符。
        assert!(!is_hit_valid(&cache, "{}", 2, "date", "datetime", "desc"));
        assert!(!is_hit_valid(
            &cache,
            "{\"x\":1}",
            1,
            "date",
            "datetime",
            "desc"
        ));

        // 敏感写降级 reusable=false → 探针同步失效(仍作载荷源,但不可命中)。
        let mut sensitive = canonical_data();
        sensitive.filter.favorited_only = Some(true);
        store_items(&cache, sensitive);
        set_favorite(&cache, &[4], false);
        assert!(!is_hit_valid(&cache, "{}", 1, "date", "datetime", "desc"));

        // Sql 序形态:三元组逐项相等才可命中。
        let mut sql = canonical_data();
        sql.order = CachedOrder::Sql {
            group_by: "date".into(),
            sort_within: "filename".into(),
            sort_order: "desc".into(),
        };
        store_items(&cache, sql);
        assert!(is_hit_valid(&cache, "{}", 1, "date", "filename", "desc"));
        assert!(!is_hit_valid(&cache, "{}", 1, "date", "filename", "asc"));

        // CanonicalFilename 序形态(双键统一):filename none/folder/date 皆可派生(date 用下标为位次 +
        // UTC 日桶 div_euclid);datetime 轴任意 group 也可(items 自带 sort_datetime,跨键实排,免额外查询)。
        let mut fname = canonical_data();
        fname.order = CachedOrder::CanonicalFilename;
        store_items(&cache, fname);
        assert!(is_hit_valid(&cache, "{}", 1, "folder", "filename", "desc"));
        assert!(is_hit_valid(&cache, "{}", 1, "none", "filename", "asc"));
        // 双键:filename 基准也服务 datetime。
        assert!(is_hit_valid(&cache, "{}", 1, "folder", "datetime", "desc"));
        assert!(is_hit_valid(&cache, "{}", 1, "date", "datetime", "asc"));
        // date+filename 现亦可派生(B-file-iii/A′)。
        assert!(is_hit_valid(&cache, "{}", 1, "date", "filename", "desc"));
        assert!(is_hit_valid(&cache, "{}", 1, "date", "filename", "asc"));
    }

    #[test]
    fn rating_and_color_sensitivity() {
        let cache = new_items_cache();
        let mut d = canonical_data();
        d.filter.min_rating = Some(3);
        store_items(&cache, d);
        set_rating(&cache, &[1], 5);
        {
            let guard = cache.read().unwrap();
            let data = guard.as_ref().expect("敏感写应保留数据");
            assert!(!data.reusable, "minRating 视图评分写应降级不可复用");
            assert_eq!(data.items[data.id_to_idx[&1] as usize].rating, 5);
        }

        let mut d2 = canonical_data();
        d2.filter.color_label = Some(2);
        store_items(&cache, d2);
        set_color_label(&cache, &[1], 4);
        let guard = cache.read().unwrap();
        let data = guard.as_ref().expect("敏感写应保留数据");
        assert!(!data.reusable, "colorLabel 视图色标写应降级不可复用");
        assert_eq!(data.items[data.id_to_idx[&1] as usize].color_label, 4);
    }

    /// S3 出口拼装:命中 id → 载荷来自 items;未知 id → 占位(几何保留);None 缓存 → 全占位。
    #[test]
    fn hydrate_rows_fills_payload_and_placeholders() {
        use crate::layout::geometry::SlimRowItem;
        let cache = new_items_cache();
        store_items(&cache, canonical_data());
        let slot = |id: i64, x: f64| SlimRowItem {
            id,
            x,
            w: 50.0,
            h: 40.0,
        };
        let rows = vec![
            LayoutRow::Separator {
                y: 0.0,
                height: 36.0,
                separator_label: "sep".into(),
                group_id: Some("g".into()),
                epoch_day: None,
                separator_kind: None,
                lens_folder: None,
                lens_group: None,
            },
            LayoutRow::Normal {
                y: 36.0,
                height: 40.0,
                items: vec![slot(4, 0.0), slot(999, 60.0)],
            },
        ];
        let wire = hydrate_rows(&cache, rows.clone(), None, None);
        assert_eq!(wire.len(), 2);
        match &wire[1] {
            HydratedRow::Normal { items, .. } => {
                assert_eq!(items.len(), 2, "未知 id 不丢行项(占位)");
                assert_eq!(items[0].id, 4);
                assert_eq!(items[0].file_format, "jpg", "载荷应来自 items 缓存");
                assert_eq!((items[0].x, items[0].w, items[0].h), (0.0, 50.0, 40.0));
                assert_eq!(items[1].id, 999);
                assert_eq!(items[1].file_format, "", "未知 id → 占位载荷");
                assert_eq!(items[1].x, 60.0, "占位保留几何");
            }
            _ => panic!("expected normal row"),
        }
        // None 缓存(清库瞬态)→ 全占位,不 panic 不丢行。
        invalidate(&cache);
        let wire2 = hydrate_rows(&cache, rows, None, None);
        match &wire2[1] {
            HydratedRow::Normal { items, .. } => {
                assert_eq!(items[0].file_format, "", "None 缓存 → 占位");
            }
            _ => panic!("expected normal row"),
        }
    }

    /// 重复镜头出口投影（2026-09-02 方案 §11.1）：带投影 → 逐项镜头字段按 id 填值
    /// 且线上 JSON 含 camelCase 键；无投影 → 字段 None、JSON 不含键（wire 不膨胀）；
    /// separatorKind 随常驻行透传；占位行项（查无 id）不携带镜头字段。
    #[test]
    fn hydrate_rows_fills_lens_projection() {
        use crate::db::models::DuplicateBucket;
        use crate::layout::geometry::{GallerySeparatorKind, LensGroupSeparator, SlimRowItem};
        use crate::layout::lens::{LensItemProjection, LensProjection};
        use serde_json::json;
        let cache = new_items_cache();
        store_items(&cache, canonical_data());
        let slot = |id: i64, x: f64| SlimRowItem {
            id,
            x,
            w: 50.0,
            h: 40.0,
        };
        let rows = vec![
            LayoutRow::Separator {
                y: 0.0,
                height: 36.0,
                separator_label: String::new(),
                group_id: Some("k1".into()),
                epoch_day: None,
                separator_kind: Some(GallerySeparatorKind::DuplicateGroup),
                lens_folder: None,
                lens_group: Some(LensGroupSeparator {
                    ordinal: 1,
                    member_count: 2,
                    folder_count: 2,
                    unit_size: 1024,
                }),
            },
            LayoutRow::Normal {
                y: 36.0,
                height: 40.0,
                items: vec![slot(4, 0.0), slot(999, 60.0)],
            },
        ];
        let mut lens = LensProjection::new();
        lens.insert(
            4,
            LensItemProjection {
                bucket: DuplicateBucket::Duplicate,
                group_ordinal: 1,
                member_ordinal: Some(2),
                member_count: Some(3),
            },
        );
        let wire = hydrate_rows(&cache, rows.clone(), None, Some(&lens));
        match &wire[0] {
            HydratedRow::Separator { separator_kind, .. } => {
                assert_eq!(separator_kind, &Some(GallerySeparatorKind::DuplicateGroup));
            }
            _ => panic!("expected separator row"),
        }
        match &wire[1] {
            HydratedRow::Normal { items, .. } => {
                assert_eq!(items[0].duplicate_bucket, Some(DuplicateBucket::Duplicate));
                assert_eq!(items[0].duplicate_group_ordinal, Some(1));
                assert_eq!(items[0].duplicate_member_ordinal, Some(2));
                assert_eq!(
                    items[0].duplicate_member_count,
                    Some(3),
                    "groups 投影携带组内位次"
                );
                // 占位行项（999 查无 id）不携带镜头字段。
                assert_eq!(items[1].duplicate_bucket, None);
            }
            _ => panic!("expected normal row"),
        }
        // wire 键形状：有投影 → duplicateBucket 等键存在；无投影 → 不存在。
        let with_lens = serde_json::to_value(&wire[1]).unwrap();
        assert_eq!(with_lens["items"][0]["duplicateBucket"], json!("duplicate"));
        assert_eq!(with_lens["items"][0]["duplicateMemberOrdinal"], 2);
        let group = serde_json::to_value(&wire[0]).unwrap();
        assert_eq!(group["separatorLabel"], "");
        assert_eq!(group["duplicateGroupOrdinal"], 1);
        assert_eq!(group["duplicateMemberCount"], 2);
        assert_eq!(group["duplicateFolderCount"], 2);
        assert_eq!(group["duplicateUnitSize"], 1024);
        let plain = hydrate_rows(&cache, rows, None, None);
        let without_lens = serde_json::to_value(&plain[1]).unwrap();
        assert!(
            without_lens["items"][0].get("duplicateBucket").is_none(),
            "无投影的线上 JSON 不得含镜头键"
        );
        // 普通画廊分隔符（separatorKind=None）线上 JSON 不含 separatorKind——wire 不膨胀。
        let plain_sep_rows = vec![LayoutRow::Separator {
            y: 0.0,
            height: 36.0,
            separator_label: "2024年3月20日".into(),
            group_id: Some("2024-03".into()),
            epoch_day: None,
            separator_kind: None,
            lens_folder: None,
            lens_group: None,
        }];
        let plain_sep =
            serde_json::to_value(&hydrate_rows(&cache, plain_sep_rows, None, None)[0]).unwrap();
        assert!(
            plain_sep.get("separatorKind").is_none(),
            "date/folder 分隔符线上 JSON 不含 separatorKind"
        );
    }

    // ── P1-5:滚动取行出口的 IO/锁语义(收集 → 阻塞解析 → 应用)────────────────────

    /// 收集阶段的守卫与 hydrate_item 的选档分支同构:只有 status=1 且有非空 thumb_path 的项
    /// 进请求;几何按 DPR 折成设备像素需求(与应用阶段同一算式)。
    #[test]
    fn collect_serve_requests_matches_hydrate_guards() {
        use crate::layout::geometry::SlimRowItem;
        let cache = new_items_cache();
        let mut data = canonical_data();
        data.items[0].thumb_status = 1;
        data.items[0].thumb_path = Some("512/aa/x.webp".into());
        data.items[0].cache_key = 7;
        // status 未生成 → 不参与
        data.items[1].thumb_path = Some("512/bb/y.webp".into());
        // 空路径 → 不参与
        data.items[2].thumb_status = 1;
        data.items[2].thumb_path = Some(String::new());
        let ids: Vec<i64> = data.items.iter().map(|it| it.id).collect();
        store_items(&cache, data);

        let slot = |id: i64| SlimRowItem {
            id,
            x: 0.0,
            w: 60.0,
            h: 30.0,
        };
        let rows = vec![LayoutRow::Normal {
            y: 0.0,
            height: 30.0,
            items: vec![slot(ids[0]), slot(ids[1]), slot(ids[2]), slot(9999)],
        }];
        let reqs = collect_serve_requests(&cache, &rows, 2.0);
        assert_eq!(reqs.len(), 1, "仅 status=1 + 非空路径的项进请求");
        assert_eq!(reqs[0].cache_key, 7);
        assert_eq!(reqs[0].db_path, "512/aa/x.webp");
        assert_eq!(reqs[0].need_px, serve_need_px(60.0, 30.0, 2.0));
        // 缓存为空(清库瞬态)→ 空请求,不 panic。
        invalidate(&cache);
        assert!(collect_serve_requests(&cache, &rows, 1.0).is_empty());
    }

    /// **机制锁**(P1-5 核心):三段式里只有「收集」与「应用」持载荷读锁,两者零 IO;
    /// 磁盘 IO(probe + 逐项 stat)全在锁外。用假 IO 在每次调用时试写载荷锁做判据,
    /// 并以一次**故意锁内 IO** 自检探针灵敏度(防探针失灵造成假通过)。
    #[test]
    fn serve_phases_run_io_outside_payload_lock() {
        use crate::layout::geometry::SlimRowItem;
        use crate::thumbnail::cache::thumb_db_path;
        use crate::thumbnail::serve::test_io::CountingIo;
        use crate::thumbnail::serve::ServeIo;
        use crate::thumbnail::serve::{ThumbProbeCache, THUMB_PROBE_TTL};
        use std::sync::Arc as StdArc;

        let cache = StdArc::new(new_items_cache());
        let key = 0x1234_5678i64; // gitleaks:allow — 测试缓存索引，不是凭据。
        let mut data = canonical_data();
        data.items[0].thumb_status = 1;
        data.items[0].cache_key = key;
        data.items[0].thumb_path = Some(thumb_db_path(512, key));
        let id = data.items[0].id;
        store_items(&cache, data);
        let rows = vec![LayoutRow::Normal {
            y: 0.0,
            height: 40.0,
            items: vec![SlimRowItem {
                id,
                x: 0.0,
                w: 60.0,
                h: 45.0,
            }],
        }];

        let io = CountingIo::default();
        io.set_results(true, true);
        io.set_lock_probe({
            let c = StdArc::clone(&cache);
            move || c.try_write().is_ok()
        });

        // 自检:持读锁时调 IO,探针必须记到「锁内 IO」。
        {
            let _guard = cache.read().unwrap();
            let _ = io.file_exists(std::path::Path::new("x"));
        }
        assert_eq!(io.under_lock(), 1, "探针灵敏度自检");

        // ① 收集(载荷读锁内,零 IO)。
        let before = io.counts();
        let requests = collect_serve_requests(&cache, &rows, 1.0);
        assert_eq!(io.counts(), before, "收集阶段不得触盘");
        assert_eq!(requests.len(), 1);

        // ② 解析(锁外):probe + stat 都发生在这里,且均不在载荷锁内。
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve = ThumbServe::prepare_with(
            &io,
            &probes,
            std::path::Path::new("C:/cache"),
            1.0,
            &requests,
            std::time::Instant::now(),
        );
        assert!(
            io.counts().0 > 0 && io.counts().1 > 0,
            "解析阶段确实做了 IO"
        );
        assert_eq!(
            io.under_lock(),
            1,
            "解析阶段的 IO 不得发生在载荷锁内(仅自检那一次)"
        );

        // ③ 应用(载荷读锁内,零 IO):锁外确认存在的目标档在锁内被采用。
        let wire = hydrate_rows(&cache, rows, Some(&serve), None);
        assert_eq!(io.under_lock(), 1, "应用阶段不得触盘");
        match &wire[0] {
            HydratedRow::Normal { items, .. } => assert_eq!(
                items[0].thumb_path.as_deref(),
                Some(thumb_db_path(64, key).as_str())
            ),
            _ => panic!("expected normal row"),
        }
    }

    /// **端到端(真实 FS,临时目录)**:① 目标档存在 → 出口重写到该档;② 整个缓存目录被删
    /// (清缓存)→ 重探后回退 DB 路径,绝不端出死路径;③ 换盘(另建 cache_dir,档位不同)→
    /// 即使 TTL 未过也只认新目录的档。
    #[test]
    fn serve_phases_rewrite_and_recover_after_cache_delete() {
        use crate::layout::geometry::SlimRowItem;
        use crate::thumbnail::cache::thumb_db_path;
        use crate::thumbnail::serve::{RealServeIo, ThumbProbeCache, THUMB_PROBE_TTL};

        let base = std::env::temp_dir().join(format!("scrollery-p15-items-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir_a = base.join("cache-a");
        let dir_b = base.join("cache-b");
        let key = 0x2b2b_2b2bi64;
        // A 盘:64 档文件存在。B 盘:128 档文件存在(64/512 档目录空)。
        for (dir, tier) in [(&dir_a, 64u32), (&dir_b, 128)] {
            let p = dir.join("thumbnails").join(thumb_db_path(tier, key));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }

        let cache = new_items_cache();
        let mut data = canonical_data();
        data.items[0].thumb_status = 1;
        data.items[0].cache_key = key;
        data.items[0].thumb_path = Some(thumb_db_path(512, key));
        let id = data.items[0].id;
        store_items(&cache, data);
        let rows = vec![LayoutRow::Normal {
            y: 0.0,
            height: 40.0,
            items: vec![SlimRowItem {
                id,
                x: 0.0,
                w: 60.0,
                h: 45.0,
            }],
        }];

        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let hydrate = |dir: &std::path::Path| {
            let requests = collect_serve_requests(&cache, &rows, 1.0);
            let serve = ThumbServe::prepare_with(
                &RealServeIo,
                &probes,
                dir,
                1.0,
                &requests,
                std::time::Instant::now(),
            );
            hydrate_rows(&cache, rows.clone(), Some(&serve), None)
        };
        let thumb_of = |wire: &[HydratedRow]| match &wire[0] {
            HydratedRow::Normal { items, .. } => items[0].thumb_path.clone(),
            _ => panic!("expected normal row"),
        };

        // ① A 盘:64 档存在 → 重写
        assert_eq!(thumb_of(&hydrate(&dir_a)), Some(thumb_db_path(64, key)));
        // ② 整个缓存目录删掉(清缓存)→ 重探后回退 DB 路径,不端死路径
        std::fs::remove_dir_all(&dir_a).unwrap();
        probes.invalidate();
        assert_eq!(thumb_of(&hydrate(&dir_a)), Some(thumb_db_path(512, key)));
        // ③ 换盘 A→B(TTL 内):B 只有 128 档 → 重写到 128,不用旧盘结论
        assert_eq!(thumb_of(&hydrate(&dir_b)), Some(thumb_db_path(128, key)));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **P1-5 测量(改动后)**:同夹具(240 可视项,DB 路径=512 档)下「一次取行批」三段
    /// (收集 → 锁外解析 → 应用)的 IO 次数、锁内 IO 次数与 p50/p95。慢盘用**显式注入延迟**
    /// (2ms/read_dir + 1ms/stat)参数化,不代表任何真实设备;真实磁盘数字另见报告。
    /// 运行:`cargo test -p scrollery --lib fetch_io_profile -- --nocapture`
    ///
    /// 出口拼装夹具:`n` 个可视项(DB 路径 = 512 档,统一 60×45 格),按每行 60 格切行。
    fn serve_profile_payload(n: i64, key0: i64) -> (ItemsCache, Vec<LayoutRow>) {
        use crate::layout::geometry::SlimRowItem;
        use crate::thumbnail::cache::thumb_db_path;
        let cache = new_items_cache();
        let items: Vec<LayoutItem> = (0..n)
            .map(|i| {
                let mut it = mk_item(i + 1, 1_000 + i, 1);
                it.thumb_status = 1;
                it.cache_key = key0 + i;
                it.thumb_path = Some(thumb_db_path(512, key0 + i));
                it
            })
            .collect();
        let id_to_idx = build_id_index(&items);
        let dir_labels = std::collections::HashMap::new();
        let dir_rank = build_dir_rank(&dir_labels);
        store_items(
            &cache,
            ItemsCacheData {
                filter_key: "{}".into(),
                order: CachedOrder::Canonical,
                data_version: 1,
                items,
                id_to_idx,
                dir_labels,
                dir_rank,
                filter: MediaFilter::default(),
                reusable: true,
                geometry_revision: 0,
                median_aspect: std::sync::OnceLock::new(),
                perm_memo: std::sync::Mutex::new(None),
                filename_rank: std::sync::OnceLock::new(),
            },
        );
        const ROW: i64 = 60;
        let rows: Vec<LayoutRow> = (0..n / ROW)
            .map(|r| LayoutRow::Normal {
                y: r as f64 * 45.0,
                height: 45.0,
                items: (0..ROW)
                    .map(|c| SlimRowItem {
                        id: r * ROW + c + 1,
                        x: c as f64 * 61.0,
                        w: 60.0,
                        h: 45.0,
                    })
                    .collect(),
            })
            .collect();
        (cache, rows)
    }

    fn serve_profile_pctl(mut v: Vec<f64>, p: f64) -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let i = ((v.len() as f64 - 1.0) * p).round() as usize;
        v[i]
    }

    #[test]
    fn fetch_io_profile_after_fix() {
        use crate::thumbnail::cache::thumb_db_path;
        use crate::thumbnail::serve::test_io::CountingIo;
        use crate::thumbnail::serve::{RealServeIo, ThumbProbeCache, THUMB_PROBE_TTL};

        const N: i64 = 240;
        let base = std::env::temp_dir().join(format!("scrollery-p15-after-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let key0 = 0x0055_0000_0000_0000i64;
        // 场景 A:仅 512 档非空(全 512 旧库);场景 B:64+512 均非空(逐项可重写到 64 档)。
        let scenarios = [("A_only512", vec![512u32]), ("B_64_512", vec![64u32, 512])];
        for (name, tiers) in &scenarios {
            for &tier in tiers {
                for i in 0..N {
                    let p = base
                        .join(name)
                        .join("thumbnails")
                        .join(thumb_db_path(tier, key0 + i));
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                    std::fs::write(&p, b"x").unwrap();
                }
            }
        }

        // 载荷:240 项(4 行 × 60 格,可视区量级),DB 路径 = 512 档。
        let (cache, rows) = serve_profile_payload(N, key0);

        for (name, tiers) in &scenarios {
            let dir = base.join(name);
            // ── 真实磁盘(本地 NTFS,OS 缓存热):1 冷批 + 30 热批 ──
            let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
            let mut times = Vec::new();
            let mut cold_ms = 0.0f64;
            // 热批分段均值(收集 / 锁外解析 / 应用):三段各自耗时,便于与旧路径逐项对比。
            let mut seg = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..31 {
                let t0 = std::time::Instant::now();
                let requests = collect_serve_requests(&cache, &rows, 1.0);
                let t1 = std::time::Instant::now();
                let serve = ThumbServe::prepare_with(
                    &RealServeIo,
                    &probes,
                    &dir,
                    1.0,
                    &requests,
                    std::time::Instant::now(),
                );
                let t2 = std::time::Instant::now();
                let _wire = hydrate_rows(&cache, rows.clone(), Some(&serve), None);
                let t3 = std::time::Instant::now();
                let dt = t3.duration_since(t0).as_secs_f64() * 1000.0;
                if i > 0 {
                    times.push(dt);
                    seg.0 += t1.duration_since(t0).as_secs_f64() * 1000.0;
                    seg.1 += t2.duration_since(t1).as_secs_f64() * 1000.0;
                    seg.2 += t3.duration_since(t2).as_secs_f64() * 1000.0;
                } else {
                    cold_ms = dt;
                }
            }
            println!(
                "[after real-fs] {}: 冷批={:.3}ms; 热批(30) p50={:.3}ms p95={:.3}ms (热批均值 收集={:.3} 解析={:.3} 应用={:.3}ms)",
                name,
                cold_ms,
                serve_profile_pctl(times.clone(), 0.50),
                serve_profile_pctl(times, 0.95),
                seg.0 / 30.0,
                seg.1 / 30.0,
                seg.2 / 30.0
            );

            // ── 慢盘模型:注入 2ms/read_dir + 1ms/stat;同一批按冷/热各测 ──
            let io = CountingIo::default();
            io.set_results(true, true);
            io.set_nonempty_tiers(tiers);
            let slow_probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
            // 此处验证 TTL 内复用；IO 耗时仍测真实时间，缓存时间固定以免慢 runner 跨过 TTL。
            let probe_now = std::time::Instant::now();
            let mut cold_counts = (0u64, 0u64, 0u64);
            let mut hot_counts = (0u64, 0u64, 0u64);
            let mut times = Vec::new();
            io.set_delays(2_000, 1_000);
            for i in 0..5 {
                io.reset_counts();
                let t0 = std::time::Instant::now();
                let requests = collect_serve_requests(&cache, &rows, 1.0);
                let serve =
                    ThumbServe::prepare_with(&io, &slow_probes, &dir, 1.0, &requests, probe_now);
                let _wire = hydrate_rows(&cache, rows.clone(), Some(&serve), None);
                let dt = t0.elapsed().as_secs_f64() * 1000.0;
                let (dirs, files) = io.counts();
                let counts = (dirs, files, io.under_lock());
                if i == 0 {
                    cold_counts = counts;
                } else {
                    hot_counts = counts;
                    times.push(dt);
                }
            }
            io.set_delays(0, 0);
            println!(
                "[after slow-disk model 2ms/read_dir + 1ms/stat] {}: 冷批 dir_reads={} stats={} lockside_io={}; 热批 dir_reads={} stats={} lockside_io={}; 热批 p50={:.1}ms p95={:.1}ms",
                name,
                cold_counts.0,
                cold_counts.1,
                cold_counts.2,
                hot_counts.0,
                hot_counts.1,
                hot_counts.2,
                serve_profile_pctl(times.clone(), 0.50),
                serve_profile_pctl(times, 0.95)
            );
            // 机制断言(与延迟无关):热批不再探目录;全程零锁内 IO。
            assert_eq!(hot_counts.0, 0, "TTL 内热批不得重探档位目录");
            assert_eq!(cold_counts.2, 0, "冷批锁内 IO 必须为 0");
            assert_eq!(hot_counts.2, 0, "热批锁内 IO 必须为 0");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **P1-5 大批量测量**:1200 可视项(20 行 × 60 格,远超常规视口)下同一三段式,真实磁盘
    /// (本地 NTFS,OS 缓存热)1 冷批 + 10 热批;慢盘数字按同批 IO 次数显式推算(模型见打印行)。
    /// 运行:`cargo test -p scrollery --lib fetch_io_profile_bulk -- --nocapture`
    #[test]
    fn fetch_io_profile_bulk_1200() {
        use crate::thumbnail::cache::thumb_db_path;
        use crate::thumbnail::serve::test_io::CountingIo;
        use crate::thumbnail::serve::{RealServeIo, ThumbProbeCache, THUMB_PROBE_TTL};

        const N: i64 = 1200;
        let dir = std::env::temp_dir().join(format!("scrollery-p15-bulk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let key0 = 0x0066_0000_0000_0000i64;
        // 64+512 档均非空(逐项可重写),文件齐备使 stat 走命中路径。
        for &tier in &[64u32, 512] {
            for i in 0..N {
                let p = dir.join("thumbnails").join(thumb_db_path(tier, key0 + i));
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, b"x").unwrap();
            }
        }
        let (cache, rows) = serve_profile_payload(N, key0);

        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let mut times = Vec::new();
        let mut cold_ms = 0.0f64;
        let mut seg = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..11 {
            let t0 = std::time::Instant::now();
            let requests = collect_serve_requests(&cache, &rows, 1.0);
            let t1 = std::time::Instant::now();
            let serve = ThumbServe::prepare_with(
                &RealServeIo,
                &probes,
                &dir,
                1.0,
                &requests,
                std::time::Instant::now(),
            );
            let t2 = std::time::Instant::now();
            let _wire = hydrate_rows(&cache, rows.clone(), Some(&serve), None);
            let t3 = std::time::Instant::now();
            let dt = t3.duration_since(t0).as_secs_f64() * 1000.0;
            if i > 0 {
                times.push(dt);
                seg.0 += t1.duration_since(t0).as_secs_f64() * 1000.0;
                seg.1 += t2.duration_since(t1).as_secs_f64() * 1000.0;
                seg.2 += t3.duration_since(t2).as_secs_f64() * 1000.0;
            } else {
                cold_ms = dt;
            }
        }
        // IO 次数(零延迟假 IO,计数与真实 FS 同形)。
        let io = CountingIo::default();
        io.set_results(true, true);
        io.set_nonempty_tiers(&[64, 512]);
        let count_probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let requests = collect_serve_requests(&cache, &rows, 1.0);
        let _ = ThumbServe::prepare_with(
            &io,
            &count_probes,
            &dir,
            1.0,
            &requests,
            std::time::Instant::now(),
        );
        let (dir_reads, stats) = io.counts();
        println!(
            "[after real-fs bulk N={}] 冷批={:.3}ms; 热批(10) p50={:.3}ms p95={:.3}ms (热批均值 收集={:.3} 解析={:.3} 应用={:.3}ms); IO/批 dir_reads={} stats={} lockside_io={}",
            N,
            cold_ms,
            serve_profile_pctl(times.clone(), 0.50),
            serve_profile_pctl(times, 0.95),
            seg.0 / 10.0,
            seg.1 / 10.0,
            seg.2 / 10.0,
            dir_reads,
            stats,
            io.under_lock()
        );
        // 模型推算(非实测):同批 IO 次数 × 慢盘参数(2ms/read_dir + 1ms/stat)。
        println!(
            "[after slow-disk model 推导 bulk N={}] 冷批 ≈ {:.1}s; 热批 ≈ {:.1}s(全在阻塞池;锁内 0 次、执行器 0 阻塞)",
            N,
            (dir_reads as f64 * 2.0 + stats as f64 * 1.0) / 1000.0,
            (stats as f64) / 1000.0
        );
        // 机制断言:热批零目录探测、全程零锁内 IO。
        assert_eq!(io.under_lock(), 0, "大批量下锁内 IO 必须为 0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 大批量下收集 + 应用(两段持锁的内存工作)必须随可视项数线性且无 IO:
    /// 1200 项的两段耗时用于和 240 项对照(见 `fetch_io_profile_after_fix` 的分段均值)。
    #[test]
    fn serve_phases_scale_linearly_with_visible_items() {
        use crate::thumbnail::serve::test_io::CountingIo;
        use crate::thumbnail::serve::{ThumbProbeCache, THUMB_PROBE_TTL};

        let key0 = 0x0077_0000_0000_0000i64;
        let (cache, rows) = serve_profile_payload(1200, key0);
        let io = CountingIo::default();
        io.set_results(false, true); // 档目录全空 → 无候选、零 stat
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let requests = collect_serve_requests(&cache, &rows, 1.0);
        assert_eq!(requests.len(), 1200, "1200 项全部进请求(status=1+非空路径)");
        let t0 = std::time::Instant::now();
        let serve = ThumbServe::prepare_with(
            &io,
            &probes,
            std::path::Path::new("C:/cache"),
            1.0,
            &requests,
            std::time::Instant::now(),
        );
        let prepare_us = t0.elapsed().as_micros();
        let t1 = std::time::Instant::now();
        let wire = hydrate_rows(&cache, rows, Some(&serve), None);
        let hydrate_us = t1.elapsed().as_micros();
        assert_eq!(io.counts().1, 0, "无候选档 → 零 stat");
        assert_eq!(io.under_lock(), 0, "解析阶段零锁内 IO");
        assert_eq!(wire.len(), 20, "20 行几何不丢");
        println!(
            "[after bulk phases N=1200] 解析(含 5 次目录探测,无 stat)={}µs; 应用(hydrate 20 行/1200 项)={}µs",
            prepare_us, hydrate_us
        );
    }
}
