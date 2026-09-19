// src-tauri/src/exotic/catalog.rs
//! 冷门格式插件 · 能力目录（v3 §5.1「能力真相」/ Part1 §1.2-1.4）。
//!
//! Catalog 回答：某扩展名**是否有产品**、属哪类媒体、提供哪些能力、哪些平台可用。
//! 它**不**回答「是否已安装」「是否已授权」——那是另外两份真相（v3 §5.1）。
//!
//! 设计要点：
//!   - 内置 Catalog 用 `include_str!` 编入二进制（= 随应用签名发布，首次离线也可识别可购买格式）。
//!   - 运行时只读快照 `RwLock<Arc<CatalogSnapshot>>`：热路径一次读锁 + Arc clone，不查 DB；
//!     刷新时先**完整**解析新快照、校验通过后整体替换，禁止半更新（Part1 §1.4）。
//!   - `by_format` 以**规范化小写扩展名**为键（R13：分类只依赖扩展名，扫描事务内即可判定）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::utils::format::{classify_media_type, MediaType};

/// 内置 Catalog JSON（编译期嵌入）。
const BUILTIN_CATALOG_JSON: &str = include_str!("../../resources/exotic-catalog.json");

/// 本 Host 支持的 Catalog schema 版本。
const SUPPORTED_CATALOG_SCHEMA: u32 = 1;

/// 媒体大类。与 `utils::format::MediaType` 同义，但属于 exotic 契约的一部分（序列化为小写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Document,
}

impl From<MediaKind> for MediaType {
    fn from(k: MediaKind) -> Self {
        match k {
            MediaKind::Image => MediaType::Image,
            MediaKind::Video => MediaType::Video,
            MediaKind::Audio => MediaType::Audio,
            MediaKind::Document => MediaType::Document,
        }
    }
}

/// 能力类型。首发只交付 `thumbnail`（v3 §3.2）；metadata/text 为后续扩展预留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Thumbnail,
    Metadata,
    Text,
    /// CLIP 批量嵌入(Part4 T10/G5,协议 v2)。
    Embedding,
    /// 人脸检测+嵌入(Part4 T10/G5,协议 v2)。rename 保 snake_case 全名——
    /// 本枚举的 rename_all="lowercase" 会把驼峰挤成 "facedetectembed",
    /// 与协议侧 capability::FACE_DETECT_EMBED 不一致。
    #[serde(rename = "face_detect_embed")]
    FaceDetectEmbed,
    /// 影像增强(降噪/超分子系统 design.md §C):同 OCR 的 D-OCR-7 豁免——enhance **不**进
    /// exotic 任务化调度(host 侧 EnhanceService 直持 supervisor+worker client),本变体
    /// 仅作 catalog 展示/授权面存在,不接 coordinator 任务队列。serde 小写序列化为 "enhance"。
    Enhance,
}

impl Capability {
    /// 能力的稳定字符串标识（与 DB `exotic_tasks.capability` 列、序列化形态一致）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::Thumbnail => "thumbnail",
            Capability::Metadata => "metadata",
            Capability::Text => "text",
            Capability::Embedding => "embedding",
            Capability::FaceDetectEmbed => "face_detect_embed",
            Capability::Enhance => "enhance",
        }
    }
}

/// Catalog 解析/校验错误。整个 Catalog 校验失败时拒绝**全部**，不做部分接受（Part1 §1.2）。
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog JSON 解析失败：{0}")]
    Parse(String),
    #[error("不支持的 catalog schema 版本：{0}（支持 {SUPPORTED_CATALOG_SCHEMA}）")]
    UnsupportedSchema(u32),
    #[error("非法 plugin_id：{0}")]
    InvalidPluginId(String),
    #[error("非法 format：{0}（要求小写、无点、[a-z0-9]{{1,16}}）")]
    InvalidFormat(String),
    #[error("重复 format：{0}（同一格式只能有一个默认 offering）")]
    DuplicateFormat(String),
    #[error("重复 plugin_id：{0}")]
    DuplicatePlugin(String),
    #[error("offering {0} 的 capabilities 为空")]
    EmptyCapabilities(String),
    #[error(
        "offering {0} 声明 thumbnail 能力但缺 worker_id（或为空串）：外部 Catalog 边界拒绝,\
不静默落默认映射;OCR/Enhance 等无 thumbnail 的 offering 不受此约束"
    )]
    ThumbnailWithoutWorkerId(String),
    #[error(
        "offering {0} 声明 override_common=true（该权限只存在于主程序内置审核表，客户端拒绝）"
    )]
    OverrideCommonNotAllowed(String),
    #[error(
        "format {0} 撞常见格式（classify_media_type 已认领；客户端拒绝整个 Catalog，Part1 §1.2）"
    )]
    CommonFormatConflict(String),
}

