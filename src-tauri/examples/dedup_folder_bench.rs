//! 去重文件夹聚合查询基准。
//!
//! 这是一个独立的、只读查询基准：数据库在进程内由当前公开 schema 迁移创建，随后
//! 批量插入合成的 `directories`、`media_items` 与 `dedup_index` 行。摘要是确定性的
//! synthetic BLOB，不包含文件 IO 或真实哈希计算，因此计时只反映文件夹聚合查询，
//! 不把数据构造成本伪装成查询结果。
//!
//! “冷”运行前执行 `PRAGMA shrink_memory`，表示 SQLite page-cache 冷启动近似；它不清空
//! 操作系统文件缓存，也不等价于新进程冷启动。Windows 输出进程历史 PeakWorkingSet，
//! Unix 输出 `getrusage.ru_maxrss`；该峰值是进程级历史高水位，不能解读为单次查询独占值。
//!
//! 示例：
//!
//! ```text
//! cargo run --manifest-path src-tauri/Cargo.toml --release --example dedup_folder_bench -- --items 100k
//! cargo run --manifest-path src-tauri/Cargo.toml --release --example dedup_folder_bench -- --items 1m --warm-iterations 3
//! ```

use std::{
    error::Error,
    io::{Error as IoError, ErrorKind},
    sync::Arc,
    time::{Duration, Instant},
};

use rusqlite::{params, Connection};
use scrollery_lib::{
    db::{migration, queries},
    dedup::{
        folder_cache::{list_candidate_rows, DedupFolderStatsCache},
        DEDUP_HASH_VERSION,
    },
};

const DEFAULT_ITEMS: usize = 100_000;
const DEFAULT_GROUP_WIDTH: usize = 4;
const DEFAULT_WARM_ITERATIONS: usize = 3;
const ITEMS_PER_DIRECTORY: usize = 512;
const MIN_LEAF_DIRECTORIES: usize = 64;
const MAX_LEAF_DIRECTORIES: usize = 2_048;

#[derive(Debug, Clone, Copy)]
struct Config {
    items: usize,
    leaf_directories: usize,
    duplicate_group_width: usize,
    warm_iterations: usize,
}

#[derive(Debug)]
struct Counts {
    directories: i64,
    media_items: i64,
    dedup_index: i64,
}

#[derive(Debug)]
struct QuerySample {
    elapsed: Duration,
    rows: usize,
    checksum: u64,
}

#[derive(Debug)]
struct WarmSummary {
    samples: Vec<Duration>,
    rows: usize,
    checksum: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dedup_folder_bench failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = match parse_config().map_err(invalid_input)? {
        Some(config) => config,
        None => {
            print_usage();
            return Ok(());
        }
    };

    run_benchmark(config)
}

