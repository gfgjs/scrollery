// src-tauri/src/ai/search_control.rs
//! 语义搜索控制面(P1-3):请求身份代次、常驻缓存快照与提交线性化。
//!
//! 生产路径(ai_commands::semantic_search_cmd)的时序是:profile → ai-worker 编码文本 →
//! 打分 → 整表覆写 ai_search_results。这条路径上有两处共享态此前没有被身份约束:
//!
//! 1. **常驻缓存**:装载只认 model_name,且「确保装载」与「取读打分」分两次取锁,两步之间
//!    换模型/失效会让查询向量对上另一模型的向量空间——同维静默算错,异维按缓存维度索引
//!    panic(前端只收到笼统 JoinError)。
//! 2. **共享结果表**:ai_search_results 整表单写、谁后提交谁赢。用户清空搜索或切换模型后,
//!    一条迟到的旧请求仍会把旧结果写回去;前端 latest-only 令牌只护展示态,护不住共享表。
//!
//! 本模块把这两件事收进一个控制面对象(每个 AppState 一份):
//!
//! - **入口登记**:[`SearchControl::begin_request`] 分配单调代次并记下模型身份,返回
//!   [`SearchTicket`]。装载与打分都只认这张票的模型身份,杜绝「用 A 的查询向量打 B 的
//!   向量空间」。代次分配与登记同临界区(代次大的必是后登记的),模型身份也在闸门内现读,
//!   见 [`SearchControl::begin_request_resolved`]。
//! - **不可变快照**:[`SearchControl::snapshot_for`] 在同一读锁内整体取用「身份 + 数据」
//!   快照(`Arc<EmbeddingCache>`),评分在锁外进行——失效不再需要等评分跑完,评分也不再
//!   可能被中途换掉的缓存串味。装载的「代次校验 + 安装」与失效的「代次 +1 + 清槽」共用
//!   同一把缓存锁,装载期间发生的失效(分析落库/清空/切模型)必被判出并丢弃重读;有界重试
//!   仍连续失效时**不驻留**(宁可不省这次全量读,也不让陈旧数据占住常驻位)。
//! - **线性化提交**:[`SearchControl::commit`] 在闸门内完成「检验仍是最新请求」与「调用方
//!   写库」,两步之间不存在 TOCTOU:旧请求要么在写库前被拒,要么它当时就是最新请求。
//! - **两段式清空**:[`SearchControl::begin_clear`] 在**入口**当场吊销在途请求并领取清空代次,
//!   [`SearchControl::clear_if_current`] 在工作段擦库,但只在期间没有更新的登记时。清空的
//!   意图(别再用旧结果了)不该排在磁盘动作后面,而磁盘动作也不该擦掉一个比它更新的请求刚写
//!   的行:清空 IPC 先到、工作线程因调度晚跑时,期间新登记的搜索已是「最新」,晚到的擦除须退化。
//!   提交/清空/切模型共用同一闸门,清空后旧提交不可能复活结果表。
//!
//! 破坏性重置(重启分析 / 重建嵌入)与切模型沿用同一姿态:吊销点紧贴破坏性状态变更本身
//! (重置嵌入、写入新模型配置)之后——变更之前登记的请求一律作废,之后登记的按新状态跑;
//! 擦库仍走 [`SearchControl::clear_if_current`] 的代次守卫,因此不存在「清理 worker 晚跑,
//! 把期间新搜索结果擦掉」的次序。
//!
//! 消费侧协议(前端 aiStore):[`SearchCommit::Superseded`] **不是**错误——`null` 应答不动
//! matchCount、不报搜索错误、不触发旧布局刷新;spinner 仍由**本地令牌持有者**在 finally
//! 收尾(后端侧吊销——重启/重建/切模型——不会给前端发新令牌,不在此收尾就会永远转)。
//! 空查询本地先递增令牌并清 loading/matchCount,再发后端清空命令。
//!
//! 锁序约定:控制面闸门(`gate`)→ `db_writer`。提交/清空闭包在闸门内取 `db_writer`,
//! 反序(先持 `db_writer` 再进闸门)会造成死锁,接线方须遵守;闸门内的闭包也不得回调
//! 本控制面的登记/清空方法(`std::sync::Mutex` 非重入)。

