// crates/exotic-workers/video-worker/src/progress.rs
//! ffmpeg `-progress pipe:1` 行协议解析 + Progress 帧节流(design.md §2.3/§2.4)。
//!
//! ffmpeg 每 ≤1s 向 pipe:1 刷一组 `key=value\n` 行,以 `progress=continue|end` 收尾。
//! 我们取当前已编码时间位置(毫秒),据 probe 时长算百分比;host 侧「静默限时」机制
//! (`worker.rs`)收 Progress 即重置计时,故只要 ffmpeg 在动就不会被误杀。

use std::time::{Duration, Instant};

/// Progress 帧最小间隔(design.md §2.3:每 ≤2s 发一帧)。
pub const PROGRESS_MIN_INTERVAL: Duration = Duration::from_secs(2);

/// `-progress` 行解析器:逐行喂入,累积当前编码位置(毫秒)。
///
/// 位置来源优先级:`out_time`(时钟串 `HH:MM:SS.uuuuuu`,无歧义)> `out_time_us`
/// (微秒)。刻意**不**用 `out_time_ms`——ffmpeg 历史遗留 bug 使该字段单位实为微秒,
/// 名实不符,只取无歧义的两者。
#[derive(Default)]
pub struct ProgressParser {
    pos_ms: Option<u64>,
}

impl ProgressParser {
    /// 喂一行,返回当前累积位置(毫秒);无可解位置时返回上一已知值。
    pub fn feed_line(&mut self, line: &str) -> Option<u64> {
        if let Some((k, v)) = line.split_once('=') {
            let (k, v) = (k.trim(), v.trim());
            match k {
                "out_time" => {
                    if let Some(ms) = parse_clock_ms(v) {
                        self.pos_ms = Some(ms);
                    }
                }
                "out_time_us" => {
                    if let Ok(us) = v.parse::<u64>() {
                        self.pos_ms = Some(us / 1000);
                    }
                }
                _ => {}
            }
        }
        self.pos_ms
    }
}

