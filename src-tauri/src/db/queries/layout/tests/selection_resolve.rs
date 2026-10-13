//! T18 S1：`resolve_selection` / `count_selection` + ViewStale 守门（带真实 DB seed）。

use super::*;
use crate::db::models::{GalleryFilter, SelectionDescriptor, SortSpec, ViewDescriptor, ViewScope};

// 跨域测试定向 import(§5 规则 2:只补编译所需;裸名原经 facade glob 解析)。
use super::super::super::media::batch_set_favorite;

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