use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use tracing::{debug, info, warn};

use crate::ai::search::{score_top_k, EmbeddingCache};
use crate::error::{AppError, Result};

/// 装载重试上限:装载期间每遇一次失效就丢弃本次结果重读。
/// 有界是为了保活:分析流水线持续落库时失效会接连发生,无限重读会让搜索永不返回。
const MAX_LOAD_ATTEMPTS: usize = 3;

/// 请求身份:入口登记时分配的单调代次 + 产出查询向量的模型身份。
/// 代次决定「谁是最新请求」;模型身份决定查询向量属于哪个向量空间。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequestId {
    pub seq: u64,
    pub model: String,
}

/// 入口凭证:持有它才有资格提交结果,且只在其代次仍是最新时才能提交。
#[derive(Clone, Debug)]
pub struct SearchTicket(SearchRequestId);

impl SearchTicket {
    pub fn seq(&self) -> u64 {
        self.0.seq
    }

    pub fn model(&self) -> &str {
        &self.0.model
    }

    pub fn id(&self) -> &SearchRequestId {
        &self.0
    }
}

/// 提交结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchCommit {
    /// 已提交 N 行(N 可为 0:空库/无向量的合法空结果)。
    Committed(usize),
    /// 本请求已被更新的登记、清空或切模型取代,未写库。
    Superseded,
}

/// 清空/重置的登记凭证:[`SearchControl::begin_clear`] 当场吊销在途请求并领取清空代次,
/// [`SearchControl::clear_if_current`] 凭它擦库——只在期间没有更新的登记时才真擦。
///
/// 两段分开是为了让「别再用旧结果了」这个**意图**立刻生效,而磁盘动作可以晚一点;反过来,
/// 晚跑的擦除不能反过来擦掉期间新登记的搜索结果(那时它已不是最新意图)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearTicket {
    seq: u64,
}

impl ClearTicket {
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 提交闸门状态。
#[derive(Default)]
struct GateState {
    /// 请求代次源(单调)。与登记同临界区:代次大的必是后登记的。
    next_seq: u64,
    /// 最近登记的请求;`None` = 已被清空/切模型拦断,任何在途请求都不得提交。
    latest: Option<SearchRequestId>,
    /// 共享结果集(ai_search_results)当前属于哪个请求/模型。
    committed: Option<SearchRequestId>,
}

/// 常驻快照槽:代次与快照同锁读写。
///
/// 二者必须同锁:装载侧的「代次校验 + 安装」与失效侧的「代次 +1 + 清槽」若分属两把锁,
/// 失效就能插在校验与安装之间,让一个已经陈旧的候选重新驻留。
#[derive(Default)]
struct CacheSlot {
    /// 缓存代次:每次失效 +1;装载不得跨代次入驻。
    epoch: u64,
    /// 当前驻留快照(身份 + 数据,整体替换)。
    cache: Option<Arc<EmbeddingCache>>,
}

/// 语义搜索控制面:一 AppState 一份。
pub struct SearchControl {
    /// 登记/提交/清空的线性化闸门。
    gate: Mutex<GateState>,
    /// 装载单飞闸:同一时刻只允许一次磁盘装载;不阻塞命中快照的请求,也不阻塞失效。
    load_gate: Mutex<()>,
    /// 常驻快照槽(代次 + 快照)。
    cache: RwLock<CacheSlot>,
}

impl Default for SearchControl {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchControl {
    pub fn new() -> Self {
        Self {
            gate: Mutex::new(GateState::default()),
            load_gate: Mutex::new(()),
            cache: RwLock::new(CacheSlot::default()),
        }
    }

