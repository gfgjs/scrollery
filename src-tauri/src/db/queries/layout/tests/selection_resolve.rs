//! T18 S1：`resolve_selection` / `count_selection` + ViewStale 守门（带真实 DB seed）。

use super::*;
use crate::db::models::{GalleryFilter, SelectionDescriptor, SortSpec, ViewDescriptor, ViewScope};

// 跨域测试定向 import(§5 规则 2:只补编译所需;裸名原经 facade glob 解析)。
use super::super::super::media::{batch_set_favorite, soft_delete_items};

const VER: u64 = 5;

/// seed 一个根 + 目录 + 3 个媒体项（id 1/2/3，sort_datetime 100/200/300）。
fn seeded_db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();
    // 建库不再自注册 TREE_SORT_KEY/NATURAL_CMP;本组用例的 SQL 依赖它,在连接边界注册。
    crate::db::register_custom_collations(&c).unwrap();

    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key)
             VALUES (1, 10, 'a.jpg', 1, 1, 'jpg', 'image', 0, 0, 100, 0),
                    (2, 10, 'b.jpg', 1, 1, 'jpg', 'image', 0, 0, 200, 0),
                    (3, 10, 'c.jpg', 1, 1, 'jpg', 'image', 0, 0, 300, 0);",
    )
    .unwrap();
    c
}

/// S1 序等价契约对拍：canonical 基准序 + items_cache::derive_order 的内存派生序，必须与
/// query_layout_items（push_order_by 的 SQL ORDER）**逐项一致** —— get_view_ids（flat_ids）
/// 与 view_to_sql（SelectAll 解析）分别源于这两条路径，错位即选区漂移。两侧 folder 目录序
/// 均为前序 DFS（内存 build_dir_rank 算 encode_tree_sort_key / SQL 读持久列 d.tree_sort_key，
/// 同一键逻辑，方案 B —— fixture 插完补一行回填 UPDATE 使存列 = 算键）。fixture
/// 刻意包含：两根同 rel_path（root 序使其整棵子树连续、不跨根交错，退化并发由 directory_id 裁决）、
/// 大小写与 Unicode rel_path（DFS 字节键：大写 < 小写 < 多字节）、同 sort_datetime（id tiebreaker）、
/// 空 rel_path 根目录（空键最小，排本根之首）。
#[test]
fn canonical_derive_order_matches_sql_order() {
    let c = Connection::open_in_memory().unwrap();
    // 下方 fixture 回填 UPDATE 依赖 TREE_SORT_KEY;建库不再自注册,此处在夹具内注册。
    crate::db::schema::initialize_schema(&c).unwrap();
    // 建库不再自注册 TREE_SORT_KEY/NATURAL_CMP;本组用例的 SQL 依赖它,在连接边界注册。
    crate::db::register_custom_collations(&c).unwrap();

    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES
             (10, 1, '', 'r1'),
             (11, 1, 'Albums', 'Albums'),
             (12, 1, 'albums', 'albums'),
             (13, 1, '相册', '相册'),
             (20, 2, 'albums', 'albums');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 10, 'a.jpg', 1, 1, 'jpg', 'image', 100, 100, 500, 1),
             (2, 11, 'b.jpg', 1, 1, 'jpg', 'image', 100, 100, 300, 2),
             (3, 12, 'c.jpg', 1, 1, 'jpg', 'image', 100, 100, 400, 3),
             (4, 20, 'd.jpg', 1, 1, 'jpg', 'image', 100, 100, 400, 4),
             (5, 12, 'e.jpg', 1, 1, 'jpg', 'image', 100, 100, 400, 5),
             (6, 13, 'f.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 6),
             (7, 10, 'g.jpg', 1, 1, 'jpg', 'image', 100, 100, 500, 7);",
    )
    .unwrap();
    // 裸 INSERT 的 tree_sort_key 取 DEFAULT X''（全空）；push_order_by 现读该列 → 须回填成
    // 真键，否则 SQL 侧目录序退化按 d.id、与内存 build_dir_rank 算键分歧 → 对拍红（方案 B §7）。
    c.execute_batch("UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path)")
        .unwrap();

    let filter = MediaFilter::default();
    let canonical = query_layout_items_canonical(&c, &filter).unwrap();
    assert_eq!(canonical.len(), 7, "canonical 应取回全部 7 项");
    let dir_labels = query_dir_labels(&c).unwrap();
    let data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::Canonical,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&canonical),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: canonical,
        dir_labels,
        filter: filter.clone(),
        reusable: true,
        geometry_revision: 0,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };

    for group_by in ["date", "none", "folder"] {
        for sort_order in ["desc", "asc"] {
            let sql_ids: Vec<i64> = query_layout_items(
                &c,
                &filter,
                Some(group_by),
                Some("datetime"),
                Some(sort_order),
                false,
            )
            .unwrap()
            .iter()
            .map(|it| it.id)
            .collect();
            let derived_ids: Vec<i64> =
                crate::layout::items_cache::derive_order(&data, group_by, "datetime", sort_order)
                    .iter()
                    .map(|it| it.id)
                    .collect();
            assert_eq!(
                derived_ids, sql_ids,
                "内存派生序 != SQL 序：group_by={group_by} sort_order={sort_order}"
            );
            if group_by == "folder" {
                // 前序 DFS + root 序：root1 整棵子树 [dir10(''), 11(Albums), 12(albums), 13(相册)]
                // 先于 root2 [dir20(albums)]。关键点：root2 的 'albums'(item 4) 不再按相同 rel_path
                // 插到 root1 的 'albums'(dir12) 旁，而是整体排到 root1 子树之后 —— 即多根交错修复。
                // 组内媒体 (sort_datetime, id) 随向反转：
                //   desc: 10[7,1] 11[2] 12[5,3] 13[6] 20[4]
                //   asc : 10[1,7] 11[2] 12[3,5] 13[6] 20[4]
                let expected = if sort_order == "asc" {
                    vec![1, 7, 2, 3, 5, 6, 4]
                } else {
                    vec![7, 1, 2, 5, 3, 6, 4]
                };
                assert_eq!(
                    sql_ids, expected,
                    "folder 目录序须为前序 DFS：不同扫描根整棵子树连续，同 rel_path 不跨根交错，组内按 directory_id 连续成组"
                );
            }
        }
    }
}

