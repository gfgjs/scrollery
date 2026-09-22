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
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::Duration;

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

    /// 生产接线的测试替身:真 SQLite 内存库(含 ai_search_results 真实 DDL 与外键)+ 控制面。
    /// 结果表不再是 FakeTable——擦除/替换走 [`crate::ai::search`] 的真实落库函数,
    /// 好让「清空工作段晚跑」这类次序问题在真实读写路径上暴露。
    /// 结果表的 file_id 有外键指向 media_items,故先播 1..=9 号最小媒体项(测试用到的 id)。
    fn wired_control() -> (SearchControl, rusqlite::Connection) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth)
                 VALUES (10, 1, NULL, 'A', 'A', 0);",
        )
        .unwrap();
        for id in 1..=9i64 {
            conn.execute(
                "INSERT INTO media_items
                    (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                     width, height, sort_datetime, cache_key, is_favorited, is_deleted,
                     is_live_photo, companion_of)
                 VALUES (?1, 10, ?2, 0, 0, 'jpg', 'image', 0, 0, 0, 0, 0, 0, 0, NULL)",
                rusqlite::params![id, format!("item{id}.jpg")],
            )
            .unwrap();
        }
        (SearchControl::new(), conn)
    }

    /// 结果表当前内容(按相似度降序,便于断言「谁的结果在表里」)。
    fn search_result_ids(conn: &rusqlite::Connection) -> Vec<i64> {
        let mut stmt = conn
            .prepare("SELECT file_id FROM ai_search_results ORDER BY similarity DESC, file_id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        ids
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

        /// 注入擦除失败:用于验证「清空/切模型的意图不挂在擦库成败上」。
        fn failing_wipe() -> impl FnOnce() -> Result<()> {
            || Err(AppError::Internal("wipe failed(测试注入)".into()))
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

    /// 顺序场景:先到的请求提交成功后,后来者仍可正常提交并覆写(不是「只有第一个能提交」)。
    #[test]
    fn sequential_requests_commit_in_registration_order() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let first = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &first,
                &[1.0, 0.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0])])),
                table.writer(1),
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );

        let second = control.begin_request("m1");
        // 模拟分析流水线又写入了一条向量:常驻缓存作废,第二次查询重新装载。
        control.invalidate_cache();
        assert_eq!(
            run_search(
                &control,
                &second,
                &[0.0, 1.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0]), (2, &[0.0, 1.0])])),
                table.writer(2),
            )
            .unwrap(),
            SearchCommit::Committed(2)
        );
        assert_eq!(table.writer_tags(), vec![1, 2]);
        assert_eq!(table.content()[0].0, 2);
        assert_eq!(control.committed().unwrap().seq, second.seq());
    }

    #[test]
    fn layout_publication_requires_the_same_committed_search() {
        let control = SearchControl::new();
        let first = control.begin_request("m1");
        control.commit(&first, || Ok(1)).unwrap();
        let source = control.committed();
        assert_eq!(control.with_committed(&source, || 7), Some(7));
        let second = control.begin_request("m1");
        control.commit(&second, || Ok(1)).unwrap();
        assert!(control
            .with_committed(&source, || panic!("旧语义候选不得发布"))
            .is_none());
        let source = control.committed();
        control.clear(|| Ok(())).unwrap();
        assert!(control.with_committed(&source, || ()).is_none());
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

    /// 生产接线反例(真 SQLite 结果表 + 入口/工作段两段式):
    /// A 搜索在途 → 清空在 async 入口登记 → B 搜索登记并提交 → 清空的工作段**最后**才跑。
    /// 晚到的擦除不得抹掉 B 的结果;B 之前登记的 A 则已被入口吊销,提交不出去。
    #[test]
    fn late_clear_worker_does_not_wipe_a_newer_search() {
        let (control, conn) = wired_control();
        let conn = Arc::new(Mutex::new(conn));

        // A:在清空之前登记并装载快照(在途)。同库同模型,故快照含全部向量,供 B 复用。
        let ticket_a = control.begin_request("m1");
        control
            .snapshot_for(&ticket_a, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0]), (2, &[0.0, 1.0])]))
            })
            .unwrap();

        // 清空命令到达:入口当场登记清空代次(吊销 A)。工作段被调度推迟。
        let clear_ticket = control.begin_clear();

        // B:清空之后登记的搜索,正常装载/打分/提交。
        let ticket_b = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &ticket_b,
                &[0.0, 1.0],
                10,
                DIM,
                |_, _| unreachable!("同身份快照已常驻,不得重新装载"),
                |scored| {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, scored)
                },
            )
            .unwrap(),
            SearchCommit::Committed(2)
        );

        // 清空的工作段此刻才跑:期间已有更新登记 ⇒ 退化,不擦库。
        let wiped = control
            .clear_if_current(&clear_ticket, || {
                crate::ai::search::wipe_search_results(&conn.lock().unwrap())
            })
            .unwrap();
        assert!(!wiped, "被更新登记取代的清空不得擦库");
        assert_eq!(
            search_result_ids(&conn.lock().unwrap()),
            vec![2, 1],
            "B 的结果必须留在结果表里"
        );

        // A 的迟到提交:入口吊销已生效,写不进去。
        assert_eq!(
            control
                .commit(&ticket_a, || {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, &[(1, 1.0)])
                })
                .unwrap(),
            SearchCommit::Superseded
        );
        assert_eq!(
            search_result_ids(&conn.lock().unwrap()),
            vec![2, 1],
            "被吊销的迟到提交不得改动 B 的结果"
        );
    }

    /// 生产接线正例:清空之后没有新登记,工作段照常擦净并清身份;
    /// 且清空之前登记的请求永远提交不进来。
    #[test]
    fn clear_worker_wipes_when_no_newer_registration_arrived() {
        let (control, conn) = wired_control();
        let conn = Arc::new(Mutex::new(conn));

        // 先有一份已提交的旧结果。
        let seeded = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &seeded,
                &[1.0, 0.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0])])),
                |scored| {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, scored)
                },
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );
        assert_eq!(search_result_ids(&conn.lock().unwrap()), vec![1]);

        // 用户清空 → 工作段跑(期间无新登记)。
        let in_flight = control.begin_request("m1");
        let clear_ticket = control.begin_clear();
        assert!(
            control
                .clear_if_current(&clear_ticket, || {
                    crate::ai::search::wipe_search_results(&conn.lock().unwrap())
                })
                .unwrap(),
            "无更新登记时清空须真擦"
        );
        assert!(search_result_ids(&conn.lock().unwrap()).is_empty());
        assert!(control.committed().is_none(), "擦净后结果集身份随之清掉");

        // 清空前登记的请求:提交被拒,结果表保持空。
        assert_eq!(
            control
                .commit(&in_flight, || {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, &[(9, 1.0)])
                })
                .unwrap(),
            SearchCommit::Superseded
        );
        assert!(search_result_ids(&conn.lock().unwrap()).is_empty());
    }

    /// 破坏性重置(重启分析/重建嵌入)的次序:重置嵌入 → 吊销 → 代次守卫的擦库。
    /// 重置前登记的请求即使已写出结果,也会被这次清理抹掉(它的向量空间已经不存在);
    /// 重置后登记的请求则扛得住晚跑的清空 worker。
    #[test]
    fn destructive_reset_revokes_earlier_requests_and_late_worker_cannot_wipe_newer_ones() {
        let (control, conn) = wired_control();
        let conn = Arc::new(Mutex::new(conn));

        let stale = control.begin_request("m1");
        let _snapshot = control
            .snapshot_for(&stale, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        assert_eq!(
            control
                .commit(&stale, || {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, &[(1, 1.0)])
                })
                .unwrap(),
            SearchCommit::Committed(1)
        );

        // 重置段:嵌入已清空后当场吊销 + 作废快照(与 reset_ai_embeddings 同段)。
        let reset_ticket = control.begin_clear();
        control.invalidate_cache();
        assert!(
            control.resident_identity().is_none(),
            "重置须立刻作废常驻快照(不等新查询)"
        );

        // 重置后登记的新搜索提交成功。
        let fresh = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &fresh,
                &[0.0, 1.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(2, &[0.0, 1.0])])),
                |scored| {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, scored)
                },
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );

        // 清理 worker 晚跑:新搜索的结果不受影响。
        assert!(!control
            .clear_if_current(&reset_ticket, || {
                crate::ai::search::wipe_search_results(&conn.lock().unwrap())
            })
            .unwrap());
        assert_eq!(search_result_ids(&conn.lock().unwrap()), vec![2]);
    }

    /// 工作线程受控延迟的接线反例(真线程,复刻 spawn_blocking 的调度反转):
    /// 清空入口已登记,其工作段卡在 barrier 上;期间新搜索完成提交;放行后才执行擦库。
    #[test]
    fn delayed_clear_worker_thread_skips_wipe_after_newer_commit() {
        use std::sync::Barrier;

        let (control, conn) = wired_control();
        let control = Arc::new(control);
        let conn = Arc::new(Mutex::new(conn));

        // 清空入口(主线程,等价 async 入口)。
        let clear_ticket = control.begin_clear();

        let gate = Arc::new(Barrier::new(2));
        let worker = {
            let control = Arc::clone(&control);
            let conn = Arc::clone(&conn);
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                gate.wait(); // 卡住:模拟阻塞池排队
                gate.wait(); // 等主线程把新搜索提交完
                control
                    .clear_if_current(&clear_ticket, || {
                        crate::ai::search::wipe_search_results(&conn.lock().unwrap())
                    })
                    .unwrap()
            })
        };

        gate.wait(); // 工作段就位(但还没跑擦库)
        let ticket = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &ticket,
                &[0.0, 1.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(7, &[0.0, 1.0])])),
                |scored| {
                    let mut c = conn.lock().unwrap();
                    crate::ai::search::replace_search_results(&mut c, scored)
                },
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );
        gate.wait(); // 放行工作段

        assert!(!worker.join().unwrap(), "晚到的清空工作段须退化");
        assert_eq!(
            search_result_ids(&conn.lock().unwrap()),
            vec![7],
            "新搜索的结果不得被晚到的清空擦掉"
        );
    }
    /// 而重获写库资格;同时如实保留结果集身份(表里确实还是旧结果)。
    #[test]
    fn clear_revokes_in_flight_request_even_when_wipe_fails() {
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
        let seeded_id = control.committed().unwrap();

        // 在途请求 + 清空失败。
        let in_flight = control.begin_request("m1");
        control
            .snapshot_for(&in_flight, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        let err = control.clear(FakeTable::failing_wipe()).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)), "擦除失败须如实上报");

        assert!(control.latest().is_none(), "清空意图须立即吊销在途请求");
        assert_eq!(
            control.commit(&in_flight, table.direct(9)).unwrap(),
            SearchCommit::Superseded,
            "擦除失败不得恢复旧请求的提交资格"
        );
        assert_eq!(table.writer_tags(), vec![1], "被撤销请求的写库闭包不得执行");
        assert_eq!(
            control.committed().unwrap(),
            seeded_id,
            "表未擦净,结果集身份如实保留而非谎报已清"
        );

        // 擦除成功后身份元信息随之清掉。
        control.clear(table.wipe()).unwrap();
        assert!(control.committed().is_none());
    }

    /// 切模型擦除失败:缓存作废与在途吊销照常发生(模型身份已变),不受擦库结果影响。
    #[test]
    fn model_switch_invalidates_cache_and_revokes_even_when_wipe_fails() {
        let control = SearchControl::new();

        let warm = control.begin_request("m1");
        control
            .snapshot_for(&warm, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        assert!(control.resident_identity().is_some(), "前置:常驻缓存已装载");

        let in_flight = control.begin_request("m1");
        let err = control
            .model_switched(FakeTable::failing_wipe())
            .unwrap_err();
        assert!(matches!(err, AppError::Internal(_)), "擦除失败须如实上报");

        assert!(
            control.resident_identity().is_none(),
            "切模型须作废常驻缓存,不因擦除失败而保留"
        );
        assert_eq!(
            control.commit(&in_flight, || Ok(1)).unwrap(),
            SearchCommit::Superseded,
            "旧模型在途请求不得因擦除失败而保留提交资格"
        );
        assert!(control.latest().is_none());
    }

    /// 切模型:作废常驻缓存 + 吊销在途旧模型请求;新模型请求不得复用旧模型快照。
    #[test]
    fn model_switch_drops_cache_and_rejects_old_model_request() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let warm = control.begin_request("m1");
        control
            .snapshot_for(&warm, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        assert_eq!(control.resident_identity(), Some(("m1".to_string(), DIM)));

        let in_flight = control.begin_request("m1");
        control.model_switched(table.wipe()).unwrap();
        assert!(
            control.resident_identity().is_none(),
            "切模型须作废常驻缓存"
        );
        assert_eq!(
            control.commit(&in_flight, table.direct(9)).unwrap(),
            SearchCommit::Superseded,
            "旧模型在途请求不得提交"
        );

        let loads = AtomicUsize::new(0);
        let ticket = control.begin_request("m2");
        let snapshot = control
            .snapshot_for(&ticket, DIM, |model, dim| {
                loads.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(rows(model, dim, &[(7, &[0.0, 1.0])]))
            })
            .unwrap();
        assert_eq!(loads.load(AtomicOrdering::SeqCst), 1, "新模型须自行装载");
        assert_eq!(snapshot.model_name, "m2");
        assert_eq!(snapshot.ids, vec![7]);
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

    /// 维度变化(同模型):快照身份按维度重判,不得把旧维度数据当新维度用。
    #[test]
    fn snapshot_reloads_when_dim_changes() {
        let control = SearchControl::new();

        let four = control.begin_request("m1");
        let wide = control
            .snapshot_for(&four, 4, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0, 0.0, 0.0])]))
            })
            .unwrap();
        assert_eq!(wide.dim, 4);

        let two = control.begin_request("m1");
        let narrow = control
            .snapshot_for(&two, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();
        assert_eq!(narrow.dim, DIM);
        assert_eq!(narrow.ids, vec![1]);
    }

    /// model/dim 错配:查询向量长度与快照维度不符时报 Err(而不是按快照维度索引 panic),
    /// 且不写库;同维度对照用例证明该拒绝不是「什么都拒」。
    #[test]
    fn query_dim_mismatch_is_rejected_not_panicking() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let ok = control.begin_request("m1");
        assert_eq!(
            run_search(
                &control,
                &ok,
                &[1.0, 0.0],
                10,
                DIM,
                |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0])])),
                table.writer(1),
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );

        let mismatched = control.begin_request("m1");
        let err = run_search(
            &control,
            &mismatched,
            &[1.0, 0.0, 0.0],
            10,
            DIM,
            |model, dim| Ok(rows(model, dim, &[(1, &[1.0, 0.0])])),
            table.writer(2),
        )
        .unwrap_err();
        assert!(
            matches!(err, AppError::Internal(_)),
            "查询向量错维应是内部错误,实际: {err:?}"
        );
        assert_eq!(table.writer_tags(), vec![1], "错维请求不得写库");
    }

    /// 嵌入写入失效只作废缓存:在途搜索仍可提交(它的结果反映装载时的库状态),
    /// 但下一次请求必须重新装载。
    #[test]
    fn embedding_write_invalidation_keeps_in_flight_commit_eligible() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let in_flight = control.begin_request("m1");
        control
            .snapshot_for(&in_flight, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();

        control.invalidate_cache();
        assert!(control.resident_identity().is_none());
        assert_eq!(
            control.commit(&in_flight, table.direct(1)).unwrap(),
            SearchCommit::Committed(1)
        );

        let loads = AtomicUsize::new(0);
        let next = control.begin_request("m1");
        let snapshot = control
            .snapshot_for(&next, DIM, |model, dim| {
                loads.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(rows(model, dim, &[(1, &[1.0, 0.0]), (2, &[0.0, 1.0])]))
            })
            .unwrap();
        assert_eq!(loads.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(snapshot.ids.len(), 2);
    }

    /// 受控 barrier:装载进行中发生失效(分析落库)→ 本次装载作废重读,对外快照与常驻
    /// 快照都必须是重读后的新数据。
    #[test]
    fn invalidation_during_load_discards_stale_snapshot() {
        let control = Arc::new(SearchControl::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();

        let worker = {
            let control = Arc::clone(&control);
            let loads = Arc::clone(&loads);
            std::thread::spawn(move || {
                let ticket = control.begin_request("m1");
                control
                    .snapshot_for(&ticket, DIM, |model, dim| {
                        let attempt = loads.fetch_add(1, AtomicOrdering::SeqCst);
                        if attempt == 0 {
                            entered_tx.send(()).unwrap();
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("装载未被放行");
                            Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
                        } else {
                            Ok(rows(model, dim, &[(1, &[1.0, 0.0]), (2, &[0.0, 1.0])]))
                        }
                    })
                    .map(|snapshot| snapshot.ids.clone())
            })
        };

        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("装载未开始");
        control.invalidate_cache();
        release_tx.send(()).unwrap();

        let ids = worker.join().unwrap().unwrap();
        assert_eq!(
            loads.load(AtomicOrdering::SeqCst),
            2,
            "装载期间失效须丢弃本次结果重读"
        );
        assert_eq!(ids, vec![1, 2], "对外快照应是重读后的新数据");

        let resident = control
            .snapshot_for(&control.begin_request("m1"), DIM, |_, _| {
                unreachable!("常驻快照应命中,不得再次装载")
            })
            .unwrap();
        assert_eq!(resident.ids, vec![1, 2], "驻留的必须是重读后的新数据");
    }

    /// 受控反序(安装窗口):卡在「代次校验之后、安装之前」失效——候选代次已过期,
    /// 不得重新驻留,更不得作为本次查询结果返回;重读后的新数据才是唯一出口。
    ///
    /// 与上一个用例的分工:那个把失效卡在 load 过程中(读库期间),这个卡在 load 返回、
    /// 即将安装的那一瞬——旧实现(校验在锁外、安装另取锁)恰在此处漏掉失效。
    #[test]
    fn invalidation_between_epoch_check_and_install_never_resurrects_stale_snapshot() {
        let control = Arc::new(SearchControl::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let ticket = control.begin_request("m1");

        let hook_control = Arc::clone(&control);
        let hook_fired = AtomicUsize::new(0);
        let load_attempts = Arc::clone(&loads);

        let snapshot = control
            .load_snapshot_for(
                &ticket,
                DIM,
                move |model, dim| {
                    let attempt = load_attempts.fetch_add(1, AtomicOrdering::SeqCst);
                    if attempt == 0 {
                        Ok(rows(model, dim, &[(1, &[1.0, 0.0])])) // 陈旧候选:失效前的库状态
                    } else {
                        Ok(rows(model, dim, &[(1, &[1.0, 0.0]), (2, &[0.0, 1.0])]))
                        // 失效后重读
                    }
                },
                move || {
                    // 只在第一次安装前失效:精确落在「校验已通过、安装尚未发生」的窗口。
                    if hook_fired.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                        hook_control.invalidate_cache();
                    }
                },
            )
            .unwrap();

        assert_eq!(
            loads.load(AtomicOrdering::SeqCst),
            2,
            "安装前的失效须被判定并重读"
        );
        assert_eq!(
            snapshot.ids,
            vec![1, 2],
            "返回的必须是重读后的新快照,不是那个已过代的候选"
        );
        assert_eq!(
            control.resident_identity(),
            Some(("m1".to_string(), DIM)),
            "常驻的必须是重读后的新快照"
        );
        let resident = control
            .snapshot_for(&control.begin_request("m1"), DIM, |_, _| {
                unreachable!("常驻快照应命中,不得再次装载")
            })
            .unwrap();
        assert_eq!(resident.ids, vec![1, 2], "陈旧候选不得回到常驻位");
    }

    /// 连续失效(分析高频落库期):重试有界,不驻留可能陈旧的数据,本次查询仍拿到最新一次装载。
    #[test]
    fn constant_invalidation_never_installs_stale_snapshot() {
        let control = Arc::new(SearchControl::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let ticket = control.begin_request("m1");

        let snapshot = {
            let outer = Arc::clone(&control);
            let inner = Arc::clone(&control);
            let loads = Arc::clone(&loads);
            outer
                .snapshot_for(&ticket, DIM, move |model, dim| {
                    loads.fetch_add(1, AtomicOrdering::SeqCst);
                    inner.invalidate_cache();
                    Ok(rows(model, dim, &[(3, &[1.0, 0.0])]))
                })
                .unwrap()
        };

        assert_eq!(
            loads.load(AtomicOrdering::SeqCst),
            MAX_LOAD_ATTEMPTS,
            "重试须有界"
        );
        assert!(
            control.resident_identity().is_none(),
            "连续失效期间不得驻留可能陈旧的数据"
        );
        assert_eq!(snapshot.ids, vec![3]);
    }

    /// 并发登记:代次分配与登记同临界区,因此「代次最大的那张票」恒等于最后登记的请求。
    /// 旧形态(先原子领号、再进闸门登记)会在这里露馅:领了大号的线程被抢占后晚登记,
    /// latest 便退回小号,最新请求反被旧请求顶掉。
    #[test]
    fn registration_sequence_matches_registration_order_under_contention() {
        const THREADS: usize = 16;
        let control = Arc::new(SearchControl::new());
        let barrier = Arc::new(Barrier::new(THREADS));

        let mut handles = Vec::new();
        for _ in 0..THREADS {
            let control = Arc::clone(&control);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                control.begin_request("m1").seq()
            }));
        }

        let seqs: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let max_seq = *seqs.iter().max().unwrap();

        let mut unique = seqs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), THREADS, "代次不得重复: {seqs:?}");
        assert_eq!(
            unique,
            (1..=THREADS as u64).collect::<Vec<_>>(),
            "代次应逐个占满,不留空洞"
        );
        assert_eq!(
            control.latest().unwrap().seq,
            max_seq,
            "最后登记的必是代次最大的: {seqs:?}"
        );
    }

    /// 切模型期间的迟到登记:身份在闸门内现读,所以吊销先发生的那次登记拿到的是新模型,
    /// 按新模型装载与提交;而拿旧模型登记的票(吊销前已登记)一律作废,写不回结果集。
    #[test]
    fn registration_after_model_switch_resolves_new_model_inside_the_gate() {
        let control = SearchControl::new();
        let table = FakeTable::default();
        let active = Mutex::new("m1".to_string());

        // 旧模型的在途请求(切模型前登记)。
        let stale = control.begin_request(&active.lock().unwrap().clone());
        control
            .snapshot_for(&stale, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();

        // 切模型:配置先写(生产序),随后吊销在途请求并作废缓存。
        *active.lock().unwrap() = "m2".to_string();
        control.model_switched(table.wipe()).unwrap();
        assert_eq!(
            control.commit(&stale, table.direct(1)).unwrap(),
            SearchCommit::Superseded,
            "旧模型的在途请求不得写回结果集"
        );

        // 迟到的登记读的是闸门内的当前身份 = 新模型,装载/打分都按新向量空间走。
        let loads = AtomicUsize::new(0);
        let ticket = control.begin_request_resolved(|| active.lock().unwrap().clone());
        assert_eq!(ticket.model(), "m2", "身份须现读,不得沿用切模型前的取值");
        let snapshot = control
            .snapshot_for(&ticket, DIM, |model, dim| {
                loads.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(rows(model, dim, &[(9, &[0.0, 1.0])]))
            })
            .unwrap();
        assert_eq!(snapshot.model_name, "m2");
        assert_eq!(loads.load(AtomicOrdering::SeqCst), 1, "须按新模型重新装载");
        assert_eq!(
            run_search(
                &control,
                &ticket,
                &[0.0, 1.0],
                10,
                DIM,
                |_, _| unreachable!("快照已在常驻位"),
                table.writer(9),
            )
            .unwrap(),
            SearchCommit::Committed(1)
        );
        assert_eq!(table.content()[0].0, 9);
        assert_eq!(control.committed().unwrap().model, "m2");
    }

    /// 身份解析在闸门内完成:解析尚未返回时,并发吊销必须排队——于是「登记先、吊销后」
    /// 的次序恒成立,该票必被吊销,拿旧身份的票也无从在吊销之后登记。
    #[test]
    fn model_resolution_is_serialized_with_revocation() {
        let control = Arc::new(SearchControl::new());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();

        let registrar = {
            let control = Arc::clone(&control);
            std::thread::spawn(move || {
                control.begin_request_resolved(|| {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("身份解析未被放行");
                    "m2".to_string()
                })
            })
        };
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("解析未开始");

        // 解析卡在闸门内,吊销只能排队等登记走完。
        let revoking = {
            let control = Arc::clone(&control);
            std::thread::spawn(move || control.model_switched(|| Ok(())).unwrap())
        };
        // 给吊销一个「抢先完成」的机会:若解析在闸门外,这里它就能抢在登记前面。
        std::thread::sleep(Duration::from_millis(50));
        release_tx.send(()).unwrap();

        let ticket = registrar.join().unwrap();
        revoking.join().unwrap();
        assert_eq!(
            control.commit(&ticket, || Ok(1)).unwrap(),
            SearchCommit::Superseded,
            "登记先于吊销 ⇒ 该票必被吊销,不得成为可提交的 latest"
        );
        assert!(control.latest().is_none());
    }

    /// 已过期的请求:既不装载也不写库(全量读以 GB 计,过期后不该再付这笔账)。
    #[test]
    fn superseded_request_skips_load_and_write() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let stale = control.begin_request("m1");
        let _newer = control.begin_request("m1");

        let outcome = run_search(
            &control,
            &stale,
            &[1.0, 0.0],
            10,
            DIM,
            |_, _| unreachable!("已过期请求不得再读全量嵌入"),
            table.writer(1),
        )
        .unwrap();
        assert_eq!(outcome, SearchCommit::Superseded);
        assert!(table.writer_tags().is_empty());
    }

    /// 空库:空快照是合法的空结果集提交(Committed(0)),同样要过闸门而不是绕开。
    #[test]
    fn empty_snapshot_commits_zero_rows() {
        let control = SearchControl::new();
        let table = FakeTable::default();

        let ticket = control.begin_request("m1");
        let outcome = run_search(
            &control,
            &ticket,
            &[1.0, 0.0],
            10,
            DIM,
            |model, dim| Ok(rows(model, dim, &[])),
            table.writer(1),
        )
        .unwrap();
        assert_eq!(outcome, SearchCommit::Committed(0));
        assert_eq!(table.writer_tags(), vec![1]);
    }

    /// 并发登记/提交:提交序按登记代次线性化——最终写库行恒来自代次最大的成功提交者,
    /// 且代次最大的登记者必然成功(不被更早的提交挡掉)。
    #[test]
    fn concurrent_commits_keep_the_newest_successful_write() {
        let control = Arc::new(SearchControl::new());
        let table = Arc::new(FakeTable::default());

        let warm = control.begin_request("m1");
        control
            .snapshot_for(&warm, DIM, |model, dim| {
                Ok(rows(model, dim, &[(1, &[1.0, 0.0])]))
            })
            .unwrap();

        let mut handles = Vec::new();
        for _ in 0..8 {
            let control = Arc::clone(&control);
            let table = Arc::clone(&table);
            handles.push(std::thread::spawn(move || {
                let ticket = control.begin_request("m1");
                let outcome = control
                    .commit(&ticket, table.direct(ticket.seq() as i64))
                    .unwrap();
                (ticket.seq(), outcome)
            }));
        }

        let mut committed = Vec::new();
        let mut all_seqs = Vec::new();
        for handle in handles {
            let (seq, outcome) = handle.join().unwrap();
            all_seqs.push(seq);
            if matches!(outcome, SearchCommit::Committed(_)) {
                committed.push(seq);
            }
        }

        assert!(!committed.is_empty(), "至少有一个请求应当提交成功");
        let max_seq = *all_seqs.iter().max().unwrap();
        assert!(
            committed.contains(&max_seq),
            "代次最大的登记者必然成功,committed={committed:?}"
        );
        let max_committed = *committed.iter().max().unwrap();
        let content = table.content();
        assert_eq!(content.len(), 1);
        assert_eq!(
            content[0].0, max_committed as i64,
            "最终写库行必须来自代次最大的成功提交者"
        );
        assert_eq!(control.committed().unwrap().seq, max_committed);
    }
}
