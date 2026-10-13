//! 多档缩略图源的服务选档。
//!
//! 同一项可存多档产物，但 `media_items.thumb_path` 只记录一档；服务时可选用更贴近视口需求的现有档。
//!
//! 所需尺寸为 ceil(max(格宽,格高) × DPR) 设备像素；按升序选取最小满足且磁盘存在的档。
//! 没有可用候选时沿用 DB 路径，不因密集视图强制生成小档。
//!
//! 档目录非空不代表本项有该档文件，须逐项探测；探到 DB 档即止，更大档不会减少源字节。
//!
//! 载荷读锁内只收集和应用选档请求；[`prepare_offloaded`] 在阻塞线程探测目录与文件。
//! 存在性只在准备时确认；之后若文件被驱逐，前端通过 `regenerate_missing_thumb` 自愈。
//!
//! 文件存在性每批重判；目录探测结果按 cache_dir 与 TTL 复用，过期或换盘时重探。

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rustc_hash::{FxHashMap, FxHashSet};

use super::cache::thumb_db_path;
use super::generator::THUMB_TIERS;

/// 目录探测结果的复用窗口:滚动热路径每批取行都重探 5 档目录纯属重复;窗口内复用同一份结果,
/// 档位目录的增删最多晚一个窗口被看见——存在性 stat 仍逐批真值,故只是提示层的延迟。
pub const THUMB_PROBE_TTL: Duration = Duration::from_millis(1000);

/// 选档磁盘 IO 的适配层(探针 seam):生产实现走真实文件系统;测试注入计数/延迟/门闸实现,
/// 以可控方式复现慢盘,并断言 IO 不在载荷读锁内、不在 async 执行器上(见 tests)。
pub trait ServeIo: Send + Sync {
    /// 目录是否存在非空条目——probe 判档用(判据同旧实现:read_dir 成功且首项存在)。
    fn dir_has_entry(&self, dir: &Path) -> bool;
    /// 文件是否存在——候选档的存在性校验。
    fn file_exists(&self, file: &Path) -> bool;
}

/// 生产实现:真实文件系统。
pub struct RealServeIo;

impl ServeIo for RealServeIo {
    fn dir_has_entry(&self, dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .map(|mut rd| rd.next().is_some())
            .unwrap_or(false)
    }

    fn file_exists(&self, file: &Path) -> bool {
        file.exists()
    }
}

/// probe 结果槽(进程内单槽:同一时刻只有一个缩略图 cache_dir)。
struct ProbeEntry {
    cache_dir: PathBuf,
    probed_at: Instant,
    nonempty_tiers: Vec<u32>,
}

/// 非空档集合的可失效缓存(见 [`THUMB_PROBE_TTL`]):TTL 未过且 cache_dir 相同则零 IO 复用;
/// cache_dir 不符时自动重探；[`invalidate`](Self::invalidate) 可显式失效。
pub struct ThumbProbeCache {
    ttl: Duration,
    slot: Mutex<Option<ProbeEntry>>,
}

impl ThumbProbeCache {
    pub const fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            slot: Mutex::new(None),
        }
    }

    /// 取 cache_dir 下的非空档集合:未过期的同目录缓存零 IO 返回,否则重探 ≤5 次 read_dir。
    pub fn tiers(&self, io: &dyn ServeIo, cache_dir: &Path, now: Instant) -> Vec<u32> {
        {
            let slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = slot.as_ref() {
                if entry.cache_dir == cache_dir
                    && now.saturating_duration_since(entry.probed_at) < self.ttl
                {
                    return entry.nonempty_tiers.clone();
                }
            }
        }
        let thumb_root = cache_dir.join("thumbnails");
        let nonempty_tiers: Vec<u32> = THUMB_TIERS
            .iter()
            .copied()
            .filter(|&tier| io.dir_has_entry(&thumb_root.join(tier.to_string())))
            .collect();
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(ProbeEntry {
            cache_dir: cache_dir.to_path_buf(),
            probed_at: now,
            nonempty_tiers: nonempty_tiers.clone(),
        });
        nonempty_tiers
    }

    /// 显式作废(清缓存/档位变更可立即重探)。**仅测试使用**:生产无调用——probe 是纯提示,
    /// 1s TTL 自愈即可;清缓存/换盘的呈现正确性由逐项存在性 stat(每批重判)保证,与 probe 无关。
    #[cfg(test)]
    pub fn invalidate(&self) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 生产共享槽(单 App 单 cache_dir)。probe 结果是**纯提示**:不进任何失效契约,