fn all_view(version: u64) -> Box<ViewDescriptor> {
    Box::new(ViewDescriptor {
        scope: ViewScope::All,
        filter: GalleryFilter::default(),
        sort: SortSpec::default(),
        duplicate_lens: None,
        layout_version: version,
    })
}

#[test]
fn select_all_resolves_all_in_layout_order() {
    let c = seeded_db();
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![],
    };
    // 默认 date 分组 / desc → sort_datetime 倒序：300,200,100 → id 3,2,1。
    assert_eq!(resolve_selection(&c, &sel, VER).unwrap(), vec![3, 2, 1]);
    assert_eq!(count_selection(&c, &sel, VER).unwrap(), 3);
}

#[test]
fn select_all_stale_version_rejected() {
    let c = seeded_db();
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![],
    };
    // 当前版本 != view 携带版本 → ViewStale（resolve 与 count 都守门）。
    assert!(matches!(
        resolve_selection(&c, &sel, VER + 1),
        Err(AppError::ViewStale)
    ));
    assert!(matches!(
        count_selection(&c, &sel, VER + 1),
        Err(AppError::ViewStale)
    ));
}

#[test]
fn select_all_excludes_ids() {
    let c = seeded_db();
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![2],
    };
    assert_eq!(resolve_selection(&c, &sel, VER).unwrap(), vec![3, 1]);
}

/// R1-5 契约锁 ①：SelectAll 解析恒排除软删项（push_query_body 的 is_deleted 谓词）——
/// 这是「删除后布局缓存有意保持 stale」窗口的写路径安全网之一（cache.rs 失效契约注）。
#[test]
fn select_all_excludes_soft_deleted() {
    let c = seeded_db();
    soft_delete_items(&c, &[2]).unwrap();
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![],
    };
    assert_eq!(
        resolve_selection(&c, &sel, VER).unwrap(),
        vec![3, 1],
        "已删项不得进入全选目标集"
    );
    assert_eq!(count_selection(&c, &sel, VER).unwrap(), 2);
}

/// R1-5 契约锁 ②：批量写对软删 id 是 no-op（UPDATE 恒带 AND is_deleted=0）——
/// stale 布局缓存把已删 id 混进选区（如 rangeBetween）也不会误写回收站内容。
#[test]
fn batch_write_skips_soft_deleted() {
    let c = seeded_db();
    soft_delete_items(&c, &[2]).unwrap();
    let affected = batch_set_favorite(&c, &[1, 2, 3], true).unwrap();
    assert_eq!(affected, 2, "已删项 2 不计入影响行");
    let fav2: i64 = c
        .query_row("SELECT is_favorited FROM media_items WHERE id=2", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fav2, 0, "回收站内容不得被批量写触碰");
}

/// R1-2：跨 SELECTION_BATCH_CHUNK 边界的分块正确性（单条 IN 会超 SQLite 绑定上限的场景）。
#[test]
fn batch_update_chunks_across_boundary() {
    let c = seeded_db();
    // 追加 5001 项（连同 seed 3 项共 5004 > 5000 chunk），FK 已满足（directory 10 存在）。
    {
        let mut stmt = c
            .prepare(
                "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, width, height, sort_datetime, cache_key)
                 VALUES (?1, 10, 'x' || ?1 || '.jpg', 1, 1, 'jpg', 'image', 0, 0, ?1, 0)",
            )
            .unwrap();
        for id in 100..(100 + 5001) {
            stmt.execute(params![id]).unwrap();
        }
    }
    let ids: Vec<i64> = (1..=3).chain(100..(100 + 5001)).collect();
    let affected = batch_set_favorite(&c, &ids, true).unwrap();
    assert_eq!(affected, 5004, "两块（5000+4）应全部落库且计数累加正确");
}
