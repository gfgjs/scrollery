// src-tauri/src/scanner/fast_scan/tests.rs
//! run_fast_scan 单测四域(自 fast_scan.rs 结构性拆分,tierB-1)。

use super::*;

#[cfg(test)]
mod exotic_seed_gate_tests {
    //! builtin offering(D-OCR-5 / D-427 / D-444③)扫描器面回归:验证 run_fast_scan 主循环/
    //! suspect 定案分支共用的 [`seed_gate_admits`] 守卫——「合成标记 builtin」(如 exotic-ocr,
    //! 非真实扩展名)命中须等同 NoOffering;「真实扩展名叠加 builtin」(如 D-427 一期 RAW cr2)
    //! 命中必须放行,否则 RAW exotic 任务永不播种(P1 挡死);「media_kind=video 的 builtin」
    //! (video-extended rmvb/vob)命中须被掐(单轨走 video 派生链,不双播 exotic,D-444③)。
    use super::*;

    fn fake_builtin_catalog() -> CatalogSnapshot {
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"exotic-ocr","name":"OCR","media_kind":"image","formats":["ocr"],
           "capabilities":["text"],"license_tier":"paid","sku":"ocr-engine-2026",
           "platforms":[],"min_host_version":"0.1.0","distribution":"builtin"},
          {"plugin_id":"exotic-image-psd","name":"PSD","media_kind":"image","formats":["psd"],
           "capabilities":["thumbnail"],"license_tier":"paid","sku":"psd-engine-2026",
           "platforms":[],"min_host_version":"0.1.0"},
          {"plugin_id":"exotic-raw","name":"RAW","media_kind":"image","formats":["cr2"],
           "capabilities":["thumbnail"],"license_tier":"free",
           "platforms":[],"min_host_version":"0.1.0","distribution":"builtin"},
          {"plugin_id":"video-extended","name":"VIDEO","media_kind":"video","formats":["rmvb"],
           "capabilities":["thumbnail"],"license_tier":"free",
           "platforms":[],"min_host_version":"0.1.0","distribution":"builtin"}
        ]}"#;
        CatalogSnapshot::parse(json).expect("fixture catalog 应可解析")
    }

    /// 直接调用生产守卫(单一真源 [`seed_gate_admits`]),不再另立镜像谓词。
    fn seed_gate_passes(cat: &CatalogSnapshot, fmt: &str) -> bool {
        cat.resolve_format(fmt)
            .filter(|o| seed_gate_admits(o, fmt))
            .is_some()
    }

    #[test]
    fn synthetic_builtin_offering_is_gated_out_of_seeding_path() {
        let cat = fake_builtin_catalog();
        // "ocr" 是合成标记(非真实扩展名),classify_media_type 恒 None → 仍应被挡。
        assert!(
            !seed_gate_passes(&cat, "ocr"),
            "合成标记 builtin offering 不该进入 exotic 归类/播种分支"
        );
    }

    #[test]
    fn non_builtin_offering_still_passes_the_gate() {
        // 对照组:非 builtin(如 psd)不受该守卫影响,仍正常进入归类/播种分支。
        let cat = fake_builtin_catalog();
        let off = cat
            .resolve_format("psd")
            .filter(|o| seed_gate_admits(o, "psd"))
            .expect("非 builtin offering 应通过守卫");
        assert_eq!(off.plugin_id, "exotic-image-psd");
    }

    #[test]
    fn raw_builtin_offering_with_real_extension_passes_the_gate() {
        // D-427 一期免费 RAW:cr2 是真实扩展名(classify_media_type 命中 Image),
        // 即使 offering 是 builtin 也必须放行播种,否则 RAW exotic 任务永不入队(P1 挡死)。
        let cat = fake_builtin_catalog();
        assert!(
            seed_gate_passes(&cat, "cr2"),
            "真实扩展名叠加的 builtin RAW offering 必须能播种"
        );
    }

    #[test]
    fn video_builtin_offering_is_gated_out_to_avoid_double_track() {
        // D-444③ A4 裁决:rmvb 已入 utils::format 表(classify=video),video-extended 是
        // builtin+video → 缩略图走常规 video 派生链(V5 ffmpeg 桥),**不**再播 exotic Thumbnail
        // 任务(双轨会双写同一 thumb_path)。故此守卫必须掐掉 builtin+video 的播种。
        let cat = fake_builtin_catalog();
        assert_eq!(
            classify_media_type("rmvb"),
            Some(crate::utils::format::MediaType::Video),
            "前提:rmvb 已入表且分类为 video"
        );
        assert!(
            !seed_gate_passes(&cat, "rmvb"),
            "builtin+video offering 不得播 exotic 缩略图任务(单轨走 video 派生链)"
        );
    }
}

#[cfg(test)]
mod finalize_tests {
    use super::*;

