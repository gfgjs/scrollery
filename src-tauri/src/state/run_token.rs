// src-tauri/src/state/run_token.rs
//! 通用取消令牌槽(自 state.rs 结构性拆分,tierB-1;语义详见 [`RunTokenSlot`] 文档)。

use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// A cancellation-token slot tagged with a run generation (2026-07-18 审查 F-01/F-02)。
///
/// 后台流水线的通用「令牌槽」:`begin` 安装新一轮 (代次, token);`cancel` 显式取消当前轮;
/// `finish(代次)` 是完成回调专用的 compare-and-clear——仅当槽内仍是本轮才清空。没有代次时,
/// 旧轮迟到的收尾会 take+cancel 新一轮刚安装的 token(「停止→立即重启」竞态),这正是
/// `finish_ai_analysis` 已修过的同款问题(2026-07-10 审查 F10);本类型把该纪律沉淀为可复用件。
/// 现为全仓唯一范式:thumb/derive/ai/face 四槽均已迁入(F-025 收编 ai/face,各槽代次独立计数
/// ——finish 只与本槽存的代次比较,从无跨槽比较,共享计数器无必要)。
pub struct RunTokenSlot {
    generation: std::sync::atomic::AtomicU64,
    slot: Mutex<Option<(u64, CancellationToken)>>,
}

impl RunTokenSlot {
    pub fn new() -> Self {
        Self {
            generation: std::sync::atomic::AtomicU64::new(1),
            slot: Mutex::new(None),
        }
    }

    /// 安装新一轮 token,返回 (运行代次, token)。不取消旧轮——调用方需先显式 `cancel`
    /// (与 `new_ai_analysis_token` 的既有调用姿态一致)。
    pub fn begin(&self) -> (u64, CancellationToken) {
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let token = CancellationToken::new();
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, token.clone()));
        (generation, token)
    }

    /// 显式取消当前轮(用户停止/新一轮启动前):take + cancel 槽内任意轮次。
    pub fn cancel(&self) {
        if let Some((_, token)) = self.slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
            token.cancel();
        }
    }

    /// 取消当前轮但保留代次槽，供需要在取消后发布一次受控终态的流水线使用。
    ///
    /// 与 [`Self::cancel`] 的区别是：调用方仍可在同一代次下完成「发布 cancelled 快照 →
    /// `finish` 清槽」的线性化收尾；新一轮 `begin` 仍会替换该槽，旧轮不能取得发布权。
    pub fn cancel_keep_generation(&self) {
        if let Some((_, token)) = self.slot.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            token.cancel();
        }
    }

    /// 完成回调的 compare-and-clear。返回值 = 本轮是否仍是「当前轮」(终态发布权):
    /// - 槽内是本轮代次 → 清空,返回 `true`(正常完成);
    /// - 槽已空 → 返回 `true`(用户显式 stop 已 take,本轮仍拥有 cancelled 终态的发布权);
    /// - 槽被**更新一轮**占用 → 不动槽,返回 `false`(旧轮收尾不得再动任何全局状态)。
    pub fn finish(&self, generation: u64) -> bool {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some((g, _)) if *g == generation => {
                *slot = None;
                true
            }
            Some(_) => false,
            None => true,
        }
    }

    /// 运行态真相:槽内有 token 即在运行。
    pub fn is_running(&self) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// 「本轮是否仍持终态发布权」的**只读**判定(不清槽,语义同 [`Self::finish`] 的返回值)。
    /// 用于「先落终态快照、再清 token」的收尾姿态(审查 #13):清 token 前先据此决定是否发布,
    /// 使发布期间 token 仍在(is_running 仍 true),关闭「token 已清、快照仍 running」的误报窗口。
    pub fn is_current(&self, generation: u64) -> bool {
        let slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some((g, _)) if *g == generation => true,
            Some(_) => false,
            None => true,
        }
    }

    /// 严格的当前代次判定。与 [`Self::is_current`] 不同，槽已清空时返回 `false`，避免
    /// 旧 worker 在显式停止或正常收尾后继续发布迟到进度。
    pub fn is_generation_current(&self, generation: u64) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|(current, _)| *current == generation)
    }

    /// 当前槽内的令牌是否已取消。槽为空时返回 `false`；需要区分「已停止但尚未 finish」
    /// 的流水线可与 [`Self::is_generation_current`] 组合使用。
    pub fn current_is_cancelled(&self) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|(_, token)| token.is_cancelled())
    }
}

