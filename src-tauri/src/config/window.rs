//! 桌面窗口几何持久化(设计 §6)。
//!
//! 窗口位置、**正常**尺寸、采集时 scale factor 与最大化状态统一写进 config.toml 的 window_geometry
//! 键(按窗口 label 分别记录),取代退役的 tauri-plugin-window-state —— 后者是独立文件 + 独立读写
//! 路径,既不参与恢复默认设置,也让窗口信息有第二个持久化出口。
//!
//! # 本文件与配置核心的分工
//! 窗口模块负责采集/恢复窗口位置与尺寸,配置核心负责存储与串行写盘。两者之间只有一层薄缝:
//!
//! - 读:窗口模块经 ConfigManager 读 window_geometry 的当前生效值(规范 JSON 文本),
//!   文件结构见 schema 的 WINDOW_GEOMETRY 字段定义(x/y/width/height/scaleFactor/maximized)。
//! - 写:窗口模块走 config::settings::submit_window_geometry,由配置核心在写锁内**只合并本窗口
//!   的 label**,其他窗口的几何不被覆盖;generation 过期的提交被拒。本模块不自建第二 writer,
//!   也不绕过核心的 async 串行门与统一广播。
//! - 重置:配置核心在 clear_settings 编排里按顺序调用本模块的三个钩子(见下)。
//!
//! # 三个重置钩子的契约
//! 1. suspend_for_reset:在默认模板**生成之前**调用——暂停几何采集并丢弃待保存值。若不停,
//!    重置瞬间的迟到几何会把用户旧位置写回刚重置的文件里。
//! 2. apply_default_geometry:写盘**成功之后**调用——常驻窗口回到创建默认几何(程序化落位,
//!    不写回配置,否则「恢复默认」立刻又变成用户设置)。
//! 3. resume_after_reset:成功与失败都要调用(finally 语义)——不能因为一次重置失败就让几何
//!    采集永久停摆。配置核心在 reset_settings 里已按此顺序调用。
//!
//! # 口径(物理像素,**按派工**:物理正常边界 + scale factor)
//! - x/y = 窗口**外框**左上角物理坐标,采集自 outer_position、恢复走 set_position
//!   (屏幕坐标本就是物理量)。
//! - width/height = **客户区**(inner)物理尺寸,采集自 inner_size、恢复走 set_size ——
//!   与 tauri.conf.json 的 width/height 及 min 尺寸约束同一口径(它们都是 inner 尺寸)。
//!   若一边采外框尺寸(outer_size)、一边用 set_size(inner)恢复,每次重启都会膨胀一圈内边距。
//! - scale_factor = 采集时窗口的 scale factor。跨 DPI 恢复时:逻辑尺寸 = 物理尺寸 / 采集 scale,
//!   目标物理尺寸 = 逻辑尺寸 × 目标屏 scale;位置保持原物理坐标。
//!
//! # 采集与失败语义
//! 空闲 500ms 提交、连续拖拽最长 2s 提交一次(设计 §5.2),一次提交只落盘一次。
//! visible 与最小化状态不持久化(最小化/全屏期间直接跳过采集);最大化时用**最后已知的正常边界**
//! 配 maximized 标志,避免用最大化尺寸覆盖还原尺寸。程序化落位(启动恢复、重置恢复)触发的
//! move/resize 属回声,不当作新的用户设置写回。
//! 写盘失败**不吞错**(设计 §5.4):待保存值原样留在待保存集合里等下次重试,并向 flush 调用方
//! 返回错误,由上层决定重试或明确放弃。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 主窗口 label(与 tauri.conf.json 一致)。
pub const MAIN_LABEL: &str = "main";
/// 独立日志窗口 label(与 ipc::log_commands::open_log_window 一致)。
pub const LOGS_LABEL: &str = "logs";

/// 实际持久化几何的常驻窗口(设计 §6「按实际持久化窗口 label 注册」,与 schema 的
/// WINDOW_LABELS 注册表一致;临时窗口不写进配置)。
const REGISTERED_LABELS: &[&str] = &[MAIN_LABEL, LOGS_LABEL];

/// config.toml 里承载窗口几何的设置键。
pub const SETTINGS_KEY: &str = "window_geometry";

/// 空闲多久提交一次(设计 §5.2 首版参数)。
const IDLE_COMMIT_MS: u64 = 500;
/// 连续调整的最长等待(同上)。
const MAX_COMMIT_MS: u64 = 2_000;
/// 提交失败后的重试间隔(失败批次已还原,等这么久再试,避免立刻打转)。
const RETRY_BACKOFF: Duration = Duration::from_secs(2);
/// **后台自动重试**的次数上限:只用来避免「恒定失败 → 每 500ms 写盘 + 广播」的空转。
///
/// 达到上限的待保存值**不会被丢弃**(那会让随后的退出 flush 在空集合上报告 saved,而用户的
/// 窗口位置其实一次都没存进去);它只是不再被后台定时器反复重试,仍然保留在待保存集合里,
/// 由显式 flush(退出/关窗/重置)继续尝试,并如实按失败上报。
const MAX_AUTO_RETRIES: u32 = 2;
/// 判定「仍算可见」的最小交集边长(逻辑像素);更小即视为离屏,回可见屏幕居中。
const MIN_VISIBLE_LOGICAL: f64 = 64.0;

/// 已注册的持久化窗口 label(供生命周期与重置流程遍历)。
pub fn registered_labels() -> &'static [&'static str] {
    REGISTERED_LABELS
}

/// 某窗口的最小尺寸(客户区口径):与 tauri.conf.json / open_log_window 的创建下限同源,
/// 恢复时不允许把窗口还原到比创建默认值更小。
pub fn min_size(label: &str) -> (f64, f64) {
    match label {
        LOGS_LABEL => (640.0, 420.0),
        _ => (800.0, 560.0),
    }
}

