use rusqlite::{params, Connection};
use scrollery_lib::utils::path::encode_tree_sort_key;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_TARGET: i64 = 500_000;
const DEFAULT_ROOT_PATH: &str = "C:/MockPhotos";
const DEFAULT_ALIAS: &str = "Mock Stress";
const DEFAULT_FILE_SIZE: i64 = 5 * 1024 * 1024;

#[derive(Clone, Copy)]
enum ThumbSeedStatus {
    Pending,
    Failed,
}

impl ThumbSeedStatus {
    fn as_db_value(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Failed => 2,
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "failed" => Ok(Self::Failed),
            other => Err(format!(
                "不支持的 --thumb-status: {other}，可选 pending/failed"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending(0)",
            Self::Failed => "failed(2)",
        }
    }
}

struct Options {
    db_path: PathBuf,
    root_path: String,
    alias: String,
    target: i64,
    file_size: i64,
    thumb_status: ThumbSeedStatus,
    reset_thumbs: bool,
    /// 子目录数(=1 保持旧行为:全部条目直挂根目录)。T16 folder 分组压测用。
    dirs: i64,
    /// sort_datetime 铺开的天数(=0 保持旧行为:逐条 +1 秒)。T16 date 分组压测用。
    days: i64,
}

impl Options {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut opts = Self {
            db_path: default_db_path()?,
            root_path: DEFAULT_ROOT_PATH.to_string(),
            alias: DEFAULT_ALIAS.to_string(),
            target: DEFAULT_TARGET,
            file_size: DEFAULT_FILE_SIZE,
            thumb_status: ThumbSeedStatus::Pending,
            reset_thumbs: true,
            dirs: 1,
            days: 0,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                "--db" => {
                    opts.db_path = PathBuf::from(next_value(&mut args, "--db")?);
                }
                "--root" => {
                    opts.root_path = next_value(&mut args, "--root")?;
                }
                "--alias" => {
                    opts.alias = next_value(&mut args, "--alias")?;
                }
                "--target" => {
                    opts.target = next_value(&mut args, "--target")?.parse()?;
                }
                "--file-size" => {
                    opts.file_size = next_value(&mut args, "--file-size")?.parse()?;
                }
                "--dirs" => {
                    opts.dirs = next_value(&mut args, "--dirs")?.parse()?;
                }
                "--days" => {
                    opts.days = next_value(&mut args, "--days")?.parse()?;
                }
                "--thumb-status" => {
                    let value = next_value(&mut args, "--thumb-status")?;
                    opts.thumb_status = ThumbSeedStatus::parse(&value)?;
                }
                "--reset-thumbs" => {
                    opts.reset_thumbs = true;
                }
                "--no-reset-thumbs" => {
                    opts.reset_thumbs = false;
                }
                other => {
                    return Err(format!("未知参数: {other}，使用 --help 查看用法").into());
                }
            }
        }

        if opts.target <= 0 {
            return Err("--target 必须大于 0".into());
        }
        if opts.file_size <= 0 {
            return Err("--file-size 必须大于 0".into());
        }
        if opts.dirs < 1 {
            return Err("--dirs 必须 ≥ 1".into());
        }
        if opts.days < 0 {
            return Err("--days 必须 ≥ 0".into());
        }

        Ok(opts)
    }
}

fn default_db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let app_data = std::env::var("APPDATA")?;
    Ok(PathBuf::from(app_data)
        .join("com.scrollery.app")
        .join("scrollery.db"))
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("{name} 缺少参数值").into())
}

