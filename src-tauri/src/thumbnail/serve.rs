//! 多档缩略图源的服务选档(2026-08-16 缩略图性能线阶段 2;2026-09-12 P1-5 滚动热路径收口)。
//!
//! 背景:档位文件路径 `{tier}/{xx}/{hex}.webp`(hex = cache_key)完全确定,同项可存在多份
//! 不同档位产物;但 `media_items.thumb_path` 只记一份(生成时的档位),旧库普遍只有 512 档。
//! 生成侧视口批量对**未生成项**已按行高档位生成;缺口在**已生成项**——服务时永远回 DB
//! 旧档路径,极密视图也吃 512 源(冷区 bitmapLoad 中位 54-101ms 的输入,阶段 1 真机基线)。
//!
//! 本模块在出口拼装(hydrate_item,仅可视区 10^2 级)按需重写:need = ceil(max(格宽,格高)
//! × DPR) 设备像素,取**最小满足且磁盘存在**的档;缺档回退 DB 路径(todo 契约「缺档回退」)。
//! 只有 512 档的旧库照旧由 512 档服务小格——**不**为密集视图强制补 64 档(既有回退策略)。
//!
//! **S4(2026-09-12)稀疏多档升序探测**:档目录非空只说明「有别的图写过这一档」,不等于本项
//! 在这一档有文件。旧实现只验全局最小满足档这**一个**候选,本项缺该档就整项回退 DB
//! (need 96、全局有 128 目录、本项缺 128 而存 256、DB 记 512 时照旧吃 512 源)。现在按 ≥need
//! 的非空档**升序逐档探测、命中即停**,落到该项实际存在的最小满足档;探到 DB 档自身即止
//! ——DB 档路径免重写,更大档不会减小源字节。候选 ≤5 档、同批同一相对路径只 stat 一次,
//! 候选全缺仍回退 DB 路径。
//!
//! **P1-5 三段式(三滚动出口同一主流程)**:旧实现每批取行都在 async 命令线程上 probe 5 档
//! 目录,再在 ItemsCache 载荷读锁内逐项 exists()(慢盘上两者都直接顶在滚动热路径上):
//! ① 载荷读锁内(**零 IO**)只收集选档请求(见 layout::items_cache::collect_serve_requests);
//! ② [`prepare_offloaded`] 在阻塞线程内做全部磁盘 IO——probe(≤5 次 read_dir,结果按 cache_dir +
//!    TTL 进 [`ThumbProbeCache`] 复用)与候选档存在性 stat,既不在 async 执行器上,也不在
//!    载荷锁内(生成/批量 patch 的写锁不再被慢盘拖住);
//! ③ 载荷读锁内(**零 IO**)应用:只查 ② 已确认存在的目标档,命中才重写。
//! 应用阶段不触盘 ⇒ 判据漂移最坏只是某次重写不发生(回退 DB 路径)。**存在性确认只覆盖 prepare
//! 时刻**:确认之后文件仍可能被 LRU 驱逐/清缓存/换盘删掉,那一刻的 404 由前端自愈命令
//! (regenerate_missing_thumb)复位待重生成——本模块不承诺「呈现时刻文件仍在磁盘」。
//!
//! **失效模型**:存在性 stat 每批重判(唯一事实源——LRU 驱逐/清缓存/换盘即刻生效,404 自愈
//! 链路不变);probe 结果只是「省一次目录枚举」的提示,陈旧最坏退化为少一次重写或多一次 stat。
//! TTL 兜底档位目录增删;cache_dir 不符(换盘)即自动重探。命中即重写、未命中零改动,故不触碰
//! 布局缓存失效逻辑(DPR 更新走 compute_layout 的原子写,几何不依赖 DPR、无需重排)。

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rustc_hash::{FxHashMap, FxHashSet};

use super::cache::thumb_db_path;
use super::generator::THUMB_TIERS;
use super::scheduler::OutputFingerprint;

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
/// cache_dir 不符(换盘)自动重探;[`invalidate`](Self::invalidate) 供显式失效(测试/后续接线)。
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
    // 新产物只继承尺寸无关的家族；旧三段路径照旧探旧档。旧尺寸摘要不能反推其他档，
    // 避免给同源但不同质量的缩略图错误选档。
    let mut parts = db_path.split('/');
    let style = match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some(_), Some(_), Some(_), None, None) => Some(None),
        (Some(_), Some(fingerprint), Some(_), Some(_), None) => fingerprint
            .strip_prefix("family-")
            .filter(|hex| hex.len() == 32 && OutputFingerprint::from_hex(hex).is_some())
            .map(|_| Some(fingerprint)),
        _ => None,
    };
    THUMB_TIERS
        .iter()
        .copied()
        .filter(move |&tier| style.is_some() && tier >= need_px && nonempty_tiers.contains(&tier))
        .scan(false, move |stopped, tier| {
            if *stopped {
                return None;
            }
            let rel = match style.expect("invalid paths were filtered") {
                Some(family) => {
                    let legacy = thumb_db_path(tier, cache_key);
                    let (_, suffix) = legacy.split_once('/').expect("tier path");
                    format!("{tier}/{family}/{suffix}")
                }
                None => thumb_db_path(tier, cache_key),
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

/// 异步薄封装(生产入口,三滚动出口共用):probe + 逐项 stat 全部交给阻塞线程——
/// async 执行器只等结果,等待期间载荷读锁完全空置。
pub async fn prepare_offloaded(
    cache_dir: PathBuf,
    dpr: f64,
    requests: Vec<ServeRequest>,
) -> Result<ThumbServe, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        ThumbServe::prepare_with(
            &RealServeIo,
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
        pub fn set_results(&self, dir_nonempty: bool, file_exists: bool) {
            self.dir_has_entry_result
                .store(dir_nonempty, Ordering::Relaxed);
            self.file_exists_result
                .store(file_exists, Ordering::Relaxed);
        }

        pub fn set_lock_probe(&self, probe: impl Fn() -> bool + Send + 'static) {
            *self.lock_free.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(probe));
        }

        pub fn counts(&self) -> (u64, u64) {
            (
                self.dir_calls.load(Ordering::Relaxed),
                self.file_calls.load(Ordering::Relaxed),
            )
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
