//! 布局项查询:`LayoutItem` 行映射器 + 画廊/canonical/filename 基准序查询。
//!
//! `map_layout_item` 的 `row.get(N)` 位置与 `query_layout_items`/`canonical_layout_sql`/
//! `query_layout_items_filename_baseline` 三处 SELECT 列表逐位对齐,是无编译期保障的裸约定,
//! 故四者必须同文件(改列必同改四处)。WHERE/FROM 谓词经 `query_builder` 单向依赖复用。

use rusqlite::{Connection, Row};

use crate::db::models::{LayoutItem, MediaFilter};
use crate::error::{AppError, Result};

use super::query_builder::{push_query_body, push_root_exclusion, push_where_predicates};

// ── 行映射器 ──────────────────────────────────────────────────────────────

pub(in crate::db::queries::layout) fn map_layout_item(
    row: &Row<'_>,
) -> rusqlite::Result<LayoutItem> {
    Ok(LayoutItem {
        id: row.get(0)?,
        width: row.get(1)?,
        height: row.get(2)?,
        file_size: row.get(3)?,
        sort_datetime: row.get(4)?,
        file_format: row.get(5)?,
        media_type: row.get(6)?,
        is_live_photo: row.get::<_, i64>(7)? != 0,
        duration_ms: row.get(8)?,
        thumb_status: row.get(9)?,
        thumb_path: row.get(10)?,
        thumbhash: row.get(11)?,
        is_favorited: row.get::<_, i64>(12)? != 0,
        // S2 消脂：dir_path/dir_name 不再逐行 SELECT（标签经 query_dir_labels 映射还原），
        // 列位自 13 起整体前移 2 —— 与 query_layout_items 的 SELECT 顺序严格对齐，
        // 错位会静默串列（有 query_layout_items_maps_scalar_columns_by_position 锁位）。
        dir_id: row.get(13)?,
        availability: row.get(14)?,
        rating: row.get(15)?,
        color_label: row.get(16)?,
        similarity: row.get(17)?,
        // multi-tier serving(2026-08-16 阶段 2):档位路径确定性输入。与三处 SELECT 尾列对齐。
        cache_key: row.get(18)?,
    })
}

