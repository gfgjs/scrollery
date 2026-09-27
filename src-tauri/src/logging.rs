// src-tauri/src/logging.rs
//! 日志内核(日志能力重构线,方案 docs/designs/2026-07-20-日志能力重构方案.md)。
//! S1:JSONL 信封格式化器(§9.3-A)+ 会话 id 生成 + 日志目录大小兜底(§3.5)。
//! S4:UI 环形缓冲 Layer(§9.3-D)——独立日志窗口的实时流数据源。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// 保留大小兜底上限(方案 §3.5/§7 Q5:14 天 + 总量 512MB 兜底)。
pub const MAX_LOG_DIR_BYTES: u64 = 512 * 1024 * 1024;

/// 启动时生成的短会话 id(systemd `_BOOT_ID` 类比,如 `s-7f2a9c1e`),贯穿本次运行的全部日志行,
/// 一次运行的全部日志可用它一把 `rg` 抓出(方案 §3.3)。
pub fn generate_session_id() -> Arc<str> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = [0u8; 4];
    // SystemRandom 失败极罕见(OS 熵源故障);退化用当前时间纳秒数,保证不 panic。
    if SystemRandom::new().fill(&mut buf).is_err() {
        let nanos = chrono::Local::now().timestamp_subsec_nanos();
        buf = nanos.to_be_bytes();
    }
    format!("s-{:02x}{:02x}{:02x}{:02x}", buf[0], buf[1], buf[2], buf[3]).into()
}

/// 启动期日志目录大小兜底的执行结果(方案 §3.5「丢弃可见化」精神:清理了什么不应静默)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PurgeSummary {
    pub purged_files: u32,
    pub freed_bytes: u64,
}

/// 启动期日志目录大小兜底(方案 §3.5):总量超过 `max_bytes` 时按最旧文件优先删除,直至回落到位。
/// 只处理 `.log` 结尾的文件,防持续 trace 模式撑爆磁盘;当前活跃文件因 mtime 最新永远排最后一个删。
/// 运行时 tracing 尚未初始化(此函数在 subscriber 建好前执行),清理结果由调用方在 init 后补记日志。
pub fn enforce_size_budget(log_dir: &Path, max_bytes: u64) -> PurgeSummary {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return PurgeSummary::default();
    };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "log") {
                return None;
            }
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            Some((path, meta.len(), modified))
        })
        .collect();

    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= max_bytes {
        return PurgeSummary::default();
    }

    let mut summary = PurgeSummary::default();
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            summary.purged_files += 1;
            summary.freed_bytes += len;
        }
    }
    summary
}

/// 由用户选择的裸日志级别(如 `"info"`/`"off"`)构造完整 EnvFilter directive(方案 §3.4/S2 pipeline
/// target 归一):仅当级别比 warn 更啰嗦(trace/debug/info)时才追加流水线降噪后缀
/// `scrollery::pipeline=warn,reqwest=warn`——四条高频流水线(缩略图/AI/人脸/派生-视频)的调用点已改挂
/// `target: "scrollery::pipeline::{thumb,ai,face,video}"`,用户切到 debug/trace 诊断态时这条后缀
/// 仍能把它们摁在 warn 门槛之上,不随全局级别一起放开刷屏。
/// 用户选 warn/error 时不追加(reviewer 深审发现的真实 bug 修复):tracing-subscriber 的 EnvFilter
/// 按 target 特异性择优取最具体的一条 directive,不比较宽松/严格——若用户选了比 warn 更严格的
/// error 档,无脑追加 `scrollery::pipeline=warn` 会让 pipeline target 反而比用户选择更啰嗦。
/// off 档必须保持纯 `"off"`——§7 Q1「完全关闭」不容许任何 target 例外,追加后缀会让 pipeline target
/// 在 off 态仍以 warn 级别放行,与「off=真正全关」矛盾(§9.1 护栏 #7)。
/// 启动初始化(lib.rs)与运行时热切(config_commands.rs LOG_RELOAD)共用此函数,避免两处 directive
/// 拼接逻辑各写一份导致漂移。
pub fn build_env_filter_directive(level: &str) -> String {
    let trimmed = level.trim();
    if trimmed.eq_ignore_ascii_case("off") {
        return "off".to_string();
    }
    let more_verbose_than_warn = matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "trace" | "debug" | "info"
    );
    if more_verbose_than_warn {
        format!("{trimmed},scrollery::pipeline=warn,reqwest=warn")
    } else {
        trimmed.to_string()
    }
}

