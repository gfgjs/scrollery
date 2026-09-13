//! T18 S1：`resolve_selection` / `count_selection` + ViewStale 守门（带真实 DB seed）。

use super::*;
use crate::db::models::{GalleryFilter, SelectionDescriptor, SortSpec, ViewDescriptor, ViewScope};

// 跨域测试定向 import(§5 规则 2:只补编译所需;裸名原经 facade glob 解析)。
use super::super::super::media::{
    batch_set_color_label, batch_set_favorite, batch_set_rating, get_media_item, soft_delete_items,
};

const VER: u64 = 5;

/// seed 一个根 + 目录 + 3 个媒体项（id 1/2/3，sort_datetime 100/200/300）。
fn seeded_db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
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
    // run_migrations 顶部已自注册 TREE_SORT_KEY（方案 B），下方 fixture 回填 UPDATE 依赖它。
    crate::db::migration::run_migrations(&c).unwrap();
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

/// B-file-i 刚性等价契约:filename 基准序的内存派生(`CanonicalFilename` + `derive_order`)
/// 必须与 SQL `push_order_by` 的 filename 序**逐项一致**(none/folder 轴 × asc/desc),否则
/// filename 视图下 SelectAll(view_to_sql)与 flat_ids(内存派生)错位 → 选区漂移。与
/// datetime 的 `canonical_derive_order_matches_sql_order` 同型。文件名刻意取 `img2<img10<img100`
/// 使自然序 != 词典序,真正锁住 NATURAL_CMP 被走到(词典序会把 img10 排到 img2 前)。
#[test]
fn filename_derive_order_matches_sql_order() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES
             (10, 1, '', 'r1'),
             (11, 1, 'Albums', 'Albums'),
             (12, 1, 'albums', 'albums'),
             (13, 1, '相册', '相册'),
             (20, 2, 'albums', 'albums');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 10, 'img2.jpg',   1, 1, 'jpg', 'image', 100, 100, 100, 1),
             (2, 10, 'img10.jpg',  1, 1, 'jpg', 'image', 100, 100, 200, 2),
             (3, 12, 'img1.jpg',   1, 1, 'jpg', 'image', 100, 100, 300, 3),
             (4, 20, 'a.jpg',      1, 1, 'jpg', 'image', 100, 100, 400, 4),
             (5, 12, 'b.jpg',      1, 1, 'jpg', 'image', 100, 100, 500, 5),
             (6, 13, 'z.jpg',      1, 1, 'jpg', 'image', 100, 100, 600, 6),
             (7, 11, 'img100.jpg', 1, 1, 'jpg', 'image', 100, 100, 700, 7);",
    )
    .unwrap();
    // 裸 INSERT 的 tree_sort_key 取 DEFAULT X''；push_order_by folder 分支现读该列，须回填成真键
    // （否则 SQL 目录序退化按 d.id、与内存 build_dir_rank 分歧 → folder 对拍红）。
    c.execute_batch("UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path)")
        .unwrap();

    let filter = MediaFilter::default();
    // filename 基准序（file_name NATURAL_CMP ASC, id ASC）—— items 存自然序，下标即 filename_rank。
    let baseline = query_layout_items_filename_baseline(&c, &filter).unwrap();
    assert_eq!(baseline.len(), 7, "filename 基准应取回全部 7 项");
    let dir_labels = query_dir_labels(&c).unwrap();
    let data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::CanonicalFilename,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&baseline),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: baseline,
        dir_labels,
        filter: filter.clone(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };

    // date+filename **现已纳入**（B-file-iii/D-018 A′：UTC 日桶 `div_euclid` 内存分桶，SQL 去
    // 'localtime' 改 UTC 日界）。本 fixture 全部项落同一 UTC 日（sort_datetime 100-700 < 86400），
    // 故 date+filename == none+filename（单桶）——单桶回归守卫；跨 UTC 午夜的分桶主键由专测
    // `date_filename_derive_matches_sql_order` 锁定。
    for group_by in ["none", "folder", "date"] {
        for sort_order in ["desc", "asc"] {
            let sql_ids: Vec<i64> = query_layout_items(
                &c,
                &filter,
                Some(group_by),
                Some("filename"),
                Some(sort_order),
                false,
            )
            .unwrap()
            .iter()
            .map(|it| it.id)
            .collect();
            let derived_ids: Vec<i64> =
                crate::layout::items_cache::derive_order(&data, group_by, "filename", sort_order)
                    .iter()
                    .map(|it| it.id)
                    .collect();
            assert_eq!(
                derived_ids, sql_ids,
                "filename 内存派生序 != SQL 序：group_by={group_by} sort_order={sort_order}"
            );
            // 手钉 SQL 真值(锁 NATURAL_CMP 确被走到,非「两个错的一致」):
            // 自然序 a < b < img1 < img2 < img10 < img100 < z。
            if group_by == "none" && sort_order == "asc" {
                assert_eq!(
                    sql_ids,
                    vec![4, 5, 3, 1, 2, 7, 6],
                    "none+filename+asc 应为纯自然序(词典序会把 img10/img100 排到 img2 前)"
                );
            }
            if group_by == "folder" && sort_order == "asc" {
                // DFS 目录序 root1[10,11,12,13]→root2[20];组内 filename NATURAL_CMP ASC:
                // 10[img2<img10]=[1,2] 11[img100]=[7] 12[b<img1]=[5,3] 13[z]=[6] 20[a]=[4]。
                assert_eq!(
                    sql_ids,
                    vec![1, 2, 7, 5, 3, 6, 4],
                    "folder+filename+asc 应为 DFS 目录序 + 组内自然序"
                );
            }
        }
    }
}

