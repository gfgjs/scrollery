//! 隐藏根排除(V21):库级显隐——隐藏根的媒体须从画廊/canonical/统计/facet/搜索/全选全部消失,
//! 且**无隐藏根时查询逐字节不变**(免 JOIN 红线不触碰)。走真库(migration + 两根 + 媒体),
//! 不只对拍 SQL 字符串——NOT IN 参数错位不报错、只静默漏筛,只有真跑才抓得住。

use super::*;
use crate::db::models::{GalleryFilter, SortSpec, ViewDescriptor, ViewScope};
use crate::db::queries::media::get_app_stats;
use crate::db::queries::scan::{hidden_root_ids, set_scan_root_hidden};

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

/// hidden_root_ids 只列 is_hidden=1 的根;set_scan_root_hidden 双向翻转。
#[test]
fn hidden_root_ids_reflects_toggle() {
    let c = two_roots();
    assert!(hidden_root_ids(&c).unwrap().is_empty(), "初始无隐藏根");
    set_scan_root_hidden(&c, 2, true).unwrap();
    assert_eq!(hidden_root_ids(&c).unwrap(), vec![2]);
    set_scan_root_hidden(&c, 2, false).unwrap();
    assert!(hidden_root_ids(&c).unwrap().is_empty(), "取消隐藏后复空");
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

/// 统计各桶随可见集收窄(隐藏根的项不计入)。
#[test]
fn stats_exclude_hidden_root() {
    let c = two_roots();
    assert_eq!(get_app_stats(&c).unwrap().total_images, 4);
    set_scan_root_hidden(&c, 2, true).unwrap();
    let s = get_app_stats(&c).unwrap();
    assert_eq!(s.total_images, 2, "隐藏 root2 后图片数减半");
    assert_eq!(s.total_items, 2);
}

/// facet(格式弹层)随可见集收窄:隐藏根独有的 gif/webp 不再出现。
#[test]
fn facet_excludes_hidden_root_formats() {
    let c = two_roots();
    assert_eq!(
        list_library_formats(&c).unwrap(),
        vec!["gif", "jpg", "png", "webp"]
    );
    set_scan_root_hidden(&c, 2, true).unwrap();
    assert_eq!(list_library_formats(&c).unwrap(), vec!["jpg", "png"]);
}

/// 全选(view_to_sql)与画廊可见集一致:隐藏根 id 非空 → SQL 含排除谓词;空集 → 不含。
#[test]
fn view_to_sql_excludes_hidden_root() {
    let vd = ViewDescriptor {
        scope: ViewScope::All,
        filter: GalleryFilter::default(),
        sort: SortSpec::default(),
        duplicate_lens: None,
        layout_version: 0,
    };
    let (sql_none, _) = view_to_sql(&vd, &[]).unwrap();
    assert!(
        !sql_none.contains("NOT IN (SELECT id FROM directories"),
        "无隐藏根:不产出排除谓词"
    );
    let (sql_hidden, params) = view_to_sql(&vd, &[2]).unwrap();
    assert!(
        sql_hidden
            .contains("m.directory_id NOT IN (SELECT id FROM directories WHERE root_id IN (?1))"),
        "有隐藏根:产出排除谓词,实得: {sql_hidden}"
    );
    assert_eq!(params.len(), 1, "一个隐藏根 = 一个绑定参数");
}

/// canonical:无隐藏根时逐字节不变(不含排除谓词,保留 `+` partial-index 抑制);有隐藏根时
/// **既加排除又保留 `+` 抑制**(广域默认视图仍走顺序全表扫,不回退 idx_media_sort 随机回表)。
#[test]
fn canonical_sql_preserves_suppression_and_adds_exclusion() {
    let f = MediaFilter::default();
    let (sql_none, _) = canonical_layout_sql(&f, &[]);
    assert!(
        !sql_none.contains("NOT IN (SELECT id FROM directories"),
        "无隐藏根:canonical 不含排除谓词(逐字节不变)"
    );
    assert!(
        sql_none.contains("+m.is_deleted=0"),
        "无隐藏根仍施 `+` 抑制"
    );

    let (sql_hidden, extras) = canonical_layout_sql(&f, &[2]);
    assert!(
        sql_hidden.contains("+m.is_deleted=0"),
        "有隐藏根仍保留 `+` 抑制(顺序全表扫)"
    );
    assert!(
        sql_hidden
            .contains("m.directory_id NOT IN (SELECT id FROM directories WHERE root_id IN (?1))"),
        "有隐藏根追加排除谓词"
    );
    assert_eq!(extras.len(), 1);
}