/// 某窗口的创建默认几何:缺失记录、显示器消失或重置后按它恢复。
/// 主窗口 1280×820、日志窗口 960×640,与两处创建默认值同源。
/// 默认值的 x/y 无意义(缺记录时按目标屏居中计算),保留字段只为结构统一。
pub fn default_geometry(label: &str) -> WindowGeometry {
    match label {
        LOGS_LABEL => WindowGeometry {
            x: 0.0,
            y: 0.0,
            width: 960.0,
            height: 640.0,
            scale_factor: 1.0,
            maximized: false,
        },
        _ => WindowGeometry {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 820.0,
            scale_factor: 1.0,
            maximized: false,
        },
    }
}

// ── 数据与纯几何 ──────────────────────────────────────────────────────────────

/// 单个窗口的几何记录(逻辑像素;口径见模块头注)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
    /// 是否处于最大化。最大化不影响上面的正常边界。
    pub maximized: bool,
}

impl WindowGeometry {
    /// 结构合法:全部字段有限、宽高与 scale 为正。非有限值(NaN/无穷)一律拒绝 ——
    /// 它们既不能写进 TOML,也无法参与几何计算。
    fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.scale_factor.is_finite()
            && self.width > 0.0
            && self.height > 0.0
            && self.scale_factor > 0.0
    }

    /// 与另一份记录在几何上等价(采集回声判定用):位置、尺寸与 scale 对齐到 0.01 像素,
    /// 最大化标志按位比较。
    fn same_placement(&self, other: &WindowGeometry) -> bool {
        self.maximized == other.maximized
            && (self.x - other.x).abs() < 0.01
            && (self.y - other.y).abs() < 0.01
            && (self.width - other.width).abs() < 0.01
            && (self.height - other.height).abs() < 0.01
            && (self.scale_factor - other.scale_factor).abs() < 0.001
    }
}

/// label → 几何记录。
pub type GeometryMap = BTreeMap<String, WindowGeometry>;

/// 物理像素矩形(显示器边界与工作区)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 一块显示器的物理边界 + 工作区 + scale factor。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub bounds: Rect,
    /// 去掉任务栏等的可用区域;尺寸校验与居中用它,可见性判定用 bounds。
    pub work: Rect,
    pub scale_factor: f64,
    pub primary: bool,
}

/// 解析后的落位:**物理像素**坐标尺寸 + 目标 DPI。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
    pub maximized: bool,
}

impl Placement {
    /// 换算回几何记录(物理口径,与采集值同型):回声抑制比对与「最后已知正常边界」都用它。
    fn to_geometry(self) -> WindowGeometry {
        WindowGeometry {
            x: round2(self.x),
            y: round2(self.y),
            width: round2(self.width),
            height: round2(self.height),
            scale_factor: round2(self.scale_factor),
            maximized: self.maximized,
        }
    }
}

/// 保留两位小数:落盘值稳定(同一位置多次采集得到同一串),避免浮点尾巴污染配置与比对。
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 解析规范 JSON 为几何表。单条非法(未知结构、非有限值、宽高非正)**只丢该条**并告警,
/// 其余条目照常使用(设计 §5.4:不因一条坏记录拒绝整份配置)。
pub fn parse_map(raw: &str) -> GeometryMap {
    let mut out = GeometryMap::new();
    let parsed: BTreeMap<String, WindowGeometry> = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("window_geometry 不是合法的目标结构,本次按默认窗口几何处理 | {e}");
            return out;
        }
    };
    for (label, geometry) in parsed {
        if geometry.is_valid() {
            out.insert(label, geometry);
        } else {
            tracing::warn!("window_geometry 中 {label} 的几何值非法,已忽略该条(回退默认值)");
        }
    }
    out
}

/// 计算既定记录的落位:
/// - 目标屏 = 采集时的物理盒与各屏交集最大者;全无交集时取 scale factor 最接近的屏
///   (换显示器或改 DPI 后的合理落点),再退主屏。
/// - 可见性达标(交集在两个方向都不小于 64 逻辑像素)时**保留原物理位置**;离屏(显示器被拔掉、
///   窗口完全在屏幕外)则按目标屏工作区居中。
/// - 尺寸按**目标屏 DPI**换算,并夹在 [该窗口最小尺寸, 目标屏工作区] 之间 —— 最小尺寸优先于
///   屏幕装得下,窗口不可能被还原到比创建默认值还小。
/// - 最大化标志原样透传(位置与尺寸仍是正常边界,再由调用方 maximize)。
pub fn resolve_placement(
    stored: Option<&WindowGeometry>,
    default: &WindowGeometry,
    min: (f64, f64),
    monitors: &[MonitorRect],
) -> Placement {
    if monitors.is_empty() {
        // 拿不到显示器信息(平台不支持或取用异常):按记录原样使用,不做居中与裁剪。
        let g = stored.unwrap_or(default);
        let scale = if g.scale_factor > 0.0 {
            g.scale_factor
        } else {
            1.0
        };
        return Placement {
            x: g.x,
            y: g.y,
            width: g.width,
            height: g.height,
            scale_factor: scale,
            maximized: g.maximized,
        };
    }

    match stored {
        None => centered_on(
            &monitors[pick_primary_index(monitors)],
            default.width,
            default.height,
            min,
            false,
        ),
        Some(g) => {
            let stored_scale = if g.scale_factor > 0.0 {
                g.scale_factor
            } else {
                1.0
            };
            // 采集时的物理盒(位置:外框;尺寸:客户区)。
            let captured = Rect {
                x: g.x,
                y: g.y,
                width: g.width,
                height: g.height,
            };
            let monitor = &monitors[pick_monitor_index(&captured, stored_scale, monitors)];
            // 尺寸判据统一在**逻辑**尺度上算(下限来自创建默认值、上限来自目标屏工作区),
            // 再换算成目标 DPI 的物理尺寸;位置保持原物理坐标。
            let logical = (g.width / stored_scale, g.height / stored_scale);
            let size = clamp_logical(logical, min, monitor);
            let restored = Rect {
                x: g.x,
                y: g.y,
                width: size.0 * monitor.scale_factor,
                height: size.1 * monitor.scale_factor,
            };
            if visible_enough(&restored, monitor) {
                Placement {
                    x: restored.x,
                    y: restored.y,
                    width: restored.width,
                    height: restored.height,
                    scale_factor: monitor.scale_factor,
                    maximized: g.maximized,
                }
            } else {
                centered_on(monitor, size.0, size.1, min, g.maximized)
            }
        }
    }
}