    /// 入口登记:分配代次并记下模型身份。此后本票就是「最新请求」,直至下一次登记/清空/切模型。
    ///
    /// 代次分配与登记同临界区是硬要求:先领号再登记时,「A 领到代次 1 后被抢占、B 领到 2 并
    /// 完成登记、A 再登记」会把 latest 退回代次 1——最新请求反被旧请求顶掉,旧结果就能覆写
    /// 新结果。这里不靠调度运气,代次大的必然是后登记的。
    pub fn begin_request(&self, model_name: &str) -> SearchTicket {
        self.begin_request_resolved(|| model_name.to_string())
    }

    /// 同 [`Self::begin_request`],但模型身份在闸门内现读。
    ///
    /// 切换模型时,「写入新模型配置」与「吊销在途请求」之间有一扇窗:若调用方在闸门外先读
    /// profile 再登记,拿到的可能已是旧模型,却能赶在吊销之后登记成功——旧向量空间的打分结果
    /// 便写回了新模型的结果集。把读 profile 放进闸门内,序列化后只剩两种次序:登记先发生
    /// (被随后的吊销作废),或吊销先发生(此时新配置必已写入,读到的是新模型)。
    pub fn begin_request_resolved(&self, resolve_model: impl FnOnce() -> String) -> SearchTicket {
        self.begin_request_with(|| (resolve_model(), ())).0
    }

    /// 同 [`Self::begin_request_resolved`],并允许调用方把「与身份同源的取值」一并算出来带回去
    /// (典型:同一份 profile 的 `embed_dim`)。取值与身份出自同一次解析,不会一个用新模型、
    /// 一个用旧模型的维度。
    pub fn begin_request_with<T>(
        &self,
        resolve: impl FnOnce() -> (String, T),
    ) -> (SearchTicket, T) {
        let mut gate = self.lock_gate_state();
        gate.next_seq += 1;
        let (model, payload) = resolve();
        let id = SearchRequestId {
            seq: gate.next_seq,
            model,
        };
        gate.latest = Some(id.clone());
        (SearchTicket(id), payload)
    }

    /// 本票是否仍是最新登记(用于在昂贵动作前提前退出)。
    pub fn is_current(&self, ticket: &SearchTicket) -> bool {
        self.lock_gate_state().latest.as_ref() == Some(ticket.id())
    }

    /// 取本票身份的不可变缓存快照;缺装载或身份不符时调用 `load`(全量读嵌入表)。
    pub fn snapshot_for(
        &self,
        ticket: &SearchTicket,
        dim: usize,
        load: impl FnMut(&str, usize) -> Result<EmbeddingCache>,
    ) -> Result<Arc<EmbeddingCache>> {
        self.load_snapshot_for(ticket, dim, load, || {})
    }

    /// 装载主体。`before_install` 是测试注入点:用来确定性卡住「代次校验之后、安装之前」
    /// 那扇窗(生产路径传空闭包),证明该窗口已被缓存锁吃掉,而不是靠时序碰运气。
    fn load_snapshot_for(
        &self,
        ticket: &SearchTicket,
        dim: usize,
        mut load: impl FnMut(&str, usize) -> Result<EmbeddingCache>,
        mut before_install: impl FnMut(),
    ) -> Result<Arc<EmbeddingCache>> {
        let model = ticket.model();

        // 命中:身份与数据在同一把读锁内整体取用,快照天然自洽(评分在锁外进行)。
        if let Some(resident) = self.resident(model, dim) {
            return Ok(resident);
        }

        // 装载单飞:并发同身份请求只读一次库;后到者复用先到者装入的快照。
        let _loading = self.load_gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(resident) = self.resident(model, dim) {
            return Ok(resident);
        }

        let mut last: Option<Arc<EmbeddingCache>> = None;
        for attempt in 1..=MAX_LOAD_ATTEMPTS {
            let epoch = self.cache_epoch();
            let t0 = std::time::Instant::now();
            let candidate = Arc::new(load(model, dim)?);

            before_install();

            match self.install_if_fresh(Arc::clone(&candidate), epoch) {
                Some(installed) => {
                    info!(
                        "Embedding cache loaded: {} vectors ({:.1} MB f16) in {:.0}ms | 嵌入缓存已载入",
                        installed.len(),
                        (installed.data.len() * 2) as f64 / (1024.0 * 1024.0),
                        t0.elapsed().as_secs_f64() * 1000.0
                    );
                    return Ok(installed);
                }
                None => {
                    warn!(
                        attempt,
                        "Embedding cache invalidated while loading — discarding and rereading | 装载期间缓存失效,丢弃本次装载重读"
                    );
                    last = Some(candidate);
                }
            }
        }

        // 连续失效(分析高频落库期):重试有界是为了保活。此时宁可不驻留——让下轮装载去拿
        // 最新数据,也不把可能陈旧的一份钉进常驻位。
        match last {
            Some(candidate) => {
                warn!(
                    "Embedding cache kept being invalidated during load — serving without residency | 连续失效,本次查询不驻留缓存"
                );
                Ok(candidate)
            }
            None => Err(AppError::Internal("嵌入缓存装载未产出快照".into())),
        }
    }