/// 只脱敏 Windows/Unix 用户主目录路径里的**用户名段**,不脱敏其余路径——本地单机 app 的日志明文
/// 路径对自查/agent 调试价值大(方案 §7 Q3 已裁),脱敏只在诊断包这个"出机"通道口做,且只处理
/// 真正标识个人身份的用户名段,不误伤诊断价值更高的资源路径本身。对**已解码**的纯文本(反斜杠
/// 未经 JSON 转义)生效——调用方必须先把 JSON 字符串解码成原始值再传入本函数,不能直接对
/// 序列化后的 JSONL 字节跑这套正则:JSON 转义会把 `C:\Users\gf` 编码成 `C:\\Users\\gf`
/// (每个反斜杠变两个),此时本正则(按单反斜杠匹配)完全不命中,曾是真实 bug(见
/// `redact_diagnostics_text` 文档)。
fn redact_plain_text(input: &str) -> (String, u64) {
    use std::sync::LazyLock;
    static WIN_USER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)(C:\\Users\\)([^\\\r\n]+)").expect("valid regex"));
    static UNIX_USER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(/(?:home|Users)/)([^/\r\n]+)").expect("valid regex")
    });

    let mut hits: u64 = 0;
    let after_win = WIN_USER.replace_all(input, |caps: &regex::Captures| {
        hits += 1;
        format!("{}***", &caps[1])
    });
    let after_unix = UNIX_USER.replace_all(&after_win, |caps: &regex::Captures| {
        hits += 1;
        format!("{}***", &caps[1])
    });
    (after_unix.into_owned(), hits)
}

/// 递归脱敏 JSON 值树的字符串叶子节点(数组/对象递归,其余类型不含路径故跳过),累加命中次数。
fn redact_json_value(value: &mut serde_json::Value) -> u64 {
    match value {
        serde_json::Value::String(s) => {
            let (redacted, hits) = redact_plain_text(s);
            *s = redacted;
            hits
        }
        serde_json::Value::Array(items) => items.iter_mut().map(redact_json_value).sum(),
        serde_json::Value::Object(map) => map.values_mut().map(redact_json_value).sum(),
        _ => 0,
    }
}

/// 诊断包导出前的本地正则脱敏扫描(方案 §5 P2:「导出前本地正则脱敏扫描(用户名/路径)」)。
/// 逐行按 JSONL 处理:能解析为 JSON 的行,先解码成 `serde_json::Value` 再递归脱敏全部字符串
/// 叶子节点、脱敏后重新序列化(**不对序列化后的原始字节直接跑正则**——JSON 转义会让路径分隔符
/// `\` 变成 `\\`,直接对字节跑正则会因反斜杠数量不匹配而完全失效,是本函数修复前的真实 bug,
/// 三条早期单测因为只喂了未转义的裸字符串而给出假绿灯,未覆盖它实际处理的数据形态)。解析失败
/// 的行(重构前的史前纯文本日志,或本就不是合法 JSON 的内容)按原文直接跑正则,不整行丢弃。
/// 返回(脱敏后文本, 命中次数)——命中次数供 UI 可见化(方案 §3.2「丢弃可见化」精神的镜像:
/// 脱敏做了什么不应静默)。
///
/// 副作用(可接受的权衡):JSON 值重新序列化时,`serde_json::Value::Object` 内部无序,输出的
/// 字段顺序可能与原文件不同(如 `ts/level/target/...` 信封字段顺序漂移)——诊断包是一次性导出
/// 产物,非事实源 JSONL 本身,顺序漂移不影响可读性或 agent 解析(仍是合法 JSON,字段名不变)。
pub fn redact_diagnostics_text(input: &str) -> (String, u64) {
    let mut total_hits = 0u64;
    let mut out_lines = Vec::new();
    for line in input.lines() {
        if line.is_empty() {
            out_lines.push(String::new());
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(mut value) => {
                total_hits += redact_json_value(&mut value);
                out_lines.push(serde_json::to_string(&value).unwrap_or_else(|_| line.to_string()));
            }
            Err(_) => {
                let (redacted, hits) = redact_plain_text(line);
                total_hits += hits;
                out_lines.push(redacted);
            }
        }
    }
    (out_lines.join("\n"), total_hits)
}

