//! 画廊 SQL builder:FROM/JOIN/WHERE/ORDER 主体构造与 `ViewDescriptor` 编译。
//!
//! `query_layout_items`(item_queries)与 `view_to_sql` 共用本模块,是视图定义的单一事实源
//! (T18 §4);`push_in_predicate`/`push_root_exclusion` 经 `layout` facade 具名重导出供
//! 跨模块复用。

use crate::db::models::{
    DuplicateLensDescriptor, DuplicateLensMode, MediaFilter, ViewDescriptor, ViewScope,
};
use crate::db::queries::dedup::{normalized_dir_path_sql, push_eligible_predicates};
use crate::dedup::DEDUP_HASH_VERSION;
use crate::error::{AppError, Result};

/// 构造画廊查询的「FROM/JOIN/WHERE/ORDER BY」主体（SELECT 列由调用方前置）。
///
/// `query_layout_items`（取完整 `LayoutItem`）与 `view_to_sql`（只取 `m.id`）共用本函数 —— 二者
/// 共享同一套 FROM/JOIN/WHERE/ORDER 构造，是视图定义的单一事实源（T18 §4）。
/// 全部谓词参数绑定（符合「SQL 必参数绑定」铁律）；`extras` 由调用方传入空 Vec、本函数追加。
pub(in crate::db::queries::layout) fn push_query_body(
    sql: &mut String,
    extras: &mut Vec<Box<dyn rusqlite::ToSql>>,
    filter: &MediaFilter,
    group_by: Option<&str>,
    sort_within: Option<&str>,
    sort_order: Option<&str>,
    hidden_roots: &[i64],
) {
    sql.push_str("         FROM media_items m\n         JOIN directories d ON m.directory_id = d.id\n         JOIN scan_roots r ON d.root_id = r.id");

    // image_meta 仅在按 EXIF/GPS 列过滤的搜索范围下才需要连接。
    let mut needs_meta_join = false;
    if let Some(ref q) = filter.search_query {
        if !q.trim().is_empty() {
            let scope = filter.search_scope.as_deref().unwrap_or("filename");
            if scope == "device" || scope == "location" || scope == "global" {
                needs_meta_join = true;
            }
        }
    }

    if needs_meta_join {
        sql.push_str("\n         LEFT JOIN image_meta im ON m.id = im.item_id");
    }

    if filter.ai_search == Some(true) {
        sql.push_str("\n         JOIN ai_search_results ai ON m.id = ai.file_id");
    }

    push_where_predicates(sql, extras, filter);
    // 隐藏根排除（V21）：在筛选谓词后、ORDER BY 前追加；空集不动 SQL。
    push_root_exclusion(sql, extras, hidden_roots);
    push_order_by(sql, filter, group_by, sort_within, sort_order);
}

/// 追加一条 `AND <column> IN (?a,?b,…)` 谓词并按序绑定参数，返回新的 `param_idx`。
///
/// 抽出来不是 DRY 洁癖 —— 这段的**参数序号算术是错了不报错**的那一类：占位符编号必须与
/// `extras` 的推入顺序严格对齐，错位既不 panic 也不报 SQL 错，只会**静默筛错**（SQLite 按序号
/// 取参，拿到的是隔壁谓词的值）。原本 `media_type` 一段已在画廊/搜索两处各抄一遍，S 线 P3 的
/// `file_format` 再抄两遍就是四份同构算术，迟早有一份对不齐。
///
/// 🔴 `column` **只接受代码里的字面量**，绝不可来自用户输入 —— 它是直接拼进 SQL 的（列名无法
/// 参数化）。值一律走绑定，故 `values` 是不可信输入没关系。
///
/// 空列表返回原 `param_idx` 且不动 SQL：`IN ()` 在 SQLite 里是语法错，而「该维度不限」的正确
/// 表达是**不加谓词**，不是加一个空集谓词（后者会筛出零条）。
pub(in crate::db::queries) fn push_in_predicate(
    sql: &mut String,
    extras: &mut Vec<Box<dyn rusqlite::ToSql>>,
    param_idx: usize,
    column: &str,
    values: &[String],
) -> usize {
    if values.is_empty() {
        return param_idx;
    }
    let placeholders: Vec<String> = (0..values.len())
        .map(|i| format!("?{}", param_idx + i + 1))
        .collect();
    sql.push_str(&format!(" AND {} IN ({})", column, placeholders.join(",")));
    for v in values {
        extras.push(Box::new(v.clone()));
    }
    param_idx + values.len()
}