/// 查询与给定过滤器匹配的所有布局项。
/// 被 `compute_layout` 使用。
pub fn query_layout_items(
    conn: &Connection,
    filter: &MediaFilter,
    group_by: Option<&str>,
    sort_within: Option<&str>,
    sort_order: Option<&str>,
    _include_meta: bool, // retained for call-site compatibility; EXIF is no longer selected here
) -> Result<Vec<LayoutItem>> {
    let mut sql = String::from(
        "SELECT m.id, m.width, m.height, m.file_size, m.sort_datetime, m.file_format, m.media_type, m.is_live_photo,
                m.duration_ms, m.thumb_status, m.thumb_path, m.thumbhash, m.is_favorited,
                m.directory_id as dir_id, m.availability, m.rating, m.color_label, "
    );

    // similarity 是最后一个 SELECT 列 — 重型 EXIF/GPS/文件名列不再在此查询，
    // 改为经 get_meta_for_viewport 按需拉取。
    if filter.ai_search == Some(true) {
        sql.push_str("ai.similarity, m.cache_key\n");
    } else {
        sql.push_str("NULL as similarity, m.cache_key\n");
    }

    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    // 隐藏根排除（V21）：有 conn 处一次取隐藏集（不经 MediaFilter，使全局 filename rank 构建等
    // 所有走本查询的路径都自动吃到排除）；空集时 push_query_body 内 push_root_exclusion 不动 SQL。
    let hidden_roots = super::super::scan::hidden_root_ids(conn)?;
    // FROM/JOIN/WHERE/ORDER 主体抽到 push_query_body，与 view_to_sql（只取 id）共用同一构造，
    // 杜绝双套视图定义漂移（T18 §3.10.2 单一事实源）。
    push_query_body(
        &mut sql,
        &mut extras,
        filter,
        group_by,
        sort_within,
        sort_order,
        &hidden_roots,
    );

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = extras.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_layout_item)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// S1 取数缓存的「基准序」查询。与 `query_layout_items` 的差异（消脂 + 恒序）：
/// - SELECT 列序与 `map_layout_item` 完全一致（similarity 恒 NULL —— ai 视图不走本函数）；
/// - FROM 免 directories/scan_roots JOIN（1M 行 DB 副本实测占查询成本 2/3），仅当搜索
///   scope 的谓词引用 d.* / image_meta 列时按需补 JOIN；
/// - WHERE 经 `push_where_predicates` 与 View 路径同源（谓词单一事实源）；
/// - 基准序 `(m.sort_datetime DESC, m.id DESC)` 由内存置换排序产出（S3.7，SQL 不再下发
///   ORDER BY——索引序遍历逐行随机回表是冷启动大查询的病根，见 canonical_layout_sql）；
///   分组轴/方向由 `layout::items_cache::derive_order` 内存派生，与 `view_to_sql` 的
///   SQL 序**逐项等价**（SelectAll/flat_ids 契约，对拍测试锁定）。
///
/// 组装 canonical 布局查询 SQL（S3.7 抽出，供计划锁定测试对同一份 SQL 做 EXPLAIN）。
pub(in crate::db::queries::layout) fn canonical_layout_sql(
    filter: &MediaFilter,
    hidden_roots: &[i64],
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut sql = String::from(
        "SELECT m.id, m.width, m.height, m.file_size, m.sort_datetime, m.file_format, m.media_type, m.is_live_photo,
                m.duration_ms, m.thumb_status, m.thumb_path, m.thumbhash, m.is_favorited,
                m.directory_id as dir_id, m.availability, m.rating, m.color_label, NULL as similarity, m.cache_key
         FROM media_items m",
    );
    let (needs_dir_join, needs_meta_join) = search_join_needs(filter);
    if needs_dir_join {
        sql.push_str("\n         JOIN directories d ON m.directory_id = d.id");
    }
    if needs_meta_join {
        sql.push_str("\n         LEFT JOIN image_meta im ON m.id = im.item_id");
    }
    // 防御：ai 阈值谓词引用 ai 别名。调用方（compute_layout）对 ai 视图绕过本函数不缓存，
    // 但保持 SQL 自洽以免误用时报「no such column」。
    if filter.ai_search == Some(true) {
        sql.push_str("\n         JOIN ai_search_results ai ON m.id = ai.file_id");
    }

    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    push_where_predicates(&mut sql, &mut extras, filter);
    // S3.7：不下发 ORDER BY，且默认全量视图（基础谓词外无任何附加谓词）以 unary `+`
    // 压制 partial index 匹配——EXPLAIN 实证：仅去 ORDER BY 规划器仍选 idx_media_sort
    // （查询谓词与其部分索引 WHERE 逐字匹配），索引序遍历逐行随机回表，1M 库在表体积
    // 随缩略图/富化回填变胖后实测 6.6s（页缓存 64MB 全程颠簸）。压制后退化为顺序全表
    // 扫（每页只读一遍）；选择性视图（目录/收藏/类型等）不命中本后缀，各自索引照常
    // （计划锁定测试在环）。基准序由内存置换排序补齐，与原 SQL 序逐项等价。
    const BROAD_BASE: &str = "WHERE m.is_deleted=0 AND m.companion_of IS NULL";
    if sql.ends_with(BROAD_BASE) {
        let base_at = sql.len() - BROAD_BASE.len();
        sql.truncate(base_at);
        sql.push_str("WHERE +m.is_deleted=0 AND +m.companion_of IS NULL");
    }
    // 隐藏根排除（V21）在 `+` 抑制**之后**追加：BROAD_BASE 检测只看筛选谓词（免受排除子句干扰），
    // 广域默认视图 + 有隐藏根时仍施 `+` 抑制（保顺序全表扫，不回退 idx_media_sort 随机回表），
    // 排除作为普通选择性谓词叠在其后。空集不动 SQL/extras，canonical 与今日逐字节一致。
    push_root_exclusion(&mut sql, &mut extras, hidden_roots);
    (sql, extras)
}

