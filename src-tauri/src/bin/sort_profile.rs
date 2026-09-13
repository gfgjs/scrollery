//! 目录序排序 profile —— 量化方案 B「切列前后」A/B（B-dir 收益证据，设计 §2.4 决策门）。
//!
//! **测什么**：folder 分组的画廊查询,其 ORDER BY 目录序键的两种形态。切列前 =
//! `TREE_SORT_KEY(d.rel_path)`（每行调 SQLite→Rust 标量函数,列参数不可
//! `SQLITE_DETERMINISTIC` 折叠 → 逐行 FFI + Vec<u8> 分配）；切列后 = `d.tree_sort_key`
//! （读 V19 持久 BLOB 列,memcpy）。换列后**顺序刚性等价**（BLOB memcmp =
//! encode_tree_sort_key 的 Vec<u8>::cmp）,只比耗时。
//!
//! **为何只测这几个配置**：`d.tree_sort_key` 仅出现在 `group_by=="folder"` 分支。而
//! folder+datetime 命中 canonical 缓存、由内存 `derive_order` 排序、**绕过本 SQL**。真正
//! 下发目录序 SQL 排序的生产配置只剩 folder+filename（filename 从不命中 canonical 缓存,
//! 每次序变/数据写必重查）与 folder+similarity（需 ai_search）。切列的 A/B delta 完全落在
//! 共享的 `dir_order` 前缀上,与二级键（NATURAL_CMP / similarity）本身开销正交 —— 故额外测
//! 一个去掉二级键的 **dir-only** 变体,隔离出「纯切列 delta」。
//!
//! **similarity 的诚实边界**：`ai_search_results` 是每次搜索的小型瞬态结果集,JOIN 后只剩
//! 少量行,无法代表全库排序 —— 闲置库上不可有代表性地 profile。但其切列 delta 按构造 **等于
//! dir-only 的 delta**（同一 `dir_order` 前缀）。故 ai 结果非空时附带跑一次并明确标注「仅覆盖
//! N 行,不代表全库」,否则以 dir-only 作为 similarity 切列 delta 的等价证据。
//!
//! **只读**：经生产 `create_read_pool`（SQLITE_OPEN_READ_ONLY + 同款 PRAGMA + 注册
//! NATURAL_CMP/TREE_SORT_KEY）打开,不改数据,对运行中的应用安全。
//!
//! 用法：`cargo run --release --bin sort_profile -- [db_path] [runs] [warmup]`
//!   db_path 缺省 = %APPDATA%\com.scrollery.app\scrollery.db；runs 缺省 5；warmup 缺省 1。

use std::path::PathBuf;
use std::time::Instant;

use rusqlite::Connection;
use scrollery_lib::db::create_read_pool;
use scrollery_lib::db::models::{GalleryFilter, MediaFilter, SortSpec, ViewDescriptor, ViewScope};
use scrollery_lib::db::queries::{
    query_dir_labels, query_item_ids_filename_order, query_layout_items,
    query_layout_items_canonical, query_layout_items_filename_baseline, view_to_sql,
};
use scrollery_lib::layout::items_cache::{
    build_dir_rank, build_id_index, derive_order, CachedOrder, ItemsCacheData,
};

/// 切列后（持久列）与切列前（标量函数）在 SQL 中的目录序键片段——A/B 唯一差异处。
const COL: &str = "d.tree_sort_key";
const FUNC: &str = "TREE_SORT_KEY(d.rel_path)";
/// filename 二级键片段（含前导逗号）——去掉即得 dir-only 变体。
const SEC_FILENAME: &str = ", m.file_name COLLATE NATURAL_CMP DESC";
/// scan_roots JOIN——similarity 变体在其后注入 ai_search_results JOIN。
const SCANROOT_JOIN: &str = "JOIN scan_roots r ON d.root_id = r.id";

fn default_db_path() -> PathBuf {
    // %APPDATA%\com.scrollery.app\scrollery.db（Tauri identifier=com.scrollery.app）。
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata)
        .join("com.scrollery.app")
        .join("scrollery.db")
}