/// 故放在服务层自带失效,而不是往 AppState 加字段。
static SHARED_PROBES: LazyLock<Arc<ThumbProbeCache>> =
    LazyLock::new(|| Arc::new(ThumbProbeCache::new(THUMB_PROBE_TTL)));

/// 一次取行批的选档请求(载荷读锁内收集,纯内存)。
pub struct ServeRequest {
    pub cache_key: i64,
    /// 设备像素需求 = [`serve_need_px`](格宽, 格高, DPR)。
    pub need_px: u32,
    /// 当前 DB 相对路径:目标档与之相同即免重写(与旧实现同一条「免 stat」快捷)。
    pub db_path: String,
}

/// 纯函数:设备像素需求 = ceil(max(格宽, 格高) × DPR),下限 1。
/// DPR 钳到 [0.25, 4] 防异常值放大 need;收集与应用两处共用本函数,保证同批判据同源。
pub fn serve_need_px(cell_w: f64, cell_h: f64, dpr: f64) -> u32 {
    ((cell_w.max(cell_h) * dpr.clamp(0.25, 4.0)).ceil() as u32).max(1)
}

/// 选档与提交校验共用版本路径识别，保留媒体 ID、源代次和策略，只替换尺寸。
fn versioned_suffix(path: &str) -> Option<&str> {
    let (_, suffix) = path.split_once('/')?;
    let mut parts = suffix.split('/');
    parts.next()?; // 缓存键前缀。
    let identity = parts.next()?;
    // 新产物含媒体 ID；既有不含媒体 ID 的版本化产物仍须验证数据库提交。
    let revision = if identity.starts_with('i') {
        parts.next()?
    } else {
        identity
    };
    (revision.starts_with('r') && revision.contains('-')).then_some(suffix)
}

/// 纯函数:本项**可重写的升序目标档候选**(相对路径,惰性 yield)。
///
/// 按全局档位常量升序枚举,滤出 ≥ need 的非空档;遇 DB 档自身即止——那一刻的路径无需重写,
/// 且比它更大的档不会减小源字节(全库只有 512 档的项因此零候选、零 stat,与旧实现同一条
/// 免 stat 快捷)。无满足档 / 集合空 → 空迭代。候选上限即 [THUMB_TIERS] 5 档,且路径按需构造
/// ——调用方按序探测、命中即停,最小档命中就不会再拼后续档的字符串。
fn candidate_rels<'a>(
    db_path: &'a str,
    need_px: u32,
    cache_key: i64,
    nonempty_tiers: &'a [u32],
) -> impl Iterator<Item = String> + 'a {
    THUMB_TIERS
        .iter()
        .copied()
        .filter(move |&tier| tier >= need_px && nonempty_tiers.contains(&tier))
        .scan(false, move |stopped, tier| {
            if *stopped {
                return None;
            }
            let rel = if let Some(suffix) = versioned_suffix(db_path) {
                format!("{tier}/{suffix}")
            } else {
                thumb_db_path(tier, cache_key)
            };
            if rel == db_path {
                *stopped = true;
                return None;
            }
            Some(rel)
        })
}

/// 一次取行批的选档上下文:DPR + 非空档集合 + 已确认存在的目标档集合。
/// 只有 [`prepare_with`](Self::prepare_with) 触盘;此后 [`rewrite_path`](Self::rewrite_path)
/// 是纯内存查表,可在载荷读锁内安全调用(见模块级三段式说明)。
pub struct ThumbServe {
    dpr: f64,
    nonempty_tiers: Vec<u32>,
    verified: FxHashSet<String>,
}

impl ThumbServe {
    /// 唯一实现(阻塞线程内调用):probe 非空档(经可失效缓存)+ 对每项**升序候选**逐档 stat,
    /// 命中该项实际存在的最小满足档即停(S4,见模块级说明);结果为「已确认存在的目标档」集合。
    ///
    /// stat 有界:每项候选 ≤5 档([`candidate_rels`]),同一相对路径在批内只判一次——
    /// 结果进 `probed` 复用,重复内容/跨行重复项不重复触盘。
    /// IO/时钟/缓存均可注入,测试据此复现慢盘与冷热。
    pub fn prepare_with(
        io: &dyn ServeIo,
        probes: &ThumbProbeCache,
        cache_dir: &Path,
        dpr: f64,
        requests: &[ServeRequest],
        now: Instant,
    ) -> Self {
        let nonempty_tiers = probes.tiers(io, cache_dir, now);
        let mut verified: FxHashSet<String> = FxHashSet::default();
        if !nonempty_tiers.is_empty() {
            let thumb_root = cache_dir.join("thumbnails");
            // 同批候选结果:同一相对路径最多 stat 一次,跨项复用。
            let mut probed: FxHashMap<String, bool> = FxHashMap::default();
            for r in requests {
                for rel in candidate_rels(&r.db_path, r.need_px, r.cache_key, &nonempty_tiers) {
                    let exists = match probed.get(&rel) {
                        Some(&hit) => hit,
                        None => {
                            let hit = io.file_exists(&thumb_root.join(&rel));
                            probed.insert(rel.clone(), hit);
                            hit
                        }
                    };
                    if exists {
                        // 该项已取到实际存在的最小满足档,不再向上探。
                        verified.insert(rel);
                        break;
                    }
                }
            }
        }
        Self {
            dpr: dpr.clamp(0.25, 4.0),
            nonempty_tiers,
            verified,
        }
    }