/// 逻辑尺寸夹取:不低于该窗口的创建下限,不超过目标屏工作区(下限优先于「屏幕装得下」)。
fn clamp_logical(size: (f64, f64), min: (f64, f64), monitor: &MonitorRect) -> (f64, f64) {
    let cap = (
        monitor.work.width / monitor.scale_factor,
        monitor.work.height / monitor.scale_factor,
    );
    (
        clamp(size.0, min.0, cap.0.max(min.0)),
        clamp(size.1, min.1, cap.1.max(min.1)),
    )
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

/// 两块矩形的交面积。
fn intersection_area(a: &Rect, b: &Rect) -> f64 {
    let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if w <= 0.0 || h <= 0.0 {
        0.0
    } else {
        w * h
    }
}

/// 目标屏选择:交集面积最大者;全无交集时退 scale factor 最接近的屏(相同则主屏优先)。
/// scale 为**采集时**的 scale factor(只用于回退屏的近似匹配)。
fn pick_monitor_index(captured: &Rect, scale: f64, monitors: &[MonitorRect]) -> usize {
    let mut best: Option<(usize, f64)> = None;
    for (i, m) in monitors.iter().enumerate() {
        let area = intersection_area(captured, &m.bounds);
        if area > 0.0 && best.map(|(_, a)| area > a).unwrap_or(true) {
            best = Some((i, area));
        }
    }
    match best {
        Some((i, _)) => i,
        None => pick_fallback_index(monitors, scale),
    }
}

/// 回退屏:scale factor 最接近者,并列时主屏优先,再退第一块。
fn pick_fallback_index(monitors: &[MonitorRect], scale: f64) -> usize {
    let mut best = 0usize;
    let mut best_delta = f64::MAX;
    for (i, m) in monitors.iter().enumerate() {
        let delta = (m.scale_factor - scale).abs();
        let closer = delta < best_delta - f64::EPSILON;
        let tie_primary =
            (delta - best_delta).abs() <= f64::EPSILON && m.primary && !monitors[best].primary;
        if closer || tie_primary {
            best = i;
            best_delta = delta;
        }
    }
    best
}

/// 缺记录时的落点基准:主屏(拿不到主屏标记就退第一块)。默认值本就该出现在用户最可能看到的
/// 那块屏上,与「DPI 恰好接近默认 scale」无关。
fn pick_primary_index(monitors: &[MonitorRect]) -> usize {
    monitors.iter().position(|m| m.primary).unwrap_or(0)
}

/// 交集在两个方向都不小于 64 逻辑像素(或窗口本身更小)才算仍可见 ——
/// 窗口至少要露出一条能抓回来的边,否则视为离屏。
fn visible_enough(restored: &Rect, monitor: &MonitorRect) -> bool {
    let w = (restored.x + restored.width).min(monitor.bounds.x + monitor.bounds.width)
        - restored.x.max(monitor.bounds.x);
    let h = (restored.y + restored.height).min(monitor.bounds.y + monitor.bounds.height)
        - restored.y.max(monitor.bounds.y);
    let need = MIN_VISIBLE_LOGICAL * monitor.scale_factor;
    w >= need.min(restored.width) && h >= need.min(restored.height)
}

/// 在目标屏工作区居中(入参为**逻辑**尺寸),返回物理落位。
fn centered_on(
    monitor: &MonitorRect,
    width: f64,
    height: f64,
    min: (f64, f64),
    maximized: bool,
) -> Placement {
    let size = clamp_logical((width, height), min, monitor);
    let w = size.0 * monitor.scale_factor;
    let h = size.1 * monitor.scale_factor;
    Placement {
        x: monitor.work.x + (monitor.work.width - w) / 2.0,
        y: monitor.work.y + (monitor.work.height - h) / 2.0,
        width: w,
        height: h,
        scale_factor: monitor.scale_factor,
        maximized,
    }
}

// ── 提交目标(配置核心的统一入口)────────────────────────────────────────────

/// 几何提交失败。
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum GeometryCommitError {
    #[error("窗口几何提交被拒绝(属重置前的旧 generation) | window geometry commit rejected: stale generation")]
    StaleGeneration,
    #[error("窗口几何写入失败 | window geometry write failed: {0}")]
    Failed(String),
}

/// 窗口几何**应用**失败(文件已落盘,只是窗口没被搬到目标位置)。
///
/// 与 GeometryCommitError 分开:这类失败不是写盘失败(值已保存,不得回滚、不得报保存失败),
/// 也不能当成完全成功(窗口此刻仍在旧位置)。调用方(如恢复默认设置)据此把 window_geometry 记进
/// apply_failed,让用户看到「默认值已保存,窗口位置需重启或重试才生效」。
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum GeometryApplyError {
    #[error("窗口 {label} 的几何未能应用 | window geometry apply failed for {label}: {reason}")]
    Window { label: String, reason: String },
}

/// 几何提交目标:生产走配置核心的统一编排入口,测试用可控假实现。
///
/// 只在两条分支间切换,不对外暴露 trait,也不提供通用注入点 —— 窗口侧只有 install 一个接线点。
enum Sink {
    /// 生产:config::settings::submit_window_geometry(核心的 async 串行门 + 写锁内按 label 合并
    /// + 统一广播)。本模块不直接碰文件,故不存在第二个 writer。
    Core {
        app: tauri::AppHandle,
        state: Arc<crate::state::AppState>,
    },
    /// 仅测试。
    #[cfg(test)]
    Test(Arc<TestSink>),
}

impl Sink {
    /// 读 window_geometry 的**内存**生效值(零 IO;采集路径也会调它)。
    fn load(&self) -> Option<String> {
        match self {
            Sink::Core { state, .. } => state.config.get(SETTINGS_KEY),
            #[cfg(test)]
            Sink::Test(sink) => sink.loaded.lock().unwrap().clone(),
        }
    }

    /// 当前配置 generation(重置后递增)。
    fn generation(&self) -> u64 {
        match self {
            Sink::Core { state, .. } => state.config.generation(),
            #[cfg(test)]
            Sink::Test(sink) => *sink.generation.lock().unwrap(),
        }
    }

    /// 提交这批 label 的几何(逐 label 合并写盘,由核心统一广播)。
    async fn commit(
        &self,
        generation: u64,
        entries: Vec<(String, WindowGeometry)>,
    ) -> Result<(), GeometryCommitError> {
        match self {
            Sink::Core { app, state } => {
                for (label, geometry) in entries {
                    let json = serde_json::to_string(&geometry)
                        .map_err(|e| GeometryCommitError::Failed(e.to_string()))?;
                    super::settings::submit_window_geometry(app, state, &label, &json, generation)
                        .await
                        .map_err(map_submit_error)?;
                }
                Ok(())
            }
            #[cfg(test)]
            Sink::Test(sink) => {
                if let Some(error) = sink.fail.lock().unwrap().clone() {
                    return Err(error);
                }
                sink.commits.lock().unwrap().push((generation, entries));
                Ok(())
            }
        }
    }
}

/// 把配置核心的提交错误收敛成本模块的错误类型:核心以稳定码 config_stale_generation 报
/// 「generation 过期」(重置已发生),其余写盘失败统一为 Failed。message 只进日志,不经 IPC
/// 透传(退出流程对外用固定文案)。
fn map_submit_error(e: crate::error::AppError) -> GeometryCommitError {
    match &e {
        crate::error::AppError::Config { code, .. } if *code == "config_stale_generation" => {
            GeometryCommitError::StaleGeneration
        }
        _ => GeometryCommitError::Failed(e.to_string()),
    }
}

/// 仅测试:可控的提交目标(记录提交、可注入失败、可控 generation)。
#[cfg(test)]
struct TestSink {
    generation: Mutex<u64>,
    loaded: Mutex<Option<String>>,
    commits: Mutex<Vec<(u64, Vec<(String, WindowGeometry)>)>>,
    fail: Mutex<Option<GeometryCommitError>>,
}

// ── 采集状态机(纯) ───────────────────────────────────────────────────────────

/// 一条待保存的窗口几何:带**采集时**的 generation —— 重置后不得换新代把旧值写回。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pending {
    geometry: WindowGeometry,
    generation: u64,
    /// 首次进入本次待保存的时点(最长等待锚,不随后续事件后移)。
    first_ms: u64,
    /// 最近一次采集时点(空闲判据锚)。
    last_ms: u64,
    /// 本值的**后台自动重试**次数。达到 MAX_AUTO_RETRIES 后不再被定时器重试(避免空转),
    /// 但条目仍留在待保存集合里,由显式 flush 继续尝试 —— 失败绝不等于丢弃。
    attempts: u32,
}