fn run_benchmark(config: Config) -> Result<(), Box<dyn Error>> {
    let peak_before = peak_working_set_bytes();
    let schema_started = Instant::now();
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         PRAGMA journal_mode=OFF;
         PRAGMA synchronous=OFF;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-65536;",
    )?;
    migration::run_migrations(&conn)?;
    let schema_elapsed = schema_started.elapsed();
    let peak_after_schema = peak_working_set_bytes();

    let data_started = Instant::now();
    let counts = seed_database(&conn, config)?;
    let data_elapsed = data_started.elapsed();
    let peak_after_data = peak_working_set_bytes();

    let list_limit = config.leaf_directories.saturating_add(1);
    let root_folder_id = 1_i64;
    let include_zero_byte = false;

    println!("dedup_folder_bench");
    println!(
        "  dataset: items={} directories={} (root + {} leaves) duplicate_group_width={} groups={} ",
        config.items,
        counts.directories,
        config.leaf_directories,
        config.duplicate_group_width,
        config
            .items
            .saturating_add(config.duplicate_group_width - 1)
            / config.duplicate_group_width,
    );
    println!(
        "  rows: media_items={} dedup_index={} | hash_version={} | include_zero_byte={}",
        counts.media_items, counts.dedup_index, DEDUP_HASH_VERSION, include_zero_byte,
    );
    println!(
        "  schema: elapsed={} peak_ws_hwm={}",
        format_duration(schema_elapsed),
        format_peak(peak_after_schema, peak_before),
    );
    println!(
        "  data:   elapsed={} peak_ws_hwm={}",
        format_duration(data_elapsed),
        format_peak(peak_after_data, peak_before),
    );
    println!(
        "  query:  list_limit={} warm_iterations={} cold_reset=PRAGMA shrink_memory",
        list_limit, config.warm_iterations,
    );

    let cold_list = measure_cold_list(&conn, list_limit, include_zero_byte)?;
    let peak_after_cold_list = peak_working_set_bytes();
    let warm_list =
        measure_warm_list(&conn, list_limit, include_zero_byte, config.warm_iterations)?;
    let peak_after_warm_list = peak_working_set_bytes();

    let cold_stats = measure_cold_stats(&conn, root_folder_id, include_zero_byte)?;
    let peak_after_cold_stats = peak_working_set_bytes();
    let warm_stats = measure_warm_stats(
        &conn,
        root_folder_id,
        include_zero_byte,
        config.warm_iterations,
    )?;
    let peak_after_warm_stats = peak_working_set_bytes();

    let folder_cache = DedupFolderStatsCache::new();
    let cold_cached_list =
        measure_cold_cached_list(&conn, &folder_cache, list_limit, include_zero_byte)?;
    let peak_after_cold_cached_list = peak_working_set_bytes();
    let warm_cached_list = measure_warm_cached_list(
        &conn,
        &folder_cache,
        list_limit,
        include_zero_byte,
        config.warm_iterations,
    )?;
    let peak_after_warm_cached_list = peak_working_set_bytes();

    ensure_stable("list_duplicate_folder_candidates", &cold_list, &warm_list)?;
    ensure_stable("get_duplicate_folder_stats", &cold_stats, &warm_stats)?;
    ensure_stable(
        "cached list_duplicate_folder_candidates",
        &cold_cached_list,
        &warm_cached_list,
    )?;

    println!("\n  list_duplicate_folder_candidates:");
    println!(
        "    cold: elapsed={} rows={} checksum=0x{:016x} peak_ws_hwm={}",
        format_duration(cold_list.elapsed),
        cold_list.rows,
        cold_list.checksum,
        format_peak(peak_after_cold_list, peak_before),
    );
    print_warm_summary(&warm_list, peak_after_warm_list, peak_before);

    println!("\n  get_duplicate_folder_stats(folder_id={root_folder_id}):");
    println!(
        "    cold: elapsed={} rows={} checksum=0x{:016x} peak_ws_hwm={}",
        format_duration(cold_stats.elapsed),
        cold_stats.rows,
        cold_stats.checksum,
        format_peak(peak_after_cold_stats, peak_before),
    );
    print_warm_summary(&warm_stats, peak_after_warm_stats, peak_before);

    println!("\n  cached list_duplicate_folder_candidates:");
    println!(
        "    cold: elapsed={} rows={} checksum=0x{:016x} peak_ws_hwm={}",
        format_duration(cold_cached_list.elapsed),
        cold_cached_list.rows,
        cold_cached_list.checksum,
        format_peak(peak_after_cold_cached_list, peak_before),
    );
    print_warm_summary(&warm_cached_list, peak_after_warm_cached_list, peak_before);

    println!(
        "\n  note: peak_ws_hwm is process-level historical high-water mark; cold is SQLite page-cache approximation.",
    );

    Ok(())
}