    /// 内存库 + 一个 scan_root(id=1)/目录(id=10)/卷=5，一项在线媒体(id=100，未在 seen)。
    fn db_with_one_missing_candidate() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1');
             INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id, availability)
             VALUES (100, 10, 'a.jpg', 0,0,'jpg','image',0,0,0,0, 5, 'online');",
        )
        .unwrap();
        c
    }

    fn avail(c: &Connection, id: i64) -> String {
        c.query_row(
            "SELECT availability FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 四道闸全过：完整 + 在线 + 卷5 + 项不在 seen → 标 missing。
    #[test]
    fn all_gates_pass_marks_missing() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new(); // 100 未出现
        let n = finalize_missing_detection(&c, 1, true, 0, true, Some(5), &seen).unwrap();
        assert_eq!(n, 1);
        assert_eq!(avail(&c, 100), "missing");
    }

    /// 完整门闩拦截：扫描不完整（有遍历错误）→ 即便项真缺失也**不标**（最关键红线）。
    #[test]
    fn incomplete_walk_blocks_deletion() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new();
        let n = finalize_missing_detection(&c, 1, false, 3, true, Some(5), &seen).unwrap();
        assert_eq!(n, 0, "不完整扫描绝不删除");
        assert_eq!(avail(&c, 100), "online");
    }

    /// TOCTOU 拦截：写删前复查卷已离线 → 不标（防中途拔盘误删）。
    #[test]
    fn offline_at_recheck_blocks_deletion() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new();
        let n = finalize_missing_detection(&c, 1, true, 0, false, Some(5), &seen).unwrap();
        assert_eq!(n, 0, "卷离线绝不删除");
        assert_eq!(avail(&c, 100), "online");
    }

    /// 卷未识别（volume_id=None）：在线集为空 → 不标（宁可不删）。
    #[test]
    fn unidentified_volume_marks_nothing() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new();
        let n = finalize_missing_detection(&c, 1, true, 0, true, None, &seen).unwrap();
        assert_eq!(n, 0, "未识别卷 → 空在线集 → 不标");
        assert_eq!(avail(&c, 100), "online");
    }

    /// 项在 seen（本次出现）→ 不标（守门3）。
    #[test]
    fn seen_item_not_marked() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::from([100i64]);
        let n = finalize_missing_detection(&c, 1, true, 0, true, Some(5), &seen).unwrap();
        assert_eq!(n, 0);
        assert_eq!(avail(&c, 100), "online");
    }
}

#[cfg(test)]
mod dir_baseline_tests {
    //! T17a 目录剪枝基线：扫描期写入 directories.mtime + 直接 media_count。
    use super::*;