#[derive(Debug, Default)]
struct PendingSet {
    entries: BTreeMap<String, Pending>,
}

impl PendingSet {
    fn note(&mut self, label: &str, geometry: WindowGeometry, generation: u64, now_ms: u64) {
        match self.entries.get_mut(label) {
            Some(p) => {
                // 用户又动了一次(值真的变了)→ 重新给足自动重试预算:先前失败很可能是瞬时 IO,
                // 不该让这一次调整继承上一次的耗尽状态。同值重复采集则保留计数,避免空转。
                if p.geometry != geometry {
                    p.attempts = 0;
                }
                p.geometry = geometry;
                p.generation = generation;
                p.last_ms = now_ms;
            }
            None => {
                self.entries.insert(
                    label.to_string(),
                    Pending {
                        geometry,
                        generation,
                        first_ms: now_ms,
                        last_ms: now_ms,
                        attempts: 0,
                    },
                );
            }
        }
    }

    /// 空闲 500ms 或累计等待 2s 即到期。
    fn is_due(p: &Pending, now_ms: u64) -> bool {
        now_ms.saturating_sub(p.last_ms) >= IDLE_COMMIT_MS
            || now_ms.saturating_sub(p.first_ms) >= MAX_COMMIT_MS
    }

    fn take_due(&mut self, now_ms: u64) -> Vec<(String, Pending)> {
        // 只取仍在自动重试预算内的条目:耗尽的条目留在集合里等显式 flush,不在这里被反复取出
        // (否则会形成「取出 → 再失败 → 还原」的空转)。
        let due: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, p)| p.attempts < MAX_AUTO_RETRIES && Self::is_due(p, now_ms))
            .map(|(label, _)| label.clone())
            .collect();
        due.into_iter()
            .filter_map(|label| self.entries.remove(&label).map(|p| (label, p)))
            .collect()
    }

    /// 取出**全部**待保存值(不论是否到期)。强制 flush 走它 —— 刚拖完就退出的值不能因为
    /// 还没到 500ms 空闲窗而被丢掉。
    fn drain_all(&mut self) -> Vec<(String, Pending)> {
        std::mem::take(&mut self.entries).into_iter().collect()
    }

    /// 取出某窗口的待保存值(该窗口即将销毁时的收尾)。
    fn drain_label(&mut self, label: &str) -> Option<Pending> {
        self.entries.remove(label)
    }

    /// 下一次需要唤醒的时点(所有条目中最早到期者)。
    fn next_deadline(&self) -> Option<u64> {
        self.entries
            .values()
            .filter(|p| p.attempts < MAX_AUTO_RETRIES)
            .map(|p| (p.last_ms + IDLE_COMMIT_MS).min(p.first_ms + MAX_COMMIT_MS))
            .min()
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

// ── 运行时 ────────────────────────────────────────────────────────────────────

struct Service {
    sink: Sink,
    pending: Mutex<PendingSet>,
    /// 该窗口**最后已知的正常边界**(不含最大化标志):最大化态采集与还原都靠它,
    /// 因此它必须独立于 pending(提交成功后 pending 会清空)与 applied(只在程序化落位时更新)。
    last_normal: Mutex<BTreeMap<String, WindowGeometry>>,
    /// 程序化落位(启动恢复与重置恢复)的几何:与之等价的采集事件不算用户改动。
    applied: Mutex<BTreeMap<String, WindowGeometry>>,
    /// 已经成功落盘的几何:与之等价的采集事件**无需再提交** —— 托盘隐藏/显示等会带来几何不变的
    /// resize,若不挡住就会为同一个值反复写文件(方案 §10「写入频率」)。
    committed: Mutex<BTreeMap<String, WindowGeometry>>,
    /// 采集总闸:启动恢复完成前与重置期间关闭。
    capture_enabled: AtomicBool,
    /// 提交串行闸:合并循环与强制 flush 共用,确保「等在途提交完成」可表达。核心侧另有自己的
    /// async 串行门,窗口提交照常经它,不绕过。
    commit_lock: tokio::sync::Mutex<()>,
    notify: tokio::sync::Notify,
}

impl Service {
    fn applied_of(&self, label: &str) -> Option<WindowGeometry> {
        self.applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(label)
            .copied()
    }

    fn mark_applied(&self, label: &str, geometry: WindowGeometry) {
        self.applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(label.to_string(), geometry);
        // 程序化落位后,「已落盘值」不再可信(重置刚把整键清空):清掉该窗口的记录,
        // 否则用户之后恰好把窗口拖回旧位置时会被误判为「无需提交」而丢设置。
        self.committed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(label);
        self.set_last_normal(label, geometry);
    }

    fn last_normal_of(&self, label: &str) -> Option<WindowGeometry> {
        self.last_normal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(label)
            .copied()
    }

    fn set_last_normal(&self, label: &str, geometry: WindowGeometry) {
        self.last_normal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                label.to_string(),
                WindowGeometry {
                    maximized: false,
                    ..geometry
                },
            );
    }

    fn pending_len(&self) -> usize {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .len()
    }

    /// 该窗口是否与已落盘的值一致(一致则本次采集无需提交)。
    fn matches_committed(&self, label: &str, geometry: &WindowGeometry) -> bool {
        self.committed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(label)
            .is_some_and(|known| known.same_placement(geometry))
    }

    /// 记录一批已成功落盘的值。
    fn mark_committed(&self, entries: &[(String, WindowGeometry)]) {
        let mut committed = self
            .committed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (label, geometry) in entries {
            committed.insert(label.clone(), *geometry);
        }
    }

    /// 把失败的批次还原进待保存集合,等下次重试(已有更新值时不覆盖更新的那个)。
    fn restore_pending(
        &self,
        entries: Vec<(String, WindowGeometry)>,
        generation: u64,
        attempts: u32,
    ) {
        if entries.is_empty() {
            return;
        }
        let now = now_ms();
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (label, geometry) in entries {
            if pending.entries.contains_key(&label) {
                continue;
            }
            pending.entries.insert(
                label,
                Pending {
                    geometry,
                    generation,
                    first_ms: now,
                    last_ms: now,
                    attempts,
                },
            );
        }
    }
}

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

