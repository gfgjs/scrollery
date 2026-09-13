//! 镜头 lowering 与内存组装的**序列 parity** 测试（2026-09-02 主画廊重复项浏览方案
//! §10.2/§13 P2，最强锁）：`view_to_sql(带 lens 描述符)` 的 SQL 执行序必须**逐位等于**
//! `assemble_lens_groups(list_duplicate_lens_members(...), canonical items, id_to_idx)`
//! 的成员展平序 —— SQL ORDER BY（group_latest / unit_digest / unit_size / sort_datetime /
//! normalized_dir_path / file_name NOCASE / item_id）与内存组装四键任一漂移，在此显式失败，
//! 而非让 SelectAll / GET_VIEW_IDS 解析出的集合与镜头画面静默错序。
//!
//! 与 view_layout_parity.rs 的「同集」parity 互补：那条锁**集合**，这条锁**序列**。
//! 排除项（companion / 软删 / revision 漂移 / stale / offline / 零字节 / 隐藏根 / 独有项）
//! **都带 dedup_index 行**——证明是 eligible 谓词在排除，而非缺数据。

use super::*;
use crate::db::models::{
    DuplicateLensDescriptor, DuplicateLensMode, GalleryFilter, SelectionDescriptor, SortSpec,
    ViewDescriptor, ViewScope, DUPLICATE_LENS_ORDERING_VERSION,
};
use crate::db::queries::list_duplicate_lens_members;
use crate::layout::items_cache::build_id_index;
use crate::layout::lens::assemble_lens_groups;

/// 三根多目录 + 6 个重复组（2/3 成员、跨根同名 rel_path、同时间与 ASCII 大小写排序边界）+
/// 隐藏根。组摘要用可读 ASCII 字节（'gA'=X'6741'…），字节序 0x41<0x42<… 可手算。
fn lens_db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::db::migration::run_migrations(&c).unwrap();
    c.execute_batch(
        "INSERT INTO scan_roots (id, path, alias) VALUES
             (1, '/lens-a', 'A'), (2, '/lens-b', 'B'), (3, '/lens-h', 'H');
         INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES
             (10, 1, NULL, '', 'lens-a', 0),
             (11, 1, 10, 'dup', 'Dup', 1),
             (20, 2, NULL, '', 'lens-b', 0),
             (21, 2, 20, 'dup', 'Dup', 1),
             (30, 3, NULL, '', 'lens-h', 0);
         UPDATE scan_roots SET is_hidden = 1 WHERE id = 3;",
    )
    .unwrap();

    // (id, dir, name, sort_datetime)。目录 10/20 是根目录（rel_path='' → 完整路径 = r.path），
    // 目录 11/21 跨根同名 rel_path='dup'（路径键消歧）。
    for (id, dir, name, dt) in [
        (1, 11, "a.jpg", 500),
        (2, 11, "b.jpg", 500),
        (3, 21, "c.jpg", 700),
        (4, 11, "B.JPG", 600),
        // 同目录重名受 UNIQUE(directory_id,file_name) 限制，用 "aa.jpg" 保持 NOCASE
        // 判别力：BINARY 序会给 "B.JPG"(0x42) < "aa.jpg"(0x61)，NOCASE 序相反。
        (5, 11, "aa.jpg", 600),
        (6, 21, "d.jpg", 800),
        (7, 10, "e.jpg", 800),
        (8, 11, "f.jpg", 100),
        (9, 11, "g.jpg", 300),
        (10, 11, "h.jpg", 200),
        (11, 11, "p.jpg", 650),
        (12, 11, "q.jpg", 650),
        (13, 11, "r.jpg", 650),
        (14, 11, "s.jpg", 650),
        // 排除项：sort_datetime=999，若谓词失效会窜到组首/新增组，parity 立即红。
        (20, 11, "x_companion.jpg", 999),
        (21, 11, "x_deleted.jpg", 999),
        (22, 11, "x_drift.jpg", 999),
        (23, 11, "x_stale.jpg", 999),
        (24, 11, "x_offline.jpg", 999),
        (25, 11, "x_zero.jpg", 999),
        (26, 30, "h1.jpg", 999),
        (27, 30, "h2.jpg", 999),
        (28, 11, "unique.jpg", 999),
    ] {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                 width, height, sort_datetime, cache_key)
             VALUES (?1, ?2, ?3, 10, 1, 'jpg', 'image', 100, 100, ?4, ?1)",
            params![id, dir, name, dt],
        )
        .unwrap();
    }
    c.execute_batch(
        "UPDATE media_items SET companion_of = 1 WHERE id = 20;
         UPDATE media_items SET is_deleted = 1 WHERE id = 21;
         -- revision 漂移：媒体已换代（rev=2），dedup 行还停在 rev=1。
         UPDATE media_items SET source_revision = 2 WHERE id = 22;
         UPDATE media_items SET availability = 'offline' WHERE id = 24;
         UPDATE media_items SET file_size = 0 WHERE id = 25;",
    )
    .unwrap();

    // dedup_index 行：正常组成员 + 「带行但仍被谓词排除」的排除项。
    // 摘要：gA..gF=X'6741'..X'6746'，隐藏根组 gH=X'6748'，独有项 U1=X'7531'。
    // 值全部为测试内代码字面量（id/digest 均非用户输入），故拼 SQL 而非绑定。
    let hv = crate::dedup::DEDUP_HASH_VERSION as i64;
    let row = |id: i64, digest: &str, size: i64, status: &str| {
        format!("({id},1,{hv},X'{digest}',{size},'{status}',1)")
    };
    let rows: Vec<String> = vec![
        // gA：3 成员跨两根（1/2 在 root1，3 在 root2）。
        row(1, "6741", 100, "ready"),
        row(2, "6741", 100, "ready"),
        row(3, "6741", 100, "ready"),
        // gB：NOCASE 大小写边界（B.JPG vs a.jpg）。
        row(4, "6742", 200, "ready"),
        row(5, "6742", 200, "ready"),
        // gC：路径键（根目录 '' vs 子目录，跨根）。
        row(6, "6743", 300, "ready"),
        row(7, "6743", 300, "ready"),
        // gD：组内时间 DESC 三成员。
        row(8, "6744", 400, "ready"),
        row(9, "6744", 400, "ready"),
        row(10, "6744", 400, "ready"),
        // gE / gF：组序平局（组内最新同为 650）→ 摘要字节 ASC 裁决。
        row(11, "6745", 500, "ready"),
        row(12, "6745", 500, "ready"),
        row(13, "6746", 600, "ready"),
        row(14, "6746", 600, "ready"),
        // 排除项（全部带 ready 行，除非另行注明）：
        row(20, "6741", 100, "ready"), // companion：混入 gA 会改其组序与成员数
        row(21, "6744", 400, "ready"), // 软删：混入 gD 会使其组内最新变 999
        row(22, "6744", 400, "ready"), // 漂移：媒体 rev=2 ≠ 行 rev=1
        row(23, "6744", 400, "stale"), // 状态非 ready
        row(24, "6744", 400, "ready"), // offline
        row(25, "6744", 400, "ready"), // 零字节
        row(26, "6748", 700, "ready"), // 隐藏根整组（2 成员，若泄漏会成为最新组）
        row(27, "6748", 700, "ready"),
        row(28, "7531", 800, "ready"), // 独有项：member_count=1
    ];
    c.execute(
        &format!(
            "INSERT INTO dedup_index (item_id, source_revision, hash_version, unit_digest, unit_size, status, checked_at)
             VALUES {}",
            rows.join(",")
        ),
        [],
    )
    .unwrap();
    c
}

