// src-tauri/src/exotic/worker_log.rs
//! worker stderr 行日志转发（日志能力重构线 阶段 3 · W4，D-310/D-313）。从 `supervisor.rs`
//! 拆出的纯函数/自包含状态机单元(超长文件拆分方案 tierB-2):不触碰 `WorkerSupervisor` 私有字段。
//!
//! stdout 只走协议帧,stderr 一直是自由文本诊断区(§3.4)。此前 stderr 只全量进 64KiB
//! 环形缓冲(崩溃摘尾诊断);本模块新增**行级**扫描 + 解析 + 转发,把 worker 侧
//! `exotic_protocol::WorkerLogLine` 单行 JSON(W3)收编进主进程 tracing/JSONL 体系——
//! 字节级环形缓冲机制原样保留在 `supervisor.rs`(D-313:崩溃路径下行会双写,已接受;
//! 两套机制**互不合并**,详见 [`super::supervisor::spawn_stderr_drain`] 处的红线注释)。

use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::worker::WorkerSpec;

/// 转发进主 tracing 的固定 target(编译期常量——tracing target 不能塞运行时字符串,
/// S2/S3 两次踩过);`worker` 字段承载运行时来源区分("ai"/"psd"),镜像 S3 前端日志桥
/// (`system_commands.rs::emit_frontend_log_event`)固定 target + 来源字段的姿态。
const WORKER_LOG_TARGET: &str = "scrollery::worker";

/// 残段累积上限:无换行的字节洪流(损坏/异常 worker 疯狂写 stderr 却不换行)不得无限撑大
/// 内存——超限即把当前残段整体当一行强制冲出(W4 施工指引)。与 `STDERR_RING_CAP`(留在
/// `supervisor.rs`)同量级但语义不同:那个是字节环形缓冲上限,这个是行扫描器残段上限,
/// 互不干扰。
const LINE_RESIDUAL_CAP: usize = 64 * 1024;

/// stderr 行扫描器:把跨 4096 字节块读取的字节流按 `\n` 切成完整行,残段累积到下一次
/// `feed`。纯函数/无 IO/无 tracing 依赖,便于确定性单测覆盖(跨块半行拼接、一块多行、
/// EOF 残段、64KiB 上限强冲)。
struct LineScanner {
    residual: Vec<u8>,
}

impl LineScanner {
    fn new() -> Self {
        Self {
            residual: Vec::new(),
        }
    }

    /// 喂入新读到的字节块,返回本次新切出的完整行(不含换行符本身)。
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        let mut start = 0usize;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'\n' {
                if self.residual.is_empty() {
                    lines.push(chunk[start..i].to_vec());
                } else {
                    self.residual.extend_from_slice(&chunk[start..i]);
                    lines.push(std::mem::take(&mut self.residual));
                }
                start = i + 1;
            }
        }
        if start < chunk.len() {
            self.residual.extend_from_slice(&chunk[start..]);
        }
        // 残段超限(无换行洪流):强制把整段当一行冲出,防内存无界增长。
        if self.residual.len() > LINE_RESIDUAL_CAP {
            lines.push(std::mem::take(&mut self.residual));
        }
        lines
    }

    /// EOF 时把残段(若非空)当最后一行冲出。
    fn finish(&mut self) -> Option<Vec<u8>> {
        if self.residual.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.residual))
        }
    }
}

/// 一行 stderr 的解析结果:成功识别出 [`exotic_protocol::WorkerLogLine`] 形状,或原样文本兜底。
enum ParsedWorkerLine {
    Structured(exotic_protocol::WorkerLogLine),
    Raw(String),
}

/// 解析一行(调用方已 trim `\r`/跳过空行):先查首字符 `{` 省一次无谓解析尝试,非 JSON
/// 或反序列化失败均走 `Raw` 兜底(W4 施工指引)。纯函数,不依赖 tracing。
fn parse_worker_log_line(line: &str) -> ParsedWorkerLine {
    if line.trim_start().starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<exotic_protocol::WorkerLogLine>(line) {
            return ParsedWorkerLine::Structured(parsed);
        }
    }
    ParsedWorkerLine::Raw(line.to_string())
}

