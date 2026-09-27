// src-tauri/src/utils/format.rs
//! 媒体格式分类。
//!
//! **单一事实源（S 线 D-007）**：[`BUILTIN_FORMATS`] 是内置已注册格式的唯一定义表，
//! [`classify_media_type`]、[`is_phase1_image`]、[`doc_subtype`] 全部由它派生 —— 此前它们是三个
//! 平行 `match`，各自重列扩展名，新增格式漏改一处即静默漂移（如新增文档格式却忘了给
//! `doc_subtype` 加 arm → 落库 `"other"`）。
//!
//! 新增**内置**格式只改本表；**冷门格式**走 exotic Catalog（`exotic::catalog`，装载期已拒绝与
//! 内置表撞名，见 `CatalogError::CommonFormatConflict`）。扫描期的 common-first 并集见
//! `scanner::walker::classify_scanned_file`。
//!
//! 表内 `group` 只是 **UI 显示分组**（一个 UI 概念对应多个物理扩展名，S 线 §7.3）：DB、IPC 与 URL
//! 状态**始终存具体扩展名**，group 不落库。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Image,
    Video,
    Audio,
    Document,
}

impl MediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaType::Image => "image",
            MediaType::Video => "video",
            MediaType::Audio => "audio",
            MediaType::Document => "document",
        }
    }
}

/// `document_meta.doc_subtype` 的取值域（此前是裸字符串字面量）。
/// 非文档格式没有子类型 → [`doc_subtype`] 对其回落 `"other"`，与重构前逐值一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentSubtype {
    Pdf,
    Svg,
    /// 独立子类型（非 office/text）：前端据此走 EPUB 阅读器（CFI 进度），不与纯文本卡混淆。
    Epub,
    Office,
    Text,
}

impl DocumentSubtype {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentSubtype::Pdf => "pdf",
            DocumentSubtype::Svg => "svg",
            DocumentSubtype::Epub => "epub",
            DocumentSubtype::Office => "office",
            DocumentSubtype::Text => "text",
        }
    }
}

/// UI 显示分组标识（S 线 §7.3）。JPEG/TIFF 是别名对，RAW 是相机原始格式族 ——
/// 三者是**同一个问题**（一个 UI 概念 → 多个物理扩展名），故用同一机制，不为 RAW 另造一套。
pub const GROUP_JPEG: &str = "jpeg";
pub const GROUP_TIFF: &str = "tiff";
pub const GROUP_RAW: &str = "raw";

/// 一个内置已注册格式的完整定义。
///
/// 字段是**内部**处理能力，不等于 UI facet：IPC 只投影 `ext/media_type/group/source`
/// （S 线 §6.1）。若某 exotic 将来需要 `phase1_image` 或新 `document_subtype`，那是处理能力
/// schema 的独立扩展，不得反向塞进 UI DTO。
#[derive(Debug)]
pub struct RegisteredFormatDef {
    /// 规范化小写扩展名（不含点）。DB / IPC / URL 存的就是它。
    pub ext: &'static str,
    pub media_type: MediaType,
    /// UI 显示分组；`None` = 该格式自成一组。
    pub group: Option<&'static str>,
    /// fast scan 完全支持的第一阶段图像（主解码引擎 image crate 可直出）。
    pub phase1_image: bool,
    /// 文档子类型；非文档恒 `None`。
    pub document_subtype: Option<DocumentSubtype>,
}

// 表构造助手：让 66 行定义保持可读（否则每行都是 5 个字段的结构体字面量）。
const fn image(
    ext: &'static str,
    group: Option<&'static str>,
    phase1_image: bool,
) -> RegisteredFormatDef {
    RegisteredFormatDef {
        ext,
        media_type: MediaType::Image,
        group,
        phase1_image,
        document_subtype: None,
    }
}
const fn video(ext: &'static str) -> RegisteredFormatDef {
    RegisteredFormatDef {
        ext,
        media_type: MediaType::Video,
        group: None,
        phase1_image: false,
        document_subtype: None,
    }
}
const fn audio(ext: &'static str) -> RegisteredFormatDef {
    RegisteredFormatDef {
        ext,
        media_type: MediaType::Audio,
        group: None,
        phase1_image: false,
        document_subtype: None,
    }
}
const fn document(ext: &'static str, sub: DocumentSubtype) -> RegisteredFormatDef {
    RegisteredFormatDef {
        ext,
        media_type: MediaType::Document,
        group: None,
        phase1_image: false,
        document_subtype: Some(sub),
    }
}