/// 收集单条 tracing 事件的字段(方案 §9.3-A):`message` 提为信封 `msg`、`operation_id` 提升进信封,
/// 其余字段进 `attributes`(顶层不爆炸,§3.3)。
#[derive(Default)]
struct FieldCollector {
    message: Option<String>,
    operation_id: Option<String>,
    attributes: serde_json::Map<String, serde_json::Value>,
}

impl FieldCollector {
    fn record_value(&mut self, field: &Field, value: serde_json::Value) {
        match field.name() {
            "message" => {
                self.message = Some(match value {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                });
            }
            "operation_id" => {
                self.operation_id = Some(match value {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                });
            }
            // AppError::serialize 的 error_log_dedup_check 富化(方案 §3.3/S2):调用点已把 chain 编码成合法 JSON 数组文本传入,
            // 这里解回真正的数组值,不是把整段 JSON 文本当字符串存进 attributes。
            "error_chain" => {
                let parsed = match &value {
                    serde_json::Value::String(s) => {
                        serde_json::from_str(s).unwrap_or_else(|_| value.clone())
                    }
                    _ => value.clone(),
                };
                self.attributes.insert("error.chain".to_string(), parsed);
            }
            "error_code" => {
                self.attributes.insert("error.code".to_string(), value);
            }
            // 前端日志桥(S3,方案 §4/system_commands.rs::log_frontend_events)的 `fields` 自由上下文:
            // 调用点已编码成合法 JSON 文本传入,这里解回真对象,不当字符串存——同 error_chain 惯例。
            // worker stderr 行转发桥(worker_log::emit_worker_log_line 的 worker_context)完全同一
            // 姿态,合并共用本臂;两个来源不会同时出现在同一事件上,共用 `context` 属性键不冲突。
            "frontend_context" | "worker_context" => {
                let parsed = match &value {
                    serde_json::Value::String(s) => {
                        serde_json::from_str(s).unwrap_or_else(|_| value.clone())
                    }
                    _ => value.clone(),
                };
                self.attributes.insert("context".to_string(), parsed);
            }
            name => {
                self.attributes.insert(name.to_string(), value);
            }
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record_value(field, serde_json::Value::String(format!("{value:?}")));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, serde_json::Value::String(value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, serde_json::Value::Bool(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field, serde_json::Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field, serde_json::Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_value(field, serde_json::Value::from(value));
    }
}

/// 收集单条事件并组装成信封 JSON 值(方案 §3.3):`ts/level/target/session_id/operation_id/msg/attributes`。
/// 供 [`EnvelopeFormat`](JSONL 文件层)与 [`RingBufferLayer`](UI 环形缓冲层,S4)共用同一套字段收集逻辑,
/// 防两处手写漂移(如 S2 曾踩过的 error_chain/frontend_context 特判只改一处的隐患)。
fn build_envelope(event: &Event<'_>, session_id: &str) -> serde_json::Value {
    let mut visitor = FieldCollector::default();
    event.record(&mut visitor);
    let meta = event.metadata();
    serde_json::json!({
        "ts": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
        "level": meta.level().to_string(),
        "target": meta.target(),
        "session_id": session_id,
        "operation_id": visitor.operation_id,
        "msg": visitor.message.unwrap_or_default(),
        "attributes": serde_json::Value::Object(visitor.attributes),
    })
}

/// JSONL 信封格式化器(方案 §3.1/§9.3-A):固定信封字段 `ts/level/target/session_id/operation_id/msg`,
/// 领域字段进 `attributes`。内置 `.json().flatten_event(true)` 无法注入 session_id/operation_id 这类
/// 信封字段(方案 §9.2 陷阱表),故自定义。
pub struct EnvelopeFormat {
    pub session_id: Arc<str>,
}

impl<S, N> FormatEvent<S, N> for EnvelopeFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let line = build_envelope(event, &self.session_id);
        writeln!(writer, "{line}")
    }
}

/// 环形缓冲上限的回退默认值(方案 §5 MVP:「默认 10k 行,设置可调 5k-20k」;批次C接线
/// `log_ring_buffer_capacity` 后,真实生效值改由启动期读入的 `LogRingBuffer::capacity` 字段
/// 决定,本常量只在 config.toml 缺省时兜底)。此值是**后端**安全阀——真正面向用户可调的
/// 「渲染保留行数」在前端本地实现(纯内存数组裁剪,零后端往返,方案 §9.3-D 只要求后端有界队列
/// 不无限增长)。取区间上限:100ms 批推节奏下后端队列稳态应远小于此值,只在抽干任务迟滞的
/// 极端场景起安全阀作用。
pub const RING_BUFFER_CAPACITY: usize = 20_000;

/// UI 环形缓冲层的共享状态(方案 §9.3-D):`subscribed` 由日志窗口 open/close 翻转(建窗即置真,
/// `WindowEvent::Destroyed` 回调置假——见 lib.rs `open_log_window`),`buffer` 由 100ms 周期任务
/// 抽干批推(见 lib.rs 后台任务)。`capacity` 批次C起可配(`log_ring_buffer_capacity`,启动期读入,
/// 修改后需重启生效——本层挂在 tracing 全局 subscriber 上,构造后不可替换)。
pub struct LogRingBuffer {
    subscribed: AtomicBool,
    buffer: Mutex<VecDeque<serde_json::Value>>,
    session_id: Arc<str>,
    capacity: usize,
}

impl LogRingBuffer {
    pub fn new(session_id: Arc<str>, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            subscribed: AtomicBool::new(false),
            buffer: Mutex::new(VecDeque::new()),
            session_id,
            capacity,
        })
    }

    pub fn set_subscribed(&self, active: bool) {
        self.subscribed.store(active, Ordering::Relaxed);
        // 取消订阅时清空:历史由 JSONL 文件承载,残留内容只会在下次订阅时造成一批陈旧数据抢跑。
        if !active {
            self.buffer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
    }

    /// 抽干当前缓冲(方案 §9.3-D「100ms drain 批推」),空则返回 `None`(调用方据此跳过一次 emit,
    /// 无订阅者时零广播)。
    pub fn drain(&self) -> Option<Vec<serde_json::Value>> {
        let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        if buf.is_empty() {
            return None;
        }
        Some(buf.drain(..).collect())
    }
}