/// **双键统一缓存**刚性等价契约（双向跨键）：同一份缓存必须能派生**另一轴**的序，且与 SQL
/// 精确序逐项一致，否则跨轴切换后 SelectAll(view_to_sql) 与 flat_ids(内存派生) 错位 → 选区漂移。
/// - datetime 基准 + 惰性 `filename_rank`（经 `query_item_ids_filename_order` 建） → filename 序
///   （none/folder × asc/desc）== SQL filename 序。
/// - filename 基准（items 自带 sort_datetime） → datetime 序（none/date/folder × asc/desc）
///   == SQL datetime 序（免额外查询）。
///
/// fixture 文件名取 `img2<img10<img100`（自然序 != 词典序，锁 NATURAL_CMP 被走到）+ 全异
/// sort_datetime（datetime 序确定）+ 多根/多目录（folder DFS 目录序）。
#[test]
fn dual_key_cross_axis_derive_matches_sql_order() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES
             (10, 1, '', 'r1'),
             (11, 1, 'Albums', 'Albums'),
             (12, 1, 'albums', 'albums'),
             (13, 1, '相册', '相册'),
             (20, 2, 'albums', 'albums');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 10, 'img2.jpg',   1, 1, 'jpg', 'image', 100, 100, 100, 1),
             (2, 10, 'img10.jpg',  1, 1, 'jpg', 'image', 100, 100, 200, 2),
             (3, 12, 'img1.jpg',   1, 1, 'jpg', 'image', 100, 100, 300, 3),
             (4, 20, 'a.jpg',      1, 1, 'jpg', 'image', 100, 100, 400, 4),
             (5, 12, 'b.jpg',      1, 1, 'jpg', 'image', 100, 100, 500, 5),
             (6, 13, 'z.jpg',      1, 1, 'jpg', 'image', 100, 100, 600, 6),
             (7, 11, 'img100.jpg', 1, 1, 'jpg', 'image', 100, 100, 700, 7);",
    )
    .unwrap();
    c.execute_batch("UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path)")
        .unwrap();
    let filter = MediaFilter::default();
    let dir_labels = query_dir_labels(&c).unwrap();

    // ── 方向 1：datetime 基准 → filename 派生（经惰性 filename_rank）──────────────────
    let canonical = query_layout_items_canonical(&c, &filter).unwrap();
    let dt_data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::Canonical,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&canonical),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: canonical,
        dir_labels: dir_labels.clone(),
        filter: filter.clone(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    // 用生产同一函数建 filename_rank：id-only filename 序 → id→位次 → 平行 items 的 rank 数组。
    let fname_ids = query_item_ids_filename_order(&c, &filter).unwrap();
    let rank_of: std::collections::HashMap<i64, u32> = fname_ids
        .iter()
        .enumerate()
        .map(|(r, id)| (*id, r as u32))
        .collect();
    let ranks: Vec<u32> = dt_data.items.iter().map(|it| rank_of[&it.id]).collect();
    dt_data.filename_rank.set(ranks).unwrap();
    // date+filename **现已纳入**（B-file-iii/A′）。本 fixture 单 UTC 日，date 桶主键退化为单桶
    // → date+filename == none+filename；跨 UTC 午夜的分桶由专测 `date_filename_derive_matches_sql_order`
    // 双基准锁定。
    for group_by in ["none", "folder", "date"] {
        for sort_order in ["desc", "asc"] {
            let sql_ids: Vec<i64> = query_layout_items(
                &c,
                &filter,
                Some(group_by),
                Some("filename"),
                Some(sort_order),
                false,
            )
            .unwrap()
            .iter()
            .map(|it| it.id)
            .collect();
            let derived_ids: Vec<i64> = crate::layout::items_cache::derive_order(
                &dt_data, group_by, "filename", sort_order,
            )
            .iter()
            .map(|it| it.id)
            .collect();
            assert_eq!(
                derived_ids, sql_ids,
                "datetime 基准派 filename 序 != SQL：group_by={group_by} sort_order={sort_order}"
            );
        }
    }

    // ── 方向 2：filename 基准 → datetime 派生（免额外查询，items 自带 sort_datetime）───────
    let baseline = query_layout_items_filename_baseline(&c, &filter).unwrap();
    let fn_data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::CanonicalFilename,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&baseline),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: baseline,
        dir_labels,
        filter: filter.clone(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    // datetime 轴任意 group（含 date：epoch_day 是 sort_datetime 单调函数，平铺序 == 分桶行序）。
    for group_by in ["none", "date", "folder"] {
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
            let derived_ids: Vec<i64> = crate::layout::items_cache::derive_order(
                &fn_data, group_by, "datetime", sort_order,
            )
            .iter()
            .map(|it| it.id)
            .collect();
            assert_eq!(
                derived_ids, sql_ids,
                "filename 基准派 datetime 序 != SQL：group_by={group_by} sort_order={sort_order}"
            );
        }
    }
}