/// JSON 顶层结构。
#[derive(Debug, Clone, Deserialize)]
struct RawCatalog {
    schema: u32,
    sequence: u64,
    offerings: Vec<RawOffering>,
}

/// JSON 单个 offering（一个插件可声明多个 format）。
#[derive(Debug, Clone, Deserialize)]
struct RawOffering {
    plugin_id: String,
    name: String,
    media_kind: MediaKind,
    formats: Vec<String>,
    capabilities: Vec<Capability>,
    license_tier: String,
    platforms: Vec<String>,
    min_host_version: String,
    /// 授权 SKU（License token 验签的 expected_sku 来源，§5.2）。paid offering 应声明；
    /// free/无 SKU 时为 None（无法验签 → 已装也只能 InstalledUnlicensed）。
    #[serde(default)]
    sku: Option<String>,
    #[serde(default)]
    override_common: bool,
    #[serde(default)]
    store_url: Option<String>,
    /// 分发形态(D-OCR-5):`"builtin"` = 无安装包,`availability_of` 跳过安装态门直接验 license;
    /// 缺省/其它值按常规 package 处理。
    #[serde(default)]
    distribution: Option<String>,
    /// 运行期调度信息(Part8 前由发布侧显式声明):worker 握手期望的 worker_id。
    /// 缺省/None 表示该 offering 不进入 exotic Coordinator 调度(如 OCR/Enhance 独立服务)。
    #[serde(default)]
    worker_id: Option<String>,
    /// 运行期调度信息:是否占用 GPU 令牌。缺省 false。
    #[serde(default)]
    uses_gpu: Option<bool>,
}

/// 运行时单格式视图（`by_format` 的值）。一个 offering 的多 format 会复制成多条。
#[derive(Debug, Clone)]
pub struct CatalogOffering {
    pub plugin_id: String,
    pub display_name: String,
    pub media_kind: MediaKind,
    /// 该 plugin 声明的全部 format（同一 offering 的所有键共享此列表）。
    pub formats: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub license_tier: String,
    pub platforms: Vec<String>,
    pub min_host_version: String,
    /// 授权 SKU（§5.2）；None=无 SKU（不可验签）。
    pub sku: Option<String>,
    pub store_url: Option<String>,
    /// builtin offering(D-OCR-5):无安装包,`availability_of` 跳过安装态门直接验 license。
    pub builtin: bool,
    /// 运行期调度信息:worker 握手期望的 worker_id。None 表示不进入 exotic 调度。
    pub worker_id: Option<String>,
    /// 运行期调度信息:是否占用 GPU 令牌。
    pub uses_gpu: bool,
}

impl CatalogOffering {
    pub fn claims_capability(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// 该 offering 是否支持给定 rust target triple。
    pub fn supports_platform(&self, target: &str) -> bool {
        self.platforms.iter().any(|p| p == target)
    }
}

/// 不可变只读快照。热路径只持此结构的 `Arc`。
pub struct CatalogSnapshot {
    by_format: HashMap<String, CatalogOffering>,
    sequence: u64,
}

impl CatalogSnapshot {
    /// 空快照（无任何 offering）——测试/降级用。
    pub fn empty() -> Self {
        CatalogSnapshot {
            by_format: HashMap::new(),
            sequence: 0,
        }
    }