/// UI 环形缓冲层(方案 §9.3-D):`on_event` 内**先查订阅标志**,无订阅者(日志窗口未开)直接返回,
/// 不格式化不入队(方案 §9.1 护栏 #3:同步上下文,禁阻塞 IO/DB/tokio 锁——这里只做内存 push_back +
/// 定长裁剪,std Mutex 短临界区)。挂在与文件层同一条 `reload::Layer<EnvFilter>` 之后,故自动继承
/// 当前 logLevel/off 档过滤,无需重复实现级别判断。
pub struct RingBufferLayer {
    state: Arc<LogRingBuffer>,
}

impl RingBufferLayer {
    pub fn new(state: Arc<LogRingBuffer>) -> Self {
        Self { state }
    }
}

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !self.state.subscribed.load(Ordering::Relaxed) {
            return;
        }
        let value = build_envelope(event, &self.state.session_id);
        let mut buf = self.state.buffer.lock().unwrap_or_else(|e| e.into_inner());
        buf.push_back(value);
        while buf.len() > self.state.capacity {
            buf.pop_front();
        }
    }
}

/// span 计时守卫的级别(方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入 D-311):只分
/// info(流水线 run 级/任务型 IPC command)与 debug(视口/滚动热路径查询)两档,不做更细分级——
/// 与既有 logLevel 阈值语义对齐,off 档下两档均被顶层 EnvFilter 挡住(§9.1 护栏 #7 off 全关)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanLevel {
    Info,
    Debug,
}

/// 手动计时的 span 守卫(D-311:弃 tracing span 机制/`#[instrument]`/`FmtSpan`,改「手动计时 +
/// 普通结构化事件」)。构造后持有到作用域结束(可跨 `.await`,`SpanTimer: Send`——字段全是
/// `&'static str`/`String`/`Instant`/枚举,天然 Send,无需显式实现),`Drop` 时按 `duration_ms`
/// 发一条 `target: "scrollery::span"` 的固定字面量 `"span_close"` 事件(§9.1 护栏 #6:禁动态插值
/// 进 message,时长/名称等全部走结构化字段)。
///
/// 为什么不用 tracing span(`info_span!`/`#[instrument]`)+ `FmtSpan::CLOSE`:`FmtSpan` 合成的
/// close 事件只在 `fmt::Layer` 内部走 `FormatEvent`,不经过全局 dispatcher——本仓的
/// `RingBufferLayer`(UI 实时流)与其它自建 `Layer` 完全收不到(方案 findings.md F-028)。span 也
/// 不跨 `spawn_blocking`/`thread::spawn`(D-304 已定型,本仓贯穿 operation_id 走显式参数而非
/// span 上下文),两条理由叠加,普通事件 + 手动计时是唯一能让 JSONL 文件层 + UI 环形缓冲层同时
/// 免费收到的路径。
pub struct SpanTimer {
    name: &'static str,
    level: SpanLevel,
    operation_id: Option<String>,
    start: std::time::Instant,
}

