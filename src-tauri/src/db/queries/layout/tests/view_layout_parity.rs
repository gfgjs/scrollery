//! descriptor ↔ layout parity 表征测试（2026-09-02 主画廊重复项浏览方案 §10.2/§13 P0）。
//!
//! 契约本体：`view_to_sql(view)`（SelectAll/GET_VIEW_IDS 的全集解析）与
//! `query_layout_items(view.to_media_filter(), ...)`（画廊取数）使用**同一 lowering**
//! （`push_query_body` 单一事实源）。P0 把这条等价对当前普通画廊逐视图钉死——
//! P1/P2 扩展镜头 lowering 时本文件必须继续通过：镜头路径若分叉出第二套 WHERE，
//! 在此显式失败，而非让 SelectAll 集合与镜头画面静默漂移。

use super::*;
use crate::db::models::{DateRange, GalleryFilter, SortSpec, ViewDescriptor, ViewScope};

/// 种子库：三目录（root → A → Nested）+ 覆盖全部筛选维度的媒体项 + 一个用户夹 + 一张人脸。
fn parity_db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/parity-root', 'root');
         INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES
             (10, 1, NULL, '', 'root', 0),
             (11, 1, 10, 'a', 'A', 1),
             (12, 1, 11, 'a/nested', 'Nested', 2);",
    )
    .unwrap();

    // 覆盖维度：media_type / file_format / rating / favorite / color_label /
    // live_photo / sort_datetime / 软删除 / companion / created_at（recent_only）。
    for (id, dir, name, format, media_type, dt) in [
        (1, 11, "alpha.jpg", "jpg", "image", 1000),
        (2, 11, "beta.png", "png", "image", 2000),
        (3, 12, "gamma.mp4", "mp4", "video", 3000),
        (4, 10, "delta.heic", "heic", "image", 4000),
        (5, 11, "trashed.jpg", "jpg", "image", 5000),
        (6, 11, "echo.mp3", "mp3", "audio", 6000),
        (7, 11, "live.jpg", "jpg", "image", 7000),
        (8, 11, "old.jpg", "jpg", "image", 8000),
        (9, 11, "companion.mov", "mov", "video", 9000),
    ] {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                 width, height, sort_datetime, cache_key)
             VALUES (?1, ?2, ?3, 10, 1, ?4, ?5, 100, 100, ?6, ?1)",
            params![id, dir, name, format, media_type, dt],
        )
        .unwrap();
    }
    c.execute_batch(
        "UPDATE media_items SET is_favorited=1, rating=5, color_label=3 WHERE id=1;
         UPDATE media_items SET color_label=4 WHERE id=2;
         UPDATE media_items SET rating=2 WHERE id=3;
         UPDATE media_items SET is_favorited=1 WHERE id=4;
         UPDATE media_items SET is_deleted=1 WHERE id=5;
         UPDATE media_items SET is_live_photo=1 WHERE id=7;
         -- recent_only：8 号项落在「最近 30 天」窗口外（其余项 created_at 取默认 now）。
         UPDATE media_items
            SET created_at = CAST(strftime('%s','now','-40 days') AS INTEGER)
          WHERE id = 8;
         -- Live Photo companion：基础谓词 companion_of IS NULL 两侧必须同排除。
         UPDATE media_items SET companion_of = 1 WHERE id = 9;

         INSERT INTO albums (name, kind) VALUES ('travel', 'user');
         INSERT INTO album_items (album_id, item_id)
         SELECT id, 1 FROM albums WHERE kind='user';
         INSERT INTO album_items (album_id, item_id)
         SELECT id, 3 FROM albums WHERE kind='user';

         INSERT INTO persons (name, is_named) VALUES ('P', 1);
         INSERT INTO faces (item_id, person_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h,
                            det_score, embedding)
         VALUES (1, 1, 'm', 0, 0, 1, 1, 0.9, X'00');",
    )
    .unwrap();
    c
}

/// 便捷构造：默认排序（date/datetime/desc）与 layout_version=0；与 view_to_sql.rs 的
/// view() 形态一致（不共享 helper 是测试树惯例——各文件自持）。
fn view(scope: ViewScope, filter: GalleryFilter) -> ViewDescriptor {
    ViewDescriptor {
        scope,
        filter,
        sort: SortSpec::default(),
        duplicate_lens: None,
        layout_version: 0,
    }
}