/// **B-file-iii / D-018 A′ 刚性对拍**：`date + filename` 内存派生（UTC 日桶 `div_euclid(86400)`
/// 主键 + filename 位次次键 + id 末键）必须与 SQL `ORDER BY date(m.sort_datetime,'unixepoch')
/// {dir}, m.file_name COLLATE NATURAL_CMP {dir}, m.id {dir}`（已去 `'localtime'` 改 UTC 日界）
/// **逐项一致**——否则 date 分组下 filename 视图的 SelectAll(view_to_sql) 与 flat_ids(内存派生)
/// 错位 → 选区漂移。**两个基准都验**（Canonical 惰性 filename_rank / CanonicalFilename 下标）。
///
/// fixture 刻意**跨 3 个 UTC 日**且令 filename 自然序与日桶**交错**（自然序 a<img1<img2<img10<img100
/// 分散在 day0/1/2），真正锁住「日桶是主键、filename 是桶内次键」——单桶 fixture 无法区分。
/// UTC 桶与 SQL 去 `'localtime'` 后同为 UTC → 对拍**与测试机时区无关**（这正是 A′ 相对旧 localtime
/// 的确定性红利，也是旧设计把 date+filename 排除在内存派生外的历史原因）。
#[test]
fn date_filename_derive_matches_sql_order() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    // 单目录即可（date 分组不涉目录序）；跨 UTC 午夜靠 sort_datetime 值：
    //   day0 = [0,86400)  ts 100/200；day1 = [86400,172800) ts 86500/86600；day2 ts 172900。
    // 自然序 a(5) < img1(3) < img2(2) < img10(1) < img100(4)，与日桶交错。
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 10, 'img10.jpg',  1, 1, 'jpg', 'image', 100, 100, 100,    1),
             (2, 10, 'img2.jpg',   1, 1, 'jpg', 'image', 100, 100, 200,    2),
             (3, 10, 'img1.jpg',   1, 1, 'jpg', 'image', 100, 100, 86500,  3),
             (4, 10, 'img100.jpg', 1, 1, 'jpg', 'image', 100, 100, 86600,  4),
             (5, 10, 'a.jpg',      1, 1, 'jpg', 'image', 100, 100, 172900, 5);",
    )
    .unwrap();
    let filter = MediaFilter::default();
    let dir_labels = query_dir_labels(&c).unwrap();

    // SQL 真值 + 手钉期望（锁 NATURAL_CMP 与 UTC 桶确被走到，非「两个错的一致」）。
    // desc(日桶 DESC,桶内 filename DESC,id DESC)：day2[5] day1[img100(4),img1(3)] day0[img10(1),img2(2)]。
    // asc ：day0[img2(2),img10(1)] day1[img1(3),img100(4)] day2[5]。
    let expected_desc = vec![5, 4, 3, 1, 2];
    let expected_asc = vec![2, 1, 3, 4, 5];
    for (sort_order, expected) in [("desc", &expected_desc), ("asc", &expected_asc)] {
        let sql_ids: Vec<i64> = query_layout_items(
            &c,
            &filter,
            Some("date"),
            Some("filename"),
            Some(sort_order),
            false,
        )
        .unwrap()
        .iter()
        .map(|it| it.id)
        .collect();
        assert_eq!(
            &sql_ids, expected,
            "SQL date+filename 真值(UTC 桶+NATURAL_CMP) sort_order={sort_order}"
        );
    }

    // ── 基准 1：Canonical（datetime 基准 + 惰性 filename_rank，经生产同一函数建）─────────
    let canonical = query_layout_items_canonical(&c, &filter).unwrap();
    let dt_data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::Canonical,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&canonical),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: canonical,
        dir_labels: dir_labels.clone(),
        filter: filter.clone(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    let fname_ids = query_item_ids_filename_order(&c, &filter).unwrap();
    let rank_of: std::collections::HashMap<i64, u32> = fname_ids
        .iter()
        .enumerate()
        .map(|(r, id)| (*id, r as u32))
        .collect();
    let ranks: Vec<u32> = dt_data.items.iter().map(|it| rank_of[&it.id]).collect();
    dt_data.filename_rank.set(ranks).unwrap();

    // ── 基准 2：CanonicalFilename（filename 基准，下标即位次）───────────────────────────
    let baseline = query_layout_items_filename_baseline(&c, &filter).unwrap();
    let fn_data = crate::layout::items_cache::ItemsCacheData {
        filter_key: String::new(),
        order: crate::layout::items_cache::CachedOrder::CanonicalFilename,
        data_version: 0,
        id_to_idx: crate::layout::items_cache::build_id_index(&baseline),
        dir_rank: crate::layout::items_cache::build_dir_rank(&dir_labels),
        items: baseline,
        dir_labels,
        filter: filter.clone(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };

    for (label, data) in [("Canonical", &dt_data), ("CanonicalFilename", &fn_data)] {
        for (sort_order, expected) in [("desc", &expected_desc), ("asc", &expected_asc)] {
            let derived_ids: Vec<i64> =
                crate::layout::items_cache::derive_order(data, "date", "filename", sort_order)
                    .iter()
                    .map(|it| it.id)
                    .collect();
            assert_eq!(
                &derived_ids, expected,
                "date+filename 派生 != 真值：基准={label} sort_order={sort_order}"
            );
        }
    }
}

/// 全库 filename fixture（自然序 != 词典序 + 含 favorited 子集）：a<img1<img2<img10<img100<z。
/// favorited = {img2(1), img1(3), a(5)}。供 B-file-iii 全局 rank 两测试共用。
fn global_rank_fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, is_favorited) VALUES
             (1, 10, 'img2.jpg',   1, 1, 'jpg', 'image', 100, 100, 100, 1, 1),
             (2, 10, 'img10.jpg',  1, 1, 'jpg', 'image', 100, 100, 200, 2, 0),
             (3, 10, 'img1.jpg',   1, 1, 'jpg', 'image', 100, 100, 300, 3, 1),
             (4, 10, 'img100.jpg', 1, 1, 'jpg', 'image', 100, 100, 400, 4, 0),
             (5, 10, 'a.jpg',      1, 1, 'jpg', 'image', 100, 100, 500, 5, 1),
             (6, 10, 'z.jpg',      1, 1, 'jpg', 'image', 100, 100, 600, 6, 0);",
    )
    .unwrap();
    c
}

