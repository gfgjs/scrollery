//! 格式 facet(S 线 §7.2 / D-005):`list_library_formats` 测试。

use super::*;

// 跨域测试定向 import(§5 规则 2):搜索侧格式筛选对拍走 search owner。
use super::super::super::search::search_media;

/// fixture 的**区分度**是刻意设计的，逐条对应一种「基础谓词漏了会怎样」：
/// - `gif` 只以 `is_deleted=1` 存在 → 漏 `is_deleted=0` 则回收站里的格式污染全局 facet；
/// - `mov` 只以 Live-Photo 伴随视频存在 → 漏 `companion_of IS NULL` 则伴随视频的格式冒出来；
/// - `jpg` **两侧都有**（一条活的 + 一条软删的）→ 钉住「不是把有软删行的格式整个滤掉」，
///   而是按行判定。
///
/// ⚠ 生产库当前 raw distinct == base distinct == 29 是**数据巧合**（被基础谓词滤掉的仅 4 行
/// `is_deleted=1` 的 jpg，而 jpg 在 base 中也存在）。本 fixture 有意造出巧合**不成立**的形状 ——
/// 否则测试对「漏掉基础谓词」零区分度，跟生产库一样什么都测不出。
fn seeded() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, is_deleted, companion_of) VALUES
             (1, 10, 'a.jpg',  1, 1, 'jpg', 'image', 0, 0, 100, 11, 1, 0, NULL),
             (2, 10, 'b.png',  1, 1, 'png', 'image', 0, 0, 200, 12, 1, 0, NULL),
             (3, 10, 'c.gif',  1, 1, 'gif', 'image', 0, 0, 300, 13, 1, 1, NULL),
             (4, 10, 'd.jpg',  1, 1, 'jpg', 'image', 0, 0, 400, 14, 1, 1, NULL),
             (5, 10, 'e.mov',  1, 1, 'mov', 'video', 0, 0, 500, 15, 1, 0, 1);",
    )
    .unwrap();
    c
}

/// 🔴 基础谓词是**契约不是装饰**：回收站里的 gif 与 Live 伴随视频的 mov 都不得进 facet。
#[test]
fn facet_excludes_trashed_and_companion_formats() {
    let c = seeded();
    let got = list_library_formats(&c).unwrap();
    assert_eq!(got, vec!["jpg".to_string(), "png".to_string()]);
}

/// 按**行**判定而非按格式整体：jpg 有一条软删行，但它也有活行 → 必须留下。
#[test]
fn format_with_both_live_and_deleted_rows_stays() {
    let c = seeded();
    assert!(list_library_formats(&c)
        .unwrap()
        .contains(&"jpg".to_string()));
}

/// 输出去重且升序 —— UI 与测试都不必再排。
#[test]
fn facet_is_distinct_and_sorted() {
    let c = seeded();
    c.execute_batch(
        "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, is_deleted, companion_of) VALUES
             (6, 10, 'f.png', 1, 1, 'png', 'image', 0, 0, 600, 16, 1, 0, NULL),
             (7, 10, 'g.avi', 1, 1, 'avi', 'video', 0, 0, 700, 17, 1, 0, NULL);",
    )
    .unwrap();
    let got = list_library_formats(&c).unwrap();
    assert_eq!(got, vec!["avi".to_string(), "jpg".into(), "png".into()]);
}

/// 空库返回空表而非报错（新装 / 清库后开弹层是常态）。
#[test]
fn empty_library_yields_empty_facet() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    assert!(list_library_formats(&c).unwrap().is_empty());
}

/// 搜索侧同吃格式筛选：漏了会让「画廊按 PNG 筛过、搜同一个词却把 JPG 也搜出来」。
/// 走真库而非只对拍 SQL 字符串 —— 参数错位不报错、只静默筛错，只有真跑才抓得住。
#[test]
fn search_media_applies_file_formats() {
    let c = seeded();
    let f = |formats: Option<Vec<String>>| MediaFilter {
        file_formats: formats,
        search_scope: Some("filename".into()),
        ..Default::default()
    };
    // 不筛格式:a.jpg 与 b.png 都不含 "b"…… 用能命中两者的词。
    let all = search_media(&c, ".", &f(None), 10).unwrap();
    assert_eq!(all.len(), 2, "jpg + png 两条活行");
    let only_png = search_media(&c, ".", &f(Some(vec!["png".into()])), 10).unwrap();
    assert_eq!(only_png.len(), 1);
    assert_eq!(only_png[0].file_name, "b.png");
}