    /// 解析 + 严格校验 JSON → 快照。任一项不合规即整体拒绝。
    ///
    /// 撞常见格式（`classify_media_type` 已认领，如 jpg/mp4）→ 拒绝**整个** Catalog（Part1 §1.2）。
    /// 这是纵深防御：扫描 common-first 只挡解码劫持，挡不住缩略图 router（直接 key 于
    /// `resolve_format`）把误登记的常见格式 gate 出主 generator，也挡不住错误产品暴露到市场。
    pub fn parse(json: &str) -> Result<Self, CatalogError> {
        let raw: RawCatalog =
            serde_json::from_str(json).map_err(|e| CatalogError::Parse(e.to_string()))?;
        if raw.schema != SUPPORTED_CATALOG_SCHEMA {
            return Err(CatalogError::UnsupportedSchema(raw.schema));
        }

        let mut by_format: HashMap<String, CatalogOffering> = HashMap::new();
        let mut seen_plugins: HashMap<String, ()> = HashMap::new();

        for off in raw.offerings {
            if !is_valid_plugin_id(&off.plugin_id) {
                return Err(CatalogError::InvalidPluginId(off.plugin_id));
            }
            if seen_plugins.insert(off.plugin_id.clone(), ()).is_some() {
                return Err(CatalogError::DuplicatePlugin(off.plugin_id));
            }
            if off.override_common {
                return Err(CatalogError::OverrideCommonNotAllowed(off.plugin_id));
            }
            if off.capabilities.is_empty() {
                return Err(CatalogError::EmptyCapabilities(off.plugin_id));
            }
            // 外部 Catalog 边界的唯一校验点(P17):thumbnail 能力的 worker_id 必须显式声明。
            // Coordinator 只消费显式值(不再有 PSD/RAW/video 硬编码 fallback),故缺失时必须在
            // 加载/解析边界明确失败——否则该 offering 会静默退出调度、缩略图无产出且无错误。
            // OCR/Enhance 这类独立服务不在 task 队列(host 直持 supervisor),允许 None。
            let declares_thumbnail = off.capabilities.contains(&Capability::Thumbnail);
            let has_worker_id = off
                .worker_id
                .as_deref()
                .map(|w| !w.trim().is_empty())
                .unwrap_or(false);
            if declares_thumbnail && !has_worker_id {
                return Err(CatalogError::ThumbnailWithoutWorkerId(off.plugin_id));
            }

            let builtin = match off.distribution.as_deref() {
                Some("builtin") => true,
                Some(other) => {
                    tracing::warn!(
                        "offering {} 声明未知 distribution 值 {other:?},按 package 处理",
                        off.plugin_id
                    );
                    false
                }
                None => false,
            };

            // 校验并归一化全部 format。
            let mut norm_formats = Vec::with_capacity(off.formats.len());
            for f in &off.formats {
                if !is_valid_format(f) {
                    return Err(CatalogError::InvalidFormat(f.clone()));
                }
                // 撞常见格式 → 整表拒绝（Part1 §1.2，问题5）。纵深防御：仅靠扫描 common-first
                // 不够——缩略图 router 直接 key 于 resolve_format(fmt)，若 catalog 误登记 jpg，
                // jpg 会被判 Exotic 而 gate 出主 generator，瘫痪常见格式缩略图。
                //
                // 例外（用户裁决 A，RAW 支持线）：`distribution:"builtin"` 的 offering **可**
                // 声明已在 builtin_formats 的扩展名——该格式仍走 builtin 识别/扫描/Stage-A
                // badge，offering 只是额外声明「本插件可提供该格式的 thumbnail 解码能力」，
                // 是刻意叠加而非误登记。非 builtin（package）offering 仍受常见格式冲突拒绝，
                // 装机插件模型（如 PSD）不变。
                if classify_media_type(f).is_some() && !builtin {
                    return Err(CatalogError::CommonFormatConflict(f.clone()));
                }
                norm_formats.push(f.clone());
            }

            let view = CatalogOffering {
                plugin_id: off.plugin_id,
                display_name: off.name,
                media_kind: off.media_kind,
                formats: norm_formats.clone(),
                capabilities: off.capabilities,
                license_tier: off.license_tier,
                platforms: off.platforms,
                min_host_version: off.min_host_version,
                sku: off.sku,
                store_url: off.store_url,
                builtin,
                worker_id: off.worker_id,
                uses_gpu: off.uses_gpu.unwrap_or(false),
            };

            for f in norm_formats {
                if by_format.insert(f.clone(), view.clone()).is_some() {
                    return Err(CatalogError::DuplicateFormat(f));
                }
            }
        }

        Ok(CatalogSnapshot {
            by_format,
            sequence: raw.sequence,
        })
    }

    /// 解析内置 Catalog（编译期嵌入）。内置数据应始终合法；解析失败即配置 bug。
    pub fn builtin() -> Result<Self, CatalogError> {
        Self::parse(BUILTIN_CATALOG_JSON)
    }

    /// 查某格式的 offering（键为小写扩展名）。
    pub fn resolve_format(&self, format: &str) -> Option<&CatalogOffering> {
        self.by_format.get(format)
    }