fn print_usage() {
    println!(
        r#"用法:
  cargo run --bin mock_data -- [选项]

常用压测:
  cargo run --bin mock_data -- --target 500000 --reset-thumbs --thumb-status pending

T16 百万库(1000 目录 × 1000 日期;建议独立 root,便于在应用里整库删除):
  cargo run --bin mock_data -- --root C:/MockPhotos1M --alias "Mock 1M" --target 1000000 --dirs 1000 --days 1000

选项:
  --db <PATH>                 指定数据库路径，默认使用 APPDATA/com.scrollery.app/scrollery.db
  --root <PATH>               mock 扫描根路径，默认 C:/MockPhotos
  --alias <TEXT>              mock 扫描根别名，默认 Mock Stress
  --target <N>                mock 媒体条目目标数量，默认 500000
  --file-size <BYTES>         写入的模拟文件大小，默认 5242880
  --dirs <N>                  子目录数量(条目按 seq % N 均匀分布),默认 1(直挂根目录)
  --days <D>                  sort_datetime 铺开天数(连续块分天,每天 target/D 条,与
                              --dirs 正交:每目录内日期均匀),默认 0(旧行为逐条 +1 秒)
  --thumb-status pending|failed
                              新增/重置条目的缩略图状态，默认 pending
  --reset-thumbs              将已有 mock 条目重置为指定缩略图状态，默认开启
  --no-reset-thumbs           只补齐数量，不重置已有条目
"#
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Options::parse()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    println!("数据库路径: {:?}", opts.db_path);
    println!(
        "压测参数: target={} root={} file_size={} thumb_status={} reset_thumbs={}",
        opts.target,
        opts.root_path,
        opts.file_size,
        opts.thumb_status.label(),
        opts.reset_thumbs
    );

    if let Some(parent) = opts.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut conn = Connection::open(&opts.db_path)?;
    let root_id = ensure_root(&conn, &opts, now)?;
    let root_dir_id = ensure_root_directory(&conn, root_id, now)?;
    let dir_ids = ensure_sub_directories(&mut conn, root_id, root_dir_id, &opts, now)?;

    // 计数/重置/统计一律覆盖本 root 的全部目录(--dirs > 1 时条目分布在子目录)。
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_items
         WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)",
        params![root_id],
        |r| r.get(0),
    )?;
    println!("当前 mock 条目数: {count}");

    if count < opts.target {
        insert_missing_items(&mut conn, &opts, &dir_ids, count, now)?;
    } else {
        println!("mock 条目数量已达标，无需新增。");
    }

    if opts.reset_thumbs {
        // 压测关键：把 mock 条目保持在 pending，前端滚动时会持续触发视口缩略图队列。
        let changed = conn.execute(
            "UPDATE media_items
             SET thumb_status = ?2, thumb_path = NULL, thumbhash = NULL, updated_at = ?3
             WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)",
            params![root_id, opts.thumb_status.as_db_value(), now],
        )?;
        println!("已重置 mock 缩略图状态: {changed} 条");
    }

    let final_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_items
         WHERE directory_id IN (SELECT id FROM directories WHERE root_id = ?1)",
        params![root_id],
        |r| r.get(0),
    )?;

    // 每目录回写真实 media_count(根目录 + 全部子目录)。
    conn.execute(
        "UPDATE directories
         SET media_count = (SELECT COUNT(*) FROM media_items WHERE directory_id = directories.id)
         WHERE root_id = ?1",
        params![root_id],
    )?;
    conn.execute(
        "UPDATE scan_roots SET total_files = ?2, updated_at = ?3 WHERE id = ?1",
        params![root_id, final_count, now],
    )?;

    println!("完成。mock 条目总数: {final_count}");
    println!("最小验证: 启动应用，打开 Mock Stress 根目录，快速拖动图库滚动条；状态栏不应长期停在“视口缩略图: 处理中”。");

    Ok(())
}

fn ensure_root(
    conn: &Connection,
    opts: &Options,
    now: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT OR IGNORE INTO scan_roots (path, alias, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![opts.root_path, opts.alias, now],
    )?;
    conn.execute(
        "UPDATE scan_roots SET alias = ?2, updated_at = ?3 WHERE path = ?1",
        params![opts.root_path, opts.alias, now],
    )?;
    let root_id = conn.query_row(
        "SELECT id FROM scan_roots WHERE path = ?1",
        params![opts.root_path],
        |r| r.get(0),
    )?;
    Ok(root_id)
}

fn ensure_root_directory(
    conn: &Connection,
    root_id: i64,
    now: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT OR IGNORE INTO directories (root_id, rel_path, name, depth, media_count, created_at)
         VALUES (?1, '', 'root', 0, 0, ?2)",
        params![root_id, now],
    )?;
    let directory_id = conn.query_row(
        "SELECT id FROM directories WHERE root_id = ?1 AND rel_path = ''",
        params![root_id],
        |r| r.get(0),
    )?;
    Ok(directory_id)
}