/// **B-file-iii 全局 rank 契约①**：`build_global_filename_rank`（全库 id-only NATURAL_CMP）赋出的
/// 位次，按 rank 升序还原 id 序必须 == `query_item_ids_filename_order`（filename 基准 id 序）。
#[test]
fn global_rank_matches_filename_baseline() {
    let c = global_rank_fixture();
    let filter = MediaFilter::default();
    let global = crate::layout::items_cache::build_global_filename_rank(&c, 7).unwrap();
    assert_eq!(
        global.data_version, 7,
        "data_version 应原样带回(供 stale 校验)"
    );

    let baseline_ids = query_item_ids_filename_order(&c, &filter).unwrap();
    // 手钉自然序真值(锁 NATURAL_CMP 被走到，非词典序)：a<img1<img2<img10<img100<z。
    assert_eq!(
        baseline_ids,
        vec![5, 3, 1, 2, 4, 6],
        "filename 基准应为纯自然序(词典序会把 img10/img100 排到 img2 前)"
    );
    // 全局 rank 升序 == baseline id 序（rank 即枚举下标）。
    let mut by_rank: Vec<(u32, i64)> = global.id_to_rank.iter().map(|(&id, &r)| (r, id)).collect();
    by_rank.sort_unstable();
    let ranked_ids: Vec<i64> = by_rank.into_iter().map(|(_, id)| id).collect();
    assert_eq!(
        ranked_ids, baseline_ids,
        "全局 rank 升序还原序 != filename 基准 id 序"
    );
}