/// 把已识别的 [`exotic_protocol::WorkerLogLine`] 转发为一条 tracing 事件(镜像
/// `emit_frontend_log_event` 的固定 target + 按级别多臂 match + 动态 msg 姿态)。
/// `fields` 序列化回 JSON 字符串经 `worker_context` 字段携带(`logging.rs::FieldCollector`
/// 照 `frontend_context` 惯例特判解回真对象)。未知 `lvl` 值按 warn 转发并附原 `lvl` 字段
/// (schema 漂移应可见)。
fn emit_worker_log_line(worker_kind: &str, line: &exotic_protocol::WorkerLogLine) {
    let fields_json = if line.fields.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(line.fields.clone()).to_string())
    };
    macro_rules! emit {
        ($lvl:ident) => {
            tracing::$lvl!(
                target: WORKER_LOG_TARGET,
                worker = worker_kind,
                worker_context = fields_json.as_deref(),
                "{}",
                line.msg
            )
        };
    }
    match line.lvl.as_str() {
        "trace" => emit!(trace),
        "debug" => emit!(debug),
        "info" => emit!(info),
        "warn" => emit!(warn),
        "error" => emit!(error),
        other => {
            tracing::warn!(
                target: WORKER_LOG_TARGET,
                worker = worker_kind,
                worker_context = fields_json.as_deref(),
                lvl = other,
                "{}",
                line.msg
            );
        }
    }
}

/// 转发一整行原始 stderr 字节(已由 [`LineScanner`] 切出):trim 尾部 `\r`(CRLF 兜底)、
/// 跳过空行,能解析出 [`exotic_protocol::WorkerLogLine`] 即结构化转发,否则以 WARN +
/// `unparsed=true` 转发原文(W4 施工指引「非 JSON 行有兜底」)。
fn forward_stderr_line(worker_kind: &str, raw_line: &[u8]) {
    let text = String::from_utf8_lossy(raw_line);
    let trimmed = text.strip_suffix('\r').unwrap_or(&text);
    if trimmed.is_empty() {
        return;
    }
    match parse_worker_log_line(trimmed) {
        ParsedWorkerLine::Structured(line) => emit_worker_log_line(worker_kind, &line),
        ParsedWorkerLine::Raw(raw) => {
            tracing::warn!(
                target: WORKER_LOG_TARGET,
                worker = worker_kind,
                unparsed = true,
                "{raw}"
            );
        }
    }
}

/// 由 [`WorkerSpec::expected_worker_id`] 派生转发日志的 `worker` 字段短名(W4,D-313):
/// 免新增字段/改调用签名(施工指引「选改动最小路径」)——`expected_worker_id` 本身即两个
/// worker 的稳定字面标识("ai-worker"/"psd-worker",见 `worker.rs`/`ai/worker_client.rs`)。
/// 已知两种归一化为短名;未来若有其它 exotic 插件 worker,原样透传其 id(不强行塞进
/// ai/psd 二选一,转发管线仍能正常工作)。
pub(crate) fn worker_kind_label(spec: &WorkerSpec) -> String {
    match spec.expected_worker_id.as_str() {
        "ai-worker" => "ai".to_string(),
        "psd-worker" => "psd".to_string(),
        other => other.to_string(),
    }
}