/// 追加「隐藏根排除」谓词（V21，库级显隐）：`AND m.directory_id NOT IN (SELECT id FROM
/// directories WHERE root_id IN (...))`。**空集直接返回、不动 SQL**——无隐藏根（常态）时画廊
/// canonical 查询逐字节不变、免 JOIN 红线不触碰。
///
/// 参数序号自 `extras.len()` 起算：`push_where_predicates` 全程维持「每个绑定参数恰对一次
/// `extras.push` + 一次 `param_idx` 自增」的不变量（`param_idx == extras.len()`），故此处用当前
/// `extras.len()` 作已用参数数即续接点，无需 `push_where_predicates` 回传 param_idx。所有值走绑定
/// （root_id 是代码内查得的 i64，非用户输入，但仍一律绑定，符合「SQL 必参数绑定」铁律）。
///
/// 目录/根量级 ~10^3，内层 directories 子查询只跑一次（非相关子查询）。仅用于以 `m` 为别名的
/// 布局查询；facet/stats/搜索等无别名查询就地拼等价的 `directory_id NOT IN (...)`。
pub(in crate::db::queries) fn push_root_exclusion(
    sql: &mut String,
    extras: &mut Vec<Box<dyn rusqlite::ToSql>>,
    hidden_roots: &[i64],
) {
    if hidden_roots.is_empty() {
        return;
    }
    let base = extras.len();
    let placeholders: Vec<String> = (0..hidden_roots.len())
        .map(|i| format!("?{}", base + i + 1))
        .collect();
    sql.push_str(&format!(
        " AND m.directory_id NOT IN (SELECT id FROM directories WHERE root_id IN ({}))",
        placeholders.join(",")
    ));
    for r in hidden_roots {
        extras.push(Box::new(*r));
    }
}