/// 单调毫秒时基(仅模块内计时与排序用,不落盘)。
fn now_ms() -> u64 {
    let epoch = EPOCH.get_or_init(Instant::now);
    Instant::now().duration_since(*epoch).as_millis() as u64
}

/// 装配窗口几何服务并拉起合并提交任务。重复调用只告警,不换实现。
#[cfg(not(mobile))]
pub fn install(app: tauri::AppHandle, state: Arc<crate::state::AppState>) {
    let service = Arc::new(Service {
        sink: Sink::Core { app, state },
        pending: Mutex::new(PendingSet::default()),
        last_normal: Mutex::new(BTreeMap::new()),
        applied: Mutex::new(BTreeMap::new()),
        committed: Mutex::new(BTreeMap::new()),
        capture_enabled: AtomicBool::new(false),
        commit_lock: tokio::sync::Mutex::new(()),
        notify: tokio::sync::Notify::new(),
    });
    if SERVICE.set(Arc::clone(&service)).is_err() {
        tracing::warn!("窗口几何服务已装配,忽略重复 install");
        return;
    }
    tauri::async_runtime::spawn(commit_loop(service));
}

/// 移动端:窗口几何不参与持久化(设计 §6「移动端略过窗口恢复与捕获」)。
#[cfg(mobile)]
pub fn install(_app: tauri::AppHandle, _state: Arc<crate::state::AppState>) {}

/// 打开采集总闸(主窗口恢复并展示之后调用)。此前到达的事件一律不算用户改动。
#[cfg(not(mobile))]
pub fn activate_capture() {
    if let Some(service) = SERVICE.get() {
        service.capture_enabled.store(true, Ordering::Release);
    }
}

/// 移动端:无采集。
#[cfg(mobile)]
pub fn activate_capture() {}

/// 启动恢复:在 show() **之前**按记录落位。缺记录或离屏时按默认值居中。
#[cfg(not(mobile))]
pub fn restore_before_show(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;

    let Some(service) = SERVICE.get() else {
        return;
    };
    let Some(window) = app.get_window(label) else {
        tracing::warn!(label, "窗口几何恢复跳过:窗口不存在");
        return;
    };
    let stored = load_map(service).remove(label);
    let monitors = collect_monitors(&window);
    let placement = resolve_placement(
        stored.as_ref(),
        &default_geometry(label),
        min_size(label),
        &monitors,
    );
    if let Err(reason) = apply_placement(&window, &placement) {
        // 启动恢复失败不阻断启动:窗口留在创建默认几何,journal 留痕即可(用户可再拖一次)。
        tracing::warn!(label, "窗口几何恢复失败,沿用创建默认几何 | {reason}");
    }
    // 记下程序化落位:恢复过程触发的 move/resize 属回声,不回写配置;
    // 同时它就是该窗口的「最后已知正常边界」,最大化态采集要用。
    service.mark_applied(label, placement.to_geometry());
}

