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

    // ── P1-5:滚动取行出口的 IO/锁语义(收集 → 阻塞解析 → 应用)────────────────────

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