    /// 线性化提交:闸门内检验「仍是最新请求」并写库,旧请求直接判 [`SearchCommit::Superseded`]。
    pub fn commit(
        &self,
        ticket: &SearchTicket,
        write: impl FnOnce() -> Result<usize>,
    ) -> Result<SearchCommit> {
        let mut gate = self.lock_gate_state();
        if gate.latest.as_ref() != Some(ticket.id()) {
            return Ok(SearchCommit::Superseded);
        }
        // 检验与写库同临界区(不跨临界区重检,也就没有检验-写入之间的 TOCTOU):
        // 期间不会有新登记/清空/切模型插进来,旧结果不可能盖掉更新的。
        let written = write()?;
        gate.committed = Some(ticket.id().clone());
        Ok(SearchCommit::Committed(written))
    }

    /// 清空意图的**入口**半段:当场吊销全部在途请求并领取清空代次。
    ///
    /// 必须在 async 入口同步调用,不能拖到阻塞池的工作段——工作段的调度延迟会让「清空先到、
    /// 查询后到」被反转成「查询先提交、清空后擦掉它」。擦库交给
    /// [`Self::clear_if_current`],凭本票的代次在闸门内判定是否仍然作数。
    pub fn begin_clear(&self) -> ClearTicket {
        let mut gate = self.lock_gate_state();
        gate.next_seq += 1;
        gate.latest = None;
        ClearTicket { seq: gate.next_seq }
    }

    /// 清空的**工作**半段:仅在期间没有更新的登记时擦库,返回是否真的擦了。
    ///
    /// **意图优先**:吊销在入口就已生效,擦除失败也不恢复旧请求的提交资格——否则用户已按下的
    /// 清空会被一次写库失败悄悄撤回,旧请求又能把结果写回来。擦除失败返回错误、并如实保留
    /// `committed` 元信息(表里仍是那份未擦净的旧结果集,不谎报已清)。
    ///
    /// 期间若登记了新搜索(或又发生一次清空/切模型),表的状态归那次更新的意图管,本次擦除
    /// 直接退化为 `Ok(false)`:晚到的清理不得抹掉更新的结果。
    pub fn clear_if_current(
        &self,
        ticket: &ClearTicket,
        wipe: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        let mut gate = self.lock_gate_state();
        // next_seq 是唯一的事件序号:期间任何登记/清空/切模型都会把它推高。
        if gate.next_seq != ticket.seq {
            debug!(
                clear = ticket.seq,
                latest_event = gate.next_seq,
                "Clear superseded by a newer registration — skipping wipe | 清空已被更新的登记取代,跳过擦除"
            );
            return Ok(false);
        }
        let wiped = wipe();
        if wiped.is_ok() {
            // 只有确实擦净才清身份:留下它正是「表未擦净」的诚实表达。
            gate.committed = None;
        }
        wiped?;
        Ok(true)
    }

