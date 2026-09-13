// src-tauri/src/video/playable_cache.rs
//! 视频可播产物独立缓存池(视频格式扩展子系统 design.md §5.4)。
//!
//! **不共用缩略图 10GB 池**:转码/改封产物是 GB 级,共池会把全库几十 KB 级缩略图驱逐殆尽。
//! 独立池落 `{cache_dir}/video/`,LRU 按**最近播放时间**驱逐(触摸 = resolve 命中,`touch_played`)。
//! 默认 20GB([`DEFAULT_VIDEO_CACHE_MAX_MB`],D-444 ②),用户可经设置键 `video_cache_max_mb` 调整。
//!
//! 纯 IO 工具(无 async、无 DB、无 worker);调用方在 `spawn_blocking`/专用阻塞线程内跑
//! (遍历/删文件是阻塞 IO)。产物写入走 `*.tmp` + 同卷 rename(源自 worker 侧 work_dir 白名单)。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `video_cache_max_mb` 未配置时的 LRU 预算默认值(MB)= 20 GB(D-444 ②)。
pub const DEFAULT_VIDEO_CACHE_MAX_MB: u64 = 20 * 1024;

/// 单文件护栏比例(§5.4):产物预估 > 池预算 × 本比例 → host 回报 `needs_confirm`,由前端确认
/// (临时放行/调大池/放弃归 V7)。
const POOL_HALF: u64 = 2; // 50%

/// 视频池根目录 `{cache_dir}/video/`。也是 worker 侧 `VideoSessionInit.work_dir` 白名单前缀(同卷)。
pub fn pool_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("video")
}

/// 某源项的可播产物终名路径 `{cache_dir}/video/{cache_key}.mp4`(faststart 整文件 MP4,§5.2)。
pub fn playable_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    pool_dir(cache_dir).join(format!("{cache_key}.mp4"))
}

/// 产物中间态 `{cache_key}.mp4.tmp`(worker 写此,host 验收后同卷 rename 为终名)。
pub fn playable_tmp_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    pool_dir(cache_dir).join(format!("{cache_key}.mp4.tmp"))
}

/// 单文件产物字节预估(§5.4)。remux(`-c copy`)产物 ≈ 源大小;transcode 通常更小——故以
/// **源大小为保守上界**(宁可多弹一次确认,不冒撑爆池风险)。0 源大小回退为 0(不触护栏)。
pub fn estimate_output_bytes(source_bytes: u64) -> u64 {
    source_bytes
}

/// 预估是否超过池预算的 50%(§5.4 单文件护栏)。`max_mb` 由消费侧 [`read_video_cache_max_mb`]
/// 保证 > 0(config 侧 `filter(|&v| v>0)` 禁 0 回退默认,§V6-5),故此处不再设「0=无护栏」旁路——
/// 曾经的 0 旁路会让配错的 0 静默放行任意大产物,与「禁 0」语义统一后一并删除。
pub fn exceeds_half_pool(estimate_bytes: u64, max_mb: u64) -> bool {
    let budget_bytes = max_mb.saturating_mul(1024 * 1024);
    estimate_bytes > budget_bytes / POOL_HALF
}

/// resolve 命中已就绪产物时触摸 mtime = now,使 LRU 排序键反映**最近播放时间**(§5.4)。
/// 失败仅告警(不影响播放);跨平台契约走 `filetime`(不靠平台巧合)。
pub fn touch_played(path: &Path) {
    let now = filetime::FileTime::now();
    if let Err(e) = filetime::set_file_mtime(path, now) {
        tracing::warn!(path = %path.display(), error = %e, "触摸视频产物 mtime 失败(LRU 近期性略失准,不影响播放)");
    }
}