    /// 查某格式的媒体大类——`classify_scanned_file` 的 catalog 回退用。
    /// builtin offering(D-OCR-5)恒返回 None:它是能力插件而非文件格式,扩展名分类面
    /// 一律不可见——磁盘上真出现同名扩展杂散文件(如 `.ocr`)不得被判为媒体、进库。
    /// `resolve_format` 是独立方法,不受此过滤影响(availability 通路仍可查到 builtin offering)。
    pub fn media_kind(&self, format: &str) -> Option<MediaKind> {
        self.by_format
            .get(format)
            .filter(|o| !o.builtin)
            .map(|o| o.media_kind)
    }

    /// 某格式是否被声明提供 `cap` 能力。
    pub fn claims_capability(&self, format: &str, cap: Capability) -> bool {
        self.by_format
            .get(format)
            .map(|o| o.claims_capability(cap))
            .unwrap_or(false)
    }

    /// catalog 安全单调序号（R11：防回滚，合并取大）。
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// 遍历全部 offering（去重后的格式列表，供前端 list 命令）。
    pub fn iter_formats(&self) -> impl Iterator<Item = (&String, &CatalogOffering)> {
        self.by_format.iter()
    }
}

/// Catalog 存储：持可热替换的只读快照。
pub struct CatalogStore {
    snapshot: RwLock<Arc<CatalogSnapshot>>,
}

impl CatalogStore {
    /// 从内置 Catalog 构建。
    pub fn from_builtin() -> Result<Self, CatalogError> {
        Ok(CatalogStore {
            snapshot: RwLock::new(Arc::new(CatalogSnapshot::builtin()?)),
        })
    }

    /// 直接以给定快照构建（测试 / 远程合并结果注入）。
    pub fn with_snapshot(snap: CatalogSnapshot) -> Self {
        CatalogStore {
            snapshot: RwLock::new(Arc::new(snap)),
        }
    }

    /// 取当前快照的 Arc（热路径调用，廉价）。
    pub fn snapshot(&self) -> Arc<CatalogSnapshot> {
        self.snapshot.read().unwrap().clone()
    }

