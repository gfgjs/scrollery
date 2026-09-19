//! `view_to_sql` / `push_in_predicate` / `ViewDescriptor::to_media_filter` 测试。

use super::*;
use crate::db::models::{DateRange, GalleryFilter, SortSpec, ViewDescriptor, ViewScope};

// 跨域测试定向 import(§5 规则 2:只补编译所需;裸名原经 facade glob 解析)。
use super::super::super::collections::{
    create_collection, delete_collection, list_collections, list_deleted_collections,
    rename_collection, restore_collection,
};
use super::super::super::faces::{list_ignored_persons, list_persons};
use super::super::query_builder::push_in_predicate;

/// 便捷构造：给定 scope + filter，默认排序与 layout_version。
fn view(scope: ViewScope, filter: GalleryFilter) -> ViewDescriptor {
    ViewDescriptor {
        scope,
        filter,
        sort: SortSpec::default(),
        duplicate_lens: None,
        layout_version: 0,
    }
}

/// 测试便捷包装(V21):本模块用例验的是**无隐藏根**时的 SQL 形态,统一走空隐藏集。以本地
/// 同名 fn 遮蔽 `use super::*` 的 glob 导入(Rust:显式项优先于 glob,无冲突),使既有 9 处
/// `view_to_sql(&view(...))` 调用点零改动。验隐藏排除的用例显式调 `super::super::view_to_sql(v, &hidden)`。
fn view_to_sql(view: &ViewDescriptor) -> Result<(String, Vec<Box<dyn rusqlite::ToSql>>)> {
    super::super::view_to_sql(view, &[])
}

#[test]
fn all_scope_compiles_base_predicate_and_tiebreaker() {
    let (sql, params) = view_to_sql(&view(ViewScope::All, GalleryFilter::default())).unwrap();
    assert!(sql.starts_with("SELECT m.id "), "只取 id 的孪生 SELECT");
    assert!(
        sql.contains("WHERE m.is_deleted=0 AND m.companion_of IS NULL"),
        "All 基础谓词"
    );
    // 默认 group_by=date / sort=desc → 末键统一追加 m.id 同向次键（确定性 tiebreaker）。
    assert!(sql.trim_end().ends_with(", m.id DESC"), "确定性 tiebreaker");
    assert_eq!(params.len(), 0, "无附加筛选 → 零绑定参数");
}

#[test]
fn directory_scope_uses_recursive_subtree() {
    let (sql, params) = view_to_sql(&view(
        ViewScope::Directory { directory_id: 42 },
        GalleryFilter::default(),
    ))
    .unwrap();
    assert!(
        sql.contains("WITH RECURSIVE dir_tree"),
        "目录 scope 走递归子树（复用既有 CTE）"
    );
    assert_eq!(params.len(), 1, "directory_id 一个绑定参数");
}

#[test]
fn collection_scope_restricts_to_album_items() {
    let (sql, params) = view_to_sql(&view(
        ViewScope::Collection { album_id: 7 },
        GalleryFilter::default(),
    ))
    .unwrap();
    assert!(sql.contains("SELECT item_id FROM album_items WHERE album_id ="));
    assert_eq!(params.len(), 1);
}