impl SpanTimer {
    /// info 档:流水线 run 级、任务型/低频 IPC command(D-312)。
    pub fn info(name: &'static str) -> Self {
        Self {
            name,
            level: SpanLevel::Info,
            operation_id: None,
            start: std::time::Instant::now(),
        }
    }

    /// debug 档:视口/滚动热路径查询 IPC command(D-312)——默认 info 级不被刷屏,诊断态可见。
    pub fn debug(name: &'static str) -> Self {
        Self {
            name,
            level: SpanLevel::Debug,
            operation_id: None,
            start: std::time::Instant::now(),
        }
    }

    /// 链式挂载 operation_id(信封字段提升,非 attributes——`FieldCollector` 对字段名
    /// `operation_id` 已有特判,见上文)。派生流水线等已贯穿 operation_id 参数的调用点用它对齐
    /// 同一次运行的日志行。
    pub fn with_operation_id(mut self, op: Option<String>) -> Self {
        self.operation_id = op;
        self
    }

    /// 实际发事件的逻辑抽成关联函数(而非只内联在 `Drop::drop` 里):单测「panic 路径
    /// `panicked=true`」需要在 `catch_unwind` 内构造+panic 触发 `Drop`,若 `Drop` 与测试基建
    /// 冲突可退化为直接调用本函数验证同一逻辑(方案任务清单 W1.4-d 的预留退路)。
    fn emit_span_close(&self) {
        // f64 毫秒(非整毫秒截断):debug 档热路径查询缓存命中常 <1ms,as_millis 截断会让
        // 该档 TopN 的 totalMs/avgMs 恒 0 失真(reviewer 深审建议);FieldCollector::record_f64
        // 落 JSON number,前端 `typeof === 'number'` 判据不受影响。
        let duration_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let panicked = std::thread::panicking();
        match self.level {
            SpanLevel::Info => tracing::info!(
                target: "scrollery::span",
                span_name = self.name,
                duration_ms,
                operation_id = self.operation_id.as_deref(),
                panicked,
                "span_close"
            ),
            SpanLevel::Debug => tracing::debug!(
                target: "scrollery::span",
                span_name = self.name,
                duration_ms,
                operation_id = self.operation_id.as_deref(),
                panicked,
                "span_close"
            ),
        }
    }
}

impl Drop for SpanTimer {
    fn drop(&mut self) {
        self.emit_span_close();
    }
}

/// [`init_subscriber`] 的产出:交由调用方托管/传给 `AppState` 的日志句柄。
pub struct LoggingBoot {
    /// non_blocking 写线程的 flush guard——**必须交给 Tauri `manage`**,局部持有会立即
    /// drop 致日志静默全丢(方案 §9.2 陷阱表)。
    pub guard: tracing_appender::non_blocking::WorkerGuard,
    pub log_ring: Arc<LogRingBuffer>,
    pub dropped_counter: tracing_appender::non_blocking::ErrorCounter,
    /// 启动期目录大小兜底的清理结果;subscriber 建好后由调用方补记一条 info。
    pub purge_summary: PurgeSummary,
}