/// **B-file-iii 全局 rank 契约②（filter-invariance）**：全局 rank **限制到任意 filter 子集**后，
/// 按 rank 排序须与「该子集自身的 SQL filename 序」逐项一致——这是「一份全局 rank 服务所有筛选
/// 子集」的正确性根据（natural_cmp 是全序，限制到子集保持相对序）。取 favorited 子集验证。
#[test]
fn global_rank_restricts_to_filtered_order() {
    let c = global_rank_fixture();
    let global = crate::layout::items_cache::build_global_filename_rank(&c, 1).unwrap();

    let fav = MediaFilter {
        favorited_only: Some(true),
        ..MediaFilter::default()
    };
    // 子集自身的 SQL filename 序（真值）。
    let sql_subset = query_item_ids_filename_order(&c, &fav).unwrap();
    // 手钉：favorited = {a(5), img1(3), img2(1)} 自然序 → [5,3,1]。
    assert_eq!(
        sql_subset,
        vec![5, 3, 1],
        "favorited 子集 SQL filename 序应为自然序"
    );

    // 用**全局** rank（全库建，非按 fav 重建）给同一子集排序，须逐项一致。
    let mut by_global = sql_subset.clone();
    by_global.sort_by_key(|id| global.id_to_rank[id]);
    assert_eq!(
        by_global, sql_subset,
        "全局 rank 限制到 favorited 子集 != 该子集 SQL filename 序(filter-invariance 被破)"
    );
}

/// **S 线 §8.2-2 非回归**：filter-invariance 对**格式筛选**同样成立 —— 格式筛选是默认可见基集的
/// 子集，故一份全局 rank 照样服务它，`GlobalFilenameRank` **不需重建**。
///
/// 设计专门写了这条防施工抄错（B-file-iii 2026-07-15 刚 landed）。上面那条用 favorited 子集，
/// 本条用 file_format 子集 —— 两者证的是同一性质在**不同筛选维度**上的成立，缺一不可：
/// 「favorited 子集对」推不出「格式子集也对」，那要靠 natural_cmp 是全序这个前提，而前提
/// 该被断言接住而不是被相信。
///
/// ## fixture 的区分度是逐条设计的（每条各挡一种错误实现）
///
/// - **id 序 ≠ 自然序**：`img1.png` 给 id **8**、`img20.png` 给 id **7** → 自然序是 `[8,7]`
///   而 id 序是 `[7,8]`。若照直觉给 7/8，两者恰好相同 —— 那对「rank 退化成按 id 排」这个
///   错误实现**零区分度**（F-016 同款：样本看起来典型，却证伪不了任何东西）。
/// - **png 在全序里不相邻**：中间隔着 jpg 项 → 「限制到子集保持相对序」是真命题而非平凡恒等。
/// - **png 与 jpg 自然序交错**：`img1.png` 夹在 jpg 项之间，故「先按格式分再排」之类的实现
///   也会翻车。
#[test]
fn global_rank_restricts_to_format_filtered_order() {
    let c = global_rank_fixture();
    // 🔴 id 与自然序**有意反向**：img1.png=id 8、img20.png=id 7。
    c.execute_batch(
        "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key, is_favorited) VALUES
             (8, 10, 'img1.png',  1, 1, 'png', 'image', 100, 100, 700, 7, 0),
             (7, 10, 'img20.png', 1, 1, 'png', 'image', 100, 100, 800, 8, 0);",
    )
    .unwrap();
    let global = crate::layout::items_cache::build_global_filename_rank(&c, 1).unwrap();

    let png = MediaFilter {
        file_formats: Some(vec!["png".into()]),
        ..MediaFilter::default()
    };
    let sql_subset = query_item_ids_filename_order(&c, &png).unwrap();
    // 手钉自然序真值：img1.png(8) < img20.png(7) —— 与 id 序 [7,8] 相反。
    assert_eq!(
        sql_subset,
        vec![8, 7],
        "png 子集 SQL filename 序应为自然序(与 id 序相反)"
    );

    // fixture 自检：png 两项在**全序**里不相邻，否则子集序与全序退化成平凡恒等。
    assert!(
        global.id_to_rank[&8] + 1 < global.id_to_rank[&7],
        "fixture 失去区分度：png 两项在全序里相邻"
    );

    let mut by_global = sql_subset.clone();
    by_global.sort_by_key(|id| global.id_to_rank[id]);
    assert_eq!(
        by_global, sql_subset,
        "全局 rank 限制到格式子集 != 该子集 SQL filename 序(格式维度上 filter-invariance 被破)"
    );
}