/// S1 基准序查询本体：SQL 组装见 [`canonical_layout_sql`]，序由 [`sort_canonical`] 补齐。
pub fn query_layout_items_canonical(
    conn: &Connection,
    filter: &MediaFilter,
) -> Result<Vec<LayoutItem>> {
    let hidden_roots = super::super::scan::hidden_root_ids(conn)?;
    let (sql, extras) = canonical_layout_sql(filter, &hidden_roots);
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = extras.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_layout_item)?;
    let items: Vec<LayoutItem> = rows
        .map(|r| r.map_err(AppError::from))
        .collect::<Result<_>>()?;
    Ok(sort_canonical(items))
}

/// B-file-i 的 **filename 基准序**查询：与 [`query_layout_items_canonical`] 同构(共用
/// [`canonical_layout_sql`] 的 SELECT/FROM/WHERE 单一事实源),唯一差异 = 在 SQL 侧下发
/// `ORDER BY m.file_name COLLATE NATURAL_CMP ASC, m.id ASC`,让**行位置即 `filename_rank`**
/// (返回向量第 i 项的自然序位次恒为 i)。
///
/// 为何 SQL 赋 rank 而非 Rust:`LayoutItem` **不含 `file_name`**(重列早已移出布局查询,按需
/// 经 get_meta_for_viewport 拉),内存无从跑 `natural_cmp`;且这一次 NATURAL_CMP filesort 本就是
/// 「每 filter / data_version 只做一次」的不可免基准(设计 §14.1)。此后 none/folder 轴与
/// asc/desc 方向全部由 [`crate::layout::items_cache::derive_order`] 在**整数下标**上内存派生,
/// 与 datetime 家族对称——把 filename 的轴/方向切换从「每次全表 SQL filesort」降为亚秒内存派生。
///
/// **none/folder/date 三轴皆可派生**(B-file-iii/D-018 A′):date+filename 用 **UTC 日桶**
/// `sort_datetime.div_euclid(86400)` 内存分桶(每项自带 `sort_datetime`),与 SQL `push_order_by`
/// (date+filename 已去 `'localtime'` 改 UTC 日界)逐值等价,无需 SQL 日期列、无对齐风险——故本基准
/// 查询同时服务 date+filename(见 compute_layout 分派 + `is_hit_valid` + `can_derive_axis`)。
pub fn query_layout_items_filename_baseline(
    conn: &Connection,
    filter: &MediaFilter,
) -> Result<Vec<LayoutItem>> {
    let hidden_roots = super::super::scan::hidden_root_ids(conn)?;
    let (mut sql, extras) = canonical_layout_sql(filter, &hidden_roots);
    // canonical_layout_sql 末尾对全量视图施了一元 `+` 压制 partial index(datetime 用顺序全表扫);
    // 对 filename 基准无害——本就要全表 + NATURAL_CMP filesort,顺序扫同样合适。追加 ORDER BY 即得
    // 自然序基准(id ASC 作同名稳定次键,与 push_order_by filename 分支的 `m.id` tiebreaker 同向)。
    sql.push_str("\n         ORDER BY m.file_name COLLATE NATURAL_CMP ASC, m.id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = extras.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_layout_item)?;
    // SQL 已产出 filename 基准序,直接 collect(不再 sort_canonical——那是 datetime 基准的内存补序)。
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// **双键统一缓存**的惰性 `filename_rank` 数据源:只取 `m.id`、按 filename 自然序返回,供
/// datetime 基准缓存在用户首次切到 filename 序时，把「全局 NATURAL_CMP 位次 → id」映射回缓存
/// items 建 [`crate::layout::items_cache::ItemsCacheData::filename_rank`]。相较整行重查
/// ([`query_layout_items_filename_baseline`],18 列物化)仅取 id、约省一半——一次性成本，此后
/// datetime↔filename 互切全内存派生。
///
/// FROM/WHERE **成员判据**与 [`canonical_layout_sql`] 同源（共用 `search_join_needs` +
/// `push_where_predicates`，故「哪些行属于本视图」的单一事实源不漂移，仅 SELECT/ORDER 不同）。
/// 不施 canonical 的一元 `+` partial-index 压制：仅取 id 无回表，NATURAL_CMP 是自定义 collation
/// 无索引可用、必 filesort，压制无意义。
pub fn query_item_ids_filename_order(conn: &Connection, filter: &MediaFilter) -> Result<Vec<i64>> {
    let mut sql = String::from("SELECT m.id\n         FROM media_items m");
    let (needs_dir_join, needs_meta_join) = search_join_needs(filter);
    if needs_dir_join {
        sql.push_str("\n         JOIN directories d ON m.directory_id = d.id");
    }
    if needs_meta_join {
        sql.push_str("\n         LEFT JOIN image_meta im ON m.id = im.item_id");
    }
    // ai 视图不走双键缓存(reusable=false 不作命中源),但保持 SQL 自洽以免误用时报「no such column」。
    if filter.ai_search == Some(true) {
        sql.push_str("\n         JOIN ai_search_results ai ON m.id = ai.file_id");
    }
    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    push_where_predicates(&mut sql, &mut extras, filter);
    // 隐藏根排除（V21）：双键统一缓存的 filename rank 源同样须与画廊集合一致，否则隐藏后
    // datetime↔filename 互切会带回隐藏项的位次。空集不动 SQL。
    push_root_exclusion(
        &mut sql,
        &mut extras,
        &super::super::scan::hidden_root_ids(conn)?,
    );
    // 与 query_layout_items_filename_baseline / push_order_by filename 分支同向(id ASC 稳定次键)。
    sql.push_str("\n         ORDER BY m.file_name COLLATE NATURAL_CMP ASC, m.id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = extras.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), |r| r.get::<_, i64>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 基准序内存排序（S3.7）：`(sort_datetime DESC, id DESC)`——与被替换的 SQL ORDER 逐项
/// 等价（两键均 NOT NULL 整数，i64 比较与 SQLite INTEGER 排序一致）。装饰-排序-还原：
/// 紧凑键元组排序 + 置换还原，避免直接搬动 ~200B 大结构体（同 S1.1 DSU 手法）。
/// 1M 项实测 ~100ms 量级，换掉的是数秒级随机回表。
pub(in crate::db::queries::layout) fn sort_canonical(items: Vec<LayoutItem>) -> Vec<LayoutItem> {
    let mut keys: Vec<(i64, i64, usize)> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.sort_datetime, it.id, i))
        .collect();
    keys.sort_unstable_by_key(|&(ts, id, _)| (std::cmp::Reverse(ts), std::cmp::Reverse(id)));
    let mut slots: Vec<Option<LayoutItem>> = items.into_iter().map(Some).collect();
    keys.iter()
        .map(|&(_, _, i)| slots[i].take().expect("置换下标唯一"))
        .collect()
}

/// 搜索 scope 的 JOIN 需求：`(directories, image_meta)`。folder/global 谓词引用
/// d.rel_path/d.name；device/location/global 引用 image_meta 列（与 push_query_body 的
/// needs_meta_join 判定同源语义）。
fn search_join_needs(filter: &MediaFilter) -> (bool, bool) {
    let Some(q) = filter.search_query.as_ref() else {
        return (false, false);
    };
    if q.trim().is_empty() {
        return (false, false);
    }
    let scope = filter.search_scope.as_deref().unwrap_or("filename");
    (
        matches!(scope, "folder" | "global"),
        matches!(scope, "device" | "location" | "global"),
    )
}