/// 解析 ffmpeg 时钟串 `HH:MM:SS.uuuuuu`(小数位为微秒)→ 毫秒。`N/A` / 畸形 → None。
pub fn parse_clock_ms(s: &str) -> Option<u64> {
    if s.is_empty() || s.starts_with("N/A") || s.starts_with('-') {
        return None;
    }
    let (hms, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    let mut parts = hms.split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let sec: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    // 取小数前 3 位作毫秒(不足补零,多余截断)。
    let mut millis = 0u64;
    for (i, ch) in frac.chars().take(3).enumerate() {
        let d = ch.to_digit(10)? as u64;
        millis += d * 10u64.pow(2 - i as u32);
    }
    Some(((h * 60 + m) * 60 + sec) * 1000 + millis)
}

/// 位置 + 时长 → 百分比整数(0..=100);时长未知/为 0 返回 None(detail 略去百分比)。
pub fn percent(pos_ms: u64, duration_ms: Option<u64>) -> Option<u32> {
    let dur = duration_ms?;
    if dur == 0 {
        return None;
    }
    Some(((pos_ms.min(dur) as f64 / dur as f64) * 100.0).round() as u32)
}

/// finalize 心跳预算基线(design.md V4 裁决=方案 c):`-progress` 流停更而 ffmpeg 子进程
/// 仍存活(trailer/moov 搬移静默窗)时,worker 自持的止损上界基线。
pub const FINALIZE_BASE_BUDGET: Duration = Duration::from_secs(60);
/// finalize 预算的产物体量项:每 50MiB 产物追加 1s(大产物 moov 搬移更久,预算随体量放宽)。
pub const FINALIZE_BYTES_PER_SEC: u64 = 50 * 1024 * 1024;

/// finalize 自持预算 = 基线 + 产物体量项。`out_bytes` 为**进入 finalize 时**的产物字节数
/// 快照(见 [`FinalizeMonitor::on_tick`] 注:快照而非动态,防产物持续增长把预算无限抬高
/// 而失去止损意义)。纯函数,单测覆盖。
pub fn finalize_budget(out_bytes: u64) -> Duration {
    FINALIZE_BASE_BUDGET + Duration::from_secs(out_bytes / FINALIZE_BYTES_PER_SEC)
}

/// [`FinalizeMonitor::on_tick`] 每拍的裁决动作。
#[derive(Debug, PartialEq, Eq)]
pub enum FinalizeAction {
    /// 发一帧 `stage="finalize"` 心跳(host 静默限时据此重置)。
    Heartbeat,
    /// 预算耗尽 → 调用方 kill ffmpeg 并回 InternalError。
    Kill,
    /// 本拍不发帧(未达心跳间隔且未越预算)。
    Wait,
}

/// finalize 态监视器(裁决=方案 c):`-progress` 流 EOF/停更而 ffmpeg 子进程仍存活时进入,
/// 据注入时钟每拍判定「发心跳 / 到预算 kill / 等待」。纯状态机,单测用 mock 时钟驱动
/// (不起真实 ffmpeg)。liveness 依据 = 子进程存活(调用方 `try_wait` 判)+ 产物字节数
/// (`on_tick` 的 `out_bytes` 入 detail,体现 size 在动);预算是最后止损防线。
#[derive(Default)]
pub struct FinalizeMonitor {
    /// 首次真正进入 finalize 的时刻(None = 尚未进入/已被 progress 恢复重置)。
    entered_at: Option<Instant>,
    /// 进入时按产物字节数快照定死的预算。
    budget: Duration,
    /// 上一次心跳时刻(节流 ≤2s 一发,复用 [`PROGRESS_MIN_INTERVAL`])。
    last_beat: Option<Instant>,
    /// 进入宽限:第 1 拍无 progress 只记待定、不真正进入 finalize(防正常编码负载抖动
    /// 单拍 >1s 无新行就闪成 finalize 态);连续第 2 拍仍无 progress 才真正进入。
    /// 期间任何一次 [`reset`](FinalizeMonitor::reset) 都会清掉这个待定计数。
    pending_first_tick: bool,
}

impl FinalizeMonitor {
    /// 喂一拍(ffmpeg 仍存活且 `-progress` 无新位置时由调用方驱动)。连续 2 拍无 progress
    /// 才真正进入 finalize(第 1 拍只记宽限待定、回 [`FinalizeAction::Wait`]);第 2 拍以
    /// `out_bytes` 快照定预算并立即回 [`FinalizeAction::Heartbeat`]。进入后越预算回
    /// [`FinalizeAction::Kill`],否则按 ≤2s 节流回 Heartbeat / Wait。
    pub fn on_tick(&mut self, now: Instant, out_bytes: u64) -> FinalizeAction {
        if self.entered_at.is_none() {
            if !self.pending_first_tick {
                // 第 1 拍:记宽限待定,不进入 finalize、不发心跳。
                self.pending_first_tick = true;
                return FinalizeAction::Wait;
            }
            // 连续第 2 拍:真正进入 finalize。
            self.entered_at = Some(now);
            self.budget = finalize_budget(out_bytes);
        }
        let entered = self.entered_at.unwrap();
        if now.duration_since(entered) >= self.budget {
            return FinalizeAction::Kill;
        }
        let due = match self.last_beat {
            None => true,
            Some(t) => now.duration_since(t) >= PROGRESS_MIN_INTERVAL,
        };
        if due {
            self.last_beat = Some(now);
            FinalizeAction::Heartbeat
        } else {
            FinalizeAction::Wait
        }
    }

    /// 收到真实 progress 位置推进 → 退出 finalize 态(含清掉进入宽限的待定计数;此前是
    /// 正常编码阶段,或 ffmpeg 罕见地在静默后又恢复输出;下次停更重新走两拍宽限、重新按
    /// 当时产物体量定预算)。
    pub fn reset(&mut self) {
        self.entered_at = None;
        self.last_beat = None;
        self.pending_first_tick = false;
    }
}

/// Progress 帧节流器:首帧立即放行,其后每 [`PROGRESS_MIN_INTERVAL`] 至多一帧。
#[derive(Default)]
pub struct ProgressThrottle {
    last: Option<Instant>,
}

impl ProgressThrottle {
    /// 是否该发帧(达间隔即放行并记时;`force` 无视间隔——用于收尾 100%)。
    pub fn should_emit(&mut self, now: Instant, force: bool) -> bool {
        let due = match self.last {
            None => true,
            Some(t) => now.duration_since(t) >= PROGRESS_MIN_INTERVAL,
        };
        if due || force {
            self.last = Some(now);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_parse_basic() {
        assert_eq!(parse_clock_ms("00:00:01.234000"), Some(1234));
        assert_eq!(parse_clock_ms("00:01:00.000000"), Some(60_000));
        assert_eq!(parse_clock_ms("01:00:00.5"), Some(3_600_500));
        assert_eq!(parse_clock_ms("00:00:00.999999"), Some(999));
    }

    #[test]
    fn clock_parse_rejects_na_and_garbage() {
        assert_eq!(parse_clock_ms("N/A"), None);
        assert_eq!(parse_clock_ms(""), None);
        assert_eq!(parse_clock_ms("-00:00:01.0"), None);
        assert_eq!(parse_clock_ms("xx:yy:zz"), None);
        assert_eq!(parse_clock_ms("00:00:01:02.0"), None);
    }

    #[test]
    fn parser_prefers_out_time_clock() {
        let mut p = ProgressParser::default();
        assert_eq!(p.feed_line("frame=10"), None);
        assert_eq!(p.feed_line("out_time_us=1500000"), Some(1500));
        // out_time 时钟串覆盖(同组内后到,取无歧义值)。
        assert_eq!(p.feed_line("out_time=00:00:02.000000"), Some(2000));
        assert_eq!(p.feed_line("progress=continue"), Some(2000));
    }

    #[test]
    fn out_time_ms_field_ignored() {
        // out_time_ms 名实不符(实为微秒),刻意不解析,避免 1000× 误算。
        let mut p = ProgressParser::default();
        assert_eq!(p.feed_line("out_time_ms=5000000"), None);
    }

    #[test]
    fn percent_computation() {
        assert_eq!(percent(500, Some(1000)), Some(50));
        assert_eq!(percent(2000, Some(1000)), Some(100)); // 钳制
        assert_eq!(percent(500, None), None);
        assert_eq!(percent(500, Some(0)), None);
    }

    #[test]
    fn throttle_first_immediate_then_gated() {
        let mut t = ProgressThrottle::default();
        let t0 = Instant::now();
        assert!(t.should_emit(t0, false)); // 首帧
        assert!(!t.should_emit(t0 + Duration::from_millis(500), false)); // 未达 2s
        assert!(t.should_emit(t0 + Duration::from_secs(3), false)); // 达 2s
                                                                    // force 无视间隔。
        assert!(t.should_emit(t0 + Duration::from_secs(3), true));
    }

    #[test]
    fn finalize_budget_scales_with_bytes() {
        assert_eq!(finalize_budget(0), Duration::from_secs(60));
        // 50MiB → +1s;500MiB → +10s。
        assert_eq!(finalize_budget(50 * 1024 * 1024), Duration::from_secs(61));
        assert_eq!(finalize_budget(500 * 1024 * 1024), Duration::from_secs(70));
    }

    /// 进入宽限(单拍不进/两拍进):第 1 拍无 progress 只记待定,不进入 finalize、不发心跳;
    /// 连续第 2 拍仍无 progress 才真正进入,立即心跳。
    #[test]
    fn finalize_requires_two_consecutive_ticks_to_enter() {
        let mut m = FinalizeMonitor::default();
        let t0 = Instant::now();
        // 第 1 拍:宽限待定,不进入 finalize。
        assert_eq!(m.on_tick(t0, 0), FinalizeAction::Wait);
        // 连续第 2 拍:真正进入 finalize,立即心跳。
        assert_eq!(
            m.on_tick(t0 + Duration::from_secs(1), 0),
            FinalizeAction::Heartbeat
        );
    }

    /// 两拍之间收到真实 progress(`reset`)→ 宽限待定计数清零;之后再次停更须重新走两拍宽限,
    /// 不会因为之前的第 1 拍待定而直接进入 finalize。
    #[test]
    fn finalize_progress_between_ticks_clears_grace() {
        let mut m = FinalizeMonitor::default();
        let t0 = Instant::now();
        assert_eq!(m.on_tick(t0, 0), FinalizeAction::Wait); // 第 1 拍待定
        m.reset(); // 期间收到真实 progress
                   // 重新从第 1 拍计数,不直接进入。
        assert_eq!(
            m.on_tick(t0 + Duration::from_secs(1), 0),
            FinalizeAction::Wait
        );
    }

    #[test]
    fn finalize_emits_heartbeat_then_throttles() {
        let mut m = FinalizeMonitor::default();
        let t0 = Instant::now();
        // 第 1 拍宽限待定,不发心跳。
        assert_eq!(m.on_tick(t0, 0), FinalizeAction::Wait);
        // 第 2 拍真正进入(起点 = 本拍),预算按 0 字节 = 60s,立即心跳。
        let entered = t0 + Duration::from_secs(1);
        assert_eq!(m.on_tick(entered, 0), FinalizeAction::Heartbeat);
        // 0.5s 后未达 2s 间隔 → 等待。
        assert_eq!(
            m.on_tick(entered + Duration::from_millis(500), 0),
            FinalizeAction::Wait
        );
        // 2s 后再发。
        assert_eq!(
            m.on_tick(entered + Duration::from_secs(2), 0),
            FinalizeAction::Heartbeat
        );
    }

    #[test]
    fn finalize_kills_when_budget_exhausted() {
        let mut m = FinalizeMonitor::default();
        let t0 = Instant::now();
        assert_eq!(m.on_tick(t0, 0), FinalizeAction::Wait); // 第 1 拍宽限
                                                            // 第 2 拍进入 finalize,起点 = 本拍,进入时 0 字节 → 预算 60s。
        let entered = t0 + Duration::from_secs(1);
        assert_eq!(m.on_tick(entered, 0), FinalizeAction::Heartbeat);
        // 越 60s(从进入时刻起)→ kill(产物字节数即便增长,预算按进入时快照,不被抬高)。
        assert_eq!(
            m.on_tick(entered + Duration::from_secs(60), 999_999_999),
            FinalizeAction::Kill
        );
    }

    #[test]
    fn finalize_reset_reenters_with_fresh_budget() {
        let mut m = FinalizeMonitor::default();
        let t0 = Instant::now();
        assert_eq!(m.on_tick(t0, 0), FinalizeAction::Wait);
        assert_eq!(
            m.on_tick(t0 + Duration::from_secs(1), 0),
            FinalizeAction::Heartbeat
        );
        // 收到真实 progress → 退出 finalize(含宽限待定计数清零)。
        m.reset();
        // 稍后再次停更:重新走两拍宽限——第 1 拍待定,第 2 拍才心跳。
        assert_eq!(
            m.on_tick(t0 + Duration::from_secs(2), 0),
            FinalizeAction::Wait
        );
        assert_eq!(
            m.on_tick(t0 + Duration::from_secs(3), 0),
            FinalizeAction::Heartbeat
        );
    }
}