    /// ensure_dir_chain 应把目录的文件系统 mtime 写入 directories.mtime（含递归创建的祖先目录）。
    #[test]
    fn ensure_dir_chain_persists_dir_mtime() {
        let tmp = std::env::temp_dir().join(format!("scrollery_t17a_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("sub")).unwrap();

        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![tmp.to_string_lossy()],
        )
        .unwrap();

        let mut dir_mtime_cache = std::collections::HashMap::new();
        prefetch_dir_mtimes(&tmp, "sub", &mut dir_mtime_cache);
        let tx = c.transaction().unwrap();
        let mut cache = std::collections::HashMap::new();
        let id = ensure_dir_chain(&tx, 1, "sub", &mut cache, "R", &dir_mtime_cache).unwrap();
        let stored: Option<i64> = tx
            .query_row(
                "SELECT mtime FROM directories WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        // 根目录（rel_path=""）由递归创建，同样应有 mtime 基线。
        let root_mtime: Option<i64> = tx
            .query_row(
                "SELECT mtime FROM directories WHERE root_id=1 AND rel_path=''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        tx.commit().unwrap();

        let actual = std::fs::metadata(tmp.join("sub"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(stored, Some(actual), "sub 目录应写入其 FS mtime 基线");
        assert!(root_mtime.is_some(), "递归创建的根目录也应有 mtime 基线");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// set_directory_media_counts 是**绝对覆盖**（非累加）——剪枝基线须反映本次真实直接计数；
    /// 同一次调用里多目录一次写回（收尾事务内批量 UPDATE）。
    #[test]
    fn set_directory_media_counts_overwrites() {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name, media_count)
                VALUES (10, 1, '', 'r', 99), (11, 1, 'sub', 'sub', 5);",
        )
        .unwrap();
        let counts = std::collections::HashMap::from([(10i64, 7i64), (11, 0)]);
        set_directory_media_counts(&c, &counts).unwrap();
        let read = |id: i64| -> i64 {
            c.query_row(
                "SELECT media_count FROM directories WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(read(10), 7, "应绝对覆盖为 7，而非在 99 上累加");
        assert_eq!(read(11), 0, "本轮计数为 0 的目录同样要覆盖写回");
    }
}

#[cfg(test)]
mod quick_scan_tests {
    //! T17b opt-in 快速扫描剪枝判定（decide_dir_pruned）：mtime 比对 + 回填 seen 防误删。
    use super::*;

    fn fs_mtime(p: &Path) -> i64 {
        std::fs::metadata(p)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// 建临时目录（取其真实 FS mtime）+ 内存库（scan_root=1，根目录 id=10），返回 (Connection, tmp)。
    /// `baseline_mtime` = 写入 directories.mtime 的基线值（用 None 表示不写、即 NULL）——
    /// J1 修复后 decide_dir_pruned 不再读这一列，但仍保留写入以贴近真实 DB 状态（其它测试
    /// 如 ensure_dir_chain_persists_dir_mtime 依赖它），快照另由 `snapshot_of` 单独构造。
    fn setup(tag: &str, baseline_mtime: Option<i64>) -> (Connection, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!("scrollery_t17b_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![tmp.to_string_lossy()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO directories (id, root_id, rel_path, name, mtime) VALUES (10, 1, '', 'r', ?1)",
            params![baseline_mtime],
        )
        .unwrap();
        (c, tmp)
    }

    /// 构造 decide_dir_pruned 的快照参数：`rel_path=""` → `mtime`（None 即快照未命中/无基线）。
    /// J1 修复后基线来自这份「扫描启动时一次性加载」的快照，而非活 DB 行。
    fn snapshot_of(rel_path: &str, mtime: Option<i64>) -> std::collections::HashMap<String, i64> {
        let mut m = std::collections::HashMap::new();
        if let Some(v) = mtime {
            m.insert(rel_path.to_string(), v);
        }
        m
    }

    /// mtime 未变 → 可剪枝，且该目录全部**未删**媒体 id 回填 seen；**已删**项不得回填。
    #[test]
    fn unchanged_dir_is_pruned_and_reseeds() {
        let (c, tmp) = setup("prune", None);
        // 基线对齐到 setup 建目录后的真实 FS mtime（模拟「上次扫描已记录、之后未变」）。
        let actual = fs_mtime(&tmp);
        c.execute(
            "UPDATE directories SET mtime=?1 WHERE id=10",
            params![actual],
        )
        .unwrap();
        c.execute_batch(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, is_deleted)
             VALUES (100,10,'a.jpg',0,0,'jpg','image',0,0,0,0,0),
                    (101,10,'a.mov',0,0,'mov','video',0,0,0,0,0),
                    (102,10,'gone.jpg',0,0,'jpg','image',0,0,0,0,1);",
        )
        .unwrap();

        let snapshot = snapshot_of("", Some(actual));
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "", &tmp.join("a.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);

        assert!(pruned, "mtime 未变应判可剪枝");
        assert!(
            seen.contains(&100) && seen.contains(&101),
            "目录全部未删媒体 id（含 companion mov）应回填 seen 防误删"
        );
        assert!(!seen.contains(&102), "已删项不得回填 seen（不复活）");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// mtime 变（基线比真实早）→ 不剪枝（直接子项可能增/删/改名，必须处理），seen 不动。
    #[test]
    fn changed_mtime_not_pruned() {
        let (c, tmp) = setup("changed", Some(0)); // 基线 mtime=0，必与真实不符
        let snapshot = snapshot_of("", Some(0));
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "", &tmp.join("a.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(!pruned, "mtime 不符必须处理");
        assert!(seen.is_empty(), "未剪枝不应回填 seen");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 基线 mtime 为 NULL（新目录 / 历史无基线）→ 保守不剪枝。
    #[test]
    fn null_baseline_not_pruned() {
        let (c, tmp) = setup("null", None);
        let snapshot = snapshot_of("", None); // 空快照：无基线
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "", &tmp.join("a.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(!pruned, "无基线应保守处理");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 目录在 DB 无行（全新目录，快照也自然未命中）→ 不剪枝。
    #[test]
    fn missing_dir_row_not_pruned() {
        let (c, tmp) = setup("missing", Some(0));
        let snapshot = snapshot_of("", Some(0)); // 快照里没有 "sub" 这一项
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        // rel_path="sub" 既不在快照也不在 directories 表中 → 不剪枝。
        let pruned =
            decide_dir_pruned(&tx, 1, "sub", &tmp.join("sub/a.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(!pruned, "快照未命中应不剪枝");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 快照命中但活 DB 行已消失（理论边界，如目录被并发删除）→ 保守不剪枝。
    #[test]
    fn snapshot_hit_but_db_row_gone_not_pruned() {
        let (c, tmp) = setup("row-gone", None);
        // 快照里手工塞一条 "sub" → 值随意（不影响本用例，因为走不到 mtime 比较就已 return）。
        let snapshot = snapshot_of("sub", Some(123));
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "sub", &tmp.join("sub/a.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(!pruned, "快照命中但活行不存在应保守不剪枝");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── J1 表征测试：祖先链覆写不再污染剪枝基线 ─────────────────────────────
    // 快照只在扫描启动时一次性捕获,本轮自身的写入绝不回改它。

    /// 复现 J1 漏扫序列：父目录 P 带直接文件、子目录 P/C 带文件；先对 P/C 内文件跑
    /// ensure_dir_chain（模拟 walkdir 无序遍历中「先降到子目录、顺路把祖先链全部 upsert
    /// 一遍」，这一步会把 P 的活 DB mtime 覆写为当前 FS mtime），再用**扫描启动时预先加载
    /// 好的快照**（未被上一步覆写）对 P 调用 decide_dir_pruned —— 断言不剪枝，因为 P 目录
    /// 本身在快照锚定的时间点之后新增了一个直接文件（真实 FS mtime 已变，快照仍是旧值）。
    #[test]
    fn ancestor_chain_overwrite_does_not_corrupt_pruning_baseline() {
        let tmp = std::env::temp_dir().join(format!("scrollery_j1_repro_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let p_dir = tmp.join("P");
        let c_dir = p_dir.join("C");
        std::fs::create_dir_all(&c_dir).unwrap();
        std::fs::write(c_dir.join("c.jpg"), b"c").unwrap();
        // mtime 钉定(复核修):create_dir_all 与下方 write(new.jpg) 仅隔数 ms,as_secs 秒级
        // 截断会让基线与新 mtime 撞同秒,后面的 assert_ne 双向 flaky。把 P 钉到确定的过去值
        // (2000-01-01),使「新增直接文件改变 P 的 mtime」成为确定事实而非撞秒运气。
        filetime::set_file_mtime(&p_dir, filetime::FileTime::from_unix_time(946_684_800, 0))
            .unwrap();

        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![tmp.to_string_lossy()],
        )
        .unwrap();

        // 首轮全扫建基线：递归 ensure_dir_chain 到 P、P/C，写入各自当前 FS mtime。
        {
            let mut dir_mtime_cache = std::collections::HashMap::new();
            prefetch_dir_mtimes(&tmp, "P/C", &mut dir_mtime_cache);
            let tx = c.transaction().unwrap();
            let mut cache = std::collections::HashMap::new();
            ensure_dir_chain(&tx, 1, "P/C", &mut cache, "R", &dir_mtime_cache).unwrap();
            tx.commit().unwrap();
        }

        // 扫描启动时一次性加载快照（=「上一轮扫描终态」，此刻就是首轮刚写的基线）。
        let snapshot = load_directory_mtime_snapshot(&c, 1).unwrap();
        let p_baseline_before = *snapshot.get("P").expect("首轮应已为 P 写入基线");

        // P 新增一个直接文件，改变 P 的真实 FS mtime（模拟"父目录新增直接文件"）。
        std::fs::write(p_dir.join("new.jpg"), b"new").unwrap();
        let p_mtime_after_new_file = std::fs::metadata(&p_dir)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // P 的基线已被钉到 2000-01-01,新增文件使 FS 把 P 的 mtime 更新为当前时间,必不同。
        assert_ne!(
            p_mtime_after_new_file, p_baseline_before,
            "新增直接文件后 P 的真实 FS mtime 应变化（基线已钉过去值，此处必不同）"
        );

        // 模拟 walkdir 无序遍历：先访问到 P/C 内的文件，其 ensure_dir_chain 递归下降会
        // 顺路把祖先 P 的活 DB mtime 覆写为当前值（=治愈前 bug 的直接原因）。
        {
            let mut dir_mtime_cache = std::collections::HashMap::new();
            prefetch_dir_mtimes(&tmp, "P/C", &mut dir_mtime_cache);
            let tx = c.transaction().unwrap();
            let mut cache = std::collections::HashMap::new();
            ensure_dir_chain(&tx, 1, "P/C", &mut cache, "R", &dir_mtime_cache).unwrap();
            tx.commit().unwrap();
        }
        // 活 DB 行此刻确已被覆写为新值（证明覆写确实发生，不是测试没搭好台）。
        let live_p_mtime: i64 = c
            .query_row(
                "SELECT mtime FROM directories WHERE root_id=1 AND rel_path='P'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            live_p_mtime, p_mtime_after_new_file,
            "祖先链应已把 P 的活 DB mtime 覆写为新增文件后的当前值"
        );

        // 关键断言：用**未被覆写的快照**（p_baseline_before）对 P 的直接文件 new.jpg 做剪枝判定，
        // 必须判「不剪枝」——因为快照锚定的旧基线与当前 FS mtime 不符，新文件必须被处理。
        // 若这里错误读了已被覆写的活行（旧 bug 行为），cur==stored 恒成立、会误判剪枝、漏扫。
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "P", &p_dir.join("new.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(
            !pruned,
            "J1：基线必须来自快照而非被覆写的活行，P 新增直接文件不得被剪枝"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 阶段3集成:QuickDirPruner 挂在 walker 上,未变根目录的直接文件在 WalkedFile
    /// stat 之前就被跳过(不产出),且该目录全部未删媒体 id 已回填 seen 防误删。
    #[test]
    fn walker_quick_pruner_skips_direct_files_and_backfills_seen() {
        let (c, tmp) = setup("walker-quick", None);
        std::fs::write(tmp.join("a.jpg"), b"x").unwrap();
        let actual = fs_mtime(&tmp);
        c.execute(
            "UPDATE directories SET mtime=?1 WHERE id=10",
            params![actual],
        )
        .unwrap();
        c.execute_batch(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, is_deleted)
             VALUES (300,10,'a.jpg',0,0,'jpg','image',0,0,0,0,0);",
        )
        .unwrap();

        init_seen_table(&c, 1).unwrap();
        let writer = std::sync::Mutex::new(c);
        let snapshot = snapshot_of("", Some(actual));
        let pruner = QuickDirPruner {
            writer: &writer,
            root_id: 1,
            seen: SeenWriter::new(1),
            snapshot: &snapshot,
            decisions: std::cell::RefCell::new(std::collections::HashMap::new()),
        };
        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let mut walker = MediaWalker::new(&tmp, &catalog, &cancel).with_dir_pruner(Some(&pruner));
        let mut files = Vec::new();
        for f in walker.by_ref() {
            files.push(f);
        }
        let outcome = walker.finish();

        assert!(outcome.complete, "quick 剪枝不得破坏遍历完整性");
        assert!(
            files.is_empty(),
            "根目录 mtime 未变 → 直接文件不应产出 WalkedFile"
        );
        let conn = writer.lock().unwrap();
        let seen_count: i64 = conn
            .query_row("SELECT count(*) FROM _mm_seen_r1 WHERE id=300", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            seen_count, 1,
            "被剪枝目录的全部未删媒体 id 必须已流式回填本根 seen 表防误删"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 护住正向路径：快照命中且 FS mtime 确实未变时，仍应正确剪枝（不能为修 J1 过度保守）。
    #[test]
    fn snapshot_hit_and_mtime_unchanged_still_prunes() {
        let tmp =
            std::env::temp_dir().join(format!("scrollery_j1_positive_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![tmp.to_string_lossy()],
        )
        .unwrap();
        {
            let mut dir_mtime_cache = std::collections::HashMap::new();
            prefetch_dir_mtimes(&tmp, "", &mut dir_mtime_cache);
            let tx = c.transaction().unwrap();
            let mut cache = std::collections::HashMap::new();
            ensure_dir_chain(&tx, 1, "", &mut cache, "R", &dir_mtime_cache).unwrap();
            tx.commit().unwrap();
        }
        c.execute_batch(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, is_deleted)
             VALUES (200,1,'x.jpg',0,0,'jpg','image',0,0,0,0,0);",
        )
        .unwrap();
        // directory id 是 ensure_dir_chain 首次插入的自增 id，media_items.directory_id 须对齐。
        let dir_id: i64 = c
            .query_row(
                "SELECT id FROM directories WHERE root_id=1 AND rel_path=''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        c.execute(
            "UPDATE media_items SET directory_id=?1 WHERE id=200",
            params![dir_id],
        )
        .unwrap();

        // 未做任何 FS 变更，直接加载快照并判定 —— FS mtime 与快照一致 → 应剪枝。
        let snapshot = load_directory_mtime_snapshot(&c, 1).unwrap();
        let tx = c.unchecked_transaction().unwrap();
        let mut seen = HashSet::new();
        let pruned =
            decide_dir_pruned(&tx, 1, "", &tmp.join("x.jpg"), &snapshot, &mut seen).unwrap();
        drop(tx);
        assert!(pruned, "快照命中且 mtime 未变应正确剪枝");
        assert!(seen.contains(&200), "剪枝时应回填该目录未删媒体 id");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod p1_4_freshness_tests {
    //! P1-4 扫描新鲜度契约（真临时目录 → 真扫描入口 → 真 DB，非单点 mock）：
    //!  - 完整扫描必须发现「父目录 mtime 不变」的**就地文件变更**（同 size、内容变），推进
    //!    `source_revision`，并使旧缩略图/派生/AI/人脸/去重结果全部失效；
    //!  - 携带旧 `source_revision` 的迟到写（缩略图回写、去重摘要）必须在真库上被拒绝，
    //!    且换当前代次即放行（拒绝来自代次而非其它字段被写坏）；
    //!  - 上一轮未成功收尾（取消/中断）后，下一轮即使请求 quick 也强制全量（F-001）。
    use super::*;
    use std::io::Write;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("scrollery_p14_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 内存库 + scan_root(id=1) 指向 `root`。FK 关闭：本域不构造卷/父目录链（与既有扫描测试同款）。
    fn db_for_root(root: &Path) -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![root.to_string_lossy()],
        )
        .unwrap();
        c
    }

    /// 生产同款 Channel：回调即丢（本域只断言 DB 终态）。
    fn silent_channel() -> Channel<ScanChannelPayload> {
        Channel::new(|_| Ok(()))
    }

    /// 走生产入口 `run_fast_scan`（无 generation 闸门的真实扫描路径）。
    fn scan(writer: &Mutex<Connection>, root: &Path, run_id: &str, quick: bool) -> Result<u64> {
        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let channel = silent_channel();
        run_fast_scan(
            writer,
            1,
            run_id,
            &root.to_string_lossy(),
            &catalog,
            &channel,
            &cancel,
            &|| {},
            quick,
        )
    }

    fn config_value(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row(
            "SELECT value FROM app_config WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
    }

    fn item_count(writer: &Mutex<Connection>) -> i64 {
        writer
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM media_items", [], |r| r.get(0))
            .unwrap()
    }

    fn scalar_i64(conn: &Connection, sql: &str, id: i64) -> i64 {
        conn.query_row(sql, params![id], |r| r.get(0)).unwrap()
    }

    /// 完整扫描检出「目录 mtime 不变」的就地编辑，并让旧缩略图/派生/AI/人脸/去重结果失效，
    /// 同时拒绝携带旧代次的迟到写。quick 作为对照漏掉同一处变更（明示的设计取舍）。
    #[test]
    fn full_scan_detects_in_place_edit_and_rejects_stale_derived_writes() {
        use crate::db::queries as q;

        let root = tmp_root("inplace");
        let file = root.join("a.jpg");
        // 16B ≤ 64MB → content_fingerprint 走**全文** sha256（不带抽样漏检边界）。
        std::fs::write(&file, b"AAAAAAAAAAAAAAAA").unwrap();
        let writer = Mutex::new(db_for_root(&root));

        // ── 第一轮全量：入库 + 建立目录 mtime 基线 ────────────────────────────────
        assert_eq!(scan(&writer, &root, "run-1", false).unwrap(), 1);
        let (item_id, cache_key, baseline_mtime) = {
            let c = writer.lock().unwrap();
            let (id, revision, cache_key): (i64, i64, i64) = c
                .query_row(
                    "SELECT id, source_revision, cache_key FROM media_items WHERE file_name='a.jpg'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(revision, 1, "新项从 source_revision=1 起算");
            let mtime: i64 = c
                .query_row(
                    "SELECT mtime FROM directories WHERE root_id=1 AND rel_path=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            (id, cache_key, mtime)
        };
        assert_eq!(
            read_dir_mtime(&root),
            Some(baseline_mtime),
            "首轮全量应写入当前目录 mtime 基线（quick 剪枝的前置条件）"
        );

        // ── 就地改写：同 size、内容不同；文件 mtime 前移，目录 mtime 还原为基线值 ──
        // 「目录 mtime ≡ 基线」正是 quick 的剪枝判据；本用例要证明完整扫描不据它跳过。
        let dir_baseline =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&root).unwrap());
        {
            let mut f = std::fs::File::create(&file).unwrap();
            f.write_all(b"BBBBBBBBBBBBBBBB").unwrap();
            f.sync_all().unwrap();
        }
        filetime::set_file_mtime(
            &file,
            filetime::FileTime::from_unix_time(dir_baseline.unix_seconds() + 7, 0),
        )
        .unwrap();
        filetime::set_file_mtime(&root, dir_baseline).unwrap();
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&root).unwrap()),
            dir_baseline,
            "前置条件：就地改写后父目录 mtime 必须与基线完全一致"
        );
        assert_eq!(read_dir_mtime(&root), Some(baseline_mtime));

        // ── 对照（设计取舍，非缺陷）：目录 mtime 未变 → quick 剪掉本目录 → 检出不到 ──
        assert_eq!(scan(&writer, &root, "run-quick", true).unwrap(), 0);
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT source_revision FROM media_items WHERE id=?1",
                    item_id
                ),
                1,
                "quick 剪枝目录内的就地编辑按设计不检出（完整扫描兜底）"
            );
        }

        // ── 播种「旧版本（rev=1）」派生结果：缩略图/EXIF/派生任务/AI/人脸/去重摘要 ──
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                q::update_thumb_result_if_current(
                    &c,
                    item_id,
                    1,
                    cache_key,
                    1,
                    Some("thumb/old.webp"),
                    None
                )
                .unwrap(),
                1,
                "前置条件：rev=1 + 当前 cache_key 的缩略图回写应当生效"
            );
            c.execute_batch(&format!(
                "INSERT INTO image_meta (item_id, orientation) VALUES ({item_id}, 6);
                 INSERT INTO media_derivations (item_id, kind, status, payload_path)
                    VALUES ({item_id}, 'video_cover', 2, 'deriv/old.webp');
                 INSERT INTO ai_embeddings (item_id, model_name, embedding)
                    VALUES ({item_id}, 'clip-test', x'0001');
                 INSERT INTO faces (item_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h, det_score, embedding)
                    VALUES ({item_id}, 'yunet-sface', 0.1, 0.1, 0.2, 0.2, 0.9, x'0001');
                 INSERT INTO face_coverage (item_id, model_name) VALUES ({item_id}, 'yunet-sface');
                 UPDATE media_items SET ai_status=2, face_status=2 WHERE id={item_id};"
            ))
            .unwrap();
            assert!(q::begin_dedup_run(&c, 1, true).unwrap());
            let (size, mtime_ns): (i64, i64) = c
                .query_row(
                    "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                    params![item_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert!(
                q::write_dedup_index_if_generation_current(
                    &c,
                    &q::DedupIndexUpdate {
                        item_id,
                        source_revision: 1,
                        file_size: size,
                        file_mtime_ns: Some(mtime_ns),
                        hash_version: 1,
                        quick_digest: Some(b"old"),
                        exact_digest: Some(b"old"),
                        unit_digest: Some(b"old"),
                        unit_size: Some(size),
                        physical_key: None,
                        status: "ready",
                        error_code: None,
                    },
                    1,
                )
                .unwrap(),
                "前置条件：rev=1 的去重摘要应当写入"
            );
        }

        // ── 第二轮完整扫描：检出就地变更 → 推进代次 + 全链失效 ───────────────────
        assert_eq!(
            scan(&writer, &root, "run-2", false).unwrap(),
            0,
            "就地编辑是原地更新，不是新增"
        );
        let c = writer.lock().unwrap();
        #[allow(clippy::type_complexity)]
        let (new_revision, thumb_status, thumb_path, ai_status, face_status, new_cache_key): (
            i64,
            i64,
            Option<String>,
            i64,
            i64,
            i64,
        ) = c
            .query_row(
                "SELECT source_revision, thumb_status, thumb_path, ai_status, face_status, cache_key
                   FROM media_items WHERE id=?1",
                params![item_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            new_revision, 2,
            "同 size 内容变 → SourceChanged 必须推进 source_revision"
        );
        assert_ne!(
            new_cache_key, cache_key,
            "换内容必须换 cache_key（含纳秒 mtime）"
        );
        assert_eq!(thumb_status, 0);
        assert!(thumb_path.is_none());
        assert_eq!(ai_status, 0);
        assert_eq!(face_status, 0);
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM image_meta WHERE item_id=?1",
                item_id
            ),
            0,
            "旧 EXIF 必须失效重算"
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM ai_embeddings WHERE item_id=?1",
                item_id
            ),
            0
        );
        assert_eq!(
            scalar_i64(&c, "SELECT COUNT(*) FROM faces WHERE item_id=?1", item_id),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM face_coverage WHERE item_id=?1",
                item_id
            ),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM dedup_index WHERE item_id=?1",
                item_id
            ),
            0,
            "旧代次的精确摘要必须整行作废"
        );
        let (deriv_status, deriv_path): (i64, Option<String>) = c
            .query_row(
                "SELECT status, payload_path FROM media_derivations
                  WHERE item_id=?1 AND kind='video_cover'",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(deriv_status, 0, "源变后派生任务必须退回 pending");
        assert!(deriv_path.is_none(), "旧派生产物路径必须清空");
        assert_eq!(
            config_value(&c, &baseline_incomplete_key(1)),
            Some("0".to_string()),
            "成功收尾应清零未完成标记"
        );

        // ── 拒旧版本结果：携带旧 source_revision 的迟到写一律不得落库 ─────────────
        assert_eq!(
            q::update_thumb_result_if_current(
                &c,
                item_id,
                1,
                cache_key,
                1,
                Some("thumb/late.webp"),
                None
            )
            .unwrap(),
            0,
            "旧代次缩略图回写必须被拒绝"
        );
        let (thumb_status, thumb_path): (i64, Option<String>) = c
            .query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumb_status, 0);
        assert!(thumb_path.is_none(), "被拒绝的回写不得留下产物路径");

        // 唯一变量是代次：size/mtime_ns 用**当前**值，旧代次仍必须被拒；换当前代次即放行。
        let (size_now, mtime_ns_now): (i64, i64) = c
            .query_row(
                "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            !q::dedup_write_batch_current(
                &c,
                1,
                &[q::DedupWriteCheck {
                    item_id,
                    source_revision: 1,
                    file_size: size_now,
                    file_mtime_ns: Some(mtime_ns_now),
                    companion_of: None,
                    physical_key: None,
                }]
            )
            .unwrap(),
            "旧代次去重摘要写回必须被拒绝"
        );
        assert!(
            q::dedup_write_batch_current(
                &c,
                1,
                &[q::DedupWriteCheck {
                    item_id,
                    source_revision: 2,
                    file_size: size_now,
                    file_mtime_ns: Some(mtime_ns_now),
                    companion_of: None,
                    physical_key: None,
                }]
            )
            .unwrap(),
            "对照组：同快照换当前代次应放行（证明拒绝来自代次）"
        );
        assert_eq!(
            q::update_thumb_result_if_current(
                &c,
                item_id,
                2,
                new_cache_key,
                1,
                Some("thumb/new.webp"),
                None
            )
            .unwrap(),
            1,
            "对照组：当前代次 + 当前 cache_key 的回写应当生效"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// F-001：上一轮未成功收尾（持久标记置位）→ 本轮即使请求 quick 也强制全量。
    /// 场景即残留态本身：目录 mtime 基线已「治愈」为当前值，而该目录的在盘文件尚未入库
    /// （中断前基线先写、文件后入的窗口）。quick 单独跑必然剪掉它 → 漏扫；标记兜底 → 全量必扫到。
    #[test]
    fn interrupted_scan_forces_next_quick_to_full() {
        let root = tmp_root("interrupted");
        std::fs::write(root.join("a.jpg"), b"xxxxxxxx").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        {
            let c = writer.lock().unwrap();
            let mtime = read_dir_mtime(&root).expect("临时目录应可读 mtime");
            c.execute(
                "INSERT INTO directories (root_id, rel_path, name, mtime)
                   VALUES (1, '', 'R', ?1)",
                params![mtime],
            )
            .unwrap();
        }

        // quick 单独跑：基线与目录 mtime 一致 → 剪枝 → 盘上文件不入库（残留态确实会漏）。
        assert_eq!(scan(&writer, &root, "run-pruned", true).unwrap(), 0);
        assert_eq!(item_count(&writer), 0);

        // 模拟「上一轮中断」留下的持久标记（生产由扫描入口在遍历前自行置位；此处直写等价态）。
        writer
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, '1')",
                params![baseline_incomplete_key(1)],
            )
            .unwrap();

        // 同一请求（quick=true）必须降级为全量 → 在盘文件入库，且成功收尾后标记清零。
        assert_eq!(scan(&writer, &root, "run-forced-full", true).unwrap(), 1);
        assert_eq!(
            item_count(&writer),
            1,
            "中断后下一轮必须强制全量，不得再据被治愈的基线剪枝"
        );
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                config_value(&c, &baseline_incomplete_key(1)),
                Some("0".to_string()),
                "成功收尾应清零标记"
            );
        }

        // 不永久降级：基线已重建，下一轮 quick 恢复剪枝（在盘内容与库已一致）。
        assert_eq!(scan(&writer, &root, "run-quick-after", true).unwrap(), 0);
        assert_eq!(item_count(&writer), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 注入真实收尾的遍历/卷探测结果：缺失检测被拦下后仍会写计数并收尾，但必须保留 dirty。
    /// 再走一次真实 quick 扫描，验证它能补回基线已更新、前一轮却没入库的文件。
    fn assert_incomplete_finalize_retries(
        tag: &str,
        walk_complete: bool,
        walk_error_count: usize,
        volume_online: bool,
    ) {
        let root = tmp_root(tag);
        std::fs::write(root.join("seen-before.jpg"), b"before").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        assert_eq!(scan(&writer, &root, "baseline", false).unwrap(), 1);

        std::fs::write(root.join("missed.jpg"), b"missed").unwrap();
        {
            let c = writer.lock().unwrap();
            c.execute(
                "UPDATE scan_roots SET volume_id=?1 WHERE id=?2",
                params![5, 1],
            )
            .unwrap();
            c.execute("UPDATE media_items SET volume_id=?1", params![5])
                .unwrap();
            // 模拟前一批已提前写入当前 mtime，随后遍历错误/卷离线使 missed.jpg 未进入 seen。
            c.execute(
                "UPDATE directories SET mtime=?1 WHERE root_id=?2",
                params![read_dir_mtime(&root), 1],
            )
            .unwrap();
            let dir_id: i64 = c
                .query_row(
                    "SELECT id FROM directories WHERE root_id=1 AND rel_path=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            set_baseline_incomplete(&c, 1).unwrap();
            init_seen_table_for_run(&c, 1, 42).unwrap();

            assert_eq!(
                finalize_scan_root(
                    &c,
                    1,
                    42,
                    walk_complete,
                    walk_error_count,
                    volume_online,
                    Some(5),
                    &std::collections::HashMap::from([(dir_id, 0)]),
                    0,
                )
                .unwrap(),
                0,
            );
            assert_eq!(
                config_value(&c, &baseline_incomplete_key(1)).as_deref(),
                Some("1"),
                "缺失检测被完整性/在线闸门拦下，收尾成功也不能清除 dirty"
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT media_count FROM directories WHERE id=?1",
                    dir_id
                ),
                0,
                "确实执行了生产收尾的计数写回"
            );
            let availability: String = c
                .query_row("SELECT availability FROM media_items", [], |r| r.get(0))
                .unwrap();
            assert_eq!(availability, "online", "不完整 seen 不得把既有项标成缺失");
            cleanup_scan_temp_tables_for_run(&c, 1, 42).unwrap();
        }

        assert_eq!(scan(&writer, &root, "retry-quick", true).unwrap(), 1);
        assert_eq!(item_count(&writer), 2, "dirty 使下一轮 quick 补回漏扫文件");
        assert_eq!(
            config_value(&writer.lock().unwrap(), &baseline_incomplete_key(1)).as_deref(),
            Some("0"),
            "完整遍历且最终在线后才清除 dirty"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn finalize_walk_errors_preserve_dirty_and_force_next_quick_full() {
        assert_incomplete_finalize_retries("finalize-walk-error", false, 1, true);
    }

    #[test]
    fn finalize_offline_volume_preserves_dirty_and_forces_next_quick_full() {
        assert_incomplete_finalize_retries("finalize-offline", true, 0, false);
    }

    /// 标记判定的失败方向：**只有确证干净（key 缺失 / "0"）才干净**，其余（含读失败）一律按
    /// 未完成。读取失败在库忙/损坏时最可能发生，而那时恰恰最需要这层保险——若把「读不到」当
    /// 干净，quick 剪枝会在最该强制全量的场景放开，重演 F-001 的跨轮漏扫。
    #[test]
    fn baseline_marker_only_clean_when_proven_clean() {
        assert!(
            !marker_is_incomplete(Ok(None)),
            "key 从未置位（新根）→ 干净"
        );
        assert!(
            !marker_is_incomplete(Ok(Some("0".to_string()))),
            "成功收尾写入的 0 → 干净"
        );
        assert!(
            marker_is_incomplete(Ok(Some("1".to_string()))),
            "中断留下的 1 → 未完成"
        );
        assert!(
            marker_is_incomplete(Ok(Some("garbage".to_string()))),
            "异常残留值 → 保守按未完成"
        );
        assert!(
            marker_is_incomplete(Err(AppError::internal(
                "内部任务失败 | internal task failed",
                "read failed"
            ))),
            "读取失败 → 保守按未完成（失败方向恒为全量）"
        );
    }

    /// F-001 的另一半：中断（取消）必须留痕——置位在遍历之前、清零只在成功收尾，
    /// 故取消返回后标记仍为「未完成」，下一轮（含进程重启后重读 DB）强制全量。
    #[test]
    fn cancelled_scan_leaves_incomplete_marker() {
        let root = tmp_root("cancelled");
        std::fs::write(root.join("a.jpg"), b"xxxxxxxx").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel(); // 开扫即取消：模拟用户在遍历中途停止
        let channel = silent_channel();

        let err = run_fast_scan(
            &writer,
            1,
            "run-cancel",
            &root.to_string_lossy(),
            &catalog,
            &channel,
            &cancel,
            &|| {},
            false,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Cancelled));
        let c = writer.lock().unwrap();
        assert_eq!(
            config_value(&c, &baseline_incomplete_key(1)),
            Some("1".to_string()),
            "取消后必须留下未完成标记，使下一轮强制全量"
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM media_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "取消的轮次不得写入任何媒体项"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 收尾落库是**一次事务**：目录计数 / 根状态 / dirty 标记要么一起生效、要么一起不留痕。
    ///
    /// 两处故障注入（trigger RAISE 打断写库）：① 目录计数 UPDATE 就失败 → 三者都未写；
    /// ② 计数已写、根状态 UPDATE 才失败 → 已写入的计数必须一起回滚（三者各自 autocommit 时它会留在 12）。
    /// 两种情况下 dirty 都必须仍置位，使下一轮 quick 强制全量重建基线。
    #[test]
    fn finalize_write_failure_rolls_back_counts_status_and_dirty() {
        let root = tmp_root("finalize-rollback");
        std::fs::write(root.join("a.jpg"), b"x").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        assert_eq!(scan(&writer, &root, "baseline", false).unwrap(), 1);

        let counts = {
            let c = writer.lock().unwrap();
            let dir_id: i64 = c
                .query_row(
                    "SELECT id FROM directories WHERE root_id=1 AND rel_path=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            // 收尾前哨兵状态：计数 99、根状态 scanning、未写过 last_scan_at、dirty 置位。
            c.execute(
                "UPDATE directories SET media_count=99 WHERE id=?1",
                params![dir_id],
            )
            .unwrap();
            c.execute(
                "UPDATE scan_roots SET scan_status='scanning', scan_progress=3,
                        total_files=3, last_scan_at=NULL WHERE id=?1",
                params![1i64],
            )
            .unwrap();
            set_baseline_incomplete(&c, 1).unwrap();
            std::collections::HashMap::from([(dir_id, 12i64)])
        };

        let c = writer.lock().unwrap();
        let dir_id = *counts.keys().next().unwrap();
        // 收尾失败后必须原样保持收尾前哨兵状态 —— 两种故障注入共用同一组断言。
        let assert_untouched = |c: &Connection| {
            assert_eq!(
                scalar_i64(c, "SELECT media_count FROM directories WHERE id=?1", dir_id),
                99,
                "目录计数写入必须整体回滚，不得留下部分提交"
            );
            let (status, progress, last_scan_at): (String, i64, Option<i64>) = c
                .query_row(
                    "SELECT scan_status, scan_progress, last_scan_at FROM scan_roots WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(status, "scanning", "根状态不得被单独推进为 idle");
            assert_eq!(progress, 3, "根进度不得被单独推进");
            assert!(last_scan_at.is_none(), "收尾未成功不得写 last_scan_at");
            assert_eq!(
                config_value(c, &baseline_incomplete_key(1)).as_deref(),
                Some("1"),
                "收尾失败必须保留 dirty，下一轮强制全量重建基线"
            );
            let availability: String = c
                .query_row("SELECT availability FROM media_items", [], |r| r.get(0))
                .unwrap();
            assert_eq!(availability, "online", "既有项不得被标缺失");
        };

        // ① 目录计数 UPDATE 就失败 → 三者都未写。
        c.execute_batch(
            "CREATE TRIGGER t_block_dir_count BEFORE UPDATE OF media_count ON directories
             BEGIN SELECT RAISE(ABORT, 'blocked-dir-count'); END;",
        )
        .unwrap();
        init_seen_table_for_run(&c, 1, 7).unwrap();
        let err = finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap_err();
        assert!(
            err.to_string().contains("blocked-dir-count"),
            "前置条件：失败必须来自目录计数 UPDATE 的 trigger，而非其它路径: {err}"
        );
        assert_untouched(&c);

        // ② 计数已写、根状态 UPDATE 才失败 → 已写入的计数必须一起回滚。
        c.execute_batch(
            "DROP TRIGGER t_block_dir_count;
             CREATE TRIGGER t_block_root_finish BEFORE UPDATE OF scan_status ON scan_roots
             BEGIN SELECT RAISE(ABORT, 'blocked-root-finish'); END;",
        )
        .unwrap();
        init_seen_table_for_run(&c, 1, 7).unwrap();
        let err = finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap_err();
        assert!(
            err.to_string().contains("blocked-root-finish"),
            "前置条件：失败必须来自根状态 UPDATE 的 trigger: {err}"
        );
        assert_untouched(&c);

        c.execute_batch("DROP TRIGGER t_block_root_finish;")
            .unwrap();
        // 失败只是回滚，不是永久损伤：去掉故障后同一轮收尾正常落地，三项一起生效。
        init_seen_table_for_run(&c, 1, 7).unwrap();
        assert_eq!(
            finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap(),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT media_count FROM directories WHERE id=?1",
                dir_id
            ),
            12,
            "重试收尾应写回本轮计数"
        );
        assert_eq!(
            config_value(&c, &baseline_incomplete_key(1)).as_deref(),
            Some("0"),
            "重试成功后 dirty 才清零"
        );

        drop(c);
        let _ = std::fs::remove_dir_all(&root);
    }
}
