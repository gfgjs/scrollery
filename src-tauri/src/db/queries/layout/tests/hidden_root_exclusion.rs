//! 隐藏根排除(V21):库级显隐——隐藏根的媒体须从画廊/canonical/统计/facet/搜索/全选全部消失,
//! 且**无隐藏根时查询逐字节不变**(免 JOIN 红线不触碰)。走真库(migration + 两根 + 媒体),
//! 不只对拍 SQL 字符串——NOT IN 参数错位不报错、只静默漏筛,只有真跑才抓得住。

use super::*;

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
