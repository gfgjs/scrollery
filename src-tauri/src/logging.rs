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

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    /// off 档全链路特征化测试(方案 §9.4 S1 手测清单的自动化等价物):同一 WorkerGuard 生命周期内
    /// 依次 info→off→info,验证① off 期间事件在过滤阶段即被丢弃、从未触达写入器,
    /// ② 切回后恢复写入,③ guard drop 时阻塞至后台线程写完全部已入队内容(退出前 flush)。
    /// 用 `tracing::subscriber::with_default`(线程局部)而非 `.init()`(方案 §9.2 陷阱表:
    /// 全局默认只能设一次,同进程第二个用例会 panic)。
    #[test]
    fn off_directive_stops_writes_then_resumes_and_flushes_on_guard_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::NEVER)
            .filename_prefix("test")
            .filename_suffix("log")
            .build(dir.path())
            .expect("build rolling appender");
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);

        let env_filter = tracing_subscriber::EnvFilter::new("info");
        let (filter, handle) = tracing_subscriber::reload::Layer::new(env_filter);
        let session_id: Arc<str> = Arc::from("s-test");
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(EnvelopeFormat { session_id })
            .with_ansi(false)
            .with_writer(non_blocking);
        let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("marker-before-off");

            handle
                .modify(|f| *f = tracing_subscriber::EnvFilter::new("off"))
                .expect("switch to off");
            tracing::info!("marker-during-off");

            handle
                .modify(|f| *f = tracing_subscriber::EnvFilter::new("info"))
                .expect("switch back to info");
            tracing::info!("marker-after-resume");
        });

        // guard 的 Drop 会 flush 并 join 后台写线程,drop 之后文件内容才是终态。
        drop(guard);

        let log_path = dir.path().join("test.log");
        let content = std::fs::read_to_string(&log_path).expect("read log file");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();

        assert!(
            content.contains("marker-before-off"),
            "off 前的事件应已落盘: {content}"
        );
        assert!(
            content.contains("marker-after-resume"),
            "切回 info 后的事件应已落盘(验证「切回恢复」+「退出 flush」): {content}"
        );
        assert!(
            !content.contains("marker-during-off"),
            "off 档期间的事件不应触达写入器(验证「切 off 文件停增」): {content}"
        );
        assert_eq!(lines.len(), 2, "off 档应恰好拦下中间那一条: {content}");
    }

    fn write_fake_log(dir: &Path, name: &str, bytes: usize, age_secs_ago: u64) {
        let path = dir.join(name);
        std::fs::write(&path, vec![b'x'; bytes]).expect("write fake log");
        let mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs_ago);
        // 用 FileTimes 显式打旧 mtime,不然全部文件同一秒创建、排序无法验证「按最旧删」。
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("reopen for mtime");
        file.set_times(std::fs::FileTimes::new().set_modified(mtime))
            .expect("set mtime");
    }

    #[test]
    fn enforce_size_budget_noop_when_under_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fake_log(dir.path(), "scrollery.2026-07-18.log", 100, 200);
        write_fake_log(dir.path(), "scrollery.2026-07-19.log", 100, 100);

        let summary = enforce_size_budget(dir.path(), 1_000);
        assert_eq!(summary, PurgeSummary::default(), "总量未超限不应删任何文件");
        assert!(dir.path().join("scrollery.2026-07-18.log").exists());
        assert!(dir.path().join("scrollery.2026-07-19.log").exists());
    }

    #[test]
    fn enforce_size_budget_deletes_oldest_first_until_under_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 最旧(200s 前)200B、次旧(100s 前)200B、最新(10s 前,模拟当前活跃文件)200B,总量 600B、
        // 上限 450B——删掉最旧一个后总量降到 400B(≤450),即回落到位;次旧+最新必须保留
        // (尤其最新,验证「活跃文件不被误删」)。
        write_fake_log(dir.path(), "scrollery.2026-07-18.log", 200, 200);
        write_fake_log(dir.path(), "scrollery.2026-07-19.log", 200, 100);
        write_fake_log(dir.path(), "scrollery.2026-07-20.log", 200, 10);

        let summary = enforce_size_budget(dir.path(), 450);

        assert_eq!(summary.purged_files, 1, "应恰好删掉最旧那一个");
        assert_eq!(summary.freed_bytes, 200);
        assert!(
            !dir.path().join("scrollery.2026-07-18.log").exists(),
            "最旧文件应被删除"
        );
        assert!(
            dir.path().join("scrollery.2026-07-19.log").exists(),
            "次旧文件应保留"
        );
        assert!(
            dir.path().join("scrollery.2026-07-20.log").exists(),
            "最新(活跃)文件不应被误删"
        );
    }

    #[test]
    fn enforce_size_budget_ignores_non_log_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fake_log(dir.path(), "scrollery.2026-07-20.log", 100, 10);
        std::fs::write(dir.path().join("not-a-log.txt"), vec![b'x'; 10_000])
            .expect("write other file");

        let summary = enforce_size_budget(dir.path(), 50);

        assert_eq!(
            summary.purged_files, 1,
            "只统计/清理 .log 文件,不理会同目录其它文件"
        );
        assert!(
            dir.path().join("not-a-log.txt").exists(),
            "非 .log 文件不受影响"
        );
    }

    /// 锁住 pipeline target 归一的 directive 拼接规则(方案 §3.4/S2):非 off 档追加降噪后缀;
    /// off 档必须保持纯 "off"(§9.1 护栏 #7,追加后缀会让 pipeline target 在 off 态仍以 warn 放行)。
    #[test]
    fn build_env_filter_directive_appends_pipeline_suffix_except_off() {
        assert_eq!(
            build_env_filter_directive("trace"),
            "trace,scrollery::pipeline=warn,reqwest=warn"
        );
        assert_eq!(
            build_env_filter_directive("debug"),
            "debug,scrollery::pipeline=warn,reqwest=warn"
        );
        assert_eq!(
            build_env_filter_directive("info"),
            "info,scrollery::pipeline=warn,reqwest=warn"
        );
        assert_eq!(build_env_filter_directive("off"), "off");
        assert_eq!(build_env_filter_directive("OFF"), "off", "大小写不敏感");
    }

    /// reviewer 深审揪出的真实 bug(方案 §9.5 每期收口前置审查):warn/error 档不该被无脑追加
    /// `scrollery::pipeline=warn` 后缀——EnvFilter 按 target 特异性择优、不比较宽松/严格,用户选
    /// error 时追加 warn 后缀反而让 pipeline target 比用户选择更啰嗦。
    #[test]
    fn build_env_filter_directive_does_not_loosen_warn_or_error() {
        assert_eq!(
            build_env_filter_directive("warn"),
            "warn",
            "warn 本就等于阈值,不应重复追加"
        );
        assert_eq!(
            build_env_filter_directive("error"),
            "error",
            "error 比 warn 更严格,追加后缀会变啰嗦"
        );
    }

    /// UI 环形缓冲层(方案 §9.3-D/S4):无订阅者时零广播——验证「不格式化不入队」不是文档空谈。
    #[test]
    fn ring_buffer_layer_drops_events_when_unsubscribed() {
        use tracing_subscriber::layer::SubscriberExt;

        let ring = LogRingBuffer::new(Arc::from("s-test"), RING_BUFFER_CAPACITY);
        let subscriber = tracing_subscriber::registry().with(RingBufferLayer::new(ring.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("nobody listening");
        });
        assert!(ring.drain().is_none(), "未订阅期间的事件不应进入环形缓冲");
    }

    /// 订阅后事件入队、可抽干;抽干后队列清空,再抽一次返回 None(不重复吐出同一批)。
    #[test]
    fn ring_buffer_layer_buffers_and_drains_when_subscribed() {
        use tracing_subscriber::layer::SubscriberExt;

        let ring = LogRingBuffer::new(Arc::from("s-test"), RING_BUFFER_CAPACITY);
        ring.set_subscribed(true);
        let subscriber = tracing_subscriber::registry().with(RingBufferLayer::new(ring.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(operation_id = "op-1", "first");
            tracing::warn!("second");
        });

        let drained = ring.drain().expect("订阅期间应有事件入队");
        assert_eq!(drained.len(), 2, "两条事件都应入队: {drained:?}");
        assert_eq!(drained[0]["msg"], "first");
        assert_eq!(drained[0]["operation_id"], "op-1");
        assert_eq!(drained[1]["level"], "WARN");
        assert!(ring.drain().is_none(), "抽干后队列应为空,不重复吐出");
    }

    /// 取消订阅(`set_subscribed(false)`)清空残留缓冲——防止「关窗前的旧批」在下次重新订阅时抢跑。
    #[test]
    fn ring_buffer_unsubscribe_clears_pending_buffer() {
        use tracing_subscriber::layer::SubscriberExt;

        let ring = LogRingBuffer::new(Arc::from("s-test"), RING_BUFFER_CAPACITY);
        ring.set_subscribed(true);
        let subscriber = tracing_subscriber::registry().with(RingBufferLayer::new(ring.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("queued before unsubscribe");
        });

        ring.set_subscribed(false);
        assert!(
            ring.drain().is_none(),
            "取消订阅应清空残留缓冲,不留到下次订阅"
        );
    }

    /// 环形缓冲有界(方案 §9.3-D「cap=环形上限」安全阀):超容量按 FIFO 丢最旧。用小容量场景不现实
    /// (常量是编译期定值),故直接对 `RING_BUFFER_CAPACITY` 做温和的行为验证——push 超量后长度钳制、
    /// 保留的是最新一批(丢弃可见化精神的镜像:不测试具体丢了多少,只测「不越界」+「留的是新的」)。
    #[test]
    fn ring_buffer_caps_length_and_keeps_newest() {
        use tracing_subscriber::layer::SubscriberExt;

        let ring = LogRingBuffer::new(Arc::from("s-test"), RING_BUFFER_CAPACITY);
        ring.set_subscribed(true);
        let subscriber = tracing_subscriber::registry().with(RingBufferLayer::new(ring.clone()));
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..(RING_BUFFER_CAPACITY + 10) {
                tracing::info!(seq = i as u64, "spam");
            }
        });

        let drained = ring.drain().expect("应有事件入队");
        assert_eq!(drained.len(), RING_BUFFER_CAPACITY, "长度应钳制在容量上限");
        assert_eq!(
            drained.last().unwrap()["attributes"]["seq"],
            (RING_BUFFER_CAPACITY + 9) as u64,
            "保留的应是最新一批,不是最旧的"
        );
    }

    /// 诊断包脱敏(方案 §5 P2):Windows 用户目录的用户名段被替换,前后缀(盘符+`\Users\`)保留,
    /// 路径其余部分(非用户名段)不受影响。
    #[test]
    fn redact_diagnostics_text_masks_windows_username() {
        let (redacted, hits) =
            redact_diagnostics_text(r"failed to read C:\Users\gf\Documents\photo.jpg");
        assert_eq!(hits, 1);
        assert_eq!(redacted, r"failed to read C:\Users\***\Documents\photo.jpg");
    }

    /// Unix 风格 /home/<user> 与 macOS /Users/<user> 均命中;大小写不敏感(`(?i)`)。
    #[test]
    fn redact_diagnostics_text_masks_unix_and_macos_username() {
        let (redacted, hits) = redact_diagnostics_text("/home/alice/scan.log /Users/Bob/lib.rs");
        assert_eq!(hits, 2);
        assert_eq!(redacted, "/home/***/scan.log /Users/***/lib.rs");
    }

    /// 无用户路径的普通文本不应产生任何替换,命中数为 0(不误伤)。
    #[test]
    fn redact_diagnostics_text_leaves_plain_text_untouched() {
        let (redacted, hits) = redact_diagnostics_text("scan completed, 42 items processed");
        assert_eq!(hits, 0);
        assert_eq!(redacted, "scan completed, 42 items processed");
    }

    /// 回归测试(reviewer 深审 2026-07-20 发现的严重 bug):早期实现直接对序列化后的 JSONL
    /// 字节跑正则,反斜杠经 JSON 转义(`C:\Users\gf` 落盘为字面 `C:\\Users\\gf`,双写)后
    /// 完全不命中,诊断包里 Windows 用户名不会被脱敏——上面三条早期单测只喂了未转义的裸字符串,
    /// 没覆盖这里用真实 `serde_json::to_string` 产出的数据形态,给出了假绿灯。修复后对**解码后**
    /// 的值脱敏、再重新序列化,此测锁定该修复。
    #[test]
    fn redact_diagnostics_text_masks_username_inside_real_jsonl_line() {
        let line = serde_json::json!({
            "ts": "2026-07-20T21:00:00.000+08:00",
            "level": "WARN",
            "target": "t",
            "session_id": "s",
            "operation_id": null,
            "msg": "msg",
            "attributes": { "path": r"C:\Users\gf\Documents\photo.jpg" },
        })
        .to_string();
        assert!(
            line.contains(r"C:\\Users\\gf"),
            "前置条件:序列化文本里的反斜杠应是双写转义,不是单个: {line}"
        );

        let (redacted, hits) = redact_diagnostics_text(&line);
        assert_eq!(hits, 1);
        let v: serde_json::Value = serde_json::from_str(&redacted).expect("脱敏后仍应是合法 JSON");
        assert_eq!(v["attributes"]["path"], r"C:\Users\***\Documents\photo.jpg");
    }

    /// 混合输入:能解析的 JSONL 行走值树脱敏,解析失败的行(史前纯文本)按原文直接跑正则——
    /// 两条分支都要生效,且行数/顺序不因脱敏而错乱。
    #[test]
    fn redact_diagnostics_text_handles_mixed_jsonl_and_plain_lines() {
        let jsonl_line =
            serde_json::json!({ "msg": r"C:\Users\gf\a.jpg", "level": "INFO" }).to_string();
        let input = format!("{jsonl_line}\nplain text line C:\\Users\\bob\\b.jpg\n");

        let (redacted, hits) = redact_diagnostics_text(&input);
        assert_eq!(hits, 2);
        let lines: Vec<&str> = redacted.lines().collect();
        assert_eq!(lines.len(), 2);
        let v: serde_json::Value = serde_json::from_str(lines[0]).expect("首行仍应是合法 JSON");
        assert_eq!(v["msg"], r"C:\Users\***\a.jpg");
        assert!(
            lines[1].contains(r"C:\Users\***\b.jpg"),
            "次行(非 JSON)应走原文正则分支: {}",
            lines[1]
        );
    }

    // ── SpanTimer(span 埋点线 W1,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入)──

    /// 同步内存 writer:把 `fmt::Layer` 输出直接攒进 `Arc<Mutex<Vec<u8>>>`,免去
    /// `non_blocking` + `WorkerGuard` 的异步落盘/等待(现有 off 档测试那套是验「真落盘」;这里
    /// 只关心「事件字段形状对不对」,同步捕获更直接、也不必在每个用例末尾 drop guard)。
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

    /// 建一条「EnvelopeFormat 文件层 + 给定 EnvFilter directive」的 subscriber,输出同步攒进
    /// 返回的 buffer——调用方在 `tracing::subscriber::with_default` 作用域内跑被测代码,
    /// 结束后直接读 buffer(无需 guard/drop 时序)。
    fn span_test_subscriber(
        directive: &str,
    ) -> (
        impl tracing::Subscriber + for<'a> LookupSpan<'a>,
        Arc<Mutex<Vec<u8>>>,
    ) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_for_writer = buf.clone();
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(EnvelopeFormat {
                session_id: Arc::from("s-test"),
            })
            .with_ansi(false)
            .with_writer(move || SharedBuf(buf_for_writer.clone()));
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(directive))
            .with(file_layer);
        (subscriber, buf)
    }

    fn first_json_line(buf: &Arc<Mutex<Vec<u8>>>) -> serde_json::Value {
        let content = String::from_utf8(buf.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .expect("captured bytes are valid utf8");
        let line = content
            .lines()
            .next()
            .unwrap_or_else(|| panic!("expected at least one emitted line, got: {content:?}"));
        serde_json::from_str(line).unwrap_or_else(|e| panic!("line not valid JSON: {e}: {line}"))
    }

    /// info 档 `SpanTimer` Drop 后:信封六字段齐全(ts/level/target/session_id/operation_id/msg),
    /// `attributes.span_name`/`attributes.duration_ms` 存在且 `duration_ms` 是数字。
    #[test]
    fn span_timer_info_emits_full_envelope_with_span_name_and_duration() {
        let (subscriber, buf) = span_test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            let _span = SpanTimer::info("test:span-a");
        });

        let v = first_json_line(&buf);
        assert!(v
            .get("ts")
            .is_some_and(|t| t.is_string() && !t.as_str().unwrap().is_empty()));
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["target"], "scrollery::span");
        assert_eq!(v["session_id"], "s-test");
        assert_eq!(v["msg"], "span_close");
        assert_eq!(v["attributes"]["span_name"], "test:span-a");
        assert!(
            v["attributes"]["duration_ms"].is_number(),
            "duration_ms 应为数字: {v}"
        );
        assert_eq!(v["attributes"]["panicked"], false);
    }

    /// `with_operation_id` 提升进信封顶层 `operation_id` 字段,不落进 `attributes`
    /// (`FieldCollector` 对字段名 `operation_id` 的既有特判,§9.3-A 信封契约)。
    #[test]
    fn span_timer_operation_id_lifts_into_envelope_not_attributes() {
        let (subscriber, buf) = span_test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            let _span =
                SpanTimer::info("test:span-op").with_operation_id(Some("op-42".to_string()));
        });

        let v = first_json_line(&buf);
        assert_eq!(v["operation_id"], "op-42");
        assert!(
            v["attributes"].get("operation_id").is_none(),
            "operation_id 不应重复出现在 attributes 里: {v}"
        );
    }

    /// debug 档 `SpanTimer` 在 info 级 EnvFilter 下产生零输出(D-312:默认 info 档不被热路径
    /// 查询刷屏)。
    #[test]
    fn span_timer_debug_produces_no_output_under_info_filter() {
        let (subscriber, buf) = span_test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            let _span = SpanTimer::debug("test:span-debug");
        });

        let content =
            String::from_utf8(buf.lock().unwrap_or_else(|e| e.into_inner()).clone()).expect("utf8");
        assert!(
            content.trim().is_empty(),
            "info 档过滤下 debug span 不应有任何输出: {content}"
        );
    }

    /// panic 路径:`SpanTimer` 在栈展开期间(`catch_unwind` 边界内)被 Drop 时,
    /// `std::thread::panicking()` 为真,事件的 `attributes.panicked` 须为 `true`。
    /// 临时静默默认 panic hook 只为不污染测试输出,不影响 `catch_unwind` 本身的捕获行为。
    #[test]
    fn span_timer_panicked_flag_true_when_dropped_during_unwind() {
        let (subscriber, buf) = span_test_subscriber("info");
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        tracing::subscriber::with_default(subscriber, || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _span = SpanTimer::info("test:span-panic");
                panic!("boom");
            }));
        });
        std::panic::set_hook(prev_hook);

        let v = first_json_line(&buf);
        assert_eq!(
            v["attributes"]["panicked"], true,
            "展开期间 Drop 应记录 panicked=true: {v}"
        );
    }

    // ── worker stderr 转发桥(阶段 3 · W4)FieldCollector 特判 ─────────────────────────

    /// worker stderr 行转发桥(阶段 3 · W4,`exotic::worker_log::emit_worker_log_line`)的
    /// `worker_context` 字段:模拟转发调用点的编码姿态(JSON 文本传入),断言信封六字段齐全 +
    /// `attributes.context` 解回真对象(不是原样字符串)+ `attributes.worker` 在场。
    /// scoped `with_default`(禁 `set_global_default`,方案 §9.2 陷阱表)。
    #[test]
    fn worker_context_field_decodes_into_attributes_context_object() {
        let (subscriber, buf) = span_test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            let fields_json = serde_json::json!({"session_id": 9, "req_id": "r-1"}).to_string();
            tracing::info!(
                target: "scrollery::worker",
                worker = "ai",
                worker_context = fields_json.as_str(),
                "会话就绪"
            );
        });

        let v = first_json_line(&buf);
        // 信封六字段(§3.3):ts/level/target/session_id/operation_id/msg。
        assert!(v
            .get("ts")
            .is_some_and(|t| t.is_string() && !t.as_str().unwrap().is_empty()));
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["target"], "scrollery::worker");
        assert_eq!(v["session_id"], "s-test");
        assert!(v.get("operation_id").is_some_and(|o| o.is_null()));
        assert_eq!(v["msg"], "会话就绪");
        // worker_context 解回真对象(不是原样字符串),落在 attributes.context——同 frontend_context 惯例。
        assert!(
            v["attributes"]["context"].is_object(),
            "worker_context 应解回真对象: {v}"
        );
        assert_eq!(v["attributes"]["context"]["session_id"], 9);
        assert_eq!(v["attributes"]["context"]["req_id"], "r-1");
        // worker 字段在场(区分 ai/psd 来源)。
        assert_eq!(v["attributes"]["worker"], "ai");
    }

    /// `worker_context` 值不是合法 JSON 时的兜底:原样字符串存进 `attributes.context`
    /// (同 `error_chain`/`frontend_context` 既有兜底惯例,不 panic、不丢字段)。
    #[test]
    fn worker_context_field_falls_back_to_raw_string_on_invalid_json() {
        let (subscriber, buf) = span_test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(
                target: "scrollery::worker",
                worker = "psd",
                worker_context = "not valid json",
                "解析失败兜底"
            );
        });

        let v = first_json_line(&buf);
        assert_eq!(v["attributes"]["context"], "not valid json");
    }
}