/// stderr 排空线程:持续读,①字节照旧全量追加进有界环形缓冲(超过上限从头丢弃,
/// 崩溃摘尾诊断,由调用方 `ring`/`ring_cap` 提供,机制留在 `supervisor.rs`)②另按行扫描,
/// 完整行解析/转发进主 tracing/JSONL 体系(W4)。两件事各自独立、互不影响;崩溃路径下
/// 同一行「双写」(环形缓冲 + 已转发的 tracing 行)是已接受的代价(D-313,两套机制勿合并)。
pub(crate) fn spawn_stderr_drain<R: Read + Send + 'static>(
    mut r: R,
    ring: Arc<Mutex<VecDeque<u8>>>,
    ring_cap: usize,
    worker_kind: String,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut scanner = LineScanner::new();
        loop {
            match r.read(&mut buf) {
                Ok(0) => {
                    // EOF：残段（若非空）当最后一行冲出。
                    if let Some(last) = scanner.finish() {
                        forward_stderr_line(&worker_kind, &last);
                    }
                    break;
                }
                Ok(n) => {
                    let chunk = &buf[..n];
                    {
                        let mut g = ring.lock().unwrap_or_else(|e| e.into_inner());
                        g.extend(chunk);
                        // 截断到最近 ring_cap 字节。
                        while g.len() > ring_cap {
                            g.pop_front();
                        }
                    }
                    for line in scanner.feed(chunk) {
                        forward_stderr_line(&worker_kind, &line);
                    }
                }
                Err(_) => {
                    // 读错误(worker 被 kill 等非 EOF 终止):与 EOF 路径对称,残段(若非空)
                    // 当最后一行冲出再退,避免最后一条无换行诊断只留在字节环形缓冲。
                    if let Some(last) = scanner.finish() {
                        forward_stderr_line(&worker_kind, &last);
                    }
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn line_scanner_splits_single_chunk_multi_line() {
        let mut sc = LineScanner::new();
        let lines = sc.feed(b"a\nbb\nccc\n");
        assert_eq!(lines, vec![b"a".to_vec(), b"bb".to_vec(), b"ccc".to_vec()]);
        assert!(sc.finish().is_none(), "整块以换行结尾,不应残留残段");
    }

    #[test]
    fn line_scanner_joins_half_line_across_chunks() {
        let mut sc = LineScanner::new();
        assert!(sc.feed(b"hel").is_empty(), "无换行时不应产出完整行");
        let lines = sc.feed(b"lo\nworld\n");
        assert_eq!(lines, vec![b"hello".to_vec(), b"world".to_vec()]);
    }

    #[test]
    fn line_scanner_flushes_residual_on_eof() {
        let mut sc = LineScanner::new();
        assert!(sc.feed(b"no newline yet").is_empty());
        let last = sc.finish().expect("EOF 应把残段当最后一行冲出");
        assert_eq!(last, b"no newline yet".to_vec());
        assert!(
            sc.finish().is_none(),
            "冲出后残段应清空,二次 finish 返回 None"
        );
    }

    #[test]
    fn line_scanner_leaves_cr_for_caller_to_trim() {
        // LineScanner 只按 \n 切分,\r 留给调用方(forward_stderr_line)统一 trim——
        // 这里断言切出的行末尾确实带 \r,证明分工边界符合设计。
        let mut sc = LineScanner::new();
        let lines = sc.feed(b"line1\r\nline2\r\n");
        assert_eq!(lines, vec![b"line1\r".to_vec(), b"line2\r".to_vec()]);
    }

    #[test]
    fn line_scanner_force_flushes_when_residual_exceeds_cap() {
        let mut sc = LineScanner::new();
        let flood = vec![b'x'; LINE_RESIDUAL_CAP + 10];
        let lines = sc.feed(&flood);
        assert_eq!(lines.len(), 1, "超限应强制冲出一整段,不等换行");
        assert_eq!(lines[0].len(), flood.len());
        assert!(sc.finish().is_none(), "强冲后残段应已清空");
    }

    #[test]
    fn parse_worker_log_line_recognizes_valid_json() {
        let json = r#"{"lvl":"info","msg":"会话就绪","fields":{"req_id":7}}"#;
        match parse_worker_log_line(json) {
            ParsedWorkerLine::Structured(line) => {
                assert_eq!(line.lvl, "info");
                assert_eq!(line.msg, "会话就绪");
                assert_eq!(line.fields.get("req_id").and_then(|v| v.as_i64()), Some(7));
            }
            ParsedWorkerLine::Raw(_) => panic!("应识别为结构化行"),
        }
    }

    #[test]
    fn parse_worker_log_line_falls_back_on_non_json() {
        match parse_worker_log_line("plain text, not json") {
            ParsedWorkerLine::Raw(text) => assert_eq!(text, "plain text, not json"),
            ParsedWorkerLine::Structured(_) => panic!("非 JSON 不应被误判为结构化"),
        }
    }

    #[test]
    fn parse_worker_log_line_falls_back_on_malformed_json_starting_with_brace() {
        // 首字符是 `{` 但不是合法 WorkerLogLine 形状(如帧协议 JSON 泄漏进 stderr)→ 仍走 Raw 兜底。
        match parse_worker_log_line(r#"{"not":"a worker log line"}"#) {
            ParsedWorkerLine::Raw(_) => {}
            ParsedWorkerLine::Structured(_) => panic!("形状不符不应解析成功"),
        }
    }

    #[test]
    fn worker_kind_label_maps_known_ids_and_passes_through_unknown() {
        assert_eq!(
            worker_kind_label(&WorkerSpec {
                exe_path: std::path::PathBuf::from("x"),
                expected_worker_id: "psd-worker".into(),
                required_capabilities: vec![],
            }),
            "psd"
        );
        assert_eq!(
            worker_kind_label(&WorkerSpec {
                exe_path: std::path::PathBuf::from("y"),
                expected_worker_id: "ai-worker".into(),
                required_capabilities: vec![],
            }),
            "ai"
        );
        assert_eq!(
            worker_kind_label(&WorkerSpec {
                exe_path: std::path::PathBuf::from("z"),
                expected_worker_id: "future-plugin".into(),
                required_capabilities: vec![],
            }),
            "future-plugin"
        );
    }

    // ── stderr 行转发 → tracing 事件(W4)——scoped 捕获断言转发事件信封形状 ──────────────

    /// 同步内存 writer(镜像 logging.rs::tests::SharedBuf/system_commands.rs 测试姿态):
    /// 把 fmt::Layer 输出直接攒进 Arc<Mutex<Vec<u8>>>,scoped `with_default` 断言完即读。
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 建一条「EnvelopeFormat 文件层 + trace 档 EnvFilter」的 scoped subscriber,在 `f` 作用域内
    /// 跑被测转发调用,返回按行解析出的 JSON 信封列表(与 `logging.rs::tests` 同姿态,禁
    /// `set_global_default`,方案 §9.2 陷阱表)。
    fn capture_forwarded_lines<F: FnOnce()>(f: F) -> Vec<serde_json::Value> {
        use tracing_subscriber::layer::SubscriberExt;
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_for_writer = buf.clone();
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(crate::logging::EnvelopeFormat {
                session_id: Arc::from("s-test"),
            })
            .with_ansi(false)
            .with_writer(move || SharedBuf(buf_for_writer.clone()));
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new("trace"))
            .with(file_layer);
        tracing::subscriber::with_default(subscriber, f);
        let content = String::from_utf8(buf.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .expect("captured bytes are valid utf8");
        content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                serde_json::from_str(l).unwrap_or_else(|e| panic!("line not valid JSON: {e}: {l}"))
            })
            .collect()
    }

    #[test]
    fn forward_stderr_line_structured_carries_worker_context_and_msg() {
        let lines = capture_forwarded_lines(|| {
            forward_stderr_line(
                "ai",
                r#"{"lvl":"info","msg":"会话就绪","fields":{"session_id":9}}"#.as_bytes(),
            );
        });
        assert_eq!(lines.len(), 1);
        let v = &lines[0];
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["target"], "scrollery::worker");
        assert_eq!(v["msg"], "会话就绪");
        assert_eq!(v["attributes"]["worker"], "ai");
        assert_eq!(v["attributes"]["context"]["session_id"], 9);
    }

    #[test]
    fn forward_stderr_line_unknown_level_falls_back_to_warn_with_original_lvl() {
        let lines = capture_forwarded_lines(|| {
            forward_stderr_line("psd", r#"{"lvl":"critical","msg":"未知级别"}"#.as_bytes());
        });
        assert_eq!(lines.len(), 1);
        let v = &lines[0];
        assert_eq!(v["level"], "WARN");
        assert_eq!(v["attributes"]["lvl"], "critical");
        assert_eq!(v["msg"], "未知级别");
    }

    #[test]
    fn forward_stderr_line_non_json_falls_back_to_warn_with_unparsed_flag() {
        let lines = capture_forwarded_lines(|| {
            forward_stderr_line("psd", b"plain diagnostic text");
        });
        assert_eq!(lines.len(), 1);
        let v = &lines[0];
        assert_eq!(v["level"], "WARN");
        assert_eq!(v["msg"], "plain diagnostic text");
        assert_eq!(v["attributes"]["unparsed"], true);
        assert_eq!(v["attributes"]["worker"], "psd");
    }

    #[test]
    fn forward_stderr_line_skips_empty_lines() {
        let lines = capture_forwarded_lines(|| {
            forward_stderr_line("ai", b"");
            forward_stderr_line("ai", b"\r");
        });
        assert!(lines.is_empty(), "空行(含仅 \\r)不应转发任何事件");
    }
}