    /// 整体替换快照（远程刷新后调用；先完整校验再 replace，禁止半更新）。
    pub fn replace(&self, snap: Arc<CatalogSnapshot>) {
        *self.snapshot.write().unwrap() = snap;
    }
}

/// format 合规：仅 `[a-z0-9]`，长度 1..=16，无点。手写校验避免引入 regex 依赖。
fn is_valid_format(f: &str) -> bool {
    let len = f.len();
    (1..=16).contains(&len)
        && f.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// plugin_id 合规：`[a-z0-9-]`，长度 1..=64。安装目录名只用已验证 plugin_id（Part3 §6.4）。
fn is_valid_plugin_id(id: &str) -> bool {
    let len = id.len();
    (1..=64).contains(&len)
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_strings_align_with_protocol() {
        // 两端能力名单一事实源核对:catalog 枚举 serde/as_str 与协议常量一致(T10)。
        use exotic_protocol::capability as cap;
        assert_eq!(Capability::Thumbnail.as_str(), cap::THUMBNAIL);
        assert_eq!(Capability::Metadata.as_str(), cap::METADATA);
        assert_eq!(Capability::Embedding.as_str(), cap::EMBEDDING);
        assert_eq!(Capability::FaceDetectEmbed.as_str(), cap::FACE_DETECT_EMBED);
        assert_eq!(
            serde_json::to_string(&Capability::Embedding).unwrap(),
            r#""embedding""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::FaceDetectEmbed).unwrap(),
            r#""face_detect_embed""#
        );
    }

    #[test]
    fn builtin_parses_and_resolves_psd() {
        let snap = CatalogSnapshot::builtin().expect("内置 Catalog 必须合法");
        let off = snap.resolve_format("psd").expect("psd 必须在内置 Catalog");
        assert_eq!(off.plugin_id, "exotic-image-psd");
        assert_eq!(off.media_kind, MediaKind::Image);
        assert!(off.claims_capability(Capability::Thumbnail));
        assert!(!off.claims_capability(Capability::Text));
        assert_eq!(snap.media_kind("psd"), Some(MediaKind::Image));
        assert!(snap.claims_capability("psd", Capability::Thumbnail));
        assert!(snap.resolve_format("jpg").is_none());
    }

    #[test]
    fn builtin_distribution_flag_parses_and_defaults() {
        // distribution="builtin" → builtin==true(exotic-ocr 内置 offering,D-OCR-5)。
        let snap = CatalogSnapshot::builtin().expect("内置 Catalog 必须合法");
        let ocr = snap.resolve_format("ocr").expect("ocr 必须在内置 Catalog");
        assert!(ocr.builtin);
        // 缺省 distribution 字段 → builtin==false(PSD offering 未声明该字段)。
        let psd = snap.resolve_format("psd").expect("psd 必须在内置 Catalog");
        assert!(!psd.builtin);
    }

    #[test]
    fn media_kind_hides_builtin_offering() {
        // builtin(如 exotic-ocr)非文件格式:media_kind 一律 None(扩展名分类面不可见),
        // 但 resolve_format 仍能查到(availability 通路不受影响)。
        let snap = CatalogSnapshot::builtin().expect("内置 Catalog 必须合法");
        assert_eq!(
            snap.media_kind("ocr"),
            None,
            "builtin offering 不该被 media_kind 命中"
        );
        assert!(
            snap.resolve_format("ocr").is_some(),
            "resolve_format 是独立方法,不受 media_kind 过滤影响"
        );
    }

    #[test]
    fn reject_duplicate_format() {
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"a","name":"A","media_kind":"image","formats":["psd"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"},
          {"plugin_id":"b","name":"B","media_kind":"image","formats":["psd"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"}
        ]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::DuplicateFormat(_))
        ));
    }

    #[test]
    fn reject_invalid_format() {
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"a","name":"A","media_kind":"image","formats":["PSD"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"}
        ]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::InvalidFormat(_))
        ));
    }

    #[test]
    fn allow_builtin_offering_overlap_with_common_format() {
        // 用户裁决 A(RAW 支持线):distribution=="builtin" 的 offering 可声明已在
        // builtin_formats 的扩展名(如 cr2)——刻意叠加,不触发 CommonFormatConflict。
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"exotic-image-raw","name":"RAW","media_kind":"image","formats":["cr2"],
           "capabilities":["thumbnail"],"license_tier":"free","platforms":[],
           "min_host_version":"0.1.0","distribution":"builtin","worker_id":"raw-worker"}
        ]}"#;
        let snap = CatalogSnapshot::parse(json).expect("builtin 叠加 offering 应放行");
        let off = snap.resolve_format("cr2").expect("cr2 应可解析到 offering");
        assert_eq!(off.plugin_id, "exotic-image-raw");
        assert!(off.builtin);
    }

    #[test]
    fn non_builtin_offering_still_rejects_common_format() {
        // distribution 非 "builtin"(或缺省)的 offering 撞常见格式仍须整表拒绝——
        // 装机插件模型(如 PSD)不受本次豁免影响。
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"a","name":"A","media_kind":"image","formats":["cr2"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"}
        ]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::CommonFormatConflict(_))
        ));
    }

    #[test]
    fn builtin_catalog_parses_and_resolves_raw() {
        // 内置 Catalog(真实 resources/exotic-catalog.json)须在加入 RAW builtin
        // offering 后仍整体解析成功;resolve_format 对 10 个 RAW 扩展名均命中。
        let snap = CatalogSnapshot::builtin().expect("内置 Catalog 必须合法(含 RAW 叠加)");
        for ext in [
            "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
        ] {
            let off = snap
                .resolve_format(ext)
                .unwrap_or_else(|| panic!("{ext} 必须在内置 Catalog"));
            assert_eq!(off.plugin_id, "exotic-image-raw");
            assert!(off.builtin);
            assert!(off.claims_capability(Capability::Thumbnail));
        }
    }

    #[test]
    fn builtin_video_offering_claims_common_format_without_conflict() {
        // D-444③ A4:rmvb/vob 入 utils::format 表(classify=video)后成「常见格式」;video-extended
        // 是 builtin,认领 rmvb/vob 走用户裁决 A 的 builtin 豁免,不触发 CommonFormatConflict。
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"video-extended","name":"VIDEO","media_kind":"video","formats":["rmvb","vob"],
           "capabilities":["thumbnail"],"license_tier":"free","platforms":[],
           "min_host_version":"0.1.0","distribution":"builtin","worker_id":"video-worker"}
        ]}"#;
        let snap = CatalogSnapshot::parse(json).expect("builtin video 叠加 offering 应放行");
        for ext in ["rmvb", "vob"] {
            let off = snap
                .resolve_format(ext)
                .unwrap_or_else(|| panic!("{ext} 应可解析到 offering"));
            assert_eq!(off.plugin_id, "video-extended");
            assert!(off.builtin);
            assert!(off.claims_capability(Capability::Thumbnail));
        }
        // media_kind 对 builtin 恒隐藏(扫描器 catalog 回退面不可见);但 rmvb/vob 现由
        // 注册表直接分类为 video(classify_scanned_file 首段命中),故收录不依赖此回退。
        assert!(snap.media_kind("rmvb").is_none());
    }

    #[test]
    fn builtin_catalog_parses_and_resolves_video_extended() {
        // 内置 Catalog(真实 resources/exotic-catalog.json)在 rmvb/vob 入表后仍整体解析成功,
        // video-extended 认领 rmvb/vob 命中(builtin 豁免生效)。
        let snap = CatalogSnapshot::builtin().expect("内置 Catalog 必须合法(含 video 叠加)");
        for ext in ["rmvb", "vob"] {
            let off = snap
                .resolve_format(ext)
                .unwrap_or_else(|| panic!("{ext} 必须在内置 Catalog"));
            assert_eq!(off.plugin_id, "video-extended");
            assert!(off.builtin);
            assert!(off.claims_capability(Capability::Thumbnail));
        }
    }

    #[test]
    fn reject_override_common() {
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"a","name":"A","media_kind":"image","formats":["psd"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],
           "min_host_version":"0.1.0","override_common":true,"worker_id":"w"}
        ]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::OverrideCommonNotAllowed(_))
        ));
    }

    #[test]
    fn reject_common_format_offering() {
        // catalog 误登记常见格式（jpg/mp4）→ 整表拒绝（问题5）。
        for fmt in ["jpg", "mp4", "png"] {
            let json = format!(
                r#"{{"schema":1,"sequence":1,"offerings":[
                  {{"plugin_id":"a","name":"A","media_kind":"image","formats":["{fmt}"],
                   "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"}}
                ]}}"#
            );
            assert!(
                matches!(
                    CatalogSnapshot::parse(&json),
                    Err(CatalogError::CommonFormatConflict(_))
                ),
                "format {fmt} 撞 common 必须拒绝整表"
            );
        }
    }

    #[test]
    fn reject_unsupported_schema() {
        let json = r#"{"schema":2,"sequence":1,"offerings":[]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::UnsupportedSchema(2))
        ));
    }

    /// P17 边界锁:thumbnail 能力的 worker_id 必须在 **Catalog 解析**这一外部输入边界显式给出
    /// (缺失/空串 → 整表拒绝,不静默落任何默认映射);无 thumbnail 的独立服务(OCR/Enhance)允许 None。
    #[test]
    fn thumbnail_offering_requires_explicit_worker_id() {
        let bad_suffixes = [
            // 缺字段
            "\"license_tier\":\"paid\",\"platforms\":[],\"min_host_version\":\"0.1.0\"",
            // 空串
            "\"license_tier\":\"paid\",\"platforms\":[],\"min_host_version\":\"0.1.0\",\"worker_id\":\"\"",
            // 纯空白
            "\"license_tier\":\"paid\",\"platforms\":[],\"min_host_version\":\"0.1.0\",\"worker_id\":\"  \"",
        ];
        for suffix in bad_suffixes {
            let json = format!(
                r#"{{"schema":1,"sequence":1,"offerings":[
                  {{"plugin_id":"a","name":"A","media_kind":"image","formats":["psd"],
                   "capabilities":["thumbnail"],{suffix}}}
                ]}}"#
            );
            assert!(
                matches!(
                    CatalogSnapshot::parse(&json),
                    Err(CatalogError::ThumbnailWithoutWorkerId(_))
                ),
                "thumbnail offering 缺显式 worker_id 必须拒绝整表:{suffix}"
            );
        }

        // 无 thumbnail 能力 → 允许缺 worker_id(OCR/Enhance 由 host 直持 supervisor,不进 task 队列)。
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"exotic-ocr","name":"OCR","media_kind":"image","formats":["ocr"],
           "capabilities":["text"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0"}
        ]}"#;
        let off = CatalogSnapshot::parse(json)
            .expect("无 thumbnail 的 offering 不要求 worker_id")
            .resolve_format("ocr")
            .cloned()
            .expect("ocr 应可解析");
        assert!(off.worker_id.is_none());
    }

    #[test]
    fn reject_invalid_plugin_id() {
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"Bad_ID","name":"A","media_kind":"image","formats":["psd"],
           "capabilities":["thumbnail"],"license_tier":"paid","platforms":[],"min_host_version":"0.1.0","worker_id":"w"}
        ]}"#;
        assert!(matches!(
            CatalogSnapshot::parse(json),
            Err(CatalogError::InvalidPluginId(_))
        ));
    }
}