fn seed_database(conn: &Connection, config: Config) -> Result<Counts, Box<dyn Error>> {
    conn.execute(
        "INSERT INTO scan_roots (id, path, alias) VALUES (?1, ?2, ?3)",
        params![1_i64, "/synthetic/scrollery-dedup-bench", "synthetic-bench"],
    )?;
    conn.execute(
        "INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth)
         VALUES (?1, ?2, NULL, ?3, ?4, 0)",
        params![1_i64, 1_i64, "", "bench-root"],
    )?;

    let transaction = conn.unchecked_transaction()?;
    {
        let mut directory_insert = transaction.prepare(
            "INSERT INTO directories
                 (id, root_id, parent_id, rel_path, name, depth)
             VALUES (?1, 1, 1, ?2, ?3, 1)",
        )?;
        for leaf_index in 0..config.leaf_directories {
            let directory_id = i64::try_from(leaf_index + 2)?;
            let path = format!("folder-{leaf_index:05}");
            directory_insert.execute(params![directory_id, &path, &path])?;
        }
    }

    {
        let mut media_insert = transaction.prepare(
            "INSERT INTO media_items
                 (id, directory_id, file_name, file_size, file_mtime, file_mtime_ns,
                  file_format, media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, ?2, ?3, ?4, 1, 1, 'jpg', 'image', 1920, 1080, ?1, ?1)",
        )?;
        let mut dedup_insert = transaction.prepare(
            "INSERT INTO dedup_index
                 (item_id, source_revision, hash_version, quick_digest, exact_digest,
                  unit_digest, unit_size, status, checked_at)
             VALUES (?1, 1, ?2, ?3, ?3, ?3, ?4, ?5, 1)",
        )?;

        for item_index in 0..config.items {
            let item_id = i64::try_from(item_index + 1)?;
            let directory_id = i64::try_from(2 + item_index % config.leaf_directories)?;
            let group = item_index / config.duplicate_group_width;
            let unit_size = 16_384_i64 + i64::try_from(group % 1_024)? * 1_024;
            let file_name = format!("item-{item_id:08}.jpg");
            let digest = synthetic_digest(group);

            media_insert.execute(params![item_id, directory_id, &file_name, unit_size])?;
            dedup_insert.execute(params![
                item_id,
                i64::from(DEDUP_HASH_VERSION),
                digest.as_slice(),
                unit_size,
                queries::STATUS_READY,
            ])?;
        }
    }
    transaction.commit()?;

    // 让 SQLite 在同一份数据上先收集真实索引统计信息，避免查询计划取决于“刚插入、尚未
    // ANALYZE”的偶然状态；该成本归入 data setup，不归入被测查询。
    conn.execute_batch("ANALYZE;")?;

    Ok(Counts {
        directories: count_rows(conn, "directories")?,
        media_items: count_rows(conn, "media_items")?,
        dedup_index: count_rows(conn, "dedup_index")?,
    })
}

fn count_rows(conn: &Connection, table: &str) -> Result<i64, rusqlite::Error> {
    // table 只来自本文件的固定字符串，不承载用户输入；查询值仍使用 SQLite 聚合结果。
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
}

fn measure_cold_list(
    conn: &Connection,
    limit: usize,
    include_zero_byte: bool,
) -> Result<QuerySample, Box<dyn Error>> {
    shrink_sqlite_memory(conn)?;
    let started = Instant::now();
    let rows = queries::list_duplicate_folder_candidates(conn, None, limit, include_zero_byte)?;
    let elapsed = started.elapsed();
    let checksum = rows_checksum(&rows);
    let row_count = rows.len();
    std::hint::black_box(&rows);
    Ok(QuerySample {
        elapsed,
        rows: row_count,
        checksum,
    })
}

fn measure_warm_list(
    conn: &Connection,
    limit: usize,
    include_zero_byte: bool,
    iterations: usize,
) -> Result<WarmSummary, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    let mut rows = None;
    let mut checksum = None;
    for _ in 0..iterations {
        let started = Instant::now();
        let result =
            queries::list_duplicate_folder_candidates(conn, None, limit, include_zero_byte)?;
        let elapsed = started.elapsed();
        let result_rows = result.len();
        let result_checksum = rows_checksum(&result);
        std::hint::black_box(&result);
        if rows.is_none() {
            rows = Some(result_rows);
            checksum = Some(result_checksum);
        }
        if rows != Some(result_rows) || checksum != Some(result_checksum) {
            return Err(Box::new(invalid_input(
                "warm list query returned unstable row count/checksum".to_string(),
            )));
        }
        samples.push(elapsed);
    }
    Ok(WarmSummary {
        samples,
        rows: rows.unwrap_or(0),
        checksum: checksum.unwrap_or(0),
    })
}