/// SelectAll 视角：view_to_sql 编译出的 SQL 执行取 id 集（排序后）。
fn sql_ids(c: &Connection, v: &ViewDescriptor) -> Vec<i64> {
    let (sql, params) = view_to_sql(v, &[]).unwrap();
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = c.prepare(&sql).unwrap();
    let mut ids: Vec<i64> = stmt
        .query_map(refs.as_slice(), |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

/// 画廊视角：to_media_filter 下沉后经 query_layout_items 取 id 集（排序后）。
fn layout_ids(c: &Connection, v: &ViewDescriptor) -> Vec<i64> {
    let mf = v.to_media_filter();
    let mut items = query_layout_items(
        c,
        &mf,
        Some(v.sort.group_by.as_str()),
        Some(v.sort.sort_within_group.as_str()),
        Some(v.sort.sort_order.as_str()),
        false,
    )
    .unwrap();
    items.sort_unstable_by_key(|it| it.id);
    items.iter().map(|it| it.id).collect()
}

/// 双路径 id 集相等（parity 契约本体）+ 与手算期望集相等（种子形态锁）。
fn assert_parity(c: &Connection, v: &ViewDescriptor, expected: &[i64]) {
    let a = sql_ids(c, v);
    let b = layout_ids(c, v);
    assert_eq!(
        a, b,
        "view_to_sql 与 query_layout_items 必须同集（scope={v:?}）"
    );
    assert_eq!(a, expected, "种子数据形态漂移（scope={:?}）", v.scope);
}

#[test]
fn all_scope_empty_filter_parity() {
    let c = parity_db();
    // 5=软删、9=companion：基础谓词两侧同排除。
    assert_parity(
        &c,
        &view(ViewScope::All, GalleryFilter::default()),
        &[1, 2, 3, 4, 6, 7, 8],
    );
}

#[test]
fn all_scope_media_types_parity() {
    let c = parity_db();
    let f = GalleryFilter {
        media_types: Some(vec!["image".into()]),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, f), &[1, 2, 4, 7, 8]);
}

/// 细分格式与媒体大类取 AND（D-011）。
#[test]
fn all_scope_media_types_and_file_formats_parity() {
    let c = parity_db();
    let f = GalleryFilter {
        media_types: Some(vec!["image".into()]),
        file_formats: Some(vec!["png".into()]),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, f), &[2]);
}

/// livePhotoOnly / favoritedOnly / minRating / colorLabel 四个逐项小标量各一。
#[test]
fn all_scope_scalar_filters_parity() {
    let c = parity_db();

    let live = GalleryFilter {
        live_photo_only: Some(true),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, live), &[7]);

    let fav = GalleryFilter {
        favorited_only: Some(true),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, fav), &[1, 4]);

    let rated = GalleryFilter {
        min_rating: Some(2),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, rated), &[1, 3]);

    let color = GalleryFilter {
        color_label: Some(4),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, color), &[2]);
}

#[test]
fn all_scope_date_range_parity() {
    let c = parity_db();
    let f = GalleryFilter {
        date_range: Some(DateRange {
            from: 1500,
            to: 3500,
        }),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, f), &[2, 3]);
}

/// recent_only 走 created_at 窗口（R1-2 补字段）：8 号项（40 天前）两侧同排除。
#[test]
fn all_scope_recent_only_parity() {
    let c = parity_db();
    let f = GalleryFilter {
        recent_only: Some(true),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, f), &[1, 2, 3, 4, 6, 7]);
}

/// Directory scope 复用 WITH RECURSIVE dir_tree：子目录 11 含嵌套 12 的项。
#[test]
fn directory_scope_recursive_subtree_parity() {
    let c = parity_db();
    assert_parity(
        &c,
        &view(
            ViewScope::Directory { directory_id: 11 },
            GalleryFilter::default(),
        ),
        &[1, 2, 3, 6, 7, 8],
    );
    // 更深一层只含自身。
    assert_parity(
        &c,
        &view(
            ViewScope::Directory { directory_id: 12 },
            GalleryFilter::default(),
        ),
        &[3],
    );
}

#[test]
fn collection_scope_album_members_parity() {
    let c = parity_db();
    let album_id: i64 = c
        .query_row("SELECT id FROM albums WHERE kind='user'", [], |r| r.get(0))
        .unwrap();
    assert_parity(
        &c,
        &view(ViewScope::Collection { album_id }, GalleryFilter::default()),
        &[1, 3],
    );
}

#[test]
fn trash_scope_parity() {
    let c = parity_db();
    assert_parity(&c, &view(ViewScope::Trash, GalleryFilter::default()), &[5]);
}

/// 多条件组合（大类 × 格式 × 评分 × 收藏 × 时间窗）：仅 1 号项全部命中。
#[test]
fn all_scope_combined_filters_parity() {
    let c = parity_db();
    let f = GalleryFilter {
        media_types: Some(vec!["image".into(), "video".into()]),
        file_formats: Some(vec!["jpg".into(), "mp4".into()]),
        min_rating: Some(2),
        favorited_only: Some(true),
        date_range: Some(DateRange { from: 0, to: 2000 }),
        ..Default::default()
    };
    assert_parity(&c, &view(ViewScope::All, f), &[1]);
}

/// Person scope（F6 人物墙 → 某人物的照片）：人脸归属项两侧同集。
#[test]
fn person_scope_faces_parity() {
    let c = parity_db();
    assert_parity(
        &c,
        &view(ViewScope::Person { person_id: 1 }, GalleryFilter::default()),
        &[1],
    );
}