    /// 按需重写 DB 相对路径(**载荷读锁内调用,零 IO**):在升序候选里取第一个 prepare 已确认
    /// 存在者。候选全缺失、DB 档自身即最小满足档(候选列表为空,免重写快路径)、或 db_path 为空
    /// (自愈/未生成)时返回 None,调用方保留原路径。
    pub fn rewrite_path(
        &self,
        db_path: &str,
        cell_w: f64,
        cell_h: f64,
        cache_key: i64,
    ) -> Option<String> {
        if db_path.is_empty() {
            return None;
        }
        let need_px = serve_need_px(cell_w, cell_h, self.dpr);
        candidate_rels(db_path, need_px, cache_key, &self.nonempty_tiers)
            .into_iter()
            .find(|rel| self.verified.contains(rel))
    }
}

struct CommittedIo<'a> {
    root: &'a Path,
    committed: FxHashSet<String>,
}
impl ServeIo for CommittedIo<'_> {
    fn dir_has_entry(&self, directory: &Path) -> bool {
        RealServeIo.dir_has_entry(directory)
    }
    fn file_exists(&self, file: &Path) -> bool {
        let relative = file
            .strip_prefix(self.root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let allowed = relative
            .as_deref()
            .is_some_and(|path| versioned_suffix(path).is_none() || self.committed.contains(path));
        allowed && RealServeIo.file_exists(file)
    }
}

/// 异步薄封装(生产入口,三滚动出口共用):probe + 逐项 stat 全部交给阻塞线程——
/// async 执行器只等结果,等待期间载荷读锁完全空置。
pub async fn prepare_offloaded(
    cache_dir: PathBuf,
    dpr: f64,
    requests: Vec<ServeRequest>,
    pool: crate::db::DbPool,
) -> Result<ThumbServe, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        // 版本化产物必须已有当前源的提交记录，不能仅凭并行写线程留下的文件选档。
        let mut committed = FxHashSet::default();
        let paths: Vec<_> = requests
            .iter()
            .flat_map(|request| {
                candidate_rels(
                    &request.db_path,
                    request.need_px,
                    request.cache_key,
                    &THUMB_TIERS,
                )
            })
            .filter(|path| versioned_suffix(path).is_some())
            .collect();
        if let Ok(conn) = pool.get() {
            for chunk in paths.chunks(128) {
                let placeholders = vec!["?"; chunk.len()].join(",");
                let sql = format!(
                    "SELECT a.path FROM image_artifacts a JOIN media_items m ON m.id=a.item_id \
                     WHERE a.kind='display' AND a.status=1 AND m.is_deleted=0 \
                     AND m.source_revision=a.source_revision AND m.cache_key=a.cache_key \
                     AND a.path IN ({placeholders})"
                );
                if let Ok(mut statement) = conn.prepare(&sql) {
                    if let Ok(rows) = statement
                        .query_map(rusqlite::params_from_iter(chunk), |row| {
                            row.get::<_, String>(0)
                        })
                    {
                        committed.extend(rows.filter_map(std::result::Result::ok));
                    }
                }
            }
        }
        let thumb_root = cache_dir.join("thumbnails");
        ThumbServe::prepare_with(
            &CommittedIo {
                root: &thumb_root,
                committed,
            },
            &SHARED_PROBES,
            &cache_dir,
            dpr,
            &requests,
            Instant::now(),
        )
    })
    .await
}