/// 移动端:略过窗口恢复。
#[cfg(mobile)]
pub fn restore_before_show(_app: &tauri::AppHandle, _label: &str) {}

/// 采集一次窗口几何事件(移动、改尺寸、DPI 变化),按 500ms 空闲、2s 上限合并提交。
#[cfg(not(mobile))]
pub fn note_geometry_event(window: &tauri::Window) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    if !service.capture_enabled.load(Ordering::Acquire) {
        return;
    }
    let label = window.label().to_string();
    if !REGISTERED_LABELS.contains(&label.as_str()) {
        return;
    }
    // visible、最小化与全屏状态不持久化:这些状态下的边界不是用户想要的落位,也不该覆盖它。
    if window.is_minimized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let scale = match window.scale_factor() {
        Ok(s) if s.is_finite() && s > 0.0 => s,
        _ => return,
    };
    let geometry = if window.is_maximized().unwrap_or(false) {
        // 最大化时系统返回的是最大化边界,拿它当正常边界会让「还原」失去尺寸:
        // 只翻转标志,保留最后已知的正常边界(与是否已提交无关)。
        let mut known = service
            .last_normal_of(&label)
            .unwrap_or_else(|| default_geometry(&label));
        known.maximized = true;
        known
    } else {
        let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
            return;
        };
        let geometry = WindowGeometry {
            x: round2(f64::from(position.x)),
            y: round2(f64::from(position.y)),
            width: round2(f64::from(size.width)),
            height: round2(f64::from(size.height)),
            scale_factor: round2(scale),
            maximized: false,
        };
        service.set_last_normal(&label, geometry);
        geometry
    };
    if let Some(applied) = service.applied_of(&label) {
        if applied.same_placement(&geometry) {
            return;
        }
    }
    // 采集侧先自检一次:平台给出 NaN/无穷/零尺寸时该值**根本无法表示**(既不能写进 TOML,也无法
    // 参与几何计算)。这类值不入队,并留下证据日志 —— 与「写盘失败后不得无证据丢弃」不冲突:
    // 那种失败是可重试的 IO 问题,这里则是值本身不可表示,连一次都不可能成功。
    if !geometry.is_valid() {
        tracing::warn!(
            label,
            ?geometry,
            "平台返回的窗口几何非法(NaN/无穷/零尺寸),本次不采集 | invalid window geometry from platform, not captured"
        );
        return;
    }
    // 与已落盘的值一致:无需再写一次文件(托盘隐藏/显示等几何不变的 resize 会走到这里)。
    if service.matches_committed(&label, &geometry) {
        return;
    }
    service
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .note(&label, geometry, service.sink.generation(), now_ms());
    service.notify.notify_one();
}

/// 移动端:无窗口采集。
#[cfg(mobile)]
pub fn note_geometry_event(_window: &tauri::Window) {}

/// 是否还有未落盘的待保存几何(退出前如实报告,不假装已保存)。
#[cfg(not(mobile))]
pub fn has_pending() -> bool {
    SERVICE
        .get()
        .map(|service| service.pending_len() > 0)
        .unwrap_or(false)
}

/// 移动端:恒无待保存值。
#[cfg(mobile)]
pub fn has_pending() -> bool {
    false
}

/// 强制提交**全部**待保存值(不等空闲窗口),并等在途提交完成。
///
/// 退出、关窗与重置前的收尾都走它。返回错误表示这批值仍未落盘 —— 它们已还原进待保存集合,
/// 调用方可以重试或明确放弃,不把失败当成功。
#[cfg(not(mobile))]
pub async fn commit_pending_now() -> Result<(), GeometryCommitError> {
    match SERVICE.get().cloned() {
        Some(service) => commit_pending_with(&service).await,
        None => Ok(()),
    }
}

/// 移动端:无待保存值。
#[cfg(mobile)]
pub async fn commit_pending_now() -> Result<(), GeometryCommitError> {
    Ok(())
}

/// 强制提交某窗口的待保存值(该窗口即将销毁时调用)。
#[cfg(not(mobile))]
pub async fn commit_pending_for_label(label: &str) -> Result<(), GeometryCommitError> {
    let Some(service) = SERVICE.get() else {
        return Ok(());
    };
    let entries: Vec<(String, Pending)> = service
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain_label(label)
        .map(|p| vec![(label.to_string(), p)])
        .unwrap_or_default();
    commit_locked(service, entries).await
}

/// 移动端:无待保存值。
#[cfg(mobile)]
pub async fn commit_pending_for_label(_label: &str) -> Result<(), GeometryCommitError> {
    Ok(())
}