/// 集合重命名（T21）：用户夹可改名；系统夹受 `kind='user'` 守卫保护、为空操作。
#[test]
fn rename_collection_user_only() {
    // 该测试模块无 DB helper（纯 SQL-string 测试），自建带 migrations 的内存库。
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();
    let id = create_collection(&c, "旧名", None).unwrap();
    rename_collection(&c, id, "新名").unwrap();
    let name: String = c
        .query_row("SELECT name FROM albums WHERE id=?1", params![id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(name, "新名");

    // 系统夹（kind='system'）不应被改名（守卫 kind='user'）。
    c.execute(
        "INSERT INTO albums (name, kind) VALUES ('系统夹', 'system')",
        [],
    )
    .unwrap();
    let sys_id = c.last_insert_rowid();
    rename_collection(&c, sys_id, "被改了吗").unwrap();
    let sys_name: String = c
        .query_row(
            "SELECT name FROM albums WHERE id=?1",
            params![sys_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sys_name, "系统夹", "系统夹不应被重命名");
}

/// 收藏夹软删除（可撤销删除，S5 阶段 11）：删除置 `deleted_at` 使夹从列表消失但**行与成员
/// 保留**，restore 清零后重现；系统夹受 `kind='user'` 守卫保护、删除为空操作。
#[test]
fn soft_delete_and_restore_collection() {
    // 该测试模块无 DB helper；自建带 migrations 的内存库（裸连接默认 foreign_keys=OFF，
    // 故可直接插 album_items 行、item_id 无需真 media_items 即可验证成员保留）。
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();

    let has = |id: i64| list_collections(&c).unwrap().iter().any(|col| col.id == id);
    let deleted_at = |id: i64| -> Option<i64> {
        c.query_row(
            "SELECT deleted_at FROM albums WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    };

    let id = create_collection(&c, "度假", None).unwrap();
    // 给夹加一个成员（直接插 album_items，验证软删除后成员不被级联清除）。initialize_schema 后
    // foreign_keys=ON，故照本仓迁移测试 idiom 关 FK，免为一个 orphan item_id 铺全 directories→
    // media_items 链。
    c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    c.execute(
        "INSERT INTO album_items (album_id, item_id) VALUES (?1, 12345)",
        params![id],
    )
    .unwrap();
    assert!(has(id), "新建的用户夹应出现在列表中");
    assert_eq!(deleted_at(id), None, "新建夹 deleted_at 应为 NULL");

    // 软删除：从列表消失，但 albums 行仍在（deleted_at 置位）、album_items 成员保留。
    delete_collection(&c, id).unwrap();
    assert!(!has(id), "软删除后夹应从列表消失");
    assert!(deleted_at(id).is_some(), "软删除应置 deleted_at 时间戳");
    let member_rows: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM album_items WHERE album_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(member_rows, 1, "软删除不得清除成员（可撤销的前提）");

    // 恢复：清零 deleted_at，夹重新出现在列表中。
    restore_collection(&c, id).unwrap();
    assert!(has(id), "restore 后夹应重新出现");
    assert_eq!(deleted_at(id), None, "restore 应清零 deleted_at");

    // 系统夹保护：delete_collection 对系统夹为空操作（deleted_at 保持 NULL、仍在列表）。
    c.execute(
        "INSERT INTO albums (name, kind, media_type_filter) VALUES ('系统夹', 'system', 'image')",
        [],
    )
    .unwrap();
    let sys_id = c.last_insert_rowid();
    delete_collection(&c, sys_id).unwrap();
    assert_eq!(deleted_at(sys_id), None, "系统夹不应被软删除");
    assert!(has(sys_id), "系统夹应始终在列表中");
}

/// 软删收藏夹的**读路径**（2026-07-16 裁决：只补读路径、不做 purge）：`list_deleted_collections`
/// 与 `list_collections` 对同一批夹构成**不重不漏的划分**，且按 deleted_at 倒序（最近删除在前）。
/// 这条读路径是「软删行永久滞留却无 UI 可捞回」的唯一修补——它一失效，重启后夹就再也回不来。
#[test]
fn list_deleted_collections_partitions_and_orders_by_deleted_at() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();

    let live_ids = |conn: &Connection| -> Vec<i64> {
        list_collections(conn)
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect()
    };
    let dead_ids = |conn: &Connection| -> Vec<i64> {
        list_deleted_collections(conn)
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect()
    };

    let a = create_collection(&c, "旅行", None).unwrap();
    let b = create_collection(&c, "美食", None).unwrap();
    assert!(dead_ids(&c).is_empty(), "未删除时回收站应为空");

    // 划分不重不漏：软删 a 后，a 只在 deleted 侧、b 只在 live 侧。
    delete_collection(&c, a).unwrap();
    assert_eq!(dead_ids(&c), vec![a], "软删的夹应出现在回收站");
    assert!(!live_ids(&c).contains(&a), "软删的夹不得同时留在正列表");
    assert!(live_ids(&c).contains(&b), "未删的夹不受影响");

    // 排序按 deleted_at 倒序而非 id：让**先建的 a** 后删（时间戳更大），断言 a 排在 b 前。
    // delete_collection 用 strftime('%s','now') 取整秒，同一测试内两次删除会撞同一秒 → 退化为
    // id 倒序兜底、测不出真实排序键，故此处直接钉死两个不同的时间戳。
    delete_collection(&c, b).unwrap();
    c.execute("UPDATE albums SET deleted_at=100 WHERE id=?1", params![b])
        .unwrap();
    c.execute("UPDATE albums SET deleted_at=200 WHERE id=?1", params![a])
        .unwrap();
    assert_eq!(
        dead_ids(&c),
        vec![a, b],
        "回收站应按 deleted_at 倒序（最近删除在前）"
    );

    // 系统夹永不进回收站（delete_collection 的 kind='user' 守卫已保证，此处锁读侧同样不漏）。
    c.execute(
        "INSERT INTO albums (name, kind, media_type_filter, deleted_at)
         VALUES ('系统夹', 'system', 'image', 300)",
        [],
    )
    .unwrap();
    let sys_id = c.last_insert_rowid();
    assert!(
        !dead_ids(&c).contains(&sys_id),
        "即使 deleted_at 被置位，系统夹也不应出现在回收站（kind='user' 过滤）"
    );

    // 恢复即离开回收站、回到正列表。
    restore_collection(&c, a).unwrap();
    assert_eq!(dead_ids(&c), vec![b], "restore 后该夹应离开回收站");
    assert!(live_ids(&c).contains(&a), "restore 后该夹应回到正列表");
}

/// 误检桶管理(ignored 历史管理):`list_ignored_persons` 只返回 is_ignored=1,`list_persons`
/// 反之排除;二者均按激活模型隔离(Part4-T6)。
#[test]
fn list_ignored_persons_returns_only_ignored() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();
    // 三个人物,同一激活模型 'm':普通/隐藏/误检桶。cover_face_id 留空(LEFT JOIN 容许)。
    c.execute(
        "INSERT INTO persons (name, is_named, model_name, face_count, is_hidden, is_ignored) VALUES
             ('普通', 1, 'm', 5, 0, 0),
             ('隐藏', 1, 'm', 3, 1, 0),
             ('误检', 0, 'm', 2, 0, 1)",
        [],
    )
    .unwrap();

    // 人物墙:排除 is_ignored=1,含隐藏(前端再过滤)。
    let wall = list_persons(&c, "m").unwrap();
    assert_eq!(wall.len(), 2, "墙应含普通+隐藏,排除误检桶");
    assert!(wall.iter().all(|p| p.name.as_deref() != Some("误检")));

    // 误检桶:只含 is_ignored=1。
    let ignored = list_ignored_persons(&c, "m").unwrap();
    assert_eq!(ignored.len(), 1, "误检桶应恰含 1 个");
    assert_eq!(ignored[0].name.as_deref(), Some("误检"));

    // 模型隔离:换激活模型 'other' 时两者皆空。
    assert!(list_persons(&c, "other").unwrap().is_empty());
    assert!(list_ignored_persons(&c, "other").unwrap().is_empty());
}

#[test]
fn person_scope_restricts_to_faces() {
    let (sql, params) = view_to_sql(&view(
        ViewScope::Person { person_id: 3 },
        GalleryFilter::default(),
    ))
    .unwrap();
    assert!(sql.contains("SELECT item_id FROM faces WHERE person_id ="));
    assert_eq!(params.len(), 1);
}

#[test]
fn trash_scope_flips_is_deleted() {
    let (sql, _) = view_to_sql(&view(ViewScope::Trash, GalleryFilter::default())).unwrap();
    assert!(
        sql.contains("WHERE m.is_deleted=1 AND m.companion_of IS NULL"),
        "回收站 scope"
    );
}

#[test]
fn filter_increments_where_and_params() {
    let filter = GalleryFilter {
        media_types: Some(vec!["image".into(), "video".into()]),
        min_rating: Some(3),
        color_label: Some(5),
        date_range: Some(DateRange { from: 100, to: 200 }),
        favorited_only: Some(true),
        ..Default::default()
    };
    let (sql, params) = view_to_sql(&view(ViewScope::All, filter)).unwrap();
    assert!(sql.contains("media_type IN (?1,?2)"), "media_types IN 绑定");
    assert!(sql.contains("rating >= "), "min_rating 谓词");
    assert!(
        sql.contains("color_label = "),
        "color_label 谓词（等值匹配）"
    );
    assert!(sql.contains("sort_datetime >= ") && sql.contains("sort_datetime <= "));
    assert!(sql.contains("is_favorited=1"), "favorited 谓词（无参数）");
    // media_types ×2 + min_rating ×1 + color_label ×1 + date_range ×2 = 6（favorited 字面量谓词不占参数）。
    assert_eq!(params.len(), 6);
}

// ── 细分格式筛选（S 线 P3 / D-011）────────────────────────────────────────

/// 🔴 格式谓词的**参数序号必须接在媒体大类之后**，且与绑定顺序严格对齐。
///
/// 错位是这段代码最凶的失败模式：既不 panic 也不报 SQL 错，SQLite 只是按序号取到隔壁谓词的
/// 值 —— 于是「筛 PNG」静默变成「筛 image 那个字符串当格式」，结果恒空。故断言钉的是**完整的
/// 占位符编号**（`?3,?4`），不是「含有 file_format IN」这种谁都能过的模糊断言。
#[test]
fn file_formats_predicate_binds_after_media_types() {
    let filter = GalleryFilter {
        media_types: Some(vec!["image".into(), "video".into()]),
        file_formats: Some(vec!["png".into(), "jpg".into()]),
        ..Default::default()
    };
    let (sql, params) = view_to_sql(&view(ViewScope::All, filter)).unwrap();
    assert!(sql.contains("media_type IN (?1,?2)"), "大类占 ?1,?2");
    assert!(
        sql.contains("file_format IN (?3,?4)"),
        "格式须接在大类之后占 ?3,?4，实得 SQL: {sql}"
    );
    assert_eq!(params.len(), 4);
}

/// 维度间取 **AND**（D-011：`图片 AND (PNG OR JPEG)`）—— 两个谓词都在，不是二选一。
#[test]
fn media_type_and_format_are_anded_not_replaced() {
    let filter = GalleryFilter {
        media_types: Some(vec!["image".into()]),
        file_formats: Some(vec!["png".into()]),
        ..Default::default()
    };
    let (sql, _) = view_to_sql(&view(ViewScope::All, filter)).unwrap();
    assert!(sql.contains("AND media_type IN") && sql.contains("AND file_format IN"));
}

/// 🔴 空列表 = **该维度不限**，绝不能退化成 `IN ()`。
///
/// `IN ()` 在 SQLite 里是语法错（整条查询炸），而就算它能跑，语义也是**筛出零条** ——
/// 与「不限」正好相反。前端把 `fileFormats: []` 当「没选格式」传下来是常态，故这条是真实路径。
#[test]
fn empty_format_list_means_unrestricted_not_empty_set() {
    let filter = GalleryFilter {
        file_formats: Some(vec![]),
        ..Default::default()
    };
    let (sql, params) = view_to_sql(&view(ViewScope::All, filter)).unwrap();
    assert!(!sql.contains("file_format"), "空列表不该产出任何格式谓词");
    assert!(params.is_empty());
}

/// 🔴 `to_media_filter()` 必须把格式带下去 —— SelectAll 靠它解析全集。
///
/// 漏了不会报错：批量收藏/评分/删除照跑，只是**作用到比画面更大的集合**上（用户按格式筛过、
/// 全选、批量删 → 删掉了没筛出来的那些）。这是本字段最贵的漏法，故单独钉。
#[test]
fn to_media_filter_carries_file_formats_for_select_all() {
    let v = view(
        ViewScope::All,
        GalleryFilter {
            file_formats: Some(vec!["png".into()]),
            ..Default::default()
        },
    );
    assert_eq!(v.to_media_filter().file_formats, Some(vec!["png".into()]));
}

/// `push_in_predicate` 的序号算术：接着已有参数往下排，不从 1 重来。
#[test]
fn push_in_predicate_continues_param_numbering() {
    let mut sql = String::new();
    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let next = push_in_predicate(&mut sql, &mut extras, 2, "file_format", &["png".into()]);
    assert_eq!(sql, " AND file_format IN (?3)");
    assert_eq!(next, 3);
    assert_eq!(extras.len(), 1);
}

#[test]
fn push_in_predicate_noop_on_empty() {
    let mut sql = String::new();
    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let next = push_in_predicate(&mut sql, &mut extras, 5, "file_format", &[]);
    assert!(sql.is_empty(), "空列表不得产出 IN () —— 那是语法错");
    assert_eq!(next, 5, "空列表不得吃掉参数序号");
    assert!(extras.is_empty());
}

#[test]
fn semantic_search_scope_is_rejected_in_v1() {
    let v = view(
        ViewScope::SemanticSearch {
            query_embedding_id: 1,
            top_k: 50,
        },
        GalleryFilter::default(),
    );
    // 不能用 unwrap_err()：Ok 型含 Box<dyn ToSql> 未实现 Debug。
    match view_to_sql(&v) {
        Err(AppError::Internal(_)) => {}
        Ok(_) => panic!("SemanticSearch v1 不应支持纯 SQL 解析"),
        Err(e) => panic!("期望 Internal 错误，实得 {e:?}"),
    }
}

#[test]
fn to_media_filter_lowers_scope_and_filter() {
    // scope=Directory + filter.min_rating → MediaFilter.directory_id + min_rating。
    let v = view(
        ViewScope::Directory { directory_id: 9 },
        GalleryFilter {
            min_rating: Some(4),
            ..Default::default()
        },
    );
    let mf = v.to_media_filter();
    assert_eq!(mf.directory_id, Some(9));
    assert_eq!(mf.min_rating, Some(4));
    assert_eq!(mf.album_id, None);
    assert_eq!(mf.person_id, None);
    assert_eq!(mf.trashed_only, None);
}

// ── 重复镜头契约（2026-09-02 主画廊重复项浏览方案 §10.2，P0 契约冻结）──────────

use crate::db::models::{
    DuplicateLensDescriptor, DuplicateLensMode, DUPLICATE_LENS_ORDERING_VERSION,
};

/// 镜头便捷构造：默认全库 + 空 filter，正是 validate 唯一放行的形态。
fn lens(mode: DuplicateLensMode, show_unique_items: bool) -> DuplicateLensDescriptor {
    DuplicateLensDescriptor {
        mode,
        show_unique_items,
        ordering_version: DUPLICATE_LENS_ORDERING_VERSION,
    }
}

/// 表征锁（P0）：无 duplicate_lens 的既有 ViewDescriptor 线上 JSON 逐字节不变——
/// 字段顺序（scope/filter/sort/layoutVersion）、camelCase、内联 null filter 一律冻结。
/// 任何「顺手重构」改变 wire 形状都应在此显式失败，而非静默漂移前端契约。
#[test]
fn view_descriptor_wire_without_lens_is_unchanged() {
    let json = serde_json::to_string(&view(ViewScope::All, GalleryFilter::default())).unwrap();
    assert_eq!(
        json,
        r#"{"scope":{"kind":"all"},"filter":{"mediaTypes":null,"fileFormats":null,"livePhotoOnly":null,"favoritedOnly":null,"minRating":null,"colorLabel":null,"dateRange":null,"searchQuery":null,"searchScope":null,"recentOnly":null},"sort":{"groupBy":"date","sortWithinGroup":"datetime","sortOrder":"desc"},"layoutVersion":0}"#,
        "无镜头描述符的 wire 形状必须与方案前逐字节一致"
    );
    assert!(
        !json.contains("duplicateLens"),
        "缺省镜头不得出现在 wire 上"
    );
}

/// 表征锁：带 duplicate_lens 的 ViewDescriptor 线上 JSON 形状（键名 duplicateLens/
/// showUniqueItems/orderingVersion + 值 "groups"）与字段位置（sort 之后、layoutVersion 之前）。
#[test]
fn view_descriptor_wire_with_lens_locks_shape() {
    let mut v = view(ViewScope::All, GalleryFilter::default());
    v.duplicate_lens = Some(lens(DuplicateLensMode::Groups, false));
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"scope":{"kind":"all"},"filter":{"mediaTypes":null,"fileFormats":null,"livePhotoOnly":null,"favoritedOnly":null,"minRating":null,"colorLabel":null,"dateRange":null,"searchQuery":null,"searchScope":null,"recentOnly":null},"sort":{"groupBy":"date","sortWithinGroup":"datetime","sortOrder":"desc"},"duplicateLens":{"mode":"groups","showUniqueItems":false,"orderingVersion":1},"layoutVersion":0}"#
    );

    // round-trip：前端构造的 JSON 能原样解析回同一描述符。
    let parsed: ViewDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.duplicate_lens,
        Some(lens(DuplicateLensMode::Groups, false))
    );
    assert_eq!(parsed.layout_version, 0);
}