fn measure_cold_stats(
    conn: &Connection,
    folder_id: i64,
    include_zero_byte: bool,
) -> Result<QuerySample, Box<dyn Error>> {
    shrink_sqlite_memory(conn)?;
    let started = Instant::now();
    let row = queries::get_duplicate_folder_stats(conn, folder_id, include_zero_byte)?;
    let elapsed = started.elapsed();
    let checksum = row.as_ref().map(stats_checksum).unwrap_or(0);
    let row_count = usize::from(row.is_some());
    std::hint::black_box(&row);
    Ok(QuerySample {
        elapsed,
        rows: row_count,
        checksum,
    })
}

fn measure_warm_stats(
    conn: &Connection,
    folder_id: i64,
    include_zero_byte: bool,
    iterations: usize,
) -> Result<WarmSummary, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    let mut rows = None;
    let mut checksum = None;
    for _ in 0..iterations {
        let started = Instant::now();
        let result = queries::get_duplicate_folder_stats(conn, folder_id, include_zero_byte)?;
        let elapsed = started.elapsed();
        let result_rows = usize::from(result.is_some());
        let result_checksum = result.as_ref().map(stats_checksum).unwrap_or(0);
        std::hint::black_box(&result);
        if rows.is_none() {
            rows = Some(result_rows);
            checksum = Some(result_checksum);
        }
        if rows != Some(result_rows) || checksum != Some(result_checksum) {
            return Err(Box::new(invalid_input(
                "warm stats query returned unstable row count/checksum".to_string(),
            )));
        }
        samples.push(elapsed);
    }
    Ok(WarmSummary {
        samples,
        rows: rows.unwrap_or(0),
        checksum: checksum.unwrap_or(0),
    })
}

fn get_cached_snapshot(
    conn: &Connection,
    cache: &DedupFolderStatsCache,
    include_zero_byte: bool,
) -> Result<Arc<Vec<queries::DuplicateFolderStatsRow>>, Box<dyn Error>> {
    Ok(cache.get_or_build(conn, 1, 1, DEDUP_HASH_VERSION, false, include_zero_byte)?)
}

fn measure_cold_cached_list(
    conn: &Connection,
    cache: &DedupFolderStatsCache,
    limit: usize,
    include_zero_byte: bool,
) -> Result<QuerySample, Box<dyn Error>> {
    cache.clear();
    shrink_sqlite_memory(conn)?;
    let started = Instant::now();
    let snapshot = get_cached_snapshot(conn, cache, include_zero_byte)?;
    let rows = list_candidate_rows(&snapshot, None, limit);
    let elapsed = started.elapsed();
    let checksum = rows_checksum(&rows);
    let row_count = rows.len();
    std::hint::black_box(&rows);
    Ok(QuerySample {
        elapsed,
        rows: row_count,
        checksum,
    })
}

fn measure_warm_cached_list(
    conn: &Connection,
    cache: &DedupFolderStatsCache,
    limit: usize,
    include_zero_byte: bool,
    iterations: usize,
) -> Result<WarmSummary, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    let mut rows = None;
    let mut checksum = None;
    for _ in 0..iterations {
        let started = Instant::now();
        let snapshot = get_cached_snapshot(conn, cache, include_zero_byte)?;
        let result = list_candidate_rows(&snapshot, None, limit);
        let elapsed = started.elapsed();
        let result_rows = result.len();
        let result_checksum = rows_checksum(&result);
        std::hint::black_box(&result);
        if rows.is_none() {
            rows = Some(result_rows);
            checksum = Some(result_checksum);
        }
        if rows != Some(result_rows) || checksum != Some(result_checksum) {
            return Err(Box::new(invalid_input(
                "warm cached list query returned unstable row count/checksum".to_string(),
            )));
        }
        samples.push(elapsed);
    }
    Ok(WarmSummary {
        samples,
        rows: rows.unwrap_or(0),
        checksum: checksum.unwrap_or(0),
    })
}