/// 建/取 N 个子目录(folder_0000…),返回按 dir_index 排列的目录 id;--dirs 1 时退化为
/// 根目录本身(旧行为)。幂等:重跑/加量只补缺,不重复建。
fn ensure_sub_directories(
    conn: &mut Connection,
    root_id: i64,
    root_dir_id: i64,
    opts: &Options,
    now: i64,
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    if opts.dirs <= 1 {
        return Ok(vec![root_dir_id]);
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO directories
                (root_id, parent_id, rel_path, name, depth, media_count, created_at, tree_sort_key)
             VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?6)",
        )?;
        for i in 0..opts.dirs {
            let name = format!("folder_{i:04}");
            // 单段 rel_path=name;算 tree_sort_key(方案 B),否则 perf 库上 folder 键全空、
            // 排序退化按 d.id,恰好测不出持久列特性。根目录(rel_path='')经列 DEFAULT X'' 取空键。
            let key = encode_tree_sort_key(&name);
            stmt.execute(params![root_id, root_dir_id, name, name, now, key])?;
        }
    }
    tx.commit()?;
    // 零填充目录名保证字典序 = 数字序,按 rel_path 取回即为 dir_index 顺序。
    let mut stmt = conn.prepare(
        "SELECT id FROM directories
         WHERE root_id = ?1 AND parent_id = ?2
         ORDER BY rel_path",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![root_id, root_dir_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if ids.len() as i64 != opts.dirs {
        return Err(format!("子目录数不符: 期望 {},实际 {}", opts.dirs, ids.len()).into());
    }
    Ok(ids)
}

/// 补齐条目。seq → 目录/日期的映射是确定性的(dir = seq % dirs、day = seq*days/target,
/// 二者正交:每目录内日期均匀铺开),断点续跑/追加 target 不打乱既有分布。
fn insert_missing_items(
    conn: &mut Connection,
    opts: &Options,
    dir_ids: &[i64],
    existing_count: i64,
    now: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let to_insert = opts.target - existing_count;
    println!("开始新增 mock 条目: {to_insert}");

    // days 模式:日期铺满 [now − days 天, now),连续块分天,每天 target/days 条。
    // 天内偏移取 seq % per_day(≪ 86400 秒)——块内条目挤在天槽起点的小窗口里,
    // 无论 day_base 是否午夜对齐/本地时区如何,块都不跨自然日 → 每自然日条数精确均匀;
    // 槽内偏移唯一 + 槽间不相交 → sort_time 全局唯一,沿用旧约定 cache_key = sort_time。
    let per_day = (opts.target / opts.days.max(1)).max(1);
    let day_base = now - opts.days * 86_400;
    let legacy_start = now - 100_000_000;

    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO media_items (
                directory_id, file_name, file_size, file_mtime, file_format,
                media_type, width, height, sort_datetime, cache_key, thumb_status,
                is_favorited, is_deleted, rating, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, 'jpg', 'image', ?5, ?6, ?7, ?8, ?9, 0, 0, 0, ?10, ?10
            )",
        )?;

        for i in 0..to_insert {
            let seq = existing_count + i;
            let filename = format!("mock_stress_{seq:07}.jpg");
            let directory_id = dir_ids[(seq % dir_ids.len() as i64) as usize];
            let sort_time = if opts.days > 0 {
                let day_index = seq * opts.days / opts.target;
                day_base + day_index * 86_400 + (seq % per_day) % 86_400
            } else {
                legacy_start + seq
            };
            let (width, height) = match seq % 6 {
                0 => (1080, 1920),
                1 | 2 => (1920, 1080),
                3 => (1920, 1200),
                4 => (3000, 2000),
                _ => (1200, 1200),
            };

            stmt.execute(params![
                directory_id,
                filename,
                opts.file_size,
                sort_time,
                width,
                height,
                sort_time,
                sort_time,
                opts.thumb_status.as_db_value(),
                now
            ])?;

            if i > 0 && i % 50_000 == 0 {
                println!("已新增 {i} 条");
            }
        }
    }
    tx.commit()?;

    Ok(())
}