/// 启动清扫(§5.4/§9.6):删 `{cache_dir}/video/*.tmp` 残留(转码中途崩溃/应用退出遗留)。
/// 池目录不存在 = 从未产出过,noop 不报错。终名 `.mp4` 产物**不动**(它们是有效缓存)。
pub fn sweep_tmp(cache_dir: &Path) -> std::io::Result<()> {
    let dir = pool_dir(cache_dir);
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(path = %path.display(), error = %e, "清扫视频池 .tmp 残留失败");
            }
        }
    }
    Ok(())
}

/// 视频池占用统计(字节 + 文件数)。设置页缓存卡消费归 V7(CacheStats 扩容先例)。纯只读。
pub fn pool_stat(cache_dir: &Path) -> (u64, u64) {
    let dir = pool_dir(cache_dir);
    if !dir.exists() {
        return (0, 0);
    }
    let mut bytes = 0u64;
    let mut files = 0u64;
    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        // .tmp 中间态不计入占用(它们随下次 sweep/rename 消失)。
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            bytes += meta.len();
            files += 1;
        }
    }
    (bytes, files)
}

/// LRU 驱逐(§5.4):池占用超预算时按 mtime 升序(最旧先)删终名 `.mp4` 产物,直至回到预算内。
///
/// - **在播文件跳过**:`in_use` 内路径直接跳过;此外 Windows 删占用文件本就失败——删失败即
///   `continue` 跳过(不计入已释放),下轮再试(§9.7),不 panic、不中断。
/// - `.tmp` 中间态不参与驱逐(归 [`sweep_tmp`])。
/// - `max_mb=0` 视为无限,直接返回。
///
/// 返回删除的文件数。**阻塞 IO**,调用方在 `spawn_blocking`/阻塞线程内跑。
pub fn enforce_pool_limit(cache_dir: &Path, max_mb: u64, in_use: &HashSet<PathBuf>) -> usize {
    if max_mb == 0 {
        return 0;
    }
    let dir = pool_dir(cache_dir);
    if !dir.exists() {
        return 0;
    }
    let budget = max_mb.saturating_mul(1024 * 1024);

    // 收集终名 .mp4 产物(path, mtime, size)。
    let mut files: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    let mut total: u64 = 0;
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_mp4 = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
        if !is_mp4 {
            continue; // .tmp 及其他一律不参与驱逐
        }
        if let Ok(meta) = entry.metadata() {
            let size = meta.len();
            total += size;
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            files.push((path, mtime, size));
        }
    }

    if total <= budget {
        return 0;
    }

    // 最旧先(mtime 升序)= 最久未播先驱逐。
    files.sort_by_key(|(_, mtime, _)| *mtime);

    let mut freed: u64 = 0;
    let mut deleted = 0usize;
    for (path, _, size) in files {
        if total.saturating_sub(freed) <= budget {
            break;
        }
        if in_use.contains(&path) {
            continue; // 在播,跳过(下轮再试)
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                freed += size;
                deleted += 1;
            }
            Err(e) => {
                // Windows 删占用文件失败:跳过,下轮再试(§9.7),不中断整轮驱逐。
                tracing::warn!(path = %path.display(), error = %e, "驱逐视频产物失败(可能在播),跳过");
            }
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn tmp_root(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "scrollery-vpool-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        p
    }

    /// 写一个 `.mp4` 产物并把 mtime 设为「now - age」,用于构造确定的 LRU 顺序。
    fn write_mp4(cache_dir: &Path, cache_key: i64, size: usize, age: Duration) {
        std::fs::create_dir_all(pool_dir(cache_dir)).unwrap();
        let path = playable_path(cache_dir, cache_key);
        std::fs::write(&path, vec![0u8; size]).unwrap();
        let t = filetime::FileTime::from_system_time(SystemTime::now() - age);
        filetime::set_file_mtime(&path, t).unwrap();
    }

    #[test]
    fn estimate_and_half_pool_guard() {
        assert_eq!(estimate_output_bytes(1_000), 1_000);
        // 20GB 池的 50% = 10GB。
        let ten_gb = 10u64 * 1024 * 1024 * 1024;
        assert!(!exceeds_half_pool(ten_gb, DEFAULT_VIDEO_CACHE_MAX_MB));
        assert!(exceeds_half_pool(ten_gb + 1, DEFAULT_VIDEO_CACHE_MAX_MB));
        // 禁 0(§V6-5):config 侧回退默认使 max_mb 恒 > 0,不再有「0=无护栏」旁路。
    }

    #[test]
    fn sweep_removes_only_tmp() {
        let root = tmp_root("sweep");
        let pool = pool_dir(&root);
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(playable_path(&root, 1), b"keep").unwrap();
        std::fs::write(playable_tmp_path(&root, 2), b"drop").unwrap();
        std::fs::write(pool.join("3.MP4.TMP"), b"drop2").unwrap();

        sweep_tmp(&root).unwrap();
        assert!(playable_path(&root, 1).exists(), "终名 .mp4 保留");
        assert!(!playable_tmp_path(&root, 2).exists(), ".tmp 清除");
        assert!(!pool.join("3.MP4.TMP").exists(), "大小写不敏感 .tmp 清除");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sweep_missing_pool_is_noop() {
        let root = tmp_root("sweep-noop");
        assert!(sweep_tmp(&root).is_ok());
    }

    #[test]
    fn lru_evicts_oldest_until_under_budget() {
        let root = tmp_root("lru");
        // 预算 1 MB。写三个 ~400KB 产物,总 ~1.2MB 超预算;须驱逐最旧至回落 ≤1MB。
        let sz = 400 * 1024;
        write_mp4(&root, 1, sz, Duration::from_secs(300)); // 最旧
        write_mp4(&root, 2, sz, Duration::from_secs(200));
        write_mp4(&root, 3, sz, Duration::from_secs(100)); // 最新
        let deleted = enforce_pool_limit(&root, 1, &HashSet::new());
        assert_eq!(deleted, 1, "删一个 400KB 即回到 ~800KB ≤ 1MB");
        assert!(!playable_path(&root, 1).exists(), "最旧(key=1)被驱逐");
        assert!(playable_path(&root, 2).exists());
        assert!(playable_path(&root, 3).exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn lru_skips_in_use_file() {
        let root = tmp_root("lru-inuse");
        let sz = 400 * 1024;
        write_mp4(&root, 1, sz, Duration::from_secs(300)); // 最旧但在播
        write_mp4(&root, 2, sz, Duration::from_secs(200));
        write_mp4(&root, 3, sz, Duration::from_secs(100));
        let mut in_use = HashSet::new();
        in_use.insert(playable_path(&root, 1));
        let deleted = enforce_pool_limit(&root, 1, &in_use);
        assert_eq!(deleted, 1);
        assert!(playable_path(&root, 1).exists(), "在播文件跳过,不被驱逐");
        assert!(!playable_path(&root, 2).exists(), "次旧(key=2)被驱逐");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn lru_noop_within_budget_and_zero_max() {
        let root = tmp_root("lru-noop");
        write_mp4(&root, 1, 100, Duration::from_secs(10));
        assert_eq!(
            enforce_pool_limit(&root, 1024, &HashSet::new()),
            0,
            "预算内不驱逐"
        );
        assert_eq!(
            enforce_pool_limit(&root, 0, &HashSet::new()),
            0,
            "max=0 无限不驱逐"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pool_stat_counts_mp4_excludes_tmp() {
        let root = tmp_root("stat");
        let pool = pool_dir(&root);
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(playable_path(&root, 1), vec![0u8; 1000]).unwrap();
        std::fs::write(playable_tmp_path(&root, 2), vec![0u8; 500]).unwrap();
        let (bytes, files) = pool_stat(&root);
        assert_eq!((bytes, files), (1000, 1), ".tmp 不计入占用");
        std::fs::remove_dir_all(&root).ok();
    }
}