fn shrink_sqlite_memory(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("PRAGMA shrink_memory;")
}

fn ensure_stable(name: &str, cold: &QuerySample, warm: &WarmSummary) -> Result<(), Box<dyn Error>> {
    if cold.rows != warm.rows || cold.checksum != warm.checksum {
        return Err(Box::new(invalid_input(format!(
            "{name} changed between cold and warm runs: cold rows/checksum={}/0x{:016x}, warm={}/0x{:016x}",
            cold.rows, cold.checksum, warm.rows, warm.checksum,
        ))));
    }
    Ok(())
}

fn print_warm_summary(summary: &WarmSummary, peak: Option<u64>, baseline: Option<u64>) {
    let mut sorted = summary.samples.clone();
    sorted.sort_unstable();
    let min = sorted[0];
    let median = sorted[sorted.len() / 2];
    let max = *sorted.last().expect("warm samples are non-empty");
    let mean_seconds = summary
        .samples
        .iter()
        .map(Duration::as_secs_f64)
        .sum::<f64>()
        / summary.samples.len() as f64;

    println!(
        "    warm: n={} min={} median={} mean={} max={} rows={} checksum=0x{:016x} peak_ws_hwm={}",
        summary.samples.len(),
        format_duration(min),
        format_duration(median),
        format_duration(Duration::from_secs_f64(mean_seconds)),
        format_duration(max),
        summary.rows,
        summary.checksum,
        format_peak(peak, baseline),
    );
}

fn rows_checksum(rows: &[queries::DuplicateFolderStatsRow]) -> u64 {
    rows.iter().fold(0xcbf2_9ce4_8422_2325, |hash, row| {
        hash.wrapping_mul(0x1000_0000_01b3)
            .wrapping_add(stats_checksum(row))
    })
}

fn stats_checksum(row: &queries::DuplicateFolderStatsRow) -> u64 {
    [
        row.folder_id,
        row.root_id,
        row.parent_id.unwrap_or(0),
        row.depth,
        row.total_positions,
        row.analyzed_positions,
        row.duplicate_positions,
        row.external_covered_positions,
        row.internal_duplicate_positions,
        row.unreviewed_positions,
        row.protected_positions,
        row.recommended_positions,
        row.recommended_logical_bytes,
        row.retained_positions,
    ]
    .into_iter()
    .fold(0x9e37_79b9_7f4a_7c15, |hash, value| {
        hash.rotate_left(7).wrapping_mul(0x1000_0000_01b3) ^ value as u64
    })
}

fn synthetic_digest(group: usize) -> [u8; 16] {
    let first = group as u64;
    let second = first.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17);
    let mut digest = [0_u8; 16];
    digest[..8].copy_from_slice(&first.to_le_bytes());
    digest[8..].copy_from_slice(&second.to_le_bytes());
    digest
}