    /// 清空搜索(入口登记 + 工作段擦除,适用于无需分段的一次性调用)。
    pub fn clear(&self, wipe: impl FnOnce() -> Result<()>) -> Result<()> {
        let ticket = self.begin_clear();
        self.clear_if_current(&ticket, wipe).map(|_| ())
    }

    /// 切换激活模型:清空旧结果集 + 吊销在途旧模型请求 + 作废常驻缓存。
    ///
    /// **调用点必须紧跟在「新模型配置已写入」之后**(config 的 `set_and_persist` 之后):
    /// 吊销点之前登记的请求按旧模型解析,一律作废;之后登记的请求在闸门内读到的是新模型,
    /// 走新向量空间。这样就不存在「拿旧 profile 的迟到登记写回结果集」的窗口。
    ///
    /// 缓存作废与吊销不挂在擦除成败上:模型身份已变,旧的常驻快照与旧模型的在途请求都
    /// 不再可用——擦除失败只影响「旧结果集是否还躺在表里」,不影响这两件事必须发生。
    pub fn model_switched(&self, wipe: impl FnOnce() -> Result<()>) -> Result<()> {
        let cleared = self.clear(wipe);
        self.invalidate_cache();
        cleared
    }

    /// 嵌入向量写入/重建后作废常驻缓存。**不**改变请求代次:分析落库期的在途搜索仍可提交,
    /// 它的结果反映装载那一刻的库状态,属自洽的历史读而非越权覆写。
    ///
    /// 代次 +1 与清槽同锁,与 [`Self::install_if_fresh`] 的「校验 + 安装」互斥:失效要么
    /// 发生在安装之前(候选代次不符,不驻留),要么发生在安装之后(已驻留的那份被清掉,
    /// 下次装载重读)。不存在「校验通过 → 失效 → 陈旧候选重新驻留」的次序。
    pub fn invalidate_cache(&self) {
        let mut slot = self.lock_cache_slot();
        slot.epoch += 1;
        slot.cache = None;
    }

    /// 最近登记的请求身份。
    pub fn latest(&self) -> Option<SearchRequestId> {
        self.lock_gate_state().latest.clone()
    }

    /// 共享结果集当前归属的请求身份。
    pub fn committed(&self) -> Option<SearchRequestId> {
        self.lock_gate_state().committed.clone()
    }

    /// 布局发布与语义结果换代互斥，避免复核后被另一份搜索结果穿插。
    pub fn with_committed<T>(
        &self,
        expected: &Option<SearchRequestId>,
        publish: impl FnOnce() -> T,
    ) -> Option<T> {
        let gate = self.lock_gate_state();
        if &gate.committed != expected {
            return None;
        }
        Some(publish())
    }

    /// 常驻快照身份(模型, 维度);`None` = 未驻留。
    pub fn resident_identity(&self) -> Option<(String, usize)> {
        self.lock_cache_slot_read()
            .cache
            .as_ref()
            .map(|cache| (cache.model_name.clone(), cache.dim))
    }

    /// 毒锁恢复:控制面不因某次提交 panic 而永久失效(与 AI 命令族的 into_inner 契约一致)。
    fn lock_gate_state(&self) -> MutexGuard<'_, GateState> {
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_cache_slot(&self) -> RwLockWriteGuard<'_, CacheSlot> {
        self.cache.write().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_cache_slot_read(&self) -> RwLockReadGuard<'_, CacheSlot> {
        self.cache.read().unwrap_or_else(|e| e.into_inner())
    }

    /// 当前缓存代次(每次装载尝试的起点)。
    fn cache_epoch(&self) -> u64 {
        self.lock_cache_slot_read().epoch
    }