/// **S 线 §8.2-1 非回归**：`filter_key` = `MediaFilter` 的 canonical JSON，故加 `file_formats`
/// **自动改键、正确失效**，无需任何缓存迁移。
///
/// 这条不是「相信 serde 会做对」而是**把前提钉住**：`filter_key` 只是 `serde_json::to_string`
/// 的结果（`layout_commands.rs:204`），若日后有人给 `MediaFilter` 的某字段加
/// `#[serde(skip)]`，键就不再随该字段变 —— 缓存于是把两个不同筛选的结果当同一份复用，
/// 而这**不会报错**，只会让画廊显示上一次筛选的内容。
#[test]
fn filter_key_changes_with_file_formats() {
    let key = |f: &MediaFilter| serde_json::to_string(f).unwrap();
    let base = MediaFilter::default();
    let png = MediaFilter {
        file_formats: Some(vec!["png".into()]),
        ..MediaFilter::default()
    };
    let jpg = MediaFilter {
        file_formats: Some(vec!["jpg".into()]),
        ..MediaFilter::default()
    };
    assert_ne!(
        key(&base),
        key(&png),
        "加格式筛选必须改键(否则复用无格式的缓存)"
    );
    assert_ne!(
        key(&png),
        key(&jpg),
        "换格式必须改键(否则 png 复用 jpg 的结果)"
    );
    // 空列表与 None 也应可区分：前者是「显式空」，SQL 侧两者都不加谓词，但键不同不影响正确性
    // （最多多查一次），键相同才危险。此处只断言 None ≠ Some(png) 这条要害。
}