/// 内置已注册格式全表 —— **本模块的唯一事实源**。
///
/// ⚠️ `psd` **有意不在表内**：已交冷门格式插件子系统（exotic Catalog）接管。主解码引擎
/// （image crate）无法解码 PSD；common-first 必须让 `classify_media_type("psd")` 返回 `None`，
/// Catalog 才能把它识别为 exotic image 并走 Worker 缩略图流水线
/// （见 `exotic/catalog.rs`、`scanner::walker::classify_scanned_file`）。
static BUILTIN_FORMATS: &[RegisteredFormatDef] = &[
    // ── 第一阶段图像（fast scan 完全支持）────────────────────────────────
    image("jpg", Some(GROUP_JPEG), true),
    image("jpeg", Some(GROUP_JPEG), true),
    image("png", None, true),
    image("webp", None, true),
    image("bmp", None, true),
    image("gif", None, true),
    image("tif", Some(GROUP_TIFF), true),
    image("tiff", Some(GROUP_TIFF), true),
    // ── 第二阶段图像 ─────────────────────────────────────────────────────
    image("heic", None, false),
    image("heif", None, false),
    image("avif", None, false),
    // 相机原始格式族：UI 归入 RAW 快捷组，核心状态仍存具体扩展名（S 线 §7.4）。
    image("cr2", Some(GROUP_RAW), false),
    image("cr3", Some(GROUP_RAW), false),
    image("nef", Some(GROUP_RAW), false),
    image("arw", Some(GROUP_RAW), false),
    image("dng", Some(GROUP_RAW), false),
    image("raf", Some(GROUP_RAW), false),
    image("orf", Some(GROUP_RAW), false),
    image("rw2", Some(GROUP_RAW), false),
    image("pef", Some(GROUP_RAW), false),
    image("srw", Some(GROUP_RAW), false),
    // ── 视频 ─────────────────────────────────────────────────────────────
    video("mp4"),
    video("m4v"),
    video("mov"),
    video("avi"),
    video("mkv"),
    video("webm"),
    video("wmv"),
    video("flv"),
    video("mpg"),
    video("mpeg"),
    video("3gp"),
    video("3g2"),
    video("ts"),
    video("mts"),
    video("m2ts"),
    video("ogv"),
    video("asf"),
    // rmvb/vob:MF/常规解码器不认的冷门容器,由 video-extended(builtin+free)经 ffmpeg 桥
    // (V5 WorkerVideoBackend)出缩略图。入表使 classify=video → 走常规 video 派生链
    // (video_cover/video_keyframes),与 RAW cr2「在表+builtin offering 认领」同型叠加
    // (catalog.rs:242 builtin 豁免 CommonFormatConflict);D-444③ A4 裁决。
    video("rmvb"),
    video("vob"),
    // ── 音频 ─────────────────────────────────────────────────────────────
    audio("mp3"),
    audio("flac"),
    audio("wav"),
    audio("aac"),
    audio("m4a"),
    audio("ogg"),
    audio("oga"),
    audio("opus"),
    audio("wma"),
    audio("aiff"),
    audio("aif"),
    audio("ape"),
    audio("alac"),
    // ── 文档 ─────────────────────────────────────────────────────────────
    // ⚠️ `epub` 必须在此登记为 Document，否则扫描器判 None → epub 根本不入库，下游已就绪的封面链
    // （DOC_THUMB_FORMATS 含 epub、derive/doc.rs 后端 zip 抽 OPF 封面）全成死代码。
    // epub 封面由后端抽取（跨平台），pdf/svg 由前端离屏渲染回传（见 derive/doc.rs）。
    document("pdf", DocumentSubtype::Pdf),
    document("svg", DocumentSubtype::Svg),
    document("epub", DocumentSubtype::Epub),
    document("doc", DocumentSubtype::Office),
    document("docx", DocumentSubtype::Office),
    document("xls", DocumentSubtype::Office),
    document("xlsx", DocumentSubtype::Office),
    document("ppt", DocumentSubtype::Office),
    document("pptx", DocumentSubtype::Office),
    document("odt", DocumentSubtype::Office),
    document("ods", DocumentSubtype::Office),
    document("odp", DocumentSubtype::Office),
    document("txt", DocumentSubtype::Text),
    document("md", DocumentSubtype::Text),
    document("rtf", DocumentSubtype::Text),
];

/// `ext → def` 索引。扫描热路径（每文件一次，百万级库）走 O(1) 哈希而非 66 项线扫。
fn builtin_index() -> &'static HashMap<&'static str, &'static RegisteredFormatDef> {
    static INDEX: OnceLock<HashMap<&'static str, &'static RegisteredFormatDef>> = OnceLock::new();
    INDEX.get_or_init(|| BUILTIN_FORMATS.iter().map(|d| (d.ext, d)).collect())
}

/// 全部内置定义（供合并 registry / facet / 契约测试枚举；**不要**在别处重列扩展名）。
pub fn builtin_formats() -> &'static [RegisteredFormatDef] {
    BUILTIN_FORMATS
}

/// 查内置定义。`None` = 非内置（可能是 exotic，也可能根本不是媒体）。
pub fn lookup_builtin(ext: &str) -> Option<&'static RegisteredFormatDef> {
    builtin_index().get(ext).copied()
}

/// 返回给定小写文件扩展名的 `MediaType`，如果不支持则返回 `None`。
///
/// **只认内置表**：exotic 格式（如 psd）在此恒返回 `None`，由 common-first 回退查 Catalog。
pub fn classify_media_type(ext: &str) -> Option<MediaType> {
    lookup_builtin(ext).map(|d| d.media_type)
}

/// 如果格式是第一阶段图像（在快速扫描中完全支持），则返回 `true`。
pub fn is_phase1_image(ext: &str) -> bool {
    lookup_builtin(ext).is_some_and(|d| d.phase1_image)
}

/// 如果扩展名可能是 Apple Live Photo 的 MOV 伴随文件，则返回 `true`。
pub fn is_live_photo_companion_ext(ext: &str) -> bool {
    ext == "mov"
}

/// `document_meta` 表的文档子类型。
///
/// 非文档格式与未知扩展名一律 `"other"`（与重构前的 `match` 兜底逐值一致）。
pub fn doc_subtype(ext: &str) -> &'static str {
    lookup_builtin(ext)
        .and_then(|d| d.document_subtype)
        .map_or("other", |s| s.as_str())
}
