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
    THUMB_TIERS
        .iter()
        .copied()
        .filter(move |&tier| tier >= need_px && nonempty_tiers.contains(&tier))
        .scan(false, move |stopped, tier| {
            if *stopped {
                return None;
            }
            let rel = thumb_db_path(tier, cache_key);
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

        pub fn set_delays(&self, dir_us: u64, file_us: u64) {
            self.dir_delay_us.store(dir_us, Ordering::Relaxed);
            self.file_delay_us.store(file_us, Ordering::Relaxed);
        }

        pub fn set_results(&self, dir_nonempty: bool, file_exists: bool) {
            self.dir_has_entry_result
                .store(dir_nonempty, Ordering::Relaxed);
            self.file_exists_result
                .store(file_exists, Ordering::Relaxed);
        }

        /// 按档位建模目录非空集合(运行时按目录名匹配);比 set_results 的「全非空/全空」更贴近真实盘。
        pub fn set_nonempty_tiers(&self, tiers: &[u32]) {
            *self
                .nonempty_tiers
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = tiers.iter().map(|t| t.to_string()).collect();
        }

        /// 按相对路径建模逐项存在性(建夹具须与真实盘一致:文件在 ⇒ 其档目录必非空)。
        pub fn set_existing_rels(&self, rels: &[String]) {
            *self.existing_rels.lock().unwrap_or_else(|e| e.into_inner()) = Some(rels.to_vec());
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

        /// 计数清零(测量循环逐批重置;延迟/结果/门闸配置保留)。
        pub fn reset_counts(&self) {
            self.dir_calls.store(0, Ordering::Relaxed);
            self.file_calls.store(0, Ordering::Relaxed);
            self.under_lock_calls.store(0, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::test_io::{CountingIo, Gate};
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn key() -> i64 {
        0x00ff_00ff_00ff_00ffi64
    }

    /// 选档请求夹具:`dpr` 必须与 prepare/rewrite 用的一致(生产里同一个 dpr 值贯穿三段)。
    fn request(db_tier: u32, cell_px: f64, dpr: f64) -> ServeRequest {
        ServeRequest {
            cache_key: key(),
            need_px: serve_need_px(cell_px, cell_px * 0.75, dpr),
            db_path: thumb_db_path(db_tier, key()),
        }
    }

    fn prepare(
        io: &dyn ServeIo,
        probes: &ThumbProbeCache,
        dpr: f64,
        requests: &[ServeRequest],
    ) -> ThumbServe {
        ThumbServe::prepare_with(
            io,
            probes,
            Path::new("C:/fake-cache"),
            dpr,
            requests,
            Instant::now(),
        )
    }

    /// 候选列表契约:升序、只取 ≥ need 的非空档、在 DB 档处截止且不含 DB 档自身。
    #[test]
    fn candidate_rels_ascend_and_stop_before_db_tier() {
        let k = key();
        let db512 = thumb_db_path(512, k);
        let rels = |db_path: &str, need_px: u32, tiers: &[u32]| -> Vec<String> {
            candidate_rels(db_path, need_px, k, tiers).collect()
        };
        // need 96:非空 128/256/512 → 候选 128、256(512 即 DB 档,止于它)
        assert_eq!(
            rels(&db512, 96, &[128, 256, 512]),
            vec![thumb_db_path(128, k), thumb_db_path(256, k)]
        );
        // 传入顺序无关(档位常量源决定升序)
        assert_eq!(
            rels(&db512, 96, &[512, 256, 128]),
            vec![thumb_db_path(128, k), thumb_db_path(256, k)]
        );
        // 最小满足档就是 DB 档 → 候选为空(免 stat 快路径)
        assert!(rels(&thumb_db_path(64, k), 60, &[64, 512]).is_empty());
        // DB 档比 need 小 → 候选不被它截断(升级到更大档仍是合法重写)
        assert_eq!(
            rels(&thumb_db_path(64, k), 96, &[128, 512]),
            vec![thumb_db_path(128, k), thumb_db_path(512, k)]
        );
        // need 超过所有非空档 / 集合空 → 无候选(调用方回退 DB 路径)
        assert!(rels(&db512, 2000, &[64, 128, 512]).is_empty());
        assert!(rels(&db512, 64, &[]).is_empty());
        // 惰性:最小档命中后不再构造更大档候选(取首项即止,512 档路径不出现)。
        let mut lazy = candidate_rels(&db512, 96, k, &[128, 256, 512]);
        assert_eq!(lazy.next(), Some(thumb_db_path(128, k)));
        assert_eq!(lazy.next(), Some(thumb_db_path(256, k)));
        assert_eq!(lazy.next(), None);
    }

    /// 选档回退契约(不改):
    /// - 有更小满足档且文件存在 → 重写到该档;
    /// - 目标档与 DB 档相同 → 免重写(512 档服务 60px 小格照旧,不强制补 64 档);
    /// - 非空档里无满足档 / 空 DB 路径 → 保留 DB 路径。
    #[test]
    fn rewrite_picks_smaller_existing_tier_and_falls_back() {
        let io = CountingIo::default();
        io.set_results(true, true); // 5 档目录均非空,目标档文件存在
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve = prepare(&io, &probes, 1.0, &[request(512, 60.0, 1.0)]);
        // need 60 → 最小满足档 64(非 512)→ 重写
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(512, key()), 60.0, 45.0, key()),
            Some(thumb_db_path(64, key()))
        );
        // DB 已是目标档 → 免重写
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(64, key()), 60.0, 45.0, key()),
            None
        );
        // 空 db_path → 不重写
        assert_eq!(serve.rewrite_path("", 60.0, 45.0, key()), None);

        // 只有 512 非空(全 512 旧库)→ 60px 小格仍由 512 档服务:不重写、不强制补 64 档。
        let io_512 = CountingIo::default();
        io_512.set_results(false, true);
        let probes_512 = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve_512 = prepare(&io_512, &probes_512, 1.0, &[request(512, 60.0, 1.0)]);
        assert_eq!(
            serve_512.rewrite_path(&thumb_db_path(512, key()), 60.0, 45.0, key()),
            None
        );
        // need 超顶档 → 保留 DB 路径
        assert_eq!(
            serve_512.rewrite_path(&thumb_db_path(256, key()), 2000.0, 1500.0, key()),
            None
        );
    }

    /// 档目录非空但目标档文件缺失(LRU 驱逐/清缓存/换盘)→ 不重写,回退 DB 路径。
    /// 这是「删缓存仍能恢复」的服务侧一半:未在 prepare 时刻确认存在就不重写(确认之后文件仍可能
    /// 被删,那一刻由前端 404 自愈复位重生成——本模块不承诺原子磁盘存在性)。
    #[test]
    fn rewrite_falls_back_when_tier_file_missing() {
        let io = CountingIo::default();
        io.set_results(true, false); // 档目录非空,但目标档文件不在
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve = prepare(&io, &probes, 1.0, &[request(512, 60.0, 1.0)]);
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(512, key()), 60.0, 45.0, key()),
            None
        );
    }

    /// **S4 回归(稀疏多档)**:need 96(方形 64 格 × DPR1.5),128 档目录因别的图片非空、
    /// 本项缺 128、存在 256、DB 记 512 → 必须选 256(实际存在的最小满足档),不再整项回退 512。
    #[test]
    fn sparse_tiers_pick_next_existing_candidate() {
        let k = key();
        // 只有 256 这一份:128 探一次落空(1 stat)、256 命中(第 2 stat)即停,512 是 DB 档不再探。
        let io = CountingIo::default();
        io.set_nonempty_tiers(&[128, 256, 512]);
        io.set_existing_rels(&[thumb_db_path(256, k)]);
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve = prepare(&io, &probes, 1.5, &[request(512, 64.0, 1.5)]);
        assert_eq!(io.counts().1, 2, "升序探测两次即命中,候选有界");
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(512, k), 64.0, 48.0, k),
            Some(thumb_db_path(256, k))
        );

        // 更小候选(128)也存在 → 取 128 而非 256:命中即停,后续候选零 stat。
        let io128 = CountingIo::default();
        io128.set_nonempty_tiers(&[128, 256, 512]);
        io128.set_existing_rels(&[thumb_db_path(128, k), thumb_db_path(256, k)]);
        let probes128 = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve128 = prepare(&io128, &probes128, 1.5, &[request(512, 64.0, 1.5)]);
        assert_eq!(io128.counts().1, 1, "最小满足档已存在 → 只 stat 一次");
        assert_eq!(
            serve128.rewrite_path(&thumb_db_path(512, k), 64.0, 48.0, k),
            Some(thumb_db_path(128, k))
        );

        // 候选全缺(128/256 都被驱逐)→ 保留 DB 路径;512 是 DB 档,不产生额外 stat。
        let io_gone = CountingIo::default();
        io_gone.set_nonempty_tiers(&[128, 256, 512]);
        io_gone.set_existing_rels(&[]);
        let probes_gone = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve_gone = prepare(&io_gone, &probes_gone, 1.5, &[request(512, 64.0, 1.5)]);
        assert_eq!(io_gone.counts().1, 2, "候选各探一次");
        assert_eq!(
            serve_gone.rewrite_path(&thumb_db_path(512, k), 64.0, 48.0, k),
            None
        );

        // 同批多项指向同一候选(重复内容/跨行重复项):候选结果批内复用,不重复触盘。
        let io_dup = CountingIo::default();
        io_dup.set_nonempty_tiers(&[128, 256, 512]);
        io_dup.set_existing_rels(&[thumb_db_path(256, k)]);
        let probes_dup = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let reqs: Vec<ServeRequest> = (0..4).map(|_| request(512, 64.0, 1.5)).collect();
        let serve_dup = prepare(&io_dup, &probes_dup, 1.5, &reqs);
        assert_eq!(io_dup.counts().1, 2, "同批同一候选只判一次");
        assert_eq!(
            serve_dup.rewrite_path(&thumb_db_path(512, k), 64.0, 48.0, k),
            Some(thumb_db_path(256, k))
        );
    }

    #[test]
    fn rewrite_respects_dpr_scaling() {
        let io = CountingIo::default();
        io.set_results(true, true);
        // DPR2:60px CSS 格 → need 120 → 128 档(而非 64)
        let probes2 = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve_dpr2 = prepare(&io, &probes2, 2.0, &[request(512, 60.0, 2.0)]);
        assert_eq!(
            serve_dpr2.rewrite_path(&thumb_db_path(512, key()), 60.0, 60.0, key()),
            Some(thumb_db_path(128, key()))
        );
        // DPR1:同格 → 64 档
        let probes1 = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let serve_dpr1 = prepare(&io, &probes1, 1.0, &[request(512, 60.0, 1.0)]);
        assert_eq!(
            serve_dpr1.rewrite_path(&thumb_db_path(512, key()), 60.0, 60.0, key()),
            Some(thumb_db_path(64, key()))
        );
    }

    /// 每批取行的目录探测次数:冷 = 5 档各一次;TTL 内热 = 0;
    /// TTL 过期 / cache_dir 变更(换盘)/ 显式失效 = 重新 5 次。
    #[test]
    fn probe_cache_reuses_within_ttl_and_reevaluates_on_change() {
        let io = CountingIo::default();
        io.set_results(true, true);
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let dir_a = Path::new("C:/cache-a");
        let dir_b = Path::new("C:/cache-b");
        let t0 = Instant::now();
        assert_eq!(probes.tiers(&io, dir_a, t0).len(), 5);
        assert_eq!(io.counts().0, 5, "冷批 5 档各探一次");

        // TTL 内复用:零 IO。
        assert_eq!(probes.tiers(&io, dir_a, t0 + THUMB_PROBE_TTL / 2).len(), 5);
        assert_eq!(io.counts().0, 5, "TTL 内不得重探");

        // TTL 过期 → 重探。
        assert_eq!(probes.tiers(&io, dir_a, t0 + THUMB_PROBE_TTL * 2).len(), 5);
        assert_eq!(io.counts().0, 10, "TTL 过期须重探");

        // 换成另一个盘/目录 → 键不符自动重探(TTL 内也不例外)。
        assert_eq!(probes.tiers(&io, dir_b, t0).len(), 5);
        assert_eq!(io.counts().0, 15, "cache_dir 变更须重探");

        // 显式失效 → 重探。
        probes.invalidate();
        assert_eq!(probes.tiers(&io, dir_b, t0).len(), 5);
        assert_eq!(io.counts().0, 20, "显式失效须重探");
    }

    /// 同批重复目标档(重复内容/跨行重复项)只 stat 一次;无非空满足档时完全不 stat。
    #[test]
    fn prepare_stats_each_candidate_once() {
        // 所有档目录都空 → 无满足档 → 零 stat。
        let io = CountingIo::default();
        io.set_results(false, true);
        let probes = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let _ = prepare(&io, &probes, 1.0, &[request(512, 60.0, 1.0)]);
        assert_eq!(io.counts().1, 0, "无满足档 → 零 stat");

        // 64/512 非空:两个请求(DB 档不同)指向同一目标档 64 → 只 1 次 stat。
        let io2 = CountingIo::default();
        io2.set_results(true, true);
        let probes2 = ThumbProbeCache::new(THUMB_PROBE_TTL);
        let a = request(512, 60.0, 1.0);
        let b = request(128, 60.0, 1.0);
        let serve = prepare(&io2, &probes2, 1.0, &[a, b]);
        assert_eq!(io2.counts().1, 1, "同批同一目标档只 stat 一次");
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(512, key()), 60.0, 45.0, key()),
            Some(thumb_db_path(64, key()))
        );

        // 负向自检(同一门闸、同一 canary 断言):把「同步 IO 直接写在 async 任务里」的旧形态跑一遍——
        // 单 worker 被同步调用占死,canary 在 IO 期间必然排不上队。证明本测试真能区分两种形态。
        let rt_legacy = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let sync_gate = Arc::new(Gate::default());
        let gate_for_task = sync_gate.clone();
        rt_legacy.spawn(async move {
            gate_for_task.pass();
        });
        assert!(
            sync_gate.wait_entered_timeout(Duration::from_secs(10)),
            "负向自检:同步阻塞未进入"
        );
        let legacy_canary = Arc::new(AtomicBool::new(false));
        let legacy_set = legacy_canary.clone();
        rt_legacy.spawn(async move {
            legacy_set.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(100));
        let legacy_polled = legacy_canary.load(Ordering::SeqCst);
        sync_gate.release();
        rt_legacy.shutdown_timeout(Duration::from_secs(5));
        assert!(
            !legacy_polled,
            "负向自检失败:同步占死 worker 时 canary 仍被调度,本测试无法区分新旧形态"
        );
    }

    /// **机制锁**:probe + 逐项 stat 全跑在阻塞线程上——单 worker 的 runtime 在 IO 在途期间
    /// 必须仍能调度其他任务(canary)。旧实现(同步 IO 直接写在 async 命令里)在本门闸下永远
    /// 过不了:worker 被同步 IO 占死,canary 排不上队。
    #[test]
    fn offloaded_prepare_keeps_single_worker_free() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let gate = Arc::new(Gate::default());
        let io = Arc::new(CountingIo::default());
        io.set_gate(gate.clone());
        io.set_results(true, true);
        let probes = Arc::new(ThumbProbeCache::new(THUMB_PROBE_TTL));

        let handle = rt.handle().clone();
        let fetch = handle.spawn(prepare_offloaded_with(
            io.clone(),
            probes,
            PathBuf::from("C:/fake-cache"),
            1.0,
            vec![request(512, 60.0, 1.0)],
        ));
        assert!(
            gate.wait_entered_timeout(Duration::from_secs(10)),
            "IO 未进入(夹具失效)"
        );

        // IO 在途:同一 worker 必须还能调度 canary。
        let canary = Arc::new(AtomicBool::new(false));
        let canary_set = canary.clone();
        handle.spawn(async move {
            canary_set.store(true, Ordering::SeqCst);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !canary.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let polled = canary.load(Ordering::SeqCst);
        gate.release();
        let serve = rt
            .block_on(fetch)
            .expect("runtime join")
            .expect("spawn_blocking join");
        assert!(polled, "IO 期间单 worker 被占死 → IO 跑在 async 执行器上");
        // 阻塞化不改变产出:IO 结果照常被采用。
        assert_eq!(
            serve.rewrite_path(&thumb_db_path(512, key()), 60.0, 45.0, key()),
            Some(thumb_db_path(64, key()))
        );
    }
}