/// 可注入 IO/缓存的阻塞化变体(**仅测试**):借它与生产同一个 spawn_blocking 调度路径,
/// 以受控 IO(计数/延迟/门闸)复现慢盘与在途竞态。
#[cfg(test)]
pub async fn prepare_offloaded_with(
    io: Arc<dyn ServeIo>,
    probes: Arc<ThumbProbeCache>,
    cache_dir: PathBuf,
    dpr: f64,
    requests: Vec<ServeRequest>,
) -> Result<ThumbServe, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        ThumbServe::prepare_with(
            io.as_ref(),
            probes.as_ref(),
            &cache_dir,
            dpr,
            &requests,
            Instant::now(),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::generator::ThumbConfig;

    fn config(cache_dir: &Path, size: u32) -> ThumbConfig {
        ThumbConfig {
            cache_dir: cache_dir.to_path_buf(),
            size,
            skip_max_bytes: 0,
            strategy: "auto".into(),
            gpu_engine: "wic".into(),
            ai_hq_cache: false,
            webp_quality: 80,
            ai_cache_short_edge: 336,
        }
    }

    fn write_variant(cache_dir: &Path, relative: &str) {
        let path = cache_dir.join("thumbnails").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"thumbnail").unwrap();
    }

    fn request(db_path: &str) -> ServeRequest {
        ServeRequest {
            cache_key: 91,
            need_px: 96,
            db_path: db_path.into(),
        }
    }

    #[test]
    fn item_variants_select_smallest_tier_without_legacy_fallback() {
        let cache = tempfile::tempdir().unwrap();
        // 路径直接来自生产生成器，避免测试也沿用漏掉媒体 ID 的旧格式。
        let large = config(cache.path(), 512).display_path(7, 2, 91);
        let small = config(cache.path(), 128).display_path(7, 2, 91);
        let medium = config(cache.path(), 256).display_path(7, 2, 91);
        write_variant(cache.path(), &large);
        write_variant(cache.path(), &small);
        write_variant(cache.path(), &medium);
        write_variant(cache.path(), &thumb_db_path(128, 91));
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let prepare = || {
            ThumbServe::prepare_with(
                &RealServeIo,
                &probes,
                cache.path(),
                1.0,
                &[request(&large)],
                Instant::now(),
            )
        };
        assert_eq!(
            prepare().rewrite_path(&large, 96.0, 96.0, 91),
            Some(small.clone())
        );
        std::fs::remove_file(cache.path().join("thumbnails").join(&small)).unwrap();
        assert_eq!(
            prepare().rewrite_path(&large, 96.0, 96.0, 91),
            Some(medium.clone())
        );
        std::fs::remove_file(cache.path().join("thumbnails").join(&medium)).unwrap();
        assert_eq!(prepare().rewrite_path(&large, 96.0, 96.0, 91), None);
    }

    #[tokio::test]
    async fn item_variants_require_current_committed_artifacts() {
        let cache = tempfile::tempdir().unwrap();
        let db_path = cache.path().join("library.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        crate::db::connection::apply_pragmas(&conn).unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots(id,path) VALUES(1,'C:/photos');
             INSERT INTO directories(id,root_id,rel_path,name) VALUES(1,1,'','photos');
             INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,
                 file_format,sort_datetime,cache_key,source_revision)
                 VALUES(7,1,'image.jpg',10,11,'jpg',12,91,2);",
        )
        .unwrap();
        let pool = crate::db::create_read_pool(&db_path, 1).unwrap();
        let large = config(cache.path(), 512).display_path(7, 2, 91);
        let small_config = config(cache.path(), 128);
        let small = small_config.display_path(7, 2, 91);
        write_variant(cache.path(), &large);
        write_variant(cache.path(), &small);
        let prepare = || {
            prepare_offloaded(
                cache.path().to_path_buf(),
                1.0,
                vec![request(&large)],
                pool.clone(),
            )
        };
        // 文件已落盘但尚未提交，不能抢先展示。
        assert_eq!(
            prepare()
                .await
                .unwrap()
                .rewrite_path(&large, 96.0, 96.0, 91),
            None
        );
        conn.execute(
            "INSERT INTO image_artifacts(item_id,source_revision,cache_key,kind,fingerprint,status,path)
             VALUES(?1,?2,?3,'display',?4,1,?5)",
            rusqlite::params![7, 2, 91, small_config.display_fingerprint(), small],
        )
        .unwrap();
        assert_eq!(
            prepare()
                .await
                .unwrap()
                .rewrite_path(&large, 96.0, 96.0, 91),
            Some(small)
        );
        // 选档请求仍持有旧路径时，也必须拒绝源换代后的旧提交。
        conn.execute(
            "UPDATE media_items SET source_revision=?1 WHERE id=?2",
            [3, 7],
        )
        .unwrap();
        assert_eq!(
            prepare()
                .await
                .unwrap()
                .rewrite_path(&large, 96.0, 96.0, 91),
            None
        );
    }
}