/// 消费全部 id 行（触发完整排序），返回行数。
fn consume(conn: &Connection, sql: &str) -> usize {
    let mut stmt = conn.prepare(sql).expect("prepare failed");
    let rows = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .expect("query failed");
    let mut n = 0usize;
    for r in rows {
        r.expect("row decode failed");
        n += 1;
    }
    n
}

/// 跑一组样本：warmup 次预热（暖页缓存,不计），再 runs 次计时。返回 (行数, 每次 ms)。
fn run_series(
    conn: &Connection,
    sql: &str,
    warmup: usize,
    runs: usize,
    tag: &str,
) -> (usize, Vec<f64>) {
    let mut rows = 0usize;
    for _ in 0..warmup {
        rows = consume(conn, sql);
    }
    let mut samples = Vec::with_capacity(runs);
    for i in 0..runs {
        let t = Instant::now();
        rows = consume(conn, sql);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        samples.push(ms);
        eprintln!("    [{tag}] run {}/{}: {:.0} ms", i + 1, runs, ms);
    }
    (rows, samples)
}

/// (min, median, mean)。
fn stats(samples: &[f64]) -> (f64, f64, f64) {
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = s[0];
    let median = s[s.len() / 2];
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    (min, median, mean)
}

/// ORDER BY 尾巴（供人工核对实际比较的是哪条 SQL）。
fn order_tail(sql: &str) -> &str {
    sql.find("ORDER BY").map(|i| &sql[i..]).unwrap_or(sql)
}

