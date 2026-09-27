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