/// 旧 JSON（无 duplicateLens 键）反序列化 → duplicate_lens == None（serde(default) 保证
/// 方案前的前端载荷在方案后的后端上继续可解析）。
#[test]
fn legacy_json_without_lens_deserializes_to_none() {
    let v: ViewDescriptor = serde_json::from_str(
        r#"{"scope":{"kind":"all"},"filter":{},"sort":{"groupBy":"date","sortWithinGroup":"datetime","sortOrder":"desc"},"layoutVersion":3}"#,
    )
    .unwrap();
    assert_eq!(v.duplicate_lens, None, "缺键应落 default(None)");
    assert_eq!(v.layout_version, 3);
}

/// validate 合法形态：groups+false / folders+false / folders+true（全库 + 空 filter）。
#[test]
fn lens_validate_accepts_mvp_legal_forms() {
    let all = ViewScope::All;
    let empty = GalleryFilter::default();
    lens(DuplicateLensMode::Groups, false)
        .validate(&all, &empty)
        .unwrap();
    lens(DuplicateLensMode::Folders, false)
        .validate(&all, &empty)
        .unwrap();
    // Mode::Folders + show_unique_items 任意值合法（独有项由顶栏开关控制，方案 §11.2）。
    lens(DuplicateLensMode::Folders, true)
        .validate(&all, &empty)
        .unwrap();
}