/// 测试用可控 IO 探针(仅 test 构建):计数 + 可注入延迟 + 门闸 + 载荷锁探测。
#[cfg(test)]
pub(crate) mod test_io {
    use super::ServeIo;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    /// 可控门闸:IO 线程进入后置位 entered 并阻塞到 release。测试据此在「IO 在途」的确定
    /// 状态下观察 async 执行器是否仍能调度别的任务(不依赖真实磁盘速度)。
    #[derive(Default)]
    pub struct Gate {
        state: Mutex<(bool, bool)>,
        cv: Condvar,
    }

    impl Gate {
        /// IO 侧(含负向自检里的「同步 IO」替身):标记已进入并等待放行。
        pub fn pass(&self) {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.0 = true;
            self.cv.notify_all();
            while !s.1 {
                s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        }

        /// 测试侧:等到有 IO 进入(超时返回 false,防夹具失效时挂死测试)。
        pub fn wait_entered_timeout(&self, timeout: std::time::Duration) -> bool {
            let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let (s, _) = self
                .cv
                .wait_timeout_while(s, timeout, |s| !s.0)
                .unwrap_or_else(|e| e.into_inner());
            s.0
        }

        pub fn release(&self) {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.1 = true;
            self.cv.notify_all();
        }
    }

    /// 计数/延迟/门闸/锁探测四合一假 IO。延迟参数是**显式模型值**(如 2ms/read_dir 代表慢盘),
    /// 不代表任何真实设备;调用次数与「调用时载荷锁是否空闲」是确定的。
    #[derive(Default)]
    pub struct CountingIo {
        pub dir_calls: AtomicU64,
        pub file_calls: AtomicU64,
        pub dir_delay_us: AtomicU64,
        pub file_delay_us: AtomicU64,
        pub dir_has_entry_result: AtomicBool,
        pub file_exists_result: AtomicBool,
        gate: Mutex<Option<Arc<Gate>>>,
        /// 每次 IO 调用时执行「载荷锁是否空闲」探测(返回 true = 未被载荷锁占用)。
        lock_free: Mutex<Option<Box<dyn Fn() -> bool + Send>>>,
        /// 载荷锁内发生的 IO 次数(探测为 false 的调用数)。
        pub under_lock_calls: AtomicU64,
        /// 「非空档目录名」白名单(如 ["512"])。空 → 用 dir_has_entry_result 统一返回。
        nonempty_tiers: Mutex<Vec<String>>,
        /// 逐项「确实存在」的相对路径名单(如 thumb_db_path(256, key));
        /// None → 用 file_exists_result 统一返回,Some → 只认名单里的路径后缀。
        existing_rels: Mutex<Option<Vec<String>>>,
    }

    impl CountingIo {
        pub fn set_gate(&self, gate: Arc<Gate>) {
            *self.gate.lock().unwrap_or_else(|e| e.into_inner()) = Some(gate);
        }

        pub fn set_results(&self, dir_nonempty: bool, file_exists: bool) {
            self.dir_has_entry_result
                .store(dir_nonempty, Ordering::Relaxed);
            self.file_exists_result
                .store(file_exists, Ordering::Relaxed);
        }

        pub fn under_lock(&self) -> u64 {
            self.under_lock_calls.load(Ordering::Relaxed)
        }

        fn enter(&self, calls: &AtomicU64, delay_us: &AtomicU64) {
            calls.fetch_add(1, Ordering::Relaxed);
            {
                let probe = self.lock_free.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(probe) = probe.as_ref() {
                    if !probe() {
                        self.under_lock_calls.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let us = delay_us.load(Ordering::Relaxed);
            if us > 0 {
                std::thread::sleep(std::time::Duration::from_micros(us));
            }
            let gate = self
                .gate
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .cloned();
            if let Some(gate) = gate {
                gate.pass();
            }
        }
    }

    impl ServeIo for CountingIo {
        fn dir_has_entry(&self, dir: &Path) -> bool {
            self.enter(&self.dir_calls, &self.dir_delay_us);
            let tiers = self
                .nonempty_tiers
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if tiers.is_empty() {
                self.dir_has_entry_result.load(Ordering::Relaxed)
            } else {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| tiers.iter().any(|t| t == n))
            }
        }

        fn file_exists(&self, file: &Path) -> bool {
            self.enter(&self.file_calls, &self.file_delay_us);
            let rels = self.existing_rels.lock().unwrap_or_else(|e| e.into_inner());
            match rels.as_ref() {
                Some(rels) => rels.iter().any(|rel| file.ends_with(Path::new(rel))),
                None => self.file_exists_result.load(Ordering::Relaxed),
            }
        }
    }
}