    /// 命中本身份的常驻快照(未驻留/身份不符均为 None)。
    fn resident(&self, model: &str, dim: usize) -> Option<Arc<EmbeddingCache>> {
        self.lock_cache_slot_read()
            .cache
            .as_ref()
            .filter(|cache| cache.matches(model, dim))
            .map(Arc::clone)
    }

    /// 校验代次并安装(同一把写锁内完成,与 [`Self::invalidate_cache`] 互斥)。
    /// 代次不符说明装载期间缓存已失效:候选作废不驻留,返回 `None` 让调用方重读。
    fn install_if_fresh(
        &self,
        candidate: Arc<EmbeddingCache>,
        epoch: u64,
    ) -> Option<Arc<EmbeddingCache>> {
        let mut slot = self.lock_cache_slot();
        if slot.epoch != epoch {
            return None;
        }
        match slot.cache.as_ref() {
            Some(current) if current.matches(&candidate.model_name, candidate.dim) => {
                Some(Arc::clone(current))
            }
            _ => {
                slot.cache = Some(Arc::clone(&candidate));
                Some(candidate)
            }
        }
    }
}

/// 内核主路径:登记票 → 不可变快照 → 评分 → 线性化提交。
/// `load` 只在常驻快照缺失/身份不符时调用;`write` 在提交闸门内执行(接线方应在此完成
/// ai_search_results 整表事务与结果集身份戳)。
pub fn run_search(
    control: &SearchControl,
    ticket: &SearchTicket,
    query_vec: &[f32],
    top_k: usize,
    dim: usize,
    load: impl FnMut(&str, usize) -> Result<EmbeddingCache>,
    write: impl FnOnce(&[(i64, f32)]) -> Result<usize>,
) -> Result<SearchCommit> {
    // 已经过期就不再付全量装载/打分的账(嵌入全量读以 GB 计)。
    if !control.is_current(ticket) {
        return Ok(SearchCommit::Superseded);
    }
    let snapshot = control.snapshot_for(ticket, dim, load)?;
    let scored = score_top_k(&snapshot, query_vec, top_k)?;
    debug!(
        "Semantic search scored {} rows (model {}) | 已打分 {} 条",
        scored.len(),
        ticket.model(),
        scored.len()
    );
    control.commit(ticket, || write(&scored))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use super::*;

    const DIM: usize = 2;

    fn blob(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// 构造装载完成的快照，身份由控制面传入，转换使用生产的逐行打包逻辑。
    fn rows(model: &str, dim: usize, entries: &[(i64, &[f32])]) -> EmbeddingCache {
        EmbeddingCache::pack(
            model,
            dim,
            entries.iter().map(|(id, v)| (*id, blob(v))).collect(),
        )
    }

    /// 「结果表」替身:记录每次写库的调用者标签与内容。
    #[derive(Default)]
    struct FakeTable {
        content: Mutex<Vec<(i64, f32)>>,
        writers: Mutex<Vec<i64>>,
    }

    impl FakeTable {
        /// 写库闭包:把打分结果写进替身表。
        fn writer(&self, tag: i64) -> impl FnOnce(&[(i64, f32)]) -> Result<usize> + '_ {
            move |scored| {
                self.writers.lock().unwrap().push(tag);
                *self.content.lock().unwrap() = scored.to_vec();
                Ok(scored.len())
            }
        }

        /// 直接提交(不经打分)用的写库闭包:把标签记进写库流水并写成单行结果。
        fn direct(&self, tag: i64) -> impl FnOnce() -> Result<usize> + '_ {
            move || {
                self.writers.lock().unwrap().push(tag);
                *self.content.lock().unwrap() = vec![(tag, 0.5)];
                Ok(1)
            }
        }

        fn wipe(&self) -> impl FnOnce() -> Result<()> + '_ {
            move || {
                self.content.lock().unwrap().clear();
                Ok(())
            }
        }

        fn content(&self) -> Vec<(i64, f32)> {
            self.content.lock().unwrap().clone()
        }