impl Default for RunTokenSlot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::RunTokenSlot;

    /// 正常生命周期:begin → finish(本轮) 清槽,运行态归零。
    #[test]
    fn finish_clears_own_generation() {
        let slot = RunTokenSlot::new();
        let (g1, t1) = slot.begin();
        assert!(slot.is_running());
        assert!(slot.finish(g1));
        assert!(!slot.is_running());
        assert!(!t1.is_cancelled());
    }

    /// 审查 F-01/F-02 核心场景:旧轮 finish 晚于新轮 start——旧轮不得清掉新轮 token。
    #[test]
    fn stale_finish_does_not_steal_new_run() {
        let slot = RunTokenSlot::new();
        let (g1, _t1) = slot.begin();
        // 重启:显式取消旧轮,再安装新轮(与 start_derivation / run_thumbnail_generation 姿态一致)。
        slot.cancel();
        let (g2, t2) = slot.begin();
        assert!(g2 > g1);
        // 旧轮迟到的收尾:代次不匹配 → 不清槽、无终态发布权。
        assert!(!slot.finish(g1));
        assert!(slot.is_running());
        assert!(!t2.is_cancelled());
        // 新轮自己的收尾正常清槽。
        assert!(slot.finish(g2));
        assert!(!slot.is_running());
    }

    /// 用户显式 stop(cancel 已 take 槽)后旧轮收尾:槽空 → 本轮仍拥有 cancelled 终态发布权。
    #[test]
    fn finish_after_explicit_stop_keeps_publish_right() {
        let slot = RunTokenSlot::new();
        let (g1, t1) = slot.begin();
        slot.cancel();
        assert!(t1.is_cancelled());
        assert!(!slot.is_running());
        assert!(slot.finish(g1));
    }

    /// 审查 #13:is_current 与 finish 返回值同义但**不清槽**——用于「先发布终态、再清 token」姿态。
    #[test]
    fn is_current_matches_finish_semantics_without_clearing() {
        let slot = RunTokenSlot::new();
        let (g1, _t1) = slot.begin();
        assert!(slot.is_current(g1));
        assert!(slot.is_running(), "is_current 只读、不清槽");
        // 新一轮顶掉:旧轮 is_current=false,新轮=true。
        let (g2, _t2) = slot.begin();
        assert!(!slot.is_current(g1));
        assert!(slot.is_current(g2));
        // cancel 清空后本轮仍持发布权(槽空 → true)。
        slot.cancel();
        assert!(slot.is_current(g2));
    }

    #[test]
    fn cancel_keep_generation_allows_strict_terminal_ownership() {
        let slot = RunTokenSlot::new();
        let (generation, token) = slot.begin();
        slot.cancel_keep_generation();
        assert!(token.is_cancelled());
        assert!(slot.is_generation_current(generation));
        assert!(slot.current_is_cancelled());
        assert!(slot.finish(generation));
        assert!(!slot.is_generation_current(generation));
    }

    /// 未经 cancel 的连续 begin(异常调用姿态)下,旧轮 finish 同样不得动新轮。
    #[test]
    fn back_to_back_begin_still_generation_safe() {
        let slot = RunTokenSlot::new();
        let (g1, _t1) = slot.begin();
        let (g2, t2) = slot.begin();
        assert!(!slot.finish(g1));
        assert!(slot.is_running());
        assert!(!t2.is_cancelled());
        assert!(slot.finish(g2));
    }
}