/// 画廊查询的 WHERE 基底 + 全部谓词（参数绑定）。`push_query_body`（View 全量形态）与
/// `query_layout_items_canonical`（S1 基准序、免 JOIN）共用 —— 谓词单一事实源，杜绝
/// 「缓存路径与 SelectAll 路径各持一套 WHERE」漂移（T18 §4 同旨）。
/// （R-14：本段曾被 `2afe1e1` 插入的 `push_in_predicate` 顶到错误的函数头上，已归位。）
///
/// ⚠️ 不含**隐藏根排除**（V21）：那是全局显隐、非视图筛选，由调用方在本函数后调
/// [`push_root_exclusion`] 追加（canonical 路径须在 partial-index `+` 抑制**之后**再追加，
/// 保持 BROAD_BASE 检测只看筛选谓词）。
pub(in crate::db::queries::layout) fn push_where_predicates(
    sql: &mut String,
    extras: &mut Vec<Box<dyn rusqlite::ToSql>>,
    filter: &MediaFilter,
) {
    if filter.trashed_only == Some(true) {
        sql.push_str("\n         WHERE m.is_deleted=1 AND m.companion_of IS NULL");
    } else {
        sql.push_str("\n         WHERE m.is_deleted=0 AND m.companion_of IS NULL");
    }

    let mut param_idx = 0usize;

    if filter.ai_search == Some(true) {
        if let Some(threshold) = filter.ai_threshold {
            param_idx += 1;
            // Match the frontend's visual rounding (e.g. Math.round(similarity * 100))
            sql.push_str(&format!(
                " AND ROUND(ai.similarity * 100.0) >= ?{param_idx}"
            ));
            extras.push(Box::new((threshold * 100.0).round()));
        }
    }

    if let Some(dir_id) = filter.directory_id {
        param_idx += 1;
        sql.push_str(&format!(
            " AND directory_id IN (
            WITH RECURSIVE dir_tree(id) AS (
                SELECT ?{param_idx}
                UNION ALL
                SELECT d.id FROM directories d
                JOIN dir_tree t ON d.parent_id = t.id
            )
            SELECT id FROM dir_tree
        )"
        ));
        extras.push(Box::new(dir_id));
    }

    if let Some(ref types) = filter.media_types {
        param_idx = push_in_predicate(sql, extras, param_idx, "media_type", types);
    }
    // 细分格式（S 线 D-011）：与媒体大类取 **AND**（`图片 AND (PNG OR JPEG)`），维度内 OR 由 IN 表达。
    // 存的是**规范化具体扩展名**，`group`（JPEG={jpg,jpeg}）只是 UI 概念、不落库也不进谓词。
    if let Some(ref formats) = filter.file_formats {
        param_idx = push_in_predicate(sql, extras, param_idx, "file_format", formats);
    }

    if filter.favorited_only == Some(true) {
        sql.push_str(" AND is_favorited=1");
    }

    // 用户收藏夹：限定为其 album_items 成员。（系统夹用上面的 media_types + favorited_only，不设 album_id。）
    if let Some(album_id) = filter.album_id {
        param_idx += 1;
        sql.push_str(&format!(
            " AND m.id IN (SELECT item_id FROM album_items WHERE album_id = ?{param_idx})"
        ));
        extras.push(Box::new(album_id));
    }

    // 人物（F6 人物墙 → 某人物的照片）：包含此簇人脸的图像。
    if let Some(person_id) = filter.person_id {
        param_idx += 1;
        sql.push_str(&format!(
            " AND m.id IN (SELECT item_id FROM faces WHERE person_id = ?{param_idx})"
        ));
        extras.push(Box::new(person_id));
    }

    if let Some(min_r) = filter.min_rating {
        param_idx += 1;
        sql.push_str(&format!(" AND rating >= ?{param_idx}"));
        extras.push(Box::new(min_r));
    }

    // 颜色标签：精确匹配某色档（min_rating 是 >=，颜色无序故取等值 =）。
    if let Some(cl) = filter.color_label {
        param_idx += 1;
        sql.push_str(&format!(" AND color_label = ?{param_idx}"));
        extras.push(Box::new(cl));
    }

    if let Some(ref dr) = filter.date_range {
        param_idx += 1;
        sql.push_str(&format!(" AND sort_datetime >= ?{param_idx}"));
        extras.push(Box::new(dr.from));
        param_idx += 1;
        sql.push_str(&format!(" AND sort_datetime <= ?{param_idx}"));
        extras.push(Box::new(dr.to));
    }

    if filter.live_photo_only == Some(true) {
        sql.push_str(" AND m.is_live_photo=1");
    }

    if filter.recent_only == Some(true) {
        sql.push_str(" AND m.created_at >= strftime('%s', 'now', '-30 days')");
    }

    if let Some(ref q) = filter.search_query {
        if !q.trim().is_empty() {
            let scope = filter.search_scope.as_deref().unwrap_or("filename");
            // LIKE 通配转义(2026-07-06 审查 R19):用户输入里的 % / _ 是 LIKE 元字符,不转义则
            // 搜 "IMG_20"(下划线在文件名极常见)会把 _ 当「任意单字符」→ 过度匹配。先转义
            // \ % _ 再包 %…%,并给每个 LIKE 加 ESCAPE '\'。注入无风险(参数绑定),这是匹配正确性。
            let escaped = q
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{}%", escaped);
            match scope {
                "folder" => {
                    param_idx += 1;
                    let p1 = format!("?{}", param_idx);
                    param_idx += 1;
                    let p2 = format!("?{}", param_idx);
                    sql.push_str(&format!(
                        " AND (d.rel_path LIKE {} ESCAPE '\\' OR d.name LIKE {} ESCAPE '\\')",
                        p1, p2
                    ));
                    extras.push(Box::new(pattern.clone()));
                    extras.push(Box::new(pattern));
                }
                "date" => {
                    param_idx += 1;
                    sql.push_str(&format!(" AND strftime('%Y-%m-%d %H:%M:%S', m.sort_datetime, 'unixepoch', 'localtime') LIKE ?{} ESCAPE '\\'", param_idx));
                    extras.push(Box::new(pattern));
                }
                "device" => {
                    param_idx += 1;
                    let p1 = format!("?{}", param_idx);
                    param_idx += 1;
                    let p2 = format!("?{}", param_idx);
                    param_idx += 1;
                    let p3 = format!("?{}", param_idx);
                    sql.push_str(&format!(" AND (im.exif_make LIKE {} ESCAPE '\\' OR im.exif_model LIKE {} ESCAPE '\\' OR im.exif_lens LIKE {} ESCAPE '\\')", p1, p2, p3));
                    extras.push(Box::new(pattern.clone()));
                    extras.push(Box::new(pattern.clone()));
                    extras.push(Box::new(pattern));
                }
                "location" => {
                    param_idx += 1;
                    let p1 = format!("?{}", param_idx);
                    param_idx += 1;
                    let p2 = format!("?{}", param_idx);
                    // 预留经纬度字符串或未来 city 字段匹配
                    sql.push_str(&format!(" AND (CAST(im.exif_gps_lat AS TEXT) LIKE {} ESCAPE '\\' OR CAST(im.exif_gps_lng AS TEXT) LIKE {} ESCAPE '\\')", p1, p2));
                    extras.push(Box::new(pattern.clone()));
                    extras.push(Box::new(pattern));
                }
                "global" => {
                    let mut p = vec![];
                    for _ in 0..9 {
                        param_idx += 1;
                        p.push(format!("?{}", param_idx));
                        extras.push(Box::new(pattern.clone()));
                    }
                    sql.push_str(&format!(
                        " AND (m.file_name LIKE {} ESCAPE '\\' OR d.rel_path LIKE {} ESCAPE '\\' OR d.name LIKE {} ESCAPE '\\' OR strftime('%Y-%m-%d %H:%M:%S', m.sort_datetime, 'unixepoch', 'localtime') LIKE {} ESCAPE '\\' OR im.exif_make LIKE {} ESCAPE '\\' OR im.exif_model LIKE {} ESCAPE '\\' OR im.exif_lens LIKE {} ESCAPE '\\' OR CAST(im.exif_gps_lat AS TEXT) LIKE {} ESCAPE '\\' OR CAST(im.exif_gps_lng AS TEXT) LIKE {} ESCAPE '\\')",
                        p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8]
                    ));
                }
                _ => {
                    // "filename"
                    param_idx += 1;
                    sql.push_str(&format!(" AND m.file_name LIKE ?{} ESCAPE '\\'", param_idx));
                    extras.push(Box::new(pattern));
                }
            }
        }
    }
}