/// validate 非法形态四例：非全库 scope / 成员级 filter / groups+独有项 / 版本漂移。
#[test]
fn lens_validate_rejects_mvp_illegal_forms() {
    let empty = GalleryFilter::default();
    // scope 必须 All。
    assert!(lens(DuplicateLensMode::Groups, false)
        .validate(&ViewScope::Directory { directory_id: 1 }, &empty)
        .is_err());
    // filter 必须全空（不接受静默忽略的成员级谓词）。
    let filtered = GalleryFilter {
        media_types: Some(vec!["image".into()]),
        ..Default::default()
    };
    assert!(lens(DuplicateLensMode::Groups, false)
        .validate(&ViewScope::All, &filtered)
        .is_err());
    // groups 模式禁止 showUniqueItems。
    assert!(lens(DuplicateLensMode::Groups, true)
        .validate(&ViewScope::All, &empty)
        .is_err());
    // orderingVersion 漂移 → 显式拒绝（防旧描述符被新排序语义错误执行）。
    let stale = DuplicateLensDescriptor {
        ordering_version: DUPLICATE_LENS_ORDERING_VERSION + 1,
        ..lens(DuplicateLensMode::Folders, true)
    };
    assert!(stale.validate(&ViewScope::All, &empty).is_err());
}

/// view_to_sql 镜头分支守门（方案 §10.2/§13）：带镜头的描述符先过 validate；groups 模式
/// 在 P2 已走真实 lowering（tests/lens_lowering.rs 锁序列 parity），folders 模式仍显式
/// 拒绝（P3）——绝不静默按普通画廊编译或按 groups 降维（画面与选择契约漂移）。
#[test]
fn view_to_sql_rejects_duplicate_lens_explicitly() {
    // folders 模式 → DuplicateLensUnsupported（P3 落地前的守门）。
    let mut ok = view(ViewScope::All, GalleryFilter::default());
    ok.duplicate_lens = Some(lens(DuplicateLensMode::Folders, true));
    match view_to_sql(&ok) {
        Err(AppError::DuplicateLensUnsupported) => {}
        Err(e) => panic!("期望 DuplicateLensUnsupported，实得 {e:?}"),
        Ok(_) => panic!("镜头描述符不得被静默按普通画廊编译"),
    }

    // 非法镜头（groups + 独有项）→ 先于 Unsupported 报 DuplicateLensInvalid。
    let mut bad = view(ViewScope::All, GalleryFilter::default());
    bad.duplicate_lens = Some(lens(DuplicateLensMode::Groups, true));
    match view_to_sql(&bad) {
        Err(AppError::DuplicateLensInvalid(_)) => {}
        Err(e) => panic!("期望 DuplicateLensInvalid，实得 {e:?}"),
        Ok(_) => panic!("非法镜头不得放行"),
    }

    // 非全库 scope 的镜头同样先报校验错误。
    let mut scoped = view(
        ViewScope::Directory { directory_id: 1 },
        GalleryFilter::default(),
    );
    scoped.duplicate_lens = Some(lens(DuplicateLensMode::Folders, false));
    assert!(matches!(
        view_to_sql(&scoped),
        Err(AppError::DuplicateLensInvalid(_))
    ));
}
