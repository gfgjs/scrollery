//! 用真实数据库验证画廊及 canonical 查询排除隐藏根。

use super::*;
use crate::db::queries::scan::set_scan_root_hidden;

/// 两根:root1(可见,dir10:a.jpg/b.png) + root2(待隐,dir20:c.gif/d.webp)。全为 image、活行。
fn two_roots() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::schema::initialize_schema(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1'), (20, 2, '', 'r2');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, thumb_status, is_deleted, companion_of) VALUES
             (1, 10, 'a.jpg',  1, 1, 'jpg',  'image', 0, 0, 100, 11, 1, 0, NULL),
             (2, 10, 'b.png',  1, 1, 'png',  'image', 0, 0, 200, 12, 1, 0, NULL),
             (3, 20, 'c.gif',  1, 1, 'gif',  'image', 0, 0, 300, 13, 1, 0, NULL),
             (4, 20, 'd.webp', 1, 1, 'webp', 'image', 0, 0, 400, 14, 1, 0, NULL);",
    )
    .unwrap();
    c
}

fn ids(items: &[crate::db::models::LayoutItem]) -> Vec<i64> {
    let mut v: Vec<i64> = items.iter().map(|it| it.id).collect();
    v.sort_unstable();
    v
}

/// 画廊主查询(含 canonical 基准序路径)排除隐藏根的媒体。
#[test]
fn layout_queries_exclude_hidden_root() {
    let c = two_roots();
    let f = MediaFilter::default();
    assert_eq!(
        ids(&query_layout_items(&c, &f, None, None, None, false).unwrap()),
        vec![1, 2, 3, 4],
        "未隐藏:四项全在"
    );

    set_scan_root_hidden(&c, 2, true).unwrap();
    assert_eq!(
        ids(&query_layout_items(&c, &f, None, None, None, false).unwrap()),
        vec![1, 2],
        "隐藏 root2 后画廊只剩 root1 的两项"
    );
    assert_eq!(
        ids(&query_layout_items_canonical(&c, &f).unwrap()),
        vec![1, 2],
        "canonical 基准序路径同样排除隐藏根"
    );
}