        fn writer_tags(&self) -> Vec<i64> {
            self.writers.lock().unwrap().clone()
        }
    }

    /// 受控反序 A/B:A 先登记拿到快照(在途),B 后登记却先完成提交;A 迟到提交必须被拒——
    /// 写库闭包一次都不能跑,结果表保持 B 的内容(不是「写了再被覆盖」)。
    #[test]
    fn in_flight_older_request_cannot_overwrite_newer_commit() {
        let control = SearchControl::new();
        let table_b = FakeTable::default();
        let table_a = FakeTable::default();

        let ticket_a = control.begin_request("m1");
        let snapshot_a = control
            .snapshot_for(&ticket_a, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();

        let ticket_b = control.begin_request("m1");
        let outcome_b = run_search(
            &control,
            &ticket_b,
            &[0.0, 1.0],
            10,
            DIM,
            |model, dim| Ok(rows(model, dim, &[(2, &[0.0, 1.0])])),
            table_b.writer(2),
        )
        .unwrap();
        assert_eq!(outcome_b, SearchCommit::Committed(1));

        // A 用自己那份快照照常打分,但提交点已被 B 取代。
        assert_eq!(score_top_k(&snapshot_a, &[1.0, 0.0], 10).unwrap()[0].0, 1);
        let outcome_a = control.commit(&ticket_a, table_a.direct(1)).unwrap();
        assert_eq!(outcome_a, SearchCommit::Superseded);
        assert!(table_a.writer_tags().is_empty(), "被取代的请求不得写库");
        assert_eq!(table_b.content().len(), 1);
        assert_eq!(control.committed().unwrap().seq, ticket_b.seq());
    }

    /// 清空后旧提交:清空吊销在途请求,迟到的旧提交不得复活结果表;之后新请求照常。
    #[test]
    fn clear_revokes_in_flight_request_and_late_commit_cannot_resurrect() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let seeded = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &seeded,
                &[1.0, 0.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0])])),
                table.writer(1),
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );

        // 用户又发了一次查询(在途),随后清空搜索。
        let in_flight = control.begin_request("m1");
        let _snapshot = control
            .snapshot_for(&in_flight, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        control.clear(table.wipe()).unwrap();
        assert!(table.content().is_empty(), "清空须擦除结果表");
        assert!(control.committed().is_none(), "清空后不得留结果集身份");
        assert!(control.latest().is_none(), "清空须吊销在途请求");

        let outcome = control.commit(&in_flight, table.direct(9)).unwrap();
        assert_eq!(outcome, SearchCommit::Superseded);
        assert!(table.content().is_empty(), "清空后不得被迟到提交复活");

        // 清空后新登记的请求正常走完。
        control.invalidate_cache();
        let fresh = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &fresh,
                &[1.0, 0.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(5, &[1.0, 0.0])])),
                table.writer(10),
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );
        assert_eq!(table.content()[0].0, 5);
        assert_eq!(control.committed().unwrap().seq, fresh.seq());
    }

    /// 同维不同模型:身份判定必须按 (模型, 维度) 整体比较——
    /// 只比模型名会让 m2 的查询向量打在 m1 的向量空间上(静默算错)。
    #[test]
    fn snapshot_never_crosses_model_identity_at_equal_dim() {
        let control = SearchControl::new();

        let m1 = control.begin_request("m1");
        control
            .snapshot_for(&m1, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();

        let loads = AtomicUsize::new(0);
        let m2 = control.begin_request("m2");
        let snapshot = control
            .snapshot_for(&m2, DIM, |model, dim| {
                loads.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(rows(model, dim, &[(2, &[0.0, 1.0])]))
            })
            .unwrap();
        assert_eq!(loads.load(AtomicOrdering::SeqCst), 1, "换模型须重新装载");
        assert_eq!(snapshot.model_name, "m2");
        assert_eq!(snapshot.ids, vec![2]);
        assert_eq!(score_top_k(&snapshot, &[0.0, 1.0], 1).unwrap()[0].0, 2);
    }
}