fn parse_config() -> Result<Option<Config>, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config = Config {
        items: DEFAULT_ITEMS,
        leaf_directories: default_leaf_directories(DEFAULT_ITEMS),
        duplicate_group_width: DEFAULT_GROUP_WIDTH,
        warm_iterations: DEFAULT_WARM_ITERATIONS,
    };
    let mut directories_explicit = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => return Ok(None),
            "--items" => {
                let value = next_argument(&args, &mut index, "--items")?;
                config.items = parse_count(&value)?;
                if !directories_explicit {
                    config.leaf_directories = default_leaf_directories(config.items);
                }
            }
            "--directories" => {
                let value = next_argument(&args, &mut index, "--directories")?;
                config.leaf_directories = value
                    .parse()
                    .map_err(|_| format!("invalid --directories value: {value}"))?;
                directories_explicit = true;
            }
            "--duplicate-group-width" => {
                let value = next_argument(&args, &mut index, "--duplicate-group-width")?;
                config.duplicate_group_width = value
                    .parse()
                    .map_err(|_| format!("invalid --duplicate-group-width value: {value}"))?;
            }
            "--warm-iterations" => {
                let value = next_argument(&args, &mut index, "--warm-iterations")?;
                config.warm_iterations = value
                    .parse()
                    .map_err(|_| format!("invalid --warm-iterations value: {value}"))?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    if config.items == 0 {
        return Err("--items must be greater than zero".to_string());
    }
    if config.leaf_directories == 0 {
        return Err("--directories must be greater than zero".to_string());
    }
    if config.duplicate_group_width < 2 {
        return Err("--duplicate-group-width must be at least 2".to_string());
    }
    if config.warm_iterations == 0 {
        return Err("--warm-iterations must be greater than zero".to_string());
    }

    Ok(Some(config))
}

fn next_argument(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_count(value: &str) -> Result<usize, String> {
    let lower = value.to_ascii_lowercase();
    let (digits, multiplier) = if let Some(digits) = lower.strip_suffix('k') {
        (digits, 1_000_u64)
    } else if let Some(digits) = lower.strip_suffix('m') {
        (digits, 1_000_000_u64)
    } else {
        (lower.as_str(), 1_u64)
    };
    let base: u64 = digits
        .parse()
        .map_err(|_| format!("invalid count: {value}"))?;
    usize::try_from(
        base.checked_mul(multiplier)
            .ok_or_else(|| format!("count overflows u64: {value}"))?,
    )
    .map_err(|_| format!("count does not fit usize: {value}"))
}

fn default_leaf_directories(items: usize) -> usize {
    items
        .saturating_add(ITEMS_PER_DIRECTORY - 1)
        .checked_div(ITEMS_PER_DIRECTORY)
        .unwrap_or(1)
        .clamp(MIN_LEAF_DIRECTORIES, MAX_LEAF_DIRECTORIES)
}

fn invalid_input(message: String) -> IoError {
    IoError::new(ErrorKind::InvalidInput, message)
}

fn print_usage() {
    println!(
        "Usage: dedup_folder_bench [--items N|100k|1m] [--directories N] \\
         [--duplicate-group-width N] [--warm-iterations N]"
    );
    println!("Defaults: --items 100k, group width 4, warm iterations 3");
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds >= 1.0 {
        format!("{seconds:.3}s")
    } else if duration.as_millis() >= 1 {
        format!("{:.3}ms", seconds * 1_000.0)
    } else if duration.as_micros() >= 1 {
        format!("{:.3}us", seconds * 1_000_000.0)
    } else {
        format!("{}ns", duration.as_nanos())
    }
}

fn format_peak(current: Option<u64>, baseline: Option<u64>) -> String {
    match (current, baseline) {
        (Some(current), Some(baseline)) => format!(
            "{} ({:.1} MiB, +{})",
            current,
            current as f64 / 1_048_576.0,
            format_bytes(current.saturating_sub(baseline)),
        ),
        (Some(current), None) => format!("{} ({:.1} MiB)", current, current as f64 / 1_048_576.0),
        _ => "unavailable".to_string(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(windows)]
fn peak_working_set_bytes() -> Option<u64> {
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    result.is_ok().then_some(counters.PeakWorkingSetSize as u64)
}

#[cfg(unix)]
fn peak_working_set_bytes() -> Option<u64> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if result != 0 {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        Some(usage.ru_maxrss as u64)
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux/Android 以 KiB 报告 ru_maxrss；其它遵循同一 ABI 的 Unix 目标也按此保守换算。
        Some((usage.ru_maxrss as u64).saturating_mul(1_024))
    }
}

#[cfg(not(any(windows, unix)))]
fn peak_working_set_bytes() -> Option<u64> {
    None
}