/// 日志子系统装配:目录大小兜底 → RollingFileAppender → non_blocking → EnvFilter(可 reload)
/// → 文件层(JSONL 信封)+ UI 环形缓冲层(+ dev 控制台层)→ 全局 subscriber + panic hook。
///
/// 自 `lib.rs::run()` 的 setup 段 j+k 迁出(D-450 纯结构移动,行为不变)。
///
/// 顺序不变量(拆分方案 §3.1 第 4 条):本函数返回前,一切错误只能走 `eprintln!`/原生对话框;
/// 启动期 DB 自愈等需要日志可见的步骤必须排在本函数**之后**。
pub fn init_subscriber(
    log_dir: &Path,
    config: &crate::config::ConfigManager,
    log_level: &str,
) -> Result<LoggingBoot, crate::StartupFailure> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    // 大小兜底(方案 §3.5/§7 Q5):启动时若日志目录总量超上限,按最旧文件优先删除。
    // 此时 tracing 尚未就绪,清理结果先存着,留到 subscriber 建好后补记一条 info(丢弃可见化)。
    // 批次C:max_log_dir_bytes(advanced 键,单位 MB)接线——config_manager 已在本段之前
    // 构造好(见 config::boot),故此处直接读、无需重排启动序;仅在启动期这一次
    // 读取生效(RollingFileAppender 的 max_log_files=14 也是启动期定死的同类兜底),
    // 运行期改配置需重启才影响下次启动的清理上限,故本键 hot 保持假(通知重启由此推导)。
    let max_log_dir_bytes: u64 = config
        .get("max_log_dir_bytes")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|mb| mb.saturating_mul(1024 * 1024))
        .unwrap_or(MAX_LOG_DIR_BYTES);
    let purge_summary = enforce_size_budget(log_dir, max_log_dir_bytes);

    // 会话 id(方案 §3.3):一次运行的全部日志行共享同一 id,可用它一把 rg 抓出整段运行。
    let session_id = generate_session_id();

    // RollingFileAppender(daily + max_log_files=14,方案 §3.1)取代旧 RealTimeDailyAppender——
    // 后者每条 open+append+fsync 是为「资源管理器实时看到文件增长」服务的,有了应用内 UI
    // 实时视图(S4)后此需求消失;fsync 在高频流水线下是吞吐瓶颈(§3.2)。
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("scrollery")
        .filename_suffix("log")
        .max_log_files(14)
        .build(log_dir)
        .map_err(|e| {
            crate::StartupFailure::new(
                "无法初始化日志滚动文件 / cannot initialize rolling log file",
                e,
            )
        })?;
    // non_blocking 卸写盘到后台线程;guard 交给 Tauri 管理(而非 Box::leak),退出时正常 flush
    // (方案 §9.2 陷阱表:局部变量持 guard 会立即 drop 致日志静默全丢)。
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // 丢弃计数(方案 §3.2 风险「non_blocking lossy 在日志风暴下丢弃」+ §5 P1「丢弃计数可见化」):
    // ErrorCounter 是 Arc 包装的廉价 Clone,存入 AppState 供诊断面板轮询,不静默丢日志无感知。
    let dropped_counter = non_blocking.error_counter();

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(build_env_filter_directive(log_level)));
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);
    let _ = crate::ipc::config_commands::LOG_RELOAD.set(reload_handle);

    // 文件层:JSONL 信封格式化器(§9.3-A),事实源——agent/UI 历史都读它。
    let file_layer = tracing_subscriber::fmt::layer()
        .event_format(EnvelopeFormat {
            session_id: session_id.clone(),
        })
        .with_ansi(false)
        .with_writer(non_blocking);

    // UI 环形缓冲层(方案 §5/§9.3-D,S4):独立日志窗口的实时流数据源。挂在与文件层同一条
    // reload::Layer<EnvFilter> 之后,自动继承当前 logLevel/off 档过滤。订阅标志默认为假
    // (日志窗口未开时零成本,见 RingBufferLayer 文档)。
    // 批次C:log_ring_buffer_capacity(advanced 键)接线——同上,config_manager 已就位,
    // 启动期读一次;环形缓冲挂在 tracing 全局 subscriber 上构造后不可替换,故运行期改配置
    // 需重启才生效(本键 hot 保持假,通知重启由 hot 推导)。
    let log_ring_buffer_capacity: usize = config
        .get("log_ring_buffer_capacity")
        .and_then(|v| v.parse().ok())
        .unwrap_or(RING_BUFFER_CAPACITY);
    let log_ring = LogRingBuffer::new(session_id.clone(), log_ring_buffer_capacity);
    let ring_layer = RingBufferLayer::new(log_ring.clone());

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(ring_layer);

    // 控制台文本层仅 dev 构建装载(方案 §3.1:release 也装此层是白付格式化成本)。
    #[cfg(debug_assertions)]
    {
        let timer = tracing_subscriber::fmt::time::ChronoLocal::rfc_3339();
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_timer(timer)
                    .with_ansi(true),
            )
            .init();
    }
    #[cfg(not(debug_assertions))]
    {
        registry.init();
    }

    // panic hook:panic 消息(+backtrace,视 RUST_BACKTRACE 环境变量而定)作为最后一条 ERROR
    // 进文件(方案 §3.1/§9.1);non_blocking 场景配合调用方 app.manage(guard) 保证落盘。
    std::panic::set_hook(Box::new(tracing_panic::panic_hook));

    Ok(LoggingBoot {
        guard,
        log_ring,
        dropped_counter,
        purge_summary,
    })
}