/// 镜头便捷构造：全库 + 空 filter（validate 唯一放行形态）+ layout_version=0。
fn lens_view(mode: DuplicateLensMode, show_unique_items: bool) -> ViewDescriptor {
    ViewDescriptor {
        scope: ViewScope::All,
        filter: GalleryFilter::default(),
        sort: SortSpec::default(),
        duplicate_lens: Some(DuplicateLensDescriptor {
            mode,
            show_unique_items,
            ordering_version: DUPLICATE_LENS_ORDERING_VERSION,
        }),
        layout_version: 0,
    }
}

/// view_to_sql 编译出的 SQL 执行取 id——**保持输出序**（parity 锁的就是序列，不排序）。
fn sql_ids_ordered(c: &Connection, v: &ViewDescriptor, hidden_roots: &[i64]) -> Vec<i64> {
    let (sql, params) = view_to_sql(v, hidden_roots).unwrap();
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = c.prepare(&sql).unwrap();
    stmt.query_map(refs.as_slice(), |row| row.get::<_, i64>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// 核心锁（方案 §10.2「同一 lowering」端到端）：view_to_sql(lens) 的 SQL 序 ==
/// assemble_lens_groups(list_duplicate_lens_members, canonical items, id_to_idx) 的
/// 成员展平序，且 == 手算期望序。
#[test]
fn lens_sql_sequence_matches_assemble() {
    let c = lens_db();

    // layout 侧：与生产 compute_lens_blocking 同一管道（成员 → canonical 快照 → 组装）。
    let members = list_duplicate_lens_members(&c).unwrap();
    let items = query_layout_items_canonical(&c, &MediaFilter::default()).unwrap();
    let slices = assemble_lens_groups(&members, &items, &build_id_index(&items));
    let assemble_order: Vec<i64> = slices
        .iter()
        .flat_map(|s| s.members.iter().map(|m| m.id))
        .collect();

    // view_to_sql 侧：SelectAll / 计数解析用的同一编译产物。
    let sql_order = sql_ids_ordered(&c, &lens_view(DuplicateLensMode::Groups, false), &[]);

    assert_eq!(
        sql_order, assemble_order,
        "SQL ORDER BY 与 assemble_lens_groups 内存序漂移"
    );
    // 手算形态锁：组序 gC(800) → gA(700) → gE(650) → gF(650, 摘要字节平局) → gB(600) →
    // gD(300)；组内 时间 DESC → 路径 ASC → 文件名 NOCASE ASC → id ASC。
    assert_eq!(
        sql_order,
        vec![7, 6, 3, 1, 2, 11, 12, 13, 14, 5, 4, 9, 10, 8]
    );
}

/// eligible 谓词排除项：companion / 软删 / 漂移 / stale / offline / 零字节 / 隐藏根 /
/// 独有项均不进镜头集合（fixture 中它们都带 dedup_index 行，排除必出自谓词）。
#[test]
fn lens_set_excludes_ineligible_members() {
    let c = lens_db();
    let got = sql_ids_ordered(&c, &lens_view(DuplicateLensMode::Groups, false), &[]);
    for id in [20, 21, 22, 23, 24, 25, 26, 27, 28] {
        assert!(!got.contains(&id), "排除项 {id} 泄漏进镜头集合");
    }
    assert_eq!(got.len(), 14, "镜头集合 = 6 组共 14 名成员");
}

/// folders 模式（P3）显式拒绝：不静默按 groups 降维（validate 先行，合法形态亦拒）。
/// 纯 SQL 编译路径，不触 DB。
#[test]
fn folders_mode_still_unsupported() {
    match view_to_sql(&lens_view(DuplicateLensMode::Folders, false), &[]) {
        Err(AppError::DuplicateLensUnsupported) => {}
        Err(e) => panic!("期望 DuplicateLensUnsupported，实得 {e:?}"),
        Ok(_) => panic!("folders 模式不得静默按 groups 降维"),
    }
    // SQL 形态抽查：groups lowering 含窗口计数与 NOCASE 文件名键（路径表达式由
    // dedup.normalized_dir_path_sql 单一事实源保证，此处不重复字符串比对）。
    let (sql, params) = view_to_sql(&lens_view(DuplicateLensMode::Groups, false), &[]).unwrap();
    assert!(sql.contains("PARTITION BY di.unit_digest, di.unit_size"));
    assert!(sql.contains("file_name COLLATE NOCASE ASC"));
    assert_eq!(
        params.len(),
        3,
        "?1=hash_version ?2=include_offline ?3=include_zero_byte"
    );
}

/// hidden_roots 对镜头为**冗余语义**：eligible 的 `r.is_hidden=0` 与 `hidden_root_ids()`
/// （SELECT id FROM scan_roots WHERE is_hidden=1）是同一事实源 scan_roots.is_hidden 的
/// 正反两面，隐藏根排除已内含（普通 All 视图经 push_root_exclusion 达成同一结果语义）。
/// 传参与否集合不变——调用方传过期 hidden_roots 也不会引入漂移。
#[test]
fn hidden_roots_param_is_redundant_for_lens() {
    let c = lens_db();
    let with = sql_ids_ordered(&c, &lens_view(DuplicateLensMode::Groups, false), &[3]);
    let without = sql_ids_ordered(&c, &lens_view(DuplicateLensMode::Groups, false), &[]);
    assert_eq!(
        with, without,
        "镜头集不因 hidden_roots 入参而变（同一排除语义）"
    );
    assert!(
        !with.contains(&26) && !with.contains(&27),
        "隐藏根媒体两侧均不出现"
    );
}

/// 任务 3（选择契约闭环）：resolve_selection(SelectAll{lens view, excluded}) 走
/// view_to_sql → 镜头 lowering，返回 id 集 = 镜头全集 − excluded（保持视图序）；
/// count_selection 精确扣除 excluded ∩ view（列名 `id` 契约）。
#[test]
fn select_all_on_lens_view_resolves_via_lowering() {
    let c = lens_db();
    let sel = SelectionDescriptor::SelectAll {
        view: Box::new(lens_view(DuplicateLensMode::Groups, false)),
        excluded_ids: vec![1, 9999], // 9999 不在视图内：交集计数必须忽略
    };
    let got = resolve_selection(&c, &sel, 0).unwrap();
    assert_eq!(
        got,
        vec![7, 6, 3, 2, 11, 12, 13, 14, 5, 4, 9, 10, 8],
        "镜头全集 − excluded（扣除后保持视图序）"
    );
    let n = count_selection(&c, &sel, 0).unwrap();
    assert_eq!(n, 13, "count = 全集 14 − (excluded ∩ view = 仅 id 1)");
}