/// 重置第 1 步:暂停采集并**丢弃**待保存的旧几何 —— 旧 generation 的值不得在新代写回。
#[cfg(not(mobile))]
pub fn suspend_for_reset() {
    if let Some(service) = SERVICE.get() {
        service.capture_enabled.store(false, Ordering::Release);
        service
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        // 重置会清空 window_geometry 整键:已落盘记录随之失效,不能用来抑制重置后的采集。
        service
            .committed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

/// 移动端:无采集。
#[cfg(mobile)]
pub fn suspend_for_reset() {}

/// 重置第 2 步:把常驻窗口恢复到各自的创建默认几何(程序化落位,不会被采集回写)。
///
/// 返回值可判定,因为这是「已保存但未生效」与「完全成功」的分界:
/// - 未打开或已销毁的辅助窗口、服务未装配的场景、移动端 → 无可应用对象,按成功返回;
/// - 某个窗口的 unmaximize / set_size / set_position 失败 → 返回 Err(带 label 与原因),
///   调用方须把 window_geometry 记进 SettingsChange 的 apply_failed —— 文件里的默认值已经写好了,
///   既不能回滚成「保存失败」,也不能报成「完全成功」(窗口此刻仍在旧位置)。
///
/// 失败只影响该次应用:采集总闸由 resume_after_reset 无条件恢复,不因这里出错而永久停摆。
#[cfg(not(mobile))]
pub fn apply_default_geometry(app: &tauri::AppHandle) -> Result<(), GeometryApplyError> {
    use tauri::Manager;

    let Some(service) = SERVICE.get() else {
        // 服务未装配(桌面端启动即装,这里只兜底):没有可应用的窗口记录,不算失败。
        return Ok(());
    };
    for label in REGISTERED_LABELS {
        let Some(window) = app.get_window(label) else {
            // 辅助窗口没开:它下次打开时直接按默认几何创建,无需应用。
            continue;
        };
        let monitors = collect_monitors(&window);
        let placement =
            resolve_placement(None, &default_geometry(label), min_size(label), &monitors);
        match apply_placement(&window, &placement) {
            Ok(()) => service.mark_applied(label, placement.to_geometry()),
            Err(reason) => {
                tracing::warn!(label, "恢复默认窗口几何未生效 | {reason}");
                return Err(GeometryApplyError::Window {
                    label: (*label).to_string(),
                    reason,
                });
            }
        }
    }
    Ok(())
}

/// 移动端:无窗口几何,恒成功。
#[cfg(mobile)]
pub fn apply_default_geometry(_app: &tauri::AppHandle) -> Result<(), GeometryApplyError> {
    Ok(())
}

/// 重置第 3 步:恢复采集(写盘失败的分支也必须走到,否则保存被永久禁用)。
#[cfg(not(mobile))]
pub fn resume_after_reset() {
    if let Some(service) = SERVICE.get() {
        service.capture_enabled.store(true, Ordering::Release);
    }
}

/// 移动端:无采集。
#[cfg(mobile)]
pub fn resume_after_reset() {}

/// 合并提交循环:被新采集唤醒后按到期情况提交;失败批次已还原,退避后再试。
#[cfg(not(mobile))]
async fn commit_loop(service: Arc<Service>) {
    loop {
        service.notify.notified().await;
        loop {
            let due = service
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take_due(now_ms());
            if !due.is_empty() {
                if commit_locked(&service, due).await.is_err() {
                    // 失败批次已还原进待保存集合:退避后重试,不丢值、不空转。
                    tokio::time::sleep(RETRY_BACKOFF).await;
                }
                continue;
            }
            let next = service
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .next_deadline();
            match next {
                Some(deadline) => {
                    let wait = deadline.saturating_sub(now_ms()).max(1);
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
                None => break,
            }
        }
    }
}

/// 取出该服务实例的全部待保存值并提交。
#[cfg(not(mobile))]
async fn commit_pending_with(service: &Arc<Service>) -> Result<(), GeometryCommitError> {
    let entries = service
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain_all();
    commit_locked(service, entries).await
}

/// 在提交闸内把一批待保存值写盘(等在途提交完成 → 核心的统一入口 → 失败还原)。
#[cfg(not(mobile))]
async fn commit_locked(
    service: &Arc<Service>,
    entries: Vec<(String, Pending)>,
) -> Result<(), GeometryCommitError> {
    if entries.is_empty() {
        return Ok(());
    }
    let _guard = service.commit_lock.lock().await;
    // 本批的尝试序号 = 批次里最大的已有次数 + 1(同一批各 label 的进度可能不同,取最大者)。
    // 它**只**用于抑制后台定时重试,绝不用于丢弃待保存值:失败一次就丢会让随后的退出 flush 在
    // 空集合上报告已保存,而用户的窗口位置一次都没落盘。
    let attempts = entries
        .iter()
        .map(|(_, pending)| pending.attempts)
        .max()
        .unwrap_or(0)
        + 1;
    let generation = service.sink.generation();
    let fresh = split_fresh(entries, generation);
    if fresh.is_empty() {
        // 全部属旧代:重置已发生,直接丢弃,不算失败(此非「失败」而是「已作废」)。
        return Ok(());
    }
    match service.sink.commit(generation, fresh.clone()).await {
        Ok(()) => {
            service.mark_committed(&fresh);
            Ok(())
        }
        Err(e) => {
            // 不吞错、也不丢值:还原待保存值(累加尝试次数,后台到上限即停止空转,但值仍在),
            // 调用方据此如实上报未落盘,由显式 flush 继续重试或由用户明确放弃。
            if !matches!(e, GeometryCommitError::StaleGeneration) {
                tracing::warn!(
                    "窗口几何保存失败,已保留待保存值等待重试(后台自动重试上限 {MAX_AUTO_RETRIES}) | {e}"
                );
                service.restore_pending(fresh, generation, attempts);
            }
            Err(e)
        }
    }
}

/// 丢弃属旧 generation 的条目(重置已发生,它们不得换新代写回)。
#[cfg(not(mobile))]
fn split_fresh(entries: Vec<(String, Pending)>, generation: u64) -> Vec<(String, WindowGeometry)> {
    let mut fresh = Vec::new();
    for (label, pending) in entries {
        if pending.generation != generation {
            tracing::warn!(
                label,
                "窗口几何待保存值属重置前的旧 generation,丢弃不写回 | stale generation pending dropped"
            );
            continue;
        }
        fresh.push((label, pending.geometry));
    }
    fresh
}

/// 读当前几何表(缺失或损坏 → 空表)。仅启动恢复使用。
#[cfg(not(mobile))]
fn load_map(service: &Service) -> GeometryMap {
    service
        .sink
        .load()
        .map(|raw| parse_map(&raw))
        .unwrap_or_default()
}

/// 采集当前显示器布局(物理边界 + 工作区 + DPI)。
#[cfg(not(mobile))]
fn collect_monitors(window: &tauri::Window) -> Vec<MonitorRect> {
    let primary = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (m.position().x, m.position().y));
    let Ok(monitors) = window.available_monitors() else {
        return Vec::new();
    };
    monitors
        .iter()
        .map(|m| {
            let position = m.position();
            let size = m.size();
            let work = m.work_area();
            let bounds = Rect {
                x: f64::from(position.x),
                y: f64::from(position.y),
                width: f64::from(size.width),
                height: f64::from(size.height),
            };
            let work_rect = Rect {
                x: f64::from(work.position.x),
                y: f64::from(work.position.y),
                width: f64::from(work.size.width),
                height: f64::from(work.size.height),
            };
            MonitorRect {
                bounds,
                // 个别平台不给工作区(返回 0×0):退回整屏边界,避免把窗口居中到空矩形里。
                work: if work_rect.width > 0.0 && work_rect.height > 0.0 {
                    work_rect
                } else {
                    bounds
                },
                scale_factor: if m.scale_factor() > 0.0 {
                    m.scale_factor()
                } else {
                    1.0
                },
                primary: primary == Some((position.x, position.y)),
            }
        })
        .collect()
}

/// 应用一次落位,返回可判定的结果(失败原因回给调用方,不在这里吞掉)。
///
/// 次序:先退出最大化(否则 set_size/set_position 对最大化态窗口无效,恢复默认几何会「看起来
/// 什么都没发生」)→ 尺寸 → 位置 → 需要时再最大化。尺寸走 set_size(客户区)、位置走
/// set_position(外框),与采集口径一一对应。
#[cfg(not(mobile))]
fn apply_placement(window: &tauri::Window, placement: &Placement) -> Result<(), String> {
    if !placement.maximized && window.is_maximized().unwrap_or(false) {
        window
            .unmaximize()
            .map_err(|e| format!("unmaximize failed: {e}"))?;
    }
    let size = tauri::PhysicalSize::new(
        placement.width.round().max(1.0) as u32,
        placement.height.round().max(1.0) as u32,
    );
    let position =
        tauri::PhysicalPosition::new(placement.x.round() as i32, placement.y.round() as i32);
    window
        .set_size(size)
        .map_err(|e| format!("set_size failed: {e}"))?;
    window
        .set_position(position)
        .map_err(|e| format!("set_position failed: {e}"))?;
    if placement.maximized && !window.is_maximized().unwrap_or(false) {
        window
            .maximize()
            .map_err(|e| format!("maximize failed: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(x: f64, y: f64, w: f64, h: f64, scale: f64, maximized: bool) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width: w,
            height: h,
            scale_factor: scale,
            maximized,
        }
    }

    fn sink(generation: u64) -> Arc<TestSink> {
        Arc::new(TestSink {
            generation: Mutex::new(generation),
            loaded: Mutex::new(None),
            commits: Mutex::new(Vec::new()),
            fail: Mutex::new(None),
        })
    }

    fn service_with(sink: Arc<TestSink>) -> Arc<Service> {
        Arc::new(Service {
            sink: Sink::Test(sink),
            pending: Mutex::new(PendingSet::default()),
            last_normal: Mutex::new(BTreeMap::new()),
            applied: Mutex::new(BTreeMap::new()),
            committed: Mutex::new(BTreeMap::new()),
            capture_enabled: AtomicBool::new(true),
            commit_lock: tokio::sync::Mutex::new(()),
            notify: tokio::sync::Notify::new(),
        })
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(future)
    }

    /// 提交失败 → 批次还原进待保存集合且向调用方报错(不吞错、不丢值);恢复后可重试成功。
    #[test]
    fn failed_commit_restores_pending_and_reports_error() {
        let test_sink = sink(1);
        *test_sink.fail.lock().unwrap() = Some(GeometryCommitError::Failed("磁盘只读".to_string()));
        let service = service_with(Arc::clone(&test_sink));
        service.pending.lock().unwrap().note(
            MAIN_LABEL,
            geometry(10.0, 10.0, 1280.0, 820.0, 1.0, false),
            1,
            0,
        );
        let failed = block_on(commit_pending_with(&service));
        assert!(failed.is_err(), "失败必须向调用方报错");
        assert_eq!(service.pending_len(), 1, "待保存值必须还原,等下次重试");
        assert!(
            test_sink.commits.lock().unwrap().is_empty(),
            "失败的批次不得落盘"
        );

        // 恢复可写后再 flush:值仍在,且能真正落盘。
        *test_sink.fail.lock().unwrap() = None;
        let retry = block_on(commit_pending_with(&service));
        assert!(retry.is_ok());
        assert_eq!(service.pending_len(), 0);
        assert_eq!(test_sink.commits.lock().unwrap().len(), 1);
    }

    /// 重置后的旧代批次被核心拒绝时按丢弃处理,不进待保存集合(否则会一直重试旧值)。
    #[test]
    fn stale_generation_commit_is_not_restored() {
        let test_sink = sink(1);
        let service = service_with(Arc::clone(&test_sink));
        service.pending.lock().unwrap().note(
            MAIN_LABEL,
            geometry(10.0, 10.0, 1280.0, 820.0, 1.0, false),
            1,
            0,
        );
        // 重置发生:generation 前进,采集时的 1 变成旧代 → 提交前就被本地丢弃,不报错。
        *test_sink.generation.lock().unwrap() = 2;
        let result = block_on(commit_pending_with(&service));
        assert!(result.is_ok(), "旧代条目按丢弃处理,不算失败");
        assert_eq!(service.pending_len(), 0);
        assert!(test_sink.commits.lock().unwrap().is_empty());
    }

    /// 提交失败后的还原不覆盖更新的待保存值(后到的调整优先)。
    #[test]
    fn restore_does_not_overwrite_newer_pending() {
        let test_sink = sink(1);
        let service = service_with(test_sink);
        service.pending.lock().unwrap().note(
            MAIN_LABEL,
            geometry(50.0, 50.0, 1400.0, 900.0, 1.0, false),
            1,
            0,
        );
        service.restore_pending(
            vec![(
                MAIN_LABEL.to_string(),
                geometry(10.0, 10.0, 1280.0, 820.0, 1.0, false),
            )],
            1,
            // 失败批次的尝试次数(测试里显式给一个值,与生产路径传 attempts 同型)。
            1,
        );
        let pending = service.pending.lock().unwrap().entries[MAIN_LABEL];
        assert_eq!(pending.geometry.x, 50.0, "更新的值不被失败批次覆盖");
    }
}