/// 打印一个配置的切列前后对比。
fn report(name: &str, rows: usize, before: &[f64], after: &[f64]) {
    let (bmin, bmed, bmean) = stats(before);
    let (amin, amed, amean) = stats(after);
    println!("\n── {name}  ({rows} 行) ─────────────────────────────");
    println!("   切列前 函数 TREE_SORT_KEY(rel_path): min {bmin:.0} / median {bmed:.0} / mean {bmean:.0} ms");
    println!("   切列后 列   d.tree_sort_key        : min {amin:.0} / median {amed:.0} / mean {amean:.0} ms");
    let delta = bmed - amed;
    let pct = if bmed > 0.0 {
        delta / bmed * 100.0
    } else {
        0.0
    };
    let sign = if delta >= 0.0 { "省" } else { "反增" };
    println!(
        "   切列收益(median 差): {sign} {:.0} ms  ({pct:+.1}%)",
        delta.abs()
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let db_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_db_path);
    let runs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let warmup: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    if !db_path.exists() {
        eprintln!("DB 不存在: {}", db_path.display());
        std::process::exit(1);
    }
    println!("目录序排序 profile（方案 B 切列前后 A/B）");
    println!("DB   : {}", db_path.display());
    println!("runs : {runs}（+{warmup} 预热）\n");

    // 生产读连接（只读 + 同款 PRAGMA + 注册 NATURAL_CMP/TREE_SORT_KEY）。
    let pool = create_read_pool(&db_path, 1).map_err(|e| e.to_string())?;
    let conn = pool.get().map_err(|e| e.to_string())?;

    // ── 诊断 ──────────────────────────────────────────────────────────────
    // schema 版本记在 app_config 表(key='schema_version'),非 PRAGMA user_version(恒 0)。
    let schema_version: String = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "?".into());
    let media: i64 = conn.query_row(
        "SELECT count(*) FROM media_items WHERE is_deleted=0 AND companion_of IS NULL",
        [],
        |r| r.get(0),
    )?;
    let dirs: i64 = conn.query_row("SELECT count(*) FROM directories", [], |r| r.get(0))?;
    // tree_sort_key 回填自检：非根目录键不应为空（空 = V19 回填漏 / DEFAULT X'' 未覆盖）。
    let dirs_keyed: i64 = conn.query_row(
        "SELECT count(*) FROM directories WHERE length(tree_sort_key) > 0",
        [],
        |r| r.get(0),
    )?;
    let dirs_empty_nonroot: i64 = conn.query_row(
        "SELECT count(*) FROM directories WHERE length(tree_sort_key)=0 AND rel_path <> ''",
        [],
        |r| r.get(0),
    )?;
    let ai_rows: i64 = conn
        .query_row("SELECT count(*) FROM ai_search_results", [], |r| r.get(0))
        .unwrap_or(-1);

    println!("── 诊断 ─────────────────────────────────────────");
    println!("   schema_version      : {schema_version}（19 = B-dir V19 已迁移）");
    println!("   media_items(可见)   : {media}");
    println!("   directories         : {dirs}（其中 tree_sort_key 非空 {dirs_keyed}）");
    if dirs_empty_nonroot > 0 {
        println!("   ⚠️ 非根目录 tree_sort_key 为空: {dirs_empty_nonroot}（V19 回填异常,profile 顺序可能错乱）");
    } else {
        println!("   tree_sort_key 回填  : OK（无非根空键）");
    }
    println!("   ai_search_results   : {ai_rows}");

    // ── 构造 A/B SQL（切列后经 view_to_sql 取生产真身,切列前经字符串替换派生）──────
    let vd = ViewDescriptor {
        scope: ViewScope::All,
        filter: GalleryFilter::default(),
        sort: SortSpec {
            group_by: "folder".into(),
            sort_within_group: "filename".into(),
            sort_order: "desc".into(),
        },
        duplicate_lens: None,
        layout_version: 0,
    };
    let (fname_after, params) = view_to_sql(&vd, &[]).map_err(|e| e.to_string())?;
    assert!(
        params.is_empty(),
        "scope=All + 默认 filter 不应产生绑定参数（否则本 bench 需改绑参）"
    );
    assert!(
        fname_after.contains(COL),
        "切列后 SQL 应含持久列 {COL}（push_order_by 可能已改）"
    );
    assert!(
        fname_after.contains(SEC_FILENAME),
        "filename 二级键片段应存在: {SEC_FILENAME}"
    );

    let fname_before = fname_after.replace(COL, FUNC);
    let dironly_after = fname_after.replace(SEC_FILENAME, "");
    let dironly_before = dironly_after.replace(COL, FUNC);

    println!("\n── 待测 SQL（ORDER BY 尾）──────────────────────");
    println!("   filename 切列后: {}", order_tail(&fname_after));
    println!("   filename 切列前: {}", order_tail(&fname_before));
    println!("   dir-only 切列后: {}", order_tail(&dironly_after));

    // 参考：完整生产查询（18 列物化 + 排序）的绝对耗时（仅切列后,给 SQL 排序占比一个量级）。
    {
        let filter = MediaFilter::default();
        // 预热一次再计时。
        let _ = query_layout_items(
            &conn,
            &filter,
            Some("folder"),
            Some("filename"),
            Some("desc"),
            false,
        )?;
        let t = Instant::now();
        let items = query_layout_items(
            &conn,
            &filter,
            Some("folder"),
            Some("filename"),
            Some("desc"),
            false,
        )?;
        println!(
            "\n参考：完整生产查询 query_layout_items(folder+filename, 列形态, 18 列物化) = {:.0} ms, {} items",
            t.elapsed().as_secs_f64() * 1000.0,
            items.len()
        );
    }

    // ── 跑 A/B（每个配置：先切列前,后切列后；同表页缓存,预热后两者皆热,公平）────────
    println!("\n运行 A/B（stderr 打印每次 run 进度）…");

    let (r1, fname_b) = run_series(&conn, &fname_before, warmup, runs, "filename 前");
    let (_, fname_a) = run_series(&conn, &fname_after, warmup, runs, "filename 后");

    let (r2, diro_b) = run_series(&conn, &dironly_before, warmup, runs, "dir-only 前");
    let (_, diro_a) = run_series(&conn, &dironly_after, warmup, runs, "dir-only 后");

    // similarity：ai 结果非空才跑（否则以 dir-only 作等价证据）。
    let sim = if ai_rows > 0 {
        let sim_after = fname_after
            .replace(
                SCANROOT_JOIN,
                &format!(
                    "{SCANROOT_JOIN}\n         JOIN ai_search_results ai ON m.id = ai.file_id"
                ),
            )
            .replace("m.file_name COLLATE NATURAL_CMP DESC", "ai.similarity DESC");
        let sim_before = sim_after.replace(COL, FUNC);
        println!("   similarity 切列后: {}", order_tail(&sim_after));
        let (rs, sim_b) = run_series(&conn, &sim_before, warmup, runs, "similarity 前");
        let (_, sim_a) = run_series(&conn, &sim_after, warmup, runs, "similarity 后");
        Some((rs, sim_b, sim_a))
    } else {
        None
    };

    // ── 结果 ─────────────────────────────────────────────────────────────
    println!("\n╔══════════════════ 结果 ══════════════════╗");
    report(
        "filename-folder（真实重负载,生产配置 #1）",
        r1,
        &fname_b,
        &fname_a,
    );
    report(
        "dir-only（隔离纯切列 delta = similarity 切列 delta 等价物）",
        r2,
        &diro_b,
        &diro_a,
    );
    if let Some((rs, sim_b, sim_a)) = sim {
        report(
            "similarity-folder（⚠ 仅覆盖 ai 结果行,不代表全库,仅参考）",
            rs,
            &sim_b,
            &sim_a,
        );
    } else {
        println!("\n── similarity-folder：跳过（ai_search_results 为空/不足）。");
        println!("   其切列 delta 按构造等于上面的 dir-only（同一 dir_order 前缀）。");
    }
    println!("\n╚══════════════════════════════════════════╝");
    println!(
        "\n解读：dir-only 的 median 差 = 每行省下的 TREE_SORT_KEY FFI+alloc 的净额;\n\
         filename-folder 的 median 差 = 同一净额在真实重负载配置里的占比（分母含 NATURAL_CMP 排序开销）。"
    );

    // ── B-file-i:filename 基准 + 内存派生 vs 每次 SQL filesort ─────────────────
    // 证明:进入 filename 视图付一次基准 SQL 排序(≈上面的 filename-folder),此后 none/folder 轴与
    // asc/desc 方向切换由内存派生(整数下标排序 + 置换 memo),不再每次全表 NATURAL_CMP filesort。
    println!("\n╔════════════ B-file-i:filename 内存派生 ════════════╗");
    let t_base = Instant::now();
    let baseline = query_layout_items_filename_baseline(&conn, &MediaFilter::default())
        .map_err(|e| e.to_string())?;
    let base_ms = t_base.elapsed().as_secs_f64() * 1000.0;
    let dir_labels = query_dir_labels(&conn).map_err(|e| e.to_string())?;
    let data = ItemsCacheData {
        filter_key: String::new(),
        order: CachedOrder::CanonicalFilename,
        data_version: 0,
        id_to_idx: build_id_index(&baseline),
        dir_rank: build_dir_rank(&dir_labels),
        items: baseline,
        dir_labels,
        filter: MediaFilter::default(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    println!(
        "   基准 SQL 排序(进入 filename 视图一次性): {base_ms:.0} ms, {} items",
        data.items.len()
    );

    // folder 派生(memo miss = 一次轴/方向切换的全排序开销):每次清 memo 测全排序。
    let mut folder_samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        *data.perm_memo.lock().unwrap() = None;
        let t = Instant::now();
        let v = derive_order(&data, "folder", "filename", "desc");
        folder_samples.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(v);
    }
    let (fmin, fmed, _) = stats(&folder_samples);

    // folder 同轴同向(memo hit,如滑块/窗宽):暖 memo 后测 O(N) 置换还原。
    let _ = derive_order(&data, "folder", "filename", "desc");
    let t = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(derive_order(&data, "folder", "filename", "desc"));
    }
    let folder_memo_ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;

    // none 轴(纯反转/恒等):
    let t = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(derive_order(&data, "none", "filename", "asc"));
    }
    let none_ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;

    println!("   folder 轴/方向切换 派生(memo miss,全排序): min {fmin:.1} / median {fmed:.1} ms");
    println!("   folder 同轴同向 派生(memo hit,滑块/窗宽) : {folder_memo_ms:.2} ms");
    println!("   none 轴切换 派生(纯反转)                 : {none_ms:.2} ms");

    // ── 双键统一缓存:从这份 filename 基准**跨键派生 datetime**(免额外查询,items 自带 sort_datetime)。
    //    量化「filename→datetime 切换」的修法收益:改动前该切换是全表 SQL 重查(单槽缓存挤掉 filename
    //    基准),现在是纯内存派生。
    println!("\n   ── 跨键:filename 基准 → datetime 派生(双键统一,免重查)──");
    // none+datetime(跨键实排 (sort_datetime,id)):
    let mut xnone = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        let v = derive_order(&data, "none", "datetime", "desc");
        xnone.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(v);
    }
    let (xnmin, xnmed, _) = stats(&xnone);
    // folder+datetime(跨键,清 memo 测全排序):
    let mut xfolder = Vec::with_capacity(runs);
    for _ in 0..runs {
        *data.perm_memo.lock().unwrap() = None;
        let t = Instant::now();
        let v = derive_order(&data, "folder", "datetime", "desc");
        xfolder.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(v);
    }
    let (xfmin, xfmed, _) = stats(&xfolder);
    println!(
        "   none  轴 datetime 跨键派生(实排)          : min {xnmin:.1} / median {xnmed:.1} ms"
    );
    println!(
        "   folder 轴 datetime 跨键派生(memo miss,全排): min {xfmin:.1} / median {xfmed:.1} ms"
    );
    println!("╚════════════════════════════════════════════════════╝");
    println!(
        "\n解读(B-file-i):进入 filename 视图付 {base_ms:.0}ms 基准 SQL 排序一次;此后 folder/none 轴与\n\
         asc/desc 方向切换 = {fmed:.0}ms 内存派生(memo 命中仅 {folder_memo_ms:.2}ms),取代改动前每次\n\
         ~{base_ms:.0}ms 的全表 SQL NATURAL_CMP filesort —— 即 filename 轴/方向切换与 datetime 平权。"
    );

    // ── 双键统一缓存:反方向 datetime 基准 → filename 派生(须惰性建 filename_rank)──────────
    // 这是「datetime→filename 切换」的修法路径。改动前该切换是全表 SQL 重查(单槽缓存被 filename
    // 请求挤掉 datetime 基准);现在:datetime 基准常驻,用户首次切 filename 序时补**一次** id-only
    // NATURAL_CMP 查询建 rank(一次性,复刻 ensure_filename_rank_for_hit),此后 datetime↔filename
    // 互切全内存派生。本段量化那次 rank 构建成本 + 稳态派生,与上文 filename→datetime 方向合成完整
    // 的**双向**证据。datetime 基准由 query_layout_items_canonical 取(与生产 compute_layout MISS 同构)。
    println!("\n╔════════ 双键反方向:datetime 基准 → filename 派生 ════════╗");
    let t_base2 = Instant::now();
    let baseline2 =
        query_layout_items_canonical(&conn, &MediaFilter::default()).map_err(|e| e.to_string())?;
    let base2_ms = t_base2.elapsed().as_secs_f64() * 1000.0;
    let dir_labels2 = query_dir_labels(&conn).map_err(|e| e.to_string())?;
    let data2 = ItemsCacheData {
        filter_key: String::new(),
        order: CachedOrder::Canonical,
        data_version: 0,
        id_to_idx: build_id_index(&baseline2),
        dir_rank: build_dir_rank(&dir_labels2),
        items: baseline2,
        dir_labels: dir_labels2,
        filter: MediaFilter::default(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    println!(
        "   基准 SQL 排序(进入 datetime 视图一次性,内存补序): {base2_ms:.0} ms, {} items",
        data2.items.len()
    );

    // 一次性 filename_rank 构建 = 首次切 filename 付的额外成本(复刻 ensure_filename_rank_for_hit:
    // id-only NATURAL_CMP 查询 + HashMap 反查 + 与 items 平行的 rank 向量)。
    let t_rank = Instant::now();
    let fname_ids =
        query_item_ids_filename_order(&conn, &MediaFilter::default()).map_err(|e| e.to_string())?;
    let mut rank_of: std::collections::HashMap<i64, u32> =
        std::collections::HashMap::with_capacity(fname_ids.len());
    for (r, id) in fname_ids.iter().enumerate() {
        rank_of.insert(*id, r as u32);
    }
    let ranks: Vec<u32> = data2
        .items
        .iter()
        .map(|it| rank_of.get(&it.id).copied().unwrap_or(u32::MAX))
        .collect();
    data2
        .filename_rank
        .set(ranks)
        .expect("首次 set filename_rank");
    let rank_ms = t_rank.elapsed().as_secs_f64() * 1000.0;
    println!(
        "   惰性 filename_rank 构建(首次切 filename 一次性:id-only 查询+反查+平行向量): {rank_ms:.0} ms"
    );

    // rank 就绪后:datetime 基准跨键派生 filename —— 此后每次 datetime→filename 切换的稳态成本。
    let mut yfolder = Vec::with_capacity(runs);
    for _ in 0..runs {
        *data2.perm_memo.lock().unwrap() = None;
        let t = Instant::now();
        let v = derive_order(&data2, "folder", "filename", "desc");
        yfolder.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(v);
    }
    let (yfmin, yfmed, _) = stats(&yfolder);
    let mut ynone = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        let v = derive_order(&data2, "none", "filename", "asc");
        ynone.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(v);
    }
    let (ynmin, ynmed, _) = stats(&ynone);
    println!(
        "   none  轴 filename 跨键派生(rank 就绪,实排)   : min {ynmin:.1} / median {ynmed:.1} ms"
    );
    println!(
        "   folder 轴 filename 跨键派生(rank 就绪,memo miss): min {yfmin:.1} / median {yfmed:.1} ms"
    );
    println!("╚════════════════════════════════════════════════════╝");
    println!(
        "\n解读(双键反方向):datetime 基准常驻,首次切 filename 付一次 {rank_ms:.0}ms 的 rank 构建(仅\n\
         id-only 查询,约为整行 filename 基准重查 {base_ms:.0}ms 的一半);此后 filename 轴/方向切换 =\n\
         {yfmed:.0}ms 内存派生。对比改动前:datetime↔filename 每次切换都触发 ~{base_ms:.0}ms 全表 SQL\n\
         重查(单槽缓存互斥)——双键统一缓存把这一互切从「每次重查」降为「首次一次 rank + 此后纯派生」。"
    );

    // ── 决定性隔离:filename collation FFI 成本(NATURAL_CMP vs BINARY,+ EXPLAIN)────────────
    // 回答「compute_layout MISS 的 ~5s(dev)/~1s(release)是我们自研 NATURAL_CMP collation 的
    // SQLite→Rust FFI 开销,还是 join / 18 列物化 / filesort 结构本身?」方法 = 同一批行、同一条
    // SQL、同一 filesort 结构,**只把比较函数从自定义 NATURAL_CMP(FFI)换成内建 BINARY(memcmp,
    // 无 FFI)**,两者耗时差即纯 collation FFI 成本。BINARY 会改变次序(img10<img2),但行数与 filesort
    // 结构不变——本段只比耗时,不比序。用 id-only 查询排除 18 列物化,隔离纯排序开销;none/datetime
    // (走 idx_media_sort 索引扫、免 collation)作对照下限。dev 与 release 各跑一次:同一 NATURAL−BINARY
    // 差值在 dev 若比 release 放大数倍,即铁证该开销落在我们未优化 + overflow-checks 的 Rust FFI 上。
    println!("\n╔═══ 决定性隔离:filename collation FFI(NATURAL_CMP vs BINARY,id-only)═══╗");

    // 真机 MISS 的两种 filename 查询形状,经 view_to_sql 取生产真身(id-only,第 0 列为 m.id):
    //   none/filename = 全表 filename 主键排序(folder/filename MISS 走 filename_baseline,SQL 只做
    //   这条全表 filename 序、folder 分组在内存派生,故与 none/filename 等价);
    //   date/filename = date 主键 + filename 次键(date+filename 不可内存派生,走 SQL 精确序)。
    let mk_fname_sql = |gb: &str| -> Result<String, String> {
        let vd = ViewDescriptor {
            scope: ViewScope::All,
            filter: GalleryFilter::default(),
            sort: SortSpec {
                group_by: gb.into(),
                sort_within_group: "filename".into(),
                sort_order: "desc".into(),
            },
            duplicate_lens: None,
            layout_version: 0,
        };
        let (sql, params) = view_to_sql(&vd, &[]).map_err(|e| e.to_string())?;
        assert!(
            params.is_empty(),
            "scope=All + 默认 filter 不应产生绑定参数(否则本段需改绑参)"
        );
        assert!(
            sql.contains("COLLATE NATURAL_CMP"),
            "{gb}/filename SQL 应含 NATURAL_CMP(push_order_by 可能已改)"
        );
        Ok(sql)
    };
    let none_nat = mk_fname_sql("none")?;
    let date_nat = mk_fname_sql("date")?;
    // datetime 对照(none/datetime:免 collation,走 idx_media_sort 索引扫,给排序开销一个下限)。
    let dt_vd = ViewDescriptor {
        scope: ViewScope::All,
        filter: GalleryFilter::default(),
        sort: SortSpec {
            group_by: "none".into(),
            sort_within_group: "datetime".into(),
            sort_order: "desc".into(),
        },
        duplicate_lens: None,
        layout_version: 0,
    };
    let (dt_sql, _) = view_to_sql(&dt_vd, &[]).map_err(|e| e.to_string())?;

    // BINARY 变体:唯一改动是把比较函数从 FFI 的 NATURAL_CMP 换成内建 memcmp(replace 全部出现处)。
    let none_bin = none_nat.replace("COLLATE NATURAL_CMP", "COLLATE BINARY");
    let date_bin = date_nat.replace("COLLATE NATURAL_CMP", "COLLATE BINARY");

    // EXPLAIN QUERY PLAN:证实 filesort(USE TEMP B-TREE)在场,以及 datetime 是否走索引扫。
    let explain = |conn: &Connection, sql: &str, tag: &str| {
        println!("   ── EXPLAIN [{tag}] ──");
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("EXPLAIN prepare");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .expect("EXPLAIN query");
        for d in rows {
            println!("      {}", d.expect("EXPLAIN row decode"));
        }
    };
    explain(&conn, &none_nat, "none/filename NATURAL_CMP");
    explain(&conn, &date_nat, "date/filename NATURAL_CMP");
    explain(&conn, &dt_sql, "none/datetime 对照");

    println!("\n   计时(id-only,{runs} 次 + {warmup} 预热)…");
    let (rn, none_nat_s) = run_series(&conn, &none_nat, warmup, runs, "none/fname NATURAL");
    let (_, none_bin_s) = run_series(&conn, &none_bin, warmup, runs, "none/fname BINARY");
    let (rd, date_nat_s) = run_series(&conn, &date_nat, warmup, runs, "date/fname NATURAL");
    let (_, date_bin_s) = run_series(&conn, &date_bin, warmup, runs, "date/fname BINARY");
    let (_, dt_s) = run_series(&conn, &dt_sql, warmup, runs, "none/datetime 对照");

    let (_, nn_med, _) = stats(&none_nat_s);
    let (_, nb_med, _) = stats(&none_bin_s);
    let (_, dn_med, _) = stats(&date_nat_s);
    let (_, db_med, _) = stats(&date_bin_s);
    let (_, dt_med, _) = stats(&dt_s);

    println!("\n   ── 结果(median,id-only)────────────────────────────");
    println!(
        "   none/filename : NATURAL {nn_med:.0} ms  vs  BINARY {nb_med:.0} ms  → 纯 collation FFI = {:.0} ms ({rn} 行)",
        nn_med - nb_med
    );
    println!(
        "   date/filename : NATURAL {dn_med:.0} ms  vs  BINARY {db_med:.0} ms  → 纯 collation FFI = {:.0} ms ({rd} 行)",
        dn_med - db_med
    );
    println!("   none/datetime : {dt_med:.0} ms(免 collation,走索引扫,对照下限)");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!(
        "\n解读(决定性隔离):NATURAL−BINARY 差值若占 NATURAL 大头 ⇒ 瓶颈确为自研 NATURAL_CMP 的\n\
         FFI + 算法开销(方案 1「全局内存 rank」/ 方案 2「持久字节键」攻此点有效);BINARY 若已逼近\n\
         datetime 对照 ⇒ 物化 / join / filesort 结构本身非主成本。dev 下该 FFI 未优化 + overflow-checks,\n\
         故同一差值 dev 比 release 放大数倍——即「dev 5s vs release 1s」的归属证据。"
    );

    Ok(())
}