/// folder + filename 的 SQL 特殊排序也必须先按唯一目录分组。两个扫描根具有完全相同的
/// rel_path、文件夹名与文件名时，若缺 directory_id 键，NATURAL_CMP 会把两目录逐文件交错。
#[test]
fn folder_filename_sort_keeps_duplicate_paths_and_names_in_distinct_groups() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap(); // 顶部自注册 TREE_SORT_KEY（方案 B）。
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
         INSERT INTO directories (id, root_id, rel_path, name) VALUES
             (10, 1, 'Shared/Album', 'Album'),
             (20, 2, 'Shared/Album', 'Album');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 10, 'IMG_0001.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 1),
             (2, 20, 'IMG_0001.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 2),
             (3, 10, 'IMG_0002.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 3),
             (4, 20, 'IMG_0002.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 4);",
    )
    .unwrap();
    // push_order_by 读持久列 → 回填：两目录同 rel_path 'Shared/Album'、键相等，目录序由 root
    // 序 (created_at,id) 裁决，dir10(root1) 整组先于 dir20(root2)（方案 B §7）。
    c.execute_batch("UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path)")
        .unwrap();

    let items = query_layout_items(
        &c,
        &MediaFilter::default(),
        Some("folder"),
        Some("filename"),
        Some("asc"),
        false,
    )
    .unwrap();
    let dir_ids: Vec<i64> = items.iter().map(|it| it.dir_id.unwrap()).collect();
    assert_eq!(
        dir_ids,
        vec![10, 10, 20, 20],
        "完整相同的 rel_path/文件名也必须按唯一目录连续成组"
    );
}

/// 回归防线（方案 B 消费者已切列）：folder 目录序由**持久列 d.tree_sort_key 驱动**，而非
/// rel_path 字符串序或现算 TREE_SORT_KEY(rel_path) 函数。手法：故意写入与 rel_path 自然序
/// **相悖**的 tree_sort_key，断言 SQL 目录序 follow 存列 —— 若有人误把 push_order_by 的 ORDER
/// BY 改回读 rel_path 或标量函数，此测试立即红。
#[test]
fn folder_sql_order_driven_by_stored_tree_sort_key_column() {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    // rel_path 自然序 aaa < zzz；但把 tree_sort_key 反写（aaa→大键 X'FF00'、zzz→小键 X'0100'）。
    // 读存列 → zzz(小键)组先出 [12,11]；读 rel_path/函数 → aaa 组先出 [11,12]。二者可区分。
    // （run_migrations 在空 directories 表上跑 V19 回填 = no-op，不覆盖下方显式键。）
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
         INSERT INTO directories (id, root_id, rel_path, name, tree_sort_key) VALUES
             (11, 1, 'aaa', 'aaa', X'FF00'),
             (12, 1, 'zzz', 'zzz', X'0100');
         INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
             (1, 11, 'a.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 1),
             (2, 12, 'z.jpg', 1, 1, 'jpg', 'image', 100, 100, 100, 2);",
    )
    .unwrap();
    let items = query_layout_items(
        &c,
        &MediaFilter::default(),
        Some("folder"),
        Some("datetime"),
        Some("asc"),
        false,
    )
    .unwrap();
    let dir_ids: Vec<i64> = items.iter().map(|it| it.dir_id.unwrap()).collect();
    assert_eq!(
        dir_ids,
        vec![12, 11],
        "folder 目录序须由持久列 tree_sort_key 驱动：zzz(小键 X'0100') 先于 aaa(大键 X'FF00')"
    );
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
fn explicit_returns_ids_regardless_of_version() {
    let c = seeded_db();
    let sel = SelectionDescriptor::Explicit { ids: vec![2, 3] };
    // version 不影响 Explicit。
    assert_eq!(resolve_selection(&c, &sel, 999).unwrap(), vec![2, 3]);
    assert_eq!(count_selection(&c, &sel, 999).unwrap(), 2);
}

#[test]
fn explicit_over_limit_errs() {
    let c = seeded_db();
    let sel = SelectionDescriptor::Explicit {
        ids: vec![0i64; SELECTION_EXPLICIT_MAX + 1],
    };
    assert!(matches!(
        resolve_selection(&c, &sel, 0),
        Err(AppError::Internal(_))
    ));
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

/// 锁住 `map_layout_item` 的位置映射 ↔ `query_layout_items` 的 SELECT 列序：插入已知
/// rating / color_label / is_favorited 的项，跑真实查询，断言映射字段读对位置——防 SELECT
/// 加列后 `row.get(N)` 静默串列。既有 286 测试不覆盖此端到端路径，rating/color_label 的列位
/// 此前仅靠肉眼对齐；本测试把"数对了列"从信念变成断言（补 e526952 起的未测缺口）。
#[test]
fn query_layout_items_maps_scalar_columns_by_position() {
    let c = seeded_db();
    // 3 项各设不同 rating / color_label / favorite —— 若任一列读错位置，会互相串值被断言抓到。
    c.execute_batch(
        "UPDATE media_items SET rating=5, color_label=3, is_favorited=1 WHERE id=3;
         UPDATE media_items SET rating=2, color_label=7, is_favorited=0 WHERE id=2;
         UPDATE media_items SET rating=0, color_label=0, is_favorited=0 WHERE id=1;",
    )
    .unwrap();

    let items = query_layout_items(&c, &MediaFilter::default(), None, None, None, false).unwrap();
    let by_id = |id: i64| items.iter().find(|it| it.id == id).unwrap().clone();

    let it3 = by_id(3);
    assert_eq!(
        (it3.rating, it3.color_label, it3.is_favorited),
        (5, 3, true)
    );
    let it2 = by_id(2);
    assert_eq!(
        (it2.rating, it2.color_label, it2.is_favorited),
        (2, 7, false)
    );
    let it1 = by_id(1);
    assert_eq!(
        (it1.rating, it1.color_label, it1.is_favorited),
        (0, 0, false)
    );
}

/// 锁住 `map_media_item` 的位置映射:color_label 追加在末列(索引 25)后,验证 get_media_item
/// 读对它且不串既有 rating(索引 18)。喂 map_media_item 的三处 SELECT 共用此映射,本测试覆盖
/// get_media_item 路径——防末列追加时漏改某处 SELECT 致 row.get(25) 越界/串值。
#[test]
fn get_media_item_maps_rating_and_color_label() {
    let c = seeded_db();
    c.execute_batch("UPDATE media_items SET rating=4, color_label=6 WHERE id=2;")
        .unwrap();
    let it = get_media_item(&c, 2).unwrap();
    assert_eq!((it.rating, it.color_label), (4, 6));
    // 未设的项取 schema 默认 0（color_label NOT NULL DEFAULT 0），确认非 NULL 越界。
    let it1 = get_media_item(&c, 1).unwrap();
    assert_eq!((it1.rating, it1.color_label), (0, 0));
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

#[test]
fn count_select_all_excluded_is_precise() {
    let c = seeded_db();
    // excluded 含一个真实成员(2) + 一个非成员(999)：精确计数只扣真实交集 → 3-1=2，
    // 不近似为 total - excluded.len()(=3-2=1)。
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![2, 999],
    };
    assert_eq!(count_selection(&c, &sel, VER).unwrap(), 2);
}

/// R1-2 wire 契约锁：前端以 camelCase JSON 构造描述符（kind 小驼峰变体名 + excludedIds /
/// directoryId 字段）。serde 的 enum 级 `rename_all` 不改 struct 变体字段名——此前无前端
/// 消费者、形状从未被实测；本测试把「前端手写的 JSON 能被后端反序列化」钉死为断言。
#[test]
fn selection_descriptor_wire_format_locks_camel_case() {
    use serde_json::json;
    let explicit: SelectionDescriptor =
        serde_json::from_value(json!({ "kind": "explicit", "ids": [1, 2] })).unwrap();
    assert!(matches!(explicit, SelectionDescriptor::Explicit { ids } if ids == vec![1, 2]));

    let select_all: SelectionDescriptor = serde_json::from_value(json!({
        "kind": "selectAll",
        "view": {
            "scope": { "kind": "directory", "directoryId": 10 },
            "filter": { "recentOnly": true },
            "sort": { "groupBy": "date", "sortWithinGroup": "datetime", "sortOrder": "desc" },
            "layoutVersion": 5
        },
        "excludedIds": [2]
    }))
    .unwrap();
    let SelectionDescriptor::SelectAll { view, excluded_ids } = select_all else {
        panic!("应解析为 SelectAll");
    };
    assert_eq!(excluded_ids, vec![2]);
    assert!(matches!(
        view.scope,
        ViewScope::Directory { directory_id: 10 }
    ));
    assert_eq!(view.filter.recent_only, Some(true));
    assert_eq!(view.layout_version, 5);
    // recent_only 须传导到 MediaFilter（R1-2 补字段——此前 recent 视图的 SelectAll 会静默丢谓词）。
    assert_eq!(view.to_media_filter().recent_only, Some(true));
}

/// R1-2：SelectAll − excluded 解析后经分块批量写落库（IPC 命令的 db 层路径）。
#[test]
fn batch_helpers_write_resolved_selection() {
    let c = seeded_db();
    let sel = SelectionDescriptor::SelectAll {
        view: all_view(VER),
        excluded_ids: vec![2],
    };
    let ids = resolve_selection(&c, &sel, VER).unwrap();
    let affected = batch_set_favorite(&c, &ids, true).unwrap();
    assert_eq!(affected, 2, "3 项全选排除 1 项 → 影响 2 行");
    let fav = |id: i64| -> i64 {
        c.query_row(
            "SELECT is_favorited FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!((fav(1), fav(2), fav(3)), (1, 0, 1), "排除项 2 不得被写");

    // 评分/色签越界钳制在 db 层。
    assert_eq!(batch_set_rating(&c, &ids, 99).unwrap(), 2);
    let r1: i64 = c
        .query_row("SELECT rating FROM media_items WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(r1, 5, "rating 钳到 5");
    assert_eq!(batch_set_color_label(&c, &ids, -3).unwrap(), 2);
    let cl1: i64 = c
        .query_row("SELECT color_label FROM media_items WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(cl1, 0, "color_label 钳到 0");
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