/// 画廊查询的 ORDER BY（View 全量形态：folder 轴目录序 = 前序 DFS
/// `(r.created_at, r.id, d.tree_sort_key, d.id)`，特殊排序含 NATURAL_CMP/similarity；
/// 全部分支末尾追加 m.id 同向 tiebreaker）。S1 canonical 缓存路径不走本函数（恒 (sort_datetime, id)
/// 基准序，分组/方向由 items_cache::derive_order 内存派生，与本函数产出的 SQL 序逐项等价——
/// 对拍测试锁定；SQL 读持久列 d.tree_sort_key、内存算 encode_tree_sort_key，同一键逻辑（方案 B）
/// 使等价结构成立而非巧合）。
fn push_order_by(
    sql: &mut String,
    filter: &MediaFilter,
    group_by: Option<&str>,
    sort_within: Option<&str>,
    sort_order: Option<&str>,
) {
    let order_dir = match sort_order {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    if group_by == Some("folder") {
        // folder 目录序 = 前序 DFS：root 序 (created_at, id) 最高位使不同扫描根整棵子树连续，
        // 其次 d.tree_sort_key（方案 B 持久列）给出 root 内 DFS 序，d.id 稳定裁决退化并发；目录序
        // 恒 ASC（方向只作用于组内媒体次键）。改读持久列（原每行调 TREE_SORT_KEY(rel_path) 标量
        // 函数，列参数不可折叠 → 逐行 SQLite→Rust FFI + Vec<u8> 分配）省掉每行 FFI 与分配；键值与
        // 内存 build_dir_rank 的 encode_tree_sort_key 逐位同构（BLOB memcmp = Vec<u8>::cmp），
        // 刚性等价契约不变（写路径 upsert/move 保证列与 rel_path 一致）。
        let dir_order = "r.created_at ASC, r.id ASC, d.tree_sort_key ASC, d.id ASC";
        if sort_within == Some("similarity") && filter.ai_search == Some(true) {
            sql.push_str(&format!(" ORDER BY {dir_order}, ai.similarity {order_dir}"));
        } else if sort_within == Some("filename") {
            sql.push_str(&format!(
                " ORDER BY {dir_order}, m.file_name COLLATE NATURAL_CMP {order_dir}"
            ));
        } else {
            sql.push_str(&format!(
                " ORDER BY {dir_order}, m.sort_datetime {order_dir}"
            ));
        }
    } else if group_by == Some("date") {
        // B-file-iii / D-018 A′：去 `'localtime'` 改 **UTC 日界**。`sort_datetime` 是 EXIF 墙钟当 UTC 存
        // （`scanner/metadata.rs::parse_exif_datetime` 忽略时区），故 UTC `date()` 才与显示标签
        // （`justified.rs::timestamp_to_date_label`）、月桶、date+datetime 分组（`div_euclid(86400)`）一致；
        // 原 `'localtime'` 在墙钟上再叠一次本地时区 = 双重偏移 bug（UTC+8 机器傍晚照片错分次日）。此桶与
        // 内存 `derive_order` date+filename 的 `sort_datetime.div_euclid(86400)` 逐值等价（SelectAll 刚性对拍）。
        let date_expr = "date(m.sort_datetime, 'unixepoch')";
        if sort_within == Some("similarity") && filter.ai_search == Some(true) {
            sql.push_str(&format!(
                " ORDER BY {} {}, ai.similarity {}",
                date_expr, order_dir, order_dir
            ));
        } else if sort_within == Some("filename") {
            sql.push_str(&format!(
                " ORDER BY {} {}, m.file_name COLLATE NATURAL_CMP {}",
                date_expr, order_dir, order_dir
            ));
        } else {
            sql.push_str(&format!(" ORDER BY m.sort_datetime {}", order_dir));
        }
    } else {
        if sort_within == Some("similarity") && filter.ai_search == Some(true) {
            sql.push_str(&format!(" ORDER BY ai.similarity {}", order_dir));
        } else if sort_within == Some("filename") {
            sql.push_str(&format!(
                " ORDER BY m.file_name COLLATE NATURAL_CMP {}",
                order_dir
            ));
        } else {
            sql.push_str(&format!(" ORDER BY m.sort_datetime {}", order_dir));
        }
    }

    // 确定性 tiebreaker：上面所有分支的末键都按 order_dir 排，统一追加 m.id 同向次键——
    // 同秒/同名/同相似度时稳定序（消除布局抖动），并让默认 sort_datetime DESC 分支吃满
    // 复合索引 idx_media_sort(sort_datetime DESC, id DESC)。索引≠tiebreaker，两处都改（§3.5）。
    sql.push_str(&format!(", m.id {}", order_dir));
}

/// 把 `ViewDescriptor` 编译为「只取 `m.id`」的 SQL + 绑定参数，与 `query_layout_items` 共用
/// `push_query_body` 主体构造（单一事实源）。供 `resolve_selection` / `count_selection` /
/// `get_view_ids` 按当前 filter 在 SQL 层取全集 id，**不把百万 id 灌前端 / 不经 IPC 整包传**。
///
/// `SemanticSearch` scope 是有序、非纯 SQL 的例外（v1 由 ai_search 既有路径承载），此处显式不支持。
/// `hidden_roots`（V21）：隐藏根 id 集，由调用方（`resolve_selection` / `count_selection`，均持
/// conn）经 `hidden_root_ids(conn)` 取入——本函数无 conn 故不自取。使 Ctrl+A 全选/计数与画廊可见
/// 集一致（隐藏根媒体不入选）。空集不动 SQL。
pub fn view_to_sql(
    view: &ViewDescriptor,
    hidden_roots: &[i64],
) -> Result<(String, Vec<Box<dyn rusqlite::ToSql>>)> {
    if let ViewScope::SemanticSearch { .. } = view.scope {
        return Err(AppError::Internal(
            "view_to_sql: SemanticSearch scope 不支持纯 SQL 解析（v1 走 ai_search 路径）".into(),
        ));
    }
    // 重复镜头（方案 §10.2/§13 P2）：先按 MVP 规则校验（scope/filter/showUnique/
    // orderingVersion），再走镜头 lowering（groups 模式）——与 layout `assemble_lens_groups`
    // 同契约定序，SelectAll/GET_VIEW_IDS 解析的集合与镜头画面同源；序列逐位 parity 由
    // `tests/lens_lowering.rs` 锁定，若 SQL ORDER 与内存排序任一键漂移在此显式失败。
    if let Some(lens) = &view.duplicate_lens {
        lens.validate(&view.scope, &view.filter)?;
        return build_duplicate_lens_sql(*lens);
    }
    let filter = view.to_media_filter();
    let mut sql = String::from("SELECT m.id ");
    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    push_query_body(
        &mut sql,
        &mut extras,
        &filter,
        Some(view.sort.group_by.as_str()),
        Some(view.sort.sort_within_group.as_str()),
        Some(view.sort.sort_order.as_str()),
        hidden_roots,
    );
    Ok((sql, extras))
}

/// 重复镜头（groups 模式）的 `view_to_sql` lowering（2026-09-02 主画廊重复项浏览方案
/// §10.2「后端 `view_to_sql`、`GET_VIEW_IDS` 和所有 SelectAll 解析必须与 layout 使用
/// 同一 lowering」）。SQL 内序与 layout `assemble_lens_groups` 的内存组装序**同契约定序**：
///
/// - 组序（§6.3）：组内最新 `sort_datetime` DESC（窗口 MAX `group_latest`）→
///   `(unit_digest,unit_size)` 字节 ASC（BLOB memcmp = `Vec<u8>::cmp`）；
/// - 组内（§6.3）：`sort_datetime` DESC → `normalized_dir_path` ASC（BINARY = UTF-8
///   字节序 = `String::cmp`）→ `file_name COLLATE NOCASE` ASC（ASCII 大写折叠 =
///   `ascii_nocase_cmp`）→ `item_id` ASC。
///
/// 路径表达式与 eligible 谓词分别复用 dedup 的 [`normalized_dir_path_sql`] /
/// [`push_eligible_predicates`] 单一事实源（与 `list_duplicate_lens_members` 逐字符同源）；
/// 参数契约一致：?1 = `DEDUP_HASH_VERSION`、?2 = include_offline(false)、
/// ?3 = include_zero_byte(false)。镜头排序由 `orderingVersion` 契约固定，描述符的
/// `sort` 不参与本分支（`show_unique_items` 在 groups 恒 false 已由 validate 保证）。
///
/// `hidden_roots` 不在本分支拼接：eligible 的 `r.is_hidden=0` 与 `hidden_root_ids()`
/// （`SELECT id FROM scan_roots WHERE is_hidden=1`）是**同一事实源** `scan_roots.is_hidden`
/// 的正反两面，隐藏根排除已内含；`push_root_exclusion` 是普通画廊（别名 `m`）对同一机制
/// 的 SQL 侧表达，叠加只会重复排除同一批根。入参保留以维持 `view_to_sql` 统一签名
/// （语义叠加关系：镜头集 ⊆ 可见根媒体，与普通 All 视图对隐藏根的结果语义一致）。
fn build_duplicate_lens_sql(
    lens: DuplicateLensDescriptor,
) -> Result<(String, Vec<Box<dyn rusqlite::ToSql>>)> {
    if lens.mode == DuplicateLensMode::Folders {
        // folders 排列（关联文件夹簇 / 独有项 / 文件夹轴）属 P3（方案 §13）：
        // 显式拒绝而非静默按 groups 降维——静默降维会让画面与选择契约漂移。
        return Err(AppError::DuplicateLensUnsupported);
    }
    let mut sql = String::from(
        // 外层列名 `id`（别名）与普通路径 `SELECT m.id` 一致——`count_selection` 把
        // view_to_sql 产物包成 `… FROM ({sql}) WHERE id IN (…)`，列名漂移会静默炸掉交集计数。
        "SELECT item_id AS id FROM (
            SELECT di.item_id AS item_id,
                   di.unit_digest,
                   di.unit_size,
                   mi.sort_datetime,
                   ",
    );
    sql.push_str(normalized_dir_path_sql());
    sql.push_str(
        " AS normalized_dir_path,
                   mi.file_name,
                   mi.directory_id,
                   COUNT(*) OVER (PARTITION BY di.unit_digest, di.unit_size) AS member_count,
                   MAX(mi.sort_datetime) OVER (PARTITION BY di.unit_digest, di.unit_size)
                       AS group_latest
              FROM dedup_index di
              JOIN media_items mi ON mi.id = di.item_id
              JOIN directories d ON d.id = mi.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE ",
    );
    push_eligible_predicates(&mut sql);
    sql.push_str(
        "\n        ) WHERE member_count >= 2
         ORDER BY group_latest DESC, unit_digest ASC, unit_size ASC,
                  sort_datetime DESC, normalized_dir_path ASC,
                  file_name COLLATE NOCASE ASC, item_id ASC",
    );
    Ok((
        sql,
        vec![
            Box::new(DEDUP_HASH_VERSION as i64),
            Box::new(false),
            Box::new(false),
        ],
    ))
}
